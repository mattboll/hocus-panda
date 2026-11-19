//! An MCP agent's ink (#825, docs/design/mcp.md, "Drawing on the screen"):
//! the shapes `draw` draws, in the approved mockup's variant B (tools'
//! `mockups/s14-mcp/agent-ink`): a marker of the agents' violet, drawn by
//! hand over a short time with a slight wobble, the agent's name on the
//! first shape and its initial at the start of the next ones, and a short
//! word written beside a shape.
//!
//! The strokes are painted by [`super::Canvas`], as the user's are, with
//! the agent's [`Look`](super::Look); the name, the initial and the word
//! are small pictures drawn once per scale (fontdue's glyphs, a Gaussian
//! blur for their glow and shadow). The shapes' points are the mockup's
//! functions, ported: its seeded wobbles, Chaikin's smoothing, the hand's
//! minimum-jerk easing. An agent's shapes stay until its next action or
//! `Esc`, then fade as one in [`FADE`].

use std::{cell::RefCell, collections::HashMap, f64::consts::TAU, time::Duration};

use hocus_agents::mcp::Shape;
use smithay::backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer};
use smithay::utils::Transform;

/// A point of a screen, in its logical pixels.
pub type P = (f64, f64);

/// The agent's ink fades in this time once it goes.
pub const FADE: Duration = Duration::from_millis(600);
/// The arrow's head is drawn in this time, after its shaft.
const HEAD: Duration = Duration::from_millis(200);
/// The word starts this long after its shape, and is written in this time.
const WORD_AFTER: Duration = Duration::from_millis(50);
const WORD: Duration = Duration::from_millis(450);
/// The name's and the initial's fade in.
const MARK_IN: f64 = 0.2;
/// The points of a line are this far apart: the stroke grows by them.
const STEP: f64 = 3.0;

/// The violet of the agents (`#B69CFF`), and the dark ink on it.
const VIOLET: [f64; 3] = [182.0, 156.0, 255.0];
const ON_VIOLET: [f64; 3] = [0x1B as f64, 0x12 as f64, 0x30 as f64];

const MANROPE_BOLD: &[u8] = include_bytes!("../../../hocus-shell/fonts/Manrope-Bold.ttf");
/// The hand of the word written beside a shape (SIL Open Font License).
const CAVEAT_BOLD: &[u8] = include_bytes!("../../../hocus-shell/fonts/Caveat-Bold.ttf");

/// A shape to draw, on a screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub shape: Shape,
    /// The screen's logical pixels: a box's two corners, an arrow's tail
    /// and tip, a path's points.
    pub points: Vec<P>,
    /// Points already drawn by hand (an underline's squiggle): not smoothed
    /// again.
    pub hand_made: bool,
    pub word: Option<String>,
    /// Its screen's logical size, for where the word goes.
    pub room: (f64, f64),
    pub seed: u32,
}

// --- The hand ------------------------------------------------------------------

/// The mockup's random numbers: a linear congruence, the same for a seed.
fn rng(seed: u32) -> impl FnMut() -> f64 {
    let mut s = seed;
    move || {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        f64::from(s) / 4_294_967_296.0
    }
}

/// A slow drift: waves of amplitude and frequency, each its own phase.
fn wobbler(seed: u32, waves: &[(f64, f64)]) -> impl Fn(f64) -> f64 + use<> {
    let mut r = rng(seed);
    let ks: Vec<(f64, f64, f64)> = waves.iter().map(|&(a, f)| (a, f, r() * TAU)).collect();
    move |u| ks.iter().map(|(a, f, p)| a * (f * u + p).sin()).sum()
}

fn lengths(pts: &[P]) -> Vec<f64> {
    let mut cum = Vec::with_capacity(pts.len());
    let mut total = 0.0;
    let mut last: Option<P> = None;
    for p in pts {
        if let Some(q) = last {
            total += (p.0 - q.0).hypot(p.1 - q.1);
        }
        cum.push(total);
        last = Some(*p);
    }
    cum
}

/// The point `d` along the line.
fn along(pts: &[P], cum: &[f64], d: f64) -> P {
    let i = cum
        .partition_point(|c| *c < d)
        .clamp(1, pts.len().saturating_sub(1).max(1));
    let (Some(a), Some(b), Some(ca), Some(cb)) =
        (pts.get(i - 1), pts.get(i), cum.get(i - 1), cum.get(i))
    else {
        return pts.first().copied().unwrap_or_default();
    };
    let span = cb - ca;
    let u = if span > 0.0 { (d - ca) / span } else { 0.0 };
    (a.0 + (b.0 - a.0) * u, a.1 + (b.1 - a.1) * u)
}

/// The line, its points `step` apart.
pub fn resample(pts: &[P], step: f64) -> Vec<P> {
    if pts.len() < 2 {
        return pts.to_vec();
    }
    let cum = lengths(pts);
    let total = cum.last().copied().unwrap_or_default();
    let mut out: Vec<P> = pts.first().copied().into_iter().collect();
    let mut d = step;
    while d < total {
        out.push(along(pts, &cum, d));
        d += step;
    }
    out.extend(pts.last().copied());
    out
}

/// Chaikin's corner cutting, `n` times.
fn chaikin(mut pts: Vec<P>, n: usize) -> Vec<P> {
    for _ in 0..n {
        if pts.len() < 3 {
            return pts;
        }
        let mut out: Vec<P> = pts.first().copied().into_iter().collect();
        for (a, b) in pts.iter().zip(pts.iter().skip(1)) {
            out.push((0.75 * a.0 + 0.25 * b.0, 0.75 * a.1 + 0.25 * b.1));
            out.push((0.25 * a.0 + 0.75 * b.0, 0.25 * a.1 + 0.75 * b.1));
        }
        out.extend(pts.last().copied());
        pts = out;
    }
    pts
}

