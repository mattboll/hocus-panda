//! What a source gives: items, each with its notebook and a pointer to
//! where it lives at its source (which only the user opens).

use std::fmt;

use jiff::{Timestamp, civil::Date};
use serde::{Deserialize, Serialize};

/// Pro or perso (§7.4): set by the account, as the user chose at consent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Notebook {
    Pro,
    Perso,
}

/// What of an account is read; each is consented on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// Its calendars, hidden ones included.
    Calendar,
    /// Its contacts' birthdays and anniversaries, nothing else of them.
    Birthdays,
    /// Its mail (Gmail): inbox and sent, 30 days back, read-only.
    Mail,
}

impl SourceKind {
    pub const ALL: [Self; 3] = [Self::Calendar, Self::Birthdays, Self::Mail];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Calendar => "calendar",
            Self::Birthdays => "birthdays",
            Self::Mail => "mail",
        }
    }
}

/// One source: an account (Online Accounts' stable id for it, e.g.
/// `account_1700000000_0`) and what of it is read.
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
}

impl fmt::Display for SourceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.account, self.kind.as_str())
    }
}

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
