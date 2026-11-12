//! The desktop's effects, `[effects]` (#434, docs/design/effects.md): one
//! switch each, all off by default. Read where each plays, as it starts:
//! a change applies to the next one.

use serde::Deserialize;

/// The fourteen effects, grouped as Settings › Appearance › Effects shows
/// them. Only `flames` plays yet; the others' switches are kept for their
/// tickets.
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
    pub shatter: bool,
    pub jelly: bool,
    pub garland: bool,
    // The moments.
    pub fireworks: bool,
    pub confetti: bool,
    // The agents.
    pub creatures: bool,
}
