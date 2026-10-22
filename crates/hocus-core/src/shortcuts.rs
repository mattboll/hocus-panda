//! Shortcuts: any key combination bound to any action, from the
//! configuration over built-in defaults. Actions on desktops take a scope:
//! the output under the pointer, or all outputs.

use std::{collections::BTreeMap, fmt, str::FromStr};

use crate::Zone;

/// What an action acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Scope {
    /// The output under the pointer.
    #[default]
    PointerOutput,
    AllOutputs,
}

/// Something a key can do.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    Quit,
    /// Shows desktop `n` (from 1).
    Desktop(usize),
    NextDesktop(Scope),
    PreviousDesktop(Scope),
    /// Sends the focused window to desktop `n` (from 1) of the output under
    /// the pointer.
    SendToDesktop(usize),
    SendToNextDesktop,
    SendToPreviousDesktop,
    /// Sends the focused window to the output on its left or right.
    SendToOutputLeft,
    SendToOutputRight,
    ToggleMaximized,
    ToggleFullscreen,
    /// Snaps the focused window to a zone of its output.
    Snap(Zone),
    /// Tiles the shown desktop of the output under the pointer, or not.
    ToggleTiling,
    /// Makes the focused window the master of its tiled desktop.
    MakeMaster,
    GrowMaster,
    ShrinkMaster,
    /// Takes the focused window out of the tiling, or back in.
    ToggleFloating,
    Minimize,
    Close,
    /// The next keyboard layout.
    SwitchLayout,
    /// Opens the launcher, or closes it.
    Launcher,
    /// Opens quick settings on the screen under the pointer, or closes them.
    QuickSettings,
    /// Shows the drop-down terminal on the screen under the pointer, or
    /// hides it.
    Dropdown,
    /// Opens the settings app, or brings it forward.
    Settings,
    /// Locks the session now.
    Lock,
    /// Window switcher (#39): the windows of the output under the pointer,
    /// or of every output.
    SwitchWindows(Scope),
    /// Runs a command through the shell.
    Spawn(String),
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scope = |s: &Scope| if *s == Scope::AllOutputs { " all" } else { "" };
        match self {
            Self::Quit => write!(f, "quit"),
            Self::Desktop(n) => write!(f, "desktop {n}"),
            Self::NextDesktop(s) => write!(f, "next-desktop{}", scope(s)),
            Self::PreviousDesktop(s) => write!(f, "previous-desktop{}", scope(s)),
            Self::SendToDesktop(n) => write!(f, "send-to-desktop {n}"),
            Self::SendToNextDesktop => write!(f, "send-to-next-desktop"),
            Self::SendToPreviousDesktop => write!(f, "send-to-previous-desktop"),
            Self::SendToOutputLeft => write!(f, "send-to-output-left"),
            Self::SendToOutputRight => write!(f, "send-to-output-right"),
            Self::ToggleMaximized => write!(f, "toggle-maximized"),
            Self::Snap(zone) => write!(f, "snap {}", zone.name()),
            Self::ToggleTiling => write!(f, "toggle-tiling"),
            Self::MakeMaster => write!(f, "make-master"),
            Self::GrowMaster => write!(f, "grow-master"),
            Self::ShrinkMaster => write!(f, "shrink-master"),
            Self::ToggleFloating => write!(f, "toggle-floating"),
            Self::ToggleFullscreen => write!(f, "toggle-fullscreen"),
            Self::Minimize => write!(f, "minimize"),
            Self::Close => write!(f, "close"),
            Self::SwitchLayout => write!(f, "switch-layout"),
            Self::Launcher => write!(f, "launcher"),
            Self::QuickSettings => write!(f, "quick-settings"),
            Self::Dropdown => write!(f, "dropdown"),
            Self::Settings => write!(f, "settings"),
            Self::Lock => write!(f, "lock"),
            Self::SwitchWindows(s) => write!(f, "switch-windows{}", scope(s)),
            Self::Spawn(command) => write!(f, "spawn {command}"),
        }
    }
}

