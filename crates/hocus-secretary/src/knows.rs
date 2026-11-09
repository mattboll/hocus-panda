//! "Ce qu'elle sait" made from her memory (docs/design/secretary.md §8,
//! lot 6): the sections the drawer's second tab shows, each item with
//! "d'où je le sais" and what can be done with it. The shell only draws
//! what this makes ([`hocus_secretary_link::knows`]).
//!
//! - **APPRIS CETTE SEMAINE** (§9.3, the weekly digest): what she was
//!   told or accepted these seven days and the user has not kept yet
//!   ("Garder · Oublier"), the proposals waiting ("C'est vrai · Oublier")
//!   and her observations ("Oui · Non · Plus tard").
//! - **PERSONNES**: role, team, a small timeline (arrival, changes,
//!   departure), on leave now, aliases; departed people dashed and "gardé
//!   pour l'histoire". **PROCHES**, the perso notebook's.
//! - **PROJETS ET LEURS NOMS**: aliases as tags, new ones blue for a
//!   week; the people on them.
//! - **RÈGLES**, **SES OUTILS** (a card per source, how much it is used,
//!   its rules), **TOI**, **MAINTENANCE** and **DOSSIERS PERSO** (the
//!   registers, later lots).
//!
//! Only what counts shows as known: a proposal shows as one, "à
//! confirmer", and drives nothing.

use std::collections::{BTreeMap, BTreeSet};

use hocus_secretary_link::{
    Notebook, Provenance,
    knows::{Act, Chip, Kind, Knowledge, Known, Origin, Query, Section, Step, StepTone, Tag, Tone},
};
use jiff::{SignedDuration, Timestamp, civil::Date};
use rusqlite::{OptionalExtension, params};

use crate::{
    fold::fold,
    graph::{Edge, Node, NodeKind, Target},
    learn::{self, Seen, short, shown_source},
    retrieve::day_of,
    store::{Result, Store, stamp, unstamp},
};

/// What the view is made with besides her memory.
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub now: Timestamp,
    pub fr: bool,
    /// Her sources: their names and notebooks.
    pub sources: Vec<(String, Notebook)>,
    /// Her observations.
    pub seen: Vec<Seen>,
}

/// A week, for "Appris cette semaine" and new aliases.
const WEEK: SignedDuration = SignedDuration::from_hours(24 * 7);

/// What the user kept from the week's digest ("Garder"), in `meta`.
const KEPT: &str = "digest-kept";

