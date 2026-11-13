//! Where the sun is, for the effects that follow it: the evening
//! fireflies (#804) and the daylight (#805). The place is the system's
//! time zone's (its line in `zone1970.tab`; without one, 45° north at the
//! longitude of its offset from UTC), the time the wall clock's: the sun
//! the user sees.

use std::{
    collections::HashMap,
    f64::consts::PI,
    sync::{Mutex, OnceLock},
};

/// How often the sun is looked at.
const EVERY: std::time::Duration = std::time::Duration::from_secs(60);

/// The sun's upper edge on the horizon, refraction counted: its elevation,
/// degrees.
const HORIZON: f64 = -0.833;

/// Whether the sun is down now where the system's time zone is.
pub fn sun_down_now() -> bool {
    let now = jiff::Zoned::now();
    let (lat, lon) = place(&now);
    sun_down(lat, lon, now.timestamp())
}

/// A place: latitude and longitude, degrees; `None`, unknown.
type Place = Option<(f64, f64)>;

/// Where the time zone of `now` is: its line's place in `zone1970.tab`,
/// else 45° north at the longitude of its offset (15° an hour). Read once
/// a zone.
fn place(now: &jiff::Zoned) -> (f64, f64) {
    static PLACES: OnceLock<Mutex<HashMap<String, Place>>> = OnceLock::new();
    let zone = now.time_zone().iana_name().unwrap_or_default().to_owned();
    let known = PLACES
        .get_or_init(Default::default)
        .lock()
        .ok()
        .and_then(|mut places| {
            *places.entry(zone.clone()).or_insert_with(|| {
                [
                    "/usr/share/zoneinfo/zone1970.tab",
                    "/usr/share/zoneinfo/zone.tab",
                ]
                .iter()
                .filter_map(|f| std::fs::read_to_string(f).ok())
                .find_map(|tab| zone_place(&tab, &zone))
            })
        });
    known.unwrap_or_else(|| (45.0, f64::from(now.offset().seconds()) / 240.0))
}

/// `zone`'s place in a `zone.tab`: latitude and longitude, degrees.
fn zone_place(tab: &str, zone: &str) -> Option<(f64, f64)> {
    tab.lines()
        .filter(|l| !l.starts_with('#'))
        .find_map(|line| {
            let mut fields = line.split('\t');
            let at = fields.nth(1)?;
            (fields.next()? == zone).then_some(at)
        })
        .and_then(coordinates)
}

/// ISO 6709 as `zone.tab` writes it: `±DDMM±DDDMM` or `±DDMMSS±DDDMMSS`.
fn coordinates(at: &str) -> Option<(f64, f64)> {
    let split = at.get(1..)?.find(['+', '-'])? + 1;
    let angle = |s: &str, degrees: usize| -> Option<f64> {
        let sign = if s.starts_with('-') { -1.0 } else { 1.0 };
        let digits = s.get(1..)?;
        let d: f64 = digits.get(..degrees)?.parse().ok()?;
        let m: f64 = digits.get(degrees..degrees + 2)?.parse().ok()?;
        let sec: f64 = digits
            .get(degrees + 2..)
            .filter(|s| !s.is_empty())
            .map_or(Some(0.0), |s| s.parse().ok())?;
        Some(sign * (d + m / 60.0 + sec / 3600.0))
    };
    Some((angle(at.get(..split)?, 2)?, angle(at.get(split..)?, 3)?))
}

