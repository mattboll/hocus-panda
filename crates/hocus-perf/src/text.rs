//! A day said in words, French or English: `hocus perf day`, the
//! Settings' Performances page, a signal's notification and its report.

use jiff::{Timestamp, civil::Date, tz::TimeZone};

use crate::{
    format::{Event, EventKind, Who},
    report::{Peak, Report},
    signals::{self, Signal, SignalKind},
};

/// The language said in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    French,
    #[default]
    English,
}

impl Lang {
    /// The messages' locale: `LC_ALL`, `LC_MESSAGES`, `LANG`.
    pub fn from_env() -> Self {
        let said = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
            .unwrap_or_default();
        if said.starts_with("fr") {
            Self::French
        } else {
            Self::English
        }
    }

    fn pick(self, fr: &'static str, en: &'static str) -> &'static str {
        match self {
            Self::French => fr,
            Self::English => en,
        }
    }
}

/// `x` with `decimals`, its separator the language's.
pub fn number(x: f64, decimals: usize, lang: Lang) -> String {
    let s = format!("{x:.decimals$}");
    match lang {
        Lang::French => s.replace('.', ","),
        Lang::English => s,
    }
}

/// A share of one core: whole from 10 %, else with a decimal.
pub fn percent(x: f64, lang: Lang) -> String {
    let n = if x >= 9.95 {
        number(x, 0, lang)
    } else {
        number(x, 1, lang)
    };
    match lang {
        Lang::French => format!("{n} %"),
        Lang::English => format!("{n}%"),
    }
}

/// Kilobytes in megabytes.
pub fn megabytes(kb: u32, lang: Lang) -> String {
    format!(
        "{} {}",
        (f64::from(kb) / 1024.0).round(),
        lang.pick("Mo", "MB")
    )
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
    use jiff::civil::Weekday::*;
    let day = match (date.weekday(), lang) {
        (Monday, Lang::French) => "lundi",
        (Tuesday, Lang::French) => "mardi",
        (Wednesday, Lang::French) => "mercredi",
        (Thursday, Lang::French) => "jeudi",
        (Friday, Lang::French) => "vendredi",
        (Saturday, Lang::French) => "samedi",
        (Sunday, Lang::French) => "dimanche",
        (Monday, Lang::English) => "Monday",
        (Tuesday, Lang::English) => "Tuesday",
        (Wednesday, Lang::English) => "Wednesday",
        (Thursday, Lang::English) => "Thursday",
        (Friday, Lang::English) => "Friday",
        (Saturday, Lang::English) => "Saturday",
        (Sunday, Lang::English) => "Sunday",
    };
    const FR: [&str; 12] = [
        "janvier",
        "février",
        "mars",
        "avril",
        "mai",
        "juin",
        "juillet",
        "août",
        "septembre",
        "octobre",
        "novembre",
        "décembre",
    ];
    const EN: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let month = usize::try_from(date.month() - 1).unwrap_or(0);
    let month = match lang {
        Lang::French => FR[month],
        Lang::English => EN[month],
    };
    format!("{day} {} {month}", date.day())
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
        Who::Compositor => lang.pick("compositeur", "compositor"),
        Who::Shell => "shell",
        Who::Settings => lang.pick("Réglages", "Settings"),
    }
}

/// An event in a few words.
pub fn event(e: &Event, lang: Lang) -> String {
    let what = e.what.as_str();
    match (e.kind, lang) {
        (EventKind::WindowOpened, Lang::French) => format!("fenêtre ouverte : {what}"),
        (EventKind::WindowOpened, Lang::English) => format!("window opened: {what}"),
        (EventKind::WindowClosed, Lang::French) => format!("fenêtre fermée : {what}"),
        (EventKind::WindowClosed, Lang::English) => format!("window closed: {what}"),
        (EventKind::ScreenPlugged, Lang::French) => format!("écran {what} branché"),
        (EventKind::ScreenPlugged, Lang::English) => format!("screen {what} plugged in"),
        (EventKind::ScreenUnplugged, Lang::French) => format!("écran {what} débranché"),
        (EventKind::ScreenUnplugged, Lang::English) => format!("screen {what} unplugged"),
        (EventKind::SettingsPage, Lang::French) => format!("Réglages › {what}"),
        (EventKind::SettingsPage, Lang::English) => format!("Settings › {what}"),
        (EventKind::Config, Lang::French) => format!("configuration changée : {what}"),
        (EventKind::Config, Lang::English) => format!("configuration changed: {what}"),
        (EventKind::WallpaperMoving, Lang::French) => format!("fond animé en marche : {what}"),
        (EventKind::WallpaperMoving, Lang::English) => format!("moving wallpaper on: {what}"),
        (kind, lang) => match kind {
            EventKind::Locked => lang.pick("session verrouillée", "session locked"),
            EventKind::Unlocked => lang.pick("session déverrouillée", "session unlocked"),
            EventKind::Suspend => lang.pick("mise en veille", "going to sleep"),
            EventKind::Resume => lang.pick("sortie de veille", "woken from sleep"),
            EventKind::OnBattery => lang.pick("sur batterie", "on battery"),
            EventKind::OnAc => lang.pick("sur secteur", "on AC"),
            EventKind::WallpaperStill => lang.pick("fond animé arrêté", "moving wallpaper stopped"),
            _ => lang.pick("autre chose", "something else"),
        }
        .to_owned(),
    }
}

