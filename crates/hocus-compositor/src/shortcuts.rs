//! The desktop's own keyboard shortcuts (docs/design/windows.md §13): the
//! chords the `[shortcuts]` section binds to actions, and what each action
//! does. Nested, `Super` is `Alt+Super`, so that the host keeps its own.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer};
use smithay::input::keyboard::{Keysym, ModifiersState, xkb};

use crate::{
    State,
    model::{Side, Snap},
    programs,
};

/// What a shortcut does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Opens a terminal (`[programs] terminal`).
    Terminal,
    /// Ends the session: the display manager comes back.
    LogOut,
    /// Asks the focused window to close.
    Close,
    /// Minimises the focused window.
    Minimize,
    /// Brings back the window minimised last.
    Unminimize,
    /// The keys of windows.md §9 on the focused window.
    Snap(Snap),
    /// Focuses the window on that side.
    FocusToward(i32, i32),
    /// Alt+Tab, or Alt+Shift+Tab (§11).
    SwitchWindows { backward: bool },
    /// The desktop on that side, of the screen under the pointer (§12).
    DesktopBy(i32),
    /// That desktop (from 0).
    Desktop(usize),
    /// Sends the focused window to that desktop.
    SendToDesktop(usize),
    /// And goes there with it.
    TakeToDesktop(usize),
}

/// Modifiers and a key, as `Super+Shift+E`. The key is the one of the
/// keyboard's first Latin layout, so that a shortcut stays where it is
/// whatever layout is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub logo: bool,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// Lower case for letters.
    pub key: Keysym,
}

impl Chord {
    /// Whether this chord is what is pressed: exactly these modifiers (and
    /// `Alt` with `Super` when nested), and the key.
    pub fn matches(&self, mods: &ModifiersState, key: Keysym, nested: bool) -> bool {
        let alt = self.alt || (nested && self.logo);
        mods.logo == self.logo
            && mods.shift == self.shift
            && mods.ctrl == self.ctrl
            && mods.alt == alt
            && key == self.key
    }
}

/// A chord that does not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseChordError(String);

impl fmt::Display for ParseChordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseChordError {}

impl FromStr for Chord {
    type Err = ParseChordError;

    /// `Super+Shift+E`: modifiers (`Super`, `Shift`, `Ctrl`, `Alt`, in any
    /// case and order) then an XKB key name (`Return`, `E`, `F1`, `space`).
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let err = |why: &str| ParseChordError(format!("shortcut {text:?}: {why}"));
        let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let key = parts
            .pop()
            .filter(|k| !k.is_empty())
            .ok_or_else(|| err("no key"))?;
        let mut chord = Self {
            logo: false,
            shift: false,
            ctrl: false,
            alt: false,
            key: Keysym::NoSymbol,
        };
        for modifier in parts {
            let held = match modifier.to_ascii_lowercase().as_str() {
                "super" | "logo" | "mod4" => &mut chord.logo,
                "shift" => &mut chord.shift,
                "ctrl" | "control" => &mut chord.ctrl,
                "alt" | "mod1" => &mut chord.alt,
                _ => return Err(err(&format!("unknown modifier {modifier:?}"))),
            };
            if std::mem::replace(held, true) {
                return Err(err(&format!("{modifier} twice")));
            }
        }
        // XKB is handed a C string.
        if key.contains('\0') {
            return Err(err("not a key name"));
        }
        // Case-insensitive, letters come lower case: what a key's first
        // level gives.
        chord.key = xkb::keysym_from_name(key, xkb::KEYSYM_CASE_INSENSITIVE);
        if chord.key == Keysym::NoSymbol {
            return Err(err(&format!("unknown key {key:?}")));
        }
        Ok(chord)
    }
}

