//! The settings' model: the 20 sections in their 5 groups, and every
//! setting a section holds, with what the window needs to show it, find
//! it and write it.
//!
//! Each section's ticket (6.2 to 6.17) fills its section: a module like
//! [`crate::power`] returns its settings ([`Setting`]), listed in
//! [`settings_of`]. A setting says
//!
//! - its id (`power.suspend-after`), its section and the group of rows it
//!   sits in on its page;
//! - its name and the line under it, in French and in English;
//! - a few nearby words in both languages, for the search;
//! - its kind: a switch, a menu of choices, a slider or a text;
//! - where it is written ([`Places`]): one key of the file, or one per
//!   power source (on battery, on AC), each with how its value is written
//!   ([`Codec`]) and the value that applies when the file says none.
//!
//! The window does the rest: it reads the file, shows each value (or warns
//! that one is not understood), writes a change at once with its undo, and
//! finds settings by their words and their keys.

use crate::lang::{Lang, Words};
use hocus_words::settings::model as words;

/// The five groups of the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    Personalisation,
    Devices,
    Connections,
    Work,
    System,
}

impl Group {
    pub const ALL: [Self; 5] = [
        Self::Personalisation,
        Self::Devices,
        Self::Connections,
        Self::Work,
        Self::System,
    ];

    pub fn label(self) -> Words {
        match self {
            Self::Personalisation => words::PERSONALISATION,
            Self::Devices => words::DEVICES,
            Self::Connections => words::CONNECTIONS,
            Self::Work => words::WORK,
            Self::System => words::SYSTEM,
        }
    }
}

/// The twenty sections, in the sidebar's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SectionId {
    Appearance,
    Wallpaper,
    Bar,
    Accessibility,
    Agenda,
    Screens,
    Sound,
    Keyboard,
    Mouse,
    Network,
    Bluetooth,
    Desktops,
    Apps,
    Agents,
    /// The agents that act on the desktop, their rights (#440).
    Acting,
    Captures,
    Power,
    Account,
    DateTime,
    /// A day of the desktop's CPU and memory (#655).
    Performance,
}

/// A section: its page and its row in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub id: SectionId,
    /// Its name on the command line and in the log: `hocus settings power`.
    pub slug: &'static str,
    pub group: Group,
    pub name: Words,
    /// What its page is for, under its title.
    pub line: Words,
    pub icon: Icon,
    /// The ticket that fills it.
    pub ticket: &'static str,
}

impl SectionId {
    pub const ALL: [Self; 20] = [
        Self::Appearance,
        Self::Wallpaper,
        Self::Bar,
        Self::Accessibility,
        Self::Agenda,
        Self::Screens,
        Self::Sound,
        Self::Keyboard,
        Self::Mouse,
        Self::Network,
        Self::Bluetooth,
        Self::Desktops,
        Self::Apps,
        Self::Agents,
        Self::Acting,
        Self::Captures,
        Self::Power,
        Self::Account,
        Self::DateTime,
        Self::Performance,
    ];

    // Over 200 lines: a table, one entry per section.
    pub fn section(self) -> Section {
        let s = |slug, group, name, line, icon, ticket| Section {
            id: self,
            slug,
            group,
            name,
            line,
            icon,
            ticket,
        };
        use Group::*;
        match self {
            Self::Appearance => s(
                "appearance",
                Personalisation,
                words::APPEARANCE_AND_LANGUAGE,
                words::THE_THEME_THE_ACCENT_COLOUR_THE,
                Icon::Appearance,
                "6.2",
            ),
            Self::Wallpaper => s(
                "wallpaper",
                Personalisation,
                words::WALLPAPER,
                words::EACH_SCREEN_S_BACKGROUND_AND_THE,
                Icon::Wallpaper,
                "6.3",
            ),
            Self::Bar => s(
                "bar",
                Personalisation,
                words::THE_BAR,
                words::THE_SIDE_IT_STANDS_ON_ITS,
                Icon::Bar,
                "6.4",
            ),
            Self::Accessibility => s(
                "accessibility",
                Personalisation,
                words::ACCESSIBILITY,
                words::SEE_LARGER_SEE_CLEARER_DO_EVERYTHING,
                Icon::Accessibility,
                "A.2–A.4",
            ),
            Self::Agenda => s(
                "agenda",
                Personalisation,
                words::CALENDAR_AND_TASKS,
                words::WHAT_THE_BAR_SHOWS_OF_YOUR,
                Icon::Agenda,
                "Day.1",
            ),
            Self::Screens => s(
                "screens",
                Devices,
                words::SCREENS,
                words::WHERE_EACH_SCREEN_SITS_ITS_RESOLUTION,
                Icon::Screens,
                "6.9",
            ),
            Self::Sound => s(
                "sound",
                Devices,
                words::SOUND,
                words::OUTPUTS_INPUTS_AND_THEIR_PROFILES_EACH,
                Icon::Sound,
                "6.5",
            ),
            Self::Keyboard => s(
                "keyboard",
                Devices,
                words::KEYBOARD_AND_SHORTCUTS,
                words::THE_LAYOUTS_THE_KEYS_REPEAT_AND,
                Icon::Keyboard,
                "6.8",
            ),
            Self::Mouse => s(
                "mouse",
                Devices,
                words::MOUSE_AND_TOUCHPAD,
                words::THE_POINTER_CLICKS_AND_SCROLLING_OF,
                Icon::Mouse,
                "6.12",
            ),
            Self::Network => s(
                "network",
                Connections,
                words::NETWORK,
                words::WI_FI_AND_ITS_SAVED_NETWORKS,
                Icon::Network,
                "6.6",
            ),
            Self::Bluetooth => s(
                "bluetooth",
                Connections,
                words::BLUETOOTH,
                words::PAIR_A_DEVICE_WITH_OR_WITHOUT,
                Icon::Bluetooth,
                "6.7",
            ),
            Self::Desktops => s(
                "desktops",
                Work,
                words::DESKTOPS_AND_WINDOWS,
                words::HOW_MANY_DESKTOPS_AND_THEIR_NAMES,
                Icon::Desktops,
                "6.14",
            ),
            Self::Apps => s(
                "applications",
                Work,
                words::APPLICATIONS,
                words::THOSE_THAT_OPEN_YOUR_LINKS_AND,
                Icon::Apps,
                "6.15",
            ),
            Self::Agents => s(
                "agents",
                Work,
                words::AGENTS,
                words::WHERE_YOUR_AGENTS_STATES_COME_FROM,
                Icon::Agents,
                "6.16",
            ),
            Self::Acting => s(
                "act-on-desktop",
                Work,
                words::ACT_ON_THE_DESKTOP,
                words::THE_AGENTS_YOU_PLUG_IN_HERE,
                Icon::Acting,
                "#440",
            ),
            Self::Captures => s(
                "captures",
                Work,
                words::SCREENSHOTS_AND_CLIPBOARD,
                words::WHERE_YOUR_SCREENSHOTS_AND_SCREEN_RECORDINGS,
                Icon::Captures,
                "6.17",
            ),
            Self::Power => s(
                "power",
                System,
                words::POWER,
                words::WHEN_THE_SCREEN_DIMS_LOCKS_AND,
                Icon::Power,
                "6.10",
            ),
            Self::Account => s(
                "account",
                System,
                words::ACCOUNT,
                words::YOUR_NAME_AVATAR_AND_PASSWORD_THE,
                Icon::Account,
                "6.11",
            ),
            Self::DateTime => s(
                "date-and-time",
                System,
                words::DATE_AND_TIME,
                words::THE_COMPUTER_S_TIME_AND_ITS,
                Icon::DateTime,
                "6.11",
            ),
            Self::Performance => s(
                "performance",
                System,
                words::PERFORMANCE,
                words::WHAT_THE_DESKTOP_THE_COMPOSITOR_AND,
                Icon::Performance,
                "#655",
            ),
        }
    }

