//! Bureaux et fenêtres / Desktops and windows (6.14, issue #448, variant
//! A): how many desktops and their names, how a window moves and snaps,
//! Alt+Tab, and the ink.
//!
//! The compositor follows `[desktops]`, `[windows]` and `[ink]` as the
//! file changes (windows.md §5, §6, §9, §11, §12). Every row is a key of
//! the file; the desktops' count and names are drawn by the page itself
//! (a stepper, chips that become a field). A desktop taken out is asked of
//! the compositor first, on the Settings' line (`desktop-remove N`), which
//! gives its windows to the one before it and says which moved; the page
//! writes the file once answered, the count and the names shifted, so
//! that nothing moves twice. "Annuler" puts it back with them
//! (`desktop-restore N ID…`), then writes.

use hocus_words::settings::desktops as words;
use std::sync::OnceLock;

use crate::{
    lang::{Lang, Words, pair, w},
    model::{Choice, Codec, Kind, Places, RowGroup, SectionId, Setting, Value, one},
};

pub const DESKTOPS: RowGroup = RowGroup {
    label: w(words::DESKTOPS),
    per_source: false,
};
pub const MOVE: RowGroup = RowGroup {
    label: w(words::MOVE_AND_SNAP),
    per_source: false,
};
pub const ALT_TAB: RowGroup = RowGroup {
    label: w(words::ALT_TAB),
    per_source: false,
};
pub const OVERVIEW: RowGroup = RowGroup {
    label: w(words::OVERVIEW),
    per_source: false,
};
pub const DRAW: RowGroup = RowGroup {
    label: w(words::DRAW_ON_THE_SCREEN),
    per_source: false,
};

pub const COUNT: &str = "desktops.count";
pub const NAMES: &str = "desktops.names";
pub const MOVE_KEY: &str = "windows.move-modifier";
pub const RESIZE_KEY: &str = "windows.resize-modifier";
pub const MAGNET: &str = "windows.magnet";
pub const SNAP: &str = "windows.snap-by-drag";
pub const WOBBLE: &str = "windows.wobble";
/// How windows open and close (#432): a list each theme of its own adds
/// to.
pub const ANIMATION: &str = "windows.animation";
pub const ALT_TAB_ROW: &str = "windows.alt-tab";
pub const HOT_CORNER: &str = "overview.hot-corner";
pub const INK: &str = "ink.enabled";
/// The rules per application, a row of the page's own (6.14).
pub const RULES: &str = "desktops.rules";
pub const RULES_GROUP: RowGroup = RowGroup {
    label: w(words::RULES_PER_APPLICATION),
    per_source: false,
};
/// Keyboard's "Raccourcis du bureau", the page's link lights it.
pub const SHORTCUTS: &str = "keyboard.shortcuts";

/// Desktops on each screen, at most, and by default.
pub const MOST: usize = 9;
pub const DEFAULT: usize = 4;
/// A name's characters, at most.
pub const NAME_MOST: usize = 24;

const DRAG_KEYS: &[&str] = &["alt", "super", "none"];
const ALT_TAB_WORDS: &[&str] = &["all", "desktop"];
const ANIMATIONS: &[&str] = &["standard", "rain", "frost"];

/// Desktop `index`'s name (from 0) as `names` say it, else "Bureau N".
pub fn name_of(names: &[String], index: usize, lang: Lang) -> String {
    match names.get(index).map(|n| n.trim()).filter(|n| !n.is_empty()) {
        Some(name) => name.to_owned(),
        None => placeholder(index, lang),
    }
}

/// "Bureau 3" / "Desktop 3", for desktop `index` (from 0).
pub fn placeholder(index: usize, lang: Lang) -> String {
    words::desktop(lang, index + 1)
}

/// `names` as the file holds them for `count` desktops: those past the
/// count and the empty ones last left out.
pub fn trimmed(mut names: Vec<String>, count: usize) -> Vec<String> {
    names.truncate(count);
    while names.last().is_some_and(|n| n.trim().is_empty()) {
        names.pop();
    }
    names
}

/// `names` with desktop `index`'s taken out, the later ones moving down.
pub fn without(names: &[String], index: usize) -> Vec<String> {
    let mut out = names.to_vec();
    if index < out.len() {
        out.remove(index);
    }
    out
}

