//! A koi pond (#803, the board "Bassin de koïs", its JS ported): the
//! wallpaper is a pond. Its water is a field of heights (the board's
//! 240 × 144 over its 1200 × 720: a cell every 5 logical pixels) rippling
//! as waves do; the koi swim with a spine of fourteen joints that bends
//! no more than 0.32 rad at each, their fins flapping and their patterns
//! on them; they come to the pointer when it rests on the water, circle
//! it, and scatter from a click; a drop falls now and then; lily pads
//! float, bobbing with the water. Drawn by `crate::render::koi`: the koi
//! and their blurred shadows over the pond's floor into a picture, seen
//! through the water (the board's `KOI_FS`), the pads over it.
//!
//! Stepped at a fixed 60 steps a second from its start (the board's
//! frames: two waves a step, as its two a frame), with a seeded
//! generator; as many koi and pads per pixel as the board's seven and six.
//! Its places are logical pixels from its screen's corner; the koi and
//! the pads keep to the screen's work area (beside the bar), as the
//! board's kept left of its bar. The floor, drawn once a screen, is drawn
//! by a worker.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    f32::consts::PI,
    rc::Rc,
    sync::mpsc,
    time::Duration,
};

use smithay::backend::renderer::{element::Id, utils::CommitCounter};

use super::{
    draw::Heights,
    mesh::{Mesh, Shape},
    particles::{Rng, STEP},
};
use crate::wobble::Kept;

/// A cell of the water's heights, logical pixels (the board's 1200 / 240).
pub const CELL: f32 = 5.0;
/// The board's stage, square pixels.
const BOARD: f32 = 1200.0 * 720.0;
/// The board's koi, its pads and its floor's pebbles.
const KOI_A_BOARD: f32 = 7.0;
const PADS_A_BOARD: f32 = 6.0;
const PEBBLES_A_BOARD: f32 = 900.0;
/// A koi's joints.
const SEG: usize = 14;
/// How far it keeps from the sides.
const MARGIN: f32 = 120.0;
/// The pointer rests once it has not moved more than this (logical
/// pixels) for this long (the board's 0.6 s).
const REST_MOVE: f64 = 3.0;
const REST_AFTER: Duration = Duration::from_millis(600);
/// Half a second of the board before the pond is first drawn: the koi's
/// spines bent, the water rippling.
const WARM_STEPS: u32 = 30;
/// At most this many steps at once (10 s).
const MOST_STEPS: u64 = 600;
/// The shadows' blur: the board's `blur(6px)`.
const SHADOW_SIGMA: f32 = 6.0;

/// The board's `PAL`: a koi's colour, its spots' and whether ink.
const PALETTE: [([u8; 3], &[[u8; 3]]); 7] = [
    (
        [0xF4, 0xEF, 0xE6],
        &[[0xE2, 0x48, 0x2F], [0xD9, 0x3A, 0x22]],
    ),
    (
        [0xF2, 0xED, 0xE3],
        &[[0x1B, 0x1A, 0x1F], [0xE5, 0x53, 0x2E]],
    ),
    ([0xF0, 0xB2, 0x3E], &[[0xF7, 0xD2, 0x7A]]),
    ([0xEC, 0xE7, 0xDD], &[[0xE9, 0x56, 0x2F]]),
    ([0xE6, 0x60, 0x3A], &[[0xF6, 0xEF, 0xE4]]),
    ([0xF7, 0xF2, 0xEA], &[[0x22, 0x21, 0x2A]]),
    ([0xE9, 0xD0, 0x6A], &[[0xF3, 0xE3, 0xA0]]),
];
/// The golden koi's fins are its own colour.
const GOLDEN: usize = 2;

fn rgb(c: [u8; 3], a: f32) -> [f32; 4] {
    let [r, g, b] = c.map(|v| f32::from(v) / 255.0 * a);
    [r, g, b, a]
}

/// `b - a`, between -π and π.
fn ang_diff(a: f32, b: f32) -> f32 {
    let mut d = b - a;
    while d > PI {
        d -= 2.0 * PI;
    }
    while d < -PI {
        d += 2.0 * PI;
    }
    d
}

type P = [f32; 2];

fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1]]
}
fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}
fn mul(a: P, k: f32) -> P {
    [a[0] * k, a[1] * k]
}
fn dir(a: f32) -> P {
    [a.cos(), a.sin()]
}
fn dot(a: P, b: P) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}
fn angle(a: P) -> f32 {
    a[1].atan2(a[0])
}
fn quad_at(a: P, c: P, b: P, t: f32) -> P {
    let u = 1.0 - t;
    add(add(mul(a, u * u), mul(c, 2.0 * u * t)), mul(b, t * t))
}

/// A spot on a koi: where along it (0 to 1), how far across (a share of
/// its width), its radii (along, a share of its length; across, of its
/// width) and its colour.
#[derive(Debug, Clone, Copy)]
struct Spot {
    t: f32,
    off: f32,
    rx: f32,
    ry: f32,
    colour: [u8; 3],
}

/// A koi.
#[derive(Debug, Clone)]
pub struct Koi {
    palette: usize,
    len: f32,
    /// Where it heads, radians.
    a: f32,
    speed: f32,
    /// Its joints from its head, logical pixels from the screen's corner.
    pub spine: [P; SEG],
    spots: [Spot; 4],
    phase: f32,
    /// Fleeing a click, seconds left.
    flee: f32,
}

impl Koi {
    #[expect(
        clippy::indexing_slicing,
        reason = "the palette is taken modulo its length, and each has a spot"
    )]
    fn new(palette: usize, zone: [f32; 4], rng: &mut Rng) -> Self {
        let [zx, zy, zw, zh] = zone;
        let mut rnd = |a: f32, b: f32| a + rng.next() * (b - a);
        let len = rnd(120.0, 170.0);
        let x = rnd(zx + 150.0, (zx + zw - 110.0).max(zx + 151.0));
        let y = rnd(zy + 120.0, (zy + zh - 160.0).max(zy + 121.0));
        let a = rnd(0.0, std::f32::consts::TAU);
        let spine = std::array::from_fn(|i| sub([x, y], mul(dir(a), i as f32 * len / SEG as f32)));
        let spots_of = PALETTE[palette].1;
        let spots = std::array::from_fn(|i| Spot {
            t: rnd(0.1, 0.75),
            off: rnd(-0.6, 0.6),
            rx: rnd(0.10, 0.2),
            ry: rnd(0.35, 0.7),
            colour: spots_of[i % spots_of.len()],
        });
        Self {
            palette,
            len,
            a,
            speed: rnd(40.0, 70.0),
            spine,
            spots,
            phase: rnd(0.0, 6.0),
            flee: 0.0,
        }
    }

    /// The board's `width(t)`: half its width at `t` along it.
    fn width(&self, t: f32) -> f32 {
        self.len
            * 0.135
            * (PI * (t * 0.92 + 0.07).min(1.0)).sin().max(0.0).powf(0.75)
            * (1.0 - t * 0.35)
    }
}

