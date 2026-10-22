//! The pandas (#47): Hocuspanda's two characters and its logo, drawn from
//! shapes in a 120×120 box. The apprentice magician and the watcher are one
//! family: each has an ear touched by magic, in the accent colour, that says
//! the state: lit while it acts, radiating when something waits for you,
//! orange when stuck, dark asleep.
//!
//! Pandas show real state and never interrupt. They move for a moment when
//! their state changes, then hold still, so they cost nothing at rest; a
//! panda that is the subject of a page (the welcome) may move all the time.
//! With reduced motion they never move.

use std::f32::consts::TAU;
use std::time::Duration;

use hocus_ui::{
    AccessNode, Color, Kind as NodeKind, Length, Look, PixelRect, Point, Rect, Shape, Size, Style,
    Surface, Theme, Transform, Tree, canvas::Canvas, tree::Draw,
};

use crate::column::View;

const VIEW: f32 = 120.0;
/// How long a panda moves after its state changed.
pub const MOVES_FOR: Duration = Duration::from_secs(4);
/// Motion grows in, then settles to the still pose, over these.
const EASE_IN: f32 = 0.3;
const SETTLE: f32 = 0.6;

const INK: Color = Color::rgb(0x23, 0x26, 0x2E);
const CREAM: Color = Color::rgb(0xF7, 0xF4, 0xEC);
const WHITE: Color = Color::rgb(0xFF, 0xFF, 0xFF);
const SLATE: Color = Color::rgb(0x3A, 0x3D, 0x45);
const SCREEN: Color = Color::rgb(0x2A, 0x2C, 0x33);
const SHADE: Color = Color::rgb(0x4A, 0x4E, 0x58);
const SMOKE: Color = Color::rgb(0x8E, 0x93, 0x9C);
const MUTED: Color = Color::rgb(0xA6, 0xA9, 0xB1);
const STUCK: Color = Color::rgb(0xE8, 0x8A, 0x5A);
const STRIPE: Color = Color::rgb(0xD9, 0x48, 0x3B);
const CORN: Color = Color::rgb(0xFF, 0xF4, 0xD8);
const BUTTER: Color = Color::rgb(0xE8, 0xC7, 0x7A);
const HULL: Color = Color::rgb(0xB9, 0x82, 0x35);
const GLOW: Color = Color::rgb(0xCF, 0xE0, 0xFF);
const FLAME: Color = Color::rgb(0xFF, 0xF3, 0xC4);
const SPARKS: Color = Color::rgb(0xFF, 0xE9, 0xA8);
const LENS: Color = Color::rgb(0x8C, 0x9A, 0xAD);

/// Who the panda is. Agents get them in turn, so that two agents side by
/// side are told apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Character {
    /// Mischievous, a little overtaken by his own magic: a patched top hat
    /// that does everything, a bow tie, a wand.
    Apprentice,
    /// Calm and round, keeps the machine at night: welding goggles, a
    /// scarf, a lantern.
    Watcher,
}

impl Character {
    /// The character of the `n`th agent.
    pub fn nth(n: usize) -> Self {
        if n.is_multiple_of(2) {
            Self::Apprentice
        } else {
            Self::Watcher
        }
    }
}

/// What the panda is up to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    /// Nothing asked of it.
    Rest,
    Work,
    /// Waiting for an answer.
    Wait,
    Done,
    Error,
    Play,
    Popcorn,
    Sleep,
    /// Saying hello.
    Wave,
}

impl Mood {
    pub const ALL: [Mood; 9] = [
        Mood::Rest,
        Mood::Work,
        Mood::Wait,
        Mood::Done,
        Mood::Error,
        Mood::Play,
        Mood::Popcorn,
        Mood::Sleep,
        Mood::Wave,
    ];

    /// What it shows, for screen readers.
    pub fn label(self, lang: crate::Lang) -> &'static str {
        lang.panda(self)
    }
}

/// When a panda moves.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Moves {
    Never,
    /// For [`MOVES_FOR`] from then.
    From(Duration),
    Always,
}

