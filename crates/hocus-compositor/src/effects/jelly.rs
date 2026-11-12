//! Windows in jelly (#797, the "Fenêtres en gelée" board of the effects
//! canvas): with `[effects] jelly = true`, a window dragged deforms on the
//! board's spring mesh in place of the standard wobble (`crate::wobble`,
//! which draws it): the point held follows the pointer, the rest follows
//! with a lag that grows away from it, then it settles in a few
//! oscillations once let go.
//!
//! The board's model, its lengths in logical pixels: 22 × 16 points even
//! over the window, each pulled toward its place on the rigid window by a
//! damped spring, stiffest near the hand (`22 + 1100 e^(-d/55)`, damped
//! `3.2 + 12 e^(-d/90)`), the points within 6 px of it pinned; let go, all
//! at 120 damped 5.5. The hand holds the window as the board's does, by
//! that stiffness falling off smoothly away from it: the window under it
//! follows the pointer closely, and nothing creases there. Stepped at the
//! board's pace, 180 steps a second (three a frame at 60 Hz), so that a
//! moment is the same whatever the frames' pace.
//!
//! Where the board keeps neighbours along a row near their spacing (8 %
//! of the gap a step, on their places), each point here is pulled toward
//! its four neighbours' displacements instead: the board's pull, with a
//! point lagging up or down, shortened the row and folded the sheet over
//! itself into a zig-zag; this one bends it smoothly.

use std::time::Duration;

/// Points across and down, the board's.
const ACROSS: usize = 22;
const DOWN: usize = 16;
/// The board's springs: let go, and held (at the hand, and how far from
/// it they soften, logical pixels).
const REST_STIFF: f64 = 120.0;
const REST_DAMP: f64 = 5.5;
const HELD_STIFF: (f64, f64, f64) = (22.0, 1100.0, 55.0);
const HELD_DAMP: (f64, f64, f64) = (3.2, 12.0, 90.0);
/// Points this near the hand go with it, logical pixels.
const PINNED: f64 = 6.0;
/// How hard each point is pulled toward its neighbours' displacements,
/// per second squared.
const SHEET: f64 = 300.0;
/// The step, the board's third of a 60 Hz frame; the longest time caught
/// up in one frame.
const STEP: Duration = Duration::from_nanos(5_555_556);
const CATCH_UP: Duration = Duration::from_millis(50);
/// No point strays further than the window's larger side: a fling as
/// hard as it gets never folds it out of sight.
const MAX_BEND: f64 = 1.0;

/// A window's jelly: each point's displacement from its place on the
/// rigid window, and its velocity, logical.
#[derive(Debug)]
pub struct Gel {
    /// Where the columns and the rows are on the window, from its corner.
    xs: [f64; ACROSS],
    ys: [f64; DOWN],
    d: Vec<(f64, f64)>,
    v: Vec<(f64, f64)>,
    /// Each point's spring while held: its stiffness and damping, and
    /// whether the hand pins it.
    held: Vec<(f64, f64, bool)>,
    holding: bool,
    reach: f64,
    last: Duration,
}

/// `N` places evenly from 0 to `len`.
fn even<const N: usize>(len: f64) -> [f64; N] {
    std::array::from_fn(|i| len * i as f64 / (N - 1) as f64)
}

impl Gel {
    /// A window `size` large held at `held` (from its corner, logical).
    pub fn new(size: (f64, f64), held: (f64, f64), now: Duration) -> Self {
        let (w, h) = (size.0.max(1.0), size.1.max(1.0));
        let xs = even::<ACROSS>(w);
        let ys = even::<DOWN>(h);
        let mut springs = Vec::with_capacity(ACROSS * DOWN);
        for y in ys {
            for x in xs {
                let d = (x - held.0).hypot(y - held.1);
                springs.push((
                    HELD_STIFF.0 + HELD_STIFF.1 * (-d / HELD_STIFF.2).exp(),
                    HELD_DAMP.0 + HELD_DAMP.1 * (-d / HELD_DAMP.2).exp(),
                    d < PINNED,
                ));
            }
        }
        Self {
            xs,
            ys,
            d: vec![(0.0, 0.0); ACROSS * DOWN],
            v: vec![(0.0, 0.0); ACROSS * DOWN],
            held: springs,
            holding: true,
            reach: w.max(h) * MAX_BEND,
            last: now,
        }
    }

    /// The window moved by (`dx`, `dy`): its points stay where they were,
    /// the springs bring them along; the pinned ones go with the hand.
    pub fn moved(&mut self, dx: f64, dy: f64, now: Duration) {
        self.advance(now);
        for (d, (_, _, pinned)) in self.d.iter_mut().zip(&self.held) {
            if !(self.holding && *pinned) {
                d.0 -= dx;
                d.1 -= dy;
            }
        }
    }

    /// Let go: every spring the same, it settles.
    pub fn release(&mut self) {
        self.holding = false;
    }

    pub fn is_held(&self) -> bool {
        self.holding
    }

    /// The springs, up to `now`.
    pub fn advance(&mut self, now: Duration) {
        let mut left = now.saturating_sub(self.last).min(CATCH_UP);
        self.last = now;
        while !left.is_zero() {
            let dt = left.min(STEP);
            left -= dt;
            self.step(dt.as_secs_f64());
        }
    }