impl FromStr for Action {
    type Err = String;

    /// `"desktop 2"`, `"next-desktop all"`, `"spawn foot --server"`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let (name, rest) = s.split_once(' ').map_or((s, ""), |(n, r)| (n, r.trim()));
        let scope = || match rest {
            "" => Ok(Scope::PointerOutput),
            "all" => Ok(Scope::AllOutputs),
            other => Err(format!(
                "{name}: unknown scope {other:?} (nothing, or \"all\")"
            )),
        };
        let number = || match rest.parse::<usize>() {
            Ok(n @ 1..=9) => Ok(n),
            _ => Err(format!(
                "{name}: expected a desktop number from 1 to 9, got {rest:?}"
            )),
        };
        let bare = |action: Action| {
            if rest.is_empty() {
                Ok(action)
            } else {
                Err(format!("{name} takes nothing after it, got {rest:?}"))
            }
        };
        match name {
            "quit" => bare(Self::Quit),
            "desktop" => number().map(Self::Desktop),
            "next-desktop" => scope().map(Self::NextDesktop),
            "previous-desktop" => scope().map(Self::PreviousDesktop),
            "send-to-desktop" => number().map(Self::SendToDesktop),
            "send-to-next-desktop" => bare(Self::SendToNextDesktop),
            "send-to-previous-desktop" => bare(Self::SendToPreviousDesktop),
            "send-to-output-left" => bare(Self::SendToOutputLeft),
            "send-to-output-right" => bare(Self::SendToOutputRight),
            "toggle-maximized" => bare(Self::ToggleMaximized),
            "snap" => rest
                .parse()
                .map(Self::Snap)
                .map_err(|e| format!("snap: {e}")),
            "toggle-tiling" => bare(Self::ToggleTiling),
            "make-master" => bare(Self::MakeMaster),
            "grow-master" => bare(Self::GrowMaster),
            "shrink-master" => bare(Self::ShrinkMaster),
            "toggle-floating" => bare(Self::ToggleFloating),
            "toggle-fullscreen" => bare(Self::ToggleFullscreen),
            "minimize" => bare(Self::Minimize),
            "close" => bare(Self::Close),
            "switch-layout" => bare(Self::SwitchLayout),
            "launcher" => bare(Self::Launcher),
            "quick-settings" => bare(Self::QuickSettings),
            "dropdown" => bare(Self::Dropdown),
            "settings" => bare(Self::Settings),
            "lock" => bare(Self::Lock),
            "switch-windows" => scope().map(Self::SwitchWindows),
            "spawn" if !rest.is_empty() => Ok(Self::Spawn(rest.to_string())),
            "spawn" => Err("spawn: which command?".into()),
            other => Err(format!("unknown action {other:?}")),
        }
    }
}

/// A key, as the combination names it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    /// A digit of the top row, by position: the same keys on every layout.
    Digit(u8),
    /// Anything else, by its XKB keysym name in the active layout, lower
    /// case: `"p"`, `"right"`, `"return"`, `"f4"`.
    Named(String),
}

/// Modifiers that must be held, and no others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Modifiers {
    pub logo: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Combo {
    pub modifiers: Modifiers,
    pub key: Key,
}

impl FromStr for Combo {
    type Err = String;

    /// `"Super+Shift+2"`, `"Ctrl+Alt+Right"`, `"Alt+F4"`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut modifiers = Modifiers::default();
        let parts: Vec<&str> = s.split('+').map(str::trim).collect();
        let Some((key, mods)) = parts.split_last() else {
            return Err(format!("{s:?}: empty"));
        };
        for m in mods {
            match m.to_ascii_lowercase().as_str() {
                "super" | "logo" | "mod4" => modifiers.logo = true,
                "ctrl" | "control" => modifiers.ctrl = true,
                "alt" | "mod1" => modifiers.alt = true,
                "shift" => modifiers.shift = true,
                other => return Err(format!("{s:?}: unknown modifier {other:?}")),
            }
        }
        let lower = key.to_ascii_lowercase();
        let key = match lower.as_str() {
            "" => return Err(format!("{s:?}: no key")),
            d if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() => {
                Key::Digit(d.as_bytes()[0] - b'0')
            }
            "enter" => Key::Named("return".into()),
            "esc" => Key::Named("escape".into()),
            "pageup" => Key::Named("prior".into()),
            "pagedown" => Key::Named("next".into()),
            // The key left of 1, whatever the layout says it is.
            "backtick" => Key::Named("grave".into()),
            other => Key::Named(other.to_string()),
        };
        Ok(Self { modifiers, key })
    }
}

