//! What the palette lists (#736, docs/design/actions.md §3, §4): with the
//! field empty, the recent actions then every action by category; as the
//! user types, the actions matching it, best first, their matched letters
//! marked; for an action asking for a desktop or an agent, its targets.
//! No result: the line saying so, and the rows that search elsewhere.
//!
//! The launcher's search (`crate::launcher::search`): letters in order,
//! word starts and runs counting more, accents and case ignored; on the
//! action's names in both languages, its keywords and its category.

use hocus_link::{
    ActionsShown,
    actions::{self, Category, Def, Inert, Step},
};

use crate::launcher::search::{fold, fold_map, fuzzy_at};

/// The recent actions shown at most.
pub const RECENT: usize = 5;
/// The best score a match must pass, the launcher's.
const LEAST: f64 = 2.0;
/// The keywords' and the category's weights against a name's.
const KEYWORD: f64 = 0.6;
const CATEGORY: f64 = 0.5;

/// A row of the list.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    /// A heading, as written (shown in capitals).
    Head(String),
    Action(ActionRow),
    Target(TargetRow),
    /// "Aucune action pour « … »."
    Nothing(String),
    /// The same search in the launcher, or in the Settings.
    Elsewhere(Elsewhere),
}

impl Row {
    /// Whether the selection goes to it.
    pub fn selectable(&self) -> bool {
        matches!(self, Self::Action(_) | Self::Target(_) | Self::Elsewhere(_))
    }

    /// Why it cannot run, if it cannot.
    pub fn inert(&self) -> Option<Inert> {
        match self {
            Self::Action(a) => a.inert,
            Self::Target(t) => t.inert,
            _ => None,
        }
    }
}

/// An action's row.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionRow {
    pub def: &'static Def,
    /// A desktop (from 0) given with it ("envoyer 3").
    pub arg: Option<u64>,
    pub label: String,
    /// The label's characters the search matched.
    pub marks: Vec<usize>,
    /// The keyword it was found by, when its name was not.
    pub keyword: Option<String>,
    pub inert: Option<Inert>,
    /// A switch's state.
    pub on: Option<bool>,
}

/// A step's target.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetRow {
    pub target: Target,
    pub label: String,
    /// What it holds ("2 fenêtres"), or its state ("t'attend").
    pub sub: String,
    /// The digit that picks it.
    pub digit: Option<u8>,
    pub inert: Option<Inert>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// A desktop (from 0), its windows sketched (left, top, width, height
    /// in shares of the screen).
    Desktop { index: u8, windows: Vec<[u16; 4]> },
    /// An agent, by the compositor's id, and its state's word.
    Agent { id: u64, state: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elsewhere {
    Launcher,
    Settings,
}

/// The desktops of the target's screen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Desks {
    pub count: u8,
    /// The one the screen shows.
    pub current: u8,
    /// The target window's, if it is on this screen.
    pub window: Option<u8>,
    /// Each one's name, if it has one.
    pub names: Vec<Option<String>>,
    /// Each one's windows, sketched.
    pub windows: Vec<Vec<[u16; 4]>>,
}

/// An agent, as the bar's rows list them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub id: u64,
    pub name: String,
    pub state: String,
    pub waiting: bool,
}

/// What the list is made from besides the catalogue.
#[derive(Debug, Clone, Copy)]
pub struct Seen<'a> {
    pub fr: bool,
    pub shown: &'a ActionsShown,
    /// The recent actions' ids, the latest first.
    pub recent: &'a [String],
    /// Do not disturb is on.
    pub dnd: bool,
    pub agents: &'a [Agent],
    pub desks: &'a Desks,
}

impl Seen<'_> {
    fn row(&self, def: &'static Def) -> ActionRow {
        let inert = self.shown.inert(def.id).or_else(|| match def.id {
            "waiting-agent" if self.agents.is_empty() => Some(Inert::NoAgent),
            "waiting-agent" if !self.agents.iter().any(|a| a.waiting) => Some(Inert::NoneWaiting),
            "go-to-agent" if self.agents.is_empty() => Some(Inert::NoAgent),
            _ => None,
        });
        let on = def.switch.then(|| match def.id {
            "dnd" => self.dnd,
            id => self.shown.is_on(id),
        });
        ActionRow {
            def,
            arg: None,
            label: def.name(self.fr).to_owned(),
            marks: Vec::new(),
            keyword: None,
            inert,
            on,
        }
    }

    /// Desktop `index`'s name as a row says it: "Bureau 3 · Notes".
    pub fn desk_name(&self, index: u8) -> String {
        let n = u32::from(index) + 1;
        let word = if self.fr { "Bureau" } else { "Desktop" };
        match self.desks.names.get(usize::from(index)).cloned().flatten() {
            Some(name) => format!("{word} {n} · {name}"),
            None => format!("{word} {n}"),
        }
    }
}

