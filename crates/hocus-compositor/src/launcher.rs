//! The launcher (5.1): the shell's full-screen layer surface, namespace
//! [`LAUNCHER_NAMESPACE`], over one screen while it is open.
//!
//! `Super+Space` tells the shell to open it on the screen under the
//! pointer, or to close it ([`ToShell::Launcher`]). The shell draws its
//! field and its grid and takes the keyboard; the compositor draws what is
//! under and around them, as it does for the bar and its popovers: the
//! desktop behind it blurred (with glass) and tinted in the bar's dark
//! blue ([`LAUNCHER_TINT`]), the bar too, and
//! its motion: it fades in over 160 ms from its first frame, its panel
//! growing from 0.97 of its size, and back out over 120 ms once the shell
//! says it closes (`closing`), the shell removing it after that. With
//! reduced motion it only fades. The shell draws it once for that motion.
//!
//! An application the shell asks for ([`ToCompositor::Launch`]) starts as
//! the compositor's other programs do, on a thread of its own, its first
//! window taking the focus on the current desktop.
//!
//! The toast a copied result leaves (5.2, [`LAUNCHER_TOAST_NAMESPACE`]) is
//! a small surface of the shell's at the bottom of the screen: from its
//! first frame, the compositor fades it in rising 8 px over 216 ms, and
//! out over the last 270 ms of its [`LAUNCHER_TOAST`], the mockup's
//! `toast-in`; with reduced motion it only fades. In between nothing
//! moves: a timer wakes the frame its fade out starts in.
//!
//! [`ToCompositor::Launch`]: hocus_link::ToCompositor::Launch

use std::{cell::OnceCell, time::Duration};

use hocus_link::{
    LAUNCHER_CLOSE, LAUNCHER_NAMESPACE, LAUNCHER_OPEN, LAUNCHER_SCALE, LAUNCHER_TINT,
    LAUNCHER_TOAST, LAUNCHER_TOAST_NAMESPACE, LAUNCHER_ZOOM, Launch, ToShell,
};
use smithay::{
    backend::renderer::{element::memory::MemoryRenderBuffer, utils::with_renderer_surface_state},
    desktop::{LayerSurface, layer_map_for_output},
    output::Output,
};

use crate::{
    State,
    motion::{Bezier, Tween},
    programs,
};

/// The mockup's `cubic-bezier(.2, .8, .2, 1)`, the panel growing in.
const GROW: Bezier = Bezier(0.2, 0.8, 0.2, 1.0);
/// The toast's `toast-in` over its 1.8 s: in over its first 12 %, rising
/// 8 px (× the launcher's zoom); out over its last 15 %, each `ease-out`.
const TOAST_IN: Duration = Duration::from_millis(216);
const TOAST_OUT: Duration = Duration::from_millis(270);
const TOAST_RISE: f64 = 8.0 * LAUNCHER_ZOOM;
/// Its corners, which the compositor cuts on the CPU.
pub const TOAST_RADIUS: f64 = 12.0 * LAUNCHER_ZOOM;
/// The tint's rows: drawn stretched over the screen, its gradient smooth.
pub const TINT_ROWS: usize = 64;

/// The launchers shown (one, but one closing may still fade out as
/// another opens), with their motion.
#[derive(Debug, Default)]
pub struct Launchers {
    shown: Vec<Shown>,
    /// What tints the screen behind it, made once and stretched over each
    /// screen: the same pixels from one frame to the next, for the damage.
    tint: OnceCell<MemoryRenderBuffer>,
    /// The toasts shown, from their first frame.
    toasts: Vec<(LayerSurface, Duration)>,
}

/// How the launcher's toast is drawn at a moment: how opaque, and how far
/// under its place (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToastLook {
    pub alpha: f32,
    pub drop: f64,
}