/// A hand's slow drift, across the line.
fn jitter(pts: &[P], seed: u32, amp: f64) -> Vec<P> {
    let cum = lengths(pts);
    let w = wobbler(seed, &[(amp, 0.045), (amp * 0.5, 0.11), (amp * 0.25, 0.23)]);
    let last = pts.len().saturating_sub(1);
    pts.iter()
        .zip(&cum)
        .enumerate()
        .map(|(i, (p, along))| {
            let a = pts.get(i.saturating_sub(1)).copied().unwrap_or(*p);
            let b = pts.get((i + 1).min(last)).copied().unwrap_or(*p);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l = dx.hypot(dy);
            let l = if l > 0.0 { l } else { 1.0 };
            let o = w(*along);
            (p.0 - dy / l * o, p.1 + dx / l * o)
        })
        .collect()
}

/// An ellipse round `c`, begun a little past its start and closed with an
/// overlap, slightly tilted, its radius breathing.
fn ellipse(c: P, rx: f64, ry: f64, seed: u32) -> Vec<P> {
    let w = wobbler(seed, &[(0.03, 2.0), (0.016, 3.3), (0.008, 5.1)]);
    let (th0, sweep, n, tilt) = (-0.5, TAU + 0.45, 180, -0.035_f64);
    (0..=n)
        .map(|i| {
            let u = f64::from(i) / f64::from(n);
            let th = th0 + sweep * u;
            let k = 1.0 + w(th) + 0.06 * u - 0.025;
            let (x, y) = (rx * k * th.cos(), ry * k * th.sin());
            (
                c.0 + x * tilt.cos() - y * tilt.sin(),
                c.1 + x * tilt.sin() + y * tilt.cos(),
            )
        })
        .collect()
}

/// A box drawn by hand: its corners missed by a pixel or two, its end
/// running on past its start.
fn rectangle(x: f64, y: f64, w: f64, h: f64, seed: u32) -> Vec<P> {
    let (lead, over) = ((w / 6.0).min(10.0), (w / 3.0).min(30.0));
    let p = vec![
        (x + lead, y - 2.0),
        (x + w + 2.0, y + 1.0),
        (x + w - 1.0, y + h + 2.0),
        (x - 2.0, y + h - 1.0),
        (x + 1.0, y - 3.0),
        (x + over, y - 6.0),
    ];
    let p = chaikin(resample(&p, 10.0), 2);
    jitter(&resample(&p, 2.5), seed, 1.3)
}

/// An arrow from `a` to `b`, its shaft bent a little, and its head: two
/// strokes of 22 px at 31°, 40 % of a shaft shorter than 55 px.
fn arrow(a: P, b: P, seed: u32) -> (Vec<P>, Vec<P>) {
    const BEND: f64 = 0.2;
    let (mx, my) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let c = (mx - dy * BEND, my + dx * BEND);
    let pts: Vec<P> = (0..=60)
        .map(|i| {
            let u = f64::from(i) / 60.0;
            let v = 1.0 - u;
            (
                v * v * a.0 + 2.0 * v * u * c.0 + u * u * b.0,
                v * v * a.1 + 2.0 * v * u * c.1 + u * u * b.1,
            )
        })
        .collect();
    let shaft = jitter(&pts, seed, 0.8);
    let length = lengths(&shaft).last().copied().unwrap_or_default();
    let e = shaft.last().copied().unwrap_or(b);
    let q = shaft
        .get(shaft.len().saturating_sub(5))
        .copied()
        .unwrap_or(a);
    let (ux, uy) = (e.0 - q.0, e.1 - q.1);
    let ul = ux.hypot(uy).max(1e-6);
    let len = if length < 55.0 { length * 0.4 } else { 22.0 };
    let angle = 31.0_f64.to_radians();
    let back = |s: f64| {
        let (cs, sn) = ((s * angle).cos(), (s * angle).sin());
        let (rx, ry) = (ux / ul * cs - uy / ul * sn, ux / ul * sn + uy / ul * cs);
        (e.0 - rx * len, e.1 - ry * len)
    };
    (shaft, vec![back(1.0), e, back(-1.0)])
}

/// A wavy underline from `x0` to `x1` at `y`, ending in a little tick up.
pub fn squiggle(x0: f64, x1: f64, y: f64, seed: u32) -> Vec<P> {
    let w = wobbler(seed, &[(0.6, 0.05)]);
    let span = (x1 - x0).max(1.0);
    let mut pts = Vec::new();
    let mut x = x0;
    while x <= x1 {
        let u = (x - x0) / span;
        pts.push((
            x,
            y + 2.8 * ((x - x0) / 5.2).sin() * (0.6 + 0.4 * (u * 3.0).sin()) - u * 3.0 + w(x),
        ));
        x += 2.0;
    }
    if let Some(&e) = pts.last() {
        pts.push((e.0 + 6.0, e.1 - 4.0));
        pts.push((e.0 + 10.0, e.1 - 10.0));
    }
    chaikin(pts, 1)
}

fn bounds(lines: &[Vec<P>]) -> (f64, f64, f64, f64) {
    let mut b = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for p in lines.iter().flatten() {
        b = (b.0.min(p.0), b.1.min(p.1), b.2.max(p.0), b.3.max(p.1));
    }
    b
}

