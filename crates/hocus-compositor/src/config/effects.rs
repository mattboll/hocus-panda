//! The desktop's effects, `[effects]` (#434, docs/design/effects.md): one
//! switch each, all off by default. Read where each plays, as it starts:
//! a change applies to the next one.

use serde::Deserialize;

/// When the rain on the glass falls (`[effects] rain-when`, #814).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RainWhen {
    #[default]
    Always,
    /// Only while the shell says it rains where `[weather]` is for.
    Raining,
}

impl RainWhen {
    /// Whether the rain falls now, a place set (`[weather] place`) or not,
    /// the shell saying it rains there or not: never without a place when
    /// it follows the weather.
    pub fn falls(self, place: bool, raining: bool) -> bool {
        match self {
            Self::Always => true,
            Self::Raining => place && raining,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RainWhen;

    #[test]
    fn the_rain_follows_the_weather_only_when_asked_and_never_without_a_place() {
        for (place, raining) in [(false, false), (false, true), (true, false), (true, true)] {
            assert!(RainWhen::Always.falls(place, raining));
        }
        assert!(RainWhen::Raining.falls(true, true));
        assert!(!RainWhen::Raining.falls(true, false));
        assert!(!RainWhen::Raining.falls(false, true));
    }
}

/// The effects (the board's fourteen, and a second garland), grouped as
/// Settings › Appearance › Effects shows them.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Effects {
    // The wallpaper.
    pub snow: bool,
    /// The wallpaper is an aurora rippling while the bar's music plays
    /// (#801).
    pub aurora: bool,
    /// The wallpaper becomes wet glass, drops forming and sliding (#802).
    pub rain_on_glass: bool,
    /// When it rains on the glass: always, or only while the weather
    /// says it rains (#814).
    pub rain_when: RainWhen,
    pub koi: bool,
    /// After sunset, fireflies drift and blink over the wallpaper (#804).
    pub fireflies: bool,
    pub daylight: bool,
    pub depth: bool,
    // The windows.
    /// A closing window burns from the bottom up (#794).
    pub flames: bool,
    /// A closing window cracks, then breaks into shards that fall (#798);
    /// with the flames on too, they take turns.
    pub shatter: bool,
    /// A dragged window deforms like jelly, in place of the standard
    /// wobble (#797).
    pub jelly: bool,
    /// A garland hangs under the active window (#799).
    pub garland: bool,
    /// Another hangs across the top of each screen's work area, under
    /// the windows (#813).
    pub garland_top: bool,
    // The moments.
    /// Fireworks behind the windows when an agent finishes with all its
    /// tests passing (#810).
    pub fireworks: bool,
    /// Confetti from the bar for an agent's good news, `hocus celebrate`
    /// (#811).
    pub confetti: bool,
    // The agents.
    /// Each agent's panda sits on its window's top edge, playing its
    /// state (#796).
    pub creatures: bool,
}
