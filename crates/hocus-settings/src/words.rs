//! The settings app's words, in English and French.

use hocus_shell::Lang;

/// One text in both languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    pub en: &'static str,
    pub fr: &'static str,
}

impl Words {
    pub const fn new(en: &'static str, fr: &'static str) -> Self {
        Self { en, fr }
    }

    pub fn get(self, lang: Lang) -> &'static str {
        match lang {
            Lang::En => self.en,
            Lang::Fr => self.fr,
        }
    }
}

pub const SETTINGS: Words = Words::new("Settings", "Réglages");
pub const SEARCH_HINT: Words = Words::new(
    "Search a setting, a key, an app…",
    "Rechercher un réglage, une touche, une appli…",
);
pub const SEARCH: Words = Words::new("Search", "Rechercher");
pub const SEARCH_NAME: Words = Words::new("Search a setting", "Rechercher un réglage");
pub const CLOSE: Words = Words::new("Close the settings", "Fermer les réglages");
pub const NO_RESULT: Words = Words::new("Nothing found", "Rien trouvé");
pub const NOT_YET: Words = Words::new("Not yet", "Pas encore");
pub const LIVE: Words = Words::new(
    "Everything applies at once, without restarting.",
    "Tout s’applique à l’instant, sans redémarrer.",
);
pub const ON: Words = Words::new("On", "Activé");
pub const OFF: Words = Words::new("Off", "Désactivé");

pub const THEME: Words = Words::new("Theme", "Thème");
pub const ACCENT: Words = Words::new("Accent", "Accent");
pub const FROM_WALLPAPER: Words = Words::new("From the wallpaper", "Pris du fond d’écran");
pub const ACCENT_NOTE: Words = Words::new(
    "A darker version is used on light themes, to stay readable.",
    "Une version plus sombre est utilisée sur les thèmes clairs, pour rester lisible.",
);
pub const TEXT: Words = Words::new("Text", "Texte");
pub const TEXT_SIZE: Words = Words::new("Text size", "Taille du texte");
pub const SIZE: Words = Words::new("Size", "Taille");
pub const FONT_UI: Words = Words::new("Interface: Manrope", "Interface : Manrope");
pub const FONT_MONO: Words = Words::new("Mono: JetBrains Mono", "Mono : JetBrains Mono");
pub const THEME_PER_DESKTOP: Words = Words::new("Theme per desktop", "Thème par bureau");
pub const THEME_PER_DESKTOP_NOTE: Words = Words::new(
    "Know where you are at a glance. Comes with #49.",
    "On sait d’un coup d’œil où l’on est. Arrive avec #49.",
);
pub const PANEL: Words = Words::new("Panel", "Panneau");
/// The panel's forms (#263): their names in the file, names and what
/// each does.
pub const PANEL_FORMS: [(&str, Words, Words); 4] = [
    (
        "strip",
        Words::new("Strip", "Bande fine"),
        Words::new(
            "72 px on the right: the time, the next meeting, the agents, counters. The whole column slides over the windows on hover, a click or Super+C.",
            "72 px à droite : l’heure, la prochaine réunion, les agents, des compteurs. La colonne entière glisse sur les fenêtres au survol, d’un clic ou avec Super+C.",
        ),
    ),
    (
        "column",
        Words::new("Column", "Colonne"),
        Words::new(
            "Everything at a glance on the right, 320 px taken for good.",
            "Tout d’un coup d’œil à droite, 320 px pris en permanence.",
        ),
    ),
    (
        "top",
        Words::new("Top bar", "Barre en haut"),
        Words::new(
            "One line: the active window and its buttons, the time, the agents, the tasks. A click on it brings the column out.",
            "Une ligne : la fenêtre active et ses boutons, l’heure, les agents, les tâches. Un clic dessus fait sortir la colonne.",
        ),
    ),
    (
        "hidden",
        Words::new("Hidden", "Caché"),
        Words::new(
            "The whole screen to the windows: the column slides out of the right edge, the agents’ bubbles stay.",
            "Tout l’écran aux fenêtres : la colonne sort du bord droit, les bulles des agents restent.",
        ),
    ),
];
pub const REDUCED_MOTION: Words = Words::new("Reduced motion", "Mouvement réduit");
pub const REDUCED_MOTION_SUB: Words =
    Words::new("Short fades, no bounce", "Fondus courts, pas de rebond");
pub const HIGH_CONTRAST: Words = Words::new("High contrast", "Contraste élevé");
pub const HIGH_CONTRAST_SUB: Words =
    Words::new("Sharp edges, plainer text", "Bords nets, texte plus franc");
pub const TRANSPARENCY: Words = Words::new("Transparency", "Transparences");
pub const TRANSPARENCY_SUB: Words = Words::new(
    "Frosted glass behind the panels",
    "Verre dépoli derrière les panneaux",
);

pub const LAYOUTS: Words = Words::new("Layouts", "Dispositions");
pub const SWITCH_LAYOUT: Words = Words::new("Switch layout", "Changer de disposition");
pub const SHORTCUTS: Words = Words::new("Shortcuts", "Raccourcis");
pub const SHORTCUTS_NOTE: Words = Words::new(
    "Super+P shows these shortcuts over any window.",
    "Super+P montre ces raccourcis par-dessus n’importe quelle fenêtre.",
);
pub const ACTION: Words = Words::new("Action", "Action");
pub const KEYS: Words = Words::new("Keys", "Touches");
pub const ACCOUNT: Words = Words::new("Account", "Compte");
pub const UPDATES: Words = Words::new("Updates", "Mises à jour");
pub const NO_MANAGER: Words = Words::new(
    "No package manager known here: updates come as the system's do.",
    "Aucun gestionnaire de paquets connu ici : les mises à jour viennent comme celles du système.",
);
pub const CHECKING: Words = Words::new("Looking for updates…", "Recherche de mises à jour…");
pub const UP_TO_DATE: Words = Words::new("Everything is up to date.", "Tout est à jour.");
pub const INSTALL_ALL: Words = Words::new("Install all", "Tout installer");

pub fn version(version: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Hocuspanda {version}"),
        Lang::Fr => format!("Hocuspanda {version}"),
    }
}

pub fn updates_ready(count: usize, lang: Lang) -> String {
    match (lang, count) {
        (Lang::En, 1) => "1 update ready".into(),
        (Lang::En, n) => format!("{n} updates ready"),
        (Lang::Fr, 1) => "1 mise à jour prête".into(),
        (Lang::Fr, n) => format!("{n} mises à jour prêtes"),
    }
}

pub fn installing(count: usize, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Installing {count} updates…"),
        Lang::Fr => format!("Installation de {count} mises à jour…"),
    }
}

pub const CHANGE_HEADER: Words = Words::new("Change", "Modifier");
pub const CHANGE: Words = Words::new("Change", "Changer");
pub const CANCEL: Words = Words::new("Cancel", "Annuler");
pub const SAVE: Words = Words::new("Save", "Enregistrer");
pub const REPLACE: Words = Words::new("Replace", "Remplacer");
pub const PRESS_KEYS: Words = Words::new(
    "Press the new keys · Escape cancels",
    "Appuie sur la nouvelle combinaison · Échap annule",
);
pub const NEEDS_MODIFIER: Words = Words::new(
    "With Super, Ctrl or Alt, and a key · Escape cancels",
    "Avec Super, Ctrl ou Alt, et une touche · Échap annule",
);
pub const NEW_SHORTCUT: Words = Words::new(
    "New shortcut: start an app, a command or an action",
    "Nouveau raccourci : lancer une appli, une commande ou une action",
);
pub const NEW_WHAT: Words = Words::new("What the shortcut runs", "Ce que lance le raccourci");
pub const NEW_WHAT_HINT: Words = Words::new(
    "An app, a command or an action: firefox, screenshot region…",
    "Une appli, une commande ou une action : firefox, screenshot region…",
);
pub const CHOOSE_KEYS: Words = Words::new("Choose its keys", "Choisir ses touches");
pub const RESET: Words = Words::new(
    "Back to the original shortcuts",
    "Revenir aux raccourcis d’origine",
);
pub const RESET_NOTE: Words = Words::new(
    "Your own shortcuts will be forgotten.",
    "Tes propres raccourcis seront oubliés.",
);
pub const GO_BACK: Words = Words::new("Go back", "Revenir");
pub const ADD: Words = Words::new("Add", "Ajouter");
pub const ADD_LAYOUT: Words = Words::new("Add a layout", "Ajouter une disposition");

/// The new keys, as caps joined.
pub fn new_keys(keys: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("New keys: {keys}"),
        Lang::Fr => format!("Nouvelles touches\u{a0}: {keys}"),
    }
}

/// Keys already bound to another action.
pub fn taken_by(action: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Already used by “{action}”: replace?"),
        Lang::Fr => format!("Déjà pris par «\u{a0}{action}\u{a0}»\u{a0}: remplacer\u{a0}?"),
    }
}

/// The Change button of a shortcut, for screen readers.
pub fn change_shortcut(action: &str, keys: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Change {action}, {keys}"),
        Lang::Fr => format!("Changer {action}, {keys}"),
    }
}

pub fn remove_layout(name: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Remove {name}"),
        Lang::Fr => format!("Retirer {name}"),
    }
}

pub fn layout_before(name: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Put {name} before"),
        Lang::Fr => format!("Placer {name} avant"),
    }
}

/// The search among the keyboard layouts and their variants.
pub const LAYOUT_SEARCH_NAME: Words = Words::new("Search the layouts", "Chercher une disposition");
pub const LAYOUT_SEARCH_HINT: Words = Words::new(
    "A layout or a variant: BÉPO, Dvorak…",
    "Une disposition ou une variante : BÉPO, Dvorak…",
);
pub const NO_LAYOUT: Words = Words::new(
    "No layout goes by that name.",
    "Aucune disposition ne porte ce nom.",
);
/// After the welcome's common layouts.
pub const MORE_LAYOUTS: Words = Words::new("More…", "Plus…");

pub const SCREENS_NOTE: Words = Words::new(
    "Each set of screens keeps its own layout.",
    "Chaque ensemble d’écrans garde sa disposition.",
);
pub const PRIMARY: Words = Words::new("Primary", "Principal");
pub const SCALE: Words = Words::new("Scale", "Échelle");
pub const DRAG_HINT: Words = Words::new(
    "Drag a screen to place it; they snap edge to edge.",
    "Glisser un écran pour le placer ; ils s’aimantent bord à bord.",
);
pub const IDENTIFY: Words = Words::new("Identify", "Identifier");
pub const MIRROR: Words = Words::new("Mirror the screens", "Dupliquer les écrans");
pub const STOP_MIRROR: Words = Words::new("Stop mirroring", "Ne plus dupliquer");
pub const RESOLUTION: Words = Words::new("Resolution", "Définition");
pub const REFRESH: Words = Words::new("Refresh rate", "Fréquence");
pub const DESKTOPS: Words = Words::new("Desktops", "Bureaux");
pub const FEWER_DESKTOPS: Words = Words::new("One desktop fewer", "Un bureau de moins");
pub const MORE_DESKTOPS: Words = Words::new("One desktop more", "Un bureau de plus");
pub const ONE_DESKTOP_NOTE: Words = Words::new(
    "With one desktop, the screen never changes.",
    "Avec un seul bureau, l’écran ne change jamais.",
);
pub const SCREEN_ON: Words = Words::new("On", "Allumé");
pub const VRR: Words = Words::new("Variable refresh", "Fréquence variable");
pub const LAYOUTS_KEPT: Words = Words::new("Remembered layouts", "Dispositions retenues");
pub const LAYOUTS_NOTE: Words = Words::new(
    "Each set of screens keeps its layout and the windows’ places; unplugging loses none.",
    "Chaque ensemble d’écrans garde sa disposition et la place des fenêtres ; débrancher n’en perd aucune.",
);
pub const CURRENT: Words = Words::new("current", "en cours");
pub const RENAME: Words = Words::new("Rename", "Renommer");
pub const LAPTOP: Words = Words::new("Laptop", "Portable");
pub const LAYOUT_NAME: Words = Words::new("Layout name", "Nom de la disposition");

/// "Layout “Home” recognised: …" under the Screens page's title.
pub fn layout_recognised(name: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Layout “{name}” recognised: the windows went back to their places."),
        Lang::Fr => {
            format!("Disposition « {name} » reconnue : les fenêtres ont repris leur place.")
        }
    }
}

/// "1 desktop", "4 desktops".
pub fn desktops(n: u32, lang: Lang) -> String {
    match (lang, n) {
        (Lang::En, 1) => "1 desktop".into(),
        (Lang::En, n) => format!("{n} desktops"),
        (Lang::Fr, 1) => "1 bureau".into(),
        (Lang::Fr, n) => format!("{n} bureaux"),
    }
}

pub const THEMES: [(&str, Words, Words); 5] = [
    (
        "dark",
        Words::new("Dark", "Sombre"),
        Words::new("ink, frosted glass", "encre, verre dépoli"),
    ),
    (
        "light",
        Words::new("Light", "Clair"),
        Words::new("white fur", "fourrure blanche"),
    ),
    (
        "paper",
        Words::new("Paper", "Papier"),
        Words::new("warm, ink", "chaud, encre"),
    ),
    (
        "hud",
        Words::new("HUD", "HUD"),
        Words::new("head-up, thin lines", "tête haute, traits fins"),
    ),
    (
        "matrix",
        Words::new("Matrix", "Matrix"),
        Words::new("green rain, mono", "pluie verte, mono"),
    ),
];

