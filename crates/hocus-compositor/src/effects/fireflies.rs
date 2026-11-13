//! Evening fireflies (#804, the board "Lucioles du soir", its JS ported):
//! after sunset, fireflies drift over the wallpaper on a slow flow field,
//! each blinking now and then; when the pointer rests on the wallpaper,
//! those near gather slowly and circle it. Drawn by
//! `crate::render::fireflies`: each a glow added to the wallpaper, and the
//! bright ones a wider glow screened over it (the board's front canvas,
//! which it screened over everything: here on the wallpaper alone).
//!
//! Stepped at a fixed 60 steps a second from their start (the board's
//! frames), with a seeded generator; as many per pixel as the board's 70
//! over the 1000 × 720 left of its bar. Their places are logical pixels
//! from the corner of their screen's work area (beside the bar).
//!
//! When is evening (`Dusk`): the sun below the horizon at the place of
//! the system's time zone ([`super::sun`]), looked at every minute on the
//! wall clock, as the user sees the evening.

use std::{cell::Cell, collections::HashMap, time::Duration};

use smithay::backend::renderer::{element::Id, utils::CommitCounter};

use super::{
    mesh::{Glow, Mesh, Shape},
    particles::{Rng, STEP},
    sun::sun_down_now,
};

/// The board's 70 over the 1000 × 720 its fireflies roam.
const FLIES_PER_PIXEL: f32 = 70.0 / (1000.0 * 720.0);
/// At most this many steps at once (10 s).
const MOST_STEPS: u64 = 600;
/// How long they take to come out.
const COMING_OUT: f32 = 3.0;
/// The pointer rests once it has not moved more than this (logical
/// pixels) for this long (seconds).
pub const REST_MOVE: f64 = 3.0;
pub const REST_AFTER: Duration = Duration::from_millis(800);
/// A firefly's brightness with reduced motion: still, lit this much.
const STILL_LIT: f32 = 0.6;

/// One firefly, from its zone's corner.
#[derive(Debug, Clone, Copy)]
pub struct Fly {
    pub x: f32,
    pub y: f32,
    vx: f32,
    vy: f32,
    seed: f32,
    next: f32,
    on: f32,
    period: f32,
    z: f32,
}

impl Fly {
    /// How lit it is, from 0 to 1: a blink rising and falling.
    fn lit(&self, moving: bool) -> f32 {
        if moving {
            (self.on.min(1.0) * std::f32::consts::PI)
                .sin()
                .max(0.0)
                .powf(0.7)
        } else {
            STILL_LIT
        }
    }
}

/// The board's flow field: where the air takes a firefly at `(x, y)`.
fn flow(x: f32, y: f32, t: f32) -> (f32, f32) {
    let a = (x * 0.006 + t * 0.21).sin() * 1.7
        + (y * 0.008 - t * 0.17).cos() * 1.3
        + ((x + y) * 0.003 + t * 0.11).sin();
    (a.cos(), a.sin() * 0.7)
}

/// The fireflies of one screen.
#[derive(Debug)]
pub struct Swarm {
    /// Where they roam: the screen's work area (global, logical: `x, y,
    /// w, h`).
    pub zone: [f32; 4],
    pub flies: Vec<Fly>,
    /// Their glows, and the blank standing for them behind the glass.
    pub ids: [Id; 2],
    generation: u64,
    commit: Cell<CommitCounter>,
    drawn: Cell<u64>,
}

impl Swarm {
    fn new(zone: [f32; 4], rng: &mut Rng) -> Self {
        let [_, _, w, h] = zone;
        let n = (w * h * FLIES_PER_PIXEL).round() as usize;
        let mut rnd = |a: f32, b: f32| a + rng.next() * (b - a);
        let flies = (0..n)
            .map(|_| Fly {
                x: rnd(0.0, w - 10.0),
                y: rnd(h * 0.3, h - 20.0),
                vx: 0.0,
                vy: 0.0,
                seed: rnd(0.0, 100.0),
                next: rnd(0.0, 4.0),
                on: 0.0,
                period: rnd(2.5, 5.5),
                z: rnd(0.4, 1.0),
            })
            .collect();
        Self {
            zone,
            flies,
            ids: [Id::new(), Id::new()],
            generation: 0,
            commit: Cell::new(CommitCounter::default()),
            drawn: Cell::new(u64::MAX),
        }
    }