pub fn kept(store: &Store) -> BTreeSet<String> {
    store
        .meta(KEPT)
        .ok()
        .flatten()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// "Garder": `item` leaves the week's digest.
pub fn keep(store: &Store, item: &str) -> Result<()> {
    let mut all = kept(store);
    all.insert(item.to_owned());
    store.set_meta(KEPT, &serde_json::to_string(&all).unwrap_or_default())
}

struct Words {
    fr: bool,
}

impl Words {
    fn t(&self, fr: &str, en: &str) -> String {
        if self.fr {
            fr.to_owned()
        } else {
            en.to_owned()
        }
    }

    fn day(&self, day: Date) -> String {
        short(day, self.fr)
    }
}

/// An episode found by its pointer: its source, kind, digest, day, what
/// opens it.
/// An episode as "d'où je le sais" reads it: its source, kind, digest,
/// day, and what opens it.
type Read = (Option<String>, String, String, Date, Option<String>);

fn episode(store: &Store, pointer: &str) -> Option<Read> {
    store
        .conn
        .query_row(
            "SELECT source, kind, digest, at, open FROM episodes WHERE pointer = ?1 ORDER BY id DESC LIMIT 1",
            [pointer],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()
        .ok()
        .flatten()
        .map(|(s, k, d, at, open)| (s, k, d, unstamp(&at).map_or_else(|| store.today(), day_of), open))
}

/// The item's id at its source, from its pointer (`fixture-board:CHAP-412`
/// → `CHAP-412`).
fn item_id(pointer: &str) -> &str {
    pointer.rsplit(':').next().unwrap_or(pointer)
}

/// "D'où je le sais" for a fact of `provenance`, from `pointer`, recorded
/// or accepted when they say.
fn origin(
    store: &Store,
    w: &Words,
    provenance: Provenance,
    pointer: &str,
    quote: Option<&str>,
    recorded: Timestamp,
    accepted: Option<Timestamp>,
) -> Origin {
    let when = w.day(day_of(accepted.unwrap_or(recorded)));
    let mut out = Origin {
        provenance: Some(provenance),
        ..Origin::default()
    };
    out.text = match (provenance, accepted) {
        (Provenance::Told, _) => format!("{} · {when}", w.t("tu me l’as dit", "you told me")),
        (Provenance::Extracted, accepted) => {
            let (source, day, open) = match episode(store, pointer) {
                Some((source, _, digest, day, open)) => {
                    out.quote = Some(quote.map_or(digest, ToOwned::to_owned));
                    (source.map(|s| shown_source(&s)), day, open)
                }
                None => (None, day_of(recorded), None),
            };
            out.open = open;
            if out.quote.is_none() {
                out.quote = quote.map(ToOwned::to_owned);
            }
            let source = source.unwrap_or_else(|| item_id(pointer).to_owned());
            let read = if w.fr {
                format!("lu dans {source}, {}", w.day(day))
            } else {
                format!("read in {source}, {}", w.day(day))
            };
            match accepted {
                Some(_) => read,
                None => format!("{read} · {}", w.t("à confirmer", "to confirm")),
            }
        }
        (Provenance::Inferred, Some(_)) if pointer.starts_with("seen:") => format!(
            "{} · {when}",
            w.t("proposé par elle, accepté", "she proposed it, accepted")
        ),
        (Provenance::Inferred, Some(_)) => {
            if w.fr {
                format!("déduit de {}, accepté · {when}", item_id(pointer))
            } else {
                format!("inferred from {}, accepted · {when}", item_id(pointer))
            }
        }
        (Provenance::Inferred, None) => {
            if w.fr {
                format!("déduit de {} · à confirmer", item_id(pointer))
            } else {
                format!("inferred from {} · to confirm", item_id(pointer))
            }
        }
    };
    out
}

fn edge_origin(store: &Store, w: &Words, edge: &Edge) -> Origin {
    origin(
        store,
        w,
        edge.provenance,
        &edge.pointer,
        edge.quote.as_deref(),
        edge.recorded_at,
        edge.accepted_at,
    )
}

/// A node's name, or a value in quotes.
fn target(store: &Store, target: &Target) -> String {
    match target {
        Target::Node(id) => store.node(*id).map(|n| n.name).unwrap_or_default(),
        Target::Value(v) => v.clone(),
    }
}

/// An edge said in a line: "dirige Chapitô", "travaille sur le site".
fn said(store: &Store, w: &Words, edge: &Edge) -> String {
    let to = target(store, &edge.target);
    let date = |v: &str| {
        v.parse::<Date>()
            .map_or_else(|_| v.to_owned(), |d| w.day(d))
    };
    match (edge.kind.as_str(), w.fr) {
        ("leads", true) => format!("dirige {to}"),
        ("leads", false) => format!("leads {to}"),
        ("works_on", true) => format!("travaille sur {to}"),
        ("works_on", false) => format!("works on {to}"),
        ("member_of", true) => format!("équipe {to}"),
        ("member_of", false) => format!("team {to}"),
        ("reviews", true) => format!("relit {to}"),
        ("reviews", false) => format!("reviews {to}"),
        ("replaces", true) => format!("remplace {to}"),
        ("replaces", false) => format!("replaces {to}"),
        ("reports_to", true) => format!("rend compte à {to}"),
        ("reports_to", false) => format!("reports to {to}"),
        ("client_of", true) => format!("client de {to}"),
        ("client_of", false) => format!("client of {to}"),
        ("arrived", true) => format!("arrivée le {}", date(&to)),
        ("arrived", false) => format!("arrived on {}", date(&to)),
        ("left", true) => format!("parti·e le {}", date(&to)),
        ("left", false) => format!("left on {}", date(&to)),
        ("role", _) => to,
        ("likes", true) => format!("aime {to}"),
        ("wishes", true) => format!("aimerait {to}"),
        ("birthday", true) => format!("anniversaire le {to}"),
        (kind, _) => format!("{} {to}", kind.replace('_', " ")),
    }
}

/// The view for `query`.
pub fn view(store: &Store, query: &Query, context: &Context) -> Result<Knowledge> {
    let w = Words { fr: context.fr };
    let today = day_of(context.now);
    let week_ago = context.now.checked_sub(WEEK).unwrap_or(context.now);
    let notebook = query.notebook;
    let in_book = |n: Notebook| notebook.is_none_or(|b| b == n);
    let mut nodes: Vec<Node> = {
        let mut statement = store
            .conn
            .prepare("SELECT id FROM nodes ORDER BY kind, folded")?;
        let ids: Vec<i64> = statement
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        ids.into_iter()
            .filter_map(|id| store.node(id).ok())
            .collect()
    };
    nodes.retain(|n| in_book(n.notebook));
    let leaves: BTreeMap<String, Option<Date>> = learn::leaves(store, today, notebook)?
        .into_iter()
        .map(|l| (l.person, l.last))
        .collect();
    let kept = kept(store);
    let mut week: Vec<Known> = Vec::new();

    // Observations, then proposals, then what was learned.
    for seen in context.seen.iter().filter(|s| s.open(context.now)) {
        let notebook_of = match &seen.noticed {
            learn::Noticed::Arrival { notebook, .. } => *notebook,
            _ => Notebook::Pro,
        };
        if !in_book(notebook_of) {
            continue;
        }
        let (question, evidence) = seen.noticed.question(w.fr);
        week.push(Known {
            item: format!("seen:{}", seen.id),
            title: String::new(),
            text: question,
            origin: Origin {
                provenance: Some(Provenance::Inferred),
                text: format!("{evidence} · {}", w.t("à confirmer", "to confirm")),
                ..Origin::default()
            },
            notebook: Some(notebook_of),
            actions: vec![Act::Accept, Act::Reject, Act::Later],
            proposal: true,
            ..Known::default()
        });
    }
    let mut edges: Vec<Edge> = Vec::new();
    {
        let mut statement = store
            .conn
            .prepare("SELECT id FROM edges WHERE superseded_at IS NULL ORDER BY id")?;
        let ids: Vec<i64> = statement
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for id in ids {
            let edge = store.edge(id)?;
            if in_book(edge.notebook) {
                edges.push(edge);
            }
        }
    }
    // Arrivals this week, and projects named this week: one item each,
    // their edges with them.
    let new_nodes: BTreeSet<i64> = nodes
        .iter()
        .filter(|n| n.created >= week_ago && n.source.is_none())
        .filter(|n| match n.kind {
            NodeKind::Person => edges.iter().any(|e| {
                e.src == n.id
                    && e.kind == "arrived"
                    && !e.is_proposal()
                    && e.valid_from.is_some_and(|d| d >= day_of(week_ago))
            }),
            NodeKind::Project | NodeKind::Client => true,
            _ => false,
        })
        .map(|n| n.id)
        .collect();
    for edge in edges.iter().filter(|e| e.is_proposal()) {
        let who = store.node(edge.src).map(|n| n.name).unwrap_or_default();
        week.push(Known {
            item: format!("edge:{}", edge.id),
            title: who,
            text: format!(" {}", said(store, &w, edge)),
            origin: edge_origin(store, &w, edge),
            notebook: Some(edge.notebook),
            actions: vec![Act::Accept, Act::Reject],
            proposal: true,
            ..Known::default()
        });
    }
    let aliases = aliases(store)?;
    for alias in aliases
        .iter()
        .filter(|a| a.accepted.is_none() && in_book(a.notebook))
    {
        let of = store.node(alias.node).map(|n| n.name).unwrap_or_default();
        week.push(Known {
            item: format!("alias:{}", alias.id),
            title: String::new(),
            text: if w.fr {
                format!("«\u{a0}{}\u{a0}» est un mot de {of}", alias.alias)
            } else {
                format!("“{}” is a word for {of}", alias.alias)
            },
            origin: origin(
                store,
                &w,
                alias.provenance,
                &alias.pointer,
                None,
                alias.since,
                None,
            ),
            notebook: Some(alias.notebook),
            actions: vec![Act::Accept, Act::Reject],
            proposal: true,
            ..Known::default()
        });
    }
    let rules: Vec<learn::Rule> = learn::rules(store)?
        .into_iter()
        .filter(|r| in_book(r.notebook))
        .collect();
    let rule_text = |r: &learn::Rule| match &r.compiled {
        Some(c) => c.words(w.fr).0,
        None => r.sentence.clone(),
    };
    let rule_origin = |r: &learn::Rule| {
        let mut o = origin(
            store,
            &w,
            r.provenance,
            &r.pointer,
            None,
            r.recorded_at,
            r.accepted_at,
        );
        if r.accepted_at.is_some() {
            o.text = format!("{} · {}", w.t("règle", "rule"), o.text);
        }
        o
    };
    for rule in rules.iter().filter(|r| r.accepted_at.is_none()) {
        week.push(Known {
            item: format!("rule:{}", rule.id),
            text: rule_text(rule),
            origin: rule_origin(rule),
            notebook: Some(rule.notebook),
            actions: vec![Act::Accept, Act::Reject],
            proposal: true,
            ..Known::default()
        });
    }
    // What was learned this week, not kept yet.
    for node in nodes.iter().filter(|n| new_nodes.contains(&n.id)) {
        let item = format!("node:{}", node.id);
        if kept.contains(&item)
            || !matches!(
                node.kind,
                NodeKind::Person | NodeKind::Project | NodeKind::Client
            )
        {
            continue;
        }
        let current: Vec<&Edge> = edges
            .iter()
            .filter(|e| e.src == node.id && !e.is_proposal() && e.valid_on(today))
            .collect();
        let team = current
            .iter()
            .find(|e| e.kind == "member_of")
            .map(|e| target(store, &e.target));
        let text = match (node.kind, &team, w.fr) {
            (NodeKind::Person, Some(team), true) => format!(" est arrivé·e, équipe {team}"),
            (NodeKind::Person, None, true) => " est arrivé·e".to_owned(),
            (NodeKind::Person, Some(team), false) => format!(" joined, team {team}"),
            (NodeKind::Person, None, false) => " joined".to_owned(),
            (_, _, true) => " : un projet de plus".to_owned(),
            (_, _, false) => ": one more project".to_owned(),
        };
        let first = current
            .iter()
            .find(|e| e.provenance == Provenance::Told)
            .or(current.first());
        week.push(Known {
            item,
            title: node.name.clone(),
            text,
            origin: first.map_or_else(
                || Origin {
                    provenance: Some(Provenance::Told),
                    text: format!(
                        "{} · {}",
                        w.t("tu me l’as dit", "you told me"),
                        w.day(day_of(node.created))
                    ),
                    ..Origin::default()
                },
                |e| edge_origin(store, &w, e),
            ),
            notebook: Some(node.notebook),
            actions: vec![Act::Keep, Act::Forget],
            ..Known::default()
        });
    }
    for edge in edges.iter().filter(|e| {
        !e.is_proposal()
            && !new_nodes.contains(&e.src)
            && e.accepted_at.unwrap_or(e.recorded_at) >= week_ago
            && e.recorded_at >= week_ago
            && !matches!(e.kind.as_str(), "arrived")
    }) {
        let item = format!("edge:{}", edge.id);
        if kept.contains(&item) {
            continue;
        }
        let who = store.node(edge.src).map(|n| n.name).unwrap_or_default();
        week.push(Known {
            item,
            title: who,
            text: format!(" {}", said(store, &w, edge)),
            origin: edge_origin(store, &w, edge),
            notebook: Some(edge.notebook),
            actions: vec![Act::Keep, Act::Forget],
            ..Known::default()
        });
    }
    for alias in aliases.iter().filter(|a| {
        a.accepted.is_some_and(|t| t >= week_ago)
            && a.provenance != Provenance::Told
            && in_book(a.notebook)
    }) {
        let item = format!("alias:{}", alias.id);
        if kept.contains(&item) {
            continue;
        }
        let of = store.node(alias.node).map(|n| n.name).unwrap_or_default();
        week.push(Known {
            item,
            text: if w.fr {
                format!("«\u{a0}{}\u{a0}» est un mot de {of}", alias.alias)
            } else {
                format!("“{}” is a word for {of}", alias.alias)
            },
            origin: origin(
                store,
                &w,
                alias.provenance,
                &alias.pointer,
                None,
                alias.since,
                alias.accepted,
            ),
            notebook: Some(alias.notebook),
            actions: vec![Act::Keep, Act::Forget],
            ..Known::default()
        });
    }
    for rule in rules
        .iter()
        .filter(|r| r.accepted_at.is_some_and(|t| t >= week_ago))
    {
        let item = format!("rule:{}", rule.id);
        if kept.contains(&item) {
            continue;
        }
        week.push(Known {
            item,
            text: rule_text(rule),
            origin: rule_origin(rule),
            notebook: Some(rule.notebook),
            actions: vec![Act::Keep, Act::Forget],
            ..Known::default()
        });
    }
    for merge in merges(store, week_ago, w.fr)? {
        let item = format!("edit:{}", merge.0);
        if kept.contains(&item) {
            continue;
        }
        week.push(Known {
            item,
            text: merge.1,
            origin: Origin {
                provenance: Some(Provenance::Inferred),
                text: format!(
                    "{} · {}",
                    w.t("rangé la nuit", "tidied at night"),
                    w.day(day_of(merge.2))
                ),
                ..Origin::default()
            },
            actions: vec![Act::Keep, Act::Reject],
            ..Known::default()
        });
    }

    // People, and the close ones (perso).
    let person = |node: &Node| -> Known {
        let mine: Vec<&Edge> = edges
            .iter()
            .filter(|e| e.src == node.id && !e.is_proposal())
            .collect();
        let current: Vec<&&Edge> = mine.iter().filter(|e| e.valid_on(today)).collect();
        let left = mine
            .iter()
            .filter(|e| e.kind == "left")
            .find_map(|e| e.valid_from)
            .filter(|d| *d <= today);
        let arrived = mine
            .iter()
            .find(|e| e.kind == "arrived")
            .and_then(|e| e.valid_from);
        let mut parts: Vec<String> = Vec::new();
        for kind in [
            "role",
            "leads",
            "works_on",
            "member_of",
            "reviews",
            "replaces",
        ] {
            for e in current.iter().filter(|e| e.kind == kind) {
                parts.push(match kind {
                    "role" => target(store, &e.target),
                    "leads" => format!("lead {}", target(store, &e.target)),
                    _ => said(store, &w, e),
                });
            }
        }
        let text = match left {
            Some(_) => format!(" · {}", w.t("parti·e", "left")),
            None if parts.is_empty() => String::new(),
            None => format!(" · {}", parts.join(" · ")),
        };
        let mut tags: Vec<Tag> = aliases
            .iter()
            .filter(|a| {
                a.node == node.id && a.accepted.is_some() && fold(&a.alias) != fold(&node.name)
            })
            .map(|a| Tag {
                text: a.alias.clone(),
                tone: if a.accepted.is_some_and(|t| t >= week_ago) {
                    Tone::New
                } else {
                    Tone::Plain
                },
            })
            .collect();
        if let Some(last) = leaves.get(&node.name) {
            tags.push(Tag {
                text: match (last, w.fr) {
                    (Some(last), true) if *last > today => {
                        format!("en congés jusqu’au {}", last.day())
                    }
                    (Some(last), false) if *last > today => {
                        format!("on leave until {}", w.day(*last))
                    }
                    (_, true) => "en congés".to_owned(),
                    (_, false) => "on leave".to_owned(),
                },
                tone: Tone::Warn,
            });
        }
        let new = arrived.is_some_and(|d| d >= day_of(week_ago)) && left.is_none();
        if new {
            tags.push(Tag {
                text: w.t("nouvelle recrue", "new"),
                tone: Tone::New,
            });
        }
        if node.notebook == Notebook::Perso && notebook.is_none() {
            tags.push(Tag {
                text: "perso".to_owned(),
                tone: Tone::Perso,
            });
        }
        let mut timeline = Vec::new();
        if let Some(day) = arrived {
            timeline.push(Step {
                text: if w.fr {
                    format!("arrivée {}", w.day(day))
                } else {
                    format!("joined {}", w.day(day))
                },
                tone: if new { StepTone::New } else { StepTone::Done },
            });
        }
        for e in mine.iter().filter(|e| e.kind == "leads") {
            if let Some(from) = e.valid_from {
                let to = target(store, &e.target);
                timeline.push(Step {
                    text: match (e.valid_to, w.fr) {
                        (Some(end), true) => format!("lead {to} jusqu’au {}", w.day(end)),
                        (Some(end), false) => format!("lead {to} until {}", w.day(end)),
                        (None, true) => format!("lead {to} depuis {}", w.day(from)),
                        (None, false) => format!("lead {to} since {}", w.day(from)),
                    },
                    tone: StepTone::Done,
                });
            }
        }
        if let Some(day) = left {
            timeline.push(Step {
                text: if w.fr {
                    format!("parti·e le {}", w.day(day))
                } else {
                    format!("left on {}", w.day(day))
                },
                tone: StepTone::Ended,
            });
        }
        let mut lines = Vec::new();
        for e in current
            .iter()
            .filter(|e| matches!(e.kind.as_str(), "likes" | "wishes" | "birthday"))
        {
            let o = edge_origin(store, &w, e);
            lines.push(format!("{} · {}", said(store, &w, e), o.text));
        }
        let shown = mine
            .iter()
            .filter(|e| e.kind != "left")
            .max_by_key(|e| e.recorded_at)
            .copied();
        let origin = match (left, shown) {
            (Some(_), _) => Origin {
                provenance: None,
                text: w.t(
                    "gardé pour l’histoire · on ne te le propose plus",
                    "kept for the record · no longer suggested",
                ),
                ..Origin::default()
            },
            (None, Some(e)) => edge_origin(store, &w, e),
            (None, None) => Origin {
                provenance: None,
                text: match &node.source {
                    Some(source) => {
                        format!("{} {}", w.t("vu dans", "seen in"), shown_source(source))
                    }
                    None => w.t("tu me l’as dit", "you told me"),
                },
                ..Origin::default()
            },
        };
        let actions = if left.is_some() {
            vec![Act::Edit, Act::Forget]
        } else {
            vec![Act::Edit, Act::Close, Act::Forget]
        };
        Known {
            item: format!("node:{}", node.id),
            title: node.name.clone(),
            text,
            tags,
            timeline,
            lines,
            origin,
            notebook: Some(node.notebook),
            actions,
            left: left.is_some(),
            proposal: false,
        }
    };
    let people: Vec<Known> = nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Person && n.notebook == Notebook::Pro)
        .map(person)
        .collect();
    let close: Vec<Known> = nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Person && n.notebook == Notebook::Perso)
        .map(person)
        .collect();

    // Projects and their names.
    let mut projects = Vec::new();
    for node in nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Project | NodeKind::Client))
    {
        let tags: Vec<Tag> = aliases
            .iter()
            .filter(|a| {
                a.node == node.id && a.accepted.is_some() && fold(&a.alias) != fold(&node.name)
            })
            .map(|a| Tag {
                text: a.alias.clone(),
                tone: if a.accepted.is_some_and(|t| t >= week_ago) {
                    Tone::New
                } else {
                    Tone::Plain
                },
            })
            .collect();
        let on: Vec<String> = edges
            .iter()
            .filter(|e| e.target == Target::Node(node.id) && !e.is_proposal() && e.valid_on(today))
            .filter_map(|e| store.node(e.src).ok())
            .filter(|n| n.kind == NodeKind::Person)
            .map(|n| {
                n.name
                    .split_whitespace()
                    .next()
                    .unwrap_or(&n.name)
                    .to_owned()
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut lines = Vec::new();
        if !on.is_empty() {
            lines.push(on.join(", "));
        }
        let text = if node.summary.is_empty() {
            String::new()
        } else {
            format!(" · {}", node.summary)
        };
        projects.push(Known {
            item: format!("node:{}", node.id),
            title: node.name.clone(),
            text,
            tags,
            lines,
            origin: Origin {
                provenance: None,
                text: match &node.source {
                    Some(source) => format!(
                        "{} {} · {}",
                        w.t("vu dans", "seen in"),
                        shown_source(source),
                        w.t(
                            &format!("depuis le {}", w.day(day_of(node.created))),
                            &format!("since {}", w.day(day_of(node.created)))
                        )
                    ),
                    None => w.t(
                        &format!("tu me l’as dit · depuis le {}", w.day(day_of(node.created))),
                        &format!("you told me · since {}", w.day(day_of(node.created))),
                    ),
                },
                ..Origin::default()
            },
            notebook: Some(node.notebook),
            actions: vec![Act::Edit, Act::Forget],
            ..Known::default()
        });
    }

    // Rules that count.
    let rules_known: Vec<Known> = rules
        .iter()
        .filter(|r| r.accepted_at.is_some() && r.valid_to.is_none_or(|to| to > today))
        .map(|r| Known {
            item: format!("rule:{}", r.id),
            text: rule_text(r),
            origin: rule_origin(r),
            notebook: Some(r.notebook),
            actions: vec![Act::Edit, Act::Close, Act::Forget],
            ..Known::default()
        })
        .collect();

    // Her tools: each source, and the tools taught.
    let month = context
        .now
        .checked_sub(SignedDuration::from_hours(24 * 30))
        .unwrap_or(context.now);
    let mut tools = Vec::new();
    let mut tool_rules: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for rule in rules
        .iter()
        .filter(|r| r.accepted_at.is_some() && r.valid_to.is_none_or(|to| to > today))
    {
        let key = match &rule.compiled {
            Some(learn::Compiled::Quiet { tool }) => fold(tool),
            Some(learn::Compiled::Leave { .. } | learn::Compiled::Ignore { .. }) => {
                "agenda".to_owned()
            }
            Some(learn::Compiled::Urgent { .. }) => "gmail".to_owned(),
            _ => continue,
        };
        tool_rules.entry(key).or_default().push(format!(
            "{} · {}",
            rule_text(rule),
            rule_origin(rule).text
        ));
    }
    let mut shown_tools: BTreeSet<String> = BTreeSet::new();
    for (source, book) in &context.sources {
        if !in_book(*book) {
            continue;
        }
        let shown = shown_source(source);
        let count: i64 = store.conn.query_row(
            "SELECT count(*) FROM episodes WHERE (source = ?1 OR source LIKE ?2) AND at >= ?3",
            params![source, format!("{source}:%"), stamp(month)],
            |r| r.get(0),
        )?;
        let ever: i64 = store.conn.query_row(
            "SELECT count(*) FROM episodes WHERE source = ?1 OR source LIKE ?2",
            params![source, format!("{source}:%")],
            |r| r.get(0),
        )?;
        if ever == 0 {
            // Connected, nothing read yet: not a tool of hers yet.
            continue;
        }
        let quiet = tool_rules.get(&fold(&shown)).is_some_and(|r| !r.is_empty());
        let level = if quiet || count <= 3 {
            w.t("à peine", "barely")
        } else if count < 40 {
            w.t("un peu", "some")
        } else {
            w.t("beaucoup", "a lot")
        };
        shown_tools.insert(fold(&shown));
        let mut lines = tool_rules.get(&fold(&shown)).cloned().unwrap_or_default();
        if fold(&shown) == "gmail" || source.contains("google") || source.contains("account") {
            lines.extend(tool_rules.get("agenda").cloned().unwrap_or_default());
        }
        tools.push(Known {
            item: format!("source:{source}"),
            title: shown.clone(),
            text: format!(" · {level}"),
            lines,
            origin: Origin {
                provenance: None,
                text: if w.fr {
                    format!(
                        "{count} élément{} ces 30 jours · lecture seule",
                        if count > 1 { "s" } else { "" }
                    )
                } else {
                    format!(
                        "{count} item{} in 30 days · read only",
                        if count > 1 { "s" } else { "" }
                    )
                },
                ..Origin::default()
            },
            notebook: Some(*book),
            ..Known::default()
        });
    }
    for node in nodes.iter().filter(|n| n.kind == NodeKind::Tool) {
        if shown_tools.contains(&fold(&node.name)) {
            continue;
        }
        let lines = tool_rules
            .get(&fold(&node.name))
            .cloned()
            .unwrap_or_default();
        let quiet = !lines.is_empty();
        tools.push(Known {
            item: format!("node:{}", node.id),
            title: node.name.clone(),
            text: format!(
                " · {}",
                if quiet {
                    w.t("à peine", "barely")
                } else {
                    w.t("connu", "known")
                }
            ),
            lines,
            origin: Origin {
                provenance: Some(Provenance::Told),
                text: w.t("tu m’en as parlé", "you told me about it"),
                ..Origin::default()
            },
            notebook: Some(node.notebook),
            actions: vec![Act::Forget],
            ..Known::default()
        });
    }

    // You: what she knows of the user.
    let mut you = Vec::new();
    for node in nodes.iter().filter(|n| n.kind == NodeKind::User) {
        for e in edges
            .iter()
            .filter(|e| e.src == node.id && !e.is_proposal() && e.valid_on(today))
        {
            you.push(Known {
                item: format!("edge:{}", e.id),
                text: said(store, &w, e),
                origin: edge_origin(store, &w, e),
                notebook: Some(e.notebook),
                actions: vec![Act::Edit, Act::Close, Act::Forget],
                ..Known::default()
            });
        }
    }

    // The registers (later lots).
    let register = |book: Notebook| -> Vec<Known> {
        nodes
            .iter()
            .filter(|n| {
                matches!(n.kind, NodeKind::RegisterItem | NodeKind::Trip) && n.notebook == book
            })
            .map(|n| Known {
                item: format!("node:{}", n.id),
                title: n.name.clone(),
                text: if n.summary.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", n.summary)
                },
                notebook: Some(n.notebook),
                actions: vec![Act::Edit, Act::Forget],
                ..Known::default()
            })
            .collect()
    };
    let maintenance = register(Notebook::Pro);
    let perso_files = register(Notebook::Perso);

    let departed = people.iter().filter(|p| p.left).count();
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let chips = vec![
        (None, w.t("Tout", "All"), 0),
        (
            Some(Kind::People),
            w.t("Personnes", "People"),
            people.len() + close.len(),
        ),
        (
            Some(Kind::Projects),
            w.t("Projets", "Projects"),
            projects.len(),
        ),
        (Some(Kind::Rules), w.t("Règles", "Rules"), rules_known.len()),
        (Some(Kind::Tools), w.t("Outils", "Tools"), tools.len()),
        (Some(Kind::You), w.t("Toi", "You"), you.len()),
        (
            Some(Kind::Maintenance),
            w.t("Maintenance", "Maintenance"),
            maintenance.len(),
        ),
        (
            Some(Kind::PersoFiles),
            w.t("Dossiers perso", "Perso files"),
            perso_files.len(),
        ),
    ];
    let chips: Vec<Chip> = chips
        .into_iter()
        .filter(|(k, _, n)| {
            !matches!(k, Some(Kind::Maintenance | Kind::PersoFiles)) || *n > 0 || query.kind == *k
        })
        .map(|(kind, label, n)| Chip {
            kind,
            label,
            count: count(n),
        })
        .collect();
    let week_len = week.len();
    let mut sections = vec![
        (
            None,
            "week",
            w.t("APPRIS CETTE SEMAINE", "LEARNED THIS WEEK"),
            if w.fr {
                format!("{week_len} · à garder ou oublier")
            } else {
                format!("{week_len} · keep or forget")
            },
            week,
        ),
        (
            Some(Kind::People),
            "people",
            w.t("PERSONNES", "PEOPLE"),
            if departed > 0 {
                if w.fr {
                    format!(
                        "{} · {departed} parti{}",
                        people.len(),
                        if departed > 1 { "es" } else { "e" }
                    )
                } else {
                    format!("{} · {departed} left", people.len())
                }
            } else {
                people.len().to_string()
            },
            people,
        ),
        (
            Some(Kind::Projects),
            "projects",
            w.t("PROJETS ET LEURS NOMS", "PROJECTS AND THEIR NAMES"),
            projects.len().to_string(),
            projects,
        ),
        (
            Some(Kind::Rules),
            "rules",
            w.t("RÈGLES", "RULES"),
            rules_known.len().to_string(),
            rules_known,
        ),
        (
            Some(Kind::Tools),
            "tools",
            w.t("SES OUTILS", "HER TOOLS"),
            tools.len().to_string(),
            tools,
        ),
        (
            Some(Kind::You),
            "you",
            w.t("TOI", "YOU"),
            you.len().to_string(),
            you,
        ),
        (
            Some(Kind::People),
            "close",
            w.t("PROCHES", "CLOSE ONES"),
            close.len().to_string(),
            close,
        ),
        (
            Some(Kind::Maintenance),
            "maintenance",
            w.t("MAINTENANCE", "MAINTENANCE"),
            maintenance.len().to_string(),
            maintenance,
        ),
        (
            Some(Kind::PersoFiles),
            "perso-files",
            w.t("DOSSIERS PERSO", "PERSO FILES"),
            perso_files.len().to_string(),
            perso_files,
        ),
    ];
    let words: Vec<String> = fold(&query.search)
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect();
    let matches = |k: &Known| {
        if words.is_empty() {
            return true;
        }
        let mut hay = format!("{} {} {}", k.title, k.text, k.origin.text);
        for t in &k.tags {
            hay.push(' ');
            hay.push_str(&t.text);
        }
        for l in &k.lines {
            hay.push(' ');
            hay.push_str(l);
        }
        let hay = fold(&hay);
        words.iter().all(|w| hay.contains(w.as_str()))
    };
    let sections: Vec<Section> = sections
        .drain(..)
        .filter(|(kind, key, ..)| match query.kind {
            None => true,
            Some(k) => *kind == Some(k) && *key != "week",
        })
        .map(|(_, key, title, note, mut items)| {
            items.retain(&matches);
            Section {
                key: key.to_owned(),
                title,
                note,
                items,
            }
        })
        .filter(|s| !s.items.is_empty())
        .collect();
    Ok(Knowledge {
        query: query.clone(),
        chips,
        sections,
        total: total(store, notebook)?,
        hidden: 0,
    })
}

