//! The desktop's effects, `[effects]` (#434, docs/design/effects.md): one
//! switch each, all off by default. Read where each plays, as it starts:
//! a change applies to the next one.

use serde::Deserialize;

/// The effects (the board's fourteen, and a second garland), grouped as
/// Settings › Appearance › Effects shows them. The moments' switches are
/// kept for their tickets; the others play.
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
    pub fireworks: bool,
    pub confetti: bool,
    // The agents.
    /// Each agent's panda sits on its window's top edge, playing its
    /// state (#796).
    pub creatures: bool,
}
