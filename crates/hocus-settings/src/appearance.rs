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

use hocus_words::settings::appearance as words;
use std::{path::PathBuf, sync::OnceLock};

use hocus_link::theme::{Accent, Scheme};

use crate::{
    lang::Lang,
    model::{Choice, Codec, Kind, Place, Places, RowGroup, SectionId, Setting, Value, one},
};

const THEME: RowGroup = RowGroup {
    label: words::THEME_AND_COLOUR,
    per_source: false,
};
const TEXT: RowGroup = RowGroup {
    label: words::TEXT,
    per_source: false,
};
const CURSOR: RowGroup = RowGroup {
    label: words::CURSOR,
    per_source: false,
};
const MOTION: RowGroup = RowGroup {
    label: words::MOTION_AND_LEGIBILITY,
    per_source: false,
};
const LANGUAGE: RowGroup = RowGroup {
    label: words::LANGUAGE_AND_FORMATS,
    per_source: false,
};

const SCHEMES: &[&str] = &["dark", "light", "frost", "matrix"];
const ACCENTS: &[&str] = &[
    "green",
    "blue",
    "violet",
    "rose",
    "teal",
    "wallpaper",
    "ice",
    "phosphor",
];
const LANGUAGES: &[&str] = &["system", "fr", "en"];
const DATES: &[&str] = &["long", "short", "numeric", "iso"];
const TIMES: &[&str] = &["24h", "12h"];
/// The cursor's sizes offered; the compositor takes 8 to 256.
pub const CURSOR_SIZES: [i64; 4] = [24, 32, 48, 64];

fn word(text: hocus_words::Text, word: &'static str) -> Choice {
    Choice {
        label: text,
        value: Value::Word(word),
    }
}

/// The theme's setting, and the accent's.
pub const THEME_ID: &str = "appearance.theme";
pub const ACCENT_ID: &str = "appearance.accent";

/// The keys a theme's choice may change besides the theme (#431, #740):
/// the accent, the windows' motion and the wallpaper.
pub const ACCENT_KEY: [&str; 2] = ["appearance", "accent"];
pub const ANIMATION_KEY: [&str; 2] = ["windows", "animation"];
pub const PICTURE_KEY: [&str; 2] = ["wallpaper", "picture"];

/// A key a theme changes, and its word there (`None`: its line taken
/// out).
pub type KeyWord = ([&'static str; 2], Option<String>);

/// What the file says now at the keys a theme may change: each word, or
/// `None` where the file says nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Said {
    pub accent: Option<String>,
    pub animation: Option<String>,
    pub picture: Option<String>,
}

impl Said {
    fn get(&self, key: [&str; 2]) -> &Option<String> {
        match key {
            ACCENT_KEY => &self.accent,
            ANIMATION_KEY => &self.animation,
            _ => &self.picture,
        }
    }

    fn get_mut(&mut self, key: [&str; 2]) -> &mut Option<String> {
        match key {
            ACCENT_KEY => &mut self.accent,
            ANIMATION_KEY => &mut self.animation,
            _ => &mut self.picture,
        }
    }

    const KEYS: [[&str; 2]; 3] = [ACCENT_KEY, ANIMATION_KEY, PICTURE_KEY];
}

/// What a themed theme changed when it was chosen, to give back when it
/// is left: kept in `$XDG_STATE_HOME/hocuspanda/theme-before`, one line a
/// key it changed, `section.key = "word"`, or `section.key =` where the
/// file said nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Before {
    /// The keys changed, and what the file said there before.
    pub keys: Vec<KeyWord>,
}

impl Before {
    const FILE: &str = "theme-before";

