//! A day said in words, in the desktop's language: `hocus perf day`, the
//! Settings' Performances page, a signal's notification and its report.
//! The words are the catalogue's (`hocus_words::perf`).

use hocus_words::{date as dates, number as numbers, perf as words};
use jiff::{Timestamp, civil::Date, tz::TimeZone};

use crate::{
    format::{Event, EventKind, Who},
    report::{Peak, Report},
    signals::{self, Signal, SignalKind},
};

/// The language said in.
pub use hocus_words::Lang;

/// `x` with `decimals`, its separator the language's.
pub fn number(x: f64, decimals: usize, lang: Lang) -> String {
    format!("{x:.decimals$}").replace('.', numbers::DECIMAL.get(lang))
}

/// A share of one core: whole from 10 %, else with a decimal.
pub fn percent(x: f64, lang: Lang) -> String {
    let n = if x >= 9.95 {
        number(x, 0, lang)
    } else {
        number(x, 1, lang)
    };
    numbers::percent(lang, n)
}

/// Kilobytes in megabytes.
pub fn megabytes(kb: u32, lang: Lang) -> String {
    words::megabytes(lang, (f64::from(kb) / 1024.0).round())
}

/// A count of frames: 520, 52 k, 0.52 M.
pub fn count(n: u64, lang: Lang) -> String {
    match n {
        0..10_000 => n.to_string(),
        10_000..1_000_000 => format!("{} k", number(n as f64 / 1e3, 0, lang)),
        _ => format!("{} M", number(n as f64 / 1e6, 2, lang)),
    }
}

/// A length of time: 40 s, 15 min, 1 h 05.
pub fn duration(seconds: i64) -> String {
    let s = seconds.max(0);
    if s < 60 {
        format!("{s} s")
    } else if s < 3600 {
        format!("{} min", (s + 30) / 60)
    } else {
        let m = (s + 30) / 60;
        format!("{} h {:02}", m / 60, m % 60)
    }
}

/// CPU time: 40 s, 7 min 12 s, 1 h 02 min.
pub fn cpu_time(ms: u64) -> String {
    let s = (ms + 500) / 1000;
    if s < 60 {
        format!("{s} s")
    } else if s < 3600 {
        format!("{} min {:02} s", s / 60, s % 60)
    } else {
        format!("{} h {:02} min", s / 3600, s / 60 % 60)
    }
}

/// The local time of Unix second `t`: 09:05.
pub fn clock(t: i64, tz: &TimeZone) -> String {
    Timestamp::from_second(t)
        .map(|ts| ts.to_zoned(tz.clone()).strftime("%H:%M").to_string())
        .unwrap_or_default()
}

/// When something lasted: 09:05, or 14:02–14:33 past a minute.
pub fn when(from: i64, to: i64, tz: &TimeZone) -> String {
    if to - from <= 60 {
        clock(from, tz)
    } else {
        format!("{}–{}", clock(from, tz), clock(to, tz))
    }
}

/// A date said in full: vendredi 7 novembre, Friday 7 November.
pub fn date(date: Date, lang: Lang) -> String {
    let day = usize::from(date.weekday().to_monday_zero_offset().unsigned_abs());
    let month = usize::try_from(date.month() - 1).unwrap_or(0);
    let name =
        |list: hocus_words::List, i: usize| list.get(lang).get(i).copied().unwrap_or_default();
    format!(
        "{} {} {}",
        name(dates::WEEKDAYS, day),
        date.day(),
        name(dates::MONTHS, month)
    )
}

fn capital(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// Whose: the compositor, the shell, the Settings.
pub fn who(who: Who, lang: Lang) -> &'static str {
    match who {
        Who::Compositor => words::who::COMPOSITOR,
        Who::Shell => words::who::SHELL,
        Who::Settings => words::who::SETTINGS,
    }
    .get(lang)
}

