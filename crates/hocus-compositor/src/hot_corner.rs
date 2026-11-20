//! The screen corners (issue #727, widened by #893; docs/design/windows.md
//! §12b): the pointer held in a screen's corner for [`DWELL`] runs the
//! action `[shortcuts]` binds to that corner (`corner:top-left`,
//! `hocus_config::triggers`), by default the overview at the top left, as
//! `Super+W` does.
//!
//! Once per entry: the corner is spent until the pointer has gone
//! [`REARM`] px away from it; at the session's start the pointer only
//! resting at a corner (or moved there in place) is no entry, until it
//! has been that far away from every corner once. Only where the pointer
//! stops: not the bar's own corner (the top one on its side, on a screen
//! with a bar: there the pointer brings the bar in front), nor where
//! another screen touches the corner (the pointer passes through it); not
//! while a button is held (a window dragged, a selection), nor over a
//! fullscreen window or the launcher; never a corner no action takes. The
//! corner itself draws nothing. Nothing runs at rest but the dwell's one
//! timer.

use std::time::Duration;

use hocus_config::triggers::{Corner, Trigger};
use smithay::{
    output::Output,
    reexports::calloop::{
        RegistrationToken,
        timer::{TimeoutAction, Timer},
    },
    utils::{Logical, Point, Rectangle},
};

use crate::{State, shortcuts::Chord};

/// How long the pointer stays in the corner before its action runs: a
/// pointer only passing does not run it.
pub const DWELL: Duration = Duration::from_millis(150);
/// The corner: this far from the screen's edges, logical pixels (the
/// pointer stops on the edges, so it lands there pushed into the corner).
pub const REACH: f64 = 1.0;
/// How far the pointer goes from the corner before it runs it again.
pub const REARM: f64 = 8.0;

/// Where the pointer stands to the corners.
#[derive(Debug, Default)]
pub enum HotCorner {
    /// Not yet: the session starts with the pointer where it may be (a
    /// screen's top-left corner, at 0, 0, as a headless one does), which
    /// is no entry. Armed once it has been [`REARM`] px away from every
    /// corner.
    #[default]
    Unarmed,
    /// Away from them: the next entry counts.
    Armed,
    /// In `output`'s `corner` since a moment: the timer acts as the dwell
    /// ends.
    Dwelling {
        output: String,
        corner: Corner,
        timer: RegistrationToken,
    },
    /// Used (or entered while it could not act): until the pointer goes
    /// [`REARM`] px away from `output`'s `corner`.
    Spent { output: String, corner: Corner },
}

/// How far `at` is from `screen`'s `corner`, across and down (the
/// pointer's last column and row are the right and bottom edges).
fn from_corner(
    screen: Rectangle<f64, Logical>,
    corner: Corner,
    at: Point<f64, Logical>,
) -> (f64, f64) {
    let (left, top) = (screen.loc.x, screen.loc.y);
    let right = left + screen.size.w - 1.0;
    let bottom = top + screen.size.h - 1.0;
    let dx = if corner.left() {
        at.x - left
    } else {
        right - at.x
    };
    let dy = if corner.top() {
        at.y - top
    } else {
        bottom - at.y
    };
    (dx, dy)
}

impl State {
    /// The screen and the corner `at` (global) is in, if it is one that
    /// runs an action.
    fn hot_corner_at(&self, at: Point<f64, Logical>) -> Option<(Output, Corner)> {
        let output = self.space.output_under(at).next()?.clone();
        let screen = self.space.output_geometry(&output)?.to_f64();
        let corner = Corner::ALL.into_iter().find(|c| {
            let (dx, dy) = from_corner(screen, *c, at);
            dx < REACH && dy < REACH
        })?;
        // Another screen touching the corner: the pointer goes on there.
        let x = if corner.left() {
            screen.loc.x
        } else {
            screen.loc.x + screen.size.w - 1.0
        };
        let y = if corner.top() {
            screen.loc.y
        } else {
            screen.loc.y + screen.size.h - 1.0
        };
        let beyond = |v: f64, near: bool| if near { v - 1.0 } else { v + 1.0 };
        let (out_x, out_y) = (beyond(x, corner.left()), beyond(y, corner.top()));
        let touching = [(out_x, y), (x, out_y), (out_x, out_y)]
            .into_iter()
            .any(|p| self.space.output_under(Point::from(p)).next().is_some());
        if touching {
            return None;
        }
        // The bar's own corner: the pointer there brings it in front.
        if corner.bars(self.bar_side() == hocus_link::Side::Left)
            && self.panel.bars.contains_key(&output.name())
        {
            return None;
        }
        self.config.trigger_action(Trigger::Corner(corner))?;
        Some((output, corner))
    }