    /// The board's frame, `dt` seconds on at `t`: `resting` the pointer
    /// at rest over the wallpaper, from the zone's corner.
    fn step(&mut self, dt: f32, t: f32, resting: Option<(f32, f32)>, rng: &mut Rng) {
        let [_, _, w, h] = self.zone;
        for f in &mut self.flies {
            let (fx, fy) = flow(f.x, f.y, t + f.seed);
            let (mut ax, mut ay) = (fx * 26.0, fy * 26.0);
            if let Some((rx, ry)) = resting {
                let (dx, dy) = (rx - f.x, ry - f.y);
                let d = dx.hypot(dy);
                let d = if d > 0.0 { d } else { 1.0 };
                if d < 360.0 {
                    let pull = if d > 70.0 { 40.0 } else { -10.0 };
                    ax += dx / d * pull - dy / d * 30.0;
                    ay += dy / d * pull + dx / d * 30.0;
                }
            }
            f.vx += (ax - f.vx) * dt * 1.4;
            f.vy += (ay - f.vy) * dt * 1.4;
            f.x += f.vx * dt * f.z;
            f.y += f.vy * dt * f.z;
            if f.x < -20.0 {
                f.x = w - 20.0;
            }
            if f.x > w {
                f.x = -10.0;
            }
            if f.y < h * 0.2 {
                f.vy += 20.0 * dt;
            }
            if f.y > h - 10.0 {
                f.vy -= 30.0 * dt;
            }
            f.next -= dt;
            if f.next <= 0.0 {
                f.on = 1.0;
                f.next = f.period * (0.7 + rng.next() * 0.6);
            }
            f.on = (f.on - dt * 1.3).max(0.0);
        }
        self.generation += 1;
    }

    /// The commit of this frame's drawing: a new one when they moved since
    /// they were last drawn.
    pub fn commit(&self) -> CommitCounter {
        let mut c = self.commit.get();
        if self.drawn.get() != self.generation {
            c.increment();
            self.commit.set(c);
            self.drawn.set(self.generation);
        }
        c
    }

    /// Their glows on a screen whose corner is `origin` (global, logical)
    /// at `scale`, as opaque as `alpha` (coming out): those added to the
    /// wallpaper, and those screened over it. Output-local, physical.
    pub fn glows(&self, origin: (f64, f64), scale: f32, alpha: f32, moving: bool) -> (Mesh, Mesh) {
        let (mut added, mut screened) = (Mesh::default(), Mesh::default());
        let (ox, oy) = (
            self.zone[0] - origin.0 as f32,
            self.zone[1] - origin.1 as f32,
        );
        for f in &self.flies {
            let lit = f.lit(moving);
            let centre = [(ox + f.x) * scale, (oy + f.y) * scale];
            let s = (14.0 + 26.0 * lit) * f.z;
            let a = (0.07 + lit * 0.9) * f.z * alpha;
            added.square(
                centre,
                [s * scale; 2],
                [1.0, 1.0],
                [a; 4],
                Shape::Glow,
                [Glow::Firefly as u8 as f32, 0.0, 0.0],
            );
            if lit > 0.3 {
                let a = (lit - 0.3) * 0.45 * f.z * alpha;
                let r = s * 2.6 * scale;
                screened.square(
                    centre,
                    [r; 2],
                    [1.0, 1.0],
                    [a; 4],
                    Shape::Glow,
                    [Glow::Firefly as u8 as f32, 0.0, 0.0],
                );
            }
        }
        (added, screened)
    }
}

/// The fireflies out on every screen they show on.
#[derive(Debug)]
pub struct Fireflies {
    start: Duration,
    steps: u64,
    rng: Rng,
    /// By screen name.
    pub swarms: HashMap<String, Swarm>,
    /// Where the pointer was last seen, and since when it is there.
    pub pointer: Option<((f64, f64), Duration)>,
}

impl Fireflies {
    pub fn new(start: Duration, seed: u64) -> Self {
        Self {
            start,
            steps: 0,
            rng: Rng::new(seed),
            swarms: HashMap::new(),
            pointer: None,
        }
    }

    /// How far they came out at `now`, from 0 to 1.
    pub fn out(&self, now: Duration) -> f32 {
        (now.saturating_sub(self.start).as_secs_f32() / COMING_OUT).min(1.0)
    }

    /// Screen `name`'s zone is `zone` (global, logical): its swarm, made
    /// when it is new or the zone's size changed (moved with it otherwise).
    pub fn swarm(&mut self, name: &str, zone: [f32; 4]) -> bool {
        if let Some(swarm) = self.swarms.get_mut(name)
            && swarm.zone[2..] == zone[2..]
        {
            if swarm.zone != zone {
                swarm.zone = zone;
                swarm.generation += 1;
            }
            return false;
        }
        let swarm = Swarm::new(zone, &mut self.rng);
        self.swarms.insert(name.to_owned(), swarm);
        true
    }

    /// Only the screens of `names` keep theirs.
    pub fn retain_swarms(&mut self, names: &[&str]) {
        self.swarms.retain(|n, _| names.contains(&n.as_str()));
    }

    /// Steps them to `now` on the screens of `on`, each with where the
    /// pointer rests on it (global, logical).
    pub fn advance(&mut self, now: Duration, on: &[(String, Option<(f64, f64)>)]) {
        let due = (now.saturating_sub(self.start).as_secs_f64() / f64::from(STEP)) as u64;
        let from = self.steps.max(due.saturating_sub(MOST_STEPS));
        for step in from..due {
            let t = step as f32 * STEP;
            for (name, resting) in on {
                if let Some(swarm) = self.swarms.get_mut(name) {
                    let resting =
                        resting.map(|(x, y)| (x as f32 - swarm.zone[0], y as f32 - swarm.zone[1]));
                    swarm.step(STEP, t, resting, &mut self.rng);
                }
            }
        }
        self.steps = self.steps.max(due);
    }
}