    /// The section named `slug` on the command line.
    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.section().slug == slug)
    }

    pub fn name(self, lang: Lang) -> &'static str {
        self.section().name.get(lang)
    }
}

/// The sections' icons: the mockup's 24 px stroke paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    Appearance,
    Wallpaper,
    Bar,
    Accessibility,
    Agenda,
    Screens,
    Sound,
    Keyboard,
    Mouse,
    Network,
    Bluetooth,
    Desktops,
    Apps,
    Agents,
    /// An agent's pointer: Agir sur le bureau.
    Acting,
    Captures,
    Power,
    Account,
    DateTime,
    Performance,
    Home,
    Search,
    Sliders,
    Warning,
    Close,
    Chevron,
    /// The chevron pointing up: a group moved up (6.4).
    ChevronUp,
    Check,
    Arrow,
    /// A folder (6.17's path buttons).
    Folder,
}

impl Icon {
    /// Its path in a 24 × 24 box, drawn with round 1.8 px strokes.
    pub fn path(self) -> &'static str {
        match self {
            Self::Appearance => {
                "M12 3 A9 9 0 1 0 12 21 A9 9 0 1 0 12 3 Z M12 3 V21 M12 7.5 H17.5 M12 12 H20.8 M12 16.5 H17.5"
            }
            Self::Wallpaper => {
                "M3.5 5 H20.5 V19 H3.5 Z M3.5 15.5 L8.5 10.5 L13 15 L15.5 12.5 L20.5 17.5"
            }
            Self::Bar => "M3.5 4.5 H20.5 V19.5 H3.5 Z M15 4.5 V19.5 M17 8 H18.5 M17 11 H18.5",
            Self::Accessibility => {
                "M12 6.5 A1.8 1.8 0 1 0 12 2.9 A1.8 1.8 0 1 0 12 6.5 Z M5 8.5 L12 10 L19 8.5 M12 10 V14.5 M12 14.5 L8.5 21 M12 14.5 L15.5 21"
            }
            Self::Agenda => {
                "M4 6 H20 V20 H4 Z M4 10 H20 M8.5 3.5 V7.5 M15.5 3.5 V7.5 M8 14 H11 M13 14 H16 M8 17 H11"
            }
            Self::Screens => "M3 4.5 H21 V16 H3 Z M8 20 H16 M12 16 V20",
            Self::Sound => {
                "M4 9.5 H7.5 L12 5.5 V18.5 L7.5 14.5 H4 Z M15.5 9 A4 4 0 0 1 15.5 15 M18 6.5 A7.5 7.5 0 0 1 18 17.5"
            }
            Self::Keyboard => {
                "M3 6.5 H21 V17.5 H3 Z M6.5 10 H7.5 M10 10 H11 M13 10 H14 M16.5 10 H17.5 M7.5 14 H16.5"
            }
            Self::Mouse => {
                "M12 3 A6 6 0 0 0 6 9 V15 A6 6 0 0 0 18 15 V9 A6 6 0 0 0 12 3 Z M12 6.5 V10.5"
            }
            Self::Network => {
                "M2.5 9 A14 14 0 0 1 21.5 9 M5.5 12.5 A9.5 9.5 0 0 1 18.5 12.5 M8.5 16 A5 5 0 0 1 15.5 16 M12 19.5 V19.6"
            }
            Self::Bluetooth => "M7 7.5 L17 16.5 L12 20.5 V3.5 L17 7.5 L7 16.5",
            Self::Desktops => {
                "M3.5 5 H10.5 V11 H3.5 Z M13.5 5 H20.5 V11 H13.5 Z M3.5 13 H10.5 V19 H3.5 Z M13.5 13 H20.5 V19 H13.5 Z"
            }
            Self::Apps => {
                "M4 4 H10 V10 H4 Z M14 4 H20 V10 H14 Z M4 14 H10 V20 H4 Z M17 14 V20 M14 17 H20"
            }
            Self::Agents => {
                "M12 20.5 A7.5 7.5 0 1 0 12 5.5 A7.5 7.5 0 1 0 12 20.5 Z M5.2 9 A2.6 2.6 0 1 1 8.6 5.6 M18.8 9 A2.6 2.6 0 1 0 15.4 5.6 M9.5 12.5 V13.5 M14.5 12.5 V13.5"
            }
            Self::Acting => "M5.5 3.5 V18 L9.5 14.2 L12.3 20.5 L14.8 19.4 L12.1 13.2 H17.5 Z",
            Self::Captures => {
                "M3.5 8 H7 L9 5 H15 L17 8 H20.5 V19 H3.5 Z M12 16.2 A3 3 0 1 0 12 10.2 A3 3 0 1 0 12 16.2 Z"
            }
            Self::Power => "M3 7.5 H17.5 V16.5 H3 Z M20.5 10.5 V13.5 M10.8 9 L8.8 12 H11.8 L9.8 15",
            Self::Account => {
                "M12 12 A4 4 0 1 0 12 4 A4 4 0 1 0 12 12 Z M4.5 20.5 A7.5 7.5 0 0 1 19.5 20.5"
            }
            Self::DateTime => "M12 21 A9 9 0 1 0 12 3 A9 9 0 1 0 12 21 Z M12 7.5 V12 L15 14",
            Self::Performance => "M3 12.5 H7 L9.5 6 L14 19 L16.5 12.5 H21",
            Self::Home => "M3.5 11 L12 4 L20.5 11 M6 9.5 V20 H18 V9.5 M10 20 V14.5 H14 V20",
            Self::Search => {
                "M10.5 17.5 A7 7 0 1 0 10.5 3.5 A7 7 0 1 0 10.5 17.5 Z M15.6 15.6 L20.5 20.5"
            }
            Self::Sliders => "M4 7 H13 M17 7 H20 M15 4.5 V9.5 M4 17 H7 M11 17 H20 M9 14.5 V19.5",
            Self::Warning => "M12 4 L21 19.5 H3 Z M12 10 V14 M12 16.8 V17",
            Self::Close => "M6.5 6.5 L17.5 17.5 M17.5 6.5 L6.5 17.5",
            Self::Chevron => "M7 10 L12 15 L17 10",
            Self::ChevronUp => "M7 14 L12 9 L17 14",
            Self::Check => "M5 12.5 L10 17 L19 7.5",
            Self::Arrow => "M5 12 H19 M13.5 6.5 L19 12 L13.5 17.5",
            Self::Folder => "M3.5 6.5 H9.5 L11.5 8.5 H20.5 V18.5 H3.5 Z",
        }
    }
}

