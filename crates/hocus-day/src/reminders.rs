//! When a meeting's reminder comes (Day.6, issue #430): the event's own
//! alarm, the earliest, else `[calendar] remind` minutes before its start
//! (default 5; `0` turns that default off and keeps the events' own).
//! Only for what takes a slot of the day ([`Event::takes_time`]): timed,
//! neither cancelled nor declined, not marked free.
//!
//! ```toml
//! [calendar]
//! remind = 5
//! ```
//!
//! The shell reads `[calendar]` from the configuration file the compositor
//! read, as it reads `[day]`; the rest of the file is the compositor's.

use std::path::Path;

use serde::Deserialize;

use crate::model::Event;

/// The reminders' minutes before a start when an event has no alarm of its
/// own.
pub const REMIND: u32 = 5;

/// `[calendar]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Minutes before an event's start, for events without an alarm of
    /// their own; `0`: none for them.
    pub remind: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { remind: REMIND }
    }
}

/// `[calendar]` from the configuration file's text.
pub fn from_toml(text: &str) -> Result<Settings, toml::de::Error> {
    #[derive(Deserialize)]
    struct File {
        #[serde(default)]
        calendar: Settings,
    }
    toml::from_str::<File>(text).map(|f| f.calendar)
}

/// `[calendar]` of the configuration file at `path`: the default when
/// there is no file; an error (to log) when it does not read.
pub fn read(path: &Path) -> Result<Settings, crate::config::ReadError> {
    crate::config::ReadError::text_or(path, Settings::default, from_toml)
}

/// How many minutes before its start `event`'s reminder comes, if it has
/// one: its earliest alarm, else the default.
pub fn lead(event: &Event, settings: Settings) -> Option<u32> {
    if !event.takes_time() {
        return None;
    }
    // Its alarms are earliest first: the most minutes before its start.
    event
        .alarms
        .iter()
        .copied()
        .max()
        .or((settings.remind > 0).then_some(settings.remind))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Answer, CalendarId, EventId, FreeBusy, Status, When};

    fn event() -> Event {
        let start: jiff::Timestamp = "2026-11-04T14:00:00Z".parse().expect("a time");
        Event {
            id: EventId::new("e"),
            calendar: CalendarId::new("work"),
            title: "Point produit".to_owned(),
            when: When::Timed {
                start,
                end: start + jiff::SignedDuration::from_mins(30),
            },
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

    #[test]
    fn five_minutes_before_by_default_the_events_own_alarm_first() {
        let defaults = Settings::default();
        assert_eq!(lead(&event(), defaults), Some(5));
        let alarmed = Event {
            alarms: vec![15, 2],
            ..event()
        };
        assert_eq!(lead(&alarmed, defaults), Some(15));
        // `0`: no default, the events' own alarms kept.
        let off = Settings { remind: 0 };
        assert_eq!(lead(&event(), off), None);
        assert_eq!(lead(&alarmed, off), Some(15));
        assert_eq!(lead(&event(), Settings { remind: 10 }), Some(10));
    }

    #[test]
    fn none_for_a_declined_a_cancelled_an_all_day_or_a_free_event() {
        let defaults = Settings::default();
        let declined = Event {
            answer: Answer::Declined,
            ..event()
        };
        let cancelled = Event {
            status: Status::Cancelled,
            alarms: vec![10],
            ..event()
        };
        let all_day = Event {
            when: When::AllDay {
                first: jiff::civil::date(2026, 11, 4),
                last: jiff::civil::date(2026, 11, 4),
            },
            ..event()
        };
        let free = Event {
            free_busy: FreeBusy::Free,
            ..event()
        };
        for event in [declined, cancelled, all_day, free] {
            assert_eq!(lead(&event, defaults), None, "{event:?}");
        }
    }

    #[test]
    fn calendar_remind_reads_from_the_file_the_rest_let_through() {
        assert_eq!(from_toml(""), Ok(Settings::default()));
        assert_eq!(
            from_toml("[calendar]\nremind = 0\n\n[panel]\nform = \"mini\"\n"),
            Ok(Settings { remind: 0 })
        );
        assert!(from_toml("[calendar]\nremind = -1\n").is_err());
        assert!(from_toml("[calendar]\nsoon = 3\n").is_err());
    }
}
