//! Apparence et langue / Appearance and language (6.2, issue #402, variant
//! A): the theme and the accent, the text's size, the cursor, motion and
//! legibility, the language and the formats of the whole desktop.
//!
//! The compositor follows `[appearance]` and `[locale]` as the file
//! changes: the shell draws in the theme, the accent and the formats, and
//! speaks the language; applications are told the theme through the
//! portal; the cursor changes at once. The window draws in the theme and
//! speaks the language too, as soon as the file says them: it is the
//! page's own preview. `text-scale` sizes the Settings window.

use std::{path::PathBuf, sync::OnceLock};

use hocus_link::theme::Accent;

use crate::{
    lang::{Lang, w},
    model::{Choice, Codec, Kind, Place, Places, RowGroup, SectionId, Setting, Value},
};

const THEME: RowGroup = RowGroup {
    label: w("THÈME ET COULEUR", "THEME AND COLOUR"),
    per_source: false,
};
const TEXT: RowGroup = RowGroup {
    label: w("TEXTE", "TEXT"),
    per_source: false,
};
const CURSOR: RowGroup = RowGroup {
    label: w("CURSEUR", "CURSOR"),
    per_source: false,
};
const MOTION: RowGroup = RowGroup {
    label: w("MOUVEMENT ET LISIBILITÉ", "MOTION AND LEGIBILITY"),
    per_source: false,
};
const LANGUAGE: RowGroup = RowGroup {
    label: w("LANGUE ET FORMATS", "LANGUAGE AND FORMATS"),
    per_source: false,
};

const SCHEMES: &[&str] = &["dark", "light"];
const ACCENTS: &[&str] = &["green", "blue", "violet", "rose", "teal", "wallpaper"];
const LANGUAGES: &[&str] = &["system", "fr", "en"];
const DATES: &[&str] = &["long", "short", "numeric", "iso"];
const TIMES: &[&str] = &["24h", "12h"];
/// The cursor's sizes offered; the compositor takes 8 to 256.
pub const CURSOR_SIZES: [i64; 4] = [24, 32, 48, 64];

fn word(fr: &'static str, en: &'static str, word: &'static str) -> Choice {
    Choice {
        label: w(fr, en),
        value: Value::Word(word),
    }
}

fn one(key: &'static [&'static str], codec: Codec, default: Value) -> Places {
    Places::One(Place {
        key,
        codec,
        default,
    })
}

/// The accent the file names, the default's when it names none.
pub fn accent_of(value: Option<&Value>) -> Accent {
    match value {
        Some(Value::Word(word)) => Accent::of(word).unwrap_or_default(),
        _ => Accent::default(),
    }
}

/// The hours the session's locale writes when the file says none: 12 in
/// the English of the countries that write it so (as the shell decides).
fn locale_hours() -> &'static str {
    let locale = ["LC_ALL", "LC_TIME", "LANG"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty())
        .unwrap_or_default();
    let name = locale.split(['.', '@']).next().unwrap_or("");
    match name {
        "en_US" | "en_CA" | "en_AU" | "en_NZ" | "en_PH" | "en_IN" => "12h",
        _ => "24h",
    }
}

/// The Xcursor themes installed, by their folders' names: read once as
/// the program starts, before the window ([`find_cursor_themes`]).
static CURSOR_THEMES: OnceLock<Vec<&'static str>> = OnceLock::new();

/// Looks for the installed cursor themes, where Xcursor does
/// (`XCURSOR_PATH`, else `~/.local/share/icons`, `~/.icons`, the data
/// directories' `icons`, `/usr/share/pixmaps`): a folder with `cursors`
/// in it. Before the window opens: it reads folders.
pub fn find_cursor_themes() {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let dirs: Vec<PathBuf> = match std::env::var("XCURSOR_PATH") {
        Ok(path) if !path.trim().is_empty() => path
            .split(':')
            .map(|p| match (p.strip_prefix("~/"), &home) {
                (Some(rest), Some(home)) => home.join(rest),
                _ => PathBuf::from(p),
            })
            .collect(),
        _ => {
            let data = std::env::var("XDG_DATA_DIRS")
                .ok()
                .filter(|d| !d.trim().is_empty())
                .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
            let mut dirs: Vec<PathBuf> = Vec::new();
            if let Some(home) = &home {
                dirs.push(home.join(".local/share/icons"));
                dirs.push(home.join(".icons"));
            }
            dirs.extend(data.split(':').map(|d| PathBuf::from(d).join("icons")));
            dirs.push(PathBuf::from("/usr/share/pixmaps"));
            dirs
        }
    };
    let mut names: Vec<String> = dirs
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flatten()
        .flatten()
        .filter(|entry| entry.path().join("cursors").is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name != "default")
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    let leaked = names
        .into_iter()
        .map(|n| &*Box::leak(n.into_boxed_str()))
        .collect();
    let _ = CURSOR_THEMES.set(leaked);
}