/// A power source: settings that hold one value on battery and one on AC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    Battery,
    Ac,
}

impl Source {
    pub fn name(self) -> Words {
        match self {
            Self::Battery => words::ON_BATTERY,
            Self::Ac => words::ON_AC,
        }
    }

    /// As the toast says it: ", sur batterie".
    pub fn aside(self) -> Words {
        match self {
            Self::Battery => words::ON_BATTERY_2,
            Self::Ac => words::ON_AC_2,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Self::Battery => "battery",
            Self::Ac => "ac",
        }
    }
}

/// A value a setting takes, as the window holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    /// A delay; `None`: never.
    Seconds(Option<u64>),
    /// One of a few words (`"suspend"`).
    Word(&'static str),
    Text(String),
    /// Several texts: a list of layouts, an action's shortcuts (6.8).
    List(Vec<String>),
}

/// How a value is written in the file, and read back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// `true` or `false`.
    Bool,
    /// A whole number from `min` to `max`.
    Int { min: i64, max: i64 },
    /// A number of seconds from 1 to 86400, or `"off"`: `[idle] delay`
    /// (docs/design/idle.md).
    IdleSeconds,
    /// `"0s"`, `"5m"`, `"1h"`, or `"never"`: 6.10's delays.
    Delay,
    /// One of these words.
    Word(&'static [&'static str]),
    /// Any text.
    Text,
    /// A number from `min` to `max` tenths, written as a decimal (`0.5`,
    /// `-1.0`; a whole number reads too): 6.12's speeds, held as tenths.
    Tenths { min: i64, max: i64 },
    /// An action's shortcuts: `"Super+Q"`, or a list of them, `""` or
    /// `[]` for none (6.8).
    Chords,
    /// A list of texts: `["fr", "us"]`.
    Texts,
    /// A colour, `"#RRGGBB"`, held in lower case (6.3's plain colour).
    Colour,
    /// A number from `min` to `max` hundredths, written as a decimal
    /// (`1.2`): 6.2's text size, held as a percentage.
    Hundredths { min: i64, max: i64 },
    /// One XKB option of a family in `[keyboard] options` (`grp:`,
    /// `caps:`, `compose:`): these words, the first `""` for none; the
    /// other options of the line are kept as they are, in their place.
    XkbOption(&'static [&'static str]),
}

/// What the options of a family start with (`grp:`), from its first
/// named option.
pub fn xkb_prefix(family: &[&str]) -> &'static str {
    match family
        .iter()
        .find(|f| !f.is_empty())
        .and_then(|f| f.split_once(':'))
    {
        Some(("grp", _)) => "grp:",
        Some(("caps", _)) => "caps:",
        Some(("compose", _)) => "compose:",
        _ => "\u{0}",
    }
}

/// `options` with `choice` in place of its family's option (or added at
/// the end), every other option kept in its place; `compose:caps` takes
/// Caps Lock, so a `caps:` option goes with it.
pub fn xkb_options(options: &str, family: &[&str], choice: &str) -> String {
    let prefix = xkb_prefix(family);
    let mut out: Vec<String> = Vec::new();
    let mut placed = false;
    for option in options.split(',').map(str::trim).filter(|o| !o.is_empty()) {
        if option.starts_with(prefix) {
            if !placed && !choice.is_empty() {
                out.push(choice.to_owned());
            }
            placed = true;
        } else if !(choice == "compose:caps" && option.starts_with("caps:")) {
            out.push(option.to_owned());
        }
    }
    if !placed && !choice.is_empty() {
        out.push(choice.to_owned());
    }
    out.join(",")
}

/// The longest idle delay the compositor takes (docs/design/idle.md).
const IDLE_MOST: u64 = 24 * 60 * 60;

