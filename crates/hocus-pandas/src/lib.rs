//! The pandas (docs/design/bar.md §7 A, 4.1): the agents' apprentice
//! magician family and the secretary (4.6), their validated drawings, one
//! still mood each (`assets/`, with a note of their origin), and the
//! motion a panda plays when something changes (#649).
//!
//! A panda moves only for a few seconds after a change (its agent's state,
//! the opening of the surface that shows it, the pointer coming over it),
//! then lies still until the next one: a *burst*. Its motion is the pandas
//! canvas's own, the parts of the drawing that already move there: working
//! types and its laptop's star twinkles; waiting sways its held-out hat,
//! its ear sends rings and its "?" bobs; done tosses its hat among its
//! confetti; stuck's smoke rises; resting breathes as two "z" rise; the
//! wink waves its hat. No new art: each moving part of a still drawing is
//! a group (`<g id="…">`) that a frame of the motion transforms.
//!
//! Each part plays whole cycles of its animation while they fit in the
//! burst's window (2.6 s, a resting panda one breath), a part whose cycle
//! does not fit cut at its end; then every part settles back on the still
//! drawing in 300 ms, and nothing more is drawn. [`frame`] gives the
//! drawing at a moment of a burst, none once it is over: the places that
//! draw pandas keep drawing frames only while it gives one.
//!
//! A theme may dress them ([`Dress`], #740): Givre's ice visor, a layer
//! over the still drawing and over each frame alike.

use std::time::Duration;

/// The drawings' view box, `-3 -3.5 126 120.5`: its size.
pub const VIEW_BOX: (f32, f32) = (126.0, 120.5);
/// The secretary's: `0 0 120 120`.
pub const SECRETARY_BOX: f32 = 120.0;

/// A burst's window: whole cycles within it, about 2.5 s of motion.
pub const BURST: Duration = Duration::from_millis(2600);
/// A resting panda's burst: one breath.
pub const BREATH: Duration = Duration::from_millis(4000);
/// Once its window is over, each part settles on the still drawing.
pub const SETTLE: Duration = Duration::from_millis(300);
/// Frames are drawn at most this often (60 a second): what a cache of
/// frames keys on ([`step`]).
pub const STEP: Duration = Duration::from_micros(16_667);

/// A panda's mood: an agent's state (4.1), or the wink that says windows
/// moved with a screen (docs/design/windows.md §12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Mood {
    /// Holding out its hat: it waits for the user.
    Waiting,
    /// Smoking ears: it failed.
    Stuck,
    /// Arms up: it finished.
    Done,
    /// At its laptop.
    Working,
    /// Asleep.
    Resting,
    /// Winking, its hat raised: never an agent's state.
    Wink,
}

impl Mood {
    pub const ALL: [Self; 6] = [
        Self::Waiting,
        Self::Stuck,
        Self::Done,
        Self::Working,
        Self::Resting,
        Self::Wink,
    ];

