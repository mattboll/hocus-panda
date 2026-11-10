//! Hybrid retrieval over her memory (§7.3), in code, for a question:
//!
//! 1. **Entity linking**: the question's words and phrases against every
//!    node's name and accepted alias, accents and case folded, a small
//!    edit distance allowed on long words, a person's first name when it
//!    is only one person's.
//! 2. **Time**: the day the question is about, resolved in code: a date
//!    said ("le 15 avril 2026", "2026-04-15", "en mars 2026", "l'an
//!    dernier"), or an event of the graph ("quand on a livré la v2": the
//!    linked node's `shipped_on` "v2" edge's date). The graph is then read
//!    **as of** that day.
//! 3. **Graph traversal**: from the linked nodes, two hops of the edges
//!    that count on that day; the relations the question names ("qui
//!    relit", "l'équipe de", "qui remplace") are followed two hops
//!    further, so a chain of them is found whole.
//! 4. **Full text**: FTS5's BM25 over what she saw and her summaries,
//!    any of the question's words.
//! 5. **Rank fusion** (reciprocal ranks), with a boost for what is near
//!    the day asked about, then cut to the budget.
//!
//! Embeddings (§7.3, 4) are optional and not built.

use std::collections::{BTreeMap, BTreeSet};

use hocus_secretary_link::Notebook;
use jiff::{Timestamp, ToSpan, civil::Date, tz::TimeZone};
use rusqlite::params;

use crate::{
    fold::fold,
    graph::{Edge, Node, NodeKind, Target},
    inert,
    store::{Item, Result, Store, unstamp},
};

/// Words never linked to a node on their own.
const STOP: &[&str] = &[
    "le", "la", "les", "un", "une", "des", "de", "du", "d", "l", "qui", "que", "quoi", "quand",
    "on", "a", "à", "au", "aux", "et", "ou", "en", "est", "sur", "pour", "par", "avec", "dans",
    "the", "a", "an", "of", "to", "in", "on", "is", "was", "who", "when", "what", "and", "or",
    "je", "tu", "il", "elle", "nous", "vous", "ils", "mon", "ma", "mes", "ton", "ta", "tes", "son",
    "sa", "ses", "ce", "cet", "cette", "ces", "était", "etait", "ete", "été", "c", "qu", "s", "n",
    "ne", "pas", "plus", "moi", "toi",
];

/// A word of the question that names a relation, and the edge type.
const RELATIONS: &[(&str, &str)] = &[
    ("remplac", "replaces"),
    ("replac", "replaces"),
    ("relit", "reviews"),
    ("relis", "reviews"),
    ("relir", "reviews"),
    ("relec", "reviews"),
    ("review", "reviews"),
    ("equipe", "member_of"),
    ("team", "member_of"),
    ("membre", "member_of"),
    ("dirig", "leads"),
    ("lead", "leads"),
    ("responsable", "leads"),
    ("travaill", "works_on"),
    ("client", "client_of"),
    ("manager", "reports_to"),
    ("chef", "reports_to"),
];

/// A verb of the question that names an event, and the edge types whose
/// date it is.
const EVENTS: &[(&str, &[&str])] = &[
    ("livr", &["shipped_on"]),
    ("sorti", &["shipped_on"]),
    ("ship", &["shipped_on"]),
    ("releas", &["shipped_on"]),
    ("mis en prod", &["shipped_on"]),
    ("mise en prod", &["shipped_on"]),
    ("lanc", &["shipped_on"]),
];

const MONTHS: [&str; 12] = [
    "janvier",
    "fevrier",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "aout",
    "septembre",
    "octobre",
    "novembre",
    "decembre",
];

/// The graph's base depth (§7.3: k = 2).
pub const HOPS: u8 = 2;
/// How much further the relations the question names are followed.
const FURTHER: u8 = 2;