impl Codec {
    /// What `item` (a value of the file) means, or `None` when it is not
    /// understood.
    pub fn read(self, item: &toml_edit::Value) -> Option<Value> {
        match self {
            Self::Bool => item.as_bool().map(Value::Bool),
            Self::Int { min, max } => item
                .as_integer()
                .filter(|n| (min..=max).contains(n))
                .map(Value::Int),
            Self::IdleSeconds => match (item.as_str(), item.as_integer()) {
                (Some("off"), _) => Some(Value::Seconds(None)),
                (_, Some(n)) => u64::try_from(n)
                    .ok()
                    .filter(|n| (1..=IDLE_MOST).contains(n))
                    .map(|n| Value::Seconds(Some(n))),
                _ => None,
            },
            Self::Delay => item.as_str().and_then(parse_delay).map(Value::Seconds),
            Self::Word(words) => item
                .as_str()
                .and_then(|s| words.iter().find(|w| **w == s))
                .map(|w| Value::Word(w)),
            Self::Text => item.as_str().map(|s| Value::Text(s.to_owned())),
            Self::Colour => item
                .as_str()
                .and_then(hocus_link::wallpaper::colour_of)
                .map(|rgb| Value::Text(hocus_link::wallpaper::colour_word(rgb))),
            Self::Chords => match (item.as_str(), item.as_array()) {
                (Some(one), _) => Some(Value::List(
                    (!one.trim().is_empty())
                        .then(|| one.trim().to_owned())
                        .into_iter()
                        .collect(),
                )),
                (_, Some(list)) => list
                    .iter()
                    .map(|v| v.as_str().map(|s| s.trim().to_owned()))
                    .collect::<Option<Vec<String>>>()
                    .map(|l| Value::List(l.into_iter().filter(|s| !s.is_empty()).collect())),
                _ => None,
            },
            Self::Texts => item
                .as_array()?
                .iter()
                .map(|v| v.as_str().map(ToOwned::to_owned))
                .collect::<Option<Vec<String>>>()
                .map(Value::List),
            Self::XkbOption(family) => {
                let prefix = xkb_prefix(family);
                let options = item.as_str()?;
                match options
                    .split(',')
                    .map(str::trim)
                    .find(|o| o.starts_with(prefix))
                {
                    None => Some(Value::Word(family[0])),
                    Some(option) => family
                        .iter()
                        .find(|f| **f == option)
                        .map(|f| Value::Word(f)),
                }
            }
            Self::Hundredths { min, max } => {
                let number = item
                    .as_float()
                    .or_else(|| item.as_integer().map(|n| n as f64))?;
                let hundredths = (number * 100.0).round();
                ((hundredths - number * 100.0).abs() < 1e-6
                    && (min as f64..=max as f64).contains(&hundredths))
                .then_some(Value::Int(hundredths as i64))
            }
            Self::Tenths { min, max } => {
                let number = item
                    .as_float()
                    .or_else(|| item.as_integer().map(|n| n as f64))?;
                let tenths = (number * 10.0).round();
                ((tenths - number * 10.0).abs() < 1e-6
                    && (min as f64..=max as f64).contains(&tenths))
                .then_some(Value::Int(tenths as i64))
            }
        }
    }

    /// A value this codec writes, for a key with no default to read.
    fn neutral(self) -> Value {
        match self {
            Self::Bool => Value::Bool(false),
            Self::Int { min, .. } | Self::Tenths { min, .. } | Self::Hundredths { min, .. } => {
                Value::Int(min)
            }
            Self::IdleSeconds | Self::Delay => Value::Seconds(None),
            Self::Word(words) | Self::XkbOption(words) => Value::Word(words.first().unwrap_or(&"")),
            Self::Text | Self::Colour => Value::Text(String::new()),
            Self::Chords | Self::Texts => Value::List(Vec::new()),
        }
    }

    /// `value` as the file says it; `None` when this codec cannot say it.
    pub fn write(self, value: &Value) -> Option<toml_edit::Value> {
        Some(match (self, value) {
            (Self::Bool, Value::Bool(b)) => (*b).into(),
            (Self::Int { min, max }, Value::Int(n)) if (min..=max).contains(n) => (*n).into(),
            (Self::IdleSeconds, Value::Seconds(None)) => "off".into(),
            (Self::IdleSeconds, Value::Seconds(Some(n))) if (1..=IDLE_MOST).contains(n) => {
                i64::try_from(*n).ok()?.into()
            }
            (Self::Delay, Value::Seconds(s)) => delay_word(*s).into(),
            (Self::Word(words), Value::Word(word)) if words.contains(word) => (*word).into(),
            (Self::Text, Value::Text(t)) => t.as_str().into(),
            (Self::Colour, Value::Text(t)) if hocus_link::wallpaper::colour_of(t).is_some() => {
                t.as_str().into()
            }
            (Self::Tenths { min, max }, Value::Int(n)) if (min..=max).contains(n) => {
                (*n as f64 / 10.0).into()
            }
            (Self::Hundredths { min, max }, Value::Int(n)) if (min..=max).contains(n) => {
                (*n as f64 / 100.0).into()
            }
            (Self::Chords, Value::List(list)) => match list.as_slice() {
                [] => "".into(),
                [one] => one.as_str().into(),
                many => toml_edit::Value::Array(many.iter().map(String::as_str).collect()),
            },
            (Self::Texts, Value::List(list)) => {
                toml_edit::Value::Array(list.iter().map(String::as_str).collect())
            }
            // The option alone; the store puts it among the line's others.
            (Self::XkbOption(family), Value::Word(word)) if family.contains(word) => (*word).into(),
            _ => return None,
        })
    }
}

/// `"5m"`, `"90s"`, `"1h"`, `"never"`: seconds, `None` for never.
pub fn parse_delay(text: &str) -> Option<Option<u64>> {
    if text == "never" {
        return Some(None);
    }
    let (number, unit) = text.split_at(text.find(|c: char| !c.is_ascii_digit())?);
    let n: u64 = number.parse().ok()?;
    let by = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        _ => return None,
    };
    n.checked_mul(by).filter(|s| *s <= IDLE_MOST).map(Some)
}

