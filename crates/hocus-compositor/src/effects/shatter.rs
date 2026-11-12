//! A closing window shatters into glass (#798, the "Éclats de verre" board
//! of the effects canvas): with `[effects] shatter = true`, a closing
//! window keeps its last picture, cracks where it was clicked (or at its
//! middle), then breaks into Voronoi shards that keep its picture, spin,
//! tilt to catch the light and fall, in place of the closing animation.
//!
//! The board's, its lengths in logical pixels: 22 sites gathered round
//! the hit (within 160 px, most near it) and 16 anywhere on the window,
//! each shard the window cut to its site's cell. The crack (220 ms): the
//! cells' edges drawn out from the hit over 160 ms, 520 px far, under a
//! white flash fading over 90 px. Then each shard, after a delay of its
//! distance over 900 px/s, is thrown from the hit (240 e^(-d/120) + 18
//! px/s, up a little near it), falls at 1500 px/s², spins and tilts about
//! an axis of its own: squashed by its tilt's cosine, it shines as it
//! turns, darkens showing its back, its edges lit. Stepped at a fixed 60
//! steps a second from the break with a seeded generator, so that a
//! moment of it is the same whatever the frames' pace. Over once every
//! shard is 200 px below the screen, or 3 s after the break.
//!
//! The window is gone from the desktop as it starts (its picture alone
//! breaks): input never reaches it. Once over, nothing is drawn.

use std::{
    cell::{Cell, RefCell},
    time::Duration,
};

use smithay::{
    backend::renderer::{element::Id, utils::CommitCounter},
    utils::{Logical, Rectangle},
};

use super::{
    mesh::{Mesh, Shape},
    particles::{Rng, STEP},
};
use crate::{decoration::FrameIds, wobble::Kept};

/// The crack, before the shards fly; the cracks grow over its first
/// `GROW`, `REACH` far from the hit.
pub const CRACK: Duration = Duration::from_millis(220);
const GROW: f32 = 0.16;
const REACH: f32 = 520.0;
/// The flash at the hit: its radius and how bright it starts.
pub const FLASH: f32 = 90.0;
const FLASH_ALPHA: f32 = 0.35;
/// The cracks: their width (logical) and alpha.
const CRACK_WIDTH: f32 = 1.0;
const CRACK_ALPHA: f32 = 0.75;
/// How far below the screen a shard is gone.
const BELOW: f32 = 200.0;
/// Whatever the frames drawn, nothing lingers after this: the crack, and
/// the shards' longest flight (3 s).
pub const LONGEST: Duration = Duration::from_millis(220 + 3000);
/// The window's shadow goes as the shards fly, this fast.
const SHADOW_GONE: f32 = 0.12;
/// Gravity, px/s²; how much of its sideways speed a shard keeps a step.
const GRAVITY: f32 = 1500.0;
const DRAG: f32 = 0.995;
/// Floats a vertex of the glass: its place (output-local, physical), its
/// place on the picture (0 to 1), and its look: the white of the light
/// it catches, the dark of its back, its edge's light, how far in from
/// its edge it is (physical pixels; out of it, negative).
pub const SHARD_FLOATS: usize = 8;
/// How far out of a shard's edge it is drawn, smoothing it, physical
/// pixels.
const FRINGE: f32 = 1.5;
/// A vertex of the window whole: no edge near.
const WHOLE: f32 = 1.0e4;
/// The edges' width, logical pixels.
pub const EDGE: f32 = 1.2;
/// At most this many steps at once, after a long pause.
const MOST_STEPS: u64 = 400;

/// A shard: its outline where it was on the window (global, logical), and
/// how it moves.
#[derive(Debug, Clone)]
pub struct Shard {
    pub outline: Vec<[f32; 2]>,
    /// The middle of its outline's corners, what it turns about.
    pub centre: [f32; 2],
    /// How far it went, and how fast.
    pub moved: [f32; 2],
    velocity: [f32; 2],
    /// Its spin, and its tilt about `axis` (radians), and their speeds.
    pub spin: f32,
    spin_speed: f32,
    pub tilt: f32,
    tilt_speed: f32,
    pub axis: f32,
    /// It starts moving this long after the break, seconds.
    delay: f32,
}

