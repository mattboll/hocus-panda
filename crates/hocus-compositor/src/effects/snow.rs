//! Snow that settles (#795, the "Neige qui tient" board of the effects
//! canvas, its JS ported): flakes fall on three depth planes with the
//! wind's gusts and their own sway, behind the windows; those of the middle
//! plane settle on the top edges of the windows and of the bar, and pile at
//! the bottom of the screen. Each pile is a heightfield (a column every
//! 4 px on a window, every 6 px on the ground) whose slopes slide down to
//! an angle of repose, its ends thinned. A window moved sharply sheds its
//! snow in clumps that fall with gravity, inherit its speed and break up on
//! the ground, which grows.
//!
//! Stepped at a fixed 60 steps a second from its start, as the particles
//! are ([`super::particles`]), with a seeded generator: a moment is the
//! same whatever the frames' pace. Its places are logical pixels: a
//! window's cap global, a screen's flakes, clumps and ground from its
//! corner. Drawn by [`super::snow_look`].

use std::{
    cell::Cell,
    collections::{BTreeMap, HashMap},
    time::Duration,
};

use smithay::backend::renderer::{element::Id, utils::CommitCounter};

use super::particles::{Rng, STEP};
use crate::model::WindowId;

/// A cap's column, logical pixels; its highest.
pub const COL: f32 = 4.0;
pub const MAX_CAP: f32 = 24.0;
/// The ground's column; its highest.
pub const GROUND_COL: f32 = 6.0;
pub const MAX_GROUND: f32 = 46.0;
/// The board's 520 flakes over its 1200 × 720 stage: as many per pixel.
const FLAKES_PER_PIXEL: f32 = 520.0 / (1200.0 * 720.0);
/// The middle plane, where flakes settle: their depth between these.
const PLANE: (f32, f32) = (0.42, 0.8);
/// Its share of the flakes (depth = u^1.4, u uniform: u from 0.42^(1/1.4)
/// to 0.8^(1/1.4)), and their mean fall, pixels a second.
const PLANE_U: (f32, f32) = (0.5381, 0.8527);
const PLANE_SPEED: f32 = 58.3;
/// The board's warm-up: 1500 steps of 1/30 s before the first frame.
const WARM_STEPS: u32 = 1500;
const WARM_STEP: f32 = 1.0 / 30.0;
/// A window sheds from this acceleration on, pixels a second each second,
/// all of it this much beyond.
pub const SHED_FROM: f64 = 2600.0;
const SHED_SPAN: f64 = 9000.0;
/// At most this many steps at once (10 s): after a longer pause the snow
/// goes on from where it was.
const MOST_STEPS: u64 = 600;

/// What holds a cap: a window, or the bar of a screen (its name).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Holder {
    Window(WindowId),
    Bar(String),
}

/// The snow on a top edge: its place (global, logical) and its columns'
/// heights.
#[derive(Debug)]
pub struct Cap {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: Vec<f32>,
    /// Drawn: the same id over its frames, a new commit as it changes.
    pub id: Id,
    pub commit: Cell<CommitCounter>,
    pub drawn: Cell<u64>,
}

impl Cap {
    fn new(x: f32, y: f32, w: f32) -> Self {
        Self {
            x,
            y,
            w,
            h: vec![0.0; columns(w, COL)],
            id: Id::new(),
            commit: Cell::new(CommitCounter::default()),
            drawn: Cell::new(u64::MAX),
        }
    }

    /// Follows its edge; a wider or narrower edge keeps the columns it
    /// still has.
    fn place(&mut self, x: f32, y: f32, w: f32) {
        self.x = x;
        self.y = y;
        self.w = w;
        self.h.resize(columns(w, COL), 0.0);
    }

    /// The snow's top at `x` (global), when over the cap.
    fn surface_at(&self, x: f32) -> Option<f32> {
        let i = ((x - self.x) / COL).floor();
        let h = (i >= 0.0).then(|| self.h.get(i as usize)).flatten()?;
        Some(self.y - h)
    }

