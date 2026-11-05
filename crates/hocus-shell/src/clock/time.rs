//! What the clock shows, and when it looks again (bar.md §4 group 1).
//!
//! The time the user reads is the wall clock in the system's time zone;
//! when to look again is on the monotonic clock: the next look is a timer
//! set for just past the next wall-clock second, never polling. A clock set
//! forward or back, a suspend or a time zone changed are all seen at that
//! next look, within a second, since each look reads the wall clock afresh
//! and aligns the next one to it.

use std::{path::Path, time::Duration};

use hocus_link::locale::DateFormat;
use jiff::{SignedDuration, Timestamp, Zoned, civil::Weekday, tz::TimeZone};

use crate::popover::calendar::words::Hours;

/// Past the second's start: a timer may fire a little early, and a look
/// just before the second would show the one before and look again at once.
const PAST_THE_SECOND: Duration = Duration::from_millis(5);

/// The language of the date, from the session's locale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    English,
    French,
}

impl Lang {
    /// `[locale] language` when the file chooses one (#402), else from
    /// `LC_ALL`, `LC_TIME` and `LANG`, the first one set, as the C library
    /// takes them; English unless French.
    pub fn from_env() -> Self {
        crate::locale::chosen_lang()
            .unwrap_or_else(|| Self::from_vars(["LC_ALL", "LC_TIME", "LANG"]))
    }

    /// The messages' language: `[locale] language`, else `LC_ALL`,
    /// `LC_MESSAGES`, then `LANG`.
    pub fn of_messages() -> Self {
        crate::locale::chosen_lang()
            .unwrap_or_else(|| Self::from_vars(["LC_ALL", "LC_MESSAGES", "LANG"]))
    }

    /// How numbers are written: `[locale] language`, else `LC_ALL`,
    /// `LC_NUMERIC`, then `LANG`.
    pub fn of_numbers() -> Self {
        crate::locale::chosen_lang()
            .unwrap_or_else(|| Self::from_vars(["LC_ALL", "LC_NUMERIC", "LANG"]))
    }

    fn from_vars(names: [&str; 3]) -> Self {
        let locale = names
            .into_iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|value| !value.is_empty())
            .unwrap_or_default();
        Self::of(&locale)
    }

    /// From a locale's name, `fr_FR.UTF-8`.
    pub fn of(locale: &str) -> Self {
        if locale.starts_with("fr") {
            Self::French
        } else {
            Self::English
        }
    }

    /// The weekday and the month, short: "jeu.", "nov."; "Thu", "Nov".
    fn short_weekday(self, day: Weekday) -> &'static str {
        let i = usize::from(day.to_monday_zero_offset().unsigned_abs());
        match self {
            Self::English => ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][i],
            Self::French => ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."][i],
        }
    }

    fn short_month(self, month: i8) -> &'static str {
        let i = usize::from(month.unsigned_abs().clamp(1, 12)) - 1;
        match self {
            Self::English => [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ][i],
            Self::French => [
                "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.",
                "nov.", "déc.",
            ][i],
        }
    }

    /// `date` as `format` writes it: "jeudi 5 novembre", "jeu. 5 nov.",
    /// "05/11/2026", "2026-11-05".
    pub fn date(self, date: jiff::civil::Date, format: DateFormat) -> String {
        match format {
            DateFormat::Long => format!(
                "{} {} {}",
                self.weekday(date.weekday()),
                date.day(),
                self.month(date.month())
            ),
            DateFormat::Short => format!(
                "{} {} {}",
                self.short_weekday(date.weekday()),
                date.day(),
                self.short_month(date.month())
            ),
            DateFormat::Numeric => {
                format!("{:02}/{:02}/{}", date.day(), date.month(), date.year())
            }
            DateFormat::Iso => format!("{}-{:02}-{:02}", date.year(), date.month(), date.day()),
        }
    }

    fn weekday(self, day: Weekday) -> &'static str {
        let i = usize::from(day.to_monday_zero_offset().unsigned_abs());
        match self {
            Self::English => [
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
                "Sunday",
            ][i],
            Self::French => [
                "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
            ][i],
        }
    }

    fn month(self, month: i8) -> &'static str {
        let i = usize::from(month.unsigned_abs().clamp(1, 12)) - 1;
        match self {
            Self::English => [
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
            ][i],
            Self::French => [
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
            ][i],
        }
    }
}