    /// Its still drawing.
    pub const fn still(self) -> &'static str {
        match self {
            Self::Waiting => include_str!("../assets/waiting.svg"),
            Self::Stuck => include_str!("../assets/stuck.svg"),
            Self::Done => include_str!("../assets/done.svg"),
            Self::Working => include_str!("../assets/working.svg"),
            Self::Resting => include_str!("../assets/resting.svg"),
            Self::Wink => include_str!("../assets/wink.svg"),
        }
    }

    /// The window of its burst: 2.6 s, or one breath for a resting panda.
    pub const fn burst(self) -> Duration {
        match self {
            Self::Resting => BREATH,
            _ => BURST,
        }
    }

    /// Its name, for logs and the control socket.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Stuck => "stuck",
            Self::Done => "done",
            Self::Working => "working",
            Self::Resting => "resting",
            Self::Wink => "wink",
        }
    }

    /// Its moving parts, as the pandas canvas animates them.
    fn parts(self) -> &'static [Part] {
        use Anim::*;
        use Ease::*;
        // The hat's star twinkles in every mood (the hat's own space).
        const SPARK: Part = part("spark", Twinkle, 2.6, InOut, 0.0, (14.0, -12.6));
        const RING1: Part = part("ring1", Ring, 1.9, Out, 0.0, (85.0, 33.0));
        const RING2: Part = part("ring2", Ring, 1.9, Out, 0.95, (85.0, 33.0));
        const WORKING: &[Part] = &[
            SPARK,
            part("lid", Twinkle, 1.8, InOut, 0.0, (60.0, 88.5)),
            part("paw-l", Type, 0.46, InOut, 0.0, (42.0, 100.0)),
            part("paw-r", Type, 0.46, InOut, 0.23, (78.0, 100.0)),
        ];
        const WAITING: &[Part] = &[
            SPARK,
            RING1,
            RING2,
            part("ask", Bob, 1.3, InOut, 0.0, (97.5, 9.8)),
            part("swing", Sway, 2.6, InOut, 0.0, (60.0, 100.0)),
        ];
        const DONE: &[Part] = &[
            SPARK,
            RING1,
            RING2,
            part("toss", Toss, 1.5, InOut, 0.0, (0.0, -11.8)),
            part("c1", Twinkle, 1.1, InOut, 0.0, (42.0, 10.0)),
            part("c2", Twinkle, 1.1, InOut, 0.55, (108.0, 55.5)),
            part("c3", Twinkle, 1.1, InOut, 0.0, (72.0, 6.0)),
            part("c4", Twinkle, 1.1, InOut, 0.55, (112.0, 20.0)),
            part("c5", Twinkle, 1.1, InOut, 0.0, (10.0, 62.0)),
        ];
        const STUCK: &[Part] = &[
            SPARK,
            part("p1", Smoke, 2.1, Out, 0.0, (104.0, 74.0)),
            part("p2", Smoke, 2.1, Out, 0.7, (109.0, 66.0)),
            part("p3", Smoke, 2.1, Out, 1.4, (104.0, 59.0)),
        ];
        const RESTING: &[Part] = &[
            SPARK,
            part("body", Breathe, 4.0, InOut, 0.0, (60.0, 115.0)),
            part("z1", Z, 2.8, Out, 0.0, (96.0, 24.0)),
            part("z2", Z, 2.8, Out, 1.4, (106.5, 8.5)),
        ];
        const WINK: &[Part] = &[
            SPARK,
            part("eye", Blink, 4.6, Plain, 0.0, (72.0, 51.5)),
            part("swing", Wave, 1.1, InOut, 0.0, (36.0, 76.0)),
        ];
        match self {
            Self::Working => WORKING,
            Self::Waiting => WAITING,
            Self::Done => DONE,
            Self::Stuck => STUCK,
            Self::Resting => RESTING,
            Self::Wink => WINK,
        }
    }
}

/// The secretary's mood (4.6): her ear, and the dot on her clipboard, in
/// the moment's colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Secretary {
    Calm,
    Waiting,
    Stuck,
}

impl Secretary {
    pub const fn still(self) -> &'static str {
        match self {
            Self::Calm => include_str!("../assets/secretary-calm.svg"),
            Self::Waiting => include_str!("../assets/secretary-waiting.svg"),
            Self::Stuck => include_str!("../assets/secretary-stuck.svg"),
        }
    }
}

/// How the pandas are dressed for the theme (#740): as drawn, or Givre's
/// ice visor. A dress is a layer over the drawing, laid on its still
/// drawing as on each frame of its motion: the moving parts move as they
/// do, under it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Dress {
    #[default]
    Plain,
    /// Givre's: a translucent ice visor over the eyes, with a highlight,
    /// pushed up on the forehead when the panda sleeps; the pandas' green
    /// in Givre's ice blue.
    Visor,
}

/// The pandas' green, and Givre's ice blue that takes its place.
const GREEN: &str = "#7BC47F";
const ICE: &str = "#7FD8FF";

impl Dress {
    /// `drawing` (a still drawing or a frame of `mood`) in this dress.
    pub fn wear(self, drawing: &str, mood: Mood) -> String {
        match self {
            Self::Plain => drawing.to_owned(),
            Self::Visor => {
                let drawing = drawing.replace(GREEN, ICE);
                let Some(end) = drawing.rfind("</svg>") else {
                    return drawing;
                };
                let mut dressed = String::with_capacity(drawing.len() + 512);
                dressed.push_str(&drawing[..end]);
                dressed.push_str(&visor(mood == Mood::Resting));
                dressed.push_str(&drawing[end..]);
                dressed
            }
        }
    }
}

