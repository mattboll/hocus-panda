//! Drawing on the screen (docs/design/ink.md, issue #564): `Super`+drag
//! draws a stroke, `Super`+`Shift`+drag an arrow, and `Super`+`Alt` held
//! shows the laser, a red dot with a trail. All of it fades by itself, and
//! is an element of the compositor's scene, so that shares, recordings and
//! screenshots of the screen show it as the screen does.
//!
//! The strokes are drawn on the CPU, into tiles of 120 logical pixels
//! (a whole number of pixels at any scale the fractional-scale protocol
//! gives) made as the ink reaches them. A new segment repaints only the
//! pixels its glow reaches, each from the segments near it: a pixel is a
//! function of its distance to the ink, so strokes crossing read as one.
//! The drawing fades as one, by the tiles' opacity. The laser's dots are
//! two small images drawn once per scale and placed, scaled and faded
//! each frame. Tiles and dots are drawn by one element per screen, whose
//! damage is what changed ([`Scene`]).

use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    time::Duration,
};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            ImportMem, Renderer, Texture,
            element::{
                Element, Id, Kind, RenderElement,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
            },
            utils::{CommitCounter, DamageBag, DamageSet, DamageSnapshot, OpaqueRegions},
        },
    },
    output::Output,
    utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Size, Transform},
};

use crate::{
    State,
    motion::Bezier,
    render::{OutputElement, OutputElements},
};

/// The strokes stay this long after the last release, then fade.
pub const HOLD: Duration = Duration::from_secs(3);
pub const FADE: Duration = Duration::from_millis(600);
/// The laser's trail fades in this time.
pub const TRAIL: Duration = Duration::from_millis(450);
/// A stroke stops growing past this many points, until its release.
pub const MAX_POINTS: usize = 4096;
/// A stroke takes a point of the pointer's this far from its last, in
/// logical pixels, as the mockup does.
const STEP: f64 = 2.0;
/// The laser's trail takes a point this far from its last.
const TRAIL_STEP: f64 = 2.0;
/// The trail's dots at most: at a mouse's 1000 Hz, 450 ms would be 450.
const TRAIL_MOST: usize = 96;

/// The strokes' and arrows' look, in logical pixels: `#F2545B`, 4 px wide,
/// a 5 px glow `rgba(242,84,91,0.55)` and a 1 px dark shadow 1 px down
/// (the mockup's `drop-shadow(0 0 5px …) drop-shadow(0 1px 1px
/// rgba(0,0,0,0.6))`; a CSS blur's radius is twice its deviation).
const RED: [f64; 3] = [242.0, 84.0, 91.0];
const WIDTH: f64 = 4.0;
const GLOW: f64 = 0.55;
const GLOW_SIGMA: f64 = 2.5;
const SHADOW: f64 = 0.6;
const SHADOW_DOWN: f64 = 1.0;
const SHADOW_SIGMA: f64 = 0.5;
/// The arrow's head: two strokes of 22 px back from the tip, at about
/// 31° (a slope of 0.6) each side; 40 % of a shorter arrow's length.
const HEAD: f64 = 22.0;
const HEAD_SHARE: f64 = 0.4;
const HEAD_SLOPE: f64 = 0.6;

/// The laser: a 12 px dot `rgba(255,90,96,1)` with a 14 px glow
/// `rgba(255,70,80,0.9)`; its trail, dots of 10 px down to 3 px, at 75 %
/// down to nothing, with an 8 px glow.
const LASER: [f64; 3] = [255.0, 90.0, 96.0];
const LASER_GLOW: [f64; 3] = [255.0, 70.0, 80.0];
const LASER_GLOW_ALPHA: f64 = 0.9;
const DOT: f64 = 12.0;
const DOT_SIGMA: f64 = 7.0;
const TRAIL_FROM: f64 = 10.0;
const TRAIL_TO: f64 = 3.0;
const TRAIL_SIGMA: f64 = 4.0;
const TRAIL_ALPHA: f64 = 0.75;

/// A tile's side, in logical pixels: 120 × any multiple of 1/120 is a
/// whole number of pixels.
const TILE: i32 = 120;

/// What a press draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `Super`+drag: a line along the pointer's points.
    Stroke,
    /// `Super`+`Shift`+drag: straight from the press to the pointer, its
    /// head there.
    Arrow,
}

/// One stroke or arrow, on the screen where it started; its points are
/// that screen's (logical, from its top left corner).
#[derive(Debug)]
struct Mark {
    id: u64,
    output: String,
    shape: Shape,
    points: Vec<(f64, f64)>,
}

/// Where the drawing is in its life.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    /// Nothing shows.
    Empty,
    /// A press under way: everything shows fully. `mark` is the one it
    /// draws; `None` once `Esc` took it.
    Drawing { mark: Option<u64> },
    /// Released at `since`: it shows for `HOLD`, then fades.
    Resting { since: Duration },
}

/// How the drawing shows at a moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Showing {
    /// Nothing to show.
    Nothing,
    /// Fully, until that moment (or the release, while drawing: `None`).
    Full(Option<Duration>),
    /// Fading: this opaque.
    Fading(f32),
    /// Its time is over: it goes.
    Over,
}

/// The laser's keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Keys {
    Off,
    /// `Super`+`Alt` held.
    On,
    /// Held, but another key was pressed with them: a shortcut, not the
    /// laser, until they are let go.
    Spent,
}

/// One of the trail's dots.
#[derive(Debug)]
struct Dot {
    at: Point<f64, Logical>,
    since: Duration,
    id: u64,
}

#[derive(Debug)]
pub struct Laser {
    keys: Keys,
    trail: VecDeque<Dot>,
    next: u64,
}

impl Default for Laser {
    fn default() -> Self {
        Self {
            keys: Keys::Off,
            trail: VecDeque::new(),
            next: 0,
        }
    }
}

impl Laser {
    fn on(&self) -> bool {
        self.keys == Keys::On
    }
}

