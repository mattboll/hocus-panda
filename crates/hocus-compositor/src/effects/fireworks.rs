//! Fireworks when an agent's tests go green (#810, the board "Tests au
//! vert", its JS ported): seven rockets rise behind the windows over two
//! seconds and burst, peonies, willows and rings, their sparks falling
//! and fading, a peony's crackling at their end; each burst's flash
//! lights the wallpaper and the windows' edges. Once, for some seven
//! seconds, then nothing.
//!
//! Stepped at a fixed 60 steps a second from its start (the board's
//! frames), with a seeded generator. Its places are logical pixels from
//! the corner of its zone: the work area of the screen it plays on (the
//! board's 1200 × 720 but its bar), its rockets' heights scaled with it.
//! Drawn by `crate::render::moments`: the sparks and the rockets' glows
//! added to the wallpaper, the flashes screened over it; where it plays
//! is its zone alone, so that the bar's moment when a window is
//! maximised (#800) can give it another.

use std::{cell::RefCell, time::Duration};

use smithay::backend::renderer::element::Id;

use super::{
    kept::Traced,
    mesh::{Glow, Mesh, Shape},
    particles::{Rng, STEP},
};

/// Over by then whatever happens: its last spark is gone within 7 s.
pub const LAST: Duration = Duration::from_secs(10);
/// The board's stage, its rockets' heights' measure.
const BOARD_H: f32 = 720.0;
/// A rocket's gravity, px/s².
const LIFT: f32 = 520.0;
/// How long a flash lasts, and how far its light reaches (px).
const FLASH: f32 = 0.9;
pub const FLASH_REACH: f32 = 700.0;
/// How bright a flash is at its start.
const FLASH_LIGHT: f32 = 0.32;
/// How long a spark's streak is: the board's sky fading under it by 24 %
/// a frame leaves its last few frames' strokes, some 6 of them seen.
const TRAIL: f32 = 6.0 / 60.0;
/// At most this many steps at once, after a long pause.
const MOST_STEPS: u64 = 600;

/// The board's palettes, two colours each (0 to 255).
const PALETTES: [[[f32; 3]; 2]; 5] = [
    [[255.0, 214.0, 120.0], [255.0, 170.0, 60.0]],
    [[120.0, 255.0, 170.0], [60.0, 220.0, 140.0]],
    [[150.0, 190.0, 255.0], [90.0, 140.0, 255.0]],
    [[255.0, 130.0, 170.0], [255.0, 80.0, 120.0]],
    [[200.0, 160.0, 255.0], [150.0, 110.0, 255.0]],
];
/// A rocket's glow, and the sparks it leaves.
const ROCKET: [f32; 3] = [255.0, 220.0, 170.0];
const TRAIL_SPARK: [f32; 3] = [255.0, 200.0, 140.0];
/// A peony's spark crackling.
const CRACKLE: [f32; 3] = [255.0, 250.0, 230.0];

/// How a rocket bursts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Peony,
    Willow,
    Ring,
}

/// The board's show: across its stage (a share of its width), the apex
/// (px from its top), the kind, the palette, when it leaves (s).
const PLAN: [(f32, f32, Kind, usize, f32); 7] = [
    (0.30, 170.0, Kind::Peony, 0, 0.0),
    (0.58, 120.0, Kind::Willow, 0, 0.35),
    (0.44, 200.0, Kind::Ring, 2, 0.7),
    (0.72, 180.0, Kind::Peony, 1, 1.05),
    (0.22, 140.0, Kind::Ring, 4, 1.4),
    (0.50, 90.0, Kind::Willow, 0, 1.9),
    (0.64, 150.0, Kind::Peony, 3, 2.2),
];

#[derive(Debug, Clone)]
struct Rocket {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    kind: Kind,
    palette: usize,
    /// When it leaves, seconds from the show's start.
    at: f32,
}

