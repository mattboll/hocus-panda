//! The system's authentication dialog (7.5, variant A, docs/design/polkit.md):
//! the shell is the session's polkit agent and draws the dialog, a layer
//! surface of its own ([`hocus_link::AUTH_NAMESPACE`]); the compositor
//! finds where it goes, dims every screen under it, moves it in and out,
//! and keeps everything else from the pointer and the keys while it shows.
//!
//! The shell says a dialog is coming (`auth open`) with the asking
//! process and its parents; the compositor finds that process's window
//! (as a bubble finds its application's), and answers `auth-at`: the
//! window's screen, else the one under the pointer, and the window's
//! application and title, for the dialog's details. Only a surface of
//! that namespace on that screen, once asked, is taken for the dialog: no
//! other client's surface gets the dim by naming itself so.
//!
//! From its first frame: the dialog fades and grows from 0.96 of its size
//! over 250 ms (out-cubic), the dim (`rgba(5,7,12,0.58)`) fades in over
//! every screen, the bar included, over 160 ms (`ease-out`); once the shell
//! says it closes (`auth closing`), both fade out over 150 ms (in-cubic)
//! and the shell removes the surface. With reduced motion, or a shell on
//! the CPU, they show and go at once.
//!
//! While it shows, the dialog alone has the keyboard; of the compositor's
//! shortcuts only the sound's and the brightness's keys work (the layout
//! switches with XKB's own keys, which go through); the virtual terminals'
//! keys too. A press anywhere but on the dialog does nothing, and nothing
//! under the dim sees the pointer, the wheel or a gesture. Neither the
//! dialog nor the dim is ever in a capture, a recording or a share.

use std::time::Duration;

use hocus_link::{
    AUTH_DIM, AUTH_DIM_ALPHA, AUTH_DIM_IN, AUTH_FROM, AUTH_IN, AUTH_OUT, AUTH_RADIUS, AuthAsk,
    AuthAt, ToShell,
};
use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{
            AsRenderElements, Id, Kind, solid::SolidColorRenderElement,
            surface::WaylandSurfaceRenderElement, utils::RescaleRenderElement,
        },
        utils::with_renderer_surface_state,
    },
    desktop::{LayerSurface, layer_map_for_output},
    output::Output,
    utils::{Logical, Physical, Point, Rectangle, Scale},
};

use crate::{
    State,
    decoration::{FrameIds, Outside, Rounded, ShadowStyle, shadow_elements},
    going::Shaders,
    render::{OutputElement, OutputElements},
    wobble::Bend,
};

/// The dialog's `0 24px 60px rgba(0,0,0,0.50)`.
const SHADOW: ShadowStyle = ShadowStyle {
    dy: 24.0,
    blur: 60.0,
    alpha: 0.5,
    radius: AUTH_RADIUS,
};

/// The dialog asked for: its request, and the screen it goes on.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Asked {
    id: u64,
    output: String,
}

/// The dialog shown, from its first frame.
#[derive(Debug)]
struct Shown {
    id: u64,
    layer: LayerSurface,
    opened: Duration,
    closing: Option<Duration>,
}

/// The dialog, as the compositor keeps it.
#[derive(Debug)]
pub struct Auth {
    asked: Option<Asked>,
    shown: Option<Shown>,
    /// Caps Lock's state as last told to the shell.
    caps: Option<bool>,
    /// The dialog's surface's commits, for tests.
    pub commits: u64,
    dim: Id,
    shadow: FrameIds,
}

impl Default for Auth {
    fn default() -> Self {
        Self {
            asked: None,
            shown: None,
            caps: None,
            commits: 0,
            dim: Id::new(),
            shadow: FrameIds::default(),
        }
    }
}

/// How the dialog and the dim are drawn at a moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// The dialog's opacity and its size (1: its own).
    pub alpha: f32,
    pub scale: f64,
    /// The dim's share of its opacity.
    pub dim: f32,
}

fn out_cubic(x: f64) -> f64 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

