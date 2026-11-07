//! Clavier et raccourcis / Keyboard and shortcuts (6.8, variant A): the
//! layouts, the keys' repeat and the special keys on the section's page;
//! every desktop shortcut (`[shortcuts]`, the compositor's
//! `shortcuts.rs`: 73 actions) on a sub-page of its own, with its search.
//! Each is an action of the palette's catalogue too
//! (`hocus_link::actions`, #736): its tests check so.
//!
//! The compositor reads all of it again as the file changes: it rebuilds
//! the keymap, gives clients the repeat, takes the new shortcuts.
//! "Rétablir" takes a shortcut's line out of the file, so that its default
//! applies and follows a changed default later.

use std::sync::OnceLock;

use crate::{
    lang::{Words, w},
    model::{Choice, Codec, Kind, Place, Places, RowGroup, SectionId, Setting, Value},
};

pub const LAYOUTS: RowGroup = RowGroup {
    label: w("DISPOSITIONS", "LAYOUTS"),
    per_source: false,
};
pub const REPEAT: RowGroup = RowGroup {
    label: w("RÉPÉTITION DES TOUCHES", "KEY REPEAT"),
    per_source: false,
};
pub const SPECIAL: RowGroup = RowGroup {
    label: w("TOUCHES SPÉCIALES", "SPECIAL KEYS"),
    per_source: false,
};
pub const WINDOWS: RowGroup = RowGroup {
    label: w("FENÊTRES", "WINDOWS"),
    per_source: false,
};
pub const DESKTOPS: RowGroup = RowGroup {
    label: w("BUREAUX", "DESKTOPS"),
    per_source: false,
};
pub const BAR: RowGroup = RowGroup {
    label: w("BARRE ET LANCEUR", "BAR AND LAUNCHER"),
    per_source: false,
};
pub const CAPTURES: RowGroup = RowGroup {
    label: w("CAPTURES", "CAPTURES"),
    per_source: false,
};
pub const SESSION: RowGroup = RowGroup {
    label: w("SESSION", "SESSION"),
    per_source: false,
};
pub const MEDIA: RowGroup = RowGroup {
    label: w("TOUCHES MULTIMÉDIA", "MEDIA KEYS"),
    per_source: false,
};

/// The shortcuts' groups, in the sub-page's order.
pub const MAGNIFIER: RowGroup = RowGroup {
    label: w("LOUPE", "MAGNIFIER"),
    per_source: false,
};

/// The screen reader's shortcut (#734).
pub const SCREEN_READER: RowGroup = RowGroup {
    label: w("LECTEUR D'ÉCRAN", "SCREEN READER"),
    per_source: false,
};

pub const SHORTCUT_GROUPS: [RowGroup; 8] = [
    WINDOWS,
    DESKTOPS,
    BAR,
    CAPTURES,
    SESSION,
    MEDIA,
    MAGNIFIER,
    SCREEN_READER,
];

pub const SWITCHES: &[&str] = &[
    "",
    "grp:alt_shift_toggle",
    "grp:ctrl_shift_toggle",
    "grp:shifts_toggle",
    "grp:win_space_toggle",
];
pub const CAPS: &[&str] = &["", "caps:ctrl_modifier", "caps:escape"];
pub const COMPOSE: &[&str] = &[
    "",
    "compose:ralt",
    "compose:menu",
    "compose:rctrl",
    "compose:caps",
];

/// A family of nine (desktops 1 to 9, send to, take to): one row, folded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Family {
    /// `desktop-`, the members' keys without their digit.
    pub prefix: &'static str,
    pub name: Words,
}

pub const FAMILIES: [Family; 3] = [
    Family {
        prefix: "desktop-",
        name: w("Aller au bureau 1 à 9", "Go to desktop 1 to 9"),
    },
    Family {
        prefix: "send-to-desktop-",
        name: w("Envoyer au bureau 1 à 9", "Send to desktop 1 to 9"),
    },
    Family {
        prefix: "take-to-desktop-",
        name: w("Emmener au bureau 1 à 9", "Take to desktop 1 to 9"),
    },
];