/// A lily pad.
#[derive(Debug, Clone, Copy)]
struct Pad {
    x: f32,
    y: f32,
    r: f32,
    a: f32,
    notch: f32,
    flower: bool,
}

/// What a step knows of the pointer, from the screen's corner.
#[derive(Debug, Clone, Copy, Default)]
pub struct Pointer {
    /// Resting on the water.
    pub resting: Option<(f32, f32)>,
}

/// The pond's floor: drawn by a worker, then kept until it is a texture.
#[derive(Debug)]
enum Floor {
    Drawing(mpsc::Receiver<Vec<u8>>),
    Drawn(Rc<Vec<u8>>),
    /// A texture now (`Pond::floor_texture`).
    Made,
}

/// The pond on one screen.
#[derive(Debug)]
pub struct Pond {
    /// Its size, logical.
    pub size: (f32, f32),
    /// Where its koi and pads keep: the screen's work area, from its
    /// corner (`x, y, w, h`).
    zone: [f32; 4],
    /// The heights' cells across and down, and the heights now and before.
    pub grid: (usize, usize),
    pub cur: Vec<f32>,
    prev: Vec<f32>,
    pub koi: Vec<Koi>,
    pads: Vec<Pad>,
    /// Its clock, seconds; when the next drop falls.
    time: f32,
    next_drop: f32,
    /// Where it was last clicked.
    click: Option<P>,
    /// A new one each step.
    generation: u64,
    /// Its water, the still water standing for it behind the glass of the
    /// bar, its pads, and the blank standing for them there.
    pub ids: [Id; 4],
    commit: Cell<CommitCounter>,
    drawn: Cell<u64>,
    floor: RefCell<Option<Floor>>,
    /// The floor's texture, and the still water's commit.
    pub floor_texture: RefCell<Option<Rc<dyn std::any::Any>>>,
    pub floor_commit: Cell<CommitCounter>,
    /// What is under the water, and its heights, kept between frames.
    pub under: RefCell<Option<Kept>>,
    pub heights: RefCell<Option<Heights>>,
    /// The generation the textures show.
    pub shown: Cell<Option<u64>>,
}

impl Pond {
    fn new(size: (f32, f32), zone: [f32; 4], rng: &mut Rng, seed: u64) -> Self {
        let gw = ((size.0 / CELL).ceil() as usize).max(3);
        let gh = ((size.1 / CELL).ceil() as usize).max(3);
        let boards = size.0 * size.1 / BOARD;
        let fish = ((KOI_A_BOARD * boards).round() as usize).max(1);
        let koi = (0..fish)
            .map(|n| Koi::new(n % PALETTE.len(), zone, rng))
            .collect();
        let [zx, zy, zw, zh] = zone;
        let pads = (0..((PADS_A_BOARD * boards).round() as usize).max(1))
            .map(|i| {
                let mut rnd = |a: f32, b: f32| a + rng.next() * (b - a);
                Pad {
                    x: rnd(zx + 80.0, (zx + zw - 90.0).max(zx + 81.0)),
                    y: rnd(zy + 80.0, (zy + zh - 120.0).max(zy + 81.0)),
                    r: rnd(30.0, 52.0),
                    a: rnd(0.0, std::f32::consts::TAU),
                    notch: rnd(0.0, std::f32::consts::TAU),
                    flower: i % 6 == 1 || i % 6 == 4,
                }
            })
            .collect();
        let (tx, rx) = mpsc::channel();
        let (w, h) = (size.0.ceil() as u32, size.1.ceil() as u32);
        let spawned = std::thread::Builder::new()
            .name("koi floor".into())
            .spawn(move || {
                // The receiver gone (the pond with it): nothing to tell.
                let _sent = tx.send(floor(w, h, seed));
            });
        if let Err(err) = &spawned {
            tracing::warn!(%err, "no worker for the koi's floor: no pond");
        }
        let mut pond = Self {
            size,
            zone,
            grid: (gw, gh),
            cur: vec![0.0; gw * gh],
            prev: vec![0.0; gw * gh],
            koi,
            pads,
            time: 0.0,
            next_drop: 1.5,
            click: None,
            generation: 0,
            ids: std::array::from_fn(|_| Id::new()),
            commit: Cell::new(CommitCounter::default()),
            drawn: Cell::new(u64::MAX),
            floor: RefCell::new(spawned.ok().map(|_| Floor::Drawing(rx))),
            floor_texture: RefCell::new(None),
            floor_commit: Cell::new(CommitCounter::default()),
            under: RefCell::new(None),
            heights: RefCell::new(None),
            shown: Cell::new(None),
        };
        for _ in 0..WARM_STEPS {
            pond.step(STEP, Pointer::default(), rng);
        }
        pond
    }

    /// Takes the floor the worker finished: whether it just came.
    fn poll_floor(&self) -> bool {
        let mut floor = self.floor.borrow_mut();
        let Some(Floor::Drawing(rx)) = floor.as_ref() else {
            return false;
        };
        match rx.try_recv() {
            Ok(pixels) => {
                *floor = Some(Floor::Drawn(Rc::new(pixels)));
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                tracing::warn!("the koi's floor was not drawn");
                *floor = None;
                false
            }
        }
    }

    /// Whether its floor is drawn.
    pub fn floor_ready(&self) -> bool {
        matches!(
            self.floor.borrow().as_ref(),
            Some(Floor::Drawn(_) | Floor::Made)
        )
    }

    /// The floor's pixels (premultiplied RGBA, its logical size), until
    /// they are a texture.
    pub fn floor_pixels(&self) -> Option<Rc<Vec<u8>>> {
        match self.floor.borrow().as_ref()? {
            Floor::Drawn(pixels) => Some(pixels.clone()),
            Floor::Drawing(_) | Floor::Made => None,
        }
    }

    /// Its floor is the texture `texture` now: its pixels go.
    pub fn floor_made(&self, texture: Rc<dyn std::any::Any>) {
        *self.floor_texture.borrow_mut() = Some(texture);
        *self.floor.borrow_mut() = Some(Floor::Made);
        let mut c = self.floor_commit.get();
        c.increment();
        self.floor_commit.set(c);
    }

