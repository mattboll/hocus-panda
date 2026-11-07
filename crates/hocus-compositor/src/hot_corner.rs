//! The overview's hot corner (issue #727; docs/design/windows.md §12b), as
//! GNOME's: the pointer held in a screen's top-left corner for
//! [`DWELL`] opens the overview, or closes it when open, as `Super+W` does.
//!
//! Once per entry: the corner is spent until the pointer has gone
//! [`REARM`] px away from it. Only where the pointer stops: not where the
//! bar stands (its side on the left, on a screen with a bar), nor where
//! another screen touches the corner (the pointer passes through it); not
//! while a button is held (a window dragged, a selection), nor over a
//! fullscreen window or the launcher; never with `[overview] hot-corner
//! = false` (the Settings' "Coin en haut à gauche : vue d'ensemble"). The
//! corner itself draws nothing: with reduced motion the overview fades in
//! as by its keys. Nothing runs at rest but the dwell's one timer.

use std::time::Duration;

use smithay::{
    output::Output,
    reexports::calloop::{
        RegistrationToken,
        timer::{TimeoutAction, Timer},
    },
    utils::{Logical, Point},
};

use crate::State;

/// How long the pointer stays in the corner before the overview opens: a
/// pointer only passing does not open it.
pub const DWELL: Duration = Duration::from_millis(150);
/// The corner: this far from the screen's top and left edges, logical
/// pixels (the pointer stops on the edges, so it lands there pushed up
/// and left).
pub const REACH: f64 = 1.0;
/// How far the pointer goes from the corner before it opens it again.
pub const REARM: f64 = 8.0;

/// Where the pointer stands to the corners.
#[derive(Debug, Default)]
pub enum HotCorner {
    /// Away from them: the next entry counts.
    #[default]
    Armed,
    /// In `output`'s since a moment: the timer acts as the dwell ends.
    Dwelling {
        output: String,
        timer: RegistrationToken,
    },
    /// Used (or entered while it could not act): until the pointer goes
    /// [`REARM`] px away from `output`'s corner.
    Spent { output: String },
}

impl State {
    /// The screen whose hot corner `at` (global) is in, if it is one now.
    fn hot_corner_at(&self, at: Point<f64, Logical>) -> Option<Output> {
        let output = self.space.output_under(at).next()?.clone();
        let screen = self.space.output_geometry(&output)?.to_f64();
        let corner = screen.loc;
        if at.x - corner.x >= REACH || at.y - corner.y >= REACH {
            return None;
        }
        // Another screen touching the corner: the pointer goes on there.
        let touching = [(-1.0, 0.0), (0.0, -1.0), (-1.0, -1.0)]
            .into_iter()
            .map(|(dx, dy)| Point::from((corner.x + dx, corner.y + dy)))
            .any(|p| self.space.output_under(p).next().is_some());
        if touching {
            return None;
        }
        // The bar stands there.
        if self.bar_side() == hocus_link::Side::Left && self.panel.bars.contains_key(&output.name())
        {
            return None;
        }
        Some(output)
    }

    /// Whether the corner of `output` may act now.
    fn hot_corner_acts(&self, output: &Output) -> bool {
        self.config.overview.hot_corner
            && !self.locked()
            && !self.auth_holds()
            && self.radial.is_none()
            && !self.seat.get_pointer().is_some_and(|p| p.is_grabbed())
            && !self.fullscreen_in_front(output)
            && self.launcher_on(output).is_none()
    }

    /// The pointer moved to `at` (global): it enters a corner, stays, or
    /// leaves it.
    pub fn hot_corner_motion(&mut self, at: Point<f64, Logical>) {
        let here = self.hot_corner_at(at);
        match &self.hot_corner {
            HotCorner::Dwelling { output, .. }
                if here.as_ref().is_some_and(|o| o.name() == *output) => {}
            HotCorner::Spent { output } => {
                let away = self
                    .space
                    .outputs()
                    .find(|o| o.name() == *output)
                    .and_then(|o| self.space.output_geometry(o))
                    .is_none_or(|g| {
                        let corner = g.loc.to_f64();
                        at.x - corner.x > REARM || at.y - corner.y > REARM
                    });
                if away {
                    self.hot_corner = HotCorner::Armed;
                }
            }
            HotCorner::Dwelling { .. } => {
                // Left before the dwell's end: nothing.
                self.hot_corner_cancel();
                self.hot_corner_enter(here);
            }
            HotCorner::Armed => self.hot_corner_enter(here),
        }
    }

    /// The pointer came into `output`'s corner (or none): the dwell starts
    /// if the corner may act, else it is spent until left.
    fn hot_corner_enter(&mut self, output: Option<Output>) {
        let Some(output) = output else {
            return;
        };
        let name = output.name();
        if !self.hot_corner_acts(&output) {
            self.hot_corner = HotCorner::Spent { output: name };
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
                self.hot_corner = HotCorner::Dwelling {
                    output: name,
                    timer,
                }
            }
            Err(err) => {
                tracing::warn!(%err.error, "no timer for the hot corner");
                self.hot_corner = HotCorner::Spent { output: name };
            }
        }
    }

    /// The pointer stayed in the corner: the overview opens (or closes),
    /// if the corner may still act.
    fn hot_corner_dwelt(&mut self) {
        let HotCorner::Dwelling { output, .. } = std::mem::take(&mut self.hot_corner) else {
            return;
        };
        let acts = self
            .space
            .outputs()
            .find(|o| o.name() == output)
            .cloned()
            .is_some_and(|o| self.hot_corner_acts(&o));
        if acts {
            tracing::debug!(output, "the hot corner toggles the overview");
            self.toggle_overview();
        }
        self.hot_corner = HotCorner::Spent { output };
    }

    /// The dwell under way, if any, ends without acting.
    fn hot_corner_cancel(&mut self) {
        if let HotCorner::Dwelling { timer, .. } = std::mem::take(&mut self.hot_corner) {
            self.loop_handle.remove(timer);
        }
    }

    /// `[overview] hot-corner` turned off: a dwell under way acts no more.
    pub(crate) fn hot_corner_off(&mut self) {
        self.hot_corner_cancel();
    }
}
