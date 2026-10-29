//! What the levels show: the sound, the brightness, the battery and the
//! power profile as last read, and the words the bar says about them, in
//! English and French.

use std::time::Duration;

use crate::clock::time::Lang;

/// Everything the levels show; `None`: no such service or device, and the
/// row (or the part) is hidden.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Snapshot {
    pub sound: Option<Sound>,
    /// The internal panel's backlight, in percent.
    pub brightness: Option<f32>,
    pub battery: Option<Battery>,
    pub profile: Option<Profile>,
}

/// The default output's volume.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sound {
    /// In percent, as `wpctl` shows it (the cube root of the linear
    /// volume); above 100 when amplified.
    pub volume: f32,
    pub muted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Battery {
    pub percent: f32,
    pub charge: Charge,
    /// Until empty when discharging, until full when charging, when known.
    pub left: Option<Duration>,
}

/// Whether the battery charges, from UPower's `State`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charge {
    Discharging,
    Charging,
    /// Plugged in and full.
    Full,
    /// Plugged in, not charging (a charge limit, say).
    PluggedIn,
    Unknown,
}

impl Charge {
    /// UPower's `State`: 1 charging, 2 discharging, 3 empty, 4 fully
    /// charged, 5 pending charge, 6 pending discharge.
    pub fn from_upower(state: u32) -> Self {
        match state {
            1 => Self::Charging,
            2 | 3 | 6 => Self::Discharging,
            4 => Self::Full,
            5 => Self::PluggedIn,
            _ => Self::Unknown,
        }
    }

    pub fn plugged(self) -> bool {
        matches!(self, Self::Charging | Self::Full | Self::PluggedIn)
    }
}

/// How the battery's level looks: green, amber when low, orange when
/// critical (only while it is not plugged in).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Ok,
    /// 20 % or less.
    Low,
    /// 10 % or less: the status line asks for the charger.
    Critical,
}

impl Battery {
    /// The percentage as shown.
    pub fn shown(&self) -> u32 {
        self.percent.round().clamp(0.0, 100.0) as u32
    }

    pub fn band(&self) -> Band {
        if self.charge.plugged() {
            return Band::Ok;
        }
        match self.shown() {
            0..=10 => Band::Critical,
            11..=20 => Band::Low,
            _ => Band::Ok,
        }
    }

    /// Whether the bolt shows: on the charger, charging or full.
    pub fn bolt(&self) -> bool {
        matches!(self.charge, Charge::Charging | Charge::Full)
    }
}

/// power-profiles-daemon's `ActiveProfile`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    PowerSaver,
    Balanced,
    Performance,
}

impl Profile {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "power-saver" => Some(Self::PowerSaver),
            "balanced" => Some(Self::Balanced),
            "performance" => Some(Self::Performance),
            _ => None,
        }
    }
}

/// The words of the levels in a language.
#[derive(Debug, Clone, Copy)]
pub struct Words {
    lang: Lang,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    fn fr(self) -> bool {
        self.lang == Lang::French
    }

