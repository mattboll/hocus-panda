//! Desktops in depth (#806, docs/design/effects.md): the wallpaper is the
//! board "Bureaux en profondeur"'s landscape, and switching desktops is a
//! camera move through it. Its sky barely drifts, far mountains barely
//! move, the forests slide, the nearest trees pass faster than the
//! windows: each layer moves by its share of the camera's travel (the
//! board's 0.03, 0.14, 0.38, 0.72, 1 and 1.55), the windows by one, and
//! all of it pulls back a little on the way (its layers by their share of
//! it too).
//!
//! The camera follows each screen's strip of desktops (`crate::desktops`):
//! where it stands, at the board's pace while depth is on
//! ([`crate::desktops::Slide::Depth`]). The landscape is drawn by one
//! shader into a picture of the screen's size (`draw.rs`, `DEPTH`), again
//! only when the camera moves, at most 60 times a second (the effects'
//! frames, `effects::Pace`): at rest nothing
//! is drawn per frame.
//!
//! The board drew its nearest row of trees on a canvas in front of its
//! windows. Here it stays in front of the landscape's other layers but
//! behind the windows and the bar: in front, it would cover what the
//! windows show (#434's rule).

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    time::Duration,
};

use smithay::{
    backend::renderer::{element::Id, utils::CommitCounter},
    output::Output,
};

use super::Plays;
use crate::wobble::Kept;

/// The board's sky drifts this far (logical pixels) over the strip's
/// whole travel (`-cam / SPAN × 200 × 0.3`).
const SKY_DRIFT: f64 = 60.0;

/// Where a screen's camera stands: its travel along the strip (logical
/// pixels, one desktop a screen's width and the gap between two), how far
/// it stands back (1 at rest), and the sky's drift (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub x: f64,
    pub zoom: f64,
    pub drift: f64,
}

/// What a landscape's picture shows: the camera (its floats' bits), and
/// its size (physical).
type Drawn = ([u32; 3], (i32, i32));

/// The landscape of one screen, drawn as the camera moves.
#[derive(Debug)]
pub struct Pane {
    pub id: Id,
    /// Its picture, kept between frames.
    pub kept: RefCell<Option<Kept>>,
    /// What its picture shows.
    pub drawn: Cell<Option<Drawn>>,
    pub commit: Cell<CommitCounter>,
}

impl Default for Pane {
    fn default() -> Self {
        Self {
            id: Id::new(),
            kept: RefCell::new(None),
            drawn: Cell::new(None),
            commit: Cell::new(CommitCounter::default()),
        }
    }
}

/// Desktops in depth, while the switch is on.
#[derive(Debug, Default)]
pub struct Depth {
    /// The landscapes, by screen.
    pub panes: HashMap<String, Pane>,
}

/// The board's layers' phases: its generator (Park and Miller's, seeded
/// 7), three a layer, times 9.
pub fn phases() -> [[f32; 3]; 6] {
    let mut seed: u64 = 7;
    let mut rand = || {
        seed = (seed * 16807) % 2_147_483_647;
        seed as f64 / 2_147_483_647.0
    };
    let mut out = [[0.0; 3]; 6];
    for layer in &mut out {
        for p in layer.iter_mut() {
            *p = (rand() * 9.0) as f32;
        }
    }
    out
}

impl crate::State {
    /// How desktops in depth show now: moving, still with reduced motion
    /// (the landscape stands where the desktop shown puts it), or not at
    /// all.
    pub(crate) fn depth_plays(&self) -> Option<Plays> {
        self.wallpaper_effect(self.config.effects.depth)
    }

    /// Whether a switch of desktops is the camera's move now.
    pub(crate) fn depth_moves(&self) -> bool {
        self.depth_plays() == Some(Plays::Moving)
    }