/// How a toast first drawn at `opened` is drawn at `now`; `motion`: it
/// rises as it comes, else it only fades.
fn toast_look(opened: Duration, now: Duration, motion: bool) -> ToastLook {
    let came = Tween::new(opened, TOAST_IN, Bezier::EASE_OUT).at(now);
    let gone = Tween::new(
        opened + LAUNCHER_TOAST - TOAST_OUT,
        TOAST_OUT,
        Bezier::EASE_OUT,
    )
    .at(now);
    ToastLook {
        alpha: came.min(1.0 - gone).clamp(0.0, 1.0) as f32,
        drop: if motion {
            TOAST_RISE * (1.0 - came.clamp(0.0, 1.0))
        } else {
            0.0
        },
    }
}

#[derive(Debug)]
struct Shown {
    layer: LayerSurface,
    /// Its first frame, on the compositor's clock.
    opened: Duration,
    /// Since when it closes.
    closing: Option<Duration>,
}

/// How a launcher is drawn at a moment: how opaque it and what is behind
/// it are, and how large its panel is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub alpha: f32,
    pub scale: f64,
}

/// How a launcher opened at `opened` (and closing since `closing`) is drawn
/// at `now`; `motion`: its panel grows and shrinks, else it only fades.
fn look(opened: Duration, closing: Option<Duration>, now: Duration, motion: bool) -> Look {
    let (alpha, grown) = match closing {
        None => (
            Tween::new(opened, LAUNCHER_OPEN, Bezier::EASE_OUT).at(now),
            Tween::new(opened, LAUNCHER_OPEN, GROW).at(now),
        ),
        Some(since) => {
            let gone = Tween::new(since, LAUNCHER_CLOSE, Bezier::EASE_IN).at(now);
            (1.0 - gone, 1.0 - gone)
        }
    };
    let scale = if motion {
        LAUNCHER_SCALE + (1.0 - LAUNCHER_SCALE) * grown.clamp(0.0, 1.0)
    } else {
        1.0
    };
    Look {
        alpha: alpha.clamp(0.0, 1.0) as f32,
        scale,
    }
}

impl Shown {
    fn moving(&self, now: Duration) -> bool {
        match self.closing {
            Some(since) => now < since + LAUNCHER_CLOSE,
            None => now < self.opened + LAUNCHER_OPEN,
        }
    }
}

impl Launchers {
    /// The launcher `layer`, if it shows, and how.
    pub fn look(&self, layer: &LayerSurface, now: Duration, motion: bool) -> Option<Look> {
        let shown = self.shown.iter().find(|s| &s.layer == layer)?;
        Some(look(shown.opened, shown.closing, now, motion))
    }

    /// Whether one, or a toast, is still moving at `now`.
    pub fn tick(&self, now: Duration) -> bool {
        let fading = |opened: Duration| {
            now < opened + TOAST_IN
                || (now >= opened + LAUNCHER_TOAST - TOAST_OUT && now < opened + LAUNCHER_TOAST)
        };
        self.shown.iter().any(|s| s.moving(now))
            || self.toasts.iter().any(|(_, opened)| fading(*opened))
    }

    /// Whether a launcher shows, and is done moving by `at`.
    pub fn at_rest(&self, at: Duration) -> bool {
        !self.shown.is_empty() && !self.shown.iter().any(|s| s.moving(at))
    }

    /// When the toast shown first was first drawn, on the motions' clock.
    pub fn toast_since(&self) -> Option<Duration> {
        self.toasts.first().map(|(_, opened)| *opened)
    }

    /// When a toast resting at `now` starts to fade out.
    pub fn next(&self, now: Duration) -> Option<Duration> {
        self.toasts
            .iter()
            .map(|(_, opened)| *opened + LAUNCHER_TOAST - TOAST_OUT)
            .filter(|at| *at > now)
            .min()
    }

    /// The toast `layer`, if it shows, and how.
    pub fn toast_look(
        &self,
        layer: &LayerSurface,
        now: Duration,
        motion: bool,
    ) -> Option<ToastLook> {
        let (_, opened) = self.toasts.iter().find(|(l, _)| l == layer)?;
        Some(toast_look(*opened, now, motion))
    }

    /// Whether one shows and is not closing.
    pub fn open(&self) -> bool {
        self.shown.iter().any(|s| s.closing.is_none())
    }

