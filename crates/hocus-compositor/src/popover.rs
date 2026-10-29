//! The shell's popovers beside the bar (docs/design/bar.md §9): layer
//! surfaces of the shell's, namespace [`POPOVER_NAMESPACE`], one at a time.
//!
//! The shell draws what is in them; the compositor draws what goes around
//! and under them, as it does for the bar: the glass (the blur of what is
//! behind, in a pane of its own), the shadow, and their motion. A popover
//! fades in and slides 8 px away from the bar over 160 ms from its first
//! frame, and back out over 120 ms once the shell says it closes
//! (`closing`), the shell removing it after that; with reduced motion, or a
//! shell on the CPU, it shows and goes at once. The shell commits it once
//! for that motion: the compositor moves it.
//!
//! A press on none of the shell's surfaces while one shows, or while a
//! surface of the shell's holds the keyboard (the item a popover came from,
//! after `Esc`), is told to the shell (`outside`), which closes it or lets
//! the keyboard go. A popover that holds the keyboard is a menu, opened by
//! a click: that press goes no further, as a press outside a window's menu
//! does not. One opened by hovering does not hold the keyboard: the press
//! goes on to what is under it. The shell sees the presses on its own
//! surfaces itself, which keeps a second click on the item a toggle.

use std::time::Duration;

use hocus_link::{POPOVER_CLOSE, POPOVER_NAMESPACE, POPOVER_OPEN, POPOVER_SHIFT, ToShell};
use smithay::{
    backend::renderer::utils::with_renderer_surface_state,
    desktop::{LayerSurface, layer_map_for_output},
    reexports::wayland_server::Resource,
    utils::{Logical, Point},
    wayland::shell::wlr_layer::KeyboardInteractivity,
};

use crate::{
    State,
    decoration::{FrameIds, ShadowStyle},
    focus::KeyboardFocus,
    motion::{Bezier, Tween},
};

/// The mockup's `box-shadow: 0 14px 40px rgba(0, 0, 0, 0.38)`.
pub const SHADOW: ShadowStyle = ShadowStyle {
    dy: 14.0,
    blur: 40.0,
    alpha: 0.38,
    radius: hocus_link::RADIUS,
};

/// The popovers shown, with their motion.
#[derive(Debug, Default)]
pub struct Popovers {
    shown: Vec<Shown>,
}

#[derive(Debug)]
pub struct Shown {
    pub layer: LayerSurface,
    /// Its first frame, on the compositor's clock.
    opened: Duration,
    /// Since when it closes.
    closing: Option<Duration>,
    /// Its shadow's pieces.
    pub ids: FrameIds,
}

/// How a popover is drawn at a moment: how opaque, and how far toward the
/// bar from where it is (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub alpha: f32,
    pub shift: f64,
}

/// How a popover opened at `opened` (and closing since `closing`) is drawn
/// at `now`.
fn look(opened: Duration, closing: Option<Duration>, now: Duration, motion: bool) -> Look {
    // Shown: 0 to 1 as it opens, back to 0 as it closes.
    let shown = match (closing, motion) {
        (Some(_), false) => 0.0,
        (None, false) => 1.0,
        (None, true) => Tween::new(opened, POPOVER_OPEN, Bezier::EASE_OUT).at(now),
        (Some(since), true) => 1.0 - Tween::new(since, POPOVER_CLOSE, Bezier::EASE_IN).at(now),
    };
    let shown = shown.clamp(0.0, 1.0);
    Look {
        alpha: shown as f32,
        shift: POPOVER_SHIFT * (1.0 - shown),
    }
}

impl Shown {
    fn moving(&self, now: Duration) -> bool {
        match self.closing {
            Some(since) => now < since + POPOVER_CLOSE,
            None => now < self.opened + POPOVER_OPEN,
        }
    }
}

impl Popovers {
    /// The popover `layer`, if it shows, and how.
    pub fn look(&self, layer: &LayerSurface, now: Duration, motion: bool) -> Option<Look> {
        let shown = self.shown(layer)?;
        Some(look(shown.opened, shown.closing, now, motion))
    }

    pub fn shown(&self, layer: &LayerSurface) -> Option<&Shown> {
        self.shown.iter().find(|s| &s.layer == layer)
    }

