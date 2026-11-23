//! The night light (#436, #929, docs/design/windows.md §14): the screens'
//! white made warmer in the evening, through each screen's gamma ramp, on
//! the session's own screens (the virtual terminal's backend).
//!
//! The schedule is the user's, on the wall clock in the system's time
//! zone: from sunset to sunrise at the weather's place (`[night-light]
//! schedule = "sun"`, recomputed each day; without a place, the fixed
//! hours), or from `from` to `to`. It fades in over 30 min from the start
//! and out over 30 min from the end. The loop never ticks for it but in a
//! fade: it wakes when the next change is due (a few seconds apart in a
//! fade, at most 15 min apart otherwise, so that a clock, a time zone or
//! a day changed is caught), and again as the machine wakes from sleep
//! and as the file changes. What the backends hand the screens is the
//! ramp for [`State::night_light_kelvin`]; headless keeps it for the
//! tests.

use std::time::Duration;

use hocus_link::sun::{self, SunDay};
use jiff::{Timestamp, Zoned};
use smithay::reexports::calloop::{
    RegistrationToken,
    timer::{TimeoutAction, Timer},
};

use crate::{
    State,
    config::{NightLight, NightSchedule},
};

/// The screen's own white: no change.
pub const NEUTRAL: u32 = 6500;
/// How long it fades in and out.
pub const FADE_MINUTES: f64 = 30.0;
const DAY: f64 = 24.0 * 60.0;
/// While fading, the temperature is looked at again this often.
const FADE_STEP: Duration = Duration::from_secs(10);
/// Otherwise at most this long apart.
const LONGEST: Duration = Duration::from_secs(15 * 60);

/// When the night is on a day, in minutes since the wall clock's midnight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hours {
    /// It warms from the first and goes back from the second.
    Span(f64, f64),
    /// All day: the same hour twice, or a polar night.
    Always,
    /// Not that day: a polar day.
    Never,
}

/// Minutes since midnight of `at`, on its own clock.
fn minutes_of(at: &Zoned) -> f64 {
    f64::from(at.hour()) * 60.0 + f64::from(at.minute()) + f64::from(at.second()) / 60.0
}

/// The night of `config` on `now`'s day: from sunset to sunrise at
/// `place` (latitude, longitude) when it follows the sun and has one,
/// else its fixed hours.
pub fn hours(config: &NightLight, place: Option<(f64, f64)>, now: &Zoned) -> Hours {
    let place = place.filter(|_| config.schedule == NightSchedule::Sun);
    let Some((lat, lon)) = place else {
        return if config.from == config.to {
            Hours::Always
        } else {
            Hours::Span(f64::from(config.from), f64::from(config.to))
        };
    };
    match sun::on(now.date(), lat, lon) {
        SunDay::Rises { rise, set } => {
            let here = |t: Timestamp| minutes_of(&t.to_zoned(now.time_zone().clone()));
            Hours::Span(here(set), here(rise))
        }
        SunDay::AlwaysUp => Hours::Never,
        SunDay::AlwaysDown => Hours::Always,
    }
}

/// Minutes from `start` to `now`, round the day.
fn since(now: f64, start: f64) -> f64 {
    (now - start).rem_euclid(DAY)
}

/// How far into the night `now` is (minutes since midnight), from 0 (day)
/// to 1 (night): from the night's start, 30 min to warm; from its end,
/// 30 min to go back.
pub fn strength(now: f64, hours: Hours) -> f64 {
    let (from, to) = match hours {
        Hours::Always => return 1.0,
        Hours::Never => return 0.0,
        Hours::Span(from, to) => (from, to),
    };
    let night = since(to, from);
    let t = since(now, from);
    let full = (night / FADE_MINUTES).min(1.0);
    if t < night {
        (t / FADE_MINUTES).min(1.0)
    } else {
        let after = t - night;
        if after < FADE_MINUTES {
            full * (1.0 - after / FADE_MINUTES)
        } else {
            0.0
        }
    }
}

/// Minutes until `strength` next changes from `now`: 0 while it fades.
fn until_change(now: f64, hours: Hours) -> f64 {
    let Hours::Span(from, to) = hours else {
        return f64::INFINITY;
    };
    let s = strength(now, hours);
    if s > 0.0 && s < 1.0 {
        return 0.0;
    }
    // Day: until it warms; night: until it goes back. At the hour
    // itself, 0: it starts to fade.
    let next = if s == 0.0 { from } else { to };
    (next - now).rem_euclid(DAY)
}

