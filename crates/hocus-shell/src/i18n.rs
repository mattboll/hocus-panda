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

    pub fn nothing_planned(self) -> &'static str {
        self.pick("Nothing else planned", "Rien d’autre de prévu")
    }

    /// The meeting going on, and the minutes it has left.
    pub fn meeting_now(self, left: i64) -> String {
        match self {
            Self::En => format!("In progress · {left} min"),
            Self::Fr => format!("En cours · {left} min"),
        }
    }

    pub fn meeting_in(self, minutes: i64) -> String {
        match self {
            Self::En => format!("In {minutes} min"),
            Self::Fr => format!("Dans {minutes} min"),
        }
    }

    pub fn meeting_at(self, hour: u8, minute: u8) -> String {
        match self {
            Self::En => format!("At {hour:02}:{minute:02}"),
            Self::Fr => format!("À {hour:02}:{minute:02}"),
        }
    }

    pub fn meeting_tomorrow(self, hour: u8, minute: u8) -> String {
        match self {
            Self::En => format!("Tomorrow {hour:02}:{minute:02}"),
            Self::Fr => format!("Demain {hour:02}:{minute:02}"),
        }
    }

    /// A reminder's title.
    pub fn starts_in(self, summary: &str, minutes: i64) -> String {
        match (self, minutes) {
            (Self::En, 1) => format!("{summary} starts in 1 minute"),
            (Self::En, m) => format!("{summary} starts in {m} minutes"),
            (Self::Fr, 1) => format!("{summary} commence dans 1 minute"),
            (Self::Fr, m) => format!("{summary} commence dans {m} minutes"),
        }
    }

    pub fn join(self) -> &'static str {
        self.pick("Join", "Rejoindre")
    }

    pub fn details(self) -> &'static str {
        self.pick("Details", "Détails")
    }

    pub fn all_day(self) -> &'static str {
        self.pick("All day", "Journée")
    }

    pub fn today_upper(self) -> &'static str {
        self.pick("TODAY", "AUJOURD’HUI")
    }

    pub fn nothing_today(self) -> &'static str {
        self.pick("Nothing today", "Rien aujourd’hui")
    }

    pub fn month_year(self, month: u8, year: i16) -> String {
        let i = usize::from(month.clamp(1, 12) - 1);
        match self {
            Self::En => format!("{} {year}", MONTHS_EN[i]),
            Self::Fr => format!("{} {year}", MONTHS_FR[i]),
        }
    }

    /// Monday first.
    pub fn weekday_letters(self) -> [&'static str; 7] {
        match self {
            Self::En => ["M", "T", "W", "T", "F", "S", "S"],
            Self::Fr => ["L", "M", "M", "J", "V", "S", "D"],
        }
    }

    /// The week numbers' heading.
    pub fn week_short(self) -> &'static str {
        self.pick("Wk", "Sem.")
    }

    pub fn agenda(self) -> &'static str {
        self.pick("Agenda", "Agenda")
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

    pub fn palette_hint(self) -> &'static str {
        self.pick(
            "Enter runs it on the active window · Tab: its shortcut in Settings · or press its keys",
            "Entrée l’exécute sur la fenêtre active · Tab : son raccourci dans les Réglages · ou tapez ses touches",
        )
    }

    pub fn palette_placeholder(self) -> &'static str {
        self.pick(
            "Search an action, or press its keys…",
            "Chercher une action, ou taper ses touches…",
        )
    }

    pub fn no_shortcut(self) -> &'static str {
        self.pick("no shortcut", "pas de raccourci")
    }

    pub fn tray_title(self) -> &'static str {
        self.pick("Tray", "Zone de notification")
    }

    /// The tray's chip: how many applications, how many waiting.
    pub fn tray_count(self, count: usize, waiting: usize) -> String {
        let apps = match (self, count) {
            (Self::En, 1) => "Tray: 1 application".to_owned(),
            (Self::En, n) => format!("Tray: {n} applications"),
            (Self::Fr, 1) => "Zone de notification\u{a0}: 1 appli".to_owned(),
            (Self::Fr, n) => format!("Zone de notification\u{a0}: {n} applis"),
        };
        match (self, waiting) {
            (_, 0) => apps,
            (Self::En, n) => format!("{apps}, {n} waiting"),
            (Self::Fr, n) => format!("{apps}, {n} en attente"),
        }
    }

    pub fn menu_of(self, title: &str) -> String {
        match self {
            Self::En => format!("{title}’s menu"),
            Self::Fr => format!("Menu de {title}"),
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

    /// The status row's battery, with its level.
    pub fn battery_level(self, percent: u8) -> String {
        match self {
            Self::En => format!("Battery {percent}%"),
            Self::Fr => format!("Batterie {percent}\u{202f}%"),
        }
    }

    pub fn network_name(self, wifi: bool, name: Option<&str>) -> String {
        match (self, wifi, name) {
            (_, _, Some(name)) => format!("{}: {name}", self.network()),
            (Self::En, true, None) => "Network: not connected".into(),
            (Self::Fr, true, None) => "Réseau : non connecté".into(),
            (Self::En, false, None) => "Network: off".into(),
            (Self::Fr, false, None) => "Réseau : coupé".into(),
        }
    }

    pub fn sound_level(self, volume: u8, muted: bool) -> String {
        match (self, muted) {
            (Self::En, true) => "Sound: muted".into(),
            (Self::Fr, true) => "Son : coupé".into(),
            (Self::En, false) => format!("Sound {volume}%"),
            (Self::Fr, false) => format!("Son {volume}\u{202f}%"),
        }
    }

    pub fn profile_name(self, profile: usize) -> String {
        let names = self.profiles();
        let name = names.get(profile).copied().unwrap_or(names[1]);
        match self {
            Self::En => format!("Power profile: {name}"),
            Self::Fr => format!("Profil d’énergie : {name}"),
        }
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

    pub fn night_from(self, time: &str) -> String {
        match self {
            Self::En => format!("From {time}"),
            Self::Fr => format!("Dès {time}"),
        }
    }

    pub fn night_until(self, time: &str) -> String {
        match self {
            Self::En => format!("On until {time}"),
            Self::Fr => format!("Jusqu’à {time}"),
        }
    }

    pub fn focus_detail(self) -> &'static str {
        self.pick("Everything else dims", "Tout s’assombrit autour")
    }

    pub fn record_detail(self) -> &'static str {
        self.pick("Screen or window", "Écran ou fenêtre")
    }

    pub fn updates_installed(self) -> &'static str {
        self.pick("Updates installed", "Mises à jour installées")
    }

    pub fn updates_failed(self) -> &'static str {
        self.pick(
            "The updates were not installed",
            "Les mises à jour n’ont pas été installées",
        )
    }

    pub fn updates_ready(self, count: usize) -> String {
        match (self, count) {
            (Self::En, 1) => "1 update ready".into(),
            (Self::En, n) => format!("{n} updates ready"),
            (Self::Fr, 1) => "1 mise à jour prête".into(),
            (Self::Fr, n) => format!("{n} mises à jour prêtes"),
        }
    }

    pub fn updates_installing(self) -> &'static str {
        self.pick("Installing the updates…", "Installation des mises à jour…")
    }

    pub fn install(self) -> &'static str {
        self.pick("Install", "Installer")
    }

    pub fn notes_placeholder(self) -> &'static str {
        self.pick("A note, or - [ ] a to-do", "Une note, ou - [ ] une tâche")
    }

    pub fn notes_hint(self) -> &'static str {
        self.pick(
            "Enter adds the line, a to-do when it starts with - [ ] · on an empty line, Enter ticks · Delete removes",
            "Entrée ajoute la ligne, une tâche si elle commence par - [ ] · sur une ligne vide, Entrée coche · Suppr retire",
        )
    }

    pub fn note_kind(self, todo: bool) -> &'static str {
        match todo {
            true => self.pick("To-do", "Tâche"),
            false => self.pick("Note", "Note"),
        }
    }

    pub fn tasks_caption(self, open: usize) -> String {
        match self {
            Self::En => format!("Tasks · {open} to do"),
            Self::Fr => format!("Tâches · {open} à faire"),
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

    pub fn welcome_back(self) -> &'static str {
        self.pick("Welcome back", "Bon retour")
    }

    pub fn windows_were_open(self, n: usize) -> String {
        match (self, n) {
            (Self::En, 1) => "1 window was open.".into(),
            (Self::En, n) => format!("{n} windows were open."),
            (Self::Fr, 1) => "1 fenêtre était ouverte.".into(),
            (Self::Fr, n) => format!("{n} fenêtres étaient ouvertes."),
        }
    }

    pub fn put_everything_back(self) -> &'static str {
        self.pick("Put everything back", "Tout remettre")
    }

    pub fn choose(self) -> &'static str {
        self.pick("Choose", "Choisir")
    }

    pub fn start_fresh(self) -> &'static str {
        self.pick("Start fresh", "Repartir de zéro")
    }

    pub fn put_these_back(self) -> &'static str {
        self.pick("Put these back", "Remettre celles-ci")
    }

    pub fn not_restored(self, app: &str) -> String {
        match self {
            Self::En => format!("{app} was not put back"),
            Self::Fr => format!("{app} n’a pas été remis en place"),
        }
    }

    /// The curtain's countdown.
    pub fn locking_in(self, seconds: u32) -> String {
        match self {
            Self::En => format!("Locking in {seconds}\u{a0}s"),
            Self::Fr => format!("Verrouillage dans {seconds}\u{a0}s"),
        }
    }

    pub fn stay_hint(self) -> &'static str {
        self.pick(
            "Move the mouse or press a key to stay.",
            "Bouge la souris ou touche une touche pour rester.",
        )
    }

    pub fn password(self) -> &'static str {
        self.pick("Password", "Mot de passe")
    }

    pub fn caps_lock_on(self) -> &'static str {
        self.pick("Caps Lock is on", "Verr. Maj est activé")
    }

    pub fn wrong_password(self) -> &'static str {
        self.pick("Wrong password", "Mot de passe incorrect")
    }

    pub fn checking(self) -> &'static str {
        self.pick("Checking…", "Vérification…")
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

    pub fn suspend(self) -> &'static str {
        self.pick("Suspend", "Mettre en veille")
    }

    pub fn hibernate(self) -> &'static str {
        self.pick("Hibernate", "Mettre en veille prolongée")
    }

    pub fn cancel(self) -> &'static str {
        self.pick("Cancel", "Annuler")
    }

    /// "Restarting in 7 s."
    pub fn leaving_in(self, what: &str, seconds: u32) -> String {
        match self {
            Self::En => format!("{what} in {seconds} s."),
            Self::Fr => format!("{what} dans {seconds} s."),
        }
    }

    /// The countdown's subject: "Restarting", "Redémarrage".
    pub fn leaving(self, what: usize) -> &'static str {
        match (self, what) {
            (Self::En, 0) => "Logging out",
            (Self::En, 1) => "Restarting",
            (Self::En, _) => "Shutting down",
            (Self::Fr, 0) => "Déconnexion",
            (Self::Fr, 1) => "Redémarrage",
            (Self::Fr, _) => "Extinction",
        }
    }

    pub fn now(self, what: &str) -> String {
        match self {
            Self::En => format!("{what} now"),
            Self::Fr => format!("{what} maintenant"),
        }
    }

    pub fn anyway(self, what: &str) -> String {
        match self {
            Self::En => format!("{what} anyway"),
            Self::Fr => format!("{what} quand même"),
        }
    }

    pub fn held_back(self) -> &'static str {
        self.pick(
            "These applications ask to wait:",
            "Ces applications demandent d’attendre :",
        )
    }

    pub fn closing_apps(self) -> &'static str {
        self.pick("Closing the applications…", "Fermeture des applications…")
    }

    pub fn still_open(self) -> &'static str {
        self.pick("Still open:", "Encore ouvertes :")
    }

    /// Over the system's password dialog: a client cannot draw this.
    pub fn system_asks(self) -> &'static str {
        self.pick("SYSTEM · HOCUSPANDA", "SYSTÈME · HOCUSPANDA")
    }

    pub fn authentication_required(self) -> &'static str {
        self.pick("Authentication required", "Authentification requise")
    }

    pub fn password_for(self, user: &str) -> String {
        match self {
            Self::En => format!("Password for {user}"),
            Self::Fr => format!("Mot de passe de {user}"),
        }
    }

    pub fn authenticate(self) -> &'static str {
        self.pick("Authenticate", "Authentifier")
    }

    pub fn pick_region(self) -> &'static str {
        self.pick(
            "Drag over a region · Escape to cancel",
            "Tracer une zone · Échap pour annuler",
        )
    }

    pub fn annotate(self) -> &'static str {
        self.pick("Annotate", "Annoter")
    }

    pub fn annotate_arrow(self) -> &'static str {
        self.pick("Arrow", "Flèche")
    }

    pub fn annotate_frame(self) -> &'static str {
        self.pick("Frame", "Cadre")
    }

    pub fn annotate_text(self) -> &'static str {
        self.pick("Text", "Texte")
    }

    pub fn annotate_blur(self) -> &'static str {
        self.pick("Blur", "Flou")
    }

    /// The annotation colours, in their order.
    pub fn annotate_colour(self, i: usize) -> &'static str {
        let (en, fr) = match i {
            0 => ("Red", "Rouge"),
            1 => ("Yellow", "Jaune"),
            2 => ("Green", "Vert"),
            3 => ("Blue", "Bleu"),
            _ => ("White", "Blanc"),
        };
        self.pick(en, fr)
    }

    pub fn annotate_undo(self) -> &'static str {
        self.pick("Undo", "Défaire")
    }

    pub fn annotate_copy(self) -> &'static str {
        self.pick("Copy", "Copier")
    }

    pub fn annotate_save(self) -> &'static str {
        self.pick("Save", "Enregistrer")
    }

    pub fn annotate_saved(self, name: &str) -> String {
        match self {
            Self::En => format!("Saved as {name}"),
            Self::Fr => format!("Enregistrée sous {name}"),
        }
    }

    pub fn annotate_copied(self) -> &'static str {
        self.pick("Copied", "Copiée")
    }

    pub fn share_title(self) -> &'static str {
        self.pick("Share your screen", "Partager l’écran")
    }

    pub fn share_note(self) -> &'static str {
        self.pick(
            "An application asks to see the screen: choose what it sees.",
            "Une application demande à voir l’écran : choisis ce qu’elle voit.",
        )
    }

    pub fn share_screens(self) -> &'static str {
        self.pick("Screens", "Écrans")
    }

    pub fn share_windows(self) -> &'static str {
        self.pick("Windows", "Fenêtres")
    }

    pub fn share_cancel(self) -> &'static str {
        self.pick("Share nothing", "Ne rien partager")
    }

    pub fn sharing_now(self) -> &'static str {
        self.pick("Screen shared", "Écran partagé")
    }

    pub fn stop(self) -> &'static str {
        self.pick("Stop", "Arrêter")
    }

    pub fn stop_sharing(self) -> &'static str {
        self.pick("Stop sharing", "Arrêter le partage")
    }

    pub fn pick_record(self) -> &'static str {
        self.pick(
            "Drag over a region, click a window, or Enter for the screen · Escape to cancel",
            "Tracer une zone, cliquer une fenêtre, ou Entrée pour l’écran · Échap pour annuler",
        )
    }

    pub fn recording_in(self, seconds: u32) -> String {
        match self {
            Self::En => format!("Recording in {seconds}…"),
            Self::Fr => format!("Enregistrement dans {seconds}…"),
        }
    }

    pub fn recording_paused(self, time: &str) -> String {
        match self {
            Self::En => format!("{time} · paused"),
            Self::Fr => format!("{time} · en pause"),
        }
    }

    pub fn recording_pause(self) -> &'static str {
        self.pick("Pause the recording", "Mettre l’enregistrement en pause")
    }

    pub fn recording_resume(self) -> &'static str {
        self.pick("Resume the recording", "Reprendre l’enregistrement")
    }

    pub fn recording_stop(self) -> &'static str {
        self.pick("Stop the recording", "Arrêter l’enregistrement")
    }

    pub fn recording_cancel(self) -> &'static str {
        self.pick("Cancel the recording", "Annuler l’enregistrement")
    }

    pub fn recording_saved(self) -> &'static str {
        self.pick("Recording saved", "Enregistrement sauvegardé")
    }

    pub fn recording_failed(self) -> &'static str {
        self.pick("The recording failed", "L’enregistrement a échoué")
    }

    pub fn pick_window(self) -> &'static str {
        self.pick(
            "Click a window · Escape to cancel",
            "Cliquer une fenêtre · Échap pour annuler",
        )
    }

    pub fn screenshot_taken(self) -> &'static str {
        self.pick("Screenshot taken", "Capture prise")
    }

    /// Under the screenshot's toast: where it went.
    pub fn screenshot_where(self, file: Option<&str>, copied: bool) -> String {
        match (self, file, copied) {
            (Self::En, Some(f), true) => format!("{f}, and copied"),
            (Self::En, Some(f), false) => f.to_owned(),
            (Self::En, None, _) => "Copied to the clipboard".into(),
            (Self::Fr, Some(f), true) => format!("{f}, et copiée"),
            (Self::Fr, Some(f), false) => f.to_owned(),
            (Self::Fr, None, _) => "Copiée dans le presse-papiers".into(),
        }
    }

    pub fn open_file(self) -> &'static str {
        self.pick("Open", "Ouvrir")
    }

    pub fn delete(self) -> &'static str {
        self.pick("Delete", "Supprimer")
    }

    pub fn copy_text(self) -> &'static str {
        self.pick("Copy the text", "Copier le texte")
    }

    pub fn text_copied(self) -> &'static str {
        self.pick("Text copied", "Texte copié")
    }

    pub fn no_text_found(self) -> &'static str {
        self.pick("No text found", "Aucun texte trouvé")
    }

    pub fn history_hint(self) -> &'static str {
        self.pick(
            "Enter pastes · Tab pins · Delete forgets",
            "Entrée colle · Tab épingle · Suppr oublie",
        )
    }

    pub fn emoji_hint(self, tone: &str) -> String {
        match self {
            Self::En => format!("Enter types it · Tab: skin tone {tone}"),
            Self::Fr => format!("Entrée l’écrit · Tab : teinte {tone}"),
        }
    }

    pub fn pinned(self) -> &'static str {
        self.pick("Pinned", "Épinglé")
    }

    pub fn clear_history(self) -> &'static str {
        self.pick("Clear the history", "Effacer l’historique")
    }

    pub fn copied_paste(self) -> &'static str {
        self.pick("Copied: paste it", "Copié : collez-le")
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
