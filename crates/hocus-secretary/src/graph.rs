//! Her temporal knowledge graph (§7.1): nodes, their aliases, edges with
//! a valid time and a recorded time, rules attached to nodes, the
//! episodes she saw and the summaries of them, and which nodes each
//! mentions. Every item is in a notebook, pro or perso.
//!
//! - **Valid time** is the user's: civil dates, an edge true from
//!   `valid_from` (included) to `valid_to` (excluded), either open.
//! - **Recorded time** is hers: when she learned a version of an edge
//!   (`recorded_at`) and when a newer version replaced it
//!   (`superseded_at`). An edge is never deleted to say it stopped being
//!   true: a new version closes it (`valid_to`), the old one superseded,
//!   both in its lineage. "Who was there then" asks the valid time; "what
//!   did she believe then" the recorded time.
//! - **Provenance**: told (kept at once), extracted (kept at once with its
//!   pointer, but a change to people, projects or rules is a proposal),
//!   inferred (a proposal). A proposal (`accepted_at` empty) drives
//!   nothing: no query of the graph returns it, and it closes nothing.
//!   A told fact, or a proposal accepted, closes the older edge it
//!   contradicts at its own `valid_from`; an extracted one never does.

use hocus_secretary_link::{Notebook, Provenance};
use jiff::{Timestamp, civil::Date};
use rusqlite::{OptionalExtension, ToSql, params};

use crate::{
    fold::fold,
    store::{Change, Error, Item, Result, Store, Table, Who, stamp, unstamp},
};

/// What a node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    /// The user.
    User,
    /// A colleague, a family member, a friend: the relation is an edge.
    Person,
    Project,
    Client,
    /// Slack, Planka, a repository, a server.
    Tool,
    Place,
    Topic,
    Commitment,
    RegisterItem,
    Trip,
    Milestone,
}

impl NodeKind {
    pub const ALL: [Self; 11] = [
        Self::User,
        Self::Person,
        Self::Project,
        Self::Client,
        Self::Tool,
        Self::Place,
        Self::Topic,
        Self::Commitment,
        Self::RegisterItem,
        Self::Trip,
        Self::Milestone,
    ];

    pub const fn word(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Person => "person",
            Self::Project => "project",
            Self::Client => "client",
            Self::Tool => "tool",
            Self::Place => "place",
            Self::Topic => "topic",
            Self::Commitment => "commitment",
            Self::RegisterItem => "register-item",
            Self::Trip => "trip",
            Self::Milestone => "milestone",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.word() == word)
    }
}

/// How many edges of a type can hold at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exclusive {
    /// Any number.
    No,
    /// One per source node: one manager, one home, one birthday.
    PerSource,
    /// One per target node: a project's one lead.
    PerTarget,
}

fn exclusive(kind: &str) -> Exclusive {
    match kind {
        "reports_to" | "lives_in" | "birthday" | "partner_of" => Exclusive::PerSource,
        "leads" => Exclusive::PerTarget,
        _ => Exclusive::No,
    }
}

/// Whether an extracted edge of this type changes people, projects or
/// rules: then it is a proposal until accepted (§7.1). An arrival, a
/// departure and a role are (lot 6).
pub fn structural(kind: &str) -> bool {
    matches!(
        kind,
        "works_on"
            | "member_of"
            | "leads"
            | "reports_to"
            | "client_of"
            | "replaces"
            | "reviews"
            | "arrived"
            | "left"
            | "role"
    )
}

/// A node as she keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: i64,
    pub kind: NodeKind,
    pub name: String,
    pub notebook: Notebook,
    pub summary: String,
    pub created: Timestamp,
    pub source: Option<String>,
}

/// Where an edge goes: another node, or a value ("la céramique", "v2").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Node(i64),
    Value(String),
}

/// Where a fact comes from, and the words that say it.
#[derive(Debug, Clone, PartialEq)]
pub struct Origin {
    pub provenance: Provenance,
    /// The conversation's message, the item's id at its source, or the
    /// edges and episodes an inference was drawn from.
    pub pointer: String,
    /// The source's name, for an extracted fact (forgotten with it).
    pub source: Option<String>,
    /// The quoted words, for an extracted fact.
    pub quote: Option<String>,
    pub confidence: f64,
}

