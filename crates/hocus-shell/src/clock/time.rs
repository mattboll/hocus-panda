//! What the clock shows, and when it looks again (bar.md §4 group 1).
//!
//! The time the user reads is the wall clock in the system's time zone;
//! when to look again is on the monotonic clock: the next look is a timer
//! set for just past the next wall-clock second, never polling. A clock set
//! forward or back, a suspend or a time zone changed are all seen at that
//! next look, within a second, since each look reads the wall clock afresh
//! and aligns the next one to it.

use std::{path::Path, time::Duration};

use jiff::{SignedDuration, Timestamp, Zoned, civil::Weekday, tz::TimeZone};

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
    /// From `LC_ALL`, `LC_TIME` and `LANG`, the first one set, as the C
    /// library takes them; English unless French.
    pub fn from_env() -> Self {
        let locale = ["LC_ALL", "LC_TIME", "LANG"]
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
    /// `HH:MM`, 24 hours, leading zeros.
    pub hm: [char; 5],
    /// The seconds, leading zero.
    pub ss: [char; 2],
    /// "Wednesday 28 October", "mercredi 28 octobre".
    pub date: String,
}

impl Shown {
    pub fn at(time: &Zoned, lang: Lang) -> Self {
        let digits = |n: i8| {
            let n = n.unsigned_abs();
            [char::from(b'0' + n / 10 % 10), char::from(b'0' + n % 10)]
        };
        let [h1, h2] = digits(time.hour());
        let [m1, m2] = digits(time.minute());
        Self {
            hm: [h1, h2, ':', m1, m2],
            ss: digits(time.second()),
            date: format!(
                "{} {} {}",
                lang.weekday(time.weekday()),
                time.day(),
                lang.month(time.month())
            ),
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
        Shown::at(&time, lang)
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
        let now = Shown::at(&wall(SignedDuration::ZERO).now(), Lang::English);
        let later = Shown::at(&wall(SignedDuration::from_hours(3)).now(), Lang::English);
        assert_ne!(now.hm, later.hm);
        let back = Shown::at(&wall(SignedDuration::from_hours(-3)).now(), Lang::English);
        assert_ne!(now.hm, back.hm);
    }
}