#[derive(Debug, Clone)]
struct Spark {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    life: f32,
    max: f32,
    colour: [f32; 3],
    drag: f32,
    gravity: f32,
    crackle: bool,
    /// A rocket's trail's: smaller, fainter, no glow.
    small: bool,
    /// Its crackle's flicker this step: bright or out.
    flicker: f32,
}

#[derive(Debug, Clone)]
struct Flash {
    x: f32,
    y: f32,
    colour: [f32; 3],
    life: f32,
}

/// The fireworks, while they play.
#[derive(Debug)]
pub struct Show {
    start: Duration,
    steps: u64,
    rng: Rng,
    /// The screen it plays on.
    pub output: String,
    /// Where (global, logical: `x, y, w, h`).
    pub zone: [f32; 4],
    rockets: Vec<Rocket>,
    sparks: Vec<Spark>,
    flashes: Vec<Flash>,
    /// The blank standing for it behind the glass.
    pub blank: Id,
    /// What it drew last (#815).
    pub drawn: RefCell<Traced>,
}

impl Show {
    /// The board's show over `zone` (global, logical) of screen `output`,
    /// from `start`.
    pub fn new(start: Duration, seed: u64, output: String, zone: [f32; 4]) -> Self {
        let mut rng = Rng::new(seed);
        let [_, _, w, h] = zone;
        let tall = h / BOARD_H;
        let mut rnd = |a: f32, b: f32| a + rng.next() * (b - a);
        let rockets = PLAN
            .iter()
            .map(|&(across, apex, kind, palette, at)| {
                let apex = (apex + rnd(-30.0, 30.0)) * tall;
                Rocket {
                    x: w * across + rnd(-40.0, 40.0),
                    y: h + 10.0,
                    vx: rnd(-20.0, 20.0),
                    vy: -(2.0 * LIFT * (h - apex).max(0.0)).sqrt(),
                    kind,
                    palette,
                    at,
                }
            })
            .collect();
        Self {
            start,
            steps: 0,
            rng,
            output,
            zone,
            rockets,
            sparks: Vec::new(),
            flashes: Vec::new(),
            blank: Id::new(),
            drawn: RefCell::default(),
        }
    }

    /// Whether it is over at `now`: nothing left to show, or its time
    /// gone.
    pub fn over(&self, now: Duration) -> bool {
        (self.rockets.is_empty() && self.sparks.is_empty() && self.flashes.is_empty())
            || now.saturating_sub(self.start) > LAST
    }

    /// Steps it to `now`.
    pub fn advance(&mut self, now: Duration) {
        let due = (now.saturating_sub(self.start).as_secs_f64() / f64::from(STEP)) as u64;
        let from = self.steps.max(due.saturating_sub(MOST_STEPS));
        for step in from..due {
            self.step(STEP, (step + 1) as f32 * STEP);
        }
        self.steps = self.steps.max(due);
    }

    /// The board's frame, `dt` seconds on, at `t` from the start.
    fn step(&mut self, dt: f32, t: f32) {
        let rng = &mut self.rng;
        let mut burst = Vec::new();
        let sparks = &mut self.sparks;
        self.rockets.retain_mut(|r| {
            if t < r.at {
                return true;
            }
            r.vy += LIFT * dt;
            r.x += r.vx * dt;
            r.y += r.vy * dt;
            for _ in 0..2 {
                let mut rnd = |a: f32, b: f32| a + rng.next() * (b - a);
                sparks.push(Spark {
                    x: r.x + rnd(-1.5, 1.5),
                    y: r.y + rnd(0.0, 4.0),
                    vx: rnd(-25.0, 25.0),
                    vy: rnd(20.0, 70.0),
                    life: 0.0,
                    max: rnd(0.25, 0.5),
                    colour: TRAIL_SPARK,
                    drag: 3.0,
                    gravity: 60.0,
                    crackle: false,
                    small: true,
                    flicker: 1.0,
                });
            }
            if r.vy >= -10.0 {
                burst.push(r.clone());
                return false;
            }
            true
        });
        for r in &burst {
            self.burst(r);
        }
        self.sparks.retain_mut(|s| {
            s.life += dt;
            s.life < s.max
        });
        for s in &mut self.sparks {
            let d = (-s.drag * dt).exp();
            s.vx *= d;
            s.vy = s.vy * d + s.gravity * dt;
            s.x += s.vx * dt;
            s.y += s.vy * dt;
            if s.crackle {
                s.flicker = if self.rng.next() < 0.5 { 1.6 } else { 0.1 };
            }
        }
        self.flashes.retain_mut(|f| {
            f.life += dt;
            f.life < FLASH
        });
    }

