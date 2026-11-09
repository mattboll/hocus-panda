//! Consent per source (§10.1, AC17): nothing is read before its consent
//! sheet's "Autoriser". Each decision is a record (who: the user; when;
//! the account; what of it; its notebook), kept in order and never
//! rewritten: a revocation is a later record.
//!
//! The readers take a [`Grant`], which only [`ConsentBook::grant`] makes:
//! a source without one cannot be read, by the types.

use std::{
    io::Write as _,
    path::{Path, PathBuf},
};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::item::{Notebook, SourceKey, SourceKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// "Autoriser".
    Granted,
    /// Turned off, or "Oublier cette source".
    Revoked,
}

/// One decision of the user's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Consent {
    pub source: SourceKey,
    /// The account's e-mail, as the sheet showed it.
    pub identity: String,
    /// The notebook its items go to, as the user chose on the sheet.
    pub notebook: Notebook,
    pub decision: Decision,
    pub at: Timestamp,
    /// "Seuls des résumés locaux sortent" (§14.1, 5): only the local
    /// model's summary of each item ever reaches Claude, never its text.
    #[serde(default, rename = "summaries-only")]
    pub summaries_only: bool,
}

/// The right to read a source, as consented now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    source: SourceKey,
    identity: String,
    notebook: Notebook,
    summaries_only: bool,
}

impl Grant {
    pub fn source(&self) -> &SourceKey {
        &self.source
    }

    pub fn identity(&self) -> &str {
        &self.identity
    }

    pub fn notebook(&self) -> Notebook {
        self.notebook
    }

    /// Only local summaries of its items may leave the machine.
    pub fn summaries_only(&self) -> bool {
        self.summaries_only
    }
}

/// Every decision, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsentBook {
    records: Vec<Consent>,
}

impl ConsentBook {
    /// The user's "Autoriser" on `source`'s sheet, its items going to
    /// `notebook`.
    pub fn allow(&mut self, source: SourceKey, identity: &str, notebook: Notebook, at: Timestamp) {
        self.records.push(Consent {
            source,
            identity: identity.to_owned(),
            notebook,
            decision: Decision::Granted,
            at,
            summaries_only: false,
        });
    }

    /// The per-source option "Seuls des résumés locaux sortent", on or
    /// off, for a source granted now: a new record, the grant otherwise
    /// as it was.
    pub fn summaries_only(&mut self, source: &SourceKey, on: bool, at: Timestamp) {
        if let Some(last) = self.last(source).cloned()
            && last.decision == Decision::Granted
        {
            self.records.push(Consent {
                summaries_only: on,
                at,
                ..last
            });
        }
    }

    /// Every kind of an account at once, as its sheet's one "Autoriser"
    /// does.
    pub fn allow_account(
        &mut self,
        account: &str,
        identity: &str,
        kinds: &[SourceKind],
        notebook: Notebook,
        at: Timestamp,
    ) {
        for kind in kinds {
            self.allow(SourceKey::new(account, *kind), identity, notebook, at);
        }
    }

    /// The user turns `source` off. What came from it is to be purged
    /// ([`crate::cycle::Event::Purge`]).
    pub fn revoke(&mut self, source: &SourceKey, at: Timestamp) {
        if let Some(last) = self.last(source).cloned() {
            self.records.push(Consent {
                decision: Decision::Revoked,
                at,
                ..last
            });
        }
    }

    fn last(&self, source: &SourceKey) -> Option<&Consent> {
        self.records.iter().rev().find(|c| &c.source == source)
    }

    /// The right to read `source`, when its last decision is "Autoriser".
    pub fn grant(&self, source: &SourceKey) -> Option<Grant> {
        let last = self.last(source)?;
        (last.decision == Decision::Granted).then(|| Grant {
            source: last.source.clone(),
            identity: last.identity.clone(),
            notebook: last.notebook,
            summaries_only: last.summaries_only,
        })
    }

    /// The sources whose last decision is a revocation.
    pub fn revoked(&self) -> Vec<SourceKey> {
        let mut keys: Vec<SourceKey> = self.records.iter().map(|c| c.source.clone()).collect();
        keys.sort();
        keys.dedup();
        keys.into_iter()
            .filter(|k| {
                self.last(k)
                    .is_some_and(|c| c.decision == Decision::Revoked)
            })
            .collect()
    }

    /// Every decision, oldest first ("Ce qu'elle a lu", the journal).
    pub fn records(&self) -> &[Consent] {
        &self.records
    }

    /// The book at `path`; an empty one when there is none. One that does
    /// not read is not taken as consent to anything.
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|err| {
                tracing::warn!(%err, path = %path.display(), "the consent records do not read: none taken");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        write_atomically(path, &serde_json::to_vec_pretty(self)?)
    }
}

/// `bytes` written to `path` through a file beside it, renamed over it;
/// readable by the user alone, its folder made if missing.
pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
    if let Some(dir) = path.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    let mut temporary = PathBuf::from(path);
    temporary.as_mut_os_string().push(".new");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_is_granted_until_revoked_and_the_records_stay() {
        let mut book = ConsentBook::default();
        let mail = SourceKey::new("account_1", SourceKind::Mail);
        assert_eq!(book.grant(&mail), None);
        book.allow(
            mail.clone(),
            "alex@example.org",
            Notebook::Pro,
            Timestamp::UNIX_EPOCH,
        );
        assert_eq!(book.grant(&mail).map(|g| g.notebook()), Some(Notebook::Pro));
        assert_eq!(book.grant(&mail).map(|g| g.summaries_only()), Some(false));
        book.summaries_only(&mail, true, Timestamp::UNIX_EPOCH);
        assert_eq!(book.grant(&mail).map(|g| g.summaries_only()), Some(true));
        book.revoke(&mail, Timestamp::UNIX_EPOCH);
        assert_eq!(book.grant(&mail), None);
        assert_eq!(book.revoked(), [mail]);
        assert_eq!(book.records().len(), 3);
    }

    #[test]
    fn the_book_is_kept_on_disk() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("secretary/consents.json");
        let mut book = ConsentBook::default();
        book.allow_account(
            "account_2",
            "alex.perso@gmail.com",
            &SourceKind::ALL,
            Notebook::Perso,
            Timestamp::UNIX_EPOCH,
        );
        book.save(&path)?;
        assert_eq!(ConsentBook::load(&path), book);
        Ok(())
    }
}
