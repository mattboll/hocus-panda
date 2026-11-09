//! An account's calendars, every one of them, hidden ones included
//! (§10.1: "Congés", a family calendar) and those others share (#752),
//! read with `hocus-day`'s Google reader. The first read lists a window of
//! events; the next ones ask what changed since each calendar's sync token
//! (a token expired, or refused, lists again). A calendar shared as
//! free/busy only gives its busy blocks (`POST /freeBusy`, a read that only
//! answers), as "Occupé" events of no title. Read-only otherwise: the
//! Calendar API is only asked `GET`s.
//!
//! One calendar that cannot be read never stops the others: it is said
//! ([`Read::unreadable`]) and logged with its name; only an account whose
//! token is refused, or whose every calendar fails, fails the read. The
//! calendars the user left out on the consent are not read, and what came
//! from one left out since is to be forgotten ([`Read::left`]).

use std::collections::HashMap;

use hocus_day::{
    google::{AccessToken, GoogleCalendar},
    model::{AccountId, Answer, Calendar, CalendarId, FreeBusy, Status},
    source::{CalendarSource as _, Range, SyncToken},
};
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

use hocus_secretary_link::calendars::CalendarInfo;

use crate::{
    Result, SourceError,
    consent::Grant,
    item::{Content, Event, Item, Pointer},
};

/// Where each calendar's reading stopped.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    tokens: HashMap<String, String>,
    /// A free/busy-only calendar's blocks, as last read (#752).
    #[serde(default)]
    busy: HashMap<String, Vec<String>>,
}

/// What a read gives: events new or changed, and the ids of those gone
/// (their pointers' ids).
#[derive(Debug, Default)]
pub struct Read {
    pub items: Vec<Item>,
    pub removed: Vec<String>,
    pub calendars: Vec<Calendar>,
    /// Every calendar listed, as the consent's sheet and row show it, its
    /// failure this time said.
    pub listed: Vec<CalendarInfo>,
    /// The calendars read before and left out since: what came from them
    /// goes.
    pub left: Vec<String>,
}

impl Read {
    /// The calendars that could not be read this time.
    pub fn unreadable(&self) -> impl Iterator<Item = &CalendarInfo> {
        self.listed.iter().filter(|c| c.unreadable.is_some())
    }
}

/// An account's calendars, through its tokens.
pub struct Reader {
    google: GoogleCalendar,
}

impl std::fmt::Debug for Reader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reader").finish_non_exhaustive()
    }
}

impl Reader {
    /// `grant`'s account at `api` (`hocus_day::google::api`), with
    /// `tokens`.
    pub fn new(grant: &Grant, tokens: Box<dyn AccessToken>, api: String) -> Self {
        let account = AccountId::new(format!("secretary-{}", grant.source().account));
        Self {
            google: GoogleCalendar::new(account, tokens, api).with_hidden(),
        }
    }

    /// Every calendar of the account, as its consent's sheet shows them:
    /// names and who shares them, never an event.
    pub fn list(&mut self) -> Result<Vec<CalendarInfo>> {
        let calendars = self.google.calendars()?;
        Ok(calendars.iter().map(|c| self.info(c)).collect())
    }

    fn info(&self, calendar: &Calendar) -> CalendarInfo {
        let access = self
            .google
            .access(&calendar.id)
            .cloned()
            .unwrap_or_default();
        CalendarInfo {
            id: calendar.id.as_str().to_owned(),
            name: calendar.name.clone(),
            shared: access.shared_by.is_none() && !access.primary && access.role != "owner",
            shared_by: access.shared_by.clone(),
            hidden: self.google.hidden(&calendar.id),
            busy_only: access.busy_only(),
            unreadable: None,
        }
    }

    /// The events of every calendar but those left out, `range` on a first
    /// read, what changed since `cursor` after. `grant` says it is
    /// consented, and for which notebook.
    pub fn read(
        &mut self,
        grant: &Grant,
        range: Range,
        cursor: &mut Cursor,
        zone: &TimeZone,
    ) -> Result<Read> {
        let calendars = self.google.calendars()?;
        let mut read = Read::default();
        let mut failed: Option<SourceError> = None;
        let mut fine = 0;
        for calendar in &calendars {
            let key = calendar.id.as_str().to_owned();
            let mut info = self.info(calendar);
            if grant.left_out().contains(&key) {
                // Read before, left out since: what came from it goes.
                if cursor.tokens.remove(&key).is_some() || cursor.busy.remove(&key).is_some() {
                    read.left.push(key);
                }
                read.listed.push(info);
                continue;
            }
            match self.read_one(grant, calendar, &info, range, cursor, zone, &mut read) {
                Ok(()) => fine += 1,
                Err(err @ SourceError::SignIn(_)) => return Err(err),
                Err(err) => {
                    tracing::info!(
                        calendar = calendar.name,
                        %err,
                        "a calendar is not read this time; the others are"
                    );
                    info.unreadable = Some(err.to_string());
                    failed.get_or_insert(err);
                }
            }
            read.listed.push(info);
        }
        // Every calendar failed: the account's read did.
        if fine == 0
            && let Some(err) = failed
        {
            return Err(err);
        }
        // A calendar gone: its token goes too.
        cursor
            .tokens
            .retain(|id, _| calendars.iter().any(|c| c.id.as_str() == id));
        cursor
            .busy
            .retain(|id, _| calendars.iter().any(|c| c.id.as_str() == id));
        read.calendars = calendars;
        Ok(read)
    }