/// The strokes, the arrows and the laser.
#[derive(Debug)]
pub struct Ink {
    marks: Vec<Mark>,
    next: u64,
    phase: Phase,
    laser: Laser,
    /// What each screen's strokes are drawn into, by output name.
    canvases: RefCell<HashMap<String, Canvas>>,
    /// What each screen's ink element showed last, by output name.
    scenes: RefCell<HashMap<String, Scene>>,
    /// The laser's dot and the trail's, by scale (in thousandths).
    sprites: RefCell<HashMap<u32, Sprites>>,
}

impl Default for Ink {
    fn default() -> Self {
        Self {
            marks: Vec::new(),
            next: 0,
            phase: Phase::Empty,
            laser: Laser::default(),
            canvases: RefCell::default(),
            scenes: RefCell::default(),
            sprites: RefCell::default(),
        }
    }
}

impl Ink {
    /// A press of `shape` at `at` on screen `output` (its own logical
    /// pixels): a new mark, and everything back to full opacity.
    pub fn begin(&mut self, output: String, at: (f64, f64), shape: Shape) {
        let id = self.next;
        self.next += 1;
        self.marks.push(Mark {
            id,
            output,
            shape,
            points: vec![at],
        });
        self.phase = Phase::Drawing { mark: Some(id) };
    }

    /// The screen the mark under way is on.
    pub fn drawing_on(&self) -> Option<&str> {
        let Phase::Drawing { mark: Some(id) } = self.phase else {
            return None;
        };
        self.marks
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.output.as_str())
    }

    /// Whether a press is under way.
    pub fn drawing(&self) -> bool {
        matches!(self.phase, Phase::Drawing { .. })
    }

    /// The pointer at `at` (the mark's screen's logical pixels): whether
    /// the mark under way changed.
    pub fn extend(&mut self, at: (f64, f64)) -> bool {
        let Phase::Drawing { mark: Some(id) } = self.phase else {
            return false;
        };
        let Some(mark) = self.marks.iter_mut().find(|m| m.id == id) else {
            return false;
        };
        let last = mark.points.last().copied().unwrap_or(at);
        match mark.shape {
            Shape::Stroke => {
                if mark.points.len() >= MAX_POINTS || distance(last, at) <= STEP {
                    return false;
                }
                mark.points.push(at);
            }
            Shape::Arrow => {
                if mark.points.len() > 1 && last == at {
                    return false;
                }
                mark.points.truncate(1);
                if mark.points[0] != at {
                    mark.points.push(at);
                }
            }
        }
        true
    }

    /// The press released at `now`: what it drew stays, a press without
    /// motion draws nothing, and the hold starts again.
    pub fn end(&mut self, now: Duration) {
        if !self.drawing() {
            return;
        }
        self.marks.retain(|m| m.points.len() > 1);
        self.phase = if self.marks.is_empty() {
            Phase::Empty
        } else {
            Phase::Resting { since: now }
        };
    }

    /// Everything goes at once (`Esc`, a time over); a press under way
    /// draws nothing more until its release.
    pub fn clear(&mut self) {
        self.marks.clear();
        self.canvases.borrow_mut().clear();
        self.phase = match self.phase {
            Phase::Drawing { .. } => Phase::Drawing { mark: None },
            _ => Phase::Empty,
        };
    }

    /// Whether strokes show (`Esc` clears them).
    pub fn shows(&self) -> bool {
        !self.marks.is_empty()
    }

    /// Screen `output` went: its strokes with it.
    pub fn output_gone(&mut self, output: &str) {
        self.marks.retain(|m| m.output != output);
        self.canvases.borrow_mut().remove(output);
        self.scenes.borrow_mut().remove(output);
        if self.marks.is_empty() && !self.drawing() {
            self.phase = Phase::Empty;
        }
    }

    /// How the drawing shows at `now`.
    pub fn showing(&self, now: Duration, reduced: bool) -> Showing {
        if self.marks.is_empty() {
            return Showing::Nothing;
        }
        match self.phase {
            Phase::Empty => Showing::Nothing,
            Phase::Drawing { .. } => Showing::Full(None),
            Phase::Resting { since } => {
                let held = since + HOLD;
                if now < held {
                    return Showing::Full(Some(held));
                }
                if reduced {
                    return Showing::Over;
                }
                let t = (now - held).as_secs_f64() / FADE.as_secs_f64();
                if t >= 1.0 {
                    Showing::Over
                } else {
                    Showing::Fading((1.0 - Bezier::EASE_IN.at(t)) as f32)
                }
            }
        }
    }

    /// The modifiers held now: the laser shows while `Super`+`Alt` are,
    /// once no other key was pressed with them. Whether that changed.
    pub fn laser_keys(&mut self, held: bool) -> bool {
        let before = self.laser_shown();
        self.laser.keys = match (self.laser.keys, held) {
            (_, false) => Keys::Off,
            (Keys::Off, true) => Keys::On,
            (keys, true) => keys,
        };
        if !self.laser.on() {
            self.laser.trail.clear();
        }
        before != self.laser_shown()
    }

    /// A key other than a modifier was pressed: with `Super`+`Alt` held,
    /// it is a shortcut's, and the laser goes until they are let go.
    /// Whether it showed.
    pub fn other_key(&mut self) -> bool {
        if self.laser.keys != Keys::On {
            return false;
        }
        let shown = self.laser_shown();
        self.laser.keys = Keys::Spent;
        self.laser.trail.clear();
        shown
    }

    /// Whether the laser shows: its keys held, no press drawing.
    pub fn laser_shown(&self) -> bool {
        self.laser.on() && !self.drawing()
    }

    /// The pointer moved to `at` (global) at `now`: the trail follows.
    /// Whether the laser shows.
    pub fn laser_moved(&mut self, at: Point<f64, Logical>, now: Duration) -> bool {
        if !self.laser_shown() {
            return false;
        }
        let far = self
            .laser
            .trail
            .back()
            .is_none_or(|d| distance((d.at.x, d.at.y), (at.x, at.y)) >= TRAIL_STEP);
        if far {
            let id = self.laser.next;
            self.laser.next += 1;
            self.laser.trail.push_back(Dot { at, since: now, id });
            while self.laser.trail.len() > TRAIL_MOST {
                self.laser.trail.pop_front();
            }
        }
        true
    }

    /// The trail as of `now`: its dots gone are dropped. Whether some
    /// still fade.
    pub fn laser_tick(&mut self, now: Duration) -> bool {
        while self
            .laser
            .trail
            .front()
            .is_some_and(|d| now.saturating_sub(d.since) >= TRAIL)
        {
            self.laser.trail.pop_front();
        }
        self.laser_shown() && !self.laser.trail.is_empty()
    }
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// A segment of ink, in a screen's pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Seg {
    a: (f64, f64),
    b: (f64, f64),
}

