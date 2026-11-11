//! What the day is made of: calendars and their events, a board's columns
//! and its tasks. Owned values, as a source gives them and as the bar
//! reads them; nothing here talks to a source.

use std::{cmp::Ordering, fmt, str::FromStr};

use jiff::{Timestamp, Zoned, civil::Date, tz::TimeZone};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

macro_rules! id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(id: impl Into<String>) -> Self {
                Self(id.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id!(
    /// An account of the configuration (`[[day.calendars]]`,
    /// `[[day.tasks]]`, or the local list): its cache's and its secret's
    /// name, so made of letters, digits, `@`, `.`, `_` and `-`.
    AccountId
);
id!(
    /// A calendar, as its account names it.
    CalendarId
);
id!(
    /// An event, as its calendar names it.
    EventId
);
id!(
    /// A board, as its account names it.
    BoardId
);
id!(
    /// A board's column.
    ColumnId
);
id!(
    /// A task: a card of a board's column.
    TaskId
);

/// A colour, `#RRGGBB` (or `#RGB`) in the configuration and on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Colour(pub [u8; 3]);

impl Colour {
    /// The calendars' blue (Day.1's first), for a calendar with none.
    pub const DEFAULT: Self = Self([0x8F, 0xB3, 0xFF]);
}

/// A text that is not a colour.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?}: a colour is #RRGGBB or #RGB")]
pub struct BadColour(pub String);

impl FromStr for Colour {
    type Err = BadColour;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bad = || BadColour(text.to_owned());
        let hex = text.trim().strip_prefix('#').ok_or_else(bad)?;
        let digit = |c: u8| char::from(c).to_digit(16).ok_or_else(bad);
        let bytes = hex.as_bytes();
        match bytes.len() {
            3 => {
                let mut rgb = [0; 3];
                for (out, c) in rgb.iter_mut().zip(bytes) {
                    *out = (digit(*c)? * 17) as u8;
                }
                Ok(Self(rgb))
            }
            6 => {
                let mut rgb = [0; 3];
                for (out, [high, low]) in rgb.iter_mut().zip(bytes.as_chunks::<2>().0) {
                    *out = (digit(*high)? * 16 + digit(*low)?) as u8;
                }
                Ok(Self(rgb))
            }
            _ => Err(bad()),
        }
    }
}

impl fmt::Display for Colour {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b] = self.0;
        write!(f, "#{r:02X}{g:02X}{b:02X}")
    }
}

impl Serialize for Colour {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Colour {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// A calendar of an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Calendar {
    pub id: CalendarId,
    pub account: AccountId,
    pub name: String,
    pub colour: Colour,
    /// The user cannot change its events (a subscription, a colleague's).
    pub read_only: bool,
    /// Its events are in the bar: as the configuration's `shown` says,
    /// else as the account marks it (Google's `selected`), else shown.
    pub shown: bool,
    /// Its events feed what is about to start, if shown: the meeting card
    /// (Day.2), the reminders (Day.6), the halo's meeting states and
    /// `Super+J` ([`crate::Shown::featured`]). As the configuration's
    /// `featured` says, else featured; a source gives `true`.
    #[serde(default = "featured")]
    pub featured: bool,
}

/// A calendar read from a cache written before `featured` was.
fn featured() -> bool {
    true
}

/// When an event is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum When {
    /// From `start` to `end`, instants: shown in the system's time zone,
    /// whatever zone the event was made in.
    Timed { start: Timestamp, end: Timestamp },
    /// Whole days, from `first` to `last` included, wherever the user is.
    AllDay { first: Date, last: Date },
}

impl When {
    /// Its start, as an instant in `zone` (an all-day event's: its first
    /// day's midnight there).
    pub fn start(&self, zone: &TimeZone) -> Timestamp {
        match *self {
            Self::Timed { start, .. } => start,
            Self::AllDay { first, .. } => midnight(first, zone),
        }
    }

    /// Its end, as an instant in `zone` (an all-day event's: the midnight
    /// after its last day).
    pub fn end(&self, zone: &TimeZone) -> Timestamp {
        match *self {
            Self::Timed { end, .. } => end,
            Self::AllDay { last, .. } => midnight(last.tomorrow().unwrap_or(last), zone),
        }
    }

    pub fn all_day(&self) -> bool {
        matches!(self, Self::AllDay { .. })
    }
}

/// The first instant of `date` in `zone` (its midnight, or the first time
/// after a gap).
pub fn midnight(date: Date, zone: &TimeZone) -> Timestamp {
    date.to_zoned(zone.clone())
        .map(|z| z.timestamp())
        .unwrap_or(Timestamp::UNIX_EPOCH)
}

/// Whether an event takes place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Status {
    #[default]
    Confirmed,
    Tentative,
    Cancelled,
}

/// The user's answer to an invitation; `Unanswered` for an event that is
/// not one, or not answered (a "maybe" counts as unanswered).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Answer {
    Accepted,
    Declined,
    #[default]
    Unanswered,
}

/// Whether an event takes the user's time ("busy"), or shows them free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum FreeBusy {
    #[default]
    Busy,
    Free,
}

