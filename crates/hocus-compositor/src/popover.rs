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
//!
//! The emoji picker (5.7, [`EMOJI_NAMESPACE`]) is drawn and moved as a
//! popover, with its own corners and shadow, the mockup's: it fades in
//! coming down [`EMOJI_RISE`] rather than sliding from the bar. So is the
//! actions palette (#736, [`ACTIONS_NAMESPACE`]): in over 140 ms coming
//! down [`ACTIONS_DROP`], out over 100 ms; with reduced motion or a shell
//! on the CPU it still fades, without moving.
//!
//! So is the secretary's drawer (docs/design/secretary.md §5.1,
//! [`SECRETARY_NAMESPACE`]): the popovers' glass, shadow and corners; in
//! over 180 ms sliding 16 px from the bar, out over 140 ms; with reduced
//! motion or a shell on the CPU it only fades. It takes the keyboard as it
//! first shows (its field is focused on opening), on demand: a window
//! clicked takes it back, and the drawer stays.

use std::time::Duration;

use hocus_link::{
    ACTIONS_NAMESPACE, ACTIONS_RADIUS, EMOJI_NAMESPACE, EMOJI_RADIUS, EMOJI_RISE, EMOJI_ZOOM,
    POPOVER_CLOSE, POPOVER_NAMESPACE, POPOVER_OPEN, POPOVER_SHIFT, SECRETARY_NAMESPACE, ToShell,
};
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

/// The emoji picker's: the mockup's `0 18px 50px rgba(0, 0, 0, 0.5)`,
/// zoomed.
pub const EMOJI_SHADOW: ShadowStyle = ShadowStyle {
    dy: 18.0 * EMOJI_ZOOM,
    blur: 50.0 * EMOJI_ZOOM,
    alpha: 0.5,
    radius: EMOJI_RADIUS,
};

/// The actions palette's: the popovers', round its corners.
pub const ACTIONS_SHADOW: ShadowStyle = ShadowStyle {
    radius: ACTIONS_RADIUS,
    ..SHADOW
};

/// What a surface moved as a popover is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A popover of the bar's.
    Popover,
    /// The emoji picker.
    Emoji,
    /// The actions palette.
    Actions,
    /// The secretary's drawer.
    Drawer,
}

impl Kind {
    fn of(namespace: &str) -> Self {
        match namespace {
            EMOJI_NAMESPACE => Self::Emoji,
            ACTIONS_NAMESPACE => Self::Actions,
            SECRETARY_NAMESPACE => Self::Drawer,
            _ => Self::Popover,
        }
    }

    /// Whether it fades even without motion (it only fades then).
    fn always_fades(self) -> bool {
        matches!(self, Self::Actions | Self::Drawer)
    }
}

/// How the drawer opened at `opened` (and closing since `closing`) is
/// drawn at `now`: in over 180 ms from 16 px toward the bar (`ease-out`),
/// out over 140 ms (`ease-in`); without `motion`, it only fades.
fn drawer_look(opened: Duration, closing: Option<Duration>, now: Duration, motion: bool) -> Look {
    let shown = match closing {
        None => Tween::new(opened, hocus_link::DRAWER_OPEN, Bezier::EASE_OUT).at(now),
        Some(since) => 1.0 - Tween::new(since, hocus_link::DRAWER_CLOSE, Bezier::EASE_IN).at(now),
    }
    .clamp(0.0, 1.0);
    Look {
        alpha: shown as f32,
        shift: if motion {
            hocus_link::DRAWER_SHIFT * (1.0 - shown)
        } else {
            0.0
        },
        rise: 0.0,
    }
}

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
    /// A popover of the bar's, the emoji picker or the palette.
    pub kind: Kind,
}

/// How a popover is drawn at a moment: how opaque, how far toward the
/// bar from where it is, and how far above it (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub alpha: f32,
    pub shift: f64,
    pub rise: f64,
}

/// How the actions palette opened at `opened` (and closing since
/// `closing`) is drawn at `now`: it fades in over 140 ms coming down 8 px
/// (`ease-out`), and out over 100 ms (`ease-in`); without `motion`, it
/// only fades.
fn actions_look(opened: Duration, closing: Option<Duration>, now: Duration, motion: bool) -> Look {
    let shown = match closing {
        None => Tween::new(opened, hocus_link::ACTIONS_OPEN, Bezier::EASE_OUT).at(now),
        Some(since) => 1.0 - Tween::new(since, hocus_link::ACTIONS_CLOSE, Bezier::EASE_IN).at(now),
    }
    .clamp(0.0, 1.0);
    Look {
        alpha: shown as f32,
        shift: 0.0,
        rise: if motion {
            hocus_link::ACTIONS_DROP * (1.0 - shown)
        } else {
            0.0
        },
    }
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
        rise: 0.0,
    }
}

impl Shown {
    /// Its shadow.
    pub fn shadow(&self) -> ShadowStyle {
        match self.kind {
            Kind::Popover | Kind::Drawer => SHADOW,
            Kind::Emoji => EMOJI_SHADOW,
            Kind::Actions => ACTIONS_SHADOW,
        }
    }

    /// Its corners' radius.
    pub fn radius(&self) -> f64 {
        match self.kind {
            Kind::Popover | Kind::Drawer => hocus_link::RADIUS,
            Kind::Emoji => EMOJI_RADIUS,
            Kind::Actions => ACTIONS_RADIUS,
        }
    }