    /// The board's `disturb`: the water at `(x, y)` raised by `amp` over
    /// `r`, a cosine's bump.
    fn disturb(&mut self, x: f32, y: f32, r: f32, amp: f32) {
        let (gw, gh) = self.grid;
        let (gx, gy, gr) = (x / CELL, y / CELL, r / CELL);
        let j0 = (gy - gr).floor().max(1.0) as usize;
        let j1 = (gy + gr).ceil().min(gh as f32 - 1.0).max(0.0) as usize;
        let i0 = (gx - gr).floor().max(1.0) as usize;
        let i1 = (gx + gr).ceil().min(gw as f32 - 1.0).max(0.0) as usize;
        for j in j0..j1 {
            for i in i0..i1 {
                let d = (i as f32 - gx).hypot(j as f32 - gy) / gr;
                if d < 1.0
                    && let Some(c) = self.cur.get_mut(j * gw + i)
                {
                    *c += amp * ((d * PI).cos() * 0.5 + 0.5);
                }
            }
        }
    }

    /// The board's `waves`: a step of the waves, damped.
    fn waves(&mut self) {
        let (gw, gh) = self.grid;
        let (cur, prev) = (&self.cur, &mut self.prev);
        for j in 1..gh - 1 {
            let row = j * gw;
            for i in 1..gw - 1 {
                let k = row + i;
                if let (Some(l), Some(r), Some(u), Some(d), Some(p)) = (
                    cur.get(k - 1),
                    cur.get(k + 1),
                    cur.get(k - gw),
                    cur.get(k + gw),
                    prev.get_mut(k),
                ) {
                    *p = ((l + r + u + d) * 0.5 - *p) * 0.978;
                }
            }
        }
        std::mem::swap(&mut self.cur, &mut self.prev);
    }

    /// The board's `swim`: koi `n`, `dt` seconds on.
    #[expect(
        clippy::indexing_slicing,
        reason = "a koi's joints are its spine's, SEG of them, looked at from 0 to SEG - 1"
    )]
    fn swim(&mut self, n: usize, dt: f32, pointer: Pointer, rng: &mut Rng) {
        let time = self.time;
        let [zx, zy, zw, zh] = self.zone;
        let click = self.click;
        let Some(f) = self.koi.get_mut(n) else {
            return;
        };
        let [hx, hy] = f.spine[0];
        let mut want = f.a
            + (time * 0.37 + f.phase).sin() * 0.9 * dt
            + (time * 1.3 + f.phase * 2.0).sin() * 0.6 * dt;
        let m = MARGIN;
        if hx < zx + m {
            want = f.a + ang_diff(f.a, 0.0) * 2.2 * dt;
        }
        if hx > zx + zw - m {
            want = f.a + ang_diff(f.a, PI) * 2.2 * dt;
        }
        if hy < zy + m {
            want = f.a + ang_diff(f.a, PI / 2.0) * 2.2 * dt;
        }
        if hy > zy + zh - m {
            want = f.a + ang_diff(f.a, -PI / 2.0) * 2.2 * dt;
        }
        let mut sp = f.speed;
        if f.flee > 0.0
            && let Some([cx, cy]) = click
        {
            f.flee -= dt;
            let away = (hy - cy).atan2(hx - cx);
            want = f.a + ang_diff(f.a, away) * 5.0 * dt;
            sp = f.speed * 3.2;
        } else if let Some((px, py)) = pointer.resting {
            let d = (px - hx).hypot(py - hy);
            if d < 420.0 {
                let to = (py - hy).atan2(px - hx);
                let circle = if d < 90.0 { to + PI / 2.0 } else { to };
                want = f.a + ang_diff(f.a, circle) * 1.8 * dt;
                sp = if d < 90.0 {
                    f.speed * 0.6
                } else {
                    f.speed * 1.4
                };
            }
        }
        let turn = ang_diff(f.a, want);
        let most = if f.flee > 0.0 { 3.2 } else { 1.4 } * dt;
        f.a += turn.clamp(-most, most);
        f.spine[0] = add(f.spine[0], mul(dir(f.a), sp * dt));
        let l = f.len / SEG as f32;
        for i in 1..SEG {
            let (p, q) = (f.spine[i - 1], f.spine[i]);
            let wag = (time * (4.0 + sp / 25.0) - i as f32 * 0.55 + f.phase).sin()
                * 0.06
                * (i as f32 / SEG as f32);
            let mut ang = angle(sub(q, p)) + wag;
            if i > 1 {
                let back = angle(sub(p, f.spine[i - 2]));
                let rel = ang_diff(back, ang);
                if rel.abs() > 0.32 {
                    ang = back + rel.signum() * 0.32;
                }
            }
            f.spine[i] = add(p, mul(dir(ang), l));
        }
        if rng.next() < 0.5 {
            self.disturb(hx, hy, 7.0, 0.06 * sp / 50.0);
        }
    }

    /// The board's frame: `dt` seconds on.
    fn step(&mut self, dt: f32, pointer: Pointer, rng: &mut Rng) {
        for n in 0..self.koi.len() {
            self.swim(n, dt, pointer, rng);
        }
        if self.time > self.next_drop {
            let [zx, zy, zw, zh] = self.zone;
            let mut rnd = |a: f32, b: f32| a + rng.next() * (b - a);
            let x = rnd(zx + 100.0, (zx + zw - 60.0).max(zx + 101.0));
            let y = rnd(zy + 80.0, (zy + zh - 100.0).max(zy + 81.0));
            self.disturb(x, y, 10.0, 3.0);
            self.next_drop = self.time + rnd(1.5, 4.0);
        }
        self.waves();
        self.waves();
        self.time += dt;
        self.generation += 1;
    }

    /// The board's `onDown`: a click at `(x, y)` rings the water and
    /// scatters the koi near.
    fn clicked(&mut self, x: f32, y: f32) {
        self.disturb(x, y, 26.0, 7.0);
        for f in &mut self.koi {
            let [hx, hy] = f.spine[0];
            if (hx - x).hypot(hy - y) < 260.0 {
                f.flee = 1.4;
            }
        }
        self.click = Some([x, y]);
    }

    /// The commit of this frame's drawing: a new one when it moved since
    /// it was last drawn.
    pub fn commit(&self) -> CommitCounter {
        let mut c = self.commit.get();
        if self.drawn.get() != self.generation {
            c.increment();
            self.commit.set(c);
            self.drawn.set(self.generation);
        }
        c
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The koi and their shadows, as the board draws them into what is
    /// under its water: in the picture's pixels (logical).
    pub fn under_mesh(&self) -> Mesh {
        let mut mesh = Mesh::default();
        for f in &self.koi {
            fish_shadow(&mut mesh, f, self.time);
        }
        for f in &self.koi {
            fish(&mut mesh, f, self.time);
        }
        mesh
    }

    /// The lily pads, on the water, bobbing with it: output-local,
    /// physical, from the screen's corner at `scale`.
    pub fn pads_mesh(&self, scale: f32) -> Mesh {
        let mut mesh = Mesh::default();
        let (gw, _) = self.grid;
        for p in &self.pads {
            let k = (p.y / CELL).floor().max(0.0) as usize * gw
                + (p.x / CELL).floor().max(0.0) as usize;
            let bob = self.cur.get(k).copied().unwrap_or(0.0) * 1.5;
            let rot = p.a + (self.time * 0.3 + p.notch).sin() * 0.05;
            pad(&mut mesh, p, [p.x, p.y + bob], rot, scale);
        }
        mesh
    }
}