/// An episode as retrieval reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Episode {
    pub id: i64,
    pub at: Timestamp,
    pub kind: String,
    pub digest: String,
    pub source: Option<String>,
    pub pointer: String,
    pub notebook: Notebook,
}

fn episode_of(r: &rusqlite::Row<'_>) -> rusqlite::Result<Episode> {
    Ok(Episode {
        id: r.get(0)?,
        at: unstamp(&r.get::<_, String>(1)?).unwrap_or_default(),
        kind: r.get(2)?,
        digest: r.get(3)?,
        source: r.get(4)?,
        pointer: r.get(5)?,
        notebook: Notebook::of(&r.get::<_, String>(6)?).unwrap_or(Notebook::Pro),
    })
}

const EPISODE_COLUMNS: &str = "id, at, kind, digest, source, pointer, notebook";

pub fn episode(store: &Store, id: i64) -> Result<Option<Episode>> {
    use rusqlite::OptionalExtension;
    Ok(store
        .conn
        .query_row(
            &format!("SELECT {EPISODE_COLUMNS} FROM episodes WHERE id = ?1"),
            [id],
            episode_of,
        )
        .optional()?)
}

/// The episodes between two instants, of `notebook` if given, oldest
/// first, `most` at most (the latest).
pub fn episodes_between(
    store: &Store,
    (from, to): (Timestamp, Timestamp),
    notebook: Option<Notebook>,
    most: usize,
) -> Result<Vec<Episode>> {
    let mut statement = store.conn.prepare(&format!(
        "SELECT {EPISODE_COLUMNS} FROM episodes
         WHERE at >= ?1 AND at < ?2 AND (?3 IS NULL OR notebook = ?3)
         ORDER BY at DESC, id DESC LIMIT ?4"
    ))?;
    let rows = statement.query_map(
        params![
            crate::store::stamp(from),
            crate::store::stamp(to),
            notebook.map(Notebook::word),
            i64::try_from(most).unwrap_or(i64::MAX)
        ],
        episode_of,
    )?;
    let mut out: Vec<Episode> = rows.collect::<rusqlite::Result<_>>()?;
    out.reverse();
    Ok(out)
}

/// What arrived between two instants (events and catch-ups left out),
/// of `notebook` if given, oldest first, `most` at most (the latest):
/// filtered before the limit, so the day's events never crowd it out.
pub fn arrived_between(
    store: &Store,
    (from, to): (Timestamp, Timestamp),
    notebook: Option<Notebook>,
    most: usize,
) -> Result<Vec<Episode>> {
    let mut statement = store.conn.prepare(&format!(
        "SELECT {EPISODE_COLUMNS} FROM episodes
         WHERE at >= ?1 AND at < ?2 AND (?3 IS NULL OR notebook = ?3)
           AND kind NOT IN ('event', 'catch-up')
         ORDER BY at DESC, id DESC LIMIT ?4"
    ))?;
    let rows = statement.query_map(
        params![
            crate::store::stamp(from),
            crate::store::stamp(to),
            notebook.map(Notebook::word),
            i64::try_from(most).unwrap_or(i64::MAX)
        ],
        episode_of,
    )?;
    let mut out: Vec<Episode> = rows.collect::<rusqlite::Result<_>>()?;
    out.reverse();
    Ok(out)
}

/// The day an instant falls on, for the user.
pub fn day_of(at: Timestamp) -> Date {
    at.to_zoned(TimeZone::system()).date()
}

/// An episode as the model reads it: one line, whatever its digest (a
/// mail's subject, the triage's words on it) and its pointer say
/// ([`inert`]).
pub fn episode_line(episode: &Episode) -> String {
    inert::line(&format!(
        "- {} {} · {} ({})",
        day_of(episode.at),
        episode.kind,
        episode.digest,
        episode.pointer
    ))
    .into_owned()
}

fn name(store: &Store, id: i64) -> String {
    store.node(id).map(|n| n.name).unwrap_or_default()
}

