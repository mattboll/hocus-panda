//! What she keeps across a restart (#750), beside the graph in her
//! encrypted memory:
//!
//! - **Her thread**: the drawer's last [`THREAD_MOST`] items, each with
//!   its notebook (a perso one is hidden in public moments, and forgotten
//!   with "Oublier tout le perso"), and a proposal's effect for its card:
//!   "Envoyer" still does what it said after a restart.
//! - **The items waiting for the triage**: their text after the first
//!   pass, until the local model checked them (the second pass, §14.1).
//!   Before, nothing of that text leaves (fail closed); once checked, the
//!   row goes, and only what the triage made of it stays (the episode's
//!   digest). A source revoked, a notebook forgotten: theirs go too.
//!
//! The tables are made when missing, with the store's, as the day's are.

use hocus_secretary_link::{Item, Notebook};
use hocus_sources::Redacted;
use rusqlite::{Connection, params};

use crate::{context::Held, effects::Proposal};

/// The thread's items kept.
pub const THREAD_MOST: usize = 200;
/// The items waiting for the triage kept, the newest.
pub const PENDING_MOST: usize = 1_000;

pub const SQL: &str = r#"
CREATE TABLE IF NOT EXISTS thread (
    id INTEGER PRIMARY KEY,
    notebook TEXT CHECK (notebook IN ('pro', 'perso')),
    item TEXT NOT NULL,
    proposal TEXT
);
CREATE TABLE IF NOT EXISTS pending (
    pointer TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    notebook TEXT NOT NULL CHECK (notebook IN ('pro', 'perso')),
    episode INTEGER NOT NULL,
    summaries_only INTEGER NOT NULL,
    redacted TEXT NOT NULL
);
"#;

type Result<T> = rusqlite::Result<T>;

fn json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

/// A thread item kept (a new one, or one that changed), with its card's
/// proposal; the oldest past [`THREAD_MOST`] go.
pub fn keep_item(conn: &Connection, item: &Item, proposal: Option<&Proposal>) -> Result<()> {
    let id = i64::try_from(item.id).unwrap_or(i64::MAX);
    conn.execute(
        "INSERT INTO thread (id, notebook, item, proposal) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (id) DO UPDATE SET notebook = ?2, item = ?3,
             proposal = coalesce(?4, proposal)",
        params![
            id,
            item.notebook.map(Notebook::word),
            json(item)?,
            proposal.map(json).transpose()?,
        ],
    )?;
    conn.execute(
        "DELETE FROM thread WHERE id NOT IN (SELECT id FROM thread ORDER BY id DESC LIMIT ?1)",
        [THREAD_MOST as i64],
    )?;
    Ok(())
}

/// The thread kept, oldest first, with each card's proposal.
pub fn thread(conn: &Connection) -> Result<Vec<(Item, Option<Proposal>)>> {
    let mut statement = conn.prepare("SELECT item, proposal FROM thread ORDER BY id")?;
    let rows = statement.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (item, proposal) = row?;
        match serde_json::from_str::<Item>(&item) {
            Ok(item) => out.push((item, proposal.and_then(|p| serde_json::from_str(&p).ok()))),
            Err(err) => tracing::info!(%err, "a thread item that no longer reads: left out"),
        }
    }
    Ok(out)
}

/// An item's text waiting for the triage, kept until it is checked.
pub fn keep_pending(conn: &Connection, pointer: &str, source: &str, held: &Held) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO pending
             (pointer, source, notebook, episode, summaries_only, redacted)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            pointer,
            source,
            held.redacted.item.notebook.word(),
            held.episode,
            held.summaries_only,
            json(&held.redacted)?,
        ],
    )?;
    conn.execute(
        "DELETE FROM pending WHERE pointer NOT IN
             (SELECT pointer FROM pending ORDER BY rowid DESC LIMIT ?1)",
        [PENDING_MOST as i64],
    )?;
    Ok(())
}

/// The triage checked it: its text is no longer kept.
pub fn checked(conn: &Connection, pointer: &str) -> Result<()> {
    conn.execute("DELETE FROM pending WHERE pointer = ?1", [pointer])?;
    Ok(())
}

/// The items waiting for the triage, in the order they came, whose
/// episode is still in her memory.
pub fn pending(conn: &Connection) -> Result<Vec<(String, Held)>> {
    let mut statement = conn.prepare(
        "SELECT p.pointer, p.episode, p.summaries_only, p.redacted FROM pending p
         JOIN episodes e ON e.id = p.episode ORDER BY p.rowid",
    )?;
    let rows = statement.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, bool>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (pointer, episode, summaries_only, redacted) = row?;
        match serde_json::from_str::<Redacted>(&redacted) {
            Ok(redacted) => out.push((
                pointer,
                Held {
                    redacted,
                    episode,
                    summaries_only,
                    summary: None,
                    suspicious: false,
                },
            )),
            Err(err) => tracing::info!(%err, "a pending item that no longer reads: left out"),
        }
    }
    Ok(out)
}

/// The items of `store` waiting for the triage (tests, her status).
pub fn waiting(store: &crate::store::Store) -> Result<Vec<(String, Held)>> {
    pending(&store.conn)
}

/// A source revoked: its items waiting for the triage go.
pub fn forget_source(conn: &Connection, source: &str) -> Result<()> {
    conn.execute(
        &format!("DELETE FROM pending WHERE source{}", crate::store::OF_SOURCE),
        [source],
    )?;
    Ok(())
}

/// A source's items waiting move to `notebook` (#752).
pub fn refile_source(conn: &Connection, source: &str, notebook: Notebook) -> Result<()> {
    conn.execute(
        &format!(
            "UPDATE pending SET notebook = ?2 WHERE source{}",
            crate::store::OF_SOURCE
        ),
        rusqlite::params![source, notebook.word()],
    )?;
    Ok(())
}

/// A notebook forgotten: its thread items and its items waiting go.
pub fn forget_notebook(conn: &Connection, notebook: Notebook) -> Result<()> {
    conn.execute("DELETE FROM thread WHERE notebook = ?1", [notebook.word()])?;
    conn.execute("DELETE FROM pending WHERE notebook = ?1", [notebook.word()])?;
    Ok(())
}