    /// In place of the volume, muted.
    pub fn muted(self) -> &'static str {
        if self.fr() { "muet" } else { "muted" }
    }

    /// The sound popover's title.
    pub fn sound(self) -> &'static str {
        if self.fr() { "Son" } else { "Sound" }
    }

    /// Over the outputs' list, in capitals.
    pub fn output(self) -> &'static str {
        if self.fr() { "SORTIE" } else { "OUTPUT" }
    }

    /// Over the microphone, in capitals.
    pub fn microphone(self) -> &'static str {
        if self.fr() { "MICRO" } else { "MICROPHONE" }
    }

    /// Over the applications' volumes, in capitals.
    pub fn applications(self) -> &'static str {
        "APPLICATIONS"
    }

    /// In place of the microphone's volume, muted.
    pub fn off(self) -> &'static str {
        if self.fr() { "coupé" } else { "off" }
    }

    /// In place of the applications' list.
    pub fn no_application(self) -> &'static str {
        if self.fr() {
            "Aucune application ne joue de son."
        } else {
            "No application is playing sound."
        }
    }

    /// Under the applications listed, the others' number.
    pub fn more_applications(self, more: usize) -> String {
        match (self.fr(), more) {
            (true, 1) => "+1 autre application".to_owned(),
            (true, n) => format!("+{n} autres applications"),
            (false, 1) => "+1 more application".to_owned(),
            (false, n) => format!("+{n} more applications"),
        }
    }

    pub fn profile(self, profile: Profile) -> &'static str {
        match (profile, self.fr()) {
            (Profile::PowerSaver, true) => "économie",
            (Profile::PowerSaver, false) => "power saver",
            (Profile::Balanced, true) => "équilibré",
            (Profile::Balanced, false) => "balanced",
            (Profile::Performance, _) => "performance",
        }
    }

    /// The battery's part of the status line, if any.
    fn battery(self, battery: &Battery) -> Option<String> {
        let fr = self.fr();
        let left = battery.left.filter(|d| !d.is_zero()).map(duration);
        let mut text = match (battery.charge, left) {
            (Charge::Discharging, Some(t)) => {
                if fr {
                    format!("{t} restantes")
                } else {
                    format!("{t} left")
                }
            }
            (Charge::Discharging, None) => (if fr { "sur batterie" } else { "on battery" }).into(),
            (Charge::Charging, Some(t)) => {
                if fr {
                    format!("plein dans {t}")
                } else {
                    format!("full in {t}")
                }
            }
            (Charge::Charging, None) => (if fr { "en charge" } else { "charging" }).into(),
            (Charge::Full, _) => (if fr {
                "branché, plein"
            } else {
                "plugged in, full"
            })
            .into(),
            (Charge::PluggedIn, _) => (if fr { "branché" } else { "plugged in" }).into(),
            (Charge::Unknown, _) => return None,
        };
        if battery.band() == Band::Critical {
            text.push_str(" · ");
            text.push_str(if fr {
                "branche le chargeur"
            } else {
                "plug in the charger"
            });
        }
        Some(text)
    }

    /// The status line under the gauges: the battery's time left (or
    /// what it does) and the power profile; the profile alone without a
    /// battery; nothing without either.
    pub fn status(self, levels: &Snapshot) -> Option<String> {
        let battery = levels.battery.and_then(|b| self.battery(&b));
        let profile = levels.profile.map(|p| self.profile(p));
        match (battery, profile) {
            (Some(b), Some(p)) => Some(format!("{b} · {p}")),
            (Some(b), None) => Some(b),
            (None, Some(p)) => Some(p.to_owned()),
            (None, None) => None,
        }
    }
}

/// A time left, to the minute: `35 min`, `3 h 40`, `2 h`, `1 h 05`.
pub fn duration(time: Duration) -> String {
    let minutes = ((time.as_secs_f64() / 60.0).round() as u64).max(1);
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m:02}"),
    }
}

/// The step of a level key, in percent.
pub const STEP: f32 = 5.0;
/// The lowest brightness the keys go to: the screen never goes black.
pub const BRIGHTNESS_FLOOR: f32 = 5.0;