/// An event of a calendar, recurring ones expanded into their occurrences
/// by the source (each its own id), their exceptions honoured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: EventId,
    pub calendar: CalendarId,
    pub title: String,
    pub when: When,
    pub status: Status,
    pub answer: Answer,
    pub free_busy: FreeBusy,
    /// Its place, as written ("Salle Jupiter · 3e étage", an address).
    pub place: Option<String>,
    /// Its video call's address: the event's own conference data, else
    /// the first meeting address the source found in its place or
    /// description.
    pub video: Option<String>,
    /// Its own page in its calendar's web application, when it has one.
    pub page: Option<String>,
    pub description: Option<String>,
    /// Its own alarms, in minutes before its start, earliest first.
    pub alarms: Vec<u32>,
}

impl Event {
    /// Whether it takes a slot of the day in the bar (the meeting card,
    /// Today, the reminders): timed, neither cancelled nor declined, and
    /// not marked free.
    pub fn takes_time(&self) -> bool {
        !self.when.all_day()
            && self.status != Status::Cancelled
            && self.answer != Answer::Declined
            && self.free_busy == FreeBusy::Busy
    }

    /// Whether it overlaps `[from, to)` in `zone`.
    pub fn overlaps(&self, from: Timestamp, to: Timestamp, zone: &TimeZone) -> bool {
        self.when.start(zone) < to && self.when.end(zone) > from
    }
}

/// Events in the bar's order: by start; on the same start, all-day ones
/// first, then the shorter, then by title and id, so that the order never
/// depends on how the source listed them.
pub fn event_order(a: &Event, b: &Event, zone: &TimeZone) -> Ordering {
    a.when
        .start(zone)
        .cmp(&b.when.start(zone))
        .then_with(|| b.when.all_day().cmp(&a.when.all_day()))
        .then_with(|| a.when.end(zone).cmp(&b.when.end(zone)))
        .then_with(|| a.title.cmp(&b.title))
        .then_with(|| a.id.cmp(&b.id))
}

/// Sorts `events` in the bar's order ([`event_order`]).
pub fn sort_events(events: &mut [Event], zone: &TimeZone) {
    events.sort_by(|a, b| event_order(a, b, zone));
}

/// The video services whose addresses are taken from a description.
const VIDEO_HOSTS: &[&str] = &[
    "meet.google.com",
    "zoom.us",
    "teams.microsoft.com",
    "teams.live.com",
    "whereby.com",
    "meet.jit.si",
    "webex.com",
    "gotomeeting.com",
    "bigbluebutton.org",
];

/// An event's meeting address when its source gives none of its own: the
/// first `https://` address in its place, else the first of a usual video
/// service in its description (Day.2).
pub fn meeting_address(place: Option<&str>, description: Option<&str>) -> Option<String> {
    let in_place = place.and_then(|p| addresses(p).next());
    in_place.or_else(|| {
        description.and_then(|d| {
            addresses(d).find(|address| {
                let host = address["https://".len()..]
                    .split(['/', '?', '#', ':'])
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                VIDEO_HOSTS
                    .iter()
                    .any(|h| host == *h || host.ends_with(&format!(".{h}")))
            })
        })
    })
}

/// The `https://` addresses in `text`, in its order: up to a space, a
/// quote or a bracket (a description is often HTML), its last
/// punctuation left out.
fn addresses(text: &str) -> impl Iterator<Item = String> + '_ {
    text.match_indices("https://").filter_map(|(at, _)| {
        let rest = &text[at..];
        let end = rest
            .find(|c: char| c.is_whitespace() || "\"'<>()[]{}".contains(c))
            .unwrap_or(rest.len());
        let address = rest[..end].trim_end_matches(['.', ',', ';', ':', '!', '?']);
        (address.len() > "https://".len()).then(|| address.to_owned())
    })
}

/// A board of an account: its columns, in their order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskBoard {
    pub id: BoardId,
    pub name: String,
    pub columns: Vec<Column>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    pub id: ColumnId,
    pub name: String,
}

impl TaskBoard {
    /// The column named `name`, case-insensitively.
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns
            .iter()
            .find(|c| c.name.trim().to_lowercase() == name.trim().to_lowercase())
    }
}

/// A task: a card of the tasks' column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub title: String,
    pub due: Option<Timestamp>,
    /// Its place in its column, as its source numbers it: the column's
    /// order is by this number, smallest first (Planka's own positions;
    /// the user's manual sort).
    pub position: f64,
}

/// Tasks in their column's order: by position, then by id.
pub fn sort_tasks(tasks: &mut [Task]) {
    tasks.sort_by(|a, b| {
        a.position
            .total_cmp(&b.position)
            .then_with(|| a.id.cmp(&b.id))
    });
}

/// Where a card goes: a column, and a position in it, or its top.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub column: ColumnId,
    /// `None`: above every card of the column.
    pub position: Option<f64>,
}

