//! Her memory (docs/design/secretary.md §7.1): one SQLite database in
//! `$XDG_DATA_HOME/hocuspanda/secretary/memory.db`, encrypted page by page
//! by SQLCipher with a key from the keyring ([`crate::key`]), the FTS5
//! indexes included.
//!
//! Event-sourced: every change goes through an [`Edit`], one transaction
//! that writes the rows and one line of the journal saying who made it
//! (the user, a source, an inference), when, why, the item it came from,
//! and each row it inserted, updated (as it was before) or deleted (as it
//! was). Undo plays an edit's changes backwards, as a new edit. Forgetting
//! is the only deletion that undo cannot bring back: the rows go, and the
//! journal keeps only that a forget happened (the snapshots of what was
//! forgotten are erased from it).
//!
//! The graph's own operations are in [`crate::graph`], the audit log in
//! [`crate::audit`].

use std::path::Path;

use hocus_secretary_link::Notebook;
use jiff::Timestamp;
use rusqlite::{Connection, OptionalExtension, ToSql, Transaction, params, types::Value};
use serde_json::{Map, Value as Json, json};

use crate::clock::Clock;

/// The schema's version, in `meta`.
const SCHEMA: i64 = 1;

const SCHEMA_SQL: &str = r#"
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);

CREATE TABLE nodes (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    folded TEXT NOT NULL,
    notebook TEXT NOT NULL CHECK (notebook IN ('pro', 'perso')),
    summary TEXT NOT NULL DEFAULT '',
    created TEXT NOT NULL,
    source TEXT
);
CREATE INDEX nodes_folded ON nodes (folded);

CREATE TABLE aliases (
    id INTEGER PRIMARY KEY,
    alias TEXT NOT NULL,
    folded TEXT NOT NULL,
    node INTEGER NOT NULL REFERENCES nodes (id),
    provenance TEXT NOT NULL CHECK (provenance IN ('told', 'extracted', 'inferred')),
    pointer TEXT NOT NULL,
    source TEXT,
    since TEXT NOT NULL,
    accepted_at TEXT,
    notebook TEXT NOT NULL CHECK (notebook IN ('pro', 'perso'))
);
CREATE INDEX aliases_folded ON aliases (folded);
CREATE INDEX aliases_node ON aliases (node);

CREATE TABLE edges (
    id INTEGER PRIMARY KEY,
    lineage INTEGER NOT NULL,
    src INTEGER NOT NULL REFERENCES nodes (id),
    type TEXT NOT NULL,
    dst INTEGER REFERENCES nodes (id),
    value TEXT,
    valid_from TEXT,
    valid_to TEXT,
    recorded_at TEXT NOT NULL,
    superseded_at TEXT,
    provenance TEXT NOT NULL CHECK (provenance IN ('told', 'extracted', 'inferred')),
    pointer TEXT NOT NULL,
    quote TEXT,
    source TEXT,
    confidence REAL NOT NULL DEFAULT 1.0,
    accepted_at TEXT,
    notebook TEXT NOT NULL CHECK (notebook IN ('pro', 'perso')),
    CHECK ((dst IS NULL) <> (value IS NULL))
);
CREATE INDEX edges_src ON edges (src);
CREATE INDEX edges_dst ON edges (dst);
CREATE INDEX edges_lineage ON edges (lineage);

CREATE TABLE rules (
    id INTEGER PRIMARY KEY,
    node INTEGER REFERENCES nodes (id),
    sentence TEXT NOT NULL,
    compiled TEXT,
    scope TEXT NOT NULL DEFAULT '',
    valid_from TEXT,
    valid_to TEXT,
    provenance TEXT NOT NULL CHECK (provenance IN ('told', 'extracted', 'inferred')),
    pointer TEXT NOT NULL,
    source TEXT,
    recorded_at TEXT NOT NULL,
    accepted_at TEXT,
    notebook TEXT NOT NULL CHECK (notebook IN ('pro', 'perso'))
);

CREATE TABLE episodes (
    id INTEGER PRIMARY KEY,
    at TEXT NOT NULL,
    kind TEXT NOT NULL,
    digest TEXT NOT NULL,
    source TEXT,
    pointer TEXT NOT NULL,
    notebook TEXT NOT NULL CHECK (notebook IN ('pro', 'perso')),
    recorded_at TEXT NOT NULL
);
CREATE INDEX episodes_at ON episodes (at);

CREATE TABLE summaries (
    id INTEGER PRIMARY KEY,
    period TEXT NOT NULL CHECK (period IN ('daily', 'weekly', 'monthly')),
    start TEXT NOT NULL,
    end TEXT NOT NULL,
    text TEXT NOT NULL,
    stale INTEGER NOT NULL DEFAULT 0,
    notebook TEXT NOT NULL CHECK (notebook IN ('pro', 'perso')),
    recorded_at TEXT NOT NULL
);