    pub fn read(state: Option<&std::path::Path>) -> Self {
        let text = state
            .and_then(|s| std::fs::read_to_string(s.join(Self::FILE)).ok())
            .unwrap_or_default();
        let mut before = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let Some(key) = Said::KEYS.into_iter().find(|k| k.join(".") == key.trim()) else {
                continue;
            };
            let value = value.trim().trim_matches('"');
            let value = (!value.is_empty()).then(|| value.to_owned());
            before.keys.push((key, value));
        }
        before
    }

    pub fn write(&self, state: Option<&std::path::Path>) {
        let Some(state) = state else {
            return;
        };
        let text: String = self
            .keys
            .iter()
            .map(|(key, value)| match value {
                Some(word) => format!("{} = \"{word}\"\n", key.join(".")),
                None => format!("{} =\n", key.join(".")),
            })
            .collect();
        if let Err(err) = hocus_config::replace(&state.join(Self::FILE), text.as_bytes(), None) {
            tracing::warn!(%err, "could not keep what the theme changed");
        }
    }
}

/// What a themed theme sets: its accent, its windows' motion, and the
/// wallpaper it proposes (written only over a built-in one).
fn theme_sets(scheme: Scheme) -> Vec<([&'static str; 2], &'static str)> {
    match scheme {
        Scheme::Frost => vec![
            (ACCENT_KEY, "ice"),
            (ANIMATION_KEY, "frost"),
            (PICTURE_KEY, "aurora"),
        ],
        Scheme::Matrix => vec![
            (ACCENT_KEY, "phosphor"),
            (ANIMATION_KEY, "rain"),
            (PICTURE_KEY, "rain"),
        ],
        Scheme::Dark | Scheme::Light => Vec::new(),
    }
}

/// Whether `picture` is one the desktop ships (or none): a picture of
/// one's own is never replaced by a theme's.
fn built_in(picture: Option<&str>) -> bool {
    picture.is_none_or(|p| {
        p == "plain"
            || hocus_link::backdrop::Backdrop::of(p).is_some()
            || hocus_link::moving::Moving::of(p).is_some()
    })
}

/// What choosing theme `to` over `from` changes besides the theme (the
/// hook of #431 and #740), `now` what the file says, `before` what the
/// themed theme left had changed: the keys to write (a word, or `None`
/// to take the line out), and what to remember for leaving `to`.
///
/// Leaving a themed theme gives back each key it changed that still holds
/// its value (another chosen meanwhile stays). Entering one sets its
/// accent and its windows' motion, and its wallpaper over a built-in one,
/// remembering what each replaced. Called whenever the theme changes in
/// the window.
pub fn theme_proposes(
    from: Scheme,
    to: Scheme,
    now: &Said,
    before: &Before,
) -> (Vec<KeyWord>, Before) {
    let mut values = now.clone();
    let mut remember = Before::default();
    if from == to {
        return (Vec::new(), before.clone());
    }
    if from.themed() {
        for (key, set) in theme_sets(from) {
            if values.get(key).as_deref() != Some(set) {
                continue;
            }
            if let Some((_, was)) = before.keys.iter().find(|(k, _)| *k == key) {
                *values.get_mut(key) = was.clone();
            }
        }
    }
    for (key, set) in theme_sets(to) {
        if key == PICTURE_KEY && !built_in(values.picture.as_deref()) {
            continue;
        }
        if values.get(key).as_deref() == Some(set) {
            continue;
        }
        remember.keys.push((key, values.get(key).clone()));
        *values.get_mut(key) = Some(set.to_owned());
    }
    let edits = Said::KEYS
        .into_iter()
        .filter(|key| values.get(*key) != now.get(*key))
        .map(|key| (key, values.get(key).clone()))
        .collect();
    (edits, remember)
}

/// A themed theme's own accent.
pub const fn theme_accent(scheme: Scheme) -> Accent {
    match scheme {
        Scheme::Frost => Accent::Ice,
        Scheme::Matrix => Accent::Phosphor,
        _ => Accent::Green,
    }
}

/// The accents' swatches in `scheme`, `chosen` the accent now: the six,
/// and a themed theme's own first while it is chosen (or its accent is).
pub fn accents_shown(scheme: Scheme, chosen: Accent) -> Vec<Accent> {
    let own = scheme.themed().then(|| theme_accent(scheme));
    let mut shown: Vec<Accent> = Vec::new();
    if let Some(own) = own {
        shown.push(own);
    }
    for accent in Accent::ALL {
        let themed = matches!(accent, Accent::Ice | Accent::Phosphor);
        if !shown.contains(&accent) && (!themed || accent == chosen) {
            shown.push(accent);
        }
    }
    shown
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
// Over 200 lines: a table, one entry per setting of the page.
pub fn settings() -> &'static [Setting] {
    static SETTINGS: OnceLock<Vec<Setting>> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        let mut cursor_themes = vec![Choice {
            label: words::THE_SYSTEM_S,
            value: Value::Text(String::new()),
        }];
        cursor_themes.extend(
            CURSOR_THEMES
                .get()
                .into_iter()
                .flatten()
                .map(|name| Choice {
                    label: hocus_words::Text::same(name),
                    value: Value::Text((*name).to_owned()),
                }),
        );
        let size = |n: i64| Choice {
            label: match n {
                24 => words::N_24,
                32 => words::N_32,
                48 => words::N_48,
                _ => words::N_64,
            },
            value: Value::Int(n),
        };
        vec![
            Setting {
                id: "appearance.theme",
                section: SectionId::Appearance,
                group: THEME,
                name: words::THEME,
                line: Some(words::THE_BAR_MENUS_LAUNCHER_AND_SETTINGS),
                keywords: (
                    &[
                        "sombre",
                        "clair",
                        "nuit",
                        "mode sombre",
                        "thème",
                        "givre",
                        "glacé",
                        "matrix",
                        "phosphore",
                    ],
                    &[
                        "dark",
                        "light",
                        "night",
                        "dark mode",
                        "theme",
                        "frost",
                        "ice",
                        "matrix",
                        "phosphor",
                    ],
                ),
                kind: Kind::Segmented(vec![
                    word(words::DARK, "dark"),
                    word(words::LIGHT, "light"),
                    word(words::FROST, "frost"),
                    word(words::MATRIX, "matrix"),
                ]),
                places: one(&["appearance", "theme"], Codec::Word(SCHEMES)),
                needs: None,
            },
            Setting {
                id: "appearance.accent",
                section: SectionId::Appearance,
                group: THEME,
                name: words::ACCENT_COLOUR,
                line: Some(words::SWITCHES_SELECTION_THE_KEYBOARD_S_RING),
                keywords: (
                    &[
                        "couleur", "teinte", "vert", "bleu", "violet", "rose", "accent",
                    ],
                    &[
                        "colour",
                        "color",
                        "hue",
                        "highlight",
                        "green",
                        "blue",
                        "accent",
                    ],
                ),
                kind: Kind::Swatches,
                places: one(&["appearance", "accent"], Codec::Word(ACCENTS)),
                needs: None,
            },
            Setting {
                id: "appearance.text-scale",
                section: SectionId::Appearance,
                group: TEXT,
                name: words::TEXT_SIZE,
                line: Some(words::THE_SETTINGS_WINDOW_WITHOUT_CHANGING_THE),
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
                ),
                needs: None,
            },
            Setting {
                id: "appearance.cursor-theme",
                section: SectionId::Appearance,
                group: CURSOR,
                name: words::CURSOR_THEME,
                line: Some(words::PROGRAMS_THE_DESKTOP_STARTS_TAKE_THE),
                keywords: (
                    &["pointeur", "souris", "flèche", "curseur"],
                    &["pointer", "mouse", "arrow", "cursor"],
                ),
                kind: Kind::Menu(cursor_themes),
                places: Places::One(Place::unset(
                    &["appearance", "cursor-theme"],
                    Codec::Text,
                    Value::Text(String::new()),
                )),
                needs: None,
            },
            Setting {
                id: "appearance.cursor-size",
                section: SectionId::Appearance,
                group: CURSOR,
                name: words::CURSOR_SIZE,
                line: Some(words::IN_PIXELS_AT_EACH_SCREEN_S),
                keywords: (
                    &["pointeur", "souris", "flèche", "grand curseur"],
                    &["pointer", "mouse", "arrow", "big cursor"],
                ),
                kind: Kind::Segmented(CURSOR_SIZES.map(size).to_vec()),
                places: Places::One(Place::unset(
                    &["appearance", "cursor-size"],
                    Codec::Int { min: 8, max: 256 },
                    Value::Int(24),
                )),
                needs: None,
            },
            Setting {
                id: "appearance.accessibility",
                section: SectionId::Appearance,
                group: MOTION,
                name: words::ACCESSIBILITY,
                line: Some(words::REDUCED_MOTION_REDUCED_TRANSPARENCY_HIGH_CONTRAST),
                keywords: (
                    &[
                        "mouvement",
                        "transparence",
                        "contraste",
                        "loupe",
                        "accessibilité",
                    ],
                    &[
                        "motion",
                        "transparency",
                        "contrast",
                        "magnifier",
                        "accessibility",
                    ],
                ),
                kind: Kind::Link(SectionId::Accessibility),
                places: Places::Page(Place {
                    key: &["appearance", "accessibility"],
                    codec: Codec::Text,
                    default: Value::Text(String::new()),
                }),
                needs: None,
            },
            Setting {
                id: "locale.language",
                section: SectionId::Appearance,
                group: LANGUAGE,
                name: words::LANGUAGE,
                line: Some(words::THE_WHOLE_DESKTOP_CHANGES_AT_ONCE),
                keywords: (
                    &["anglais", "français", "traduction", "langue"],
                    &["english", "french", "translation", "locale", "language"],
                ),
                kind: Kind::Menu(vec![
                    word(words::THE_SYSTEM_S_2, "system"),
                    word(words::FRAN_AIS, "fr"),
                    word(words::ENGLISH, "en"),
                ]),
                places: one(&["locale", "language"], Codec::Word(LANGUAGES)),
                needs: None,
            },
            Setting {
                id: "locale.date",
                section: SectionId::Appearance,
                group: LANGUAGE,
                name: words::DATE,
                line: Some(words::IN_THE_BAR_ON_THE_LOCK),
                keywords: (
                    &["format", "jour", "mois", "jj/mm"],
                    &["format", "day", "month", "dd/mm"],
                ),
                kind: Kind::Menu(vec![
                    word(words::IN_WORDS, "long"),
                    word(words::SHORT, "short"),
                    word(words::IN_NUMBERS, "numeric"),
                    word(words::YEAR_FIRST, "iso"),
                ]),
                places: one(&["locale", "date"], Codec::Word(DATES)),
                needs: None,
            },
            Setting {
                id: "locale.time",
                section: SectionId::Appearance,
                group: LANGUAGE,
                name: words::TIME,
                line: Some(words::OVER_24_HOURS_OR_12_WITH),
                keywords: (
                    &["12 h", "24 h", "am", "pm", "format de l’heure"],
                    &["12 h", "24 h", "am", "pm", "time format"],
                ),
                kind: Kind::Segmented(vec![
                    word(words::N_14_05, "24h"),
                    word(words::N_2_05_PM, "12h"),
                ]),
                places: Places::One(Place::unset(
                    &["locale", "time"],
                    Codec::Word(TIMES),
                    Value::Word(locale_hours()),
                )),
                needs: None,
            },
            Setting {
                id: "appearance.bar-clock",
                section: SectionId::Appearance,
                group: LANGUAGE,
                name: words::THE_BAR_S_CLOCK,
                line: Some(words::ITS_FONT_COLOUR_AND_SECONDS),
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
    let weekday = usize::from(today.weekday().to_monday_zero_offset().unsigned_abs());
    let month_i = usize::from(month.unsigned_abs().clamp(1, 12)) - 1;
    let name =
        |list: hocus_words::List, i: usize| list.get(lang).get(i).copied().unwrap_or_default();
    match format {
        "short" => format!(
            "{} {day} {}",
            name(hocus_words::date::SHORT_WEEKDAYS, weekday),
            name(hocus_words::date::SHORT_MONTHS, month_i)
        ),
        "numeric" => format!("{day:02}/{month:02}/{year}"),
        "iso" => format!("{year}-{month:02}-{day:02}"),
        _ => format!(
            "{} {day} {}",
            name(hocus_words::date::WEEKDAYS, weekday),
            name(hocus_words::date::MONTHS, month_i)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(accent: Option<&str>, animation: Option<&str>, picture: Option<&str>) -> Said {
        Said {
            accent: accent.map(str::to_owned),
            animation: animation.map(str::to_owned),
            picture: picture.map(str::to_owned),
        }
    }

    fn set(key: [&'static str; 2], word: &str) -> ([&'static str; 2], Option<String>) {
        (key, Some(word.to_owned()))
    }

    #[test]
    fn choosing_givre_sets_its_accent_motion_and_aurora_and_leaving_gives_them_back() {
        let none = Before::default();
        // Entering Givre: the ice blue, its motion and the aurora, what
        // they replace kept.
        let now = said(Some("violet"), None, Some("mist"));
        let (edits, kept) = theme_proposes(Scheme::Dark, Scheme::Frost, &now, &none);
        assert_eq!(
            edits,
            [
                set(ACCENT_KEY, "ice"),
                set(ANIMATION_KEY, "frost"),
                set(PICTURE_KEY, "aurora"),
            ]
        );
        assert_eq!(
            kept.keys,
            [
                (ACCENT_KEY, Some("violet".to_owned())),
                (ANIMATION_KEY, None),
                (PICTURE_KEY, Some("mist".to_owned())),
            ]
        );
        // Leaving it as it set them: the violet and the mist again, the
        // motion's line taken out.
        let now = said(Some("ice"), Some("frost"), Some("aurora"));
        let (edits, kept) = theme_proposes(Scheme::Frost, Scheme::Light, &now, &kept);
        assert_eq!(
            edits,
            [
                set(ACCENT_KEY, "violet"),
                (ANIMATION_KEY, None),
                set(PICTURE_KEY, "mist"),
            ]
        );
        assert_eq!(kept, Before::default());
        // Another accent chosen while in Givre stays.
        let (_, kept) = theme_proposes(Scheme::Dark, Scheme::Frost, &said(None, None, None), &none);
        let now = said(Some("rose"), Some("frost"), Some("aurora"));
        let (edits, _) = theme_proposes(Scheme::Frost, Scheme::Dark, &now, &kept);
        assert_eq!(edits, [(ANIMATION_KEY, None), (PICTURE_KEY, None)]);
        // Between the plain themes, nothing more than the theme.
        let now = said(Some("teal"), Some("rain"), None);
        let (edits, _) = theme_proposes(Scheme::Dark, Scheme::Light, &now, &none);
        assert!(edits.is_empty());
    }

    #[test]
    fn choosing_matrix_sets_the_rain_over_a_built_in_wallpaper_only() {
        let none = Before::default();
        let now = said(None, None, Some("forest"));
        let (edits, kept) = theme_proposes(Scheme::Dark, Scheme::Matrix, &now, &none);
        assert_eq!(
            edits,
            [
                set(ACCENT_KEY, "phosphor"),
                set(ANIMATION_KEY, "rain"),
                set(PICTURE_KEY, "rain"),
            ]
        );
        // Back to dark: all three as they were.
        let now = said(Some("phosphor"), Some("rain"), Some("rain"));
        let (edits, _) = theme_proposes(Scheme::Matrix, Scheme::Dark, &now, &kept);
        assert_eq!(
            edits,
            [
                (ACCENT_KEY, None),
                (ANIMATION_KEY, None),
                set(PICTURE_KEY, "forest")
            ]
        );
        // A picture of one's own stays, and is not remembered.
        let now = said(Some("blue"), None, Some("~/Images/lac.jpg"));
        let (edits, kept) = theme_proposes(Scheme::Light, Scheme::Matrix, &now, &none);
        assert_eq!(
            edits,
            [set(ACCENT_KEY, "phosphor"), set(ANIMATION_KEY, "rain")]
        );
        assert!(kept.keys.iter().all(|(k, _)| *k != PICTURE_KEY));
        // Givre to Matrix: Givre's undone, Matrix's set, what was before
        // Givre remembered for leaving Matrix.
        let (_, kept) = theme_proposes(
            Scheme::Dark,
            Scheme::Frost,
            &said(Some("violet"), None, None),
            &none,
        );
        let now = said(Some("ice"), Some("frost"), Some("aurora"));
        let (edits, kept) = theme_proposes(Scheme::Frost, Scheme::Matrix, &now, &kept);
        assert_eq!(
            edits,
            [
                set(ACCENT_KEY, "phosphor"),
                set(ANIMATION_KEY, "rain"),
                set(PICTURE_KEY, "rain"),
            ]
        );
        assert_eq!(
            kept.keys,
            [
                (ACCENT_KEY, Some("violet".to_owned())),
                (ANIMATION_KEY, None),
                (PICTURE_KEY, None),
            ]
        );
    }

    #[test]
    fn what_a_theme_changed_is_kept_across_launches() {
        let dir = tempfile::tempdir().expect("a folder");
        let before = Before {
            keys: vec![
                (ACCENT_KEY, Some("teal".to_owned())),
                (ANIMATION_KEY, None),
                (PICTURE_KEY, Some("dawn".to_owned())),
            ],
        };
        before.write(Some(dir.path()));
        assert_eq!(Before::read(Some(dir.path())), before);
        Before::default().write(Some(dir.path()));
        assert_eq!(Before::read(Some(dir.path())), Before::default());
        assert_eq!(Before::read(None), Before::default());
    }

    #[test]
    fn the_theme_offers_givre_and_its_swatch_shows_with_it() {
        let theme = settings()
            .iter()
            .find(|s| s.id == THEME_ID)
            .expect("the theme's row");
        let Kind::Segmented(choices) = &theme.kind else {
            panic!("a segmented control");
        };
        let words: Vec<&str> = choices.iter().map(|c| c.label.get(Lang::French)).collect();
        assert_eq!(words, ["Sombre", "Clair", "Givre", "Matrix"]);
        assert_eq!(
            accents_shown(Scheme::Matrix, Accent::Phosphor)[0],
            Accent::Phosphor
        );
        assert!(!accents_shown(Scheme::Matrix, Accent::Phosphor).contains(&Accent::Ice));
        assert_eq!(accents_shown(Scheme::Dark, Accent::Green).len(), 6);
        assert!(!accents_shown(Scheme::Dark, Accent::Green).contains(&Accent::Ice));
        assert_eq!(accents_shown(Scheme::Frost, Accent::Ice)[0], Accent::Ice);
        assert_eq!(accents_shown(Scheme::Frost, Accent::Ice).len(), 7);
        // Kept in the plain themes while it is the accent.
        assert!(accents_shown(Scheme::Dark, Accent::Ice).contains(&Accent::Ice));
    }

    #[test]
    fn the_page_has_its_five_groups_in_order() {
        let mut groups: Vec<&str> = settings()
            .iter()
            .map(|s| s.group.label.get(hocus_words::Lang::French))
            .collect();
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