impl Shard {
    /// Where `p` of its outline is now: turned and squashed about its
    /// centre, moved.
    pub fn place(&self, p: [f32; 2]) -> [f32; 2] {
        let [cx, cy] = self.centre;
        let (x, y) = (p[0] - cx, p[1] - cy);
        // rotate(-axis), scale(1, squash), rotate(spin + axis): the board's
        // canvas transform, read from the right.
        let (s0, c0) = (-self.axis).sin_cos();
        let (x, y) = (x * c0 - y * s0, x * s0 + y * c0);
        let y = y * self.squash();
        let (s1, c1) = (self.spin + self.axis).sin_cos();
        let (x, y) = (x * c1 - y * s1, x * s1 + y * c1);
        [cx + self.moved[0] + x, cy + self.moved[1] + y]
    }

    /// How flat its tilt shows it, from 0.06 to 1.
    pub fn squash(&self) -> f32 {
        self.tilt.cos().abs().max(0.06)
    }

    /// How much light it catches, from 0 to 1.
    pub fn shine(&self) -> f32 {
        (self.tilt * 0.5 + self.axis).sin().max(0.0).powi(6)
    }

    /// Whether it shows its back.
    pub fn back(&self) -> bool {
        self.tilt.cos() < 0.0
    }
}

/// Where a closing window is in its breaking.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    /// Whole, cracking: how far the cracks reach (0 to 1).
    Crack {
        grow: f32,
    },
    /// Its shards flying, so long after the break, seconds.
    Fly {
        since: f32,
    },
    Over,
}

/// A window shattering.
#[derive(Debug)]
pub struct Shatter {
    pub start: Duration,
    /// Where the window was, global and logical.
    pub rect: Rectangle<f64, Logical>,
    /// Where it cracks, global and logical.
    pub hit: [f32; 2],
    /// Below this (global), a shard is gone.
    floor: f32,
    pub shards: RefCell<Vec<Shard>>,
    steps: Cell<u64>,
    /// The window's last picture, drawn once, the renderer's.
    pub picture: RefCell<Option<Kept>>,
    /// Its cracks and flash, its picture or shards: the same ids over its
    /// frames; and its shadow's pieces.
    pub ids: [Id; 2],
    pub shadow_ids: FrameIds,
    commits: Cell<CommitCounter>,
}

impl Shatter {
    /// The window at `rect` breaking from `hit` (both global, logical) at
    /// `now`; `floor`: the bottom of the screens under it.
    pub fn new(
        now: Duration,
        rect: Rectangle<f64, Logical>,
        hit: (f64, f64),
        floor: f64,
        seed: u32,
    ) -> Self {
        let hit = [hit.0 as f32, hit.1 as f32];
        Self {
            start: now,
            rect,
            hit,
            floor: floor as f32 + BELOW,
            shards: RefCell::new(fracture(rect, hit, &mut Rng::new(u64::from(seed) + 7))),
            steps: Cell::new(0),
            picture: RefCell::new(None),
            ids: std::array::from_fn(|_| Id::new()),
            shadow_ids: FrameIds::default(),
            commits: Cell::new(CommitCounter::default()),
        }
    }

    fn elapsed(&self, now: Duration) -> Duration {
        now.saturating_sub(self.start)
    }

    pub fn phase(&self, now: Duration) -> Phase {
        let elapsed = self.elapsed(now);
        if elapsed < CRACK {
            Phase::Crack {
                grow: (elapsed.as_secs_f32() / GROW).min(1.0),
            }
        } else if self.done(now) {
            Phase::Over
        } else {
            Phase::Fly {
                since: (elapsed - CRACK).as_secs_f32(),
            }
        }
    }

    /// The shards whose cracks show while it cracks, `grow` far.
    pub fn cracked(&self, grow: f32) -> Vec<Vec<[f32; 2]>> {
        let [hx, hy] = self.hit;
        self.shards
            .borrow()
            .iter()
            .filter(|s| (s.centre[0] - hx).hypot(s.centre[1] - hy) <= grow * REACH)
            .map(|s| s.outline.clone())
            .collect()
    }

    /// The flash's alpha while it cracks.
    pub fn flash(grow: f32) -> f32 {
        FLASH_ALPHA * (1.0 - grow)
    }

    /// Its shadow's share left as the shards start flying.
    pub fn shadow(&self, now: Duration) -> f32 {
        match self.phase(now) {
            Phase::Crack { .. } => 1.0,
            Phase::Fly { since } => (1.0 - since / SHADOW_GONE).max(0.0),
            Phase::Over => 0.0,
        }
    }