/// `query` (folded) found whole in `text` (folded), at a word's start if
/// it is there: scored as the fuzzy match scores letters that follow each
/// other, and where its letters are. The launcher's fuzzy match takes
/// each letter at the first word starting with it, which misses "ecran de
/// droite" in "envoyer sur l ecran de droite".
fn whole_at(text: &str, query: &str) -> Option<(f64, Vec<usize>)> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let wanted: Vec<char> = query.chars().collect();
    let starts = (0..chars.len()).filter(|&at| chars[at..].starts_with(&wanted));
    let mut best: Option<usize> = None;
    for at in starts {
        let word = at == 0 || matches!(chars[at - 1], ' ' | '-');
        if word {
            best = Some(at);
            break;
        }
        best.get_or_insert(at);
    }
    let at = best?;
    let word = at == 0 || matches!(chars[at - 1], ' ' | '-');
    let first = match (at, word) {
        (0, _) => 12.0,
        (_, true) => 9.0,
        _ => 0.0,
    };
    let letters = wanted.iter().filter(|c| **c != ' ').count() as f64;
    let score = first + 1.0 + (letters - 1.0).max(0.0) * 7.0 - chars.len() as f64 * 0.05;
    Some((
        score,
        (at..at + wanted.len())
            .filter(|i| chars[*i] != ' ')
            .collect(),
    ))
}

/// The best of the fuzzy match and the whole one.
fn best_at(text: &str, query: &str) -> Option<(f64, Vec<usize>)> {
    match (fuzzy_at(text, query), whole_at(text, query)) {
        (Some(a), Some(b)) => Some(if b.0 > a.0 { b } else { a }),
        (a, b) => a.or(b),
    }
}

/// A row's text as matched: its name's, folded, with where each folded
/// character comes from.
struct Match {
    score: f64,
    marks: Vec<usize>,
    keyword: Option<String>,
}

/// How well `query` (folded) matches `def`: its name in the user's
/// language (its letters marked), in the other, its keywords, its
/// category.
fn matches(def: &Def, query: &str, fr: bool, name: &str) -> Option<Match> {
    let (folded, from) = fold_map(name);
    let own = best_at(&folded, query).map(|(score, at)| Match {
        score,
        marks: at.iter().filter_map(|i| from.get(*i).copied()).collect(),
        keyword: None,
    });
    let other = best_at(&fold(def.name(!fr)), query).map(|(score, _)| Match {
        score,
        marks: Vec::new(),
        keyword: None,
    });
    let keyword = def
        .keywords
        .iter()
        .filter_map(|k| best_at(&fold(k), query).map(|(s, _)| (s * KEYWORD, *k)))
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(score, k)| Match {
            score,
            marks: Vec::new(),
            keyword: Some(k.to_owned()),
        });
    let category = best_at(&fold(def.category.name(fr)), query).map(|(s, _)| Match {
        score: s * CATEGORY,
        marks: Vec::new(),
        keyword: None,
    });
    [own, other, keyword, category]
        .into_iter()
        .flatten()
        .reduce(|a, b| if b.score > a.score { b } else { a })
}

/// A trailing digit, 1 to 9, after a space: the rest and the desktop
/// (from 0). "envoyer 3" → ("envoyer", 2).
fn trailing_desk(query: &str) -> Option<(&str, u8)> {
    let (rest, last) = query.trim_end().rsplit_once(' ')?;
    let digit = last.parse::<u8>().ok().filter(|d| (1..=9).contains(d))?;
    Some((rest.trim_end(), digit - 1)).filter(|(rest, _)| !rest.is_empty())
}

/// Whether the palette lists `def`: the secretary's drawer only in
/// assistant mode (docs/design/secretary.md §21: classic shows nothing of
/// her).
pub fn listed(def: &Def) -> bool {
    def.id != "secretary" || crate::secretary::state() != crate::secretary::State::Off
}