/// An event in a few words.
pub fn event(e: &Event, lang: Lang) -> String {
    use words::event as w;
    let what = e.what.as_str();
    match e.kind {
        EventKind::WindowOpened => w::window_opened(lang, what),
        EventKind::WindowClosed => w::window_closed(lang, what),
        EventKind::ScreenPlugged => w::screen_plugged(lang, what),
        EventKind::ScreenUnplugged => w::screen_unplugged(lang, what),
        EventKind::SettingsPage => w::settings_page(lang, what),
        EventKind::Config => w::config(lang, what),
        EventKind::WallpaperMoving => w::wallpaper_moving(lang, what),
        kind => match kind {
            EventKind::Locked => w::LOCKED,
            EventKind::Unlocked => w::UNLOCKED,
            EventKind::Suspend => w::SUSPEND,
            EventKind::Resume => w::RESUME,
            EventKind::OnBattery => w::ON_BATTERY,
            EventKind::OnAc => w::ON_AC,
            EventKind::WallpaperStill => w::WALLPAPER_STILL,
            _ => w::OTHER,
        }
        .get(lang)
        .to_owned(),
    }
}

/// What a peak was, and the line under it: its events, else what its
/// counters say.
pub fn peak(p: &Peak, lang: Lang) -> (String, String) {
    use words::peak as w;
    let seconds = p.seconds().max(1) as f64;
    let fps = p.frames as f64 / seconds;
    let of = |kind: EventKind| p.events.iter().filter(move |e| e.kind == kind);
    let opened = of(EventKind::WindowOpened).count();
    let what = if p.signal == Some(SignalKind::FramesAtRest) {
        w::frames_at_rest(lang, number(fps, 0, lang))
    } else if let Some(e) = [
        EventKind::ScreenPlugged,
        EventKind::ScreenUnplugged,
        EventKind::SettingsPage,
    ]
    .into_iter()
    .find_map(|k| of(k).next())
    {
        capital(&event(e, lang))
    } else if p.wallpaper {
        w::WALLPAPER.get(lang).to_owned()
    } else if opened > 1 {
        w::windows_opened(lang, opened)
    } else if let Some(e) = [
        EventKind::WindowOpened,
        EventKind::WindowClosed,
        EventKind::Config,
        EventKind::Resume,
        EventKind::Unlocked,
        EventKind::Locked,
        EventKind::WallpaperMoving,
        EventKind::OnBattery,
        EventKind::OnAc,
        EventKind::Suspend,
    ]
    .into_iter()
    .find_map(|k| of(k).next())
    {
        capital(&event(e, lang))
    } else {
        match p.signal {
            Some(SignalKind::SustainedCpu) => w::SUSTAINED_CPU,
            Some(SignalKind::Wakeups) => w::WAKEUPS,
            Some(SignalKind::SlowMoment) => w::SLOW_MOMENT,
            _ if p.inputs > 0 => w::IN_USE,
            _ if p.app_commits > 0 => w::WINDOWS_DRAWING,
            _ if p.motion_frames > 0 => w::MOTION,
            _ => w::NOTHING,
        }
        .get(lang)
        .to_owned()
    };
    let mut why = words::fps(lang, number(fps, 0, lang));
    why.push_str(if p.inputs > 0 {
        words::IN_USE.get(lang)
    } else {
        words::UNTOUCHED.get(lang)
    });
    if p.events.len() > 1 {
        why.push_str(&words::more_events(lang, p.events.len() - 1));
    }
    (what, why)
}

/// A signal's title and the line under it, as the page shows them.
pub fn signal(s: &Signal, lang: Lang, tz: &TimeZone) -> (String, String) {
    use words::signal as w;
    let span = when(s.from, s.to, tz);
    let long = duration(s.to - s.from);
    match s.kind {
        SignalKind::SustainedCpu => (
            w::sustained_cpu(lang, &span),
            w::sustained_cpu_line(
                lang,
                w::SustainedCpuLine {
                    long: &long,
                    percent: percent(s.value, lang),
                    threshold: percent(signals::CPU_PERCENT, lang),
                },
            ),
        ),
        SignalKind::FramesAtRest => (
            w::frames_at_rest(lang, &span),
            w::frames_at_rest_line(
                lang,
                w::FramesAtRestLine {
                    fps: number(s.value, 0, lang),
                    long: &long,
                },
            ),
        ),
        SignalKind::Wakeups => (
            w::wakeups(lang, &span),
            w::wakeups_line(
                lang,
                w::WakeupsLine {
                    long: &long,
                    rate: number(s.value, 1, lang),
                },
            ),
        ),
        SignalKind::MemoryGrowing => {
            let whose = who(s.who.unwrap_or(Who::Shell), lang);
            let [a, b] = s.kb.unwrap_or_default();
            (
                w::memory(lang, whose),
                w::memory_line(
                    lang,
                    w::MemoryLine {
                        end: megabytes(b, lang),
                        rate: number(s.value, 1, lang),
                        since: clock(s.from, tz),
                        start: (f64::from(a) / 1024.0).round(),
                    },
                ),
            )
        }
        SignalKind::SlowMoment => (
            w::slow_moment(lang, &span),
            w::slow_moment_line(lang, number(s.value, 0, lang)),
        ),
    }
}