pub const ACCENTS: [(&str, Words); 4] = [
    ("bamboo", Words::new("Bamboo", "Bambou")),
    ("amber", Words::new("Amber", "Ambre")),
    ("blue", Words::new("Blue", "Bleu")),
    ("pink", Words::new("Pink", "Rose")),
];

/// The name of a theme or accent value as the user reads it.
pub fn value_name(value: &str, lang: Lang) -> String {
    THEMES
        .iter()
        .map(|(id, name, _)| (*id, *name))
        .chain(ACCENTS.iter().copied())
        .chain([
            ("theme", Words::new("The theme's", "Celui du thème")),
            (
                "wallpaper",
                Words::new("From the wallpaper", "Du fond d’écran"),
            ),
        ])
        .find(|(id, _)| *id == value)
        .map_or_else(|| value.to_owned(), |(_, w)| w.get(lang).to_owned())
}

/// `100 %` in French, `100%` in English.
pub fn percent(value: u32, lang: Lang) -> String {
    match lang {
        Lang::En => format!("{value}%"),
        Lang::Fr => format!("{value}\u{202f}%"),
    }
}

pub use hocus_shell::actions::Group as ShortcutGroup;

pub const ALL_GROUPS: Words = Words::new("All", "Tous");
pub const ACTS_ON: Words = Words::new("Acts on", "Agit sur");

/// What a shortcut does, in words: its name, its group and what it acts
/// on.
pub fn action(action: &hocus_core::Action, lang: Lang) -> (String, ShortcutGroup, &'static str) {
    let d = hocus_shell::actions::describe(action, lang);
    (d.name, d.group, d.scope)
}

// Accounts and calendar.
pub const CALENDARS: Words = Words::new("Calendars", "Agendas");
pub const NO_CALENDAR: Words = Words::new(
    "No calendar yet: add a CalDAV account or a subscription link.",
    "Aucun agenda pour l’instant : ajoutez un compte CalDAV ou un lien d’abonnement.",
);
pub const ADD_CALENDAR: Words = Words::new("Add a calendar", "Ajouter un agenda");
pub const CALENDAR_NAME: Words = Words::new("Name", "Nom");
pub const CALENDAR_NAME_HINT: Words = Words::new("Work", "Travail");
pub const CALENDAR_URL: Words = Words::new("Address", "Adresse");
pub const CALENDAR_URL_HINT: Words = Words::new(
    "https://… (CalDAV server or .ics link)",
    "https://… (serveur CalDAV ou lien .ics)",
);
pub const CALENDAR_USER: Words = Words::new("User name", "Identifiant");
pub const CALENDAR_PASSWORD: Words = Words::new("Password", "Mot de passe");
pub const CALENDAR_NOTE: Words = Words::new(
    "For a subscription link, leave the user name and password empty. The password stays in the keyring.",
    "Pour un lien d’abonnement, laissez l’identifiant et le mot de passe vides. Le mot de passe reste dans le trousseau.",
);
pub const CALENDAR_BAD_URL: Words = Words::new(
    "The address must start with https:// so that the password never travels in the clear.",
    "L’adresse doit commencer par https:// pour que le mot de passe ne circule jamais en clair.",
);
pub const REMOVE: Words = Words::new("Remove", "Retirer");
pub const SYNCING: Words = Words::new("Syncing…", "Synchronisation…");
pub const WRONG_PASSWORD: Words = Words::new(
    "The server refused the user name or password",
    "Le serveur a refusé l’identifiant ou le mot de passe",
);
pub const UNREACHABLE: Words = Words::new(
    "Not reached: showing what was saved",
    "Injoignable : ce qui était enregistré reste affiché",
);

pub const FROM_GOA: Words = Words::new(
    "From GNOME Online Accounts",
    "Depuis les comptes en ligne de GNOME",
);
pub const GOA_NEEDED: Words = Words::new(
    "Google calendars need GNOME Online Accounts: install it and sign in to Google there, and the calendars show here.",
    "Les agendas Google demandent les comptes en ligne de GNOME (GNOME Online Accounts) : installez-les et connectez-vous à Google, les agendas s’afficheront ici.",
);
pub const NO_GOOGLE_ACCOUNT: Words = Words::new(
    "No Google account with its calendar on in GNOME Online Accounts.",
    "Aucun compte Google avec son agenda activé dans les comptes en ligne de GNOME.",
);
pub const OPEN_GOA: Words = Words::new("Open Online Accounts", "Ouvrir les comptes en ligne");
pub const GOA_ELSEWHERE: Words = Words::new(
    "Google accounts are added and removed in GNOME Online Accounts, not here.",
    "Les comptes Google s’ajoutent et se retirent dans les comptes en ligne de GNOME, pas ici.",
);
pub const GOOGLE_SIGN_IN_AGAIN: Words = Words::new(
    "Google refused the sign-in: sign in again in GNOME Online Accounts",
    "Google a refusé la connexion : reconnectez-vous dans les comptes en ligne de GNOME",
);

pub fn synced(events: usize, lang: Lang) -> String {
    match (lang, events) {
        (Lang::En, 1) => "Synced · 1 event".into(),
        (Lang::En, n) => format!("Synced · {n} events"),
        (Lang::Fr, 1) => "Synchronisé · 1 événement".into(),
        (Lang::Fr, n) => format!("Synchronisé · {n} événements"),
    }
}

pub const HOT_CORNERS: Words = Words::new("Hot corners", "Coins actifs");
pub const HOT_CORNERS_NOTE: Words = Words::new(
    "Rest the pointer in a corner of a screen for it to act. A corner against another screen does nothing.",
    "Laissez le pointeur dans un coin d’un écran pour qu’il agisse. Un coin contre un autre écran ne fait rien.",
);
pub const TOP_LEFT: Words = Words::new("Top left", "En haut à gauche");
pub const TOP_RIGHT: Words = Words::new("Top right", "En haut à droite");
pub const BOTTOM_LEFT: Words = Words::new("Bottom left", "En bas à gauche");
pub const BOTTOM_RIGHT: Words = Words::new("Bottom right", "En bas à droite");
pub const OVERVIEW: Words = Words::new("Overview", "Vue d’ensemble");
pub const LAUNCHER: Words = Words::new("Launcher", "Lanceur");
pub const SHOW_DESKTOP: Words = Words::new("Desktop", "Bureau");
pub const NOTHING: Words = Words::new("Nothing", "Rien");

/// What a hot corner does, as the settings name it.
pub fn trigger(t: hocus_core::config::Trigger) -> Words {
    use hocus_core::config::Trigger;
    match t {
        Trigger::Overview => OVERVIEW,
        Trigger::Launcher => LAUNCHER,
        Trigger::ShowDesktop => SHOW_DESKTOP,
        Trigger::None => NOTHING,
    }
}

pub const INTERFACE_LANGUAGE: Words = Words::new("Interface language", "Langue de l’interface");
pub const INTERFACE_LANGUAGE_NOTE: Words = Words::new(
    "The column, panels, menus and this app change at once; applications follow their own setting.",
    "La colonne, les panneaux, les menus et cette appli changent tout de suite ; les applications suivent leur propre réglage.",
);
pub const LIKE_THE_SYSTEM: Words = Words::new("Like the system", "Comme le système");
/// Language names are written in their own language.
pub const ENGLISH: Words = Words::new("English", "English");
pub const FRENCH: Words = Words::new("Français", "Français");

pub const WELCOME_TITLE: Words = Words::new("Welcome to Hocuspanda", "Bienvenue dans Hocuspanda");
pub const WELCOME_LINE: Words = Words::new(
    "A fast, beautiful desktop, with pandas that show what is going on without ever interrupting. Choose a few things now: everything can be changed later in Settings.",
    "Un bureau rapide et beau, avec des pandas qui montrent ce qui se passe, sans jamais interrompre. Choisissez quelques réglages maintenant : tout se change plus tard dans les Réglages.",
);
pub const KEYBOARD_LAYOUT: Words = Words::new("Keyboard", "Clavier");
pub const NEXT: Words = Words::new("Next", "Ensuite");
pub const WIFI: Words = Words::new("Wi-Fi", "Wi-Fi");
pub const AI: Words = Words::new("AI and agents", "IA et agents");
pub const ALL_SETTINGS: Words = Words::new("All settings", "Tous les réglages");
pub const START: Words = Words::new("Start", "Commencer");
pub const VERSION: Words = Words::new("Version", "Version");
pub const LICENCES: Words = Words::new("Licences", "Licences");
pub const LICENCES_TEXT: Words = Words::new(
    "Hocuspanda's code is under the MIT licence. The pandas and the icons are drawn for Hocuspanda and come with the code, under the same licence. The fonts, Manrope, Fraunces, JetBrains Mono and Space Grotesk, are under the SIL Open Font License.",
    "Le code de Hocuspanda est sous licence MIT. Les pandas et les icônes sont dessinés pour Hocuspanda et viennent avec le code, sous la même licence. Les polices, Manrope, Fraunces, JetBrains Mono et Space Grotesk, sont sous licence SIL Open Font License.",
);
pub const CREDITS: Words = Words::new("Credits", "Remerciements");
pub const CREDITS_TEXT: Words = Words::new(
    "Hocuspanda stands on smithay, calloop, vello, parley, zbus and many other free libraries: thank you to everyone who makes them.",
    "Hocuspanda s’appuie sur smithay, calloop, vello, parley, zbus et bien d’autres bibliothèques libres : merci à toutes celles et ceux qui les font.",
);

// Wallpaper.
pub const STYLE: Words = Words::new("Style", "Style");
pub const WALLPAPERS: [(&str, Words, Words); 6] = [
    (
        "glow",
        Words::new("Glow", "Lueur"),
        Words::new("soft light by the hour", "douce, selon l’heure"),
    ),
    (
        "image",
        Words::new("Picture", "Image"),
        Words::new("one of yours", "une des tiennes"),
    ),
    (
        "folder",
        Words::new("Folder", "Dossier"),
        Words::new("its pictures through the day", "ses images au fil du jour"),
    ),
    (
        "colour",
        Words::new("Colour", "Couleur"),
        Words::new("plain", "unie"),
    ),
    (
        "rain",
        Words::new("Rain", "Pluie"),
        Words::new("the Matrix rain", "la pluie de Matrix"),
    ),
    (
        "stars",
        Words::new("Stars", "Étoiles"),
        Words::new("a slow night sky", "un ciel de nuit lent"),
    ),
];
pub const MOVES_NOTE: Words = Words::new(
    "Rain and stars move; they hold still with reduced motion, on the power saver, under a fullscreen window and without a graphics card.",
    "La pluie et les étoiles bougent ; elles restent immobiles avec le mouvement réduit, en économie d’énergie, sous une fenêtre en plein écran et sans carte graphique.",
);
pub const SCREEN: Words = Words::new("Screen", "Écran");
pub const EVERY_SCREEN: Words = Words::new("Every screen", "Tous les écrans");
pub const OWN_WALLPAPER: Words = Words::new(
    "This screen has its own wallpaper.",
    "Cet écran a son propre fond.",
);
pub const LIKE_EVERY_SCREEN: Words = Words::new(
    "It shows every screen’s until changed here.",
    "Il montre celui de tous les écrans tant qu’il n’est pas changé ici.",
);
pub const LIKE_THE_OTHERS: Words = Words::new("Like the other screens", "Comme les autres écrans");
pub const PICTURE: Words = Words::new("Picture", "Image");
pub const PICTURE_FILE: Words = Words::new("Picture file", "Fichier image");
pub const PICTURE_HINT: Words = Words::new("~/Pictures/hills.jpg", "~/Images/collines.jpg");
pub const FOLDER: Words = Words::new("Folder", "Dossier");
pub const FOLDER_FIELD: Words = Words::new("Folder of pictures", "Dossier d’images");
pub const FOLDER_HINT: Words = Words::new("~/Pictures/day", "~/Images/jour");
pub const PATH_NOTE: Words = Words::new(
    "Type its path, then Enter.",
    "Tape son chemin, puis Entrée.",
);
pub const FOLDER_NOTE: Words = Words::new(
    "Its pictures follow one another through the day, in name order.",
    "Ses images se suivent au fil du jour, dans l’ordre de leur nom.",
);
pub const FIT: Words = Words::new("Fit", "Ajustement");
pub const MODES: [(&str, Words); 4] = [
    ("fill", Words::new("Fill", "Remplir")),
    ("fit", Words::new("Fit", "Adapter")),
    ("stretch", Words::new("Stretch", "Étirer")),
    ("center", Words::new("Centre", "Centrer")),
];
pub const COLOUR: Words = Words::new("Colour", "Couleur");
pub const COLOUR_CODE: Words = Words::new("Colour code", "Code couleur");
pub const THE_THEMES: Words = Words::new("The theme’s", "Celle du thème");
pub const COLOUR_NOTE: Words = Words::new(
    "Around a fitted or centred picture, or the whole screen.",
    "Autour d’une image adaptée ou centrée, ou tout l’écran.",
);
pub const NOT_A_COLOUR: Words = Words::new(
    "Not a colour: write it like #0e0f12.",
    "Pas une couleur : écris-la comme #0e0f12.",
);
pub const COLOURS: [(&str, Words); 6] = [
    ("#0e0f12", Words::new("Ink", "Encre")),
    ("#14213d", Words::new("Night blue", "Bleu nuit")),
    ("#1f3326", Words::new("Forest", "Forêt")),
    ("#3b2238", Words::new("Plum", "Prune")),
    ("#7a4b2a", Words::new("Clay", "Argile")),
    ("#e9e6df", Words::new("Sand", "Sable")),
];