/// The page's settings, in its order.
pub fn settings() -> &'static [Setting] {
    static SETTINGS: OnceLock<Vec<Setting>> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        let mut cursor_themes = vec![Choice {
            label: w("Celui du système", "The system's"),
            value: Value::Text(String::new()),
        }];
        cursor_themes.extend(
            CURSOR_THEMES
                .get()
                .into_iter()
                .flatten()
                .map(|name| Choice {
                    label: w(name, name),
                    value: Value::Text((*name).to_owned()),
                }),
        );
        let size = |n: i64| Choice {
            label: match n {
                24 => w("24", "24"),
                32 => w("32", "32"),
                48 => w("48", "48"),
                _ => w("64", "64"),
            },
            value: Value::Int(n),
        };
        vec![
            Setting {
                id: "appearance.theme",
                section: SectionId::Appearance,
                group: THEME,
                name: w("Thème", "Theme"),
                line: Some(w(
                    "La barre, les menus, le lanceur et les Réglages ; les applications le savent aussi.",
                    "The bar, menus, launcher and Settings; applications are told too.",
                )),
                keywords: (
                    &["sombre", "clair", "nuit", "mode sombre", "thème"],
                    &["dark", "light", "night", "dark mode", "theme"],
                ),
                kind: Kind::Segmented(vec![
                    word("Sombre", "Dark", "dark"),
                    word("Clair", "Light", "light"),
                ]),
                places: one(
                    &["appearance", "theme"],
                    Codec::Word(SCHEMES),
                    Value::Word("dark"),
                ),
                needs: None,
            },
            Setting {
                id: "appearance.accent",
                section: SectionId::Appearance,
                group: THEME,
                name: w("Couleur d’accent", "Accent colour"),
                line: Some(w(
                    "Les interrupteurs, la sélection, l’anneau du clavier.",
                    "Switches, selection, the keyboard’s ring.",
                )),
                keywords: (
                    &["couleur", "teinte", "vert", "bleu", "violet", "rose", "accent"],
                    &["colour", "color", "hue", "highlight", "green", "blue", "accent"],
                ),
                kind: Kind::Swatches,
                places: one(
                    &["appearance", "accent"],
                    Codec::Word(ACCENTS),
                    Value::Word("green"),
                ),
                needs: None,
            },
            Setting {
                id: "appearance.text-scale",
                section: SectionId::Appearance,
                group: TEXT,
                name: w("Taille du texte", "Text size"),
                line: Some(w(
                    "La fenêtre des Réglages, sans changer l’échelle de l’écran ; la barre garde ses tailles.",
                    "The Settings window, without changing the screen’s scale; the bar keeps its sizes.",
                )),
                keywords: (
                    &["police", "grand", "petit", "zoom", "agrandir", "taille"],
                    &["font", "big", "small", "zoom", "larger", "text size"],
                ),
                kind: Kind::Slider {
                    min: 80,
                    max: 150,
                    step: 10,
                    unit: " %",
                },
                places: one(
                    &["appearance", "text-scale"],
                    Codec::Hundredths { min: 80, max: 150 },
                    Value::Int(100),
                ),
                needs: None,
            },
            Setting {
                id: "appearance.cursor-theme",
                section: SectionId::Appearance,
                group: CURSOR,
                name: w("Thème du curseur", "Cursor theme"),
                line: Some(w(
                    "Les programmes que le bureau lance prennent le même.",
                    "Programs the desktop starts take the same.",
                )),
                keywords: (
                    &["pointeur", "souris", "flèche", "curseur"],
                    &["pointer", "mouse", "arrow", "cursor"],
                ),
                kind: Kind::Menu(cursor_themes),
                places: one(
                    &["appearance", "cursor-theme"],
                    Codec::Text,
                    Value::Text(String::new()),
                ),
                needs: None,
            },
            Setting {
                id: "appearance.cursor-size",
                section: SectionId::Appearance,
                group: CURSOR,
                name: w("Taille du curseur", "Cursor size"),
                line: Some(w(
                    "En pixels, à l’échelle de chaque écran.",
                    "In pixels, at each screen’s scale.",
                )),
                keywords: (
                    &["pointeur", "souris", "flèche", "grand curseur"],
                    &["pointer", "mouse", "arrow", "big cursor"],
                ),
                kind: Kind::Segmented(CURSOR_SIZES.map(size).to_vec()),
                places: one(
                    &["appearance", "cursor-size"],
                    Codec::Int { min: 8, max: 256 },
                    Value::Int(24),
                ),
                needs: None,
            },
            Setting {
                id: "appearance.reduced-motion",
                section: SectionId::Appearance,
                group: MOTION,
                name: w("Mouvements réduits", "Reduced motion"),
                line: Some(w(
                    "Les fenêtres et les bureaux se fondent au lieu de bouger ; rien n’ondule.",
                    "Windows and desktops fade instead of moving; nothing wobbles.",
                )),
                keywords: (
                    &["animation", "onduler", "mouvement"],
                    &["animation", "motion", "wobbly", "reduce motion"],
                ),
                kind: Kind::Switch,
                places: one(
                    &["appearance", "reduced-motion"],
                    Codec::Bool,
                    Value::Bool(false),
                ),
                needs: None,
            },
            Setting {
                id: "appearance.reduce-transparency",
                section: SectionId::Appearance,
                group: MOTION,
                name: w("Transparence réduite", "Reduced transparency"),
                line: Some(w(
                    "La barre devient opaque au lieu de laisser voir le fond.",
                    "The bar turns opaque instead of showing what is behind it.",
                )),
                keywords: (
                    &["verre", "flou", "opacité", "transparence"],
                    &["glass", "blur", "opacity", "transparency"],
                ),
                kind: Kind::Switch,
                places: one(
                    &["appearance", "reduce-transparency"],
                    Codec::Bool,
                    Value::Bool(false),
                ),
                needs: None,
            },
            Setting {
                id: "appearance.high-contrast",
                section: SectionId::Appearance,
                group: MOTION,
                name: w("Contraste élevé", "High contrast"),
                line: Some(w(
                    "En partie : le bord de la fenêtre active passe à 2 px ; les applications le savent.",
                    "Partly: the focused window’s rim is 2 px; applications are told.",
                )),
                keywords: (
                    &["contraste", "malvoyant", "accessibilité"],
                    &["contrast", "low vision", "accessibility"],
                ),
                kind: Kind::Switch,
                places: one(
                    &["appearance", "high-contrast"],
                    Codec::Bool,
                    Value::Bool(false),
                ),
                needs: None,
            },
            Setting {
                id: "locale.language",
                section: SectionId::Appearance,
                group: LANGUAGE,
                name: w("Langue", "Language"),
                line: Some(w(
                    "Tout le bureau change aussitôt ; une application déjà ouverte, à son prochain démarrage.",
                    "The whole desktop changes at once; an application already open, when it next starts.",
                )),
                keywords: (
                    &["anglais", "français", "traduction", "langue"],
                    &["english", "french", "translation", "locale", "language"],
                ),
                kind: Kind::Menu(vec![
                    word("Celle du système", "The system’s", "system"),
                    word("Français", "Français", "fr"),
                    word("English", "English", "en"),
                ]),
                places: one(
                    &["locale", "language"],
                    Codec::Word(LANGUAGES),
                    Value::Word("system"),
                ),
                needs: None,
            },
            Setting {
                id: "locale.date",
                section: SectionId::Appearance,
                group: LANGUAGE,
                name: w("Date", "Date"),
                line: Some(w(
                    "Dans la barre, sur l’écran verrouillé et dans le calendrier.",
                    "In the bar, on the lock screen and in the calendar.",
                )),
                keywords: (
                    &["format", "jour", "mois", "jj/mm"],
                    &["format", "day", "month", "dd/mm"],
                ),
                kind: Kind::Menu(vec![
                    word("En toutes lettres", "In words", "long"),
                    word("Abrégée", "Short", "short"),
                    word("En chiffres", "In numbers", "numeric"),
                    word("Année d’abord", "Year first", "iso"),
                ]),
                places: one(
                    &["locale", "date"],
                    Codec::Word(DATES),
                    Value::Word("long"),
                ),
                needs: None,
            },
            Setting {
                id: "locale.time",
                section: SectionId::Appearance,
                group: LANGUAGE,
                name: w("Heure", "Time"),
                line: Some(w(
                    "Sur 24 heures, ou sur 12 avec le matin et l’après-midi.",
                    "Over 24 hours, or 12 with AM and PM.",
                )),
                keywords: (
                    &["12 h", "24 h", "am", "pm", "format de l’heure"],
                    &["12 h", "24 h", "am", "pm", "time format"],
                ),
                kind: Kind::Segmented(vec![
                    word("14:05", "14:05", "24h"),
                    word("2:05 PM", "2:05 PM", "12h"),
                ]),
                places: one(
                    &["locale", "time"],
                    Codec::Word(TIMES),
                    Value::Word(locale_hours()),
                ),
                needs: None,
            },
            Setting {
                id: "appearance.bar-clock",
                section: SectionId::Appearance,
                group: LANGUAGE,
                name: w("L’horloge de la barre", "The bar’s clock"),
                line: Some(w(
                    "Sa police, sa couleur et ses secondes.",
                    "Its font, colour and seconds.",
                )),
                keywords: (&["horloge", "secondes"], &["clock", "seconds"]),
                kind: Kind::Link(SectionId::Bar),
                places: Places::Page(Place {
                    key: &["panel", "clock"],
                    codec: Codec::Text,
                    default: Value::Text(String::new()),
                }),
                needs: None,
            },
        ]
    })
}

