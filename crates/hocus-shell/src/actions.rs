//! What every action is called, in English and French, the group it
//! belongs to and what it acts on: one table for the settings' shortcut
//! list and the action palette (#68).

use hocus_core::{
    Action, Scope,
    shortcuts::{Combo, Key, ScreenshotMode},
    snap::Zone,
};

use crate::i18n::Lang;

/// The groups of the shortcut list and the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    Windows,
    Desktops,
    Screens,
    Launcher,
    Captures,
    System,
}

impl Group {
    pub const ALL: [Group; 6] = [
        Self::Windows,
        Self::Desktops,
        Self::Screens,
        Self::Launcher,
        Self::Captures,
        Self::System,
    ];

    pub fn name(self, lang: Lang) -> &'static str {
        let (en, fr) = match self {
            Self::Windows => ("Windows", "Fenêtres"),
            Self::Desktops => ("Desktops", "Bureaux"),
            Self::Screens => ("Screens", "Écrans"),
            Self::Launcher => ("Launcher and AI", "Lanceur et IA"),
            Self::Captures => ("Captures", "Captures"),
            Self::System => ("System", "Système"),
        };
        match lang {
            Lang::En => en,
            Lang::Fr => fr,
        }
    }
}

/// An action in words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Described {
    pub name: String,
    pub group: Group,
    /// What it acts on: "Active window", "Everywhere"…
    pub scope: &'static str,
}

const EVERYWHERE: (&str, &str) = ("Everywhere", "Partout");
const POINTER_SCREEN: (&str, &str) = ("Screen under the pointer", "Écran sous le pointeur");
const ACTIVE_WINDOW: (&str, &str) = ("Active window", "Fenêtre active");
const ALL_SCREENS: (&str, &str) = ("All screens", "Tous les écrans");