/// "Colour Ink", "Couleur Encre": a swatch's name.
pub fn colour_named(name: Words, lang: Lang) -> String {
    format!("{} {}", COLOUR.get(lang), name.get(lang))
}

/// The name of a wallpaper style as the user reads it.
pub fn wallpaper_name(kind: &str, lang: Lang) -> String {
    WALLPAPERS
        .iter()
        .find(|(id, _, _)| *id == kind)
        .map_or_else(|| kind.to_owned(), |(_, name, _)| name.get(lang).to_owned())
}

// Column and dock.
pub const COLUMN: Words = Words::new("Column", "Colonne");
pub const COLUMN_NOTE: Words = Words::new(
    "The clock, the agenda and notifications always show; these lines when there is something to say.",
    "L’horloge, l’agenda et les notifications sont toujours là ; ces lignes quand il y a quelque chose à dire.",
);
pub const AGENTS: Words = Words::new("Coding agents", "Agents de code");
pub const AGENTS_SUB: Words = Words::new(
    "Each with its panda, from herdr",
    "Chacun avec son panda, depuis herdr",
);
pub const PHONE: Words = Words::new("Phone", "Téléphone");
pub const PHONE_SUB: Words = Words::new(
    "Battery and Ring, through KDE Connect",
    "Batterie et Sonner, par KDE Connect",
);
pub const SYSTEM_UPDATES: Words = Words::new("System updates", "Mises à jour du système");
pub const SYSTEM_UPDATES_SUB: Words = Words::new(
    "Looked for every few hours",
    "Cherchées toutes les quelques heures",
);
pub const DOCK: Words = Words::new("Dock", "Dock");
pub const DOCK_SUB: Words = Words::new(
    "Super+D, an empty desktop or the bottom edge",
    "Super+D, un bureau vide ou le bord du bas",
);
pub const PINNED: Words = Words::new("Kept in the dock", "Gardées dans le dock");
pub const PINNED_NOTE: Words = Words::new(
    "Open applications come after them; those not installed are left out.",
    "Les applications ouvertes viennent après ; celles qui ne sont pas installées sont omises.",
);
pub const NO_PINNED: Words = Words::new(
    "None: the dock shows the open applications.",
    "Aucune : le dock montre les applications ouvertes.",
);

/// An application's name from its desktop entry's: the last part of
/// `org.codeberg.dnkl.foot.desktop`, capitalised.
pub fn app_name(entry: &str) -> String {
    let id = entry.strip_suffix(".desktop").unwrap_or(entry);
    let last = id.rsplit('.').next().unwrap_or(id);
    let mut chars = last.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

// Pandas.
pub const SHOW_PANDAS: Words = Words::new("Show the pandas", "Montrer les pandas");
pub const SHOW_PANDAS_SUB: Words = Words::new(
    "Off, none is drawn anywhere",
    "Désactivé, aucun n’est dessiné nulle part",
);
pub const PANDAS_NOTE: Words = Words::new(
    "They show what happens, move a moment when it changes, then hold still. Quick settings hide them for the session only.",
    "Ils montrent ce qui se passe, bougent un instant quand ça change, puis restent immobiles. Les réglages rapides les cachent pour la session seulement.",
);
pub const EAR: Words = Words::new("What their ear says", "Ce que dit leur oreille");
pub const MOODS: [(hocus_shell::panda::Mood, Words); 5] = [
    (hocus_shell::panda::Mood::Work, Words::new("Acts", "Agit")),
    (
        hocus_shell::panda::Mood::Wait,
        Words::new("Waits for you", "T’attend"),
    ),
    (hocus_shell::panda::Mood::Done, Words::new("Done", "A fini")),
    (
        hocus_shell::panda::Mood::Error,
        Words::new("Stuck", "Bloqué"),
    ),
    (
        hocus_shell::panda::Mood::Sleep,
        Words::new("Asleep", "Dort"),
    ),
];
pub const WHERE_PANDAS: Words = Words::new("Where they show", "Où ils apparaissent");
pub const PANDAS_WHERE: [Words; 3] = [
    Words::new(
        "Beside each coding agent, in the column.",
        "À côté de chaque agent de code, dans la colonne.",
    ),
    Words::new(
        "With popcorn on the other screens while a video plays fullscreen.",
        "Avec du pop-corn sur les autres écrans pendant une vidéo en plein écran.",
    ),
    Words::new(
        "When the session starts, under the curtain and on the lock screen.",
        "Au début de la session, sous le rideau et sur l’écran de verrouillage.",
    ),
];

// Accessibility.
pub const SEEING: Words = Words::new("Seeing", "Vue");
pub const MOTION: Words = Words::new("Motion", "Mouvement");
pub const MAGNIFIER: Words = Words::new("Magnifier", "Loupe");
pub const ZOOM_IN: Words = Words::new("Zoom in", "Agrandir");
pub const ZOOM_OUT: Words = Words::new("Zoom out", "Réduire");
pub const MAGNIFIER_NOTE: Words = Words::new(
    "Around the pointer, up to 8×; Super+Alt+wheel too. Back to 1×, it is off.",
    "Autour du pointeur, jusqu’à 8× ; Super+Alt+molette aussi. Revenue à 1×, elle s’éteint.",
);
pub const CHANGE_KEYS: Words = Words::new("Change the keys", "Changer les touches");
pub const NO_KEYS: Words = Words::new("No keys", "Aucune touche");

// Language and region.
pub const CLOCK: Words = Words::new("Clock", "Horloge");
pub const HOURS_24: Words = Words::new("24 hours · 14:37", "24 heures · 14:37");
pub const HOURS_12: Words = Words::new("12 hours · 2:37 PM", "12 heures · 2:37 PM");
pub const TIME_ZONE: Words = Words::new("Time zone", "Fuseau horaire");
pub const TIME_ZONE_NOTE: Words = Words::new(
    "The system’s: the clock, the agenda, the lock screen and applications follow it. Changing it asks for an administrator’s password.",
    "Celui du système : l’horloge, l’agenda, l’écran de verrouillage et les applications le suivent. Le changer demande le mot de passe d’un administrateur.",
);
pub const NO_TIMEDATED: Words = Words::new(
    "The system’s time zone is set through systemd-timedated, which is not here: change it with the system’s own tools.",
    "Le fuseau du système se règle par systemd-timedated, absent ici : changez-le avec les outils du système.",
);

pub const MAP_NAME: Words = Words::new(
    "World map of the time zones",
    "Carte du monde des fuseaux horaires",
);
pub const MAP_NOTE: Words = Words::new(
    "Click near a city to take its time zone; the list below offers the main ones by name.",
    "Cliquez près d’une ville pour prendre son fuseau ; la liste ci-dessous propose les principales par leur nom.",
);

/// The zone the map rings: "In effect: Paris".
pub fn zone_in_effect(zone: &str, lang: Lang) -> String {
    let city = zone.rsplit('/').next().unwrap_or(zone);
    match lang {
        Lang::En => format!("In effect: {city}"),
        Lang::Fr => format!("En vigueur\u{a0}: {city}"),
    }
}

/// The system did not change its zone, and why.
pub fn zone_refused(why: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Not changed: {why}"),
        Lang::Fr => format!("Pas changé\u{a0}: {why}"),
    }
}

/// Time zones offered, by region: the city named, its zone.
pub const ZONES: [(Words, &[(&str, Words)]); 4] = [
    (
        Words::new("Europe", "Europe"),
        &[
            ("Europe/London", Words::new("London", "Londres")),
            ("Europe/Lisbon", Words::new("Lisbon", "Lisbonne")),
            ("Europe/Paris", Words::new("Paris", "Paris")),
            ("Europe/Brussels", Words::new("Brussels", "Bruxelles")),
            ("Europe/Zurich", Words::new("Zurich", "Zurich")),
            ("Europe/Berlin", Words::new("Berlin", "Berlin")),
            ("Europe/Madrid", Words::new("Madrid", "Madrid")),
            ("Europe/Rome", Words::new("Rome", "Rome")),
            ("Europe/Athens", Words::new("Athens", "Athènes")),
            ("Europe/Istanbul", Words::new("Istanbul", "Istanbul")),
            ("Europe/Moscow", Words::new("Moscow", "Moscou")),
        ],
    ),
    (
        Words::new("Africa", "Afrique"),
        &[
            ("Africa/Casablanca", Words::new("Casablanca", "Casablanca")),
            ("Africa/Dakar", Words::new("Dakar", "Dakar")),
            ("Africa/Abidjan", Words::new("Abidjan", "Abidjan")),
            ("Africa/Lagos", Words::new("Lagos", "Lagos")),
            ("Africa/Cairo", Words::new("Cairo", "Le Caire")),
            ("Africa/Nairobi", Words::new("Nairobi", "Nairobi")),
            (
                "Africa/Johannesburg",
                Words::new("Johannesburg", "Johannesburg"),
            ),
            ("Indian/Reunion", Words::new("Réunion", "La Réunion")),
        ],
    ),
    (
        Words::new("Americas", "Amériques"),
        &[
            ("America/Vancouver", Words::new("Vancouver", "Vancouver")),
            (
                "America/Los_Angeles",
                Words::new("Los Angeles", "Los Angeles"),
            ),
            ("America/Denver", Words::new("Denver", "Denver")),
            ("America/Chicago", Words::new("Chicago", "Chicago")),
            ("America/Mexico_City", Words::new("Mexico City", "Mexico")),
            ("America/Toronto", Words::new("Montreal", "Montréal")),
            ("America/New_York", Words::new("New York", "New York")),
            ("America/Guadeloupe", Words::new("Guadeloupe", "Guadeloupe")),
            ("America/Bogota", Words::new("Bogotá", "Bogota")),
            ("America/Sao_Paulo", Words::new("São Paulo", "São Paulo")),
            (
                "America/Argentina/Buenos_Aires",
                Words::new("Buenos Aires", "Buenos Aires"),
            ),
            ("Pacific/Honolulu", Words::new("Honolulu", "Honolulu")),
        ],
    ),
    (
        Words::new("Asia and Oceania", "Asie et Océanie"),
        &[
            ("Asia/Dubai", Words::new("Dubai", "Dubaï")),
            ("Asia/Kolkata", Words::new("Mumbai", "Bombay")),
            ("Asia/Bangkok", Words::new("Bangkok", "Bangkok")),
            ("Asia/Singapore", Words::new("Singapore", "Singapour")),
            ("Asia/Shanghai", Words::new("Shanghai", "Shanghai")),
            ("Asia/Seoul", Words::new("Seoul", "Séoul")),
            ("Asia/Tokyo", Words::new("Tokyo", "Tokyo")),
            ("Australia/Sydney", Words::new("Sydney", "Sydney")),
            ("Pacific/Noumea", Words::new("Nouméa", "Nouméa")),
            ("Pacific/Auckland", Words::new("Auckland", "Auckland")),
            ("Pacific/Tahiti", Words::new("Tahiti", "Tahiti")),
        ],
    ),
];

/// A time zone as the user reads it: its city when offered, else its
/// name in the database; the system's when empty.
pub fn zone_name(zone: &str, lang: Lang) -> String {
    if zone.is_empty() {
        return LIKE_THE_SYSTEM.get(lang).to_owned();
    }
    if zone == "UTC" {
        return "UTC".into();
    }
    ZONES
        .iter()
        .flat_map(|(_, cities)| cities.iter())
        .find(|(id, _)| *id == zone)
        .map_or_else(
            || zone.replace('_', " "),
            |(_, city)| city.get(lang).to_owned(),
        )
}

/// "24 h" or "12 h", for summaries.
pub fn clock_short(format: hocus_core::config::ClockFormat) -> &'static str {
    match format {
        hocus_core::config::ClockFormat::TwentyFour => "24 h",
        hocus_core::config::ClockFormat::Twelve => "12 h",
    }
}