/// The sun's elevation above the horizon, degrees, at latitude `lat`,
/// longitude `lon` at `when`: NOAA's equations of the sun's place.
pub fn elevation(lat: f64, lon: f64, when: jiff::Timestamp) -> f64 {
    let utc = when.to_zoned(jiff::tz::TimeZone::UTC);
    let hour =
        f64::from(utc.hour()) + f64::from(utc.minute()) / 60.0 + f64::from(utc.second()) / 3600.0;
    let g = 2.0 * PI / 365.0 * (f64::from(utc.day_of_year()) - 1.0 + (hour - 12.0) / 24.0);
    let eqtime = 229.18
        * (0.000075 + 0.001868 * g.cos()
            - 0.032077 * g.sin()
            - 0.014615 * (2.0 * g).cos()
            - 0.040849 * (2.0 * g).sin());
    let decl = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin() - 0.006758 * (2.0 * g).cos()
        + 0.000907 * (2.0 * g).sin()
        - 0.002697 * (3.0 * g).cos()
        + 0.00148 * (3.0 * g).sin();
    let solar_minutes = hour * 60.0 + eqtime + 4.0 * lon;
    let ha = (solar_minutes / 4.0 - 180.0).to_radians();
    let lat = lat.to_radians();
    let cos_zenith = lat.sin() * decl.sin() + lat.cos() * decl.cos() * ha.cos();
    90.0 - cos_zenith.clamp(-1.0, 1.0).acos().to_degrees()
}

/// Whether the sun is below the horizon (its upper edge, refraction
/// counted) at latitude `lat`, longitude `lon` at `when`.
pub fn sun_down(lat: f64, lon: f64, when: jiff::Timestamp) -> bool {
    elevation(lat, lon, when) < HORIZON
}

/// The board "La lumière du jour"'s hour now where the system's time zone
/// is (#805).
pub fn board_hour_now() -> f64 {
    board_hour_at(jiff::Timestamp::now())
}

/// The same at `when`.
pub fn board_hour_at(when: jiff::Timestamp) -> f64 {
    let (lat, lon) = place(&when.to_zoned(jiff::tz::TimeZone::system()));
    board_hour(lat, lon, when)
}

/// The hour of the board "La lumière du jour" (#805) that the real sun
/// stands for at latitude `lat`, longitude `lon` at `when`. The board's
/// sun rises at 6 and sets at 20, its noon at 13: the day between the real
/// sunrise and sunset is laid over 6 to 20, the night between sunset and
/// the next sunrise over 20 to 30 (6 the next day), past 24 taken from 24.
/// With no sunrise or sunset within a day (near the poles), the board's
/// noon or its midnight's 1 o'clock.
pub fn board_hour(lat: f64, lon: f64, when: jiff::Timestamp) -> f64 {
    let up = |t: jiff::Timestamp| elevation(lat, lon, t) >= HORIZON;
    let day = up(when);
    let (Some(before), Some(after)) = (crossing(&up, when, -1), crossing(&up, when, 1)) else {
        return if day { 13.0 } else { 1.0 };
    };
    let span = after.duration_since(before).as_secs_f64().max(1.0);
    let k = (when.duration_since(before).as_secs_f64() / span).clamp(0.0, 1.0);
    if day {
        6.0 + 14.0 * k
    } else {
        (20.0 + 10.0 * k) % 24.0
    }
}

/// When the sun last rose or set before `when` (`dir` -1), or will next
/// (`dir` 1), within a day; to the minute.
fn crossing(
    up: &impl Fn(jiff::Timestamp) -> bool,
    when: jiff::Timestamp,
    dir: i64,
) -> Option<jiff::Timestamp> {
    const STEP: i64 = 10 * 60;
    let at = |s: i64| when.checked_add(jiff::SignedDuration::from_secs(s)).ok();
    let now = up(when);
    let mut near = 0;
    for i in 1..=(26 * 6) {
        let far = dir * i * STEP;
        if up(at(far)?) != now {
            // Between `near` and `far`: halved down to a minute.
            let (mut a, mut b) = (near, far);
            while (b - a).abs() > 60 {
                let m = (a + b) / 2;
                if up(at(m)?) == now {
                    a = m;
                } else {
                    b = m;
                }
            }
            return at(b);
        }
        near = far;
    }
    None
}