impl Seg {
    fn bounds(self, by: f64) -> (f64, f64, f64, f64) {
        (
            self.a.0.min(self.b.0) - by,
            self.a.1.min(self.b.1) - by,
            self.a.0.max(self.b.0) + by,
            self.a.1.max(self.b.1) + by,
        )
    }

    /// The squared distance from `p` to the segment.
    fn distance2(self, p: (f64, f64)) -> f64 {
        let (dx, dy) = (self.b.0 - self.a.0, self.b.1 - self.a.1);
        let length2 = dx * dx + dy * dy;
        let t = if length2 > 0.0 {
            (((p.0 - self.a.0) * dx + (p.1 - self.a.1) * dy) / length2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (x, y) = (self.a.0 + t * dx - p.0, self.a.1 + t * dy - p.1);
        x * x + y * y
    }

    /// The part of the segment inside the box (x0, y0, x1, y1), if any
    /// (Liang and Barsky's clipping).
    fn clipped(self, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> Option<Self> {
        let (dx, dy) = (self.b.0 - self.a.0, self.b.1 - self.a.1);
        let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
        for (p, q) in [
            (-dx, self.a.0 - x0),
            (dx, x1 - self.a.0),
            (-dy, self.a.1 - y0),
            (dy, y1 - self.a.1),
        ] {
            if p == 0.0 {
                if q < 0.0 {
                    return None;
                }
            } else {
                let r = q / p;
                if p < 0.0 {
                    t0 = t0.max(r);
                } else {
                    t1 = t1.min(r);
                }
                if t0 > t1 {
                    return None;
                }
            }
        }
        Some(Self {
            a: (self.a.0 + t0 * dx, self.a.1 + t0 * dy),
            b: (self.a.0 + t1 * dx, self.a.1 + t1 * dy),
        })
    }
}

/// An arrow's three segments, from `from` to `to` (logical): the shaft
/// and the head's two sides, as the mockup draws them.
pub fn arrow(from: (f64, f64), to: (f64, f64)) -> [((f64, f64), (f64, f64)); 3] {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = dx.hypot(dy).max(1.0);
    let (ux, uy) = (dx / length, dy / length);
    let h = HEAD.min(length * HEAD_SHARE);
    let left = (
        to.0 - ux * h - uy * h * HEAD_SLOPE,
        to.1 - uy * h + ux * h * HEAD_SLOPE,
    );
    let right = (
        to.0 - ux * h + uy * h * HEAD_SLOPE,
        to.1 - uy * h - ux * h * HEAD_SLOPE,
    );
    [(from, to), (left, to), (to, right)]
}

impl Mark {
    /// Its segments at `scale` (a screen's pixels), from point `from` on.
    fn segments(&self, scale: f64, from: usize) -> Vec<Seg> {
        let px = |p: (f64, f64)| (p.0 * scale, p.1 * scale);
        match self.shape {
            Shape::Stroke => self.points[from.saturating_sub(1)..]
                .windows(2)
                .map(|w| Seg {
                    a: px(w[0]),
                    b: px(w[1]),
                })
                .collect(),
            Shape::Arrow => match self.points.as_slice() {
                [a, .., b] => arrow(*a, *b)
                    .into_iter()
                    .map(|(a, b)| Seg { a: px(a), b: px(b) })
                    .collect(),
                _ => Vec::new(),
            },
        }
    }
}

/// What a canvas has drawn of a mark.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Drawn {
    /// A stroke's first points.
    Points(usize),
    /// An arrow, from and to.
    Arrow((f64, f64), (f64, f64)),
}

/// One tile of a screen's strokes.
#[derive(Debug)]
struct Tile {
    buffer: MemoryRenderBuffer,
    /// Never the same twice: a tile made again where another was (after
    /// `Esc`, the fade, a new scale) is another piece of the scene, whose
    /// whole place is damaged, not the same one kept as it was.
    serial: u64,
    /// Its pixels with ink: none, and it goes.
    inked: usize,
}

/// A screen's strokes, drawn at its scale.
#[derive(Debug)]
struct Canvas {
    scale: f64,
    /// A tile's side, in pixels.
    side: i32,
    tiles: HashMap<(i32, i32), Tile>,
    drawn: HashMap<u64, Drawn>,
}

impl Canvas {
    fn new(scale: f64) -> Self {
        Self {
            scale,
            side: (f64::from(TILE) * scale).round().max(1.0) as i32,
            tiles: HashMap::new(),
            drawn: HashMap::new(),
        }
    }

    /// The marks of its screen drawn as they are now, in a screen `size`
    /// pixels large: the areas painted again (its pixels).
    fn catch_up(&mut self, marks: &[&Mark], size: (i32, i32)) -> Vec<Rectangle<i32, Physical>> {
        let scale = self.scale;
        let mut dirty: Vec<Seg> = Vec::new();
        for mark in marks {
            let now = match mark.shape {
                Shape::Stroke => Drawn::Points(mark.points.len()),
                Shape::Arrow => match mark.points.as_slice() {
                    [a, .., b] => Drawn::Arrow(*a, *b),
                    _ => Drawn::Points(mark.points.len()),
                },
            };
            let before = self.drawn.insert(mark.id, now);
            if before == Some(now) {
                continue;
            }
            match (before, now) {
                (Some(Drawn::Points(n)), Drawn::Points(_)) => {
                    dirty.extend(mark.segments(scale, n));
                }
                (Some(Drawn::Arrow(a, b)), _) => {
                    let old = arrow(a, b).map(|(a, b)| Seg {
                        a: (a.0 * scale, a.1 * scale),
                        b: (b.0 * scale, b.1 * scale),
                    });
                    dirty.extend(old);
                    dirty.extend(mark.segments(scale, 0));
                }
                _ => dirty.extend(mark.segments(scale, 0)),
            }
        }
        let mut painted = Vec::new();
        if dirty.is_empty() {
            return painted;
        }
        let pen = Pen::new(scale);
        let side = self.side;
        // The part of each tile the dirty segments' ink reaches.
        let mut areas: HashMap<(i32, i32), (i32, i32, i32, i32)> = HashMap::new();
        let (w, h) = (f64::from(size.0), f64::from(size.1));
        for seg in &dirty {
            let (x0, y0, x1, y1) = seg.bounds(pen.reach);
            let (x0, y0, x1, y1) = (x0.max(0.0), y0.max(0.0), x1.min(w), y1.min(h));
            if x0 >= x1 || y0 >= y1 {
                continue;
            }
            let tiles_x = (x0 as i32 / side)..=((x1 as i32 - 1).max(0) / side);
            for tx in tiles_x {
                for ty in (y0 as i32 / side)..=((y1 as i32 - 1).max(0) / side) {
                    let (left, top) = (f64::from(tx * side), f64::from(ty * side));
                    let room = (
                        left - pen.reach,
                        top - pen.reach,
                        left + f64::from(side) + pen.reach,
                        top + f64::from(side) + pen.reach,
                    );
                    let Some(part) = seg.clipped(room) else {
                        continue;
                    };
                    let (a0, b0, a1, b1) = part.bounds(pen.reach);
                    let local = (
                        ((a0 - left).floor() as i32).clamp(0, side),
                        ((b0 - top).floor() as i32).clamp(0, side),
                        ((a1 - left).ceil() as i32).clamp(0, side),
                        ((b1 - top).ceil() as i32).clamp(0, side),
                    );
                    if local.0 >= local.2 || local.1 >= local.3 {
                        continue;
                    }
                    areas
                        .entry((tx, ty))
                        .and_modify(|r| {
                            *r = (
                                r.0.min(local.0),
                                r.1.min(local.1),
                                r.2.max(local.2),
                                r.3.max(local.3),
                            );
                        })
                        .or_insert(local);
                }
            }
        }
        if areas.is_empty() {
            return painted;
        }
        let all: Vec<Seg> = marks.iter().flat_map(|m| m.segments(scale, 0)).collect();
        for ((tx, ty), area) in areas {
            let origin = (tx * side, ty * side);
            let near_area = (
                f64::from(origin.0 + area.0) - pen.reach,
                f64::from(origin.1 + area.1) - pen.reach,
                f64::from(origin.0 + area.2) + pen.reach,
                f64::from(origin.1 + area.3) + pen.reach,
            );
            let near: Vec<Seg> = all
                .iter()
                .copied()
                .filter(|s| {
                    let (x0, y0, x1, y1) = s.bounds(0.0);
                    x1 >= near_area.0
                        && x0 <= near_area.2
                        && y1 >= near_area.1 - pen.down
                        && y0 <= near_area.3
                })
                .collect();
            let tile = self.tiles.entry((tx, ty)).or_insert_with(|| Tile {
                buffer: MemoryRenderBuffer::new(
                    Fourcc::Argb8888,
                    (side, side),
                    1,
                    Transform::Normal,
                    None,
                ),
                serial: NEXT_TILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                inked: 0,
            });
            paint(tile, side, origin, area, &near, &pen);
            if tile.inked == 0 {
                self.tiles.remove(&(tx, ty));
            }
            painted.push(Rectangle::new(
                (origin.0 + area.0, origin.1 + area.1).into(),
                (area.2 - area.0, area.3 - area.1).into(),
            ));
        }
        painted
    }
}

/// The stroke's look at a scale, in pixels.
#[derive(Debug, Clone, Copy)]
struct Pen {
    radius: f64,
    glow: f64,
    shadow: f64,
    down: f64,
    /// Past this distance from the ink (and from it 1 px up), nothing
    /// shows.
    reach: f64,
}

impl Pen {
    fn new(scale: f64) -> Self {
        let radius = WIDTH / 2.0 * scale;
        let glow = GLOW_SIGMA * scale;
        let shadow = SHADOW_SIGMA * scale;
        let down = SHADOW_DOWN * scale;
        Self {
            radius,
            glow,
            shadow,
            down,
            reach: radius + 3.5 * glow + down,
        }
    }

    /// A pixel at `d` from the ink and `up` from the ink 1 px over it, as
    /// premultiplied B, G, R, A.
    fn pixel(&self, d: f64, up: f64) -> [u8; 4] {
        let lit = self.lit(d, (self.radius + 0.5 - d).clamp(0.0, 1.0));
        let under = SHADOW * self.lit(up, band(self.radius, up, self.shadow));
        let alpha = lit + under * (1.0 - lit);
        let byte = |v: f64| (v * 255.0).round().clamp(0.0, 255.0) as u8;
        [
            byte(lit * RED[2] / 255.0),
            byte(lit * RED[1] / 255.0),
            byte(lit * RED[0] / 255.0),
            byte(alpha),
        ]
    }

    /// The stroke over its glow, at `d` from the ink: how opaque, given
    /// the stroke's own `core` there.
    fn lit(&self, d: f64, core: f64) -> f64 {
        let glow = GLOW * band(self.radius, d, self.glow);
        core + glow * (1.0 - core)
    }
}

/// A band `radius` either side of a line, blurred by `sigma`, at `d` from
/// the line.
fn band(radius: f64, d: f64, sigma: f64) -> f64 {
    phi((radius - d) / sigma) - phi((-radius - d) / sigma)
}

/// The normal distribution's cumulative function.
fn phi(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / std::f64::consts::SQRT_2))
}

