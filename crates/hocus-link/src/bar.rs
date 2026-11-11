//! The bar's forms, its side, its clock and its groups (bar.md §3, §13),
//! and the motion of its form switch, which both sides compute.

use std::{str::FromStr, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{BadLine, text::tidy};

/// The bar's distance from the output's top, right and bottom edges, and
/// the windows' from the bar (logical pixels).
pub const EDGE: i32 = 8;
/// The bar's corner radius.
pub const RADIUS: f64 = 14.0;
/// An output at least this wide without the bar has the full form.
pub const FULL_FROM: i32 = 1600;
/// The form switch's duration; its curve is [`ease`].
pub const SWITCH: Duration = Duration::from_millis(240);

/// The bar's two forms (§3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Form {
    Full,
    Mini,
}

impl Form {
    /// The bar's width in this form, logical pixels.
    pub const fn width(self) -> i32 {
        match self {
            Self::Full => 196,
            Self::Mini => 56,
        }
    }

    pub const fn other(self) -> Self {
        match self {
            Self::Full => Self::Mini,
            Self::Mini => Self::Full,
        }
    }

    /// The default form on an output `width` logical pixels wide: full when
    /// what is left beside the full bar is at least [`FULL_FROM`].
    pub const fn default_for(width: i32) -> Self {
        if width - Self::Full.width() >= FULL_FROM {
            Self::Full
        } else {
            Self::Mini
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Mini => "mini",
        }
    }
}

/// The side of the screens the bar stands on (`[panel] side`, 6.4): its
/// edge zone, the room kept for it and what opens from it go with it, the
/// popovers on its inner side.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Side {
    #[default]
    Right,
    Left,
}

impl Side {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Left => "left",
        }
    }

    /// Which way is away from the screen's edge the bar stands on, along
    /// x: toward the windows. `-1` with the bar on the right.
    pub const fn inward(self) -> f64 {
        match self {
            Self::Right => -1.0,
            Self::Left => 1.0,
        }
    }
}

/// A keyboard layout as the compositor's keymap has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyLayout {
    /// Its XKB layout and variant as `[keyboard] layouts` writes them
    /// (`fr`, `fr(bepo)`); empty when unknown.
    #[serde(serialize_with = "tidy::name")]
    pub code: String,
    /// The name the keymap gives it: XKB's description, in English
    /// (`French (BEPO)`).
    #[serde(serialize_with = "tidy::name")]
    pub name: String,
}

/// The keyboard's level keys, as `[shortcuts]` names their actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LevelKey {
    VolumeUp,
    VolumeDown,
    Mute,
    BrightnessUp,
    BrightnessDown,
}

impl LevelKey {
    pub const ALL: [Self; 5] = [
        Self::VolumeUp,
        Self::VolumeDown,
        Self::Mute,
        Self::BrightnessUp,
        Self::BrightnessDown,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::VolumeUp => "volume-up",
            Self::VolumeDown => "volume-down",
            Self::Mute => "mute",
            Self::BrightnessUp => "brightness-up",
            Self::BrightnessDown => "brightness-down",
        }
    }

    /// Whether it steps on while held, as a key repeats (not mute).
    pub const fn repeats(self) -> bool {
        !matches!(self, Self::Mute)
    }
}

/// What the clock does when the minute changes (`[panel.clock]
/// transition`), from the 3.2 mockup's keyframes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Transition {
    /// Each changed digit leaves upward with a little blur; the new one
    /// comes from below with a slight overshoot.
    #[default]
    Odometer,
    /// The new digits rise into place and fade in.
    Roll,
    /// The old digits fold away, the new ones fold down like a split-flap.
    Flip,
    /// The new digits flicker on like a neon tube.
    Neon,
    /// They change at once.
    None,
}

impl Transition {
    pub const ALL: [Self; 5] = [
        Self::Odometer,
        Self::Roll,
        Self::Flip,
        Self::Neon,
        Self::None,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Odometer => "odometer",
            Self::Roll => "roll",
            Self::Flip => "flip",
            Self::Neon => "neon",
            Self::None => "none",
        }
    }
}

/// A group of the bar's column that the user moves (`[panel] order`,
/// 6.4): the meeting's card and the sign of what is in use stay first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BarGroup {
    Today,
    Tasks,
    Agents,
}