/// A quad turned by `rot` about `centre` (in the frame `at`, `rot`
/// turned): its middle `c` and half sizes in that frame, its places across
/// from -1 to 1.
#[expect(clippy::too_many_arguments, reason = "a turned quad's parts")]
fn turned(
    mesh: &mut Mesh,
    at: P,
    rot: f32,
    c: P,
    half: P,
    turn: f32,
    colours: [[f32; 4]; 4],
    shape: Shape,
    numbers: [f32; 3],
    scale: f32,
) {
    let (fx, fy) = (dir(rot + turn), dir(rot + turn + PI / 2.0));
    let centre = add(at, add(mul(dir(rot), c[0]), mul(dir(rot + PI / 2.0), c[1])));
    let corner = |sx: f32, sy: f32| {
        mul(
            add(centre, add(mul(fx, sx * half[0]), mul(fy, sy * half[1]))),
            scale,
        )
    };
    mesh.quad_of(
        [
            corner(-1.0, -1.0),
            corner(1.0, -1.0),
            corner(1.0, 1.0),
            corner(-1.0, 1.0),
        ],
        [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]],
        colours,
        shape,
        numbers,
    );
}

/// The board's `drawPad`: its shadow, its leaf (a disc but a notch, its
/// gradient and veins), and on some a water lily.
fn pad(mesh: &mut Mesh, p: &Pad, at: P, rot: f32, scale: f32) {
    let r = p.r;
    turned(
        mesh,
        at,
        rot,
        [6.0, 8.0],
        [r, r * 0.96],
        0.0,
        [[0.0, 0.0, 0.0, 0.25]; 4],
        Shape::Pad,
        [r * 0.96 * scale, 1.0, scale],
        scale,
    );
    turned(
        mesh,
        at,
        rot,
        [0.0, 0.0],
        [r, r],
        0.0,
        [[1.0; 4]; 4],
        Shape::Pad,
        [r * scale, 0.0, scale],
        scale,
    );
    if p.flower {
        for ring in 0..2 {
            let tip = if ring == 0 {
                [0xEC, 0x8F, 0xAA]
            } else {
                [0xF7, 0xB7, 0xC8]
            };
            let base = rgb([0xFF, 0xF3, 0xF6], 1.0);
            let tip = rgb(tip, 1.0);
            for k in 0..8 {
                let a = k as f32 * PI / 4.0 + ring as f32 * 0.39;
                let len = r * if ring == 1 { 0.42 } else { 0.6 };
                // The petal's gradient from its base (0) to `len`, along
                // it: its quad reaches from -0.05 to 1.05 of it.
                let c = |x: f32| {
                    let t = x.clamp(0.0, 1.0);
                    let mut out = base;
                    for (o, b) in out.iter_mut().zip(tip) {
                        *o += (b - *o) * t;
                    }
                    out
                };
                let (back, front) = (c(-0.05), c(1.05));
                turned(
                    mesh,
                    at,
                    rot,
                    mul(dir(a), len * 0.5),
                    [len * 0.55, len * 0.2],
                    a,
                    [back, front, front, back],
                    Shape::Oval,
                    [len * 0.2 * scale, 0.0, 0.0],
                    scale,
                );
            }
        }
        let d = r * 0.12;
        turned(
            mesh,
            at,
            rot,
            [0.0, 0.0],
            [d, d],
            0.0,
            [rgb([0xF6, 0xD3, 0x5B], 1.0); 4],
            Shape::Oval,
            [d * scale, 0.0, 0.0],
            scale,
        );
    }
}

/// A koi's body as triangles: each vertex its place and its place on the
/// body (`u` along from its first joint, `v` across, left positive).
struct Body {
    vertices: Vec<(P, P)>,
    /// The outline, in order round it (indices into `vertices`).
    outline: Vec<usize>,
    triangles: Vec<[usize; 3]>,
}

/// The board's `outline` and `bodyPath`: the sides at each joint, the
/// head's two curves from the first joint's sides to the snout.
#[expect(
    clippy::indexing_slicing,
    reason = "a koi's joints are its spine's, SEG of them, looked at from 0 to SEG - 1"
)]
fn body(f: &Koi) -> Body {
    let s = &f.spine;
    let l = f.len / SEG as f32;
    let mut vertices = Vec::with_capacity(4 * SEG + 16);
    let mut lefts = Vec::with_capacity(SEG);
    let mut rights = Vec::with_capacity(SEG);
    for i in 0..SEG {
        let (p, q, o) = (s[i], s[(i + 1).min(SEG - 1)], s[i.saturating_sub(1)]);
        let ang = angle(sub(o, q));
        let w = f.width(i as f32 / (SEG - 1) as f32);
        let u = i as f32 * l;
        lefts.push(vertices.len());
        vertices.push((add(p, mul(dir(ang + PI / 2.0), w)), [u, w]));
        rights.push(vertices.len());
        vertices.push((add(p, mul(dir(ang - PI / 2.0), w)), [u, -w]));
    }
    let h = s[0];
    let a = angle(sub(h, s[1]));
    let (fwd, left) = (dir(a), dir(a + PI / 2.0));
    let w = f.width(0.0) + f.len * 0.05;
    let tip = add(h, mul(fwd, w * 1.3));
    let c1 = add(h, add(mul(fwd, w * 1.6), mul(left, w * 0.4)));
    let c2 = add(h, sub(mul(fwd, w * 1.6), mul(left, w * 0.4)));
    let on_body = |p: P| {
        let d = sub(p, h);
        [-dot(d, fwd), dot(d, left)]
    };
    let (l0, r0) = (vertices[lefts[0]].0, vertices[rights[0]].0);
    let centre = vertices.len();
    vertices.push((h, [0.0, 0.0]));
    // The head: from the left side round the snout to the right.
    let mut head = vec![lefts[0]];
    for (from, ctrl, to) in [(l0, c1, tip), (tip, c2, r0)] {
        for k in 1..=5 {
            let t = k as f32 / 5.0;
            if to == r0 && k == 5 {
                head.push(rights[0]);
            } else {
                let p = quad_at(from, ctrl, to, t);
                head.push(vertices.len());
                vertices.push((p, on_body(p)));
            }
        }
    }
    let mut triangles = Vec::with_capacity(2 * SEG + head.len());
    for pair in head.windows(2) {
        triangles.push([centre, pair[0], pair[1]]);
    }
    for i in 0..SEG - 1 {
        triangles.push([lefts[i], lefts[i + 1], rights[i + 1]]);
        triangles.push([lefts[i], rights[i + 1], rights[i]]);
    }
    // Round it: the head from the left to the right, down the right side,
    // up the left.
    let mut outline = head;
    outline.extend(rights.iter().skip(1));
    outline.extend(lefts.iter().skip(1).rev());
    Body {
        vertices,
        outline,
        triangles,
    }
}