impl Origin {
    pub fn told(pointer: impl Into<String>) -> Self {
        Self {
            provenance: Provenance::Told,
            pointer: pointer.into(),
            source: None,
            quote: None,
            confidence: 1.0,
        }
    }

    pub fn extracted(source: &str, item: &str, quote: impl Into<String>) -> Self {
        Self {
            provenance: Provenance::Extracted,
            pointer: format!("{source}:{item}"),
            source: Some(source.to_owned()),
            quote: Some(quote.into()),
            confidence: 1.0,
        }
    }

    pub fn inferred(from: impl Into<String>, confidence: f64) -> Self {
        Self {
            provenance: Provenance::Inferred,
            pointer: from.into(),
            source: None,
            quote: None,
            confidence,
        }
    }

    fn who(&self) -> Who {
        match (&self.provenance, &self.source) {
            (Provenance::Told, _) => Who::User,
            (Provenance::Extracted, Some(source)) => Who::Source(source.clone()),
            (Provenance::Extracted, None) => Who::Herself,
            (Provenance::Inferred, _) => Who::Inference,
        }
    }
}

/// A new edge.
#[derive(Debug, Clone, PartialEq)]
pub struct NewEdge {
    pub src: i64,
    pub kind: String,
    pub target: Target,
    pub valid_from: Option<Date>,
    pub valid_to: Option<Date>,
    pub origin: Origin,
    pub notebook: Notebook,
}

/// One version of an edge.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub id: i64,
    /// Its first version's id: every version of one fact shares it.
    pub lineage: i64,
    pub src: i64,
    pub kind: String,
    pub target: Target,
    pub valid_from: Option<Date>,
    pub valid_to: Option<Date>,
    pub recorded_at: Timestamp,
    pub superseded_at: Option<Timestamp>,
    pub provenance: Provenance,
    pub pointer: String,
    pub quote: Option<String>,
    pub source: Option<String>,
    pub confidence: f64,
    /// `None`: a proposal.
    pub accepted_at: Option<Timestamp>,
    pub notebook: Notebook,
}

impl Edge {
    /// True on `day`.
    pub fn valid_on(&self, day: Date) -> bool {
        self.valid_from.is_none_or(|from| from <= day) && self.valid_to.is_none_or(|to| to > day)
    }

    pub fn is_proposal(&self) -> bool {
        self.accepted_at.is_none()
    }
}

/// A rule, attached to the node it is about.
#[derive(Debug, Clone, PartialEq)]
pub struct NewRule {
    pub node: Option<i64>,
    /// As the user said it.
    pub sentence: String,
    pub scope: String,
    pub origin: Origin,
    pub notebook: Notebook,
}

/// One thing seen or done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEpisode {
    pub at: Timestamp,
    /// `mail`, `message`, `event`, `question`, `decision`, `catch-up`…
    pub kind: String,
    /// What it was about, in a line: never a whole message.
    pub digest: String,
    pub source: Option<String>,
    /// Its id at its source.
    pub pointer: String,
    pub notebook: Notebook,
}

/// The graph around a node.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Neighbourhood {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

const EDGE_COLUMNS: &str = "id, lineage, src, type, dst, value, valid_from, valid_to, recorded_at, \
    superseded_at, provenance, pointer, quote, source, confidence, accepted_at, notebook";

fn edge_of(r: &rusqlite::Row<'_>) -> rusqlite::Result<Edge> {
    let date = |text: Option<String>| text.and_then(|t| t.parse::<Date>().ok());
    let time = |text: Option<String>| text.and_then(|t| unstamp(&t));
    let dst: Option<i64> = r.get(4)?;
    let value: Option<String> = r.get(5)?;
    Ok(Edge {
        id: r.get(0)?,
        lineage: r.get(1)?,
        src: r.get(2)?,
        kind: r.get(3)?,
        target: match dst {
            Some(dst) => Target::Node(dst),
            None => Target::Value(value.unwrap_or_default()),
        },
        valid_from: date(r.get(6)?),
        valid_to: date(r.get(7)?),
        recorded_at: time(r.get(8)?).unwrap_or_default(),
        superseded_at: time(r.get(9)?),
        provenance: Provenance::of(&r.get::<_, String>(10)?).unwrap_or(Provenance::Inferred),
        pointer: r.get(11)?,
        quote: r.get(12)?,
        source: r.get(13)?,
        confidence: r.get(14)?,
        accepted_at: time(r.get(15)?),
        notebook: Notebook::of(&r.get::<_, String>(16)?).unwrap_or(Notebook::Pro),
    })
}

