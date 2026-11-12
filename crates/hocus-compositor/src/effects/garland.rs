//! A garland under the active window (#799, docs/design/effects.md): the
//! board "Guirlande"'s rope ported, its lengths in logical pixels. A
//! string of 34 points pinned at five of them along the window's bottom
//! edge, 1 px below it, 16 % slack; Verlet stepped with gravity, a light
//! wind and fourteen passes of its links each step, two steps a 60 Hz
//! frame as the board's. It swings as the window moves (its pins drag it),
//! and jumps to the window that takes the keyboard, lifted on its way.
//! Fifteen bulbs hang from it, each a pendulum swung by its point's speed,
//! twinkling in three ways in turn, nine seconds each.
//!
//! What it draws (`crate::render::garland`): the cord and the bulbs right
//! in front of the window, never on it; their halos on the wallpaper,
//! under the windows.

use std::{cell::RefCell, collections::HashMap, f64::consts::PI, time::Duration};

use smithay::{
    backend::renderer::{element::Id, utils::CommitCounter},
    utils::{Logical, Rectangle},
};

use super::{
    Still,
    mesh::{Glow, Mesh, Shape},
    particles::Rng,
    snowing::Snowing,
};
use crate::{
    State,
    model::{Mode, WindowId, Zone},
};

/// The rope's points.
const POINTS: usize = 34;
/// Those pinned to the window's bottom edge, evenly across it.
const PINS: [usize; 5] = [0, 8, 16, 25, 33];
/// The rope's length over the span of its end pins.
const SLACK: f64 = 1.16;
/// The end pins in from the window's sides.
const INSET: f64 = 8.0;
const GRAVITY: f64 = 900.0;
const DAMPING: f64 = 0.992;
const PASSES: usize = 14;
/// A step, seconds: the board's two a frame at 60 Hz.
const STEP: f64 = 1.0 / 120.0;
/// The most steps a frame catches up on: after a pause, it goes on from
/// where it was.
const MOST_STEPS: u64 = 12;
/// A jump to another window: how long, and how high it is lifted midway.
const JUMP: f64 = 0.8;
const LIFT: f64 = 110.0;
/// How it hangs as it starts, before its first steps.
const SAG: f64 = 40.0;
const WARM_STEPS: u32 = 240;
/// Drawn still (reduced motion): stepped this many times, its swing
/// damped hard, from where it hung.
const SETTLE_STEPS: u32 = 300;
const SETTLE_DAMPING: f64 = 0.85;
/// How long each way of twinkling lasts, seconds.
const MODE: f64 = 9.0;
/// The bulbs' colours, in turn.
const COLOURS: [[f32; 3]; 5] = [
    [255.0, 92.0, 92.0],
    [255.0, 206.0, 102.0],
    [94.0, 230.0, 168.0],
    [108.0, 184.0, 255.0],
    [255.0, 150.0, 230.0],
];
/// How lit the bulbs are, still: between the twinkles' means.
const STILL_LEVEL: f32 = 0.8;
/// The room it needs under the window, on its screen: else none.
const ROOM: f64 = 20.0;
/// The garland's wind with the snow's: a share of it, as the board "Neige
/// qui tient" sways it.
const SNOW_WIND: f64 = 0.4;

#[derive(Debug, Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
    px: f64,
    py: f64,
}

#[derive(Debug, Clone, Copy)]
struct Bulb {
    /// Its point on the rope.
    at: usize,
    colour: usize,
    /// Its swing, radians, and how fast.
    ang: f64,
    av: f64,
    seed: f64,
}

/// A jump from where the pins were, at its time (seconds of the garland's
/// clock).
#[derive(Debug, Clone, Copy)]
struct Jump {
    from: [(f64, f64); 5],
    at: f64,
}

/// What a part of it drew last on a screen, and its commit.
#[derive(Debug, Default)]
pub struct Drawn {
    pub commit: CommitCounter,
    pub vertices: Vec<f32>,
}

