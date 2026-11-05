//! Shortcuts as the file writes them (`Super+Shift+E`: modifiers, then an
//! XKB key name, as the compositor reads `[shortcuts]`), as the window
//! shows them (in French: "Super + Maj + E"), and as recorded from the
//! keys pressed (6.8).

use crate::lang::Lang;

/// A chord: its modifiers and its key, the key's XKB name as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    pub logo: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: String,
}

impl Chord {
    /// `Super+Shift+E`, in any case and order of its modifiers.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let key = parts.pop().filter(|k| !k.is_empty())?;
        let mut chord = Self {
            logo: false,
            ctrl: false,
            alt: false,
            shift: false,
            key: key.to_owned(),
        };
        for part in parts {
            let held = match part.to_ascii_lowercase().as_str() {
                "super" | "logo" | "mod4" => &mut chord.logo,
                "ctrl" | "control" => &mut chord.ctrl,
                "alt" | "mod1" => &mut chord.alt,
                "shift" => &mut chord.shift,
                _ => return None,
            };
            *held = true;
        }
        Some(chord)
    }

    /// As the file writes it: Super, Ctrl, Alt, Shift, then the key.
    pub fn written(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        for (held, name) in [
            (self.logo, "Super"),
            (self.ctrl, "Ctrl"),
            (self.alt, "Alt"),
            (self.shift, "Shift"),
        ] {
            if held {
                parts.push(name);
            }
        }
        parts.push(&self.key);
        parts.join("+")
    }

    /// The same chord, the key's case aside (XKB reads names so).
    pub fn same(&self, other: &Self) -> bool {
        (self.logo, self.ctrl, self.alt, self.shift)
            == (other.logo, other.ctrl, other.alt, other.shift)
            && self.key.eq_ignore_ascii_case(&other.key)
    }

    /// Its keycaps as the window shows them, in `lang`.
    pub fn caps(&self, lang: Lang) -> Vec<String> {
        let fr = lang.fr();
        let mut caps = Vec::new();
        for (held, name) in [
            (self.logo, "Super"),
            (self.ctrl, "Ctrl"),
            (self.alt, "Alt"),
            (self.shift, if fr { "Maj" } else { "Shift" }),
        ] {
            if held {
                caps.push(name.to_owned());
            }
        }
        caps.push(key_shown(&self.key, lang));
        caps
    }

    /// No Super, Ctrl or Alt: it would type in applications, unless its
    /// key types nothing (F1, Print, the media keys).
    pub fn types(&self) -> bool {
        if self.logo || self.ctrl || self.alt {
            return false;
        }
        let key = self.key.as_str();
        let silent = key.starts_with("XF86")
            || key.eq_ignore_ascii_case("Print")
            || (key.len() >= 2
                && key.starts_with(['F', 'f'])
                && key[1..].chars().all(|c| c.is_ascii_digit()));
        !silent
    }

    /// Ctrl+Alt+F1 to F12: the virtual terminals', fixed.
    pub fn fixed(&self) -> bool {
        self.ctrl
            && self.alt
            && !self.logo
            && self.key.len() >= 2
            && self.key.starts_with(['F', 'f'])
            && self.key[1..]
                .parse::<u8>()
                .is_ok_and(|n| (1..=12).contains(&n))
    }
}

/// `text` as the window shows it: "Super + Maj + E"; as it is when it does
/// not read.
pub fn shown(text: &str, lang: Lang) -> String {
    Chord::parse(text).map_or_else(|| text.to_owned(), |c| c.caps(lang).join(" + "))
}

/// A key's XKB name as the window shows it.
pub fn key_shown(key: &str, lang: Lang) -> String {
    let fr = lang.fr();
    let named = match key.to_ascii_lowercase().as_str() {
        "return" => Some(if fr { "Entrée" } else { "Enter" }),
        "space" => Some(if fr { "Espace" } else { "Space" }),
        "left" => Some("←"),
        "right" => Some("→"),
        "up" => Some("↑"),
        "down" => Some("↓"),
        "print" => Some(if fr { "Impr. écran" } else { "Print Screen" }),
        "period" => Some("."),
        "comma" => Some(","),
        "tab" => Some("Tab"),
        "escape" => Some(if fr { "Échap" } else { "Esc" }),
        "backspace" => Some(if fr { "Retour arrière" } else { "Backspace" }),
        "delete" => Some(if fr { "Suppr" } else { "Delete" }),
        "xf86audioraisevolume" => Some("Volume +"),
        "xf86audiolowervolume" => Some("Volume −"),
        "xf86audiomute" => Some(if fr { "Muet" } else { "Mute" }),
        "xf86monbrightnessup" => Some(if fr { "Luminosité +" } else { "Brightness +" }),
        "xf86monbrightnessdown" => Some(if fr {
            "Luminosité −"
        } else {
            "Brightness −"
        }),
        _ => None,
    };
    match named {
        Some(name) => name.to_owned(),
        None if key.chars().count() == 1 => key.to_uppercase(),
        None => key.to_owned(),
    }
}