/// The ice visor, across the eyes (`up`: on the forehead, asleep): a
/// translucent band with an icy rim, a highlight, the accent's tick.
fn visor(up: bool) -> String {
    let lift = if up {
        " transform=\"translate(0 -19)\""
    } else {
        ""
    };
    format!(
        "<g id=\"visor\"{lift}>\
         <path d=\"M35.5 45.5 Q60 37.5 84.5 45.5 L83 57.5 Q60 51.5 37 57.5 Z\" \
         fill=\"{ICE}\" fill-opacity=\"0.42\" stroke=\"#CFF3FF\" stroke-width=\"1.5\" \
         stroke-linejoin=\"round\" />\
         <path d=\"M41 47.5 Q52 43.6 62 44.2\" fill=\"none\" stroke=\"#FFFFFF\" \
         stroke-width=\"1.6\" stroke-linecap=\"round\" opacity=\"0.8\" />\
         <path d=\"M80 49 L83.5 49\" stroke=\"{ICE}\" stroke-width=\"2\" \
         stroke-linecap=\"round\" /></g>"
    )
}

/// How long a burst of `window` draws frames: its window, then the settle.
pub fn lasts(window: Duration) -> Duration {
    window + SETTLE
}

/// The frame of a burst `t` into it: `t` in [`STEP`]s, so that the same
/// moment gives the same frame (and a cache of frames hits).
pub fn step(t: Duration) -> u32 {
    (t.as_micros() / STEP.as_micros()).min(u128::from(u32::MAX)) as u32
}

/// `mood`'s drawing `t` into a burst moving for `window`: its moving parts
/// where they are then; none once the burst is over (the still drawing,
/// [`Mood::still`], from then on).
pub fn frame(mood: Mood, window: Duration, t: Duration) -> Option<String> {
    if t >= lasts(window) {
        return None;
    }
    let mut drawing = mood.still().to_owned();
    let (window, t) = (window.as_secs_f32(), t.as_secs_f32());
    for part in mood.parts() {
        let pose = part.at(window, t);
        if pose.is_rest() {
            continue;
        }
        let group = format!("<g id=\"{}\">", part.id);
        let moved = format!(
            "<g id=\"{}\" transform=\"{}\" opacity=\"{:.3}\">",
            part.id,
            pose.matrix(part.origin),
            pose.opacity.clamp(0.0, 1.0)
        );
        drawing = drawing.replacen(&group, &moved, 1);
    }
    Some(drawing)
}

// --- The parts and their animations ---------------------------------------------

/// A moving part: its group's id, its animation, the animation's period
/// (s) and timing, when it starts in a burst (s), and the point it turns
/// and scales round (the CSS `transform-origin`: its box's middle, in its
/// own space, or as the canvas sets it).
#[derive(Debug, Clone, Copy)]
struct Part {
    id: &'static str,
    anim: Anim,
    period: f32,
    ease: Ease,
    phase: f32,
    origin: (f32, f32),
}

const fn part(
    id: &'static str,
    anim: Anim,
    period: f32,
    ease: Ease,
    phase: f32,
    origin: (f32, f32),
) -> Part {
    Part {
        id,
        anim,
        period,
        ease,
        phase,
        origin,
    }
}

impl Part {
    /// Where it is `t` s into a burst whose window is `window` s: from the
    /// still drawing to its first keyframe until its phase, then whole
    /// cycles while they fit in the window (or until the window's end when
    /// not one does), then back to the still drawing in [`SETTLE`].
    fn at(&self, window: f32, t: f32) -> Pose {
        let cycles = ((window - self.phase) / self.period).floor();
        let end = if cycles >= 1.0 {
            self.phase + cycles * self.period
        } else {
            window.max(self.phase)
        };
        let settle = SETTLE.as_secs_f32();
        if t < self.phase {
            let share = self.ease.at(t / self.phase);
            return Pose::REST.towards(self.anim.at(0.0, self.ease), share);
        }
        if t < end {
            let share = ((t - self.phase) / self.period).fract();
            return self.anim.at(share, self.ease);
        }
        if t < end + settle {
            let share = if cycles >= 1.0 {
                1.0
            } else {
                ((end - self.phase) / self.period).fract()
            };
            let from = self.anim.at(share, self.ease).unwound();
            return from.towards(Pose::REST, self.ease.at((t - end) / settle));
        }
        Pose::REST
    }
}

