//! One read of every consented source, as the secretary's source workers
//! run it (§16): for each Google account Online Accounts has, and each of
//! its kinds the user allowed, what is new, redacted (the first pass) and
//! tagged with the notebook chosen at consent. A source not allowed is
//! not read (no event, no mail, no contact); only an Agenda not decided
//! yet has its calendars listed, their names and who shares them, for its
//! consent sheet to show (#752, [`Event::Calendars`]). A source revoked
//! since the last cycle is not touched again and its items are to be
//! purged ([`Event::Purge`], once); a calendar left out since, its items
//! ([`Event::PurgePart`]).
//!
//! Items come out as [`crate::Redacted`]: never raw. The tokens Online
//! Accounts handed during the cycle are given to the redactor as known
//! secrets, so that one found in content is masked too.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use hocus_day::{google::AccessToken, secrets::Token, source::Range};
use jiff::{Zoned, civil::Date};
use serde::{Deserialize, Serialize};

use crate::{
    Result, SourceError,
    calendar::{self, Reader},
    consent::ConsentBook,
    contacts, goa,
    goa::GoogleAccount,
    imap,
    item::{SourceKey, SourceKind},
    redact::{Redacted, Redactor},
};

/// Where the sources are.
#[derive(Debug, Clone)]
pub struct Env {
    /// The session bus the process was given (an address).
    pub bus: String,
    /// The Calendar API (`hocus_day::google::api`).
    pub calendar_api: String,
    /// CardDAV ([`contacts::api`]).
    pub carddav_api: String,
    /// Mail's text is fetched with its headers (else on demand).
    pub mail_text: bool,
    /// IMAP's address, for tests only (a fake on this machine): it
    /// replaces every account's.
    pub imap_override: Option<goa::Imap>,
}

impl Env {
    /// Google's own services, the accounts on `bus`.
    pub fn google(bus: &str) -> Self {
        Self {
            bus: bus.to_owned(),
            calendar_api: hocus_day::google::api(None),
            carddav_api: contacts::api(None),
            mail_text: true,
            imap_override: None,
        }
    }
}

/// What a cycle gives.
#[derive(Debug)]
pub enum Event {
    /// New or changed items, redacted.
    Items {
        source: SourceKey,
        items: Vec<Redacted>,
    },
    /// Items gone at their source (their pointers' ids).
    Removed { source: SourceKey, ids: Vec<String> },
    /// A mailbox's UIDs changed: what was read of it before is void.
    Reset { source: SourceKey, mailbox: String },
    /// The source was revoked: everything that came from it is to be
    /// forgotten (its digests, index lines, episodes).
    Purge(SourceKey),
    /// An account's calendars as listed (its Agenda's sheet and row),
    /// each failing one said.
    Calendars {
        account: String,
        list: Vec<hocus_secretary_link::calendars::CalendarInfo>,
    },
    /// A part of a source left out since (a calendar of an account): what
    /// came from it is to be forgotten.
    PurgePart { source: SourceKey, part: String },
    /// It could not be read this time; what was read stays.
    Failed {
        source: SourceKey,
        error: SourceError,
    },
}

/// Where each source's reading stopped, kept across cycles (and on disk
/// by the process: a suspend's catch-up starts from it).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursors {
    calendars: BTreeMap<String, calendar::Cursor>,
    mail: BTreeMap<String, imap::Cursor>,
    birthdays: BTreeMap<String, Date>,
    purged: BTreeSet<SourceKey>,
}

/// An account's tokens, each also kept to be masked.
struct Remembering {
    inner: Box<dyn AccessToken>,
    seen: Arc<Mutex<Vec<String>>>,
}

impl AccessToken for Remembering {
    fn get(&mut self) -> Result<(Token, Duration)> {
        let (token, lasts) = self.inner.get()?;
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(token.expose().to_owned());
        Ok((token, lasts))
    }
}

/// The sources' reader.
#[derive(Debug)]
pub struct Sources {
    pub env: Env,
    pub redactor: Redactor,
    pub cursors: Cursors,
}

impl Sources {
    pub fn new(env: Env, cursors: Cursors) -> Self {
        Self {
            env,
            redactor: Redactor::new(),
            cursors,
        }
    }

    fn tokens(
        &self,
        account: &GoogleAccount,
        seen: &Arc<Mutex<Vec<String>>>,
    ) -> Result<Box<dyn AccessToken>> {
        Ok(Box::new(Remembering {
            inner: goa::token(&self.env.bus, &account.path)?,
            seen: seen.clone(),
        }))
    }