/// The XKB name of a key iced gives, as a shortcut writes it; `None` for
/// a modifier alone, or a key no shortcut takes.
pub fn xkb_name(
    key: &iced::keyboard::Key,
    physical: &iced::keyboard::key::Physical,
) -> Option<String> {
    use iced::keyboard::{
        Key,
        key::{Code, Named, Physical},
    };
    // The top row's digits whatever the layout (AZERTY types `&`), as the
    // compositor reads them.
    if let Physical::Code(code) = physical {
        let digit = match code {
            Code::Digit1 => Some("1"),
            Code::Digit2 => Some("2"),
            Code::Digit3 => Some("3"),
            Code::Digit4 => Some("4"),
            Code::Digit5 => Some("5"),
            Code::Digit6 => Some("6"),
            Code::Digit7 => Some("7"),
            Code::Digit8 => Some("8"),
            Code::Digit9 => Some("9"),
            Code::Digit0 => Some("0"),
            _ => None,
        };
        if let Some(digit) = digit {
            return Some(digit.to_owned());
        }
    }
    match key {
        Key::Named(named) => Some(
            match named {
                Named::Enter => "Return",
                Named::Space => "space",
                Named::Tab => "Tab",
                Named::ArrowLeft => "Left",
                Named::ArrowRight => "Right",
                Named::ArrowUp => "Up",
                Named::ArrowDown => "Down",
                Named::PrintScreen => "Print",
                Named::Home => "Home",
                Named::End => "End",
                Named::PageUp => "Page_Up",
                Named::PageDown => "Page_Down",
                Named::Insert => "Insert",
                Named::Delete => "Delete",
                Named::AudioVolumeUp => "XF86AudioRaiseVolume",
                Named::AudioVolumeDown => "XF86AudioLowerVolume",
                Named::AudioVolumeMute => "XF86AudioMute",
                Named::BrightnessUp => "XF86MonBrightnessUp",
                Named::BrightnessDown => "XF86MonBrightnessDown",
                Named::F1 => "F1",
                Named::F2 => "F2",
                Named::F3 => "F3",
                Named::F4 => "F4",
                Named::F5 => "F5",
                Named::F6 => "F6",
                Named::F7 => "F7",
                Named::F8 => "F8",
                Named::F9 => "F9",
                Named::F10 => "F10",
                Named::F11 => "F11",
                Named::F12 => "F12",
                _ => return None,
            }
            .to_owned(),
        ),
        Key::Character(c) => {
            let c = c.as_str();
            let named = match c {
                "." => "period",
                "," => "comma",
                ";" => "semicolon",
                ":" => "colon",
                "-" => "minus",
                "=" => "equal",
                "/" => "slash",
                "'" => "apostrophe",
                _ => "",
            };
            if !named.is_empty() {
                return Some(named.to_owned());
            }
            let mut chars = c.chars();
            match (chars.next(), chars.next()) {
                (Some(one), None) if one.is_ascii_alphanumeric() => {
                    Some(one.to_ascii_uppercase().to_string())
                }
                _ => None,
            }
        }
        Key::Unidentified => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chord_is_written_shown_and_compared() {
        let chord = Chord::parse("shift+SUPER+e").expect("a chord");
        assert_eq!(chord.written(), "Super+Shift+e");
        assert!(chord.same(&Chord::parse("Super+Shift+E").expect("a chord")));
        assert_eq!(shown("Super+Shift+E", Lang::French), "Super + Maj + E");
        assert_eq!(shown("Super+Return", Lang::French), "Super + Entrée");
        assert_eq!(shown("Alt+Print", Lang::French), "Alt + Impr. écran");
        assert_eq!(shown("XF86AudioRaiseVolume", Lang::French), "Volume +");
        assert!(Chord::parse("Q").expect("a chord").types());
        assert!(!Chord::parse("Print").expect("a chord").types());
        assert!(!Chord::parse("F11").expect("a chord").types());
        assert!(Chord::parse("Ctrl+Alt+F3").expect("a chord").fixed());
        assert!(!Chord::parse("Super+F3").expect("a chord").fixed());
        assert_eq!(Chord::parse("Hyper+Q"), None);
    }
}