/// The garland, while it shows.
#[derive(Debug)]
pub struct Garland {
    /// The window it hangs under (or jumps to).
    pub window: WindowId,
    /// That window where it is drawn (global, logical, on its strip).
    pub rect: Rectangle<f64, Logical>,
    points: Vec<Point>,
    bulbs: Vec<Bulb>,
    /// Where its pins are now.
    pins: [(f64, f64); 5],
    jump: Option<Jump>,
    start: Duration,
    steps: u64,
    /// Moving, or drawn still (reduced motion).
    moving: bool,
    /// Still and hanging where its window is.
    settled: bool,
    /// Its cord and bulbs, its halos, and the blank standing for them
    /// behind the glass.
    pub ids: [Id; 3],
    /// What each part drew last on each screen.
    pub drawn: RefCell<HashMap<(usize, String), Drawn>>,
}

/// Its pins under `rect`.
fn pins_of(rect: Rectangle<f64, Logical>) -> [(f64, f64); 5] {
    std::array::from_fn(|j| {
        (
            rect.loc.x + INSET + (rect.size.w - 2.0 * INSET) * j as f64 / 4.0,
            rect.loc.y + rect.size.h + 1.0,
        )
    })
}

impl Garland {
    /// A garland under `window` (at `rect`), hanging at rest.
    pub fn new(
        window: WindowId,
        rect: Rectangle<f64, Logical>,
        now: Duration,
        seed: u64,
        moving: bool,
    ) -> Self {
        let pins = pins_of(rect);
        let ((x0, y0), (x1, y1)) = (pins[0], pins[4]);
        let points = (0..POINTS)
            .map(|i| {
                let t = i as f64 / (POINTS - 1) as f64;
                let x = x0 + (x1 - x0) * t;
                let y = y0 + (y1 - y0) * t + (t * PI).sin() * SAG;
                Point { x, y, px: x, py: y }
            })
            .collect();
        let mut rng = Rng::new(seed);
        let bulbs = (1..POINTS - 1)
            .step_by(2)
            .filter(|i| !PINS.contains(i))
            .map(|at| Bulb {
                at,
                colour: ((at - 1) / 2) % COLOURS.len(),
                ang: 0.0,
                av: 0.0,
                seed: f64::from(rng.next()) * 100.0,
            })
            .collect();
        let mut garland = Self {
            window,
            rect,
            points,
            bulbs,
            pins,
            jump: None,
            start: now,
            steps: 0,
            moving,
            settled: false,
            ids: [Id::new(), Id::new(), Id::new()],
            drawn: RefCell::default(),
        };
        for _ in 0..WARM_STEPS {
            garland.step(0.0, DAMPING);
        }
        garland
    }

    /// Whether it is on its way to its window.
    pub fn jumping(&self) -> bool {
        self.jump.is_some()
    }

    /// Its clock, seconds: the twinkles'.
    pub fn time(&self) -> f64 {
        self.steps as f64 * STEP
    }

    fn since(&self, now: Duration) -> f64 {
        now.saturating_sub(self.start).as_secs_f64()
    }

    /// It hangs under `window` at `rect` now: jumping there from where it
    /// is when that is another window and it moves.
    pub fn hang(
        &mut self,
        window: WindowId,
        rect: Rectangle<f64, Logical>,
        now: Duration,
        moving: bool,
    ) {
        if moving != self.moving {
            self.moving = moving;
            self.settled = false;
            // Going on from where it is, not catching up.
            self.steps = self.steps.max((self.since(now) / STEP) as u64);
        }
        if window != self.window {
            self.jump = moving.then(|| Jump {
                from: self.pins,
                at: self.since(now),
            });
            self.window = window;
        }
        if rect != self.rect {
            self.rect = rect;
            self.settled = false;
        }
    }

    /// Stepped on to `now`, in `wind` (pixels a second each second;
    /// `None`: its own).
    pub fn advance(&mut self, now: Duration, wind: Option<f64>) {
        let due = (self.since(now) / STEP) as u64;
        let from = self.steps.max(due.saturating_sub(MOST_STEPS));
        for step in from..due {
            let t = step as f64 * STEP;
            self.pins = self.pins_at(t);
            let wind = wind.unwrap_or_else(|| (t * 0.7).sin() * 30.0 + (t * 1.9).sin() * 12.0);
            self.step(wind, DAMPING);
        }
        self.steps = self.steps.max(due);
    }