/// `action` in words, in `lang`.
pub fn describe(action: &Action, lang: Lang) -> Described {
    use Action as A;
    use Group as G;
    let pick = |(en, fr): (&'static str, &'static str)| match lang {
        Lang::En => en,
        Lang::Fr => fr,
    };
    let w = |en: &'static str, fr: &'static str| pick((en, fr)).to_owned();
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
            {
                use hocus_core::snap::Zone as Z;
                match zone {
                    Z::Left => w("Snap to the left half", "Aimanter à la moitié gauche"),
                    Z::Right => w("Snap to the right half", "Aimanter à la moitié droite"),
                    Z::TopLeft => w("Snap to the top left", "Aimanter en haut à gauche"),
                    Z::TopRight => w("Snap to the top right", "Aimanter en haut à droite"),
                    Z::BottomLeft => w("Snap to the bottom left", "Aimanter en bas à gauche"),
                    Z::BottomRight => w("Snap to the bottom right", "Aimanter en bas à droite"),
                    Z::Top => w("Snap to the whole screen", "Aimanter à tout l’écran"),
                }
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
        A::LogOut => (w("Log out", "Se déconnecter"), G::System, EVERYWHERE),
        A::Restart => (w("Restart", "Redémarrer"), G::System, EVERYWHERE),
        A::ShutDown => (w("Shut down", "Éteindre"), G::System, EVERYWHERE),
        A::Suspend => (w("Suspend", "Mettre en veille"), G::System, EVERYWHERE),
        A::ClipboardHistory => (
            w("Clipboard history", "Historique du presse-papiers"),
            G::System,
            EVERYWHERE,
        ),
        A::Emoji => (w("Emoji", "Emoji"), G::System, EVERYWHERE),
        A::Notes => (w("Notes", "Notes"), G::Launcher, EVERYWHERE),
        A::FocusMode => (
            w(
                "Focus: dim everything else",
                "Concentration : assombrir le reste",
            ),
            G::Windows,
            ACTIVE_WINDOW,
        ),
        A::NightLight => (w("Night light", "Lumière nocturne"), G::System, ALL_SCREENS),
        A::ZoomIn => (w("Zoom in", "Agrandir (loupe)"), G::Screens, POINTER_SCREEN),
        A::Dock => (
            w("Show or hide the dock", "Afficher ou masquer le dock"),
            G::System,
            EVERYWHERE,
        ),
        A::Panel => (
            w("Show or hide the panel", "Afficher ou masquer le panneau"),
            G::System,
            EVERYWHERE,
        ),
        A::ZoomOut => (w("Zoom out", "Réduire (loupe)"), G::Screens, POINTER_SCREEN),
        A::Overview => (
            w("Overview of every window", "Vue d’ensemble des fenêtres"),
            G::Windows,
            ALL_SCREENS,
        ),
        A::Welcome => (
            w("Welcome to Hocuspanda", "Bienvenue dans Hocuspanda"),
            G::System,
            EVERYWHERE,
        ),
        A::About => (
            w("About Hocuspanda", "À propos de Hocuspanda"),
            G::System,
            EVERYWHERE,
        ),
        A::StopAgents => (
            w(
                "Stop every AI agent, or allow them again",
                "Arrêter tous les agents IA, ou les autoriser à nouveau",
            ),
            G::Launcher,
            EVERYWHERE,
        ),
        A::ShowDesktop => (
            w(
                "Show the desktop, or the windows again",
                "Afficher le bureau, ou les fenêtres à nouveau",
            ),
            G::Windows,
            POINTER_SCREEN,
        ),
        A::Record => (
            w(
                "Record the screen, or stop",
                "Enregistrer l’écran, ou arrêter",
            ),
            G::Captures,
            EVERYWHERE,
        ),
        A::Palette => (
            w(
                "Every action and its keys",
                "Toutes les actions et leurs touches",
            ),
            G::Launcher,
            EVERYWHERE,
        ),
        A::Screenshot { mode, delay } => (
            {
                use hocus_core::shortcuts::ScreenshotMode as M;
                let what = match mode {
                    M::Screen => w("Capture the screen", "Capturer l’écran"),
                    M::All => w("Capture every screen", "Capturer tous les écrans"),
                    M::Region => w("Capture a region", "Capturer une zone"),
                    M::Window => w("Capture a window", "Capturer une fenêtre"),
                };
                match (lang, delay) {
                    (_, 0) => what,
                    (Lang::En, d) => format!("{what} in {d} s"),
                    (Lang::Fr, d) => format!("{what} dans {d}\u{a0}s"),
                }
            },
            G::Captures,
            EVERYWHERE,
        ),
        A::Hibernate => (
            w("Hibernate", "Mettre en veille prolongée"),
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
    Described {
        name: label,
        group,
        scope: pick(on),
    }
}

/// Every action the palette lists, bound or not: those with a parameter
/// once per value that makes sense; commands only when bound.
pub fn catalog() -> Vec<Action> {
    use Action as A;
    let mut all = vec![
        A::NextDesktop(Scope::PointerOutput),
        A::PreviousDesktop(Scope::PointerOutput),
        A::NextDesktop(Scope::AllOutputs),
        A::PreviousDesktop(Scope::AllOutputs),
        A::SendToNextDesktop,
        A::SendToPreviousDesktop,
        A::SendToOutputLeft,
        A::SendToOutputRight,
        A::ToggleMaximized,
        A::ToggleFullscreen,
    ];
    all.extend(
        [
            Zone::Left,
            Zone::Right,
            Zone::TopLeft,
            Zone::TopRight,
            Zone::BottomLeft,
            Zone::BottomRight,
        ]
        .map(A::Snap),
    );
    all.extend([
        A::ToggleTiling,
        A::MakeMaster,
        A::GrowMaster,
        A::ShrinkMaster,
        A::ToggleFloating,
        A::Minimize,
        A::Close,
        A::SwitchWindows(Scope::PointerOutput),
        A::SwitchWindows(Scope::AllOutputs),
        A::SwitchLayout,
        A::Launcher,
        A::Palette,
        A::StopAgents,
        A::QuickSettings,
        A::Dropdown,
        A::Settings,
        A::ClipboardHistory,
        A::Emoji,
    ]);
    all.extend(
        [
            ScreenshotMode::Screen,
            ScreenshotMode::All,
            ScreenshotMode::Region,
            ScreenshotMode::Window,
        ]
        .map(Action::screenshot),
    );
    all.push(A::Record);
    all.extend([
        A::FocusMode,
        A::NightLight,
        A::ZoomIn,
        A::ZoomOut,
        A::Notes,
        A::Dock,
        A::Panel,
        A::Overview,
        A::ShowDesktop,
        A::Welcome,
        A::About,
    ]);
    all.extend([
        A::Lock,
        A::Suspend,
        A::Hibernate,
        A::LogOut,
        A::Restart,
        A::ShutDown,
        A::Quit,
    ]);
    for n in 1..=9 {
        all.push(A::Desktop(n));
    }
    for n in 1..=9 {
        all.push(A::SendToDesktop(n));
    }
    all
}

/// A combination as key caps, in `lang`: `["Super", "Maj", "S"]`.
pub fn caps(combo: &Combo, lang: Lang) -> Vec<String> {
    let fr = lang == Lang::Fr;
    let m = combo.modifiers;
    let mut caps: Vec<String> = [
        (m.logo, "Super"),
        (m.ctrl, "Ctrl"),
        (m.alt, "Alt"),
        (m.shift, if fr { "Maj" } else { "Shift" }),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .map(|(_, name)| name.to_owned())
    .collect();
    let key = match &combo.key {
        Key::Digit(d) => d.to_string(),
        Key::Named(name) => match (name.as_str(), fr) {
            ("left", _) => "←".into(),
            ("right", _) => "→".into(),
            ("up", _) => "↑".into(),
            ("down", _) => "↓".into(),
            ("space", false) => "Space".into(),
            ("space", true) => "Espace".into(),
            ("return", false) => "Enter".into(),
            ("return", true) => "Entrée".into(),
            ("print", false) => "Print".into(),
            ("print", true) => "Impr".into(),
            ("backspace", _) => "⌫".into(),
            ("escape", false) => "Esc".into(),
            ("escape", true) => "Échap".into(),
            ("grave", _) => "`".into(),
            ("comma", _) => ",".into(),
            ("period", _) => ".".into(),
            ("tab", _) => "Tab".into(),
            (other, _) => {
                let mut chars = other.chars();
                let first: String = chars
                    .next()
                    .into_iter()
                    .flat_map(char::to_uppercase)
                    .collect();
                format!("{first}{}", chars.as_str())
            }
        },
    };
    caps.push(key);
    caps
}

/// What a typed or pressed combination asks for: the canonical names of
/// its parts, lower case (`"maj+s"` is `["shift", "s"]`); `None` when it
/// does not read as keys.
pub fn keys_query(query: &str) -> Option<Vec<String>> {
    let parts: Vec<String> = query
        .split('+')
        .map(|p| crate::fuzzy::fold(p.trim()))
        .collect();
    if parts.len() < 2 || parts.iter().any(String::is_empty) {
        return None;
    }
    Some(
        parts
            .into_iter()
            .map(|p| {
                match p.as_str() {
                    "maj" | "majuscule" => "shift",
                    "logo" | "win" | "meta" => "super",
                    "control" | "ctl" => "ctrl",
                    "espace" => "space",
                    "entree" | "enter" => "return",
                    "impr" | "imprecran" => "print",
                    "echap" | "esc" => "escape",
                    "←" | "gauche" => "left",
                    "→" | "droite" => "right",
                    "↑" | "haut" => "up",
                    "↓" | "bas" => "down",
                    "`" | "backtick" => "grave",
                    "," => "comma",
                    "." => "period",
                    _ => return p,
                }
                .to_owned()
            })
            .collect(),
    )
}

/// How well `combo` answers keys asked: 0 when they are its keys, 1 when
/// they are among them; `None` otherwise.
pub fn keys_rank(asked: &[String], combo: &Combo) -> Option<u8> {
    let m = combo.modifiers;
    let mut own: Vec<String> = [
        (m.logo, "super"),
        (m.ctrl, "ctrl"),
        (m.alt, "alt"),
        (m.shift, "shift"),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .map(|(_, n)| n.to_owned())
    .collect();
    own.push(match &combo.key {
        Key::Digit(d) => d.to_string(),
        Key::Named(n) => n.clone(),
    });
    let mut sorted_asked = asked.to_vec();
    sorted_asked.sort();
    let mut sorted_own = own.clone();
    sorted_own.sort();
    if sorted_asked == sorted_own {
        Some(0)
    } else if asked.iter().all(|a| own.contains(a)) {
        Some(1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_action_has_a_name_in_both_languages() {
        for action in catalog() {
            for lang in [Lang::En, Lang::Fr] {
                let d = describe(&action, lang);
                assert!(!d.name.is_empty() && !d.scope.is_empty(), "{action}");
            }
        }
        let region = Action::screenshot(ScreenshotMode::Region);
        assert_eq!(describe(&region, Lang::En).name, "Capture a region");
        assert_eq!(describe(&region, Lang::Fr).name, "Capturer une zone");
    }

    #[test]
    fn keys_read_in_either_language_and_show_as_caps() {
        let combo: Combo = "Super+Shift+S".parse().expect("a combination");
        assert_eq!(caps(&combo, Lang::Fr), ["Super", "Maj", "S"]);
        assert_eq!(caps(&combo, Lang::En), ["Super", "Shift", "S"]);
        let asked = keys_query("Maj+S").expect("keys");
        assert_eq!(asked, ["shift", "s"]);
        assert_eq!(keys_rank(&asked, &combo), Some(1));
        let pressed = keys_query("Super+Maj+S").expect("keys");
        assert_eq!(keys_rank(&pressed, &combo), Some(0));
        let print: Combo = "Shift+Print".parse().expect("a combination");
        assert_eq!(keys_rank(&asked, &print), None);
        assert!(keys_query("capture").is_none());
    }
}
