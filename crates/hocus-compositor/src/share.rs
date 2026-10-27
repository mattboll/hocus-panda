//! Choosing what to share (#116). xdg-desktop-portal-wlr streams the
//! screen through PipeWire on our capture protocols (#114) and asks a
//! chooser program what to share; `hocus share-pick` is that program: it
//! asks here, over the control API, and the shell shows the screens and
//! the windows as pictures, in its own look, over everything. The reply
//! waits for the choice; the loop does not.

use hocus_api::{ApiError, ConnectionId, Reply};
use hocus_shell::share::{ShareChoice, SharePicker, Shareable};
use smithay::{
    output::Output,
    utils::{Logical, Point},
};

use crate::{App, BackendState, State, capture::Pixels, shell::Panel};

/// A picture's width, physical pixels at most.
const PICTURE: u32 = 400;

#[derive(Debug)]
pub struct Picking {
    /// Who asked, to be answered.
    connection: ConnectionId,
    id: u64,
    output: String,
    panel: Panel<SharePicker>,
}

impl crate::shell::Pixels for SharePicker {
    fn pixels(&self) -> hocus_shell::column::View<'_> {
        self.view()
    }
}

impl App {
    /// The portal's chooser asks: the picker shows on the screen under the
    /// pointer, with a picture of each screen and each window.
    pub fn open_share_picker(&mut self, connection: ConnectionId, id: u64) {
        let Some(output) = self.state.output_under_pointer() else {
            self.state
                .api
                .reply(connection, id, Ok(Reply::Shared(None)));
            return;
        };
        // One question at a time: an older one is answered "nothing".
        self.state.share_answer(None);
        let outputs: Vec<Output> = self.state.space.outputs().cloned().collect();
        let mut screens = Vec::new();
        for o in &outputs {
            let pixels = self.picture_of_output(o);
            let size = o.current_mode().map(|m| m.size).unwrap_or_default();
            screens.push(Shareable {
                answer: format!("Monitor: {}", o.name()),
                name: o.name(),
                detail: format!("{}×{}", size.w, size.h),
                picture: pixels.map(|p| small(&p)),
            });
        }
        let mut windows = Vec::new();
        let managed: Vec<_> = self
            .state
            .space
            .elements()
            .rev()
            .filter_map(|w| {
                let id = crate::desktops::model_id(w)?;
                let m = self.state.windows.get(&id)?;
                Some((m.window.clone(), m.foreign.identifier()))
            })
            .collect();
        for (window, identifier) in managed {
            let (title, app_id) = crate::desktops::title_and_app_id(&window);
            let scale = self
                .state
                .output_of(&window)
                .map_or(1.0, |o| o.current_scale().fractional_scale());
            let pixels = match &mut self.backend {
                BackendState::Winit(b) => b.capture_window(&window, scale),
                BackendState::Headless(b) => b.capture_window(&window, scale),
                BackendState::Tty(b) => b.capture_window(&window, scale),
            }
            .ok();
            let name = title
                .clone()
                .filter(|t| !t.is_empty())
                .or_else(|| app_id.clone())
                .unwrap_or_default();
            windows.push(Shareable {
                answer: format!("Window: {identifier}"),
                name,
                detail: app_id.unwrap_or_default(),
                picture: pixels.map(|p| small(&p)),
            });
        }
        let state = &mut self.state;
        let g = state.space.output_geometry(&output).unwrap_or_default();
        let mut view = SharePicker::new(
            state.theme(),
            state.shell_lang(),
            hocus_ui::Size::new(g.size.w as f32, g.size.h as f32),
            output.current_scale().fractional_scale() as f32,
            screens,
            windows,
        );
        let drawn = view.draw(state.clock.now());
        let mut panel = Panel::new(view);
        panel.upload(&drawn);
        state.share = Some(Picking {
            connection,
            id,
            output: output.name(),
            panel,
        });
        state.needs_redraw = true;
    }

    fn picture_of_output(&mut self, output: &Output) -> Option<Pixels> {
        let state = &self.state;
        match &mut self.backend {
            BackendState::Winit(b) => b.capture_output(output, state, false),
            BackendState::Headless(b) => b.capture_output(output, state, false),
            BackendState::Tty(b) => b.capture_output(output, state, false),
        }
        .ok()
    }
}