    /// Whether the landscape is the wallpaper of `output`: depth plays,
    /// the wallpaper shows there, and the koi's pond does not take its
    /// place (the pond first, both on).
    pub(crate) fn depth_shows_on(&self, output: &Output) -> bool {
        self.depth_plays().is_some()
            && self.fx.depth.is_some()
            && !self.wall_hidden(output)
            && !self.koi_shows_on(output)
    }

    /// Where `output`'s camera stands at `now`; moving, as of the
    /// effects' last frame ([`super::Pace`], #815), so that the landscape
    /// is drawn again at most 60 times a second.
    pub(crate) fn depth_camera(&self, output: &Output, now: Duration) -> Camera {
        let strip = self.strip(output);
        let at = match strip.moves_until() {
            Some(end) if now < end => self.fx.pace.clock.min(now),
            _ => now,
        };
        let stride = self.strip_stride(output);
        let x = strip.position(at) * stride;
        let span = (strip.count().saturating_sub(1)) as f64 * stride;
        Camera {
            x,
            zoom: strip.pull(at),
            drift: if span > 0.0 {
                x / span * SKY_DRIFT
            } else {
                0.0
            },
        }
    }

    /// How much `output`'s windows are drawn pulled back at `now` (1: as
    /// they are), while its camera moves; on the screen alone.
    pub(crate) fn depth_pull(&self, output: &Output, now: Duration) -> f64 {
        if !self.depth_shows_on(output) {
            return 1.0;
        }
        self.strip(output).pull(now)
    }

    /// Depth follows its switch: on, a landscape for each screen; off,
    /// gone. In the power saver or while a screen is shared it waits,
    /// kept. Whether a camera moves at `now` (the effects' frames go on).
    pub(crate) fn tick_depth(&mut self, now: Duration) -> bool {
        match (self.depth_plays().is_some(), self.fx.depth.is_some()) {
            (true, false) => {
                tracing::debug!("desktops in depth");
                self.fx.depth = Some(Depth::default());
                self.redraw = true;
            }
            (false, true) if !self.config.effects.depth => {
                tracing::debug!("desktops no longer in depth");
                self.fx.depth = None;
                self.redraw = true;
            }
            _ => {}
        }
        if let Some(depth) = self.fx.depth.as_mut() {
            let names: Vec<String> = self.space.outputs().map(Output::name).collect();
            depth.panes.retain(|n, _| names.contains(n));
            for name in names {
                depth.panes.entry(name).or_default();
            }
        }
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        outputs.iter().any(|o| {
            self.depth_shows_on(o) && self.strip(o).moves_until().is_some_and(|end| now < end)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktops::{DEPTH_SLIDE, Slide, Strip};

    #[test]
    fn the_board_s_phases_from_its_seed() {
        let p = phases();
        // 7 × 16807 / (2³¹ - 1) × 9.
        assert!((f64::from(p[0][0]) - 117_649.0 / 2_147_483_647.0 * 9.0).abs() < 1e-6);
        assert!(p.iter().flatten().all(|v| (0.0..9.0).contains(v)));
    }

    #[test]
    fn a_switch_in_depth_takes_the_board_s_time_and_pulls_back_halfway() {
        let ms = Duration::from_millis;
        let mut strip = Strip::new(4);
        strip.switch_by(1, ms(0), Slide::Depth);
        assert_eq!(strip.moves_until(), Some(DEPTH_SLIDE));
        // In-out: slow away, halfway at half time, pulled back the most.
        assert!(strip.position(ms(150)) < 0.02);
        assert!((strip.position(ms(750)) - 0.5).abs() < 0.01);
        assert!((strip.pull(ms(750)) - 0.9).abs() < 1e-6);
        assert_eq!(strip.pull(ms(1500)), 1.0);
        assert_eq!(strip.position(ms(1500)), 1.0);
        // A standard slide does not pull back.
        let mut strip = Strip::new(4);
        strip.switch_by(1, ms(0), Slide::Standard);
        assert_eq!(strip.pull(ms(160)), 1.0);
    }
}