    /// Drawn still: hung at rest under its window. Whether it changed.
    pub fn hold(&mut self) -> bool {
        if self.settled {
            return false;
        }
        let target = pins_of(self.rect);
        let (dx, dy) = (target[0].0 - self.pins[0].0, target[0].1 - self.pins[0].1);
        for p in &mut self.points {
            p.x += dx;
            p.y += dy;
            p.px = p.x;
            p.py = p.y;
        }
        self.pins = target;
        self.jump = None;
        for _ in 0..SETTLE_STEPS {
            self.step(0.0, SETTLE_DAMPING);
        }
        for b in &mut self.bulbs {
            b.ang = 0.0;
            b.av = 0.0;
        }
        self.settled = true;
        true
    }

    /// Where its pins are at `t` (its clock): under its window, or on the
    /// way there, lifted.
    fn pins_at(&mut self, t: f64) -> [(f64, f64); 5] {
        let target = pins_of(self.rect);
        let Some(jump) = self.jump else {
            return target;
        };
        let k = ((t - jump.at) / JUMP).clamp(0.0, 1.0);
        if k >= 1.0 {
            self.jump = None;
            return target;
        }
        let e = 1.0 - (1.0 - k).powi(3);
        let lift = (k * PI).sin() * LIFT;
        let mut pins = target;
        for (pin, (fx, fy)) in pins.iter_mut().zip(jump.from) {
            let (tx, ty) = *pin;
            *pin = (fx + (tx - fx) * e, fy + (ty - fy) * e - lift);
        }
        pins
    }

    /// One step: the board's `step`.
    fn step(&mut self, wind: f64, damping: f64) {
        let dt = STEP;
        let (first, last) = (self.pins[0], self.pins[4]);
        let len = (last.0 - first.0).hypot(last.1 - first.1) * SLACK / (POINTS - 1) as f64;
        for p in &mut self.points {
            let (vx, vy) = ((p.x - p.px) * damping, (p.y - p.py) * damping);
            p.px = p.x;
            p.py = p.y;
            p.x += vx + wind * dt * dt;
            p.y += vy + GRAVITY * dt * dt;
        }
        let pinned = |points: &mut [Point], pins: &[(f64, f64); 5]| {
            for (i, (x, y)) in PINS.iter().zip(pins) {
                if let Some(p) = points.get_mut(*i) {
                    p.x = *x;
                    p.y = *y;
                }
            }
        };
        for _ in 0..PASSES {
            pinned(&mut self.points, &self.pins);
            for i in 1..self.points.len() {
                let (a, b) = self.points.split_at_mut(i);
                let (Some(p), Some(q)) = (a.last_mut(), b.first_mut()) else {
                    continue;
                };
                let (dx, dy) = (q.x - p.x, q.y - p.y);
                let d = dx.hypot(dy).max(1e-6);
                if d > len {
                    let k = (d - len) / d * 0.5;
                    p.x += dx * k;
                    p.y += dy * k;
                    q.x -= dx * k;
                    q.y -= dy * k;
                }
            }
        }
        pinned(&mut self.points, &self.pins);
        for b in &mut self.bulbs {
            let Some(p) = self.points.get(b.at) else {
                continue;
            };
            let ax = (p.x - p.px) / dt;
            b.av += (-b.ang * 60.0 - ax * 0.9 - b.av * 4.5) * dt;
            b.ang = (b.ang + b.av * dt).clamp(-1.1, 1.1);
        }
    }

    /// How lit `b` is now, from 0 to 1: a chase along the string, then a
    /// random twinkle, then a slow breath, nine seconds each.
    fn level(&self, b: &Bulb) -> f64 {
        if !self.moving {
            return f64::from(STILL_LEVEL);
        }
        let time = self.time();
        let i = b.at as f64;
        match (time / MODE) as u64 % 3 {
            0 => 0.55 + 0.45 * (time * 2.2 - i * 0.55).sin().max(0.0),
            1 => {
                let s = (time * 3.1 + b.seed).sin() * (time * 1.7 + b.seed * 2.3).sin();
                0.45 + 0.55 * s.abs()
            }
            _ => 0.65 + 0.35 * (time * 1.1 + b.colour as f64 * 1.3).sin(),
        }
    }

    /// The bulbs as drawn now: each one's point, swing, colour and level.
    fn lit(&self) -> Vec<(Point, f64, [f32; 3], f64)> {
        self.bulbs
            .iter()
            .filter_map(|b| {
                let p = self.points.get(b.at)?;
                let c = COLOURS.get(b.colour)?;
                Some((*p, b.ang, c.map(|v| v / 255.0), self.level(b)))
            })
            .collect()
    }