/// `names` with desktop `index` named `name` (`""`: none).
pub fn renamed(names: &[String], index: usize, name: &str) -> Vec<String> {
    let mut out = names.to_vec();
    if out.len() <= index {
        out.resize(index + 1, String::new());
    }
    out[index] = name
        .chars()
        .take(NAME_MOST)
        .collect::<String>()
        .trim()
        .to_owned();
    out
}

/// A list of names as the file writes it: `["Travail", "", "Courrier"]`.
pub fn toml_list(names: &[String]) -> String {
    let quoted: Vec<String> = names
        .iter()
        .map(|n| toml_edit::Value::from(n.as_str()).to_string())
        .collect();
    format!("[{}]", quoted.join(", "))
}

/// "3 fenêtres", "1 fenêtre", "aucune fenêtre".
pub fn windows_said(n: usize, lang: Lang) -> String {
    match n {
        0 => words::NO_WINDOWS.get(lang).to_owned(),
        1 => words::N_1_WINDOW.get(lang).to_owned(),
        n => words::windows(lang, n),
    }
}

/// The toast of desktop `name` taken out, its `moved` windows going to
/// `to`.
pub fn removed_said(name: &str, moved: usize, to: &str, lang: Lang) -> String {
    match moved {
        0 => words::removed(lang, name),
        1 => words::removed_its_window_goes_to(lang, name, to),
        n => words::removed_its_windows_go_to(lang, name, to, n),
    }
}

/// A rule per application, `[[windows.rules]]`, as the file says it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rule {
    pub app_id: String,
    /// From 1; none: where it is opened.
    pub desktop: Option<i64>,
    /// `"maximized"`, `"left"`, `"right"`, `"fullscreen"`; none: its last
    /// size.
    pub size: Option<String>,
    pub above: bool,
}

/// The rules the file holds, as the window keeps them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rules(pub Vec<Rule>);

/// The sizes a rule may give, as written, and said.
pub const SIZES: [(&str, Words); 5] = [
    ("last", w(words::ITS_LAST_SIZE)),
    ("maximized", w(words::MAXIMISED)),
    ("left", w(words::LEFT_HALF)),
    ("right", w(words::RIGHT_HALF)),
    ("fullscreen", w(words::FULL_SCREEN)),
];

/// The rules the file holds, in its order (those without an app id left
/// out).
pub fn rules_of(doc: &toml_edit::DocumentMut) -> Vec<Rule> {
    let Some(rules) = doc.get("windows").and_then(|w| w.get("rules")) else {
        return Vec::new();
    };
    let tables: Vec<&dyn toml_edit::TableLike> = match rules {
        toml_edit::Item::ArrayOfTables(a) => {
            a.iter().map(|t| t as &dyn toml_edit::TableLike).collect()
        }
        toml_edit::Item::Value(toml_edit::Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_inline_table().map(|t| t as &dyn toml_edit::TableLike))
            .collect(),
        _ => Vec::new(),
    };
    tables
        .into_iter()
        .filter_map(|t| {
            Some(Rule {
                app_id: t.get("app-id")?.as_str()?.to_owned(),
                desktop: t.get("desktop").and_then(toml_edit::Item::as_integer),
                size: t
                    .get("size")
                    .and_then(toml_edit::Item::as_str)
                    .filter(|s| *s != "last")
                    .map(ToOwned::to_owned),
                above: t
                    .get("above")
                    .and_then(toml_edit::Item::as_bool)
                    .unwrap_or(false),
            })
        })
        .collect()
}

/// The rules as the file writes them: an array of inline tables (the
/// worker writes it as `[[windows.rules]]`), none for an empty list.
pub fn rules_toml(rules: &[Rule]) -> Option<String> {
    if rules.is_empty() {
        return None;
    }
    let tables: Vec<String> = rules
        .iter()
        .map(|r| {
            let mut fields = vec![format!(
                "app-id = {}",
                toml_edit::Value::from(r.app_id.as_str())
            )];
            if let Some(d) = r.desktop {
                fields.push(format!("desktop = {d}"));
            }
            if let Some(size) = &r.size {
                fields.push(format!("size = {}", toml_edit::Value::from(size.as_str())));
            }
            if r.above {
                fields.push("above = true".to_owned());
            }
            format!("{{ {} }}", fields.join(", "))
        })
        .collect();
    Some(format!("[{}]", tables.join(", ")))
}