// Desktops.
pub const NEW_SCREENS: Words = Words::new("New screens", "Nouveaux écrans");
pub const NEW_SCREENS_NOTE: Words = Words::new(
    "Desktops of a screen plugged for the first time; each then keeps its own.",
    "Les bureaux d’un écran branché pour la première fois ; chacun garde ensuite les siens.",
);
pub const DESKTOP_NAMES_NOTE: Words = Words::new(
    "A name shows in the column and in bars that list desktops; empty, the desktop goes by its number. Type it, then Enter.",
    "Un nom s’affiche dans la colonne et dans les barres qui listent les bureaux ; vide, le bureau garde son numéro. Tape-le, puis Entrée.",
);
pub const SWITCHING: Words = Words::new("Switching", "Changer de bureau");
pub const SWITCHING_NOTE: Words = Words::new(
    "Super+1…9, Ctrl+Alt+Left and Right, or three fingers sideways: always on the screen under the pointer.",
    "Super+1…9, Ctrl+Alt+Gauche et Droite, ou trois doigts sur le côté : toujours sur l’écran sous le pointeur.",
);
pub const FEWER_NEW: Words = Words::new(
    "One desktop fewer for new screens",
    "Un bureau de moins pour les nouveaux écrans",
);
pub const MORE_NEW: Words = Words::new(
    "One desktop more for new screens",
    "Un bureau de plus pour les nouveaux écrans",
);
pub const DESKTOP_NAMES: Words = Words::new("Desktop names", "Noms des bureaux");

/// "Name of desktop 2", told apart by screen when there are several.
pub fn desktop_name_field(n: usize, screen: Option<&str>, lang: Lang) -> String {
    match (lang, screen) {
        (Lang::En, None) => format!("Name of desktop {n}"),
        (Lang::En, Some(s)) => format!("Name of desktop {n} on {s}"),
        (Lang::Fr, None) => format!("Nom du bureau {n}"),
        (Lang::Fr, Some(s)) => format!("Nom du bureau {n} sur {s}"),
    }
}

/// "Desktop 2", a name field's hint.
pub fn desktop_hint(n: usize, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Desktop {n}"),
        Lang::Fr => format!("Bureau {n}"),
    }
}

/// A stepper's button, told apart by screen when there are several.
pub fn on_screen(words: Words, screen: Option<&str>, lang: Lang) -> String {
    match (lang, screen) {
        (_, None) => words.get(lang).to_owned(),
        (Lang::En, Some(s)) => format!("{} on {s}", words.get(lang)),
        (Lang::Fr, Some(s)) => format!("{} sur {s}", words.get(lang)),
    }
}

// Windows.
pub const SNAPPING: Words = Words::new("Snapping", "Aimantation");
pub const SNAP_EDGES: Words = Words::new("Snap at the edges", "Aimanter aux bords");
pub const SNAP_EDGES_SUB: Words = Words::new(
    "Dragged to an edge or a corner: a half, a quarter, the top maximizes",
    "Glissée vers un bord ou un coin : une moitié, un quart, le haut agrandit",
);
pub const SNAP_NOTE: Words = Words::new(
    "Off, windows move freely; their edges still stick to the screen’s and to each other. Super+Left and Super+Right snap either way.",
    "Désactivé, les fenêtres bougent librement ; leurs bords collent toujours à ceux de l’écran et entre eux. Super+Gauche et Super+Droite aimantent dans tous les cas.",
);
pub const GAPS: Words = Words::new("Gaps", "Espaces");
pub const GAPS_INNER: Words = Words::new("Between windows", "Entre les fenêtres");
pub const GAPS_OUTER: Words = Words::new("At the edges", "Aux bords");
pub const GAPS_NOTE: Words = Words::new(
    "Around maximized, snapped and tiled windows; in logical pixels.",
    "Autour des fenêtres agrandies, aimantées et en mosaïque ; en pixels logiques.",
);
pub const EFFECTS: Words = Words::new("Effects", "Effets");
pub const WOBBLY: Words = Words::new("Wobbly windows", "Fenêtres en gelée");
pub const WOBBLY_SUB: Words = Words::new(
    "They bend while dragged",
    "Elles plient quand on les glisse",
);
pub const SHADOWS: Words = Words::new("Shadows", "Ombres");
pub const SHADOWS_SUB: Words = Words::new(
    "Soft, as dark as the theme",
    "Douces, aussi sombres que le thème",
);
pub const TITLE_BARS: Words = Words::new("Title bars", "Barres de titre");
pub const TITLE_BARS_SUB: Words = Words::new("Above each window", "Au-dessus de chaque fenêtre");
pub const CORNERS: Words = Words::new("Rounded corners", "Coins arrondis");
pub const EFFECTS_NOTE: Words = Words::new(
    "Thrown fast, a window glides and lands, on another screen if aimed there. Reduced motion stills them all.",
    "Lancée vite, une fenêtre glisse et se pose, sur un autre écran si on la vise. Le mouvement réduit les arrête toutes.",
);
pub const RULES: Words = Words::new("Window rules", "Règles des fenêtres");
pub const RULES_NOTE: Words = Words::new(
    "The windows of an app, by its id or a pattern (* and ?), open as set here; a later rule wins over an earlier one. Desktop, screen, size and opacity are set in the file.",
    "Les fenêtres d’une appli, par son identifiant ou un motif (* et ?), s’ouvrent comme réglé ici ; une règle plus bas l’emporte. Bureau, écran, taille et opacité se règlent dans le fichier.",
);
pub const NO_RULE: Words = Words::new(
    "No rule: windows open where the pointer is.",
    "Aucune règle : les fenêtres s’ouvrent là où est le pointeur.",
);
pub const ANY_APP: Words = Words::new("Any app", "Toute appli");
pub const RULE_MAXIMIZED: Words = Words::new("Maximized", "Agrandie");
pub const RULE_PINNED: Words = Words::new("On every desktop", "Sur tous les bureaux");
pub const RULE_ON_TOP: Words = Words::new("Above the others", "Au-dessus des autres");
pub const RULE_BLUR: Words = Words::new("Blur behind", "Flou derrière");
pub const NEW_RULE: Words = Words::new("App of a new rule", "Appli d’une nouvelle règle");
pub const NEW_RULE_HINT: Words = Words::new(
    "Its id or a pattern: org.mozilla.firefox, chat*",
    "Son identifiant ou un motif : org.mozilla.firefox, chat*",
);
pub const ADD_RULE: Words = Words::new("Add the rule", "Ajouter la règle");

/// "chat*: Maximized", a rule's choice for screen readers.
pub fn of_rule(pattern: &str, what: Words, lang: Lang) -> String {
    match lang {
        Lang::En => format!("{pattern}: {}", what.get(lang)),
        Lang::Fr => format!("{pattern} : {}", what.get(lang)),
    }
}

pub fn remove_rule(pattern: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Remove the rule for {pattern}"),
        Lang::Fr => format!("Retirer la règle de {pattern}"),
    }
}

/// What a rule sets that the page does not change, in words.
pub fn rule_detail(rule: &hocus_core::config::Rule, lang: Lang) -> String {
    let mut said = Vec::new();
    if let Some(t) = &rule.title {
        said.push(match lang {
            Lang::En => format!("title {t}"),
            Lang::Fr => format!("titre {t}"),
        });
    }
    if let Some(d) = rule.desktop {
        said.push(match lang {
            Lang::En => format!("desktop {d}"),
            Lang::Fr => format!("bureau {d}"),
        });
    }
    if let Some(o) = &rule.output {
        said.push(o.clone());
    }
    if let Some([w, h]) = rule.size {
        said.push(format!("{w} × {h}"));
    }
    if let Some(o) = rule.opacity {
        let p = (o * 100.0).round() as u32;
        said.push(match lang {
            Lang::En => format!("opacity {}", percent(p, lang)),
            Lang::Fr => format!("opacité {}", percent(p, lang)),
        });
    }
    said.join(" · ")
}

/// "8 px", "Between windows: 8 px".
pub fn pixels(n: u32) -> String {
    format!("{n} px")
}

pub fn named_value(name: Words, value: &str, lang: Lang) -> String {
    name_value(name.get(lang), value, lang)
}

/// The same, the name said as it comes (an application's).
pub fn name_value(name: &str, value: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("{name}: {value}"),
        Lang::Fr => format!("{name} : {value}"),
    }
}

// Mouse and touchpad.
pub const TOUCHPAD: Words = Words::new("Touchpad", "Pavé tactile");
pub const MOUSE: Words = Words::new("Mouse", "Souris");
pub const NATURAL_SCROLL: Words = Words::new("Natural scrolling", "Défilement naturel");
pub const NATURAL_SCROLL_SUB: Words = Words::new(
    "The content follows the fingers",
    "Le contenu suit les doigts",
);
pub const NATURAL_WHEEL_SUB: Words = Words::new(
    "The content follows the wheel, as on a touchpad",
    "Le contenu suit la molette, comme sur un pavé",
);
pub const TAP_TO_CLICK: Words = Words::new("Tap to click", "Toucher pour cliquer");
pub const TAP_TO_CLICK_SUB: Words = Words::new(
    "One finger the left button, two the right",
    "Un doigt le bouton gauche, deux le droit",
);
pub const WHILE_TYPING: Words = Words::new("Off while typing", "Coupé pendant la frappe");
pub const WHILE_TYPING_SUB: Words = Words::new(
    "A palm brushing it does nothing",
    "Une paume qui l’effleure ne fait rien",
);
pub const LEFT_HANDED: Words = Words::new("Left-handed", "Gaucher");
pub const LEFT_HANDED_SUB: Words = Words::new(
    "The buttons swapped, the touchpad’s too",
    "Les boutons inversés, ceux du pavé aussi",
);
pub const SPEED: Words = Words::new("Speed", "Vitesse");
pub const SPEEDS: [(f64, Words); 5] = [
    (-0.6, Words::new("Slowest", "Très lente")),
    (-0.3, Words::new("Slow", "Lente")),
    (0.0, Words::new("Normal", "Normale")),
    (0.3, Words::new("Fast", "Rapide")),
    (0.6, Words::new("Fastest", "Très rapide")),
];
pub const POINTING_NOTE: Words = Words::new(
    "Set at once on a session of its own; in a window of another session, that one’s settings apply.",
    "Réglé à l’instant sur une session à part entière ; dans une fenêtre d’une autre session, ce sont ses réglages qui s’appliquent.",
);
pub const GESTURES: Words = Words::new("Gestures", "Gestes");
pub const THREE_SIDEWAYS: Words = Words::new("Three fingers sideways", "Trois doigts sur le côté");
pub const THREE_SIDEWAYS_SUB: Words = Words::new(
    "Switch desktops, the windows following the fingers",
    "Changent de bureau, les fenêtres suivant les doigts",
);
pub const THREE_UP: Words = Words::new("Three fingers up", "Trois doigts vers le haut");
pub const THREE_DOWN: Words = Words::new("Three fingers down", "Trois doigts vers le bas");
pub const FOUR_UP: Words = Words::new("Four fingers up", "Quatre doigts vers le haut");
pub const FOUR_DOWN: Words = Words::new("Four fingers down", "Quatre doigts vers le bas");
pub const SUPER_ALONE: Words = Words::new("Super alone", "Super seul");
pub const GESTURES_NOTE: Words = Words::new(
    "Two fingers stay the applications’: scrolling, zooming in the browser. A pinch closing in opens the window switcher.",
    "Deux doigts restent aux applications : défiler, zoomer dans le navigateur. Un pincement qui se referme ouvre le sélecteur de fenêtres.",
);

/// "Touchpad: Natural scrolling", told apart from the mouse's.
pub fn of_device(device: Words, what: Words, lang: Lang) -> String {
    match lang {
        Lang::En => format!("{}: {}", device.get(lang), what.get(lang)),
        Lang::Fr => format!("{} : {}", device.get(lang), what.get(lang)),
    }
}

// Screenshots and recording.
pub const SCREENSHOTS: Words = Words::new("Screenshots", "Captures d’écran");
pub const SAVE_SHOTS: Words = Words::new("Save to a folder", "Enregistrer dans un dossier");
pub const SAVE_SHOTS_SUB: Words = Words::new("Each capture as a PNG", "Chaque capture en PNG");
pub const COPY_SHOTS: Words = Words::new("Copy to the clipboard", "Copier dans le presse-papiers");
pub const COPY_SHOTS_SUB: Words = Words::new("To paste it at once", "Pour la coller aussitôt");
pub const SHOTS_FOLDER: Words = Words::new("Screenshots folder", "Dossier des captures");
pub const SHOTS_FOLDER_HINT: Words = Words::new(
    "Empty: Screenshots in the pictures folder",
    "Vide : Captures dans le dossier des images",
);
pub const SHOTS_NOTE: Words = Words::new(
    "A toast then offers to open, annotate, delete or copy its text.",
    "Une bulle propose ensuite de l’ouvrir, l’annoter, la supprimer ou copier son texte.",
);
pub const RECORDING: Words = Words::new("Recording", "Enregistrement");
pub const FPS: Words = Words::new("Frames a second", "Images par seconde");
pub const COUNTDOWN: Words = Words::new("Countdown", "Décompte");
pub const SHOW_CLICKS: Words = Words::new("Show clicks", "Montrer les clics");
pub const SHOW_CLICKS_SUB: Words = Words::new(
    "Rings in the video, never on the screen",
    "Des anneaux dans la vidéo, jamais à l’écran",
);
pub const SHOW_KEYS: Words = Words::new("Show keys", "Montrer les touches");
pub const SHOW_KEYS_SUB: Words = Words::new(
    "Combinations drawn in the video, never plain typing",
    "Les combinaisons dans la vidéo, jamais la frappe",
);
pub const MICROPHONE: Words = Words::new("Microphone", "Micro");
pub const MICROPHONE_SUB: Words = Words::new("Your voice", "Ta voix");
pub const SYSTEM_SOUND: Words = Words::new("System sound", "Son du système");
pub const SYSTEM_SOUND_SUB: Words = Words::new("What plays", "Ce qui joue");
pub const WEBCAM: Words = Words::new("Webcam", "Webcam");
pub const WEBCAM_SUB: Words = Words::new(
    "A round bubble, moved with the pointer",
    "Une bulle ronde, déplacée au pointeur",
);
pub const HARDWARE: Words = Words::new("Graphics card encodes", "Encodage par la carte graphique");
pub const HARDWARE_SUB: Words =
    Words::new("Lighter, where it can", "Plus léger, quand elle le peut");