fn node_of(r: &rusqlite::Row<'_>) -> rusqlite::Result<Node> {
    Ok(Node {
        id: r.get(0)?,
        kind: NodeKind::of(&r.get::<_, String>(1)?).unwrap_or(NodeKind::Topic),
        name: r.get(2)?,
        notebook: Notebook::of(&r.get::<_, String>(3)?).unwrap_or(Notebook::Pro),
        summary: r.get(4)?,
        created: unstamp(&r.get::<_, String>(5)?).unwrap_or_default(),
        source: r.get(6)?,
    })
}

impl Store {
    /// A new node, its name an alias of itself.
    pub fn add_node(
        &mut self,
        kind: NodeKind,
        name: &str,
        notebook: Notebook,
        who: &Who,
        why: &str,
    ) -> Result<(i64, i64)> {
        let source = match who {
            Who::Source(source) => Some(source.clone()),
            _ => None,
        };
        let mut e = self.edit()?;
        let now = stamp(e.now);
        let id = e.insert(
            Table::Nodes,
            "INSERT INTO nodes (kind, name, folded, notebook, created, source) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            &[&kind.word(), &name, &fold(name), &notebook.word(), &now, &source],
        )?;
        let (edit, changes) = e.commit(who, why, None, "add-node")?;
        self.audit_memory(edit, &changes)?;
        Ok((id, edit))
    }

    pub fn node(&self, id: i64) -> Result<Node> {
        self.conn
            .query_row(
                "SELECT id, kind, name, notebook, summary, created, source FROM nodes WHERE id = ?1",
                [id],
                node_of,
            )
            .optional()?
            .ok_or(Error::Missing(Item::Node(id)))
    }

    /// The nodes `text` names, by their name or an accepted alias, folded.
    pub fn find(&self, text: &str) -> Result<Vec<i64>> {
        let folded = fold(text);
        let mut statement = self.conn.prepare(
            "SELECT id FROM nodes WHERE folded = ?1
             UNION SELECT node FROM aliases WHERE folded = ?1 AND accepted_at IS NOT NULL
             ORDER BY 1",
        )?;
        let ids = statement.query_map([folded], |r| r.get(0))?;
        Ok(ids.collect::<rusqlite::Result<_>>()?)
    }