/// The `[shortcuts]` section: every shortcut of the desktop, in one place,
/// with its default and its action. An empty string turns one off.
macro_rules! shortcuts {
    ($($field:ident: $default:expr => $action:expr,)*) => {
        #[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
        #[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
        pub struct Shortcuts {
            $(
                #[serde(deserialize_with = "chord")]
                pub $field: Option<Chord>,
            )*
        }

        impl Default for Shortcuts {
            fn default() -> Self {
                let chord = |text: &str| text.parse().ok();
                Self {
                    $($field: chord($default),)*
                }
            }
        }

        impl Shortcuts {
            /// Every shortcut and its action, in the file's order.
            pub fn all(&self) -> Vec<(Option<Chord>, Action)> {
                vec![$((self.$field, $action),)*]
            }
        }
    };
}

shortcuts! {
    terminal: "Super+Return" => Action::Terminal,
    log_out: "Super+Shift+E" => Action::LogOut,
    close: "Super+Q" => Action::Close,
    minimize: "Super+H" => Action::Minimize,
    unminimize: "Super+Shift+H" => Action::Unminimize,
    snap_left: "Super+Left" => Action::Snap(Snap::Half(Side::Start)),
    snap_right: "Super+Right" => Action::Snap(Snap::Half(Side::End)),
    maximize: "Super+Up" => Action::Snap(Snap::Maximize),
    restore: "Super+Down" => Action::Snap(Snap::Restore),
    quarter_up: "Super+Shift+Up" => Action::Snap(Snap::QuarterTop),
    quarter_down: "Super+Shift+Down" => Action::Snap(Snap::QuarterBottom),
    quarter_left: "Super+Shift+Left" => Action::Snap(Snap::QuarterLeft),
    quarter_right: "Super+Shift+Right" => Action::Snap(Snap::QuarterRight),
    focus_left: "" => Action::FocusToward(-1, 0),
    focus_right: "" => Action::FocusToward(1, 0),
    focus_up: "" => Action::FocusToward(0, -1),
    focus_down: "" => Action::FocusToward(0, 1),
    switch_windows: "Alt+Tab" => Action::SwitchWindows { backward: false },
    switch_windows_backward: "Alt+Shift+Tab" => Action::SwitchWindows { backward: true },
    desktop_left: "Super+Ctrl+Left" => Action::DesktopBy(-1),
    desktop_right: "Super+Ctrl+Right" => Action::DesktopBy(1),
    desktop_1: "Super+1" => Action::Desktop(0),
    desktop_2: "Super+2" => Action::Desktop(1),
    desktop_3: "Super+3" => Action::Desktop(2),
    desktop_4: "Super+4" => Action::Desktop(3),
    desktop_5: "Super+5" => Action::Desktop(4),
    desktop_6: "Super+6" => Action::Desktop(5),
    desktop_7: "Super+7" => Action::Desktop(6),
    desktop_8: "Super+8" => Action::Desktop(7),
    desktop_9: "Super+9" => Action::Desktop(8),
    send_to_desktop_1: "Super+Shift+1" => Action::SendToDesktop(0),
    send_to_desktop_2: "Super+Shift+2" => Action::SendToDesktop(1),
    send_to_desktop_3: "Super+Shift+3" => Action::SendToDesktop(2),
    send_to_desktop_4: "Super+Shift+4" => Action::SendToDesktop(3),
    send_to_desktop_5: "Super+Shift+5" => Action::SendToDesktop(4),
    send_to_desktop_6: "Super+Shift+6" => Action::SendToDesktop(5),
    send_to_desktop_7: "Super+Shift+7" => Action::SendToDesktop(6),
    send_to_desktop_8: "Super+Shift+8" => Action::SendToDesktop(7),
    send_to_desktop_9: "Super+Shift+9" => Action::SendToDesktop(8),
    take_to_desktop_1: "Super+Ctrl+Shift+1" => Action::TakeToDesktop(0),
    take_to_desktop_2: "Super+Ctrl+Shift+2" => Action::TakeToDesktop(1),
    take_to_desktop_3: "Super+Ctrl+Shift+3" => Action::TakeToDesktop(2),
    take_to_desktop_4: "Super+Ctrl+Shift+4" => Action::TakeToDesktop(3),
    take_to_desktop_5: "Super+Ctrl+Shift+5" => Action::TakeToDesktop(4),
    take_to_desktop_6: "Super+Ctrl+Shift+6" => Action::TakeToDesktop(5),
    take_to_desktop_7: "Super+Ctrl+Shift+7" => Action::TakeToDesktop(6),
    take_to_desktop_8: "Super+Ctrl+Shift+8" => Action::TakeToDesktop(7),
    take_to_desktop_9: "Super+Ctrl+Shift+9" => Action::TakeToDesktop(8),
}

