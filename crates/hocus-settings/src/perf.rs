//! Performances (#655, docs/design/settings.md "Performances"): a day of
//! the desktop's CPU and memory, as the compositor's recorder wrote it
//! (`hocus_perf`), read on a thread of the page's own; and `[perf]
//! record`, its switch.
//!
//! What the page shows is computed here, apart from drawing it: the day in
//! buckets along one time axis, the four tiles, the signals and the peaks
//! in words. `hocus perf day` reads the same files with the same report.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use hocus_perf::{Day, Report, files, format::Who, report::report, text};
use jiff::{civil::Date, tz::TimeZone};

use crate::{
    lang::{Lang, w},
    model::{Codec, Kind, Place, Places, RowGroup, SectionId, Setting, Value},
};

/// The switch's row.
pub const RECORD: &str = "perf.record";

const RECORDING: RowGroup = RowGroup {
    label: w("ENREGISTREMENT", "RECORDING"),
    per_source: false,
};

/// The page's settings: the switch.
pub fn settings() -> &'static [Setting] {
    static SETTINGS: OnceLock<Vec<Setting>> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        vec![Setting {
            id: RECORD,
            section: SectionId::Performance,
            group: RECORDING,
            name: w("Mesurer la journée", "Record the day"),
            line: Some(w(
                "Toutes les 10 s : le CPU et la mémoire du compositeur et du shell, les images, les réveils, la puissance sur batterie.",
                "Every 10 s: the compositor’s and the shell’s CPU and memory, the frames, the wakeups, the power on battery.",
            )),
            keywords: (
                &[
                    "performances",
                    "cpu",
                    "processeur",
                    "mémoire",
                    "consommation",
                    "batterie",
                    "gourmand",
                ],
                &["performance", "cpu", "processor", "memory", "usage", "battery"],
            ),
            kind: Kind::Switch,
            places: Places::One(Place {
                key: &["perf", "record"],
                codec: Codec::Bool,
                default: Value::Bool(true),
            }),
            needs: None,
        }]
    })
}

/// The language `hocus_perf` says things in.
pub fn said_in(lang: Lang) -> text::Lang {
    match lang {
        Lang::French => text::Lang::French,
        Lang::English => text::Lang::English,
    }
}

/// The line that tells the compositor the window's page (`""`: none),
/// with its process, measured with the desktop's.
pub fn settings_line(page: &str) -> String {
    hocus_perf::record::settings_line(std::process::id(), page)
}

/// The days' folder, under the state folder.
pub fn dir(state: &Path) -> PathBuf {
    files::dir_in(state)
}

/// What the page's thread read.
#[derive(Debug, Clone)]
pub struct Read {
    pub date: Date,
    pub day: Arc<Day>,
    pub report: Arc<Report>,
    /// The days kept, oldest first.
    pub days: Vec<Date>,
    /// The seven days up to `date`, each one's report, when asked.
    pub week: Vec<(Date, Report)>,
}

/// Reads `date` in `dir`, and its week when asked: on the page's thread.
pub fn read(dir: &Path, date: Date, week: bool) -> Read {
    let day = Day::read(dir, date).unwrap_or_else(|err| {
        tracing::warn!(%err, %date, "a day of the performance does not read");
        Day::default()
    });
    let report = report(&day);
    let week = if week {
        (0..7)
            .rev()
            .filter_map(|back| date.checked_sub(jiff::Span::new().days(back)).ok())
            .map(|d| {
                (
                    d,
                    Day::read(dir, d)
                        .map(|day| report_of(&day))
                        .unwrap_or_default(),
                )
            })
            .collect()
    } else {
        Vec::new()
    };
    Read {
        date,
        day: Arc::new(day),
        report: Arc::new(report),
        days: files::days(dir),
        week,
    }
}

fn report_of(day: &Day) -> Report {
    report(day)
}

/// The day before or after `date` among those kept.
pub fn step(days: &[Date], date: Date, later: bool) -> Option<Date> {
    if later {
        days.iter().copied().find(|d| *d > date)
    } else {
        days.iter().rev().copied().find(|d| *d < date)
    }
}

