//! The touchpad's swipes the desktop takes (6.13, issue #893): three or
//! four fingers left, right, up or down, each running the action
//! `[shortcuts]` binds to it (`swipe:3:left`, `hocus_config::triggers`).
//! By default three fingers across move the desktops (windows.md §12),
//! four fingers up open the launcher and down close it (launcher.md);
//! four fingers down with no launcher open open the overview, or close it
//! (8.5 A, windows.md §12b); three fingers down every desktop at once, and,
//! while it shows, three fingers up close it, whatever they are bound to
//! (#944, windows.md §12c). A swipe no action takes is the application's
//! under the pointer (`zwp_pointer_gestures_v1`).
//!
//! The fingers' first travel decides ([`Hold::Deciding`]): until they have
//! gone [`SWIPE_AXIS`] one way, their events are kept; a swipe the desktop
//! does not take then reaches the application whole, from its start (a
//! three-finger swipe that starts vertical is the application's for its
//! whole length, unless an action takes three fingers that way). The
//! bindings are read as the fingers come down: one changed under way
//! changes nothing until they lift.
//!
//! Switching desktops, the launcher and the overview follow the fingers
//! (the desktops when the fingers go the way the strip moves: right
//! brings the desktop on the left). Any other action runs once, as the
//! fingers lift, when they went [`RELEASE_TRAVEL`] its way.
//!
//! The launcher follows the fingers: its presence goes with their travel
//! its way, whole at [`LAUNCHER_TRAVEL`]; let go past a third of it, or on
//! a flick, it goes on to whole, else back out (`crate::launcher`); the
//! other way with it open closes it. The direction is always the fingers',
//! whatever natural scrolling says (libinput's gestures are not reversed
//! by it). With reduced motion nothing follows: the release decides.

use smithay::utils::{Logical, Point};

use hocus_config::triggers::{Toward, Trigger};

use crate::{
    State,
    desktops::SWIPE_AXIS,
    input::Gesture,
    shortcuts::{Action, Chord},
};

/// Finger travel up (libinput's units) that brings the launcher whole.
pub const LAUNCHER_TRAVEL: f64 = 300.0;
/// Past this share of it, the launcher let go goes on to whole.
pub const LAUNCHER_COMPLETES: f64 = 1.0 / 3.0;
/// Or when the fingers leave this fast, in [`LAUNCHER_TRAVEL`]s a second.
pub const LAUNCHER_FLICK: f64 = 1.2;
/// How far the fingers go an action's way (libinput's units) for it to run
/// as they lift: a third of the launcher's travel.
pub const RELEASE_TRAVEL: f64 = LAUNCHER_TRAVEL * LAUNCHER_COMPLETES;

/// The touchpad's swipe, from the fingers down to their lift.
#[derive(Debug, Default)]
pub enum Hold {
    /// None, or one the application has: its events go to it.
    #[default]
    Free,
    /// Down, not yet far enough to say which way: its events kept, to be
    /// given to the application if the desktop does not take it.
    Deciding {
        fingers: u32,
        time: u32,
        moves: Vec<(u32, Point<f64, Logical>)>,
    },
    /// Three fingers across: the strip of `State::swiping` follows.
    Desktops,
    /// The launcher's swipe, or the way back with it open: it follows.
    Launcher(LauncherSwipe),
    /// The overview's swipe opened or closed it, or every desktop's: the
    /// rest of the swipe is the desktop's, doing nothing.
    Overview,
    /// Another action's: it runs as the fingers lift, if they went
    /// [`RELEASE_TRAVEL`] its way (`travel`, so far).
    Released {
        action: Action,
        toward: Toward,
        travel: f64,
    },
}

/// How far (`dx`, `dy`) goes `toward`.
fn along((dx, dy): (f64, f64), toward: Toward) -> f64 {
    let (ux, uy) = toward.unit();
    dx * ux + dy * uy
}