CREATE TABLE summary_episodes (
    id INTEGER PRIMARY KEY,
    summary INTEGER NOT NULL REFERENCES summaries (id),
    episode INTEGER NOT NULL REFERENCES episodes (id)
);

CREATE TABLE mentions (
    id INTEGER PRIMARY KEY,
    episode INTEGER REFERENCES episodes (id),
    summary INTEGER REFERENCES summaries (id),
    node INTEGER NOT NULL REFERENCES nodes (id),
    CHECK ((episode IS NULL) <> (summary IS NULL))
);
CREATE INDEX mentions_node ON mentions (node);

CREATE TABLE journal (
    id INTEGER PRIMARY KEY,
    at TEXT NOT NULL,
    who TEXT NOT NULL,
    why TEXT NOT NULL,
    came_from TEXT,
    op TEXT NOT NULL,
    changes TEXT NOT NULL,
    undoes INTEGER,
    undone_by INTEGER
);

CREATE TABLE audit (
    id INTEGER PRIMARY KEY,
    at TEXT NOT NULL,
    kind TEXT NOT NULL
        CHECK (kind IN ('read', 'call', 'proposal', 'answer', 'delegation', 'memory')),
    what TEXT NOT NULL,
    ids TEXT NOT NULL,
    provider TEXT,
    model TEXT,
    tokens_in INTEGER,
    tokens_out INTEGER,
    duration_ms INTEGER,
    outcome TEXT
);
CREATE INDEX audit_at ON audit (at);
CREATE TRIGGER audit_append_only BEFORE UPDATE ON audit
BEGIN SELECT RAISE(ABORT, 'the audit log is append-only'); END;

CREATE TABLE cursors (
    source TEXT PRIMARY KEY,
    cursor TEXT NOT NULL,
    synced_at TEXT NOT NULL
);