/// A slice of the day on the chart.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Bucket {
    /// The compositor's and the shell's CPU, percent of one core.
    pub compositor: f32,
    pub shell: f32,
    /// Its highest sample's: the compositor's and the shell's together.
    pub highest: f32,
    /// Their memory, megabytes, at its end.
    pub compositor_mb: f32,
    pub shell_mb: f32,
    /// Frames drawn a second.
    pub fps: f32,
    /// The machine's power, watts, on battery.
    pub watts: Option<f32>,
    /// Events within it.
    pub events: u32,
    /// Something was recorded in it.
    pub measured: bool,
}

/// `day` from `from` to `to` (Unix seconds) in `n` buckets: each sample
/// in the one its middle falls in.
pub fn buckets(day: &Day, from: i64, to: i64, n: usize) -> Vec<Bucket> {
    let mut sums = vec![(0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64); n];
    let mut out = vec![Bucket::default(); n];
    let span = (to - from).max(1) as f64;
    for s in &day.samples {
        let middle = (s.from() + s.t) as f64 / 2.0;
        let at = ((middle - from as f64) / span * n as f64).floor();
        if at < 0.0 || at >= n as f64 {
            continue;
        }
        let i = at as usize;
        let (ms, comp, shell, frames, power_ms, energy, events) = &mut sums[i];
        *ms += u64::from(s.d);
        *comp += u64::from(s.cpu_ms(Who::Compositor));
        *shell += u64::from(s.cpu_ms(Who::Shell));
        *frames += u64::from(s.f);
        if let Some(mw) = s.p {
            *power_ms += u64::from(s.d);
            *energy += u64::from(mw) * u64::from(s.d);
        }
        *events += s.e.len() as u64;
        let b = &mut out[i];
        b.measured = true;
        b.highest = b.highest.max(s.desktop_percent() as f32);
        b.compositor_mb = s.rss_kb(Who::Compositor) as f32 / 1024.0;
        b.shell_mb = s.rss_kb(Who::Shell) as f32 / 1024.0;
    }
    for (b, (ms, comp, shell, frames, power_ms, energy, events)) in out.iter_mut().zip(sums) {
        if ms == 0 {
            continue;
        }
        b.compositor = comp as f32 * 100.0 / ms as f32;
        b.shell = shell as f32 * 100.0 / ms as f32;
        b.fps = frames as f32 * 1000.0 / ms as f32;
        b.watts = (power_ms > 0).then(|| energy as f32 / power_ms as f32 / 1000.0);
        b.events = u32::try_from(events).unwrap_or(u32::MAX);
    }
    out
}

/// A tile: its label, its number, the line under it and a smaller one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub label: String,
    pub value: String,
    pub line: String,
    pub sub: String,
}