    /// The bulbs' points, global, for the tests.
    pub fn bulbs(&self) -> Vec<(f64, f64)> {
        self.lit().into_iter().map(|(p, ..)| (p.x, p.y)).collect()
    }

    /// The cord's path: the board's quadratic curves through the points'
    /// middles, each cut in four.
    fn path(&self) -> Vec<(f64, f64)> {
        let (Some(first), Some(last)) = (self.points.first(), self.points.last()) else {
            return Vec::new();
        };
        let mut out = vec![(first.x, first.y)];
        let mut from = (first.x, first.y);
        for pair in self.points.windows(2).skip(1) {
            let [c, n] = pair else {
                continue;
            };
            let to = ((c.x + n.x) / 2.0, (c.y + n.y) / 2.0);
            for k in 1..=4 {
                let t = f64::from(k) / 4.0;
                let (a, b, d) = ((1.0 - t) * (1.0 - t), 2.0 * (1.0 - t) * t, t * t);
                out.push((
                    a * from.0 + b * c.x + d * to.0,
                    a * from.1 + b * c.y + d * to.1,
                ));
            }
            from = to;
        }
        out.push((last.x, last.y));
        out
    }

    /// The cord and the bulbs, as drawn right in front of the window:
    /// what goes over what is under it (the cord, lit by the halos as the
    /// board's is; the bulbs' sockets and glass), and what is added to it
    /// (their cores and glints).
    pub fn front(&self, view: &View) -> (Mesh, Mesh) {
        let mut over = Mesh::default();
        let mut added = Mesh::default();
        let lit = self.lit();
        let s = view.scale;
        let path: Vec<[f32; 2]> = self.path().into_iter().map(|p| view.at(p)).collect();
        // The halos' light on the cord, at `p` (physical).
        let light = |p: [f32; 2]| -> [f32; 3] {
            let mut sum = [0.0f32; 3];
            for (b, _, c, l) in &lit {
                let [cx, cy] = view.at((b.x, b.y + 10.0));
                let hs = 120.0 * (0.6 + 0.4 * *l) as f32 * s;
                let a = halo(((p[0] - cx).hypot(p[1] - cy)) / hs) * (*l as f32) * 0.85;
                for (sum, c) in sum.iter_mut().zip(c) {
                    *sum += c * a;
                }
            }
            sum
        };
        let lit_by = |base: [f32; 3], alpha: f32, p: [f32; 2]| -> [f32; 4] {
            let l = light(p);
            let a = alpha * view.alpha;
            [
                (base[0] + l[0]).min(1.0) * a,
                (base[1] + l[1]).min(1.0) * a,
                (base[2] + l[2]).min(1.0) * a,
                a,
            ]
        };
        for (width, base) in [(3.2, rgb(0x0d, 0x1f, 0x14)), (1.8, rgb(0x1f, 0x4a, 0x2c))] {
            for pair in path.windows(2) {
                let [a, b] = pair else {
                    continue;
                };
                let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
                capsule(&mut over, *a, *b, width * s / 2.0, lit_by(base, 1.0, mid));
            }
        }
        // Its sheen: dashes 2.5 long every 6, round at their ends.
        for (a, b) in dashes(&path, 2.5 * s, 3.5 * s) {
            let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            let colour = lit_by(rgb(160, 220, 170), 0.22, mid);
            capsule(&mut over, a, b, 0.9 * s / 2.0, colour);
        }
        for (p, ang, c, l) in &lit {
            let l = *l as f32;
            let (sin, cos) = ang.sin_cos();
            // A point of the bulb, from its point on the rope, swung.
            let at = |x: f64, y: f64| view.at((p.x + x * cos - y * sin, p.y + x * sin + y * cos));
            let pad = 1.0 / f64::from(s);
            // Its socket.
            let (hx, hy, cy) = (2.6, 3.0, 2.0);
            let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
            over.quad(
                corners.map(|(u, v)| at(u * (hx + pad), cy + v * (hy + pad))),
                corners.map(|(u, v)| [(u * (hx + pad)) as f32 * s, (v * (hy + pad)) as f32 * s]),
                glow(rgb(0x16, 0x32, 0x22), view.alpha),
                Shape::Rounded,
                [hx as f32 * s, hy as f32 * s, 1.5 * s],
            );
            // Its glass: an oval lit from its tip, the board's gradient
            // from (-4, 5) to (4, 18).
            let (rx, ry, cy) = (4.3, 7.2, 11.5);
            let a0 = 0.55 + 0.45 * l;
            let a1 = 0.7 + 0.3 * l;
            let c0 = [c[0] * a0, c[1] * a0, c[2] * a0, a0];
            let lifted = c.map(|v| (v + 60.0 / 255.0).min(1.0));
            let c1 = [lifted[0] * a1, lifted[1] * a1, lifted[2] * a1, a1];
            let local = corners.map(|(u, v)| (u * (rx + pad), cy + v * (ry + pad)));
            let colours = local.map(|(x, y)| {
                let t = (((x + 4.0) * 8.0 + (y - 5.0) * 13.0) / 233.0) as f32;
                let mut colour = c0;
                for (v, end) in colour.iter_mut().zip(c1) {
                    *v = (*v + (end - *v) * t) * view.alpha;
                }
                colour
            });
            over.quad_of(
                local.map(|(x, y)| at(x, y)),
                corners.map(|(u, v)| [(u * (rx + pad) / rx) as f32, (v * (ry + pad) / ry) as f32]),
                colours,
                Shape::Oval,
                [rx as f32 * s, 0.0, 0.0],
            );
            // Its core and its glint, added.
            let core = at(0.0, cy);
            let r = 15.0 * s;
            added.square(
                core,
                [r, r],
                [1.0, 1.0],
                glow(*c, l * view.alpha),
                Shape::Glow,
                [Glow::Core as u8 as f32, 0.0, 0.0],
            );
            let r = 4.5 * s;
            added.square(
                [core[0] - s, core[1] - 2.0 * s],
                [r, r],
                [1.0, 1.0],
                glow(rgb(255, 250, 235), l * 0.9 * view.alpha),
                Shape::Glow,
                [Glow::Glint as u8 as f32, 0.0, 0.0],
            );
        }
        (over, added)
    }