/// The list of actions for `query` (as typed).
pub fn actions(query: &str, seen: &Seen) -> Vec<Row> {
    let catalogue: Vec<&'static Def> = actions::catalogue().iter().filter(|d| listed(d)).collect();
    let mut rows = Vec::new();
    if query.trim().is_empty() {
        let recent: Vec<&'static Def> = seen
            .recent
            .iter()
            .filter_map(|id| actions::find(id))
            .take(RECENT)
            .collect();
        if !recent.is_empty() {
            rows.push(Row::Head(
                if seen.fr { "Récentes" } else { "Recent" }.to_owned(),
            ));
            rows.extend(recent.into_iter().map(|d| Row::Action(seen.row(d))));
        }
        for category in Category::ALL {
            let mut of = catalogue
                .iter()
                .copied()
                .filter(|d| d.category == category)
                .peekable();
            if of.peek().is_none() {
                continue;
            }
            rows.push(Row::Head(category.name(seen.fr).to_owned()));
            rows.extend(of.map(|d| Row::Action(seen.row(d))));
        }
        return rows;
    }
    let typed = fold(query.trim());
    let desk = trailing_desk(&typed);
    // (score, recency, catalogue order, row)
    let mut found: Vec<(f64, usize, usize, ActionRow)> = Vec::new();
    let recency = |id: &str| {
        seen.recent
            .iter()
            .position(|r| r == id)
            .unwrap_or(usize::MAX)
    };
    for (order, def) in catalogue.iter().copied().enumerate() {
        // "envoyer 3": the action with its desktop, first.
        if let (Some((rest, index)), Some(Step::Desktop)) = (desk, def.step)
            && index < seen.desks.count
        {
            let stem = def.stem(seen.fr);
            if let Some(m) = matches(def, rest, seen.fr, stem).filter(|m| m.score > LEAST) {
                let mut row = seen.row(def);
                row.arg = Some(u64::from(index));
                row.label = format!(
                    "{stem} {}",
                    seen.desk_name(index).split_once(' ').map_or("", |(_, n)| n)
                );
                row.marks = m.marks;
                // The window's own desktop, for send and take.
                let here = match def.id {
                    "desktop" => Some(seen.desks.current),
                    _ => seen.desks.window,
                };
                if here == Some(index) && row.inert.is_none() {
                    row.inert = Some(Inert::Current);
                }
                found.push((m.score + 100.0, recency(def.id), order, row));
                continue;
            }
        }
        let label = def.name(seen.fr);
        let Some(m) = matches(def, &typed, seen.fr, label).filter(|m| m.score > LEAST) else {
            continue;
        };
        let mut row = seen.row(def);
        row.marks = m.marks;
        row.keyword = m.keyword;
        found.push((m.score, recency(def.id), order, row));
    }
    found.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    if found.is_empty() {
        let shown = query.trim();
        rows.push(Row::Nothing(if seen.fr {
            format!("Aucune action pour «\u{a0}{shown}\u{a0}».")
        } else {
            format!("No action for \u{201c}{shown}\u{201d}.")
        }));
        rows.push(Row::Elsewhere(Elsewhere::Launcher));
        rows.push(Row::Elsewhere(Elsewhere::Settings));
        return rows;
    }
    rows.extend(found.into_iter().map(|(.., row)| Row::Action(row)));
    rows
}

/// The targets of `def`'s step for `query` (as typed).
pub fn targets(def: &Def, query: &str, seen: &Seen) -> Vec<Row> {
    let typed = fold(query.trim());
    let keep = |label: &str| typed.is_empty() || fuzzy_at(&fold(label), &typed).is_some();
    let mut rows = Vec::new();
    match def.step {
        Some(Step::Desktop) => {
            let here = match def.id {
                "desktop" => Some(seen.desks.current),
                _ => seen.desks.window,
            };
            for index in 0..seen.desks.count {
                let label = seen.desk_name(index);
                if !(keep(&label) || typed == (index + 1).to_string()) {
                    continue;
                }
                let windows = seen
                    .desks
                    .windows
                    .get(usize::from(index))
                    .cloned()
                    .unwrap_or_default();
                let sub = match (windows.len(), seen.fr) {
                    (0, true) => "vide".to_owned(),
                    (0, false) => "empty".to_owned(),
                    (1, true) => "1 fenêtre".to_owned(),
                    (1, false) => "1 window".to_owned(),
                    (n, true) => format!("{n} fenêtres"),
                    (n, false) => format!("{n} windows"),
                };
                rows.push(Row::Target(TargetRow {
                    target: Target::Desktop { index, windows },
                    label,
                    sub,
                    digit: (index < 9).then_some(index + 1),
                    inert: (here == Some(index)).then_some(Inert::Current),
                }));
            }
        }
        Some(Step::Agent) => {
            for agent in seen.agents {
                if !keep(&agent.name) {
                    continue;
                }
                rows.push(Row::Target(TargetRow {
                    target: Target::Agent {
                        id: agent.id,
                        state: agent.state.clone(),
                    },
                    label: agent.name.clone(),
                    sub: agent_state(&agent.state, seen.fr).to_owned(),
                    digit: None,
                    inert: None,
                }));
            }
        }
        None => {}
    }
    rows
}