pub const RECORDING_FOLDER: Words = Words::new("Recordings folder", "Dossier des enregistrements");
pub const RECORDING_FOLDER_HINT: Words = Words::new(
    "Empty: Screencasts in the videos folder",
    "Vide : Screencasts dans le dossier des vidéos",
);
pub const RECORDING_NOTE: Words = Words::new(
    "Sound, webcam and keys start like this when picking what to record; the picker changes them for that recording.",
    "Le son, la webcam et les touches démarrent ainsi au choix de ce qu’on enregistre ; le sélecteur les change pour cet enregistrement.",
);
pub const FOLDER_FIELD_NOTE: Words = Words::new(
    "Type its path, then Enter; empty, the usual folder.",
    "Tape son chemin, puis Entrée ; vide, le dossier habituel.",
);

/// "3 s".
pub fn seconds(n: u32, lang: Lang) -> String {
    match lang {
        Lang::En => format!("{n} s"),
        Lang::Fr => format!("{n}\u{a0}s"),
    }
}

// Clipboard and emoji.
pub const CLIPBOARD_HISTORY: Words =
    Words::new("Clipboard history", "Historique du presse-papiers");
pub const KEEP_HISTORY: Words = Words::new("Keep a history", "Garder un historique");
pub const KEEP_HISTORY_SUB: Words = Words::new(
    "Never what password managers mark secret",
    "Jamais les mots de passe",
);
pub const ACROSS_SESSIONS: Words = Words::new("Across sessions", "D’une session à l’autre");
pub const ACROSS_SESSIONS_SUB: Words =
    Words::new("Kept encrypted on disk", "Gardé chiffré sur le disque");
pub const ENTRIES: Words = Words::new("Copies kept", "Copies gardées");
pub const CLIPBOARD_NOTE: Words = Words::new(
    "Newest first; Enter pastes into the focused app, Tab pins, Delete forgets. Pinned ones are kept besides.",
    "Les plus récentes d’abord ; Entrée colle dans l’appli active, Tab épingle, Suppr oublie. Les épinglées sont gardées en plus.",
);
pub const EMOJI: Words = Words::new("Emoji", "Emoji");
pub const EMOJI_NOTE: Words = Words::new(
    "Searched in English and French, recent ones first; Tab changes the skin tone, Enter types it into the focused app.",
    "Cherchés en anglais et en français, les récents d’abord ; Tab change la couleur de peau, Entrée l’écrit dans l’appli active.",
);

/// "100 copies kept".
pub fn copies(n: u32, lang: Lang) -> String {
    match lang {
        Lang::En => format!("{n} copies kept"),
        Lang::Fr => format!("{n} copies gardées"),
    }
}

// Home tiles of the Work pages.
pub const CLASSIC_SCROLL: Words = Words::new("Classic scrolling", "Défilement classique");
pub const SAVED_AND_COPIED: Words = Words::new("Saved and copied", "Enregistrées et copiées");
pub const SAVED: Words = Words::new("Saved", "Enregistrées");
pub const COPIED: Words = Words::new("Copied", "Copiées");
pub const NEITHER: Words = Words::new("Neither saved nor copied", "Ni enregistrées ni copiées");
pub const DESKTOP_COUNT: Words = Words::new("Desktops per screen", "Bureaux par écran");

// The Hardware pages.
pub const UNKNOWN: Words = Words::new("Unknown", "Inconnu");
pub const NO_SERVICE: Words = Words::new(
    "Not heard from the system’s services yet: what they say shows here once they answer.",
    "Les services du système n’ont pas encore répondu : ce qu’ils disent s’affiche ici dès qu’ils répondent.",
);

// Sound.
pub const OUTPUT: Words = Words::new("Output", "Sortie");
pub const INPUT: Words = Words::new("Input", "Entrée");
pub const VOLUME: Words = Words::new("Volume", "Volume");
pub const MUTE: Words = Words::new("Mute", "Couper le son");
pub const MUTE_SUB: Words = Words::new(
    "Nothing plays until it is on again",
    "Plus rien ne joue jusqu’à ce qu’il revienne",
);
pub const MUTED: Words = Words::new("Muted", "Coupé");
pub const OUTPUT_DEVICE: Words = Words::new("Output device", "Appareil de sortie");
pub const INPUT_DEVICE: Words = Words::new("Microphone", "Micro");
pub const NO_DEVICE: Words = Words::new("None known", "Aucun connu");
pub const SOUND_NOTE: Words = Words::new(
    "Through PipeWire: quick settings and the column show the same volume. The device is the system’s default; choosing another comes later.",
    "Par PipeWire : les réglages rapides et la colonne montrent le même volume. L’appareil est celui du système par défaut ; en choisir un autre viendra plus tard.",
);

// Network.
pub const WIRELESS: Words = Words::new("Wireless", "Sans fil");
pub const AIRPLANE: Words = Words::new("Airplane mode", "Mode avion");
pub const AIRPLANE_SUB: Words = Words::new("Wi-Fi and Bluetooth off", "Wi-Fi et Bluetooth coupés");
pub const WIFI_SUB_OFF: Words = Words::new("The radio is off", "La radio est coupée");
pub const NOT_CONNECTED: Words = Words::new("Not connected", "Non connecté");
pub const NETWORK_NOTE: Words = Words::new(
    "Through NetworkManager, which keeps the passwords. The networks are looked for again every 10 seconds while this page shows.",
    "Par NetworkManager, qui garde les mots de passe. Les réseaux sont recherchés de nouveau toutes les 10 secondes tant que la page est affichée.",
);
pub const NETWORKS: Words = Words::new("Networks in reach", "Réseaux à portée");
pub const LOOKING_NETWORKS: Words = Words::new("Looking for networks…", "Recherche des réseaux…");
pub const WIFI_OFF_LIST: Words = Words::new(
    "Turn Wi-Fi on to see the networks in reach.",
    "Activez le Wi-Fi pour voir les réseaux à portée.",
);
pub const NO_WIFI_DEVICE: Words = Words::new(
    "No Wi-Fi device on this machine.",
    "Pas d’appareil Wi-Fi sur cette machine.",
);
pub const NO_NETWORKMANAGER: Words = Words::new(
    "The networks are listed through NetworkManager, which is not here: join them with the system’s own tools.",
    "Les réseaux sont listés par NetworkManager, absent ici : rejoignez-les avec les outils du système.",
);
pub const NO_NETWORK: Words = Words::new("No network in reach.", "Aucun réseau à portée.");
pub const JOIN: Words = Words::new("Join", "Rejoindre");
pub const DISCONNECT: Words = Words::new("Disconnect", "Déconnecter");
pub const FORGET: Words = Words::new("Forget", "Oublier");
pub const CONNECTED: Words = Words::new("Connected", "Connecté");
pub const SAVED_NETWORK: Words = Words::new("Saved", "Enregistré");
pub const OPEN_NETWORK: Words = Words::new("Open", "Ouvert");
pub const SECURED: Words = Words::new("Secured", "Sécurisé");
pub const SECURED_WPA3: Words = Words::new("Secured, WPA3", "Sécurisé, WPA3");
pub const ACCOUNT_NETWORK: Words = Words::new(
    "Joined with the system’s own tools",
    "À rejoindre avec les outils du système",
);
pub const JOINING: Words = Words::new("Joining…", "Connexion…");
pub const WIFI_PASSWORD_NOTE: Words = Words::new(
    "The password goes to NetworkManager, which keeps it; never into Hocuspanda’s settings. Enter joins, Escape cancels.",
    "Le mot de passe va à NetworkManager, qui le garde ; jamais dans les réglages de Hocuspanda. Entrée rejoint, Échap annule.",
);

/// A button's name with what it acts on: "Join Panda den".
pub fn acting(verb: Words, on: &str, lang: Lang) -> String {
    format!("{} {on}", verb.get(lang))
}

/// The password field of a network.
pub fn password_for(network: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Password for {network}"),
        Lang::Fr => format!("Mot de passe de {network}"),
    }
}

/// Why a network was not joined.
pub fn not_joined(why: Option<&str>, lang: Lang) -> String {
    match (why, lang) {
        (None, Lang::En) => "Not joined: check the password".into(),
        (None, Lang::Fr) => "Pas connecté\u{a0}: vérifiez le mot de passe".into(),
        (Some(why), Lang::En) => format!("Not joined: {why}"),
        (Some(why), Lang::Fr) => format!("Pas connecté\u{a0}: {why}"),
    }
}

/// "Joined: Bamboo grove".
pub fn joined(network: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Joined: {network}"),
        Lang::Fr => format!("Connecté : {network}"),
    }
}

// Bluetooth.
pub const BLUETOOTH: Words = Words::new("Bluetooth", "Bluetooth");
/// Its switch, told apart from the page's name beside it.
pub const BLUETOOTH_RADIO: Words = Words::new("Bluetooth radio", "Radio Bluetooth");
pub const BLUETOOTH_SUB_OFF: Words = Words::new("Nothing connects", "Rien ne se connecte");
pub const NO_BLUETOOTH_DEVICE: Words = Words::new("No device connected", "Aucun appareil connecté");
pub const BLUETOOTH_NOTE: Words = Words::new(
    "Through BlueZ. A device that asks for a code typed on this machine is not paired here yet: pair it with bluetoothctl.",
    "Par BlueZ. Un appareil qui demande un code tapé sur cette machine ne s’appaire pas encore ici : appairez-le avec bluetoothctl.",
);
pub const MY_DEVICES: Words = Words::new("My devices", "Mes appareils");
pub const DEVICES_IN_REACH: Words = Words::new("Devices in reach", "Appareils à portée");
pub const LOOKING_DEVICES: Words = Words::new(
    "Looked for while this page shows: put a device in pairing mode to see it here.",
    "Recherchés tant que la page est affichée : mettez un appareil en mode appairage pour le voir ici.",
);
pub const NONE_IN_REACH: Words = Words::new("None found yet.", "Aucun trouvé pour l’instant.");
pub const NO_PAIRED: Words = Words::new(
    "No device paired yet.",
    "Aucun appareil appairé pour l’instant.",
);
pub const BLUETOOTH_OFF_LIST: Words = Words::new(
    "Turn Bluetooth on to see the devices.",
    "Activez le Bluetooth pour voir les appareils.",
);
pub const NO_BLUEZ: Words = Words::new(
    "The devices are listed through BlueZ, which is not here: pair them with the system’s own tools.",
    "Les appareils sont listés par BlueZ, absent ici : appairez-les avec les outils du système.",
);
pub const NO_ADAPTER: Words = Words::new(
    "No Bluetooth adapter on this machine.",
    "Pas d’adaptateur Bluetooth sur cette machine.",
);
pub const LOOKING_BLUETOOTH: Words = Words::new("Asking BlueZ…", "BlueZ est interrogé…");
pub const PAIR: Words = Words::new("Pair", "Appairer");
pub const CONNECT: Words = Words::new("Connect", "Connecter");
pub const PAIRING: Words = Words::new("Pairing…", "Appairage…");
pub const WORKING: Words = Words::new("Asked…", "Demandé…");
pub const SAME_CODE: Words = Words::new("Same code: pair", "Même code : appairer");

/// What a device is, from what BlueZ says.
pub fn device_kind(kind: hocus_platform::bluetooth::Kind) -> Words {
    use hocus_platform::bluetooth::Kind;
    match kind {
        Kind::Headphones => Words::new("Headphones", "Casque"),
        Kind::Speaker => Words::new("Speaker", "Enceinte"),
        Kind::Keyboard => Words::new("Keyboard", "Clavier"),
        Kind::Mouse => Words::new("Mouse", "Souris"),
        Kind::Gamepad => Words::new("Game controller", "Manette"),
        Kind::Phone => Words::new("Phone", "Téléphone"),
        Kind::Computer => Words::new("Computer", "Ordinateur"),
        Kind::Other => Words::new("Device", "Appareil"),
    }
}

/// "Battery 80%".
pub fn device_battery(level: u8, lang: Lang) -> String {
    format!("{} {}", BATTERY.get(lang), percent(u32::from(level), lang))
}

/// The passkey a device shows, to compare.
pub fn compare_code(device: &str, code: u32, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Does {device} show {code:06}? Pair only if it is the same."),
        Lang::Fr => {
            format!("{device} affiche-t-il {code:06}\u{a0}? N’appairez que si c’est le même.")
        }
    }
}

/// The code typed on the device.
pub fn type_code(device: &str, code: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Type {code} on {device}, then Enter."),
        Lang::Fr => format!("Tapez {code} sur {device}, puis Entrée."),
    }
}

/// Why what was asked of a device did not happen.
pub fn device_failed(why: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Did not work: {why}"),
        Lang::Fr => format!("Échec\u{a0}: {why}"),
    }
}