/// The family `setting` (`shortcuts.desktop-3`) belongs to.
pub fn family_of(setting: &str) -> Option<&'static Family> {
    let key = setting.strip_prefix("shortcuts.")?;
    FAMILIES.iter().find(|f| {
        key.strip_prefix(f.prefix)
            .is_some_and(|n| n.len() == 1 && n.chars().all(|c| ('1'..='9').contains(&c)))
    })
}

/// One action of `[shortcuts]`: its key, its group, its names, its
/// defaults, a few nearby words.
struct Action {
    key: &'static str,
    group: RowGroup,
    name: Words,
    defaults: &'static [&'static str],
    near: (&'static [&'static str], &'static [&'static str]),
}

const fn a(
    key: &'static str,
    group: RowGroup,
    name: Words,
    defaults: &'static [&'static str],
    near: (&'static [&'static str], &'static [&'static str]),
) -> Action {
    Action {
        key,
        group,
        name,
        defaults,
        near,
    }
}

fn actions() -> Vec<Action> {
    let mut list = vec![
        a(
            "close",
            WINDOWS,
            w("Fermer la fenêtre", "Close the window"),
            &["Super+Q", "Alt+F4"],
            (&["quitter"], &["quit"]),
        ),
        a(
            "minimize",
            WINDOWS,
            w("Réduire la fenêtre", "Minimise the window"),
            &["Super+H"],
            (&["cacher", "minimiser"], &["hide", "minimize"]),
        ),
        a(
            "snap-left",
            WINDOWS,
            w("Moitié gauche", "Left half"),
            &["Super+Left"],
            (&["ancrer", "côte à côte"], &["snap", "tile"]),
        ),
        a(
            "snap-right",
            WINDOWS,
            w("Moitié droite", "Right half"),
            &["Super+Right"],
            (&["ancrer", "côte à côte"], &["snap", "tile"]),
        ),
        a(
            "maximize",
            WINDOWS,
            w("Agrandir", "Maximise"),
            &["Super+Up"],
            (&["plein écran", "maximiser"], &["maximize", "full"]),
        ),
        a(
            "restore",
            WINDOWS,
            w("Restaurer", "Restore"),
            &["Super+Down"],
            (&["réduire"], &["unmaximize"]),
        ),
        a(
            "quarter-up",
            WINDOWS,
            w("Quart du haut", "Top quarter"),
            &["Super+Shift+Up"],
            (&["ancrer", "coin"], &["snap", "corner"]),
        ),
        a(
            "quarter-down",
            WINDOWS,
            w("Quart du bas", "Bottom quarter"),
            &["Super+Shift+Down"],
            (&["ancrer", "coin"], &["snap", "corner"]),
        ),
        a(
            "quarter-left",
            WINDOWS,
            w("Quart de gauche", "Left quarter"),
            &["Super+Shift+Left"],
            (&["ancrer", "coin"], &["snap", "corner"]),
        ),
        a(
            "quarter-right",
            WINDOWS,
            w("Quart de droite", "Right quarter"),
            &["Super+Shift+Right"],
            (&["ancrer", "coin"], &["snap", "corner"]),
        ),
        a(
            "focus-left",
            WINDOWS,
            w("Fenêtre de gauche", "Window on the left"),
            &[],
            (&["focus", "aller"], &["focus"]),
        ),
        a(
            "focus-right",
            WINDOWS,
            w("Fenêtre de droite", "Window on the right"),
            &[],
            (&["focus", "aller"], &["focus"]),
        ),
        a(
            "focus-up",
            WINDOWS,
            w("Fenêtre du haut", "Window above"),
            &[],
            (&["focus", "aller"], &["focus"]),
        ),
        a(
            "focus-down",
            WINDOWS,
            w("Fenêtre du bas", "Window below"),
            &[],
            (&["focus", "aller"], &["focus"]),
        ),
        a(
            "switch-windows",
            WINDOWS,
            w("Passer d’une fenêtre à l’autre", "Switch windows"),
            &["Alt+Tab"],
            (&["alt tab", "basculer"], &["alt tab", "switcher"]),
        ),
        a(
            "switch-windows-backward",
            WINDOWS,
            w(
                "Passer d’une fenêtre à l’autre, à rebours",
                "Switch windows backward",
            ),
            &["Alt+Shift+Tab"],
            (&["alt tab", "basculer"], &["alt tab", "switcher"]),
        ),
        // « geste »: what three fingers sideways do, so the search for
        // the touchpad's gestures lists them after its rows (#681).
        a(
            "desktop-left",
            DESKTOPS,
            w("Bureau de gauche", "Desktop on the left"),
            &["Super+Ctrl+Left"],
            (&["espace de travail", "geste"], &["workspace", "gesture"]),
        ),
        a(
            "desktop-right",
            DESKTOPS,
            w("Bureau de droite", "Desktop on the right"),
            &["Super+Ctrl+Right"],
            (&["espace de travail", "geste"], &["workspace", "gesture"]),
        ),
    ];
    const DESKTOP: [&[&str]; 9] = [
        &["Super+1"],
        &["Super+2"],
        &["Super+3"],
        &["Super+4"],
        &["Super+5"],
        &["Super+6"],
        &["Super+7"],
        &["Super+8"],
        &["Super+9"],
    ];
    const SEND: [&[&str]; 9] = [
        &["Super+Shift+1"],
        &["Super+Shift+2"],
        &["Super+Shift+3"],
        &["Super+Shift+4"],
        &["Super+Shift+5"],
        &["Super+Shift+6"],
        &["Super+Shift+7"],
        &["Super+Shift+8"],
        &["Super+Shift+9"],
    ];
    const TAKE: [&[&str]; 9] = [
        &["Super+Ctrl+Shift+1"],
        &["Super+Ctrl+Shift+2"],
        &["Super+Ctrl+Shift+3"],
        &["Super+Ctrl+Shift+4"],
        &["Super+Ctrl+Shift+5"],
        &["Super+Ctrl+Shift+6"],
        &["Super+Ctrl+Shift+7"],
        &["Super+Ctrl+Shift+8"],
        &["Super+Ctrl+Shift+9"],
    ];
    const NUMBERED: [(&str, &str, &str, &str); 9] = [
        ("desktop-1", "send-to-desktop-1", "take-to-desktop-1", "1"),
        ("desktop-2", "send-to-desktop-2", "take-to-desktop-2", "2"),
        ("desktop-3", "send-to-desktop-3", "take-to-desktop-3", "3"),
        ("desktop-4", "send-to-desktop-4", "take-to-desktop-4", "4"),
        ("desktop-5", "send-to-desktop-5", "take-to-desktop-5", "5"),
        ("desktop-6", "send-to-desktop-6", "take-to-desktop-6", "6"),
        ("desktop-7", "send-to-desktop-7", "take-to-desktop-7", "7"),
        ("desktop-8", "send-to-desktop-8", "take-to-desktop-8", "8"),
        ("desktop-9", "send-to-desktop-9", "take-to-desktop-9", "9"),
    ];
    let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    for (keys, words, defaults) in [
        (0, ("Aller au bureau", "Go to desktop"), DESKTOP),
        (1, ("Envoyer au bureau", "Send to desktop"), SEND),
        (2, ("Emmener au bureau", "Take to desktop"), TAKE),
    ] {
        for (i, numbered) in NUMBERED.iter().enumerate() {
            let key = [numbered.0, numbered.1, numbered.2][keys];
            let n = numbered.3;
            list.push(a(
                key,
                DESKTOPS,
                w(
                    leak(format!("{} {n}", words.0)),
                    leak(format!("{} {n}", words.1)),
                ),
                defaults[i],
                (&["espace de travail"], &["workspace"]),
            ));
        }
    }
    list.extend([
        a(
            "take-to-desktop-left",
            DESKTOPS,
            w(
                "Emmener au bureau de gauche",
                "Take to the desktop on the left",
            ),
            &["Super+Ctrl+Shift+Left"],
            (&["déplacer"], &["move"]),
        ),
        a(
            "take-to-desktop-right",
            DESKTOPS,
            w(
                "Emmener au bureau de droite",
                "Take to the desktop on the right",
            ),
            &["Super+Ctrl+Shift+Right"],
            (&["déplacer"], &["move"]),
        ),
        // Nested, the compositor gives these no default: `Super+Alt` is
        // already its `Super` there (8.4).
        a(
            "move-to-screen-left",
            DESKTOPS,
            w(
                "Envoyer sur l’écran de gauche",
                "Move to the screen on the left",
            ),
            &["Super+Alt+Left"],
            (&["déplacer", "moniteur"], &["move", "monitor", "display"]),
        ),
        a(
            "move-to-screen-right",
            DESKTOPS,
            w(
                "Envoyer sur l’écran de droite",
                "Move to the screen on the right",
            ),
            &["Super+Alt+Right"],
            (&["déplacer", "moniteur"], &["move", "monitor", "display"]),
        ),
        a(
            "overview",
            DESKTOPS,
            w("Vue d’ensemble", "Overview"),
            &["Super+W"],
            (
                &["exposé", "bureaux", "fenêtres", "tout voir"],
                &["expose", "desktops", "windows", "workspaces"],
            ),
        ),
        a(
            "terminal",
            BAR,
            w("Ouvrir un terminal", "Open a terminal"),
            &["Super+Return"],
            (&["console", "shell"], &["console", "shell"]),
        ),
        a(
            "launcher",
            BAR,
            w("Ouvrir le lanceur", "Open the launcher"),
            &["Super+Space"],
            (&["applications", "chercher"], &["apps", "search"]),
        ),
        a(
            "actions",
            BAR,
            w("Actions", "Actions"),
            &["Super+P"],
            (
                &["palette", "commandes", "tout faire", "chercher une action"],
                &["palette", "commands", "command palette", "search actions"],
            ),
        ),
        a(
            "focus-bar",
            BAR,
            w("Aller dans la barre", "Go to the bar"),
            &["Super+B"],
            (
                &["clavier", "barre", "accessibilité", "focus"],
                &["keyboard", "panel", "accessibility", "focus"],
            ),
        ),
        a(
            "toggle-bar",
            BAR,
            w("Changer la forme de la barre", "Change the bar's form"),
            &["Super+Shift+B"],
            (&["mini", "barre"], &["panel", "bar"]),
        ),
        a(
            "clipboard",
            BAR,
            w("Historique du presse-papiers", "Clipboard history"),
            &["Super+V"],
            (&["copier", "coller"], &["copy", "paste"]),
        ),
        a(
            "emoji",
            BAR,
            w("Émojis", "Emoji"),
            &["Super+period"],
            (&["émoticône", "smiley"], &["emoji", "smiley"]),
        ),
        a(
            "join",
            BAR,
            w("Rejoindre la réunion", "Join the meeting"),
            &["Super+J"],
            (&["visio", "appel"], &["call", "video"]),
        ),
        a(
            "capture",
            CAPTURES,
            w("Capture d’écran", "Screenshot"),
            &["Print"],
            (&["capture", "copie d’écran"], &["screenshot", "capture"]),
        ),
        a(
            "capture-screen",
            CAPTURES,
            w("Capturer l’écran", "Capture the screen"),
            &["Shift+Print"],
            (&["capture", "écran entier"], &["screenshot", "full screen"]),
        ),
        a(
            "capture-window",
            CAPTURES,
            w("Capturer la fenêtre", "Capture the window"),
            &["Alt+Print"],
            (&["capture"], &["screenshot"]),
        ),
        a(
            "record",
            CAPTURES,
            w("Enregistrer l’écran", "Record the screen"),
            &["Super+Shift+R"],
            (
                &["capture vidéo", "vidéo", "enregistrement"],
                &["screen recording", "video"],
            ),
        ),
        a(
            "lock",
            SESSION,
            w("Verrouiller la session", "Lock the session"),
            &["Super+L"],
            (&["verrou", "mot de passe"], &["lock"]),
        ),
        a(
            "log-out",
            SESSION,
            w("Se déconnecter", "Log out"),
            &["Super+Shift+E"],
            (&["quitter la session"], &["log off", "sign out"]),
        ),
        a(
            "volume-up",
            MEDIA,
            w("Monter le son", "Volume up"),
            &["XF86AudioRaiseVolume"],
            (&["volume", "son"], &["volume", "sound"]),
        ),
        a(
            "volume-down",
            MEDIA,
            w("Baisser le son", "Volume down"),
            &["XF86AudioLowerVolume"],
            (&["volume", "son"], &["volume", "sound"]),
        ),
        a(
            "mute",
            MEDIA,
            w("Couper le son", "Mute"),
            &["XF86AudioMute"],
            (&["muet", "silence"], &["mute", "silence"]),
        ),
        a(
            "brightness-up",
            MEDIA,
            w("Plus de luminosité", "Brightness up"),
            &["XF86MonBrightnessUp"],
            (&["luminosité", "écran"], &["brightness"]),
        ),
        a(
            "brightness-down",
            MEDIA,
            w("Moins de luminosité", "Brightness down"),
            &["XF86MonBrightnessDown"],
            (&["luminosité", "écran"], &["brightness"]),
        ),
        // The magnifier (#424).
        a(
            "magnifier-in",
            MAGNIFIER,
            w("Zoomer", "Zoom in"),
            &["Super+equal", "Super+plus", "Super+KP_Add"],
            (
                &["loupe", "grossir", "agrandir", "malvoyant"],
                &["magnifier", "magnify", "zoom", "low vision"],
            ),
        ),
        a(
            "magnifier-out",
            MAGNIFIER,
            w("Dézoomer", "Zoom out"),
            &["Super+minus", "Super+KP_Subtract"],
            (&["loupe", "réduire"], &["magnifier", "zoom"]),
        ),
        a(
            "magnifier-off",
            MAGNIFIER,
            w("Éteindre la loupe", "Turn the magnifier off"),
            &["Super+Escape"],
            (&["loupe", "zoom"], &["magnifier", "zoom"]),
        ),
        // The screen reader (#734).
        a(
            "screen-reader",
            SCREEN_READER,
            w(
                "Lancer ou arrêter le lecteur d'écran",
                "Start or stop the screen reader",
            ),
            &["Super+Alt+S"],
            (
                &["lecteur d'écran", "orca", "aveugle", "voix"],
                &["screen reader", "orca", "blind", "speech"],
            ),
        ),
    ]);
    list
}