/// Abramowitz and Stegun's 7.1.26, within 1.5e-7.
fn erf(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    sign * (1.0 - poly * (-x * x).exp())
}

/// Paints `area` (x0, y0, x1, y1, the tile's pixels) of the tile at
/// `origin` (the screen's pixels) from the segments `near` it.
fn paint(
    tile: &mut Tile,
    side: i32,
    origin: (i32, i32),
    area: (i32, i32, i32, i32),
    near: &[Seg],
    pen: &Pen,
) {
    let Tile { buffer, inked, .. } = tile;
    let reach2 = pen.reach * pen.reach;
    let (x0, y0, x1, y1) = area;
    let drawn = buffer.render().draw(|pixels| {
        for y in y0..y1 {
            let py = f64::from(origin.1 + y) + 0.5;
            // The segments this row's ink can come from.
            let row: Vec<Seg> = near
                .iter()
                .copied()
                .filter(|s| {
                    let (_, top, _, bottom) = s.bounds(pen.reach);
                    py >= top && py <= bottom + pen.down
                })
                .collect();
            for x in x0..x1 {
                let p = (f64::from(origin.0 + x) + 0.5, py);
                let lifted = (p.0, p.1 - pen.down);
                let (mut d2, mut up2) = (f64::INFINITY, f64::INFINITY);
                for s in &row {
                    d2 = d2.min(s.distance2(p));
                    up2 = up2.min(s.distance2(lifted));
                }
                let value = if d2 >= reach2 && up2 >= reach2 {
                    [0; 4]
                } else {
                    pen.pixel(d2.sqrt(), up2.sqrt())
                };
                let at = ((y * side + x) * 4) as usize;
                let Some(old) = pixels.get_mut(at..at + 4) else {
                    continue;
                };
                match (old[3] != 0, value[3] != 0) {
                    (false, true) => *inked += 1,
                    (true, false) => *inked = inked.saturating_sub(1),
                    _ => {}
                }
                old.copy_from_slice(&value);
            }
        }
        Ok::<_, std::convert::Infallible>(vec![Rectangle::<i32, Buffer>::new(
            (x0, y0).into(),
            (x1 - x0, y1 - y0).into(),
        )])
    });
    if let Err(never) = drawn {
        match never {}
    }
}

