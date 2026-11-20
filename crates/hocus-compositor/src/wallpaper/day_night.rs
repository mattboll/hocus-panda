//! Day and night's pictures (#894): which one shows at a time, and when
//! the other takes its place. At the sunrise and sunset of the
//! weather's place (`[weather]`, computed by [`hocus_link::sun`]), else
//! at `[wallpaper] day-starts` and `night-starts`, on the wall clock:
//! the hours the user reads.

use hocus_link::sun::{Sun, sun};
use jiff::{SignedDuration, Timestamp, Zoned, civil::Date};

/// Which of the two shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Day,
    Night,
}

/// What decides the switches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Switches {
    /// The sun at this latitude and longitude.
    Sun(f64, f64),
    /// These minutes after midnight, local: the day's start, the night's.
    Hours(u16, u16),
}

/// The switches of `date`, in their order: when, and what shows after.
/// None on a day the sun neither rises nor sets: what shows all day.
fn switches(date: Date, now: &Zoned, by: Switches) -> Result<Vec<(Timestamp, Phase)>, Phase> {
    match by {
        Switches::Sun(lat, lon) => {
            let day = u16::try_from(date.day_of_year()).unwrap_or(1);
            let midnight = date
                .to_zoned(jiff::tz::TimeZone::UTC)
                .map(|z| z.timestamp())
                .unwrap_or(Timestamp::UNIX_EPOCH);
            let at = |minutes: f64| {
                midnight
                    .checked_add(SignedDuration::from_secs((minutes * 60.0).round() as i64))
                    .unwrap_or(midnight)
            };
            match sun(day, lat, lon) {
                Sun::Rises { rise, set } => {
                    Ok(vec![(at(rise), Phase::Day), (at(set), Phase::Night)])
                }
                Sun::AlwaysUp => Err(Phase::Day),
                Sun::AlwaysDown => Err(Phase::Night),
            }
        }
        Switches::Hours(day_starts, night_starts) => {
            let at = |minutes: u16| {
                let time = jiff::civil::time(
                    i8::try_from(minutes / 60).unwrap_or(0),
                    i8::try_from(minutes % 60).unwrap_or(0),
                    0,
                    0,
                );
                date.to_datetime(time)
                    .to_zoned(now.time_zone().clone())
                    .map(|z| z.timestamp())
                    .unwrap_or(Timestamp::UNIX_EPOCH)
            };
            let mut list = vec![
                (at(day_starts), Phase::Day),
                (at(night_starts), Phase::Night),
            ];
            list.sort_by_key(|(t, _)| *t);
            Ok(list)
        }
    }
}

/// What shows at `now`, and when the next switch comes (else the next
/// midnight, to look again).
pub fn phase_at(now: &Zoned, by: Switches) -> (Phase, Timestamp) {
    let today = now.date();
    let days = [
        today.yesterday().unwrap_or(today),
        today,
        today.tomorrow().unwrap_or(today),
    ];
    let mut all = Vec::new();
    let mut polar = None;
    for date in days {
        match switches(date, now, by) {
            Ok(list) => all.extend(list),
            Err(phase) if date == today => polar = Some(phase),
            Err(_) => {}
        }
    }
    all.sort_by_key(|(t, _)| *t);
    let t = now.timestamp();
    let midnight = today
        .tomorrow()
        .ok()
        .and_then(|d| d.to_zoned(now.time_zone().clone()).ok())
        .map_or(t, |z| z.timestamp());
    let next = all
        .iter()
        .find(|(at, _)| *at > t)
        .map_or(midnight, |(at, _)| *at);
    let phase = match polar {
        Some(phase) => phase,
        None => all
            .iter()
            .rev()
            .find(|(at, _)| *at <= t)
            .map_or(Phase::Night, |(_, p)| *p),
    };
    (phase, next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> Zoned {
        text.parse::<Timestamp>()
            .expect("a time")
            .to_zoned(jiff::tz::TimeZone::UTC)
    }

    #[test]
    fn without_a_place_the_hours_decide() {
        let hours = Switches::Hours(7 * 60, 20 * 60);
        assert_eq!(
            phase_at(&at("2026-11-20T06:00:00Z"), hours),
            (
                Phase::Night,
                "2026-11-20T07:00:00Z".parse().expect("a time")
            )
        );
        assert_eq!(
            phase_at(&at("2026-11-20T12:00:00Z"), hours),
            (Phase::Day, "2026-11-20T20:00:00Z".parse().expect("a time"))
        );
        assert_eq!(
            phase_at(&at("2026-11-20T21:30:00Z"), hours),
            (
                Phase::Night,
                "2026-11-21T07:00:00Z".parse().expect("a time")
            )
        );
    }

    #[test]
    fn with_a_place_the_sun_decides() {
        let paris = Switches::Sun(48.8566, 2.3522);
        // 21 June, noon UTC: day, until sunset about 19:58 UTC.
        let (phase, next) = phase_at(&at("2026-06-21T12:00:00Z"), paris);
        assert_eq!(phase, Phase::Day);
        let sunset: Timestamp = "2026-06-21T19:58:00Z".parse().expect("a time");
        assert!(
            next.duration_since(sunset).abs() < SignedDuration::from_mins(4),
            "{next}"
        );
        // Before sunrise: night, until about 03:47 UTC.
        let (phase, next) = phase_at(&at("2026-06-21T02:00:00Z"), paris);
        assert_eq!(phase, Phase::Night);
        let sunrise: Timestamp = "2026-06-21T03:47:00Z".parse().expect("a time");
        assert!(
            next.duration_since(sunrise).abs() < SignedDuration::from_mins(4),
            "{next}"
        );
        // A polar night: night all day, looked at again at midnight.
        let (phase, next) = phase_at(&at("2026-12-21T12:00:00Z"), Switches::Sun(69.65, 18.96));
        assert_eq!(phase, Phase::Night);
        assert_eq!(
            next,
            "2026-12-22T00:00:00Z".parse::<Timestamp>().expect("a time")
        );
    }
}
