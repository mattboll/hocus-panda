//! The moments (#810, #811): an agent's good news plays once, behind the
//! windows, on the screen the user looks at (the bubbles', the focused
//! window's, else the pointer's): fireworks as its tests go green
//! ([`super::fireworks`]), confetti as it celebrates ([`super::confetti`]).
//!
//! Decided as it starts, with every effect's rules: its switch on; none
//! with reduced motion (the bar says the news, still), in the power
//! saver, while a screen is shared or recorded, on the CPU, while locked,
//! nor on a screen off or showing a fullscreen window. Once started it
//! plays to its end, only on the screen (never in a capture, a share's or
//! a recording's frames), and nothing is drawn for it after.

use std::time::Duration;

use smithay::output::Output;

use super::{
    confetti::{self, Confetti, Ledge},
    fireworks::Show,
    snow::Holder,
};
use crate::State;

/// Where the confetti leave the bar, from its side's edge and top
/// (logical): the board's, at its bar's side, 54 px down.
const BURST_OUT: f64 = 6.0;
const BURST_DOWN: f64 = 54.0;

impl State {
    /// The screen a moment plays on now, if it may: its switch `on`, the
    /// effects playing, the session open, the screen on and showing no
    /// fullscreen window.
    fn moment_screen(&self, on: bool, what: &str) -> Option<Output> {
        if !on {
            return None;
        }
        if let Some(still) = self.effects_still() {
            tracing::debug!(what, ?still, "no moment");
            return None;
        }
        if self.locked() {
            tracing::debug!(what, "no moment while locked");
            return None;
        }
        let output = self.bubbles_screen()?;
        if !self.output_powered(&output) || self.wall_hidden(&output) {
            tracing::debug!(what, output = output.name(), "no moment on that screen");
            return None;
        }
        Some(output)
    }

    /// An agent's tests went green (#810): fireworks, if they may play
    /// and none plays already. Whether they go off.
    pub(crate) fn fireworks_go_off(&mut self) -> bool {
        if self.fx.fireworks.is_some() {
            return false;
        }
        let Some(output) = self.moment_screen(self.config.effects.fireworks, "fireworks") else {
            return false;
        };
        let Some(zone) = self.work_area(&output) else {
            return false;
        };
        let zone = [zone.x, zone.y, zone.width, zone.height].map(|v| v as f32);
        self.fx.moments += 1;
        tracing::debug!(output = output.name(), "fireworks go off");
        self.fx.fireworks = Some(Show::new(self.now(), self.fx.moments, output.name(), zone));
        self.redraw = true;
        true
    }

    /// An agent celebrates (#811): confetti from the bar, more of them if
    /// some fly there already. Whether they go off.
    pub(crate) fn confetti_go_off(&mut self) -> bool {
        let Some(output) = self.moment_screen(self.config.effects.confetti, "confetti") else {
            return false;
        };
        let (Some(area), Some(work)) =
            (self.space.output_geometry(&output), self.work_area(&output))
        else {
            return false;
        };
        let now = self.now();
        let (ax, ay) = (f64::from(area.loc.x), f64::from(area.loc.y));
        // From the bar's side facing the windows, else where the board's
        // bar would be.
        let from = match self.bar_rect(&output, now) {
            Some(bar) => {
                let leftward = bar.loc.x + bar.size.w / 2.0 > f64::from(area.size.w) / 2.0;
                let x = if leftward {
                    bar.loc.x - BURST_OUT
                } else {
                    bar.loc.x + bar.size.w + BURST_OUT
                };
                confetti::From {
                    x: (ax + x) as f32,
                    y: (ay + bar.loc.y + BURST_DOWN) as f32,
                    leftward,
                }
            }
            None => confetti::From {
                x: (ax + f64::from(area.size.w) - 222.0) as f32,
                y: (ay + 70.0) as f32,
                leftward: true,
            },
        };
        let above = [work.x as f32, work.width as f32, work.y as f32];
        let name = output.name();
        let mut confetti = match self.fx.confetti.take() {
            Some(c) if c.output == name => c,
            _ => {
                self.fx.moments += 1;
                let zone = [area.loc.x, area.loc.y, area.size.w, area.size.h].map(|v| v as f32);
                Confetti::new(now, self.fx.moments, name.clone(), zone)
            }
        };
        confetti.fire(from, above);
        confetti.fired = now;
        tracing::debug!(output = name, "confetti go off");
        self.fx.confetti = Some(confetti);
        self.redraw = true;
        true
    }

    /// The edges confetti settle on, on screen `name`, front first: the
    /// windows' tops as drawn (not fullscreen, nor minimised), its bar's.
    pub(crate) fn confetti_ledges(&self, name: &str, now: Duration) -> Vec<Ledge> {
        let Some(output) = self.space.outputs().find(|o| o.name() == name) else {
            return Vec::new();
        };
        let Some(area) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        self.model
            .stack()
            .iter()
            .rev()
            .filter(|id| self.on_current_desktop(**id))
            .map(|id| Holder::Window(*id))
            .chain(std::iter::once(Holder::Bar(name.to_owned())))
            .filter_map(|h| {
                let edge = self.snow_edge(&h, now)?;
                edge.overlaps(area.to_f64()).then_some((
                    h,
                    [edge.loc.x as f32, edge.loc.y as f32, edge.size.w as f32],
                ))
            })
            .collect()
    }

    /// The moments move on to `now`; over, they go (one frame more, to
    /// take them off). Whether one moves.
    pub(crate) fn tick_moments(&mut self, now: Duration) -> bool {
        let mut moving = false;
        if let Some(show) = self.fx.fireworks.as_mut() {
            show.advance(now);
            if show.over(now) {
                tracing::debug!("the fireworks are over");
                self.fx.fireworks = None;
                self.redraw = true;
            } else {
                moving = true;
            }
        }
        if let Some(output) = self.fx.confetti.as_ref().map(|c| c.output.clone()) {
            let ledges = self.confetti_ledges(&output, now);
            if let Some(confetti) = self.fx.confetti.as_mut() {
                confetti.advance(now, &ledges);
                if confetti.over(now) {
                    tracing::debug!("the confetti are over");
                    self.fx.confetti = None;
                    self.redraw = true;
                } else {
                    moving = true;
                }
            }
        }
        moving
    }
}