/// An edge as the model reads it, with where she has it from.
pub fn edge_line(store: &Store, edge: &Edge) -> String {
    let target = match &edge.target {
        Target::Node(id) => name(store, *id),
        Target::Value(value) => format!("« {value} »"),
    };
    let from = edge.valid_from.map_or("—".to_owned(), |d| d.to_string());
    let to = edge.valid_to.map_or("…".to_owned(), |d| d.to_string());
    let how = match edge.provenance {
        hocus_secretary_link::Provenance::Told => "dit par toi",
        hocus_secretary_link::Provenance::Extracted => "lu dans une source",
        hocus_secretary_link::Provenance::Inferred => "déduit, confirmé",
    };
    // Its names and its value may have been read in a source.
    inert::line(&format!(
        "{} —{}→ {} [{from}, {to}) · {how} · edge:{}",
        name(store, edge.src),
        edge.kind,
        target,
        edge.id
    ))
    .into_owned()
}

/// The question's content words, folded.
fn words(text: &str) -> Vec<String> {
    fold(text)
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|w| !w.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn stop(word: &str) -> bool {
    STOP.contains(&word)
}

/// Full text over episodes, summaries and nodes: any of the content
/// words, best first, `most` of each.
pub fn search_any(
    store: &Store,
    query: &str,
    notebook: Option<Notebook>,
    most: usize,
) -> Result<Vec<Item>> {
    let terms: Vec<String> = words(query)
        .into_iter()
        .filter(|w| !stop(w) && w.chars().count() >= 2)
        .map(|w| format!("\"{}\"", w.replace('"', "")))
        .collect();
    if terms.is_empty() {
        return Ok(Vec::new());
    }
    let query = terms.join(" OR ");
    let notebook = notebook.map(Notebook::word);
    let most = i64::try_from(most).unwrap_or(i64::MAX);
    let mut lists: Vec<Vec<Item>> = Vec::new();
    for (sql, make) in [
        (
            "SELECT e.id FROM episodes_fts f JOIN episodes e ON e.id = f.rowid
             WHERE episodes_fts MATCH ?1 AND (?2 IS NULL OR e.notebook = ?2)
             ORDER BY bm25(episodes_fts) LIMIT ?3",
            Item::Episode as fn(i64) -> Item,
        ),
        (
            "SELECT s.id FROM summaries_fts f JOIN summaries s ON s.id = f.rowid
             WHERE summaries_fts MATCH ?1 AND (?2 IS NULL OR s.notebook = ?2)
             ORDER BY bm25(summaries_fts) LIMIT ?3",
            Item::Summary,
        ),
        (
            "SELECT n.id FROM nodes_fts f JOIN nodes n ON n.id = f.rowid
             WHERE nodes_fts MATCH ?1 AND (?2 IS NULL OR n.notebook = ?2)
             ORDER BY bm25(nodes_fts) LIMIT ?3",
            Item::Node,
        ),
    ] {
        let mut statement = store.conn.prepare(sql)?;
        let ids = statement.query_map(params![query, notebook, most], |r| r.get::<_, i64>(0))?;
        lists.push(
            ids.map(|id| id.map(make))
                .collect::<rusqlite::Result<_>>()?,
        );
    }
    // Interleaved by rank: the best of each first.
    let mut out = Vec::new();
    let longest = lists.iter().map(Vec::len).max().unwrap_or_default();
    for i in 0..longest {
        for list in &lists {
            if let Some(item) = list.get(i) {
                out.push(*item);
            }
        }
    }
    Ok(out)
}

/// A node named in the question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linked {
    pub node: i64,
    /// The words that named it.
    pub said: String,
    /// 0: exactly (folded); more: by so many edits.
    pub distance: usize,
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let here = row[j + 1];
            row[j + 1] = (previous + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(row[j + 1] + 1);
            previous = here;
        }
    }
    row[b.len()]
}