/// The minimum-jerk curve: how a hand moves.
pub fn hand(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * x * (x * (6.0 * x - 15.0) + 10.0)
}

fn ease_out(x: f64) -> f64 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

/// The lines `plan` draws, in order: a shape's, then an arrow's head.
pub fn lines(plan: &Plan) -> Vec<Vec<P>> {
    let corners = || {
        let (a, b) = (
            plan.points.first().copied().unwrap_or_default(),
            plan.points.get(1).copied().unwrap_or_default(),
        );
        (
            a.0.min(b.0),
            a.1.min(b.1),
            (a.0 - b.0).abs(),
            (a.1 - b.1).abs(),
        )
    };
    let drawn = match plan.shape {
        Shape::Rectangle => {
            let (x, y, w, h) = corners();
            vec![rectangle(x, y, w, h, plan.seed)]
        }
        Shape::Ellipse => {
            let (x, y, w, h) = corners();
            vec![ellipse(
                (x + w / 2.0, y + h / 2.0),
                w / 2.0,
                h / 2.0,
                plan.seed,
            )]
        }
        Shape::Arrow => {
            let a = plan.points.first().copied().unwrap_or_default();
            let b = plan.points.last().copied().unwrap_or_default();
            let (shaft, head) = arrow(a, b, plan.seed);
            vec![shaft, head]
        }
        Shape::Path if plan.hand_made => vec![jitter(&plan.points, plan.seed, 0.4)],
        Shape::Path => {
            let smooth = chaikin(resample(&plan.points, 10.0), 2);
            vec![jitter(&resample(&smooth, 2.5), plan.seed, 0.8)]
        }
    };
    drawn
        .into_iter()
        .map(|l| resample(&l, STEP))
        .filter(|l| l.len() > 1)
        .collect()
}

/// How long a line of `length` takes the hand.
fn duration_of(length: f64) -> Duration {
    Duration::from_secs_f64((0.35 + length / 1000.0).clamp(0.4, 1.3))
}

// --- The marks ------------------------------------------------------------------

/// One shape an agent drew: its lines, when each starts and how long it
/// takes, from the drawing's start.
#[derive(Debug)]
pub struct AgentMark {
    /// The ink's marks' ids, one per line: [`super::Mark`]s for the canvas.
    pub ids: Vec<u64>,
    pub output: String,
    pub lines: Vec<Vec<P>>,
    timing: Vec<(Duration, Duration)>,
    pub start: Duration,
    /// Drawn whole, whatever the clock says.
    pub done: bool,
    /// Its agent's name on it (its first shape), else its initial.
    pub named: bool,
    pub word: Option<Word>,
}

/// A word written beside a shape, its baseline's left at `at`.
#[derive(Debug, Clone)]
pub struct Word {
    pub text: String,
    pub at: P,
    pub width: f64,
    since: Duration,
}

impl AgentMark {
    /// How far each line is drawn at `now`, 0 to 1.
    fn progress(&self, now: Duration) -> Vec<f64> {
        self.timing
            .iter()
            .map(|&(off, dur)| {
                if self.done {
                    return 1.0;
                }
                let t = now.saturating_sub(self.start).saturating_sub(off);
                hand(t.as_secs_f64() / dur.as_secs_f64().max(1e-3))
            })
            .collect()
    }

    /// How long it takes, its word with it.
    pub fn total(&self) -> Duration {
        let lines = self
            .timing
            .iter()
            .map(|(o, d)| *o + *d)
            .max()
            .unwrap_or_default();
        match &self.word {
            Some(word) => lines.max(word.since + WORD),
            None => lines,
        }
    }

    /// The lines' points drawn at `now`.
    pub fn shown(&self, now: Duration) -> Vec<(u64, Vec<P>)> {
        self.progress(now)
            .into_iter()
            .zip(self.lines.iter().zip(&self.ids))
            .filter_map(|(u, (line, id))| {
                let n = ((line.len() as f64) * u).ceil() as usize;
                let drawn = line.get(..n.min(line.len()))?;
                (n > 1).then(|| (*id, drawn.to_vec()))
            })
            .collect()
    }

    /// Where the hand is at `now`: the tip of the line being drawn, then
    /// along the word.
    pub fn tip(&self, now: Duration) -> Option<P> {
        let t = now.saturating_sub(self.start);
        if let Some(word) = &self.word
            && (t >= word.since || self.done)
        {
            let u = if self.done {
                1.0
            } else {
                ((t - word.since).as_secs_f64() / WORD.as_secs_f64()).min(1.0)
            };
            return Some((
                word.at.0 + word.width * hand(u),
                word.at.1 - 8.0 + 6.0 * (u * 14.0).sin(),
            ));
        }
        let shown = self.shown(now);
        shown
            .last()
            .and_then(|(_, l)| l.last().copied())
            .or_else(|| self.lines.first().and_then(|l| l.first().copied()))
    }

    /// The name's or the initial's opacity at `now`.
    fn mark_alpha(&self, now: Duration) -> f64 {
        if self.done {
            return 1.0;
        }
        ease_out(now.saturating_sub(self.start).as_secs_f64() / MARK_IN)
    }

    /// How much of the word shows at `now`, 0 to 1.
    fn word_shown(&self, now: Duration) -> f64 {
        let Some(word) = &self.word else {
            return 0.0;
        };
        if self.done {
            return 1.0;
        }
        let t = now.saturating_sub(self.start);
        if t < word.since {
            return 0.0;
        }
        hand((t - word.since).as_secs_f64() / WORD.as_secs_f64())
    }
}

