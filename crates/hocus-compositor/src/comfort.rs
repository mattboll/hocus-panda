//! Comfort and accessibility over the whole screen (#78): focus mode dims
//! everything but the focused window; the magnifier draws the screen larger
//! around the pointer; night light warms the colours in the evening, on a
//! schedule, over a few seconds, through each screen's gamma (the display
//! engine's colour pipeline: pictures taken of the screen are not warmed).

use std::time::Duration;

use hocus_core::night;
use smithay::{
    output::Output,
    reexports::calloop::timer::{TimeoutAction, Timer},
};

use crate::State;

/// How long the colours take to change.
const RAMP: Duration = Duration::from_secs(3);
/// How far the magnifier goes, and by how much a step.
pub const ZOOM_MAX: f64 = 8.0;
const ZOOM_STEP: f64 = 1.25;

/// What the screens show beyond the windows.
#[derive(Debug)]
pub struct Comfort {
    pub focus_mode: bool,
    /// Focus mode's veil: black, see-through.
    pub veil: smithay::backend::renderer::element::solid::SolidColorBuffer,
    /// 1 is the magnifier off.
    pub zoom: f64,
    pub night: Night,
}

impl Default for Comfort {
    fn default() -> Self {
        let mut veil = smithay::backend::renderer::element::solid::SolidColorBuffer::default();
        veil.update((1, 1), [0.0, 0.0, 0.0, 0.62]);
        Self {
            focus_mode: false,
            veil,
            zoom: 1.0,
            night: Night::default(),
        }
    }
}

/// Night light's state: what it shows now and where it goes.
#[derive(Debug)]
pub struct Night {
    /// Kelvin now, and since when it moves from where to where.
    pub kelvin: f32,
    ramp: Option<(Duration, f32, f32)>,
    /// The user's choice against the schedule, until the schedule changes
    /// its mind: on or off, and what the schedule said then.
    forced: Option<(bool, bool)>,
    /// The last kelvin each screen was given.
    applied: Vec<(String, f32)>,
    ticking: bool,
}

impl Default for Night {
    fn default() -> Self {
        Self {
            kelvin: night::DAYLIGHT as f32,
            ramp: None,
            forced: None,
            applied: Vec::new(),
            ticking: false,
        }
    }
}

impl State {
    pub fn toggle_focus_mode(&mut self) {
        self.comfort.focus_mode = !self.comfort.focus_mode;
        self.needs_redraw = true;
    }

    /// The magnifier a step closer (`in`) or further; at 1, it is off.
    pub fn zoom_step(&mut self, closer: bool) {
        let z = if closer {
            self.comfort.zoom * ZOOM_STEP
        } else {
            self.comfort.zoom / ZOOM_STEP
        };
        self.set_zoom(z);
    }

    pub fn set_zoom(&mut self, zoom: f64) {
        let z = zoom.clamp(1.0, ZOOM_MAX);
        // Close to 1 is off, not a blurry 1.02.
        let z = if z < 1.05 { 1.0 } else { z };
        if (z - self.comfort.zoom).abs() > f64::EPSILON {
            self.comfort.zoom = z;
            self.needs_redraw = true;
        }
    }

    /// What the schedule says now.
    fn night_scheduled(&self) -> bool {
        let n = &self.config.night_light;
        if !n.enabled {
            return false;
        }
        let civil = self.wall_civil();
        let now = u32::from(civil.hour() as u8) * 60 + u32::from(civil.minute() as u8);
        let fixed = || {
            let from = hocus_core::config::parse_clock(&n.from).unwrap_or(21 * 60);
            let to = hocus_core::config::parse_clock(&n.to).unwrap_or(7 * 60);
            night::within(now, from, to)
        };
        match (n.schedule.as_str(), n.latitude, n.longitude) {
            ("sunset", Some(lat), Some(lon)) => {
                let day = civil.day_of_year() as u32;
                match night::sun(day, lat, lon) {
                    Some((rise, set)) => {
                        let offset = self.wall_offset_minutes();
                        let local = |utc: f64| (utc + f64::from(offset)).rem_euclid(1440.0) as u32;
                        night::within(now, local(set), local(rise))
                    }
                    None => fixed(),
                }
            }
            _ => fixed(),
        }
    }

