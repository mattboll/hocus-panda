//! Her Google accounts as one [`Source`]: `hocus-sources`' cycle (every
//! account Online Accounts has on her bus, each kind the user consented
//! to, redacted by the first pass) run on the source's worker, its items
//! handed on with their text for the triage, its revocations as parts to
//! forget.
//!
//! The cycle's cursors (calendar sync tokens, IMAP positions, the day
//! birthdays were read) are her cursor for this source, as JSON, kept in
//! her memory like any other: a suspend's catch-up starts from them. The
//! consent book is a file in her folder (`consents.json`), which only the
//! consent sheets write.

use std::{path::PathBuf, time::Duration};

use hocus_secretary_link::Notebook;
use hocus_sources::{
    ConsentBook, Content, Redacted,
    cycle::{Cursors, Env, Event, Sources},
    goa,
};
use jiff::{Timestamp, tz::TimeZone};

use crate::sources::{EventInfo, Item, MailInfo, Source, Synced};

/// The consent book's file, in her folder.
pub const CONSENTS: &str = hocus_secretary_link::consent::CONSENTS;

/// Her Google accounts.
#[derive(Debug)]
pub struct Google {
    bus: String,
    consents: PathBuf,
    sources: Sources,
    every: Duration,
}

impl Google {
    /// The accounts on `bus` (an address, never the session's unless
    /// given), as consented in `consents`.
    pub fn new(bus: String, consents: PathBuf, env: Env) -> Self {
        Self {
            bus,
            consents,
            sources: Sources::new(env, Cursors::default()),
            every: Duration::from_secs(300),
        }
    }
}

impl Source for Google {
    fn name(&self) -> &str {
        "google"
    }

    fn notebook(&self) -> Notebook {
        // Each item carries its account's, as consented.
        Notebook::Pro
    }

    fn every(&self) -> Duration {
        self.every
    }

    fn sync(&mut self, cursor: Option<&str>, now: Timestamp) -> Result<Synced, String> {
        self.sources.cursors = cursor
            .filter(|c| !c.is_empty())
            .and_then(|c| serde_json::from_str(c).ok())
            .unwrap_or_default();
        let accounts = match goa::discover(&self.bus) {
            Ok(accounts) => accounts,
            // No Online Accounts on this bus at all: no account, not a
            // failure (her first run says so).
            Err(err) if absent(&err.to_string()) => {
                tracing::info!(%err, "no Online Accounts: no Google account");
                Vec::new()
            }
            Err(err) => return Err(format!("Online Accounts: {err}")),
        };
        let consents = ConsentBook::load(&self.consents);
        let now = now.to_zoned(TimeZone::system());
        let events = self.sources.cycle(&accounts, &consents, &now);
        let mut synced = gather(events, &consents);
        synced.cursor = serde_json::to_string(&self.sources.cursors)
            .map_err(|e| format!("the cursors do not write: {e}"))?;
        synced.found = Some(accounts);
        Ok(synced)
    }
}

/// `env` with Google's services at `url`, a fake on this machine (tests):
/// the Calendar API and CardDAV there, IMAP on its port in clear. Another
/// address is refused: tests never reach the network.
pub fn at(mut env: Env, url: &str) -> Result<Env, String> {
    let local = ["http://127.0.0.1:", "http://localhost:"];
    let Some(port) = local
        .iter()
        .find_map(|l| url.strip_prefix(l))
        .and_then(|rest| rest.split('/').next())
        .and_then(|p| p.parse::<u16>().ok())
    else {
        return Err(format!("--google-at {url}: only this machine"));
    };
    env.calendar_api = hocus_day::google::api(Some(url));
    env.carddav_api = hocus_sources::contacts::api(Some(url));
    env.imap_override = Some(goa::Imap {
        host: "127.0.0.1".to_owned(),
        port,
        user: String::new(),
        tls: false,
    });
    Ok(env)
}

/// Online Accounts is not on the bus (not installed, not activatable).
fn absent(error: &str) -> bool {
    error.contains("ServiceUnknown") || error.contains("NameHasNoOwner")
}

/// A cycle's events as hers: items with their text, parts to forget.
fn gather(events: Vec<Event>, consents: &ConsentBook) -> Synced {
    let mut synced = Synced::default();
    for event in events {
        match event {
            Event::Items { source, items } => {
                let summaries_only = consents.grant(&source).is_some_and(|g| g.summaries_only());
                synced.items.extend(
                    items
                        .into_iter()
                        .map(|r| item(r, &source.to_string(), summaries_only)),
                );
            }
            Event::Purge(source) => synced.forget.push(source.to_string()),
            Event::PurgePart { source, part } => {
                tracing::info!(%source, part, "a calendar left out: what came from it goes");
                synced.forget.push(format!("{source}#{part}"));
            }
            Event::Calendars { account, list } => {
                synced
                    .calendars
                    .get_or_insert_with(Default::default)
                    .insert(account, list);
            }
            Event::Removed { source, ids } => {
                tracing::info!(%source, removed = ids.len(), "items gone at their source");
            }
            Event::Reset { source, mailbox } => {
                tracing::info!(%source, mailbox, "a mailbox's ids changed: read again");
            }
            Event::Failed { source, error } => {
                tracing::info!(%source, %error, "a source is not read this time");
            }
        }
    }
    synced
}

