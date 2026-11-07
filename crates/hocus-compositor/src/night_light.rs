//! The night light (#436, docs/design/windows.md §14): the screens' white
//! made warmer in the evening, through each screen's gamma ramp, on the
//! session's own screens (the virtual terminal's backend).
//!
//! The schedule is the user's: wall-clock hours (`[night-light] from` and
//! `to`), read in the system's time zone. It fades in over 30 min from
//! `from` and out over 30 min from `to`. The loop never ticks for it but
//! in a fade: it wakes when the next change is due (a few seconds apart
//! in a fade, at most 15 min apart otherwise, so that a clock or a time
//! zone changed is caught), and again as the machine wakes from sleep and
//! as the file changes. What the backends hand the screens is the ramp
//! for [`State::night_light_kelvin`]; headless keeps it for the tests.

use std::time::Duration;

use smithay::reexports::calloop::{
    RegistrationToken,
    timer::{TimeoutAction, Timer},
};

use crate::{State, config::NightLight};

/// The screen's own white: no change.
pub const NEUTRAL: u32 = 6500;
/// How long it fades in and out.
pub const FADE_MINUTES: f64 = 30.0;
const DAY: f64 = 24.0 * 60.0;
/// While fading, the temperature is looked at again this often.
const FADE_STEP: Duration = Duration::from_secs(10);
/// Otherwise at most this long apart.
const LONGEST: Duration = Duration::from_secs(15 * 60);

/// Minutes from `start` to `now`, round the day.
fn since(now: f64, start: u16) -> f64 {
    (now - f64::from(start)).rem_euclid(DAY)
}

/// How far into the night `now` is (minutes since midnight), from 0 (day)
/// to 1 (night): from `from`, 30 min to warm; from `to`, 30 min to go
/// back. The same hour twice: night all day.
pub fn strength(now: f64, from: u16, to: u16) -> f64 {
    if from == to {
        return 1.0;
    }
    let night = since(f64::from(to), from);
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
fn until_change(now: f64, from: u16, to: u16) -> f64 {
    if from == to {
        return f64::INFINITY;
    }
    let s = strength(now, from, to);
    if s > 0.0 && s < 1.0 {
        return 0.0;
    }
    // Day: until it warms; night: until it goes back. At the hour
    // itself, 0: it starts to fade.
    let next = if s == 0.0 { from } else { to };
    (f64::from(next) - now).rem_euclid(DAY)
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
    [0, 1, 2].map(|i| (here[i] / neutral[i]).clamp(0.0, 1.0))
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
    /// Tests: the wall clock's minutes since midnight, in place of the
    /// system's.
    pub(crate) clock: Option<f64>,
}

/// Minutes since midnight now, on the system's clock and in its time zone.
fn wall_minutes() -> f64 {
    let now = jiff::Zoned::now();
    f64::from(now.hour()) * 60.0 + f64::from(now.minute()) + f64::from(now.second()) / 60.0
}

/// What `config` asks at `minutes` since midnight: the temperature, or
/// `None` for the screens' own white; and how long until it changes.
pub fn at(config: &NightLight, minutes: f64) -> (Option<u32>, Duration) {
    if !config.enabled {
        return (None, Duration::MAX);
    }
    let s = strength(minutes, config.from, config.to);
    let kelvin = kelvin(config.temperature, s);
    let wanted = (kelvin < NEUTRAL).then_some(kelvin);
    let next = until_change(minutes, config.from, config.to);
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
        let minutes = self.night.clock.unwrap_or_else(wall_minutes);
        let (kelvin, wait) = at(&config, minutes);
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

    /// Tests: the wall clock at `minutes` since midnight, and the night
    /// light looked at again.
    pub(crate) fn night_light_clock(&mut self, minutes: f64) {
        self.night.clock = Some(minutes);
        self.night_light_update();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NINE_PM: u16 = 21 * 60;
    const SEVEN_AM: u16 = 7 * 60;

    fn hm(h: u16, m: u16) -> f64 {
        f64::from(h * 60 + m)
    }

    #[test]
    fn it_fades_in_from_nine_and_out_from_seven() {
        let s = |h, m| strength(hm(h, m), NINE_PM, SEVEN_AM);
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
        assert_eq!(strength(hm(14, 0), 13 * 60, 15 * 60), 1.0);
        assert_eq!(strength(hm(16, 0), 13 * 60, 15 * 60), 0.0);
        assert_eq!(strength(hm(16, 0), NINE_PM, NINE_PM), 1.0);
        // A night shorter than the fade never fully warms, and goes back
        // from where it got.
        assert!((strength(hm(21, 10), NINE_PM, 21 * 60 + 10) - 1.0 / 3.0).abs() < 1e-9);
        assert!((strength(hm(21, 25), NINE_PM, 21 * 60 + 10) - 1.0 / 6.0).abs() < 1e-9);
    }

    #[test]
    fn it_wakes_for_the_next_change_only() {
        let config = NightLight {
            enabled: true,
            ..NightLight::default()
        };
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