impl Body {
    /// Its triangles in `mesh`, each vertex's colour, shape, place across
    /// and numbers from `look` (its place and its place on the body).
    fn emit(&self, mesh: &mut Mesh, look: impl Fn(P, P) -> ([f32; 4], Shape, P, [f32; 3])) {
        let mut at = Vec::with_capacity(self.vertices.len());
        for &(p, uv) in &self.vertices {
            let (colour, shape, local, numbers) = look(p, uv);
            match mesh.vertex(p, local, colour, shape, numbers) {
                Some(i) => at.push(i),
                None => return,
            }
        }
        for [a, b, c] in &self.triangles {
            if let (Some(a), Some(b), Some(c)) = (at.get(*a), at.get(*b), at.get(*c)) {
                mesh.triangle(*a, *b, *c);
            }
        }
    }

    /// A fringe a pixel wide round its outline, from `colour` to nothing:
    /// its edge smoothed.
    fn fringe(&self, mesh: &mut Mesh, colour: [f32; 4]) {
        polygon_fringe(
            mesh,
            &self
                .outline
                .iter()
                .filter_map(|i| self.vertices.get(*i).map(|v| v.0))
                .collect::<Vec<_>>(),
            colour,
        );
    }
}

/// A fringe a pixel wide out of the closed polygon `points`, from
/// `colour` at it to nothing.
#[expect(
    clippy::indexing_slicing,
    reason = "its places looked at modulo their number, the ring as many"
)]
fn polygon_fringe(mesh: &mut Mesh, points: &[P], colour: [f32; 4]) {
    let n = points.len();
    if n < 3 {
        return;
    }
    let area: f32 = (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    let out = if area > 0.0 { 1.0 } else { -1.0 };
    let mut ring = Vec::with_capacity(n);
    for i in 0..n {
        let (prev, p, next) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
        let t = sub(next, prev);
        let len = t[0].hypot(t[1]).max(1.0e-3);
        let normal = [t[1] / len * out, -t[0] / len * out];
        let inner = mesh.vertex(p, [0.0, 0.0], colour, Shape::Fill, [0.0; 3]);
        let outer = mesh.vertex(add(p, normal), [0.0, 0.0], [0.0; 4], Shape::Fill, [0.0; 3]);
        let (Some(inner), Some(outer)) = (inner, outer) else {
            return;
        };
        ring.push((inner, outer));
    }
    for i in 0..n {
        let ((a, b), (c, d)) = (ring[i], ring[(i + 1) % n]);
        mesh.triangle(a, c, d);
        mesh.triangle(a, d, b);
    }
}

/// The board's fins, as a fan of `points` (closed, round `centre`).
#[expect(
    clippy::indexing_slicing,
    reason = "its vertices looked at modulo their number"
)]
fn fan(mesh: &mut Mesh, centre: P, points: &[P], colour: [f32; 4]) {
    let Some(c) = mesh.vertex(centre, [0.0, 0.0], colour, Shape::Fill, [0.0; 3]) else {
        return;
    };
    let mut at = Vec::with_capacity(points.len());
    for p in points {
        match mesh.vertex(*p, [0.0, 0.0], colour, Shape::Fill, [0.0; 3]) {
            Some(i) => at.push(i),
            None => return,
        }
    }
    for i in 0..at.len() {
        mesh.triangle(c, at[i], at[(i + 1) % at.len()]);
    }
}

/// The tail fin's outline (the board's three curves), from the tail out.
fn tail_fin(f: &Koi, time: f32) -> (P, Vec<P>) {
    let s = &f.spine;
    let (tail, pre) = (s[SEG - 1], s[SEG - 3]);
    let flap = (time * 6.0 + f.phase).sin() * 0.35;
    let rot = angle(sub(tail, pre)) + flap * 0.5;
    let k = f.len;
    let at = |x: f32, y: f32| {
        add(
            tail,
            add(mul(dir(rot), x * k), mul(dir(rot + PI / 2.0), y * k)),
        )
    };
    let curves = [
        ([0.0, 0.0], [0.25, -0.2], [0.32, -0.14]),
        ([0.32, -0.14], [0.2, 0.0], [0.32, 0.14]),
        ([0.32, 0.14], [0.25, 0.2], [0.0, 0.0]),
    ];
    let mut points = Vec::with_capacity(18);
    for (a, c, b) in curves {
        for i in 0..6 {
            let p = quad_at(a, c, b, i as f32 / 6.0);
            points.push(at(p[0], p[1]));
        }
    }
    (at(0.12, 0.0), points)
}

/// The pectoral fins: each its middle, its half sizes and its angle.
fn side_fins(f: &Koi, time: f32) -> [(P, P, f32); 2] {
    let s = &f.spine;
    let flap = (time * 6.0 + f.phase).sin() * 0.35;
    let a = angle(sub(s[3], s[4]));
    [-1.0, 1.0].map(|side| {
        let turn = a + side * (2.1 + flap);
        (
            add(s[3], mul(dir(turn), f.len * 0.12)),
            [f.len * 0.13, f.len * 0.055],
            turn,
        )
    })
}

