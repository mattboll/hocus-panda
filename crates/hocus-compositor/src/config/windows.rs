//! The desktops and the windows: `[desktops]`, `[windows]`, `[ink]`, `[overview]`.

use super::*;

/// Virtual desktops, `[desktops]` (docs/design/windows.md §12).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Desktops {
    /// Desktops on each screen, from 1 to 9.
    #[serde(deserialize_with = "desktop_count")]
    pub count: usize,
    /// Their names, by position (6.14): one missing or empty is "Bureau
    /// N" (the shell's words).
    pub names: Vec<String>,
    /// A project's browsers' tabs are read from their session files and
    /// come back with it (docs/design/projects.md §8.3, §13 F); off, no
    /// browser file is read and browsers come back as plain applications.
    #[serde(rename = "remember-tabs")]
    pub remember_tabs: bool,
}

impl Default for Desktops {
    fn default() -> Self {
        Self {
            count: crate::desktops::DEFAULT_COUNT,
            names: Vec::new(),
            remember_tabs: true,
        }
    }
}

impl Desktops {
    /// Desktop `index`'s name (from 0), if it has one.
    pub fn name(&self, index: usize) -> Option<&str> {
        self.names
            .get(index)
            .map(|n| n.trim())
            .filter(|n| !n.is_empty())
    }

    /// The names the desktops there are have, the empty ones last left
    /// out: what the shell is told.
    pub fn shown_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .names
            .iter()
            .take(self.count)
            .map(|n| n.trim().to_owned())
            .collect();
        while names.last().is_some_and(String::is_empty) {
            names.pop();
        }
        names
    }
}

pub(super) fn desktop_count<'de, D: Deserializer<'de>>(deserializer: D) -> Result<usize, D::Error> {
    let count = usize::deserialize(deserializer)?;
    if (1..=crate::desktops::MAX_COUNT).contains(&count) {
        Ok(count)
    } else {
        Err(serde::de::Error::custom(format!(
            "{count} desktops: from 1 to 9"
        )))
    }
}

/// How windows move, snap and switch, `[windows]` (6.14;
/// docs/design/windows.md §6, §8, §9, §11).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Windows {
    /// The key held to move a window by a drag anywhere in it (`Alt`).
    pub move_modifier: DragKey,
    /// The key held to resize one by a right-button drag (`Super`).
    pub resize_modifier: DragKey,
    /// A moved window's edges stick to others' and the work area's.
    pub magnet: bool,
    /// Dragged against an edge of the work area, it snaps there.
    pub snap_by_drag: bool,
    /// A moved window wobbles (never with reduced motion).
    pub wobble: bool,
    /// How applications' windows open and close (#432).
    pub animation: Animation,
    /// Which windows Alt+Tab lists.
    pub alt_tab: AltTab,
    /// Rules per application, `[[windows.rules]]`, in the file's order:
    /// the first of an application's applies.
    pub rules: Vec<Rule>,
}

impl Windows {
    /// The rule of the application that goes by `app_id`, if any.
    pub fn rule(&self, app_id: &str) -> Option<&Rule> {
        if app_id.is_empty() {
            return None;
        }
        self.rules
            .iter()
            .find(|r| r.app_id.eq_ignore_ascii_case(app_id))
    }
}

/// How an application's windows open (6.14): on a desktop, at a size,
/// above the others. A dialog follows its parent, not the rule.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Rule {
    pub app_id: String,
    /// The desktop it opens on (from 1), on the screen it opens on; none:
    /// the one the screen shows.
    #[serde(default, deserialize_with = "rule_desktop")]
    pub desktop: Option<usize>,
    #[serde(default)]
    pub size: OpenSize,
    /// Kept above the others, even without the focus; a fullscreen
    /// window in front covers it.
    #[serde(default)]
    pub above: bool,
}

pub(super) fn rule_desktop<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<usize>, D::Error> {
    let desktop = usize::deserialize(deserializer)?;
    if (1..=crate::desktops::MAX_COUNT).contains(&desktop) {
        Ok(Some(desktop))
    } else {
        Err(serde::de::Error::custom(format!(
            "desktop {desktop}: from 1 to 9"
        )))
    }
}

/// The size a window opens at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenSize {
    /// Its application's last size (§5).
    #[default]
    Last,
    Maximized,
    Left,
    Right,
    Fullscreen,
}

impl OpenSize {
    /// The mode it opens in, none for its last size.
    pub fn mode(self) -> Option<crate::model::Mode> {
        use crate::model::{Mode, Zone};
        match self {
            Self::Last => None,
            Self::Maximized => Some(Mode::Snapped(Zone::Maximized)),
            Self::Left => Some(Mode::Snapped(Zone::Left)),
            Self::Right => Some(Mode::Snapped(Zone::Right)),
            Self::Fullscreen => Some(Mode::Fullscreen),
        }
    }
}

impl Default for Windows {
    fn default() -> Self {
        Self {
            move_modifier: DragKey::Alt,
            resize_modifier: DragKey::Super,
            magnet: true,
            snap_by_drag: true,
            wobble: true,
            animation: Animation::Standard,
            alt_tab: AltTab::All,
            rules: Vec::new(),
        }
    }
}

/// How applications' windows open and close, `[windows] animation`
/// (docs/design/windows.md §5): each theme with a look of its own brings
/// its own, and any can be chosen in any theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Animation {
    /// Growing from 94 % as they fade in; shrinking a little as they go.
    #[default]
    Standard,
    /// Columns of glyphs falling through the window (#432).
    Rain,
    /// Givre's thawing pane (#740): the window seen through ice thawing
    /// to clear glass.
    Frost,
}

/// The key held for a drag anywhere in a window: nested, `Super` is
/// `Alt+Super`, as every shortcut there (windows.md §13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DragKey {
    Alt,
    Super,
    /// None: title bars, the grip and the frame only.
    None,
}

/// Which windows Alt+Tab lists (windows.md §11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AltTab {
    /// This desktop's first, then the others' after a line, dimmed.
    All,
    /// This desktop's only.
    Desktop,
}

/// Drawing on the screen, `[ink]` (docs/design/ink.md).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Ink {
    /// Off, `Super`+drag and the laser do nothing, and pass to the window.
    pub enabled: bool,
}

impl Default for Ink {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// The overview, `[overview]` (docs/design/windows.md §12b).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Overview {
    /// The pointer held in a screen's top-left corner opens it (#727).
    pub hot_corner: bool,
}

impl Default for Overview {
    fn default() -> Self {
        Self { hot_corner: true }
    }
}