/// Fingers bringing the launcher, or taking it away.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LauncherSwipe {
    /// The way of the launcher's swipe (up by default).
    way: Toward,
    /// That way from closed; else the other way while it is open.
    opening: bool,
    /// The fingers' travel towards that, in libinput's units.
    travel: f64,
    /// [`LAUNCHER_TRAVEL`]s a second towards that, from the last moves;
    /// and the last move's time (libinput's milliseconds).
    velocity: f64,
    last: Option<u32>,
    /// With reduced motion it does not follow; the release decides.
    reduced: bool,
}

impl LauncherSwipe {
    fn new(way: Toward, opening: bool, reduced: bool) -> Self {
        Self {
            way,
            opening,
            travel: 0.0,
            velocity: 0.0,
            last: None,
            reduced,
        }
    }

    /// The fingers moved `step` the launcher's way (negative: back) at
    /// `time`.
    fn update(&mut self, step: f64, time: u32) {
        let step = if self.opening { step } else { -step };
        self.travel += step;
        if let Some(last) = self.last {
            let dt = f64::from(time.wrapping_sub(last)) / 1000.0;
            if dt > 0.0 && dt < 0.25 {
                let v = step / LAUNCHER_TRAVEL / dt;
                self.velocity = 0.6 * v + 0.4 * self.velocity;
            }
        }
        self.last = Some(time);
    }

    /// How far the launcher shows: its share of the travel, or of what is
    /// left of it as it closes; with reduced motion, as it was.
    pub fn presence(&self) -> f64 {
        let done = if self.reduced {
            0.0
        } else {
            (self.travel / LAUNCHER_TRAVEL).clamp(0.0, 1.0)
        };
        if self.opening { done } else { 1.0 - done }
    }

    /// Whether, let go now, it goes where the fingers took it: past a
    /// third, or on a flick that way; a flick back keeps it as it was.
    pub fn completes(&self) -> bool {
        if self.velocity <= -LAUNCHER_FLICK {
            return false;
        }
        self.velocity >= LAUNCHER_FLICK || self.travel >= LAUNCHER_TRAVEL * LAUNCHER_COMPLETES
    }
}

impl State {
    /// A swipe's step: whether the desktop takes it (else it goes to the
    /// application). `grabbed`: a grab holds the pointer, which keeps the
    /// swipes the application's.
    pub(crate) fn take_swipe(&mut self, gesture: Gesture, grabbed: bool) -> bool {
        match (std::mem::take(&mut self.gestures.hold), gesture) {
            (Hold::Free, Gesture::SwipeBegin { time, fingers }) => {
                // Every desktop showing: three fingers up close it.
                let closes = fingers == 3 && self.all_desktops_open();
                let ours = !grabbed && (closes || self.config.swipes_of(fingers));
                if ours {
                    self.gestures.hold = Hold::Deciding {
                        fingers,
                        time,
                        moves: Vec::new(),
                    };
                }
                ours
            }
            (Hold::Free, _) => false,
            // A new swipe before the last one lifted (not libinput's way):
            // the last one ends there.
            (hold, Gesture::SwipeBegin { time, .. }) => {
                self.gestures.hold = hold;
                let end = Gesture::SwipeEnd {
                    time,
                    cancelled: true,
                };
                if !self.take_swipe(end, grabbed) {
                    self.gesture_to_client(end);
                }
                self.gestures.hold = Hold::Free;
                self.take_swipe(gesture, grabbed)
            }
            (
                Hold::Deciding {
                    fingers,
                    time: began,
                    mut moves,
                },
                Gesture::SwipeUpdate { time, delta },
            ) => {
                moves.push((time, delta));
                let (dx, dy) = moves
                    .iter()
                    .fold((0.0, 0.0), |(x, y), (_, d)| (x + d.x, y + d.y));
                if dx.hypot(dy) < SWIPE_AXIS {
                    self.gestures.hold = Hold::Deciding {
                        fingers,
                        time: began,
                        moves,
                    };
                    return true;
                }
                self.decide(fingers, began, moves, (dx, dy), time);
                true
            }
            (
                Hold::Deciding {
                    fingers,
                    time: began,
                    moves,
                },
                Gesture::SwipeEnd { .. },
            ) => {
                // Lifted before it said which way: the application's.
                self.give_to_application(fingers, began, moves);
                self.gestures.hold = Hold::Free;
                false
            }
            (Hold::Desktops, Gesture::SwipeUpdate { time, delta }) => {
                self.desktop_swipe_update(delta.x, delta.y, time);
                self.gestures.hold = Hold::Desktops;
                true
            }
            (Hold::Desktops, Gesture::SwipeEnd { cancelled, .. }) => {
                self.desktop_swipe_end(cancelled);
                true
            }
            (Hold::Launcher(mut swipe), Gesture::SwipeUpdate { time, delta }) => {
                swipe.update(along((delta.x, delta.y), swipe.way), time);
                if !swipe.reduced {
                    self.panel.launchers.hold(swipe.presence());
                    self.redraw = true;
                }
                self.gestures.hold = Hold::Launcher(swipe);
                true
            }
            (Hold::Launcher(swipe), Gesture::SwipeEnd { cancelled, .. }) => {
                self.launcher_swipe_end(swipe, cancelled);
                true
            }
            (Hold::Overview, Gesture::SwipeUpdate { .. }) => {
                self.gestures.hold = Hold::Overview;
                true
            }
            (Hold::Overview, Gesture::SwipeEnd { .. }) => true,
            (
                Hold::Released {
                    action,
                    toward,
                    travel,
                },
                Gesture::SwipeUpdate { delta, .. },
            ) => {
                self.gestures.hold = Hold::Released {
                    action,
                    toward,
                    travel: travel + along((delta.x, delta.y), toward),
                };
                true
            }
            (Hold::Released { action, travel, .. }, Gesture::SwipeEnd { cancelled, .. }) => {
                if !cancelled && travel >= RELEASE_TRAVEL {
                    tracing::debug!(?action, travel, "a swipe runs its action");
                    self.run_action(action, Chord::NONE);
                }
                true
            }
            // Pinches and holds are never the desktop's.
            (hold, _) => {
                self.gestures.hold = hold;
                false
            }
        }
    }