fn place(key: &'static [&'static str], codec: Codec, default: Value) -> Places {
    Places::One(Place {
        key,
        codec,
        default,
    })
}

fn word(fr: &'static str, en: &'static str, word: &'static str) -> Choice {
    Choice {
        label: w(fr, en),
        value: Value::Word(word),
    }
}

/// The section's settings: the keyboard's, then every shortcut.
/// The page's rows the model does not hold, for the search and the
/// other pages' links: the desktop's shortcuts' row (6.14's link).
pub fn outside() -> &'static [crate::model::Outside] {
    const ALL: [crate::model::Outside; 1] = [crate::model::Outside {
        id: crate::desktops::SHORTCUTS,
        section: SectionId::Keyboard,
        group: w("RACCOURCIS", "SHORTCUTS"),
        name: w("Raccourcis du bureau", "Desktop shortcuts"),
        keywords: (
            &["raccourcis", "touches", "combinaisons"],
            &["shortcuts", "keys", "bindings"],
        ),
    }];
    &ALL
}

pub fn settings() -> &'static [Setting] {
    static SETTINGS: OnceLock<Vec<Setting>> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        let setting = |id, group, name, line, keywords, kind, places| Setting {
            id,
            section: SectionId::Keyboard,
            group,
            name,
            line,
            keywords,
            kind,
            places,
            needs: None,
        };
        let mut list = vec![
            setting(
                "keyboard.layouts",
                LAYOUTS,
                w("Dispositions", "Layouts"),
                None,
                (
                    &[
                        "disposition",
                        "azerty",
                        "qwerty",
                        "bépo",
                        "langue",
                        "clavier",
                    ],
                    &["layout", "azerty", "qwerty", "dvorak", "keyboard"],
                ),
                Kind::Layouts,
                place(
                    &["keyboard", "layouts"],
                    Codec::Texts,
                    Value::List(Vec::new()),
                ),
            ),
            setting(
                "keyboard.switch",
                LAYOUTS,
                w("Changer de disposition", "Switch layout"),
                Some(w(
                    "Un clic sur la ligne de la barre passe à la suivante.",
                    "A click on the bar's line goes to the next.",
                )),
                (
                    &["disposition", "basculer", "groupe"],
                    &["layout", "switch", "group"],
                ),
                Kind::Menu(vec![
                    word("Aucun raccourci", "No shortcut", ""),
                    word("Alt + Maj", "Alt + Shift", "grp:alt_shift_toggle"),
                    word("Ctrl + Maj", "Ctrl + Shift", "grp:ctrl_shift_toggle"),
                    word("Les deux Maj", "Both Shift keys", "grp:shifts_toggle"),
                    word("Super + Espace", "Super + Space", "grp:win_space_toggle"),
                ]),
                place(
                    &["keyboard", "options"],
                    Codec::XkbOption(SWITCHES),
                    Value::Word(""),
                ),
            ),
            setting(
                "keyboard.repeat-delay",
                REPEAT,
                w("Délai avant répétition", "Repeat delay"),
                None,
                (
                    &["répétition", "délai", "touche maintenue"],
                    &["repeat", "delay", "held key"],
                ),
                Kind::Slider {
                    min: 100,
                    max: 1000,
                    step: 50,
                    unit: " ms",
                },
                place(
                    &["keyboard", "repeat_delay"],
                    Codec::Int {
                        min: 100,
                        max: 1000,
                    },
                    Value::Int(200),
                ),
            ),
            setting(
                "keyboard.repeat-rate",
                REPEAT,
                w("Vitesse de répétition", "Repeat rate"),
                None,
                (&["répétition", "vitesse"], &["repeat", "rate", "speed"]),
                Kind::Slider {
                    min: 5,
                    max: 60,
                    step: 1,
                    unit: " /s",
                },
                place(
                    &["keyboard", "repeat_rate"],
                    Codec::Int { min: 5, max: 60 },
                    Value::Int(25),
                ),
            ),
            setting(
                "keyboard.caps",
                SPECIAL,
                w("Verr. Maj.", "Caps Lock"),
                None,
                (
                    &["majuscules", "verrouillage", "caps"],
                    &["caps lock", "capitals"],
                ),
                Kind::Menu(vec![
                    word("Comme d’habitude", "As usual", ""),
                    word("Comme Ctrl", "As Ctrl", "caps:ctrl_modifier"),
                    word("Comme Échap", "As Escape", "caps:escape"),
                ]),
                place(
                    &["keyboard", "options"],
                    Codec::XkbOption(CAPS),
                    Value::Word(""),
                ),
            ),
            setting(
                "keyboard.compose",
                SPECIAL,
                w("Touche Compose", "Compose key"),
                Some(w(
                    "Puis deux touches : ' et e font é, o et e font œ.",
                    "Then two keys: ' and e make é, o and e make œ.",
                )),
                (
                    &["compose", "accents", "caractères"],
                    &["compose", "accents", "characters"],
                ),
                Kind::Menu(vec![
                    word("Aucune", "None", ""),
                    word("Alt droite", "Right Alt", "compose:ralt"),
                    word("Menu", "Menu", "compose:menu"),
                    word("Ctrl droite", "Right Ctrl", "compose:rctrl"),
                    word("Verr. Maj.", "Caps Lock", "compose:caps"),
                ]),
                place(
                    &["keyboard", "options"],
                    Codec::XkbOption(COMPOSE),
                    Value::Word(""),
                ),
            ),
            setting(
                "keyboard.numlock",
                SPECIAL,
                w("Verr. Num. à l’ouverture", "Num Lock on at login"),
                Some(w(
                    "Le pavé numérique tape des chiffres dès l’ouverture de la session.",
                    "The keypad types digits as soon as the session opens.",
                )),
                (
                    &["pavé numérique", "chiffres", "verr num"],
                    &["num lock", "keypad", "digits"],
                ),
                Kind::Switch,
                place(&["keyboard", "numlock"], Codec::Bool, Value::Bool(false)),
            ),
        ];
        for action in actions() {
            let id: &'static str = Box::leak(format!("shortcuts.{}", action.key).into_boxed_str());
            let key: &'static [&'static str] = Box::leak(Box::new(["shortcuts", action.key]));
            list.push(Setting {
                id,
                section: SectionId::Keyboard,
                group: action.group,
                name: action.name,
                line: None,
                keywords: (
                    leak_list(action.near.0, &["raccourci", "touches"]),
                    leak_list(action.near.1, &["shortcut", "keys"]),
                ),
                kind: Kind::Shortcut,
                places: place(
                    key,
                    Codec::Chords,
                    Value::List(action.defaults.iter().map(|s| (*s).to_owned()).collect()),
                ),
                needs: None,
            });
        }
        list
    })
}