    /// The bulbs' halos, added to the wallpaper under the windows.
    pub fn halos(&self, view: &View) -> Mesh {
        let mut mesh = Mesh::default();
        for (p, _, c, l) in self.lit() {
            let r = (120.0 * (0.6 + 0.4 * l)) as f32 * view.scale;
            mesh.square(
                view.at((p.x, p.y + 10.0)),
                [r, r],
                [1.0, 1.0],
                glow(c, (l * 0.85) as f32 * view.alpha),
                Shape::Glow,
                [Glow::Halo as u8 as f32, 0.0, 0.0],
            );
        }
        mesh
    }
}

/// Where a garland is drawn on a screen: global logical pixels moved by
/// `shift`, then scaled into the screen's (output-local, physical);
/// `alpha` of it.
#[derive(Debug, Clone, Copy)]
pub struct View {
    pub shift: (f64, f64),
    pub scale: f32,
    pub alpha: f32,
}

impl View {
    fn at(&self, (x, y): (f64, f64)) -> [f32; 2] {
        let s = f64::from(self.scale);
        [
            ((x + self.shift.0) * s) as f32,
            ((y + self.shift.1) * s) as f32,
        ]
    }
}

fn rgb(r: u8, g: u8, b: u8) -> [f32; 3] {
    [r, g, b].map(|v| f32::from(v) / 255.0)
}

/// A glow of colour `c`, `alpha` of it (premultiplied).
fn glow(c: [f32; 3], alpha: f32) -> [f32; 4] {
    [c[0] * alpha, c[1] * alpha, c[2] * alpha, alpha]
}

/// The halo's profile at `d` of its radius ([`Glow::Halo`], as the shader
/// draws it).
fn halo(d: f32) -> f32 {
    let mix = |a: f32, b: f32, t: f32| a + (b - a) * t;
    if d < 0.12 {
        mix(0.55, 0.32, d / 0.12)
    } else if d < 0.35 {
        mix(0.32, 0.10, (d - 0.12) / 0.23)
    } else if d < 0.7 {
        mix(0.10, 0.025, (d - 0.35) / 0.35)
    } else {
        mix(0.025, 0.0, ((d - 0.7) / 0.3).min(1.0))
    }
}

