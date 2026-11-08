//! Her audit log (§14): local, append-only, readable as "Ce qu'elle a lu /
//! fait". Every source read (what, by id), every model call (its role,
//! provider, model, tokens, the items it included, how long it took),
//! every proposal and its answer, every delegation, every memory edit.
//! Never content and never secrets: an entry holds fixed words, ids and
//! numbers, nothing else (no field takes a text from a source or a
//! model). Kept [`KEPT_DAYS`] days, in her encrypted memory; a trigger
//! refuses any change to a line written.

use jiff::{SignedDuration, Timestamp};
use rusqlite::params;

use crate::{
    graph::changed_ids,
    store::{Change, Item, Result, Store, stamp, unstamp},
};

/// How long the log keeps a line.
pub const KEPT_DAYS: i64 = 30;

/// What a line says happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    /// A source read: its name in `what`, the items' ids.
    #[default]
    Read,
    /// A model call: its role in `what`.
    Call,
    /// A proposal made: the item's id.
    Proposal,
    /// The user's answer to a proposal: `outcome`.
    Answer,
    /// A job delegated (lot 12).
    Delegation,
    /// A change to her memory: the journal's line and the rows.
    Memory,
}

impl Kind {
    pub const fn word(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Call => "call",
            Self::Proposal => "proposal",
            Self::Answer => "answer",
            Self::Delegation => "delegation",
            Self::Memory => "memory",
        }
    }

    fn of(word: &str) -> Option<Self> {
        [
            Self::Read,
            Self::Call,
            Self::Proposal,
            Self::Answer,
            Self::Delegation,
            Self::Memory,
        ]
        .into_iter()
        .find(|k| k.word() == word)
    }
}

/// One line of the log.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Entry {
    pub kind: Kind,
    /// A fixed word: the source's name, the call's role, the edit's op.
    pub what: String,
    /// The items concerned, by id: `edge:12`, `fixture-mail:m3`.
    pub ids: Vec<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub tokens_in: Option<u64>,
    pub tokens_out: Option<u64>,
    pub duration_ms: Option<u64>,
    /// A fixed word: `accepted`, `rejected`, `failed`, `3 rows`.
    pub outcome: Option<String>,
}

/// A line as read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub id: i64,
    pub at: Timestamp,
    pub entry: Entry,
}

impl Store {
    pub fn audit(&self, entry: Entry) -> Result<()> {
        let to_i64 = |n: Option<u64>| n.map(|n| i64::try_from(n).unwrap_or(i64::MAX));
        self.conn.execute(
            "INSERT INTO audit (at, kind, what, ids, provider, model, tokens_in, tokens_out, duration_ms, outcome)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                stamp(self.now()),
                entry.kind.word(),
                entry.what,
                serde_json::to_string(&entry.ids).unwrap_or_default(),
                entry.provider,
                entry.model,
                to_i64(entry.tokens_in),
                to_i64(entry.tokens_out),
                to_i64(entry.duration_ms),
                entry.outcome,
            ],
        )?;
        Ok(())
    }

    pub(crate) fn audit_memory(&self, edit: i64, changes: &[Change]) -> Result<()> {
        let mut ids = vec![format!("journal:{edit}")];
        ids.extend(changed_ids(changes));
        self.audit(Entry {
            kind: Kind::Memory,
            what: "edit".to_owned(),
            ids,
            ..Default::default()
        })
    }

    pub(crate) fn audit_proposal(&self, item: Item) -> Result<()> {
        self.audit(Entry {
            kind: Kind::Proposal,
            what: "memory".to_owned(),
            ids: vec![item.word()],
            ..Default::default()
        })
    }

    pub(crate) fn audit_answer(&self, item: Item, outcome: &str) -> Result<()> {
        self.audit(Entry {
            kind: Kind::Answer,
            what: "memory".to_owned(),
            ids: vec![item.word()],
            outcome: Some(outcome.to_owned()),
            ..Default::default()
        })
    }

    /// The log, oldest first.
    pub fn audit_lines(&self) -> Result<Vec<Line>> {
        let mut statement = self.conn.prepare(
            "SELECT id, at, kind, what, ids, provider, model, tokens_in, tokens_out, duration_ms, outcome
             FROM audit ORDER BY id",
        )?;
        let to_u64 = |n: Option<i64>| n.and_then(|n| u64::try_from(n).ok());
        let rows = statement.query_map([], |r| {
            Ok(Line {
                id: r.get(0)?,
                at: unstamp(&r.get::<_, String>(1)?).unwrap_or_default(),
                entry: Entry {
                    kind: Kind::of(&r.get::<_, String>(2)?).unwrap_or_default(),
                    what: r.get(3)?,
                    ids: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or_default(),
                    provider: r.get(5)?,
                    model: r.get(6)?,
                    tokens_in: to_u64(r.get(7)?),
                    tokens_out: to_u64(r.get(8)?),
                    duration_ms: to_u64(r.get(9)?),
                    outcome: r.get(10)?,
                },
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Drops the lines older than [`KEPT_DAYS`].
    pub fn prune_audit(&self) -> Result<usize> {
        let oldest = self.now() - SignedDuration::from_hours(24 * KEPT_DAYS);
        Ok(self
            .conn
            .execute("DELETE FROM audit WHERE at < ?1", [stamp(oldest)])?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_written_is_never_changed_and_old_lines_go() -> anyhow::Result<()> {
        let clock = crate::clock::Clock::virtual_at("2026-11-09T08:00:00Z".parse()?);
        let store = Store::in_memory(clock.clone())?;
        store.audit(Entry {
            kind: Kind::Read,
            what: "fixture-mail".to_owned(),
            ids: vec!["fixture-mail:m1".to_owned()],
            ..Default::default()
        })?;
        assert!(
            store
                .conn
                .execute("UPDATE audit SET what = 'other'", [])
                .is_err(),
            "append-only"
        );
        clock.advance(std::time::Duration::from_secs(31 * 24 * 3600));
        store.audit(Entry::default())?;
        assert_eq!(store.prune_audit()?, 1);
        assert_eq!(store.audit_lines()?.len(), 1);
        Ok(())
    }
}