/// A signal's notification, as it begins.
pub fn notification(s: &Signal, lang: Lang, tz: &TimeZone) -> String {
    use words::notification as w;
    let at = clock(s.from, tz);
    match s.kind {
        SignalKind::SustainedCpu => w::sustained_cpu(
            lang,
            w::SustainedCpu {
                at: &at,
                percent: percent(s.value, lang),
            },
        ),
        SignalKind::FramesAtRest => w::frames_at_rest(
            lang,
            w::FramesAtRest {
                at: &at,
                fps: number(s.value, 0, lang),
            },
        ),
        SignalKind::Wakeups => w::wakeups(lang, number(s.value, 1, lang)),
        SignalKind::MemoryGrowing => w::memory(
            lang,
            w::Memory {
                at: &at,
                rate: number(s.value, 1, lang),
                whose: who(s.who.unwrap_or(Who::Shell), lang),
            },
        ),
        SignalKind::SlowMoment => w::slow_moment(
            lang,
            w::SlowMoment {
                at: &at,
                ms: number(s.value, 0, lang),
            },
        ),
    }
}

/// The session's line: Friday 7 November · session 08:30 → 18:30 (10 h,
/// 1 h 05 of it locked).
pub fn heading(r: &Report, day: Date, lang: Lang, tz: &TimeZone) -> String {
    let mut line = capital(&date(day, lang));
    if let (Some(from), Some(to)) = (r.from, r.to) {
        line.push_str(&format!(
            " · session {} → {} ({}",
            clock(from, tz),
            clock(to, tz),
            duration(to - from)
        ));
        if r.locked_s > 0 {
            line.push_str(&words::locked(lang, duration(r.locked_s as i64)));
        }
        line.push(')');
    }
    line
}

/// The day's totals, a line each: what the page's tiles say.
pub fn totals(r: &Report, lang: Lang, tz: &TimeZone) -> Vec<(String, String)> {
    use words::totals as w;
    let mut lines = Vec::new();
    lines.push((
        "CPU".to_owned(),
        w::cpu(
            lang,
            w::Cpu {
                compositor: cpu_time(r.cpu.compositor_ms),
                percent: percent(r.cpu.average_percent, lang),
                shell: cpu_time(r.cpu.shell_ms),
            },
        ),
    ));
    let sum = |kb: [u32; 2]| kb[0] + kb[1];
    lines.push((
        w::MEMORY.get(lang).to_owned(),
        format!(
            "{} → {} · {} {} {} {}",
            (f64::from(sum(r.memory.start_kb)) / 1024.0).round(),
            megabytes(sum(r.memory.end_kb), lang),
            w::PEAK.get(lang),
            megabytes(r.memory.peak_kb, lang),
            w::AT.get(lang),
            clock(r.memory.peak_at, tz)
        ),
    ));
    lines.push((
        w::FRAMES.get(lang).to_owned(),
        format!(
            "{} · {} {}/s",
            count(r.drawing.frames, lang),
            w::WAKEUPS_AT_REST.get(lang),
            number(r.drawing.rest_wakeups_per_s, 1, lang)
        ),
    ));
    if r.drawing.motion_frames > 0 || r.moving_wallpaper_s > 0 {
        let mut said = w::motion_frames(lang, count(r.drawing.motion_frames, lang));
        if r.moving_wallpaper_s > 0 {
            said.push_str(&format!(
                " · {} {}",
                w::MOVING_WALLPAPER.get(lang),
                duration(r.moving_wallpaper_s as i64)
            ));
        }
        lines.push((w::MOTION.get(lang).to_owned(), said));
    }
    if let Some(power) = &r.power {
        lines.push((
            w::BATTERY.get(lang).to_owned(),
            format!(
                "{} W {} {}",
                number(f64::from(power.average_mw) / 1000.0, 1, lang),
                w::ON_AVERAGE_OVER.get(lang),
                duration(power.on_battery_s as i64)
            ),
        ));
    }
    lines
}

