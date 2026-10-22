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
    "Changing a shortcut comes with #110; the configuration file holds them meanwhile.",
    "Changer un raccourci arrive avec #110 ; le fichier de configuration les tient en attendant.",
);
pub const ACTION: Words = Words::new("Action", "Action");
pub const KEYS: Words = Words::new("Keys", "Touches");

pub const SCREENS_NOTE: Words = Words::new(
    "Arranging screens and changing their modes comes with #111.",
    "Placer les écrans et changer leurs modes arrive avec #111.",
);
pub const DESKTOPS_PER_SCREEN: Words =
    Words::new("Desktops on a new screen", "Bureaux sur un nouvel écran");
pub const PRIMARY: Words = Words::new("Primary", "Principal");
pub const SCALE: Words = Words::new("Scale", "Échelle");

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

/// The groups of the shortcut list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutGroup {
    Windows,
    Desktops,
    Screens,
    Launcher,
    System,
}

impl ShortcutGroup {
    pub const ALL: [ShortcutGroup; 5] = [
        Self::Windows,
        Self::Desktops,
        Self::Screens,
        Self::Launcher,
        Self::System,
    ];

    pub fn label(self) -> Words {
        match self {
            Self::Windows => Words::new("Windows", "Fenêtres"),
            Self::Desktops => Words::new("Desktops", "Bureaux"),
            Self::Screens => Words::new("Screens", "Écrans"),
            Self::Launcher => Words::new("Launcher and AI", "Lanceur et IA"),
            Self::System => Words::new("System", "Système"),
        }
    }
}

pub const ALL_GROUPS: Words = Words::new("All", "Tous");
pub const ACTS_ON: Words = Words::new("Acts on", "Agit sur");
const EVERYWHERE: Words = Words::new("Everywhere", "Partout");
const POINTER_SCREEN: Words = Words::new("Screen under the pointer", "Écran sous le pointeur");
const ACTIVE_WINDOW: Words = Words::new("Active window", "Fenêtre active");
const ALL_SCREENS: Words = Words::new("All screens", "Tous les écrans");

/// What a shortcut does, in words: its name, its group and what it acts
/// on.
pub fn action(action: &hocus_core::Action, lang: Lang) -> (String, ShortcutGroup, &'static str) {
    use ShortcutGroup as G;
    use hocus_core::{Action as A, Scope};
    let w = |en: &'static str, fr: &'static str| Words::new(en, fr).get(lang).to_owned();
    let scope = |s: &Scope| match s {
        Scope::AllOutputs => ALL_SCREENS,
        _ => POINTER_SCREEN,
    };
    let (label, group, on) = match action {
        A::Quit => (
            w("Quit Hocuspanda", "Quitter Hocuspanda"),
            G::System,
            EVERYWHERE,
        ),
        A::Desktop(n) => (
            match lang {
                Lang::En => format!("Show desktop {n}"),
                Lang::Fr => format!("Afficher le bureau {n}"),
            },
            G::Desktops,
            POINTER_SCREEN,
        ),
        A::NextDesktop(s) => (w("Next desktop", "Bureau suivant"), G::Desktops, scope(s)),
        A::PreviousDesktop(s) => (
            w("Previous desktop", "Bureau précédent"),
            G::Desktops,
            scope(s),
        ),
        A::SendToDesktop(n) => (
            match lang {
                Lang::En => format!("Send the window to desktop {n}"),
                Lang::Fr => format!("Envoyer la fenêtre au bureau {n}"),
            },
            G::Desktops,
            ACTIVE_WINDOW,
        ),
        A::SendToNextDesktop => (
            w(
                "Send the window to the next desktop",
                "Envoyer la fenêtre au bureau suivant",
            ),
            G::Desktops,
            ACTIVE_WINDOW,
        ),
        A::SendToPreviousDesktop => (
            w(
                "Send the window to the previous desktop",
                "Envoyer la fenêtre au bureau précédent",
            ),
            G::Desktops,
            ACTIVE_WINDOW,
        ),
        A::SendToOutputLeft => (
            w(
                "Send the window to the screen on the left",
                "Envoyer la fenêtre à l’écran de gauche",
            ),
            G::Screens,
            ACTIVE_WINDOW,
        ),
        A::SendToOutputRight => (
            w(
                "Send the window to the screen on the right",
                "Envoyer la fenêtre à l’écran de droite",
            ),
            G::Screens,
            ACTIVE_WINDOW,
        ),
        A::ToggleMaximized => (
            w("Maximize or restore", "Agrandir ou restaurer"),
            G::Windows,
            ACTIVE_WINDOW,
        ),
        A::ToggleFullscreen => (w("Fullscreen", "Plein écran"), G::Windows, ACTIVE_WINDOW),
        A::Snap(zone) => (
            match lang {
                Lang::En => format!("Snap: {zone:?}"),
                Lang::Fr => format!("Aimanter : {zone:?}"),
            },
            G::Windows,
            ACTIVE_WINDOW,
        ),
        A::ToggleTiling => (
            w("Tile the desktop", "Paver le bureau"),
            G::Windows,
            POINTER_SCREEN,
        ),
        A::MakeMaster => (
            w("Make the main window", "Mettre en fenêtre principale"),
            G::Windows,
            ACTIVE_WINDOW,
        ),
        A::GrowMaster => (
            w("Widen the main area", "Élargir la zone principale"),
            G::Windows,
            POINTER_SCREEN,
        ),
        A::ShrinkMaster => (
            w("Narrow the main area", "Rétrécir la zone principale"),
            G::Windows,
            POINTER_SCREEN,
        ),
        A::ToggleFloating => (
            w("Float or tile the window", "Fenêtre flottante ou pavée"),
            G::Windows,
            ACTIVE_WINDOW,
        ),
        A::Minimize => (w("Minimize", "Réduire"), G::Windows, ACTIVE_WINDOW),
        A::Close => (
            w("Close the window", "Fermer la fenêtre"),
            G::Windows,
            ACTIVE_WINDOW,
        ),
        A::SwitchLayout => (
            w("Next keyboard layout", "Disposition suivante"),
            G::System,
            EVERYWHERE,
        ),
        A::Launcher => (
            w("Command field", "Champ de commande"),
            G::Launcher,
            POINTER_SCREEN,
        ),
        A::QuickSettings => (
            w("Quick settings", "Réglages rapides"),
            G::System,
            POINTER_SCREEN,
        ),
        A::Dropdown => (
            w("Drop-down terminal", "Terminal déroulant"),
            G::Launcher,
            POINTER_SCREEN,
        ),
        A::Settings => (w("Settings", "Réglages"), G::System, EVERYWHERE),
        A::Lock => (
            w("Lock the session", "Verrouiller la session"),
            G::System,
            EVERYWHERE,
        ),
        A::SwitchWindows(s) => (
            w("Switch windows", "Alterner les fenêtres"),
            G::Windows,
            scope(s),
        ),
        A::Spawn(command) => (
            match lang {
                Lang::En => format!("Run {command}"),
                Lang::Fr => format!("Lancer {command}"),
            },
            G::Launcher,
            EVERYWHERE,
        ),
    };
    (label, group, on.get(lang))
}