/// `pixels` shrunk to `PICTURE` wide at most, by averaging: width, height,
/// RGBA.
fn small(pixels: &Pixels) -> (u16, u16, std::sync::Arc<[u8]>) {
    let step = pixels.width.div_ceil(PICTURE).max(1);
    let (w, h) = ((pixels.width / step).max(1), (pixels.height / step).max(1));
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            for dy in 0..step {
                for dx in 0..step {
                    let (sx, sy) = (x * step + dx, y * step + dy);
                    let i = ((sy * pixels.width + sx) * 4) as usize;
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += u32::from(pixels.rgba.get(i + c).copied().unwrap_or(0));
                    }
                }
            }
            let n = step * step;
            out.extend(sum.map(|s| (s / n) as u8));
        }
    }
    (w as u16, h as u16, out.into())
}

impl State {
    pub fn share_open(&self) -> bool {
        self.share.is_some()
    }

    /// Answers the chooser and closes the picker.
    fn share_answer(&mut self, answer: Option<String>) {
        if let Some(picking) = self.share.take() {
            tracing::info!(?answer, "chosen to share");
            let reply: Result<Reply, ApiError> = Ok(Reply::Shared(answer));
            self.api.reply(picking.connection, picking.id, reply);
            self.needs_redraw = true;
        }
    }

    fn share_chose(&mut self, choice: Option<ShareChoice>) {
        match choice {
            Some(ShareChoice::Share(answer)) => self.share_answer(Some(answer)),
            Some(ShareChoice::Cancel) => self.share_answer(None),
            None => {}
        }
    }

    pub fn share_pointer(&mut self, at: Point<f64, Logical>, pressed: Option<bool>) -> bool {
        let Some(picking) = &self.share else {
            return false;
        };
        let Some(g) = self
            .find_output(&picking.output)
            .and_then(|o| self.space.output_geometry(&o))
        else {
            return false;
        };
        let local = hocus_ui::Point::new(
            (at.x - f64::from(g.loc.x)) as f32,
            (at.y - f64::from(g.loc.y)) as f32,
        );
        let Some(picking) = self.share.as_mut() else {
            return false;
        };
        picking.panel.view.pointer_move(local);
        let choice = pressed.and_then(|p| picking.panel.view.pointer_button(p));
        self.share_chose(choice);
        self.needs_redraw = true;
        true
    }

    pub fn share_key(&mut self, sym: smithay::input::keyboard::Keysym) -> bool {
        let Some(picking) = self.share.as_mut() else {
            return false;
        };
        let choice = crate::launcher::key_of(sym).and_then(|k| picking.panel.view.key(k));
        self.share_chose(choice);
        self.needs_redraw = true;
        true
    }

    /// After each dispatch: the picker draws.
    pub fn refresh_share(&mut self) {
        let now = self.clock.now();
        if let Some(picking) = &mut self.share {
            let drawn = picking.panel.view.draw(now);
            if picking.panel.upload(&drawn) {
                self.needs_redraw = true;
            }
        }
    }

    pub fn share_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> Option<smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement<R>>
    where
        R: smithay::backend::renderer::Renderer + smithay::backend::renderer::ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let picking = self.share.as_ref()?;
        if picking.output != output.name() {
            return None;
        }
        picking.panel.element(renderer, output, Point::default())
    }

    /// Its panel on glass and its shadow, on the output it shows on.
    pub(crate) fn share_panes(&self, output: &smithay::output::Output) -> crate::scene::Panes {
        match self.share.as_ref() {
            Some(picking) if picking.output == output.name() => crate::scene::Panes::of(
                crate::blur::Surface::Share,
                Default::default(),
                &picking.panel.view.glass(),
                &picking.panel.view.shadows(),
            ),
            _ => crate::scene::Panes::default(),
        }
    }

    pub fn share_ui(&self, out: &mut Vec<hocus_api::UiNode>) {
        let Some(picking) = &self.share else {
            return;
        };
        if let Some(g) = self
            .find_output(&picking.output)
            .and_then(|o| self.space.output_geometry(&o))
        {
            crate::shell::flatten(&picking.panel.view.access(), g.loc, 0, out);
        }
    }
}
