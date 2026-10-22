//! Hot corners, Super alone and touchpad gestures (#86): ways in that need
//! no key combination. A corner acts once the pointer rests in it, so
//! passing by does nothing; Super acts alone only when nothing else
//! happened while it was held; a swipe goes sideways (desktops, #36) or
//! up and down, chosen once the fingers have moved enough to tell.

use std::time::Duration;

use hocus_core::{
    OutputName, WindowId,
    config::{Corners, Trigger},
    shortcuts::Scope,
};
use smithay::{
    reexports::calloop::timer::{TimeoutAction, Timer},
    utils::{Logical, Point, SERIAL_COUNTER},
};

use crate::State;

/// How far the fingers go before the swipe's way is chosen, logical.
const DECIDE: f64 = 16.0;
/// How far up or down a swipe goes to act.
const ACT: f64 = 60.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    fn of(self, corners: Corners) -> Trigger {
        match self {
            Self::TopLeft => corners.top_left,
            Self::TopRight => corners.top_right,
            Self::BottomLeft => corners.bottom_left,
            Self::BottomRight => corners.bottom_right,
        }
    }
}

/// Which way a swipe goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Way {
    Undecided,
    /// Desktops follow the fingers.
    Sideways,
    /// Desktops switch once the fingers are lifted: with reduced motion.
    SidewaysStill,
    Vertical,
}

#[derive(Debug, Clone, Copy)]
struct Swipe {
    fingers: u32,
    dx: f64,
    dy: f64,
    way: Way,
}

#[derive(Debug, Default)]
pub struct Triggers {
    /// The corner the pointer is in, since when, and whether it acted.
    resting: Option<(String, Corner, Duration, bool)>,
    /// Super is down with nothing else so far.
    super_alone: bool,
    /// Super alone was let go: acted on after the key is handled.
    fire: Option<Trigger>,
    swipe: Option<Swipe>,
    /// Show desktop: the windows put out of the way, by screen.
    peeking: Option<(String, Vec<WindowId>)>,
}

impl State {
    /// The pointer moved: a corner may start counting.
    pub fn corner_pointer(&mut self, at: Point<f64, Logical>) {
        let found = self.corner_at(at);
        match (&self.triggers.resting, found) {
            (Some((o, c, _, _)), Some((name, corner))) if *o == name && *c == corner => {}
            (_, Some((name, corner))) => {
                let now = self.clock.now();
                self.triggers.resting = Some((name, corner, now, false));
                let delay = Duration::from_millis(u64::from(self.config.hot_corners.delay_ms));
                if let Some(handle) = &self.loop_handle {
                    let woken = handle.insert_source(Timer::from_duration(delay), |_, _, app| {
                        app.state.needs_redraw = true;
                        TimeoutAction::Drop
                    });
                    if let Err(err) = woken {
                        tracing::debug!(%err.error, "the corner will act on the next event");
                    }
                }
            }
            (_, None) => self.triggers.resting = None,
        }
    }

    /// The hot corner under `at`: a screen's corner set to act, and not
    /// against another screen.
    fn corner_at(&self, at: Point<f64, Logical>) -> Option<(String, Corner)> {
        if self.locked() {
            return None;
        }
        let output = self
            .space
            .outputs()
            .find(|o| {
                self.space
                    .output_geometry(o)
                    .is_some_and(|g| g.to_f64().contains(at))
            })?
            .clone();
        let area = self.space.output_geometry(&output)?.to_f64();
        let (left, top) = (at.x < area.loc.x + 1.0, at.y < area.loc.y + 1.0);
        let right = at.x >= area.loc.x + area.size.w - 1.0;
        let bottom = at.y >= area.loc.y + area.size.h - 1.0;
        let corner = match (left, right, top, bottom) {
            (true, _, true, _) => Corner::TopLeft,
            (_, true, true, _) => Corner::TopRight,
            (true, _, _, true) => Corner::BottomLeft,
            (_, true, _, true) => Corner::BottomRight,
            _ => return None,
        };
        // Against another screen, the pointer goes on: not a corner.
        let (sx, sy) = match corner {
            Corner::TopLeft => (-1.0, -1.0),
            Corner::TopRight => (1.0, -1.0),
            Corner::BottomLeft => (-1.0, 1.0),
            Corner::BottomRight => (1.0, 1.0),
        };
        let beyond = [
            Point::from((at.x + 2.0 * sx, at.y - 2.0 * sy)),
            Point::from((at.x - 2.0 * sx, at.y + 2.0 * sy)),
            Point::from((at.x + 2.0 * sx, at.y + 2.0 * sy)),
        ];
        let neighbour = self.space.outputs().any(|o| {
            *o != output
                && self.space.output_geometry(o).is_some_and(|g| {
                    let g = g.to_f64();
                    beyond.iter().any(|p| g.contains(*p))
                })
        });
        if neighbour {
            return None;
        }
        let corners = self.config.hot_corners.of(&output.name());
        (corner.of(corners) != Trigger::None).then(|| (output.name(), corner))
    }

    /// After each dispatch: a corner rested in long enough acts, once.
    pub fn refresh_corners(&mut self) {
        let delay = Duration::from_millis(u64::from(self.config.hot_corners.delay_ms));
        let now = self.clock.now();
        let Some((name, corner, since, fired)) = self.triggers.resting.clone() else {
            return;
        };
        if fired || now < since + delay {
            return;
        }
        self.triggers.resting = Some((name.clone(), corner, since, true));
        let trigger = corner.of(self.config.hot_corners.of(&name));
        tracing::debug!(output = name, ?corner, ?trigger, "hot corner");
        self.run_trigger(trigger);
    }