/// What the clock shows at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    /// `HH:MM`, leading zeros over 24 hours; over 12, the hour's first
    /// digit a space when it has one digit (` 2:25`), drawn as nothing.
    pub hm: [char; 5],
    /// The seconds, leading zero.
    pub ss: [char; 2],
    /// Over 12 hours, "AM" or "PM": the bar shows it in the seconds'
    /// place.
    pub half: Option<&'static str>,
    /// "Wednesday 28 October", "mercredi 28 octobre", or as `[locale]
    /// date` says.
    pub date: String,
}

impl Shown {
    /// As the desktop's formats say ([`crate::locale`]).
    pub fn at(time: &Zoned, lang: Lang) -> Self {
        Self::with(time, lang, Hours::from_env(), crate::locale::date_format())
    }

    pub fn with(time: &Zoned, lang: Lang, hours: Hours, date: DateFormat) -> Self {
        let digits = |n: i8| {
            let n = n.unsigned_abs();
            [char::from(b'0' + n / 10 % 10), char::from(b'0' + n % 10)]
        };
        let (hour, half) = match hours {
            Hours::TwentyFour => (time.hour(), None),
            Hours::Twelve => match time.hour() {
                0 => (12, Some("AM")),
                h @ 1..=11 => (h, Some("AM")),
                12 => (12, Some("PM")),
                h => (h - 12, Some("PM")),
            },
        };
        let [mut h1, h2] = digits(hour);
        if half.is_some() && h1 == '0' {
            h1 = ' ';
        }
        let [m1, m2] = digits(time.minute());
        Self {
            hm: [h1, h2, ':', m1, m2],
            ss: digits(time.second()),
            half,
            date: lang.date(time.date(), date),
        }
    }

    /// The time as written: "14:25", "2:25 PM".
    pub fn time(&self) -> String {
        let hm: String = self.hm.iter().collect();
        match self.half {
            Some(half) => format!("{} {half}", hm.trim_start()),
            None => hm,
        }
    }
}

/// The wall clock as the clock reads it: the system's time zone, and, for
/// tests and recordings, a shift from the real time.
#[derive(Debug, Clone)]
pub struct Wall {
    pub zone: TimeZone,
    pub shift: SignedDuration,
}

impl Wall {
    /// The time now, in the zone.
    pub fn now(&self) -> Zoned {
        let now = Timestamp::now();
        now.checked_add(self.shift)
            .unwrap_or(now)
            .to_zoned(self.zone.clone())
    }

    /// The shift that makes the clock read `at` now: `HOCUS_CLOCK_AT`, a
    /// local date and time (`2026-10-28T09:59:57`) or a time of today
    /// (`09:59:57`), for recordings and tests; the system's clock is never
    /// touched.
    pub fn shift_to(at: &str, zone: &TimeZone) -> Option<SignedDuration> {
        let now = Timestamp::now().to_zoned(zone.clone());
        let civil = match at.parse::<jiff::civil::DateTime>() {
            Ok(datetime) => datetime,
            Err(_) => now
                .date()
                .to_datetime(at.parse::<jiff::civil::Time>().ok()?),
        };
        let target = civil.to_zoned(zone.clone()).ok()?;
        Some(target.timestamp().duration_since(now.timestamp()))
    }
}

/// How long until just past the next wall-clock second, from a time
/// `subsec` past a whole second.
pub fn until_next_second(subsec: Duration) -> Duration {
    Duration::from_secs(1).saturating_sub(subsec) + PAST_THE_SECOND
}

/// The time past the whole second of `time`.
pub fn subsec(time: &Zoned) -> Duration {
    Duration::from_nanos(u64::from(time.subsec_nanosecond().unsigned_abs()))
}

/// The system's time zone, read afresh: `TZ` when set, else
/// `/etc/localtime`, else UTC. jiff's own keeps it for minutes; a zone
/// changed by the user shows at the next look instead.
pub fn system_zone() -> TimeZone {
    if let Ok(tz) = std::env::var("TZ")
        && !tz.is_empty()
    {
        let name = tz.trim_start_matches(':');
        let zone = if name.starts_with('/') {
            std::fs::read(name)
                .ok()
                .and_then(|data| TimeZone::tzif(name, &data).ok())
        } else {
            TimeZone::get(name)
                .ok()
                .or_else(|| TimeZone::posix(name).ok())
        };
        if let Some(zone) = zone {
            return zone;
        }
        tracing::warn!(tz, "TZ names no time zone: the system's");
    }
    zone_of(Path::new(LOCALTIME)).unwrap_or_else(|| {
        tracing::warn!("no time zone in {LOCALTIME}: UTC");
        TimeZone::UTC
    })
}

pub const LOCALTIME: &str = "/etc/localtime";