/// "Connected: Headphones".
pub fn connected(device: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Connected: {device}"),
        Lang::Fr => format!("Connecté : {device}"),
    }
}

// Power.
pub const BATTERY: Words = Words::new("Battery", "Batterie");
pub const NO_BATTERY: Words = Words::new(
    "No battery: the charger’s delays always apply.",
    "Pas de batterie : les délais sur secteur s’appliquent toujours.",
);
pub const POWER_PROFILE: Words = Words::new("Power profile", "Profil d’énergie");
pub const POWER_PROFILE_NOTE: Words = Words::new(
    "On the power saver, moving wallpapers stand still and updates wait.",
    "En économie, les fonds animés s’arrêtent et les mises à jour attendent.",
);
pub const DELAYS: Words = Words::new("Idle delays", "Délais d’inactivité");
pub const ON_CHARGER: Words = Words::new("On the charger", "Sur secteur");
pub const ON_BATTERY: Words = Words::new("On battery", "Sur batterie");
pub const DIM_AFTER: Words = Words::new("Dim the screens after", "Assombrir les écrans après");
pub const SLEEP_AFTER: Words = Words::new("Sleep after", "Mettre en veille après");
pub const DELAYS_NOTE: Words = Words::new(
    "Without input for that long. Plugging or unplugging the charger counts as being there: the other delays start from then.",
    "Sans rien toucher pendant ce temps. Brancher ou débrancher le chargeur compte comme une présence : les autres délais partent de là.",
);
pub const SLEEP_HOW: Words = Words::new("How it sleeps", "Mise en veille");
pub const SUSPEND: Words = Words::new("Suspend", "Veille");
pub const HIBERNATE: Words = Words::new("Hibernate", "Hibernation");
pub const SLEEP_NOTE: Words = Words::new(
    "The session locks first. Hibernating keeps nothing powered, and needs swap set up for it.",
    "La session se verrouille d’abord. L’hibernation n’alimente plus rien, et demande un swap prévu pour.",
);
pub const CANNOT_HIBERNATE: Words = Words::new(
    "This machine cannot hibernate: it suspends instead.",
    "Cette machine ne peut pas hiberner : elle se met en veille à la place.",
);
pub const KEEP_AWAKE: Words = Words::new("Kept awake", "Maintenu éveillé");
pub const FULLSCREEN_AWAKE: Words =
    Words::new("Fullscreen keeps awake", "Le plein écran maintient éveillé");
pub const FULLSCREEN_AWAKE_SUB: Words = Words::new(
    "A film, a game or slides hold the curtain and sleep off",
    "Un film, un jeu ou des diapos retiennent le rideau et la veille",
);
pub const KEEP_AWAKE_NOTE: Words = Words::new(
    "Applications asking (a video call, a player) always keep the session awake; the countdown starts again once they stop.",
    "Les applications qui le demandent (un appel vidéo, un lecteur) maintiennent toujours la session éveillée ; le décompte reprend dès qu’elles s’arrêtent.",
);
pub const NEVER: Words = Words::new("Never", "Jamais");

/// A power profile's name, by its power-profiles-daemon name.
pub fn profile(name: &str) -> Words {
    match name {
        "power-saver" => Words::new("Saver", "Économie"),
        "performance" => Words::new("Performance", "Performance"),
        _ => Words::new("Balanced", "Équilibré"),
    }
}

pub fn sleep_name(sleep: hocus_core::config::Sleep) -> Words {
    match sleep {
        hocus_core::config::Sleep::Suspend => SUSPEND,
        hocus_core::config::Sleep::Hibernate => HIBERNATE,
    }
}

/// "30 s", "5 min", "5 min 30 s", "1 h", "1 h 30"; 0 is never.
pub fn duration(seconds: u32, lang: Lang) -> String {
    let nb = match lang {
        Lang::En => " ",
        Lang::Fr => "\u{a0}",
    };
    let (h, m, s) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    match (h, m, s) {
        (0, 0, 0) => NEVER.get(lang).to_owned(),
        (0, 0, s) => format!("{s}{nb}s"),
        (0, m, 0) => format!("{m}{nb}min"),
        (0, m, s) => format!("{m}{nb}min {s}{nb}s"),
        (h, 0, _) => format!("{h}{nb}h"),
        (h, m, _) => format!("{h}{nb}h {m:02}"),
    }
}

/// "On battery: Dim the screens after: 5 min", told apart from the
/// charger's.
pub fn on_source(source: Words, name: Words, value: &str, lang: Lang) -> String {
    format!(
        "{}{} {}",
        source.get(lang),
        if lang == Lang::Fr { " :" } else { ":" },
        named_value(name, value, lang)
    )
}

/// "80% · charging", "64% · about 3 h 50 left".
pub fn battery_state(battery: &hocus_api::BatteryInfo, lang: Lang) -> String {
    let percent = percent(u32::from(battery.percent), lang);
    let state = match (battery.state.as_str(), battery.minutes_left, lang) {
        ("discharging", Some(m), Lang::En) => format!("about {} h {:02} left", m / 60, m % 60),
        ("discharging", Some(m), Lang::Fr) => {
            format!("environ {}\u{a0}h {:02} restantes", m / 60, m % 60)
        }
        ("discharging", None, _) => ON_BATTERY.get(lang).to_lowercase(),
        ("charging", _, Lang::En) => "charging".into(),
        ("charging", _, Lang::Fr) => "en charge".into(),
        ("full", _, Lang::En) => "charged".into(),
        ("full", _, Lang::Fr) => "chargée".into(),
        (_, _, _) => ON_CHARGER.get(lang).to_lowercase(),
    };
    format!("{percent} · {state}")
}

// Lock.
pub const LOCKING: Words = Words::new("Locking", "Verrouillage");
pub const LOCK_ON_IDLE: Words = Words::new("Lock after the curtain", "Verrouiller après le rideau");
pub const LOCK_ON_IDLE_SUB: Words = Words::new(
    "Otherwise the curtain only dims",
    "Sinon le rideau ne fait qu’assombrir",
);
pub const CURTAIN_FOR: Words = Words::new("Curtain before locking", "Rideau avant de verrouiller");
pub const LOCK_NOTE: Words = Words::new(
    "The curtain falls after the Power page’s delay, with a countdown; any input lifts it.",
    "Le rideau tombe après le délai de la page Énergie, avec un décompte ; toucher à quoi que ce soit le lève.",
);
pub const ALWAYS_LOCKED: Words = Words::new(
    "The session always locks before the machine sleeps, and a crash never unlocks it.",
    "La session se verrouille toujours avant la mise en veille, et un plantage ne la déverrouille jamais.",
);
pub const LOCK_SCREEN: Words = Words::new("Lock screen", "Écran de verrouillage");
pub const LOCK_SCREEN_NOTE: Words = Words::new(
    "The time, the account’s name and a warning when Caps Lock is on; the pandas sleep in its corner while they are shown.",
    "L’heure, le nom du compte et un avertissement quand la touche Verr. Maj est active ; les pandas dorment dans son coin quand ils sont affichés.",
);
pub const NOT_ON_IDLE: Words = Words::new("Not when idle", "Pas à l’inactivité");

/// "Locks after 5 min 30 s".
pub fn locks_after(seconds: u32, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Locks after {}", duration(seconds, lang)),
        Lang::Fr => format!("Verrouille après {}", duration(seconds, lang)),
    }
}

// The System pages.

// Notifications.
pub const DND: Words = Words::new("Do not disturb", "Ne pas déranger");
pub const DND_SUB: Words = Words::new(
    "Notifications wait in the column; critical ones still show",
    "Les notifications attendent dans la colonne ; les critiques s’affichent quand même",
);
pub const DND_NOTE: Words = Words::new(
    "The same as quick settings’ tile, until the session ends.",
    "Le même que la tuile des réglages rapides, jusqu’à la fin de la session.",
);
pub const SHARING: Words = Words::new("While the screen is shared", "Pendant un partage d’écran");
pub const HOLD_WHILE_SHARING: Words =
    Words::new("Hold while sharing", "Retenir pendant le partage");
pub const HOLD_WHILE_SHARING_SUB: Words = Words::new(
    "Nothing private shows; one notification sums them up after",
    "Rien de privé ne s’affiche ; une seule notification les résume ensuite",
);
pub const HISTORY: Words = Words::new("History", "Historique");
pub const HISTORY_KEEPS: Words = Words::new("The column keeps", "La colonne garde");
pub const HISTORY_NOTE: Words = Words::new(
    "The latest first; older ones go. Clear empties it from the column.",
    "Les plus récentes d’abord ; les plus anciennes s’en vont. Effacer la vide depuis la colonne.",
);
pub const NONE: Words = Words::new("None", "Aucune");
pub const APPLICATIONS: Words = Words::new("Applications", "Applications");
pub const SHOW_TOASTS: Words = Words::new("Banners", "Bannières");
pub const COLUMN_ONLY: Words = Words::new("Column only", "Colonne seule");
pub const NO_APP_YET: Words = Words::new(
    "No application has sent a notification yet this session: each one shows here once it has.",
    "Aucune application n’a encore envoyé de notification dans cette session : chacune s’affiche ici dès qu’elle l’a fait.",
);
pub const APPS_NOTE: Words = Words::new(
    "By the name each application gives. Critical notifications always show.",
    "Selon le nom que donne chaque application. Les notifications critiques s’affichent toujours.",
);
pub const NOTIFICATIONS_SOUND_NOTE: Words = Words::new(
    "Notifications make no sound here: there is nothing to silence.",
    "Les notifications ne font aucun son ici : il n’y a rien à couper.",
);

/// What becomes of an application's notifications, in words.
pub fn app_mode(mode: hocus_core::config::AppNotifications) -> Words {
    use hocus_core::config::AppNotifications as M;
    match mode {
        M::Show => SHOW_TOASTS,
        M::History => COLUMN_ONLY,
        M::Off => OFF,
    }
}

/// "3 in the column".
pub fn in_the_column(n: usize, lang: Lang) -> String {
    match (lang, n) {
        (Lang::En, 0) => "Nothing in the column".into(),
        (Lang::En, n) => format!("{n} in the column"),
        (Lang::Fr, 0) => "Rien dans la colonne".into(),
        (Lang::Fr, n) => format!("{n} dans la colonne"),
    }
}

// Applications.
pub const DEFAULT_APPS: Words = Words::new("Default applications", "Applications par défaut");
pub const DEFAULT_APPS_NOTE: Words = Words::new(
    "Written in mimeapps.list, which every application that opens files or links reads.",
    "Écrites dans mimeapps.list, que lisent toutes les applications qui ouvrent des fichiers ou des liens.",
);
pub const NONE_SET: Words = Words::new("None set", "Aucune choisie");
pub const NO_APP_FOR: Words = Words::new(
    "No application installed opens these",
    "Aucune application installée ne les ouvre",
);
pub const NOT_READ_YET: Words =
    Words::new("Reading the applications…", "Lecture des applications…");
pub const AT_START: Words = Words::new("At the start of the session", "Au début de la session");
pub const AUTOSTART: Words = Words::new("Start applications", "Démarrer les applications");
pub const AUTOSTART_SUB: Words = Words::new(
    "Those of the autostart folders, below",
    "Celles des dossiers de démarrage, ci-dessous",
);
pub const STARTS: Words = Words::new("Starts with the session", "Démarre avec la session");
pub const DOES_NOT_START: Words = Words::new("Does not start", "Ne démarre pas");
pub const NO_AUTOSTART: Words = Words::new(
    "No application asks to start with the session.",
    "Aucune application ne demande à démarrer avec la session.",
);
pub const AUTOSTART_NOTE: Words = Words::new(
    "Turned off in your own copy of the entry (~/.config/autostart), as every desktop reads it.",
    "Désactivée dans votre propre copie de l’entrée (~/.config/autostart), que lisent tous les bureaux.",
);
pub const RESTORE: Words = Words::new("Last session’s windows", "Fenêtres de la dernière session");
pub const ASK: Words = Words::new("Ask", "Demander");
pub const ALWAYS: Words = Words::new("Always", "Toujours");

/// A kind of file, by its name in `hocus_platform::mime::KINDS`.
pub fn app_kind(kind: &str) -> Words {
    match kind {
        "web" => Words::new("Web browser", "Navigateur web"),
        "mail" => Words::new("Mail", "Courriel"),
        "files" => Words::new("Files", "Fichiers"),
        "text" => Words::new("Text", "Texte"),
        "images" => Words::new("Images", "Images"),
        "video" => Words::new("Videos", "Vidéos"),
        "music" => Words::new("Music", "Musique"),
        "pdf" => Words::new("PDF documents", "Documents PDF"),
        _ => Words::new("Other", "Autre"),
    }
}

/// "2 at start".
pub fn at_start(n: usize, lang: Lang) -> String {
    match lang {
        Lang::En => format!("{n} at start"),
        Lang::Fr => format!("{n} au démarrage"),
    }
}

