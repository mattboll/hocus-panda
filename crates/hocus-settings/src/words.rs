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
pub const REDUCED_MOTION: Words = Words::new("Reduced motion", "Mouvement réduit");
pub const REDUCED_MOTION_SUB: Words =
    Words::new("Short fades, no bounce", "Fondus courts, pas de rebond");
pub const HIGH_CONTRAST: Words = Words::new("High contrast", "Contraste élevé");
pub const HIGH_CONTRAST_SUB: Words =
    Words::new("Sharp edges, plainer text", "Bords nets, texte plus franc");
pub const TRANSPARENCY: Words = Words::new("Transparency", "Transparences");
pub const TRANSPARENCY_SUB: Words = Words::new(
    "Frosted glass of the panels, with blur (#37)",
    "Verre dépoli des panneaux, avec le flou (#37)",
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

/// Keyboard layouts offered, by XKB name, in both languages.
pub const LAYOUT_NAMES: [(&str, Words); 22] = [
    ("us", Words::new("English · US", "Anglais · US")),
    ("gb", Words::new("English · UK", "Anglais · Royaume-Uni")),
    ("fr", Words::new("French · AZERTY", "Français · AZERTY")),
    ("be", Words::new("Belgian", "Belge")),
    ("ch", Words::new("Swiss", "Suisse")),
    ("ca", Words::new("Canadian French", "Français canadien")),
    ("de", Words::new("German", "Allemand")),
    ("es", Words::new("Spanish", "Espagnol")),
    ("latam", Words::new("Latin American", "Latino-américain")),
    ("it", Words::new("Italian", "Italien")),
    ("pt", Words::new("Portuguese", "Portugais")),
    ("br", Words::new("Brazilian", "Brésilien")),
    ("nl", Words::new("Dutch", "Néerlandais")),
    ("se", Words::new("Swedish", "Suédois")),
    ("no", Words::new("Norwegian", "Norvégien")),
    ("dk", Words::new("Danish", "Danois")),
    ("fi", Words::new("Finnish", "Finnois")),
    ("pl", Words::new("Polish", "Polonais")),
    ("cz", Words::new("Czech", "Tchèque")),
    ("ru", Words::new("Russian", "Russe")),
    ("ua", Words::new("Ukrainian", "Ukrainien")),
    ("gr", Words::new("Greek", "Grec")),
];

/// A layout's name, or its XKB name when it has none here.
pub fn layout_name(code: &str, lang: Lang) -> String {
    LAYOUT_NAMES
        .iter()
        .find(|(c, _)| *c == code)
        .map_or_else(|| code.to_uppercase(), |(_, w)| w.get(lang).to_owned())
}

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

pub fn synced(events: usize, lang: Lang) -> String {
    match (lang, events) {
        (Lang::En, 1) => "Synced · 1 event".into(),
        (Lang::En, n) => format!("Synced · {n} events"),
        (Lang::Fr, 1) => "Synchronisé · 1 événement".into(),
        (Lang::Fr, n) => format!("Synchronisé · {n} événements"),
    }
}