impl Shortcuts {
    /// The action of what is pressed, if it is a shortcut. Nested,
    /// Alt+Tab takes `Super` too, as every other shortcut: the host keeps
    /// its own Alt+Tab.
    pub fn action(&self, mods: &ModifiersState, key: Keysym, nested: bool) -> Option<Action> {
        self.all().into_iter().find_map(|(chord, action)| {
            let mut chord = chord?;
            if nested && matches!(action, Action::SwitchWindows { .. }) && chord.alt {
                chord.logo = true;
            }
            chord.matches(mods, key, nested).then_some(action)
        })
    }

    /// The chord that opened the switcher, as it is held (nested, with
    /// `Super`).
    pub fn switcher_chord(&self, backward: bool, nested: bool) -> Option<Chord> {
        let mut chord = if backward {
            self.switch_windows_backward
        } else {
            self.switch_windows
        }?;
        if nested && chord.alt {
            chord.logo = true;
        }
        Some(chord)
    }
}

fn chord<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Chord>, D::Error> {
    let text = String::deserialize(deserializer)?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    text.parse().map(Some).map_err(serde::de::Error::custom)
}

/// Why the compositor is ending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// The user logged out.
    LoggedOut,
    /// SIGTERM or SIGINT.
    Signalled,
}

impl State {
    pub fn run_action(&mut self, action: Action) {
        tracing::info!(?action, "shortcut");
        match action {
            Action::Terminal => {
                // The window it opens takes the focus (§4).
                self.spawned_at = Some(self.now());
                let env = programs::Env::of(self).with_activation(self.activation_token());
                programs::spawn_terminal(self.config.programs.terminal.clone(), env);
            }
            Action::LogOut => self.end(Ending::LoggedOut),
            Action::Close => self.close_focused(),
            Action::Minimize => {
                if let Some(id) = self.model.focused() {
                    self.minimize(id);
                }
            }
            Action::Unminimize => self.unminimize_last(),
            Action::Snap(snap) => self.keyboard_snap(snap),
            Action::FocusToward(dx, dy) => self.focus_toward(dx, dy),
            Action::SwitchWindows { backward } => {
                if let Some(chord) = self.config.shortcuts.switcher_chord(backward, self.nested) {
                    let hold = crate::switcher::Hold {
                        alt: chord.alt,
                        logo: chord.logo,
                        ctrl: chord.ctrl,
                    };
                    self.open_switcher(backward, hold, chord.key);
                }
            }
            Action::DesktopBy(delta) => self.switch_desktop_by(delta),
            Action::Desktop(to) => self.switch_desktop_to(to),
            Action::SendToDesktop(to) => self.send_to_desktop(to, false),
            Action::TakeToDesktop(to) => self.send_to_desktop(to, true),
        }
    }