/// The laser's dot and its trail's, at a scale.
#[derive(Debug)]
struct Sprites {
    dot: (MemoryRenderBuffer, i32),
    trail: (MemoryRenderBuffer, i32),
}

/// A dot `size` logical pixels wide in a `box_size` image, its glow
/// blurred by `sigma`: premultiplied ARGB, `box_size × scale` pixels.
fn sprite(size: f64, sigma: f64, box_size: f64, scale: f64) -> (MemoryRenderBuffer, i32) {
    let px = (box_size * scale).round().max(1.0) as i32;
    let centre = f64::from(px) / 2.0;
    let radius = size / 2.0 * scale;
    let sigma = sigma * scale;
    let mut pixels = vec![0u8; (px * px * 4) as usize];
    for y in 0..px {
        for x in 0..px {
            let d = (f64::from(x) + 0.5 - centre).hypot(f64::from(y) + 0.5 - centre);
            let core = (radius + 0.5 - d).clamp(0.0, 1.0);
            let glow = LASER_GLOW_ALPHA * band(radius, d, sigma) * (1.0 - core);
            let alpha = core + glow;
            let channel = |c: usize| {
                ((core * LASER[c] + glow * LASER_GLOW[c]).round()).clamp(0.0, 255.0) as u8
            };
            let at = ((y * px + x) * 4) as usize;
            pixels[at..at + 4].copy_from_slice(&[
                channel(2),
                channel(1),
                channel(0),
                (alpha * 255.0).round().clamp(0.0, 255.0) as u8,
            ]);
        }
    }
    let buffer = MemoryRenderBuffer::from_slice(
        &pixels,
        Fourcc::Argb8888,
        (px, px),
        1,
        Transform::Normal,
        None,
    );
    (buffer, px)
}

/// A sprite's box: the dot and three deviations of glow each side.
fn sprite_box(size: f64, sigma: f64) -> f64 {
    (size / 2.0 + 3.0 * sigma).ceil() * 2.0
}

impl Sprites {
    fn new(scale: f64) -> Self {
        Self {
            dot: sprite(DOT, DOT_SIGMA, sprite_box(DOT, DOT_SIGMA), scale),
            trail: sprite(
                TRAIL_FROM,
                TRAIL_SIGMA,
                sprite_box(TRAIL_FROM, TRAIL_SIGMA),
                scale,
            ),
        }
    }
}

/// A trail's dot of age `t` (0 to 1): its size and how opaque.
pub fn trail_dot(t: f64) -> (f64, f64) {
    let t = t.clamp(0.0, 1.0);
    (
        (TRAIL_FROM * (1.0 - t)).max(TRAIL_TO),
        TRAIL_ALPHA * (1.0 - t),
    )
}

impl State {
    /// What a press of the left button with `modifiers` held draws:
    /// `Super`, and `Shift` for an arrow; nested, `Alt+Super`, where the
    /// host takes `Super` (docs/design/windows.md §13). With `Alt` (not
    /// nested) it is the laser's, and the click goes to the window.
    pub fn ink_shape(&self, modifiers: &smithay::input::keyboard::ModifiersState) -> Option<Shape> {
        let with_alt = self.nested;
        (self.ink_on() && modifiers.logo && !modifiers.ctrl && modifiers.alt == with_alt).then_some(
            if modifiers.shift {
                Shape::Arrow
            } else {
                Shape::Stroke
            },
        )
    }

