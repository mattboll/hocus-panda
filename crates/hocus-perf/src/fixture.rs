//! An invented working day, for tests and screenshots: the same every
//! time. From 08:30 to 18:30: on battery until a screen is plugged at
//! 09:05 and again from 17:20; the Settings' wallpaper page at 11:40;
//! locked from 12:20 to 13:25; a moving wallpaper from 14:02 to 14:33 and
//! from 17:35 to 17:45; 9 frames a second with nothing moving from 15:10
//! to 15:25; 20 windows opened at 16:30; the shell's memory rising 5 MB
//! an hour from 09:10.

use jiff::{civil::Date, tz::TimeZone};

use crate::{
    files::Day,
    format::{Event, EventKind, INTERVAL_MS, Line, Sample},
};

/// A small deterministic generator (xorshift), so that the day is the
/// same on every machine.
struct Dice(u64);

impl Dice {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

const APPS: [&str; 6] = [
    "org.mozilla.firefox",
    "foot",
    "org.gnome.Nautilus",
    "code",
    "org.telegram.desktop",
    "thunderbird",
];

/// The day of `date`, its times local to `tz`.
pub fn working_day(date: Date, tz: &TimeZone) -> Day {
    let start = date
        .at(8, 30, 0, 0)
        .to_zoned(tz.clone())
        .map_or(0, |z| z.timestamp().as_second());
    let minute = |h: i64, m: i64| start + (h * 60 + m - 8 * 60 - 30) * 60;
    let mut dice = Dice(655);
    let mut samples = Vec::new();
    let n = 10 * 360;
    for i in 0..n {
        let t = start + 10 * (i + 1);
        let from = t - 10;
        let within = |a: i64, b: i64| from >= a && from < b;
        let locked = within(minute(12, 20), minute(13, 25));
        let mut e = Vec::new();
        let mut event = |at: i64, kind: EventKind, what: &str| {
            if at >= from && at < t {
                e.push(Event {
                    t: at,
                    kind,
                    what: what.to_owned(),
                });
            }
        };
        let (mut comp, mut shell, mut fps, mut motion);
        let (inputs, commits);
        if locked {
            (comp, shell, fps, motion) = (8.0, 4.0, 0.0, 0.0);
            (inputs, commits) = (0, 0);
        } else {
            comp = 45.0 + dice.next() * 160.0;
            shell = 25.0 + dice.next() * 80.0;
            fps = 6.0 + dice.next() * 10.0;
            motion = fps * 10.0 * dice.next() * 0.3;
            inputs = if dice.next() < 0.7 {
                20 + (dice.next() * 380.0) as u32
            } else {
                0
            };
            commits = 30 + (dice.next() * 120.0) as u32;
        }
        let mut commits = commits;
        let mut inputs = inputs;
        let mut settings = None;
        // 09:05: the 34-inch screen plugged.
        event(minute(9, 5) + 2, EventKind::ScreenPlugged, "DP-2");
        event(minute(9, 5) + 3, EventKind::OnAc, "");
        if within(minute(9, 5), minute(9, 5) + 40) {
            comp += 1_400.0;
            shell += 600.0;
            fps += 20.0;
        }
        // 11:40: the Settings' wallpaper page.
        event(minute(11, 40) + 3, EventKind::SettingsPage, "Fond d’écran");
        if within(minute(11, 40), minute(11, 41)) {
            comp += 400.0;
            shell += 1_100.0;
            fps += 8.0;
            settings = Some(2_000);
        } else if within(minute(11, 39), minute(11, 52)) {
            settings = Some(60);
        }
        event(minute(12, 20) + 4, EventKind::Locked, "");
        event(minute(13, 25) + 2, EventKind::Unlocked, "");
        // The moving wallpaper.
        event(minute(14, 2), EventKind::WallpaperMoving, "rain");
        event(minute(14, 33), EventKind::WallpaperStill, "");
        event(minute(17, 35), EventKind::WallpaperMoving, "rain");
        event(minute(17, 45), EventKind::WallpaperStill, "");
        let rain = within(minute(14, 2), minute(14, 33)) || within(minute(17, 35), minute(17, 45));
        if within(minute(14, 2), minute(14, 33)) {
            comp += 360.0 + dice.next() * 50.0;
            fps += 15.0;
            motion += 150.0;
        }
        if within(minute(17, 35), minute(17, 45)) {
            comp += 180.0;
            fps += 8.0;
            motion += 80.0;
        }
        // 15:10–15:25: the shell redraws for nothing.
        if within(minute(15, 10), minute(15, 25)) {
            shell = 680.0 + dice.next() * 60.0;
            comp = 80.0;
            fps = 9.0;
            motion = 0.0;
            inputs = 0;
            commits = 0;
        }
        // 16:30: twenty windows at once.
        if within(minute(16, 30), minute(16, 30) + 50) {
            comp += 1_700.0;
            shell += 700.0;
            fps += 30.0;
        }
        for k in 0..20 {
            event(minute(16, 30) + 1 + k * 2, EventKind::WindowOpened, "foot");
        }
        // Windows now and then.
        if !locked && i % 17 == 5 {
            let app = APPS
                .get((dice.next() * APPS.len() as f64) as usize % APPS.len())
                .copied()
                .unwrap_or_default();
            let kind = if i % 2 == 0 {
                EventKind::WindowOpened
            } else {
                EventKind::WindowClosed
            };
            event(from + 4, kind, app);
        }
        event(minute(17, 20), EventKind::OnBattery, "");
        let battery = from < minute(9, 5) || from >= minute(17, 20);
        let minutes = (from - start) as f64 / 60.0;
        let comp_mb = 118.0
            + if from >= minute(9, 5) { 20.0 } else { 0.0 }
            + if within(minute(16, 30), minute(16, 50)) {
                24.0
            } else {
                0.0
            }
            + dice.next() * 2.0;
        let shell_mb = 62.0 + (minutes - 40.0).max(0.0) * 5.0 / 60.0 + dice.next() * 1.5;
        let mut c = vec![comp as u32, shell as u32];
        let mut m = vec![(comp_mb * 1024.0) as u32, (shell_mb * 1024.0) as u32];
        if let Some(cpu) = settings {
            c.push(cpu);
            m.push(80 * 1024);
        }
        samples.push(Sample {
            t,
            d: INTERVAL_MS,
            c,
            m,
            f: (fps * 10.0) as u32,
            s: (fps * 10.0) as u32 + 4,
            w: if locked {
                1
            } else if within(minute(15, 10), minute(15, 25)) {
                35 + (dice.next() * 10.0) as u32
            } else if inputs == 0 {
                2 + (dice.next() * 6.0) as u32
            } else {
                40 + (dice.next() * 200.0) as u32
            },
            i: inputs,
            a: motion as u32,
            k: commits,
            tu: if locked {
                0
            } else {
                2_000 + (dice.next() * 4_000.0) as u32
            },
            fu: if locked || fps == 0.0 {
                0
            } else {
                1_500 + (dice.next() * 3_500.0) as u32
            },
            al: 0,
            p: battery
                .then(|| (6_200.0 + dice.next() * 1_400.0 + if rain { 400.0 } else { 0.0 }) as u32),
            o: Default::default(),
            e,
        });
    }
    Day {
        samples,
        starts: vec![start],
        notified: Vec::new(),
    }
}

/// The day as its file says it.
pub fn text(day: &Day) -> String {
    let mut text = String::new();
    for start in &day.starts {
        text.push_str(
            &Line::Start {
                start: *start,
                interval: INTERVAL_MS,
            }
            .encode(),
        );
        text.push('\n');
    }
    for s in &day.samples {
        text.push_str(&Line::Sample(s.clone()).encode());
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{report::report, signals::SignalKind};

    #[test]
    fn the_invented_day_has_its_two_signals_and_its_peaks() {
        let day = working_day(jiff::civil::date(2026, 11, 7), &TimeZone::UTC);
        assert_eq!(Day::parse(&text(&day)), day);
        let r = report(&day);
        let kinds: Vec<SignalKind> = r.signals.iter().map(|s| s.kind).collect();
        assert_eq!(kinds, [SignalKind::MemoryGrowing, SignalKind::FramesAtRest]);
        assert!(r.peaks.len() >= 5, "{:#?}", r.peaks);
        assert!(r.power.is_some());
        assert_eq!(r.locked_s, 65 * 60 - 2);
        assert!(text(&day).len() < 1_000_000);
    }
}
