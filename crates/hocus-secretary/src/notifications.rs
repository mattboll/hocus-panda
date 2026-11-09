//! Applications' notifications as one [`Source`] (lot 7, §10.1): Slack
//! read through its notifications, its default mode, and any other
//! application the user consented to, each in the notebook chosen then.
//!
//! The shell's server receives every notification; it forwards her only
//! those of the applications her consent book grants
//! ([`ToSecretary::Notification`](hocus_secretary_link::ToSecretary)), and
//! she checks the book again as she reads them: one that is not granted
//! now is dropped, unread. Her loop puts what arrives in [`Arrivals`] and
//! makes this source due at once; its worker reads the book, makes each
//! one an item through the first pass (its title and text), and hands it
//! on like a mail: its text held for the triage, its digest (the
//! application and the title) kept.
//!
//! A revoked application is forgotten once (what came from it), at its
//! next sync: every five minutes, as her Google accounts'.
//!
//! **Never an action.** Nothing here, nor in her protocol, can invoke,
//! close or mark a notification: its actions are not even forwarded.
//! Slack marks a channel read when one of its notifications is acted on;
//! in this mode no request of hers ever reaches Slack.

use std::{
    collections::{BTreeSet, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use hocus_secretary_link::{Notebook, Notification};
use hocus_sources::{
    ConsentBook, Content, Redactor, SourceKey, SourceKind, item::Notification as Said,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::sources::{Item, Source, Synced};

/// The source's name: its cursor's key.
pub const NAME: &str = "notifications";

/// At most this many wait for the next sync; the oldest go first.
const MOST: usize = 500;

/// What the shell forwarded and her source has not read yet: her loop
/// puts them in, the source's worker takes them out.
#[derive(Debug, Clone, Default)]
pub struct Arrivals(Arc<Mutex<VecDeque<Notification>>>);

impl Arrivals {
    pub fn push(&self, notification: Notification) {
        let mut waiting = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if waiting.len() >= MOST {
            waiting.pop_front();
        }
        waiting.push_back(notification);
    }

    fn take(&self) -> Vec<Notification> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
            .collect()
    }
}

/// Where the source stopped: the applications whose revocation was
/// already purged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Cursor {
    purged: BTreeSet<SourceKey>,
}

/// The applications' notifications, as the consent book at `book` allows.
#[derive(Debug)]
pub struct Notifications {
    book: PathBuf,
    arrivals: Arrivals,
    redactor: Redactor,
    every: Duration,
}

impl Notifications {
    /// The source, and what her loop puts the forwarded notifications in.
    pub fn new(book: PathBuf) -> (Self, Arrivals) {
        let arrivals = Arrivals::default();
        (
            Self {
                book,
                arrivals: arrivals.clone(),
                redactor: Redactor::new(),
                every: Duration::from_secs(300),
            },
            arrivals,
        )
    }
}

impl Source for Notifications {
    fn name(&self) -> &str {
        NAME
    }

    fn notebook(&self) -> Notebook {
        // Each item carries its application's, as consented.
        Notebook::Pro
    }

    fn every(&self) -> Duration {
        self.every
    }

    fn sync(&mut self, cursor: Option<&str>, now: Timestamp) -> Result<Synced, String> {
        let mut cursor: Cursor = cursor
            .filter(|c| !c.is_empty())
            .and_then(|c| serde_json::from_str(c).ok())
            .unwrap_or_default();
        let book = ConsentBook::load(&self.book);
        let mut synced = Synced::default();
        for source in book
            .revoked()
            .into_iter()
            .filter(|s| s.kind == SourceKind::Notifications)
        {
            if cursor.purged.insert(source.clone()) {
                synced.forget.push(source.to_string());
            }
        }
        for grant in book.notifying() {
            cursor.purged.remove(grant.source());
        }
        let mut dropped = 0;
        for notification in self.arrivals.take() {
            let Some(grant) = book.notifications(&notification.app, notification.entry.as_deref())
            else {
                // Not (or no longer) consented: dropped unread.
                dropped += 1;
                continue;
            };
            let at = Timestamp::from_millisecond(notification.at_ms).unwrap_or(now);
            let id = format!("{}-{}", notification.at_ms, notification.id);
            let item = hocus_sources::notification::item(
                &grant,
                id.clone(),
                Said {
                    app: notification.app,
                    title: notification.title,
                    body: notification.body,
                },
                at,
            );
            let redacted = self.redactor.redact(item);
            let digest = match &redacted.item.content {
                Content::Notification(n) => format!("{} : {}", n.app, n.title),
                _ => String::new(),
            };
            synced.items.push(Item {
                id,
                at,
                kind: "notification".to_owned(),
                digest,
                notebook: Some(grant.notebook()),
                from: Some(grant.source().to_string()),
                project: None,
                sender: None,
                body: None,
                calendar: None,
                until: None,
                open: None,
                text: Some(Box::new(redacted)),
                summaries_only: grant.summaries_only(),
            });
        }
        if dropped > 0 {
            tracing::info!(
                dropped,
                "notifications of applications not consented to: dropped unread"
            );
        }
        synced.cursor = serde_json::to_string(&cursor)
            .map_err(|e| format!("the cursor does not write: {e}"))?;
        Ok(synced)
    }
}

#[cfg(test)]
mod tests {
    use hocus_sources::Check;

    use super::*;

    fn slack(id: u32, body: &str) -> Notification {
        Notification {
            id,
            app: "Slack".to_owned(),
            entry: Some("slack".to_owned()),
            title: "Karim (#prod-alertes)".to_owned(),
            body: body.to_owned(),
            at_ms: 1_794_215_520_000,
        }
    }

    #[test]
    fn only_a_consented_application_s_notifications_are_read_and_a_revoked_one_is_forgotten() {
        let dir = tempfile::tempdir().expect("a folder");
        let path = dir.path().join("consents.json");
        let now: Timestamp = "2026-11-09T09:13:00Z".parse().expect("a time");
        let (mut source, arrivals) = Notifications::new(path.clone());

        // Nothing consented: what arrives is dropped.
        arrivals.push(slack(1, "le serveur est tombé"));
        let synced = source.sync(None, now).expect("a sync");
        assert!(synced.items.is_empty());

        let mut book = ConsentBook::default();
        book.allow(
            SourceKey::notifications("Slack", Some("slack")),
            "Slack",
            Notebook::Pro,
            now,
        );
        book.allow(
            SourceKey::notifications("Signal", None),
            "Signal",
            Notebook::Perso,
            now,
        );
        book.save(&path).expect("saved");
        arrivals.push(slack(2, "mdp du wifi : chienchat"));
        arrivals.push(Notification {
            id: 3,
            app: "Signal".to_owned(),
            entry: None,
            title: "Claire".to_owned(),
            body: "Dimanche chez maman ?".to_owned(),
            at_ms: 1_794_215_580_000,
        });
        arrivals.push(Notification {
            id: 4,
            app: "Thunderbird".to_owned(),
            entry: None,
            title: "Nouveau courrier".to_owned(),
            body: "Facture".to_owned(),
            at_ms: 1_794_215_590_000,
        });
        let synced = source.sync(Some(&synced.cursor), now).expect("a sync");
        let ids: Vec<&str> = synced.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["1794215520000-2", "1794215580000-3"]);
        let first = &synced.items[0];
        assert_eq!(first.kind, "notification");
        assert_eq!(first.digest, "Slack : Karim (#prod-alertes)");
        assert_eq!(first.notebook, Some(Notebook::Pro));
        assert_eq!(first.from.as_deref(), Some("slack:notifications"));
        let text = first.text.as_ref().expect("its text, for the triage");
        assert_eq!(text.check, Check::FirstPass);
        let Content::Notification(n) = &text.item.content else {
            panic!("a notification");
        };
        assert_eq!(
            n.body,
            "mdp du wifi : [secret masqué — voir la notification de Slack, 9 nov.]"
        );
        assert_eq!(synced.items[1].notebook, Some(Notebook::Perso));
        assert!(synced.forget.is_empty());

        // Slack revoked: forgotten once, and what arrives from it dropped.
        book.revoke(&SourceKey::notifications("Slack", Some("slack")), now);
        book.save(&path).expect("saved");
        arrivals.push(slack(5, "encore là ?"));
        let revoked = source.sync(Some(&synced.cursor), now).expect("a sync");
        assert_eq!(revoked.forget, ["slack:notifications"]);
        assert!(revoked.items.is_empty());
        let again = source.sync(Some(&revoked.cursor), now).expect("a sync");
        assert!(again.forget.is_empty(), "forgotten once");
    }
}