/// What a peak was, and the line under it: its events, else what its
/// counters say.
pub fn peak(p: &Peak, lang: Lang) -> (String, String) {
    let seconds = p.seconds().max(1) as f64;
    let fps = p.frames as f64 / seconds;
    let of = |kind: EventKind| p.events.iter().filter(move |e| e.kind == kind);
    let opened = of(EventKind::WindowOpened).count();
    let what = if p.signal == Some(SignalKind::FramesAtRest) {
        match lang {
            Lang::French => format!(
                "Des images sans que rien ne bouge : {} par seconde",
                number(fps, 0, lang)
            ),
            Lang::English => format!(
                "Frames with nothing moving: {} a second",
                number(fps, 0, lang)
            ),
        }
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
        lang.pick("Le fond animé en marche", "The moving wallpaper running")
            .to_owned()
    } else if opened > 1 {
        match lang {
            Lang::French => format!("{opened} fenêtres ouvertes"),
            Lang::English => format!("{opened} windows opened"),
        }
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
            Some(SignalKind::SustainedCpu) => lang.pick("Un CPU soutenu", "Sustained CPU"),
            Some(SignalKind::Wakeups) => {
                lang.pick("La boucle réveillée au repos", "The loop woken at rest")
            }
            Some(SignalKind::SlowMoment) => lang.pick("Un moment lent", "A slow moment"),
            _ if p.inputs > 0 => lang.pick("Le bureau en usage", "The desktop in use"),
            _ if p.app_commits > 0 => lang.pick("Des fenêtres qui se dessinent", "Windows drawing"),
            _ if p.motion_frames > 0 => {
                lang.pick("Une animation du bureau", "A motion of the desktop")
            }
            _ => lang.pick("Rien de noté", "Nothing noted"),
        }
        .to_owned()
    };
    let mut why = match lang {
        Lang::French => format!("{} images/s", number(fps, 0, lang)),
        Lang::English => format!("{} frames/s", number(fps, 0, lang)),
    };
    why.push_str(if p.inputs > 0 {
        lang.pick(" · en usage", " · in use")
    } else {
        lang.pick(" · sans geste", " · untouched")
    });
    if p.events.len() > 1 {
        let more = p.events.len() - 1;
        why.push_str(&match lang {
            Lang::French => format!(
                " · {more} autre{} événement{}",
                if more > 1 { "s" } else { "" },
                if more > 1 { "s" } else { "" }
            ),
            Lang::English => format!(" · {more} more event{}", if more > 1 { "s" } else { "" }),
        });
    }
    (what, why)
}