/// `hocus perf day`: the day's totals, peaks and signals.
pub fn day(r: &Report, on: Date, lang: Lang, tz: &TimeZone) -> String {
    let mut out = heading(r, on, lang, tz);
    out.push('\n');
    if r.from.is_none() {
        out.push_str(words::NOTHING_RECORDED.get(lang));
        return out;
    }
    let lines = totals(r, lang, tz);
    let width = lines
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0)
        + 2;
    for (key, said) in &lines {
        out.push_str(&format!("{key:width$}{said}\n"));
    }
    out.push('\n');
    out.push_str(words::PEAKS.get(lang));
    if r.peaks.is_empty() {
        out.push_str(words::NONE.get(lang));
    }
    for p in &r.peaks {
        let (what, _) = peak(p, lang);
        out.push_str(&format!(
            "{} {:<12} {:>6}  {:<7}{what}\n",
            if p.signal.is_some() { '!' } else { ' ' },
            when(p.from, p.to, tz),
            percent(p.cpu_percent, lang),
            duration(p.seconds()),
        ));
    }
    out.push('\n');
    out.push_str(words::SIGNALS.get(lang));
    if r.signals.is_empty() {
        out.push_str(words::NONE.get(lang));
    }
    for s in &r.signals {
        let (title, line) = signal(s, lang, tz);
        out.push_str(&format!("! {title}\n  {line}\n"));
    }
    out
}

/// "Copier le rapport": a signal with its day, for a ticket.
pub fn signal_report(r: &Report, s: &Signal, on: Date, lang: Lang, tz: &TimeZone) -> String {
    let (title, line) = signal(s, lang, tz);
    format!(
        "Hocuspanda {} {}\n{title}\n{line}\n\n{}",
        env!("CARGO_PKG_VERSION"),
        words::REPORT.get(lang),
        day(r, on, lang, tz)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{report, tests::fixture};

    fn on() -> Date {
        jiff::civil::date(2025, 11, 7)
    }

    #[test]
    fn numbers_said_in_each_language() {
        assert_eq!(percent(22.04, Lang::French), "22 %");
        assert_eq!(percent(4.6, Lang::French), "4,6 %");
        assert_eq!(percent(4.6, Lang::English), "4.6%");
        assert_eq!(duration(40), "40 s");
        assert_eq!(duration(15 * 60), "15 min");
        assert_eq!(duration(3900), "1 h 05");
        assert_eq!(cpu_time(432_000), "7 min 12 s");
        assert_eq!(count(520_000, Lang::French), "520 k");
        assert_eq!(count(5_200_000, Lang::French), "5,20 M");
        assert_eq!(date(on(), Lang::French), "vendredi 7 novembre");
    }

    #[test]
    fn the_fixture_day_said_in_french() {
        let tz = TimeZone::UTC;
        let said = day(&report(&fixture()), on(), Lang::French, &tz);
        let expected = "\
Vendredi 7 novembre · session 07:20 → 08:20 (1 h 00, dont 10 min verrouillé)
CPU      1,2 % d’un cœur en moyenne · compositeur 31 s · shell 13 s
Mémoire  178 → 178 Mo · pic 197 Mo à 07:30
Images   3300 · réveils au repos 0,2/s

Pics
  07:30          22 %  20 s   Écran DP-2 branché
! 08:00–08:05   9,0 %  5 min  Des images sans que rien ne bouge : 9 par seconde

Signaux
! 08:00–08:05 : des images sans que rien ne bouge
  9 images par seconde pendant 5 min, sans geste ni fenêtre qui se dessine. Signal : plus d’une image par seconde au repos pendant 1 min.
";
        assert_eq!(said, expected);
    }

    #[test]
    fn a_signal_s_notification() {
        let tz = TimeZone::UTC;
        let r = report(&fixture());
        assert_eq!(
            notification(&r.signals[0], Lang::French, &tz),
            "Le bureau a dessiné 9 images par seconde sans rien qui bouge (depuis 08:00)."
        );
        assert_eq!(
            notification(&r.signals[0], Lang::English, &tz),
            "The desktop drew 9 frames a second with nothing moving (since 08:00)."
        );
    }
}
