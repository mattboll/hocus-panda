//! The phone's words and icons (#85): its line in the column, its
//! popover, and the reply of its notifications' toasts.

use crate::i18n::Lang;

/// A phone, stroked like the other icons.
pub const PHONE: &str = "M8 3h8v18H8zM11 18h2";
/// A bell, for ringing it.
pub const RING: &str = "M6 17V11a6 6 0 0 1 12 0v6l2 2H4zM10 21h4";
/// A sheet going up, for the clipboard sent.
pub const SEND: &str = "M7 4h10v16H7zM12 15V9M9.5 11.5L12 9l2.5 2.5";
/// A folder, for its files.
pub const FOLDER: &str = "M3 6h7l2 2h9v11H3z";

/// The action key of a toast that takes a reply typed into it, as KDE's
/// notifications name it.
pub const REPLY: &str = "inline-reply";

impl Lang {
    /// The column's line, as board D13 has it: "Phone · 82 %".
    pub fn phone_line(self, battery: Option<u8>) -> String {
        match (self, battery) {
            (Self::En, Some(p)) => format!("Phone · {p}%"),
            (Self::Fr, Some(p)) => format!("Téléphone · {p}\u{202f}%"),
            (Self::En, None) => "Phone".into(),
            (Self::Fr, None) => "Téléphone".into(),
        }
    }

    /// The line for screen readers, and what opens the popover.
    pub fn phone_name(self, name: &str, battery: Option<u8>, charging: bool) -> String {
        let level = match (self, battery, charging) {
            (_, None, _) => String::new(),
            (Self::En, Some(p), false) => format!(", {p}%"),
            (Self::En, Some(p), true) => format!(", {p}%, charging"),
            (Self::Fr, Some(p), false) => format!(", {p}\u{202f}%"),
            (Self::Fr, Some(p), true) => format!(", {p}\u{202f}%, en charge"),
        };
        match self {
            Self::En => format!("Phone: {name}{level}"),
            Self::Fr => format!("Téléphone\u{a0}: {name}{level}"),
        }
    }

    /// The popover's battery line.
    pub fn phone_battery(self, battery: Option<u8>, charging: bool) -> String {
        match (self, battery, charging) {
            (Self::En, None, _) => "Battery not shared".into(),
            (Self::Fr, None, _) => "Batterie non partagée".into(),
            (Self::En, Some(p), false) => format!("Battery {p}%"),
            (Self::En, Some(p), true) => format!("Battery {p}%, charging"),
            (Self::Fr, Some(p), false) => format!("Batterie {p}\u{202f}%"),
            (Self::Fr, Some(p), true) => format!("Batterie {p}\u{202f}%, en charge"),
        }
    }

    pub fn ring_phone(self) -> &'static str {
        self.pick("Ring", "Faire sonner")
    }

    pub fn send_clipboard(self) -> &'static str {
        self.pick("Send the clipboard", "Envoyer le presse-papiers")
    }

    pub fn browse_phone(self) -> &'static str {
        self.pick("Browse its files", "Parcourir ses fichiers")
    }

    pub fn reply(self) -> &'static str {
        self.pick("Reply", "Répondre")
    }

    pub fn reply_placeholder(self) -> &'static str {
        self.pick("Your reply", "Votre réponse")
    }

    pub fn send(self) -> &'static str {
        self.pick("Send", "Envoyer")
    }

    /// A toast after the clipboard or files went to the phone.
    pub fn sent_to(self, name: &str, files: usize) -> String {
        match (self, files) {
            (Self::En, 0) => format!("Sent to {name}"),
            (Self::En, 1) => format!("1 file sent to {name}"),
            (Self::En, n) => format!("{n} files sent to {name}"),
            (Self::Fr, 0) => format!("Envoyé à {name}"),
            (Self::Fr, 1) => format!("1 fichier envoyé à {name}"),
            (Self::Fr, n) => format!("{n} fichiers envoyés à {name}"),
        }
    }

    pub fn nothing_copied(self) -> &'static str {
        self.pick("Nothing copied to send", "Rien de copié à envoyer")
    }

    pub fn phone_failed(self, name: &str) -> String {
        match self {
            Self::En => format!("{name} did not answer"),
            Self::Fr => format!("{name} n’a pas répondu"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icons_parse() {
        for path in [PHONE, RING, SEND, FOLDER] {
            assert!(hocus_ui::Icon::stroked(path, 16.0, 2.0).is_some(), "{path}");
        }
    }

    #[test]
    fn the_line_reads_as_the_board_has_it() {
        assert_eq!(Lang::Fr.phone_line(Some(82)), "Téléphone · 82\u{202f}%");
        assert_eq!(Lang::En.phone_line(None), "Phone");
        assert_eq!(
            Lang::En.phone_name("Pixel", Some(40), true),
            "Phone: Pixel, 40%, charging"
        );
    }
}