fn in_cubic(x: f64) -> f64 {
    x.clamp(0.0, 1.0).powi(3)
}

fn ease_out(x: f64) -> f64 {
    crate::motion::Bezier::EASE_OUT.at(x)
}

fn part(t: Duration, of: Duration) -> f64 {
    t.as_secs_f64() / of.as_secs_f64()
}

/// The dialog first drawn at `opened` (closing since `closing`), at `now`.
fn look(opened: Duration, closing: Option<Duration>, now: Duration, motion: bool) -> Look {
    if !motion {
        let on = if closing.is_some() { 0.0 } else { 1.0 };
        return Look {
            alpha: on,
            scale: 1.0,
            dim: on,
        };
    }
    let t = now.saturating_sub(opened);
    let p = out_cubic(part(t, AUTH_IN));
    let dim = ease_out(part(t, AUTH_DIM_IN));
    let left = match closing {
        Some(since) => 1.0 - in_cubic(part(now.saturating_sub(since), AUTH_OUT)),
        None => 1.0,
    };
    Look {
        alpha: (p * left) as f32,
        scale: AUTH_FROM + (1.0 - AUTH_FROM) * p,
        dim: (dim * left) as f32,
    }
}

impl Shown {
    fn moving(&self, now: Duration, motion: bool) -> bool {
        motion
            && match self.closing {
                Some(since) => now < since + AUTH_OUT,
                None => now < self.opened + AUTH_IN.max(AUTH_DIM_IN),
            }
    }
}

impl State {
    fn auth_motion(&self) -> bool {
        !self.reduced_motion() && !self.shell.software
    }

    /// What the shell says of its dialog.
    pub fn auth_asked(&mut self, ask: AuthAsk) {
        match ask {
            AuthAsk::Open { id, pids } => {
                let window = self.window_of_ancestry(&pids);
                let output = window
                    .and_then(|w| self.output_of(w))
                    .or_else(|| self.output_under_pointer());
                let Some(output) = output else {
                    tracing::warn!(id, "no screen for the authentication dialog");
                    return;
                };
                let Some(geometry) = self.space.output_geometry(&output) else {
                    return;
                };
                let (app, title) = window.map_or_else(Default::default, |w| {
                    let app = self
                        .windows
                        .get(&w)
                        .and_then(|m| crate::windows::app_id(&m.window))
                        .unwrap_or_default();
                    (app, crate::windows::title_of(self, w))
                });
                tracing::debug!(
                    id,
                    output = output.name(),
                    window = window.map(|w| w.0),
                    "the authentication dialog's place"
                );
                self.auth.asked = Some(Asked {
                    id,
                    output: output.name(),
                });
                let scale = (output.current_scale().fractional_scale() * 120.0).round() as u32;
                self.tell_shell(ToShell::AuthAt(AuthAt {
                    id,
                    output: output.name(),
                    scale,
                    height: geometry.size.h,
                    app,
                    title,
                }));
            }
            AuthAsk::Closing { id } => {
                let now = self.now();
                if let Some(shown) = self.auth.shown.as_mut().filter(|s| s.id == id) {
                    shown.closing.get_or_insert(now);
                    self.redraw = true;
                }
                if self.auth.asked.as_ref().is_some_and(|a| a.id == id) {
                    self.auth.asked = None;
                }
                // What the dialog kept from the pointer sees it again.
                self.auth_refresh_pointer();
            }
        }
    }

    /// A commit of a surface of the dialog's namespace on `output`: the
    /// dialog asked there, once drawn, shows from then.
    pub fn auth_committed(&mut self, layer: &LayerSurface, output: &Output) {
        self.auth.commits += 1;
        let Some(asked) = self.auth.asked.clone() else {
            return;
        };
        if asked.output != output.name()
            || self
                .auth
                .shown
                .as_ref()
                .is_some_and(|s| &s.layer == layer && s.id == asked.id)
        {
            return;
        }
        let drawn = with_renderer_surface_state(layer.wl_surface(), |s| s.buffer().is_some())
            .unwrap_or(false);
        if !drawn {
            return;
        }
        self.auth.shown = Some(Shown {
            id: asked.id,
            layer: layer.clone(),
            opened: self.now(),
            closing: None,
        });
        self.redraw = true;
        // The window under a still pointer no longer has it.
        self.auth_refresh_pointer();
    }