    /// A key went down or up, before shortcuts: Super alone acts when let
    /// go with nothing else between.
    pub fn super_key(&mut self, pressed: bool, is_super: bool, others_held: bool) {
        match (pressed, is_super) {
            (true, true) => self.triggers.super_alone = !others_held,
            (true, false) => self.triggers.super_alone = false,
            (false, true) => {
                if std::mem::take(&mut self.triggers.super_alone)
                    && !self.shortcuts_inhibited()
                    && !self.locked()
                {
                    self.triggers.fire = Some(self.config.gestures.super_alone);
                }
            }
            (false, false) => {}
        }
    }

    /// Anything else than keys while Super is held: no longer alone.
    pub fn super_spoiled(&mut self) {
        self.triggers.super_alone = false;
    }

    /// After a key is handled: Super alone acts.
    pub fn super_fired(&mut self) {
        if let Some(trigger) = self.triggers.fire.take() {
            self.run_trigger(trigger);
        }
    }

    pub fn run_trigger(&mut self, trigger: Trigger) {
        match trigger {
            Trigger::Overview => self.toggle_overview(),
            Trigger::Launcher => self.toggle_launcher(),
            Trigger::ShowDesktop => self.toggle_show_desktop(),
            Trigger::None => {}
        }
    }

    /// Show desktop: the screen under the pointer's windows out of the
    /// way; asked again, back as they were.
    pub fn toggle_show_desktop(&mut self) {
        let serial = SERIAL_COUNTER.next_serial();
        if let Some((_, windows)) = self.triggers.peeking.take() {
            // Bottom first: they come back in their order, the top one
            // focused.
            for id in windows {
                if self.windows.contains_key(&id) && self.model.is_minimized(id) {
                    let _ = self.model.focus(id);
                }
            }
            self.sync_desktops();
            self.apply_focus(serial);
            return;
        }
        let Some(output) = self.output_under_pointer() else {
            return;
        };
        let windows = self.model.visible_windows(&OutputName(output.name()));
        if windows.is_empty() {
            return;
        }
        for id in &windows {
            let _ = self.model.minimize(*id);
        }
        self.triggers.peeking = Some((output.name(), windows));
        self.sync_desktops();
        self.apply_focus(serial);
    }

    /// Fingers on the touchpad: whether the compositor takes the swipe.
    pub fn gesture_begin(&mut self, fingers: u32) -> bool {
        if self.locked() || !(3..=4).contains(&fingers) {
            return false;
        }
        self.super_spoiled();
        self.triggers.swipe = Some(Swipe {
            fingers,
            dx: 0.0,
            dy: 0.0,
            way: Way::Undecided,
        });
        true
    }

    pub fn gesture_update(&mut self, dx: f64, dy: f64) {
        let Some(mut swipe) = self.triggers.swipe else {
            return;
        };
        swipe.dx += dx;
        swipe.dy += dy;
        match swipe.way {
            Way::Undecided if swipe.dx.hypot(swipe.dy) >= DECIDE => {
                let sideways = swipe.dx.abs() > swipe.dy.abs();
                let desktops = swipe.fingers == 3 && self.config.gestures.three_sideways;
                swipe.way = match (sideways, desktops) {
                    (true, true) if self.swipe_begin(3) => {
                        // The desktops catch up with the fingers.
                        self.swipe_update(swipe.dx);
                        Way::Sideways
                    }
                    (true, true) => Way::SidewaysStill,
                    (true, false) => Way::Undecided,
                    (false, _) => Way::Vertical,
                };
            }
            Way::Sideways => {
                self.swipe_update(dx);
            }
            _ => {}
        }
        self.triggers.swipe = Some(swipe);
    }

    pub fn gesture_end(&mut self, cancelled: bool) {
        let Some(swipe) = self.triggers.swipe.take() else {
            return;
        };
        match swipe.way {
            Way::Sideways => {
                self.swipe_end(cancelled);
            }
            // Fingers to the left bring the next desktop.
            Way::SidewaysStill if !cancelled && swipe.dx.abs() >= 2.0 * ACT => {
                let step = if swipe.dx < 0.0 { 1 } else { -1 };
                self.step_desktop(Scope::PointerOutput, step, SERIAL_COUNTER.next_serial());
            }
            Way::Vertical if !cancelled && swipe.dy.abs() >= ACT => {
                let g = &self.config.gestures;
                let trigger = match (swipe.fingers, swipe.dy < 0.0) {
                    (3, true) => g.three_up,
                    (3, false) => g.three_down,
                    (_, true) => g.four_up,
                    (_, false) => g.four_down,
                };
                self.run_trigger(trigger);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_corner_reads_its_trigger() {
        let corners = Corners {
            top_left: Trigger::Overview,
            bottom_right: Trigger::ShowDesktop,
            ..Corners::default()
        };
        assert_eq!(Corner::TopLeft.of(corners), Trigger::Overview);
        assert_eq!(Corner::BottomRight.of(corners), Trigger::ShowDesktop);
        assert_eq!(Corner::TopRight.of(corners), Trigger::None);
    }
}
