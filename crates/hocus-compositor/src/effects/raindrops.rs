//! Rain on the glass (#802, the board "Pluie sur la vitre", its JS
//! ported): the wallpaper becomes wet glass. Small drops sit on it, many
//! and still; big ones form and grow, then slide down, the faster the
//! bigger, wavering, swallowing the drops in their way, wiping a trail
//! above them and leaving a few small ones behind. Drawn by
//! `crate::render::raindrops`: each drop a lens that refracts the
//! wallpaper, the glass between them the wallpaper blurred and darkened.
//!
//! Stepped at a fixed 60 steps a second from its start (the board's
//! frames: its per-frame terms stay per step), with a seeded generator; as
//! many drops per pixel as the board's over its 1200 × 720 stage. Its
//! places are logical pixels from its screen's corner. The drops sit in a
//! grid of cells, so that a sliding drop looks only at those near it.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    time::Duration,
};

use smithay::backend::renderer::{element::Id, utils::CommitCounter};

use super::{
    mesh::{Mesh, Shape},
    particles::{Rng, STEP},
};
use crate::wobble::Kept;

/// The board's stage, square pixels.
const BOARD: f32 = 1200.0 * 720.0;
/// The board's still drops at first, at most, and added a frame; its
/// sliding drops at first, and a second.
const STILL_START: f32 = 900.0;
const STILL_MOST: f32 = 1100.0;
const STILL_A_STEP: f32 = 6.0;
const ROLLING_START: f32 = 20.0;
const ROLLING_A_SECOND: f32 = 2.4;
/// A drop slides from this radius on; grows to this at most.
const SLIDES_FROM: f32 = 13.0;
const LARGEST: f32 = 24.0;
/// The board's warm-up: 300 steps of 1/30 s before its first frame.
const WARM_STEPS: u32 = 300;
const WARM_STEP: f32 = 1.0 / 30.0;
/// At most this many steps at once (10 s): after a longer pause the rain
/// goes on from where it was.
const MOST_STEPS: u64 = 600;
/// The drops' map is drawn at this share of the screen's logical pixels:
/// the board's half, its texels' linear filtering softening the drops.
pub const MAP_SCALE: f32 = 0.5;
/// A cell of the grid, logical pixels.
const CELL: f32 = 32.0;
/// Around a drop changed, what is drawn again: the map's filtering reaches
/// a texel (2 logical pixels) beyond its sprite.
const PAD: f32 = 3.0;
/// More rectangles than this to draw again: the whole screen.
const MOST_RECTS: usize = 192;

/// A drop sitting still.
#[derive(Debug, Clone, Copy)]
pub struct Still {
    pub x: f32,
    pub y: f32,
    pub r: f32,
}

impl Still {
    /// Its sprite's radius, as the board draws it.
    fn reach(&self) -> f32 {
        self.r * 1.15
    }

    fn rect(&self) -> [f32; 4] {
        let r = self.reach() + PAD;
        [self.x - r, self.y - r, self.x + r, self.y + r]
    }
}

/// A big drop: growing where it formed, then sliding.
#[derive(Debug, Clone, Copy)]
pub struct Rolling {
    pub x: f32,
    pub y: f32,
    pub r: f32,
    vy: f32,
    wob: f32,
    grow: f32,
    shift: f32,
}

impl Rolling {
    /// Its sprite: its middle and half sizes, an oval a little taller than
    /// wide, as the board draws it.
    fn oval(&self) -> ([f32; 2], [f32; 2]) {
        let r = self.r * 1.1;
        ([self.x, self.y - 0.03 * r], [0.92 * r, 1.15 * r])
    }

    fn rect(&self) -> [f32; 4] {
        let ([x, y], [hx, hy]) = self.oval();
        [x - hx - PAD, y - hy - PAD, x + hx + PAD, y + hy + PAD]
    }
}