/// An animation's transforms' keyframes and its opacities'.
type Keys = (&'static [(f32, Pose)], &'static [(f32, f32)]);

/// The pandas canvas's keyframes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Anim {
    Twinkle,
    Ring,
    Blink,
    Bob,
    Type,
    Sway,
    Wave,
    Toss,
    Smoke,
    Z,
    Breathe,
}

impl Anim {
    /// Its transform's keyframes and its opacity's, by share of a cycle
    /// (a property a keyframe leaves out goes between those that set it,
    /// as CSS does).
    fn keys(self) -> Keys {
        const fn scale(s: f32) -> Pose {
            Pose::scaled(s, s)
        }
        const TWINKLE: Keys = (
            &[(0.0, scale(1.0)), (0.5, scale(0.55)), (1.0, scale(1.0))],
            &[(0.0, 1.0), (0.5, 0.6), (1.0, 1.0)],
        );
        const RING: Keys = (
            &[(0.0, scale(0.75)), (1.0, scale(1.3))],
            &[(0.0, 0.95), (1.0, 0.0)],
        );
        const BLINK: Keys = (
            &[
                (0.0, Pose::REST),
                (0.88, Pose::REST),
                (0.92, Pose::scaled(1.0, 0.12)),
                (1.0, Pose::REST),
            ],
            &[],
        );
        const BOB: Keys = (
            &[
                (0.0, Pose::REST),
                (0.5, Pose::moved(0.0, -2.5)),
                (1.0, Pose::REST),
            ],
            &[],
        );
        const TYPE: Keys = (
            &[
                (0.0, Pose::REST),
                (0.5, Pose::moved(0.0, -2.2)),
                (1.0, Pose::REST),
            ],
            &[],
        );
        const SWAY: Keys = (
            &[
                (0.0, Pose::turned(-4.0)),
                (0.5, Pose::turned(4.0)),
                (1.0, Pose::turned(-4.0)),
            ],
            &[],
        );
        const WAVE: Keys = (
            &[
                (0.0, Pose::turned(-8.0)),
                (0.5, Pose::turned(9.0)),
                (1.0, Pose::turned(-8.0)),
            ],
            &[],
        );
        const TOSS: Keys = (
            &[
                (0.0, Pose::moved(0.0, 4.0)),
                (0.5, Pose::moved(0.0, -6.0).turning(180.0)),
                (1.0, Pose::moved(0.0, 4.0).turning(360.0)),
            ],
            &[],
        );
        const SMOKE: Keys = (
            &[
                (0.0, Pose::moved(0.0, 4.0).scaling(0.45)),
                (1.0, Pose::moved(6.0, -15.0).scaling(1.25)),
            ],
            &[(0.0, 0.0), (0.25, 1.0), (1.0, 0.0)],
        );
        const Z: Keys = (
            &[
                (0.0, Pose::moved(-5.0, 7.0).scaling(0.6)),
                (1.0, Pose::moved(5.0, -8.0).scaling(1.1)),
            ],
            &[(0.0, 0.0), (0.3, 1.0), (1.0, 0.0)],
        );
        const BREATHE: Keys = (
            &[
                (0.0, Pose::REST),
                (0.5, Pose::scaled(1.03, 0.98)),
                (1.0, Pose::REST),
            ],
            &[],
        );
        match self {
            Self::Twinkle => TWINKLE,
            Self::Ring => RING,
            Self::Blink => BLINK,
            Self::Bob => BOB,
            Self::Type => TYPE,
            Self::Sway => SWAY,
            Self::Wave => WAVE,
            Self::Toss => TOSS,
            Self::Smoke => SMOKE,
            Self::Z => Z,
            Self::Breathe => BREATHE,
        }
    }