    /// Whether the corners of `output` may act now.
    fn hot_corner_acts(&self, output: &Output) -> bool {
        !self.locked()
            && !self.auth_holds()
            && self.radial_menu.open.is_none()
            && !self.capture_open()
            && !self.seat.get_pointer().is_some_and(|p| p.is_grabbed())
            && !self.fullscreen_in_front(output)
            && self.launcher_on(output).is_none()
    }

    /// Whether `at` is [`REARM`] px away from `output`'s `corner`.
    fn away_from(&self, output: &str, corner: Corner, at: Point<f64, Logical>) -> bool {
        self.space
            .outputs()
            .find(|o| o.name() == output)
            .and_then(|o| self.space.output_geometry(o))
            .is_none_or(|g| {
                let (dx, dy) = from_corner(g.to_f64(), corner, at);
                dx > REARM || dy > REARM
            })
    }

    /// The pointer moved to `at` (global): it enters a corner, stays, or
    /// leaves it.
    pub fn hot_corner_motion(&mut self, at: Point<f64, Logical>) {
        let here = self.hot_corner_at(at);
        match &self.pointer.hot_corner {
            HotCorner::Dwelling { output, corner, .. }
                if here
                    .as_ref()
                    .is_some_and(|(o, c)| o.name() == *output && c == corner) => {}
            HotCorner::Spent { output, corner } => {
                if self.away_from(output, *corner, at) {
                    self.pointer.hot_corner = HotCorner::Armed;
                }
            }
            HotCorner::Dwelling { .. } => {
                // Left before the dwell's end: nothing.
                self.hot_corner_cancel();
                self.hot_corner_enter(here);
            }
            HotCorner::Armed => self.hot_corner_enter(here),
            HotCorner::Unarmed => {
                let away = self.space.output_under(at).next().is_none_or(|o| {
                    let name = o.name();
                    Corner::ALL
                        .into_iter()
                        .all(|c| self.away_from(&name, c, at))
                });
                if away {
                    self.pointer.hot_corner = HotCorner::Armed;
                }
            }
        }
    }

    /// The pointer came into `output`'s `corner` (or none): the dwell
    /// starts if the corner may act, else it is spent until left.
    fn hot_corner_enter(&mut self, here: Option<(Output, Corner)>) {
        let Some((output, corner)) = here else {
            return;
        };
        let name = output.name();
        if !self.hot_corner_acts(&output) {
            self.pointer.hot_corner = HotCorner::Spent {
                output: name,
                corner,
            };
            return;
        }
        let inserted = self
            .loop_handle
            .insert_source(Timer::from_duration(DWELL), |_, _, app| {
                app.state.hot_corner_dwelt();
                TimeoutAction::Drop
            });
        match inserted {
            Ok(timer) => {
                self.pointer.hot_corner = HotCorner::Dwelling {
                    output: name,
                    corner,
                    timer,
                }
            }
            Err(err) => {
                tracing::warn!(%err.error, "no timer for the screen corner");
                self.pointer.hot_corner = HotCorner::Spent {
                    output: name,
                    corner,
                };
            }
        }
    }

    /// The pointer stayed in the corner: its action runs, if the corner
    /// may still act and still has one.
    fn hot_corner_dwelt(&mut self) {
        let HotCorner::Dwelling { output, corner, .. } =
            std::mem::take(&mut self.pointer.hot_corner)
        else {
            return;
        };
        let acts = self
            .space
            .outputs()
            .find(|o| o.name() == output)
            .cloned()
            .is_some_and(|o| self.hot_corner_acts(&o));
        let action = self.config.trigger_action(Trigger::Corner(corner));
        if let (true, Some(action)) = (acts, action) {
            tracing::debug!(output, ?corner, ?action, "a screen corner runs its action");
            self.run_action(action, Chord::NONE);
        }
        self.pointer.hot_corner = HotCorner::Spent { output, corner };
    }

    /// The dwell under way, if any, ends without acting.
    fn hot_corner_cancel(&mut self) {
        if let HotCorner::Dwelling { timer, .. } = std::mem::take(&mut self.pointer.hot_corner) {
            self.loop_handle.remove(timer);
        }
    }

    /// The corners' actions changed: a dwell under way acts no more.
    pub(crate) fn hot_corner_off(&mut self) {
        self.hot_corner_cancel();
    }
}