    /// How far it comes down from as it opens (and goes back up to).
    pub fn reach(&self) -> f64 {
        match self.kind {
            Kind::Popover | Kind::Drawer => 0.0,
            Kind::Emoji => EMOJI_RISE,
            Kind::Actions => hocus_link::ACTIONS_DROP,
        }
    }

    fn moving(&self, now: Duration) -> bool {
        let (open, close) = match self.kind {
            Kind::Actions => (hocus_link::ACTIONS_OPEN, hocus_link::ACTIONS_CLOSE),
            Kind::Drawer => (hocus_link::DRAWER_OPEN, hocus_link::DRAWER_CLOSE),
            Kind::Popover | Kind::Emoji => (POPOVER_OPEN, POPOVER_CLOSE),
        };
        match self.closing {
            Some(since) => now < since + close,
            None => now < self.opened + open,
        }
    }
}

impl Popovers {
    /// The popover `layer`, if it shows, and how.
    pub fn look(&self, layer: &LayerSurface, now: Duration, motion: bool) -> Option<Look> {
        let shown = self.shown(layer)?;
        Some(match shown.kind {
            Kind::Popover => look(shown.opened, shown.closing, now, motion),
            Kind::Emoji => {
                let look = look(shown.opened, shown.closing, now, motion);
                Look {
                    alpha: look.alpha,
                    shift: 0.0,
                    rise: if motion {
                        EMOJI_RISE * f64::from(1.0 - look.alpha)
                    } else {
                        0.0
                    },
                }
            }
            Kind::Actions => actions_look(shown.opened, shown.closing, now, motion),
            Kind::Drawer => drawer_look(shown.opened, shown.closing, now, motion),
        })
    }

    pub fn shown(&self, layer: &LayerSurface) -> Option<&Shown> {
        self.shown.iter().find(|s| &s.layer == layer)
    }

    /// Whether one is still moving at `now`.
    pub fn tick(&self, now: Duration, motion: bool) -> bool {
        // The palette and the drawer fade in and out even without motion.
        self.shown
            .iter()
            .any(|s| s.moving(now) && (motion || s.kind.always_fades()))
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
        let kind = Kind::of(layer.namespace());
        self.panel.popovers.shown.push(Shown {
            layer: layer.clone(),
            opened,
            closing: None,
            ids: FrameIds::default(),
            kind,
        });
        // The drawer's field is focused as it opens (§5.5); it asks for
        // the keyboard on demand only, so that a window clicked takes it
        // back while the drawer stays.
        if kind == Kind::Drawer
            && let Some(keyboard) = self.seat.get_keyboard()
        {
            keyboard.set_focus(
                self,
                Some(KeyboardFocus::Wayland(layer.wl_surface().clone())),
                smithay::utils::SERIAL_COUNTER.next_serial(),
            );
        }
        self.redraw = true;
    }

    /// The shell says its popovers of `namespace` close.
    pub fn popovers_closing(&mut self, namespace: &str) {
        if Kind::of(namespace) == Kind::Popover && namespace != POPOVER_NAMESPACE {
            return;
        }
        let now = self.now();
        for shown in &mut self.panel.popovers.shown {
            if shown.layer.namespace() == namespace {
                shown.closing.get_or_insert(now);
            }
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
        let launcher = self
            .space
            .outputs()
            .find_map(|o| self.launcher_on(o))
            .filter(|_| self.panel.launchers.open());
        let Some(shell) = open
            .first()
            .cloned()
            .or_else(|| launcher.clone())
            .or_else(|| keyboard.clone())
        else {
            return false;
        };
        let on_shell = self
            .surface_under(at)
            .is_some_and(|(surface, _)| surface.id().same_client_as(&shell.wl_surface().id()));
        if on_shell {
            return false;
        }
        self.tell_shell(ToShell::Outside);
        // The launcher open on another screen closes, as a menu does.
        launcher.is_some()
            || open.iter().any(|layer| {
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
                .find(|l| {
                    l.wl_surface() == &surface
                        && (l.namespace().starts_with("hocus-bar")
                            || l.namespace() == EMOJI_NAMESPACE
                            || l.namespace() == ACTIONS_NAMESPACE)
                })
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
                shift: 0.0,
                rise: 0.0
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

    #[test]
    fn the_palette_comes_down_fading_and_without_motion_only_fades() {
        let ms = Duration::from_millis;
        let start = actions_look(ms(1000), None, ms(1000), true);
        assert_eq!((start.alpha, start.rise), (0.0, hocus_link::ACTIONS_DROP));
        let mid = actions_look(ms(1000), None, ms(1070), true);
        assert!(
            mid.alpha > 0.5 && mid.alpha < 1.0 && mid.rise > 0.0,
            "{mid:?}"
        );
        assert_eq!(
            actions_look(ms(1000), None, ms(1140), true),
            Look {
                alpha: 1.0,
                shift: 0.0,
                rise: 0.0
            }
        );
        assert_eq!(
            actions_look(ms(1000), Some(ms(2000)), ms(2100), true).alpha,
            0.0
        );
        // Reduced motion: a fade, in place.
        let still = actions_look(ms(1000), None, ms(1070), false);
        assert!(still.alpha > 0.5 && still.alpha < 1.0, "{still:?}");
        assert_eq!(still.rise, 0.0);
        let out = actions_look(ms(1000), Some(ms(2000)), ms(2050), false);
        assert!(out.alpha > 0.0 && out.alpha < 1.0, "{out:?}");
    }
}
