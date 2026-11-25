//! What counts as a problem (docs/performance.md, "Signals"). Judged
//! sample after sample by a [`Watch`], with a few minutes of samples at
//! most in hand: the recorder raises a signal as it begins, a day read
//! back finds the same ones.
//!
//! | Signal | When |
//! |---|---|
//! | sustained CPU | the compositor and the shell above 10 % of one core over 5 min |
//! | frames at rest | more than 1 frame/s for 1 min, no input, no application drawing, beside the desktop's motions |
//! | wakeups | more than 5 loop wakeups/s on average over 10 min with no input |
//! | memory only growing | a rise over more than 3 h, more than 2 MB/h, never more than 5 % back |
//! | slow moment | a frame or a loop turn over 50 ms |
//!
//! Runs of sustained CPU that overlap are one signal, and so are slow
//! moments a minute apart or less, their worst kept.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::format::{INTERVAL_MS, Sample, Who};

/// Sustained CPU: above this share of one core…
pub const CPU_PERCENT: f64 = 10.0;
/// …on average over this long.
pub const CPU_OVER_MS: u32 = 5 * 60 * 1000;
/// Frames at rest: more than one a second…
pub const REST_FPS: f64 = 1.0;
/// …for this long.
pub const REST_OVER_MS: u32 = 60 * 1000;
/// Wakeups: more than this many a second with no input…
pub const WAKEUPS_PER_S: f64 = 5.0;
/// …on average over this long.
pub const WAKEUPS_OVER_MS: u32 = 10 * 60 * 1000;
/// Memory only growing: rising for this long…
pub const GROWTH_OVER_S: i64 = 3 * 3600;
/// …faster than this, kilobytes an hour (2 MB/h)…
pub const GROWTH_KB_PER_H: f64 = 2048.0;
/// …never back by more than this share of its highest.
pub const GROWTH_BACK: f64 = 0.05;
/// A slow moment: a loop turn or a frame over this long (a shorter turn
/// is a hitch nobody sees).
pub const SLOW_US: u32 = 50_000;
/// Slow moments this close together (seconds) are one.
pub const SLOW_JOINED_S: i64 = 60;

/// The kinds of signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SignalKind {
    SustainedCpu,
    FramesAtRest,
    Wakeups,
    MemoryGrowing,
    SlowMoment,
}

impl SignalKind {
    pub const ALL: [Self; 5] = [
        Self::SustainedCpu,
        Self::FramesAtRest,
        Self::Wakeups,
        Self::MemoryGrowing,
        Self::SlowMoment,
    ];

    /// How long after one of this kind ended another may begin and still
    /// be the same (seconds): a run of sustained CPU that overlaps the
    /// last (its window reaches back into it), slow moments a minute
    /// apart. None: each is its own.
    fn joins_within(self) -> Option<i64> {
        match self {
            Self::SustainedCpu => Some(0),
            Self::SlowMoment => Some(SLOW_JOINED_S),
            Self::FramesAtRest | Self::Wakeups | Self::MemoryGrowing => None,
        }
    }
}

/// A signal: its kind and the time it held, from and to (Unix seconds).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    pub kind: SignalKind,
    pub from: i64,
    pub to: i64,
    /// Its measure at its worst: percent of a core, frames or wakeups a
    /// second, megabytes an hour, milliseconds.
    pub value: f64,
    /// Whose memory grows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub who: Option<Who>,
    /// The memory where the rise began and where it is, kilobytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kb: Option<[u32; 2]>,
}

/// A run of samples kept for an average over a window.
#[derive(Debug, Default)]
struct Window {
    /// Each sample's start, length (ms) and count.
    held: VecDeque<(i64, u32, u64)>,
    ms: u64,
    sum: u64,
}

impl Window {
    /// Adds a sample and lets go of those beyond `over` (the oldest one
    /// that still fits keeps the window at least that long).
    fn push(&mut self, from: i64, d: u32, n: u64, over: u32) {
        self.held.push_back((from, d, n));
        self.ms += u64::from(d);
        self.sum += n;
        while let Some(&(_, d0, n0)) = self.held.front() {
            if self.ms - u64::from(d0) < u64::from(over) {
                break;
            }
            self.held.pop_front();
            self.ms -= u64::from(d0);
            self.sum -= n0;
        }
    }