/// A delay as 6.10's keys write it: the largest whole unit.
pub fn delay_word(seconds: Option<u64>) -> String {
    match seconds {
        None => "never".to_owned(),
        Some(s) if s > 0 && s % 3600 == 0 => format!("{}h", s / 3600),
        Some(s) if s > 0 && s % 60 == 0 => format!("{}m", s / 60),
        Some(s) => format!("{s}s"),
    }
}

/// One key of the file: where a value is, how it is written, and what
/// applies when the file does not say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// Its tables then its name: `["power", "battery", "suspend-after"]`.
    pub key: &'static [&'static str],
    pub codec: Codec,
    pub default: Value,
}

impl Place {
    /// The key at `key`, its default the one the configuration's defaults
    /// list (`hocus_config::defaults`), as the compositor's.
    pub fn new(key: &'static [&'static str], codec: Codec) -> Self {
        let default = hocus_config::defaults::value(key)
            .and_then(|value| codec.read(value))
            .unwrap_or_else(|| {
                // A test walks every row: never in a build that passed it.
                tracing::error!(key = %key.join("."), "no default for this key");
                codec.neutral()
            });
        Self {
            key,
            codec,
            default,
        }
    }

    /// A key the file leaves unset by default, the system's value applying
    /// (`hocus_config::defaults::UNSET`): `shown` is what its row shows
    /// then.
    pub fn unset(key: &'static [&'static str], codec: Codec, shown: Value) -> Self {
        Self {
            key,
            codec,
            default: shown,
        }
    }

    /// `[power.battery] suspend-after`, as the file shows it.
    pub fn shown(&self) -> String {
        match self.key.split_last() {
            Some((name, [])) => (*name).to_owned(),
            Some((name, tables)) => format!("[{}] {name}", tables.join(".")),
            None => String::new(),
        }
    }

    /// `power.battery.suspend-after`, for the search.
    pub fn dotted(&self) -> String {
        self.key.join(".")
    }
}

/// The setting with one value at `key`, its default the configuration's.
pub fn one(key: &'static [&'static str], codec: Codec) -> Places {
    Places::One(Place::new(key, codec))
}

/// Where a setting is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Places {
    One(Place),
    /// One value on battery, one on AC.
    PerSource {
        battery: Place,
        ac: Place,
    },
    /// Written by its page's own code, its key holding a screen's name
    /// (`[outputs.NAME] mode`, 6.9): the place is for the search; the
    /// window reads and writes nothing of it as a slot.
    Page(Place),
}

impl Places {
    /// The place of the value for `source` (ignored by a setting with one).
    pub fn of(&self, source: Source) -> &Place {
        match self {
            Self::One(place) | Self::Page(place) => place,
            Self::PerSource { battery, ac } => match source {
                Source::Battery => battery,
                Source::Ac => ac,
            },
        }
    }

    pub fn per_source(&self) -> bool {
        matches!(self, Self::PerSource { .. })
    }

    pub fn all(&self) -> Vec<&Place> {
        match self {
            Self::One(place) | Self::Page(place) => vec![place],
            Self::PerSource { battery, ac } => vec![battery, ac],
        }
    }
}

/// A menu's choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub label: Words,
    pub value: Value,
}

/// How a setting is changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Switch,
    Menu(Vec<Choice>),
    /// From `min` to `max` by `step`, its value followed by `unit` (` %`).
    Slider {
        min: i64,
        max: i64,
        step: i64,
        unit: &'static str,
    },
    Text,
    /// A speed from `min` to `max` tenths around a middle at 0, by tenths:
    /// a bipolar slider "Lente … Rapide" that shows no number, its value
    /// said as a word (6.12).
    Speed {
        min: i64,
        max: i64,
    },
    /// An action's shortcuts, recorded (6.8's sub-page).
    Shortcut,
    /// The keyboard's layouts, a list of its own (6.8).
    Layouts,
    /// A few choices side by side, one lit (6.2's theme, cursor size and
    /// time); a value of the file that is none of them shows as one more.
    Segmented(Vec<Choice>),
    /// The accents' round swatches, a radio group (6.2).
    Swatches,
    /// A link to another section's page: "La barre ›" (6.2).
    Link(SectionId),
    /// Not a setting: what the desktop never takes, said on the right
    /// ("Toujours", 6.13's gestures). Nothing to focus or write.
    Note(Words),
}

/// A group of rows on a section's page: its label, in capitals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowGroup {
    pub label: Words,
    /// Its rows hold one value per power source: a segmented control on
    /// its right chooses which one they show.
    pub per_source: bool,
}

/// A setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    /// `power.suspend-after`.
    pub id: &'static str,
    pub section: SectionId,
    pub group: RowGroup,
    pub name: Words,
    /// The line under its name.
    pub line: Option<Words>,
    /// Nearby words, French then English.
    pub keywords: (&'static [&'static str], &'static [&'static str]),
    pub kind: Kind,
    pub places: Places,
    /// The switch it depends on: while that one is off, it does not apply
    /// and its control is inert.
    pub needs: Option<&'static str>,
}

impl Setting {
    /// What `value` reads as on its row and in the toast.
    pub fn say(&self, value: &Value, lang: Lang) -> String {
        if let Kind::Note(note) = &self.kind {
            return note.get(lang).to_owned();
        }
        if let Kind::Menu(choices) | Kind::Segmented(choices) = &self.kind
            && let Some(choice) = choices.iter().find(|c| &c.value == value)
        {
            return choice.label.get(lang).to_owned();
        }
        if self.kind == Kind::Swatches
            && let Value::Word(word) = value
            && let Some(accent) = hocus_link::theme::Accent::of(word)
        {
            return accent.name(lang).to_owned();
        }
        match value {
            Value::Bool(true) => words::ON.get(lang).to_owned(),
            Value::Bool(false) => words::OFF.get(lang).to_owned(),
            Value::Int(n) => match &self.kind {
                Kind::Slider { unit, .. } => format!("{n}{unit}"),
                Kind::Speed { .. } => say_speed(*n, lang).to_owned(),
                _ => n.to_string(),
            },
            Value::Seconds(None) => words::NEVER.get(lang).to_owned(),
            Value::Seconds(Some(s)) => say_seconds(*s, lang),
            Value::Word(word) => (*word).to_owned(),
            Value::Text(text) => text.clone(),
            Value::List(list) if list.is_empty() => words::NONE.get(lang).to_owned(),
            Value::List(list) => match self.kind {
                Kind::Shortcut => list
                    .iter()
                    .map(|c| crate::keys::shown(c, lang))
                    .collect::<Vec<_>>()
                    .join(", "),
                _ => list.join(", "),
            },
        }
    }
}

