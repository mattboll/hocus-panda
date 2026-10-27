//! The radial menu at the pointer (#40): the configured button with its
//! keys held (Super and the right button by default; Super and the left
//! one draw on the screen, #86) over a window opens that window's actions
//! around the pointer, kept whole on the screen the pointer is on. Held, a
//! flick or a slow move picks one on release; let go without moving, it
//! stays for a click or the keys. No client sees its buttons or the pointer
//! over it. The actions are the compositor's own: maximizing, pinning,
//! sending, minimizing, keeping on top, snapping, closing.

use hocus_core::{WindowId, config::RadialButton};
use hocus_shell::radial::{self, Act, Gesture, Outcome, Radial, RadialEvent, Subject};
use smithay::{
    backend::renderer::{ImportAll, ImportMem, Renderer, Texture},
    input::keyboard::Keysym,
    output::Output,
    utils::{Logical, Point, SERIAL_COUNTER, Serial},
};

use crate::{State, desktops::model_id, grabs::BTN_RIGHT, shell::Panel, window_mode::WindowMode};

/// `BTN_MIDDLE` from linux/input-event-codes.h.
const BTN_MIDDLE: u32 = 0x112;

impl crate::shell::Pixels for Radial {
    fn pixels(&self) -> hocus_shell::column::View<'_> {
        self.view()
    }
}

/// The menu on a screen.
#[derive(Debug)]
struct Shown {
    output: String,
    /// The panel's corner, from the output's, logical.
    place: Point<i32, Logical>,
    panel: Panel<Radial>,
}

#[derive(Debug)]
struct Open {
    window: WindowId,
    shown: Shown,
    /// While the button that opened it is held: which, and its way.
    held: Option<(u32, Gesture)>,
}

#[derive(Debug, Default)]
pub struct Radials {
    open: Option<Open>,
    /// Folding away.
    closing: Option<Shown>,
    /// A button whose press it took: its release is not a client's.
    swallow: Option<u32>,
}

impl State {
    /// The menu is open: it takes the keys and the pointer.
    pub fn radial_open(&self) -> bool {
        self.radials.open.is_some()
    }

    /// A button. With the keys held over a window, the configured one
    /// opens the menu; while it is open, every button is its own.
    pub fn radial_button(&mut self, at: Point<f64, Logical>, button: u32, pressed: bool) -> bool {
        let Some(open) = self.radials.open.as_ref() else {
            if !pressed && self.radials.swallow == Some(button) {
                self.radials.swallow = None;
                return true;
            }
            return pressed && self.radial_starts(at, button);
        };
        let now = self.clock.now();
        let hover = self.radial_local(at);
        match &open.held {
            Some((held, _)) if *held == button => {
                if pressed {
                    return true;
                }
                let Some(open) = self.radials.open.as_mut() else {
                    return true;
                };
                let Some((_, gesture)) = open.held.take() else {
                    return true;
                };
                let outcome = open.shown.panel.view.let_go(&gesture, hover, now);
                tracing::debug!(?outcome, "radial menu let go");
                match outcome {
                    Outcome::Open => {}
                    Outcome::Event(event) => self.radial_event(event),
                }
            }
            // Another button while one is held: its own, nothing else.
            Some(_) => {}
            None => {
                // Its release is the menu's too, whatever the press did.
                self.radials.swallow = pressed.then_some(button);
                let event = match (hover, self.radial_on_output(at)) {
                    (Some(p), true) => self
                        .radials
                        .open
                        .as_mut()
                        .and_then(|o| o.shown.panel.view.pointer(p, Some(pressed), now)),
                    // Another screen: a press there closes it.
                    _ => pressed.then_some(RadialEvent::Close),
                };
                if let Some(event) = event {
                    self.radial_event(event);
                }
            }
        }
        self.needs_redraw = true;
        true
    }

    /// The menu opens at `at` when the keys are held with the button, over
    /// a window and nothing of the shell's.
    fn radial_starts(&mut self, at: Point<f64, Logical>, button: u32) -> bool {
        let Some((held, which)) = self.config.radial.held() else {
            return false;
        };
        let wanted = match which {
            RadialButton::Right => BTN_RIGHT,
            RadialButton::Middle => BTN_MIDDLE,
        };
        if button != wanted {
            return false;
        }
        let Some(m) = self.seat.get_keyboard().map(|k| k.modifier_state()) else {
            return false;
        };
        if (m.logo, m.shift, m.ctrl, m.alt) != (held.logo, held.shift, held.ctrl, held.alt) {
            return false;
        }
        if self.locked() || self.modal_open() {
            return false;
        }
        let Some(window) = self.radial_window_at(at) else {
            return false;
        };
        let Some(output) = self.output_at(at) else {
            return false;
        };
        let Some(area) = self.space.output_geometry(&output) else {
            return false;
        };
        let subject = self.radial_subject(window);
        let now = self.clock.now();
        let lang = self.shell_lang();
        let moves = !self.instant_motion && !self.config.appearance.reduced_motion;
        let theme = self.theme().with_reduced_motion(!moves);
        let scale = output.current_scale().fractional_scale() as f32;
        let view = Radial::new(theme, lang, subject, scale, now);
        let local = at - area.loc.to_f64();
        let corner = radial::place(
            hocus_ui::Point::new(local.x as f32, local.y as f32),
            hocus_ui::Size::new(area.size.w as f32, area.size.h as f32),
        );
        let place = Point::from((corner.x.round() as i32, corner.y.round() as i32));
        let mut panel = Panel::new(view);
        panel.upload(&[]);
        tracing::info!(
            window = window.0,
            output = output.name(),
            ?place,
            "radial menu"
        );
        self.radials.closing = None;
        self.radials.open = Some(Open {
            window,
            shown: Shown {
                output: output.name(),
                place,
                panel,
            },
            held: Some((button, Gesture::new(point(at), now))),
        });
        self.needs_redraw = true;
        true
    }