    fn clear(&mut self) {
        *self = Self::default();
    }

    fn from(&self) -> Option<i64> {
        self.held.front().map(|h| h.0)
    }

    /// Its count per millisecond, once it spans `over`.
    fn rate(&self, over: u32) -> Option<f64> {
        (self.ms >= u64::from(over) && self.ms > 0).then(|| self.sum as f64 / self.ms as f64)
    }
}

/// A process's memory since it last went back: where it began, its
/// highest, and where it was each hour since (a step up, a screen
/// plugged, rises in one hour only: not a leak).
#[derive(Debug, Default, Clone)]
struct Growth {
    from: i64,
    from_kb: u32,
    highest: u32,
    hours: VecDeque<(i64, u32)>,
}

impl Growth {
    fn new(t: i64, kb: u32) -> Self {
        Self {
            from: t,
            from_kb: kb,
            highest: kb,
            hours: [(t, kb)].into(),
        }
    }

    /// Notes `kb` at `t`: a new hour's mark when an hour went by.
    fn note(&mut self, t: i64, kb: u32) {
        self.highest = self.highest.max(kb);
        if self.hours.back().is_none_or(|h| t - h.0 >= 3600) {
            self.hours.push_back((t, kb));
            if self.hours.len() > 4 {
                self.hours.pop_front();
            }
        }
    }

    /// Risen each of its last three hours, by a quarter of the slope at
    /// least.
    fn rising(&self) -> bool {
        let rose = |a: u32, b: u32| f64::from(b) - f64::from(a) >= GROWTH_KB_PER_H / 4.0;
        self.hours.len() >= 4
            && self
                .hours
                .iter()
                .zip(self.hours.iter().skip(1))
                .all(|(a, b)| rose(a.1, b.1))
    }
}

/// The five signals, judged sample after sample.
#[derive(Debug, Default)]
pub struct Watch {
    last: Option<i64>,
    /// Where the samples run unbroken from (the last gap's end): a signal
    /// that ended before it is not taken up again.
    since: i64,
    cpu: Window,
    rest: Window,
    wakeups: Window,
    growth: [Option<Growth>; 2],
    open: Vec<Signal>,
    done: Vec<Signal>,
}

impl Watch {
    pub fn new() -> Self {
        Self::default()
    }

    /// The signal of `kind` (and `who`) that holds now.
    fn open_of(&mut self, kind: SignalKind, who: Option<Who>) -> Option<usize> {
        self.open
            .iter()
            .position(|s| s.kind == kind && s.who == who)
    }

    /// The signal of `kind` (and `who`) that ended close enough before
    /// `from` to be taken up again.
    fn joinable(&self, kind: SignalKind, who: Option<Who>, from: i64) -> Option<usize> {
        let within = kind.joins_within()?;
        let i = self
            .done
            .iter()
            .rposition(|s| s.kind == kind && s.who == who)?;
        let s = self.done.get(i)?;
        (s.to >= self.since && from - s.to <= within).then_some(i)
    }

    /// `kind` holds from `from` to `to`: extended if it held or ended just
    /// before, else raised. Whether it is new.
    fn hold(&mut self, kind: SignalKind, from: i64, to: i64, value: f64, who: Option<Who>) -> bool {
        if self.open_of(kind, who).is_none()
            && let Some(i) = self.joinable(kind, who, from)
        {
            let s = self.done.remove(i);
            self.open.push(s);
        }
        match self.open_of(kind, who).and_then(|i| self.open.get_mut(i)) {
            Some(s) => {
                s.to = to;
                s.value = s.value.max(value);
                false
            }
            None => {
                self.open.push(Signal {
                    kind,
                    from,
                    to,
                    value,
                    who,
                    kb: None,
                });
                true
            }
        }
    }

    /// `kind` no longer holds.
    fn end(&mut self, kind: SignalKind, who: Option<Who>) {
        if let Some(i) = self.open_of(kind, who) {
            let s = self.open.remove(i);
            self.done.push(s);
        }
    }

