//! La barre / The bar (6.4, issue #404, variant A): the screens with a
//! bar, its form on each screen, what it shows from top to bottom and in
//! which order, and its clock.
//!
//! The compositor follows `[panel]`, `[panel.clock]` and the screens'
//! `panel-form` as the file changes (bar.md "6.4, the bar followed live").
//! The rows that are one key each are settings of the window's model; a
//! screen's form (one row per screen known) and the groups' order are
//! keys the page writes itself ([`File`] reads them back). The form chosen
//! with a click on the bar's edge is the compositor's: the page reads it
//! from its line and asks it to forget it (`bar-auto NAME`).

use hocus_words::settings::bar as words;
use std::sync::{Mutex, OnceLock, PoisonError};

use hocus_link::BarGroup;
use toml_edit::{DocumentMut, Item};

use crate::{
    lang::{Lang, Words},
    model::{Choice, Codec, Kind, Outside, RowGroup, SectionId, Setting, Value, one},
};

pub const PLACE: RowGroup = RowGroup {
    label: words::PLACE,
    per_source: false,
};
pub const FORM: RowGroup = RowGroup {
    label: words::FORM,
    per_source: false,
};
pub const SHOWS: RowGroup = RowGroup {
    label: words::WHAT_IT_SHOWS_TOP_TO_BOTTOM,
    per_source: false,
};
pub const CLOCK: RowGroup = RowGroup {
    label: words::THE_CLOCK,
    per_source: false,
};

pub const SIDE: &str = "bar.side";
pub const SCREENS: &str = "bar.screens";
pub const MEETING: &str = "bar.meeting";
pub const TODAY: &str = "bar.today";
pub const TASKS: &str = "bar.tasks";
pub const AGENTS: &str = "bar.agents";
pub const WINDOWS: &str = "bar.windows";
pub const SECONDS: &str = "bar.seconds";
pub const TRANSITION: &str = "bar.transition";
pub const FONT: &str = "bar.font";
pub const COLOR: &str = "bar.color";
/// Outside the model's one key each: a screen's form, by hand, the order.
pub const FORM_ROW: &str = "bar.form";
pub const BY_HAND: &str = "bar.by-hand";
pub const ORDER: &str = "bar.order";

/// The clock's bundled font.
pub const BUNDLED: &str = hocus_link::ClockLook::DEFAULT_FONT;

/// The clock's colours offered, `#RRGGBB` in lower case as the model
/// holds them.
pub const COLOURS: [(&str, Words); 4] = [
    ("#eef1f6", words::SOFT_WHITE),
    ("#7ee0a0", words::GREEN),
    ("#8fb3ff", words::BLUE),
    ("#f5c46e", words::AMBER),
];

const TRANSITIONS: &[&str] = &["odometer", "roll", "flip", "neon", "none"];
const SCREENS_WORDS: &[&str] = &["primary", "all"];
const SIDES: &[&str] = &["right", "left"];

/// A group's setting, by its group.
pub fn group_setting(group: BarGroup) -> &'static str {
    match group {
        BarGroup::Today => TODAY,
        BarGroup::Tasks => TASKS,
        BarGroup::Agents => AGENTS,
    }
}

/// A group's name, as its row says it.
pub fn group_name(group: BarGroup) -> Words {
    match group {
        BarGroup::Today => words::TODAY,
        BarGroup::Tasks => words::TO_DO,
        BarGroup::Agents => words::THE_AGENTS,
    }
}

/// "À faire, Aujourd’hui, Les agents".
pub fn say_order(order: [BarGroup; 3], lang: Lang) -> String {
    order
        .iter()
        .map(|g| group_name(*g).get(lang))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `order` with `group` one place up (`up`) or down; none at an end.
pub fn moved(order: [BarGroup; 3], group: BarGroup, up: bool) -> Option<[BarGroup; 3]> {
    let at = order.iter().position(|g| *g == group)?;
    let to = if up { at.checked_sub(1)? } else { at + 1 };
    if to >= order.len() {
        return None;
    }
    let mut out = order;
    out.swap(at, to);
    Some(out)
}

/// What the file says of the bar beyond the model's keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct File {
    /// `[panel] form`: a screen without its own takes it.
    pub form: Option<String>,
    /// `[outputs.NAME] panel-form`, by name.
    pub outputs: Vec<(String, String)>,
    /// `[panel] order`, as written.
    pub order: Option<Vec<String>>,
    /// A calendar or a task source is set (`[[day.calendars]]`,
    /// `[[day.tasks]]`).
    pub calendars: bool,
    pub tasks: bool,
}

