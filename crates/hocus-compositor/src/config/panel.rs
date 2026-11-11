//! The bar: `[panel]`, its clock.

use super::*;

/// The bar, `[panel]` (docs/design/bar.md §2, §3).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Panel {
    /// The screens with a bar.
    pub screens: PanelScreens,
    /// The side of every screen the bar stands on (6.4); its edge zone,
    /// the windows' room and what opens from it go with it.
    pub side: PanelSide,
    /// The primary screen, by connector name. Empty: the built-in panel
    /// (eDP, LVDS, DSI) when it is connected, else the first screen.
    pub output: String,
    /// The bar's form on every screen; `[outputs.NAME] panel-form` sets one
    /// screen's.
    pub form: FormChoice,
    pub clock: Clock,
    /// The windows of the screen's current desktop over its desktops'
    /// tiles (#604); `false` hides them in both forms.
    pub windows: bool,
    /// The next meeting's card and mark (6.4); `false` hides them in both
    /// forms, the day still read.
    pub meeting: bool,
    /// Today's events after the meeting (6.4).
    pub today: bool,
    /// The to-do list and its mini item (6.4).
    pub tasks: bool,
    /// The agents' group and its mini item (6.4).
    pub agents: bool,
    /// The order of Today, the to-do list and the agents under the
    /// meeting's card and the sign of what is in use (6.4): an unknown
    /// name is skipped, a missing one comes after those named.
    #[serde(deserialize_with = "bar_order")]
    pub order: [hocus_link::BarGroup; 3],
}

impl Default for Panel {
    fn default() -> Self {
        Self {
            screens: PanelScreens::default(),
            side: PanelSide::default(),
            output: String::new(),
            form: FormChoice::default(),
            clock: Clock::default(),
            windows: true,
            meeting: true,
            today: true,
            tasks: true,
            agents: true,
            order: hocus_link::BarGroup::DEFAULT,
        }
    }
}

impl Panel {
    /// What the bar's column shows, as the shell is told it.
    pub fn groups(&self) -> hocus_link::BarGroups {
        hocus_link::BarGroups {
            order: self.order,
            meeting: self.meeting,
            today: self.today,
            tasks: self.tasks,
            agents: self.agents,
        }
    }
}

pub(super) fn bar_order<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<[hocus_link::BarGroup; 3], D::Error> {
    Ok(hocus_link::BarGroup::order_of(&Vec::<String>::deserialize(
        deserializer,
    )?))
}

/// The bar's clock, `[panel.clock]` (bar.md §4 group 1, §5): sent to the
/// shell as it connects.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Clock {
    /// The family of the hours and minutes: the bundled Martian Mono, or
    /// any installed family by name (the shell falls back to Martian Mono
    /// when it is not installed).
    #[serde(deserialize_with = "font_family")]
    pub font: String,
    /// `#RRGGBB` or `#RGB`.
    #[serde(deserialize_with = "color")]
    pub color: [u8; 3],
    /// On a minute change.
    #[serde(deserialize_with = "transition")]
    pub transition: hocus_link::Transition,
    /// The seconds beside the minutes, in the full form.
    pub seconds: bool,
}

impl Default for Clock {
    fn default() -> Self {
        let look = hocus_link::ClockLook::default();
        Self {
            font: look.font,
            color: look.color,
            transition: look.transition,
            seconds: look.seconds,
        }
    }
}

impl Clock {
    pub fn look(&self) -> hocus_link::ClockLook {
        hocus_link::ClockLook {
            font: self.font.clone(),
            color: self.color,
            transition: self.transition,
            seconds: self.seconds,
        }
    }
}

pub(super) fn font_family<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let family = String::deserialize(deserializer)?;
    // One line to the shell: no line breaks; spaces are kept.
    let family = family.split_whitespace().collect::<Vec<_>>().join(" ");
    if family.is_empty() {
        Ok(hocus_link::ClockLook::DEFAULT_FONT.to_owned())
    } else {
        Ok(family)
    }
}

pub(super) fn color<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[u8; 3], D::Error> {
    hocus_link::parse_color(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
}

pub(super) fn transition<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<hocus_link::Transition, D::Error> {
    String::deserialize(deserializer)?
        .parse()
        .map_err(serde::de::Error::custom)
}

/// Which screens have a bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PanelScreens {
    /// The primary screen only.
    #[default]
    Primary,
    All,
}

/// The side of the screens the bar stands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PanelSide {
    #[default]
    Right,
    Left,
}

impl PanelSide {
    /// As the shell is told it.
    pub const fn link(self) -> hocus_link::Side {
        match self {
            Self::Right => hocus_link::Side::Right,
            Self::Left => hocus_link::Side::Left,
        }
    }
}

/// The bar's form on a screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FormChoice {
    /// Full on a screen wide enough (bar.md §3), or as last chosen with a
    /// click or `Super+Shift+B`.
    #[default]
    Auto,
    Full,
    Mini,
}

impl FormChoice {
    /// As the file writes it.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Full => "full",
            Self::Mini => "mini",
        }
    }
}