    /// The window under `at`, unless a bar, an overlay or the shell is
    /// over it there.
    fn radial_window_at(&self, at: Point<f64, Logical>) -> Option<WindowId> {
        let fullscreen = self
            .space
            .output_under(at)
            .next()
            .is_some_and(|o| self.fullscreen_on(o));
        let above = &crate::handlers::layer_shell::ABOVE;
        let above = if fullscreen { &above[..1] } else { &above[..] };
        if self.layer_surface_under(at, above).is_some() {
            return None;
        }
        let (window, _) = self.space.element_under(at)?;
        let id = model_id(window)?;
        let over_shell = self
            .shell_ui()
            .iter()
            .any(|n| n.depth == 0 && n.rect.contains(at.x, at.y));
        (!over_shell).then_some(id)
    }

    /// What the menu says of the window.
    fn radial_subject(&self, id: WindowId) -> Subject {
        let lang = self.shell_lang();
        let host = self.model.output_showing(id);
        let desktops = host
            .map(|o| {
                self.desktop_names(&o.name)
                    .iter()
                    .enumerate()
                    .map(|(i, n)| lang.desktop_label(n, i + 1))
                    .collect()
            })
            .unwrap_or_default();
        let desktop = host.and_then(|o| {
            let on = self.model.desktop_of(id)?;
            o.desktops.iter().position(|d| *d == on)
        });
        let managed = self.windows.get(&id);
        Subject {
            maximized: managed.is_some_and(|m| matches!(m.mode, WindowMode::Maximized { .. })),
            pinned: self.model.is_pinned(id),
            on_top: managed.is_some_and(|m| m.on_top),
            other_screen: self.space.outputs().count() > 1,
            desktops,
            desktop,
        }
    }

    /// `at` from the panel's corner, while it is open.
    fn radial_local(&self, at: Point<f64, Logical>) -> Option<hocus_ui::Point> {
        let shown = &self.radials.open.as_ref()?.shown;
        let area = self
            .find_output(&shown.output)
            .and_then(|o| self.space.output_geometry(&o))?;
        let corner = (area.loc + shown.place).to_f64();
        Some(hocus_ui::Point::new(
            (at.x - corner.x) as f32,
            (at.y - corner.y) as f32,
        ))
    }

    fn radial_on_output(&self, at: Point<f64, Logical>) -> bool {
        self.radials
            .open
            .as_ref()
            .and_then(|o| self.find_output(&o.shown.output))
            .and_then(|o| self.space.output_geometry(&o))
            .is_some_and(|g| g.to_f64().contains(at))
    }

    /// The pointer moved: held, the sector it points at lights up; open,
    /// the action under it. No window sees it meanwhile.
    pub fn radial_pointer_motion(&mut self, at: Point<f64, Logical>) -> bool {
        if self.radials.open.is_none() {
            return false;
        }
        let now = self.clock.now();
        let hover = self.radial_local(at);
        let on_output = self.radial_on_output(at);
        let Some(open) = self.radials.open.as_mut() else {
            return false;
        };
        match open.held.as_mut() {
            Some((_, gesture)) => {
                gesture.moved(point(at), now);
                let gesture = gesture.clone();
                open.shown
                    .panel
                    .view
                    .drag(&gesture, hover.filter(|_| on_output), now);
            }
            None => {
                if let Some(p) = hover.filter(|_| on_output) {
                    open.shown.panel.view.pointer(p, None, now);
                }
            }
        }
        self.needs_redraw = true;
        true
    }

    /// A key while it is open: the arrows, Enter and Escape are its own.
    pub fn radial_key(&mut self, sym: Keysym) {
        let now = self.clock.now();
        let Some(key) = crate::launcher::key_of(sym) else {
            return;
        };
        let event = self
            .radials
            .open
            .as_mut()
            .and_then(|o| o.shown.panel.view.key(key, now));
        if let Some(event) = event {
            self.radial_event(event);
        }
        self.needs_redraw = true;
    }

    fn radial_event(&mut self, event: RadialEvent) {
        let window = self.radials.open.as_ref().map(|o| o.window);
        self.close_radial();
        if let (RadialEvent::Act(act), Some(id)) = (event, window) {
            tracing::info!(?act, window = id.0, "radial menu");
            self.radial_act(id, act, SERIAL_COUNTER.next_serial());
        }
    }