    /// Judges `s`; the kinds raised by it.
    pub fn feed(&mut self, s: &Sample) -> Vec<SignalKind> {
        let mut raised = Vec::new();
        let from = s.from();
        // A gap (the machine slept, the session was off): what was
        // running ended.
        if self
            .last
            .is_some_and(|last| from - last > i64::from(2 * INTERVAL_MS / 1000))
        {
            self.cpu.clear();
            self.rest.clear();
            self.wakeups.clear();
            for kind in [
                SignalKind::SustainedCpu,
                SignalKind::FramesAtRest,
                SignalKind::Wakeups,
                SignalKind::SlowMoment,
            ] {
                self.end(kind, None);
            }
            self.since = from;
        }
        self.last = Some(s.t);
        let mut raise = |new: bool, kind| {
            if new && !raised.contains(&kind) {
                raised.push(kind);
            }
        };
        // Sustained CPU.
        let cpu = u64::from(s.cpu_ms(Who::Compositor) + s.cpu_ms(Who::Shell));
        self.cpu.push(from, s.d, cpu, CPU_OVER_MS);
        match self.cpu.rate(CPU_OVER_MS).map(|r| r * 100.0) {
            Some(percent) if percent > CPU_PERCENT => {
                let start = self.cpu.from().unwrap_or(from);
                let new = self.hold(SignalKind::SustainedCpu, start, s.t, percent, None);
                raise(new, SignalKind::SustainedCpu);
            }
            _ => self.end(SignalKind::SustainedCpu, None),
        }
        // Frames at rest: every sample of the run above one a second.
        let rest_frames = u64::from(s.f.saturating_sub(s.a));
        if s.at_rest() && rest_frames as f64 * 1000.0 > REST_FPS * f64::from(s.d) {
            self.rest.push(from, s.d, rest_frames, u32::MAX);
            if let Some(per_ms) = self.rest.rate(REST_OVER_MS) {
                let start = self.rest.from().unwrap_or(from);
                let new = self.hold(SignalKind::FramesAtRest, start, s.t, per_ms * 1000.0, None);
                raise(new, SignalKind::FramesAtRest);
            }
        } else {
            self.rest.clear();
            self.end(SignalKind::FramesAtRest, None);
        }
        // Wakeups with no input.
        if s.i == 0 {
            self.wakeups
                .push(from, s.d, u64::from(s.w), WAKEUPS_OVER_MS);
            match self.wakeups.rate(WAKEUPS_OVER_MS).map(|r| r * 1000.0) {
                Some(per_s) if per_s > WAKEUPS_PER_S => {
                    let start = self.wakeups.from().unwrap_or(from);
                    let new = self.hold(SignalKind::Wakeups, start, s.t, per_s, None);
                    raise(new, SignalKind::Wakeups);
                }
                _ => self.end(SignalKind::Wakeups, None),
            }
        } else {
            self.wakeups.clear();
            self.end(SignalKind::Wakeups, None);
        }
        // Memory only growing, the compositor's and the shell's.
        for who in [Who::Compositor, Who::Shell] {
            let kb = s.rss_kb(who);
            if kb == 0 {
                continue;
            }
            let i = who.index();
            let growth = match self.growth.get_mut(i).and_then(Option::take) {
                Some(mut g) if f64::from(kb) >= f64::from(g.highest) * (1.0 - GROWTH_BACK) => {
                    g.note(s.t, kb);
                    g
                }
                // Back by more than 5 % (or a new process): it starts
                // again from here.
                _ => {
                    self.end(SignalKind::MemoryGrowing, Some(who));
                    Growth::new(s.t, kb)
                }
            };
            // Once raised, it holds until the memory goes back or slows.
            let rising =
                growth.rising() || self.open_of(SignalKind::MemoryGrowing, Some(who)).is_some();
            let (from, from_kb) = (growth.from, growth.from_kb);
            if let Some(kept) = self.growth.get_mut(i) {
                *kept = Some(growth);
            }
            let hours = (s.t - from) as f64 / 3600.0;
            let slope = (f64::from(kb) - f64::from(from_kb)) / hours.max(f64::EPSILON);
            if s.t - from >= GROWTH_OVER_S && slope > GROWTH_KB_PER_H && rising {
                let new = self.hold(
                    SignalKind::MemoryGrowing,
                    from,
                    s.t,
                    slope / 1024.0,
                    Some(who),
                );
                if let Some(open) = self
                    .open_of(SignalKind::MemoryGrowing, Some(who))
                    .and_then(|at| self.open.get_mut(at))
                {
                    open.kb = Some([from_kb, kb]);
                }
                raise(new, SignalKind::MemoryGrowing);
            } else {
                self.end(SignalKind::MemoryGrowing, Some(who));
            }
        }
        // A slow moment.
        if s.tu.max(s.fu) > SLOW_US {
            let ms = f64::from(s.tu.max(s.fu)) / 1000.0;
            let new = self.hold(SignalKind::SlowMoment, from, s.t, ms, None);
            raise(new, SignalKind::SlowMoment);
        } else {
            self.end(SignalKind::SlowMoment, None);
        }
        raised
    }