    /// Rocket `r` bursts where it is.
    fn burst(&mut self, r: &Rocket) {
        let rng = &mut self.rng;
        let mut rnd = |a: f32, b: f32| a + rng.next() * (b - a);
        let [c1, c2] = PALETTES.get(r.palette).copied().unwrap_or(PALETTES[0]);
        let (n, speed) = match r.kind {
            Kind::Willow => (110, 200.0),
            Kind::Ring => (90, 260.0),
            Kind::Peony => (150, 300.0),
        };
        let tilt = rnd(0.35, 0.75);
        let turn = rnd(0.0, std::f32::consts::PI);
        for i in 0..n {
            let (vx, vy) = match r.kind {
                Kind::Ring => {
                    let a = i as f32 / n as f32 * std::f32::consts::TAU;
                    let (ex, ey) = (a.cos() * speed, a.sin() * speed * tilt);
                    (
                        ex * turn.cos() - ey * turn.sin(),
                        ex * turn.sin() + ey * turn.cos(),
                    )
                }
                _ => {
                    let a = rnd(0.0, std::f32::consts::TAU);
                    let z = rnd(-1.0, 1.0);
                    let m = (1.0 - z * z).sqrt() * speed * (0.85 + rnd(0.0, 0.25));
                    (a.cos() * m, a.sin() * m)
                }
            };
            let willow = r.kind == Kind::Willow;
            let max = if willow { rnd(2.2, 3.0) } else { rnd(1.2, 1.8) };
            let colour = if rnd(0.0, 1.0) < 0.5 { c1 } else { c2 };
            let crackle = r.kind == Kind::Peony && rnd(0.0, 1.0) < 0.35;
            self.sparks.push(Spark {
                x: r.x,
                y: r.y,
                vx,
                vy,
                life: 0.0,
                max,
                colour,
                drag: if willow { 2.2 } else { 1.35 },
                gravity: if willow { 90.0 } else { 140.0 },
                crackle,
                small: false,
                flicker: 1.0,
            });
        }
        self.flashes.push(Flash {
            x: r.x,
            y: r.y,
            colour: c1,
            life: 0.0,
        });
    }