// Account.
pub const FULL_NAME: Words = Words::new("Full name", "Nom complet");
pub const AVATAR: Words = Words::new("Picture", "Photo");
pub const AVATAR_HINT: Words = Words::new(
    "/home/…/picture.png, then Enter",
    "/home/…/photo.png, puis Entrée",
);
pub const ACCOUNT_NOTE: Words = Words::new(
    "Through AccountsService: Enter writes the name or the picture (a PNG file, which it copies).",
    "Par AccountsService : Entrée enregistre le nom ou la photo (un fichier PNG, qu’il copie).",
);
pub const NO_ACCOUNTS_SERVICE: Words = Words::new(
    "AccountsService is not there: the name and the picture are changed with the system’s own tools.",
    "AccountsService est absent : le nom et la photo se changent avec les outils du système.",
);
pub const PASSWORD: Words = Words::new("Password", "Mot de passe");
pub const PASSWORD_NOTE: Words = Words::new(
    "Changed with passwd in a terminal for now: the system asks for the current one first.",
    "Se change avec passwd dans un terminal pour l’instant : le système demande d’abord l’actuel.",
);
pub const FINGERPRINT: Words = Words::new("Fingerprint", "Empreinte");
pub const NO_FPRINTD: Words = Words::new(
    "fprintd is not there: no fingerprint is read.",
    "fprintd est absent : aucune empreinte n’est lue.",
);
pub const NO_READER: Words = Words::new("No fingerprint reader", "Aucun lecteur d’empreintes");
pub const FINGERPRINT_NOTE: Words = Words::new(
    "Fingers are enrolled with fprintd-enroll for now; enrolling here comes later.",
    "On enregistre un doigt avec fprintd-enroll pour l’instant ; l’enregistrement ici viendra plus tard.",
);
pub const LOOK_FOR_UPDATES: Words = Words::new("Look for updates", "Chercher les mises à jour");
pub const LOOK_FOR_UPDATES_SUB: Words = Words::new(
    "The column says when some are ready",
    "La colonne prévient quand il y en a",
);
pub const LOOK_EVERY: Words = Words::new("Look every", "Chercher toutes les");
pub const DAY: Words = Words::new("1 day", "1 jour");
pub const WEEK: Words = Words::new("1 week", "1 semaine");

/// "1 finger enrolled".
pub fn fingers(n: usize, lang: Lang) -> String {
    match (lang, n) {
        (Lang::En, 0) => "No finger enrolled".into(),
        (Lang::En, 1) => "1 finger enrolled".into(),
        (Lang::En, n) => format!("{n} fingers enrolled"),
        (Lang::Fr, 0) => "Aucun doigt enregistré".into(),
        (Lang::Fr, 1) => "1 doigt enregistré".into(),
        (Lang::Fr, n) => format!("{n} doigts enregistrés"),
    }
}

/// How often updates are looked for: "6 h", "1 day".
pub fn every(hours: u32, lang: Lang) -> String {
    match hours {
        24 => DAY.get(lang).to_owned(),
        168 => WEEK.get(lang).to_owned(),
        h => duration(h * 3600, lang),
    }
}

/// "2 calendars".
pub fn calendars(n: usize, lang: Lang) -> String {
    match (lang, n) {
        (Lang::En, 0) => "No calendar".into(),
        (Lang::En, 1) => "1 calendar".into(),
        (Lang::En, n) => format!("{n} calendars"),
        (Lang::Fr, 0) => "Aucun agenda".into(),
        (Lang::Fr, 1) => "1 agenda".into(),
        (Lang::Fr, n) => format!("{n} agendas"),
    }
}

// AI and agents.
pub const AI_CAPTION: Words = Words::new("AI", "IA");
pub const ASK_AI: Words = Words::new("Ask the AI", "Interroger l’IA");
pub const ASK_AI_SUB: Words = Words::new(
    "Questions typed in the command field, and circled areas",
    "Les questions du champ de commande, et les zones entourées",
);
pub const AI_OFF: Words = Words::new("AI off", "IA désactivée");
pub const PROVIDER: Words = Words::new("Provider", "Fournisseur");
pub const OPENAI_LIKE: Words = Words::new("OpenAI-compatible", "Compatible OpenAI");
pub const ADDRESS: Words = Words::new("Address", "Adresse");
pub const ADDRESS_OF_SERVER: Words = Words::new(
    "https://…, or http://127.0.0.1:…, then Enter",
    "https://…, ou http://127.0.0.1:…, puis Entrée",
);
pub const ADDRESS_NOTE: Words = Words::new(
    "https, or http to this machine only: questions never travel in the clear. Enter writes it.",
    "https, ou http vers cette machine seulement : les questions ne circulent jamais en clair. Entrée l’enregistre.",
);
pub const BAD_ADDRESS: Words = Words::new(
    "Not written: an https:// address, or http:// to this machine.",
    "Non enregistrée : une adresse https://, ou http:// vers cette machine.",
);
pub const MODEL: Words = Words::new("Model", "Modèle");
// The fields' names, apart from the names shown above them.
pub const AI_ADDRESS: Words = Words::new("AI address", "Adresse de l’IA");
pub const AI_MODEL: Words = Words::new("AI model", "Modèle de l’IA");
pub const AI_KEY: Words = Words::new("Provider key", "Clé du fournisseur");
pub const MODEL_HINT: Words = Words::new(
    "Empty: the provider’s newest",
    "Vide : le plus récent du fournisseur",
);
pub const MODEL_NOTE: Words = Words::new(
    "Left empty, the provider is asked at the first question, and its newest model kept for the session.",
    "Laissé vide, le fournisseur est interrogé à la première question, et son modèle le plus récent gardé pour la session.",
);
pub const KEY: Words = Words::new("Key", "Clé");
pub const KEY_KEPT: Words = Words::new("A key is kept", "Une clé est gardée");
pub const NO_KEY: Words = Words::new("No key", "Aucune clé");
pub const KEY_NOT_READ: Words = Words::new("Reading the keyring…", "Lecture du trousseau…");
pub const KEY_HINT: Words = Words::new("Paste the key, then Enter", "Collez la clé, puis Entrée");
pub const KEY_HINT_REPLACE: Words = Words::new(
    "Another key replaces it, then Enter",
    "Une autre clé la remplace, puis Entrée",
);
pub const KEEP: Words = Words::new("Keep", "Garder");
pub const KEEP_KEY: Words = Words::new("Keep the key", "Garder la clé");
pub const REMOVE_KEY: Words = Words::new("Remove the key", "Retirer la clé");
pub const KEY_NOTE: Words = Words::new(
    "In the keyring, never in the configuration file; never shown again.",
    "Dans le trousseau, jamais dans le fichier de configuration ; plus jamais affichée.",
);
pub const KEY_IF_ASKED: Words = Words::new(
    "Only if the server asks for one. In the keyring, never in the configuration file; never shown again.",
    "Seulement si le serveur en demande une. Dans le trousseau, jamais dans le fichier de configuration ; plus jamais affichée.",
);
pub const NO_KEY_NEEDED: Words =
    Words::new("ollama needs no key.", "ollama n’a pas besoin de clé.");
pub const HERDR_NOTE: Words = Words::new(
    "Each herdr’s agents show in full on its desktop and in a line elsewhere; the desktop’s badge says when one waits.",
    "Les agents de chaque herdr s’affichent en entier sur son bureau, en une ligne ailleurs ; le badge du bureau dit quand l’un attend.",
);
pub const NO_HERDR: Words = Words::new(
    "No herdr named: the session looks for herdr’s own socket, ~/.config/herdr/herdr.sock.",
    "Aucun herdr nommé : la session cherche le socket de herdr lui-même, ~/.config/herdr/herdr.sock.",
);
pub const ANY_DESKTOP: Words = Words::new("Wherever you are", "Là où vous êtes");
pub const NO_DESKTOP: Words = Words::new("None", "Aucun");
pub const AGENTS_IN_COLUMN: Words = Words::new("In the column", "Dans la colonne");
pub const DESKTOP: Words = Words::new("Desktop", "Bureau");
pub const OLD_SOCKET: Words = Words::new(
    "herdr_socket: wherever you are",
    "herdr_socket : là où vous êtes",
);
pub const NEW_HERDR: Words = Words::new("New herdr socket", "Nouveau socket herdr");
pub const NEW_HERDR_HINT: Words = Words::new(
    "~/.config/herdr/project.sock, then Enter",
    "~/.config/herdr/projet.sock, puis Entrée",
);
pub const ADD_HERDR: Words = Words::new("Add the herdr socket", "Ajouter le socket herdr");
pub const MCP_AGENTS: Words =
    Words::new("Agents using the desktop", "Agents qui utilisent le bureau");
pub const JOURNAL_TEXT: Words =
    Words::new("Journal keeps typed text", "Le journal garde le texte tapé");
pub const JOURNAL_TEXT_SUB: Words = Words::new(
    "Otherwise only how many characters: it may be a secret",
    "Sinon seulement le nombre de caractères : ce peut être un secret",
);
pub const SHOW_BEFORE_CLICK: Words = Words::new("Shows before a click", "Montre avant un clic");
pub const AT_ONCE: Words = Words::new("At once", "Aussitôt");
pub const SHOW_NOTE: Words = Words::new(
    "Its ring and what it means to do, at the point, before the click lands.",
    "Son cercle et ce qu’il compte faire, au point visé, avant que le clic ne parte.",
);
pub const CONFIRM_TIMEOUT: Words = Words::new("Waits for your answer", "Attend votre réponse");
pub const CONFIRM_APPS: Words = Words::new("Asks you first in", "Vous demande d’abord dans");
pub const CONFIRM_NOTE: Words = Words::new(
    "There an agent’s click, typing or keys wait on a card you answer, as typing into a password field does; scrolling does not. Unanswered in time, it is refused.",
    "Là, le clic, la saisie ou les touches d’un agent attendent sur une carte à laquelle vous répondez, comme une saisie dans un champ de mot de passe ; le défilement non. Sans réponse à temps, c’est refusé.",
);
pub const NO_CONFIRM_APPS: Words = Words::new(
    "No application: only password fields ask you first.",
    "Aucune application : seuls les champs de mot de passe vous demandent d’abord.",
);
pub const NEW_CONFIRM_APP: Words = Words::new(
    "New application asking first",
    "Nouvelle application qui demande d’abord",
);
pub const NEW_CONFIRM_APP_HINT: Words = Words::new(
    "An app id, or a pattern: my.bank.*, then Enter",
    "Un app id, ou un motif : ma.banque.*, puis Entrée",
);
pub const ADD_CONFIRM_APP: Words = Words::new(
    "Add the application asking first",
    "Ajouter l’application qui demande d’abord",
);
pub const AGENTS_ALLOWED: Words = Words::new(
    "They act as [[agents.mcp]] allows them",
    "Ils agissent comme [[agents.mcp]] le leur permet",
);
pub const AGENTS_STOPPED: Words = Words::new(
    "Stopped: their requests are refused",
    "Arrêtés : leurs demandes sont refusées",
);
pub const STOP_KEY: Words = Words::new("Stop key", "Touche d’arrêt");
pub const CHANGE_STOP_KEY: Words = Words::new("Change the stop key", "Changer la touche d’arrêt");
pub const STOP_ALL: Words = Words::new("Stop all agents", "Arrêter tous les agents");
pub const ALLOW_AGENTS: Words = Words::new("Allow agents", "Autoriser les agents");
pub const STOP_NOTE: Words = Words::new(
    "The same as the key and the column’s button, until the session ends. herdr’s agents are not stopped.",
    "Comme la touche et le bouton de la colonne, jusqu’à la fin de la session. Les agents de herdr ne sont pas arrêtés.",
);
pub const AGENTS_STOPPED_SHORT: Words = Words::new("Agents stopped", "Agents arrêtés");
pub const CIRCLE: Words = Words::new("Circle to ask", "Entourer pour demander");
pub const KEYS_HELD: Words = Words::new("Keys held", "Touches tenues");
pub const CIRCLE_NOTE: Words = Words::new(
    "Its text is read, then copied, searched or noted; explained, translated or asked about while the AI is on. Super alone draws on the screen.",
    "Son texte est lu, puis copié, cherché ou noté ; expliqué, traduit ou interrogé quand l’IA est activée. Super seule dessine sur l’écran.",
);

/// "Hold Super+Shift and circle with the left button".
pub fn circle_how(keys: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Hold {keys} and draw a loop with the left button"),
        Lang::Fr => format!("Tenir {keys} et tracer une boucle avec le bouton gauche"),
    }
}

/// "Empty: http://127.0.0.1:11434".
pub fn empty_is(address: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Empty: {address}"),
        Lang::Fr => format!("Vide : {address}"),
    }
}

/// "Desktop 3", "Desktop 3 · DP-1".
pub fn on_desktop(desktop: u32, output: Option<&str>, lang: Lang) -> String {
    let desktop = format!("{} {desktop}", DESKTOP.get(lang));
    match output {
        Some(output) => format!("{desktop} · {output}"),
        None => desktop,
    }
}

/// The name of one herdr's choices: "Desktop of web.sock".
pub fn of_herdr(what: Words, socket: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("{} of {socket}", what.en),
        Lang::Fr => format!("{} de {socket}", what.fr),
    }
}

/// "Remove web.sock".
pub fn remove_herdr(socket: &str, lang: Lang) -> String {
    remove_named(socket, lang)
}