    /// Does `act` to window `id`, as the shortcuts and the API do.
    fn radial_act(&mut self, id: WindowId, act: Act, serial: Serial) {
        if !self.windows.contains_key(&id) {
            return;
        }
        match act {
            Act::ToggleMaximized => self.toggle_maximized(id, serial),
            Act::TogglePinned => {
                let on = !self.model.is_pinned(id);
                if let Err(err) = self.model.set_pinned(id, on) {
                    tracing::warn!(%err, "could not pin the window");
                }
                self.sync_desktops();
            }
            Act::NextScreen => {
                if let Some(output) = self.next_output(id)
                    && let Err(err) = self.send_window(id, &output, None, serial)
                {
                    tracing::warn!(%err, "could not send the window to the next screen");
                }
            }
            Act::ToDesktop(n) => {
                let host = self
                    .model
                    .output_showing(id)
                    .and_then(|o| self.find_output(&o.name.0));
                if let Some(output) = host
                    && let Err(err) = self.send_window(id, &output, Some(n), serial)
                {
                    tracing::warn!(%err, "could not send the window to a desktop");
                }
            }
            Act::Minimize => self.minimize(id, serial),
            Act::ToggleOnTop => {
                if let Some(managed) = self.windows.get_mut(&id) {
                    managed.on_top = !managed.on_top;
                }
                self.sync_desktops();
            }
            Act::Snap(zone) => self.snap(id, zone, serial),
            Act::Close => {
                if let Some(managed) = self.windows.get(&id) {
                    crate::surface::close(&managed.window);
                }
            }
        }
        self.needs_redraw = true;
    }

    /// The screen after the one showing window `id`, left to right then
    /// top to bottom, round.
    fn next_output(&self, id: WindowId) -> Option<Output> {
        let host = self.model.output_showing(id)?.name.0.clone();
        let mut outputs: Vec<(Output, Point<i32, Logical>)> = self
            .space
            .outputs()
            .filter_map(|o| Some((o.clone(), self.space.output_geometry(o)?.loc)))
            .collect();
        outputs.sort_by_key(|(_, at)| (at.x, at.y));
        let here = outputs.iter().position(|(o, _)| o.name() == host)?;
        let next = outputs.get((here + 1) % outputs.len())?;
        (next.0.name() != host).then(|| next.0.clone())
    }

    /// The menu folds away.
    pub fn close_radial(&mut self) {
        if let Some(mut open) = self.radials.open.take() {
            // Closed while its button is held (Escape, a shortcut): the
            // release is still its own.
            if let Some((button, _)) = open.held.take() {
                self.radials.swallow = Some(button);
            }
            open.shown.panel.view.close();
            self.radials.closing = Some(open.shown);
            self.needs_redraw = true;
        }
    }

    /// After each dispatch: gone with its window, its screen or the lock;
    /// otherwise it draws what changed.
    pub fn refresh_radial(&mut self) {
        let gone = self.radials.open.as_ref().is_some_and(|o| {
            !self.windows.contains_key(&o.window) || self.find_output(&o.shown.output).is_none()
        });
        if gone || (self.radials.open.is_some() && self.locked()) {
            self.close_radial();
        }
        let now = self.clock.now();
        if let Some(open) = self.radials.open.as_mut() {
            let drawn = open.shown.panel.view.draw(now);
            if open.shown.panel.upload(&drawn) {
                self.needs_redraw = true;
            }
        }
        if let Some(shown) = self.radials.closing.as_mut() {
            if shown.panel.view.is_animating(now) {
                let drawn = shown.panel.view.draw(now);
                shown.panel.upload(&drawn);
                self.needs_redraw = true;
            } else {
                self.radials.closing = None;
                self.needs_redraw = true;
            }
        }
    }

    pub fn radial_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> Vec<crate::scene::SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        if self.locked() {
            return Vec::new();
        }
        let open = self.radials.open.as_ref().map(|o| &o.shown);
        let mut out = Vec::new();
        for shown in open
            .into_iter()
            .chain(self.radials.closing.as_ref())
            .filter(|s| s.output == output.name())
        {
            let Some(menu) = shown.panel.element(renderer, output, shown.place) else {
                continue;
            };
            out.push(crate::scene::SceneElement::Frame(menu));
            // Each action's shadow under it (#281).
            let shadows =
                crate::decoration::casters(shown.place.to_f64(), &shown.panel.view.shadows());
            out.extend(self.cast_elements(renderer, output, &shadows));
        }
        out
    }

    pub fn radial_ui(&self, out: &mut Vec<hocus_api::UiNode>) {
        let Some(shown) = self.radials.open.as_ref().map(|o| &o.shown) else {
            return;
        };
        if let Some(area) = self
            .find_output(&shown.output)
            .and_then(|o| self.space.output_geometry(&o))
        {
            crate::shell::flatten(&shown.panel.view.access(), area.loc + shown.place, 0, out);
        }
    }
}

fn point(at: Point<f64, Logical>) -> hocus_ui::Point {
    hocus_ui::Point::new(at.x as f32, at.y as f32)
}