/// An agent's shapes since its ink last went, fading as one.
#[derive(Debug)]
pub struct Group {
    pub id: u64,
    pub agent: String,
    pub marks: Vec<AgentMark>,
    /// Since when it fades.
    pub fading: Option<Duration>,
}

impl Group {
    /// How opaque at `now`: none once faded.
    pub fn alpha(&self, now: Duration, reduced: bool) -> Option<f64> {
        match self.fading {
            None => Some(1.0),
            Some(_) if reduced => None,
            Some(since) => {
                let t = now.saturating_sub(since).as_secs_f64() / FADE.as_secs_f64();
                // The mockup's ease-in, quadratic.
                (t < 1.0).then_some(1.0 - t * t)
            }
        }
    }
}

/// The agents' ink.
#[derive(Debug, Default)]
pub struct AgentInk {
    pub groups: Vec<Group>,
    next: u64,
    /// What each group's strokes are drawn into, by group and screen.
    pub(super) canvases: RefCell<HashMap<(u64, String), super::Canvas>>,
    /// The names, initials and words, drawn per scale.
    pictures: RefCell<HashMap<(Picture, u32), Sprite>>,
    fonts: RefCell<Option<Option<(fontdue::Font, fontdue::Font)>>>,
}

/// A picture of the agent's ink.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Picture {
    /// The name's pill.
    Name(String),
    /// The initial's disc.
    Initial(String),
    /// A word in the agent's hand.
    Word(String),
}

/// A picture drawn at a scale: its pixels and its logical size, and where
/// its anchor is in it (logical, from its top left).
#[derive(Debug)]
pub struct Sprite {
    pub buffer: MemoryRenderBuffer,
    pub px: (i32, i32),
    pub size: (f64, f64),
    pub anchor: P,
    /// The word's ink starts this far in (logical): its glow's room.
    pub pad: f64,
}

/// A group's shapes on a screen at a moment: its id, its opacity, its
/// lines' points by their ids, and its pictures.
pub type Shown = (u64, f64, Vec<(u64, Vec<P>)>, Vec<Placed>);

/// A picture placed on a screen: its top left (logical), its opacity, and
/// the part of it shown (logical width; all of it when none).
#[derive(Debug, Clone)]
pub struct Placed {
    pub picture: Picture,
    pub key: (u64, u8),
    pub at: P,
    pub alpha: f64,
    pub width: Option<f64>,
}

impl AgentInk {
    /// Whether an agent's ink shows and does not fade: `Esc` takes it.
    pub fn shows(&self) -> bool {
        self.groups.iter().any(|g| g.fading.is_none())
    }

    /// The agents whose ink shows and does not fade.
    pub fn inked(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .groups
            .iter()
            .filter(|g| g.fading.is_none())
            .map(|g| g.agent.clone())
            .collect();
        names.dedup();
        names
    }

    /// `agent`'s ink fades from `now` (its next action, `Esc`, the toast's
    /// « Effacer »); every agent's when none. Whether some did.
    pub fn fade(&mut self, agent: Option<&str>, now: Duration) -> bool {
        let mut any = false;
        for group in &mut self.groups {
            if group.fading.is_none() && agent.is_none_or(|a| a == group.agent) {
                group.fading = Some(now);
                any = true;
            }
        }
        any
    }

    /// Everything goes at once (the lock).
    pub fn clear(&mut self) {
        self.groups.clear();
        self.canvases.borrow_mut().clear();
    }

    /// The groups faded by `now` go. Whether some still draw or fade.
    pub fn tick(&mut self, now: Duration, reduced: bool) -> bool {
        let gone: Vec<u64> = self
            .groups
            .iter()
            .filter(|g| g.alpha(now, reduced).is_none())
            .map(|g| g.id)
            .collect();
        if !gone.is_empty() {
            self.groups.retain(|g| !gone.contains(&g.id));
            self.canvases
                .borrow_mut()
                .retain(|(g, _), _| !gone.contains(g));
        }
        self.groups.iter().any(|g| {
            g.fading.is_some() || g.marks.iter().any(|m| !m.done && now < m.start + m.total())
        })
    }

    /// Screen `output` went: its shapes with it.
    pub fn output_gone(&mut self, output: &str) {
        for group in &mut self.groups {
            group.marks.retain(|m| m.output != output);
        }
        self.groups.retain(|g| !g.marks.is_empty());
        self.canvases.borrow_mut().retain(|(_, o), _| o != output);
    }

    /// `agent` draws `plan` on `output` from `now`: its mark's key (its
    /// group and its index there) and how long it takes. `next_id` gives
    /// the ink's ids.
    pub fn draw(
        &mut self,
        agent: &str,
        output: String,
        plan: &Plan,
        now: Duration,
        next_id: &mut u64,
    ) -> Option<((u64, usize), Duration)> {
        let lines = lines(plan);
        if lines.is_empty() {
            return None;
        }
        let mut timing = Vec::new();
        let mut at = Duration::ZERO;
        for (i, line) in lines.iter().enumerate() {
            let length = lengths(line).last().copied().unwrap_or_default();
            let dur = if plan.shape == Shape::Arrow && i > 0 {
                HEAD
            } else {
                duration_of(length)
            };
            timing.push((at, dur));
            at += dur;
        }
        let word = plan.word.as_ref().and_then(|text| {
            let width = self.word_width(text)?;
            Some(Word {
                text: text.clone(),
                at: word_place(plan, &lines, width),
                width,
                since: at + WORD_AFTER,
            })
        });
        let ids: Vec<u64> = lines
            .iter()
            .map(|_| {
                *next_id += 1;
                *next_id
            })
            .collect();
        let group = match self
            .groups
            .iter_mut()
            .find(|g| g.agent == agent && g.fading.is_none())
        {
            Some(group) => group,
            None => {
                self.next += 1;
                self.groups.push(Group {
                    id: self.next,
                    agent: agent.to_owned(),
                    marks: Vec::new(),
                    fading: None,
                });
                self.groups.last_mut()?
            }
        };
        let named = group.marks.is_empty();
        group.marks.push(AgentMark {
            ids,
            output,
            lines,
            timing,
            start: now,
            done: false,
            named,
            word,
        });
        let mark = group.marks.last()?;
        Some(((group.id, group.marks.len() - 1), mark.total()))
    }