fn leak_list(a: &[&'static str], b: &[&'static str]) -> &'static [&'static str] {
    Box::leak(
        a.iter()
            .chain(b)
            .copied()
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
}

/// The shortcuts, in the sub-page's order.
pub fn shortcuts() -> impl Iterator<Item = &'static Setting> {
    settings().iter().filter(|s| s.kind == Kind::Shortcut)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_of_the_compositor_is_here() {
        let all: Vec<&Setting> = shortcuts().collect();
        assert_eq!(all.len(), 73);
        for group in SHORTCUT_GROUPS {
            assert!(all.iter().any(|s| s.group == group));
        }
        let count = |g| all.iter().filter(|s| s.group == g).count();
        assert_eq!(count(WINDOWS), 16);
        assert_eq!(count(DESKTOPS), 34);
        assert_eq!(count(BAR), 8);
        assert_eq!(count(CAPTURES), 4);
        assert_eq!(count(SESSION), 2);
        assert_eq!(count(MEDIA), 5);
        assert_eq!(count(MAGNIFIER), 3);
        assert_eq!(count(SCREEN_READER), 1);
        // The shipped file's keys, every one.
        let shipped = include_str!("../../../packaging/config.toml");
        for setting in &all {
            let key = setting.id.trim_start_matches("shortcuts.");
            assert!(
                shipped.contains(&format!("# {key} = ")),
                "{key} is not in packaging/config.toml"
            );
        }
        // Each one is an action of the palette's (#736), but the
        // palette's own.
        for setting in &all {
            let key = setting.id.trim_start_matches("shortcuts.");
            assert!(
                key == "actions" || hocus_link::actions::of_shortcut(key).is_some(),
                "{key} is not in the palette's catalogue"
            );
        }
        assert_eq!(
            family_of("shortcuts.send-to-desktop-4").map(|f| f.prefix),
            Some("send-to-desktop-")
        );
        assert_eq!(family_of("shortcuts.desktop-left"), None);
        for setting in settings() {
            for place in setting.places.all() {
                assert!(
                    place.codec.write(&place.default).is_some(),
                    "{}",
                    setting.id
                );
            }
        }
    }
}