impl File {
    pub fn of(doc: &DocumentMut) -> Self {
        let panel = doc.get("panel");
        let day = doc.get("day");
        let any = |key: &str| {
            day.and_then(|d| d.get(key)).is_some_and(|i| match i {
                Item::ArrayOfTables(a) => !a.is_empty(),
                Item::Value(v) => v.as_array().is_some_and(|a| !a.is_empty()),
                _ => false,
            })
        };
        Self {
            form: panel
                .and_then(|p| p.get("form"))
                .and_then(Item::as_str)
                .map(ToOwned::to_owned),
            outputs: doc
                .get("outputs")
                .and_then(Item::as_table_like)
                .map(|outputs| {
                    outputs
                        .iter()
                        .filter_map(|(name, o)| {
                            let form = o.get("panel-form")?.as_str()?;
                            Some((name.to_owned(), form.to_owned()))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            order: panel
                .and_then(|p| p.get("order"))
                .and_then(Item::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                        .collect()
                }),
            calendars: any("calendars"),
            tasks: any("tasks"),
        }
    }

    /// The groups' order the compositor reads from the file.
    pub fn order(&self) -> [BarGroup; 3] {
        BarGroup::order_of(self.order.as_deref().unwrap_or_default())
    }

    /// Screen `name`'s form asked: `"auto"`, `"full"` or `"mini"`; its
    /// own, else `[panel] form`'s, as the compositor reads them.
    pub fn form_of(&self, name: &str) -> &'static str {
        let own = self
            .outputs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, f)| f.as_str());
        match own.or(self.form.as_deref()) {
            Some("full") => "full",
            Some("mini") => "mini",
            _ => "auto",
        }
    }

    /// The screens the file gives a form of their own.
    pub fn named(&self) -> impl Iterator<Item = &str> {
        self.outputs.iter().map(|(n, _)| n.as_str())
    }
}

/// The monospaced families installed (fontconfig's `spacing=mono`),
/// looked for on a thread of their own as the program starts.
static FONTS: Mutex<Option<std::thread::JoinHandle<Vec<String>>>> = Mutex::new(None);
static FOUND: OnceLock<Vec<&'static str>> = OnceLock::new();

/// Starts looking for the monospaced families (`fc-list`), before the
/// window: the clock's font menu lists them.
pub fn find_fonts() {
    let started = std::thread::Builder::new()
        .name("monospaced fonts".to_owned())
        .spawn(|| {
            let out = std::process::Command::new("fc-list")
                .args([":spacing=mono", "family"])
                .stderr(std::process::Stdio::null())
                .output();
            match out {
                Ok(out) => families(&String::from_utf8_lossy(&out.stdout)),
                Err(err) => {
                    tracing::info!(%err, "fc-list does not run: only the bundled clock font");
                    Vec::new()
                }
            }
        });
    if let Ok(handle) = started {
        *FONTS.lock().unwrap_or_else(PoisonError::into_inner) = Some(handle);
    }
}

/// The families `fc-list` lists, one each, sorted; a line naming several
/// (`DejaVu Sans Mono,DejaVu Sans Mono Book`) is its first; the bundled
/// one left out.
pub fn families(listed: &str) -> Vec<String> {
    let mut names: Vec<String> = listed
        .lines()
        .filter_map(|l| l.split(',').next())
        .map(|n| n.trim().replace('\\', ""))
        .filter(|n| !n.is_empty() && n != BUNDLED)
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    names
}

fn fonts() -> &'static [&'static str] {
    FOUND.get_or_init(|| {
        let handle = FONTS.lock().unwrap_or_else(PoisonError::into_inner).take();
        handle
            .and_then(|h| h.join().ok())
            .unwrap_or_default()
            .into_iter()
            .map(|n| &*Box::leak(n.into_boxed_str()))
            .collect()
    })
}

fn word(text: hocus_words::Text, word: &'static str) -> Choice {
    Choice {
        label: text,
        value: Value::Word(word),
    }
}

fn switch(
    id: &'static str,
    group: RowGroup,
    name: Words,
    line: Words,
    keywords: (&'static [&'static str], &'static [&'static str]),
    key: &'static [&'static str],
) -> Setting {
    Setting {
        id,
        section: SectionId::Bar,
        group,
        name,
        line: Some(line),
        keywords,
        kind: Kind::Switch,
        places: one(key, Codec::Bool),
        needs: None,
    }
}