    pub fn mark(&self, (group, index): (u64, usize)) -> Option<&AgentMark> {
        self.groups.iter().find(|g| g.id == group)?.marks.get(index)
    }

    /// Mark `key` is drawn whole: its time is over. Whether it was there.
    pub fn finish(&mut self, (group, index): (u64, usize)) -> bool {
        self.groups
            .iter_mut()
            .find(|g| g.id == group)
            .and_then(|g| g.marks.get_mut(index))
            .map(|m| m.done = true)
            .is_some()
    }

    fn fonts(&self) -> Option<std::cell::Ref<'_, (fontdue::Font, fontdue::Font)>> {
        if self.fonts.borrow().is_none() {
            let read = |bytes: &'static [u8]| {
                fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                    .map_err(|err| tracing::warn!(err, "the agent's ink's font does not read"))
                    .ok()
            };
            let both = read(MANROPE_BOLD).zip(read(CAVEAT_BOLD));
            *self.fonts.borrow_mut() = Some(both);
        }
        std::cell::Ref::filter_map(self.fonts.borrow(), |f| f.as_ref().and_then(Option::as_ref))
            .ok()
    }

    /// A word's width in the agent's hand, logical.
    fn word_width(&self, text: &str) -> Option<f64> {
        let fonts = self.fonts()?;
        Some(f64::from(crate::radial_draw::advance(
            &fonts.1, text, WORD_PX,
        )))
    }

    /// What the agents' shapes on `output` show at `now`: the lines' points
    /// of each group, its opacity, and the pictures over them.
    pub fn shown_on(&self, output: &str, now: Duration, reduced: bool) -> Vec<Shown> {
        self.groups
            .iter()
            .filter_map(|group| {
                let alpha = group.alpha(now, reduced)?;
                let mut lines = Vec::new();
                let mut placed = Vec::new();
                for (i, mark) in group.marks.iter().enumerate() {
                    if mark.output != output || now < mark.start {
                        continue;
                    }
                    lines.extend(mark.shown(now));
                    let key = |kind: u8| (group.id * 1024 + i as u64, kind);
                    let ta = mark.mark_alpha(now) * alpha;
                    let first = mark.lines.first().map(Vec::as_slice).unwrap_or_default();
                    if mark.named {
                        let (x0, y0, ..) = bounds(&mark.lines);
                        placed.push(Placed {
                            picture: Picture::Name(group.agent.clone()),
                            key: key(0),
                            at: (x0 - 2.0, (y0 - 22.0).max(2.0)),
                            alpha: ta,
                            width: None,
                        });
                    } else if let (Some(s0), Some(s1)) =
                        (first.first(), first.get(6.min(first.len() - 1)))
                    {
                        let (dx, dy) = (s1.0 - s0.0, s1.1 - s0.1);
                        let l = dx.hypot(dy).max(1e-6);
                        placed.push(Placed {
                            picture: Picture::Initial(initial(&group.agent)),
                            key: key(1),
                            at: (s0.0 - dx / l * 16.0, s0.1 - dy / l * 16.0),
                            alpha: ta,
                            width: None,
                        });
                    }
                    if let Some(word) = &mark.word {
                        let shown = mark.word_shown(now);
                        if shown > 0.0 {
                            placed.push(Placed {
                                picture: Picture::Word(word.text.clone()),
                                key: key(2),
                                at: word.at,
                                alpha,
                                width: (shown < 1.0).then_some((word.width + 12.0) * shown - 6.0),
                            });
                        }
                    }
                }
                (!lines.is_empty() || !placed.is_empty())
                    .then_some((group.id, alpha, lines, placed))
            })
            .collect()
    }

    /// Picture `picture` at `scale`, drawn the first time it is asked.
    pub fn picture(&self, picture: &Picture, scale: f64) -> Option<std::cell::Ref<'_, Sprite>> {
        let key = (picture.clone(), (scale * 1000.0).round() as u32);
        if !self.pictures.borrow().contains_key(&key) {
            let sprite = {
                let fonts = self.fonts()?;
                match picture {
                    Picture::Name(name) => name_pill(&fonts.0, name, scale),
                    Picture::Initial(letter) => initial_disc(&fonts.0, letter, scale),
                    Picture::Word(word) => hand_word(&fonts.1, word, scale),
                }
            };
            self.pictures.borrow_mut().insert(key.clone(), sprite);
        }
        std::cell::Ref::filter_map(self.pictures.borrow(), |p| p.get(&key)).ok()
    }
}

/// An agent's initial, as its mark says it.
fn initial(agent: &str) -> String {
    agent
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default()
}