/// The rules once desktop `index` (from 0) is taken out: those naming it
/// point to the one that took its windows, the later ones move down one.
pub fn rules_without(rules: &[Rule], index: usize) -> Vec<Rule> {
    let gone = i64::try_from(index + 1).unwrap_or(i64::MAX);
    rules
        .iter()
        .cloned()
        .map(|mut r| {
            r.desktop = r.desktop.map(|d| match d.cmp(&gone) {
                std::cmp::Ordering::Less => d,
                std::cmp::Ordering::Equal => (d - 1).max(1),
                std::cmp::Ordering::Greater => d - 1,
            });
            r
        })
        .collect()
}

/// What a rule does, as its row says it: "Toujours sur « Courrier » ·
/// au-dessus des autres", or that it does nothing yet.
pub fn rule_said(rule: &Rule, names: &[String], lang: Lang) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(d) = rule.desktop.and_then(|d| usize::try_from(d - 1).ok()) {
        let name = name_of(names, d, lang);
        parts.push(words::always_on(lang, &name));
    }
    if let Some(size) = &rule.size
        && let Some((_, said)) = SIZES.iter().find(|(word, _)| word == size)
    {
        let said = said.get(lang);
        parts.push(words::opens(lang, said.to_lowercase()));
    }
    if rule.above {
        parts.push(w(words::ABOVE_THE_OTHERS).get(lang).to_owned());
    }
    if parts.is_empty() {
        return w(words::NO_RULE_YET_IT_OPENS_LIKE).get(lang).to_owned();
    }
    let said = parts.join(" · ");
    let mut chars = said.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The page's rows the model does not hold, for the search.
pub fn outside() -> &'static [crate::model::Outside] {
    const ALL: [crate::model::Outside; 1] = [crate::model::Outside {
        id: RULES,
        section: SectionId::Desktops,
        group: RULES_GROUP.label,
        name: w(words::RULES_PER_APPLICATION_2),
        keywords: (
            &[
                "règle",
                "application",
                "au-dessus",
                "toujours",
                "ouvrir",
                "app-id",
            ],
            &["rule", "application", "above", "always", "open", "app-id"],
        ),
    }];
    &ALL
}

fn word(fr: &'static str, en: &'static str, word: &'static str) -> Choice {
    Choice {
        label: pair(fr, en),
        value: Value::Word(word),
    }
}

fn setting(
    id: &'static str,
    group: RowGroup,
    name: Words,
    line: Words,
    keywords: (&'static [&'static str], &'static [&'static str]),
    kind: Kind,
    places: Places,
) -> Setting {
    Setting {
        id,
        section: SectionId::Desktops,
        group,
        name,
        line: Some(line),
        keywords,
        kind,
        places,
        needs: None,
    }
}

