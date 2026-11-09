//! Consent per source (§10.1, AC17): nothing is read before its consent
//! sheet's "Autoriser". Each decision is a record (who: the user; when;
//! the account, or the application; what of it; its notebook), kept in
//! order and never rewritten: a revocation is a later record.
//!
//! The readers take a [`Grant`], which only [`ConsentBook::grant`] makes:
//! a source without one cannot be read, by the types.
//!
//! The book is a file in her folder ([`CONSENTS`]): the Settings write
//! it, she reads it, and so does the shell, which forwards her only the
//! notifications of the applications consented to (lot 7). Here, in her
//! protocol's crate, so that the shell and the Settings need nothing
//! more of hers.

use std::{
    fmt,
    io::Write as _,
    path::{Path, PathBuf},
};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::Notebook;

/// The consent book's file, in her folder.
pub const CONSENTS: &str = "consents.json";

/// What of an account is read; each is consented on its own. And an
/// application's notifications, consented per application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// Its calendars, hidden ones included.
    Calendar,
    /// Its contacts' birthdays and anniversaries, nothing else of them.
    Birthdays,
    /// Its mail (Gmail): inbox and sent, 30 days back, read-only.
    Mail,
    /// An application's notifications, as the shell's server receives
    /// them (§10.1: Slack's by default): the account is the application
    /// ([`app_key`]). Never an Online Accounts' account's.
    Notifications,
}

impl SourceKind {
    /// An Online Accounts' account's kinds.
    pub const ALL: [Self; 3] = [Self::Calendar, Self::Birthdays, Self::Mail];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Calendar => "calendar",
            Self::Birthdays => "birthdays",
            Self::Mail => "mail",
            Self::Notifications => "notifications",
        }
    }
}

/// One source: an account (Online Accounts' stable id for it, e.g.
/// `account_1700000000_0`) and what of it is read; or an application
/// ([`app_key`]) and its notifications.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceKey {
    pub account: String,
    pub kind: SourceKind,
}

impl SourceKey {
    pub fn new(account: impl Into<String>, kind: SourceKind) -> Self {
        Self {
            account: account.into(),
            kind,
        }
    }

    /// An application's notifications ([`app_key`]).
    pub fn notifications(app: &str, entry: Option<&str>) -> Self {
        Self::new(app_key(app, entry), SourceKind::Notifications)
    }
}

impl fmt::Display for SourceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.account, self.kind.as_str())
    }
}

/// An application's key in the book: its desktop entry when its
/// notifications name one (`slack`, `com.slack.Slack`, without
/// `.desktop`), else its name; lowercase.
pub fn app_key(app: &str, entry: Option<&str>) -> String {
    let entry = entry
        .map(|e| e.trim().trim_end_matches(".desktop"))
        .filter(|e| !e.is_empty());
    entry.unwrap_or_else(|| app.trim()).to_lowercase()
}

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
    /// An account's calendars not to read, by their ids (#752).
    #[serde(default, rename = "left-out", skip_serializing_if = "Vec::is_empty")]
    pub left_out: Vec<String>,
}