    /// `amount` of snow falls at `x` (global), most in its column.
    fn deposit(&mut self, x: f32, amount: f32) {
        let i = ((x - self.x) / COL).floor() as isize;
        for k in -1..=1isize {
            let j = i + k;
            if j >= 0
                && let Some(h) = self.h.get_mut(j as usize)
            {
                *h = (*h + amount * if k == 0 { 0.6 } else { 0.2 }).min(MAX_CAP);
            }
        }
    }

    /// How much snow it holds, square logical pixels.
    pub fn volume(&self) -> f32 {
        self.h.iter().sum::<f32>() * COL
    }

    /// The commit of this frame's drawing: a new one when the snow
    /// changed since it was last drawn.
    pub fn commit(&self, generation: u64) -> CommitCounter {
        commit_for(&self.commit, &self.drawn, generation)
    }
}

fn commit_for(commit: &Cell<CommitCounter>, drawn: &Cell<u64>, generation: u64) -> CommitCounter {
    let mut c = commit.get();
    if drawn.get() != generation {
        c.increment();
        commit.set(c);
        drawn.set(generation);
    }
    c
}

fn columns(width: f32, col: f32) -> usize {
    (width.max(0.0) / col).ceil() as usize
}

/// A flake, from its screen's corner.
#[derive(Debug, Clone, Copy, Default)]
pub struct Flake {
    pub x: f32,
    pub y: f32,
    /// Its depth, from far (0) to near (1): its size, speed and alpha.
    pub z: f32,
    pub r: f32,
    speed: f32,
    phase: f32,
    sway: f32,
}

/// A clump shed by a window, from its screen's corner.
#[derive(Debug, Clone, Copy)]
pub struct Clump {
    pub x: f32,
    pub y: f32,
    vx: f32,
    vy: f32,
    pub r: f32,
    pub rot: f32,
    vr: f32,
}

/// One screen's snow: its flakes, its ground, the clumps falling on it.
#[derive(Debug)]
pub struct Field {
    /// Its corner (global) and size, logical.
    pub origin: (f32, f32),
    pub size: (f32, f32),
    pub flakes: Vec<Flake>,
    pub ground: Vec<f32>,
    pub clumps: Vec<Clump>,
    /// Drawn: the layer's id, and the still one standing for it behind the
    /// glass.
    pub ids: [Id; 2],
    pub commit: Cell<CommitCounter>,
    pub drawn: Cell<u64>,
}

impl Field {
    fn new(origin: (f32, f32), size: (f32, f32), rng: &mut Rng) -> Self {
        let mut field = Self {
            origin,
            size,
            flakes: Vec::new(),
            ground: Vec::new(),
            clumps: Vec::new(),
            ids: [Id::new(), Id::new()],
            commit: Cell::new(CommitCounter::default()),
            drawn: Cell::new(u64::MAX),
        };
        field.resize(origin, size, rng);
        field
    }

    /// Its screen moved or changed size: as many flakes as it takes, the
    /// ground as wide.
    fn resize(&mut self, origin: (f32, f32), size: (f32, f32), rng: &mut Rng) {
        self.origin = origin;
        if self.size == size && !self.flakes.is_empty() {
            return;
        }
        self.size = size;
        let (w, h) = size;
        let count = (FLAKES_PER_PIXEL * w * h).round() as usize;
        self.flakes.truncate(count);
        while self.flakes.len() < count {
            let mut f = Flake::default();
            spawn(&mut f, true, size, rng);
            self.flakes.push(f);
        }
        let from = self.ground.len();
        self.ground.resize(columns(w, GROUND_COL), 0.0);
        for (i, g) in self.ground.iter_mut().enumerate().skip(from) {
            // The board's rolling ground to start from.
            let x = i as f32 * GROUND_COL;
            *g = 14.0
                + 9.0 * (x * 0.006 + 1.3).sin()
                + 6.0 * (x * 0.017 + 0.4).sin()
                + 3.0 * (x * 0.043).sin();
        }
        self.clumps.retain(|k| k.x >= -40.0 && k.x <= w + 40.0);
    }

