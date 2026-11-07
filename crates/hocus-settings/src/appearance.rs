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

use hocus_link::theme::{Accent, Scheme};

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

const SCHEMES: &[&str] = &["dark", "light", "frost"];
const ACCENTS: &[&str] = &[
    "green",
    "blue",
    "violet",
    "rose",
    "teal",
    "wallpaper",
    "ice",
];
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

/// The theme's setting, and the accent's.
pub const THEME_ID: &str = "appearance.theme";
pub const ACCENT_ID: &str = "appearance.accent";

/// What a themed theme set when it was chosen, to give back when it is
/// left (#740): kept in `$XDG_STATE_HOME/hocuspanda/theme-before`, one
/// `key = "word"` line each.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Before {
    /// The accent before the theme's own; `None`: the file named none.
    pub accent: Option<Accent>,
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
            let value = value.trim().trim_matches('"');
            if key.trim() == "accent" {
                before.accent = Accent::of(value);
            }
        }
        before
    }

    pub fn write(&self, state: Option<&std::path::Path>) {
        let Some(state) = state else {
            return;
        };
        let text = match self.accent {
            Some(accent) => format!("accent = \"{accent}\"\n"),
            None => String::new(),
        };
        if let Err(err) = hocus_config::replace(&state.join(Self::FILE), text.as_bytes(), None) {
            tracing::warn!(%err, "could not keep what the theme changed");
        }
    }
}

/// A setting a theme's choice writes besides the theme itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Proposed {
    /// `setting` set to this word.
    Set(&'static str, &'static str),
    /// `setting`'s line taken out: its default applies.
    Remove(&'static str),
}

/// What choosing theme `to` over `from` changes besides the theme (the
/// hook of #431 and #740), `accent` the accent now, `before` what the
/// themed theme left had changed; and what to remember for leaving `to`.
///
/// Entering Givre sets its accent, the ice blue, remembering the one
/// before; leaving it gives that one back, unless another was chosen
/// meanwhile. Givre's window animation (`[windows] open-close = "frost"`)
/// and its proposed wallpaper (`[wallpaper] picture = "aurora"`, only over
/// a built-in) come here once they exist, with the values they replace.
pub fn theme_proposes(
    from: Scheme,
    to: Scheme,
    accent: Option<Accent>,
    before: &Before,
) -> (Vec<Proposed>, Before) {
    let mut proposed = Vec::new();
    let mut remember = before.clone();
    if from == to {
        return (proposed, remember);
    }
    if from.themed() && accent == Some(theme_accent(from)) {
        proposed.push(match before.accent {
            Some(accent) => Proposed::Set(ACCENT_ID, accent.word()),
            None => Proposed::Remove(ACCENT_ID),
        });
        remember.accent = None;
    }
    if to.themed() {
        let own = theme_accent(to);
        if accent != Some(own) {
            proposed.push(Proposed::Set(ACCENT_ID, own.word()));
            // What was there before any themed theme.
            if !from.themed() {
                remember.accent = accent;
            }
        }
    }
    (proposed, remember)
}

/// A themed theme's own accent.
pub const fn theme_accent(scheme: Scheme) -> Accent {
    match scheme {
        Scheme::Frost => Accent::Ice,
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
        let themed = accent == Accent::Ice;
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
                    "La barre, les menus, le lanceur et les Réglages ; les applications le savent aussi. Givre garde les Réglages et les applications sombres.",
                    "The bar, menus, launcher and Settings; applications are told too. Frost keeps the Settings and applications dark.",
                )),
                keywords: (
                    &["sombre", "clair", "nuit", "mode sombre", "thème", "givre", "glacé"],
                    &["dark", "light", "night", "dark mode", "theme", "frost", "ice"],
                ),
                // Matrix joins them once its tokens exist (#431).
                kind: Kind::Segmented(vec![
                    word("Sombre", "Dark", "dark"),
                    word("Clair", "Light", "light"),
                    word("Givre", "Frost", "frost"),
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
                id: "appearance.accessibility",
                section: SectionId::Appearance,
                group: MOTION,
                name: w("Accessibilité", "Accessibility"),
                line: Some(w(
                    "Les mouvements réduits, la transparence réduite, le contraste élevé et la loupe.",
                    "Reduced motion, reduced transparency, high contrast and the magnifier.",
                )),
                keywords: (
                    &["mouvement", "transparence", "contraste", "loupe", "accessibilité"],
                    &["motion", "transparency", "contrast", "magnifier", "accessibility"],
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
    fn choosing_givre_sets_its_accent_and_leaving_it_gives_the_old_one_back() {
        let none = Before::default();
        // Entering Givre: the ice blue, the violet remembered.
        let (proposed, kept) =
            theme_proposes(Scheme::Dark, Scheme::Frost, Some(Accent::Violet), &none);
        assert_eq!(proposed, vec![Proposed::Set(ACCENT_ID, "ice")]);
        assert_eq!(kept.accent, Some(Accent::Violet));
        // Leaving it with the ice blue still chosen: the violet again.
        let (proposed, kept) =
            theme_proposes(Scheme::Frost, Scheme::Light, Some(Accent::Ice), &kept);
        assert_eq!(proposed, vec![Proposed::Set(ACCENT_ID, "violet")]);
        assert_eq!(kept, Before::default());
        // A file that named no accent: its line taken out again.
        let (_, kept) = theme_proposes(Scheme::Dark, Scheme::Frost, None, &none);
        let (proposed, _) = theme_proposes(Scheme::Frost, Scheme::Dark, Some(Accent::Ice), &kept);
        assert_eq!(proposed, vec![Proposed::Remove(ACCENT_ID)]);
        // Another accent chosen while in Givre stays.
        let (proposed, _) = theme_proposes(Scheme::Frost, Scheme::Dark, Some(Accent::Rose), &kept);
        assert!(proposed.is_empty());
        // Between the plain themes, nothing more than the theme.
        let (proposed, _) = theme_proposes(Scheme::Dark, Scheme::Light, Some(Accent::Teal), &none);
        assert!(proposed.is_empty());
        // Already in the ice blue: nothing to set, nothing remembered.
        let (proposed, kept) =
            theme_proposes(Scheme::Dark, Scheme::Frost, Some(Accent::Ice), &none);
        assert!(proposed.is_empty());
        assert_eq!(kept.accent, None);
    }

    #[test]
    fn what_givre_changed_is_kept_across_launches() {
        let dir = tempfile::tempdir().expect("a folder");
        let before = Before {
            accent: Some(Accent::Teal),
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
        assert_eq!(words, ["Sombre", "Clair", "Givre"]);
        assert_eq!(accents_shown(Scheme::Dark, Accent::Green).len(), 6);
        assert!(!accents_shown(Scheme::Dark, Accent::Green).contains(&Accent::Ice));
        assert_eq!(accents_shown(Scheme::Frost, Accent::Ice)[0], Accent::Ice);
        assert_eq!(accents_shown(Scheme::Frost, Accent::Ice).len(), 7);
        // Kept in the plain themes while it is the accent.
        assert!(accents_shown(Scheme::Dark, Accent::Ice).contains(&Accent::Ice));
    }

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