/// Whether it is evening, looked at every minute while the fireflies are
/// on ([`crate::State::watch_sun`]).
#[derive(Debug, Default)]
pub struct Dusk {
    pub evening: bool,
    /// Tests: evening or not, whatever the clock says.
    pub held: Option<bool>,
}

impl Dusk {
    pub fn evening(&self) -> bool {
        self.held.unwrap_or(self.evening)
    }
}

impl crate::State {
    /// How the fireflies show now (#804): their switch on and the evening
    /// come.
    pub(crate) fn fireflies_out(&self) -> Option<super::Plays> {
        self.wallpaper_effect(self.config.effects.fireflies && self.fx.dusk.evening())
    }

    /// The fireflies move on to `now` (#804): out as it is evening and
    /// they are on, a swarm over each screen's work area; gathering where
    /// the pointer rests on the wallpaper; stepped on the screens that are
    /// on and show no fullscreen window. Whether they move (a frame is
    /// wanted).
    pub(crate) fn tick_fireflies(&mut self, now: Duration) -> bool {
        self.watch_sun();
        let Some(plays) = self.fireflies_out() else {
            if self.config.effects.fireflies
                && self.fx.dusk.evening()
                && self.fx.fireflies.is_some()
            {
                // Waiting (the power saver, a share): kept, not drawn.
                return false;
            }
            if self.fx.fireflies.take().is_some() {
                tracing::debug!("the fireflies go");
                self.redraw = true;
            }
            return false;
        };
        if self.fx.fireflies.is_none() {
            self.fx.swarms += 1;
            tracing::debug!(?plays, "the fireflies come out");
            self.fx.fireflies = Some(Fireflies::new(now, self.fx.swarms));
            self.redraw = true;
        }
        let pointer = self
            .seat
            .get_pointer()
            .map(|p| p.current_location())
            .unwrap_or_default();
        let on_wallpaper = !self.locked() && self.surface_under(pointer).is_none();
        let pointer_screen = self.space.output_under(pointer).next().map(|o| o.name());
        let mut zones = Vec::new();
        let mut on = Vec::new();
        for output in self.space.outputs() {
            let Some(zone) = self.work_area(output) else {
                continue;
            };
            let name = output.name();
            if self.output_powered(output) && !self.fullscreen_in_front(output) {
                on.push(name.clone());
            }
            let zone = [zone.x, zone.y, zone.width, zone.height].map(|v| v as f32);
            zones.push((name, zone));
        }
        let Some(flies) = self.fx.fireflies.as_mut() else {
            return false;
        };
        let mut made = false;
        for (name, zone) in &zones {
            made |= flies.swarm(name, *zone);
        }
        let names: Vec<&str> = zones.iter().map(|(n, _)| n.as_str()).collect();
        flies.retain_swarms(&names);
        // At rest: not moved more than a few pixels for a moment, over
        // the wallpaper.
        let at = (pointer.x, pointer.y);
        let since = match flies.pointer {
            Some((last, since)) if (last.0 - at.0).hypot(last.1 - at.1) <= REST_MOVE => since,
            _ => now,
        };
        flies.pointer = Some((at, since));
        let resting = (on_wallpaper && now.saturating_sub(since) > REST_AFTER).then_some(at);
        let moving = plays == super::Plays::Moving && !on.is_empty();
        if moving {
            let on: Vec<(String, Option<(f64, f64)>)> = on
                .into_iter()
                .map(|name| {
                    let rest = resting.filter(|_| pointer_screen.as_deref() == Some(name.as_str()));
                    (name, rest)
                })
                .collect();
            flies.advance(now, &on);
        }
        if made {
            self.redraw = true;
        }
        moving
    }

    /// Whether it is evening now, on the wall clock.
    pub(super) fn dusk_look(&mut self) {
        let evening = sun_down_now();
        if evening != self.fx.dusk.evening {
            tracing::info!(evening, "the fireflies' evening");
            self.fx.dusk.evening = evening;
            self.redraw = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn they_drift_blink_and_gather_round_the_pointer_at_rest() {
        let mut rng = Rng::new(5);
        let mut swarm = Swarm::new([0.0, 0.0, 1000.0, 720.0], &mut rng);
        assert_eq!(swarm.flies.len(), 70);
        let rest = (500.0, 400.0);
        let near = |s: &Swarm| {
            s.flies
                .iter()
                .filter(|f| (f.x - rest.0).hypot(f.y - rest.1) < 140.0)
                .count()
        };
        let before = near(&swarm);
        let mut blinked = false;
        for step in 0..60 * 20 {
            swarm.step(STEP, step as f32 * STEP, Some(rest), &mut rng);
            blinked |= swarm.flies.iter().any(|f| f.lit(true) > 0.9);
        }
        assert!(blinked);
        assert!(near(&swarm) > before + 5, "{before} → {}", near(&swarm));
        // They stay within the zone (wrapping round its sides).
        assert!(swarm.flies.iter().all(|f| f.x >= -20.0 && f.x <= 1000.0));
    }
}