/// The zone in a TZif file such as `/etc/localtime`, named after where its
/// link points (`Europe/Paris`).
fn zone_of(file: &Path) -> Option<TimeZone> {
    let data = std::fs::read(file).ok()?;
    let target = std::fs::read_link(file).ok();
    let name = target
        .as_ref()
        .and_then(|t| t.to_str())
        .and_then(|t| t.split_once("zoneinfo/").map(|(_, name)| name))
        .unwrap_or("Local");
    TimeZone::tzif(name, &data).ok()
}

/// Watches `/etc/localtime`, which `timedatectl` and others replace or
/// rewrite when the time zone changes, and hands each new zone to
/// `changed` until it returns false. Blocks: for a thread of its own. With
/// `TZ` set, the zone is the environment's and never changes.
pub fn watch_zone(mut changed: impl FnMut(TimeZone) -> bool) {
    use rustix::{
        fs::inotify::{self, CreateFlags, WatchFlags},
        io::Errno,
    };
    if std::env::var_os("TZ").is_some_and(|tz| !tz.is_empty()) {
        return;
    }
    let dir = Path::new(LOCALTIME).parent().unwrap_or(Path::new("/etc"));
    let watched = inotify::init(CreateFlags::CLOEXEC).and_then(|fd| {
        let flags = WatchFlags::CREATE
            | WatchFlags::MOVED_TO
            | WatchFlags::CLOSE_WRITE
            | WatchFlags::DELETE;
        inotify::add_watch(&fd, dir, flags).map(|_| fd)
    });
    let fd = match watched {
        Ok(fd) => fd,
        Err(err) => {
            tracing::warn!(%err, "cannot watch the time zone: it is read at start only");
            return;
        }
    };
    let name = Path::new(LOCALTIME)
        .file_name()
        .map(|n| n.as_encoded_bytes());
    let mut buffer = [std::mem::MaybeUninit::uninit(); 1024];
    let mut events = inotify::Reader::new(&fd, &mut buffer);
    loop {
        match events.next() {
            Ok(event) => {
                if event.file_name().map(|n| n.to_bytes()) == name && !changed(system_zone()) {
                    return;
                }
            }
            Err(Errno::INTR) => {}
            Err(err) => {
                tracing::warn!(%err, "stopped watching the time zone");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paris's rules, without the system's database.
    fn paris() -> TimeZone {
        TimeZone::posix("CET-1CEST,M3.5.0,M10.5.0/3").expect("a POSIX zone")
    }

    fn shown(at: &str, lang: Lang) -> Shown {
        let time = at
            .parse::<Timestamp>()
            .expect("a timestamp")
            .to_zoned(paris());
        Shown::with(&time, lang, Hours::TwentyFour, DateFormat::Long)
    }

    #[test]
    fn over_12_hours_the_hour_has_no_leading_zero_and_says_am_or_pm() {
        let time = |at: &str| {
            at.parse::<Timestamp>()
                .expect("a timestamp")
                .to_zoned(paris())
        };
        let twelve = |at| Shown::with(&time(at), Lang::English, Hours::Twelve, DateFormat::Long);
        let s = twelve("2026-11-05T13:25:00Z");
        assert_eq!(s.hm, [' ', '2', ':', '2', '5']);
        assert_eq!((s.half, s.time().as_str()), (Some("PM"), "2:25 PM"));
        assert_eq!(twelve("2026-11-05T23:05:00Z").time(), "12:05 AM");
        assert_eq!(twelve("2026-11-05T11:59:00Z").time(), "12:59 PM");
        assert_eq!(twelve("2026-11-05T09:00:00Z").time(), "10:00 AM");
        let s = Shown::with(
            &time("2026-11-05T13:25:00Z"),
            Lang::French,
            Hours::TwentyFour,
            DateFormat::Long,
        );
        assert_eq!((s.half, s.time().as_str()), (None, "14:25"));
    }

    #[test]
    fn the_date_is_written_in_each_format() {
        let day = jiff::civil::date(2026, 11, 5);
        assert_eq!(Lang::French.date(day, DateFormat::Long), "jeudi 5 novembre");
        assert_eq!(
            Lang::English.date(day, DateFormat::Long),
            "Thursday 5 November"
        );
        assert_eq!(Lang::French.date(day, DateFormat::Short), "jeu. 5 nov.");
        assert_eq!(Lang::English.date(day, DateFormat::Short), "Thu 5 Nov");
        assert_eq!(Lang::French.date(day, DateFormat::Numeric), "05/11/2026");
        assert_eq!(Lang::English.date(day, DateFormat::Iso), "2026-11-05");
    }

    #[test]
    fn hours_and_minutes_take_24_hours_and_leading_zeros() {
        let s = shown("2026-10-28T08:05:07Z", Lang::English);
        assert_eq!(s.hm, ['0', '9', ':', '0', '5']);
        assert_eq!(s.ss, ['0', '7']);
        let s = shown("2026-10-28T22:59:59Z", Lang::English);
        assert_eq!(s.hm, ['2', '3', ':', '5', '9']);
        let s = shown("2026-10-28T23:00:00Z", Lang::English);
        assert_eq!(s.hm, ['0', '0', ':', '0', '0']);
        assert_eq!(s.ss, ['0', '0']);
    }

    #[test]
    fn the_date_reads_in_english_and_in_french() {
        let at = "2026-10-28T10:00:00Z";
        assert_eq!(shown(at, Lang::English).date, "Wednesday 28 October");
        assert_eq!(shown(at, Lang::French).date, "mercredi 28 octobre");
        let at = "2027-02-01T10:00:00Z";
        assert_eq!(shown(at, Lang::French).date, "lundi 1 février");
        assert_eq!(shown(at, Lang::English).date, "Monday 1 February");
        // Past local midnight, the local day.
        assert_eq!(
            shown("2026-12-31T23:30:00Z", Lang::French).date,
            "vendredi 1 janvier"
        );
        assert_eq!(Lang::of("fr_FR.UTF-8"), Lang::French);
        assert_eq!(Lang::of("fr_BE"), Lang::French);
        assert_eq!(Lang::of("en_GB.UTF-8"), Lang::English);
        assert_eq!(Lang::of("C.UTF-8"), Lang::English);
        assert_eq!(Lang::of(""), Lang::English);
    }

    #[test]
    fn summer_time_ends_and_starts_on_the_hour() {
        // 25 October 2026, 3:00 summer time becomes 2:00 again.
        let before = shown("2026-10-25T00:59:59Z", Lang::English);
        let after = shown("2026-10-25T01:00:00Z", Lang::English);
        assert_eq!(before.hm, ['0', '2', ':', '5', '9']);
        assert_eq!(after.hm, ['0', '2', ':', '0', '0']);
        // 29 March 2026, 2:00 becomes 3:00.
        let before = shown("2026-03-29T00:59:59Z", Lang::English);
        let after = shown("2026-03-29T01:00:00Z", Lang::English);
        assert_eq!(before.hm, ['0', '1', ':', '5', '9']);
        assert_eq!(after.hm, ['0', '3', ':', '0', '0']);
    }

    #[test]
    fn the_next_look_is_just_past_the_next_second() {
        let ms = Duration::from_millis;
        assert_eq!(until_next_second(ms(0)), ms(1005));
        assert_eq!(until_next_second(ms(400)), ms(605));
        assert_eq!(until_next_second(ms(999)), ms(6));
        // A look a little late, or at a whole second, never waits more
        // than a second.
        assert!(until_next_second(ms(1000)) <= ms(1005));
        // Read at a timer's end, it is past the second it aimed at.
        let time = "2026-10-28T10:00:00.005Z"
            .parse::<Timestamp>()
            .expect("a timestamp")
            .to_zoned(paris());
        assert_eq!(subsec(&time), ms(5));
        assert_eq!(until_next_second(subsec(&time)), ms(1000));
    }

    #[test]
    fn a_shift_makes_the_clock_read_the_time_asked_for() {
        let wall = Wall {
            zone: paris(),
            shift: Wall::shift_to("2026-10-28T09:59:57", &paris()).expect("a shift"),
        };
        let now = wall.now();
        assert_eq!((now.hour(), now.minute()), (9, 59));
        assert!((57..=58).contains(&now.second()), "{now}");
        let today = Wall::shift_to("23:59:58", &paris()).expect("a shift");
        let now = Wall {
            zone: paris(),
            shift: today,
        }
        .now();
        assert_eq!((now.hour(), now.minute()), (23, 59));
        assert!(Wall::shift_to("teatime", &paris()).is_none());
    }

    #[test]
    fn a_clock_set_back_or_forward_shows_at_the_next_look() {
        // What the clock shows depends on the wall time alone: a jump is
        // the next look's time.
        let wall = |shift| Wall {
            zone: paris(),
            shift,
        };
        let at =
            |w: Wall| Shown::with(&w.now(), Lang::English, Hours::TwentyFour, DateFormat::Long);
        let now = at(wall(SignedDuration::ZERO));
        let later = at(wall(SignedDuration::from_hours(3)));
        assert_ne!(now.hm, later.hm);
        let back = at(wall(SignedDuration::from_hours(-3)));
        assert_ne!(now.hm, back.hm);
    }
}