    /// Steps the shards to `now`.
    pub fn advance(&self, now: Duration) {
        let Some(since) = self.elapsed(now).checked_sub(CRACK) else {
            return;
        };
        let due = (since.as_secs_f64() / f64::from(STEP)) as u64;
        let from = self.steps.get().max(due.saturating_sub(MOST_STEPS));
        let mut shards = self.shards.borrow_mut();
        for step in from..due {
            let k = step as f32 * STEP;
            for s in shards.iter_mut().filter(|s| k > s.delay) {
                s.velocity[1] += GRAVITY * STEP;
                s.velocity[0] *= DRAG;
                s.moved[0] += s.velocity[0] * STEP;
                s.moved[1] += s.velocity[1] * STEP;
                s.spin += s.spin_speed * STEP;
                s.tilt += s.tilt_speed * STEP;
            }
        }
        self.steps.set(self.steps.get().max(due));
    }

    /// Over at `now`: every shard fallen out of sight, or flown its
    /// longest.
    pub fn done(&self, now: Duration) -> bool {
        let elapsed = self.elapsed(now);
        let fallen = || {
            self.shards
                .borrow()
                .iter()
                .all(|s| s.centre[1] + s.moved[1] >= self.floor)
        };
        elapsed >= LONGEST || (elapsed >= CRACK && fallen())
    }

    /// The triangles of its glass as it is at `now`, as the shards'
    /// program reads them ([`SHARD_FLOATS`] floats a vertex), output-local
    /// and physical (the output's corner at `origin`, `scale` physical
    /// pixels a logical one): the window whole while it cracks, else its
    /// shards; and the bounds they cover (`x0, y0, x1, y1`).
    pub fn glass(&self, now: Duration, origin: (f64, f64), scale: f64) -> (Vec<f32>, [f32; 4]) {
        let s = scale as f32;
        let (ox, oy) = (origin.0 as f32, origin.1 as f32);
        let r = self.rect;
        let (rx, ry, rw, rh) = (
            r.loc.x as f32,
            r.loc.y as f32,
            (r.size.w as f32).max(1.0),
            (r.size.h as f32).max(1.0),
        );
        let to_px = |p: [f32; 2]| [(p[0] - ox) * s, (p[1] - oy) * s];
        let uv = |p: [f32; 2]| [(p[0] - rx) / rw, (p[1] - ry) / rh];
        let mut out = Vec::new();
        let mut bounds = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        let mut vertex = |out: &mut Vec<f32>, at: [f32; 2], uv: [f32; 2], look: [f32; 4]| {
            bounds = [
                bounds[0].min(at[0]),
                bounds[1].min(at[1]),
                bounds[2].max(at[0]),
                bounds[3].max(at[1]),
            ];
            out.extend_from_slice(&[
                at[0], at[1], uv[0], uv[1], look[0], look[1], look[2], look[3],
            ]);
        };
        if let Phase::Crack { .. } = self.phase(now) {
            let corners = [[rx, ry], [rx + rw, ry], [rx + rw, ry + rh], [rx, ry + rh]];
            let whole = [0.0, 0.0, 0.0, WHOLE];
            for i in [0, 1, 2, 0, 2, 3] {
                let c = corners.get(i).copied().unwrap_or_default();
                vertex(&mut out, to_px(c), uv(c), whole);
            }
            return (out, bounds);
        }
        for shard in self.shards.borrow().iter() {
            let shine = shard.shine();
            let look = |edge: f32| {
                [
                    0.02 + 0.38 * shine,
                    if shard.back() { 0.35 } else { 0.0 },
                    0.35 + 0.5 * shine,
                    edge,
                ]
            };
            let c = to_px(shard.place(shard.centre));
            let c_uv = uv(shard.centre);
            let n = shard.outline.len();
            for i in 0..n {
                let (Some(a), Some(b)) = (shard.outline.get(i), shard.outline.get((i + 1) % n))
                else {
                    continue;
                };
                let (pa, pb) = (to_px(shard.place(*a)), to_px(shard.place(*b)));
                // The centre's distance to this edge, on the screen: the
                // edge pushed out by the fringe (about the centre, its
                // places across the picture with it).
                let (ex, ey) = (pb[0] - pa[0], pb[1] - pa[1]);
                let len = ex.hypot(ey);
                if len < 1e-3 {
                    continue;
                }
                let h = ((c[0] - pa[0]) * ey - (c[1] - pa[1]) * ex).abs() / len;
                if h < 0.25 {
                    continue;
                }
                let k = (h + FRINGE) / h;
                let out_of = |p: [f32; 2], q: [f32; 2]| {
                    [
                        c[0] + (p[0] - c[0]) * k,
                        c[1] + (p[1] - c[1]) * k,
                        c_uv[0] + (q[0] - c_uv[0]) * k,
                        c_uv[1] + (q[1] - c_uv[1]) * k,
                    ]
                };
                let [ax, ay, au, av] = out_of(pa, uv(*a));
                let [bx, by, bu, bv] = out_of(pb, uv(*b));
                vertex(&mut out, c, c_uv, look(h));
                vertex(&mut out, [ax, ay], [au, av], look(-FRINGE));
                vertex(&mut out, [bx, by], [bu, bv], look(-FRINGE));
            }
        }
        (out, bounds)
    }