/// The four tiles: CPU, memory, frames, motion.
pub fn tiles(r: &Report, lang: Lang, tz: &TimeZone) -> [Tile; 4] {
    let l = said_in(lang);
    let kb = |kb: [u32; 2]| kb[0] + kb[1];
    let mb = |kb: u32| (f64::from(kb) / 1024.0).round();
    let cpu_ms = r.cpu.compositor_ms + r.cpu.shell_ms;
    let cpu = Tile {
        label: "CPU".to_owned(),
        value: text::percent(r.cpu.average_percent, l),
        line: format!(
            "{} · {}",
            w("d’un cœur en moyenne", "of a core on average").get(lang),
            text::cpu_time(cpu_ms)
        ),
        sub: format!(
            "{} {} · shell {}",
            w("compositeur", "compositor").get(lang),
            text::cpu_time(r.cpu.compositor_ms),
            text::cpu_time(r.cpu.shell_ms)
        ),
    };
    let memory = Tile {
        label: w("Mémoire", "Memory").get(lang).to_owned(),
        value: text::megabytes(kb(r.memory.end_kb), l),
        line: match lang {
            Lang::French => format!(
                "au début {} · pic {} à {}",
                mb(kb(r.memory.start_kb)),
                mb(r.memory.peak_kb),
                text::clock(r.memory.peak_at, tz)
            ),
            Lang::English => format!(
                "at the start {} · peak {} at {}",
                mb(kb(r.memory.start_kb)),
                mb(r.memory.peak_kb),
                text::clock(r.memory.peak_at, tz)
            ),
        },
        sub: format!(
            "{} {} · shell {}",
            w("compositeur", "compositor").get(lang),
            mb(r.memory.end_kb[0]),
            mb(r.memory.end_kb[1])
        ),
    };
    let frames = Tile {
        label: w("Images", "Frames").get(lang).to_owned(),
        value: text::count(r.drawing.frames, l),
        line: if r.locked_s > 0 {
            match lang {
                Lang::French => format!(
                    "{} pendant {} verrouillé",
                    r.drawing.locked_frames,
                    text::duration(r.locked_s as i64)
                ),
                Lang::English => format!(
                    "{} in {} locked",
                    r.drawing.locked_frames,
                    text::duration(r.locked_s as i64)
                ),
            }
        } else {
            match lang {
                Lang::French => format!("{} scènes", text::count(r.drawing.scenes, l)),
                Lang::English => format!("{} scenes", text::count(r.drawing.scenes, l)),
            }
        },
        sub: match lang {
            Lang::French => format!(
                "réveils au repos : {} par seconde",
                text::number(r.drawing.rest_wakeups_per_s, 1, l)
            ),
            Lang::English => format!(
                "wakeups at rest: {} a second",
                text::number(r.drawing.rest_wakeups_per_s, 1, l)
            ),
        },
    };
    let motion = Tile {
        label: w("Mouvements", "Motion").get(lang).to_owned(),
        value: if r.moving_wallpaper_s > 0 {
            text::duration(r.moving_wallpaper_s as i64)
        } else {
            text::count(r.drawing.motion_frames, l)
        },
        line: if r.moving_wallpaper_s > 0 {
            match lang {
                Lang::French => format!(
                    "de fond animé · {} images animées",
                    text::count(r.drawing.motion_frames, l)
                ),
                Lang::English => format!(
                    "of moving wallpaper · {} frames in motion",
                    text::count(r.drawing.motion_frames, l)
                ),
            }
        } else {
            w(
                "images des animations du bureau",
                "frames of the desktop’s motions",
            )
            .get(lang)
            .to_owned()
        },
        sub: match (&r.power, lang) {
            (Some(p), Lang::French) => format!(
                "sur batterie : {} W en moyenne",
                text::number(f64::from(p.average_mw) / 1000.0, 1, l)
            ),
            (Some(p), Lang::English) => format!(
                "on battery: {} W on average",
                text::number(f64::from(p.average_mw) / 1000.0, 1, l)
            ),
            (None, _) => w(
                "sur secteur : puissance non mesurée",
                "on AC: power not measured",
            )
            .get(lang)
            .to_owned(),
        },
    };
    [cpu, memory, frames, motion]
}

/// The day's line under its date: the session's span.
pub fn session(r: &Report, lang: Lang, tz: &TimeZone) -> String {
    match (r.from, r.to) {
        (Some(from), Some(to)) => format!(
            "session {} → {}",
            text::clock(from, tz),
            text::clock(to, tz)
        ),
        _ => w("rien d’enregistré", "nothing recorded")
            .get(lang)
            .to_owned(),
    }
}

