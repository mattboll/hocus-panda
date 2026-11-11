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

use hocus_words::settings::keyboard as words;
use std::sync::OnceLock;

use crate::{
    lang::{Words, pair, w},
    model::{Choice, Codec, Kind, RowGroup, SectionId, Setting, Value, one},
};

pub const LAYOUTS: RowGroup = RowGroup {
    label: w(words::LAYOUTS),
    per_source: false,
};
pub const REPEAT: RowGroup = RowGroup {
    label: w(words::KEY_REPEAT),
    per_source: false,
};
pub const SPECIAL: RowGroup = RowGroup {
    label: w(words::SPECIAL_KEYS),
    per_source: false,
};
pub const WINDOWS: RowGroup = RowGroup {
    label: w(words::WINDOWS),
    per_source: false,
};
pub const DESKTOPS: RowGroup = RowGroup {
    label: w(words::DESKTOPS),
    per_source: false,
};
pub const BAR: RowGroup = RowGroup {
    label: w(words::BAR_AND_LAUNCHER),
    per_source: false,
};
pub const CAPTURES: RowGroup = RowGroup {
    label: w(words::CAPTURES),
    per_source: false,
};
pub const SESSION: RowGroup = RowGroup {
    label: w(words::SESSION),
    per_source: false,
};
pub const MEDIA: RowGroup = RowGroup {
    label: w(words::MEDIA_KEYS),
    per_source: false,
};

/// The shortcuts' groups, in the sub-page's order.
pub const MAGNIFIER: RowGroup = RowGroup {
    label: w(words::MAGNIFIER),
    per_source: false,
};

/// The screen reader's shortcut (#734).
pub const SCREEN_READER: RowGroup = RowGroup {
    label: w(words::SCREEN_READER),
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
        name: w(words::GO_TO_DESKTOP_1_TO_9),
    },
    Family {
        prefix: "send-to-desktop-",
        name: w(words::SEND_TO_DESKTOP_1_TO_9),
    },
    Family {
        prefix: "take-to-desktop-",
        name: w(words::TAKE_TO_DESKTOP_1_TO_9),
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

/// One action of `[shortcuts]`: its key, its group, its names, a few
/// nearby words (its defaults are the configuration's).
struct Action {
    key: &'static str,
    group: RowGroup,
    name: Words,
    near: (&'static [&'static str], &'static [&'static str]),
}

const fn a(
    key: &'static str,
    group: RowGroup,
    name: Words,
    near: (&'static [&'static str], &'static [&'static str]),
) -> Action {
    Action {
        key,
        group,
        name,
        near,
    }
}