/// Every name and accepted alias, folded, and its node.
fn names(store: &Store) -> Result<Vec<(String, i64, NodeKind)>> {
    let mut statement = store.conn.prepare(
        "SELECT folded, id, kind FROM nodes
         UNION SELECT a.folded, a.node, n.kind FROM aliases a JOIN nodes n ON n.id = a.node
             WHERE a.accepted_at IS NOT NULL",
    )?;
    let rows = statement.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            NodeKind::of(&r.get::<_, String>(2)?).unwrap_or(NodeKind::Topic),
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The nodes the question names (step 1).
pub fn link(store: &Store, question: &str) -> Result<Vec<Linked>> {
    let words = words(question);
    let names = names(store)?;
    let exact: BTreeMap<&str, Vec<i64>> =
        names.iter().fold(BTreeMap::new(), |mut m, (n, id, _)| {
            m.entry(n.as_str()).or_insert_with(Vec::new).push(*id);
            m
        });
    let mut found: BTreeMap<i64, Linked> = BTreeMap::new();
    let mut add = |node: i64, said: &str, distance: usize| {
        let better = found.get(&node).is_none_or(|l| l.distance > distance);
        if better {
            found.insert(
                node,
                Linked {
                    node,
                    said: said.to_owned(),
                    distance,
                },
            );
        }
    };
    let mut used = BTreeSet::new();
    // Longest phrases first: "le site web" before "site".
    for n in (1..=4).rev() {
        for (i, window) in words.windows(n).enumerate() {
            if (i..i + n).any(|w| used.contains(&w)) {
                continue;
            }
            if n == 1 && stop(&window[0]) {
                continue;
            }
            let phrase = window.join(" ");
            if let Some(ids) = exact.get(phrase.as_str()) {
                for id in ids {
                    add(*id, &phrase, 0);
                }
                used.extend(i..i + n);
            }
        }
    }
    for (i, word) in words.iter().enumerate() {
        if used.contains(&i) || stop(word) {
            continue;
        }
        let length = word.chars().count();
        // A person's first name, when only one person has it.
        let first: Vec<i64> = names
            .iter()
            .filter(|(n, _, kind)| {
                *kind == NodeKind::Person && n.split(' ').next() == Some(word.as_str()) && n != word
            })
            .map(|(_, id, _)| *id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if let [only] = first.as_slice() {
            add(*only, word, 0);
            continue;
        }
        let most = match length {
            0..=4 => continue,
            5..=8 => 1,
            _ => 2,
        };
        for (name, id, _) in &names {
            if name.contains(' ') {
                continue;
            }
            let d = levenshtein(word, name);
            if d > 0 && d <= most {
                add(*id, word, d);
            }
        }
    }
    Ok(found.into_values().collect())
}

/// The day a question is about, and how code found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsOf {
    pub day: Date,
    pub why: String,
    /// The edge that dated it, when an event did.
    pub edge: Option<i64>,
}

/// The day the question is about (step 2), from `today`.
pub fn when(store: &Store, question: &str, linked: &[Linked], today: Date) -> Result<Option<AsOf>> {
    let folded = fold(question);
    let words = words(question);
    // A date said.
    for (i, w) in words.iter().enumerate() {
        if let Ok(day) = w.parse::<Date>() {
            return Ok(Some(AsOf {
                day,
                why: format!("la date dite, {day}"),
                edge: None,
            }));
        }
        if let Some(month) = MONTHS.iter().position(|m| m == w) {
            let year = words.get(i + 1).and_then(|y| y.parse::<i16>().ok());
            let day = i
                .checked_sub(1)
                .and_then(|p| words.get(p))
                .and_then(|d| d.parse::<i8>().ok());
            if let Some(year) = year {
                let month = i8::try_from(month + 1).unwrap_or(1);
                let (d, why) = match day {
                    Some(d) => (d, "la date dite"),
                    None => (15, "le milieu du mois dit"),
                };
                if let Ok(date) = Date::new(year, month, d) {
                    return Ok(Some(AsOf {
                        day: date,
                        why: format!("{why}, {date}"),
                        edge: None,
                    }));
                }
            }
        }
    }
    if ["l'an dernier", "l'annee derniere", "last year"]
        .iter()
        .any(|p| folded.contains(p))
    {
        let day = today.checked_sub(1.year()).unwrap_or(today);
        return Ok(Some(AsOf {
            day,
            why: format!("« l'an dernier » : un an avant aujourd'hui, {day}"),
            edge: None,
        }));
    }
    // An event of the graph: "quand on a livré la v2".
    let asks_when = ["quand ", "when ", "lors de", "au moment"]
        .iter()
        .any(|p| folded.contains(p));
    if !asks_when {
        return Ok(None);
    }
    let kinds: BTreeSet<&str> = EVENTS
        .iter()
        .filter(|(verb, _)| folded.contains(verb))
        .flat_map(|(_, kinds)| kinds.iter().copied())
        .collect();
    if kinds.is_empty() {
        return Ok(None);
    }
    for linked in linked {
        for edge in store.history(linked.node)? {
            if edge.src != linked.node
                || edge.superseded_at.is_some()
                || edge.is_proposal()
                || !kinds.contains(edge.kind.as_str())
            {
                continue;
            }
            let (Target::Value(value), Some(day)) = (&edge.target, edge.valid_from) else {
                continue;
            };
            let value = fold(value);
            if words.contains(&value) {
                return Ok(Some(AsOf {
                    day,
                    why: format!(
                        "{} {} « {value} » le {day} (edge:{})",
                        name(store, edge.src),
                        edge.kind,
                        edge.id
                    ),
                    edge: Some(edge.id),
                }));
            }
        }
    }
    Ok(None)
}

/// The relations the question names.
fn relations(question: &str) -> BTreeSet<&'static str> {
    let words = words(question);
    RELATIONS
        .iter()
        .filter(|(stem, _)| words.iter().any(|w| w.starts_with(stem)))
        .map(|(_, kind)| *kind)
        .collect()
}

