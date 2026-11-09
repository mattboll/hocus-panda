//! Google Calendar (Day.0c, #567): a [`CalendarSource`] over the Calendar
//! API v3, its access token handed by GNOME Online Accounts
//! ([`crate::goa`]). Blocking HTTPS through `ureq`, on its account's
//! worker, never the shell's loop.
//!
//! - `calendarList` gives the calendars: `summaryOverride` else `summary`,
//!   `backgroundColor`, `selected`, `accessRole`; with `showHidden` (the
//!   secretary's reader) the calendars the user hid too, marked `hidden`.
//! - `events`, with `singleEvents=true` (recurring events expanded by
//!   Google, exceptions honoured), lists the window and gives a
//!   `nextSyncToken`; a sync from it gives what changed, a `410` that it
//!   expired (the worker lists again). A month outside the window
//!   (Day.5b) is listed the same way when the calendar turns to it, its
//!   token left unused: the window's syncs keep it up to date.
//! - The token is asked for again when it expires, and once on a `401`;
//!   Online Accounts refusing it is [`SourceError::SignIn`].
//! - A calendar the user may see only as busy (`freeBusyReader`, a
//!   colleague's shared "free/busy only") has no events to list: its busy
//!   blocks come from `POST /freeBusy` ([`GoogleCalendar::busy`]), a read
//!   that only answers (#752). Who shares a calendar is its `dataOwner`,
//!   else its id when that is someone's address.

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use jiff::{Timestamp, civil::Date};
use serde::{Deserialize, de::DeserializeOwned};

use crate::{
    model::{
        AccountId, Answer, Calendar, CalendarId, Colour, Event, EventId, FreeBusy, Status, When,
        meeting_address,
    },
    secrets::Token,
    source::{CalendarSource, Changes, Listing, Range, Result, SourceError, SyncToken},
};

/// The Calendar API's address.
pub const API: &str = "https://www.googleapis.com/calendar/v3";

/// For tests only: the Calendar API's address, a fake on this machine
/// (`http://127.0.0.1:PORT`); any other is not taken.
pub const API_ENV: &str = "HOCUS_TEST_GOOGLE_API";

/// The API's address `test` names when it is one on this machine, else
/// Google's: an account's token never leaves for another host.
pub fn api(test: Option<&str>) -> String {
    let local = ["http://127.0.0.1:", "http://[::1]:", "http://localhost:"];
    match test.map(|t| t.trim_end_matches('/')) {
        Some(test) if local.iter().any(|l| test.starts_with(l)) => test.to_owned(),
        Some(test) => {
            tracing::warn!(address = test, "{API_ENV} is not on this machine: left out");
            API.to_owned()
        }
        None => API.to_owned(),
    }
}

/// How long an answer has, and how much of one is read.
const TIMEOUT: Duration = Duration::from_secs(30);
const LARGEST: u64 = 64 * 1024 * 1024;
/// A token is asked for again this long before it expires.
const MARGIN: Duration = Duration::from_secs(60);
/// How long a token said to last no time at all is kept.
const UNSAID: Duration = Duration::from_secs(5 * 60);

/// Where an account's access token comes from (Online Accounts).
pub trait AccessToken: Send {
    /// A token, and how long it lasts; [`SourceError::SignIn`] when the
    /// account needs signing in again.
    fn get(&mut self) -> Result<(Token, Duration)>;
}

/// A Google account's calendars.
pub struct GoogleCalendar {
    account: AccountId,
    tokens: Box<dyn AccessToken>,
    token: Option<(Token, Instant)>,
    agent: ureq::Agent,
    base: String,
    /// Each calendar's own reminders, for its events that use them.
    defaults: HashMap<CalendarId, Vec<u32>>,
    /// Calendars the user hid from the list are listed too (the
    /// secretary reads them: "Congés", a family calendar).
    show_hidden: bool,
    /// The calendars listed that the user hid.
    hidden: HashSet<CalendarId>,
    /// Each calendar listed: the user's access to it, and who shares it.
    access: HashMap<CalendarId, Access>,
}

/// What the user may do with a calendar, and whose it is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Access {
    /// `owner`, `writer`, `reader`, `freeBusyReader`.
    pub role: String,
    /// The account's own main calendar.
    pub primary: bool,
    /// Who shares it with the user, when it is someone else's.
    pub shared_by: Option<String>,
}