impl fmt::Display for Combo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let m = self.modifiers;
        for (on, name) in [
            (m.logo, "Super"),
            (m.ctrl, "Ctrl"),
            (m.alt, "Alt"),
            (m.shift, "Shift"),
        ] {
            if on {
                write!(f, "{name}+")?;
            }
        }
        match &self.key {
            Key::Digit(d) => write!(f, "{d}"),
            Key::Named(name) => {
                let mut chars = name.chars();
                let first = chars
                    .next()
                    .map(|c| c.to_ascii_uppercase())
                    .unwrap_or_default();
                write!(f, "{first}{}", chars.as_str())
            }
        }
    }
}

/// The built-in shortcuts, before the configuration.
pub fn defaults() -> Vec<(String, Action)> {
    let mut list: Vec<(String, Action)> = [
        ("Ctrl+Alt+BackSpace", Action::Quit),
        ("Ctrl+Alt+Right", Action::NextDesktop(Scope::PointerOutput)),
        (
            "Ctrl+Alt+Left",
            Action::PreviousDesktop(Scope::PointerOutput),
        ),
        ("Ctrl+Alt+Shift+Right", Action::SendToNextDesktop),
        ("Ctrl+Alt+Shift+Left", Action::SendToPreviousDesktop),
        ("Super+Shift+Left", Action::SendToOutputLeft),
        ("Super+Shift+Right", Action::SendToOutputRight),
        ("Super+Up", Action::ToggleMaximized),
        ("Super+Left", Action::Snap(Zone::Left)),
        ("Super+Right", Action::Snap(Zone::Right)),
        ("Super+Down", Action::Minimize),
        ("Super+F11", Action::ToggleFullscreen),
        ("Super+Shift+T", Action::ToggleTiling),
        ("Super+Shift+J", Action::MakeMaster),
        ("Super+Shift+L", Action::GrowMaster),
        ("Super+Shift+H", Action::ShrinkMaster),
        ("Super+Shift+F", Action::ToggleFloating),
        ("Alt+F4", Action::Close),
        ("Super+Ctrl+Space", Action::SwitchLayout),
        ("Super+Space", Action::Launcher),
        ("Super+A", Action::QuickSettings),
        ("Super+Backtick", Action::Dropdown),
        ("Super+Comma", Action::Settings),
        ("Super+L", Action::Lock),
        ("Alt+Tab", Action::SwitchWindows(Scope::PointerOutput)),
        ("Super+Tab", Action::SwitchWindows(Scope::AllOutputs)),
    ]
    .into_iter()
    .map(|(combo, action)| (combo.to_string(), action))
    .collect();
    for n in 1..=9 {
        list.push((format!("Super+{n}"), Action::Desktop(n)));
        list.push((format!("Super+Shift+{n}"), Action::SendToDesktop(n)));
    }
    list
}

/// The shortcuts in effect, and what went wrong reading them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shortcuts {
    pub bindings: BTreeMap<Combo, Action>,
    /// Lines the configuration had that were left out, and why.
    pub warnings: Vec<String>,
}