/// The graph around the linked nodes (step 3): each edge with the hops
/// it took to reach it.
pub fn traverse(
    store: &Store,
    starts: &[i64],
    day: Date,
    notebook: Option<Notebook>,
    question: &str,
) -> Result<Vec<(Edge, u8)>> {
    let named = relations(question);
    let deepest = if named.is_empty() {
        HOPS
    } else {
        HOPS + FURTHER
    };
    let mut seen: BTreeSet<i64> = starts.iter().copied().collect();
    let mut frontier: Vec<i64> = starts.to_vec();
    let mut edges: BTreeMap<i64, (Edge, u8)> = BTreeMap::new();
    for hop in 1..=deepest {
        let mut next = Vec::new();
        for node in frontier {
            for edge in store.edges_of(node, day, notebook)? {
                if hop > HOPS && !named.contains(edge.kind.as_str()) {
                    continue;
                }
                let other = match edge.target {
                    Target::Node(dst) if dst != node => Some(dst),
                    Target::Node(_) if edge.src != node => Some(edge.src),
                    _ => None,
                };
                if let Some(other) = other
                    && seen.insert(other)
                {
                    next.push(other);
                }
                edges.entry(edge.id).or_insert((edge, hop));
            }
        }
        frontier = next;
    }
    let mut out: Vec<(Edge, u8)> = edges.into_values().collect();
    out.sort_by_key(|(e, hop)| (*hop, e.id));
    Ok(out)
}

/// What retrieval found for a question, ranked.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Found {
    pub linked: Vec<Linked>,
    pub as_of: Option<AsOf>,
    /// Ranked best first: each a line and the item it shows.
    pub lines: Vec<Ranked>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ranked {
    pub score: f64,
    pub line: String,
    pub item: String,
    /// An episode's pointer: its source item, maybe held with its text.
    pub pointer: Option<String>,
}