impl Access {
    /// Only its busy blocks may be read.
    pub fn busy_only(&self) -> bool {
        self.role == "freeBusyReader"
    }
}

impl std::fmt::Debug for GoogleCalendar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleCalendar")
            .field("account", &self.account)
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl GoogleCalendar {
    /// `account`'s calendars at `base` ([`api`]), with `tokens`.
    pub fn new(account: AccountId, tokens: Box<dyn AccessToken>, base: String) -> Self {
        Self {
            account,
            tokens,
            token: None,
            agent: crate::http::agent(TIMEOUT),
            base,
            defaults: HashMap::new(),
            show_hidden: false,
            hidden: HashSet::new(),
            access: HashMap::new(),
        }
    }

    /// The user's access to `calendar`, as last listed.
    pub fn access(&self, calendar: &CalendarId) -> Option<&Access> {
        self.access.get(calendar)
    }

    /// `calendar`'s busy blocks within `range` (`POST /freeBusy`): a
    /// calendar shared as free/busy only.
    pub fn busy(
        &mut self,
        calendar: &CalendarId,
        range: Range,
    ) -> Result<Vec<(Timestamp, Timestamp)>> {
        let body = serde_json::json!({
            "timeMin": range.from.to_string(),
            "timeMax": range.to.to_string(),
            "items": [{"id": calendar.as_str()}],
        });
        let answer: FreeBusyAnswer = self.post("/freeBusy", &body)?;
        let Some(found) = answer.calendars.get(calendar.as_str()) else {
            return Err(SourceError::Refused(
                "Google Calendar says nothing of the calendar".to_owned(),
            ));
        };
        if let Some(error) = found.errors.first() {
            return Err(SourceError::Refused(format!(
                "Google Calendar: {}",
                error.reason
            )));
        }
        Ok(found
            .busy
            .iter()
            .filter_map(|b| Some((b.start.parse().ok()?, b.end.parse().ok()?)))
            .collect())
    }

    /// `POST path` with `body`, read as `T`; a `401` asks for a new token
    /// once.
    fn post<T: DeserializeOwned>(&mut self, path: &str, body: &serde_json::Value) -> Result<T> {
        let url = format!("{}{path}", self.base);
        let text = body.to_string();
        let mut asked_again = false;
        loop {
            let token = self.token()?;
            let request = self
                .agent
                .post(&url)
                .header("Authorization", format!("Bearer {}", token.expose()))
                .header("content-type", "application/json");
            let (status, answer) =
                crate::http::read(request.send(text.as_bytes()), "Google Calendar", LARGEST)?;
            match status {
                200..=299 => {
                    return serde_json::from_str(&answer).map_err(|err| {
                        SourceError::Refused(format!(
                            "Google Calendar's answer does not read: {err}"
                        ))
                    });
                }
                401 if !asked_again => {
                    self.token = None;
                    asked_again = true;
                }
                401 => {
                    return Err(SourceError::SignIn(
                        "Google refuses the account's token".to_owned(),
                    ));
                }
                429 | 500..=599 => {
                    return Err(SourceError::Offline(format!(
                        "Google Calendar answers {status}"
                    )));
                }
                _ => {
                    return Err(SourceError::Refused(format!(
                        "Google Calendar answers {status}"
                    )));
                }
            }
        }
    }

    /// The calendars the user hid from the list are listed too
    /// (`showHidden`).
    pub fn with_hidden(mut self) -> Self {
        self.show_hidden = true;
        self
    }

    /// Whether `calendar`, as last listed, is one the user hid.
    pub fn hidden(&self, calendar: &CalendarId) -> bool {
        self.hidden.contains(calendar)
    }

    /// A token not about to expire: the last one, else a new one.
    fn token(&mut self) -> Result<Token> {
        if let Some((token, until)) = &self.token
            && Instant::now() < *until
        {
            return Ok(token.clone());
        }
        let (token, lasts) = self.tokens.get()?;
        let lasts = if lasts.is_zero() { UNSAID } else { lasts };
        self.token = Some((token.clone(), Instant::now() + lasts.saturating_sub(MARGIN)));
        Ok(token)
    }

    /// `GET path?query`, read as `T`; a `401` asks for a new token once.
    fn get<T: DeserializeOwned>(&mut self, path: &str, query: &[(&str, &str)]) -> Result<T> {
        let url = format!("{}{path}", self.base);
        let mut asked_again = false;
        loop {
            let token = self.token()?;
            let mut request = self
                .agent
                .get(&url)
                .header("Authorization", format!("Bearer {}", token.expose()));
            for (key, value) in query {
                request = request.query(key, value);
            }
            let (status, body) = crate::http::read(request.call(), "Google Calendar", LARGEST)?;
            match status {
                200..=299 => {
                    return serde_json::from_str(&body).map_err(|err| {
                        SourceError::Refused(format!(
                            "Google Calendar's answer does not read: {err}"
                        ))
                    });
                }
                401 if !asked_again => {
                    tracing::debug!(account = %self.account, "Google refused the token: asked again");
                    self.token = None;
                    asked_again = true;
                }
                401 => {
                    return Err(SourceError::SignIn(
                        "Google refuses the account's token".to_owned(),
                    ));
                }
                410 => return Err(SourceError::Expired),
                429 | 500..=599 => {
                    return Err(SourceError::Offline(format!(
                        "Google Calendar answers {status}"
                    )));
                }
                _ => {
                    return Err(SourceError::Refused(format!(
                        "Google Calendar answers {status}"
                    )));
                }
            }
        }
    }

    /// Every page of `calendar`'s events asked by `query`: the events
    /// (cancelled ones without their times are only ids), and the token
    /// to sync from.
    fn pages(
        &mut self,
        calendar: &CalendarId,
        query: &[(&str, &str)],
    ) -> Result<(Vec<Item>, Option<SyncToken>)> {
        let path = format!("/calendars/{}/events", segment(calendar.as_str()));
        let mut items = Vec::new();
        let mut next: Option<String> = None;
        loop {
            let mut asked: Vec<(&str, &str)> = query.to_vec();
            if let Some(page) = &next {
                asked.push(("pageToken", page));
            }
            let page: EventsPage = self.get(&path, &asked)?;
            let defaults = if page.default_reminders.is_empty() {
                self.defaults.get(calendar).cloned().unwrap_or_default()
            } else {
                popups(&page.default_reminders)
            };
            items.extend(
                page.items
                    .into_iter()
                    .map(|e| Item::of(e, calendar, &defaults)),
            );
            match page.next_page_token {
                Some(page) => next = Some(page),
                None => return Ok((items, page.next_sync_token.map(SyncToken))),
            }
        }
    }
}