    /// What tints the screen behind it: a column of [`TINT_ROWS`] pixels,
    /// to be stretched over the screen.
    pub fn tint(&self) -> MemoryRenderBuffer {
        self.tint
            .get_or_init(|| crate::decoration::buffer(&tint_pixels(), 1, TINT_ROWS as i32))
            .clone()
    }
}

/// The tint's column, top to bottom: the bar's dark blue going from
/// [`LAUNCHER_TINT`]'s first colour to its second, premultiplied, as the
/// renderers blend, in `Argb8888`'s bytes (blue first).
fn tint_pixels() -> Vec<u8> {
    let [(top, top_alpha), (bottom, bottom_alpha)] = LAUNCHER_TINT;
    (0..TINT_ROWS)
        .flat_map(|row| {
            let t = (row as f32 + 0.5) / TINT_ROWS as f32;
            let mix = |a: f32, b: f32| a + (b - a) * t;
            let alpha = mix(top_alpha, bottom_alpha);
            let channel =
                |i: usize| (mix(f32::from(top[i]), f32::from(bottom[i])) * alpha).round() as u8;
            [
                channel(2),
                channel(1),
                channel(0),
                (alpha * 255.0).round() as u8,
            ]
        })
        .collect()
}

impl State {
    /// Whether the launcher's panel grows and shrinks: not with reduced
    /// motion, where it only fades.
    pub fn launcher_motion(&self) -> bool {
        !self.reduced_motion()
    }

    /// `Super+Space`: the shell opens the launcher on the screen under the
    /// pointer, or closes it.
    pub fn toggle_launcher(&mut self) {
        let Some(output) = self.output_under_pointer() else {
            return;
        };
        if !self.shell_linked() {
            tracing::info!("no shell: no launcher");
            return;
        }
        let scale = self.bar_scale(&output.name());
        self.tell_shell(ToShell::Launcher {
            output: output.name(),
            scale,
        });
    }

    /// A commit of the shell's toast `layer`: its first frame starts its
    /// motion.
    pub fn launcher_toast_committed(&mut self, layer: &LayerSurface) {
        if self.panel.launchers.toasts.iter().any(|(l, _)| l == layer) {
            return;
        }
        let drawn = with_renderer_surface_state(layer.wl_surface(), |s| s.buffer().is_some())
            .unwrap_or(false);
        if drawn {
            let opened = self.now();
            self.panel.launchers.toasts.push((layer.clone(), opened));
            self.redraw = true;
        }
    }

    /// The toasts' surfaces on `output`, if any shows.
    pub fn launcher_toasts_on(&self, output: &Output) -> Vec<LayerSurface> {
        layer_map_for_output(output)
            .layers()
            .filter(|l| l.namespace() == LAUNCHER_TOAST_NAMESPACE)
            .cloned()
            .collect()
    }

    /// A commit of the shell's launcher `layer`: its first frame starts its
    /// motion.
    pub fn launcher_committed(&mut self, layer: &LayerSurface) {
        let launchers = &mut self.panel.launchers;
        if launchers.shown.iter().any(|s| &s.layer == layer) {
            return;
        }
        let drawn = with_renderer_surface_state(layer.wl_surface(), |s| s.buffer().is_some())
            .unwrap_or(false);
        if !drawn {
            return;
        }
        let opened = self.now();
        self.panel.launchers.shown.push(Shown {
            layer: layer.clone(),
            opened,
            closing: None,
        });
        self.redraw = true;
    }

    /// The shell says its launcher closes: it fades out.
    pub fn launcher_closing(&mut self, namespace: &str) {
        if namespace != LAUNCHER_NAMESPACE {
            return;
        }
        let now = self.now();
        for shown in &mut self.panel.launchers.shown {
            shown.closing.get_or_insert(now);
        }
        self.redraw = true;
    }

    /// A layer surface went: if it was a launcher, its motion goes too.
    pub fn launcher_gone(&mut self, layer: &LayerSurface) {
        self.panel.launchers.shown.retain(|s| &s.layer != layer);
        self.panel.launchers.toasts.retain(|(l, _)| l != layer);
    }

