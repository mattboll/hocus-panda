//! An account's calendars, every one of them, hidden ones included
//! (§10.1: "Congés", a family calendar), read with `hocus-day`'s Google
//! reader. The first read lists a window of events; the next ones ask
//! what changed since each calendar's sync token (a token expired lists
//! again). Read-only: the Calendar API is only asked `GET`s.

use std::collections::HashMap;

use hocus_day::{
    google::{AccessToken, GoogleCalendar},
    model::{AccountId, Answer, Calendar, CalendarId, FreeBusy, Status},
    source::{CalendarSource as _, Range, SyncToken},
};
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

use crate::{
    Result, SourceError,
    consent::Grant,
    item::{Content, Event, Item, Pointer},
};

/// Where each calendar's reading stopped.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    tokens: HashMap<String, String>,
}

/// What a read gives: events new or changed, and the ids of those gone
/// (their pointers' ids).
#[derive(Debug, Default)]
pub struct Read {
    pub items: Vec<Item>,
    pub removed: Vec<String>,
    pub calendars: Vec<Calendar>,
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

    /// The events of every calendar, `range` on a first read, what changed
    /// since `cursor` after. `grant` says it is consented, and for which
    /// notebook.
    pub fn read(
        &mut self,
        grant: &Grant,
        range: Range,
        cursor: &mut Cursor,
        zone: &TimeZone,
    ) -> Result<Read> {
        let calendars = self.google.calendars()?;
        let mut read = Read::default();
        for calendar in &calendars {
            let hidden = self.google.hidden(&calendar.id);
            let key = calendar.id.as_str().to_owned();
            let changes = match cursor.tokens.get(&key) {
                Some(token) => match self.google.changes(&calendar.id, &SyncToken(token.clone())) {
                    Ok(changes) => Some(changes),
                    Err(SourceError::Expired) => None,
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
                read.items.push(item(grant, calendar, hidden, event, zone));
            }
        }
        // A calendar gone: its token goes too.
        cursor
            .tokens
            .retain(|id, _| calendars.iter().any(|c| c.id.as_str() == id));
        read.calendars = calendars;
        Ok(read)
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