/// The page's settings of one key each, in its order.
// Over 200 lines: a table, one entry per setting of the page.
pub fn settings() -> &'static [Setting] {
    static SETTINGS: OnceLock<Vec<Setting>> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        let mut fonts = vec![Choice {
            label: words::MARTIAN_MONO_BUNDLED,
            value: Value::Text(BUNDLED.to_owned()),
        }];
        fonts.extend(self::fonts().iter().map(|name| Choice {
            label: Words::same(name),
            value: Value::Text((*name).to_owned()),
        }));
        vec![
            Setting {
                id: SIDE,
                section: SectionId::Bar,
                group: PLACE,
                name: words::SIDE_OF_THE_SCREEN,
                line: Some(words::THE_BAR_ITS_EDGE_TO_CLICK),
                keywords: (
                    &["gauche", "droite", "côté", "bord", "side"],
                    &["left", "right", "side", "edge"],
                ),
                kind: Kind::Segmented(vec![word(words::RIGHT, "right"), word(words::LEFT, "left")]),
                places: one(&["panel", "side"], Codec::Word(SIDES)),
                needs: None,
            },
            Setting {
                id: SCREENS,
                section: SectionId::Bar,
                group: PLACE,
                name: words::ON_WHICH_SCREENS,
                line: None,
                keywords: (
                    &["plusieurs écrans", "moniteur", "tous", "principal", "écran"],
                    &["several screens", "monitor", "all", "primary", "screen"],
                ),
                kind: Kind::Menu(vec![
                    word(words::THE_PRIMARY_SCREEN, "primary"),
                    word(words::EVERY_SCREEN, "all"),
                ]),
                places: one(&["panel", "screens"], Codec::Word(SCREENS_WORDS)),
                needs: None,
            },
            switch(
                MEETING,
                SHOWS,
                words::THE_NEXT_MEETING,
                words::ITS_CARD_UNDER_THE_CLOCK_AND,
                (
                    &["réunion", "agenda", "rendez-vous"],
                    &["meeting", "calendar"],
                ),
                &["panel", "meeting"],
            ),
            switch(
                TODAY,
                SHOWS,
                words::TODAY,
                words::THE_EVENTS_AFTER_THE_MEETING_THE,
                (
                    &["agenda", "journée", "rendez-vous"],
                    &["today", "calendar", "events"],
                ),
                &["panel", "today"],
            ),
            switch(
                TASKS,
                SHOWS,
                words::TO_DO,
                words::THE_OPEN_TASKS_THREE_AT_MOST,
                (&["tâches", "kanban", "liste"], &["tasks", "todo", "kanban"]),
                &["panel", "tasks"],
            ),
            switch(
                AGENTS,
                SHOWS,
                words::THE_AGENTS,
                words::THOSE_WAITING_FOR_YOU_FIRST_IN,
                (
                    &["agents", "secrétaire", "pandas"],
                    &["agents", "secretary"],
                ),
                &["panel", "agents"],
            ),
            switch(
                WINDOWS,
                SHOWS,
                words::THE_WINDOWS,
                words::THE_CURRENT_DESKTOP_S_OVER_THE,
                (&["fenêtres", "liste"], &["windows", "list"]),
                &["panel", "windows"],
            ),
            Setting {
                id: SECONDS,
                section: SectionId::Bar,
                group: CLOCK,
                name: words::SECONDS,
                line: Some(words::BESIDE_THE_MINUTES_IN_THE_FULL),
                keywords: (&["secondes", "horloge", "heure"], &["seconds", "clock"]),
                kind: Kind::Switch,
                places: one(&["panel", "clock", "seconds"], Codec::Bool),
                needs: None,
            },
            Setting {
                id: TRANSITION,
                section: SectionId::Bar,
                group: CLOCK,
                name: words::WHEN_THE_MINUTE_CHANGES,
                line: Some(words::WHAT_THE_DIGITS_DO_NOTHING_MOVES),
                keywords: (
                    &[
                        "animation",
                        "compteur",
                        "rouleau",
                        "volet",
                        "néon",
                        "horloge",
                    ],
                    &["animation", "odometer", "roll", "flip", "neon", "clock"],
                ),
                kind: Kind::Menu(vec![
                    word(words::ODOMETER, "odometer"),
                    word(words::ROLL, "roll"),
                    word(words::FLIP, "flip"),
                    word(words::NEON, "neon"),
                    word(words::NO_MOTION, "none"),
                ]),
                places: one(&["panel", "clock", "transition"], Codec::Word(TRANSITIONS)),
                needs: None,
            },
            Setting {
                id: FONT,
                section: SectionId::Bar,
                group: CLOCK,
                name: words::CLOCK_FONT,
                line: Some(words::A_FONT_THAT_IS_NOT_INSTALLED),
                keywords: (
                    &["police", "chiffres", "horloge"],
                    &["font", "typeface", "clock"],
                ),
                kind: Kind::Menu(fonts),
                places: one(&["panel", "clock", "font"], Codec::Text),
                needs: None,
            },
            Setting {
                id: COLOR,
                section: SectionId::Bar,
                group: CLOCK,
                name: words::CLOCK_COLOUR,
                line: None,
                keywords: (
                    &["couleur", "teinte", "horloge"],
                    &["colour", "color", "clock"],
                ),
                kind: Kind::Menu(
                    COLOURS
                        .iter()
                        .map(|(hex, name)| Choice {
                            label: *name,
                            value: Value::Text((*hex).to_owned()),
                        })
                        .collect(),
                ),
                places: one(&["panel", "clock", "color"], Codec::Colour),
                needs: None,
            },
        ]
    })
}

