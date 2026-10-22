//! The shell's words, in English and French. The language is chosen once
//! from the environment; switching it live comes with the translation work
//! (#51).

/// A language the shell speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    En,
    Fr,
}

impl Lang {
    /// From a POSIX locale such as `fr_FR.UTF-8`: French for `fr`, English
    /// otherwise.
    pub fn from_locale(locale: &str) -> Self {
        if locale.starts_with("fr") {
            Self::Fr
        } else {
            Self::En
        }
    }

    /// From `LC_ALL`, `LC_MESSAGES` or `LANG`, the first one set.
    pub fn from_env() -> Self {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))
            .map_or(Self::En, |l| Self::from_locale(&l))
    }

    fn pick(self, en: &'static str, fr: &'static str) -> &'static str {
        match self {
            Self::En => en,
            Self::Fr => fr,
        }
    }

    /// `wednesday 21 october`, `Wednesday, October 21`.
    pub fn date(self, weekday: u8, day: u8, month: u8) -> String {
        const DAYS_EN: [&str; 7] = [
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
            "Sunday",
        ];
        const DAYS_FR: [&str; 7] = [
            "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
        ];
        const MONTHS_EN: [&str; 12] = [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ];
        const MONTHS_FR: [&str; 12] = [
            "janvier",
            "février",
            "mars",
            "avril",
            "mai",
            "juin",
            "juillet",
            "août",
            "septembre",
            "octobre",
            "novembre",
            "décembre",
        ];
        let w = usize::from(weekday % 7);
        let m = usize::from(month.clamp(1, 12) - 1);
        match self {
            Self::En => format!("{}, {} {day}", DAYS_EN[w], MONTHS_EN[m]),
            Self::Fr => format!("{} {day} {}", DAYS_FR[w], MONTHS_FR[m]),
        }
    }

    pub fn no_meeting(self) -> &'static str {
        self.pick("No meeting today", "Aucune réunion aujourd’hui")
    }

    pub fn calendar_not_set(self) -> &'static str {
        self.pick("Calendar not set up", "Agenda non configuré")
    }

    pub fn active_window(self) -> &'static str {
        self.pick("ACTIVE WINDOW", "FENÊTRE ACTIVE")
    }

    pub fn no_window(self) -> &'static str {
        self.pick("No window", "Aucune fenêtre")
    }

    pub fn minimize(self) -> &'static str {
        self.pick("Minimize", "Réduire")
    }

    pub fn maximize(self) -> &'static str {
        self.pick("Maximize", "Agrandir")
    }

    pub fn restore(self) -> &'static str {
        self.pick("Restore", "Restaurer")
    }

    pub fn close(self) -> &'static str {
        self.pick("Close", "Fermer")
    }

    /// The column's desktops line.
    pub fn desktop_of(self, shown: usize, count: usize) -> String {
        match self {
            Self::En => format!("Desktop {shown} of {count} on this screen"),
            Self::Fr => format!("Bureau {shown} sur {count} de cet écran"),
        }
    }

    pub fn desktops_title(self) -> &'static str {
        self.pick("Desktops of each screen", "Bureaux de chaque écran")
    }

    pub fn desktops_count(self, count: usize) -> String {
        match (self, count) {
            (Self::En, 1) => "1 desktop".into(),
            (Self::En, n) => format!("{n} desktops"),
            (Self::Fr, 1) => "1 bureau".into(),
            (Self::Fr, n) => format!("{n} bureaux"),
        }
    }

    pub fn desktop(self, n: usize) -> String {
        match self {
            Self::En => format!("Desktop {n}"),
            Self::Fr => format!("Bureau {n}"),
        }
    }

    pub fn keyboard_layout(self, name: &str) -> String {
        match self {
            Self::En => format!("Keyboard layout: {name}"),
            Self::Fr => format!("Disposition du clavier\u{a0}: {name}"),
        }
    }

    pub fn power_profile(self) -> &'static str {
        self.pick("Power profile", "Profil d’énergie")
    }

    pub fn network(self) -> &'static str {
        self.pick("Network", "Réseau")
    }

    pub fn sound(self) -> &'static str {
        self.pick("Sound", "Son")
    }

    pub fn battery(self) -> &'static str {
        self.pick("Battery", "Batterie")
    }

    pub fn account(self, name: &str) -> String {
        match self {
            Self::En => format!("Account: {name}"),
            Self::Fr => format!("Compte\u{a0}: {name}"),
        }
    }

    /// The name of a toast's close button.
    pub fn dismiss(self, summary: &str) -> String {
        match self {
            Self::En => format!("Dismiss: {summary}"),
            Self::Fr => format!("Ignorer\u{a0}: {summary}"),
        }
    }

    pub fn notifications(self) -> &'static str {
        self.pick("NOTIFICATIONS", "NOTIFICATIONS")
    }

    pub fn nothing_new(self) -> &'static str {
        self.pick("Nothing new", "Rien de nouveau")
    }

    pub fn command_field(self) -> &'static str {
        self.pick("Command field", "Champ de commande")
    }

    pub fn command_placeholder(self) -> &'static str {
        self.pick("Search, act, ask…", "Chercher, agir, demander…")
    }

    pub fn launcher_shortcut(self) -> &'static str {
        self.pick("Super+Space", "Super+Espace")
    }

    pub fn nothing_found(self) -> &'static str {
        self.pick("Nothing found", "Rien trouvé")
    }

    pub fn calculator(self) -> &'static str {
        self.pick("Calculator", "Calculatrice")
    }

    pub fn application(self) -> &'static str {
        self.pick("Application", "Application")
    }

    pub fn quick_settings(self) -> &'static str {
        self.pick("Quick settings", "Réglages rapides")
    }

    pub fn tile(self, tile: crate::quick::Tile) -> &'static str {
        use crate::quick::Tile as T;
        match tile {
            T::Wifi => "Wi-Fi",
            T::Bluetooth => "Bluetooth",
            T::DoNotDisturb => self.pick("Do not disturb", "Ne pas déranger"),
            T::NightLight => self.pick("Night light", "Lumière nocturne"),
            T::Focus => self.pick("Focus", "Concentration"),
            T::Record => self.pick("Record", "Enregistrer"),
            T::Airplane => self.pick("Airplane mode", "Mode avion"),
            T::Pandas => "Pandas",
        }
    }

    pub fn on_off(self, on: bool) -> &'static str {
        match on {
            true => self.pick("On", "Activé"),
            false => self.pick("Off", "Désactivé"),
        }
    }

    pub fn coming(self) -> &'static str {
        self.pick("Not yet", "Pas encore")
    }

    pub fn energy(self) -> &'static str {
        self.pick("POWER", "ÉNERGIE")
    }

    pub fn profiles(self) -> [&'static str; 3] {
        match self {
            Self::En => ["Saver", "Balanced", "Performance"],
            Self::Fr => ["Économie", "Équilibré", "Performance"],
        }
    }

    pub fn sound_caption(self) -> &'static str {
        self.pick("SOUND", "SON")
    }

    pub fn volume(self) -> &'static str {
        self.pick("Volume", "Volume")
    }

    pub fn output_device(self, name: &str) -> String {
        let name = if name.is_empty() {
            self.pick("default", "par défaut")
        } else {
            name
        };
        match self {
            Self::En => format!("Output: {name}"),
            Self::Fr => format!("Sortie\u{a0}: {name}"),
        }
    }

    pub fn brightness_caption(self) -> &'static str {
        self.pick("BRIGHTNESS, PER SCREEN", "LUMINOSITÉ, PAR ÉCRAN")
    }

    pub fn brightness_of(self, screen: &str) -> String {
        match self {
            Self::En => format!("Brightness of {screen}"),
            Self::Fr => format!("Luminosité de {screen}"),
        }
    }

    pub fn keyboard(self) -> &'static str {
        self.pick("Keyboard", "Clavier")
    }

    pub fn notifications_count(self, n: usize) -> String {
        format!("Notifications · {n}")
    }

    /// The toast after sharing the screen.
    pub fn held_while_sharing(self, n: usize) -> String {
        match (self, n) {
            (Self::En, 1) => "1 notification while you were sharing".into(),
            (Self::En, n) => format!("{n} notifications while you were sharing"),
            (Self::Fr, 1) => "1 notification pendant le partage".into(),
            (Self::Fr, n) => format!("{n} notifications pendant le partage"),
        }
    }

    pub fn in_the_column(self) -> &'static str {
        self.pick(
            "They wait in the column.",
            "Elles attendent dans la colonne.",
        )
    }

    pub fn clear_all(self) -> &'static str {
        self.pick("Clear all", "Tout effacer")
    }

    pub fn battery_left(self, percent: u8, minutes: Option<u32>) -> String {
        match (self, minutes) {
            (Self::En, Some(m)) => format!(
                "Battery {percent}\u{202f}% · about {} h {:02}",
                m / 60,
                m % 60
            ),
            (Self::Fr, Some(m)) => format!(
                "Batterie {percent}\u{202f}% · environ {} h {:02}",
                m / 60,
                m % 60
            ),
            (Self::En, None) => format!("Battery {percent}\u{202f}%"),
            (Self::Fr, None) => format!("Batterie {percent}\u{202f}%"),
        }
    }

    pub fn conversion(self) -> &'static str {
        self.pick("Conversion", "Conversion")
    }

    pub fn large_type(self, text: &str) -> String {
        match self {
            Self::En => format!("Show “{text}” in large type"),
            Self::Fr => format!("Afficher «\u{202f}{text}\u{202f}» en grand"),
        }
    }

    pub fn large_type_kind(self) -> &'static str {
        self.pick("Large type", "En grand")
    }

    pub fn qr_of(self, text: &str) -> String {
        match self {
            Self::En => format!("Show a QR code of “{text}”"),
            Self::Fr => format!("Afficher un code QR de «\u{202f}{text}\u{202f}»"),
        }
    }

    pub fn open(self, url: &str) -> String {
        match self {
            Self::En => format!("Open {url}"),
            Self::Fr => format!("Ouvrir {url}"),
        }
    }

    pub fn search_web(self, query: &str) -> String {
        match self {
            Self::En => format!("Search the web for “{query}”"),
            Self::Fr => format!("Chercher «\u{202f}{query}\u{202f}» sur le web"),
        }
    }

    pub fn close_showcase(self) -> &'static str {
        self.pick(
            "Click or press Escape to close",
            "Cliquer ou Échap pour fermer",
        )
    }

    pub fn lock(self) -> &'static str {
        self.pick("Lock", "Verrouiller")
    }

    pub fn settings(self) -> &'static str {
        self.pick("Settings", "Réglages")
    }

    pub fn log_out(self) -> &'static str {
        self.pick("Log out", "Se déconnecter")
    }

    pub fn restart(self) -> &'static str {
        self.pick("Restart", "Redémarrer")
    }

    pub fn shut_down(self) -> &'static str {
        self.pick("Shut down", "Éteindre")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_locale_picks_the_language() {
        assert_eq!(Lang::from_locale("fr_FR.UTF-8"), Lang::Fr);
        assert_eq!(Lang::from_locale("fr_CA"), Lang::Fr);
        assert_eq!(Lang::from_locale("en_GB.UTF-8"), Lang::En);
        assert_eq!(Lang::from_locale("C"), Lang::En);
    }

    #[test]
    fn dates_read_naturally_in_both() {
        assert_eq!(Lang::En.date(2, 21, 10), "Wednesday, October 21");
        assert_eq!(Lang::Fr.date(2, 21, 10), "mercredi 21 octobre");
    }
}