/// How many things she knows: nodes, the facts that count now, the rules
/// and the accepted aliases.
fn total(store: &Store, notebook: Option<Notebook>) -> Result<u64> {
    let book = notebook.map(Notebook::word);
    let n: i64 = store.conn.query_row(
        "SELECT (SELECT count(*) FROM nodes WHERE ?1 IS NULL OR notebook = ?1)
              + (SELECT count(*) FROM edges WHERE superseded_at IS NULL AND accepted_at IS NOT NULL
                   AND (?1 IS NULL OR notebook = ?1))
              + (SELECT count(*) FROM rules WHERE accepted_at IS NOT NULL AND (?1 IS NULL OR notebook = ?1))
              + (SELECT count(*) FROM aliases WHERE accepted_at IS NOT NULL AND (?1 IS NULL OR notebook = ?1))",
        [book],
        |r| r.get(0),
    )?;
    Ok(u64::try_from(n).unwrap_or(0))
}

/// An alias as the view reads it.
struct Alias {
    id: i64,
    alias: String,
    node: i64,
    provenance: Provenance,
    pointer: String,
    since: Timestamp,
    accepted: Option<Timestamp>,
    notebook: Notebook,
}

fn aliases(store: &Store) -> Result<Vec<Alias>> {
    let mut statement = store.conn.prepare(
        "SELECT id, alias, node, provenance, pointer, since, accepted_at, notebook FROM aliases ORDER BY id",
    )?;
    let rows = statement.query_map([], |r| {
        Ok(Alias {
            id: r.get(0)?,
            alias: r.get(1)?,
            node: r.get(2)?,
            provenance: Provenance::of(&r.get::<_, String>(3)?).unwrap_or(Provenance::Inferred),
            pointer: r.get(4)?,
            since: unstamp(&r.get::<_, String>(5)?).unwrap_or_default(),
            accepted: r.get::<_, Option<String>>(6)?.and_then(|t| unstamp(&t)),
            notebook: Notebook::of(&r.get::<_, String>(7)?).unwrap_or(Notebook::Pro),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The merges compaction made since `since`, not undone: their edit, what
/// they say ("« Nad » et Nadia Ferreira : une seule fiche"), when.
fn merges(store: &Store, since: Timestamp, fr: bool) -> Result<Vec<(i64, String, Timestamp)>> {
    use crate::store::{Change, Table};
    let mut out = Vec::new();
    for line in store.journal()? {
        if line.op != "merge" || line.undone_by.is_some() || line.at < since {
            continue;
        }
        let gone = line.changes.iter().find_map(|c| match c {
            Change::Deleted {
                table: Table::Nodes,
                before: Some(before),
                ..
            } => before
                .get("name")
                .and_then(|n| n.as_str())
                .map(ToOwned::to_owned),
            _ => None,
        });
        let kept = line.changes.iter().find_map(|c| match c {
            Change::Inserted {
                table: Table::Aliases,
                id,
            }
            | Change::Updated {
                table: Table::Aliases,
                id,
                ..
            } => store
                .conn
                .query_row(
                    "SELECT n.name FROM aliases a JOIN nodes n ON n.id = a.node WHERE a.id = ?1",
                    [id],
                    |r| r.get::<_, String>(0),
                )
                .ok(),
            _ => None,
        });
        let (Some(gone), Some(kept)) = (gone, kept) else {
            continue;
        };
        let text = if fr {
            format!("«\u{a0}{gone}\u{a0}» et {kept}\u{a0}: une seule fiche")
        } else {
            format!("“{gone}” and {kept}: one card")
        };
        out.push((line.id, text, line.at));
    }
    Ok(out)
}
