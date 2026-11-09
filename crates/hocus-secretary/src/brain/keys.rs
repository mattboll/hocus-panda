//! Her answers' key given in her drawer or the Settings (#752): checked
//! with its provider by the smallest request there is, then kept in the
//! keyring of her bus, on a thread of its own; the user told plainly what
//! became of it ("clé refusée par Anthropic", "pas de réseau"). The key
//! itself is never logged, audited, kept in her thread or said back: its
//! clipboard digest only goes to the shells, so that a copy of it leaves
//! the clipboard's history.

use hocus_secretary_link::{FromSecretary, KeyOutcome, Secret, clip_digest};

use super::{Brain, Event};
use crate::audit;

impl Brain {
    /// `key` for `provider` (none: erase it), from connection `conn`'s
    /// request `id`.
    pub(super) fn key_given(&mut self, conn: u64, id: u64, provider: String, key: Option<Secret>) {
        let Some(model) = self.model.clone() else {
            self.error(conn, Some(id), "she has no model to keep a key for");
            return;
        };
        tracing::info!(conn, provider, erase = key.is_none(), "a key given to her");
        let sender = self.sender.clone();
        let spawned = std::thread::Builder::new()
            .name("key given".to_owned())
            .spawn(move || {
                let outcome = model.keep_key(&provider, key.as_ref());
                let digest = key
                    .as_ref()
                    .filter(|_| outcome == KeyOutcome::Kept)
                    .map(|k| clip_digest(k.expose()));
                drop(key);
                let _ = sender.send(Event::KeyDone {
                    conn,
                    id,
                    provider,
                    outcome,
                    digest,
                });
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "no thread to keep a key");
            self.send(
                conn,
                &FromSecretary::Keyed {
                    id,
                    provider: String::new(),
                    outcome: KeyOutcome::NoKeyring,
                },
            );
        }
    }

    /// What became of a key given.
    pub(super) fn key_done(
        &mut self,
        conn: u64,
        id: u64,
        provider: String,
        outcome: KeyOutcome,
        digest: Option<u64>,
    ) {
        tracing::info!(provider, ?outcome, "a key given to her, done");
        self.audited(audit::Entry {
            kind: audit::Kind::Answer,
            what: "key".to_owned(),
            ids: vec![provider.clone()],
            outcome: Some(format!("{outcome:?}").to_lowercase()),
            ..Default::default()
        });
        if let Some(digest) = digest {
            self.tell_talkers(&FromSecretary::ClipSecret { digest }, None);
        }
        self.send(
            conn,
            &FromSecretary::Keyed {
                id,
                provider: provider.clone(),
                outcome,
            },
        );
        match outcome {
            KeyOutcome::Kept if self.key_missing.as_deref() == Some(provider.as_str()) => {
                self.key_missing = None;
            }
            KeyOutcome::Erased => self.ask_key(),
            _ => {}
        }
        self.tell_look();
    }
}