    /// The signal of `kind` holding now, if one does.
    pub fn holding(&self, kind: SignalKind) -> Option<&Signal> {
        self.open.iter().find(|s| s.kind == kind)
    }

    /// Lets go of the signals that ended (the recorder keeps none), but
    /// the last of each kind that a later one may take up again.
    pub fn forget_ended(&mut self) {
        let mut kept: Vec<Signal> = Vec::new();
        for s in self.done.drain(..).rev() {
            if s.kind.joins_within().is_some() && !kept.iter().any(|k| k.kind == s.kind) {
                kept.push(s);
            }
        }
        kept.reverse();
        self.done = kept;
    }

    /// Every signal, those still holding included, by when they began.
    pub fn finish(mut self) -> Vec<Signal> {
        self.done.append(&mut self.open);
        self.done
            .sort_by(|a, b| a.from.cmp(&b.from).then(a.kind.cmp(&b.kind)));
        self.done
    }
}

/// The signals of a day's samples.
pub fn judge(samples: &[Sample]) -> Vec<Signal> {
    let mut watch = Watch::new();
    for s in samples {
        watch.feed(s);
    }
    watch.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_762_500_000;

    /// `n` samples of 10 s from T0, each made by `make` from its index.
    fn day(n: usize, make: impl Fn(usize) -> Sample) -> Vec<Sample> {
        (0..n)
            .map(|i| Sample {
                t: T0 + 10 * (i as i64 + 1),
                d: INTERVAL_MS,
                ..make(i)
            })
            .collect()
    }

    fn kinds(signals: &[Signal]) -> Vec<SignalKind> {
        signals.iter().map(|s| s.kind).collect()
    }

    fn cpu(compositor: u32, shell: u32) -> Sample {
        Sample {
            c: vec![compositor, shell],
            m: vec![100_000, 60_000],
            ..Sample::default()
        }
    }

    #[test]
    fn sustained_cpu_over_five_minutes_and_not_just_under() {
        // 11 % of a core (1.1 s in 10 s) for 5 min, then calm.
        let samples = day(60, |i| if i < 30 { cpu(800, 300) } else { cpu(10, 5) });
        let signals = judge(&samples);
        assert_eq!(kinds(&signals), [SignalKind::SustainedCpu]);
        assert_eq!(signals[0].from, T0);
        assert!((signals[0].value - 11.0).abs() < 0.01, "{signals:?}");
        // 9.9 %, or 11 % for 4 min 30 s (9.9 % over the 5 min): nothing.
        assert!(judge(&day(60, |_| cpu(700, 290))).is_empty());
        assert!(judge(&day(60, |i| if i < 27 { cpu(800, 300) } else { cpu(0, 0) })).is_empty());
    }

    #[test]
    fn frames_at_rest_for_a_minute_and_not_with_input_or_a_window_drawing() {
        let rest = |f: u32| Sample { f, ..cpu(10, 10) };
        // 9 frames a second for 15 min, nothing touched.
        let signals = judge(&day(90, |_| rest(90)));
        assert_eq!(kinds(&signals), [SignalKind::FramesAtRest]);
        assert!((signals[0].value - 9.0).abs() < 0.01);
        // 50 s of it, or one a second exactly: nothing.
        assert!(judge(&day(5, |_| rest(90))).is_empty());
        assert!(judge(&day(90, |_| rest(10))).is_empty());
        // Typing, a window drawing, or the desktop's own motion: nothing.
        assert!(judge(&day(90, |_| Sample { i: 1, ..rest(90) })).is_empty());
        assert!(judge(&day(90, |_| Sample { k: 3, ..rest(90) })).is_empty());
        assert!(judge(&day(90, |_| Sample { a: 90, ..rest(90) })).is_empty());
    }

    #[test]
    fn wakeups_over_ten_minutes_with_no_input() {
        let woke = |w: u32| Sample { w, ..cpu(1, 1) };
        let signals = judge(&day(60, |_| woke(60)));
        assert_eq!(kinds(&signals), [SignalKind::Wakeups]);
        assert!((signals[0].value - 6.0).abs() < 0.01);
        // 5 a second, or 9 min 50 s of 6: nothing.
        assert!(judge(&day(60, |_| woke(50))).is_empty());
        assert!(judge(&day(59, |_| woke(60))).is_empty());
        // An input in the middle starts the ten minutes again.
        assert!(
            judge(&day(90, |i| if i == 45 {
                Sample { i: 1, ..woke(60) }
            } else {
                woke(60)
            }))
            .is_empty()
        );
    }

    /// Every 10 min for `hours`, the shell's memory from 62 MB rising by
    /// `per_hour` KB an hour; a fall of `back` (a share) at the middle.
    fn growing(hours: i64, per_hour: f64, back: f64) -> Vec<Sample> {
        let n = (hours * 6 + 1) as usize;
        (0..n)
            .map(|i| {
                let h = i as f64 / 6.0;
                let mut kb = 62_000.0 + per_hour * h;
                if i == n / 2 {
                    kb *= 1.0 - back;
                }
                Sample {
                    t: T0 + 600 * i as i64,
                    d: INTERVAL_MS,
                    c: vec![1, 1],
                    m: vec![120_000, kb as u32],
                    ..Sample::default()
                }
            })
            .collect()
    }

    #[test]
    fn memory_only_growing_over_three_hours() {
        let signals = judge(&growing(9, 5_120.0, 0.0));
        assert_eq!(kinds(&signals), [SignalKind::MemoryGrowing]);
        let s = &signals[0];
        assert_eq!(s.who, Some(Who::Shell));
        assert_eq!(s.from, T0);
        assert!((s.value - 5.0).abs() < 0.01, "{s:?}");
        assert_eq!(s.kb, Some([62_000, 62_000 + 9 * 5_120]));
        // Under 2 MB/h, under 3 h, or back by 10 % halfway: none.
        assert!(judge(&growing(9, 2_000.0, 0.0)).is_empty());
        assert!(judge(&growing(2, 5_120.0, 0.0)).is_empty());
        assert!(judge(&growing(5, 5_120.0, 0.10)).is_empty());
        // Back by less than 5 %: still growing.
        assert_eq!(
            kinds(&judge(&growing(9, 5_120.0, 0.04))),
            [SignalKind::MemoryGrowing]
        );
    }

    /// Calm samples, but those `slow` says: its loop turn and frame, µs.
    fn slow(n: usize, slow: impl Fn(usize) -> Option<(u32, u32)>) -> Vec<Sample> {
        day(n, |i| {
            let (tu, fu) = slow(i).unwrap_or((2_000, 1_500));
            Sample {
                tu,
                fu,
                ..cpu(1, 1)
            }
        })
    }

    #[test]
    fn a_slow_moment_and_not_one_just_under() {
        // A turn over 50 ms, or a frame: one each.
        for (tu, fu) in [(50_001, 50_000), (50_000, 50_001)] {
            let signals = judge(&slow(3, |i| (i == 1).then_some((tu, fu))));
            assert_eq!(kinds(&signals), [SignalKind::SlowMoment]);
            assert_eq!((signals[0].from, signals[0].to), (T0 + 10, T0 + 20));
        }
        // 50 ms exactly, or a turn of 30 ms (once a slow moment): none.
        assert!(judge(&slow(3, |_| Some((50_000, 50_000)))).is_empty());
        assert!(judge(&slow(3, |_| Some((30_000, 4_000)))).is_empty());
    }

    #[test]
    fn slow_moments_a_minute_apart_are_one_with_their_worst() {
        // 60 ms, 0.3 s 30 s later, 80 ms a minute after that; then one
        // more than a minute after the last.
        let at = |i| match i {
            2 => Some((60_000, 3_000)),
            5 => Some((4_000, 300_000)),
            11 => Some((80_000, 2_000)),
            19 => Some((70_000, 2_000)),
            _ => None,
        };
        let signals = judge(&slow(30, at));
        assert_eq!(
            kinds(&signals),
            [SignalKind::SlowMoment, SignalKind::SlowMoment]
        );
        assert_eq!((signals[0].from, signals[0].to), (T0 + 20, T0 + 120));
        assert!((signals[0].value - 300.0).abs() < 1e-9, "{signals:?}");
        assert_eq!((signals[1].from, signals[1].to), (T0 + 190, T0 + 200));
        assert!((signals[1].value - 70.0).abs() < 1e-9);
        // A lone stall, as the screens waking at unlock: one, its own.
        let alone = judge(&slow(30, |i| (i == 12).then_some((310_000, 2_000))));
        assert_eq!(kinds(&alone), [SignalKind::SlowMoment]);
        assert!((alone[0].value - 310.0).abs() < 1e-9);
    }

    #[test]
    fn the_recorder_raises_slow_moments_close_together_once() {
        let mut watch = Watch::new();
        let raised: Vec<Vec<SignalKind>> = slow(12, |i| (i % 4 == 0).then_some((90_000, 0)))
            .iter()
            .map(|s| {
                let raised = watch.feed(s);
                watch.forget_ended();
                raised
            })
            .collect();
        assert_eq!(raised.iter().filter(|r| !r.is_empty()).count(), 1);
        assert_eq!(raised[0], [SignalKind::SlowMoment]);
    }

    #[test]
    fn overlapping_runs_of_sustained_cpu_are_one() {
        // 11 % for 6 min, a dip of 30 s that brings the 5 min under 10 %,
        // 24 % for 3 min: the second run's 5 min reach back into the
        // first. Then calm for 10 min, and 11 % for 6 min: another.
        let samples = day(160, |i| match i {
            0..36 | 117..153 => cpu(800, 300),
            39..57 => cpu(2_000, 400),
            _ => cpu(0, 0),
        });
        let signals = judge(&samples);
        assert_eq!(
            kinds(&signals),
            [SignalKind::SustainedCpu, SignalKind::SustainedCpu],
            "{signals:?}"
        );
        assert_eq!(signals[0].from, T0);
        assert!(signals[0].to > T0 + 570, "{signals:?}");
        assert!(signals[0].value > 15.0, "{signals:?}");
        assert!(signals[1].from > signals[0].to);
        // Read back sample by sample, as the recorder does: raised twice.
        let mut watch = Watch::new();
        let mut raised = 0;
        for s in &samples {
            raised += watch.feed(s).len();
            watch.forget_ended();
        }
        assert_eq!(raised, 2);
    }

    #[test]
    fn a_gap_parts_slow_moments() {
        // Slow at 30 s, asleep 40 s, slow again as it wakes: two.
        let mut samples = slow(3, |i| (i == 2).then_some((90_000, 0)));
        samples.extend(
            slow(3, |i| (i == 0).then_some((90_000, 0)))
                .into_iter()
                .map(|s| Sample { t: s.t + 70, ..s }),
        );
        assert_eq!(
            kinds(&judge(&samples)),
            [SignalKind::SlowMoment, SignalKind::SlowMoment]
        );
    }

    #[test]
    fn a_signal_is_raised_once_as_it_begins() {
        let mut watch = Watch::new();
        let raised: Vec<Vec<SignalKind>> = day(40, |_| cpu(800, 300))
            .iter()
            .map(|s| watch.feed(s))
            .collect();
        assert_eq!(raised.iter().filter(|r| !r.is_empty()).count(), 1);
        assert_eq!(raised[29], [SignalKind::SustainedCpu]);
        assert!(watch.holding(SignalKind::SustainedCpu).is_some());
    }

    #[test]
    fn a_gap_ends_what_was_running() {
        // 4 min of 11 %, the machine asleep for an hour, 4 min more.
        let mut samples = day(24, |_| cpu(800, 300));
        let later = day(24, |_| cpu(800, 300))
            .into_iter()
            .map(|s| Sample { t: s.t + 3600, ..s });
        samples.extend(later);
        assert!(judge(&samples).is_empty());
    }
}
