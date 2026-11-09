//! Her sources (§10): each one read on a worker thread of its own, from
//! where it stopped (its sync token, kept in her memory as its cursor),
//! on its own schedule, locked screen included, and at once after a
//! suspend or a start (the catch-up, §3.10).
//!
//! A source says what it read as [`Item`]s: an id at the source (the
//! pointer that opens it), a time, a kind, a one-line digest, and the
//! notebook its account is in (the personal Google account's items are
//! perso). Its text never comes here: redaction and triage (lots 3 and 4)
//! sit between a source and her memory. Lot 3's Google sources implement
//! [`Source`]; this lot has the harness's fixture sources.

use std::{path::Path, time::Duration};

use hocus_secretary_link::Notebook;
use jiff::Timestamp;
use serde::Deserialize;

/// One thing a source has.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    /// Its id at the source.
    pub id: String,
    pub at: Timestamp,
    /// `mail`, `event`, `message`, `task`…
    pub kind: String,
    /// What it is about, in a line.
    pub digest: String,
    /// Its own notebook, when not its account's.
    #[serde(default)]
    pub notebook: Option<Notebook>,
    /// The project it belongs to, when its source says (a board's card):
    /// her memory links it to that project's node.
    #[serde(default)]
    pub project: Option<String>,
}

/// What a sync read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Synced {
    pub items: Vec<Item>,
    /// Where the next sync starts.
    pub cursor: String,
}

/// A source: called on its worker thread, never on her loop; it may block
/// (the network), and fails without panicking.
pub trait Source: Send + std::fmt::Debug {
    /// Its name, stable across runs: its cursor's key and its items'
    /// pointers' prefix (`google-mail-pro`).
    fn name(&self) -> &str;
    /// Its account's notebook.
    fn notebook(&self) -> Notebook;
    /// How long between two syncs.
    fn every(&self) -> Duration;
    /// What arrived since `cursor` (from the start without one), up to
    /// `now` on her clock.
    fn sync(&mut self, cursor: Option<&str>, now: Timestamp) -> Result<Synced, String>;
}

/// A fixture source: a file of items, each arriving at its time on her
/// clock (tests, screenshots).
///
/// ```toml
/// name = "fixture-mail-perso"
/// notebook = "perso"
/// every = 300                  # seconds
/// [[items]]
/// id = "m1"
/// at = "2026-11-09T07:00:00Z"
/// kind = "mail"
/// digest = "Claire : la céramique"
/// project = "Chapitô"          # optional: a board's card
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    pub name: String,
    pub notebook: Notebook,
    #[serde(default = "five_minutes")]
    pub every: u64,
    #[serde(default)]
    pub items: Vec<Item>,
}

const fn five_minutes() -> u64 {
    300
}

impl Fixture {
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// An item's place in the sync's order: its time, then its id.
    fn key(item: &Item) -> String {
        format!("{}#{}", crate::store::stamp(item.at), item.id)
    }
}

impl Source for Fixture {
    fn name(&self) -> &str {
        &self.name
    }

    fn notebook(&self) -> Notebook {
        self.notebook
    }

    fn every(&self) -> Duration {
        Duration::from_secs(self.every.max(1))
    }

    fn sync(&mut self, cursor: Option<&str>, now: Timestamp) -> Result<Synced, String> {
        let mut items: Vec<Item> = self
            .items
            .iter()
            .filter(|i| i.at <= now && cursor.is_none_or(|c| Self::key(i).as_str() > c))
            .cloned()
            .collect();
        items.sort_by_key(Self::key);
        let cursor = items
            .last()
            .map(Self::key)
            .or_else(|| cursor.map(ToOwned::to_owned))
            .unwrap_or_default();
        Ok(Synced { items, cursor })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixture_gives_what_arrived_since_its_cursor() {
        let mut fixture: Fixture = toml::from_str(
            r#"
            name = "fixture-mail"
            notebook = "pro"
            [[items]]
            id = "m2"
            at = "2026-11-09T09:00:00Z"
            kind = "mail"
            digest = "Léa : la recette"
            [[items]]
            id = "m1"
            at = "2026-11-09T07:00:00Z"
            kind = "mail"
            digest = "Paul : le devis"
            "#,
        )
        .expect("a fixture");
        let at = |t: &str| t.parse::<Timestamp>().expect("a time");
        let first = fixture
            .sync(None, at("2026-11-09T08:00:00Z"))
            .expect("a sync");
        assert_eq!(
            first
                .items
                .iter()
                .map(|i| i.id.as_str())
                .collect::<Vec<_>>(),
            ["m1"]
        );
        let again = fixture
            .sync(Some(&first.cursor), at("2026-11-09T08:30:00Z"))
            .expect("a sync");
        assert!(again.items.is_empty());
        assert_eq!(again.cursor, first.cursor);
        let later = fixture
            .sync(Some(&again.cursor), at("2026-11-09T10:00:00Z"))
            .expect("a sync");
        assert_eq!(
            later
                .items
                .iter()
                .map(|i| i.id.as_str())
                .collect::<Vec<_>>(),
            ["m2"]
        );
    }
}