/// The date said in full, "Aujourd’hui, " before today's.
pub fn date_title(date: Date, today: Date, lang: Lang) -> String {
    let said = text::date(date, said_in(lang));
    let yesterday = today.checked_sub(jiff::Span::new().days(1)).ok();
    match (date == today, Some(date) == yesterday, lang) {
        (true, _, Lang::French) => format!("Aujourd’hui, {said}"),
        (true, _, Lang::English) => format!("Today, {said}"),
        (_, true, Lang::French) => format!("Hier, {said}"),
        (_, true, Lang::English) => format!("Yesterday, {said}"),
        _ => {
            let mut chars = said.chars();
            chars
                .next()
                .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hocus_perf::fixture;

    fn on() -> Date {
        jiff::civil::date(2026, 11, 6)
    }

    #[test]
    fn the_switch_is_perf_record_on_by_default() {
        let s = &settings()[0];
        assert_eq!(s.id, RECORD);
        let Places::One(place) = &s.places else {
            panic!("one key");
        };
        assert_eq!(place.key, ["perf", "record"]);
        assert_eq!(place.default, Value::Bool(true));
    }

    #[test]
    fn the_day_read_is_the_report_hocus_perf_day_says() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let day = fixture::working_day(on(), &TimeZone::UTC);
        std::fs::write(dir.path().join("2026-11-06"), fixture::text(&day))?;
        std::fs::write(dir.path().join("2026-11-04"), "")?;
        let read = read(dir.path(), on(), true);
        assert_eq!(*read.report, report(&day));
        assert_eq!(read.days, [jiff::civil::date(2026, 11, 4), on()]);
        assert_eq!(read.week.len(), 7);
        assert_eq!(read.week[6].1, report(&day));
        assert_eq!(read.week[0].1, Report::default());
        Ok(())
    }

    #[test]
    fn the_day_picker_goes_through_the_days_kept() {
        let d = |n| jiff::civil::date(2026, 11, n);
        let days = [d(1), d(4), d(6)];
        assert_eq!(step(&days, d(6), false), Some(d(4)));
        assert_eq!(step(&days, d(4), false), Some(d(1)));
        assert_eq!(step(&days, d(1), false), None);
        assert_eq!(step(&days, d(4), true), Some(d(6)));
        assert_eq!(step(&days, d(6), true), None);
        // Today, not recorded yet: back to the last day kept.
        assert_eq!(step(&days, d(7), false), Some(d(6)));
    }

    #[test]
    fn buckets_average_each_slice_of_the_day() {
        let day = fixture::working_day(on(), &TimeZone::UTC);
        let r = report(&day);
        let (from, to) = (r.from.unwrap_or(0), r.to.unwrap_or(0));
        // Ten hours in 120 buckets of 5 min.
        let b = buckets(&day, from, to, 120);
        assert!(b.iter().all(|b| b.measured));
        // 12:20–13:25 locked: nothing drawn.
        let locked = &b[(12 * 60 + 30 - 8 * 60 - 30) / 5];
        assert_eq!(locked.fps, 0.0);
        assert!((locked.compositor - 0.08).abs() < 1e-4, "{locked:?}");
        // 15:10–15:25: 9 frames a second.
        let rest = &b[(15 * 60 + 15 - 8 * 60 - 30) / 5];
        assert!((rest.fps - 9.0).abs() < 1e-4, "{rest:?}");
        // On battery at the start, on AC at noon.
        assert!(b[0].watts.is_some() && locked.watts.is_none());
        // The whole day's CPU, bucket by bucket, is the report's.
        let total: f32 = b
            .iter()
            .map(|b| (b.compositor + b.shell) * 300.0 * 10.0)
            .sum();
        let said = (r.cpu.compositor_ms + r.cpu.shell_ms) as f32;
        assert!((total - said).abs() / said < 1e-3, "{total} {said}");
    }

    #[test]
    fn the_tiles_of_the_invented_day() {
        let day = fixture::working_day(on(), &TimeZone::UTC);
        let r = report(&day);
        let [cpu, memory, frames, motion] = tiles(&r, Lang::French, &TimeZone::UTC);
        assert_eq!(cpu.value, "2,2 %");
        assert_eq!(cpu.sub, "compositeur 8 min 19 s · shell 4 min 37 s");
        assert_eq!(memory.value, "249 Mo");
        assert_eq!(memory.line, "au début 183 · pic 266 à 16:49");
        assert_eq!(frames.line, "0 pendant 1 h 05 verrouillé");
        assert_eq!(motion.value, "41 min");
        assert_eq!(motion.sub, "sur batterie : 6,9 W en moyenne");
        assert_eq!(
            date_title(on(), jiff::civil::date(2026, 11, 7), Lang::French),
            "Hier, vendredi 6 novembre"
        );
        assert_eq!(
            session(&r, Lang::French, &TimeZone::UTC),
            "session 08:30 → 18:30"
        );
    }
}