/// An item of hers from a redacted one: its digest the metadata that may
/// leave before the second pass (§14.1, 3); its text held for the triage.
fn item(redacted: Redacted, from: &str, summaries_only: bool) -> Item {
    let source = &redacted.item;
    let (calendar, until) = match &source.content {
        Content::Event(event) => (
            Some(event.calendar_name.clone()),
            Some(event.when.end(&TimeZone::system())),
        ),
        _ => (None, None),
    };
    let (kind, digest, text) = match &source.content {
        Content::Mail(mail) => (
            "mail",
            format!("{} : {}", mail.from, mail.subject),
            mail.body.is_some(),
        ),
        Content::Event(event) => (
            "event",
            event.title.clone(),
            event.description.is_some() || event.place.is_some(),
        ),
        Content::Birthday(b) => (
            "birthday",
            format!("anniversaire de {} : {}/{}", b.name, b.day, b.month),
            false,
        ),
        // Not an account's: her notifications' source's.
        Content::Notification(n) => ("notification", format!("{} : {}", n.app, n.title), true),
    };
    let (event, mail) = match &source.content {
        Content::Event(e) => {
            let end = match e.when {
                hocus_day::model::When::Timed { end, .. } => Some(end),
                _ => None,
            };
            (
                end.map(|end| EventInfo {
                    end,
                    calendar: e.calendar_name.clone(),
                    attendees: Vec::new(),
                    organiser: None,
                    presenter: None,
                    busy: e.busy && !e.declined && !e.cancelled,
                }),
                None,
            )
        }
        Content::Mail(m) => (
            None,
            Some(MailInfo {
                to: m.to.clone(),
                sent: m.sent,
                message_id: m.message_id.clone(),
                in_reply_to: m.in_reply_to.clone(),
                headers: std::collections::BTreeMap::new(),
            }),
        ),
        _ => (None, None),
    };
    // An event is its calendar's: forgotten with it when it is left out
    // (#752), with its account's Agenda when that is revoked.
    let from = match &source.content {
        Content::Event(e) => format!("{from}#{}", e.calendar),
        _ => from.to_owned(),
    };
    Item {
        id: source.pointer.id.clone(),
        at: source.at,
        kind: kind.to_owned(),
        digest,
        notebook: Some(source.notebook),
        from: Some(from),
        project: None,
        sender: match &source.content {
            Content::Mail(m) => Some(m.from.clone()),
            _ => None,
        },
        body: None,
        event,
        mail,
        calendar,
        until,
        open: source.pointer.open.clone(),
        summaries_only,
        text: text.then(|| Box::new(redacted)),
    }
}

#[cfg(test)]
mod tests {
    use hocus_sources::{
        Check, Pointer, Redactor, SourceKey, SourceKind,
        item::{self, Mail},
    };

    use super::*;

    #[test]
    fn a_cycle_s_mail_comes_with_its_redacted_text_and_a_revocation_is_forgotten() {
        let mail = SourceKey::new("account_1", SourceKind::Mail);
        let mut consents = ConsentBook::default();
        consents.allow(
            mail.clone(),
            "alex@example.org",
            Notebook::Perso,
            Timestamp::UNIX_EPOCH,
        );
        consents.summaries_only(&mail, true, Timestamp::UNIX_EPOCH);
        let redacted = Redactor::new().redact(item::Item {
            pointer: Pointer {
                source: mail.clone(),
                id: "INBOX/7/42".to_owned(),
                label: "le mail de Free, 3 nov.".to_owned(),
                open: None,
            },
            notebook: Notebook::Perso,
            at: Timestamp::UNIX_EPOCH,
            content: Content::Mail(Mail {
                from: "Free".to_owned(),
                subject: "Vos accès".to_owned(),
                body: Some("mdp du wifi : chienchat".to_owned()),
                ..Mail::default()
            }),
        });
        let gone = SourceKey::new("account_2", SourceKind::Calendar);
        let synced = gather(
            vec![
                Event::Items {
                    source: mail,
                    items: vec![redacted],
                },
                Event::Purge(gone),
            ],
            &consents,
        );
        assert_eq!(synced.forget, ["account_2:calendar"]);
        let item = &synced.items[0];
        assert_eq!(item.from.as_deref(), Some("account_1:mail"));
        assert_eq!(item.digest, "Free : Vos accès");
        assert_eq!(item.notebook, Some(Notebook::Perso));
        assert!(item.summaries_only);
        let text = item.text.as_ref().expect("its text");
        assert_eq!(text.check, Check::FirstPass);
        let Content::Mail(mail) = &text.item.content else {
            panic!("a mail");
        };
        assert_eq!(
            mail.body.as_deref(),
            Some("mdp du wifi : [secret masqué — voir le mail de Free, 3 nov.]")
        );
    }
}