/// Where a word goes beside its shape, its baseline's left: past an
/// arrow's tip, below it; else right of the shape, or left of it when the
/// screen ends first.
fn word_place(plan: &Plan, lines: &[Vec<P>], width: f64) -> P {
    let (room_w, room_h) = plan.room;
    let (x0, y0, x1, y1) = bounds(lines);
    let at = match plan.shape {
        Shape::Arrow => {
            let tip = lines
                .first()
                .and_then(|l| l.last().copied())
                .unwrap_or((x1, y1));
            (tip.0 + 16.0, tip.1 + 27.0)
        }
        _ => {
            let y = (y0 + y1) / 2.0 + 13.0;
            if x1 + 14.0 + width <= room_w - 8.0 {
                (x1 + 14.0, y)
            } else {
                (x0 - 14.0 - width, y)
            }
        }
    };
    (
        at.0.clamp(8.0, (room_w - width - 8.0).max(8.0)),
        at.1.clamp(40.0, (room_h - 12.0).max(40.0)),
    )
}

// --- The pictures ---------------------------------------------------------------

/// The word's size, logical (the mockup's `700 40px Caveat`).
const WORD_PX: f32 = 40.0;

/// An alpha mask, pixels.
struct Mask {
    w: usize,
    h: usize,
    a: Vec<f32>,
}

impl Mask {
    fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            a: vec![0.0; w * h],
        }
    }

    fn get(&self, x: i64, y: i64) -> f32 {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return 0.0;
        }
        self.a
            .get(y as usize * self.w + x as usize)
            .copied()
            .unwrap_or_default()
    }

    fn set(&mut self, x: usize, y: usize, value: f32) {
        if x < self.w
            && let Some(slot) = self.a.get_mut(y * self.w + x)
        {
            *slot = value;
        }
    }

    /// The same mask moved by `dy` pixels down.
    fn shifted(&self, dy: f64) -> Self {
        let mut out = Self::new(self.w, self.h);
        let (whole, part) = (dy.floor() as i64, dy.fract() as f32);
        for y in 0..self.h {
            for x in 0..self.w {
                let (xi, yi) = (x as i64, y as i64);
                let v =
                    self.get(xi, yi - whole) * (1.0 - part) + self.get(xi, yi - whole - 1) * part;
                out.set(x, y, v);
            }
        }
        out
    }

    /// Blurred by a Gaussian of deviation `sigma` pixels.
    fn blurred(&self, sigma: f64) -> Self {
        if sigma <= 0.0 {
            return Self {
                w: self.w,
                h: self.h,
                a: self.a.clone(),
            };
        }
        let r = (sigma * 3.0).ceil() as i64;
        let kernel: Vec<f32> = (-r..=r)
            .map(|i| (-(i * i) as f64 / (2.0 * sigma * sigma)).exp() as f32)
            .collect();
        let sum: f32 = kernel.iter().sum();
        let kernel: Vec<f32> = kernel.iter().map(|k| k / sum).collect();
        let mut across = Self::new(self.w, self.h);
        for y in 0..self.h {
            for x in 0..self.w {
                let mut v = 0.0;
                for (k, i) in kernel.iter().zip(-r..=r) {
                    v += k * self.get(x as i64 + i, y as i64);
                }
                across.set(x, y, v);
            }
        }
        let mut out = Self::new(self.w, self.h);
        for y in 0..self.h {
            for x in 0..self.w {
                let mut v = 0.0;
                for (k, i) in kernel.iter().zip(-r..=r) {
                    v += k * across.get(x as i64, y as i64 + i);
                }
                out.set(x, y, v);
            }
        }
        out
    }

    /// `text` written in `font` at `px` pixels, its baseline's left at
    /// `(x, y)` pixels.
    fn write(&mut self, font: &fontdue::Font, text: &str, px: f32, (x, y): (f32, f32)) {
        let mut pen = x;
        let mut previous = None;
        for c in text.chars() {
            if let Some(p) = previous {
                pen += font.horizontal_kern(p, c, px).unwrap_or(0.0);
            }
            let (metrics, coverage) = font.rasterize(c, px);
            let gx = (pen + metrics.xmin as f32).round() as i64;
            let gy = (y - (metrics.height as f32 + metrics.ymin as f32)).round() as i64;
            for (row, line) in coverage.chunks(metrics.width.max(1)).enumerate() {
                for (col, &a) in line.iter().enumerate() {
                    let (px_, py) = (gx + col as i64, gy + row as i64);
                    let v = self.get(px_, py).max(f32::from(a) / 255.0);
                    if px_ >= 0 && py >= 0 {
                        self.set(px_ as usize, py as usize, v);
                    }
                }
            }
            pen += metrics.advance_width;
            previous = Some(c);
        }
    }

    /// A rounded box, anti-aliased: `(x, y, w, h)` and its radius, pixels.
    fn rounded(&mut self, (x, y, w, h): (f64, f64, f64, f64), radius: f64) {
        for py in 0..self.h {
            for px in 0..self.w {
                let (cx, cy) = (px as f64 + 0.5, py as f64 + 0.5);
                let qx = (cx - (x + w / 2.0)).abs() - (w / 2.0 - radius);
                let qy = (cy - (y + h / 2.0)).abs() - (h / 2.0 - radius);
                let outside = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
                self.set(px, py, (0.5 - outside).clamp(0.0, 1.0) as f32);
            }
        }
    }
}