/// The rain on one screen.
#[derive(Debug)]
pub struct Pane {
    /// Its size, logical.
    pub size: (f32, f32),
    cols: usize,
    rows: usize,
    /// The still drops, by cell.
    cells: Vec<Vec<Still>>,
    stills: usize,
    pub rolling: Vec<Rolling>,
    /// Its area in boards.
    boards: f32,
    /// Still drops owed (a share of one a step).
    owed: f32,
    /// A new one each step that changes it.
    generation: u64,
    /// What changed since it was last drawn (logical, from its corner);
    /// everything when `all`.
    changed: RefCell<Vec<[f32; 4]>>,
    all: Cell<bool>,
    /// The rain's element and the fogged glass standing for it behind the
    /// glass of the bar.
    pub ids: [Id; 2],
    drawn: RefCell<Drawn>,
    /// The drops' map, kept between frames.
    pub map: RefCell<Option<Kept>>,
}

/// What a pane's last drawing was.
#[derive(Debug, Default)]
struct Drawn {
    generation: u64,
    commit: CommitCounter,
    /// The commit before, and what changed since (`None`: everything).
    since: Option<(CommitCounter, Option<Vec<[f32; 4]>>)>,
    /// The fogged glass alone: a new commit with a new wallpaper.
    fog: CommitCounter,
    /// The blurred wallpaper it was drawn over.
    blur: Option<Id>,
    /// The generation its map shows.
    mapped: Option<u64>,
}

/// A pane's drawing this frame.
#[derive(Debug)]
pub struct PaneFrame {
    pub commit: CommitCounter,
    /// The commit before, and the rectangles that changed since (logical,
    /// from its corner); `None`: all of it.
    pub since: Option<(CommitCounter, Vec<[f32; 4]>)>,
    pub fog: CommitCounter,
    /// Whether its map is to be drawn again.
    pub fresh_map: bool,
}

impl Pane {
    fn new(size: (f32, f32), rng: &mut Rng) -> Self {
        let cols = ((size.0 / CELL).ceil() as usize).max(1);
        let rows = ((size.1 / CELL).ceil() as usize).max(1);
        let mut pane = Self {
            size,
            cols,
            rows,
            cells: vec![Vec::new(); cols * rows],
            stills: 0,
            rolling: Vec::new(),
            boards: size.0 * size.1 / BOARD,
            owed: 0.0,
            generation: 0,
            changed: RefCell::new(Vec::new()),
            all: Cell::new(true),
            ids: [Id::new(), Id::new()],
            drawn: RefCell::new(Drawn::default()),
            map: RefCell::new(None),
        };
        for _ in 0..(STILL_START * pane.boards).round() as usize {
            pane.add_still(rng);
        }
        for _ in 0..(ROLLING_START * pane.boards).round() as usize {
            pane.add_big(rng);
        }
        for _ in 0..WARM_STEPS {
            pane.step(WARM_STEP, rng);
        }
        pane.changed.get_mut().clear();
        pane.all.set(true);
        pane
    }

    /// How many drops sit still, and slide or grow.
    pub fn counts(&self) -> (usize, usize) {
        (self.stills, self.rolling.len())
    }

    fn cell_of(&self, x: f32, y: f32) -> usize {
        let col = ((x / CELL).floor().max(0.0) as usize).min(self.cols - 1);
        let row = ((y / CELL).floor().max(0.0) as usize).min(self.rows - 1);
        row * self.cols + col
    }

    fn put(&mut self, drop: Still) {
        let at = self.cell_of(drop.x, drop.y);
        if let Some(cell) = self.cells.get_mut(at) {
            cell.push(drop);
            self.stills += 1;
            self.changed.get_mut().push(drop.rect());
        }
    }

    fn add_still(&mut self, rng: &mut Rng) {
        let (w, h) = self.size;
        let r = rng.next().powf(2.2) * 6.0 + 1.4;
        self.put(Still {
            x: rng.next() * w,
            y: rng.next() * h,
            r,
        });
    }

    fn add_big(&mut self, rng: &mut Rng) {
        let (w, h) = self.size;
        let mut rnd = |a: f32, b: f32| a + rng.next() * (b - a);
        let drop = Rolling {
            x: rnd(0.0, w),
            y: rnd(-40.0, h * 0.6),
            r: rnd(8.0, 13.0),
            vy: 0.0,
            wob: rnd(0.0, 9.0),
            grow: rnd(0.4, 1.2),
            shift: 0.0,
        };
        self.changed.get_mut().push(drop.rect());
        self.rolling.push(drop);
    }