    /// One calendar's events into `read`.
    #[allow(clippy::too_many_arguments)]
    fn read_one(
        &mut self,
        grant: &Grant,
        calendar: &Calendar,
        info: &CalendarInfo,
        range: Range,
        cursor: &mut Cursor,
        zone: &TimeZone,
        read: &mut Read,
    ) -> Result<()> {
        let key = calendar.id.as_str().to_owned();
        if info.busy_only {
            return self.read_busy(grant, calendar, range, cursor, zone, read);
        }
        let changes = match cursor.tokens.get(&key) {
            Some(token) => match self.google.changes(&calendar.id, &SyncToken(token.clone())) {
                Ok(changes) => Some(changes),
                Err(SourceError::Expired) => None,
                // A token some calendars refuse (or no longer take): the
                // window listed again.
                Err(SourceError::Refused(why)) => {
                    tracing::info!(
                        calendar = calendar.name,
                        why,
                        "its sync token is refused: listed again"
                    );
                    None
                }
                Err(err) => return Err(err),
            },
            None => None,
        };
        let (events, removed, token) = match changes {
            Some(changes) => (changes.changed, changes.removed, changes.token),
            None => {
                let listing = self.google.events(&calendar.id, range)?;
                (listing.events, Vec::new(), listing.token)
            }
        };
        match token {
            Some(token) => cursor.tokens.insert(key, token.0),
            None => cursor.tokens.remove(&key),
        };
        read.removed.extend(
            removed
                .into_iter()
                .map(|id| event_id(&calendar.id, id.as_str())),
        );
        for event in events {
            read.items
                .push(item(grant, calendar, info.hidden, event, zone));
        }
        Ok(())
    }

    /// A free/busy-only calendar: its busy blocks within `range`, each an
    /// event "Occupé" of no title; those gone since its last read said
    /// removed.
    fn read_busy(
        &mut self,
        grant: &Grant,
        calendar: &Calendar,
        range: Range,
        cursor: &mut Cursor,
        zone: &TimeZone,
        read: &mut Read,
    ) -> Result<()> {
        let key = calendar.id.as_str().to_owned();
        let blocks = self.google.busy(&calendar.id, range)?;
        let before = cursor.busy.remove(&key).unwrap_or_default();
        let mut now = Vec::new();
        for (start, end) in blocks {
            let id = format!("busy-{}-{}", start.as_second(), end.as_second());
            if !before.contains(&id) {
                let event = hocus_day::model::Event {
                    id: hocus_day::model::EventId::new(id.clone()),
                    calendar: calendar.id.clone(),
                    title: "Occupé".to_owned(),
                    when: hocus_day::model::When::Timed { start, end },
                    status: Status::default(),
                    answer: Answer::default(),
                    free_busy: FreeBusy::Busy,
                    place: None,
                    video: None,
                    page: None,
                    description: None,
                    alarms: Vec::new(),
                };
                read.items.push(item(grant, calendar, false, event, zone));
            }
            now.push(id);
        }
        read.removed.extend(
            before
                .iter()
                .filter(|id| !now.contains(id))
                .map(|id| event_id(&calendar.id, id)),
        );
        cursor.busy.insert(key, now);
        Ok(())
    }
}

fn event_id(calendar: &CalendarId, event: &str) -> String {
    format!("{}/{event}", calendar.as_str())
}

fn item(
    grant: &Grant,
    calendar: &Calendar,
    hidden: bool,
    event: hocus_day::model::Event,
    zone: &TimeZone,
) -> Item {
    let at = event.when.start(zone);
    let day = crate::mail::short_date(at.to_zoned(zone.clone()).date());
    Item {
        pointer: Pointer {
            source: grant.source().clone(),
            id: event_id(&calendar.id, event.id.as_str()),
            label: format!("l'agenda {}, {day}", calendar.name),
            open: event.page.clone(),
        },
        notebook: grant.notebook(),
        at,
        content: Content::Event(Event {
            calendar: calendar.id.as_str().to_owned(),
            calendar_name: calendar.name.clone(),
            hidden_calendar: hidden,
            title: event.title,
            when: event.when,
            busy: event.free_busy == FreeBusy::Busy,
            declined: event.answer == Answer::Declined,
            cancelled: event.status == Status::Cancelled,
            place: event.place,
            description: event.description,
        }),
    }
}