/// The page's settings, in its order.
// Over 200 lines: a table, one entry per setting of the page.
pub fn settings() -> &'static [Setting] {
    static SETTINGS: OnceLock<Vec<Setting>> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        let switch = |key| one(key, Codec::Bool);
        vec![
            setting(
                COUNT,
                DESKTOPS,
                w(words::NUMBER_OF_DESKTOPS),
                w(words::ON_EVERY_SCREEN_1_TO_9),
                (
                    &["bureaux virtuels", "espaces de travail", "combien"],
                    &["virtual desktops", "workspaces", "how many"],
                ),
                Kind::Slider {
                    min: 1,
                    max: MOST as i64,
                    step: 1,
                    unit: "",
                },
                one(
                    &["desktops", "count"],
                    Codec::Int {
                        min: 1,
                        max: MOST as i64,
                    },
                ),
            ),
            setting(
                NAMES,
                DESKTOPS,
                w(words::THEIR_NAMES),
                w(words::CLICK_A_NAME_TO_CHANGE_IT),
                (
                    &["nommer", "renommer", "étiquette", "espaces de travail"],
                    &["name", "rename", "label", "workspaces"],
                ),
                Kind::Text,
                one(&["desktops", "names"], Codec::Texts),
            ),
            setting(
                MOVE_KEY,
                MOVE,
                w(words::MOVE_FROM_ANYWHERE),
                w(words::HOLD_THE_KEY_AND_DRAG_ANYWHERE),
                (
                    &["déplacer", "glisser", "saisir", "alt", "super", "fenêtre"],
                    &["move", "drag", "grab", "alt", "super", "window"],
                ),
                Kind::Menu(vec![
                    word("Alt + glisser", "Alt + drag", "alt"),
                    word("Super + glisser", "Super + drag", "super"),
                    word("Aucune touche", "No key", "none"),
                ]),
                one(&["windows", "move-modifier"], Codec::Word(DRAG_KEYS)),
            ),
            setting(
                RESIZE_KEY,
                MOVE,
                w(words::RESIZE_FROM_ANYWHERE),
                w(words::THE_EDGE_NEAREST_THE_POINTER_FOLLOWS),
                (
                    &["redimensionner", "taille", "clic droit", "agrandir"],
                    &["resize", "size", "right click", "enlarge"],
                ),
                Kind::Menu(vec![
                    word("Super + clic droit glissé", "Super + right-drag", "super"),
                    word("Alt + clic droit glissé", "Alt + right-drag", "alt"),
                    word("Aucune touche", "No key", "none"),
                ]),
                one(&["windows", "resize-modifier"], Codec::Word(DRAG_KEYS)),
            ),
            setting(
                MAGNET,
                MOVE,
                w(words::MAGNETIC_EDGES),
                w(words::WITHIN_12_PX_OF_ANOTHER_WINDOW),
                (&["aimant", "coller", "bord"], &["magnet", "stick", "edge"]),
                Kind::Switch,
                switch(&["windows", "magnet"]),
            ),
            setting(
                SNAP,
                MOVE,
                w(words::SNAP_BY_DRAGGING_TO_AN_EDGE),
                w(words::AGAINST_A_SIDE_A_HALF_IN),
                (
                    &["ancrer", "moitié", "quart", "maximiser", "pavage"],
                    &["snap", "half", "quarter", "maximise", "tiling"],
                ),
                Kind::Switch,
                switch(&["windows", "snap-by-drag"]),
            ),
            setting(
                WOBBLE,
                MOVE,
                w(words::WOBBLY_WINDOWS),
                w(words::A_MOVED_WINDOW_WOBBLES_THEN_SETTLES),
                (
                    &["ondule", "ondulantes", "gelée", "animation"],
                    &["wobbly", "wobble", "jelly", "animation"],
                ),
                Kind::Switch,
                switch(&["windows", "wobble"]),
            ),
            setting(
                ANIMATION,
                MOVE,
                w(words::OPEN_AND_CLOSE),
                w(words::RAIN_COLUMNS_OF_GLYPHS_FALL_THROUGH),
                (
                    &["ouvrir", "fermer", "animation", "pluie", "matrix", "givre"],
                    &["open", "close", "animation", "rain", "matrix", "frost"],
                ),
                Kind::Menu(vec![
                    word("Standard", "Standard", "standard"),
                    word("Pluie", "Rain", "rain"),
                    word("Givre", "Frost", "frost"),
                ]),
                one(&["windows", "animation"], Codec::Word(ANIMATIONS)),
            ),
            setting(
                ALT_TAB_ROW,
                ALT_TAB,
                w(words::ALT_TAB_SHOWS),
                w(words::ALL_THIS_DESKTOP_FIRST_THEN_THE),
                (
                    &["alt+tab", "passer", "fenêtres", "bureau"],
                    &["alt+tab", "switcher", "windows", "desktop"],
                ),
                Kind::Menu(vec![
                    word("Tous les bureaux", "All desktops", "all"),
                    word("Ce bureau seulement", "This desktop only", "desktop"),
                ]),
                one(&["windows", "alt-tab"], Codec::Word(ALT_TAB_WORDS)),
            ),
            setting(
                HOT_CORNER,
                OVERVIEW,
                w(words::TOP_LEFT_CORNER_OVERVIEW),
                w(words::THE_POINTER_PUSHED_FOR_A_MOMENT),
                (
                    &["coin actif", "coin", "vue d’ensemble", "aperçu", "bureaux"],
                    &["hot corner", "corner", "overview", "activities", "desktops"],
                ),
                Kind::Switch,
                switch(&["overview", "hot-corner"]),
            ),
            setting(
                INK,
                DRAW,
                w(words::DRAW_ON_THE_SCREEN_2),
                w(words::SUPER_DRAG_DRAWS_A_STROKE_SUPER),
                (
                    &["encre", "laser", "flèche", "annoter", "trait"],
                    &["ink", "laser", "arrow", "annotate", "stroke"],
                ),
                Kind::Switch,
                switch(&["ink", "enabled"]),
            ),
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_by_position_and_said_or_numbered() {
        let names = vec!["Travail".to_owned(), String::new(), "Courrier".to_owned()];
        assert_eq!(name_of(&names, 0, Lang::French), "Travail");
        assert_eq!(name_of(&names, 1, Lang::French), "Bureau 2");
        assert_eq!(name_of(&names, 5, Lang::English), "Desktop 6");
        assert_eq!(without(&names, 1), ["Travail", "Courrier"]);
        assert_eq!(without(&names, 7), names);
        assert_eq!(
            renamed(&names, 4, " Musique "),
            ["Travail", "", "Courrier", "", "Musique"]
        );
        let long = renamed(&[], 0, "Un nom bien trop long pour une puce");
        assert_eq!(long[0].chars().count(), NAME_MOST);
        assert_eq!(trimmed(renamed(&names, 2, ""), 4), ["Travail"]);
        assert_eq!(trimmed(names.clone(), 1), ["Travail"]);
        assert_eq!(toml_list(&names), "[\"Travail\", \"\", \"Courrier\"]");
    }

    #[test]
    fn the_rules_read_write_and_move_down_with_their_desktops() -> Result<(), String> {
        let doc: toml_edit::DocumentMut = "[[windows.rules]]\napp-id = \"org.example.Mail\"\n\
             desktop = 2\nsize = \"maximized\"\nabove = true\n\n[[windows.rules]]\n\
             app-id = \"foot\"\n\n[[windows.rules]]\nsize = \"left\"\n"
            .parse()
            .map_err(|e| format!("{e}"))?;
        let rules = rules_of(&doc);
        assert_eq!(rules.len(), 2, "one without an app id left out");
        assert_eq!(rules[0].desktop, Some(2));
        let names = vec!["Travail".to_owned(), "Courrier".to_owned()];
        assert_eq!(
            rule_said(&rules[0], &names, Lang::French),
            "Toujours sur « Courrier » · s’ouvre maximisée · au-dessus des autres"
        );
        assert_eq!(
            rule_said(&rules[1], &names, Lang::French),
            "Aucune règle encore : elle s’ouvre comme les autres."
        );
        assert_eq!(
            rules_toml(&rules).as_deref(),
            Some(
                "[{ app-id = \"org.example.Mail\", desktop = 2, size = \"maximized\", above = true }, { app-id = \"foot\" }]"
            )
        );
        assert_eq!(rules_toml(&[]), None);
        // Desktop 2 taken out: its rule to 1; one on 3 to 2.
        let three = Rule {
            app_id: "x".into(),
            desktop: Some(3),
            ..Rule::default()
        };
        let moved = rules_without(&[rules[0].clone(), three], 1);
        assert_eq!(moved[0].desktop, Some(1));
        assert_eq!(moved[1].desktop, Some(2));
        // The first taken out: its rule stays on the first, the next one.
        let first = Rule {
            desktop: Some(1),
            ..Rule::default()
        };
        assert_eq!(rules_without(&[first], 0)[0].desktop, Some(1));
        Ok(())
    }

    #[test]
    fn a_desktop_removed_says_where_its_windows_go() {
        assert_eq!(
            removed_said("Courrier", 2, "Travail", Lang::French),
            "« Courrier » retiré : ses 2 fenêtres passent sur « Travail »"
        );
        assert_eq!(
            removed_said("Mail", 2, "Work", Lang::English),
            "“Mail” removed: its 2 windows go to “Work”"
        );
        assert_eq!(
            removed_said("Bureau 4", 0, "Bureau 3", Lang::French),
            "« Bureau 4 » retiré"
        );
        assert_eq!(windows_said(3, Lang::French), "3 fenêtres");
    }

    #[test]
    fn the_rows_read_the_compositors_keys() {
        let keys: Vec<String> = settings()
            .iter()
            .map(|s| s.places.of(crate::model::Source::Ac).dotted())
            .collect();
        assert_eq!(
            keys,
            [
                "desktops.count",
                "desktops.names",
                "windows.move-modifier",
                "windows.resize-modifier",
                "windows.magnet",
                "windows.snap-by-drag",
                "windows.wobble",
                "windows.animation",
                "windows.alt-tab",
                "overview.hot-corner",
                "ink.enabled"
            ]
        );
    }
}
