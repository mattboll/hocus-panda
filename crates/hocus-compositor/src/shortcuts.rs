//! The desktop's own keyboard shortcuts (docs/design/windows.md §13): the
//! chords the `[shortcuts]` section binds to actions, and what each action
//! does. Nested, `Super` is `Alt+Super`, so that the host keeps its own.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer};
use smithay::input::keyboard::{Keysym, ModifiersState, xkb};

use crate::{State, programs};

/// What a shortcut does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Opens a terminal (`[programs] terminal`).
    Terminal,
    /// Ends the session: the display manager comes back.
    LogOut,
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

/// The `[shortcuts]` section: every shortcut of the desktop, in one place.
/// An empty string turns one off.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Shortcuts {
    #[serde(deserialize_with = "chord")]
    pub terminal: Option<Chord>,
    #[serde(deserialize_with = "chord")]
    pub log_out: Option<Chord>,
}

impl Default for Shortcuts {
    fn default() -> Self {
        let chord = |text: &str| text.parse().ok();
        Self {
            terminal: chord("Super+Return"),
            log_out: chord("Super+Shift+E"),
        }
    }
}

impl Shortcuts {
    /// The action of what is pressed, if it is a shortcut.
    pub fn action(&self, mods: &ModifiersState, key: Keysym, nested: bool) -> Option<Action> {
        [
            (self.terminal, Action::Terminal),
            (self.log_out, Action::LogOut),
        ]
        .into_iter()
        .find_map(|(chord, action)| {
            chord
                .filter(|c| c.matches(mods, key, nested))
                .map(|_| action)
        })
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
            Action::Terminal => programs::spawn_terminal(
                self.config.programs.terminal.clone(),
                programs::Env::of(self),
            ),
            Action::LogOut => self.end(Ending::LoggedOut),
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
    }
}