/// A koi's shadow on the floor: its fins and its body, the board's
/// `rgba(0, 10, 12, 0.35)` blurred 6 px, 14 px right and 20 px down.
#[expect(
    clippy::indexing_slicing,
    reason = "a koi's joints, SEG of them; the tail fin's eighteen points"
)]
fn fish_shadow(mesh: &mut Mesh, f: &Koi, time: f32) {
    let colour = [0.0, 10.0 / 255.0 * 0.35, 12.0 / 255.0 * 0.35, 0.35];
    let shift = [14.0, 20.0];
    let sigma = SHADOW_SIGMA;
    let soft = |r: f32| r / (2.0 * sigma);
    let (centre, points) = tail_fin(f, time);
    let tail = add(centre, mul(sub(points[6], centre), 0.35));
    let tail_dir = angle(sub(points[6], f.spine[SEG - 1]));
    let (rx, ry) = (f.len * 0.2, f.len * 0.15);
    turned(
        mesh,
        add(tail, shift),
        tail_dir,
        [0.0, 0.0],
        [rx + sigma, ry + sigma],
        0.0,
        [colour; 4],
        Shape::Oval,
        [soft(ry + sigma), 0.0, 0.0],
        1.0,
    );
    for (c, half, turn) in side_fins(f, time) {
        turned(
            mesh,
            add(c, shift),
            turn,
            [0.0, 0.0],
            [half[0] + sigma, half[1] + sigma],
            0.0,
            [colour; 4],
            Shape::Oval,
            [soft(half[1] + sigma), 0.0, 0.0],
            1.0,
        );
    }
    // The body: a band along the spine as wide as the koi, blurred across
    // and at its ends (a box blurred, laid along it).
    let s = &f.spine;
    let l = f.len / SEG as f32;
    let a = angle(sub(s[0], s[1]));
    let w0 = f.width(0.0) + f.len * 0.05;
    let mut line: Vec<(P, f32, f32)> = vec![
        (add(s[0], mul(dir(a), w0 * 1.3)), 0.25 * w0, -w0 * 1.3),
        (add(s[0], mul(dir(a), w0 * 0.65)), 0.75 * w0, -w0 * 0.65),
    ];
    for (i, p) in s.iter().enumerate() {
        line.push((*p, f.width(i as f32 / (SEG - 1) as f32), i as f32 * l));
    }
    let (u0, u1) = (-w0 * 1.3, (SEG - 1) as f32 * l);
    let (mid, half_len) = ((u0 + u1) / 2.0, (u1 - u0) / 2.0);
    let reach = 2.0 * sigma;
    let n = line.len();
    let mut rows = Vec::with_capacity(n + 2);
    for k in 0..n {
        let (p, w, u) = line[k];
        let (q0, q1) = (line[k.saturating_sub(1)].0, line[(k + 1).min(n - 1)].0);
        let along = angle(sub(q0, q1));
        rows.push((p, along, w, u));
    }
    // Past each end, as far as the blur reaches.
    if let (Some(&first), Some(&last)) = (rows.first(), rows.last()) {
        rows.insert(
            0,
            (
                add(first.0, mul(dir(first.1), reach)),
                first.1,
                first.2,
                first.3 - reach,
            ),
        );
        rows.push((
            add(last.0, mul(dir(last.1), -reach)),
            last.1,
            last.2,
            last.3 + reach,
        ));
    }
    let mut prev: Option<[u16; 2]> = None;
    for (p, along, w, u) in rows {
        let side = dir(along + PI / 2.0);
        let across = w + reach;
        let mut pair = [0u16; 2];
        for (slot, sign) in pair.iter_mut().zip([1.0, -1.0]) {
            let at = add(add(p, mul(side, sign * across)), shift);
            match mesh.vertex(
                at,
                [u - mid, sign * across],
                colour,
                Shape::Box,
                [half_len, w, sigma],
            ) {
                Some(i) => *slot = i,
                None => return,
            }
        }
        if let Some([a, b]) = prev {
            mesh.triangle(a, pair[0], pair[1]);
            mesh.triangle(a, pair[1], b);
        }
        prev = Some(pair);
    }
}

/// The board's `drawFish`: its fins, its body of its colour, its spots on
/// it, a sheen from its head, a darker line down its back, its eyes.
#[expect(
    clippy::indexing_slicing,
    reason = "a koi's joints are its spine's, SEG of them, looked at from 0 to SEG - 1"
)]
fn fish(mesh: &mut Mesh, f: &Koi, time: f32) {
    let (base, _) = PALETTE[f.palette];
    let fins = if f.palette == GOLDEN {
        rgb([250, 200, 90], 0.55)
    } else {
        rgb([245, 240, 232], 0.42)
    };
    let (centre, points) = tail_fin(f, time);
    fan(mesh, centre, &points, fins);
    for (c, half, turn) in side_fins(f, time) {
        turned(
            mesh,
            c,
            turn,
            [0.0, 0.0],
            half,
            0.0,
            [fins; 4],
            Shape::Oval,
            [half[1], 0.0, 0.0],
            1.0,
        );
    }
    let b = body(f);
    let solid = rgb(base, 1.0);
    b.emit(mesh, |_, _| (solid, Shape::Fill, [0.0, 0.0], [0.0; 3]));
    b.fringe(mesh, solid);
    let s = &f.spine;
    for spot in &f.spots {
        let i = (spot.t * (SEG - 1) as f32).round() as usize;
        let w = f.width(spot.t);
        let (cu, cv) = (i as f32 * f.len / SEG as f32, w * spot.off);
        let (ru, rv) = (f.len * spot.rx, w * spot.ry * 1.6);
        let colour = rgb(spot.colour, 1.0);
        b.emit(mesh, |_, [u, v]| {
            (
                colour,
                Shape::Oval,
                [(u - cu) / ru, (v - cv) / rv],
                [ru.min(rv), 0.0, 0.0],
            )
        });
    }
    // The sheen: white from the head (25 %) to nothing at its middle.
    let (h, m) = (s[0], s[SEG / 2]);
    let axis = sub(m, h);
    let length2 = dot(axis, axis).max(1.0e-3);
    b.emit(mesh, |p, _| {
        let t = (dot(sub(p, h), axis) / length2).clamp(0.0, 1.0);
        let a = 0.25 * (1.0 - t);
        ([a, a, a, a], Shape::Fill, [0.0, 0.0], [0.0; 3])
    });
    // The line down its back, `len × 0.08` wide, from the second joint to
    // the third from the tail.
    let half = f.len * 0.04;
    let dark = [0.0, 0.0, 0.0, 0.12];
    let mut prev: Option<[u16; 2]> = None;
    for i in 1..=SEG - 3 {
        let (a, c) = (s[i - 1], s[(i + 1).min(SEG - 3)]);
        let t = if i == SEG - 3 {
            sub(s[i], s[i - 1])
        } else {
            sub(c, a)
        };
        let side = dir(angle(t) + PI / 2.0);
        let mut pair = [0u16; 2];
        for (slot, sign) in pair.iter_mut().zip([1.0, -1.0]) {
            match mesh.vertex(
                add(s[i], mul(side, sign * half)),
                [0.0, 0.0],
                dark,
                Shape::Fill,
                [0.0; 3],
            ) {
                Some(v) => *slot = v,
                None => return,
            }
        }
        if let Some([a, b]) = prev {
            mesh.triangle(a, pair[0], pair[1]);
            mesh.triangle(a, pair[1], b);
        }
        prev = Some(pair);
    }
    let a0 = angle(sub(s[0], s[1]));
    let ew = f.width(0.06) * 0.7;
    let r = f.len * 0.012;
    for side in [-1.0, 1.0] {
        let at = add(
            add(s[0], mul(dir(a0), f.len * 0.04)),
            mul(dir(a0 + side * PI / 2.0), ew),
        );
        mesh.square(
            at,
            [r, r],
            [1.0, 1.0],
            rgb([0x11, 0x11, 0x11], 1.0),
            Shape::Oval,
            [r, 0.0, 0.0],
        );
    }
}