    /// Its cracks `grow` far and the flash at the hit while it cracks, as
    /// the shapes' program reads them: output-local and physical (the
    /// output's corner at `origin`, `scale` physical pixels a logical
    /// one).
    pub fn cracks(&self, grow: f32, origin: (f64, f64), scale: f64) -> Mesh {
        let s = scale as f32;
        let (ox, oy) = (origin.0 as f32, origin.1 as f32);
        let to_px = |p: [f32; 2]| [(p[0] - ox) * s, (p[1] - oy) * s];
        let mut mesh = Mesh::default();
        let a = CRACK_ALPHA;
        let colour = [235.0 / 255.0 * a, 245.0 / 255.0 * a, a, a];
        let half_width = CRACK_WIDTH * 0.5 * s;
        for outline in self.cracked(grow) {
            let n = outline.len();
            for i in 0..n {
                let (Some(p), Some(q)) = (outline.get(i), outline.get((i + 1) % n)) else {
                    continue;
                };
                let (p, q) = (to_px(*p), to_px(*q));
                let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
                let len = dx.hypot(dy);
                if len < 1e-3 {
                    continue;
                }
                let (t, nrm) = ([dx / len, dy / len], [-dy / len, dx / len]);
                let m = [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
                let (along, across) = (len / 2.0 + 1.0, half_width + 1.0);
                let corner =
                    |u: f32, v: f32| [m[0] + t[0] * u + nrm[0] * v, m[1] + t[1] * u + nrm[1] * v];
                mesh.quad(
                    [
                        corner(-along, -across),
                        corner(along, -across),
                        corner(along, across),
                        corner(-along, across),
                    ],
                    [
                        [-along, -across],
                        [along, -across],
                        [along, across],
                        [-along, across],
                    ],
                    colour,
                    Shape::Box,
                    [len / 2.0, half_width, 0.0],
                );
            }
        }
        // The flash, white fading out to its edge: a soft disc whose stop
        // is at its middle falls off evenly from three quarters.
        let flash = Self::flash(grow) / 0.75;
        if flash > 0.0 {
            mesh.square(
                to_px(self.hit),
                [FLASH * s, FLASH * s],
                [1.0, 1.0],
                [flash; 4],
                Shape::Flake,
                [1.0e-4, 0.0, 0.0],
            );
        }
        mesh
    }

    /// The commit of this frame's drawing: a new one each frame.
    pub fn next_commit(&self) -> CommitCounter {
        let mut c = self.commits.get();
        c.increment();
        self.commits.set(c);
        c
    }
}

/// `outline` cut by the half-plane `n · p <= d`.
fn clip(outline: &[[f32; 2]], n: [f32; 2], d: f32) -> Vec<[f32; 2]> {
    let mut out = Vec::with_capacity(outline.len() + 1);
    let side = |p: [f32; 2]| p[0] * n[0] + p[1] * n[1] - d;
    for (i, a) in outline.iter().enumerate() {
        let b = outline
            .get(i + 1)
            .or(outline.first())
            .copied()
            .unwrap_or(*a);
        let (da, db) = (side(*a), side(b));
        if da <= 0.0 {
            out.push(*a);
        }
        if (da <= 0.0) != (db <= 0.0) {
            let t = da / (da - db);
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    out
}

/// The board's `fracture`: the window over `rect` cut into the cells of
/// sites gathered round `hit`, each thrown from it.
fn fracture(rect: Rectangle<f64, Logical>, hit: [f32; 2], rng: &mut Rng) -> Vec<Shard> {
    let (x, y, w, h) = (
        rect.loc.x as f32,
        rect.loc.y as f32,
        rect.size.w as f32,
        rect.size.h as f32,
    );
    let mut rnd = |a: f32, b: f32| a + rng.next() * (b - a);
    let mut sites = Vec::with_capacity(38);
    for _ in 0..22 {
        let a = rnd(0.0, std::f32::consts::TAU);
        let r = rnd(0.0, 1.0).powf(1.8) * 160.0;
        sites.push([hit[0] + a.cos() * r, hit[1] + a.sin() * r]);
    }
    for _ in 0..16 {
        sites.push([x + rnd(0.0, w), y + rnd(0.0, h)]);
    }
    let window = vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
    let mut shards = Vec::with_capacity(sites.len());
    for (i, s) in sites.iter().enumerate() {
        let mut outline = window.clone();
        for (j, t) in sites.iter().enumerate() {
            if i == j {
                continue;
            }
            let n = [t[0] - s[0], t[1] - s[1]];
            let d = n[0] * (s[0] + t[0]) / 2.0 + n[1] * (s[1] + t[1]) / 2.0;
            outline = clip(&outline, n, d);
            if outline.len() < 3 {
                break;
            }
        }
        if outline.len() < 3 {
            continue;
        }
        let k = outline.len() as f32;
        let centre = outline
            .iter()
            .fold([0.0, 0.0], |c, p| [c[0] + p[0] / k, c[1] + p[1] / k]);
        let (dx, dy) = (centre[0] - hit[0], centre[1] - hit[1]);
        let d = dx.hypot(dy).max(1.0);
        let push = 240.0 * (-d / 120.0).exp() + 18.0;
        let velocity = [
            dx / d * push + rnd(-30.0, 30.0),
            dy / d * push - rnd(30.0, 140.0) * (-d / 220.0).exp(),
        ];
        let spin_speed = rnd(-3.5, 3.5);
        let tilt_speed = rnd(1.5, 6.0) * if rnd(0.0, 1.0) < 0.5 { -1.0 } else { 1.0 };
        let axis = rnd(0.0, std::f32::consts::PI);
        shards.push(Shard {
            outline,
            centre,
            moved: [0.0, 0.0],
            velocity,
            spin: 0.0,
            spin_speed,
            tilt: 0.0,
            tilt_speed,
            axis,
            delay: d / 900.0,
        });
    }
    shards
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn area(outline: &[[f32; 2]]) -> f32 {
        let n = outline.len();
        (0..n)
            .map(|i| {
                let (a, b) = (outline[i], outline[(i + 1) % n]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum::<f32>()
            .abs()
            / 2.0
    }

    #[test]
    fn its_shards_tile_the_window_then_fall_out_of_sight() {
        let rect = Rectangle::new((330.0, 120.0).into(), (500.0, 380.0).into());
        let glass = Shatter::new(ms(1000), rect, (560.0, 300.0), 720.0, 5);
        let shards = glass.shards.borrow().clone();
        assert!(shards.len() > 30, "{} shards", shards.len());
        let covered: f32 = shards.iter().map(|s| area(&s.outline)).sum();
        assert!((covered - 500.0 * 380.0).abs() < 1.0, "{covered}");
        // At rest a shard's places are its outline's.
        let s = &shards[0];
        let p = s.place(s.outline[0]);
        assert!((p[0] - s.outline[0][0]).abs() < 1e-3 && (p[1] - s.outline[0][1]).abs() < 1e-3);
        assert_eq!(glass.phase(ms(1000)), Phase::Crack { grow: 0.0 });
        assert!(glass.cracked(0.0).len() <= 1);
        assert!(glass.cracked(1.0).len() > 20);
        // Flying, the nearest first; all fallen within 3.22 s.
        glass.advance(ms(1000 + 220 + 300));
        let moved = glass
            .shards
            .borrow()
            .iter()
            .filter(|s| s.moved[1] != 0.0)
            .count();
        assert!(moved > 10, "{moved} flying");
        let mut over = None;
        for t in (1300..=4300).step_by(50) {
            glass.advance(ms(t));
            if glass.done(ms(t)) {
                over = Some(t);
                break;
            }
        }
        let over = over.unwrap_or(9999);
        assert!(over < 4000, "fallen by {over}");
        assert_eq!(glass.phase(ms(over)), Phase::Over);
    }
}