    /// The board's `sim`: `dt` seconds on.
    fn step(&mut self, dt: f32, rng: &mut Rng) {
        self.owed += STILL_A_STEP * self.boards;
        while self.owed >= 1.0 {
            self.owed -= 1.0;
            if (self.stills as f32) < STILL_MOST * self.boards {
                self.add_still(rng);
            }
        }
        if rng.next() < dt * ROLLING_A_SECOND * self.boards {
            self.add_big(rng);
        }
        let mut i = self.rolling.len();
        while i > 0 {
            i -= 1;
            let Some(mut d) = self.rolling.get(i).copied() else {
                continue;
            };
            let before = d.rect();
            if d.r < SLIDES_FROM {
                d.r += d.grow * dt;
                self.changed.get_mut().push(union(before, d.rect()));
                if let Some(slot) = self.rolling.get_mut(i) {
                    *slot = d;
                }
                continue;
            }
            let target = 30.0 + d.r * 14.0;
            let stutter = if rng.next() < 0.06 { 0.1 } else { 1.0 };
            d.vy += (target - d.vy) * (dt * 3.0).min(1.0) * stutter;
            d.wob += dt * 3.0;
            d.shift += ((d.wob * 1.7).sin() * 0.6 + (d.wob * 0.43).sin() * 0.8) * dt * 18.0;
            let (ox, oy) = (d.x, d.y);
            d.y += d.vy * dt;
            let shift = d.shift;
            d.shift *= 0.98;
            d.x += d.wob.sin() * 0.4 + (shift - d.shift);
            if rng.next() < dt * 10.0 && d.r > 11.0 {
                let r = 1.8 + rng.next() * 2.2;
                let x = ox + (rng.next() * 3.0 - 1.5);
                self.put(Still { x, y: oy, r });
                d.r -= 0.12;
            }
            self.meet(&mut d);
            self.changed.get_mut().push(union(before, d.rect()));
            if d.y - d.r > self.size.1 {
                self.rolling.remove(i);
            } else if let Some(slot) = self.rolling.get_mut(i) {
                *slot = d;
            }
        }
        self.generation += 1;
    }

    /// Drop `d` swallows the still drops it touches, growing, and wipes
    /// those of the trail above it.
    fn meet(&mut self, d: &mut Rolling) {
        // As far as a drop at its largest reaches: its trail up, a still
        // drop's radius (7.4 at most) around.
        let reach = LARGEST + 8.0;
        let (c0, r0) = self.grid(d.x - reach, d.y - LARGEST * 6.0);
        let (c1, r1) = self.grid(d.x + reach, d.y + reach);
        let mut gone = Vec::new();
        for row in r0..=r1 {
            for col in c0..=c1 {
                let Some(cell) = self.cells.get_mut(row * self.cols + col) else {
                    continue;
                };
                cell.retain(|s| {
                    let near = d.r + s.r;
                    if (s.y - d.y).abs() < near
                        && (s.x - d.x).abs() < near
                        && (s.x - d.x).hypot(s.y - d.y) < d.r + s.r * 0.6
                    {
                        d.r = (d.r * d.r + s.r * s.r * 0.6).sqrt().min(LARGEST);
                        gone.push(s.rect());
                        false
                    } else if s.y > d.y - d.r * 6.0 && s.y < d.y && (s.x - d.x).abs() < d.r * 0.9 {
                        gone.push(s.rect());
                        false
                    } else {
                        true
                    }
                });
            }
        }
        self.stills -= gone.len();
        self.changed.get_mut().extend(gone);
    }

    /// The cell of the grid a point falls in: column, row.
    fn grid(&self, x: f32, y: f32) -> (usize, usize) {
        let at = self.cell_of(x, y);
        (at % self.cols, at / self.cols)
    }

    /// Its drawing this frame over the blurred wallpaper `blur`: a new
    /// commit when it changed since it was last drawn, and what changed.
    pub fn frame(&self, blur: &Id) -> PaneFrame {
        let mut d = self.drawn.borrow_mut();
        let new_wall = d.blur.as_ref() != Some(blur);
        if new_wall {
            d.blur = Some(blur.clone());
            d.fog.increment();
        }
        if new_wall || d.generation != self.generation {
            let prev = d.commit;
            d.commit.increment();
            d.generation = self.generation;
            let changed = std::mem::take(&mut *self.changed.borrow_mut());
            let all = self.all.replace(false) || new_wall || changed.len() > MOST_RECTS;
            d.since = Some((prev, (!all).then_some(changed)));
        }
        let fresh_map = d.mapped != Some(self.generation);
        d.mapped = Some(self.generation);
        PaneFrame {
            commit: d.commit,
            since: d
                .since
                .as_ref()
                .and_then(|(prev, rects)| Some((*prev, rects.clone()?))),
            fog: d.fog,
            fresh_map,
        }
    }