    /// Minutes the user's clock is ahead of UTC, at the user's time (the
    /// virtual one in tests, whose offset is then not the machine's day's).
    fn wall_offset_minutes(&self) -> i32 {
        let zone = self.wall.zone();
        self.wall_civil()
            .to_zoned(zone.clone())
            .map_or_else(|_| zone.to_offset(jiff::Timestamp::now()), |z| z.offset())
            .seconds()
            / 60
    }

    /// Whether night light shows now: the schedule, unless the user said
    /// otherwise since it last changed.
    pub fn night_on(&self) -> bool {
        let scheduled = self.night_scheduled();
        match self.comfort.night.forced {
            Some((on, when)) if when == scheduled => on,
            _ => scheduled,
        }
    }

    /// The quick settings' tile and the action: the other way until the
    /// schedule changes.
    pub fn toggle_night_light(&mut self) {
        let scheduled = self.night_scheduled();
        let on = !self.night_on();
        self.comfort.night.forced = (on != scheduled).then_some((on, scheduled));
        self.needs_redraw = true;
        self.tick_night();
    }

    /// The kelvin `output` shows.
    pub fn night_kelvin(&self, output: &str) -> u32 {
        if self.config.night_light.except.iter().any(|e| e == output) {
            night::DAYLIGHT
        } else {
            self.comfort.night.kelvin.round() as u32
        }
    }

    /// After each dispatch: the colours go where they should, over
    /// [`RAMP`], and each screen is told what changed.
    pub fn refresh_night(&mut self) {
        let now = self.clock.now();
        let target = if self.night_on() {
            self.config.night_light.temperature as f32
        } else {
            night::DAYLIGHT as f32
        };
        let night = &mut self.comfort.night;
        let going = night.ramp.map(|(_, _, to)| to);
        if going != Some(target) && (night.kelvin - target).abs() > 0.5 {
            night.ramp = Some((now, night.kelvin, target));
        }
        if let Some((start, from, to)) = night.ramp {
            let t = (now.saturating_sub(start).as_secs_f32() / RAMP.as_secs_f32()).min(1.0);
            // Eased: no jump at either end.
            let eased = t * t * (3.0 - 2.0 * t);
            night.kelvin = from + (to - from) * eased;
            if t >= 1.0 {
                night.kelvin = to;
                night.ramp = None;
            }
        }
        if self.config.night_light.enabled || self.comfort.night.ramp.is_some() {
            self.tick_night();
        }
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        for output in outputs {
            let name = output.name();
            let kelvin = self.night_kelvin(&name) as f32;
            let applied = &mut self.comfort.night.applied;
            let last = applied.iter().find(|(n, _)| *n == name).map(|(_, k)| *k);
            if last.is_some_and(|k| (k - kelvin).abs() < 1.0) {
                continue;
            }
            applied.retain(|(n, _)| *n != name);
            applied.push((name, kelvin));
            self.gamma_wanted.push((output, night::rgb(kelvin)));
        }
    }

    /// Wakes the loop while night light may change: often while the
    /// colours move, else twice a minute for the schedule.
    fn tick_night(&mut self) {
        if self.comfort.night.ticking {
            return;
        }
        let Some(handle) = self.waker() else {
            return;
        };
        let ticking = handle.insert_source(Timer::from_duration(Duration::ZERO), |_, _, app| {
            let state = &mut app.state;
            state.needs_redraw = true;
            if state.comfort.night.ramp.is_some() {
                TimeoutAction::ToDuration(Duration::from_millis(100))
            } else if state.config.night_light.enabled {
                TimeoutAction::ToDuration(Duration::from_secs(30))
            } else {
                state.comfort.night.ticking = false;
                TimeoutAction::Drop
            }
        });
        match ticking {
            Ok(_) => self.comfort.night.ticking = true,
            Err(err) => tracing::warn!(%err.error, "night light will not follow its schedule"),
        }
    }
}

impl crate::App {
    /// After a dispatch: screens get the gamma night light wants, where the
    /// backend sets it (a TTY's display engine).
    pub fn apply_gamma(&mut self) {
        let wanted = std::mem::take(&mut self.state.gamma_wanted);
        if wanted.is_empty() {
            return;
        }
        if let crate::BackendState::Tty(tty) = &mut self.backend {
            for (output, rgb) in wanted {
                tty.set_gamma(&output, rgb);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_magnifier_steps_stay_within_bounds() {
        let mut c = Comfort::default();
        for _ in 0..30 {
            c.zoom = (c.zoom * ZOOM_STEP).min(ZOOM_MAX);
        }
        assert_eq!(c.zoom, ZOOM_MAX);
    }
}