    /// The commit of this frame's drawing.
    pub fn commit(&self, generation: u64) -> CommitCounter {
        commit_for(&self.commit, &self.drawn, generation)
    }

    /// The ground column under `x`, if on the screen.
    fn ground_at(&self, x: f32) -> Option<usize> {
        let i = (x / GROUND_COL).floor();
        (i >= 0.0 && (i as usize) < self.ground.len()).then_some(i as usize)
    }
}

/// The board's `spawn`: a flake anywhere on the screen, or above it.
fn spawn(f: &mut Flake, anywhere: bool, (w, h): (f32, f32), rng: &mut Rng) {
    let mut rnd = |a: f32, b: f32| a + rng.next() * (b - a);
    f.z = rnd(0.0, 1.0).powf(1.4);
    f.x = rnd(-60.0, w + 60.0);
    f.y = if anywhere {
        rnd(-20.0, h)
    } else {
        rnd(-40.0, -5.0)
    };
    f.r = 0.6 + f.z * 2.8;
    f.speed = 16.0 + f.z * 70.0;
    f.phase = rnd(0.0, std::f32::consts::TAU);
    f.sway = rnd(6.0, 18.0);
}

/// The board's `relax`: a slope steeper than `repose` a column slides,
/// a quarter of its excess each step; `ends`: the first two and the last
/// two columns thinned.
#[expect(
    clippy::indexing_slicing,
    reason = "i and i + 1 are under the length; the ends' four with three columns or more"
)]
fn relax(h: &mut [f32], repose: f32, ends: bool) {
    for i in 0..h.len().saturating_sub(1) {
        let d = h[i] - h[i + 1];
        if d.abs() > repose {
            let m = (d.abs() - repose) * 0.25 * d.signum();
            h[i] -= m;
            h[i + 1] += m;
        }
    }
    let n = h.len();
    if ends && n >= 3 {
        h[0] = h[0].min(h[1] * 0.45);
        h[1] = h[1].min(h[2] * 0.8);
        h[n - 1] = h[n - 1].min(h[n - 2] * 0.45);
        h[n - 2] = h[n - 2].min(h[n - 3] * 0.8);
    }
}

/// The snow, while it is on.
#[derive(Debug)]
pub struct Snow {
    start: Duration,
    steps: u64,
    rng: Rng,
    /// The wind now, pixels a second.
    pub wind: f32,
    pub fields: BTreeMap<String, Field>,
    pub caps: HashMap<Holder, Cap>,
    /// Bumped as the snow changes: a frame draws it again then.
    pub generation: u64,
}

impl Snow {
    pub fn new(start: Duration, seed: u64) -> Self {
        Self {
            start,
            steps: 0,
            rng: Rng::new(seed),
            wind: 0.0,
            fields: BTreeMap::new(),
            caps: HashMap::new(),
            generation: 0,
        }
    }

    /// Its clock, seconds: the wind's and the glints'.
    pub fn time(&self) -> f32 {
        self.steps as f32 * STEP
    }

    /// The cap of `holder` follows its edge (global, logical), a new one
    /// bare.
    pub fn place(&mut self, holder: &Holder, x: f32, y: f32, w: f32) {
        match self.caps.get_mut(holder) {
            Some(cap) => cap.place(x, y, w),
            None => {
                self.caps.insert(holder.clone(), Cap::new(x, y, w));
            }
        }
    }

    /// The caps whose holder `keep` says no to go.
    pub fn retain_caps(&mut self, mut keep: impl FnMut(&Holder) -> bool) {
        self.caps.retain(|h, _| keep(h));
    }

    /// Screen `name` (its corner and size, global and logical) shows the
    /// snow, the caps of `on` over it. A screen seen first starts with its
    /// piles grown ([`Self::warm`]).
    pub fn field(&mut self, name: &str, origin: (f32, f32), size: (f32, f32), on: &[Holder]) {
        if let Some(field) = self.fields.get_mut(name) {
            field.resize(origin, size, &mut self.rng);
            return;
        }
        let mut field = Field::new(origin, size, &mut self.rng);
        self.warm(&mut field, on);
        self.fields.insert(name.to_owned(), field);
        self.generation += 1;
    }