    /// A layer surface went: if it was the dialog's, it is gone.
    pub fn auth_gone(&mut self, layer: &LayerSurface) {
        if self.auth.shown.as_ref().is_some_and(|s| &s.layer == layer) {
            self.auth.shown = None;
            self.redraw = true;
        }
    }

    /// The shell went: so did its dialog.
    pub fn auth_shell_gone(&mut self) {
        self.auth.asked = None;
        self.auth.shown = None;
        self.auth.caps = None;
    }

    /// The pointer where it is, again: what is under it now has it.
    fn auth_refresh_pointer(&mut self) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let at = pointer.current_location();
        let time = self.now().as_millis() as u32;
        self.pointer_motion(at, time);
    }

    /// The dialog's surface while it shows and does not close.
    pub fn auth_layer(&self) -> Option<LayerSurface> {
        self.auth
            .shown
            .as_ref()
            .filter(|s| s.closing.is_none())
            .map(|s| s.layer.clone())
    }

    /// Whether the dialog shows and holds the keyboard and the pointer.
    pub fn auth_holds(&self) -> bool {
        self.auth_layer().is_some()
    }

    /// Whether the dialog keeps the pointer at `at` from what is under it:
    /// it shows, and `at` is not on it.
    pub fn auth_keeps_pointer(&self, at: Point<f64, Logical>) -> bool {
        let Some(layer) = self.auth_layer() else {
            return false;
        };
        let on = self.space.outputs().find_map(|output| {
            let origin = self.space.output_geometry(output)?.loc;
            let geometry = layer_map_for_output(output).layer_geometry(&layer)?;
            Some(Rectangle::new(origin + geometry.loc, geometry.size).to_f64())
        });
        !on.is_some_and(|rect| rect.contains(at))
    }

    /// Whether the dialog is still moving at `now`.
    pub fn auth_tick(&self, now: Duration) -> bool {
        let motion = self.auth_motion();
        self.auth
            .shown
            .as_ref()
            .is_some_and(|s| s.moving(now, motion))
    }

    /// The dialog's look now, if it shows (for tests).
    pub fn auth_look(&self) -> Option<Look> {
        let shown = self.auth.shown.as_ref()?;
        Some(look(
            shown.opened,
            shown.closing,
            self.now(),
            self.auth_motion(),
        ))
    }

    /// Caps Lock is `on`: the shell is told when that changed.
    pub fn auth_caps(&mut self, on: bool) {
        if self.auth.caps != Some(on) && self.shell_linked() {
            self.auth.caps = Some(on);
            self.tell_shell(ToShell::CapsLock(on));
        }
    }

    /// The shell connected: it is told Caps Lock's state again.
    pub fn auth_tell_caps(&mut self) {
        self.auth.caps = None;
        let on = self
            .seat
            .get_keyboard()
            .is_some_and(|k| k.modifier_state().caps_lock);
        self.auth_caps(on);
    }

    /// What the dialog shows on `output` at `now`, front first: the
    /// dialog and its shadow on its screen, then the dim, on every screen.
    pub(crate) fn auth_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        now: Duration,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some(shown) = &self.auth.shown else {
            return;
        };
        let Some(size) = self.space.output_geometry(output).map(|g| g.size) else {
            return;
        };
        let scale = output.current_scale().fractional_scale();
        let look = look(shown.opened, shown.closing, now, self.auth_motion());
        let map = layer_map_for_output(output);
        let geometry = map
            .layers()
            .find(|l| *l == &shown.layer)
            .and_then(|l| map.layer_geometry(l));
        drop(map);
        if let Some(geometry) = geometry.filter(|_| look.alpha > 0.0) {
            let loc: Point<i32, Physical> = geometry.loc.to_physical_precise_round(scale);
            let surfaces: Vec<WaylandSurfaceRenderElement<R>> =
                AsRenderElements::<R>::render_elements(
                    &shown.layer,
                    renderer,
                    loc,
                    Scale::from(scale),
                    look.alpha,
                );
            if self.shell.software {
                // The CPU's buffers have no alpha: cut to its corners.
                let clip = Rectangle::new(loc, geometry.size.to_physical_precise_round(scale));
                let radius = (AUTH_RADIUS * scale).round() as i32;
                out.extend(surfaces.into_iter().map(|e| {
                    OutputElements::Bar(Rounded::new(e, clip, radius, Outside::Cropped, scale))
                }));
            } else if look.scale < 1.0 {
                let middle = Point::<f64, Logical>::from((
                    f64::from(geometry.loc.x) + f64::from(geometry.size.w) / 2.0,
                    f64::from(geometry.loc.y) + f64::from(geometry.size.h) / 2.0,
                ))
                .to_physical_precise_round(scale);
                out.extend(surfaces.into_iter().map(|e| {
                    OutputElements::Scaled(RescaleRenderElement::from_element(
                        e, middle, look.scale,
                    ))
                }));
            } else {
                out.extend(surfaces.into_iter().map(OutputElements::Surface));
            }
            if !self.shell.software {
                out.extend(
                    shadow_elements(
                        renderer,
                        &self.textures,
                        &self.auth.shadow,
                        SHADOW,
                        geometry.to_f64(),
                        scale,
                        look.alpha,
                    )
                    .into_iter()
                    .map(OutputElements::Shadow),
                );
            }
        }
        if look.dim <= 0.0 {
            return;
        }
        let a = AUTH_DIM_ALPHA * look.dim;
        let [r, g, b] = AUTH_DIM.map(|c| f32::from(c) / 255.0);
        let whole =
            Rectangle::<f64, Logical>::from_size(size.to_f64()).to_physical_precise_round(scale);
        out.push(OutputElements::Solid(SolidColorRenderElement::new(
            self.auth.dim.clone(),
            whole,
            (a * 1000.0) as usize,
            [r * a, g * a, b * a, a],
            Kind::Unspecified,
        )));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(opened: u64, closing: Option<u64>, now: u64, motion: bool) -> Look {
        let ms = Duration::from_millis;
        look(ms(opened), closing.map(ms), ms(now), motion)
    }

    #[test]
    fn it_grows_in_with_the_dim_then_fades_out() {
        let start = at(1000, None, 1000, true);
        assert_eq!((start.alpha, start.scale, start.dim), (0.0, AUTH_FROM, 0.0));
        // Out-cubic: most of the way at half the time.
        let mid = at(1000, None, 1125, true);
        assert!(mid.alpha > 0.8 && mid.alpha < 1.0, "{mid:?}");
        // The dim is in at 160 ms, the dialog at 250.
        assert_eq!(at(1000, None, 1160, true).dim, 1.0);
        assert!(at(1000, None, 1160, true).alpha < 1.0);
        assert_eq!(
            at(1000, None, 1250, true),
            Look {
                alpha: 1.0,
                scale: 1.0,
                dim: 1.0
            }
        );
        // In-cubic out: little gone at first, gone at 150 ms.
        assert!(at(1000, Some(2000), 2030, true).alpha > 0.9);
        let gone = at(1000, Some(2000), 2150, true);
        assert_eq!((gone.alpha, gone.dim), (0.0, 0.0));
        // Without motion: at once, both ways.
        assert_eq!(at(1000, None, 1000, false).alpha, 1.0);
        assert_eq!(at(1000, None, 1000, false).dim, 1.0);
        assert_eq!(at(1000, Some(2000), 2000, false).dim, 0.0);
    }
}