/// A signal's title and the line under it, as the page shows them.
pub fn signal(s: &Signal, lang: Lang, tz: &TimeZone) -> (String, String) {
    let span = when(s.from, s.to, tz);
    let long = duration(s.to - s.from);
    match (s.kind, lang) {
        (SignalKind::SustainedCpu, Lang::French) => (
            format!("{span} : un CPU soutenu"),
            format!(
                "Jusqu’à {} d’un cœur pendant {long}. Signal : le compositeur et le shell au-dessus de {} pendant 5 min.",
                percent(s.value, lang),
                percent(signals::CPU_PERCENT, lang)
            ),
        ),
        (SignalKind::SustainedCpu, Lang::English) => (
            format!("{span}: sustained CPU"),
            format!(
                "Up to {} of a core for {long}. Signal: the compositor and the shell above {} for 5 min.",
                percent(s.value, lang),
                percent(signals::CPU_PERCENT, lang)
            ),
        ),
        (SignalKind::FramesAtRest, Lang::French) => (
            format!("{span} : des images sans que rien ne bouge"),
            format!(
                "{} images par seconde pendant {long}, sans geste ni fenêtre qui se dessine. Signal : plus d’une image par seconde au repos pendant 1 min.",
                number(s.value, 0, lang)
            ),
        ),
        (SignalKind::FramesAtRest, Lang::English) => (
            format!("{span}: frames with nothing moving"),
            format!(
                "{} frames a second for {long}, untouched, no window drawing. Signal: more than one frame a second at rest for 1 min.",
                number(s.value, 0, lang)
            ),
        ),
        (SignalKind::Wakeups, Lang::French) => (
            format!("{span} : la boucle réveillée au repos"),
            format!(
                "{} réveils par seconde pendant {long}, sans geste. Signal : plus de 5 réveils par seconde en moyenne sur 10 min sans geste.",
                number(s.value, 1, lang)
            ),
        ),
        (SignalKind::Wakeups, Lang::English) => (
            format!("{span}: the loop woken at rest"),
            format!(
                "{} wakeups a second for {long}, untouched. Signal: more than 5 wakeups a second on average over 10 min with no input.",
                number(s.value, 1, lang)
            ),
        ),
        (SignalKind::MemoryGrowing, lang) => {
            let whose = who(s.who.unwrap_or(Who::Shell), lang);
            let [a, b] = s.kb.unwrap_or_default();
            match lang {
                Lang::French => (
                    format!("La mémoire du {whose} ne fait que monter"),
                    format!(
                        "{} → {}, +{} Mo par heure depuis {}, sans jamais redescendre. Signal : une hausse continue sur plus de 3 heures.",
                        (f64::from(a) / 1024.0).round(),
                        megabytes(b, lang),
                        number(s.value, 1, lang),
                        clock(s.from, tz)
                    ),
                ),
                Lang::English => (
                    format!("The {whose}'s memory only grows"),
                    format!(
                        "{} → {}, +{} MB an hour since {}, never coming back down. Signal: a steady rise over more than 3 hours.",
                        (f64::from(a) / 1024.0).round(),
                        megabytes(b, lang),
                        number(s.value, 1, lang),
                        clock(s.from, tz)
                    ),
                ),
            }
        }
        (SignalKind::SlowMoment, Lang::French) => (
            format!("{span} : un moment lent"),
            format!(
                "Un tour de la boucle ou une image de {} ms. Signal : une image de plus de 50 ms, ou un tour de boucle de plus de 16 ms.",
                number(s.value, 0, lang)
            ),
        ),
        (SignalKind::SlowMoment, Lang::English) => (
            format!("{span}: a slow moment"),
            format!(
                "A turn of the loop or a frame of {} ms. Signal: a frame over 50 ms, or a loop turn over 16 ms.",
                number(s.value, 0, lang)
            ),
        ),
    }
}

/// A signal's notification, as it begins.
pub fn notification(s: &Signal, lang: Lang, tz: &TimeZone) -> String {
    let at = clock(s.from, tz);
    match (s.kind, lang) {
        (SignalKind::SustainedCpu, Lang::French) => format!(
            "Le bureau a pris {} d’un cœur pendant plus de 5 minutes (depuis {at}).",
            percent(s.value, lang)
        ),
        (SignalKind::SustainedCpu, Lang::English) => format!(
            "The desktop took {} of a core for more than 5 minutes (since {at}).",
            percent(s.value, lang)
        ),
        (SignalKind::FramesAtRest, Lang::French) => format!(
            "Le bureau a dessiné {} images par seconde sans rien qui bouge (depuis {at}).",
            number(s.value, 0, lang)
        ),
        (SignalKind::FramesAtRest, Lang::English) => format!(
            "The desktop drew {} frames a second with nothing moving (since {at}).",
            number(s.value, 0, lang)
        ),
        (SignalKind::Wakeups, Lang::French) => format!(
            "Le bureau s’est réveillé {} fois par seconde sans geste pendant 10 minutes.",
            number(s.value, 1, lang)
        ),
        (SignalKind::Wakeups, Lang::English) => format!(
            "The desktop woke {} times a second, untouched, for 10 minutes.",
            number(s.value, 1, lang)
        ),
        (SignalKind::MemoryGrowing, Lang::French) => format!(
            "La mémoire du {} monte depuis {at} : +{} Mo par heure.",
            who(s.who.unwrap_or(Who::Shell), lang),
            number(s.value, 1, lang)
        ),
        (SignalKind::MemoryGrowing, Lang::English) => format!(
            "The {}'s memory has been growing since {at}: +{} MB an hour.",
            who(s.who.unwrap_or(Who::Shell), lang),
            number(s.value, 1, lang)
        ),
        (SignalKind::SlowMoment, Lang::French) => format!(
            "Un moment lent du bureau à {at} : {} ms.",
            number(s.value, 0, lang)
        ),
        (SignalKind::SlowMoment, Lang::English) => format!(
            "A slow moment of the desktop at {at}: {} ms.",
            number(s.value, 0, lang)
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
            line.push_str(&match lang {
                Lang::French => format!(", dont {} verrouillé", duration(r.locked_s as i64)),
                Lang::English => format!(", {} of it locked", duration(r.locked_s as i64)),
            });
        }
        line.push(')');
    }
    line
}

