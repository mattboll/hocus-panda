//! Sunrise and sunset (#894): when a wallpaper's day and night pictures
//! take turns, computed here with NOAA's equations of the sun's place,
//! without the network. The compositor switches by them; the Settings
//! say them on the wallpaper's page.

use std::f64::consts::PI;

/// The sun's upper edge on the horizon, refraction counted: the zenith
/// angle of sunrise and sunset, degrees.
const ZENITH: f64 = 90.833;

/// What the sun does on a day at a place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sun {
    /// It rises and sets: minutes after that day's midnight UTC (below 0
    /// or past 1440 far from Greenwich).
    Rises { rise: f64, set: f64 },
    /// It does not set (a polar day).
    AlwaysUp,
    /// It does not rise (a polar night).
    AlwaysDown,
}

/// The equation of time (minutes) and the sun's declination (radians) on
/// day `day` of the year (from 1) at `hour` UTC.
fn place_of_sun(day: f64, hour: f64) -> (f64, f64) {
    let g = 2.0 * PI / 365.0 * (day - 1.0 + (hour - 12.0) / 24.0);
    let eqtime = 229.18
        * (0.000075 + 0.001868 * g.cos()
            - 0.032077 * g.sin()
            - 0.014615 * (2.0 * g).cos()
            - 0.040849 * (2.0 * g).sin());
    let decl = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin() - 0.006758 * (2.0 * g).cos()
        + 0.000907 * (2.0 * g).sin()
        - 0.002697 * (3.0 * g).cos()
        + 0.00148 * (3.0 * g).sin();
    (eqtime, decl)
}

/// The cosine of the sun's hour angle at the horizon, at latitude `lat`
/// (radians) with declination `decl`: beyond ±1 it never gets there.
fn cos_hour_angle(lat: f64, decl: f64) -> f64 {
    ZENITH.to_radians().cos() / (lat.cos() * decl.cos()) - lat.tan() * decl.tan()
}

/// Sunrise and sunset on day `day_of_year` (from 1) at latitude `lat`
/// and longitude `lon` (degrees, east positive). Each is computed at its
/// own hour, once again from the first guess: within a minute or so.
pub fn sun(day_of_year: u16, lat: f64, lon: f64) -> Sun {
    let day = f64::from(day_of_year);
    let lat = lat.clamp(-89.9, 89.9).to_radians();
    let noon = 720.0 - 4.0 * lon;
    let (_, decl) = place_of_sun(day, noon / 60.0);
    let at_noon = cos_hour_angle(lat, decl);
    if at_noon > 1.0 {
        return Sun::AlwaysDown;
    }
    if at_noon < -1.0 {
        return Sun::AlwaysUp;
    }
    let event = |sign: f64| {
        let mut minutes = noon;
        for _ in 0..2 {
            let (eqtime, decl) = place_of_sun(day, minutes / 60.0);
            let ha = cos_hour_angle(lat, decl)
                .clamp(-1.0, 1.0)
                .acos()
                .to_degrees();
            minutes = 720.0 - 4.0 * (lon + sign * ha) - eqtime;
        }
        minutes
    };
    Sun::Rises {
        rise: event(1.0),
        set: event(-1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARIS: (f64, f64) = (48.8566, 2.3522);

    fn near(minutes: f64, hh: u32, mm: u32) -> bool {
        (minutes - f64::from(hh * 60 + mm)).abs() <= 3.0
    }

    #[test]
    fn paris_s_sunrise_and_sunset_at_the_solstices() {
        // 21 June 2026 (day 172): 05:47 and 21:58 in Paris's summer time,
        // 03:47 and 19:58 UTC.
        let Sun::Rises { rise, set } = sun(172, PARIS.0, PARIS.1) else {
            panic!("the sun rises in Paris");
        };
        assert!(near(rise, 3, 47) && near(set, 19, 58), "{rise} {set}");
        // 21 December 2026 (day 355): 08:42 and 16:56 in winter time,
        // 07:42 and 15:56 UTC.
        let Sun::Rises { rise, set } = sun(355, PARIS.0, PARIS.1) else {
            panic!("the sun rises in Paris");
        };
        assert!(near(rise, 7, 42) && near(set, 15, 56), "{rise} {set}");
    }

    #[test]
    fn far_north_the_sun_stays_up_or_down() {
        // Tromsø, 69.65° N.
        assert_eq!(sun(172, 69.65, 18.96), Sun::AlwaysUp);
        assert_eq!(sun(355, 69.65, 18.96), Sun::AlwaysDown);
    }
}