impl CalendarSource for GoogleCalendar {
    fn calendars(&mut self) -> Result<Vec<Calendar>> {
        let mut calendars = Vec::new();
        let mut next: Option<String> = None;
        loop {
            let mut query = vec![("maxResults", "250")];
            if self.show_hidden {
                query.push(("showHidden", "true"));
            }
            if let Some(page) = &next {
                query.push(("pageToken", page));
            }
            let page: CalendarPage = self.get("/users/me/calendarList", &query)?;
            if next.is_none() {
                self.hidden.clear();
                self.access.clear();
            }
            for entry in page.items.into_iter().filter(|c| !c.deleted) {
                let id = CalendarId::new(entry.id);
                if entry.hidden {
                    self.hidden.insert(id.clone());
                }
                let role = entry.access_role.clone().unwrap_or_default();
                // Someone else's: its owner as Google says, else its id
                // when it is an address (a person's main calendar).
                let shared_by = (!entry.primary && role != "owner")
                    .then(|| {
                        entry
                            .data_owner
                            .clone()
                            .filter(|o| !o.is_empty())
                            .or_else(|| {
                                (id.0.contains('@') && !id.0.ends_with("calendar.google.com"))
                                    .then(|| id.0.clone())
                            })
                    })
                    .flatten();
                self.access.insert(
                    id.clone(),
                    Access {
                        role,
                        primary: entry.primary,
                        shared_by,
                    },
                );
                self.defaults
                    .insert(id.clone(), popups(&entry.default_reminders));
                calendars.push(Calendar {
                    account: self.account.clone(),
                    name: entry
                        .summary_override
                        .filter(|s| !s.is_empty())
                        .or(entry.summary)
                        .unwrap_or_else(|| id.0.clone()),
                    colour: entry
                        .background_color
                        .and_then(|c| c.parse().ok())
                        .unwrap_or(Colour::DEFAULT),
                    read_only: !matches!(entry.access_role.as_deref(), Some("owner" | "writer")),
                    shown: entry.selected,
                    featured: true,
                    id,
                });
            }
            match page.next_page_token {
                Some(page) => next = Some(page),
                None => return Ok(calendars),
            }
        }
    }