    #[expect(
        clippy::indexing_slicing,
        reason = "the points' vectors hold ACROSS × DOWN entries; k and its neighbours are \
                  points of the grid"
    )]
    fn step(&mut self, h: f64) {
        let last = self.d.clone();
        for k in 0..self.d.len() {
            let (stiff, damp, pinned) = if self.holding {
                self.held[k]
            } else {
                (REST_STIFF, REST_DAMP, false)
            };
            if pinned {
                self.d[k] = (0.0, 0.0);
                self.v[k] = (0.0, 0.0);
                continue;
            }
            let (i, j) = (k % ACROSS, k / ACROSS);
            let (dx, dy) = last[k];
            // Each point pulled toward its neighbours' displacements: the
            // sheet bends smoothly, never folds.
            let mut pull = (0.0, 0.0);
            let neighbours = [
                (i > 0).then(|| k - 1),
                (i + 1 < ACROSS).then(|| k + 1),
                (j > 0).then(|| k - ACROSS),
                (j + 1 < DOWN).then(|| k + ACROSS),
            ];
            for n in neighbours.into_iter().flatten() {
                pull.0 += last[n].0 - dx;
                pull.1 += last[n].1 - dy;
            }
            let (vx, vy) = self.v[k];
            let vx = vx + (-dx * stiff + pull.0 * SHEET - vx * damp) * h;
            let vy = vy + (-dy * stiff + pull.1 * SHEET - vy * damp) * h;
            let r = self.reach;
            self.v[k] = (vx, vy);
            self.d[k] = ((dx + vx * h).clamp(-r, r), (dy + vy * h).clamp(-r, r));
        }
    }

    /// Every point at its place, and still (held or not): drawn as the
    /// window is, straight.
    pub fn still(&self, rest: f64, rest_speed: f64) -> bool {
        self.d
            .iter()
            .zip(&self.v)
            .all(|(d, v)| d.0.hypot(d.1) < rest && v.0.hypot(v.1) < rest_speed)
    }

    /// The largest displacement, logical.
    pub fn largest(&self) -> f64 {
        self.d.iter().map(|d| d.0.hypot(d.1)).fold(0.0, f64::max)
    }

    /// The displacement at (`x`, `y`) from the window's corner, logical,
    /// between the grid's points; beyond its edges, the edge's.
    #[expect(
        clippy::indexing_slicing,
        reason = "i < ACROSS - 1 and j < DOWN - 1: the four corners are points of the grid"
    )]
    pub fn at(&self, x: f64, y: f64) -> (f64, f64) {
        let cell = |cuts: &[f64], p: f64| {
            let last = cuts.len() - 2;
            let i = cuts
                .iter()
                .skip(1)
                .position(|c| p < *c)
                .unwrap_or(last)
                .min(last);
            let span = cuts[i + 1] - cuts[i];
            let f = if span > 0.0 {
                ((p - cuts[i]) / span).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (i, f)
        };
        let (i, fu) = cell(&self.xs, x);
        let (j, fv) = cell(&self.ys, y);
        let a = self.d[j * ACROSS + i];
        let b = self.d[j * ACROSS + i + 1];
        let c = self.d[(j + 1) * ACROSS + i];
        let d = self.d[(j + 1) * ACROSS + i + 1];
        let mix = |a: f64, b: f64, c: f64, d: f64| {
            (1.0 - fu) * (1.0 - fv) * a + fu * (1.0 - fv) * b + (1.0 - fu) * fv * c + fu * fv * d
        };
        (mix(a.0, b.0, c.0, d.0), mix(a.1, b.1, c.1, d.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// The board's demo: held 120 px in on the title, shaken sideways; the
    /// point held follows, the far corner lags; let go, it swings past
    /// its place, and lies still within 2.5 s.
    #[test]
    fn held_it_follows_the_hand_and_let_go_it_settles_in_a_few_swings() {
        let mut gel = Gel::new((480.0, 340.0), (120.0, 14.0), ms(0));
        let mut x = 0.0;
        for t in 1..=40u64 {
            let to = (t as f64 * 0.0167 * 3.2).sin() * 230.0;
            gel.moved(to - x, 0.0, ms(t * 17));
            x = to;
        }
        // Under the hand it follows closely (the board's weighting, no
        // point pinned there), the far corner far behind.
        let (held, _) = gel.at(120.0, 14.0);
        let (far, _) = gel.at(480.0, 340.0);
        assert!(far.abs() > 30.0, "the far corner lags: {far}");
        assert!(
            held.abs() < far.abs() / 4.0,
            "under the hand {held}, far {far}"
        );
        // Smooth along the top edge round the hand: no crease between
        // neighbours.
        let top: Vec<f64> = (0..=48)
            .map(|i| gel.at(f64::from(i) * 10.0, 0.0).1)
            .collect();
        let bend = top
            .windows(3)
            .map(|w| (w[0] - 2.0 * w[1] + w[2]).abs())
            .fold(0.0, f64::max);
        assert!(bend < 3.0, "a crease on the top edge: {bend}");
        gel.release();
        let mut sign = far.signum();
        let mut swings = 0;
        let mut settled = None;
        for step in 1..=300u64 {
            let now = ms(680 + step * 10);
            gel.advance(now);
            let (x, _) = gel.at(480.0, 340.0);
            if x.abs() > 1.0 && x.signum() != sign {
                sign = x.signum();
                swings += 1;
            }
            if settled.is_none() && gel.still(0.5, 6.0) {
                settled = Some(step * 10);
            }
        }
        assert!(swings >= 2, "a few swings: {swings}");
        let settled = settled.unwrap_or(9999);
        assert!(settled <= 2500, "still after {settled} ms");
    }
}