impl crate::State {
    /// While the fireflies (#804) or the daylight (#805) are on, the sun
    /// is looked at every minute on the wall clock; both off, not at all.
    pub(crate) fn watch_sun(&mut self) {
        use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
        let on = self.config.effects.fireflies || self.config.effects.daylight;
        match (on, self.fx.sun_timer.is_some()) {
            (true, false) => {
                self.fx.dusk.evening = sun_down_now();
                tracing::debug!(evening = self.fx.dusk.evening, "looking at the sun");
                let inserted =
                    self.loop_handle
                        .insert_source(Timer::from_duration(EVERY), |_, _, app| {
                            app.state.sun_look();
                            TimeoutAction::ToDuration(EVERY)
                        });
                match inserted {
                    Ok(token) => self.fx.sun_timer = Some(token),
                    Err(err) => {
                        tracing::warn!(%err.error, "the effects will not follow the sun");
                    }
                }
            }
            (false, true) => {
                if let Some(token) = self.fx.sun_timer.take() {
                    self.loop_handle.remove(token);
                }
            }
            _ => {}
        }
    }

    /// Where the sun is now: the fireflies' evening, the daylight's hour.
    fn sun_look(&mut self) {
        self.dusk_look();
        let hour = self.sun_hour();
        if let Some(daylight) = self.fx.daylight.as_mut()
            && daylight.set(hour)
        {
            tracing::debug!(hour, "the daylight's hour");
            self.redraw = true;
        }
    }

    /// Tests: the sun as it is at `at`, whatever the clock says.
    pub(crate) fn hold_sun(&mut self, at: jiff::Timestamp) {
        self.fx.sun_held = Some(at);
        let hour = self.sun_hour();
        if let Some(daylight) = self.fx.daylight.as_mut() {
            daylight.set(hour);
        }
        self.redraw = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sun_sets_in_tours_as_the_board_says() {
        // Tours (47°23′N 0°41′E), 13 November: the sun sets near 17:15
        // local, 16:15 UTC; it rises near 8:10 local.
        let (lat, lon) = (47.39, 0.69);
        let at = |s: &str| s.parse::<jiff::Timestamp>().expect("a time");
        assert!(!sun_down(lat, lon, at("2026-11-13T15:50:00Z")));
        assert!(sun_down(lat, lon, at("2026-11-13T16:40:00Z")));
        assert!(sun_down(lat, lon, at("2026-11-13T23:00:00Z")));
        assert!(sun_down(lat, lon, at("2026-11-14T06:30:00Z")));
        assert!(!sun_down(lat, lon, at("2026-11-14T07:40:00Z")));
        // Paris's place, as zone1970.tab writes it.
        let tab = "# comment\nFR,MC\t+4852+00220\tEurope/Paris\n";
        let (lat, lon) = zone_place(tab, "Europe/Paris").expect("Paris");
        assert!((lat - 48.867).abs() < 0.01 && (lon - 2.333).abs() < 0.01);
        let (lat, lon) = coordinates("-332448+0703952").expect("seconds");
        assert!((lat + 33.413).abs() < 0.01 && (lon - 70.664).abs() < 0.01);
    }

    #[test]
    fn the_board_hour_lays_the_real_day_over_six_to_twenty() {
        // 45° N, 0° E (UTC's place), 21 June: the sun rises near 4:20 UTC,
        // sets near 19:45.
        let (lat, lon) = (45.0, 0.0);
        let at = |s: &str| s.parse::<jiff::Timestamp>().expect("a time");
        let h = |s: &str| board_hour(lat, lon, at(s));
        let noon = h("2026-06-21T12:02:00Z");
        assert!((noon - 13.0).abs() < 0.2, "{noon}");
        let morning = h("2026-06-21T06:30:00Z");
        assert!((7.6..8.4).contains(&morning), "{morning}");
        let sunset = h("2026-06-21T19:30:00Z");
        assert!((19.5..20.0).contains(&sunset), "{sunset}");
        let night = h("2026-06-21T22:30:00Z");
        assert!((22.5..24.0).contains(&night), "{night}");
        let dawn = h("2026-06-22T04:00:00Z");
        assert!((5.0..6.0).contains(&dawn), "{dawn}");
        // Above the polar circle in June: no night.
        assert_eq!(board_hour(78.2, 15.6, at("2026-06-21T23:00:00Z")), 13.0);
    }
}