    /// An alias of `node`: kept at once when told; extracted or inferred, a
    /// proposal until accepted (an alias changes how projects and people
    /// are named).
    pub fn add_alias(
        &mut self,
        node: i64,
        alias: &str,
        origin: &Origin,
        notebook: Notebook,
    ) -> Result<(i64, i64)> {
        let accepted = (origin.provenance == Provenance::Told).then(|| stamp(self.now()));
        let mut e = self.edit()?;
        let now = stamp(e.now);
        let id = e.insert(
            Table::Aliases,
            "INSERT INTO aliases (alias, folded, node, provenance, pointer, source, since, accepted_at, notebook)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            &[
                &alias,
                &fold(alias),
                &node,
                &origin.provenance.word(),
                &origin.pointer,
                &origin.source,
                &now,
                &accepted,
                &notebook.word(),
            ],
        )?;
        let (edit, changes) = e.commit(
            &origin.who(),
            "an alias",
            Some(&origin.pointer),
            "add-alias",
        )?;
        self.audit_memory(edit, &changes)?;
        if accepted.is_none() {
            self.audit_proposal(Item::Alias(id))?;
        }
        Ok((id, edit))
    }

    /// Adds an edge: the new version's id and the edit's.
    pub fn add_edge(&mut self, new: &NewEdge) -> Result<(i64, i64)> {
        let kept = match new.origin.provenance {
            Provenance::Told => true,
            Provenance::Extracted => !structural(&new.kind),
            Provenance::Inferred => false,
        };
        let mut e = self.edit()?;
        let now = stamp(e.now);
        let accepted = kept.then(|| now.clone());
        let (dst, value) = match &new.target {
            Target::Node(id) => (Some(*id), None),
            Target::Value(value) => (None, Some(value.clone())),
        };
        let id = e.insert(
            Table::Edges,
            "INSERT INTO edges (lineage, src, type, dst, value, valid_from, valid_to, recorded_at,
                 provenance, pointer, quote, source, confidence, accepted_at, notebook)
             VALUES (0, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            &[
                &new.src,
                &new.kind,
                &dst,
                &value,
                &new.valid_from.map(|d| d.to_string()),
                &new.valid_to.map(|d| d.to_string()),
                &now,
                &new.origin.provenance.word(),
                &new.origin.pointer,
                &new.origin.quote,
                &new.origin.source,
                &new.origin.confidence,
                &accepted,
                &new.notebook.word(),
            ],
        )?;
        // Its own lineage, part of the same insert as far as undo goes.
        e.tx.execute("UPDATE edges SET lineage = ?1 WHERE id = ?1", [id])?;
        if new.origin.provenance == Provenance::Told {
            close_contradicted(&mut e, id)?;
        }
        let (edit, changes) = e.commit(
            &new.origin.who(),
            "an edge",
            Some(&new.origin.pointer),
            "add-edge",
        )?;
        self.audit_memory(edit, &changes)?;
        if !kept {
            self.audit_proposal(Item::Edge(id))?;
        }
        Ok((id, edit))
    }

    pub fn edge(&self, id: i64) -> Result<Edge> {
        self.conn
            .query_row(
                &format!("SELECT {EDGE_COLUMNS} FROM edges WHERE id = ?1"),
                [id],
                edge_of,
            )
            .optional()?
            .ok_or(Error::Missing(Item::Edge(id)))
    }

    /// Accepts a proposal (an edge or an alias): it counts from now, and
    /// an edge closes what it contradicts.
    pub fn accept(&mut self, item: Item) -> Result<i64> {
        let table = match item {
            Item::Edge(_) => Table::Edges,
            Item::Alias(_) => Table::Aliases,
            Item::Rule(_) => Table::Rules,
            other => return Err(Error::Refused(format!("{other:?} is not a proposal"))),
        };
        let accepted: Option<String> = self
            .conn
            .query_row(
                &format!("SELECT accepted_at FROM {} WHERE id = ?1", table.name()),
                [item.id()],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::Missing(item))?;
        if accepted.is_some() {
            return Err(Error::Refused(format!(
                "{} is accepted already",
                item.word()
            )));
        }
        let mut e = self.edit()?;
        let now = stamp(e.now);
        e.update(
            table,
            item.id(),
            &format!("UPDATE {} SET accepted_at = ?1 WHERE id = ?2", table.name()),
            &[&now, &item.id()],
        )?;
        if let Item::Edge(id) = item {
            close_contradicted(&mut e, id)?;
        }
        let (edit, changes) = e.commit(&Who::User, "accepted", None, "accept")?;
        self.audit_memory(edit, &changes)?;
        self.audit_answer(item, "accepted")?;
        Ok(edit)
    }

    /// Rejects a proposal: it goes (it never was a fact); undo brings it
    /// back as a proposal.
    pub fn reject(&mut self, item: Item) -> Result<i64> {
        let table = item.table();
        let accepted: Option<String> = self
            .conn
            .query_row(
                &format!("SELECT accepted_at FROM {} WHERE id = ?1", table.name()),
                [item.id()],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::Missing(item))?;
        if accepted.is_some() {
            return Err(Error::Refused(format!("{} is no proposal", item.word())));
        }
        let mut e = self.edit()?;
        e.delete(table, item.id())?;
        let (edit, changes) = e.commit(&Who::User, "rejected", None, "reject")?;
        self.audit_memory(edit, &changes)?;
        self.audit_answer(item, "rejected")?;
        Ok(edit)
    }

    /// "N'est plus vrai": the edge true until `until` (excluded), a new
    /// version of it; the new version's id and the edit's.
    pub fn close_edge(&mut self, id: i64, until: Date, why: &str) -> Result<(i64, i64)> {
        let mut e = self.edit()?;
        let closed = close_one(&mut e, id, until)?;
        let (edit, changes) = e.commit(&Who::User, why, None, "close-edge")?;
        self.audit_memory(edit, &changes)?;
        Ok((closed, edit))
    }

    /// "C'est perso" (or back to pro): `item` moved to `notebook`.
    pub fn move_to(&mut self, item: Item, notebook: Notebook) -> Result<i64> {
        let table = item.table();
        let mut e = self.edit()?;
        e.update(
            table,
            item.id(),
            &format!("UPDATE {} SET notebook = ?1 WHERE id = ?2", table.name()),
            &[&notebook.word(), &item.id()],
        )?;
        let (edit, changes) = e.commit(
            &Who::User,
            &format!("moved to {}", notebook.word()),
            None,
            "move",
        )?;
        self.audit_memory(edit, &changes)?;
        Ok(edit)
    }

    /// An item's notebook.
    pub fn notebook_of(&self, item: Item) -> Result<Notebook> {
        let word: String = self
            .conn
            .query_row(
                &format!("SELECT notebook FROM {} WHERE id = ?1", item.table().name()),
                [item.id()],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::Missing(item))?;
        Notebook::of(&word).ok_or_else(|| Error::Refused(format!("{word:?} is no notebook")))
    }

    /// A rule attached to the node it is about.
    pub fn add_rule(&mut self, rule: &NewRule) -> Result<(i64, i64)> {
        let kept = rule.origin.provenance == Provenance::Told;
        let mut e = self.edit()?;
        let now = stamp(e.now);
        let accepted = kept.then(|| now.clone());
        let id = e.insert(
            Table::Rules,
            "INSERT INTO rules (node, sentence, scope, provenance, pointer, source, recorded_at, accepted_at, notebook)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            &[
                &rule.node,
                &rule.sentence,
                &rule.scope,
                &rule.origin.provenance.word(),
                &rule.origin.pointer,
                &rule.origin.source,
                &now,
                &accepted,
                &rule.notebook.word(),
            ],
        )?;
        let (edit, changes) = e.commit(
            &rule.origin.who(),
            "a rule",
            Some(&rule.origin.pointer),
            "add-rule",
        )?;
        self.audit_memory(edit, &changes)?;
        if !kept {
            self.audit_proposal(Item::Rule(id))?;
        }
        Ok((id, edit))
    }

    /// The rules attached to `node` that count.
    pub fn rules_of(&self, node: i64) -> Result<Vec<(i64, String)>> {
        let mut statement = self.conn.prepare(
            "SELECT id, sentence FROM rules WHERE node = ?1 AND accepted_at IS NOT NULL ORDER BY id",
        )?;
        let rows = statement.query_map([node], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// One thing seen or done, and the nodes it mentions.
    pub fn add_episode(&mut self, episode: &NewEpisode, mentions: &[i64]) -> Result<(i64, i64)> {
        let who = match &episode.source {
            Some(source) => Who::Source(source.clone()),
            None => Who::Herself,
        };
        let mut e = self.edit()?;
        let now = stamp(e.now);
        let id = e.insert(
            Table::Episodes,
            "INSERT INTO episodes (at, kind, digest, source, pointer, notebook, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            &[
                &stamp(episode.at),
                &episode.kind,
                &episode.digest,
                &episode.source,
                &episode.pointer,
                &episode.notebook.word(),
                &now,
            ],
        )?;
        for node in mentions {
            e.insert(
                Table::Mentions,
                "INSERT INTO mentions (episode, node) VALUES (?1, ?2)",
                &[&id, node],
            )?;
        }
        let (edit, _) = e.commit(&who, "seen", Some(&episode.pointer), "add-episode")?;
        Ok((id, edit))
    }

    /// What a source said of an episode besides its digest: when it ends
    /// (an event), what opens it (for the user's click), its calendar.
    pub fn set_episode_extras(
        &self,
        id: i64,
        ends: Option<Timestamp>,
        open: Option<&str>,
        calendar: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE episodes SET ends = ?1, open = ?2, calendar = ?3 WHERE id = ?4",
            params![ends.map(stamp), open, calendar, id],
        )?;
        Ok(())
    }

    /// A summary of episodes (compaction's, lot 6), and the nodes it
    /// mentions.
    pub fn add_summary(
        &mut self,
        period: &str,
        (start, end): (Date, Date),
        text: &str,
        notebook: Notebook,
        episodes: &[i64],
        mentions: &[i64],
    ) -> Result<(i64, i64)> {
        let mut e = self.edit()?;
        let now = stamp(e.now);
        let id = e.insert(
            Table::Summaries,
            "INSERT INTO summaries (period, start, end, text, notebook, recorded_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            &[&period, &start.to_string(), &end.to_string(), &text, &notebook.word(), &now],
        )?;
        for episode in episodes {
            e.insert(
                Table::SummaryEpisodes,
                "INSERT INTO summary_episodes (summary, episode) VALUES (?1, ?2)",
                &[&id, episode],
            )?;
        }
        for node in mentions {
            e.insert(
                Table::Mentions,
                "INSERT INTO mentions (summary, node) VALUES (?1, ?2)",
                &[&id, node],
            )?;
        }
        let (edit, _) = e.commit(&Who::Herself, "a summary", None, "add-summary")?;
        Ok((id, edit))
    }

    /// The edges that count now around `node`, of `notebook` if given.
    pub fn edges_of(&self, node: i64, day: Date, notebook: Option<Notebook>) -> Result<Vec<Edge>> {
        let day = day.to_string();
        let notebook = notebook.map(Notebook::word);
        let mut statement = self.conn.prepare(&format!(
            "SELECT {EDGE_COLUMNS} FROM edges
             WHERE (src = ?1 OR dst = ?1)
               AND superseded_at IS NULL AND accepted_at IS NOT NULL
               AND (valid_from IS NULL OR valid_from <= ?2)
               AND (valid_to IS NULL OR valid_to > ?2)
               AND (?3 IS NULL OR notebook = ?3)
             ORDER BY id"
        ))?;
        let rows = statement.query_map(params![node, day, notebook], edge_of)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The graph `hops` edges around `start` as of `day`: only edges that
    /// count (accepted, current, valid on that day), of `notebook` if
    /// given.
    pub fn neighbourhood(
        &self,
        start: i64,
        hops: u8,
        day: Date,
        notebook: Option<Notebook>,
    ) -> Result<Neighbourhood> {
        let mut seen = std::collections::BTreeSet::from([start]);
        let mut frontier = vec![start];
        let mut edges: std::collections::BTreeMap<i64, Edge> = std::collections::BTreeMap::new();
        for _ in 0..hops {
            let mut next = Vec::new();
            for node in frontier {
                for edge in self.edges_of(node, day, notebook)? {
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
                    edges.insert(edge.id, edge);
                }
            }
            frontier = next;
        }
        let mut nodes = Vec::new();
        for id in seen {
            let node = self.node(id)?;
            if notebook.is_none_or(|n| n == node.notebook) || id == start {
                nodes.push(node);
            }
        }
        Ok(Neighbourhood {
            nodes,
            edges: edges.into_values().collect(),
        })
    }

    /// Every version of every edge of `node`'s, oldest first: its history,
    /// closed and superseded versions included (proposals too).
    pub fn history(&self, node: i64) -> Result<Vec<Edge>> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {EDGE_COLUMNS} FROM edges WHERE src = ?1 OR dst = ?1 ORDER BY lineage, id"
        ))?;
        let rows = statement.query_map([node], edge_of)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// What she believed at `at` about `node`: the versions recorded by
    /// then and not superseded by then, accepted by then.
    pub fn believed_at(&self, node: i64, at: Timestamp) -> Result<Vec<Edge>> {
        let at = stamp(at);
        let mut statement = self.conn.prepare(&format!(
            "SELECT {EDGE_COLUMNS} FROM edges
             WHERE (src = ?1 OR dst = ?1) AND recorded_at <= ?2
               AND (superseded_at IS NULL OR superseded_at > ?2)
               AND accepted_at IS NOT NULL AND accepted_at <= ?2
             ORDER BY id"
        ))?;
        let rows = statement.query_map(params![node, at], edge_of)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The proposals waiting for the user.
    pub fn proposals(&self) -> Result<Vec<Item>> {
        let mut items = Vec::new();
        for (table, make) in [
            (Table::Edges, Item::Edge as fn(i64) -> Item),
            (Table::Aliases, Item::Alias),
            (Table::Rules, Item::Rule),
        ] {
            let extra = if table == Table::Edges {
                " AND superseded_at IS NULL"
            } else {
                ""
            };
            let mut statement = self.conn.prepare(&format!(
                "SELECT id FROM {} WHERE accepted_at IS NULL{extra} ORDER BY id",
                table.name()
            ))?;
            for id in statement.query_map([], |r| r.get::<_, i64>(0))? {
                items.push(make(id?));
            }
        }
        Ok(items)
    }

    /// Full-text search over episodes and nodes: their items, best first.
    pub fn search(&self, query: &str, notebook: Option<Notebook>) -> Result<Vec<Item>> {
        // Each word a phrase: nothing the user types is FTS syntax.
        let terms: Vec<String> = query
            .split_whitespace()
            .map(|w| format!("\"{}\"", w.replace('"', "")))
            .collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let query = terms.join(" ");
        let notebook = notebook.map(Notebook::word);
        let mut items = Vec::new();
        for (sql, make) in [
            (
                "SELECT n.id FROM nodes_fts f JOIN nodes n ON n.id = f.rowid
                 WHERE nodes_fts MATCH ?1 AND (?2 IS NULL OR n.notebook = ?2) ORDER BY bm25(nodes_fts)",
                Item::Node as fn(i64) -> Item,
            ),
            (
                "SELECT e.id FROM episodes_fts f JOIN episodes e ON e.id = f.rowid
                 WHERE episodes_fts MATCH ?1 AND (?2 IS NULL OR e.notebook = ?2) ORDER BY bm25(episodes_fts)",
                Item::Episode,
            ),
            (
                "SELECT s.id FROM summaries_fts f JOIN summaries s ON s.id = f.rowid
                 WHERE summaries_fts MATCH ?1 AND (?2 IS NULL OR s.notebook = ?2) ORDER BY bm25(summaries_fts)",
                Item::Summary,
            ),
        ] {
            let mut statement = self.conn.prepare(sql)?;
            for id in statement.query_map(params![query, notebook], |r| r.get::<_, i64>(0))? {
                items.push(make(id?));
            }
        }
        Ok(items)
    }

    /// Everything she knows, of one notebook or both, as Markdown: each
    /// node, its aliases, the edges that count now and where she has
    /// them from, and the proposals apart.
    pub fn export_markdown(&self, notebook: Option<Notebook>) -> Result<String> {
        use std::fmt::Write as _;
        let today = self.today();
        let mut out = String::from("# Ce qu'elle sait\n");
        let filter = notebook.map(Notebook::word);
        let mut statement = self.conn.prepare(
            "SELECT id, kind, name, notebook, summary, created, source FROM nodes
             WHERE (?1 IS NULL OR notebook = ?1) ORDER BY kind, folded",
        )?;
        let nodes: Vec<Node> = statement
            .query_map([filter], node_of)?
            .collect::<rusqlite::Result<_>>()?;
        let mut kind = None;
        for node in &nodes {
            if kind != Some(node.kind) {
                kind = Some(node.kind);
                let _ = write!(out, "\n## {}\n", node.kind.word());
            }
            let _ = write!(out, "\n### {} ({})\n", node.name, node.notebook.word());
            if !node.summary.is_empty() {
                let _ = writeln!(out, "\n{}", node.summary);
            }
            let mut aliases = self.conn.prepare(
                "SELECT alias FROM aliases WHERE node = ?1 AND accepted_at IS NOT NULL ORDER BY id",
            )?;
            let names: Vec<String> = aliases
                .query_map([node.id], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            if !names.is_empty() {
                let _ = writeln!(out, "\nAussi : {}", names.join(", "));
            }
            for edge in self.history(node.id)? {
                if edge.src != node.id || edge.superseded_at.is_some() || edge.is_proposal() {
                    continue;
                }
                if notebook.is_some_and(|n| n != edge.notebook) {
                    continue;
                }
                let target = match &edge.target {
                    Target::Node(id) => self.node(*id).map(|n| n.name).unwrap_or_default(),
                    Target::Value(value) => format!("« {value} »"),
                };
                let from = edge.valid_from.map_or("—".to_owned(), |d| d.to_string());
                let to = edge.valid_to.map_or("…".to_owned(), |d| d.to_string());
                let now = if edge.valid_on(today) {
                    ""
                } else {
                    " (plus vrai)"
                };
                let _ = writeln!(
                    out,
                    "- {} → {} [{from}, {to}){now} · {} · {}",
                    edge.kind,
                    target,
                    edge.provenance.word(),
                    edge.pointer
                );
            }
        }
        let proposals = self.proposals()?;
        if !proposals.is_empty() {
            let _ = writeln!(out, "\n## À confirmer\n");
            for item in proposals {
                let _ = writeln!(out, "- {}", item.word());
            }
        }
        Ok(out)
    }
}

/// Closes the edges that `id` (told, or accepted) contradicts, at its
/// `valid_from` (today when it has none).
fn close_contradicted(e: &mut crate::store::Edit<'_>, id: i64) -> Result<()> {
    let new = e.tx.query_row(
        &format!("SELECT {EDGE_COLUMNS} FROM edges WHERE id = ?1"),
        [id],
        edge_of,
    )?;
    let (column, other) = match (exclusive(&new.kind), &new.target) {
        (Exclusive::No, _) => return Ok(()),
        (Exclusive::PerSource, _) => ("src", new.src),
        (Exclusive::PerTarget, Target::Node(dst)) => ("dst", *dst),
        (Exclusive::PerTarget, Target::Value(_)) => return Ok(()),
    };
    let from = new
        .valid_from
        .unwrap_or_else(|| e.now.to_zoned(jiff::tz::TimeZone::system()).date());
    let day = from.to_string();
    let older: Vec<Edge> = {
        let mut statement = e.tx.prepare(&format!(
            "SELECT {EDGE_COLUMNS} FROM edges
             WHERE id != ?1 AND type = ?2 AND {column} = ?3
               AND superseded_at IS NULL AND accepted_at IS NOT NULL
               AND (valid_to IS NULL OR valid_to > ?4)
               AND (valid_from IS NULL OR valid_from < ?4)"
        ))?;
        let args: [&dyn ToSql; 4] = [&id, &new.kind, &other, &day];
        let rows = statement.query_map(args.as_slice(), edge_of)?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for edge in older {
        if edge.target == new.target && edge.src == new.src {
            continue;
        }
        close_one(e, edge.id, from)?;
    }
    Ok(())
}

/// A new version of edge `id` that ends at `until`, the old one
/// superseded: the new version's id.
fn close_one(e: &mut crate::store::Edit<'_>, id: i64, until: Date) -> Result<i64> {
    let edge: Edge =
        e.tx.query_row(
            &format!("SELECT {EDGE_COLUMNS} FROM edges WHERE id = ?1"),
            [id],
            edge_of,
        )
        .optional()?
        .ok_or(Error::Missing(Item::Edge(id)))?;
    if edge.superseded_at.is_some() {
        return Err(Error::Refused(format!("edge {id} is an old version")));
    }
    let now = stamp(e.now);
    e.update(
        Table::Edges,
        id,
        "UPDATE edges SET superseded_at = ?1 WHERE id = ?2",
        &[&now, &id],
    )?;
    e.insert(
        Table::Edges,
        "INSERT INTO edges (lineage, src, type, dst, value, valid_from, valid_to, recorded_at,
             superseded_at, provenance, pointer, quote, source, confidence, accepted_at, notebook)
         SELECT lineage, src, type, dst, value, valid_from, ?2, ?3,
             NULL, provenance, pointer, quote, source, confidence, accepted_at, notebook
         FROM edges WHERE id = ?1",
        &[&id, &until.to_string(), &now],
    )
}

/// The rows an edit changed, as the audit log names them.
pub(crate) fn changed_ids(changes: &[Change]) -> Vec<String> {
    changes
        .iter()
        .map(|c| {
            let (table, id) = c.row();
            format!("{}:{id}", table.name())
        })
        .collect()
}