/// The board's floor, `w` by `h`, premultiplied RGBA: its radial gradient
/// (from 50 px round 40 %, 45 % to 75 % of the width round the middle:
/// `#2C5A5E`, `#163B42` at 60 %, `#0A1E24`), its pebbles (as many per
/// pixel as its 900), darkened toward its edges (multiplied by a gradient
/// from white at 30 % of the height to `rgb(90, 110, 115)` at 70 % of the
/// width).
pub fn floor(w: u32, h: u32, seed: u64) -> Vec<u8> {
    let Some(mut pixmap) = tiny_skia::Pixmap::new(w.max(1), h.max(1)) else {
        return Vec::new();
    };
    let (fw, fh) = (w as f32, h as f32);
    let (c0, r0) = ([fw * 0.4, fh * 0.45], 50.0);
    let (c1, r1) = ([fw * 0.5, fh * 0.5], fw * 0.75);
    let stops: [(f32, [f32; 3]); 3] = [
        (0.0, [44.0, 90.0, 94.0]),
        (0.6, [22.0, 59.0, 66.0]),
        (1.0, [10.0, 30.0, 36.0]),
    ];
    let e = sub(c1, c0);
    let dr = r1 - r0;
    let qa = dot(e, e) - dr * dr;
    for (i, px) in pixmap.pixels_mut().iter_mut().enumerate() {
        let (x, y) = ((i as u32 % w) as f32 + 0.5, (i as u32 / w) as f32 + 0.5);
        // The two-circle gradient: the larger `t` whose circle passes
        // through the pixel.
        let q = sub([x, y], c0);
        let qb = dot(q, e) + r0 * dr;
        let qc = dot(q, q) - r0 * r0;
        let t = ((qb - (qb * qb - qa * qc).max(0.0).sqrt()) / qa).clamp(0.0, 1.0);
        let t = if qa.abs() < 1.0e-6 { 0.0 } else { t };
        let c = gradient(&stops, t);
        *px = tiny_skia::ColorU8::from_rgba(c[0] as u8, c[1] as u8, c[2] as u8, 255).premultiply();
    }
    let mut rng = Rng::new(seed);
    let mut rnd = |a: f32, b: f32| a + rng.next() * (b - a);
    let pebbles = (PEBBLES_A_BOARD * fw * fh / BOARD).round() as usize;
    for _ in 0..pebbles {
        let (x, y) = (rnd(0.0, fw), rnd(0.0, fh));
        let r = rnd(0.0, 1.0).powi(3) * 16.0 + 2.0;
        let l = rnd(-18.0, 22.0);
        let alpha = rnd(0.12, 0.35);
        let ry = r * rnd(0.6, 0.9);
        let turn = rnd(0.0, 3.0);
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(
            (70.0 + l) as u8,
            (92.0 + l) as u8,
            (88.0 + l) as u8,
            (alpha * 255.0).round() as u8,
        );
        paint.anti_alias = true;
        let Some(oval) = tiny_skia::Rect::from_xywh(-r, -ry, 2.0 * r, 2.0 * ry)
            .and_then(tiny_skia::PathBuilder::from_oval)
        else {
            continue;
        };
        let at = tiny_skia::Transform::from_rotate(turn.to_degrees()).post_translate(x, y);
        pixmap.fill_path(&oval, &paint, tiny_skia::FillRule::Winding, at, None);
    }
    // The vignette, multiplied.
    let (cx, cy) = (fw / 2.0, fh / 2.0);
    let (v0, v1) = (fh * 0.3, fw * 0.7);
    let edge = [90.0 / 255.0, 110.0 / 255.0, 115.0 / 255.0];
    let mut data = pixmap.take();
    for (i, px) in data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = ((i as u32 % w) as f32 + 0.5, (i as u32 / w) as f32 + 0.5);
        let t = (((x - cx).hypot(y - cy) - v0) / (v1 - v0)).clamp(0.0, 1.0);
        for (c, e) in px.iter_mut().zip(edge) {
            *c = (f32::from(*c) * (1.0 + (e - 1.0) * t)).round() as u8;
        }
    }
    data
}

/// A gradient's colour at `t` between its stops.
fn gradient(stops: &[(f32, [f32; 3])], t: f32) -> [f32; 3] {
    let mut at = stops.first().map_or([0.0; 3], |s| s.1);
    for pair in stops.windows(2) {
        let (Some(&(t0, a)), Some(&(t1, b))) = (pair.first(), pair.get(1)) else {
            continue;
        };
        if t >= t0 {
            let k = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
            at = a;
            for (o, b) in at.iter_mut().zip(b) {
                *o += (b - *o) * k;
            }
        }
    }
    at
}

/// The pond on every screen it shows on.
#[derive(Debug)]
pub struct Ponds {
    start: Duration,
    steps: u64,
    rng: Rng,
    seed: u64,
    /// By screen name.
    pub panes: HashMap<String, Pond>,
    /// Where the pointer was last seen, and since when it is there.
    pointer: Option<((f64, f64), Duration)>,
    /// Clicks on the water since the last step: the screen, where (from
    /// its corner).
    clicks: Vec<(String, (f32, f32))>,
}

impl Ponds {
    pub fn new(start: Duration, seed: u64) -> Self {
        Self {
            start,
            steps: 0,
            rng: Rng::new(seed),
            seed,
            panes: HashMap::new(),
            pointer: None,
            clicks: Vec::new(),
        }
    }

    /// Screen `name` is `size` (logical), its work area `zone` (from its
    /// corner): its pond, dug when it is new or its size changed. Whether
    /// it was.
    pub fn pond(&mut self, name: &str, size: (f32, f32), zone: [f32; 4]) -> bool {
        if let Some(pond) = self.panes.get_mut(name)
            && pond.size == size
        {
            pond.zone = zone;
            return false;
        }
        self.seed = self.seed.wrapping_add(1);
        let pond = Pond::new(size, zone, &mut self.rng, self.seed);
        self.panes.insert(name.to_owned(), pond);
        true
    }

    /// Only the screens of `names` keep theirs.
    pub fn retain_panes(&mut self, names: &[&str]) {
        self.panes.retain(|n, _| names.contains(&n.as_str()));
    }

    /// Steps it to `now` on the screens of `on`, each with where the
    /// pointer rests on it (from its corner).
    pub fn advance(&mut self, now: Duration, on: &[(String, Option<(f32, f32)>)]) {
        for (name, (x, y)) in std::mem::take(&mut self.clicks) {
            if let Some(pond) = self.panes.get_mut(&name) {
                pond.clicked(x, y);
            }
        }
        let due = (now.saturating_sub(self.start).as_secs_f64() / f64::from(STEP)) as u64;
        let from = self.steps.max(due.saturating_sub(MOST_STEPS));
        for _ in from..due {
            for (name, resting) in on {
                if let Some(pond) = self.panes.get_mut(name) {
                    let pointer = Pointer { resting: *resting };
                    pond.step(STEP, pointer, &mut self.rng);
                }
            }
        }
        self.steps = self.steps.max(due);
    }
}