    fn events(&mut self, calendar: &CalendarId, range: Range) -> Result<Listing> {
        let (from, to) = (range.from.to_string(), range.to.to_string());
        let (items, token) = self.pages(
            calendar,
            &[
                ("singleEvents", "true"),
                ("maxResults", "2500"),
                ("timeMin", &from),
                ("timeMax", &to),
            ],
        )?;
        let events = items
            .into_iter()
            .filter_map(|item| match item {
                Item::Event(event) => Some(*event),
                Item::Removed(_) => None,
            })
            .collect();
        Ok(Listing { events, token })
    }

    fn changes(&mut self, calendar: &CalendarId, token: &SyncToken) -> Result<Changes> {
        let (items, token) = self.pages(
            calendar,
            &[
                ("singleEvents", "true"),
                ("maxResults", "2500"),
                ("syncToken", &token.0),
            ],
        )?;
        let mut changes = Changes {
            token,
            ..Changes::default()
        };
        for item in items {
            match item {
                Item::Event(event) => changes.changed.push(*event),
                Item::Removed(id) => changes.removed.push(id),
            }
        }
        Ok(changes)
    }
}

/// An event as listed: one with its times, or one deleted (Google gives a
/// deleted event as `cancelled`, often with nothing but its id). A
/// working location ("Home", "Office", each day) is not an event of the
/// day: left out as deleted ones are.
enum Item {
    Event(Box<Event>),
    Removed(EventId),
}

impl Item {
    fn of(event: GEvent, calendar: &CalendarId, defaults: &[u32]) -> Self {
        let id = EventId::new(event.id.clone());
        if event.event_type.as_deref() == Some("workingLocation") {
            return Self::Removed(id);
        }
        match event.when() {
            Some(when) => Self::Event(Box::new(event.into_event(calendar, when, defaults))),
            None => Self::Removed(id),
        }
    }
}

impl GEvent {
    fn when(&self) -> Option<When> {
        let (start, end) = (self.start.as_ref()?, self.end.as_ref());
        if let Some(start) = start.date_time.as_deref() {
            let start: Timestamp = start.parse().ok()?;
            let end = end
                .and_then(|e| e.date_time.as_deref())
                .and_then(|e| e.parse::<Timestamp>().ok())
                .filter(|e| *e >= start)
                .unwrap_or(start);
            return Some(When::Timed { start, end });
        }
        let first: Date = start.date.as_deref()?.parse().ok()?;
        // Google's end date is the day after the last.
        let last = end
            .and_then(|e| e.date.as_deref())
            .and_then(|e| e.parse::<Date>().ok())
            .and_then(|e| e.yesterday().ok())
            .filter(|last| *last >= first)
            .unwrap_or(first);
        Some(When::AllDay { first, last })
    }

    fn into_event(self, calendar: &CalendarId, when: When, defaults: &[u32]) -> Event {
        let status = match self.status.as_deref() {
            Some("tentative") => Status::Tentative,
            Some("cancelled") => Status::Cancelled,
            _ => Status::Confirmed,
        };
        // Not an invitation (no one invited, or the user not among them):
        // unanswered, as the model says.
        let answer = match self
            .attendees
            .iter()
            .find(|a| a.me)
            .and_then(|a| a.response_status.as_deref())
        {
            Some("accepted") => Answer::Accepted,
            Some("declined") => Answer::Declined,
            _ => Answer::Unanswered,
        };
        let free_busy = match self.transparency.as_deref() {
            Some("transparent") => FreeBusy::Free,
            _ => FreeBusy::Busy,
        };
        let place = self.location.filter(|l| !l.trim().is_empty());
        let description = self.description.filter(|d| !d.trim().is_empty());
        let video = self
            .hangout_link
            .filter(|l| !l.is_empty())
            .or_else(|| {
                self.conference_data.and_then(|c| {
                    c.entry_points
                        .into_iter()
                        .find(|e| e.entry_point_type == "video" && !e.uri.is_empty())
                        .map(|e| e.uri)
                })
            })
            .or_else(|| meeting_address(place.as_deref(), description.as_deref()));
        let alarms = match self.reminders {
            Some(Reminders {
                use_default: false,
                overrides,
            }) => popups(&overrides.unwrap_or_default()),
            _ => defaults.to_vec(),
        };
        Event {
            id: EventId::new(self.id),
            calendar: calendar.clone(),
            title: self.summary.unwrap_or_default(),
            when,
            status,
            answer,
            free_busy,
            place,
            video,
            page: self.html_link.filter(|l| !l.is_empty()),
            description,
            alarms,
        }
    }
}