/// A speed in tenths from -10 to 10, as a word: très lente to très rapide.
pub fn say_speed(tenths: i64, lang: Lang) -> &'static str {
    match tenths {
        n if n <= -7 => words::VERY_SLOW.get(lang),
        n if n <= -3 => words::SLOW.get(lang),
        n if n < 3 => words::MEDIUM.get(lang),
        n if n < 7 => words::FAST.get(lang),
        _ => words::VERY_FAST.get(lang),
    }
}

/// `90 s`, `7 min`, `1 h 30`.
pub fn say_seconds(s: u64, lang: Lang) -> String {
    match s {
        0 => words::AT_ONCE.get(lang).to_owned(),
        s if s < 60 => format!("{s} s"),
        s if s % 3600 == 0 => format!("{} h", s / 3600),
        s if s < 3600 => format!("{} min", s / 60),
        s => format!("{} h {:02}", s / 3600, (s % 3600) / 60),
    }
}

/// The settings of `section`, in their page's order (none until its
/// ticket fills it).
pub fn settings_of(section: SectionId) -> &'static [Setting] {
    match section {
        SectionId::Power => crate::power::settings(),
        SectionId::Screens => crate::screens::settings(),
        SectionId::Mouse => crate::mouse::settings(),
        SectionId::Keyboard => crate::keyboard::settings(),
        SectionId::Appearance => crate::appearance::settings(),
        SectionId::Accessibility => crate::accessibility::settings(),
        SectionId::Wallpaper => crate::wallpaper::settings(),
        SectionId::Bar => crate::bar::settings(),
        SectionId::Desktops => crate::desktops::settings(),
        SectionId::Agents => crate::agents::settings(),
        SectionId::Apps => crate::applications::settings(),
        SectionId::Captures => crate::captures::settings(),
        SectionId::Performance => crate::perf::settings(),
        _ => &[],
    }
}

/// A setting that lives outside the file (in PipeWire, for one): its
/// section's page draws its row, with the id `row:{id}`, and the search
/// finds it as it finds the file's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outside {
    /// `sound.output`.
    pub id: &'static str,
    pub section: SectionId,
    /// Its group's label.
    pub group: Words,
    pub name: Words,
    /// Nearby words, French then English.
    pub keywords: (&'static [&'static str], &'static [&'static str]),
}

/// The settings of `section` that live outside the file.
pub fn outside_of(section: SectionId) -> &'static [Outside] {
    match section {
        SectionId::Sound => crate::sound::outside(),
        SectionId::Bar => crate::bar::outside(),
        SectionId::Bluetooth => crate::bluetooth::outside(),
        SectionId::DateTime => crate::time::outside(),
        SectionId::Account => crate::account::outside(),
        SectionId::Keyboard => crate::keyboard::outside(),
        SectionId::Network => crate::network::outside(),
        SectionId::Power => crate::power::outside(),
        SectionId::Desktops => crate::desktops::outside(),
        SectionId::Agents => crate::agents::outside(),
        SectionId::Acting => crate::acting::outside(),
        SectionId::Apps => crate::applications::outside(),
        SectionId::Captures => crate::captures::outside(),
        _ => &[],
    }
}

/// The setting outside the file `id`.
pub fn outside(id: &str) -> Option<&'static Outside> {
    SectionId::ALL
        .into_iter()
        .flat_map(|section| outside_of(section).iter())
        .find(|o| o.id == id)
}

/// Every setting, section after section.
pub fn all_settings() -> impl Iterator<Item = &'static Setting> {
    SectionId::ALL
        .into_iter()
        .flat_map(|section| settings_of(section).iter())
}