/// Layers, from the back, made one picture: premultiplied ARGB.
fn compose(w: usize, h: usize, layers: &[(&Mask, [f64; 3], f64)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(w * h * 4);
    for i in 0..w * h {
        let mut c = [0.0f64; 4];
        for (mask, rgb, alpha) in layers {
            let a = f64::from(mask.a.get(i).copied().unwrap_or_default()) * alpha;
            super::over(&mut c, *rgb, a);
        }
        out.extend(super::bgra(c));
    }
    out
}

fn sprite(pixels: &[u8], w: usize, h: usize, scale: f64, anchor: P, pad: f64) -> Sprite {
    let (w, h) = (w as i32, h as i32);
    Sprite {
        buffer: MemoryRenderBuffer::from_slice(
            pixels,
            Fourcc::Argb8888,
            (w, h),
            1,
            Transform::Normal,
            None,
        ),
        px: (w, h),
        size: (f64::from(w) / scale, f64::from(h) / scale),
        anchor,
        pad,
    }
}

/// The baseline's distance under the middle of the em box, at `px`: how
/// canvas places `textBaseline = "middle"`.
fn middle_to_baseline(font: &fontdue::Font, px: f32) -> f32 {
    font.horizontal_line_metrics(px)
        .map_or(px * 0.35, |m| (m.ascent + m.descent) / 2.0)
}

/// The name's pill (the mockup's `nameTag`): 18 px high, 6 px round, the
/// name in 11 px bold on the violet, with a shadow 2 px down.
fn name_pill(font: &fontdue::Font, name: &str, scale: f64) -> Sprite {
    const PAD: f64 = 12.0;
    let text_w = f64::from(crate::radial_draw::advance(font, name, 11.0));
    let (w, h) = (text_w + 12.0, 18.0);
    let (pw, ph) = (
        ((w + 2.0 * PAD) * scale).ceil() as usize,
        ((h + 2.0 * PAD) * scale).ceil() as usize,
    );
    let mut pill = Mask::new(pw, ph);
    pill.rounded(
        (PAD * scale, PAD * scale, w * scale, h * scale),
        6.0 * scale,
    );
    let shadow = pill.shifted(2.0 * scale).blurred(3.0 * scale);
    let mut text = Mask::new(pw, ph);
    let px = (11.0 * scale) as f32;
    let baseline = (PAD + 9.5) * scale + f64::from(middle_to_baseline(font, px));
    text.write(
        font,
        name,
        px,
        (((PAD + 6.0) * scale) as f32, baseline as f32),
    );
    let pixels = compose(
        pw,
        ph,
        &[
            (&shadow, [0.0; 3], 0.5),
            (&pill, VIOLET, 1.0),
            (&text, ON_VIOLET, 1.0),
        ],
    );
    sprite(&pixels, pw, ph, scale, (PAD, PAD), 0.0)
}

/// The initial's disc (the mockup's `initialMark`): 8.5 px round, the
/// letter in its middle, a shadow 1 px down.
fn initial_disc(font: &fontdue::Font, letter: &str, scale: f64) -> Sprite {
    const R: f64 = 8.5;
    const PAD: f64 = 10.0;
    let side = ((2.0 * (R + PAD)) * scale).ceil() as usize;
    let c = (R + PAD) * scale;
    let mut disc = Mask::new(side, side);
    disc.rounded(
        (
            c - R * scale,
            c - R * scale,
            2.0 * R * scale,
            2.0 * R * scale,
        ),
        R * scale,
    );
    let shadow = disc.shifted(scale).blurred(2.5 * scale);
    let mut text = Mask::new(side, side);
    let px = (10.0 * scale) as f32;
    let width = f64::from(crate::radial_draw::advance(font, letter, px));
    let baseline = c + 0.5 * scale + f64::from(middle_to_baseline(font, px));
    text.write(
        font,
        letter,
        px,
        ((c - width / 2.0) as f32, baseline as f32),
    );
    let pixels = compose(
        side,
        side,
        &[
            (&shadow, [0.0; 3], 0.5),
            (&disc, VIOLET, 1.0),
            (&text, ON_VIOLET, 1.0),
        ],
    );
    sprite(&pixels, side, side, scale, (R + PAD, R + PAD), 0.0)
}

/// A word in the agent's hand (the mockup's `handLabel`, B): 40 px of
/// Caveat, a dark copy 1.5 px under it, the violet over it with its glow.
fn hand_word(font: &fontdue::Font, word: &str, scale: f64) -> Sprite {
    const PAD: f64 = 28.0;
    let text_w = f64::from(crate::radial_draw::advance(font, word, WORD_PX));
    let ascent = font
        .horizontal_line_metrics(WORD_PX)
        .map_or(f64::from(WORD_PX), |m| f64::from(m.ascent));
    let descent = font
        .horizontal_line_metrics(WORD_PX)
        .map_or(10.0, |m| f64::from(-m.descent));
    let (w, h) = (text_w + 2.0 * PAD, ascent + descent + 2.0 * PAD);
    let (pw, ph) = ((w * scale).ceil() as usize, (h * scale).ceil() as usize);
    let mut text = Mask::new(pw, ph);
    let baseline = (PAD + ascent) * scale;
    text.write(
        font,
        word,
        (f64::from(WORD_PX) * scale) as f32,
        ((PAD * scale) as f32, baseline as f32),
    );
    let dark = text.shifted(1.5 * scale);
    let dark_shadow = dark.blurred(2.5 * scale);
    let glow = text.blurred(9.0 * scale);
    let pixels = compose(
        pw,
        ph,
        &[
            (&dark_shadow, [8.0, 5.0, 20.0], 0.6 * 0.55),
            (&dark, [16.0, 10.0, 36.0], 0.55),
            (&glow, VIOLET, 0.95),
            (&text, VIOLET, 1.0),
        ],
    );
    sprite(&pixels, pw, ph, scale, (PAD, PAD + ascent), PAD)
}

/// The point a shape around a box starts or ends at, and the boxes'
/// shapes: what `draw` draws around a window or an element (global
/// logical `(x, y, w, h)`), as points of that space.
pub fn around(
    shape: Shape,
    (x, y, w, h): (f64, f64, f64, f64),
    room: (f64, f64, f64, f64),
    seed: u32,
) -> (Vec<P>, bool) {
    match shape {
        Shape::Rectangle => (vec![(x - 8.0, y - 6.0), (x + w + 8.0, y + h + 6.0)], false),
        Shape::Ellipse => {
            // Round the box, its corners just inside or a little out:
            // a hand encircles, it does not circumscribe.
            let (rx, ry) = (w / 2.0 * 1.2 + 12.0, h / 2.0 * 1.2 + 12.0);
            let c = (x + w / 2.0, y + h / 2.0);
            (vec![(c.0 - rx, c.1 - ry), (c.0 + rx, c.1 + ry)], false)
        }
        Shape::Arrow => {
            // At the box's left edge, its middle, from below on its left;
            // from its right when the screen ends there.
            let (left, _, right, _) = (room.0, room.1, room.0 + room.2, room.1 + room.3);
            let cy = y + h / 2.0;
            if x - 150.0 >= left + 8.0 || x + w + 150.0 > right - 8.0 {
                (vec![(x - 150.0, cy + 90.0), (x - 8.0, cy)], false)
            } else {
                (vec![(x + w + 150.0, cy + 90.0), (x + w + 8.0, cy)], false)
            }
        }
        Shape::Path => (squiggle(x, x + w, y + h + 5.0, seed), true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(shape: Shape, points: Vec<P>) -> Plan {
        Plan {
            shape,
            points,
            hand_made: false,
            word: None,
            room: (1920.0, 1080.0),
            seed: 7,
        }
    }

    #[test]
    fn a_shape_is_the_same_for_a_seed_and_wobbles_a_little() {
        let a = lines(&plan(
            Shape::Rectangle,
            vec![(100.0, 100.0), (400.0, 200.0)],
        ));
        let b = lines(&plan(
            Shape::Rectangle,
            vec![(100.0, 100.0), (400.0, 200.0)],
        ));
        assert_eq!(a, b);
        // Its points stay within a few pixels of the box.
        let (x0, y0, x1, y1) = bounds(&a);
        assert!(
            (x0 - 100.0).abs() < 6.0 && (x1 - 400.0).abs() < 6.0,
            "{x0} {x1}"
        );
        assert!(
            (y0 - 100.0).abs() < 9.0 && (y1 - 200.0).abs() < 6.0,
            "{y0} {y1}"
        );
        // An ellipse goes all round, a little past where it started.
        let e = lines(&plan(Shape::Ellipse, vec![(0.0, 0.0), (200.0, 100.0)]));
        let (x0, _, x1, _) = bounds(&e);
        assert!(x0 < 5.0 && x1 > 195.0, "{x0} {x1}");
        // An arrow is its shaft, then its head ending at the tip.
        let arrow = lines(&plan(Shape::Arrow, vec![(0.0, 0.0), (300.0, 0.0)]));
        assert_eq!(arrow.len(), 2);
        let tip = arrow[0].last().copied().unwrap_or_default();
        assert!((tip.0 - 300.0).abs() < 2.0 && tip.1.abs() < 2.0, "{tip:?}");
    }

    #[test]
    fn a_shape_is_drawn_by_hand_over_its_time() {
        let mut ink = AgentInk::default();
        let mut id = 0;
        let p = plan(Shape::Arrow, vec![(0.0, 0.0), (300.0, 100.0)]);
        let ((g, i), total) = ink
            .draw("Claude", "A".into(), &p, Duration::from_secs(10), &mut id)
            .expect("drawn");
        assert!(total > Duration::from_millis(600) && total < Duration::from_millis(1600));
        let mark = ink.mark((g, i)).expect("a mark");
        let at = |ms: u64| Duration::from_secs(10) + Duration::from_millis(ms);
        let count = |ms: u64| {
            mark.shown(at(ms))
                .iter()
                .map(|(_, l)| l.len())
                .sum::<usize>()
        };
        assert_eq!(count(0), 0);
        assert!(count(200) < count(400), "it grows");
        let whole: usize = mark.lines.iter().map(Vec::len).sum();
        assert_eq!(count(total.as_millis() as u64), whole);
        // The first shape has its agent's name; the next its initial.
        assert!(mark.named);
        ink.draw("Claude", "A".into(), &p, at(2000), &mut id);
        assert!(ink.groups[0].marks.iter().filter(|m| m.named).count() == 1);
        // It fades once told, then goes.
        assert!(ink.fade(Some("Claude"), at(3000)));
        assert!(!ink.shows());
        assert!(ink.tick(at(3300), false));
        ink.tick(at(3700), false);
        assert!(ink.groups.is_empty());
    }

    #[test]
    fn the_pictures_have_ink_where_their_anchor_says() {
        let ink = AgentInk::default();
        let name = ink
            .picture(&Picture::Name("Claude".into()), 1.0)
            .expect("drawn");
        assert!(name.size.0 > 40.0 && name.size.1 > 18.0);
        drop(name);
        let word = ink
            .picture(&Picture::Word("ici".into()), 2.0)
            .expect("drawn");
        assert!(word.px.0 as f64 >= word.size.0 * 2.0 - 1.0);
    }
}