impl BarGroup {
    /// Their order when none is said (#599).
    pub const DEFAULT: [Self; 3] = [Self::Today, Self::Tasks, Self::Agents];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Tasks => "tasks",
            Self::Agents => "agents",
        }
    }

    /// The order `names` say: the names known, each once, in their order,
    /// then those missing in their default order; an unknown name is
    /// skipped.
    pub fn order_of<S: AsRef<str>>(names: &[S]) -> [Self; 3] {
        let mut order: Vec<Self> = Vec::with_capacity(3);
        let known = names.iter().filter_map(|n| n.as_ref().trim().parse().ok());
        for group in known.chain(Self::DEFAULT) {
            if !order.contains(&group) {
                order.push(group);
            }
        }
        // The three are there: the default order was added.
        match order.as_slice() {
            [first, second, third, ..] => [*first, *second, *third],
            _ => Self::DEFAULT,
        }
    }
}

impl FromStr for BarGroup {
    type Err = BadLine;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::DEFAULT
            .into_iter()
            .find(|g| g.name() == s)
            .ok_or_else(|| BadLine(format!("no bar group {s:?}: today, tasks or agents")))
    }
}

/// What the bar's column shows (`[panel]`, 6.4): each group that can be
/// hidden, and the order of those that move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BarGroups {
    pub order: [BarGroup; 3],
    pub meeting: bool,
    pub today: bool,
    pub tasks: bool,
    pub agents: bool,
}

impl Default for BarGroups {
    fn default() -> Self {
        Self {
            order: BarGroup::DEFAULT,
            meeting: true,
            today: true,
            tasks: true,
            agents: true,
        }
    }
}

impl BarGroups {
    /// Whether `group` shows.
    pub const fn shows(&self, group: BarGroup) -> bool {
        match group {
            BarGroup::Today => self.today,
            BarGroup::Tasks => self.tasks,
            BarGroup::Agents => self.agents,
        }
    }
}

/// How the clock looks (`[panel.clock]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockLook {
    /// The font family of the hours and minutes (and of the seconds, in
    /// its light weight).
    pub font: String,
    /// The colour of the hours and minutes, as red, green, blue.
    pub color: [u8; 3],
    pub transition: Transition,
    /// The seconds show beside the minutes, in the full form.
    pub seconds: bool,
}

impl ClockLook {
    /// The bundled font, the mockup's.
    pub const DEFAULT_FONT: &str = "Martian Mono";
    /// `text` (bar.md §13).
    pub const DEFAULT_COLOR: [u8; 3] = [0xEE, 0xF1, 0xF6];
}

impl Default for ClockLook {
    fn default() -> Self {
        Self {
            font: Self::DEFAULT_FONT.to_owned(),
            color: Self::DEFAULT_COLOR,
            transition: Transition::default(),
            seconds: true,
        }
    }
}

/// A colour written `#RRGGBB` (or `#RGB`), as red, green, blue.
pub fn parse_color(text: &str) -> Result<[u8; 3], BadLine> {
    let bad = || BadLine(format!("colour {text:?}: #RRGGBB or #RGB"));
    let hex = text.strip_prefix('#').ok_or_else(bad)?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let digit = |i: usize, len: usize| u8::from_str_radix(&hex[i * len..(i + 1) * len], 16);
    match hex.len() {
        6 => Ok([digit(0, 2), digit(1, 2), digit(2, 2)].map(|d| d.unwrap_or(0))),
        3 => Ok([digit(0, 1), digit(1, 1), digit(2, 1)].map(|d| d.unwrap_or(0) * 17)),
        _ => Err(bad()),
    }
}

/// The form switch's curve, CSS's `cubic-bezier(0.2, 0.9, 0.25, 1.08)`, at
/// linear progress `x` from 0 to 1: a quick start and a slight overshoot.
pub fn ease(x: f64) -> f64 {
    const X1: f64 = 0.2;
    const Y1: f64 = 0.9;
    const X2: f64 = 0.25;
    const Y2: f64 = 1.08;
    let axis = |p1: f64, p2: f64, t: f64| {
        let u = 1.0 - t;
        3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
    };
    let x = x.clamp(0.0, 1.0);
    if x == 0.0 || x == 1.0 {
        return x;
    }
    // x(t) grows with t on this curve: bisection finds it.
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..40 {
        let t = (lo + hi) / 2.0;
        if axis(X1, X2, t) < x {
            lo = t;
        } else {
            hi = t;
        }
    }
    axis(Y1, Y2, (lo + hi) / 2.0)
}

/// The bar's width `elapsed` after a switch to `to` began from `from`
/// pixels wide (logical pixels, fractional while it moves). A switch
/// reversed midway starts from where the bar is.
pub fn width_at(from: f64, to: Form, elapsed: Duration) -> f64 {
    let p = ease(elapsed.as_secs_f64() / SWITCH.as_secs_f64());
    from + (f64::from(to.width()) - from) * p
}