    /// The fingers went far enough, (`dx`, `dy`) in all, the last move at
    /// `time`: the desktop takes the swipe, or the application has it
    /// from its start.
    fn decide(
        &mut self,
        fingers: u32,
        began: u32,
        moves: Vec<(u32, Point<f64, Logical>)>,
        (dx, dy): (f64, f64),
        time: u32,
    ) {
        let toward = Toward::of(dx, dy);
        let swipe = |toward| {
            u8::try_from(fingers)
                .ok()
                .map(|fingers| Trigger::Swipe { fingers, toward })
        };
        let open = self.panel.launchers.open();
        // Every desktop showing: three fingers up close it, built in as
        // four fingers down close the launcher, whatever they are bound to
        // (#944).
        if fingers == 3 && toward == Toward::Up && self.all_desktops_open() {
            tracing::debug!("three fingers up: every desktop closes");
            self.close_overview(None);
            self.gestures.hold = Hold::Overview;
            return;
        }
        // The launcher open: the way back from its own swipe closes it,
        // following.
        if open
            && self.shell_linked()
            && swipe(toward.opposite()).and_then(|t| self.config.trigger_action(t))
                == Some(Action::Launcher)
        {
            let way = toward.opposite();
            let mut swipe = LauncherSwipe::new(way, false, self.reduced_motion());
            swipe.update(along((dx, dy), way), time);
            self.panel.launchers.hold(swipe.presence());
            self.redraw = true;
            self.gestures.hold = Hold::Launcher(swipe);
            tracing::debug!("fingers take the launcher away");
            return;
        }
        let action = swipe(toward).and_then(|t| self.config.trigger_action(t));
        match action {
            // The strip follows the fingers: those going left bring the
            // desktop on the right.
            Some(Action::DesktopBy(by))
                if (toward == Toward::Left && by == 1) || (toward == Toward::Right && by == -1) =>
            {
                self.desktop_swipe_begin();
                self.desktop_swipe_update(dx, dy, time);
                self.gestures.hold = Hold::Desktops;
                return;
            }
            // As `Super+W`; not over an open launcher.
            Some(Action::Overview) if !open => {
                tracing::debug!(fingers, ?toward, "a swipe: the overview");
                self.toggle_overview();
                self.gestures.hold = Hold::Overview;
                return;
            }
            // From the desktop or the overview; showing already, nothing.
            Some(Action::AllDesktops) if !open => {
                tracing::debug!(fingers, ?toward, "a swipe: every desktop");
                self.open_all_desktops();
                self.gestures.hold = Hold::Overview;
                return;
            }
            Some(Action::Launcher) if !open && self.shell_linked() => {
                let mut swipe = LauncherSwipe::new(toward, true, self.reduced_motion());
                swipe.update(along((dx, dy), toward), time);
                self.toggle_launcher();
                self.panel.launchers.hold(swipe.presence());
                self.redraw = true;
                self.gestures.hold = Hold::Launcher(swipe);
                tracing::debug!(fingers, ?toward, "fingers hold the launcher");
                return;
            }
            Some(Action::Overview | Action::AllDesktops | Action::Launcher) | None => {}
            Some(action) => {
                self.gestures.hold = Hold::Released {
                    action,
                    toward,
                    travel: along((dx, dy), toward),
                };
                return;
            }
        }
        self.give_to_application(fingers, began, moves);
        self.gestures.hold = Hold::Free;
    }

