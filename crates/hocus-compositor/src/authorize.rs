//! The system's password dialog (#83): polkit's requests, one at a time,
//! drawn by the compositor over every window with the keyboard, so that no
//! client can cover it or draw one like it. While the session is locked,
//! requests wait for it to unlock.
//!
//! Only the session itself is polkit's agent; nested or headless, a test
//! gives a private system bus and a helper of its own.

use std::{collections::VecDeque, path::PathBuf};

use hocus_platform::polkit::{Agent, AgentEvent, Request};
use hocus_shell::authorize::{AuthChoice, Authorize, Check, Prompt};
use smithay::{
    reexports::calloop::{LoopHandle, channel},
    utils::{Logical, Point},
};

use crate::{App, State, shell::Panel};

/// One request on screen.
#[derive(Debug)]
pub struct Dialog {
    request: Request,
    output: String,
    panel: Panel<Authorize>,
}

#[derive(Debug, Default)]
pub struct Authorizer {
    agent: Option<Agent>,
    waiting: VecDeque<Request>,
    pub dialog: Option<Dialog>,
}

/// Registers the agent with polkit, when this is the session or a test
/// gave a bus.
pub fn start(handle: &LoopHandle<'static, App>, state: &mut State, helper: Option<PathBuf>) {
    if !state.is_session && state.system_bus.is_none() {
        return;
    }
    let (events, received) = channel::channel::<AgentEvent>();
    let inserted = handle.insert_source(received, |event, _, app| {
        if let channel::Event::Msg(event) = event {
            app.timed("polkit", |app| app.state.polkit_event(event));
        }
    });
    if let Err(err) = inserted {
        tracing::warn!(%err.error, "could not follow polkit");
        return;
    }
    match Agent::start(state.system_bus.clone(), helper, move |e| {
        let _ = events.send(e);
    }) {
        Ok(agent) => state.authorizer.agent = Some(agent),
        Err(err) => tracing::warn!(%err, "no polkit agent"),
    }
}