/// The day's totals, a line each: what the page's tiles say.
pub fn totals(r: &Report, lang: Lang, tz: &TimeZone) -> Vec<(String, String)> {
    let mut lines = Vec::new();
    lines.push((
        "CPU".to_owned(),
        match lang {
            Lang::French => format!(
                "{} d’un cœur en moyenne · compositeur {} · shell {}",
                percent(r.cpu.average_percent, lang),
                cpu_time(r.cpu.compositor_ms),
                cpu_time(r.cpu.shell_ms)
            ),
            Lang::English => format!(
                "{} of a core on average · compositor {} · shell {}",
                percent(r.cpu.average_percent, lang),
                cpu_time(r.cpu.compositor_ms),
                cpu_time(r.cpu.shell_ms)
            ),
        },
    ));
    let sum = |kb: [u32; 2]| kb[0] + kb[1];
    lines.push((
        lang.pick("Mémoire", "Memory").to_owned(),
        format!(
            "{} → {} · {} {} {} {}",
            (f64::from(sum(r.memory.start_kb)) / 1024.0).round(),
            megabytes(sum(r.memory.end_kb), lang),
            lang.pick("pic", "peak"),
            megabytes(r.memory.peak_kb, lang),
            lang.pick("à", "at"),
            clock(r.memory.peak_at, tz)
        ),
    ));
    lines.push((
        lang.pick("Images", "Frames").to_owned(),
        format!(
            "{} · {} {}/s",
            count(r.drawing.frames, lang),
            lang.pick("réveils au repos", "wakeups at rest"),
            number(r.drawing.rest_wakeups_per_s, 1, lang)
        ),
    ));
    if r.drawing.motion_frames > 0 || r.moving_wallpaper_s > 0 {
        let mut said = match lang {
            Lang::French => format!("{} images animées", count(r.drawing.motion_frames, lang)),
            Lang::English => format!("{} frames in motion", count(r.drawing.motion_frames, lang)),
        };
        if r.moving_wallpaper_s > 0 {
            said.push_str(&format!(
                " · {} {}",
                lang.pick("fond animé", "moving wallpaper"),
                duration(r.moving_wallpaper_s as i64)
            ));
        }
        lines.push((lang.pick("Mouvements", "Motion").to_owned(), said));
    }
    if let Some(power) = &r.power {
        lines.push((
            lang.pick("Batterie", "Battery").to_owned(),
            format!(
                "{} W {} {}",
                number(f64::from(power.average_mw) / 1000.0, 1, lang),
                lang.pick("en moyenne sur", "on average over"),
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
        out.push_str(lang.pick(
            "Rien d’enregistré ce jour-là.\n",
            "Nothing recorded that day.\n",
        ));
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
    out.push_str(lang.pick("Pics\n", "Peaks\n"));
    if r.peaks.is_empty() {
        out.push_str(lang.pick("  aucun\n", "  none\n"));
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
    out.push_str(lang.pick("Signaux\n", "Signals\n"));
    if r.signals.is_empty() {
        out.push_str(lang.pick("  aucun\n", "  none\n"));
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
        lang.pick("— performances", "— performance"),
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
