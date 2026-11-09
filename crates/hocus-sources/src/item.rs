//! What a source gives: items, each with its notebook and a pointer to
//! where it lives at its source (which only the user opens).

use jiff::{Timestamp, civil::Date};
use serde::{Deserialize, Serialize};

/// Pro or perso (§7.4): set by the account, as the user chose at consent.
/// One definition for the sources and her memory: the protocol's.
pub use hocus_secretary_link::Notebook;

/// What of an account is read, and an application's notifications; one
/// definition with the consent book's.
pub use hocus_secretary_link::consent::{SourceKey, SourceKind};

/// Where an item lives at its source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pointer {
    pub source: SourceKey,
    /// Its id there: `INBOX/<uidvalidity>/<uid>` for a mail, the calendar's
    /// and the event's ids for an event, the card's href for a contact.
    pub id: String,
    /// How she names it to the user ("le mail de Free, 3 nov."): what a
    /// mask says to look at.
    pub label: String,
    /// What opens it in its app (Gmail's or Calendar's page), for the
    /// user's click only: never fetched.
    pub open: Option<String>,
}

/// An item, as read; its text not yet redacted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub pointer: Pointer,
    pub notebook: Notebook,
    /// When it happened (a mail's date, an event's start).
    pub at: Timestamp,
    pub content: Content,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Content {
    Mail(Mail),
    Event(Event),
    Birthday(Birthday),
    Notification(Notification),
}

/// A mail's headers and, fetched on demand, its text. Never its
/// attachments: their names only.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Mail {
    /// `INBOX` or the sent mailbox.
    pub mailbox: String,
    pub sent: bool,
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    /// The text part, noise not yet removed; `None` until fetched.
    pub body: Option<String>,
    pub attachments: Vec<String>,
    /// The user has read it (its `\Seen` as it was; never changed by her).
    pub seen: bool,
    /// Why it looks sent by a machine (a list, a forge's notifications, a
    /// no-reply address): the header that says so ("List-Unsubscribe",
    /// "X-GitHub-Reason", "no-reply"), never its value (#754).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automated: Option<String>,
}

/// An application's notification, as the shell's server received it
/// (lot 7): what it says, nothing it can do. Its actions are not kept:
/// invoking one (Slack's would mark the channel read) is never hers.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Notification {
    /// The application's name, as it gives it (`Slack`).
    pub app: String,
    /// Its title: for a message, often who wrote it or where.
    pub title: String,
    pub body: String,
}

/// A calendar's event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub calendar: String,
    pub calendar_name: String,
    /// The calendar is hidden in the user's list (still read, §10.1).
    pub hidden_calendar: bool,
    pub title: String,
    pub when: hocus_day::model::When,
    pub busy: bool,
    pub declined: bool,
    pub cancelled: bool,
    pub place: Option<String>,
    pub description: Option<String>,
}

/// A contact's birthday or anniversary: the contact's name and the date,
/// nothing else of the card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Birthday {
    pub name: String,
    pub kind: Anniversary,
    pub month: i8,
    pub day: i8,
    /// When the card gives the year.
    pub year: Option<i16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Anniversary {
    Birthday,
    Anniversary,
}

impl Birthday {
    /// Its next date on or after `from`, and the age reached then when the
    /// year is known. A 29 February is the 28th in other years.
    pub fn next(&self, from: Date) -> Option<(Date, Option<i16>)> {
        let on = |year: i16| {
            Date::new(year, self.month, self.day)
                .or_else(|_| Date::new(year, self.month, self.day - 1))
                .ok()
        };
        let this = on(from.year())?;
        let date = if this >= from {
            this
        } else {
            on(from.year() + 1)?
        };
        Some((date, self.year.map(|born| date.year() - born)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_birthday_comes_back_each_year_with_its_age() {
        let birthday = Birthday {
            name: "Claire".to_owned(),
            kind: Anniversary::Birthday,
            month: 3,
            day: 2,
            year: Some(1990),
        };
        let from = jiff::civil::date(2026, 11, 8);
        assert_eq!(
            birthday.next(from),
            Some((jiff::civil::date(2027, 3, 2), Some(37)))
        );
        let leap = Birthday {
            month: 2,
            day: 29,
            year: None,
            ..birthday
        };
        assert_eq!(
            leap.next(from),
            Some((jiff::civil::date(2027, 2, 28), None))
        );
    }
}
