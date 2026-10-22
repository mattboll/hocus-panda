//! Night light (#78): when the evening is, and what a colour temperature
//! does to red, green and blue. Pure: the compositor brings the time.

use std::f64::consts::PI;

/// Daylight: nothing changes.
pub const DAYLIGHT: u32 = 6500;

/// Whether `now` (minutes after midnight) is between `from` and `to`,
/// across midnight when `to` comes first.
pub fn within(now: u32, from: u32, to: u32) -> bool {
    if from <= to {
        (from..to).contains(&now)
    } else {
        now >= from || now < to
    }
}

/// Sunrise and sunset, minutes after midnight UTC, on day `day` of the
/// year (1 to 366) at `latitude` and `longitude` (degrees, east positive);
/// `None` where the sun neither rises nor sets that day (NOAA's
/// approximation, a minute or two off).
pub fn sun(day: u32, latitude: f64, longitude: f64) -> Option<(f64, f64)> {
    let g = 2.0 * PI / 365.0 * (f64::from(day) - 1.0);
    let eqtime = 229.18
        * (0.000_075 + 0.001_868 * g.cos()
            - 0.032_077 * g.sin()
            - 0.014_615 * (2.0 * g).cos()
            - 0.040_849 * (2.0 * g).sin());
    let decl = 0.006_918 - 0.399_912 * g.cos() + 0.070_257 * g.sin() - 0.006_758 * (2.0 * g).cos()
        + 0.000_907 * (2.0 * g).sin()
        - 0.002_697 * (3.0 * g).cos()
        + 0.001_48 * (3.0 * g).sin();
    let lat = latitude.to_radians();
    let cos_ha = 90.833_f64.to_radians().cos() / (lat.cos() * decl.cos()) - lat.tan() * decl.tan();
    if !(-1.0..=1.0).contains(&cos_ha) {
        return None;
    }
    let ha = cos_ha.acos().to_degrees();
    let rise = 720.0 - 4.0 * (longitude + ha) - eqtime;
    let set = 720.0 - 4.0 * (longitude - ha) - eqtime;
    Some((rise.rem_euclid(1440.0), set.rem_euclid(1440.0)))
}

/// What `kelvin` keeps of red, green and blue, 0 to 1: a black body's
/// colour, white at daylight.
pub fn rgb(kelvin: f32) -> [f32; 3] {
    let t = f64::from(kelvin.clamp(1000.0, 40000.0)) / 100.0;
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
    let white = |c: f64, w: f64| (c / w).clamp(0.0, 1.0) as f32;
    // Normalised to daylight, which the black body puts a touch off white.
    let day = [
        255.0,
        99.470_802_586_1 * 65.0f64.ln() - 161.119_568_166_1,
        138.517_731_223_1 * 55.0f64.ln() - 305.044_792_730_7,
    ];
    [
        white(red, day[0]),
        white(green, day[1]),
        white(blue, day[2]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evenings_cross_midnight() {
        let at = |h: u32, m: u32| h * 60 + m;
        assert!(within(at(22, 0), at(21, 0), at(7, 0)));
        assert!(within(at(3, 0), at(21, 0), at(7, 0)));
        assert!(!within(at(12, 0), at(21, 0), at(7, 0)));
        assert!(within(at(13, 0), at(12, 0), at(14, 0)));
        assert!(!within(at(14, 0), at(12, 0), at(14, 0)));
    }

    #[test]
    fn the_sun_sets_where_and_when_it_does() {
        // Paris, 21 June: sets about 19:58 UTC, rises about 3:47.
        let (rise, set) = sun(172, 48.85, 2.35).expect("a sunset");
        assert!((set - (19.0 * 60.0 + 58.0)).abs() < 6.0, "{set}");
        assert!((rise - (3.0 * 60.0 + 47.0)).abs() < 6.0, "{rise}");
        // Tromsø in midsummer: no sunset.
        assert!(sun(172, 69.65, 18.96).is_none());
    }

    #[test]
    fn warmer_takes_blue_away_first() {
        assert_eq!(
            rgb(6500.0).map(|c| (c * 100.0).round()),
            [100.0, 100.0, 100.0]
        );
        let [r, g, b] = rgb(3500.0);
        assert!(r == 1.0 && g < 1.0 && b < g, "{r} {g} {b}");
    }
}
