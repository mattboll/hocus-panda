//! A day read back: its totals, its peaks with what was happening, its
//! signals. The Settings' page and `hocus perf day` show the same report.

use serde::Serialize;

use crate::{
    files::Day,
    format::{Event, EventKind, INTERVAL_MS, Sample, Who},
    signals::{self, Signal, SignalKind},
};

/// A peak: above twice the day's median CPU, and at least this much of
/// one core, so that a calm day's small stirs are not listed.
pub const PEAK_FLOOR_PERCENT: f64 = 2.0;
/// The peaks listed: those that cost the most CPU, and every signal's.
pub const MOST_PEAKS: usize = 8;
/// Events this long before or after a peak are what was happening.
pub const AROUND_S: i64 = 30;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Cpu {
    /// Milliseconds of CPU, each process's.
    pub compositor_ms: u64,
    pub shell_ms: u64,
    pub settings_ms: u64,
    /// The compositor's and the shell's, in percent of one core over the
    /// time recorded.
    pub average_percent: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Memory {
    /// The compositor's and the shell's, kilobytes, at the day's first
    /// sample and its last.
    pub start_kb: [u32; 2],
    pub end_kb: [u32; 2],
    /// Their sum at its highest, and when.
    pub peak_kb: u32,
    pub peak_at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Drawing {
    pub frames: u64,
    pub scenes: u64,
    /// Frames drawn while one of the desktop's motions ran.
    pub motion_frames: u64,
    pub wakeups: u64,
    /// The loop's wakeups a second while nothing was touched.
    pub rest_wakeups_per_s: f64,
    /// Frames drawn while locked.
    pub locked_frames: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Power {
    /// The machine's power on battery, on average, milliwatts.
    pub average_mw: u32,
    pub on_battery_s: u64,
}

/// A peak, numbered as the chart shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Peak {
    pub n: usize,
    pub from: i64,
    pub to: i64,
    /// The compositor's and the shell's CPU over it, in percent of one
    /// core, and in milliseconds.
    pub cpu_percent: f64,
    pub cpu_ms: u64,
    /// The signal it is, if it is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<SignalKind>,
    /// What happened from 30 s before it to 30 s after.
    pub events: Vec<Event>,
    /// What its samples counted: frames, input events, frames of the
    /// desktop's motions, the applications' commits.
    pub frames: u64,
    pub inputs: u64,
    pub motion_frames: u64,
    pub app_commits: u64,
}

impl Peak {
    pub fn seconds(&self) -> i64 {
        self.to - self.from
    }
}

/// A day: what it cost, its peaks and its signals.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Report {
    /// Its first sample's start and its last sample's end, Unix seconds.
    pub from: Option<i64>,
    pub to: Option<i64>,
    /// The time its samples cover, and the part of it locked.
    pub recorded_s: u64,
    pub locked_s: u64,
    pub cpu: Cpu,
    pub memory: Memory,
    pub drawing: Drawing,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power: Option<Power>,
    /// A moving wallpaper's running time.
    pub moving_wallpaper_s: u64,
    pub peaks: Vec<Peak>,
    pub signals: Vec<Signal>,
    /// The times locked, and a moving wallpaper's, from and to: the
    /// chart's hatches.
    pub locked: Vec<(i64, i64)>,
    pub wallpaper: Vec<(i64, i64)>,
}

/// The spans an `on` and `off` kind of event make over the samples: from
/// one `on` to the next `off`, or the day's end.
fn spans(samples: &[Sample], on: EventKind, off: EventKind) -> Vec<(i64, i64)> {
    let mut spans = Vec::new();
    let mut since: Option<i64> = None;
    let mut last_end = None;
    for s in samples {
        // A gap closes it: the session went or the machine slept.
        if let (Some(start), Some(end)) = (since, last_end)
            && s.from() - end > i64::from(2 * INTERVAL_MS / 1000)
        {
            spans.push((start, end));
            since = None;
        }
        for e in &s.e {
            if e.kind == on && since.is_none() {
                since = Some(e.t);
            } else if e.kind == off
                && let Some(start) = since.take()
            {
                spans.push((start, e.t));
            }
        }
        last_end = Some(s.t);
    }
    if let (Some(start), Some(end)) = (since, last_end) {
        spans.push((start, end));
    }
    spans
}

fn overlap(a: (i64, i64), b: (i64, i64)) -> i64 {
    (a.1.min(b.1) - a.0.max(b.0)).max(0)
}

fn median(mut values: Vec<f64>) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// A peak over `samples`, from `from` to `to`.
fn peak_of(samples: &[&Sample], from: i64, to: i64) -> Peak {
    let ms: u64 = samples.iter().map(|s| u64::from(s.d)).sum();
    let cpu_ms: u64 = samples
        .iter()
        .map(|s| u64::from(s.cpu_ms(Who::Compositor) + s.cpu_ms(Who::Shell)))
        .sum();
    Peak {
        n: 0,
        from,
        to,
        cpu_percent: if ms == 0 {
            0.0
        } else {
            cpu_ms as f64 * 100.0 / ms as f64
        },
        cpu_ms,
        signal: None,
        events: Vec::new(),
        frames: samples.iter().map(|s| u64::from(s.f)).sum(),
        inputs: samples.iter().map(|s| u64::from(s.i)).sum(),
        motion_frames: samples.iter().map(|s| u64::from(s.a)).sum(),
        app_commits: samples.iter().map(|s| u64::from(s.k)).sum(),
    }
}

/// The day's report.
pub fn report(day: &Day) -> Report {
    let samples = &day.samples;
    let mut r = Report {
        from: samples.first().map(Sample::from),
        to: samples.last().map(|s| s.t),
        ..Report::default()
    };
    let recorded_ms: u64 = samples.iter().map(|s| u64::from(s.d)).sum();
    r.recorded_s = recorded_ms / 1000;
    // CPU.
    let sum = |who| -> u64 { samples.iter().map(|s| u64::from(s.cpu_ms(who))).sum() };
    r.cpu = Cpu {
        compositor_ms: sum(Who::Compositor),
        shell_ms: sum(Who::Shell),
        settings_ms: sum(Who::Settings),
        average_percent: if recorded_ms == 0 {
            0.0
        } else {
            (sum(Who::Compositor) + sum(Who::Shell)) as f64 * 100.0 / recorded_ms as f64
        },
    };
    // Memory.
    let both = |s: &Sample| [s.rss_kb(Who::Compositor), s.rss_kb(Who::Shell)];
    let measured = || samples.iter().filter(|s| s.rss_kb(Who::Compositor) > 0);
    if let (Some(first), Some(last)) = (measured().next(), measured().next_back()) {
        r.memory.start_kb = both(first);
        r.memory.end_kb = both(last);
    }
    for s in measured() {
        let kb = s.rss_kb(Who::Compositor) + s.rss_kb(Who::Shell);
        if kb > r.memory.peak_kb {
            r.memory.peak_kb = kb;
            r.memory.peak_at = s.t;
        }
    }
    // Locked, and a moving wallpaper.
    r.locked = spans(samples, EventKind::Locked, EventKind::Unlocked);
    r.wallpaper = spans(
        samples,
        EventKind::WallpaperMoving,
        EventKind::WallpaperStill,
    );
    r.locked_s = r.locked.iter().map(|(a, b)| (b - a).max(0) as u64).sum();
    r.moving_wallpaper_s = r.wallpaper.iter().map(|(a, b)| (b - a).max(0) as u64).sum();
    let locked_at = |s: &Sample| {
        r.locked
            .iter()
            .any(|&l| overlap(l, (s.from(), s.t)) * 2 > i64::from(s.d / 1000))
    };
    // Drawing.
    let rest: Vec<&Sample> = samples.iter().filter(|s| s.i == 0).collect();
    let rest_ms: u64 = rest.iter().map(|s| u64::from(s.d)).sum();
    r.drawing = Drawing {
        frames: samples.iter().map(|s| u64::from(s.f)).sum(),
        scenes: samples.iter().map(|s| u64::from(s.s)).sum(),
        motion_frames: samples.iter().map(|s| u64::from(s.a)).sum(),
        wakeups: samples.iter().map(|s| u64::from(s.w)).sum(),
        rest_wakeups_per_s: if rest_ms == 0 {
            0.0
        } else {
            rest.iter().map(|s| u64::from(s.w)).sum::<u64>() as f64 * 1000.0 / rest_ms as f64
        },
        locked_frames: samples
            .iter()
            .filter(|s| locked_at(s))
            .map(|s| u64::from(s.f))
            .sum(),
    };
    // Power on battery.
    let battery: Vec<(u32, u32)> = samples.iter().filter_map(|s| Some((s.p?, s.d))).collect();
    let battery_ms: u64 = battery.iter().map(|b| u64::from(b.1)).sum();
    let energy: u64 = battery
        .iter()
        .map(|b| u64::from(b.0) * u64::from(b.1))
        .sum();
    if let Some(average) = energy.checked_div(battery_ms) {
        r.power = Some(Power {
            average_mw: u32::try_from(average).unwrap_or(u32::MAX),
            on_battery_s: battery_ms / 1000,
        });
    }
    // Signals.
    r.signals = signals::judge(samples);
    r.peaks = peaks(samples, &r.signals);
    r
}

/// The day's peaks: runs above twice its median CPU (and the floor), and
/// its signals, those that cost the most kept, numbered by time.
fn peaks(samples: &[Sample], signals: &[Signal]) -> Vec<Peak> {
    let threshold = (2.0 * median(samples.iter().map(Sample::desktop_percent).collect()))
        .max(PEAK_FLOOR_PERCENT);
    let mut found: Vec<Peak> = Vec::new();
    let mut run: Vec<&Sample> = Vec::new();
    let close = |run: &mut Vec<&Sample>, found: &mut Vec<Peak>| {
        if let (Some(first), Some(last)) = (run.first(), run.last()) {
            found.push(peak_of(run, first.from(), last.t));
        }
        run.clear();
    };
    for s in samples {
        let joined = run
            .last()
            .is_none_or(|last| s.from() - last.t <= i64::from(INTERVAL_MS / 1000));
        if s.desktop_percent() > threshold && joined {
            run.push(s);
        } else {
            close(&mut run, &mut found);
            if s.desktop_percent() > threshold {
                run.push(s);
            }
        }
    }
    close(&mut run, &mut found);
    // The signals (a memory's rise spans the day: not a peak), each over a
    // peak it meets, else a peak of its own.
    for signal in signals
        .iter()
        .filter(|s| s.kind != SignalKind::MemoryGrowing)
    {
        let span = (signal.from, signal.to);
        match found
            .iter_mut()
            .find(|p| overlap((p.from, p.to), span) > 0 || (p.from <= span.0 && span.1 <= p.to))
        {
            Some(p) => {
                p.signal.get_or_insert(signal.kind);
            }
            None => {
                let within: Vec<&Sample> = samples
                    .iter()
                    .filter(|s| s.from() >= span.0 && s.t <= span.1)
                    .collect();
                let mut p = peak_of(&within, span.0, span.1);
                p.signal = Some(signal.kind);
                found.push(p);
            }
        }
    }
    // The signals', then the costliest.
    found.sort_by(|a, b| {
        b.signal
            .is_some()
            .cmp(&a.signal.is_some())
            .then(b.cpu_ms.cmp(&a.cpu_ms))
    });
    found.truncate(MOST_PEAKS);
    found.sort_by_key(|p| p.from);
    let events: Vec<&Event> = samples.iter().flat_map(|s| &s.e).collect();
    for (i, p) in found.iter_mut().enumerate() {
        p.n = i + 1;
        p.events = events
            .iter()
            .filter(|e| e.t >= p.from - AROUND_S && e.t <= p.to + AROUND_S)
            .map(|e| (*e).clone())
            .collect();
    }
    found
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const T0: i64 = 1_762_500_000;

    /// A day of `n` calm samples from T0: 0.4 % of a core, 2 wakeups a
    /// sample, memory steady.
    pub fn calm(n: usize) -> Vec<Sample> {
        (0..n)
            .map(|i| Sample {
                t: T0 + 10 * (i as i64 + 1),
                d: INTERVAL_MS,
                c: vec![30, 10],
                m: vec![120_000, 62_000],
                w: 2,
                ..Sample::default()
            })
            .collect()
    }

    fn event(t: i64, kind: EventKind, what: &str) -> Event {
        Event {
            t,
            kind,
            what: what.to_owned(),
        }
    }

    /// The fixture day: an hour; a screen plugged at 10 min (a 20 s
    /// peak), locked from 20 to 30 min, 9 frames a second at rest from 40
    /// to 45 min.
    pub fn fixture() -> Day {
        let mut samples = calm(360);
        samples[59]
            .e
            .push(event(T0 + 595, EventKind::ScreenPlugged, "DP-2"));
        for s in &mut samples[60..62] {
            s.c = vec![1_800, 400];
            s.f = 300;
            s.m = vec![140_000, 62_000];
        }
        samples[120]
            .e
            .push(event(T0 + 1_205, EventKind::Locked, ""));
        samples[180]
            .e
            .push(event(T0 + 1_805, EventKind::Unlocked, ""));
        for s in &mut samples[240..270] {
            s.f = 90;
            s.c = vec![600, 300];
        }
        Day {
            samples,
            starts: vec![T0],
            notified: Vec::new(),
        }
    }

    #[test]
    fn the_fixture_day_s_totals() {
        let r = report(&fixture());
        assert_eq!((r.from, r.to), (Some(T0), Some(T0 + 3600)));
        assert_eq!(r.recorded_s, 3600);
        assert_eq!(
            r.cpu.compositor_ms,
            358 * 30 + 2 * 1_800 - 30 * 30 + 30 * 600
        );
        assert_eq!(r.locked, [(T0 + 1_205, T0 + 1_805)]);
        assert_eq!(r.locked_s, 600);
        assert_eq!(r.memory.start_kb, [120_000, 62_000]);
        assert_eq!((r.memory.peak_kb, r.memory.peak_at), (202_000, T0 + 610));
        assert_eq!(r.drawing.frames, 600 + 30 * 90);
        assert_eq!(r.drawing.locked_frames, 0);
        assert!((r.drawing.rest_wakeups_per_s - 0.2).abs() < 1e-9);
        assert_eq!(r.power, None);
    }

    #[test]
    fn the_fixture_day_s_peaks_say_what_was_happening() {
        let r = report(&fixture());
        assert_eq!(r.peaks.len(), 2, "{:?}", r.peaks);
        let plugged = &r.peaks[0];
        assert_eq!(
            (plugged.n, plugged.from, plugged.to),
            (1, T0 + 600, T0 + 620)
        );
        assert!((plugged.cpu_percent - 22.0).abs() < 1e-9);
        assert_eq!(plugged.events[0].kind, EventKind::ScreenPlugged);
        let rest = &r.peaks[1];
        assert_eq!(rest.signal, Some(SignalKind::FramesAtRest));
        assert_eq!((rest.from, rest.to), (T0 + 2_400, T0 + 2_700));
        assert_eq!(kinds(&r), [SignalKind::FramesAtRest]);
    }

    fn kinds(r: &Report) -> Vec<SignalKind> {
        r.signals.iter().map(|s| s.kind).collect()
    }

    #[test]
    fn a_calm_day_has_no_peak() {
        let r = report(&Day {
            samples: calm(8_640),
            ..Day::default()
        });
        assert!(r.peaks.is_empty() && r.signals.is_empty());
        assert!((r.cpu.average_percent - 0.4).abs() < 1e-9);
    }

    #[test]
    fn on_battery_its_power_is_averaged_over_its_time() {
        let mut samples = calm(4);
        samples[0].p = Some(6_000);
        samples[1].p = Some(8_000);
        let r = report(&Day {
            samples,
            ..Day::default()
        });
        assert_eq!(
            r.power,
            Some(Power {
                average_mw: 7_000,
                on_battery_s: 20
            })
        );
    }

    #[test]
    fn a_moving_wallpaper_runs_until_it_stops_or_the_day_ends() {
        let mut samples = calm(30);
        samples[2]
            .e
            .push(event(T0 + 25, EventKind::WallpaperMoving, "rain"));
        samples[9]
            .e
            .push(event(T0 + 95, EventKind::WallpaperStill, ""));
        samples[20]
            .e
            .push(event(T0 + 205, EventKind::WallpaperMoving, "rain"));
        let r = report(&Day {
            samples,
            ..Day::default()
        });
        assert_eq!(r.wallpaper, [(T0 + 25, T0 + 95), (T0 + 205, T0 + 300)]);
        assert_eq!(r.moving_wallpaper_s, 70 + 95);
    }
}