/// An agent's state as its row says it.
pub fn agent_state(state: &str, fr: bool) -> &'static str {
    match (state, fr) {
        ("waiting", true) => "t’attend",
        ("waiting", false) => "waiting for you",
        ("failed", true) => "bloqué",
        ("failed", false) => "stuck",
        ("done", true) => "à voir",
        ("done", false) => "to see",
        ("working", true) => "travaille",
        ("working", false) => "working",
        (_, true) => "repos",
        (_, false) => "resting",
    }
}

/// The recent actions after `id` ran: first, once each, five at most
/// (kept for the empty field, and for ties).
pub fn ran(recent: &mut Vec<String>, id: &str) {
    recent.retain(|r| r != id);
    recent.insert(0, id.to_owned());
    recent.truncate(RECENT * 4);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown() -> ActionsShown {
        ActionsShown {
            output: "HEADLESS-1".into(),
            scale: 120,
            window: Some(1),
            title: "kitty".into(),
            area: (8, 1800),
            on: vec!["above".into()],
            inert: vec![("move-to-screen-right".into(), Inert::OneScreen)],
            shortcuts: vec![("snap-right".into(), vec!["Super+Right".into()])],
        }
    }

    fn desks() -> Desks {
        Desks {
            count: 4,
            current: 0,
            window: Some(0),
            names: vec![None, Some("Web".into()), Some("Notes".into()), None],
            windows: vec![vec![[0, 0, 500, 1000]], Vec::new(), Vec::new(), Vec::new()],
        }
    }

    fn labels(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                Row::Head(h) => format!("# {h}"),
                Row::Action(a) => a.label.clone(),
                Row::Target(t) => t.label.clone(),
                Row::Nothing(n) => n.clone(),
                Row::Elsewhere(e) => format!("{e:?}"),
            })
            .collect()
    }

    #[test]
    fn the_empty_field_lists_the_recent_then_every_category_in_order() {
        let shown = shown();
        let desks = desks();
        let seen = Seen {
            fr: true,
            shown: &shown,
            recent: &[],
            dnd: false,
            agents: &[],
            desks: &desks,
        };
        let rows = actions("", &seen);
        let heads: Vec<String> = labels(&rows)
            .into_iter()
            .filter(|l| l.starts_with('#'))
            .collect();
        assert_eq!(
            heads,
            [
                "# Fenêtre",
                "# Bureaux",
                "# Écrans",
                "# Agents",
                "# Barre",
                "# Captures",
                "# Accessibilité",
                "# Session",
                "# Réglages"
            ]
        );
        // After "Moitié droite", then "Agrandir": the latest first.
        let mut recent = Vec::new();
        ran(&mut recent, "snap-right");
        ran(&mut recent, "maximize");
        let seen = Seen {
            recent: &recent,
            ..seen
        };
        let rows = actions("", &seen);
        assert_eq!(
            labels(&rows)[..3],
            ["# Récentes", "Agrandir", "Moitié droite"]
        );
        // The switch and the inert one say so.
        let above = rows
            .iter()
            .find_map(|r| match r {
                Row::Action(a) if a.def.id == "above" => Some(a.on),
                _ => None,
            })
            .flatten();
        assert_eq!(above, Some(true));
        let right = rows
            .iter()
            .find(|r| matches!(r, Row::Action(a) if a.def.id == "move-to-screen-right"));
        assert_eq!(right.and_then(Row::inert), Some(Inert::OneScreen));
        // No agent: those that need one are inert.
        let waiting = rows
            .iter()
            .find(|r| matches!(r, Row::Action(a) if a.def.id == "waiting-agent"));
        assert_eq!(waiting.and_then(Row::inert), Some(Inert::NoAgent));
    }

    #[test]
    fn typing_finds_the_names_in_both_languages_marks_them_and_says_a_keyword() {
        let shown = shown();
        let desks = desks();
        let seen = Seen {
            fr: true,
            shown: &shown,
            recent: &[],
            dnd: false,
            agents: &[],
            desks: &desks,
        };
        let rows = actions("bur", &seen);
        let ids: Vec<&str> = rows
            .iter()
            .filter_map(|r| match r {
                Row::Action(a) => Some(a.def.id),
                _ => None,
            })
            .collect();
        // The desktops' actions first; the overview by its keyword.
        let desktops = ids.iter().take(5).filter(|id| {
            let def = actions::find(id).expect("an action");
            def.fr.to_lowercase().contains("bureau")
        });
        assert_eq!(desktops.count(), 5, "{ids:?}");
        let overview = rows
            .iter()
            .find_map(|r| match r {
                Row::Action(a) if a.def.id == "overview" => Some(a.keyword.clone()),
                _ => None,
            })
            .flatten();
        assert_eq!(overview.as_deref(), Some("bureaux"));
        // The letters matched are marked.
        let left = rows
            .iter()
            .find_map(|r| match r {
                Row::Action(a) if a.def.id == "desktop-left" => Some(a.marks.clone()),
                _ => None,
            })
            .expect("desktop-left");
        assert_eq!(left, [0, 1, 2]);
        // In English, the same actions.
        let english = actions("desktop", &seen);
        let found = |rows: &[Row], id: &str| {
            rows.iter()
                .any(|r| matches!(r, Row::Action(a) if a.def.id == id))
        };
        for id in [
            "send-to-desktop",
            "desktop",
            "desktop-left",
            "take-to-desktop",
        ] {
            assert!(found(&english, id), "{id}");
        }
        // Words found whole, past the first words starting with them.
        let right = actions("ecran de droite", &seen);
        assert!(
            matches!(&right[0], Row::Action(a) if a.def.id == "move-to-screen-right"),
            "{:?}",
            labels(&right)
        );
        // Accents and case do not count.
        assert!(found(&actions("ECRAN", &seen), "move-to-screen-left"));
        // Equal scores: the most recent first.
        let recent = vec!["quarter-down".to_owned()];
        let seen_recent = Seen {
            recent: &recent,
            ..seen
        };
        let quarters = actions("quart", &seen_recent);
        assert!(
            matches!(&quarters[0], Row::Action(a) if a.def.id == "quarter-down"),
            "{:?}",
            labels(&quarters)
        );
    }

    #[test]
    fn a_trailing_digit_gives_the_desktop_and_nothing_gives_the_rows_elsewhere() {
        let shown = shown();
        let desks = desks();
        let seen = Seen {
            fr: true,
            shown: &shown,
            recent: &[],
            dnd: false,
            agents: &[],
            desks: &desks,
        };
        let rows = actions("envoyer 3", &seen);
        let Some(Row::Action(first)) = rows.first() else {
            panic!("{:?}", labels(&rows));
        };
        assert_eq!(first.def.id, "send-to-desktop");
        assert_eq!(first.arg, Some(2));
        assert_eq!(first.label, "Envoyer au bureau 3 · Notes");
        // Beyond the desktops there are: no such row.
        let rows = actions("envoyer 7", &seen);
        assert!(
            !rows
                .iter()
                .any(|r| matches!(r, Row::Action(a) if a.arg.is_some()))
        );
        let rows = actions("pizza", &seen);
        assert_eq!(
            labels(&rows),
            [
                "Aucune action pour «\u{a0}pizza\u{a0}».",
                "Launcher",
                "Settings"
            ]
        );
    }

    #[test]
    fn a_steps_targets_are_the_desktops_the_current_one_inert() {
        let shown = shown();
        let desks = desks();
        let seen = Seen {
            fr: true,
            shown: &shown,
            recent: &[],
            dnd: false,
            agents: &[],
            desks: &desks,
        };
        let send = actions::find("send-to-desktop").expect("send");
        let rows = targets(send, "", &seen);
        assert_eq!(
            labels(&rows),
            ["Bureau 1", "Bureau 2 · Web", "Bureau 3 · Notes", "Bureau 4"]
        );
        assert_eq!(rows[0].inert(), Some(Inert::Current));
        assert_eq!(rows[1].inert(), None);
        let Row::Target(first) = &rows[0] else {
            panic!();
        };
        assert_eq!(first.sub, "1 fenêtre");
        assert_eq!(first.digit, Some(1));
        // Typing a name filters them.
        assert_eq!(labels(&targets(send, "not", &seen)), ["Bureau 3 · Notes"]);
        let agents = [Agent {
            id: 4,
            name: "site-web".into(),
            state: "waiting".into(),
            waiting: true,
        }];
        let seen = Seen {
            agents: &agents,
            ..seen
        };
        let go = actions::find("go-to-agent").expect("go");
        let rows = targets(go, "", &seen);
        assert_eq!(labels(&rows), ["site-web"]);
        let Row::Target(agent) = &rows[0] else {
            panic!();
        };
        assert_eq!(agent.sub, "t’attend");
    }
}