/// The backlight's raw value one step up or down from `raw`, out of `max`:
/// 5 % of the range (a level at least), never above `max`, never below 5 %
/// (nor 1).
pub fn brightness_step(raw: u32, max: u32, up: bool) -> u32 {
    if max == 0 {
        return raw;
    }
    let max_f = max as f32;
    let percent = raw.min(max) as f32 * 100.0 / max_f;
    let to = if up { percent + STEP } else { percent - STEP };
    let to = to.clamp(BRIGHTNESS_FLOOR, 100.0);
    let floor = ((BRIGHTNESS_FLOOR * max_f / 100.0).round() as u32).clamp(1, max);
    let next = ((to * max_f / 100.0).round() as u32).clamp(floor, max);
    // A coarse backlight (a few levels) still moves a level a press.
    match (up, next.cmp(&raw)) {
        (true, std::cmp::Ordering::Less | std::cmp::Ordering::Equal) if raw < max => raw + 1,
        (false, std::cmp::Ordering::Greater | std::cmp::Ordering::Equal) if raw > floor => raw - 1,
        _ => next,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60;

    fn battery(percent: f32, charge: Charge, minutes: u64) -> Battery {
        Battery {
            percent,
            charge,
            left: Some(Duration::from_secs(minutes * MIN)),
        }
    }

    fn levels(battery: Option<Battery>, profile: Option<Profile>) -> Snapshot {
        Snapshot {
            battery,
            profile,
            ..Snapshot::default()
        }
    }

    #[test]
    fn the_status_line_in_each_state_and_language() {
        let fr = Words::new(Lang::French);
        let en = Words::new(Lang::English);
        let balanced = Some(Profile::Balanced);
        let cases = [
            // The mockup's states.
            (
                battery(82.0, Charge::Discharging, 220),
                "3 h 40 restantes · équilibré",
                "3 h 40 left · balanced",
            ),
            (
                battery(45.0, Charge::Charging, 70),
                "plein dans 1 h 10 · équilibré",
                "full in 1 h 10 · balanced",
            ),
            (
                battery(100.0, Charge::Full, 0),
                "branché, plein · équilibré",
                "plugged in, full · balanced",
            ),
            (
                battery(14.0, Charge::Discharging, 35),
                "35 min restantes · équilibré",
                "35 min left · balanced",
            ),
            (
                battery(6.0, Charge::Discharging, 9),
                "9 min restantes · branche le chargeur · équilibré",
                "9 min left · plug in the charger · balanced",
            ),
            // UPower without an estimate yet, and a charge limit.
            (
                battery(60.0, Charge::Discharging, 0),
                "sur batterie · équilibré",
                "on battery · balanced",
            ),
            (
                battery(60.0, Charge::Charging, 0),
                "en charge · équilibré",
                "charging · balanced",
            ),
            (
                battery(80.0, Charge::PluggedIn, 0),
                "branché · équilibré",
                "plugged in · balanced",
            ),
        ];
        for (b, french, english) in cases {
            assert_eq!(
                fr.status(&levels(Some(b), balanced)).as_deref(),
                Some(french)
            );
            assert_eq!(
                en.status(&levels(Some(b), balanced)).as_deref(),
                Some(english)
            );
        }
        // No battery: the profile alone; no profile: the battery alone.
        assert_eq!(
            fr.status(&levels(None, Some(Profile::PowerSaver)))
                .as_deref(),
            Some("économie")
        );
        assert_eq!(
            en.status(&levels(None, Some(Profile::Performance)))
                .as_deref(),
            Some("performance")
        );
        assert_eq!(
            en.status(&levels(Some(cases[0].0), None)).as_deref(),
            Some("3 h 40 left")
        );
        assert_eq!(en.status(&levels(None, None)), None);
        assert_eq!(fr.muted(), "muet");
        assert_eq!(en.muted(), "muted");
    }

    #[test]
    fn low_and_critical_only_off_the_charger() {
        let band = |percent, charge| battery(percent, charge, 10).band();
        assert_eq!(band(21.0, Charge::Discharging), Band::Ok);
        assert_eq!(band(20.0, Charge::Discharging), Band::Low);
        assert_eq!(band(20.4, Charge::Discharging), Band::Low);
        assert_eq!(band(11.0, Charge::Discharging), Band::Low);
        assert_eq!(band(10.0, Charge::Discharging), Band::Critical);
        assert_eq!(band(0.0, Charge::Discharging), Band::Critical);
        assert_eq!(band(5.0, Charge::Unknown), Band::Critical);
        assert_eq!(band(5.0, Charge::Charging), Band::Ok);
        assert_eq!(band(15.0, Charge::PluggedIn), Band::Ok);
        assert!(battery(50.0, Charge::Charging, 1).bolt());
        assert!(battery(100.0, Charge::Full, 1).bolt());
        assert!(!battery(80.0, Charge::PluggedIn, 1).bolt());
        assert!(!battery(80.0, Charge::Discharging, 1).bolt());
        assert_eq!(Charge::from_upower(4), Charge::Full);
        assert_eq!(Charge::from_upower(6), Charge::Discharging);
        assert_eq!(Charge::from_upower(9), Charge::Unknown);
    }

    #[test]
    fn times_left_to_the_minute() {
        let s = Duration::from_secs;
        assert_eq!(duration(s(35 * 60)), "35 min");
        assert_eq!(duration(s(9 * 60 + 20)), "9 min");
        assert_eq!(duration(s(9 * 60 + 40)), "10 min");
        assert_eq!(duration(s(20)), "1 min");
        assert_eq!(duration(s(59 * 60 + 40)), "1 h");
        assert_eq!(duration(s(220 * 60)), "3 h 40");
        assert_eq!(duration(s(65 * 60)), "1 h 05");
        assert_eq!(duration(s(25 * 3600)), "25 h");
    }

    #[test]
    fn brightness_steps_by_five_and_never_goes_black() {
        // 5 % of 1000 a step.
        assert_eq!(brightness_step(500, 1000, true), 550);
        assert_eq!(brightness_step(500, 1000, false), 450);
        assert_eq!(brightness_step(980, 1000, true), 1000);
        assert_eq!(brightness_step(1000, 1000, true), 1000);
        // Down stops at 5 %, and from below it comes back to 5 %.
        assert_eq!(brightness_step(80, 1000, false), 50);
        assert_eq!(brightness_step(50, 1000, false), 50);
        assert_eq!(brightness_step(0, 1000, false), 50);
        // A coarse backlight keeps at least 1.
        assert_eq!(brightness_step(1, 7, false), 1);
        assert_eq!(brightness_step(1, 7, true), 2);
        assert_eq!(brightness_step(3, 7, true), 4);
        assert_eq!(brightness_step(3, 7, false), 2);
        assert_eq!(brightness_step(0, 0, true), 0);
    }

    #[test]
    fn profiles_by_name() {
        assert_eq!(Profile::from_name("power-saver"), Some(Profile::PowerSaver));
        assert_eq!(Profile::from_name("balanced"), Some(Profile::Balanced));
        assert_eq!(
            Profile::from_name("performance"),
            Some(Profile::Performance)
        );
        assert_eq!(Profile::from_name("turbo"), None);
    }
}