    /// The screens not in `names` go.
    pub fn retain_fields(&mut self, names: &[&str]) {
        self.fields.retain(|n, _| names.contains(&n.as_str()));
    }

    /// The board's warm-up, the piles grown as if it had snowed 50 s: the
    /// flakes of the middle plane that would have landed in that time,
    /// each on the highest edge under it, else on the ground, the slopes
    /// sliding between them.
    fn warm(&mut self, field: &mut Field, on: &[Holder]) {
        let (w, h) = field.size;
        // Landings a pixel of width a second: the plane's flakes a square
        // pixel (spread from 60 px beyond the sides and 20 above, as they
        // are born) by their fall.
        let flux = FLAKES_PER_PIXEL * (PLANE_U.1 - PLANE_U.0) * PLANE_SPEED * (w * h)
            / ((w + 120.0) * (h + 20.0));
        let each = flux * w * WARM_STEP;
        let mut due = 0.0;
        for _ in 0..WARM_STEPS {
            due += each;
            while due >= 1.0 {
                due -= 1.0;
                let x = field.origin.0 + self.rng.next() * w;
                let u = PLANE_U.0 + self.rng.next() * (PLANE_U.1 - PLANE_U.0);
                let r = 0.6 + u.powf(1.4) * 2.8;
                let top = on
                    .iter()
                    .filter_map(|holder| {
                        let cap = self.caps.get(holder)?;
                        let y = cap.surface_at(x)?;
                        (y >= field.origin.1).then_some((holder, y))
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                match top.and_then(|(holder, _)| self.caps.get_mut(holder)) {
                    Some(cap) => cap.deposit(x, 2.4 * r),
                    None => {
                        if let Some(g) = field
                            .ground_at(x - field.origin.0)
                            .and_then(|i| field.ground.get_mut(i))
                        {
                            *g = (*g + 1.3 * r).min(MAX_GROUND);
                        }
                    }
                }
            }
            for holder in on {
                if let Some(cap) = self.caps.get_mut(holder) {
                    relax(&mut cap.h, 2.2, true);
                }
            }
            relax(&mut field.ground, 1.0, false);
        }
    }

    /// The cap of `holder` sheds `strength` of its snow (1: all), as its
    /// window moves at `v` (pixels a second): clumps thrown up and along
    /// onto screen `field`.
    pub fn shed(&mut self, holder: &Holder, field: &str, v: (f32, f32), strength: f32) {
        let (Some(cap), Some(field)) = (self.caps.get_mut(holder), self.fields.get_mut(field))
        else {
            return;
        };
        let rng = &mut self.rng;
        let mut rnd = |a: f32, b: f32| a + rng.next() * (b - a);
        let mut shed = false;
        let (cx, cy) = (cap.x, cap.y);
        for (i, h) in cap.h.iter_mut().enumerate() {
            let take = *h * strength.min(1.0);
            if take < 0.4 {
                continue;
            }
            *h -= take;
            shed = true;
            if rnd(0.0, 1.0) < 0.55 {
                field.clumps.push(Clump {
                    x: cx + i as f32 * COL - field.origin.0,
                    y: cy - *h - take * 0.5 - field.origin.1,
                    vx: v.0 * 0.7 + rnd(-60.0, 60.0),
                    vy: v.1 * 0.7 - rnd(40.0, 200.0),
                    r: 1.5 + take * 0.32,
                    rot: rnd(0.0, 6.0),
                    vr: rnd(-5.0, 5.0),
                });
            }
        }
        if shed {
            self.generation += 1;
        }
    }

    /// Steps it to `now`, each screen of `on` with the caps over it.
    pub fn advance(&mut self, now: Duration, on: &[(String, Vec<Holder>)]) {
        let due = (now.saturating_sub(self.start).as_secs_f64() / f64::from(STEP)) as u64;
        let from = self.steps.max(due.saturating_sub(MOST_STEPS));
        for step in from..due {
            let time = step as f32 * STEP;
            self.wind = (time * 0.31).sin() * 26.0 + (time * 1.13 + 1.2).sin() * 12.0;
            for (name, holders) in on {
                if let Some(field) = self.fields.get_mut(name) {
                    fall(
                        field,
                        &mut self.caps,
                        holders,
                        &mut self.rng,
                        self.wind,
                        time,
                    );
                }
            }
            for cap in self.caps.values_mut() {
                relax(&mut cap.h, 2.2, true);
            }
            self.generation += 1;
        }
        self.steps = self.steps.max(due);
    }
}

/// A step of a screen's snow: the board's `sim` but the caps' slopes.
fn fall(
    field: &mut Field,
    caps: &mut HashMap<Holder, Cap>,
    on: &[Holder],
    rng: &mut Rng,
    wind: f32,
    time: f32,
) {
    let dt = STEP;
    let (ox, oy) = field.origin;
    let size = field.size;
    let (w, h) = size;
    let mut ground = std::mem::take(&mut field.ground);
    for f in &mut field.flakes {
        let vx = wind * (0.35 + 0.65 * f.z) + (f.phase + time * 1.4).sin() * f.sway * f.z;
        let py = f.y;
        f.x += vx * dt;
        f.y += f.speed * dt;
        if f.z > PLANE.0 && f.z < PLANE.1 {
            let (gx, gy, gpy) = (f.x + ox, f.y + oy, py + oy);
            let mut landed = false;
            for holder in on {
                let Some(cap) = caps.get_mut(holder) else {
                    continue;
                };
                if let Some(sy) = cap.surface_at(gx)
                    && gpy <= sy
                    && gy >= sy
                {
                    cap.deposit(gx, 2.4 * f.r);
                    landed = true;
                    break;
                }
            }
            if !landed {
                let i = (f.x / GROUND_COL).floor();
                if i >= 0.0
                    && let Some(g) = ground.get_mut(i as usize)
                    && f.y >= h - *g
                {
                    *g = (*g + 1.3 * f.r).min(MAX_GROUND);
                    landed = true;
                }
            }
            if landed {
                spawn(f, false, size, rng);
                continue;
            }
        }
        if f.y > h + 10.0 || f.x < -80.0 || f.x > w + 80.0 {
            spawn(f, false, size, rng);
        }
    }
    relax(&mut ground, 1.0, false);
    field.ground = ground;
    let mut born = Vec::new();
    let mut rnd = |a: f32, b: f32| a + rng.next() * (b - a);
    field.clumps.retain_mut(|k| {
        k.vy += 1500.0 * dt;
        k.vx *= 0.995;
        k.x += k.vx * dt;
        k.y += k.vy * dt;
        k.rot += k.vr * dt;
        let i = (k.x / GROUND_COL).floor();
        let column = (i >= 0.0)
            .then(|| field.ground.get(i as usize).map(|g| (i as usize, *g)))
            .flatten();
        if let Some((i, g)) = column
            && k.y + k.r >= field.size.1 - g
        {
            // On the ground: it grows, and a big clump breaks in four.
            for d in -2..=2isize {
                let j = i as isize + d;
                if j >= 0
                    && let Some(g) = field.ground.get_mut(j as usize)
                {
                    *g = (*g + k.r * 0.5 / (1.0 + d.unsigned_abs() as f32)).min(MAX_GROUND);
                }
            }
            if k.r > 3.5 {
                for _ in 0..4 {
                    born.push(Clump {
                        x: k.x,
                        y: field.size.1 - field.ground.get(i).copied().unwrap_or(g) - 2.0,
                        vx: rnd(-140.0, 140.0),
                        vy: rnd(-260.0, -80.0),
                        r: k.r * 0.4,
                        rot: 0.0,
                        vr: rnd(-6.0, 6.0),
                    });
                }
            }
            return false;
        }
        !(k.y > h + 40.0 || k.x < -40.0 || k.x > w + 40.0)
    });
    field.clumps.extend(born);
}

/// How much of its snow a window shed at acceleration `acc`: none under
/// [`SHED_FROM`].
pub fn shed_strength(acc: f64) -> Option<f32> {
    (acc > SHED_FROM).then(|| ((acc - SHED_FROM) / SHED_SPAN) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn window(n: u64) -> Holder {
        Holder::Window(WindowId(n))
    }

    /// A 1200 × 720 screen as the board's, a window on it, the snow
    /// started at 1 s.
    fn board() -> (Snow, Vec<(String, Vec<Holder>)>) {
        let mut snow = Snow::new(ms(1000), 5);
        snow.place(&window(1), 540.0, 196.0, 400.0);
        snow.place(&Holder::Bar("one".into()), 990.0, 16.0, 188.0);
        let on = vec![window(1), Holder::Bar("one".into())];
        snow.field("one", (0.0, 0.0), (1200.0, 720.0), &on);
        (snow, vec![("one".to_owned(), on)])
    }

    #[test]
    fn warmed_up_the_piles_hold_snow_thinned_at_their_ends() {
        let (snow, _) = board();
        let cap = &snow.caps[&window(1)];
        let mean = cap.volume() / COL / cap.h.len() as f32;
        // The board's: some 10 px after 50 s, under 24.
        assert!(mean > 4.0 && mean < MAX_CAP, "{mean}");
        assert!(cap.h[0] <= cap.h[1] * 0.45 + 1e-3);
        assert!(snow.caps[&Holder::Bar("one".into())].volume() > 0.0);
        let field = &snow.fields["one"];
        assert_eq!(field.flakes.len(), 520);
        assert!(field.ground.iter().all(|g| *g > 0.0 && *g <= MAX_GROUND));
        // The slopes at rest: no column more than 2.2 px over the next
        // but by a step's sliding.
        for pair in cap.h.windows(2) {
            assert!((pair[0] - pair[1]).abs() < 4.0, "{pair:?}");
        }
    }

    #[test]
    fn it_snows_on_and_the_same_whatever_the_frames() {
        let (mut once, on) = board();
        let before = once.caps[&window(1)].volume();
        once.advance(ms(11_000), &on);
        let after = once.caps[&window(1)].volume();
        assert!(after > before, "{before} then {after}");
        let (mut framed, on) = board();
        for t in (1000..=11_000).step_by(16) {
            framed.advance(ms(t), &on);
        }
        framed.advance(ms(11_000), &on);
        assert_eq!(framed.caps[&window(1)].h, once.caps[&window(1)].h);
        assert_eq!(framed.fields["one"].ground, once.fields["one"].ground);
    }

    #[test]
    fn a_window_shaken_sheds_its_snow_in_clumps_that_land_on_the_ground() {
        let (mut snow, on) = board();
        let ground: f32 = snow.fields["one"].ground.iter().sum();
        assert!(shed_strength(2000.0).is_none());
        let strength = shed_strength(31_000.0).unwrap_or_default();
        snow.shed(&window(1), "one", (800.0, 0.0), strength);
        let cap = &snow.caps[&window(1)];
        assert!(cap.h.iter().all(|h| *h < 0.4), "{:?}", cap.h);
        let clumps = snow.fields["one"].clumps.len();
        assert!(clumps > 20, "{clumps} clumps");
        // Thrown along with the window, then down.
        snow.advance(ms(1100), &on);
        assert!(snow.fields["one"].clumps.iter().any(|k| k.x > 560.0));
        snow.advance(ms(4000), &on);
        assert!(snow.fields["one"].clumps.is_empty());
        let grown: f32 = snow.fields["one"].ground.iter().sum();
        assert!(grown > ground, "{ground} then {grown}");
    }

    #[test]
    fn a_cap_follows_its_edge_and_keeps_its_columns() {
        let (mut snow, _) = board();
        let h = snow.caps[&window(1)].h.clone();
        snow.place(&window(1), 600.0, 300.0, 440.0);
        let cap = &snow.caps[&window(1)];
        assert_eq!((cap.x, cap.y), (600.0, 300.0));
        assert_eq!(cap.h.len(), 110);
        assert_eq!(cap.h[..100], h[..]);
        snow.retain_caps(|h| matches!(h, Holder::Bar(_)));
        assert!(!snow.caps.contains_key(&window(1)));
    }
}