    /// Its shapes on a screen whose corner is `origin` (global, logical)
    /// at `scale`, output-local and physical: the sparks and the rockets
    /// (added), the flashes (screened).
    pub fn shapes(&self, origin: (f64, f64), scale: f32) -> (Mesh, Mesh) {
        let (mut added, mut screened) = (Mesh::default(), Mesh::default());
        let (ox, oy) = (
            self.zone[0] - origin.0 as f32,
            self.zone[1] - origin.1 as f32,
        );
        let at = |x: f32, y: f32| [(ox + x) * scale, (oy + y) * scale];
        let t = self.steps as f32 * STEP;
        for f in &self.flashes {
            let a = (1.0 - f.life / FLASH).powi(2) * FLASH_LIGHT;
            let [r, g, b] = f.colour.map(|c| c / 255.0 * a);
            screened.square(
                at(f.x, f.y),
                [FLASH_REACH * scale; 2],
                [1.0, 1.0],
                [r, g, b, a],
                Shape::Glow,
                [Glow::Glint as u8 as f32, 0.0, 0.0],
            );
        }
        for s in &self.sparks {
            let k = 1.0 - s.life / s.max;
            let mut a = if s.small { k * 0.7 } else { (k * 1.6).min(1.0) };
            let mut colour = s.colour;
            if s.crackle && k < 0.35 {
                a *= s.flicker;
                colour = CRACKLE;
            }
            let a = a.min(1.0);
            if a <= 0.004 {
                continue;
            }
            let [r, g, b] = colour.map(|c| c / 255.0 * a);
            let head = at(s.x, s.y);
            // Its streak: back along where it came from, fading out.
            let width = if s.small { 1.0 } else { 1.6 } * scale;
            let (dx, dy) = (-s.vx * TRAIL * scale, -s.vy * TRAIL * scale);
            streak(&mut added, head, [dx, dy], width, [r, g, b, a]);
            if !s.small {
                let z = 10.0 * (0.4 + k) * scale;
                let g = a * 0.55;
                let [r, g2, b] = colour.map(|c| c / 255.0 * g);
                // Its glow's trail: the board drew its glow each frame, and
                // its sky kept the last few, a soft comet behind it.
                let h = g * 0.5;
                let [hr, hg, hb] = colour.map(|c| c / 255.0 * h);
                glow_streak(&mut added, head, [dx, dy], z / 4.0, [hr, hg, hb, h]);
                added.square(
                    head,
                    [z / 2.0; 2],
                    [1.0, 1.0],
                    [r, g2, b, g],
                    Shape::Glow,
                    [Glow::Spark as u8 as f32, 0.0, 0.0],
                );
            }
        }
        for rocket in self.rockets.iter().filter(|r| t >= r.at) {
            let [r, g, b] = ROCKET.map(|c| c / 255.0);
            added.square(
                at(rocket.x, rocket.y),
                [6.0 * scale; 2],
                [1.0, 1.0],
                [r, g, b, 1.0],
                Shape::Glow,
                [Glow::Spark as u8 as f32, 0.0, 0.0],
            );
        }
        (added, screened)
    }

    /// The light its flashes cast on a window's edges (global, logical:
    /// `x0, y0, x1, y1`), top, right, bottom, left: each its colour,
    /// premultiplied, from the flashes near its middle.
    pub fn light_on(&self, [x0, y0, x1, y1]: [f32; 4]) -> Option<[[f32; 4]; 4]> {
        if self.flashes.is_empty() {
            return None;
        }
        let (zx, zy) = (self.zone[0], self.zone[1]);
        let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let middles = [(mx, y0), (x1, my), (mx, y1), (x0, my)];
        let mut lit = false;
        let edges = middles.map(|(ex, ey)| {
            let mut sum = [0.0f32; 4];
            for f in &self.flashes {
                let d = (zx + f.x - ex).hypot(zy + f.y - ey);
                let near = (1.0 - d / FLASH_REACH).max(0.0);
                // The board's flash over the windows, on their thin edges
                // half again as bright.
                let a = (1.0 - f.life / FLASH).powi(2) * FLASH_LIGHT * near * 1.5;
                for (s, c) in sum.iter_mut().zip(f.colour) {
                    *s += c / 255.0 * a;
                }
                sum[3] += a;
            }
            let a = sum[3].min(1.0);
            lit |= a > 0.004;
            let k = if sum[3] > 1.0 { 1.0 / sum[3] } else { 1.0 };
            [sum[0] * k, sum[1] * k, sum[2] * k, a]
        });
        lit.then_some(edges)
    }

    /// Its sparks' places now (global, logical), for the tests.
    pub fn sparks(&self) -> Vec<(f32, f32)> {
        self.sparks
            .iter()
            .map(|s| (self.zone[0] + s.x, self.zone[1] + s.y))
            .collect()
    }
}