/// The setting `id`.
pub fn setting(id: &str) -> Option<&'static Setting> {
    all_settings().find(|s| s.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twenty_sections_in_five_groups() {
        assert_eq!(SectionId::ALL.len(), 20);
        let count = |g| {
            SectionId::ALL
                .iter()
                .filter(|s| s.section().group == g)
                .count()
        };
        assert_eq!(
            Group::ALL.map(count),
            [5, 4, 2, 5, 4],
            "the mockup's groups"
        );
        // Each group's sections come together, in the groups' order.
        let groups: Vec<Group> = SectionId::ALL.iter().map(|s| s.section().group).collect();
        let mut seen = groups.clone();
        seen.dedup();
        assert_eq!(seen, Group::ALL.to_vec());
        for id in SectionId::ALL {
            assert_eq!(SectionId::from_slug(id.section().slug), Some(id));
        }
    }

    #[test]
    fn each_section_is_a_page_of_the_actions_palette() {
        // The palette (#736) lists "Réglages › Son"…: the same pages, in
        // the same order, by the same names.
        let pages = hocus_link::actions::SETTINGS_PAGES;
        assert_eq!(pages.len(), SectionId::ALL.len());
        for (id, (slug, fr, en)) in SectionId::ALL.into_iter().zip(pages) {
            let section = id.section();
            assert_eq!(section.slug, slug);
            assert_eq!(section.name.get(hocus_words::Lang::French), fr, "{slug}");
            assert_eq!(section.name.get(hocus_words::Lang::English), en, "{slug}");
        }
    }

    #[test]
    fn delays_read_and_write_both_ways() {
        assert_eq!(parse_delay("5m"), Some(Some(300)));
        assert_eq!(parse_delay("1h"), Some(Some(3600)));
        assert_eq!(parse_delay("0s"), Some(Some(0)));
        assert_eq!(parse_delay("never"), Some(None));
        for bad in ["3 heures", "5", "m", "-1m", "2d", ""] {
            assert_eq!(parse_delay(bad), None, "{bad}");
        }
        assert_eq!(delay_word(Some(900)), "15m");
        assert_eq!(delay_word(Some(7200)), "2h");
        assert_eq!(delay_word(Some(0)), "0s");
        assert_eq!(delay_word(None), "never");
        let idle = Codec::IdleSeconds;
        let v = |text: &str| text.parse::<toml_edit::Value>().ok();
        let read = |text: &str| v(text).and_then(|v| idle.read(&v));
        assert_eq!(read("300"), Some(Value::Seconds(Some(300))));
        assert_eq!(read("\"off\""), Some(Value::Seconds(None)));
        assert_eq!(read("0"), None);
        assert_eq!(read("\"5m\""), None);
        assert_eq!(
            idle.write(&Value::Seconds(Some(600)))
                .map(|v| v.to_string()),
            Some("600".to_owned())
        );
        assert_eq!(
            idle.write(&Value::Seconds(None)).map(|v| v.to_string()),
            Some("\"off\"".to_owned())
        );
    }

    #[test]
    fn speeds_are_tenths_written_as_decimals() {
        let codec = Codec::Tenths { min: -10, max: 10 };
        let read = |text: &str| {
            text.parse::<toml_edit::Value>()
                .ok()
                .and_then(|v| codec.read(&v))
        };
        assert_eq!(read("0.5"), Some(Value::Int(5)));
        assert_eq!(read("-1"), Some(Value::Int(-10)));
        assert_eq!(read("0.0"), Some(Value::Int(0)));
        assert_eq!(read("1.5"), None);
        assert_eq!(read("0.25"), None);
        assert_eq!(read("\"fast\""), None);
        let written = |n| codec.write(&Value::Int(n)).map(|v| v.to_string());
        assert_eq!(written(5).as_deref(), Some("0.5"));
        assert_eq!(written(-10).as_deref(), Some("-1.0"));
        assert_eq!(written(0).as_deref(), Some("0.0"));
        assert_eq!(say_speed(-7, Lang::French), "très lente");
        assert_eq!(say_speed(-3, Lang::French), "lente");
        assert_eq!(say_speed(2, Lang::French), "moyenne");
        assert_eq!(say_speed(6, Lang::English), "fast");
        assert_eq!(say_speed(7, Lang::French), "très rapide");
    }

    #[test]
    fn an_xkb_option_is_one_of_the_line() {
        const CAPS: &[&str] = &["", "caps:ctrl_modifier", "caps:escape"];
        const COMPOSE: &[&str] = &["", "compose:ralt", "compose:caps"];
        let read = |codec: Codec, text: &str| codec.read(&toml_edit::Value::from(text));
        let line = "grp:alt_shift_toggle,caps:escape,lv3:ralt_switch";
        assert_eq!(
            read(Codec::XkbOption(CAPS), line),
            Some(Value::Word("caps:escape"))
        );
        assert_eq!(read(Codec::XkbOption(COMPOSE), line), Some(Value::Word("")));
        assert_eq!(read(Codec::XkbOption(CAPS), "caps:swapescape"), None);
        assert_eq!(
            xkb_options(line, CAPS, "caps:ctrl_modifier"),
            "grp:alt_shift_toggle,caps:ctrl_modifier,lv3:ralt_switch"
        );
        assert_eq!(
            xkb_options(line, CAPS, ""),
            "grp:alt_shift_toggle,lv3:ralt_switch"
        );
        assert_eq!(
            xkb_options(line, COMPOSE, "compose:caps"),
            "grp:alt_shift_toggle,lv3:ralt_switch,compose:caps"
        );
        let chords = |text: &str| {
            text.parse::<toml_edit::Value>()
                .ok()
                .and_then(|v| Codec::Chords.read(&v))
        };
        assert_eq!(
            chords("\"Super+L\""),
            Some(Value::List(vec!["Super+L".into()]))
        );
        assert_eq!(chords("\"\""), Some(Value::List(vec![])));
        assert_eq!(
            chords("[\"Super+Q\", \"Alt+F4\"]"),
            Some(Value::List(vec!["Super+Q".into(), "Alt+F4".into()]))
        );
        assert_eq!(
            Codec::Chords
                .write(&Value::List(vec!["Super+L".into()]))
                .map(|v| v.to_string()),
            Some("\"Super+L\"".to_owned())
        );
    }

    #[test]
    fn hundredths_are_written_as_decimals() {
        let codec = Codec::Hundredths { min: 80, max: 150 };
        let read = |text: &str| {
            text.parse::<toml_edit::Value>()
                .ok()
                .and_then(|v| codec.read(&v))
        };
        assert_eq!(read("1.2"), Some(Value::Int(120)));
        assert_eq!(read("1"), Some(Value::Int(100)));
        assert_eq!(read("1.6"), None);
        let written = |n| codec.write(&Value::Int(n)).map(|v| v.to_string());
        assert_eq!(written(110).as_deref(), Some("1.1"));
        assert_eq!(written(100).as_deref(), Some("1.0"));
    }

    /// Every row's key is one of the configuration's, with the default the
    /// compositor reads (`hocus_config::defaults`), its codec reading it;
    /// a key unset by default shows the system's. A page's own keys (a
    /// screen's, by its name) are its code's.
    #[test]
    fn every_row_has_the_configuration_s_key_and_default() {
        use hocus_config::defaults;
        let mut rows = 0;
        for setting in all_settings() {
            if matches!(setting.places, Places::Page(_)) {
                continue;
            }
            for place in setting.places.all() {
                rows += 1;
                let key = place.dotted();
                assert!(defaults::known(place.key), "{}: {key} unknown", setting.id);
                if defaults::UNSET.contains(&place.key) {
                    continue;
                }
                let shipped = defaults::value(place.key).and_then(|v| place.codec.read(v));
                assert_eq!(
                    shipped.as_ref(),
                    Some(&place.default),
                    "{}: {key}'s default",
                    setting.id
                );
            }
        }
        assert!(rows > 100, "{rows} rows");
    }

    /// `hocus_config::labels`, the table the shell names settings with,
    /// is these rows' keys, names and choices: written again with
    /// `HOCUS_LABELS_WRITE=1`.
    #[test]
    fn the_labels_are_the_rows() {
        let table = labels_table();
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../hocus-config/src/labels_table.rs"
        );
        if std::env::var_os("HOCUS_LABELS_WRITE").is_some() {
            std::fs::write(path, &table).expect("the table written");
        }
        let now = std::fs::read_to_string(path).unwrap_or_default();
        assert!(
            now == table,
            "hocus-config's labels are not the rows: HOCUS_LABELS_WRITE=1 cargo test -p \
             hocus-settings the_labels_are_the_rows"
        );
        // And what the lookup says is what the rows say, of each key one
        // row writes alone.
        let shared = shared_keys();
        for setting in all_settings() {
            for place in setting.places.all() {
                if matches!(setting.places, Places::Page(_)) || shared.contains(&place.dotted()) {
                    continue;
                }
                let label = hocus_config::labels::of(&place.dotted());
                // A name made per desktop ("Bureau 3") is no constant of
                // the catalogue: only the shortcuts', which no agent changes.
                let Some(label) = label else {
                    assert_eq!(
                        place.key.first(),
                        Some(&"shortcuts"),
                        "{} has no label",
                        setting.id
                    );
                    continue;
                };
                assert!(
                    Lang::ALL
                        .iter()
                        .any(|l| label.name.get(*l) == setting.name.get(*l)),
                    "{}",
                    setting.id
                );
            }
        }
    }

    /// The keys several rows write (`[keyboard] options`' families): the
    /// table names none of them, a row's name saying only its part.
    fn shared_keys() -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        let mut shared = Vec::new();
        for setting in all_settings() {
            for place in setting.places.all() {
                let key = place.dotted();
                if seen.contains(&key) {
                    shared.push(key);
                } else {
                    seen.push(key);
                }
            }
        }
        shared
    }

    /// The labels' table as Rust: each catalogue text by its constant.
    fn labels_table() -> String {
        use std::collections::BTreeMap;
        // Each plain text of the catalogue, by what it says in every
        // language: its constants' paths.
        let mut by_said: BTreeMap<Vec<String>, Vec<String>> = BTreeMap::new();
        let all: Vec<Vec<(String, String)>> =
            Lang::ALL.iter().map(|l| hocus_words::every(*l)).collect();
        for (i, (key, _)) in all[0].iter().enumerate() {
            if key.contains('[') {
                continue;
            }
            let said: Vec<String> = all.iter().map(|texts| texts[i].1.clone()).collect();
            if said.iter().any(|t| t.contains('{')) {
                continue;
            }
            by_said.entry(said).or_default().push(key.clone());
        }
        const KEYWORDS: [&str; 38] = [
            "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern",
            "false", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move",
            "mut", "pub", "ref", "return", "static", "struct", "trait", "true", "type", "unsafe",
            "use", "where", "while", "abstract", "final", "try",
        ];
        let text = |t: &Words| -> Option<String> {
            let said: Vec<String> = Lang::ALL.iter().map(|l| t.get(*l).to_owned()).collect();
            if let Some(key) = by_said.get(&said).and_then(|keys| keys.iter().min()) {
                let mut parts: Vec<String> = key
                    .split('.')
                    .map(|p| {
                        if KEYWORDS.contains(&p) {
                            format!("r#{p}")
                        } else {
                            p.to_owned()
                        }
                    })
                    .collect();
                if let Some(last) = parts.last_mut() {
                    *last = last.trim_start_matches("r#").to_ascii_uppercase();
                }
                return Some(format!("hocus_words::{}", parts.join("::")));
            }
            said.windows(2)
                .all(|w| w[0] == w[1])
                .then(|| format!("Text::same({:?})", said[0]))
        };
        let shared = shared_keys();
        let mut rows: BTreeMap<String, String> = BTreeMap::new();
        for setting in all_settings() {
            if matches!(setting.places, Places::Page(_)) {
                continue;
            }
            let Some(name) = text(&setting.name) else {
                continue;
            };
            for place in setting.places.all() {
                if shared.contains(&place.dotted()) {
                    continue;
                }
                let said = match &setting.kind {
                    Kind::Switch => "Said::Switch".to_owned(),
                    Kind::Menu(choices) | Kind::Segmented(choices) => {
                        let items: Vec<String> = choices
                            .iter()
                            .filter_map(|c| {
                                let value = place.codec.write(&c.value)?;
                                let value = match value.as_str() {
                                    Some(word) => word.to_owned(),
                                    None => value.decorated("", "").to_string(),
                                };
                                Some(format!("({value:?}, {})", text(&c.label)?))
                            })
                            .collect();
                        format!("Said::Choices(&[{}])", items.join(", "))
                    }
                    Kind::Slider { unit, .. } => format!("Said::Number({unit:?})"),
                    _ => "Said::Other".to_owned(),
                };
                rows.entry(place.dotted()).or_insert_with(|| {
                    format!(
                        "    Label {{\n        key: {:?},\n        name: {name},\n        said: {said},\n    }},\n",
                        place.dotted()
                    )
                });
            }
        }
        let mut out = String::from(
            "// Generated from the Settings' rows: see `labels.rs`. Do not edit.\n\n\
             /// Every key a row writes, in the dotted keys' order.\n\
             pub const ALL: &[Label] = &[\n",
        );
        for row in rows.values() {
            out.push_str(row);
        }
        out.push_str("];\n");
        out
    }

    #[test]
    fn values_are_said_in_both_languages() {
        assert_eq!(say_seconds(420, Lang::French), "7 min");
        assert_eq!(say_seconds(5400, Lang::English), "1 h 30");
        assert_eq!(say_seconds(0, Lang::French), "Aussitôt");
    }
}