/// The reminders that show on the screen (`popup`; e-mails are Google's to
/// send), in minutes before the start, earliest first.
fn popups(reminders: &[Reminder]) -> Vec<u32> {
    let mut minutes: Vec<u32> = reminders
        .iter()
        .filter(|r| r.method == "popup")
        .filter_map(|r| u32::try_from(r.minutes).ok())
        .collect();
    minutes.sort_unstable_by(|a, b| b.cmp(a));
    minutes.dedup();
    minutes
}

/// `text` as one segment of a URL's path (a calendar's id holds `@` and
/// `#`).
fn segment(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

// --- The API's JSON (only what is read) ------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalendarPage {
    #[serde(default)]
    items: Vec<CalendarEntry>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalendarEntry {
    id: String,
    summary: Option<String>,
    summary_override: Option<String>,
    background_color: Option<String>,
    #[serde(default)]
    selected: bool,
    access_role: Option<String>,
    #[serde(default)]
    default_reminders: Vec<Reminder>,
    #[serde(default)]
    deleted: bool,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    primary: bool,
    /// Who owns a secondary calendar.
    data_owner: Option<String>,
}

/// `POST /freeBusy`'s answer.
#[derive(Deserialize)]
struct FreeBusyAnswer {
    #[serde(default)]
    calendars: HashMap<String, FreeBusyCalendar>,
}

#[derive(Deserialize)]
struct FreeBusyCalendar {
    #[serde(default)]
    busy: Vec<BusyBlock>,
    #[serde(default)]
    errors: Vec<FreeBusyError>,
}

#[derive(Deserialize)]
struct BusyBlock {
    start: String,
    end: String,
}

#[derive(Deserialize)]
struct FreeBusyError {
    #[serde(default)]
    reason: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventsPage {
    #[serde(default)]
    items: Vec<GEvent>,
    next_page_token: Option<String>,
    next_sync_token: Option<String>,
    #[serde(default)]
    default_reminders: Vec<Reminder>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GEvent {
    id: String,
    status: Option<String>,
    event_type: Option<String>,
    summary: Option<String>,
    description: Option<String>,
    location: Option<String>,
    html_link: Option<String>,
    hangout_link: Option<String>,
    start: Option<GTime>,
    end: Option<GTime>,
    transparency: Option<String>,
    #[serde(default)]
    attendees: Vec<Attendee>,
    reminders: Option<Reminders>,
    conference_data: Option<ConferenceData>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GTime {
    date: Option<String>,
    date_time: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Attendee {
    #[serde(default, rename = "self")]
    me: bool,
    response_status: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reminders {
    #[serde(default)]
    use_default: bool,
    overrides: Option<Vec<Reminder>>,
}

#[derive(Deserialize)]
struct Reminder {
    #[serde(default)]
    method: String,
    #[serde(default)]
    minutes: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConferenceData {
    #[serde(default)]
    entry_points: Vec<EntryPoint>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntryPoint {
    #[serde(default)]
    entry_point_type: String,
    #[serde(default)]
    uri: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_address_is_taken_only_on_this_machine() {
        assert_eq!(api(Some("http://127.0.0.1:4321/")), "http://127.0.0.1:4321");
        assert_eq!(api(Some("https://calendar.example.org")), API);
        assert_eq!(api(Some("http://127.0.0.1.example.org:80")), API);
        assert_eq!(api(None), API);
    }

    #[test]
    fn a_calendars_id_is_one_segment_of_the_path() {
        assert_eq!(
            segment("en.french#holiday@group.v.calendar.google.com"),
            "en.french%23holiday%40group.v.calendar.google.com"
        );
    }

    #[test]
    fn an_all_day_events_end_is_the_day_after_its_last() {
        let event: GEvent = serde_json::from_str(
            r#"{"id": "a", "start": {"date": "2026-11-05"}, "end": {"date": "2026-11-07"}}"#,
        )
        .expect("an event");
        assert_eq!(
            event.when(),
            Some(When::AllDay {
                first: jiff::civil::date(2026, 11, 5),
                last: jiff::civil::date(2026, 11, 6),
            })
        );
    }
}