/// A stroke's piece from `a` to `b`, `half` its width, round at its ends.
fn capsule(mesh: &mut Mesh, a: [f32; 2], b: [f32; 2], half: f32, colour: [f32; 4]) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy);
    let (ux, uy) = if len > 1e-4 {
        (dx / len, dy / len)
    } else {
        (1.0, 0.0)
    };
    let (nx, ny) = (-uy, ux);
    let (hx, hy) = (len / 2.0 + half, half);
    let (ex, ey) = (hx + 1.0, hy + 1.0);
    let (mx, my) = ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0);
    let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
    mesh.quad(
        corners.map(|(u, v)| {
            [
                mx + ux * u * ex + nx * v * ey,
                my + uy * u * ex + ny * v * ey,
            ]
        }),
        corners.map(|(u, v)| [u * ex, v * ey]),
        colour,
        Shape::Rounded,
        [hx, hy, half],
    );
}

/// The dashes along `path`: `on` long, `off` apart, from its start; each
/// as a straight piece between its ends.
fn dashes(path: &[[f32; 2]], on: f32, off: f32) -> Vec<([f32; 2], [f32; 2])> {
    let mut out = Vec::new();
    let mut start: Option<[f32; 2]> = None;
    // How far along the pattern, and whether in a dash.
    let mut at = 0.0f32;
    let mut drawing = true;
    if let Some(first) = path.first() {
        start = Some(*first);
    }
    for pair in path.windows(2) {
        let [a, b] = pair else {
            continue;
        };
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let mut along = 0.0f32;
        while along < len {
            let left = if drawing { on } else { off } - at;
            if along + left > len {
                at += len - along;
                along = len;
            } else {
                along += left;
                at = 0.0;
                let t = along / len;
                let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
                if drawing {
                    if let Some(s) = start.take() {
                        out.push((s, p));
                    }
                } else {
                    start = Some(p);
                }
                drawing = !drawing;
            }
        }
    }
    if drawing && let (Some(s), Some(end)) = (start, path.last()) {
        out.push((s, *end));
    }
    out
}

impl State {
    /// How the garland shows now: `None`, not at all; `Some(true)`,
    /// moving; `Some(false)`, still (reduced motion).
    pub(crate) fn garlanding(&self) -> Option<bool> {
        if !self.config.effects.garland {
            return None;
        }
        match self.effects_still() {
            None => Some(true),
            Some(Still::ReducedMotion) => Some(false),
            Some(Still::PowerSaver | Still::Shown | Still::Cpu) => None,
        }
    }

    /// The window the garland hangs under at `now`, and where it is drawn
    /// (global, on its strip): the focused one, shown, with a free edge
    /// below it (not maximised nor fullscreen, room under it on its
    /// screen), on a screen that is on and shows no fullscreen window.
    fn garland_host(&self, now: Duration) -> Option<(WindowId, Rectangle<f64, Logical>)> {
        if self.locked() {
            return None;
        }
        let id = self.model.focused()?;
        if !self.windows.contains_key(&id)
            || self.model.is_minimized(id)
            || matches!(
                self.model.mode(id),
                Mode::Fullscreen | Mode::Snapped(Zone::Maximized)
            )
        {
            return None;
        }
        let (offset, _) = self.strip_offset(id, now)?;
        let mut rect = self.visual_rect(id, now)?;
        rect.loc.x += offset;
        let output = self.output_of(id)?;
        if !self.output_powered(&output) || self.fullscreen_in_front(&output) {
            return None;
        }
        let area = self.space.output_geometry(&output)?;
        let bottom = f64::from(area.loc.y + area.size.h);
        (rect.loc.y + rect.size.h + ROOM <= bottom).then_some((id, rect))
    }