    /// Whether the ink starts: `[ink] enabled`, and `Super`+drag not
    /// taken to move windows (`[windows] move-modifier = "super"`, 6.14).
    pub fn ink_on(&self) -> bool {
        self.config.ink.enabled
            && self.config.windows.move_modifier != crate::config::DragKey::Super
    }

    /// The ink was turned off (6.14): what it shows goes, the laser with
    /// it.
    pub(crate) fn ink_turned_off(&mut self) {
        self.ink.laser_keys(false);
        self.ink_clear();
    }

    /// Starts drawing `shape` from the pointer, with the pointer grabbed
    /// until `button`'s release: no client sees it meanwhile.
    pub fn start_ink(&mut self, shape: Shape, button: u32, serial: smithay::utils::Serial) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let at = pointer.current_location();
        let Some((output, origin)) = self.space.output_under(at).next().and_then(|o| {
            let origin = self.space.output_geometry(o)?.loc.to_f64();
            Some((o.name(), origin))
        }) else {
            return;
        };
        self.ink
            .begin(output, (at.x - origin.x, at.y - origin.y), shape);
        let grab = crate::grabs::InkGrab::new(smithay::input::pointer::GrabStartData {
            focus: None,
            button,
            location: at,
        });
        self.frame_cursor = Some(smithay::input::pointer::CursorIcon::Crosshair);
        pointer.set_grab(self, grab, serial, smithay::input::pointer::Focus::Clear);
        self.redraw = true;
    }

    /// The pointer at `at` (global) while drawing.
    pub(crate) fn ink_to(&mut self, at: Point<f64, Logical>) {
        let Some(origin) = self
            .ink
            .drawing_on()
            .and_then(|name| self.space.outputs().find(|o| o.name() == name))
            .and_then(|o| self.space.output_geometry(o))
            .map(|g| g.loc.to_f64())
        else {
            return;
        };
        if self.ink.extend((at.x - origin.x, at.y - origin.y)) {
            self.redraw = true;
        }
    }

    /// The press drawing was released.
    pub(crate) fn ink_end(&mut self) {
        let now = self.now();
        self.ink.end(now);
        self.frame_cursor = None;
        self.redraw = true;
    }

    /// `Esc` while strokes show: they all go.
    pub(crate) fn ink_clear(&mut self) {
        self.ink.clear();
        self.redraw = true;
    }

    /// The laser's keys after a key's press or release.
    pub(crate) fn ink_modifiers(&mut self, modifiers: &smithay::input::keyboard::ModifiersState) {
        let held = self.ink_on() && modifiers.logo && modifiers.alt && !modifiers.ctrl;
        // The dot shows at the pointer; its trail starts as it moves.
        if self.ink.laser_keys(held) {
            self.redraw = true;
        }
    }

    /// The pointer moved to `at`: the laser's trail follows.
    pub(crate) fn ink_pointer(&mut self, at: Point<f64, Logical>) {
        let now = self.now();
        if self.ink.laser_moved(at, now) {
            self.redraw = true;
        }
    }

    /// The drawing's and the laser's motions as of `now`: whether they
    /// move; when the hold ends, `wake` is told.
    pub fn tick_ink(&mut self, now: Duration, wake: &mut impl FnMut(Duration)) -> bool {
        let mut moving = false;
        match self.ink.showing(now, self.reduced_motion()) {
            Showing::Full(Some(until)) => wake(until),
            Showing::Fading(_) => moving = true,
            // Not drawn as of the last frame: nothing to draw again.
            Showing::Over => self.ink.clear(),
            Showing::Full(None) | Showing::Nothing => {}
        }
        moving |= self.ink.laser_tick(now);
        moving
    }

    /// The ink on `output` at `now`: one element, its laser's dot, its
    /// trail and its strokes drawn in it, whose damage is what changed.
    /// None when nothing shows there.
    pub(crate) fn ink_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        now: Duration,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer
            + smithay::backend::renderer::ImportAll
            + ImportMem
            + crate::wobble::Bend
            + crate::going::Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some(area) = self.space.output_geometry(output) else {
            return;
        };
        let scale = output.current_scale().fractional_scale();
        let name = output.name();
        let size = Size::<i32, Physical>::from((
            (f64::from(area.size.w) * scale).ceil() as i32,
            (f64::from(area.size.h) * scale).ceil() as i32,
        ));
        let mut pieces: Vec<(Piece, MemoryRenderBufferRenderElement<R>)> = Vec::new();
        if self.ink.laser_shown() {
            self.laser_pieces(renderer, area.to_f64(), scale, now, &mut pieces);
        }
        let alpha = match self.ink.showing(now, self.reduced_motion()) {
            Showing::Full(_) => Some(1.0),
            Showing::Fading(alpha) => Some(alpha),
            Showing::Nothing | Showing::Over => None,
        };
        let marks: Vec<&Mark> = self.ink.marks.iter().filter(|m| m.output == name).collect();
        let mut painted = Vec::new();
        if let Some(alpha) = alpha.filter(|_| !marks.is_empty()) {
            let mut canvases = self.ink.canvases.borrow_mut();
            let canvas = canvases
                .entry(name.clone())
                .or_insert_with(|| Canvas::new(scale));
            if canvas.scale != scale {
                *canvas = Canvas::new(scale);
            }
            painted = canvas.catch_up(&marks, (size.w, size.h));
            let side = canvas.side;
            let src =
                Rectangle::<f64, Logical>::from_size((f64::from(side), f64::from(side)).into());
            for (&(tx, ty), tile) in &canvas.tiles {
                let at = (f64::from(tx * side), f64::from(ty * side));
                match MemoryRenderBufferRenderElement::from_buffer(
                    renderer,
                    at,
                    &tile.buffer,
                    Some(alpha),
                    Some(src),
                    Some((TILE, TILE).into()),
                    Kind::Unspecified,
                ) {
                    Ok(element) => pieces.push((Piece::Tile(tile.serial), element)),
                    Err(err) => tracing::debug!(?err, "a tile of ink could not be drawn"),
                }
            }
        }
        let mut scenes = self.ink.scenes.borrow_mut();
        if pieces.is_empty() && scenes.get(&name).is_none_or(|s| s.shown.is_empty()) {
            // Nothing shows, nor did.
            return;
        }
        let scene = scenes.entry(name).or_insert_with(Scene::new);
        let sc = Scale::from(scale);
        let shown: HashMap<Piece, (Rectangle<i32, Physical>, f32)> = pieces
            .iter()
            .map(|(piece, e)| (*piece, (e.geometry(sc), e.alpha())))
            .collect();
        // What changed: what was painted again, and every piece that came,
        // went, moved or faded, where it was and where it is.
        let mut damage = painted;
        for (piece, now) in &shown {
            match scene.shown.get(piece) {
                Some(before) if before == now => {}
                Some((before, _)) => damage.extend([*before, now.0]),
                None => damage.push(now.0),
            }
        }
        for (piece, (before, _)) in &scene.shown {
            if !shown.contains_key(piece) {
                damage.push(*before);
            }
        }
        if !damage.is_empty() {
            scene.bag.add(damage);
        }
        scene.shown = shown;
        if pieces.is_empty() {
            return;
        }
        out.push(OutputElements::Ink(InkElement {
            id: scene.id.clone(),
            damage: scene.bag.snapshot(),
            size,
            scale: sc,
            pieces: pieces.into_iter().map(|(_, e)| e).collect(),
        }));
    }

    /// The laser's dot at the pointer and its trail, in `area` (global)
    /// drawn at `scale`, front first.
    fn laser_pieces<R>(
        &self,
        renderer: &mut R,
        area: Rectangle<f64, Logical>,
        scale: f64,
        now: Duration,
        out: &mut Vec<(Piece, MemoryRenderBufferRenderElement<R>)>,
    ) where
        R: Renderer + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let mut sprites = self.ink.sprites.borrow_mut();
        let sprites = sprites
            .entry((scale * 1000.0).round() as u32)
            .or_insert_with(|| Sprites::new(scale));
        let dot_box = sprite_box(DOT, DOT_SIGMA);
        let trail_box = sprite_box(TRAIL_FROM, TRAIL_SIGMA);
        let head = (
            pointer.current_location(),
            dot_box,
            1.0,
            &sprites.dot,
            Piece::Head,
        );
        let trail = self.ink.laser.trail.iter().rev().filter_map(|dot| {
            let age = now.saturating_sub(dot.since).as_secs_f64() / TRAIL.as_secs_f64();
            if age >= 1.0 {
                return None;
            }
            let (size, alpha) = trail_dot(age);
            Some((
                dot.at,
                trail_box * size / TRAIL_FROM,
                alpha,
                &sprites.trail,
                Piece::Dot(dot.id),
            ))
        });
        for (at, box_size, alpha, (buffer, px), piece) in std::iter::once(head).chain(trail) {
            let logical = box_size.round().max(1.0);
            let half = logical / 2.0;
            let place = Rectangle::<f64, Logical>::new(
                (at.x - half, at.y - half).into(),
                (logical, logical).into(),
            );
            if !place.overlaps(area) {
                continue;
            }
            let local = (
                (place.loc.x - area.loc.x) * scale,
                (place.loc.y - area.loc.y) * scale,
            );
            let src = Rectangle::<f64, Logical>::from_size((f64::from(*px), f64::from(*px)).into());
            let size = (logical as i32, logical as i32);
            match MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                local,
                buffer,
                Some(alpha as f32),
                Some(src),
                Some(size.into()),
                Kind::Unspecified,
            ) {
                Ok(element) => out.push((piece, element)),
                Err(err) => tracing::debug!(?err, "the laser could not be drawn"),
            }
        }
    }
}

