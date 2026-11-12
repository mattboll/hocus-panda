//! Closing a window in flames (#794, the "Fermer en flammes" board of the
//! effects canvas): with `[effects] flames = true`, a closing window keeps
//! its last picture and burns from the bottom up in 1.7 s, in place of the
//! closing animation. Its shader ([`super::draw`], `BURN`) draws the burn
//! from noise rising from the bottom: the paper scorching ahead of the
//! front, a glowing rim on it, charred flakes behind, flames rising over
//! the paper still there, a heat wobble, a soft shadow fading fast. Embers
//! and smoke ([`super::particles`]) rise from the front, and a warm light
//! falls on the desk around it while it burns.
//!
//! The window is gone from the desktop as it starts (its picture alone
//! burns): input never reaches it. Once the burn and its last particles
//! are over, nothing is drawn.

use std::{
    cell::{Cell, RefCell},
    time::Duration,
};

use smithay::{
    backend::renderer::{element::Id, utils::CommitCounter},
    utils::{Logical, Rectangle},
};

use super::particles::{Look, Particle, Particles, Rng};
use crate::wobble::Kept;

/// The burn, from its first spark to the last of the paper.
pub const BURN: Duration = Duration::from_millis(1700);
/// The embers left are swept away this long after the burn (the board's
/// "gone" phase); the smoke drifts out on its own.
const EMBERS_GONE: Duration = Duration::from_millis(2800);
/// Whatever the frames drawn, nothing lingers after this.
pub const LONGEST: Duration = Duration::from_millis(5000);
/// Around the window, logical pixels: room for its shadow on the sides
/// and below, and for the flames above it (2.2 times as much).
pub const MARGIN: f64 = 60.0;
pub const MARGIN_ABOVE: f64 = MARGIN * 2.2;
/// The warm light's reach, logical pixels.
pub const LIGHT: f64 = 620.0;

/// A window burning.
#[derive(Debug)]
pub struct Burn {
    pub start: Duration,
    /// Where the window was, global and logical.
    pub rect: Rectangle<f64, Logical>,
    /// Added to the shader's clock: each burn its own flames.
    pub seed: f32,
    /// The window's last picture, drawn once, the renderer's.
    pub picture: RefCell<Option<Kept>>,
    pub sparks: RefCell<Particles>,
    /// The burn, its sparks and its light: the same ids over its frames.
    pub ids: [Id; 3],
    commits: Cell<CommitCounter>,
}

impl Burn {
    pub fn new(start: Duration, rect: Rectangle<f64, Logical>, seed: u32) -> Self {
        Self {
            start,
            rect,
            seed: (seed % 97) as f32 * 3.7,
            picture: RefCell::new(None),
            sparks: RefCell::new(Particles::new(u64::from(seed) + 1)),
            ids: std::array::from_fn(|_| Id::new()),
            commits: Cell::new(CommitCounter::default()),
        }
    }

    fn elapsed(&self, now: Duration) -> Duration {
        now.saturating_sub(self.start)
    }

    /// How far the burn is, from 0 to 1.
    pub fn progress(&self, now: Duration) -> f32 {
        (self.elapsed(now).as_secs_f32() / BURN.as_secs_f32()).clamp(0.0, 1.0)
    }

    /// The burn's front as the shader takes it: a little slower at first,
    /// as the board's.
    pub fn front(&self, now: Duration) -> f32 {
        eased(self.progress(now))
    }

    /// The shader's clock, seconds.
    pub fn time(&self, now: Duration) -> f32 {
        self.elapsed(now).as_secs_f32() + self.seed
    }

    /// The paper still burns (its picture is drawn).
    pub fn burning(&self, now: Duration) -> bool {
        self.elapsed(now) < BURN
    }

    /// How bright the fire is: rising then falling over the burn.
    pub fn fire(&self, now: Duration) -> f32 {
        let t = self.progress(now);
        if t > 0.0 && t < 1.0 {
            (t * std::f32::consts::PI).sin()
        } else {
            0.0
        }
    }

    /// The warm light's flicker at `now`.
    pub fn flicker(&self, now: Duration) -> f32 {
        let time = self.time(now);
        0.85 + 0.15 * (time * 23.0).sin() * (time * 7.3).sin()
    }