CREATE VIRTUAL TABLE episodes_fts USING fts5 (
    digest, content = 'episodes', content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER episodes_ai AFTER INSERT ON episodes BEGIN
    INSERT INTO episodes_fts (rowid, digest) VALUES (new.id, new.digest);
END;
CREATE TRIGGER episodes_ad AFTER DELETE ON episodes BEGIN
    INSERT INTO episodes_fts (episodes_fts, rowid, digest) VALUES ('delete', old.id, old.digest);
END;
CREATE TRIGGER episodes_au AFTER UPDATE ON episodes BEGIN
    INSERT INTO episodes_fts (episodes_fts, rowid, digest) VALUES ('delete', old.id, old.digest);
    INSERT INTO episodes_fts (rowid, digest) VALUES (new.id, new.digest);
END;

CREATE VIRTUAL TABLE nodes_fts USING fts5 (
    name, summary, content = 'nodes', content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER nodes_ai AFTER INSERT ON nodes BEGIN
    INSERT INTO nodes_fts (rowid, name, summary) VALUES (new.id, new.name, new.summary);
END;
CREATE TRIGGER nodes_ad AFTER DELETE ON nodes BEGIN
    INSERT INTO nodes_fts (nodes_fts, rowid, name, summary)
        VALUES ('delete', old.id, old.name, old.summary);
END;
CREATE TRIGGER nodes_au AFTER UPDATE ON nodes BEGIN
    INSERT INTO nodes_fts (nodes_fts, rowid, name, summary)
        VALUES ('delete', old.id, old.name, old.summary);
    INSERT INTO nodes_fts (rowid, name, summary) VALUES (new.id, new.name, new.summary);
END;

CREATE VIRTUAL TABLE summaries_fts USING fts5 (
    text, content = 'summaries', content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER summaries_ai AFTER INSERT ON summaries BEGIN
    INSERT INTO summaries_fts (rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER summaries_ad AFTER DELETE ON summaries BEGIN
    INSERT INTO summaries_fts (summaries_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;
CREATE TRIGGER summaries_au AFTER UPDATE ON summaries BEGIN
    INSERT INTO summaries_fts (summaries_fts, rowid, text) VALUES ('delete', old.id, old.text);
    INSERT INTO summaries_fts (rowid, text) VALUES (new.id, new.text);
END;
"#;

/// What can go wrong with her memory.
#[derive(Debug)]
pub enum Error {
    Sql(rusqlite::Error),
    /// The key does not open the database.
    WrongKey,
    /// The item is not there (anymore).
    Missing(Item),
    /// What was asked does not hold: a proposal accepted twice, an undo
    /// of an edit a later one depends on.
    Refused(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sql(err) => write!(f, "{err}"),
            Self::WrongKey => f.write_str("the key does not open her memory"),
            Self::Missing(item) => write!(f, "{item:?} is not in her memory"),
            Self::Refused(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sql(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// The tables whose rows are items of her memory or their links, each
/// with an integer `id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Table {
    Mentions,
    SummaryEpisodes,
    Aliases,
    Rules,
    Edges,
    Episodes,
    Summaries,
    Nodes,
}

impl Table {
    /// In the order rows are deleted: links before what they link.
    pub const ALL: [Self; 8] = [
        Self::Mentions,
        Self::SummaryEpisodes,
        Self::Aliases,
        Self::Rules,
        Self::Edges,
        Self::Episodes,
        Self::Summaries,
        Self::Nodes,
    ];

    /// The tables of items, each with its notebook.
    pub const ITEMS: [Self; 6] = [
        Self::Nodes,
        Self::Aliases,
        Self::Edges,
        Self::Rules,
        Self::Episodes,
        Self::Summaries,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Mentions => "mentions",
            Self::SummaryEpisodes => "summary_episodes",
            Self::Aliases => "aliases",
            Self::Rules => "rules",
            Self::Edges => "edges",
            Self::Episodes => "episodes",
            Self::Summaries => "summaries",
            Self::Nodes => "nodes",
        }
    }

    fn of(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == name)
    }
}

/// An item of her memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Item {
    Node(i64),
    Alias(i64),
    Edge(i64),
    Rule(i64),
    Episode(i64),
    Summary(i64),
}

impl Item {
    pub const fn table(self) -> Table {
        match self {
            Self::Node(_) => Table::Nodes,
            Self::Alias(_) => Table::Aliases,
            Self::Edge(_) => Table::Edges,
            Self::Rule(_) => Table::Rules,
            Self::Episode(_) => Table::Episodes,
            Self::Summary(_) => Table::Summaries,
        }
    }

    pub const fn id(self) -> i64 {
        match self {
            Self::Node(id)
            | Self::Alias(id)
            | Self::Edge(id)
            | Self::Rule(id)
            | Self::Episode(id)
            | Self::Summary(id) => id,
        }
    }

    /// As the audit log and the journal name it: `edge:12`.
    pub fn word(self) -> String {
        let kind = match self {
            Self::Node(_) => "node",
            Self::Alias(_) => "alias",
            Self::Edge(_) => "edge",
            Self::Rule(_) => "rule",
            Self::Episode(_) => "episode",
            Self::Summary(_) => "summary",
        };
        format!("{kind}:{}", self.id())
    }
}

/// Who made a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Who {
    User,
    Source(String),
    Inference,
    /// Her own bookkeeping (a catch-up, compaction).
    Herself,
}

impl Who {
    pub fn word(&self) -> String {
        match self {
            Self::User => "user".to_owned(),
            Self::Source(name) => format!("source:{name}"),
            Self::Inference => "inference".to_owned(),
            Self::Herself => "herself".to_owned(),
        }
    }
}

/// A line of the edit journal.
#[derive(Debug, Clone, PartialEq)]
pub struct Journaled {
    pub id: i64,
    pub at: Timestamp,
    pub who: String,
    pub why: String,
    pub came_from: Option<String>,
    pub op: String,
    pub changes: Vec<Change>,
    pub undoes: Option<i64>,
    pub undone_by: Option<i64>,
}

/// One row an edit changed.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Inserted {
        table: Table,
        id: i64,
    },
    /// `before`: the row as it was; `None` once what it held is forgotten.
    Updated {
        table: Table,
        id: i64,
        before: Option<Map<String, Json>>,
    },
    Deleted {
        table: Table,
        id: i64,
        before: Option<Map<String, Json>>,
    },
}

impl Change {
    pub fn row(&self) -> (Table, i64) {
        match self {
            Self::Inserted { table, id }
            | Self::Updated { table, id, .. }
            | Self::Deleted { table, id, .. } => (*table, *id),
        }
    }

    fn to_json(&self) -> Json {
        match self {
            Self::Inserted { table, id } => {
                json!({"op": "insert", "table": table.name(), "id": id})
            }
            Self::Updated { table, id, before } => {
                json!({"op": "update", "table": table.name(), "id": id, "before": before})
            }
            Self::Deleted { table, id, before } => {
                json!({"op": "delete", "table": table.name(), "id": id, "before": before})
            }
        }
    }

    fn from_json(value: &Json) -> Option<Self> {
        let table = Table::of(value.get("table")?.as_str()?)?;
        let id = value.get("id")?.as_i64()?;
        let before = value.get("before").and_then(Json::as_object).cloned();
        Some(match value.get("op")?.as_str()? {
            "insert" => Self::Inserted { table, id },
            "update" => Self::Updated { table, id, before },
            "delete" => Self::Deleted { table, id, before },
            _ => return None,
        })
    }
}

/// Her memory.
#[derive(Debug)]
pub struct Store {
    pub(crate) conn: Connection,
    pub(crate) clock: Clock,
}

/// A time as her memory writes it: UTC, to the microsecond, always the
/// same length, so that text order is time order.
pub fn stamp(at: Timestamp) -> String {
    format!(
        "{}.{:06}Z",
        at.strftime("%Y-%m-%dT%H:%M:%S"),
        at.subsec_microsecond()
    )
}

pub fn unstamp(text: &str) -> Option<Timestamp> {
    text.parse().ok()
}

impl Store {
    /// Opens (or makes) her memory at `path`, sealed with `key`.
    pub fn open(path: &Path, key: &[u8; 32], clock: Clock) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::keyed(conn, Some(key), clock)
    }

    /// Her memory in memory only, unencrypted: tests.
    pub fn in_memory(clock: Clock) -> Result<Self> {
        Self::keyed(Connection::open_in_memory()?, None, clock)
    }

    fn keyed(conn: Connection, key: Option<&[u8; 32]>, clock: Clock) -> Result<Self> {
        if let Some(key) = key {
            let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
            // A raw key: SQLCipher derives nothing from it. A wrong key is
            // said by the error below, not on standard error.
            conn.execute_batch(&format!(
                "PRAGMA cipher_log_level = NONE; PRAGMA key = \"x'{hex}'\";"
            ))?;
            // The first read of a database the key does not open fails.
            if conn
                .query_row("SELECT count(*) FROM sqlite_master", [], |r| {
                    r.get::<_, i64>(0)
                })
                .is_err()
            {
                return Err(Error::WrongKey);
            }
        }
        conn.execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;",
        )?;
        let store = Self { conn, clock };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        let made: bool = self.conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'meta'",
            [],
            |r| r.get::<_, i64>(0).map(|n| n > 0),
        )?;
        if !made {
            self.conn.execute_batch(&format!(
                "BEGIN; {SCHEMA_SQL} INSERT INTO meta (key, value) VALUES ('schema', '{SCHEMA}'); COMMIT;"
            ))?;
            self.conn.execute_batch(crate::day::SQL)?;
            return Ok(());
        }
        let version: i64 = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| {
                r.get::<_, String>(0)
            })?
            .parse()
            .unwrap_or(0);
        if version != SCHEMA {
            return Err(Error::Refused(format!(
                "her memory's schema is {version}, this program reads {SCHEMA}"
            )));
        }
        // Her day's records (lot 5): tables added beside, made if missing.
        self.conn.execute_batch(crate::day::SQL)?;
        Ok(())
    }

    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }

    /// Today, in the user's time zone.
    pub fn today(&self) -> jiff::civil::Date {
        self.now().to_zoned(jiff::tz::TimeZone::system()).date()
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    /// A source's sync token, and when it last synced.
    pub fn cursor(&self, source: &str) -> Result<Option<(String, String)>> {
        Ok(self
            .conn
            .query_row(
                "SELECT cursor, synced_at FROM cursors WHERE source = ?1",
                [source],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    pub fn set_cursor(&self, source: &str, cursor: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO cursors (source, cursor, synced_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (source) DO UPDATE SET cursor = excluded.cursor, synced_at = excluded.synced_at",
            params![source, cursor, stamp(self.now())],
        )?;
        Ok(())
    }

    /// How many rows `table` holds.
    pub fn count(&self, table: &str) -> Result<u64> {
        // `table` is one of ours, never the user's.
        let n: i64 = self
            .conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
        Ok(u64::try_from(n).unwrap_or(0))
    }

    pub fn counts(&self) -> Result<hocus_secretary_link::Counts> {
        Ok(hocus_secretary_link::Counts {
            nodes: self.count("nodes")?,
            aliases: self.count("aliases")?,
            edges: self.count("edges")?,
            rules: self.count("rules")?,
            episodes: self.count("episodes")?,
            summaries: self.count("summaries")?,
            journal: self.count("journal")?,
            audit: self.count("audit")?,
        })
    }

    /// Items without a notebook, or with one that is neither: none, ever
    /// (the schema refuses them; this checks it).
    pub fn items_without_notebook(&self) -> Result<u64> {
        let mut n = 0;
        for table in Table::ITEMS {
            n += self.conn.query_row(
                &format!(
                    "SELECT count(*) FROM {} WHERE notebook IS NULL OR notebook NOT IN ('pro', 'perso')",
                    table.name()
                ),
                [],
                |r| r.get::<_, i64>(0),
            )?;
        }
        Ok(u64::try_from(n).unwrap_or(0))
    }

    /// Starts an edit: one transaction, one line of the journal.
    pub(crate) fn edit(&mut self) -> Result<Edit<'_>> {
        let now = self.clock.now();
        Ok(Edit {
            tx: self.conn.transaction()?,
            changes: Vec::new(),
            now,
        })
    }

    /// The journal, oldest first.
    pub fn journal(&self) -> Result<Vec<Journaled>> {
        let mut statement = self.conn.prepare(
            "SELECT id, at, who, why, came_from, op, changes, undoes, undone_by
             FROM journal ORDER BY id",
        )?;
        let rows = statement.query_map([], |r| {
            let changes: String = r.get(6)?;
            Ok(Journaled {
                id: r.get(0)?,
                at: unstamp(&r.get::<_, String>(1)?).unwrap_or_default(),
                who: r.get(2)?,
                why: r.get(3)?,
                came_from: r.get(4)?,
                op: r.get(5)?,
                changes: serde_json::from_str::<Vec<Json>>(&changes)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(Change::from_json)
                    .collect(),
                undoes: r.get(7)?,
                undone_by: r.get(8)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Undoes the edit `edit` (an undo too: that redoes): its changes
    /// played backwards, as a new edit, whose id is returned. Refused when
    /// it is undone already, when what it changed was forgotten since, or
    /// when a later edit changed the same rows (undo that one first).
    pub fn undo(&mut self, edit: i64) -> Result<i64> {
        let journal = self.journal()?;
        let Some(line) = journal.iter().find(|l| l.id == edit) else {
            return Err(Error::Refused(format!("no edit {edit}")));
        };
        if line.undone_by.is_some() {
            return Err(Error::Refused(format!("edit {edit} is undone already")));
        }
        if line.op == "forget" {
            return Err(Error::Refused("a forget cannot be undone".to_owned()));
        }
        let rows: Vec<(Table, i64)> = line.changes.iter().map(Change::row).collect();
        // A later edit undone, or the undo of a later edit, cancels out;
        // a redo (the undo of an undo) does not.
        let cancels = |l: &Journaled| {
            l.undone_by.is_some()
                || l.undoes.is_some_and(|u| {
                    u > edit && journal.iter().any(|o| o.id == u && o.op != "undo")
                })
        };
        let later = journal.iter().find(|l| {
            l.id > edit && !cancels(l) && l.changes.iter().any(|c| rows.contains(&c.row()))
        });
        if let Some(later) = later {
            return Err(Error::Refused(format!(
                "edit {} changed the same items since: undo it first",
                later.id
            )));
        }
        let (undo, changes) = {
            let mut e = self.edit()?;
            for change in line.changes.iter().rev() {
                match change {
                    Change::Inserted { table, id } => e.delete(*table, *id)?,
                    Change::Updated {
                        table,
                        id,
                        before: Some(before),
                    } => e.restore_update(*table, *id, before)?,
                    Change::Deleted {
                        table,
                        before: Some(before),
                        ..
                    } => e.restore_insert(*table, before)?,
                    Change::Updated { before: None, .. } | Change::Deleted { before: None, .. } => {
                        return Err(Error::Refused(
                            "what this edit changed was forgotten since".to_owned(),
                        ));
                    }
                }
            }
            let why = format!("undo of edit {edit}");
            let undo = e.commit_journal(
                &Who::User,
                &why,
                line.came_from.as_deref(),
                "undo",
                Some(edit),
            )?;
            e.tx.execute(
                "UPDATE journal SET undone_by = ?1 WHERE id = ?2",
                params![undo, edit],
            )?;
            let changes = e.changes;
            e.tx.commit()?;
            (undo, changes)
        };
        self.audit_memory(undo, &changes)?;
        Ok(undo)
    }

    /// Undoes the last edit not undone yet (and not an undo).
    pub fn undo_last(&mut self) -> Result<i64> {
        let last = self
            .journal()?
            .into_iter()
            .rev()
            .find(|l| l.undone_by.is_none() && l.op != "undo" && l.op != "forget");
        match last {
            Some(line) => self.undo(line.id),
            None => Err(Error::Refused("nothing to undo".to_owned())),
        }
    }

    /// Forgets `item`: it goes, with what hangs on it (a node's aliases,
    /// edges, rules and mentions; an edge's every version), from the
    /// tables and the indexes; summaries that cited it are emptied until
    /// rebuilt; the journal keeps only that a forget happened.
    pub fn forget(&mut self, item: Item) -> Result<()> {
        let exists: bool = self.conn.query_row(
            &format!("SELECT count(*) FROM {} WHERE id = ?1", item.table().name()),
            [item.id()],
            |r| r.get::<_, i64>(0).map(|n| n > 0),
        )?;
        if !exists {
            return Err(Error::Missing(item));
        }
        self.purge(vec![(item.table(), item.id())], "an item")
    }

    /// Forgets every item of `notebook` ("Oublier tout le perso"); the
    /// other notebook stays as it is, but for its edges to a forgotten
    /// node.
    pub fn forget_notebook(&mut self, notebook: Notebook) -> Result<()> {
        let rows = self.rows_where("notebook = ?1", &[&notebook.word()])?;
        self.purge(rows, &format!("the {} notebook", notebook.word()))?;
        crate::day::forget_notebook(&self.conn, notebook)?;
        Ok(())
    }

    /// Forgets everything a source brought (its consent revoked), and
    /// where it was in its sync.
    pub fn forget_source(&mut self, source: &str) -> Result<()> {
        let rows = self.rows_where("source = ?1", &[&source])?;
        self.purge(rows, "a source")?;
        self.conn
            .execute("DELETE FROM cursors WHERE source = ?1", [source])?;
        crate::day::forget_source(&self.conn, source)?;
        Ok(())
    }

    /// The items' rows matching `filter` in every table of items.
    fn rows_where(&self, filter: &str, args: &[&dyn ToSql]) -> Result<Vec<(Table, i64)>> {
        let mut rows = Vec::new();
        for table in Table::ITEMS {
            if table == Table::Summaries && filter.starts_with("source") {
                continue;
            }
            let mut statement = self
                .conn
                .prepare(&format!("SELECT id FROM {} WHERE {filter}", table.name()))?;
            let ids = statement.query_map(args, |r| r.get::<_, i64>(0))?;
            for id in ids {
                rows.push((table, id?));
            }
        }
        Ok(rows)
    }

    /// Deletes `rows` and what hangs on them, scrubs the journal of what
    /// they held, and says the forget.
    fn purge(&mut self, rows: Vec<(Table, i64)>, what: &str) -> Result<()> {
        let mut gone: std::collections::BTreeSet<(Table, i64)> = rows.into_iter().collect();
        // What hangs on them, until nothing more does.
        loop {
            let before = gone.len();
            let found = self.dependents(&gone)?;
            gone.extend(found);
            if gone.len() == before {
                break;
            }
        }
        let tx = self.conn.transaction()?;
        // Summaries that cited a forgotten node or episode: emptied, to be
        // rebuilt (compaction) without it.
        let mut stale = std::collections::BTreeSet::new();
        for (table, id) in &gone {
            let sql = match table {
                Table::Nodes => {
                    "SELECT summary FROM mentions WHERE node = ?1 AND summary IS NOT NULL"
                }
                Table::Episodes => "SELECT summary FROM summary_episodes WHERE episode = ?1",
                _ => continue,
            };
            let mut statement = tx.prepare(sql)?;
            for summary in statement.query_map([id], |r| r.get::<_, i64>(0))? {
                stale.insert(summary?);
            }
        }
        for summary in stale {
            if !gone.contains(&(Table::Summaries, summary)) {
                tx.execute(
                    "UPDATE summaries SET text = '', stale = 1 WHERE id = ?1",
                    [summary],
                )?;
            }
        }
        for table in Table::ALL {
            for (_, id) in gone.iter().filter(|(t, _)| *t == table) {
                tx.execute(&format!("DELETE FROM {} WHERE id = ?1", table.name()), [id])?;
            }
        }
        // The journal keeps that these rows were changed, never what they
        // held.
        let lines: Vec<(i64, String)> = tx
            .prepare("SELECT id, changes FROM journal")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (id, changes) in lines {
            let mut list: Vec<Json> = serde_json::from_str(&changes).unwrap_or_default();
            let mut touched = false;
            for change in &mut list {
                let row = Change::from_json(change).map(|c| c.row());
                if let Some(row) = row
                    && gone.contains(&row)
                    && let Some(object) = change.as_object_mut()
                    && object.get("before").is_some_and(|b| !b.is_null())
                {
                    object.insert("before".to_owned(), Json::Null);
                    touched = true;
                }
            }
            if touched {
                tx.execute(
                    "UPDATE journal SET changes = ?1 WHERE id = ?2",
                    params![Json::Array(list).to_string(), id],
                )?;
            }
        }
        tx.execute(
            "INSERT INTO journal (at, who, why, came_from, op, changes) VALUES (?1, 'user', ?2, NULL, 'forget', '[]')",
            params![stamp(self.clock.now()), format!("forgot {what}")],
        )?;
        tx.commit()?;
        // Rebuilt without what went: the external-content indexes only
        // drop a row's tokens on its deletion, which the triggers did.
        self.audit(crate::audit::Entry {
            kind: crate::audit::Kind::Memory,
            what: "forget".to_owned(),
            ids: Vec::new(),
            outcome: Some(format!("{} rows", gone.len())),
            ..Default::default()
        })?;
        Ok(())
    }

    /// The rows that hang on `rows`, not in it yet.
    fn dependents(
        &self,
        rows: &std::collections::BTreeSet<(Table, i64)>,
    ) -> Result<Vec<(Table, i64)>> {
        let mut found = Vec::new();
        let mut add = |table: Table, sql: &str, id: i64| -> Result<()> {
            let mut statement = self.conn.prepare(sql)?;
            for dependent in statement.query_map([id], |r| r.get::<_, i64>(0))? {
                let dependent = (table, dependent?);
                if !rows.contains(&dependent) {
                    found.push(dependent);
                }
            }
            Ok(())
        };
        for (table, id) in rows {
            let id = *id;
            match table {
                Table::Nodes => {
                    add(Table::Aliases, "SELECT id FROM aliases WHERE node = ?1", id)?;
                    add(
                        Table::Edges,
                        "SELECT id FROM edges WHERE src = ?1 OR dst = ?1",
                        id,
                    )?;
                    add(Table::Rules, "SELECT id FROM rules WHERE node = ?1", id)?;
                    add(
                        Table::Mentions,
                        "SELECT id FROM mentions WHERE node = ?1",
                        id,
                    )?;
                }
                Table::Edges => add(
                    Table::Edges,
                    "SELECT id FROM edges WHERE lineage = (SELECT lineage FROM edges WHERE id = ?1)",
                    id,
                )?,
                Table::Episodes => {
                    add(
                        Table::Mentions,
                        "SELECT id FROM mentions WHERE episode = ?1",
                        id,
                    )?;
                    add(
                        Table::SummaryEpisodes,
                        "SELECT id FROM summary_episodes WHERE episode = ?1",
                        id,
                    )?;
                }
                Table::Summaries => {
                    add(
                        Table::Mentions,
                        "SELECT id FROM mentions WHERE summary = ?1",
                        id,
                    )?;
                    add(
                        Table::SummaryEpisodes,
                        "SELECT id FROM summary_episodes WHERE summary = ?1",
                        id,
                    )?;
                }
                Table::Aliases | Table::Rules | Table::Mentions | Table::SummaryEpisodes => {}
            }
        }
        Ok(found)
    }

    /// A row as the journal and the export keep it.
    pub(crate) fn row_of(conn: &Connection, table: Table, id: i64) -> Result<Map<String, Json>> {
        let mut statement =
            conn.prepare(&format!("SELECT * FROM {} WHERE id = ?1", table.name()))?;
        let names: Vec<String> = statement.column_names().map(str::to_owned).collect();
        statement
            .query_row([id], |r| {
                let mut row = Map::new();
                for (i, name) in names.iter().enumerate() {
                    let value: Value = r.get(i)?;
                    row.insert(name.clone(), json_of(value));
                }
                Ok(row)
            })
            .optional()?
            .ok_or_else(|| Error::Missing(item_of(table, id)))
    }

    /// Every row of `table`, of `notebook` only if one is given (links
    /// whole).
    pub(crate) fn rows(
        &self,
        table: Table,
        notebook: Option<Notebook>,
    ) -> Result<Vec<Map<String, Json>>> {
        let filter = match (notebook, Table::ITEMS.contains(&table)) {
            (Some(n), true) => format!(" WHERE notebook = '{}'", n.word()),
            _ => String::new(),
        };
        let mut statement = self.conn.prepare(&format!(
            "SELECT id FROM {}{filter} ORDER BY id",
            table.name()
        ))?;
        let ids: Vec<i64> = statement
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        ids.into_iter()
            .map(|id| Self::row_of(&self.conn, table, id))
            .collect()
    }

    /// Everything she knows, of one notebook or both, as JSON: every row
    /// of every table of items, each edge's every version.
    pub fn export_json(&self, notebook: Option<Notebook>) -> Result<Json> {
        let mut out = Map::new();
        out.insert("exported".to_owned(), Json::String(stamp(self.now())));
        out.insert(
            "notebook".to_owned(),
            notebook.map_or(Json::String("all".to_owned()), |n| {
                Json::String(n.word().to_owned())
            }),
        );
        for table in Table::ALL {
            let mut rows = self.rows(table, notebook)?;
            if notebook.is_some() && !Table::ITEMS.contains(&table) {
                // Links between items of the notebook only.
                let ids = |t: Table| -> Result<std::collections::HashSet<i64>> {
                    Ok(self
                        .rows(t, notebook)?
                        .iter()
                        .filter_map(|r| r.get("id").and_then(Json::as_i64))
                        .collect())
                };
                let (nodes, episodes, summaries) = (
                    ids(Table::Nodes)?,
                    ids(Table::Episodes)?,
                    ids(Table::Summaries)?,
                );
                let has =
                    |row: &Map<String, Json>, key: &str, set: &std::collections::HashSet<i64>| {
                        row.get(key)
                            .and_then(Json::as_i64)
                            .is_none_or(|id| set.contains(&id))
                    };
                rows.retain(|row| {
                    has(row, "node", &nodes)
                        && has(row, "episode", &episodes)
                        && has(row, "summary", &summaries)
                });
            }
            out.insert(
                table.name().to_owned(),
                Json::Array(rows.into_iter().map(Json::Object).collect()),
            );
        }
        Ok(Json::Object(out))
    }
}

/// An edit under way: one transaction, the rows it changes, and its line
/// of the journal once committed.
pub(crate) struct Edit<'a> {
    pub(crate) tx: Transaction<'a>,
    changes: Vec<Change>,
    pub(crate) now: Timestamp,
}

impl Edit<'_> {
    /// Inserts a row by `sql`, which must insert one: its id.
    pub(crate) fn insert(&mut self, table: Table, sql: &str, args: &[&dyn ToSql]) -> Result<i64> {
        self.tx.execute(sql, args)?;
        let id = self.tx.last_insert_rowid();
        self.changes.push(Change::Inserted { table, id });
        Ok(id)
    }

    /// Updates row `id` by `sql`, the row kept as it was first.
    pub(crate) fn update(
        &mut self,
        table: Table,
        id: i64,
        sql: &str,
        args: &[&dyn ToSql],
    ) -> Result<()> {
        let before = Store::row_of(&self.tx, table, id)?;
        self.tx.execute(sql, args)?;
        self.changes.push(Change::Updated {
            table,
            id,
            before: Some(before),
        });
        Ok(())
    }

    pub(crate) fn delete(&mut self, table: Table, id: i64) -> Result<()> {
        let before = Store::row_of(&self.tx, table, id)?;
        self.tx
            .execute(&format!("DELETE FROM {} WHERE id = ?1", table.name()), [id])?;
        self.changes.push(Change::Deleted {
            table,
            id,
            before: Some(before),
        });
        Ok(())
    }

    /// Puts a row back as `before` held it, every column.
    fn restore_update(&mut self, table: Table, id: i64, before: &Map<String, Json>) -> Result<()> {
        let columns: Vec<&String> = before.keys().filter(|k| *k != "id").collect();
        let sets: Vec<String> = columns
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{c} = ?{}", i + 1))
            .collect();
        let values: Vec<Value> = columns.iter().map(|c| value_of(&before[*c])).collect();
        let mut args: Vec<&dyn ToSql> = values.iter().map(|v| v as &dyn ToSql).collect();
        args.push(&id);
        let sql = format!(
            "UPDATE {} SET {} WHERE id = ?{}",
            table.name(),
            sets.join(", "),
            columns.len() + 1
        );
        self.update(table, id, &sql, &args)
    }

    /// Inserts a deleted row again, its id the same.
    fn restore_insert(&mut self, table: Table, before: &Map<String, Json>) -> Result<()> {
        let columns: Vec<&String> = before.keys().collect();
        let marks: Vec<String> = (1..=columns.len()).map(|i| format!("?{i}")).collect();
        let values: Vec<Value> = columns.iter().map(|c| value_of(&before[*c])).collect();
        let args: Vec<&dyn ToSql> = values.iter().map(|v| v as &dyn ToSql).collect();
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            table.name(),
            columns
                .iter()
                .map(|c| c.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            marks.join(", ")
        );
        self.tx.execute(&sql, args.as_slice())?;
        let id = before.get("id").and_then(Json::as_i64).unwrap_or_default();
        self.changes.push(Change::Inserted { table, id });
        Ok(())
    }

    /// Writes the journal's line; the transaction stays open.
    fn commit_journal(
        &mut self,
        who: &Who,
        why: &str,
        came_from: Option<&str>,
        op: &str,
        undoes: Option<i64>,
    ) -> Result<i64> {
        let changes = Json::Array(self.changes.iter().map(Change::to_json).collect()).to_string();
        self.tx.execute(
            "INSERT INTO journal (at, who, why, came_from, op, changes, undoes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![stamp(self.now), who.word(), why, came_from, op, changes, undoes],
        )?;
        Ok(self.tx.last_insert_rowid())
    }

    /// Commits: the journal's line, then the transaction. Its id, and the
    /// rows changed (for the audit log).
    pub(crate) fn commit(
        mut self,
        who: &Who,
        why: &str,
        came_from: Option<&str>,
        op: &str,
    ) -> Result<(i64, Vec<Change>)> {
        let id = self.commit_journal(who, why, came_from, op, None)?;
        self.tx.commit()?;
        Ok((id, self.changes))
    }
}

fn item_of(table: Table, id: i64) -> Item {
    match table {
        Table::Nodes => Item::Node(id),
        Table::Aliases => Item::Alias(id),
        Table::Edges => Item::Edge(id),
        Table::Rules => Item::Rule(id),
        Table::Episodes | Table::Mentions | Table::SummaryEpisodes => Item::Episode(id),
        Table::Summaries => Item::Summary(id),
    }
}

fn json_of(value: Value) -> Json {
    match value {
        Value::Null | Value::Blob(_) => Json::Null,
        Value::Integer(n) => json!(n),
        Value::Real(x) => json!(x),
        Value::Text(t) => Json::String(t),
    }
}

fn value_of(json: &Json) -> Value {
    match json {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Integer(i64::from(*b)),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Value::Integer(i),
            None => Value::Real(n.as_f64().unwrap_or_default()),
        },
        Json::String(s) => Value::Text(s.clone()),
        other => Value::Text(other.to_string()),
    }
}