// Over 200 lines: a table, one entry per action that takes a shortcut.
fn actions() -> Vec<Action> {
    let mut list = vec![
        a(
            "close",
            WINDOWS,
            w(words::CLOSE_THE_WINDOW),
            (&["quitter"], &["quit"]),
        ),
        a(
            "minimize",
            WINDOWS,
            w(words::MINIMISE_THE_WINDOW),
            (&["cacher", "minimiser"], &["hide", "minimize"]),
        ),
        a(
            "snap-left",
            WINDOWS,
            w(words::LEFT_HALF),
            (&["ancrer", "côte à côte"], &["snap", "tile"]),
        ),
        a(
            "snap-right",
            WINDOWS,
            w(words::RIGHT_HALF),
            (&["ancrer", "côte à côte"], &["snap", "tile"]),
        ),
        a(
            "maximize",
            WINDOWS,
            w(words::MAXIMISE),
            (&["plein écran", "maximiser"], &["maximize", "full"]),
        ),
        a(
            "restore",
            WINDOWS,
            w(words::RESTORE),
            (&["réduire"], &["unmaximize"]),
        ),
        a(
            "quarter-up",
            WINDOWS,
            w(words::TOP_QUARTER),
            (&["ancrer", "coin"], &["snap", "corner"]),
        ),
        a(
            "quarter-down",
            WINDOWS,
            w(words::BOTTOM_QUARTER),
            (&["ancrer", "coin"], &["snap", "corner"]),
        ),
        a(
            "quarter-left",
            WINDOWS,
            w(words::LEFT_QUARTER),
            (&["ancrer", "coin"], &["snap", "corner"]),
        ),
        a(
            "quarter-right",
            WINDOWS,
            w(words::RIGHT_QUARTER),
            (&["ancrer", "coin"], &["snap", "corner"]),
        ),
        a(
            "focus-left",
            WINDOWS,
            w(words::WINDOW_ON_THE_LEFT),
            (&["focus", "aller"], &["focus"]),
        ),
        a(
            "focus-right",
            WINDOWS,
            w(words::WINDOW_ON_THE_RIGHT),
            (&["focus", "aller"], &["focus"]),
        ),
        a(
            "focus-up",
            WINDOWS,
            w(words::WINDOW_ABOVE),
            (&["focus", "aller"], &["focus"]),
        ),
        a(
            "focus-down",
            WINDOWS,
            w(words::WINDOW_BELOW),
            (&["focus", "aller"], &["focus"]),
        ),
        a(
            "switch-windows",
            WINDOWS,
            w(words::SWITCH_WINDOWS),
            (&["alt tab", "basculer"], &["alt tab", "switcher"]),
        ),
        a(
            "switch-windows-backward",
            WINDOWS,
            w(words::SWITCH_WINDOWS_BACKWARD),
            (&["alt tab", "basculer"], &["alt tab", "switcher"]),
        ),
        // « geste »: what three fingers sideways do, so the search for
        // the touchpad's gestures lists them after its rows (#681).
        a(
            "desktop-left",
            DESKTOPS,
            w(words::DESKTOP_ON_THE_LEFT),
            (&["espace de travail", "geste"], &["workspace", "gesture"]),
        ),
        a(
            "desktop-right",
            DESKTOPS,
            w(words::DESKTOP_ON_THE_RIGHT),
            (&["espace de travail", "geste"], &["workspace", "gesture"]),
        ),
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
    for (keys, words) in [
        (0, ("Aller au bureau", "Go to desktop")),
        (1, ("Envoyer au bureau", "Send to desktop")),
        (2, ("Emmener au bureau", "Take to desktop")),
    ] {
        for numbered in NUMBERED {
            let key = [numbered.0, numbered.1, numbered.2][keys];
            let n = numbered.3;
            list.push(a(
                key,
                DESKTOPS,
                pair(
                    leak(format!("{} {n}", words.0)),
                    leak(format!("{} {n}", words.1)),
                ),
                (&["espace de travail"], &["workspace"]),
            ));
        }
    }
    list.extend([
        a(
            "take-to-desktop-left",
            DESKTOPS,
            w(words::TAKE_TO_THE_DESKTOP_ON_THE),
            (&["déplacer"], &["move"]),
        ),
        a(
            "take-to-desktop-right",
            DESKTOPS,
            w(words::TAKE_TO_THE_DESKTOP_ON_THE_2),
            (&["déplacer"], &["move"]),
        ),
        // Nested, the compositor gives these no default: `Super+Alt` is
        // already its `Super` there (8.4).
        a(
            "move-to-screen-left",
            DESKTOPS,
            w(words::MOVE_TO_THE_SCREEN_ON_THE),
            (&["déplacer", "moniteur"], &["move", "monitor", "display"]),
        ),
        a(
            "move-to-screen-right",
            DESKTOPS,
            w(words::MOVE_TO_THE_SCREEN_ON_THE_2),
            (&["déplacer", "moniteur"], &["move", "monitor", "display"]),
        ),
        a(
            "overview",
            DESKTOPS,
            w(words::OVERVIEW),
            (
                &["exposé", "bureaux", "fenêtres", "tout voir"],
                &["expose", "desktops", "windows", "workspaces"],
            ),
        ),
        a(
            "terminal",
            BAR,
            w(words::OPEN_A_TERMINAL),
            (&["console", "shell"], &["console", "shell"]),
        ),
        a(
            "launcher",
            BAR,
            w(words::OPEN_THE_LAUNCHER),
            (&["applications", "chercher"], &["apps", "search"]),
        ),
        a(
            "actions",
            BAR,
            w(words::ACTIONS),
            (
                &["palette", "commandes", "tout faire", "chercher une action"],
                &["palette", "commands", "command palette", "search actions"],
            ),
        ),
        a(
            "focus-bar",
            BAR,
            w(words::GO_TO_THE_BAR),
            (
                &["clavier", "barre", "accessibilité", "focus"],
                &["keyboard", "panel", "accessibility", "focus"],
            ),
        ),
        a(
            "toggle-bar",
            BAR,
            w(words::CHANGE_THE_BAR_S_FORM),
            (&["mini", "barre"], &["panel", "bar"]),
        ),
        a(
            "clipboard",
            BAR,
            w(words::CLIPBOARD_HISTORY),
            (&["copier", "coller"], &["copy", "paste"]),
        ),
        a(
            "emoji",
            BAR,
            w(words::EMOJI),
            (&["émoticône", "smiley"], &["emoji", "smiley"]),
        ),
        a(
            "join",
            BAR,
            w(words::JOIN_THE_MEETING),
            (&["visio", "appel"], &["call", "video"]),
        ),
        a(
            "capture",
            CAPTURES,
            w(words::SCREENSHOT),
            (&["capture", "copie d’écran"], &["screenshot", "capture"]),
        ),
        a(
            "capture-screen",
            CAPTURES,
            w(words::CAPTURE_THE_SCREEN),
            (&["capture", "écran entier"], &["screenshot", "full screen"]),
        ),
        a(
            "capture-window",
            CAPTURES,
            w(words::CAPTURE_THE_WINDOW),
            (&["capture"], &["screenshot"]),
        ),
        a(
            "record",
            CAPTURES,
            w(words::RECORD_THE_SCREEN),
            (
                &["capture vidéo", "vidéo", "enregistrement"],
                &["screen recording", "video"],
            ),
        ),
        a(
            "lock",
            SESSION,
            w(words::LOCK_THE_SESSION),
            (&["verrou", "mot de passe"], &["lock"]),
        ),
        a(
            "log-out",
            SESSION,
            w(words::LOG_OUT),
            (&["quitter la session"], &["log off", "sign out"]),
        ),
        a(
            "volume-up",
            MEDIA,
            w(words::VOLUME_UP),
            (&["volume", "son"], &["volume", "sound"]),
        ),
        a(
            "volume-down",
            MEDIA,
            w(words::VOLUME_DOWN),
            (&["volume", "son"], &["volume", "sound"]),
        ),
        a(
            "mute",
            MEDIA,
            w(words::MUTE),
            (&["muet", "silence"], &["mute", "silence"]),
        ),
        a(
            "brightness-up",
            MEDIA,
            w(words::BRIGHTNESS_UP),
            (&["luminosité", "écran"], &["brightness"]),
        ),
        a(
            "brightness-down",
            MEDIA,
            w(words::BRIGHTNESS_DOWN),
            (&["luminosité", "écran"], &["brightness"]),
        ),
        // The magnifier (#424).
        a(
            "magnifier-in",
            MAGNIFIER,
            w(words::ZOOM_IN),
            (
                &["loupe", "grossir", "agrandir", "malvoyant"],
                &["magnifier", "magnify", "zoom", "low vision"],
            ),
        ),
        a(
            "magnifier-out",
            MAGNIFIER,
            w(words::ZOOM_OUT),
            (&["loupe", "réduire"], &["magnifier", "zoom"]),
        ),
        a(
            "magnifier-off",
            MAGNIFIER,
            w(words::TURN_THE_MAGNIFIER_OFF),
            (&["loupe", "zoom"], &["magnifier", "zoom"]),
        ),
        // The screen reader (#734).
        a(
            "screen-reader",
            SCREEN_READER,
            w(words::START_OR_STOP_THE_SCREEN_READER),
            (
                &["lecteur d'écran", "orca", "aveugle", "voix"],
                &["screen reader", "orca", "blind", "speech"],
            ),
        ),
    ]);
    list
}

fn word(text: hocus_words::Text, word: &'static str) -> Choice {
    Choice {
        label: crate::lang::w(text),
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
        group: w(words::SHORTCUTS),
        name: w(words::DESKTOP_SHORTCUTS),
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
                w(words::LAYOUTS_2),
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
                one(&["keyboard", "layouts"], Codec::Texts),
            ),
            setting(
                "keyboard.switch",
                LAYOUTS,
                w(words::SWITCH_LAYOUT),
                Some(w(words::A_CLICK_ON_THE_BAR_S)),
                (
                    &["disposition", "basculer", "groupe"],
                    &["layout", "switch", "group"],
                ),
                Kind::Menu(vec![
                    word(words::NO_SHORTCUT, ""),
                    word(words::ALT_SHIFT, "grp:alt_shift_toggle"),
                    word(words::CTRL_SHIFT, "grp:ctrl_shift_toggle"),
                    word(words::BOTH_SHIFT_KEYS, "grp:shifts_toggle"),
                    word(words::SUPER_SPACE, "grp:win_space_toggle"),
                ]),
                one(&["keyboard", "options"], Codec::XkbOption(SWITCHES)),
            ),
            setting(
                "keyboard.repeat-delay",
                REPEAT,
                w(words::REPEAT_DELAY),
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
                one(
                    &["keyboard", "repeat_delay"],
                    Codec::Int {
                        min: 100,
                        max: 1000,
                    },
                ),
            ),
            setting(
                "keyboard.repeat-rate",
                REPEAT,
                w(words::REPEAT_RATE),
                None,
                (&["répétition", "vitesse"], &["repeat", "rate", "speed"]),
                Kind::Slider {
                    min: 5,
                    max: 60,
                    step: 1,
                    unit: " /s",
                },
                one(&["keyboard", "repeat_rate"], Codec::Int { min: 5, max: 60 }),
            ),
            setting(
                "keyboard.caps",
                SPECIAL,
                w(words::CAPS_LOCK),
                None,
                (
                    &["majuscules", "verrouillage", "caps"],
                    &["caps lock", "capitals"],
                ),
                Kind::Menu(vec![
                    word(words::AS_USUAL, ""),
                    word(words::AS_CTRL, "caps:ctrl_modifier"),
                    word(words::AS_ESCAPE, "caps:escape"),
                ]),
                one(&["keyboard", "options"], Codec::XkbOption(CAPS)),
            ),
            setting(
                "keyboard.compose",
                SPECIAL,
                w(words::COMPOSE_KEY),
                Some(w(words::THEN_TWO_KEYS_AND_E_MAKE)),
                (
                    &["compose", "accents", "caractères"],
                    &["compose", "accents", "characters"],
                ),
                Kind::Menu(vec![
                    word(words::NONE, ""),
                    word(words::RIGHT_ALT, "compose:ralt"),
                    word(words::MENU, "compose:menu"),
                    word(words::RIGHT_CTRL, "compose:rctrl"),
                    word(words::CAPS_LOCK, "compose:caps"),
                ]),
                one(&["keyboard", "options"], Codec::XkbOption(COMPOSE)),
            ),
            setting(
                "keyboard.numlock",
                SPECIAL,
                w(words::NUM_LOCK_ON_AT_LOGIN),
                Some(w(words::THE_KEYPAD_TYPES_DIGITS_AS_SOON)),
                (
                    &["pavé numérique", "chiffres", "verr num"],
                    &["num lock", "keypad", "digits"],
                ),
                Kind::Switch,
                one(&["keyboard", "numlock"], Codec::Bool),
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
                places: one(key, Codec::Chords),
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