    /// Whether one is still moving at `now`.
    pub fn tick(&self, now: Duration, motion: bool) -> bool {
        motion && self.shown.iter().any(|s| s.moving(now))
    }

    /// The popovers showing and not closing.
    fn open(&self) -> impl Iterator<Item = &Shown> {
        self.shown.iter().filter(|s| s.closing.is_none())
    }
}

impl State {
    /// Whether popovers move in and out.
    pub fn popover_motion(&self) -> bool {
        !self.reduced_motion() && !self.shell.software
    }

    /// A commit of the shell's popover `layer`: its first frame starts its
    /// motion.
    pub fn popover_committed(&mut self, layer: &LayerSurface) {
        if self.panel.popovers.shown(layer).is_some() {
            return;
        }
        let drawn = with_renderer_surface_state(layer.wl_surface(), |s| s.buffer().is_some())
            .unwrap_or(false);
        if !drawn {
            return;
        }
        let opened = self.now();
        self.panel.popovers.shown.push(Shown {
            layer: layer.clone(),
            opened,
            closing: None,
            ids: FrameIds::default(),
        });
        self.redraw = true;
    }

    /// The shell says its popovers of `namespace` close.
    pub fn popovers_closing(&mut self, namespace: &str) {
        if namespace != POPOVER_NAMESPACE {
            return;
        }
        let now = self.now();
        for shown in &mut self.panel.popovers.shown {
            shown.closing.get_or_insert(now);
        }
        self.redraw = true;
    }

    /// A layer surface went: if it was a popover, its motion goes too.
    pub fn popover_gone(&mut self, layer: &LayerSurface) {
        self.panel.popovers.shown.retain(|s| &s.layer != layer);
    }

    /// A press at `at`: told to the shell when it lands on none of its
    /// surfaces while a popover shows or one of its surfaces holds the
    /// keyboard. Whether the press stops there: a menu's.
    pub fn press_outside_shell(&mut self, at: Point<f64, Logical>) -> bool {
        let open: Vec<LayerSurface> = self
            .panel
            .popovers
            .open()
            .map(|s| s.layer.clone())
            .collect();
        let keyboard = self.shell_keyboard();
        let Some(shell) = open.first().cloned().or_else(|| keyboard.clone()) else {
            return false;
        };
        let on_shell = self
            .surface_under(at)
            .is_some_and(|(surface, _)| surface.id().same_client_as(&shell.wl_surface().id()));
        if on_shell {
            return false;
        }
        self.tell_shell(ToShell::Outside);
        open.iter().any(|layer| {
            layer.cached_state().keyboard_interactivity == KeyboardInteractivity::Exclusive
        })
    }

    /// The shell's layer surface holding the keyboard, if one does.
    fn shell_keyboard(&self) -> Option<LayerSurface> {
        let focus = self.seat.get_keyboard()?.current_focus()?;
        let KeyboardFocus::Wayland(surface) = focus else {
            return None;
        };
        self.space.outputs().find_map(|output| {
            let map = layer_map_for_output(output);
            map.layers()
                .find(|l| l.wl_surface() == &surface && l.namespace().starts_with("hocus-bar"))
                .cloned()
        })
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
    fn a_popover_fades_and_slides_in_then_out() {
        // At its first frame: transparent, 8 px toward the bar.
        let start = at(1000, None, 1000, true);
        assert_eq!(start.alpha, 0.0);
        assert_eq!(start.shift, POPOVER_SHIFT);
        // Ease-out: most of the way at half the time; home at 160 ms.
        let mid = at(1000, None, 1080, true);
        assert!(mid.alpha > 0.6 && mid.alpha < 1.0, "{mid:?}");
        assert_eq!(
            at(1000, None, 1160, true),
            Look {
                alpha: 1.0,
                shift: 0.0
            }
        );
        // Closing, ease-in: little gone at first, gone at 120 ms.
        let early = at(1000, Some(2000), 2030, true);
        assert!(early.alpha > 0.8, "{early:?}");
        assert_eq!(at(1000, Some(2000), 2120, true).alpha, 0.0);
        // Without motion: at once, both ways.
        assert_eq!(at(1000, None, 1000, false).alpha, 1.0);
        assert_eq!(at(1000, Some(2000), 2000, false).alpha, 0.0);
    }
}