/// A piece of a screen's ink element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Piece {
    Head,
    Dot(u64),
    /// A tile, by its serial.
    Tile(u64),
}

/// The next tile's serial.
static NEXT_TILE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// What a screen's ink element showed last, and the damage since.
///
/// The ink is one element whatever it shows: an element's place in the
/// scene is part of what the damage tracker compares, so a dot or a tile
/// coming or going as elements of their own would move every element
/// under them, and the whole screen would be drawn again.
#[derive(Debug)]
struct Scene {
    id: Id,
    bag: DamageBag<i32, Physical>,
    shown: HashMap<Piece, (Rectangle<i32, Physical>, f32)>,
}

impl Scene {
    fn new() -> Self {
        Self {
            id: Id::new(),
            bag: DamageBag::new(16),
            shown: HashMap::new(),
        }
    }
}

/// A screen's ink: its pieces drawn where its damage is.
#[derive(Debug)]
pub struct InkElement<R: Renderer> {
    id: Id,
    damage: DamageSnapshot<i32, Physical>,
    size: Size<i32, Physical>,
    scale: Scale<f64>,
    /// Front first.
    pieces: Vec<MemoryRenderBufferRenderElement<R>>,
}

impl<R: Renderer> Element for InkElement<R> {
    fn id(&self) -> &Id {
        &self.id
    }

    fn current_commit(&self) -> CommitCounter {
        self.damage.current_commit()
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        Rectangle::from_size((f64::from(self.size.w), f64::from(self.size.h)).into())
    }

    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
        Rectangle::from_size(self.size)
    }

    fn damage_since(
        &self,
        _scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.damage
            .damage_since(commit)
            .unwrap_or_else(|| DamageSet::from_slice(&[Rectangle::from_size(self.size)]))
    }

    fn opaque_regions(&self, _scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        OpaqueRegions::default()
    }

    fn kind(&self) -> Kind {
        Kind::Unspecified
    }
}