impl crate::State {
    /// How the koi's pond shows now (#803).
    pub(crate) fn koi_plays(&self) -> Option<super::Plays> {
        self.wallpaper_effect(self.config.effects.koi)
    }

    /// Whether the koi's pond is `output`'s wallpaper now: its floor
    /// drawn, no fullscreen window in front.
    pub(crate) fn koi_shows_on(&self, output: &smithay::output::Output) -> bool {
        self.koi_plays().is_some()
            && self
                .fx
                .koi
                .as_ref()
                .and_then(|k| k.panes.get(&output.name()))
                .is_some_and(Pond::floor_ready)
            && !self.fullscreen_in_front(output)
    }

    /// A click at `at` (global, logical) on the wallpaper: on a pond, its
    /// water rings and its koi scatter.
    pub(crate) fn koi_click(&mut self, at: smithay::utils::Point<f64, smithay::utils::Logical>) {
        if self.koi_plays() != Some(super::Plays::Moving) {
            return;
        }
        let Some(output) = self.space.output_under(at).next().cloned() else {
            return;
        };
        if !self.koi_shows_on(&output) {
            return;
        }
        let Some(area) = self.space.output_geometry(&output) else {
            return;
        };
        if let Some(ponds) = self.fx.koi.as_mut() {
            let local = (
                (at.x - f64::from(area.loc.x)) as f32,
                (at.y - f64::from(area.loc.y)) as f32,
            );
            ponds.clicks.push((output.name(), local));
        }
    }

    /// The koi's pond moves on to `now` (#803): dug as it is turned on, on
    /// each screen; its koi drawn to where the pointer rests on the water;
    /// stepped on the screens that are on and show no fullscreen window.
    /// Whether it moves (a frame is wanted).
    pub(crate) fn tick_koi(&mut self, now: Duration) -> bool {
        let Some(plays) = self.koi_plays() else {
            if self.config.effects.koi && self.fx.koi.is_some() {
                // Waiting (the power saver, a share): kept, not drawn.
                return false;
            }
            if self.fx.koi.take().is_some() {
                tracing::debug!("the koi's pond goes");
                self.redraw = true;
            }
            return false;
        };
        if self.fx.koi.is_none() {
            self.fx.ponds += 1;
            tracing::debug!(?plays, "a koi pond");
            self.fx.koi = Some(Ponds::new(now, self.fx.ponds));
            self.redraw = true;
        }
        let pointer = self
            .seat
            .get_pointer()
            .map(|p| p.current_location())
            .unwrap_or_default();
        let on_water = !self.locked() && self.surface_under(pointer).is_none();
        let pointer_screen = self.space.output_under(pointer).next().map(|o| o.name());
        let mut screens = Vec::new();
        for output in self.space.outputs() {
            let (Some(area), Some(zone)) =
                (self.space.output_geometry(output), self.work_area(output))
            else {
                continue;
            };
            let name = output.name();
            let on = self.output_powered(output) && !self.fullscreen_in_front(output);
            let size = (area.size.w as f32, area.size.h as f32);
            let zone = [
                (zone.x - area.loc.x) as f32,
                (zone.y - area.loc.y) as f32,
                zone.width as f32,
                zone.height as f32,
            ];
            let corner = (f64::from(area.loc.x), f64::from(area.loc.y));
            screens.push((name, size, zone, on, corner));
        }
        let Some(ponds) = self.fx.koi.as_mut() else {
            return false;
        };
        let mut dug = false;
        for (name, size, zone, _, _) in &screens {
            dug |= ponds.pond(name, *size, *zone);
        }
        let names: Vec<&str> = screens.iter().map(|s| s.0.as_str()).collect();
        ponds.retain_panes(&names);
        // At rest: not moved more than a few pixels for a moment, over the
        // water.
        let at = (pointer.x, pointer.y);
        let since = match ponds.pointer {
            Some((last, since)) if (last.0 - at.0).hypot(last.1 - at.1) <= REST_MOVE => since,
            _ => now,
        };
        ponds.pointer = Some((at, since));
        let resting = on_water && now.saturating_sub(since) > REST_AFTER;
        // A floor drawn since: a frame to show it.
        let mut fresh = false;
        for pond in ponds.panes.values() {
            fresh |= pond.poll_floor();
        }
        let on: Vec<(String, Option<(f32, f32)>)> = screens
            .into_iter()
            .filter(|s| s.3)
            .map(|(name, _, _, _, corner)| {
                let rest = (resting && pointer_screen.as_deref() == Some(name.as_str()))
                    .then_some(((at.0 - corner.0) as f32, (at.1 - corner.1) as f32));
                (name, rest)
            })
            .collect();
        let moving = plays == super::Plays::Moving && !on.is_empty();
        if moving {
            ponds.advance(now, &on);
        }
        if dug || fresh {
            self.redraw = true;
        }
        // While a floor is being drawn, frames come to look for it.
        let drawing = ponds
            .panes
            .values()
            .any(|p| matches!(p.floor.borrow().as_ref(), Some(Floor::Drawing(_))));
        moving || drawing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn koi_swim_bend_within_their_limits_and_come_to_the_pointer() {
        let mut rng = Rng::new(9);
        let zone = [0.0, 0.0, 960.0, 720.0];
        let mut pond = Pond::new((1200.0, 720.0), zone, &mut rng, 1);
        assert_eq!(pond.koi.len(), 7);
        let rest = (480.0, 360.0);
        let near = |p: &Pond| {
            p.koi
                .iter()
                .filter(|f| (f.spine[0][0] - rest.0).hypot(f.spine[0][1] - rest.1) < 200.0)
                .count()
        };
        let before = near(&pond);
        for _ in 0..60 * 12 {
            pond.step(
                STEP,
                Pointer {
                    resting: Some(rest),
                },
                &mut rng,
            );
            for f in &pond.koi {
                for i in 2..SEG {
                    let back = angle(sub(f.spine[i - 1], f.spine[i - 2]));
                    let ang = angle(sub(f.spine[i], f.spine[i - 1]));
                    assert!(ang_diff(back, ang).abs() <= 0.32 + 1.0e-3);
                }
            }
        }
        assert!(near(&pond) > before, "{before} → {}", near(&pond));
        // The water moves: some heights off zero.
        assert!(pond.cur.iter().any(|h| h.abs() > 0.01));
        // A click: the koi near flee, fast.
        let [x, y] = pond.koi[0].spine[0];
        pond.clicked(x, y);
        assert!(pond.koi[0].flee > 0.0);
    }
}
