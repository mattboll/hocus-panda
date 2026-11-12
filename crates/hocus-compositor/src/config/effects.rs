//! The desktop's effects, `[effects]` (#434, docs/design/effects.md): one
//! switch each, all off by default. Read where each plays, as it starts:
//! a change applies to the next one.

use serde::Deserialize;

/// The fourteen effects, grouped as Settings › Appearance › Effects shows
/// them. The flames, the glass, the jelly, the garland, the snow and the
/// creatures play so far; the others' switches are kept for their tickets.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Effects {
    // The wallpaper.
    pub snow: bool,
    pub aurora: bool,
    pub rain_on_glass: bool,
    pub koi: bool,
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
    // The moments.
    pub fireworks: bool,
    pub confetti: bool,
    // The agents.
    /// Each agent's panda sits on its window's top edge, playing its
    /// state (#796).
    pub creatures: bool,
}