/// How an animated part moves, as in the design's keyframes. Times are in
/// seconds; a negative delay starts it part way.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    /// Shrinks and dims, and back.
    Twinkle,
    /// Grows out and fades, again and again.
    Ring,
    Blink,
    /// Up by this much, and back.
    Bob(f32),
    /// Turns from one angle to the other, and back.
    Sway(f32, f32),
    /// Thrown up, turning once.
    Toss,
    /// A puff that rises and fades, drifting by this much sideways.
    Puff(f32),
    /// A Z that rises.
    Rise,
    Breathe,
    Hop,
    /// Through these points and round again.
    Juggle(&'static [(f32, f32)]),
    /// A popcorn grain jumping from the bucket into the mouth.
    Pop,
    /// Light that flickers between these strengths.
    Flicker(&'static [f32]),
    /// A spark flying off by this much.
    Fly(f32, f32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Anim {
    kind: Kind,
    period: f32,
    delay: f32,
    /// What it turns and grows about; `None`: the middle of what it moves.
    pivot: Option<(f32, f32)>,
}

const fn anim(kind: Kind, period: f32, delay: f32) -> Anim {
    Anim {
        kind,
        period,
        delay,
        pivot: None,
    }
}

const fn about(kind: Kind, period: f32, delay: f32, x: f32, y: f32) -> Anim {
    Anim {
        kind,
        period,
        delay,
        pivot: Some((x, y)),
    }
}

/// Away from the still pose: moved, turned and stretched about the pivot,
/// its opacity multiplied.
#[derive(Debug, Clone, Copy)]
struct Pose {
    dx: f32,
    dy: f32,
    degrees: f32,
    sx: f32,
    sy: f32,
    alpha: f32,
}

const STILL: Pose = Pose {
    dx: 0.0,
    dy: 0.0,
    degrees: 0.0,
    sx: 1.0,
    sy: 1.0,
    alpha: 1.0,
};

/// 0 at the start, 1 half way, 0 again at the end: CSS's eased back and forth.
fn swing(p: f32) -> f32 {
    (1.0 - (TAU * p).cos()) / 2.0
}

fn ease_out(p: f32) -> f32 {
    1.0 - (1.0 - p) * (1.0 - p)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Up to 1 by `rise`, then down to 0 by the end.
fn fade(p: f32, rise: f32) -> f32 {
    if p < rise {
        p / rise
    } else {
        1.0 - (p - rise) / (1.0 - rise)
    }
}

/// Where a closed path of points is at `p` through it.
fn along(points: &[(f32, f32)], p: f32) -> (f32, f32) {
    let n = points.len();
    if n == 0 {
        return (0.0, 0.0);
    }
    let at = p * n as f32;
    let i = (at as usize).min(n - 1);
    let t = at - i as f32;
    let (a, b) = (points[i], points[(i + 1) % n]);
    (lerp(a.0, b.0, t), lerp(a.1, b.1, t))
}

impl Kind {
    /// The pose at `p` through its period; `still` is where the part is drawn.
    fn pose(self, p: f32, still: (f32, f32)) -> Pose {
        match self {
            Kind::Twinkle => {
                let w = swing(p);
                Pose {
                    sx: 1.0 - 0.45 * w,
                    sy: 1.0 - 0.45 * w,
                    alpha: 1.0 - 0.4 * w,
                    ..STILL
                }
            }
            Kind::Ring => {
                let e = ease_out(p);
                let s = 0.75 + 0.55 * e;
                Pose {
                    sx: s,
                    sy: s,
                    alpha: 0.95 * (1.0 - e),
                    ..STILL
                }
            }
            Kind::Blink => {
                let sy = if p < 0.88 {
                    1.0
                } else if p < 0.92 {
                    lerp(1.0, 0.12, (p - 0.88) / 0.04)
                } else {
                    lerp(0.12, 1.0, (p - 0.92) / 0.08)
                };
                Pose { sy, ..STILL }
            }
            Kind::Bob(dy) => Pose {
                dy: dy * swing(p),
                ..STILL
            },
            Kind::Sway(from, to) => Pose {
                degrees: lerp(from, to, swing(p)),
                ..STILL
            },
            Kind::Toss => Pose {
                dy: 4.0 - 10.0 * swing(p),
                degrees: 360.0 * p,
                ..STILL
            },
            Kind::Puff(drift) => {
                let e = ease_out(p);
                let s = 0.45 + 0.8 * e;
                Pose {
                    dx: drift * e,
                    dy: 4.0 - 19.0 * e,
                    sx: s,
                    sy: s,
                    alpha: fade(p, 0.25),
                    ..STILL
                }
            }
            Kind::Rise => {
                let e = ease_out(p);
                let s = 0.6 + 0.5 * e;
                Pose {
                    dx: -5.0 + 10.0 * e,
                    dy: 7.0 - 15.0 * e,
                    sx: s,
                    sy: s,
                    alpha: fade(p, 0.3),
                    ..STILL
                }
            }
            Kind::Breathe => {
                let w = swing(p);
                Pose {
                    sx: 1.0 + 0.03 * w,
                    sy: 1.0 - 0.02 * w,
                    ..STILL
                }
            }
            Kind::Hop => {
                let (dy, squash) = if p < 0.25 {
                    (-6.0 * ease_out(p / 0.25), 0.0)
                } else if p < 0.4 {
                    let t = (p - 0.25) / 0.15;
                    (-6.0 * (1.0 - t * t), t)
                } else if p < 0.55 {
                    (0.0, 1.0 - (p - 0.4) / 0.15)
                } else {
                    (0.0, 0.0)
                };
                Pose {
                    dy,
                    sx: 1.0 + 0.03 * squash,
                    sy: 1.0 - 0.03 * squash,
                    ..STILL
                }
            }
            Kind::Juggle(points) => {
                let (x, y) = along(points, p);
                Pose {
                    dx: x - still.0,
                    dy: y - still.1,
                    ..STILL
                }
            }
            Kind::Pop => {
                // From the bucket, up past the face, into the mouth: time,
                // where, size, turn, opacity.
                const KEYS: [(f32, f32, f32, f32, f32, f32); 5] = [
                    (0.0, 66.0, 80.0, 0.5, 0.0, 0.0),
                    (0.45, 76.0, 42.0, 1.0, 160.0, 1.0),
                    (0.80, 61.0, 70.0, 0.9, 300.0, 1.0),
                    (0.86, 61.0, 70.0, 0.5, 320.0, 0.0),
                    (1.0, 61.0, 70.0, 0.5, 320.0, 0.0),
                ];
                let q = p * p;
                let i = KEYS.iter().rposition(|k| k.0 <= q).unwrap_or(0).min(3);
                let (a, b) = (KEYS[i], KEYS[i + 1]);
                let t = ((q - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
                let alpha = if q < 0.12 {
                    q / 0.12
                } else {
                    lerp(a.5, b.5, t)
                };
                let s = lerp(a.3, b.3, t);
                Pose {
                    dx: lerp(a.1, b.1, t) - still.0,
                    dy: lerp(a.2, b.2, t) - still.1,
                    degrees: lerp(a.4, b.4, t),
                    sx: s,
                    sy: s,
                    alpha,
                }
            }
            Kind::Flicker(levels) => {
                let last = levels.len().saturating_sub(1);
                let i = ((p * levels.len() as f32) as usize).min(last);
                Pose {
                    alpha: levels.get(i).copied().unwrap_or(1.0),
                    ..STILL
                }
            }
            Kind::Fly(x, y) => {
                let e = ease_out(p);
                Pose {
                    dx: x * e,
                    dy: y * e,
                    alpha: 1.0 - p,
                    ..STILL
                }
            }
        }
    }
}

impl Pose {
    /// Only `amount` of the way from still to this pose.
    fn toned(self, amount: f32) -> Self {
        Pose {
            dx: self.dx * amount,
            dy: self.dy * amount,
            degrees: self.degrees * amount,
            sx: lerp(1.0, self.sx, amount),
            sy: lerp(1.0, self.sy, amount),
            alpha: lerp(1.0, self.alpha, amount),
        }
    }

    fn transform(self, (x, y): (f32, f32)) -> Transform {
        Transform::scale(self.sx, self.sy, x, y)
            .then(Transform::rotate(self.degrees, x, y))
            .then(Transform::translate(self.dx, self.dy))
    }
}

/// How a shape is painted. A rim is a stroke under the fill: the cream edge
/// that keeps dark parts readable on a dark background.
#[derive(Debug, Clone, Copy)]
struct Paint {
    fill: Option<Color>,
    stroke: Option<(f32, Color)>,
    rim: bool,
}

/// A dark part with its cream rim.
fn dark() -> Paint {
    rimmed(INK, CREAM, 4.0)
}

/// A light part, outlined.
fn fur() -> Paint {
    outlined(CREAM, 2.5)
}

fn solid(color: Color) -> Paint {
    Paint {
        fill: Some(color),
        stroke: None,
        rim: false,
    }
}

fn outlined(color: Color, width: f32) -> Paint {
    Paint {
        fill: Some(color),
        stroke: Some((width, INK)),
        rim: false,
    }
}

fn rimmed(color: Color, rim: Color, width: f32) -> Paint {
    Paint {
        fill: Some(color),
        stroke: Some((width, rim)),
        rim: true,
    }
}

fn line(width: f32, color: Color) -> Paint {
    Paint {
        fill: None,
        stroke: Some((width, color)),
        rim: false,
    }
}

#[derive(Debug, Clone)]
struct Part {
    shape: Shape,
    paint: Paint,
    /// Innermost first, with their pivots.
    anims: Vec<(Anim, (f32, f32))>,
}

/// The parts drawn so far, and where the current placement puts them.
struct Sketch {
    parts: Vec<Part>,
    place: Transform,
}

impl Sketch {
    fn new() -> Self {
        Self {
            parts: Vec::new(),
            place: Transform::IDENTITY,
        }
    }

    fn add(&mut self, shape: Shape, paint: Paint) {
        self.parts.push(Part {
            shape: shape.transformed(self.place),
            paint,
            anims: Vec::new(),
        });
    }

    /// An SVG path; the drawings are written by hand, so one that does not
    /// parse is left out rather than failing the panda (a test checks them).
    fn path(&mut self, d: &str, paint: Paint) {
        if let Some(shape) = Shape::svg(d) {
            self.add(shape, paint);
        }
    }

    fn circle(&mut self, cx: f32, cy: f32, r: f32, paint: Paint) {
        self.add(Shape::circle(cx, cy, r), paint);
    }

    fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, degrees: f32, paint: Paint) {
        self.add(Shape::ellipse(cx, cy, rx, ry, degrees), paint);
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, paint: Paint) {
        self.add(Shape::rect(x, y, w, h, radius), paint);
    }

    /// What `draw` adds, placed by `t` (an SVG group's transform).
    fn placed(&mut self, t: Transform, draw: impl FnOnce(&mut Self)) {
        let outer = self.place;
        self.place = t.then(outer);
        draw(self);
        self.place = outer;
    }

    /// What `draw` adds, moving as `anim` says.
    fn moving(&mut self, anim: Anim, draw: impl FnOnce(&mut Self)) {
        let from = self.parts.len();
        draw(self);
        let added = &mut self.parts[from..];
        let pivot = anim.pivot.unwrap_or_else(|| middle(added));
        for part in added {
            part.anims.push((anim, pivot));
        }
    }
}

/// The middle of the parts' bounds, as CSS's `transform-box: fill-box`.
fn middle(parts: &[Part]) -> (f32, f32) {
    let mut bounds: Option<(f32, f32, f32, f32)> = None;
    for part in parts {
        let (x0, y0, x1, y1) = part.shape.bounds();
        bounds = Some(match bounds {
            None => (x0, y0, x1, y1),
            Some((a, b, c, d)) => (a.min(x0), b.min(y0), c.max(x1), d.max(y1)),
        });
    }
    bounds.map_or((60.0, 60.0), |(x0, y0, x1, y1)| {
        ((x0 + x1) / 2.0, (y0 + y1) / 2.0)
    })
}

const STAR: &str = "M0 -7 L1.8 -1.8 L7 0 L1.8 1.8 L0 7 L-1.8 1.8 L-7 0 L-1.8 -1.8 Z";
const KERNEL: &str = "M-4.6 1.2 C-6.2 -1.4 -3.8 -4.2 -1.6 -3 C-1.2 -5.6 3.2 -5.4 3.2 -2.6 \
     C5.8 -2.2 5.6 1.4 3.4 2.2 C2.8 4.6 -1.2 4.8 -2 3 C-3.6 3.8 -5.6 2.8 -4.6 1.2 Z";
const NOSE: &str = "M56.5 62.4 Q60 60.4 63.5 62.4 Q62.3 65.5 60 66.1 Q57.7 65.5 56.5 62.4 Z";
const SMIRK: &str = "M55 69.5 Q59.5 72.5 65.5 67.8";
const WINK_L: &str = "M44.3 52.4 Q48 48.6 51.7 52.4";
const WINK_R: &str = "M68.3 52.4 Q72 48.6 75.7 52.4";
const JUGGLE: [(f32, f32); 8] = [
    (26.0, 47.0),
    (35.0, 25.0),
    (60.0, 9.0),
    (85.0, 25.0),
    (94.0, 47.0),
    (85.0, 33.0),
    (60.0, 23.0),
    (35.0, 33.0),
];

/// A four-pointed sparkle, `r` from its middle to a point.
fn star(s: &mut Sketch, x: f32, y: f32, r: f32, paint: Paint) {
    s.placed(
        Transform::scale(r / 7.0, r / 7.0, 0.0, 0.0).then(Transform::translate(x, y)),
        |s| s.path(STAR, paint),
    );
}

/// A popped grain, its middle at the origin.
fn kernel(s: &mut Sketch) {
    s.path(KERNEL, outlined(CORN, 1.3));
    s.path("M-3.4 2.6 Q-0.4 4 2.8 1.8", line(1.1, BUTTER));
    s.path("M-0.6 3.4 L0.6 5 L1.6 3.2 Z", solid(HULL));
}

/// The magic ear for the mood.
fn magic_ear(mood: Mood, accent: Color) -> Paint {
    match mood {
        Mood::Error => rimmed(STUCK, INK, 2.5),
        Mood::Sleep => rimmed(SHADE, CREAM, 4.0),
        _ => rimmed(accent, INK, 2.5),
    }
}

fn ear_colour(mood: Mood, accent: Color) -> Color {
    magic_ear(mood, accent).fill.unwrap_or(accent)
}

/// The patched top hat, its brim's middle at the origin.
fn top_hat(s: &mut Sketch, accent: Color) {
    let rim = rimmed(INK, CREAM, 3.0);
    s.path(
        "M-11.5 0.5 C-12.5 -8 -14.5 -16 -13.5 -24 Q0 -27 13.5 -24 C14.5 -16 12.5 -8 11.5 0.5 \
         Q0 2 -11.5 0.5 Z",
        rim,
    );
    s.ellipse(0.0, -24.4, 13.2, 2.6, 0.0, solid(SLATE));
    s.path("M-9.6 -5 C-10.6 -11 -11.6 -16 -11.2 -21", line(1.6, SHADE));
    s.path("M-4 -22.6 Q-1 -19.4 3 -22.2", line(1.3, SHADE));
    s.placed(Transform::rotate(-8.0, 4.0, -14.0), |s| {
        s.rect(0.5, -18.0, 7.0, 6.5, 1.0, solid(SLATE));
        s.rect(0.5, -18.0, 7.0, 6.5, 1.0, line(0.7, CREAM.with_alpha(90)));
    });
    s.path(
        "M-12.1 -2.6 Q0 -1 12.1 -2.6 L12.7 -8 Q0 -6.4 -12.7 -8 Z",
        outlined(accent, 1.2),
    );
    s.path(
        "M-23 -1.5 C-21.5 5 21.5 5 23 -1.5 C17 1.6 -17 1.6 -23 -1.5 Z",
        rim,
    );
    s.moving(anim(Kind::Twinkle, 2.6, 0.0), |s| {
        star(s, 14.0, -12.6, 5.4, outlined(accent, 1.0));
    });
}

/// The apprentice's head without its ears.
fn face(s: &mut Sketch) {
    s.ellipse(60.0, 54.0, 30.0, 27.0, 0.0, fur());
    s.path(
        "M39 47 C43 40 54 42 55.5 50 C56.5 57 50 62.5 44.5 61.5 C39.5 60.5 36.5 53 39 47 Z",
        solid(INK),
    );
    s.path(
        "M81 47 C77 40 66 42 64.5 50 C63.5 57 70 62.5 75.5 61.5 C80.5 60.5 83.5 53 81 47 Z",
        solid(INK),
    );
}

/// An open eye, its pupil moved by `look`; `lid` half closes it.
fn eye(s: &mut Sketch, x: f32, look: (f32, f32), lid: bool) {
    s.circle(x, 51.5, 3.7, solid(WHITE));
    let (px, py) = (x + look.0, 51.5 + look.1);
    s.circle(px, py, 2.1, solid(INK));
    s.circle(px - 0.7, py - 0.8, 0.8, solid(WHITE));
    if lid {
        s.path(
            &format!("M{} 51.5 A4 4 0 0 1 {} 51.5 Z", x - 4.0, x + 4.0),
            solid(INK),
        );
    }
}

/// No motion: a group that only some moods animate.
const NONE: Anim = anim(Kind::Bob(0.0), 1.0, 0.0);

fn apprentice(mood: Mood, accent: Color) -> Sketch {
    use Mood::*;
    let mut s = Sketch::new();
    let body = match mood {
        Sleep => about(Kind::Breathe, 4.0, 0.0, 60.0, 115.0),
        _ => NONE,
    };
    s.moving(body, |s| {
        let is = |moods: &[Mood]| moods.contains(&mood);
        let ear = magic_ear(mood, accent);
        // Ears, the magic one folded, drooping when stuck.
        s.circle(37.0, 31.0, 10.5, dark());
        if mood == Error {
            s.ellipse(86.0, 38.0, 11.5, 7.5, 68.0, ear);
        } else {
            s.ellipse(85.0, 33.0, 11.5, 7.5, 38.0, ear);
        }
        // Legs, body, arms.
        s.ellipse(45.0, 108.0, 10.0, 7.0, 0.0, dark());
        s.ellipse(75.0, 108.0, 10.0, 7.0, 0.0, dark());
        s.ellipse(60.0, 91.0, 27.0, 22.0, 0.0, fur());
        if is(&[Rest, Error]) {
            s.ellipse(35.0, 90.0, 8.0, 14.0, 18.0, dark());
        }
        if mood == Error {
            s.ellipse(85.0, 90.0, 8.0, 14.0, -18.0, dark());
        }
        if mood == Rest {
            s.ellipse(86.0, 80.0, 7.5, 13.0, -40.0, dark());
        }
        if is(&[Done, Play]) {
            let (left, right) = if mood == Play {
                (
                    anim(Kind::Bob(-2.2), 0.9, 0.0),
                    anim(Kind::Bob(-2.2), 0.9, -0.45),
                )
            } else {
                (NONE, NONE)
            };
            s.moving(left, |s| s.ellipse(30.0, 62.0, 7.5, 15.0, -30.0, dark()));
            s.moving(right, |s| s.ellipse(90.0, 62.0, 7.5, 15.0, 30.0, dark()));
        }
        if is(&[Sleep, Wave]) {
            s.ellipse(45.0, 95.0, 7.0, 9.0, 32.0, dark());
            s.ellipse(75.0, 95.0, 7.0, 9.0, -32.0, dark());
        }
        if mood == Work {
            // The laptop seen from behind: its lid carries the spark.
            s.path("M28 98 L92 98 L86 113 L34 113 Z", outlined(SLATE, 2.0));
            s.rect(33.0, 78.0, 54.0, 21.0, 3.0, outlined(SCREEN, 2.0));
            s.moving(anim(Kind::Twinkle, 1.8, 0.0), |s| {
                star(s, 60.0, 88.5, 6.5, solid(accent));
            });
            s.moving(anim(Kind::Bob(-2.2), 0.46, 0.0), |s| {
                s.ellipse(42.0, 100.0, 8.0, 6.0, 0.0, dark());
            });
            s.moving(anim(Kind::Bob(-2.2), 0.46, -0.23), |s| {
                s.ellipse(78.0, 100.0, 8.0, 6.0, 0.0, dark());
            });
        }
        // The head and face.
        face(s);
        let look = match mood {
            Work => (0.0, 1.5),
            Wait => (1.0, -1.2),
            Error => (1.6, 0.0),
            Play => (0.0, -1.6),
            Popcorn => (1.6, 0.2),
            _ => (0.0, 0.0),
        };
        let lid = is(&[Rest, Work, Popcorn]);
        if is(&[Work, Wait, Error, Play, Popcorn]) {
            eye(s, 48.0, look, lid);
        }
        if is(&[Rest, Work, Wait, Error, Play, Popcorn, Wave]) {
            let blink = if is(&[Rest, Wave]) {
                anim(Kind::Blink, 4.6, 0.0)
            } else {
                NONE
            };
            s.moving(blink, |s| eye(s, 72.0, look, lid));
        }
        // His wink, or both eyes smiling.
        if is(&[Rest, Wave, Done]) {
            s.path(WINK_L, line(2.2, CREAM));
        }
        if mood == Done {
            s.path(WINK_R, line(2.2, CREAM));
        }
        s.path(NOSE, solid(INK));
        match mood {
            Rest | Popcorn => s.path(SMIRK, line(2.0, INK)),
            Done | Play | Wave => s.path(
                "M53.5 68 Q60 77 66.5 68 Q60 70.5 53.5 68 Z",
                outlined(INK, 1.2),
            ),
            Work => s.path("M57 70.2 L63 70.2", line(2.0, INK)),
            Wait => s.ellipse(60.0, 70.8, 2.4, 2.9, 0.0, solid(INK)),
            Error => s.path("M54 70.5 Q57 68.4 60 70.5 Q63 72.6 66 70.5", line(2.0, INK)),
            Sleep => s.path("M57.5 70 Q60 71.4 62.5 70", line(2.0, INK)),
        }
        // The bow tie, his stage costume.
        if !is(&[Work, Popcorn]) {
            let bow = outlined(accent, 1.8);
            s.path("M60 83 L50.5 78 Q48.6 83 50.5 88 Z", bow);
            s.path("M60 83 L69.5 78 Q71.4 83 69.5 88 Z", bow);
            s.rect(57.5, 80.4, 5.0, 5.2, 1.6, bow);
        }
        if mood == Popcorn {
            popcorn(s);
        }
        // The hat wherever it is: on his head, held out, thrown, over his
        // eyes; and the arm that waves it.
        let hat = match mood {
            Rest => Some((57.0, 29.0, -12.0)),
            Work => Some((56.0, 29.0, -8.0)),
            Wait => Some((60.0, 89.0, 180.0)),
            Error => Some((66.0, 28.0, 22.0)),
            Done => Some((30.0, 34.0, -16.0)),
            Popcorn => Some((55.0, 30.0, -18.0)),
            Sleep => Some((60.0, 58.0, -5.0)),
            Wave => Some((31.0, 46.0, -18.0)),
            Play => None,
        };
        let swing = match mood {
            Wait => about(Kind::Sway(-4.0, 4.0), 2.6, 0.0, 60.0, 100.0),
            Wave => about(Kind::Sway(-8.0, 9.0), 1.1, 0.0, 36.0, 76.0),
            _ => NONE,
        };
        s.moving(swing, |s| {
            if mood == Wave {
                s.ellipse(30.0, 62.0, 7.5, 15.0, -30.0, dark());
            }
            if let Some((x, y, degrees)) = hat {
                let toss = if mood == Done {
                    anim(Kind::Toss, 1.5, 0.0)
                } else {
                    NONE
                };
                s.placed(
                    Transform::rotate(degrees, 0.0, 0.0).then(Transform::translate(x, y)),
                    |s| s.moving(toss, |s| top_hat(s, accent)),
                );
            }
        });
        if mood == Wait {
            s.ellipse(38.0, 92.0, 7.0, 8.0, 14.0, dark());
            s.ellipse(82.0, 92.0, 7.0, 8.0, -14.0, dark());
        }
        // The wand: raised, behind the ear, or broken.
        match mood {
            Rest => {
                s.path("M84 80 L103 58", line(3.6, INK));
                s.path("M100.6 60.8 L103 58", line(3.6, CREAM));
                s.moving(anim(Kind::Twinkle, 1.4, 0.0), |s| {
                    star(s, 107.0, 54.0, 7.0, solid(accent));
                });
                s.circle(114.5, 44.0, 1.8, solid(accent));
            }
            Work => {
                s.path("M75 17 L101 34", line(3.4, INK));
                s.path("M98.2 32.2 L101 34", line(3.4, CREAM));
            }
            Error => {
                s.path("M86 89 L95 79 L101 86", line(3.4, INK));
                puffs(
                    s,
                    6.0,
                    [(104.0, 74.0, 5.0), (109.0, 66.0, 4.0), (104.0, 59.0, 3.2)],
                );
            }
            _ => {}
        }
        if is(&[Wait, Done]) {
            rings(
                s,
                ear_colour(mood, accent),
                (85.0, 33.0),
                [
                    "M70.9 27.9 A15 15 0 1 1 96.5 42.6",
                    "M66.2 26.2 A20 20 0 1 1 100.3 45.9",
                ],
            );
        }
        match mood {
            Wait => s.moving(anim(Kind::Bob(-2.5), 1.3, 0.0), |s| {
                s.path(
                    "M92.5 6 Q92.5 1 97.5 1 Q102.5 1 102.5 6 Q102.5 9.5 98 11 L98 14",
                    line(3.0, accent),
                );
                s.circle(98.0, 18.5, 1.8, solid(accent));
            }),
            Done => confetti(
                s,
                accent,
                &[(42.0, 10.0, 6.0), (108.0, 55.5, 5.5)],
                &[(72.0, 6.0, 2.2), (112.0, 20.0, 1.8), (10.0, 62.0, 2.0)],
            ),
            Play => juggle(s, accent),
            Sleep => zzz(
                s,
                [
                    "M92 20 L100 20 L92 28 L100 28",
                    "M104 6 L109 6 L104 11 L109 11",
                ],
            ),
            _ => {}
        }
    });
    s
}

/// Two balls and a sparkle, a third of the way apart on the same loop.
fn juggle(s: &mut Sketch, accent: Color) {
    for (i, delay) in [0.0_f32, -0.6, -1.2].into_iter().enumerate() {
        let (x, y) = along(&JUGGLE, (-delay / 1.8 + 0.25).fract());
        s.moving(
            about(Kind::Juggle(&JUGGLE), 1.8, delay - 0.45, x, y),
            |s| match i {
                0 => s.circle(x, y, 6.0, outlined(accent, 2.0)),
                1 => s.circle(x, y, 6.0, outlined(CREAM, 2.0)),
                _ => star(s, x, y, 7.0, outlined(accent, 1.6)),
            },
        );
    }
}

fn popcorn(s: &mut Sketch) {
    // The screen's light on his face.
    s.moving(anim(Kind::Flicker(&[0.9, 1.5, 0.6]), 1.3, 0.0), |s| {
        s.ellipse(81.0, 58.0, 12.0, 17.0, 0.0, solid(GLOW.with_alpha(46)));
    });
    // A striped bucket, the sign of popcorn everywhere.
    let stripes = [
        (42.0, 49.2, 47.0, 52.2),
        (49.2, 56.4, 52.2, 57.4),
        (56.4, 63.6, 57.4, 62.6),
        (63.6, 70.8, 62.6, 67.8),
        (70.8, 78.0, 67.8, 73.0),
    ];
    for (i, (t0, t1, b0, b1)) in stripes.into_iter().enumerate() {
        let colour = if i % 2 == 0 { STRIPE } else { CREAM };
        s.path(
            &format!("M{t0} 84 L{t1} 84 L{b1} 113 L{b0} 113 Z"),
            solid(colour),
        );
    }
    s.path("M42 84 L78 84 L73 113 L47 113 Z", line(2.0, INK));
    for (x, y, degrees) in [
        (57.0, 76.0, 0.0),
        (65.0, 75.5, 40.0),
        (46.5, 81.0, -30.0),
        (53.0, 80.5, 80.0),
        (61.5, 81.0, -60.0),
        (70.0, 80.5, 15.0),
        (74.5, 82.0, -20.0),
    ] {
        s.placed(
            Transform::rotate(degrees, 0.0, 0.0).then(Transform::translate(x, y)),
            kernel,
        );
    }
    s.ellipse(41.0, 97.0, 7.0, 8.0, 10.0, dark());
    s.ellipse(79.0, 97.0, 7.0, 8.0, -10.0, dark());
    s.moving(about(Kind::Pop, 2.4, 0.0, 76.0, 42.0), |s| {
        s.placed(Transform::translate(76.0, 42.0), kernel);
    });
}

/// The magic ear radiating: two arcs on its outer side.
fn rings(s: &mut Sketch, colour: Color, (x, y): (f32, f32), arcs: [&str; 2]) {
    s.moving(about(Kind::Ring, 1.9, 0.0, x, y), |s| {
        s.path(arcs[0], line(2.2, colour));
    });
    s.moving(about(Kind::Ring, 1.9, -0.95, x, y), |s| {
        s.path(arcs[1], line(2.2, colour.with_alpha(153)));
    });
}

/// Sparkles and dots twinkling, every other one half a beat apart.
fn confetti(s: &mut Sketch, accent: Color, stars: &[(f32, f32, f32)], dots: &[(f32, f32, f32)]) {
    let beat = |i: usize| anim(Kind::Twinkle, 1.1, if i % 2 == 1 { -0.55 } else { 0.0 });
    for (i, &(x, y, r)) in stars.iter().enumerate() {
        s.moving(beat(i), |s| star(s, x, y, r, solid(accent)));
    }
    for (i, &(x, y, r)) in dots.iter().enumerate() {
        s.moving(beat(stars.len() + i), |s| s.circle(x, y, r, solid(accent)));
    }
}

/// Smoke rising, a third of the way apart.
fn puffs(s: &mut Sketch, drift: f32, at: [(f32, f32, f32); 3]) {
    let puff = rimmed(SMOKE, CREAM, 2.5);
    for ((x, y, r), delay) in at.into_iter().zip([0.0, -0.7, -1.4]) {
        s.moving(anim(Kind::Puff(drift), 2.1, delay), |s| {
            s.circle(x, y, r, puff)
        });
    }
}

fn zzz(s: &mut Sketch, paths: [&str; 2]) {
    for (d, delay) in paths.into_iter().zip([0.0, -1.4]) {
        s.moving(anim(Kind::Rise, 2.8, delay), |s| {
            s.path(d, line(2.4, MUTED))
        });
    }
}

/// The watcher has fewer moods: playing, watching and waving are his rest.
fn watcher(mood: Mood, accent: Color) -> Sketch {
    use Mood::*;
    let mood = match mood {
        Play | Popcorn | Wave => Rest,
        other => other,
    };
    let mut s = Sketch::new();
    let body = match mood {
        Done => about(Kind::Hop, 1.4, 0.0, 60.0, 116.0),
        Sleep => about(Kind::Breathe, 4.2, 0.0, 60.0, 116.0),
        _ => NONE,
    };
    s.moving(body, |s| {
        let is = |moods: &[Mood]| moods.contains(&mood);
        s.ellipse(46.0, 110.0, 9.5, 6.5, 0.0, dark());
        s.ellipse(74.0, 110.0, 9.5, 6.5, 0.0, dark());
        s.ellipse(60.0, 94.0, 25.0, 19.0, 0.0, fur());
        if is(&[Rest, Wait, Error, Sleep]) {
            s.ellipse(37.0, 93.0, 7.5, 13.0, 15.0, dark());
        }
        if is(&[Error, Sleep]) {
            s.ellipse(83.0, 93.0, 7.5, 13.0, -15.0, dark());
        }
        if mood == Done {
            s.ellipse(29.0, 66.0, 7.0, 14.0, -32.0, dark());
        }
        if is(&[Done, Wait]) {
            s.ellipse(91.0, 66.0, 7.0, 14.0, 32.0, dark());
        }
        if mood == Rest {
            // Adjusting his goggles.
            s.ellipse(86.0, 58.0, 7.0, 14.0, 42.0, dark());
        }
        if mood == Work {
            s.ellipse(45.0, 96.0, 7.0, 9.0, 30.0, dark());
            s.ellipse(75.0, 96.0, 7.0, 9.0, -30.0, dark());
            // The device he mends, sparks flying off his weld.
            s.rect(42.0, 98.0, 36.0, 14.0, 4.0, outlined(SLATE, 2.0));
            s.rect(47.0, 102.0, 14.0, 2.6, 1.3, solid(accent));
            star(s, 80.0, 97.0, 5.0, solid(accent));
            for ((x, y, r), (fx, fy), delay) in [
                ((82.0, 95.0, 1.4), (12.0, -14.0), 0.0),
                ((82.0, 96.0, 1.2), (17.0, -3.0), -0.23),
                ((81.0, 94.0, 1.3), (6.0, -18.0), -0.46),
            ] {
                s.moving(anim(Kind::Fly(fx, fy), 0.7, delay), |s| {
                    s.circle(x, y, r, solid(SPARKS));
                });
            }
        }
        // The head, tilted asleep.
        let tilt = if mood == Sleep {
            Transform::rotate(9.0, 60.0, 60.0)
        } else {
            Transform::IDENTITY
        };
        s.placed(tilt, |s| watcher_head(s, mood, accent));
        // The scarf, over the neck.
        let scarf = outlined(accent, 2.0);
        s.path("M35 73 Q60 83 85 73 L86 80.5 Q60 92 34 80.5 Z", scarf);
        s.path("M72 82 L80 101 L71 102.5 L66 85 Z", scarf);
        s.path(
            "M72.5 102 L72 105.5 M75.5 101.6 L75.5 105 M78.5 101.2 L79 104.5",
            line(1.5, INK),
        );
        match mood {
            Wait => s.moving(about(Kind::Sway(-9.0, 9.0), 3.0, 0.0, 99.0, 11.0), |s| {
                // The lantern raised to call you, its halo breathing.
                s.moving(anim(Kind::Twinkle, 3.0, 0.0), |s| {
                    s.circle(99.0, 30.0, 15.0, solid(accent.with_alpha(56)));
                });
                s.path("M93 18 Q99 10 105 18", line(2.2, INK));
                s.rect(92.5, 18.0, 13.0, 4.0, 1.5, solid(INK));
                s.rect(93.5, 22.0, 11.0, 15.0, 3.0, outlined(accent, 2.0));
                s.rect(92.5, 37.0, 13.0, 3.5, 1.5, solid(INK));
                s.path("M99 26 L99 33", line(2.4, FLAME));
            }),
            Done => confetti(
                s,
                accent,
                &[(16.0, 33.0, 7.0), (106.0, 45.5, 5.5)],
                &[(26.0, 12.0, 2.0), (108.0, 58.0, 2.2)],
            ),
            Error => puffs(
                s,
                -5.0,
                [(80.0, 12.0, 5.5), (90.0, 8.0, 4.5), (98.0, 14.0, 3.5)],
            ),
            Sleep => zzz(
                s,
                [
                    "M94 14 L102 14 L94 22 L102 22",
                    "M106 3 L111 3 L106 8 L111 8",
                ],
            ),
            _ => {}
        }
    });
    s
}

fn watcher_head(s: &mut Sketch, mood: Mood, accent: Color) {
    use Mood::*;
    let is = |moods: &[Mood]| moods.contains(&mood);
    s.circle(34.0, 25.0, 10.5, dark());
    s.circle(86.0, 25.0, 10.5, magic_ear(mood, accent));
    s.circle(34.0, 25.0, 4.5, solid(SLATE));
    s.circle(60.0, 50.0, 31.0, fur());
    s.ellipse(47.0, 53.0, 9.0, 10.0, -10.0, solid(INK));
    s.ellipse(73.0, 53.0, 9.0, 10.0, 10.0, solid(INK));
    let down = is(&[Work, Sleep]);
    let look = match mood {
        Work => (0.0, 1.0),
        Wait => (1.4, -1.4),
        Error => (0.8, -1.8),
        _ => (0.0, 0.0),
    };
    let eyes_y = if down { 52.0 } else { 53.0 } + look.1;
    let open = is(&[Rest, Work, Wait, Error]);
    if open && !down {
        let blink = if is(&[Rest, Wait]) {
            anim(Kind::Blink, 5.0, 0.0)
        } else {
            NONE
        };
        s.moving(blink, |s| {
            s.circle(48.0 + look.0, eyes_y, 2.5, solid(CREAM));
            s.circle(72.0 + look.0, eyes_y, 2.5, solid(CREAM));
        });
    }
    if mood == Done {
        s.path("M44 54 Q47.5 50.5 51 54", line(2.2, CREAM));
        s.path("M69 54 Q72.5 50.5 76 54", line(2.2, CREAM));
    }
    s.ellipse(60.0, 63.5, 3.8, 2.8, 0.0, solid(INK));
    match mood {
        Rest | Sleep => s.path("M56 68.5 Q60 71.2 64 68.5", line(2.0, INK)),
        Done => s.path(
            "M54.5 67.5 Q60 75 65.5 67.5 Q60 69.6 54.5 67.5 Z",
            solid(INK),
        ),
        Work => s.path("M57 69 L63 69", line(2.0, INK)),
        Wait => s.ellipse(60.0, 69.5, 2.2, 2.6, 0.0, solid(INK)),
        Error => s.path(
            "M55 69.5 Q57.5 67.8 60 69.5 Q62.5 71.2 65 69.5",
            line(2.0, INK),
        ),
        Play | Popcorn | Wave => {}
    }
    if down {
        // Goggles down on the eyes: glowing while he welds, dark asleep.
        s.path("M29 52 L39.5 52 M80.5 52 L91 52", line(4.5, INK));
        s.path("M56.5 52 L63.5 52", line(3.0, INK));
        let lens = if mood == Sleep {
            SLATE
        } else {
            accent.with_alpha(179)
        };
        let flicker = if mood == Work {
            anim(Kind::Flicker(&[1.0, 0.55]), 0.5, 0.0)
        } else {
            NONE
        };
        s.moving(flicker, |s| {
            s.circle(48.0, 52.0, 8.5, outlined(lens, 2.5));
            s.circle(72.0, 52.0, 8.5, outlined(lens, 2.5));
        });
        if open {
            s.circle(48.0 + look.0, eyes_y, 2.3, solid(CREAM));
            s.circle(72.0 + look.0, eyes_y, 2.3, solid(CREAM));
        }
        s.path(
            "M44 49 Q46 47 48.5 47.2 M68 49 Q70 47 72.5 47.2",
            line(1.5, WHITE.with_alpha(204)),
        );
    } else {
        // Goggles up on the forehead, knocked askew when it smoked.
        let askew = if mood == Error {
            Transform::rotate(-9.0, 60.0, 31.0)
        } else {
            Transform::IDENTITY
        };
        s.placed(askew, |s| {
            s.path("M29.5 36 Q60 23 90.5 36", line(4.5, INK));
            s.path("M56 31 L64 31", line(3.0, INK));
            s.circle(49.0, 31.0, 7.5, outlined(LENS, 2.5));
            s.circle(71.0, 31.0, 7.5, outlined(LENS, 2.5));
            let shine = line(1.6, WHITE.with_alpha(217));
            s.path("M45.5 28.5 Q47.5 26.5 50 26.8", shine);
            s.path("M67.5 28.5 Q69.5 26.5 72 26.8", shine);
            if mood == Error {
                s.path("M67 28 L70 31 L68.5 33 L72 35.5", line(1.3, INK));
            }
        });
    }
    if is(&[Wait, Done]) {
        rings(
            s,
            ear_colour(mood, accent),
            (86.0, 25.0),
            [
                "M71.2 22.4 A15 15 0 0 1 99 32.5",
                "M66.3 21.5 A20 20 0 0 1 103.3 35",
            ],
        );
    }
}

/// Hocuspanda's logo: the apprentice's head, winking, its magic ear and a
/// sparkle.
fn logo(accent: Color) -> Sketch {
    let mut s = Sketch::new();
    s.placed(
        Transform::scale(1.35, 1.35, 0.0, 0.0).then(Transform::translate(-21.0, -6.9)),
        |s| {
            s.circle(37.0, 31.0, 10.5, dark());
            s.ellipse(85.0, 33.0, 11.5, 7.5, 38.0, outlined(accent, 2.5));
            face(s);
            s.path(WINK_L, line(2.2, CREAM));
            eye(s, 72.0, (0.0, 0.0), false);
            s.path(NOSE, solid(INK));
            s.path(SMIRK, line(2.0, INK));
        },
    );
    s.moving(anim(Kind::Twinkle, 2.4, 0.0), |s| {
        star(s, 104.0, 16.0, 9.0, solid(accent));
    });
    s.moving(anim(Kind::Twinkle, 2.4, -1.2), |s| {
        s.circle(112.0, 33.0, 2.4, solid(accent));
    });
    s
}

/// A panda in a mood, or the logo, ready to draw.
pub struct Panda {
    parts: Vec<Part>,
    moves: Moves,
    /// Reduced motion: it never moves, whatever it is asked.
    still: bool,
}

impl std::fmt::Debug for Panda {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Panda")
            .field("parts", &self.parts.len())
            .field("moves", &self.moves)
            .finish()
    }
}

impl Panda {
    /// Still until told to move.
    pub fn new(character: Character, mood: Mood, accent: Color) -> Self {
        Self::from_sketch(match character {
            Character::Apprentice => apprentice(mood, accent),
            Character::Watcher => watcher(mood, accent),
        })
    }

    /// The accent and the motion setting from the theme.
    pub fn themed(character: Character, mood: Mood, theme: &Theme) -> Self {
        Self {
            still: theme.reduced_motion,
            ..Self::new(character, mood, theme.colors.accent)
        }
    }

    /// The logo, in the theme's accent.
    pub fn logo(theme: &Theme) -> Self {
        Self {
            still: theme.reduced_motion,
            ..Self::from_sketch(logo(theme.colors.accent))
        }
    }

    fn from_sketch(sketch: Sketch) -> Self {
        Self {
            parts: sketch.parts,
            moves: Moves::Never,
            still: false,
        }
    }

    /// Moves for a moment from `now`: its state just changed.
    pub fn moving_from(mut self, now: Duration) -> Self {
        self.moves = Moves::From(now);
        self
    }

    /// Moves all the time, for a panda that is what the page is about.
    pub fn looping(mut self) -> Self {
        self.moves = Moves::Always;
        self
    }

    /// How much of its motion shows at `now`, and how far into it it is.
    fn motion(&self, now: Duration) -> Option<(f32, f32)> {
        if self.still {
            return None;
        }
        match self.moves {
            Moves::Never => None,
            Moves::Always => Some((1.0, now.as_secs_f32())),
            Moves::From(since) => {
                let t = now.checked_sub(since)?.as_secs_f32();
                let left = MOVES_FOR.as_secs_f32() - t;
                (left > 0.0).then(|| ((t / EASE_IN).min(left / SETTLE).min(1.0), t))
            }
        }
    }

    /// How many of its parts can move.
    pub fn moving_parts(&self) -> usize {
        self.parts
            .iter()
            .filter(|p| p.anims.iter().any(|(a, _)| a.kind != NONE.kind))
            .count()
    }
}

impl Part {
    /// Its shape and opacity at this point of the motion; `None` when it is
    /// where it was drawn.
    fn posed(&self, motion: Option<(f32, f32)>) -> (Option<Shape>, f32) {
        let Some((amount, t)) = motion else {
            return (None, 1.0);
        };
        let mut transform = Transform::IDENTITY;
        let mut alpha = 1.0;
        let mut moved = false;
        for (anim, pivot) in &self.anims {
            if anim.kind == NONE.kind {
                continue;
            }
            let p = ((t - anim.delay) / anim.period).rem_euclid(1.0);
            let pose = anim.kind.pose(p, *pivot).toned(amount);
            transform = transform.then(pose.transform(*pivot));
            alpha *= pose.alpha;
            moved = true;
        }
        if !moved {
            return (None, 1.0);
        }
        (
            Some(self.shape.transformed(transform)),
            alpha.clamp(0.0, 1.0),
        )
    }
}

fn faded(color: Color, alpha: f32) -> Color {
    if alpha >= 1.0 {
        return color;
    }
    color.with_alpha((f32::from(color.a) * alpha).round() as u8)
}

impl Draw for Panda {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, _theme: &Theme, now: Duration) {
        // Square, in the middle of what it was given.
        let side = rect.w.min(rect.h);
        let r = Rect::new(
            rect.x + (rect.w - side) / 2.0,
            rect.y + (rect.h - side) / 2.0,
            side,
            side,
        );
        let motion = self.motion(now);
        for part in &self.parts {
            let (moved, alpha) = part.posed(motion);
            if alpha <= 0.0 {
                continue;
            }
            let shape = moved.as_ref().unwrap_or(&part.shape);
            let paint = part.paint;
            let stroke = |canvas: &mut Canvas| {
                if let Some((width, color)) = paint.stroke {
                    canvas.stroke_shape(shape, VIEW, r, width, faded(color, alpha));
                }
            };
            if paint.rim {
                stroke(canvas);
            }
            if let Some(color) = paint.fill {
                canvas.fill_shape(shape, VIEW, r, faded(color, alpha));
            }
            if !paint.rim {
                stroke(canvas);
            }
        }
    }

    fn size(&self) -> Size {
        Size::new(VIEW, VIEW)
    }

    fn is_moving(&self, now: Duration) -> bool {
        self.motion(now).is_some()
    }
}

/// A panda alone on a screen, for the compositor to show over it: one eating
/// popcorn in a corner while a video plays on another screen.
pub struct Spot {
    tree: Tree,
    surface: Surface,
}

impl std::fmt::Debug for Spot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Spot").finish_non_exhaustive()
    }
}

impl Spot {
    /// `side` logical pixels square; it moves from `now`, then holds still.
    pub fn new(
        theme: Theme,
        lang: crate::Lang,
        character: Character,
        mood: Mood,
        side: f32,
        scale: f32,
        now: Duration,
    ) -> Self {
        let size = Size::new(side, side);
        let panda = Panda::themed(character, mood, &theme).moving_from(now);
        let mut tree = Tree::new(theme, size, scale);
        let root = tree.root();
        tree.set_look(root, Look::default());
        tree.add(
            root,
            Style {
                width: Length::Px(side),
                height: Length::Px(side),
                ..Style::default()
            },
            NodeKind::Custom {
                draw: Box::new(panda),
                label: Some(mood.label(lang).into()),
            },
        );
        let surface = Surface::new(size, scale);
        Self { tree, surface }
    }

    /// What changed since the last time, in pixels.
    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
        let damage = self.tree.update(now);
        self.surface.draw(&mut self.tree, &damage)
    }

    /// Whether it still moves: frames are wanted until it holds still.
    pub fn is_moving(&self, now: Duration) -> bool {
        self.tree.is_animating(now)
    }

    pub fn view(&self) -> View<'_> {
        View {
            pixels: self.surface.pixels(),
            width: self.surface.width(),
            height: self.surface.height(),
            at: Point::default(),
        }
    }

    pub fn access(&self) -> AccessNode {
        self.tree.access()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCENT: Color = Color::rgb(0x7B, 0xC4, 0x7F);

    #[test]
    fn every_mood_of_both_characters_draws_all_its_paths() {
        // Paths are parsed when drawn; one that does not parse would be
        // silently missing.
        let count = |d: &str| usize::from(Shape::svg(d).is_some());
        assert_eq!(count(STAR) + count(KERNEL) + count(NOSE) + count(SMIRK), 4);
        for character in [Character::Apprentice, Character::Watcher] {
            for mood in Mood::ALL {
                let panda = Panda::new(character, mood, ACCENT);
                assert!(
                    panda.parts.len() >= 20,
                    "{character:?} {mood:?}: {}",
                    panda.parts.len()
                );
            }
        }
    }

    #[test]
    fn it_moves_for_a_moment_after_a_change_then_holds_still() {
        let at = Duration::from_secs(10);
        let panda = Panda::new(Character::Apprentice, Mood::Play, ACCENT).moving_from(at);
        assert!(panda.moving_parts() >= 3, "the juggled balls move");
        assert!(!panda.is_moving(at - Duration::from_millis(1)));
        assert!(panda.is_moving(at + Duration::from_secs(1)));
        assert!(!panda.is_moving(at + MOVES_FOR));
    }

    #[test]
    fn a_still_panda_never_moves() {
        let panda = Panda::new(Character::Watcher, Mood::Wait, ACCENT);
        assert!(!panda.is_moving(Duration::from_secs(3)));
        let theme = Theme::default().with_reduced_motion(true);
        let reduced = Panda::themed(Character::Apprentice, Mood::Play, &theme).looping();
        assert!(!reduced.is_moving(Duration::from_secs(3)));
    }

    #[test]
    fn motion_grows_in_and_settles_without_a_jump() {
        let panda =
            Panda::new(Character::Apprentice, Mood::Wave, ACCENT).moving_from(Duration::ZERO);
        let amount = |at: Duration| panda.motion(at).map_or(1.0, |(a, _)| a);
        assert!(amount(Duration::from_millis(1)) < 0.01);
        assert!(amount(MOVES_FOR - Duration::from_millis(1)) < 0.01);
        assert_eq!(amount(Duration::from_secs(2)), 1.0);
    }

    #[test]
    fn agents_get_the_two_characters_in_turn() {
        assert_eq!(Character::nth(0), Character::Apprentice);
        assert_eq!(Character::nth(1), Character::Watcher);
        assert_eq!(Character::nth(2), Character::Apprentice);
    }

    #[test]
    fn a_juggled_ball_follows_its_loop() {
        let still = JUGGLE[0];
        let pose = Kind::Juggle(&JUGGLE).pose(0.25, still);
        assert_eq!((still.0 + pose.dx, still.1 + pose.dy), (60.0, 9.0));
    }
}