impl Shortcuts {
    /// The defaults, then the configuration's `[shortcuts]` in file order:
    /// `"none"` removes a binding; a combination bound twice in the file
    /// keeps its first action, and the second is reported.
    pub fn resolve<'a>(config: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut shortcuts = Self::default();
        for (combo, action) in defaults() {
            if let Ok(combo) = combo.parse() {
                shortcuts.bindings.insert(combo, action);
            }
        }
        let mut from_file: BTreeMap<Combo, String> = BTreeMap::new();
        for (text, action) in config {
            let combo = match text.parse::<Combo>() {
                Ok(combo) => combo,
                Err(err) => {
                    shortcuts.warnings.push(format!("shortcuts: {err}"));
                    continue;
                }
            };
            if let Some(first) = from_file.get(&combo) {
                shortcuts.warnings.push(format!(
                    "shortcuts: {text:?} is the same keys as {first:?}; the first one wins"
                ));
                continue;
            }
            from_file.insert(combo.clone(), text.to_string());
            if action.trim() == "none" {
                shortcuts.bindings.remove(&combo);
                continue;
            }
            match action.parse() {
                Ok(action) => {
                    shortcuts.bindings.insert(combo, action);
                }
                Err(err) => shortcuts
                    .warnings
                    .push(format!("shortcuts.{text:?}: {err}")),
            }
        }
        shortcuts
    }

    pub fn action(&self, combo: &Combo) -> Option<&Action> {
        self.bindings.get(combo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combo(s: &str) -> Combo {
        s.parse().unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn combinations_read_in_any_case_and_order_of_writing() {
        assert_eq!(combo("Ctrl+Alt+Right"), combo("alt+ctrl+right"));
        assert_eq!(combo("Super+2").key, Key::Digit(2));
        assert_eq!(combo("Super+Enter").key, Key::Named("return".into()));
        assert_eq!(combo("Super+Shift+S").to_string(), "Super+Shift+S");
        assert!("Hyper+X".parse::<Combo>().is_err());
    }

    #[test]
    fn actions_read_with_their_scope_and_argument() {
        assert_eq!(
            "next-desktop".parse(),
            Ok(Action::NextDesktop(Scope::PointerOutput))
        );
        assert_eq!(
            "next-desktop all".parse(),
            Ok(Action::NextDesktop(Scope::AllOutputs))
        );
        assert_eq!("desktop 3".parse(), Ok(Action::Desktop(3)));
        assert_eq!(
            "spawn foot --server".parse(),
            Ok(Action::Spawn("foot --server".into()))
        );
        assert!("desktop 12".parse::<Action>().is_err());
        assert!("fly".parse::<Action>().is_err());
        for (_, action) in defaults() {
            assert_eq!(action.to_string().parse(), Ok(action));
        }
    }

    #[test]
    fn the_file_adds_replaces_and_removes_defaults() {
        let shortcuts = Shortcuts::resolve([
            ("Super+Return", "spawn foot"),
            ("Super+Up", "toggle-fullscreen"),
            ("Alt+F4", "none"),
        ]);
        assert_eq!(
            shortcuts.action(&combo("Super+Return")),
            Some(&Action::Spawn("foot".into()))
        );
        assert_eq!(
            shortcuts.action(&combo("Super+Up")),
            Some(&Action::ToggleFullscreen)
        );
        assert_eq!(shortcuts.action(&combo("Alt+F4")), None);
        assert_eq!(
            shortcuts.action(&combo("Super+2")),
            Some(&Action::Desktop(2))
        );
        assert!(shortcuts.warnings.is_empty());
    }

    #[test]
    fn a_conflict_is_reported_and_the_first_wins() {
        let shortcuts = Shortcuts::resolve([
            ("Super+Return", "spawn foot"),
            ("super+enter", "spawn kitty"),
            ("Super+K", "fly"),
        ]);
        assert_eq!(
            shortcuts.action(&combo("Super+Return")),
            Some(&Action::Spawn("foot".into()))
        );
        assert_eq!(shortcuts.warnings.len(), 2);
        assert!(
            shortcuts.warnings[0].contains("the first one wins"),
            "{:?}",
            shortcuts.warnings
        );
    }
}