/// Reciprocal rank fusion's constant.
const K: f64 = 60.0;

/// Steps 1 to 5 for `question`, of `notebook` if given.
pub fn retrieve(
    store: &Store,
    question: &str,
    today: Date,
    notebook: Option<Notebook>,
) -> Result<Found> {
    let linked = link(store, question)?;
    let as_of = when(store, question, &linked, today)?;
    let day = as_of.as_ref().map_or(today, |a| a.day);
    let starts: Vec<i64> = linked.iter().map(|l| l.node).collect();
    let mut scores: BTreeMap<String, Ranked> = BTreeMap::new();
    let mut add = |item: String, line: String, pointer: Option<String>, score: f64| {
        scores
            .entry(item.clone())
            .and_modify(|r| r.score += score)
            .or_insert(Ranked {
                score,
                line,
                item,
                pointer,
            });
    };
    for (rank, (edge, _)) in traverse(store, &starts, day, notebook, question)?
        .iter()
        .enumerate()
    {
        add(
            format!("edge:{}", edge.id),
            edge_line(store, edge),
            None,
            1.0 / (K + rank as f64),
        );
    }
    // What mentions the linked nodes, latest first.
    let mut mentioned = Vec::new();
    if !starts.is_empty() {
        let marks: Vec<String> = starts.iter().map(i64::to_string).collect();
        let mut statement = store.conn.prepare(&format!(
            "SELECT DISTINCT episode FROM mentions WHERE episode IS NOT NULL AND node IN ({})
             ORDER BY episode DESC LIMIT 50",
            marks.join(", ")
        ))?;
        for id in statement.query_map([], |r| r.get::<_, i64>(0))? {
            mentioned.push(Item::Episode(id?));
        }
    }
    let texts = search_any(store, question, notebook, 50)?;
    for list in [mentioned, texts] {
        for (rank, item) in list.iter().enumerate() {
            let base = 1.0 / (K + rank as f64);
            match *item {
                Item::Episode(id) => {
                    let Some(e) = episode(store, id)? else {
                        continue;
                    };
                    if notebook.is_some_and(|n| n != e.notebook) {
                        continue;
                    }
                    // Near the day asked about counts more.
                    let days = (day_of(e.at) - day).get_days().unsigned_abs();
                    let near = 1.0 / (K * (1.0 + f64::from(days) / 30.0));
                    add(
                        item.word(),
                        episode_line(&e),
                        Some(e.pointer.clone()),
                        base + near,
                    );
                }
                Item::Summary(id) => {
                    let summary: Option<(String, String, String)> = {
                        use rusqlite::OptionalExtension;
                        store
                            .conn
                            .query_row(
                                "SELECT start, end, text FROM summaries WHERE id = ?1 AND stale = 0",
                                [id],
                                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                            )
                            .optional()?
                    };
                    if let Some((start, end, text)) = summary {
                        add(
                            item.word(),
                            inert::line(&format!("- résumé {start} → {end} : {text}")).into_owned(),
                            None,
                            base,
                        );
                    }
                }
                Item::Node(id) => {
                    let node: Node = store.node(id)?;
                    if notebook.is_some_and(|n| n != node.notebook) {
                        continue;
                    }
                    let mut line = format!("- fiche : {} ({})", node.name, node.kind.word());
                    if !node.summary.is_empty() {
                        line.push_str(" : ");
                        line.push_str(&node.summary);
                    }
                    add(item.word(), inert::line(&line).into_owned(), None, base);
                }
                _ => {}
            }
        }
    }
    let mut lines: Vec<Ranked> = scores.into_values().collect();
    lines.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.item.cmp(&b.item)));
    Ok(Found {
        linked,
        as_of,
        lines,
    })
}