/// "Remove my.bank.*" (an application asking first).
pub fn remove_app(app: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Remove {app}"),
        Lang::Fr => format!("Retirer {app}"),
    }
}

/// How long an agent's click shows first: "At once", "0.6 s", "1 s".
pub fn shown_for(ms: u32, lang: Lang) -> String {
    if ms == 0 {
        return AT_ONCE.get(lang).to_owned();
    }
    let s = if ms.is_multiple_of(1000) {
        (ms / 1000).to_string()
    } else {
        let s = format!("{}", f64::from(ms) / 1000.0);
        match lang {
            Lang::En => s,
            Lang::Fr => s.replace('.', ","),
        }
    };
    match lang {
        Lang::En => format!("{s} s"),
        Lang::Fr => format!("{s}\u{a0}s"),
    }
}

/// "2 herdr".
pub fn herdr_count(n: usize) -> String {
    format!("{n} herdr")
}

// Search.
pub const DOCUMENTS: Words = Words::new("Documents", "Documents");
pub const SEARCH_DOCUMENTS: Words = Words::new("Search documents", "Chercher les documents");
pub const SEARCH_DOCUMENTS_SUB: Words = Words::new(
    "Found from the command field, under “Documents”",
    "Trouvés depuis le champ de commande, sous « Documents »",
);
pub const DOCUMENTS_OFF: Words = Words::new("Documents off", "Documents désactivés");
pub const SECRETS_NEVER_READ: Words = Words::new(
    "Documents · secrets never read",
    "Documents · secrets jamais lus",
);
pub const ON_BATTERY_SHORT: Words = Words::new("On battery", "Sur batterie");
pub const INDEX_OFF: Words = Words::new(
    "Off: nothing is read, and no index is kept",
    "Désactivée : rien n’est lu, aucun index n’est gardé",
);
pub const INDEX_WAITING: Words = Words::new("Waiting to look", "En attente");
pub const INDEX_PAUSED: Words = Words::new(
    "Paused on battery: it goes on once on the charger",
    "En pause sur batterie : reprend une fois sur le chargeur",
);
pub const INDEX_LOOKING: Words = Words::new("Looking at the folders…", "Parcours des dossiers…");
pub const INDEX_READY: Words = Words::new("Up to date", "À jour");
pub const NOT_LOOKED_YET: Words = Words::new("Not looked at yet", "Pas encore parcourus");
pub const SEARCH_AGAIN: Words = Words::new("Search again now", "Parcourir maintenant");
pub const DOCUMENTS_NOTE: Words = Words::new(
    "Text, Markdown, HTML, PDF and office files. Read at the lowest priority, paused on battery, looked at again every half hour: only what changed is read.",
    "Texte, Markdown, HTML, PDF et fichiers bureautiques. Lus à la priorité la plus basse, en pause sur batterie, parcourus de nouveau toutes les demi-heures : seul ce qui a changé est lu.",
);
pub const FOLDERS_SEARCHED: Words = Words::new("Folders searched", "Dossiers parcourus");
pub const USUAL_FOLDERS: Words = Words::new(
    "None named: the Documents, Desktop and Downloads folders.",
    "Aucun nommé : les dossiers Documents, Bureau et Téléchargements.",
);
pub const NEW_FOLDER: Words = Words::new("New folder to search", "Nouveau dossier à parcourir");
pub const NEW_FOLDER_HINT: Words = Words::new(
    "~/Notes or /srv/docs, then Enter",
    "~/Notes ou /srv/docs, puis Entrée",
);
pub const ADD_FOLDER: Words = Words::new("Add the folder", "Ajouter le dossier");
pub const NOT_A_FULL_PATH: Words = Words::new(
    "Not added: a full path, from / or ~/.",
    "Non ajouté : un chemin complet, depuis / ou ~/.",
);
pub const ALREADY_SEARCHED: Words = Words::new(
    "Not added: it is searched already.",
    "Non ajouté : il est déjà parcouru.",
);
pub const SCREENSHOTS_TEXT: Words = Words::new("Screenshots’ text", "Texte des captures");
pub const SCREENSHOTS_TEXT_SUB: Words = Words::new(
    "The screenshots folder’s pictures, read by OCR",
    "Les images du dossier des captures, lues par OCR",
);
pub const NEVER_READ_NOTE: Words = Words::new(
    "Never read, whatever is named here: ~/.ssh, ~/.gnupg, keyrings, password stores, browser and mail profiles, cloud credentials, .env files, keys and certificates. Hidden folders are skipped and links are not followed.",
    "Jamais lus, quoi qu’on nomme ici : ~/.ssh, ~/.gnupg, les trousseaux, les gestionnaires de mots de passe, les profils de navigateur et de courriel, les identifiants de cloud, les fichiers .env, les clés et les certificats. Les dossiers cachés sont ignorés, les liens ne sont pas suivis.",
);
pub const LEFT_OUT: Words = Words::new("Left out", "Laissés de côté");
pub const NOTHING_LEFT_OUT: Words = Words::new(
    "Nothing besides what is never read.",
    "Rien d’autre que ce qui n’est jamais lu.",
);
pub const NEW_EXCLUDE: Words = Words::new("Name to leave out", "Nom à laisser de côté");
pub const NEW_EXCLUDE_HINT: Words = Words::new(
    "*.log or drafts, then Enter",
    "*.log ou brouillons, puis Entrée",
);
pub const ADD_EXCLUDE: Words = Words::new("Leave the name out", "Laisser le nom de côté");
pub const EXCLUDE_NOTE: Words = Words::new(
    "A file’s or a folder’s name; * stands for any letters, ? for one.",
    "Le nom d’un fichier ou d’un dossier ; * remplace des lettres, ? une seule.",
);
pub const NOT_A_NAME: Words = Words::new(
    "Not added: a name, without /.",
    "Non ajouté : un nom, sans /.",
);
pub const MEANING: Words = Words::new("Search by meaning", "Chercher par le sens");
pub const MEANING_OFF: Words = Words::new("Words only", "Les mots seulement");
pub const MEANING_LOCAL: Words = Words::new("On this machine", "Sur cette machine");
pub const MEANING_ON: Words = Words::new("Wherever the AI is", "Où que soit l’IA");
pub const MEANING_NOTE: Words = Words::new(
    "Documents are found by what they mean too, through vectors from the AI of “AI and agents”. On this machine, nothing leaves it.",
    "Les documents sont trouvés aussi par ce qu’ils veulent dire, grâce aux vecteurs de l’IA d’« IA et agents ». Sur cette machine, rien n’en sort.",
);
pub const MEANING_SENT: Words = Words::new(
    "Wherever the AI is: the start of every document (a few thousand characters each), and what is typed in the command field, are sent to its provider, even when it is another company’s server.",
    "Où que soit l’IA : le début de chaque document (quelques milliers de caractères chacun) et ce qui est tapé dans le champ de commande sont envoyés à son fournisseur, même quand c’est le serveur d’une autre entreprise.",
);
pub const MEANING_AI_OFF: Words = Words::new(
    "The AI is off: documents are found by their words.",
    "L’IA est désactivée : les documents sont trouvés par leurs mots.",
);
pub const MEANING_CLAUDE: Words = Words::new(
    "Claude makes no vectors: documents are found by their words.",
    "Claude ne fait pas de vecteurs : les documents sont trouvés par leurs mots.",
);
pub const MEANING_NOT_HERE: Words = Words::new(
    "The AI is not on this machine: documents are found by their words.",
    "L’IA n’est pas sur cette machine : les documents sont trouvés par leurs mots.",
);
pub const EMBEDDING_MODEL: Words = Words::new("Embedding model", "Modèle de vecteurs");
pub const EMBEDDING_HINT: Words = Words::new(
    "Empty: the first with “embed” in its name",
    "Vide : le premier avec « embed » dans son nom",
);
pub const EMBEDDING_NOTE: Words = Words::new(
    "nomic-embed-text, mxbai-embed-large…; with none, words only. Enter writes it.",
    "nomic-embed-text, mxbai-embed-large… ; sans, les mots seulement. Entrée l’enregistre.",
);
pub const ALSO_OFFERED: Words = Words::new(
    "Also in the command field",
    "Aussi dans le champ de commande",
);
pub const CALCULATOR: Words = Words::new("Calculator", "Calculatrice");
pub const CALCULATOR_SUB: Words = Words::new("2*21 as you type", "2*21 pendant la frappe");
pub const UNITS: Words = Words::new("Units", "Unités");
pub const UNITS_SUB: Words = Words::new("12 km in miles", "12 km en miles");
pub const URLS: Words = Words::new("Addresses", "Adresses");
pub const URLS_SUB: Words = Words::new(
    "example.org, in the browser",
    "example.org, dans le navigateur",
);
pub const WEB_SEARCH: Words = Words::new("Web search", "Recherche sur le web");
pub const WEB_SEARCH_SUB: Words = Words::new("Always last", "Toujours en dernier");
pub const LARGE_TYPE: Words = Words::new("Large type", "Grand texte");
pub const LARGE_TYPE_SUB: Words = Words::new("large <words>", "grand <mots>");
pub const QR_CODE: Words = Words::new("QR code", "Code QR");
pub const QR_CODE_SUB: Words = Words::new("qr <words>", "qr <mots>");
pub const REQUESTS: Words = Words::new("Requests", "Demandes");
pub const REQUESTS_SUB: Words = Words::new(
    "Understood offline, confirmed first",
    "Comprises hors ligne, confirmées d’abord",
);
pub const SEARCH_ENGINE: Words = Words::new("Search engine", "Moteur de recherche");
pub const ENGINE_ADDRESS: Words =
    Words::new("Search engine address", "Adresse du moteur de recherche");
pub const SEARCH_ENGINE_HINT: Words =
    Words::new("https://…?q={}, then Enter", "https://…?q={}, puis Entrée");
pub const SEARCH_ENGINE_NOTE: Words = Words::new(
    "{} is where the words go. Enter writes it.",
    "{} marque la place des mots. Entrée l’enregistre.",
);
pub const BAD_ENGINE: Words = Words::new(
    "Not written: an http(s) address with {} where the words go.",
    "Non enregistrée : une adresse http(s) avec {} à la place des mots.",
);
pub const ALWAYS_OFFERED: Words = Words::new(
    "Applications and actions are always offered.",
    "Les applications et les actions sont toujours proposées.",
);

/// "1 document", "1204 documents".
pub fn documents(n: u64, lang: Lang) -> String {
    match (lang, n) {
        (Lang::En, 0) => "No document".into(),
        (Lang::En, 1) => "1 document".into(),
        (Lang::En, n) => format!("{n} documents"),
        (Lang::Fr, 0) => "Aucun document".into(),
        (Lang::Fr, 1) => "1 document".into(),
        (Lang::Fr, n) => format!("{n} documents"),
    }
}

/// When the folders were last looked at: "Last look 5 min ago".
pub fn last_look(seconds: Option<u64>, lang: Lang) -> String {
    let Some(s) = seconds else {
        return NOT_LOOKED_YET.get(lang).to_owned();
    };
    let nb = match lang {
        Lang::En => " ",
        Lang::Fr => "\u{a0}",
    };
    let ago = match (s / 60, s / 3600, s / 86_400) {
        (0, _, _) => None,
        (m, 0, _) => Some(format!("{m}{nb}min")),
        (_, h, 0..=1) => Some(format!("{h}{nb}h")),
        (_, _, d) => Some(match lang {
            Lang::En => format!("{d} days"),
            Lang::Fr => format!("{d}{nb}jours"),
        }),
    };
    match (lang, ago) {
        (Lang::En, None) => "Last look just now".into(),
        (Lang::En, Some(ago)) => format!("Last look {ago} ago"),
        (Lang::Fr, None) => "Dernier parcours à l’instant".into(),
        (Lang::Fr, Some(ago)) => format!("Dernier parcours il y a {ago}"),
    }
}

/// "12 with their meaning".
pub fn with_meaning(n: u64, lang: Lang) -> String {
    match lang {
        Lang::En => format!("{n} with their meaning"),
        Lang::Fr => format!("{n} avec leur sens"),
    }
}

/// "3 never read".
pub fn never_read(n: u64, lang: Lang) -> String {
    match (lang, n) {
        (Lang::En, n) => format!("{n} never read"),
        (Lang::Fr, 1) => "1 jamais lu".into(),
        (Lang::Fr, n) => format!("{n} jamais lus"),
    }
}

/// "1 unreadable".
pub fn unreadable(n: u64, lang: Lang) -> String {
    match (lang, n) {
        (Lang::En, n) => format!("{n} unreadable"),
        (Lang::Fr, 1) => "1 illisible".into(),
        (Lang::Fr, n) => format!("{n} illisibles"),
    }
}

/// "Not added: ~/.ssh is never read; keys, passwords and browser profiles
/// never are."
pub fn never_read_place(place: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!(
            "Not added: {place} is never read; keys, passwords and browser profiles never are."
        ),
        Lang::Fr => format!(
            "Non ajouté : {place} n’est jamais lu ; clés, mots de passe et profils de navigateur ne le sont jamais."
        ),
    }
}

/// "Remove ~/Notes".
pub fn remove_named(name: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Remove {name}"),
        Lang::Fr => format!("Retirer {name}"),
    }
}