/// Today's bounds in `now`'s zone: its first instant and the next day's.
pub fn today(now: &Zoned) -> (Timestamp, Timestamp) {
    let zone = now.time_zone();
    let date = now.date();
    (
        midnight(date, zone),
        midnight(date.tomorrow().unwrap_or(date), zone),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_meeting_address_from_the_place_else_a_video_service_in_the_description() {
        assert_eq!(
            meeting_address(Some("Salle Jupiter, https://visio.example.org/r/42."), None),
            Some("https://visio.example.org/r/42".to_owned())
        );
        let description = "Ordre du jour : https://docs.example.org/a\n\
                           <a href=\"https://acme.zoom.us/j/123?pwd=x\">Rejoindre</a>";
        assert_eq!(
            meeting_address(Some("Salle Jupiter"), Some(description)),
            Some("https://acme.zoom.us/j/123?pwd=x".to_owned())
        );
        assert_eq!(
            meeting_address(None, Some("Voir https://docs.example.org/a")),
            None
        );
    }

    fn paris() -> TimeZone {
        TimeZone::posix("CET-1CEST,M3.5.0,M10.5.0/3").expect("a POSIX zone")
    }

    fn event(id: &str, title: &str, when: When) -> Event {
        Event {
            id: EventId::new(id),
            calendar: CalendarId::new("work"),
            title: title.to_owned(),
            when,
            status: Status::Confirmed,
            answer: Answer::Unanswered,
            free_busy: FreeBusy::Busy,
            place: None,
            video: None,
            page: None,
            description: None,
            alarms: Vec::new(),
        }
    }

    fn at(text: &str) -> Timestamp {
        text.parse().expect("a timestamp")
    }

    #[test]
    fn events_are_by_start_all_day_ones_first_then_the_shorter() {
        let zone = paris();
        let day: Date = "2026-11-04".parse().expect("a date");
        let mut events = vec![
            event(
                "c",
                "Long",
                When::Timed {
                    start: at("2026-11-04T13:00:00Z"),
                    end: at("2026-11-04T15:00:00Z"),
                },
            ),
            event(
                "b",
                "Point produit",
                When::Timed {
                    start: at("2026-11-04T13:00:00Z"),
                    end: at("2026-11-04T13:30:00Z"),
                },
            ),
            event(
                "a",
                "Point quotidien",
                When::Timed {
                    start: at("2026-11-04T08:30:00Z"),
                    end: at("2026-11-04T08:45:00Z"),
                },
            ),
            event(
                "d",
                "Congés",
                When::AllDay {
                    first: day,
                    last: day,
                },
            ),
            event(
                "e",
                "Après minuit",
                When::Timed {
                    start: at("2026-11-03T23:30:00Z"),
                    end: at("2026-11-03T23:45:00Z"),
                },
            ),
        ];
        sort_events(&mut events, &zone);
        let titles: Vec<&str> = events.iter().map(|e| e.title.as_str()).collect();
        // 23:30 UTC is 00:30 in Paris, on the 4th: after its midnight,
        // where the all-day event starts.
        assert_eq!(
            titles,
            [
                "Congés",
                "Après minuit",
                "Point quotidien",
                "Point produit",
                "Long"
            ]
        );
    }

    #[test]
    fn tasks_are_in_their_columns_order() {
        let task = |id: &str, position: f64| Task {
            id: TaskId::new(id),
            title: id.to_owned(),
            due: None,
            position,
        };
        let mut tasks = vec![
            task("c", 131_070.0),
            task("a", 65_535.0),
            task("d", 98_302.5),
            task("b", 65_535.0),
            task("e", -10.0),
        ];
        sort_tasks(&mut tasks);
        let ids: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["e", "a", "b", "d", "c"]);
    }

    #[test]
    fn what_takes_time_in_the_bar() {
        let timed = When::Timed {
            start: at("2026-11-04T13:00:00Z"),
            end: at("2026-11-04T13:30:00Z"),
        };
        let mut e = event("a", "Point", timed);
        assert!(e.takes_time());
        e.answer = Answer::Declined;
        assert!(!e.takes_time());
        e.answer = Answer::Accepted;
        e.free_busy = FreeBusy::Free;
        assert!(!e.takes_time());
        e.free_busy = FreeBusy::Busy;
        e.status = Status::Cancelled;
        assert!(!e.takes_time());
        let day: Date = "2026-11-04".parse().expect("a date");
        let all_day = event(
            "b",
            "Congés",
            When::AllDay {
                first: day,
                last: day,
            },
        );
        assert!(!all_day.takes_time());
    }

    #[test]
    fn colours_read_and_write_as_hex() {
        assert_eq!("#8FB3FF".parse(), Ok(Colour([0x8F, 0xB3, 0xFF])));
        assert_eq!("#fff".parse(), Ok(Colour([255, 255, 255])));
        assert_eq!(Colour([0x6C, 0xC2, 0x87]).to_string(), "#6CC287");
        assert!("8FB3FF".parse::<Colour>().is_err());
        assert!("#8FB3F".parse::<Colour>().is_err());
        assert!("#GGGGGG".parse::<Colour>().is_err());
    }
}