    /// The launcher's surface on `output`, open or closing, if one shows.
    pub fn launcher_on(&self, output: &Output) -> Option<LayerSurface> {
        layer_map_for_output(output)
            .layers()
            .find(|l| l.namespace() == LAUNCHER_NAMESPACE)
            .cloned()
    }

    /// The shell asks for an application: started off the loop, its
    /// first window taking the focus.
    pub fn launch(&mut self, launch: Launch) {
        tracing::info!(
            app = launch.id,
            terminal = launch.terminal,
            "the launcher starts"
        );
        self.spawned_at = Some(self.now());
        let env = programs::Env::of(self).with_activation(self.activation_token());
        let terminal = launch
            .terminal
            .then(|| self.config.programs.terminal.clone());
        programs::launch(launch, terminal, env);
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
    fn it_fades_and_grows_in_then_fades_and_shrinks_out() {
        // At its first frame: transparent, at 0.97.
        let start = at(1000, None, 1000, true);
        assert_eq!(start.alpha, 0.0);
        assert!((start.scale - LAUNCHER_SCALE).abs() < 1e-9);
        // Halfway, mostly there; at 160 ms, whole.
        let mid = at(1000, None, 1080, true);
        assert!(mid.alpha > 0.6 && mid.alpha < 1.0, "{mid:?}");
        assert!(mid.scale > 0.99 && mid.scale < 1.0, "{mid:?}");
        assert_eq!(
            at(1000, None, 1160, true),
            Look {
                alpha: 1.0,
                scale: 1.0
            }
        );
        // Closing, ease-in: little gone at first, gone at 120 ms.
        let early = at(1000, Some(2000), 2030, true);
        assert!(early.alpha > 0.8 && early.scale > 0.99, "{early:?}");
        let gone = at(1000, Some(2000), 2120, true);
        assert_eq!(gone.alpha, 0.0);
        assert!((gone.scale - LAUNCHER_SCALE).abs() < 1e-9);
    }

    #[test]
    fn the_toast_rises_in_rests_and_fades_out() {
        let ms = Duration::from_millis;
        let start = toast_look(ms(1000), ms(1000), true);
        assert_eq!(start.alpha, 0.0);
        assert!((start.drop - TOAST_RISE).abs() < 1e-9);
        assert_eq!(
            toast_look(ms(1000), ms(1500), true),
            ToastLook {
                alpha: 1.0,
                drop: 0.0
            }
        );
        let out = toast_look(ms(1000), ms(2700), true);
        assert!(out.alpha > 0.0 && out.alpha < 1.0, "{out:?}");
        assert_eq!(toast_look(ms(1000), ms(2800), true).alpha, 0.0);
        assert_eq!(toast_look(ms(1000), ms(1100), false).drop, 0.0);
    }

    #[test]
    fn the_tint_is_the_bars_dark_blue_about_85_percent_opaque() {
        let pixels = tint_pixels();
        assert_eq!(pixels.len(), TINT_ROWS * 4);
        let row = |i: usize| -> [u8; 4] {
            let p = &pixels[i * 4..i * 4 + 4];
            [p[0], p[1], p[2], p[3]]
        };
        // Blue, green, red, alpha; premultiplied. The top nearly
        // rgba(22,27,42,0.82), the bottom rgba(14,17,28,0.88).
        let [b, g, r, a] = row(0);
        assert_eq!(a, 209);
        assert_eq!([r, g, b], [18, 22, 34]);
        let [b, g, r, a] = row(TINT_ROWS - 1);
        assert_eq!(a, 224);
        assert_eq!([r, g, b], [12, 15, 25]);
        // About 85 % halfway.
        let [.., a] = row(TINT_ROWS / 2);
        assert!((215..=219).contains(&a), "{a}");
    }

    #[test]
    fn with_reduced_motion_it_only_fades() {
        let mid = at(1000, None, 1080, false);
        assert!(mid.alpha > 0.6 && mid.alpha < 1.0, "{mid:?}");
        assert_eq!(mid.scale, 1.0);
        let out = at(1000, Some(2000), 2060, false);
        assert!(out.alpha > 0.0 && out.alpha < 1.0, "{out:?}");
        assert_eq!(out.scale, 1.0);
    }
}