    /// Its drops as the map draws them: each a lens, its normal and its
    /// thickness in its colour (`Shape::Drop`), the still ones first;
    /// places in the map's pixels.
    pub fn map_mesh(&self) -> Mesh {
        let mut mesh = Mesh::default();
        let k = MAP_SCALE;
        for s in self.cells.iter().flatten() {
            let r = s.reach() * k;
            mesh.square(
                [s.x * k, s.y * k],
                [r, r],
                [1.0, 1.0],
                [1.0; 4],
                Shape::Drop,
                [0.0; 3],
            );
        }
        for d in &self.rolling {
            let ([x, y], [hx, hy]) = d.oval();
            mesh.square(
                [x * k, y * k],
                [hx * k, hy * k],
                [1.0, 1.0],
                [1.0; 4],
                Shape::Drop,
                [0.0; 3],
            );
        }
        mesh
    }

    /// The map's size, pixels.
    pub fn map_size(&self) -> (i32, i32) {
        (
            ((self.size.0 * MAP_SCALE).ceil() as i32).max(1),
            ((self.size.1 * MAP_SCALE).ceil() as i32).max(1),
        )
    }
}

fn union(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

/// The rain on every screen it shows on.
#[derive(Debug)]
pub struct Rain {
    start: Duration,
    steps: u64,
    rng: Rng,
    /// By screen name.
    pub panes: HashMap<String, Pane>,
}

impl Rain {
    pub fn new(start: Duration, seed: u64) -> Self {
        Self {
            start,
            steps: 0,
            rng: Rng::new(seed),
            panes: HashMap::new(),
        }
    }

    /// Screen `name` is `size` (logical): its pane, made (warmed up as
    /// the board's) when it is new or its size changed. Whether it was.
    pub fn pane(&mut self, name: &str, size: (f32, f32)) -> bool {
        if self.panes.get(name).is_some_and(|p| p.size == size) {
            return false;
        }
        let pane = Pane::new(size, &mut self.rng);
        self.panes.insert(name.to_owned(), pane);
        true
    }

    /// Only the screens of `names` keep theirs.
    pub fn retain_panes(&mut self, names: &[&str]) {
        self.panes.retain(|n, _| names.contains(&n.as_str()));
    }

    /// Steps it to `now` on the screens of `on`.
    pub fn advance(&mut self, now: Duration, on: &[String]) {
        let due = (now.saturating_sub(self.start).as_secs_f64() / f64::from(STEP)) as u64;
        let from = self.steps.max(due.saturating_sub(MOST_STEPS));
        for _ in from..due {
            for name in on {
                if let Some(pane) = self.panes.get_mut(name) {
                    pane.step(STEP, &mut self.rng);
                }
            }
        }
        self.steps = self.steps.max(due);
    }
}

impl crate::State {
    /// How the rain on the glass shows now (#802).
    pub(crate) fn raining_glass(&self) -> Option<super::Plays> {
        self.wallpaper_effect(self.config.effects.rain_on_glass)
    }

    /// Whether the rain on the glass shows on `output` now: the wallpaper
    /// it refracts is then its still picture (or the daylight's sky,
    /// #805), a moving one waits; the koi's pond (#803) and the aurora
    /// (#801), which move, take its place.
    pub(crate) fn rain_shows_on(&self, output: &smithay::output::Output) -> bool {
        self.raining_glass().is_some()
            && (!self.config.plain_background || self.sky_shows_on(output))
            && !self.koi_shows_on(output)
            && !self.aurora_shows_on(output)
            && self
                .fx
                .raindrops
                .as_ref()
                .is_some_and(|r| r.panes.contains_key(&output.name()))
            && !self.wall_hidden(output)
    }

    /// The rain on the glass moves on to `now` (#802): started as it is
    /// turned on, a pane warmed up on each screen; stepped on the screens
    /// that are on and show no fullscreen window. Whether it moves (a
    /// frame is wanted).
    pub(crate) fn tick_raindrops(&mut self, now: Duration) -> bool {
        let Some(plays) = self.raining_glass() else {
            if self.config.effects.rain_on_glass && self.fx.raindrops.is_some() {
                // Waiting (the power saver, a share): kept, not drawn.
                return false;
            }
            if self.fx.raindrops.take().is_some() {
                tracing::debug!("the rain on the glass stops");
                self.redraw = true;
            }
            return false;
        };
        if self.fx.raindrops.is_none() {
            self.fx.rains += 1;
            tracing::debug!(?plays, "it rains on the glass");
            self.fx.raindrops = Some(Rain::new(now, self.fx.rains));
            self.redraw = true;
        }
        let mut screens = Vec::new();
        let mut on = Vec::new();
        for output in self.space.outputs() {
            let Some(area) = self.space.output_geometry(output) else {
                continue;
            };
            let name = output.name();
            if self.output_powered(output) && !self.wall_hidden(output) {
                on.push(name.clone());
            }
            screens.push((name, (area.size.w as f32, area.size.h as f32)));
        }
        let Some(rain) = self.fx.raindrops.as_mut() else {
            return false;
        };
        let mut made = false;
        for (name, size) in &screens {
            made |= rain.pane(name, *size);
        }
        let names: Vec<&str> = screens.iter().map(|(n, _)| n.as_str()).collect();
        rain.retain_panes(&names);
        let moving = plays == super::Plays::Moving && !on.is_empty();
        if moving {
            rain.advance(now, &on);
        }
        if made {
            self.redraw = true;
        }
        moving
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn big_drops_grow_slide_and_swallow_the_small_ones() {
        let mut rng = Rng::new(7);
        let mut pane = Pane::new((1200.0, 720.0), &mut rng);
        let (stills, rolling) = pane.counts();
        // The board's, warmed up: about its thousand still drops, a few
        // dozen big ones.
        assert!((700..=1100).contains(&stills), "{stills}");
        assert!((5..=60).contains(&rolling), "{rolling}");
        // A big drop at its sliding size, above a still one in its way.
        pane.rolling.clear();
        pane.rolling.push(Rolling {
            x: 600.0,
            y: 100.0,
            r: 14.0,
            vy: 0.0,
            wob: 0.0,
            grow: 1.0,
            shift: 0.0,
        });
        pane.put(Still {
            x: 600.0,
            y: 160.0,
            r: 5.0,
        });
        let mut grew = false;
        for _ in 0..60 {
            pane.step(STEP, &mut rng);
            grew |= pane.rolling.iter().any(|d| d.r > 14.2);
        }
        assert!(grew, "it swallowed the drop in its way");
        let y = pane.rolling.iter().map(|d| d.y).fold(0.0, f32::max);
        assert!(y > 150.0, "it slid down: {y}");
        // Its trail is wiped: no still drop right above it.
        for d in &pane.rolling {
            let above =
                pane.cells.iter().flatten().any(|s| {
                    s.y < d.y - d.r && s.y > d.y - d.r * 3.0 && (s.x - d.x).abs() < d.r * 0.5
                });
            assert!(!above, "a trail above {d:?}");
        }
    }

    #[test]
    fn a_frame_says_what_changed_and_nothing_twice() {
        let mut rng = Rng::new(3);
        let mut pane = Pane::new((400.0, 300.0), &mut rng);
        let blur = Id::new();
        let first = pane.frame(&blur);
        assert!(first.fresh_map);
        assert!(first.since.is_none(), "all of it at first");
        let again = pane.frame(&blur);
        assert_eq!(again.commit, first.commit);
        assert!(!again.fresh_map);
        pane.step(STEP, &mut rng);
        let next = pane.frame(&blur);
        assert_ne!(next.commit, first.commit);
        let (prev, rects) = next.since.expect("what changed");
        assert_eq!(prev, first.commit);
        assert!(!rects.is_empty() && rects.len() <= MOST_RECTS);
        // A new wallpaper: all of it again.
        let other = pane.frame(&Id::new());
        assert!(other.since.is_none());
        assert_ne!(other.fog, next.fog);
    }
}