    /// The front's height, global and logical.
    pub fn front_y(&self, now: Duration) -> f64 {
        let r = self.rect;
        r.loc.y + r.size.h - (f64::from(self.front(now)) * 1.55 - 0.25) * r.size.h
    }

    /// Steps the embers and the smoke to `now`.
    pub fn advance(&self, now: Duration) {
        let rect = self.rect;
        let mut sparks = self.sparks.borrow_mut();
        let elapsed = self.elapsed(now);
        sparks.advance(elapsed, |at, rng, out| spawn(rect, at, rng, out));
        if elapsed >= EMBERS_GONE {
            sparks.clear(Look::Ember);
        }
    }

    /// Over at `now`: burnt, and its sparks gone.
    pub fn done(&self, now: Duration) -> bool {
        let elapsed = self.elapsed(now);
        elapsed >= LONGEST || (elapsed >= BURN && self.sparks.borrow().is_empty())
    }

    /// The commit of this frame's drawing: a new one each frame.
    pub fn next_commit(&self) -> CommitCounter {
        let mut c = self.commits.get();
        c.increment();
        self.commits.set(c);
        c
    }
}

/// The board's `ease`: smoothstep's share 35 %, linear's 65 %.
fn eased(t: f32) -> f32 {
    if t < 1.0 {
        t * t * (3.0 - 2.0 * t) * 0.35 + t * 0.65
    } else {
        1.0
    }
}

/// What is born in the step at `at` seconds of a burn over `rect`: seven
/// embers along the front, and a puff of smoke above it seven times in
/// ten, while it burns.
fn spawn(rect: Rectangle<f64, Logical>, at: f32, rng: &mut Rng, out: &mut Vec<Particle>) {
    let t = at / BURN.as_secs_f32();
    if t <= 0.0 || t >= 1.0 {
        return;
    }
    let (x, y, w, h) = (
        rect.loc.x as f32,
        rect.loc.y as f32,
        rect.size.w as f32,
        rect.size.h as f32,
    );
    let front = y + h - (eased(t) * 1.55 - 0.25) * h;
    for _ in 0..7 {
        let ex = x + 12.0 + rng.next() * (w - 24.0).max(0.0);
        let ey = (front + (rng.next() - 0.5) * 60.0).clamp(y - 10.0, y + h);
        let vx = (rng.next() - 0.5) * 40.0;
        let vy = -60.0 - rng.next() * 140.0;
        let max = 0.8 + rng.next() * 1.4;
        let size = 0.8 + rng.next() * 1.8;
        out.push(Particle {
            x: ex,
            y: ey,
            vx,
            vy,
            lift: -20.0,
            sway: 60.0,
            size,
            grow: 0.0,
            life: 0.0,
            max,
            look: Look::Ember,
        });
    }
    if rng.next() < 0.7 {
        let sx = x + 30.0 + rng.next() * (w - 60.0).max(0.0);
        let sy = (y - 20.0).max(front - 20.0);
        let r = 18.0 + rng.next() * 20.0;
        let max = 2.2 + rng.next();
        let vx = (rng.next() - 0.3) * 18.0;
        out.push(Particle {
            x: sx,
            y: sy,
            vx,
            vy: -34.0,
            lift: 0.0,
            sway: 0.0,
            size: r,
            grow: 16.0,
            life: 0.0,
            max,
            look: Look::Smoke,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn it_burns_then_its_sparks_die_and_it_is_over() {
        let rect = Rectangle::new((100.0, 100.0).into(), (600.0, 400.0).into());
        let burn = Burn::new(ms(1000), rect, 3);
        assert!(burn.burning(ms(1000)) && !burn.done(ms(1000)));
        burn.advance(ms(1850));
        let alive = burn.sparks.borrow().list.len();
        assert!(alive > 100, "{alive} sparks halfway");
        assert!(burn.fire(ms(1850)) > 0.9);
        // The front climbs from below the window to above it.
        assert!(burn.front_y(ms(1000)) > rect.loc.y + rect.size.h);
        assert!(burn.front_y(ms(2700)) < rect.loc.y);
        burn.advance(ms(2800));
        assert!(
            !burn.burning(ms(2800)) && !burn.done(ms(2800)),
            "sparks left"
        );
        // The last puff, born as the burn ends, lives 3.2 s at most.
        burn.advance(ms(5950));
        assert!(burn.done(ms(5950)));
        assert!(Burn::new(ms(0), rect, 3).done(LONGEST));
    }
}