/// The temperature `strength` of the way from the screen's white to
/// `night`, moving evenly in mireds (as the eye sees it).
pub fn kelvin(night: u32, strength: f64) -> u32 {
    let (day, night) = (1e6 / f64::from(NEUTRAL), 1e6 / f64::from(night.max(1000)));
    let mired = day + (night - day) * strength.clamp(0.0, 1.0);
    // Whole tens: no new ramp for a change nobody sees.
    ((1e6 / mired / 10.0).round() * 10.0) as u32
}

/// The red, green and blue of a black body at `kelvin`, from 0 to 1,
/// after the well-known fit of the CIE colour matching tables (Tanner
/// Helland's), divided by its value at 6500 K so that the screen's own
/// white is left as it is.
pub fn white(kelvin: u32) -> [f64; 3] {
    fn fit(kelvin: f64) -> [f64; 3] {
        let t = kelvin / 100.0;
        let red = if t <= 66.0 {
            255.0
        } else {
            329.698_727_446 * (t - 60.0).powf(-0.133_204_759_2)
        };
        let green = if t <= 66.0 {
            99.470_802_586_1 * t.ln() - 161.119_568_166_1
        } else {
            288.122_169_528_3 * (t - 60.0).powf(-0.075_514_849_2)
        };
        let blue = if t >= 66.0 {
            255.0
        } else if t <= 19.0 {
            0.0
        } else {
            138.517_731_223_1 * (t - 10.0).ln() - 305.044_792_730_7
        };
        [red, green, blue].map(|c| c.clamp(0.0, 255.0) / 255.0)
    }
    let here = fit(f64::from(kelvin.clamp(1000, NEUTRAL)));
    let neutral = fit(f64::from(NEUTRAL));
    let ([r, g, b], [nr, ng, nb]) = (here, neutral);
    [r / nr, g / ng, b / nb].map(|c: f64| c.clamp(0.0, 1.0))
}

/// A gamma ramp of `size` entries for each of red, green and blue at
/// `kelvin`: straight lines from 0 to the channel's share of white.
pub fn ramp(size: usize, kelvin: u32) -> [Vec<u16>; 3] {
    let white = white(kelvin);
    let last = size.saturating_sub(1).max(1) as f64;
    white.map(|k| {
        (0..size)
            .map(|i| (i as f64 / last * k * 65535.0).round() as u16)
            .collect()
    })
}

/// The night light's state on the loop.
#[derive(Debug, Default)]
pub struct Night {
    /// The temperature the screens should show now; `None`: their own.
    pub kelvin: Option<u32>,
    /// When it looks again.
    timer: Option<RegistrationToken>,
    /// Tests: the wall clock, in place of the system's.
    pub(crate) clock: Option<Zoned>,
}

/// What `config` asks at `minutes` since midnight on a day whose night is
/// `hours`: the temperature, or `None` for the screens' own white; and
/// how long until it changes.
pub fn at(config: &NightLight, hours: Hours, minutes: f64) -> (Option<u32>, Duration) {
    if !config.enabled {
        return (None, Duration::MAX);
    }
    let kelvin = kelvin(config.temperature, strength(minutes, hours));
    let wanted = (kelvin < NEUTRAL).then_some(kelvin);
    let next = until_change(minutes, hours);
    let wait = if next <= 0.0 {
        FADE_STEP
    } else {
        // A second past the change, so that it has begun.
        Duration::from_secs_f64((next * 60.0 + 1.0).min(LONGEST.as_secs_f64()))
    };
    (wanted, wait.min(LONGEST))
}

impl State {
    /// The temperature the screens should show; `None`: their own white.
    pub fn night_light_kelvin(&self) -> Option<u32> {
        self.night.kelvin
    }

    /// Looks at the clock and the file: the temperature now, and a wake-up
    /// for the next change. At start, as the file changes, as the machine
    /// wakes, and from its own timer.
    pub(crate) fn night_light_update(&mut self) {
        if let Some(token) = self.night.timer.take() {
            self.loop_handle.remove(token);
        }
        let config = self.config.night_light;
        if !config.enabled {
            self.night_light_set(None);
            return;
        }
        let now = self.night.clock.clone().unwrap_or_else(Zoned::now);
        let weather = &self.config.weather;
        let place = weather
            .has_place()
            .then_some((weather.latitude, weather.longitude));
        let (kelvin, wait) = at(&config, hours(&config, place, &now), minutes_of(&now));
        self.night_light_set(kelvin);
        // Tests hold the clock: they move it and look again themselves.
        if self.night.clock.is_some() {
            return;
        }
        let inserted = self
            .loop_handle
            .insert_source(Timer::from_duration(wait), |_, _, app| {
                app.state.night.timer = None;
                app.state.night_light_update();
                TimeoutAction::Drop
            });
        match inserted {
            Ok(token) => self.night.timer = Some(token),
            Err(err) => tracing::warn!(%err.error, "the night light will not follow the clock"),
        }
    }