    /// The garland moves on to `now` (#799): hung under the focused
    /// window as it is turned on, following it, jumping to the next one;
    /// gone with no window to hang under. Whether it moves (a frame is
    /// wanted).
    pub(crate) fn tick_garland(&mut self, now: Duration) -> bool {
        let showing = self.garlanding();
        let host = showing.and_then(|_| self.garland_host(now));
        let Some((moving, (id, rect))) = showing.zip(host) else {
            if self.fx.garland.take().is_some() {
                tracing::debug!("the garland goes");
                self.redraw = true;
            }
            return false;
        };
        let wind = self
            .fx
            .snow
            .as_ref()
            .filter(|_| self.snowing() == Some(Snowing::Falling))
            .map(|s| f64::from(s.wind) * SNOW_WIND);
        let garland = match &mut self.fx.garland {
            Some(garland) => {
                garland.hang(id, rect, now, moving);
                garland
            }
            empty => {
                self.fx.garlands += 1;
                tracing::debug!(window = id.0, moving, "a garland hangs");
                empty.insert(Garland::new(id, rect, now, self.fx.garlands, moving))
            }
        };
        if moving {
            garland.advance(now, wind);
            true
        } else {
            if garland.hold() {
                self.redraw = true;
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rectangle<f64, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    /// Stepped a 60 Hz frame at a time until `to`, without wind.
    fn run(g: &mut Garland, to: Duration) {
        let mut now = g.start + Duration::from_micros(16_667) * (g.steps as u32 / 2);
        while now < to {
            now += Duration::from_micros(16_667);
            g.advance(now.min(to), Some(0.0));
        }
    }

    #[test]
    fn it_hangs_below_the_window_and_swings_as_it_moves() {
        let at = rect(100.0, 100.0, 470.0, 340.0);
        let mut g = Garland::new(WindowId(1), at, Duration::ZERO, 1, true);
        let bottom = 441.0;
        // Fifteen bulbs, all below the edge, within its span.
        let bulbs = g.bulbs();
        assert_eq!(bulbs.len(), 15);
        assert!(
            bulbs
                .iter()
                .all(|(x, y)| *y > bottom && *x > 100.0 && *x < 570.0)
        );
        run(&mut g, Duration::from_secs(2));
        let rest = g.bulbs();
        // The window dragged right, 4 px a frame: the bulbs lag behind it,
        // then follow.
        for frame in 1..=15u32 {
            let x = 100.0 + 4.0 * f64::from(frame);
            let now = Duration::from_secs(2) + Duration::from_micros(16_667) * frame;
            g.hang(WindowId(1), rect(x, 100.0, 470.0, 340.0), now, true);
            g.advance(now, Some(0.0));
        }
        let dragged = g.bulbs();
        let lag = rest
            .iter()
            .zip(&dragged)
            .map(|(a, b)| b.0 - a.0)
            .sum::<f64>()
            / 15.0;
        assert!(lag > 0.0 && lag < 59.0, "{lag}");
        // Some swing.
        assert!(g.bulbs.iter().any(|b| b.ang.abs() > 0.05));
        run(&mut g, Duration::from_secs(8));
        let later = g.bulbs();
        let moved = rest.iter().zip(&later).map(|(a, b)| b.0 - a.0).sum::<f64>() / 15.0;
        assert!((moved - 60.0).abs() < 2.0, "{moved}");
    }

    #[test]
    fn it_jumps_to_another_window_lifted_on_its_way() {
        let mut g = Garland::new(
            WindowId(1),
            rect(100.0, 300.0, 400.0, 200.0),
            Duration::ZERO,
            1,
            true,
        );
        run(&mut g, Duration::from_secs(1));
        g.hang(
            WindowId(2),
            rect(700.0, 300.0, 400.0, 200.0),
            Duration::from_secs(1),
            true,
        );
        run(&mut g, Duration::from_millis(1400));
        assert!(g.pins[0].0 > 100.0 && g.pins[0].0 < 700.0, "{:?}", g.pins);
        assert!(g.pins[0].1 < 501.0 - 50.0, "lifted: {:?}", g.pins);
        run(&mut g, Duration::from_secs(3));
        assert_eq!(g.pins, pins_of(rect(700.0, 300.0, 400.0, 200.0)));
    }

    #[test]
    fn dashes_follow_the_pattern() {
        let path = [[0.0, 0.0], [5.0, 0.0], [20.0, 0.0]];
        let d = dashes(&path, 2.5, 3.5);
        let starts: Vec<f32> = d.iter().map(|(a, _)| a[0]).collect();
        assert_eq!(starts, [0.0, 6.0, 12.0, 18.0]);
        assert!(d.iter().all(|(a, b)| b[0] - a[0] <= 2.5 + 1e-4));
    }
}