/// The page's rows the model does not hold, for the search: each lights
/// its row.
pub fn outside() -> &'static [Outside] {
    const fn o(
        id: &'static str,
        group: Words,
        name: Words,
        fr: &'static [&'static str],
        en: &'static [&'static str],
    ) -> Outside {
        Outside {
            id,
            section: SectionId::Bar,
            group,
            name,
            keywords: (fr, en),
        }
    }
    const ALL: [Outside; 3] = [
        o(
            FORM_ROW,
            FORM.label,
            words::THE_BAR_S_FORM,
            &[
                "forme",
                "pleine",
                "mini",
                "réduire",
                "automatique",
                "panel-form",
            ],
            &["form", "full", "mini", "narrow", "automatic", "panel-form"],
        ),
        o(
            BY_HAND,
            FORM.label,
            words::CHANGE_FORM_BY_HAND,
            &["super+shift+b", "toggle-bar", "clic", "bord", "raccourci"],
            &["super+shift+b", "toggle-bar", "click", "edge", "shortcut"],
        ),
        o(
            ORDER,
            SHOWS.label,
            words::ORDER_OF_THE_GROUPS,
            &["ordre", "monter", "descendre", "ranger", "order"],
            &["order", "move up", "move down", "arrange", "sort"],
        ),
    ];
    &ALL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_moves_up_or_down_but_not_past_an_end() {
        use BarGroup::{Agents, Tasks, Today};
        let order = BarGroup::DEFAULT;
        assert_eq!(moved(order, Tasks, true), Some([Tasks, Today, Agents]));
        assert_eq!(moved(order, Tasks, false), Some([Today, Agents, Tasks]));
        assert_eq!(moved(order, Today, true), None);
        assert_eq!(moved(order, Agents, false), None);
        assert_eq!(
            say_order([Tasks, Today, Agents], Lang::French),
            "À faire, Aujourd’hui, Les agents"
        );
    }

    #[test]
    fn the_file_says_each_screens_form_and_the_order() -> anyhow::Result<()> {
        let doc: DocumentMut = "[panel]\nform = \"mini\"\norder = [\"agents\"]\n\n\
             [outputs.eDP-1]\npanel-form = \"full\"\n\n[outputs.DP-2]\nscale = 1.5\n\n\
             [[day.calendars]]\nkind = \"fixture\"\n"
            .parse()?;
        let file = File::of(&doc);
        assert_eq!(file.form_of("eDP-1"), "full");
        assert_eq!(file.form_of("DP-2"), "mini");
        assert_eq!(file.named().collect::<Vec<_>>(), ["eDP-1"]);
        assert_eq!(
            file.order(),
            [BarGroup::Agents, BarGroup::Today, BarGroup::Tasks]
        );
        assert!(file.calendars && !file.tasks);
        assert_eq!(File::of(&DocumentMut::new()).form_of("eDP-1"), "auto");
        Ok(())
    }

    #[test]
    fn fc_list_gives_each_family_once_sorted_without_the_bundled_one() {
        let listed = "JetBrains Mono\nDejaVu Sans Mono,DejaVu Sans Mono Book\nMartian Mono\n\
                      dejavu sans mono\nJetBrains Mono\n";
        assert_eq!(
            families(listed),
            ["DejaVu Sans Mono", "dejavu sans mono", "JetBrains Mono"]
        );
    }
}