/// A streak from `head` back by `back` (pixels), `width` wide, `colour`
/// (premultiplied) at its head fading to nothing at its tail; its edges
/// smoothed.
fn streak(mesh: &mut Mesh, head: [f32; 2], back: [f32; 2], width: f32, colour: [f32; 4]) {
    let len = back[0].hypot(back[1]).max(width);
    let (ux, uy) = if back[0].hypot(back[1]) > 0.01 {
        (back[0] / len, back[1] / len)
    } else {
        (0.0, 1.0)
    };
    // Half its length and width, a pixel more for the smoothing.
    let (hl, hw) = (len / 2.0 + 1.0, width / 2.0 + 1.0);
    let (cx, cy) = (head[0] + ux * len / 2.0, head[1] + uy * len / 2.0);
    let (nx, ny) = (-uy, ux);
    let corner = |a: f32, b: f32| [cx + ux * a + nx * b, cy + uy * a + ny * b];
    let none = [0.0; 4];
    mesh.quad_of(
        [
            corner(-hl, -hw),
            corner(-hl, hw),
            corner(hl, hw),
            corner(hl, -hw),
        ],
        [[-hl, -hw], [-hl, hw], [hl, hw], [hl, -hw]],
        [colour, colour, none, none],
        Shape::Rounded,
        [len / 2.0, width / 2.0, width / 2.0],
    );
}

/// A soft streak from `head` back by `back` (pixels), blurred by `sigma`
/// across and at its ends, `colour` (premultiplied) at its head fading to
/// nothing at its tail.
fn glow_streak(mesh: &mut Mesh, head: [f32; 2], back: [f32; 2], sigma: f32, colour: [f32; 4]) {
    let len = back[0].hypot(back[1]);
    if len < 0.5 {
        return;
    }
    let (ux, uy) = (back[0] / len, back[1] / len);
    let reach = 2.0 * sigma;
    let (hl, hw) = (len / 2.0 + reach, reach);
    let (cx, cy) = (head[0] + ux * len / 2.0, head[1] + uy * len / 2.0);
    let (nx, ny) = (-uy, ux);
    let corner = |a: f32, b: f32| [cx + ux * a + nx * b, cy + uy * a + ny * b];
    let none = [0.0; 4];
    mesh.quad_of(
        [
            corner(-hl, -hw),
            corner(-hl, hw),
            corner(hl, hw),
            corner(hl, -hw),
        ],
        [[-hl, -hw], [-hl, hw], [hl, hw], [hl, -hw]],
        [colour, colour, none, none],
        Shape::Box,
        [len / 2.0, 0.5, sigma],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seven_rockets_burst_then_all_is_gone() {
        let start = Duration::from_secs(1);
        let mut show = Show::new(start, 7, "out".into(), [0.0, 0.0, 1000.0, 720.0]);
        let mut burst_at = None;
        let mut most = 0;
        for tenth in 1..=100 {
            let now = start + Duration::from_millis(100 * tenth);
            show.advance(now);
            most = most.max(show.sparks.len());
            if burst_at.is_none() && !show.flashes.is_empty() {
                burst_at = Some(tenth);
            }
            if show.over(now) {
                break;
            }
        }
        // The first bursts once risen, some 1.5 s on.
        assert!(
            burst_at.is_some_and(|t| (10..=20).contains(&t)),
            "{burst_at:?}"
        );
        assert!(most > 300, "{most}");
        assert!(show.rockets.is_empty() && show.sparks.is_empty());
        // The meshes stay within what a mesh holds.
        let mut show = Show::new(start, 7, "out".into(), [0.0, 0.0, 1000.0, 720.0]);
        for tenth in 1..=80 {
            show.advance(start + Duration::from_millis(100 * tenth));
            let (added, _) = show.shapes((0.0, 0.0), 1.0);
            assert!(added.vertices.len() / super::super::particles::FLOATS < 65_536);
        }
    }

    #[test]
    fn a_flash_lights_the_edges_near_it() {
        let mut show = Show::new(Duration::ZERO, 1, "out".into(), [0.0, 0.0, 1200.0, 720.0]);
        show.flashes.push(Flash {
            x: 300.0,
            y: 100.0,
            colour: [255.0, 214.0, 120.0],
            life: 0.0,
        });
        let near = show.light_on([200.0, 300.0, 600.0, 500.0]).expect("lit");
        // Its top, nearer, more than its bottom.
        assert!(near[0][3] > near[2][3], "{near:?}");
        assert!(show.light_on([1900.0, 1300.0, 2000.0, 1400.0]).is_none());
    }
}
