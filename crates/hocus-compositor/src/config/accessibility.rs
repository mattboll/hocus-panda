//! The developer's and accessibility's settings: `[developer]`, `[accessibility]`, `[magnifier]`, `[night-light]`.

use super::*;

/// `[developer]` (#783, docs/workflow.md, "Development loop").
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Developer {
    /// `hocus msg shell restart --program PATH` is accepted: another
    /// program runs as the shell, which draws the lock screen and the
    /// password dialog, so off unless the user develops the desktop.
    pub live_reload: bool,
    /// Every 5 s, what each effect playing costs the loop, in the log
    /// (#815, docs/performance.md, "The effects' cost").
    pub effects_timing: bool,
}

/// `[accessibility]` (docs/design/screen-reader.md, #734).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Accessibility {
    /// The screen reader (Orca) starts with the session; `Super+Alt+S`
    /// starts or stops it whatever this says.
    pub screen_reader: bool,
}

/// The magnifier, `[magnifier]` (docs/design/accessibility.md, #424).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Magnifier {
    /// On: the screen under the pointer magnified, at `zoom`.
    pub enabled: bool,
    /// From 1 to 16: what `enabled` turns it on at.
    #[serde(deserialize_with = "zoom")]
    pub zoom: f64,
    /// How the view follows the pointer.
    pub follow_pointer: FollowPointer,
    /// The view jumps to what takes the keyboard, and to the text caret.
    pub follow_keyboard: bool,
    /// Smoothed (linear filtering); off, hard pixels.
    pub smooth: bool,
}

impl Default for Magnifier {
    fn default() -> Self {
        Self {
            enabled: false,
            zoom: 2.0,
            follow_pointer: FollowPointer::Edge,
            follow_keyboard: true,
            smooth: true,
        }
    }
}

/// The night light, `[night-light]` (#436): the screens' colours warmer
/// from `from` to `to`, wall-clock hours, fading in and out 30 min.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct NightLight {
    pub enabled: bool,
    /// The colour temperature at night, in kelvin, from 1500 to 6500
    /// (6500: the screen's own white).
    #[serde(deserialize_with = "kelvin")]
    pub temperature: u32,
    /// When it starts warming and when it goes back, in minutes since
    /// midnight (`"21:00"`, `"7:30"`). The same hour: all day.
    #[serde(deserialize_with = "clock_time")]
    pub from: u16,
    #[serde(deserialize_with = "clock_time")]
    pub to: u16,
}

impl Default for NightLight {
    fn default() -> Self {
        Self {
            enabled: false,
            temperature: 4000,
            from: 21 * 60,
            to: 7 * 60,
        }
    }
}

pub(super) fn kelvin<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let kelvin = u32::deserialize(deserializer)?;
    if (1500..=6500).contains(&kelvin) {
        Ok(kelvin)
    } else {
        Err(serde::de::Error::custom(format!(
            "night light temperature {kelvin}: from 1500 to 6500"
        )))
    }
}

/// `"21:00"`, `"7:30"`: minutes since midnight.
pub(super) fn clock_time<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u16, D::Error> {
    let text = String::deserialize(deserializer)?;
    parse_clock_time(&text)
        .ok_or_else(|| serde::de::Error::custom(format!("{text:?}: a time of day, as \"21:00\"")))
}

pub(crate) fn parse_clock_time(text: &str) -> Option<u16> {
    let (h, m) = text.trim().split_once(':')?;
    let (h, m): (u16, u16) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60 && text.trim().len() <= 5).then_some(h * 60 + m)
}

/// How the magnifier's view follows the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FollowPointer {
    /// It moves only when the pointer pushes its edge, by as much.
    #[default]
    Edge,
    /// The pointer stays in its middle.
    #[serde(alias = "center")]
    Centre,
    /// It sits where the pointer sits on the screen.
    Proportional,
}

pub(super) fn zoom<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    let zoom = f64::deserialize(deserializer)?;
    if (1.0..=16.0).contains(&zoom) {
        Ok(zoom)
    } else {
        Err(serde::de::Error::custom(format!(
            "magnifier zoom {zoom}: from 1 to 16"
        )))
    }
}