impl State {
    fn polkit_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::Begin(request) => {
                tracing::info!(action = request.action, "polkit asks for a password");
                self.authorizer.waiting.push_back(request);
            }
            AgentEvent::Cancelled { cookie } => {
                self.authorizer.waiting.retain(|r| r.cookie != cookie);
                if self
                    .authorizer
                    .dialog
                    .as_ref()
                    .is_some_and(|d| d.request.cookie == cookie)
                {
                    self.authorizer.dialog = None;
                }
            }
            AgentEvent::Checked { cookie, result } => {
                let Some(dialog) = self
                    .authorizer
                    .dialog
                    .as_mut()
                    .filter(|d| d.request.cookie == cookie)
                else {
                    return;
                };
                match result {
                    Ok(()) => {
                        if let Some(agent) = &self.authorizer.agent {
                            agent.done(&cookie);
                        }
                        self.authorizer.dialog = None;
                    }
                    Err(said) => {
                        tracing::info!(said, "polkit refused the password");
                        dialog.panel.view.set_check(Check::Refused(said));
                    }
                }
            }
        }
        self.show_next_request();
        self.needs_redraw = true;
    }

    /// The next request, once nothing else is asked and the session is
    /// not locked.
    fn show_next_request(&mut self) {
        if self.authorizer.dialog.is_some() || self.locked() {
            return;
        }
        let Some(request) = self.authorizer.waiting.pop_front() else {
            return;
        };
        let Some(output) = self
            .model
            .primary()
            .and_then(|o| self.find_output(&o.name.0))
            .or_else(|| self.output_under_pointer())
        else {
            self.authorizer.waiting.push_front(request);
            return;
        };
        let Some(g) = self.space.output_geometry(&output) else {
            return;
        };
        let view = Authorize::new(
            self.theme(),
            self.shell_lang(),
            hocus_ui::Size::new(g.size.w as f32, g.size.h as f32),
            output.current_scale().fractional_scale() as f32,
            &Prompt {
                message: request.message.clone(),
                command: request.command.clone(),
                user: request.user.clone(),
            },
        );
        self.authorizer.dialog = Some(Dialog {
            request,
            output: output.name(),
            panel: Panel::new(view),
        });
        self.needs_redraw = true;
    }

    pub fn auth_open(&self) -> bool {
        self.authorizer.dialog.is_some() && !self.locked()
    }

    fn auth_chose(&mut self, choice: Option<AuthChoice>) {
        let Some(dialog) = &self.authorizer.dialog else {
            return;
        };
        match choice {
            Some(AuthChoice::Submit(password)) => {
                if let Some(agent) = &self.authorizer.agent {
                    agent.check(&dialog.request, password);
                }
            }
            Some(AuthChoice::Cancel) => {
                if let Some(agent) = &self.authorizer.agent {
                    agent.cancel(&dialog.request.cookie);
                }
                self.authorizer.dialog = None;
                self.show_next_request();
            }
            None => {}
        }
        self.needs_redraw = true;
    }

    /// The dialog takes every key while it shows.
    pub fn auth_key(
        &mut self,
        sym: smithay::input::keyboard::Keysym,
        modifiers: &smithay::input::keyboard::ModifiersState,
    ) -> bool {
        let Some(dialog) = self.authorizer.dialog.as_mut() else {
            return false;
        };
        let choice = match crate::launcher::key_of(sym) {
            Some(key) => dialog.panel.view.key(key),
            None if !modifiers.ctrl && !modifiers.alt && !modifiers.logo => {
                if let Some(c) = sym.key_char().filter(|c| !c.is_control()) {
                    dialog.panel.view.text(c.encode_utf8(&mut [0; 4]));
                }
                None
            }
            None => None,
        };
        self.auth_chose(choice);
        self.needs_redraw = true;
        true
    }

    /// The dialog takes the pointer while it shows.
    pub fn auth_pointer(&mut self, at: Point<f64, Logical>, pressed: Option<bool>) -> bool {
        if !self.auth_open() {
            return false;
        }
        let Some(g) = self
            .authorizer
            .dialog
            .as_ref()
            .and_then(|d| self.find_output(&d.output))
            .and_then(|o| self.space.output_geometry(&o))
        else {
            return false;
        };
        let local = hocus_ui::Point::new(
            (at.x - f64::from(g.loc.x)) as f32,
            (at.y - f64::from(g.loc.y)) as f32,
        );
        let Some(dialog) = self.authorizer.dialog.as_mut() else {
            return false;
        };
        dialog.panel.view.pointer_move(local);
        let choice = pressed.and_then(|p| dialog.panel.view.pointer_button(p));
        self.auth_chose(choice);
        self.needs_redraw = true;
        true
    }

    /// After each dispatch: a request that waited for the unlock shows,
    /// and the dialog draws what changed.
    pub fn refresh_authorize(&mut self) {
        self.show_next_request();
        let now = self.clock.now();
        let Some(dialog) = self.authorizer.dialog.as_mut() else {
            return;
        };
        let drawn = dialog.panel.view.draw(now);
        if dialog.panel.upload(&drawn) {
            self.needs_redraw = true;
        }
    }

    pub fn auth_elements<R>(
        &self,
        renderer: &mut R,
        output: &smithay::output::Output,
    ) -> Option<smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement<R>>
    where
        R: smithay::backend::renderer::Renderer + smithay::backend::renderer::ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let dialog = self.authorizer.dialog.as_ref()?;
        if dialog.output != output.name() {
            return None;
        }
        dialog.panel.element(renderer, output, Point::default())
    }

    pub fn auth_ui(&self, out: &mut Vec<hocus_api::UiNode>) {
        let Some(dialog) = &self.authorizer.dialog else {
            return;
        };
        if let Some(g) = self
            .find_output(&dialog.output)
            .and_then(|o| self.space.output_geometry(&o))
        {
            crate::shell::flatten(&dialog.panel.view.access(), g.loc, 0, out);
        }
    }
}