    /// Stops the loop: the compositor ends, and with it the session.
    pub fn end(&mut self, why: Ending) {
        self.ending = Some(why);
        self.loop_signal.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(logo: bool, shift: bool, ctrl: bool, alt: bool) -> ModifiersState {
        ModifiersState {
            logo,
            shift,
            ctrl,
            alt,
            ..ModifiersState::default()
        }
    }

    #[test]
    fn a_chord_reads_in_any_case_and_order() -> anyhow::Result<()> {
        let chord: Chord = "shift+SUPER+e".parse()?;
        assert_eq!(chord, "Super+Shift+E".parse()?);
        assert!(chord.logo && chord.shift && !chord.ctrl && !chord.alt);
        assert_eq!(chord.key, Keysym::e);
        assert_eq!("Ctrl+Alt+return".parse::<Chord>()?.key, Keysym::Return);
        assert_eq!("F11".parse::<Chord>()?.key, Keysym::F11);
        for bad in [
            "",
            "Super+",
            "Hyper+E",
            "Super+Super+E",
            "Super+NoSuchKey",
            "Super+\0",
        ] {
            assert!(bad.parse::<Chord>().is_err(), "{bad:?}");
        }
        Ok(())
    }

    #[test]
    fn a_chord_needs_exactly_its_modifiers() -> anyhow::Result<()> {
        let chord: Chord = "Super+Shift+E".parse()?;
        assert!(chord.matches(&mods(true, true, false, false), Keysym::e, false));
        assert!(!chord.matches(&mods(true, false, false, false), Keysym::e, false));
        assert!(!chord.matches(&mods(true, true, true, false), Keysym::e, false));
        assert!(!chord.matches(&mods(true, true, false, false), Keysym::r, false));
        Ok(())
    }

    #[test]
    fn nested_super_is_alt_super() -> anyhow::Result<()> {
        let chord: Chord = "Super+Return".parse()?;
        assert!(!chord.matches(&mods(true, false, false, false), Keysym::Return, true));
        assert!(chord.matches(&mods(true, false, false, true), Keysym::Return, true));
        assert!(!chord.matches(&mods(true, false, false, true), Keysym::Return, false));
        // Without Super, nested changes nothing.
        let plain: Chord = "Ctrl+Alt+T".parse()?;
        assert!(plain.matches(&mods(false, false, true, true), Keysym::t, true));
        Ok(())
    }

    #[test]
    fn the_default_shortcuts() {
        let shortcuts = Shortcuts::default();
        let action = |m, k| shortcuts.action(&m, k, false);
        assert_eq!(
            action(mods(true, false, false, false), Keysym::Return),
            Some(Action::Terminal)
        );
        assert_eq!(
            action(mods(true, true, false, false), Keysym::e),
            Some(Action::LogOut)
        );
        assert_eq!(action(mods(true, false, false, false), Keysym::e), None);
        assert_eq!(
            action(mods(false, false, false, false), Keysym::Return),
            None
        );
        // Super+Ctrl+arrows switch desktops; focusing by side has no
        // default (the maintainer's decision on #323).
        assert_eq!(
            action(mods(true, false, true, false), Keysym::Right),
            Some(Action::DesktopBy(1))
        );
        assert_eq!(shortcuts.focus_left, None);
        assert_eq!(
            action(mods(true, true, false, false), Keysym::_3),
            Some(Action::SendToDesktop(2))
        );
        assert_eq!(
            action(mods(true, true, true, false), Keysym::_2),
            Some(Action::TakeToDesktop(1))
        );
        assert_eq!(
            action(mods(false, false, false, true), Keysym::Tab),
            Some(Action::SwitchWindows { backward: false })
        );
        assert_eq!(
            action(mods(false, true, false, true), Keysym::Tab),
            Some(Action::SwitchWindows { backward: true })
        );
        // Nested, Alt+Tab is the host's: Alt+Super+Tab is the desktop's.
        let nested = |m, k| shortcuts.action(&m, k, true);
        assert_eq!(nested(mods(false, false, false, true), Keysym::Tab), None);
        assert_eq!(
            nested(mods(true, false, false, true), Keysym::Tab),
            Some(Action::SwitchWindows { backward: false })
        );
    }
}