    /// One cycle over `accounts` (as [`goa::discover`] gives them), as
    /// `consents` allow, at `now`. Blocking.
    pub fn cycle(
        &mut self,
        accounts: &[GoogleAccount],
        consents: &ConsentBook,
        now: &Zoned,
    ) -> Vec<Event> {
        let mut events = Vec::new();
        // An application's notifications are her notifications' source's.
        for source in consents
            .revoked()
            .into_iter()
            .filter(|s| SourceKind::ALL.contains(&s.kind))
        {
            if self.cursors.purged.insert(source.clone()) {
                self.forget(&source);
                events.push(Event::Purge(source));
            }
        }
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        for account in accounts {
            for kind in SourceKind::ALL {
                let source = SourceKey::new(&account.id, kind);
                let Some(grant) = consents.grant(&source) else {
                    // An Agenda not decided yet: its calendars listed for
                    // its sheet, nothing of them read.
                    let undecided = !consents.records().iter().any(|r| r.source == source);
                    if kind == SourceKind::Calendar && account.calendar && undecided {
                        match self.list_calendars(account, &seen) {
                            Ok(list) => events.push(Event::Calendars {
                                account: account.id.clone(),
                                list,
                            }),
                            Err(error) => {
                                tracing::info!(%source, %error, "the calendars are not listed");
                            }
                        }
                    }
                    continue;
                };
                self.cursors.purged.remove(&source);
                let on = match kind {
                    SourceKind::Calendar => account.calendar,
                    SourceKind::Birthdays => account.contacts,
                    SourceKind::Mail => account.mail,
                    SourceKind::Notifications => false,
                };
                if !on {
                    continue;
                }
                let read = self.read(account, &grant, now, &seen, &mut events);
                // The tokens handed are masked wherever they show up.
                for token in seen
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .drain(..)
                {
                    self.redactor.know(token);
                }
                match read {
                    Ok(items) if !items.is_empty() => {
                        let items = items.into_iter().map(|i| self.redactor.redact(i)).collect();
                        events.push(Event::Items { source, items });
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::info!(%source, %error, "a source is not read this cycle");
                        events.push(Event::Failed { source, error });
                    }
                }
            }
        }
        events
    }

    /// An account's calendars, listed only.
    fn list_calendars(
        &self,
        account: &GoogleAccount,
        seen: &Arc<Mutex<Vec<String>>>,
    ) -> Result<Vec<hocus_secretary_link::calendars::CalendarInfo>> {
        let source = SourceKey::new(&account.id, SourceKind::Calendar);
        // No grant yet: a reader made for listing only.
        let mut book = ConsentBook::default();
        book.allow(
            source.clone(),
            &account.identity,
            account.suggested,
            jiff::Timestamp::UNIX_EPOCH,
        );
        let grant = book.grant(&source).ok_or_else(|| SourceError::Gone)?;
        let mut reader = Reader::new(
            &grant,
            self.tokens(account, seen)?,
            self.env.calendar_api.clone(),
        );
        reader.list()
    }

    fn forget(&mut self, source: &SourceKey) {
        let account = &source.account;
        match source.kind {
            SourceKind::Calendar => {
                self.cursors.calendars.remove(account);
            }
            SourceKind::Mail => {
                self.cursors.mail.remove(account);
            }
            SourceKind::Birthdays => {
                self.cursors.birthdays.remove(account);
            }
            SourceKind::Notifications => {}
        }
    }

    fn read(
        &mut self,
        account: &GoogleAccount,
        grant: &crate::Grant,
        now: &Zoned,
        seen: &Arc<Mutex<Vec<String>>>,
        events: &mut Vec<Event>,
    ) -> Result<Vec<crate::Item>> {
        let source = grant.source().clone();
        let zone = now.time_zone().clone();
        match source.kind {
            SourceKind::Calendar => {
                let mut reader = Reader::new(
                    grant,
                    self.tokens(account, seen)?,
                    self.env.calendar_api.clone(),
                );
                let cursor = self
                    .cursors
                    .calendars
                    .entry(account.id.clone())
                    .or_default();
                let read = reader.read(grant, Range::window(now), cursor, &zone)?;
                for calendar in read.unreadable() {
                    tracing::info!(
                        %source,
                        calendar = calendar.name,
                        why = calendar.unreadable.as_deref().unwrap_or_default(),
                        "a calendar could not be read"
                    );
                }
                events.push(Event::Calendars {
                    account: account.id.clone(),
                    list: read.listed,
                });
                for part in read.left {
                    events.push(Event::PurgePart {
                        source: source.clone(),
                        part,
                    });
                }
                if !read.removed.is_empty() {
                    events.push(Event::Removed {
                        source,
                        ids: read.removed,
                    });
                }
                Ok(read.items)
            }
            SourceKind::Birthdays => {
                let today = now.date();
                if self.cursors.birthdays.get(&account.id) == Some(&today) {
                    return Ok(Vec::new());
                }
                let mut tokens = self.tokens(account, seen)?;
                let items = contacts::read(grant, tokens.as_mut(), &self.env.carddav_api, today)?;
                self.cursors.birthdays.insert(account.id.clone(), today);
                Ok(items)
            }
            SourceKind::Mail => {
                let imap = self
                    .env
                    .imap_override
                    .clone()
                    .unwrap_or_else(|| account.imap.clone());
                let mut tokens = self.tokens(account, seen)?;
                let cursor = self.cursors.mail.entry(account.id.clone()).or_default();
                let read = imap::sync(
                    grant,
                    &imap,
                    tokens.as_mut(),
                    cursor,
                    now.date(),
                    &zone,
                    self.env.mail_text,
                )?;
                for mailbox in read.reset {
                    events.push(Event::Reset {
                        source: source.clone(),
                        mailbox,
                    });
                }
                Ok(read.items)
            }
            SourceKind::Notifications => Ok(Vec::new()),
        }
    }
}