    /// Its pose at `share` of a cycle, each keyframe to the next eased by
    /// `ease`.
    fn at(self, share: f32, ease: Ease) -> Pose {
        let (transforms, opacities) = self.keys();
        let mut pose =
            between(transforms, share, ease, |a, b, k| a.towards(*b, k)).unwrap_or(Pose::REST);
        pose.opacity = between(opacities, share, ease, |a, b, k| a + (b - a) * k).unwrap_or(1.0);
        pose
    }
}

/// The value at `share` between the keyframes `keys` round it.
fn between<T: Copy>(
    keys: &[(f32, T)],
    share: f32,
    ease: Ease,
    mix: impl Fn(&T, &T, f32) -> T,
) -> Option<T> {
    let first = keys.first()?;
    if share <= first.0 {
        return Some(first.1);
    }
    for pair in keys.windows(2) {
        let ((a, va), (b, vb)) = (pair[0], pair[1]);
        if share <= b {
            let k = if b > a { (share - a) / (b - a) } else { 1.0 };
            return Some(mix(&va, &vb, ease.at(k)));
        }
    }
    keys.last().map(|(_, v)| *v)
}

/// A part's transform (CSS's `translate rotate scale`, round its origin)
/// and its opacity.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Pose {
    dx: f32,
    dy: f32,
    /// Degrees.
    turn: f32,
    sx: f32,
    sy: f32,
    opacity: f32,
}

impl Pose {
    const REST: Self = Self {
        dx: 0.0,
        dy: 0.0,
        turn: 0.0,
        sx: 1.0,
        sy: 1.0,
        opacity: 1.0,
    };

    const fn scaled(sx: f32, sy: f32) -> Self {
        Self {
            sx,
            sy,
            ..Self::REST
        }
    }

    const fn moved(dx: f32, dy: f32) -> Self {
        Self {
            dx,
            dy,
            ..Self::REST
        }
    }

    const fn turned(turn: f32) -> Self {
        Self { turn, ..Self::REST }
    }

    const fn turning(self, turn: f32) -> Self {
        Self { turn, ..self }
    }

    const fn scaling(self, s: f32) -> Self {
        Self {
            sx: s,
            sy: s,
            ..self
        }
    }

    fn towards(self, to: Self, k: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * k;
        Self {
            dx: mix(self.dx, to.dx),
            dy: mix(self.dy, to.dy),
            turn: mix(self.turn, to.turn),
            sx: mix(self.sx, to.sx),
            sy: mix(self.sy, to.sy),
            opacity: mix(self.opacity, to.opacity),
        }
    }

    /// The same pose, its turn within half a turn of none: a hat tossed a
    /// whole turn settles without turning back.
    fn unwound(self) -> Self {
        Self {
            turn: (self.turn + 180.0).rem_euclid(360.0) - 180.0,
            ..self
        }
    }

    fn is_rest(&self) -> bool {
        let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
        near(self.dx, 0.0)
            && near(self.dy, 0.0)
            && near(self.turn, 0.0)
            && near(self.sx, 1.0)
            && near(self.sy, 1.0)
            && near(self.opacity, 1.0)
    }

    /// Its SVG matrix round `origin`.
    fn matrix(&self, (ox, oy): (f32, f32)) -> String {
        let (sin, cos) = self.turn.to_radians().sin_cos();
        let (a, b, c, d) = (cos * self.sx, sin * self.sx, -sin * self.sy, cos * self.sy);
        let e = ox + self.dx - (a * ox + c * oy);
        let f = oy + self.dy - (b * ox + d * oy);
        format!("matrix({a:.4} {b:.4} {c:.4} {d:.4} {e:.3} {f:.3})")
    }
}

/// The CSS timing functions the canvas uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ease {
    /// `ease`.
    Plain,
    /// `ease-in-out`.
    InOut,
    /// `ease-out`.
    Out,
}