    /// The fingers lifted from the launcher: it goes on, or back.
    fn launcher_swipe_end(&mut self, swipe: LauncherSwipe, cancelled: bool) {
        let completes = swipe.completes() && !cancelled;
        // Opening and not far enough, or closing far enough: it goes.
        let whole = swipe.opening == completes;
        tracing::debug!(
            opening = swipe.opening,
            completes,
            travel = swipe.travel,
            "four fingers let the launcher go"
        );
        let now = self.now();
        if !whole {
            // The shell closes it; it fades out from where it is.
            self.toggle_launcher();
        }
        self.panel.launchers.let_go(now, whole);
        self.redraw = true;
    }

    /// A swipe the desktop does not take: the application under the
    /// pointer gets it from its start.
    fn give_to_application(
        &mut self,
        fingers: u32,
        began: u32,
        moves: Vec<(u32, Point<f64, Logical>)>,
    ) {
        self.gesture_to_client(Gesture::SwipeBegin {
            time: began,
            fingers,
        });
        for (time, delta) in moves {
            self.gesture_to_client(Gesture::SwipeUpdate { time, delta });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launcher_follows_up_and_goes_on_past_a_third_or_on_a_flick() {
        let mut swipe = LauncherSwipe::new(Toward::Up, true, false);
        // Slowly up a sixth of the way at a time.
        for i in 0..3 {
            swipe.update(LAUNCHER_TRAVEL / 6.0, 200 * i);
        }
        assert!((swipe.presence() - 0.5).abs() < 1e-9);
        assert!(swipe.completes());
        // A quarter, slowly: back.
        let mut short = LauncherSwipe::new(Toward::Up, true, false);
        short.update(LAUNCHER_TRAVEL / 8.0, 0);
        short.update(LAUNCHER_TRAVEL / 8.0, 400);
        assert!(!short.completes());
        // A fifth, fast: a flick, it goes on.
        let mut flick = LauncherSwipe::new(Toward::Up, true, false);
        for i in 0..4 {
            flick.update(LAUNCHER_TRAVEL / 20.0, 10 * i);
        }
        assert!(flick.completes(), "{flick:?}");
        // Down from whole, half the way: half there.
        let mut closing = LauncherSwipe::new(Toward::Up, false, false);
        closing.update(-LAUNCHER_TRAVEL / 2.0, 0);
        assert!((closing.presence() - 0.5).abs() < 1e-9);
        assert!(closing.completes());
    }

    #[test]
    fn with_reduced_motion_it_stays_as_it_was_until_let_go() {
        let mut swipe = LauncherSwipe::new(Toward::Up, true, true);
        swipe.update(LAUNCHER_TRAVEL / 2.0, 0);
        assert_eq!(swipe.presence(), 0.0);
        assert!(swipe.completes());
        let mut closing = LauncherSwipe::new(Toward::Up, false, true);
        closing.update(-LAUNCHER_TRAVEL / 2.0, 0);
        assert_eq!(closing.presence(), 1.0);
    }
}
