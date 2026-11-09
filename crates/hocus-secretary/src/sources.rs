//! Her sources (§10): each one read on a worker thread of its own, from
//! where it stopped (its sync token, kept in her memory as its cursor),
//! on its own schedule, locked screen included, and at once after a
//! suspend or a start (the catch-up, §3.10).
//!
//! A source says what it read as [`Item`]s: an id at the source (the
//! pointer that opens it), a time, a kind, a one-line digest, and the
//! notebook its account is in (the personal Google account's items are
//! perso). Its text, when it has one, comes only through redaction's
//! first pass ([`Item::text`], `hocus_sources::Redacted`), held back from
//! any model but the local triage's until the second pass has checked it
//! (lot 4's fail closed); it is never written to her memory, only its
//! digest. The Google accounts are [`crate::google::Google`]; the harness
//! has fixture sources.

use std::{path::Path, time::Duration};

use hocus_secretary_link::Notebook;
use hocus_sources::{
    Content, Pointer, Redacted, Redactor, SourceKey, SourceKind,
    item::{self, Mail},
};
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
    /// The part of the source it came from, when a source gathers several
    /// (`account_1:mail`): its episodes' source, forgotten with it.
    #[serde(default)]
    pub from: Option<String>,
    /// A fixture mail's sender and text (tests): redacted as a real one.
    #[serde(default)]
    pub sender: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    /// Its text through the first pass, for the triage; never kept.
    #[serde(skip)]
    pub text: Option<Box<Redacted>>,
    /// Only the local model's summary of it may leave the machine
    /// ("Seuls des résumés locaux sortent", §14.1, 5).
    #[serde(skip)]
    pub summaries_only: bool,
}

/// What a sync read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Synced {
    pub items: Vec<Item>,
    /// Where the next sync starts.
    pub cursor: String,
    /// Parts of the source revoked since: what came from them is
    /// forgotten (their names, as [`Item::from`] gives them).
    pub forget: Vec<String>,
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
    /// Only local summaries of its items leave the machine.
    #[serde(default, rename = "summaries-only")]
    pub summaries_only: bool,
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
        let redactor = Redactor::new();
        for item in &mut items {
            item.summaries_only = self.summaries_only;
            if let Some(body) = &item.body {
                item.text = Some(Box::new(self.redact(&redactor, item, body)));
                // Its subject went through the first pass too.
                if let Some(text) = &item.text
                    && let Content::Mail(mail) = &text.item.content
                {
                    item.digest = mail.subject.clone();
                }
            }
        }
        let cursor = items
            .last()
            .map(Self::key)
            .or_else(|| cursor.map(ToOwned::to_owned))
            .unwrap_or_default();
        Ok(Synced {
            items,
            cursor,
            forget: Vec::new(),
        })
    }
}

impl Fixture {
    /// A fixture mail as a source's would be: its digest the subject,
    /// its body the text, both through the first pass.
    fn redact(&self, redactor: &Redactor, item: &Item, body: &str) -> Redacted {
        let source = SourceKey::new(&self.name, SourceKind::Mail);
        let mail = Mail {
            mailbox: "INBOX".to_owned(),
            from: item.sender.clone().unwrap_or_default(),
            subject: item.digest.clone(),
            body: Some(body.to_owned()),
            ..Mail::default()
        };
        redactor.redact(item::Item {
            pointer: Pointer {
                source,
                id: item.id.clone(),
                label: format!("le mail {}", item.id),
                open: None,
            },
            notebook: item.notebook.unwrap_or(self.notebook),
            at: item.at,
            content: Content::Mail(mail),
        })
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