    fn night_light_set(&mut self, kelvin: Option<u32>) {
        if self.night.kelvin != kelvin {
            if kelvin.is_none() || self.night.kelvin.is_none() {
                tracing::info!(?kelvin, "night light");
            } else {
                tracing::debug!(?kelvin, "night light");
            }
            self.night.kelvin = kelvin;
            // The backends hand the screens their ramps after the loop's
            // turn.
            self.redraw = true;
        }
    }

    /// `[night-light]` changed on disk: whether it did.
    pub(crate) fn night_light_config_changed(&mut self, new: &NightLight) -> bool {
        if *new == self.config.night_light {
            return false;
        }
        self.config.night_light = *new;
        self.night_light_update();
        true
    }

    /// Tests: the wall clock at `now`, and the night light looked at
    /// again.
    pub(crate) fn night_light_clock(&mut self, now: Zoned) {
        self.night.clock = Some(now);
        self.night_light_update();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NINE_TO_SEVEN: Hours = Hours::Span(21.0 * 60.0, 7.0 * 60.0);
    const PARIS: (f64, f64) = (48.8566, 2.3522);

    fn hm(h: u16, m: u16) -> f64 {
        f64::from(h * 60 + m)
    }

    #[test]
    fn it_fades_in_from_nine_and_out_from_seven() {
        let s = |h, m| strength(hm(h, m), NINE_TO_SEVEN);
        assert_eq!(s(12, 0), 0.0);
        assert_eq!(s(20, 59), 0.0);
        assert_eq!(s(21, 0), 0.0);
        assert!((s(21, 15) - 0.5).abs() < 1e-9);
        assert_eq!(s(21, 30), 1.0);
        assert_eq!(s(0, 0), 1.0, "past midnight");
        assert_eq!(s(6, 59), 1.0);
        assert!((s(7, 15) - 0.5).abs() < 1e-9);
        assert_eq!(s(7, 30), 0.0);
        // A night within the day, and a whole day.
        let afternoon = Hours::Span(13.0 * 60.0, 15.0 * 60.0);
        assert_eq!(strength(hm(14, 0), afternoon), 1.0);
        assert_eq!(strength(hm(16, 0), afternoon), 0.0);
        assert_eq!(strength(hm(16, 0), Hours::Always), 1.0);
        assert_eq!(strength(hm(23, 0), Hours::Never), 0.0);
        // A night shorter than the fade never fully warms, and goes back
        // from where it got.
        let short = Hours::Span(21.0 * 60.0, 21.0 * 60.0 + 10.0);
        assert!((strength(hm(21, 10), short) - 1.0 / 3.0).abs() < 1e-9);
        assert!((strength(hm(21, 25), short) - 1.0 / 6.0).abs() < 1e-9);
    }

    #[test]
    fn it_wakes_for_the_next_change_only() {
        let config = NightLight {
            enabled: true,
            ..NightLight::default()
        };
        let at = |config: &NightLight, minutes| at(config, NINE_TO_SEVEN, minutes);
        // Noon: none, and nothing until 21:00 but a look every 15 min.
        let (kelvin, wait) = at(&config, hm(12, 0));
        assert_eq!(kelvin, None);
        assert_eq!(wait, LONGEST);
        // 20:55: wakes a second after 21:00.
        let (_, wait) = at(&config, hm(20, 55));
        assert_eq!(wait, Duration::from_secs(5 * 60 + 1));
        // Fading: every 10 s, half-way in mireds.
        let (kelvin, wait) = at(&config, hm(21, 15));
        assert_eq!(wait, FADE_STEP);
        assert_eq!(kelvin, Some(4950));
        // Night: 4000 K; 6:58 wakes after 7:00.
        assert_eq!(at(&config, hm(23, 0)).0, Some(4000));
        assert_eq!(at(&config, hm(6, 58)).1, Duration::from_secs(2 * 60 + 1));
        // Off: never.
        let off = NightLight::default();
        assert_eq!(at(&off, hm(23, 0)), (None, Duration::MAX));
    }

    fn day(at: &str) -> Zoned {
        at.parse::<Timestamp>()
            .expect("a time")
            .to_zoned(jiff::tz::TimeZone::UTC)
    }

    #[test]
    fn with_the_sun_it_warms_from_sunset_to_sunrise() {
        let config = NightLight {
            enabled: true,
            ..NightLight::default()
        };
        // 21 June in Paris: sunset about 19:58 UTC, sunrise about 03:47.
        let Hours::Span(set, rise) = hours(&config, Some(PARIS), &day("2026-06-21T12:00:00Z"))
        else {
            panic!("the sun sets in Paris");
        };
        assert!((set - hm(19, 58)).abs() <= 3.0, "{set}");
        assert!((rise - hm(3, 47)).abs() <= 3.0, "{rise}");
        let summer = Hours::Span(set, rise);
        assert_eq!(strength(hm(19, 0), summer), 0.0);
        assert!(
            (strength(set + 15.0, summer) - 0.5).abs() < 1e-9,
            "fading in from sunset"
        );
        assert_eq!(strength(hm(23, 0), summer), 1.0);
        assert_eq!(strength(hm(3, 0), summer), 1.0);
        assert!(
            (strength(rise + 15.0, summer) - 0.5).abs() < 1e-9,
            "fading out from sunrise"
        );
        assert_eq!(strength(hm(5, 0), summer), 0.0);
        // Recomputed each day: in November it sets before 16:10 UTC.
        let Hours::Span(november, _) = hours(&config, Some(PARIS), &day("2026-11-20T12:00:00Z"))
        else {
            panic!("the sun sets in Paris");
        };
        assert!(november < hm(16, 10), "{november}");
        // Its wake-up: a second past sunset.
        let (kelvin, wait) = at(&config, summer, set - 5.0);
        assert_eq!(kelvin, None);
        assert!((wait.as_secs_f64() - 301.0).abs() < 1e-6, "{wait:?}");
    }

    #[test]
    fn without_a_place_or_with_hours_the_fixed_hours_apply() {
        let sun = NightLight::default();
        let now = day("2026-06-21T12:00:00Z");
        assert_eq!(sun.schedule, NightSchedule::Sun);
        assert_eq!(hours(&sun, None, &now), NINE_TO_SEVEN);
        let fixed = NightLight {
            schedule: NightSchedule::Hours,
            ..NightLight::default()
        };
        assert_eq!(hours(&fixed, Some(PARIS), &now), NINE_TO_SEVEN);
        let all_day = NightLight {
            from: 21 * 60,
            to: 21 * 60,
            ..fixed
        };
        assert_eq!(hours(&all_day, Some(PARIS), &now), Hours::Always);
        // Far north: no night in June, night all day in December.
        let tromso = Some((69.65, 18.96));
        assert_eq!(hours(&sun, tromso, &now), Hours::Never);
        let december = day("2026-12-21T12:00:00Z");
        assert_eq!(hours(&sun, tromso, &december), Hours::Always);
        let on = NightLight {
            enabled: true,
            ..sun
        };
        assert_eq!(at(&on, Hours::Never, hm(23, 0)), (None, LONGEST));
        assert_eq!(at(&on, Hours::Always, hm(12, 0)), (Some(4000), LONGEST));
    }

    #[test]
    fn the_ramp_keeps_red_and_lowers_blue() {
        assert_eq!(white(NEUTRAL), [1.0, 1.0, 1.0]);
        let [r, g, b] = white(4000);
        assert_eq!(r, 1.0);
        assert!((0.78..0.84).contains(&g), "{g}");
        assert!((0.62..0.70).contains(&b), "{b}");
        // Warmer, less blue.
        assert!(white(2700)[2] < b);
        let [red, green, blue] = ramp(256, 4000);
        assert_eq!(red.len(), 256);
        assert_eq!((red[0], red[255]), (0, 65535));
        assert_eq!(green[255], (g * 65535.0).round() as u16);
        assert_eq!(blue[255], (b * 65535.0).round() as u16);
        assert!(blue.windows(2).all(|w| w[0] <= w[1]));
        // The screen's own: straight.
        let [red, _, blue] = ramp(1024, NEUTRAL);
        assert_eq!(red, blue);
        assert_eq!(red[512], (512.0 / 1023.0 * 65535.0f64).round() as u16);
    }
}