impl Ease {
    fn at(self, x: f32) -> f32 {
        let (x1, y1, x2, y2) = match self {
            Self::Plain => (0.25, 0.1, 0.25, 1.0),
            Self::InOut => (0.42, 0.0, 0.58, 1.0),
            Self::Out => (0.0, 0.0, 0.58, 1.0),
        };
        bezier((x1, y1, x2, y2), x.clamp(0.0, 1.0))
    }
}

/// A CSS `cubic-bezier` at `x`: its `t` found by halving, then its `y`.
fn bezier((x1, y1, x2, y2): (f32, f32, f32, f32), x: f32) -> f32 {
    let curve = |a: f32, b: f32, t: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if curve(x1, x2, mid) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    curve(y1, y2, (lo + hi) / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    #[test]
    fn every_moving_part_is_a_group_of_its_drawing() {
        for mood in Mood::ALL {
            for part in mood.parts() {
                let group = format!("<g id=\"{}\">", part.id);
                assert_eq!(
                    mood.still().matches(&group).count(),
                    1,
                    "{} in {}",
                    part.id,
                    mood.name()
                );
            }
        }
    }

    #[test]
    fn a_burst_moves_then_draws_nothing_more() {
        for mood in Mood::ALL {
            let window = mood.burst();
            assert!(frame(mood, window, Duration::ZERO).is_some());
            assert!(frame(mood, window, lasts(window) - MS(1)).is_some());
            assert_eq!(frame(mood, window, lasts(window)), None);
            assert_eq!(frame(mood, window, lasts(window) + MS(5000)), None);
            // Mid-burst, it is not the still drawing.
            let mid = frame(mood, window, MS(700)).expect("a frame");
            assert_ne!(mid, mood.still(), "{}", mood.name());
        }
        // About 2.5 s, a resting panda one breath.
        assert_eq!(lasts(Mood::Waiting.burst()), MS(2900));
        assert_eq!(lasts(Mood::Resting.burst()), MS(4300));
    }

    #[test]
    fn its_parts_settle_on_the_still_drawing() {
        for mood in Mood::ALL {
            for window in [mood.burst(), MS(7700), MS(8000)] {
                let end = lasts(window).as_secs_f32();
                for part in mood.parts() {
                    assert!(
                        part.at(window.as_secs_f32(), end).is_rest(),
                        "{} of {}",
                        part.id,
                        mood.name()
                    );
                    // Just before, nearly there.
                    let near = part.at(window.as_secs_f32(), end - 0.001);
                    assert!((near.turn).abs() < 1.0 && (near.opacity - 1.0).abs() < 0.05);
                }
            }
        }
    }

    #[test]
    fn whole_cycles_fit_in_the_window() {
        let part = part("paw-l", Anim::Type, 0.46, Ease::InOut, 0.0, (0.0, 0.0));
        // Five strokes of 0.46 s in 2.6 s: back at rest at 2.3 s.
        assert!(part.at(2.6, 2.3 + 0.3).is_rest());
        assert!(!part.at(2.6, 2.3 - 0.23).is_rest());
        // A hat tossed a whole turn lands without turning back.
        let toss = Mood::Done
            .parts()
            .iter()
            .find(|p| p.id == "toss")
            .expect("toss");
        for ms in 1500..1800 {
            let pose = toss.at(2.6, ms as f32 / 1000.0);
            assert!(pose.turn.abs() < 0.5, "{ms}: {}", pose.turn);
        }
    }

    #[test]
    fn its_frames_are_drawings() {
        let options = resvg::usvg::Options::default();
        for mood in Mood::ALL {
            let window = mood.burst();
            let mut t = Duration::ZERO;
            while let Some(svg) = frame(mood, window, t) {
                let tree = resvg::usvg::Tree::from_str(&svg, &options);
                assert!(tree.is_ok(), "{} at {t:?}", mood.name());
                t += MS(50);
            }
        }
    }

    #[test]
    fn a_frame_draws_its_parts_moved() {
        let draw = |svg: &str| {
            let tree =
                resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).expect("svg");
            let mut pixmap = resvg::tiny_skia::Pixmap::new(126, 121).expect("pixmap");
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::identity(),
                &mut pixmap.as_mut(),
            );
            pixmap.take()
        };
        for mood in Mood::ALL {
            let mid = frame(mood, mood.burst(), MS(650)).expect("a frame");
            assert_ne!(draw(&mid), draw(mood.still()), "{}", mood.name());
        }
    }

    fn pixels(svg: &str) -> Vec<u8> {
        let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).expect("svg");
        let mut pixmap = resvg::tiny_skia::Pixmap::new(126, 121).expect("pixmap");
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_translate(3.0, 3.5),
            &mut pixmap.as_mut(),
        );
        pixmap.take()
    }

    /// The pixel at `(x, y)` of the drawing's units, as red, green, blue.
    fn at(pixels: &[u8], (x, y): (usize, usize)) -> [u8; 3] {
        let i = ((y + 3) * 126 + x + 3) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    }

    #[test]
    fn givre_dresses_every_mood_in_its_ice_visor() {
        for mood in Mood::ALL {
            let plain = mood.still();
            assert_eq!(Dress::Plain.wear(plain, mood), plain);
            let dressed = Dress::Visor.wear(plain, mood);
            assert_eq!(dressed.matches("<g id=\"visor\"").count(), 1);
            assert!(!dressed.contains(GREEN), "{}", mood.name());
            assert!(dressed.trim_end().ends_with("</g></svg>"));
            // Over the eyes, or on the forehead asleep.
            let shown = pixels(&dressed);
            let eye = at(&shown, (48, 51));
            let forehead = at(&shown, (48, 33));
            let icy = |[r, g, b]: [u8; 3]| b > r + 10 && g > r;
            if mood == Mood::Resting {
                assert!(icy(forehead), "{}: {forehead:?}", mood.name());
            } else {
                assert!(icy(eye), "{}: {eye:?}", mood.name());
                assert_ne!(at(&pixels(plain), (48, 51)), eye);
            }
            // Its moving parts move as they did, under the visor.
            for part in mood.parts() {
                let group = format!("<g id=\"{}\">", part.id);
                assert_eq!(dressed.matches(&group).count(), 1, "{}", part.id);
            }
            let mid = frame(mood, mood.burst(), MS(650)).expect("a frame");
            let worn = Dress::Visor.wear(&mid, mood);
            assert!(resvg::usvg::Tree::from_str(&worn, &resvg::usvg::Options::default()).is_ok());
            assert_ne!(pixels(&worn), shown, "{}", mood.name());
        }
    }

    /// The pandas in Givre's visor, every mood, as PNG files in the folder
    /// `HOCUS_SHOTS` names, for the maintainer.
    #[test]
    #[ignore = "writes screenshots: run with HOCUS_SHOTS set"]
    fn screenshots_of_the_visor() {
        let Some(dir) = std::env::var_os("HOCUS_SHOTS") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("the folder");
        let scale = 3.0;
        let (w, h) = ((VIEW_BOX.0 * scale) as u32, (VIEW_BOX.1 * scale) as u32);
        let mut sheet =
            resvg::tiny_skia::Pixmap::new(w * Mood::ALL.len() as u32, h * 2).expect("pixmap");
        sheet.fill(resvg::tiny_skia::Color::from_rgba8(8, 16, 31, 255));
        for (i, mood) in Mood::ALL.into_iter().enumerate() {
            for (row, dress) in [Dress::Plain, Dress::Visor].into_iter().enumerate() {
                let svg = dress.wear(mood.still(), mood);
                let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default())
                    .expect("svg");
                let at = resvg::tiny_skia::Transform::from_row(
                    scale,
                    0.0,
                    0.0,
                    scale,
                    (i as u32 * w) as f32 + 3.0 * scale,
                    (row as u32 * h) as f32 + 3.5 * scale,
                );
                resvg::render(&tree, at, &mut sheet.as_mut());
            }
        }
        sheet
            .save_png(dir.join("pandas-visor.png"))
            .expect("the file");
    }

    #[test]
    fn steps_are_sixty_a_second() {
        assert_eq!(step(Duration::ZERO), 0);
        assert_eq!(step(MS(1000)), 59);
        assert_eq!(step(MS(1001)), 60);
    }
}