/// Today as `[locale] date` writes it, for the date's menu: "jeudi 5
/// novembre", "jeu. 5 nov.", "05/11/2026", "2026-11-05".
pub fn today_as(format: &str, today: jiff::civil::Date, lang: Lang) -> String {
    let (year, month, day) = (today.year(), today.month(), today.day());
    let fr = lang.fr();
    let weekday = usize::from(today.weekday().to_monday_zero_offset().unsigned_abs());
    let month_i = usize::from(month.unsigned_abs().clamp(1, 12)) - 1;
    match format {
        "short" => {
            let days = if fr {
                ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."]
            } else {
                ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
            };
            let months = if fr {
                [
                    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.",
                    "oct.", "nov.", "déc.",
                ]
            } else {
                [
                    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov",
                    "Dec",
                ]
            };
            format!("{} {day} {}", days[weekday], months[month_i])
        }
        "numeric" => format!("{day:02}/{month:02}/{year}"),
        "iso" => format!("{year}-{month:02}-{day:02}"),
        _ => {
            let days = if fr {
                [
                    "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
                ]
            } else {
                [
                    "Monday",
                    "Tuesday",
                    "Wednesday",
                    "Thursday",
                    "Friday",
                    "Saturday",
                    "Sunday",
                ]
            };
            let months = if fr {
                [
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
                ]
            } else {
                [
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
                ]
            };
            format!("{} {day} {}", days[weekday], months[month_i])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_has_its_five_groups_in_order() {
        let mut groups: Vec<&str> = settings().iter().map(|s| s.group.label.fr).collect();
        groups.dedup();
        assert_eq!(
            groups,
            [
                "THÈME ET COULEUR",
                "TEXTE",
                "CURSEUR",
                "MOUVEMENT ET LISIBILITÉ",
                "LANGUE ET FORMATS"
            ]
        );
    }

    #[test]
    fn today_reads_in_each_format() {
        let today = jiff::civil::date(2026, 11, 5);
        assert_eq!(today_as("long", today, Lang::French), "jeudi 5 novembre");
        assert_eq!(today_as("short", today, Lang::English), "Thu 5 Nov");
        assert_eq!(today_as("numeric", today, Lang::French), "05/11/2026");
        assert_eq!(today_as("iso", today, Lang::English), "2026-11-05");
    }
}