/// The right to read a source, as consented now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    source: SourceKey,
    identity: String,
    notebook: Notebook,
    summaries_only: bool,
    left_out: Vec<String>,
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

    /// The calendars not to read, by their ids.
    pub fn left_out(&self) -> &[String] {
        &self.left_out
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
            left_out: Vec::new(),
        });
    }

    /// "Autoriser" on an Agenda's sheet, some of its calendars left out.
    pub fn allow_leaving_out(
        &mut self,
        source: SourceKey,
        identity: &str,
        notebook: Notebook,
        left_out: Vec<String>,
        at: Timestamp,
    ) {
        self.allow(source, identity, notebook, at);
        if let Some(last) = self.records.last_mut() {
            last.left_out = left_out;
        }
    }

    /// A granted source's calendars left out, as its row now says
    /// (#752): a new record, the grant otherwise as it was.
    pub fn leave_out(&mut self, source: &SourceKey, mut left_out: Vec<String>, at: Timestamp) {
        left_out.sort();
        left_out.dedup();
        if let Some(last) = self.last(source).cloned()
            && last.decision == Decision::Granted
            && last.left_out != left_out
        {
            self.records.push(Consent {
                left_out,
                at,
                ..last
            });
        }
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

    /// « C'est privé » checked or not on a source's row, for a source
    /// granted now (#752): a new record in `notebook`, the grant
    /// otherwise as it was. What came from it before moves with it (her
    /// memory's refile).
    pub fn set_notebook(&mut self, source: &SourceKey, notebook: Notebook, at: Timestamp) {
        if let Some(last) = self.last(source).cloned()
            && last.decision == Decision::Granted
            && last.notebook != notebook
        {
            self.records.push(Consent {
                notebook,
                at,
                ..last
            });
        }
    }

    /// Every source granted now, with its grant.
    pub fn granted(&self) -> Vec<Grant> {
        let mut keys: Vec<&SourceKey> = self.records.iter().map(|c| &c.source).collect();
        keys.sort();
        keys.dedup();
        keys.into_iter().filter_map(|k| self.grant(k)).collect()
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
    /// (`hocus_sources::cycle::Event::Purge`, her notifications' source).
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
            left_out: last.left_out.clone(),
        })
    }

    /// The right to read the notifications of the application `app`
    /// (its desktop entry `entry`), by either: a notification without its
    /// entry is still its application's.
    pub fn notifications(&self, app: &str, entry: Option<&str>) -> Option<Grant> {
        let by_entry = SourceKey::notifications(app, entry);
        self.grant(&by_entry).or_else(|| {
            let by_name = SourceKey::notifications(app, None);
            (by_name != by_entry)
                .then(|| self.grant(&by_name))
                .flatten()
        })
    }

    /// The applications whose notifications are read now, by their keys.
    pub fn notifying(&self) -> Vec<Grant> {
        let mut keys: Vec<&SourceKey> = self
            .records
            .iter()
            .map(|c| &c.source)
            .filter(|k| k.kind == SourceKind::Notifications)
            .collect();
        keys.sort();
        keys.dedup();
        keys.into_iter().filter_map(|k| self.grant(k)).collect()
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
pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
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
        // « C'est privé » on its row: the notebook moves, the rest stays.
        book.set_notebook(&mail, Notebook::Perso, Timestamp::UNIX_EPOCH);
        let grant = book.grant(&mail).expect("granted");
        assert_eq!(grant.notebook(), Notebook::Perso);
        assert!(grant.summaries_only());
        assert_eq!(book.granted().len(), 1);
        book.set_notebook(&mail, Notebook::Pro, Timestamp::UNIX_EPOCH);
        book.revoke(&mail, Timestamp::UNIX_EPOCH);
        assert_eq!(book.grant(&mail), None);
        assert_eq!(book.revoked(), [mail]);
        assert_eq!(book.records().len(), 5);
    }

    #[test]
    fn an_application_s_notifications_are_granted_by_its_entry_or_its_name() {
        let mut book = ConsentBook::default();
        assert_eq!(
            app_key("Slack", Some("com.slack.Slack.desktop")),
            "com.slack.slack"
        );
        assert_eq!(app_key(" Signal ", None), "signal");
        book.allow(
            SourceKey::notifications("Slack", Some("slack")),
            "Slack",
            Notebook::Pro,
            Timestamp::UNIX_EPOCH,
        );
        let slack = book.notifications("Slack", Some("slack")).expect("granted");
        assert_eq!(slack.notebook(), Notebook::Pro);
        assert_eq!(slack.source().to_string(), "slack:notifications");
        assert!(book.notifications("Slack", None).is_some(), "by its name");
        assert!(book.notifications("Signal", None).is_none());
        assert_eq!(book.notifying().len(), 1);
        // An account's kinds and an application's are apart.
        assert!(
            book.grant(&SourceKey::new("slack", SourceKind::Mail))
                .is_none()
        );
        book.revoke(
            &SourceKey::notifications("Slack", None),
            Timestamp::UNIX_EPOCH,
        );
        assert!(book.notifications("Slack", Some("slack")).is_none());
        assert!(book.notifying().is_empty());
        assert_eq!(
            book.revoked(),
            [SourceKey::new("slack", SourceKind::Notifications)]
        );
    }

    #[test]
    fn a_calendar_left_out_is_a_later_record() {
        let mut book = ConsentBook::default();
        let calendar = SourceKey::new("account_1", SourceKind::Calendar);
        book.allow_leaving_out(
            calendar.clone(),
            "alex@example.org",
            Notebook::Pro,
            vec!["conges".to_owned()],
            Timestamp::UNIX_EPOCH,
        );
        assert_eq!(book.grant(&calendar).expect("granted").left_out(), ["conges"]);
        book.leave_out(&calendar, Vec::new(), Timestamp::UNIX_EPOCH);
        assert!(book.grant(&calendar).expect("granted").left_out().is_empty());
        assert_eq!(book.records().len(), 2);
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