impl<R> RenderElement<R> for InkElement<R>
where
    R: Renderer + ImportMem,
    R::TextureId: 'static,
{
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        _src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), R::Error> {
        // From the back.
        for piece in self.pieces.iter().rev() {
            let place = piece.geometry(self.scale);
            let local: Vec<Rectangle<i32, Physical>> = damage
                .iter()
                .filter_map(|d| d.intersection(place))
                .map(|mut d| {
                    d.loc -= place.loc;
                    d
                })
                .collect();
            if local.is_empty() {
                continue;
            }
            let mut at = place;
            at.loc += dst.loc;
            piece.draw(frame, piece.src(), at, &local, &[])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn the_strokes_hold_three_seconds_then_fade_in_600_ms() {
        let mut ink = Ink::default();
        ink.begin("A".into(), (10.0, 10.0), Shape::Stroke);
        assert!(ink.extend((20.0, 10.0)));
        assert_eq!(ink.showing(ms(100), false), Showing::Full(None));
        ink.end(ms(1000));
        assert_eq!(ink.showing(ms(3999), false), Showing::Full(Some(ms(4000))));
        let Showing::Fading(half) = ink.showing(ms(4300), false) else {
            panic!("not fading");
        };
        // Ease-in: still more than half way at half the time.
        assert!(half > 0.5 && half < 1.0, "{half}");
        assert_eq!(ink.showing(ms(4600), false), Showing::Over);
        // Reduced motion: gone at 3 s, no fade.
        assert_eq!(ink.showing(ms(4000), true), Showing::Over);
    }

    #[test]
    fn a_new_press_brings_everything_back() {
        let mut ink = Ink::default();
        ink.begin("A".into(), (10.0, 10.0), Shape::Stroke);
        ink.extend((30.0, 10.0));
        ink.end(ms(0));
        assert!(matches!(ink.showing(ms(3300), false), Showing::Fading(_)));
        ink.begin("A".into(), (10.0, 50.0), Shape::Stroke);
        assert_eq!(ink.showing(ms(3300), false), Showing::Full(None));
        // A press without motion draws nothing, and the hold starts again.
        ink.end(ms(3400));
        assert_eq!(ink.marks.len(), 1);
        assert_eq!(ink.showing(ms(6000), false), Showing::Full(Some(ms(6400))));
    }

    #[test]
    fn a_stroke_stops_growing_at_4096_points() {
        let mut ink = Ink::default();
        ink.begin("A".into(), (0.0, 0.0), Shape::Stroke);
        for i in 1..5000 {
            ink.extend((f64::from(i) * 3.0, 0.0));
        }
        assert_eq!(ink.marks[0].points.len(), MAX_POINTS);
        // Points closer than 2 px are the same point.
        let mut ink = Ink::default();
        ink.begin("A".into(), (0.0, 0.0), Shape::Stroke);
        assert!(!ink.extend((1.5, 0.0)));
        assert!(ink.extend((2.5, 0.0)));
    }

    #[test]
    fn an_arrow_keeps_its_ends_and_its_head_shortens() {
        let mut ink = Ink::default();
        ink.begin("A".into(), (0.0, 0.0), Shape::Arrow);
        ink.extend((50.0, 40.0));
        ink.extend((100.0, 0.0));
        assert_eq!(ink.marks[0].points, vec![(0.0, 0.0), (100.0, 0.0)]);
        let [shaft, left, right] = arrow((0.0, 0.0), (100.0, 0.0));
        assert_eq!(shaft, ((0.0, 0.0), (100.0, 0.0)));
        // 22 px back, 0.6 of that across: about 31°.
        assert_eq!(left.0, (78.0, 13.2));
        assert_eq!(right.1, (78.0, -13.2));
        // A 40 px arrow's head is 16 px back.
        let [_, left, _] = arrow((0.0, 0.0), (40.0, 0.0));
        assert!((left.0.0 - 24.0).abs() < 1e-9);
    }

    #[test]
    fn the_pen_is_red_on_the_line_and_clear_past_its_reach() {
        let pen = Pen::new(1.0);
        let [b, g, r, a] = pen.pixel(0.0, 1.0);
        assert_eq!((r, g, b, a), (242, 84, 91, 255));
        assert_eq!(pen.pixel(pen.reach, pen.reach)[3], 0);
        // Just under the line, the dark shadow; above it, only the glow.
        let below = pen.pixel(3.0, 2.0);
        let above = pen.pixel(3.0, 4.0);
        assert!(below[3] > above[3], "{below:?} {above:?}");
        assert!(above[3] > 0);
    }

    #[test]
    fn a_segment_is_clipped_to_a_box() {
        let seg = Seg {
            a: (-10.0, 5.0),
            b: (30.0, 5.0),
        };
        let clipped = seg.clipped((0.0, 0.0, 20.0, 10.0)).expect("inside");
        assert_eq!(clipped.a, (0.0, 5.0));
        assert_eq!(clipped.b, (20.0, 5.0));
        assert!(seg.clipped((0.0, 6.0, 20.0, 10.0)).is_none());
    }

    #[test]
    fn the_laser_shows_while_super_and_alt_are_held_alone() {
        let mut ink = Ink::default();
        assert!(ink.laser_keys(true));
        assert!(ink.laser_shown());
        // A shortcut's key: the laser goes until they are let go.
        assert!(ink.other_key());
        assert!(!ink.laser_shown());
        assert!(!ink.laser_keys(true));
        ink.laser_keys(false);
        assert!(ink.laser_keys(true));
        // Drawing hides it.
        ink.begin("A".into(), (0.0, 0.0), Shape::Stroke);
        assert!(!ink.laser_shown());
    }

    #[test]
    fn the_trail_fades_in_450_ms() {
        let mut ink = Ink::default();
        ink.laser_keys(true);
        ink.laser_moved((0.0, 0.0).into(), ms(0));
        ink.laser_moved((10.0, 0.0).into(), ms(100));
        assert!(ink.laser_tick(ms(449)));
        assert_eq!(ink.laser.trail.len(), 2);
        assert!(ink.laser_tick(ms(500)));
        assert_eq!(ink.laser.trail.len(), 1);
        assert!(!ink.laser_tick(ms(550)));
        assert_eq!(trail_dot(0.0), (10.0, 0.75));
        assert_eq!(trail_dot(0.9).0, 3.0);
    }
}
