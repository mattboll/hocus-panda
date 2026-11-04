//! The adapters' two traits: a [`CalendarSource`] gives calendars and their
//! events, a [`KanbanSource`] a board's columns and cards. An account's
//! worker calls them on its own thread, one call at a time: they block,
//! and need not be `Sync`. How a source signs in is its own business (a
//! token handed by a provider, a file): neither trait knows of it.

use std::fmt;

use jiff::{Timestamp, Zoned, civil::Date, tz::TimeZone};
use serde::{Deserialize, Serialize};

use crate::{
    model::{
        Calendar, CalendarId, Column, ColumnId, Event, EventId, Place, Task, TaskBoard, TaskId,
    },
    snapshot::Login,
};

/// What went wrong with a source. A worker keeps what it had and shows it
/// with the error; a change refused stays queued and is tried again when
/// the source answers again, unless the card is [`SourceError::Gone`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    /// It does not answer (offline, the server down, a timeout).
    Offline(String),
    /// It needs signing in again: its token is refused or expired, or none
    /// is kept.
    SignIn(String),
    /// It answered no (a conflict, a right missing).
    Refused(String),
    /// What the change is about is no more (a card deleted elsewhere).
    Gone,
    /// The sync token is no longer valid: list everything again.
    Expired,
    /// The account cannot be used as set (a file that does not read, a
    /// column that is not there, a kind not built yet).
    Invalid(String),
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Offline(why) => write!(f, "offline: {why}"),
            Self::SignIn(why) => write!(f, "sign in again: {why}"),
            Self::Refused(why) => write!(f, "refused: {why}"),
            Self::Gone => f.write_str("gone"),
            Self::Expired => f.write_str("sync token expired"),
            Self::Invalid(why) => write!(f, "invalid: {why}"),
        }
    }
}

impl std::error::Error for SourceError {}

pub type Result<T> = std::result::Result<T, SourceError>;

/// A span of time, `[from, to)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub from: Timestamp,
    pub to: Timestamp,
}

impl Range {
    /// The window of events the bar keeps (Day.5): from 2 months back to
    /// 12 months ahead of `now`'s day.
    pub fn window(now: &Zoned) -> Self {
        let zone = now.time_zone();
        let day = now.date();
        let months = |n: i32| day.checked_add(jiff::Span::new().months(n)).unwrap_or(day);
        Self {
            from: crate::model::midnight(months(-2), zone),
            to: crate::model::midnight(months(12), zone),
        }
    }

    /// The month whose first day is `first`'s (any day of it), in `zone`.
    pub fn month(first: Date, zone: &TimeZone) -> Self {
        let first = first.first_of_month();
        let next = first
            .checked_add(jiff::Span::new().months(1))
            .unwrap_or(first);
        Self {
            from: crate::model::midnight(first, zone),
            to: crate::model::midnight(next, zone),
        }
    }

    /// Whether `other` is all within this range.
    pub fn covers(&self, other: &Range) -> bool {
        self.from <= other.from && other.to <= self.to
    }

    /// The months (their first days) that the days `[first, end)` touch
    /// and this range does not hold whole, in order: what the calendar
    /// asks the workers for when it turns to them (Day.5b).
    pub fn months_outside(&self, first: Date, end: Date, zone: &TimeZone) -> Vec<Date> {
        let mut months = Vec::new();
        let mut month = first.first_of_month();
        while month < end {
            if !self.covers(&Self::month(month, zone)) {
                months.push(month);
            }
            match month.checked_add(jiff::Span::new().months(1)) {
                Ok(next) => month = next,
                Err(_) => break,
            }
        }
        months
    }
}

/// An opaque token for a calendar's next incremental sync, kept with the
/// cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SyncToken(pub String);

/// A calendar's events in a range, and the token to sync from, if the
/// source has tokens.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Listing {
    pub events: Vec<Event>,
    pub token: Option<SyncToken>,
}

/// What changed in a calendar since a token.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Changes {
    /// New or changed events (their id says which).
    pub changed: Vec<Event>,
    pub removed: Vec<EventId>,
    pub token: Option<SyncToken>,
}

/// A source of calendars: an account of a calendar service, or a file.
pub trait CalendarSource: Send {
    /// The account's calendars, with what it says of them (its colour,
    /// whether it is selected: [`Calendar::shown`]).
    fn calendars(&mut self) -> Result<Vec<Calendar>>;

    /// `calendar`'s events overlapping `range`, recurring ones expanded.
    fn events(&mut self, calendar: &CalendarId, range: Range) -> Result<Listing>;

    /// What changed in `calendar` since `token`: [`SourceError::Expired`]
    /// when the token no longer is valid, and the worker lists again. A
    /// source whose listings give no token is never asked.
    fn changes(&mut self, calendar: &CalendarId, token: &SyncToken) -> Result<Changes> {
        let _ = (calendar, token);
        Err(SourceError::Expired)
    }
}

/// A source of tasks: a kanban board, whose columns hold cards in an
/// order of their own (the local list is one, with "todo" and "done").
pub trait KanbanSource: Send {
    /// The board, with its columns in their order.
    fn board(&mut self) -> Result<TaskBoard>;

    /// The cards of `column`, in its order.
    fn cards(&mut self, column: &ColumnId) -> Result<Vec<Task>>;

    /// A new card titled `title` at the top of `column`.
    fn create(&mut self, column: &ColumnId, title: &str) -> Result<Task>;

    /// Moves `card` to `place`: the top of a column (a task ticked into
    /// the done column), or back where it was (unticked).
    fn move_card(&mut self, card: &TaskId, place: &Place) -> Result<()>;

    /// How its server lets the user sign in, once it said; asked after
    /// each read of the board, so it may read it then, once. `None`: not
    /// known, or no server (a file, the local list).
    fn login(&mut self) -> Option<Login> {
        None
    }
}

/// The columns a tasks' account uses, found by name in its board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Columns {
    /// Its cards are the bar's tasks.
    pub tasks: Column,
    /// A ticked task goes to its top.
    pub done: Column,
    /// `+` adds at its top.
    pub add: Column,
}

/// The names the configuration gives; empty: the default.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ColumnNames {
    pub tasks: String,
    pub done: String,
    pub add: String,
}

impl ColumnNames {
    /// The columns of `board` these names say (case-insensitively): the
    /// tasks' column is the one named TODO, else the first; the done one
    /// the one named DONE, else the last; adding goes to the tasks'
    /// column. A name given that no column has is an error.
    pub fn find(&self, board: &TaskBoard) -> Result<Columns> {
        let named = |name: &str, default: &str, fallback: Option<&Column>| -> Result<Column> {
            if name.trim().is_empty() {
                board.column(default).or(fallback).cloned().ok_or_else(|| {
                    SourceError::Invalid(format!("board {:?} has no column", board.name))
                })
            } else {
                board.column(name).cloned().ok_or_else(|| {
                    SourceError::Invalid(format!("board {:?} has no column {name:?}", board.name))
                })
            }
        };
        let tasks = named(&self.tasks, "TODO", board.columns.first())?;
        let done = named(&self.done, "DONE", board.columns.last())?;
        let add = if self.add.trim().is_empty() {
            tasks.clone()
        } else {
            named(&self.add, "", None)?
        };
        Ok(Columns { tasks, done, add })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::BoardId;

    fn board(names: &[&str]) -> TaskBoard {
        TaskBoard {
            id: BoardId::new("b"),
            name: "Sprint".to_owned(),
            columns: names
                .iter()
                .map(|n| Column {
                    id: ColumnId::new(n.to_lowercase()),
                    name: (*n).to_owned(),
                })
                .collect(),
        }
    }

    #[test]
    fn the_months_a_range_does_not_hold_whole() {
        let zone = TimeZone::UTC;
        let now = jiff::civil::date(2026, 11, 4)
            .at(9, 47, 0, 0)
            .to_zoned(zone.clone())
            .expect("a time");
        let window = Range::window(&now);
        let date = jiff::civil::date;
        // November 2027's six weeks: from 1 November to 12 December, its
        // first days in the window, December out of it.
        assert_eq!(
            window.months_outside(date(2027, 11, 1), date(2027, 12, 13), &zone),
            [date(2027, 11, 1), date(2027, 12, 1)]
        );
        assert!(
            window
                .months_outside(date(2026, 10, 26), date(2026, 12, 7), &zone)
                .is_empty()
        );
        assert_eq!(
            window.months_outside(date(2026, 8, 31), date(2026, 10, 12), &zone),
            [date(2026, 8, 1), date(2026, 9, 1)]
        );
    }

    #[test]
    fn columns_are_found_by_name_else_first_and_last() {
        let defaults = ColumnNames::default();
        let found = defaults
            .find(&board(&["Backlog", "Todo", "Doing", "Done", "Archive"]))
            .expect("found");
        assert_eq!(
            (
                found.tasks.name.as_str(),
                found.done.name.as_str(),
                found.add.name.as_str()
            ),
            ("Todo", "Done", "Todo")
        );
        let found = defaults
            .find(&board(&["Inbox", "Doing", "Shipped"]))
            .expect("found");
        assert_eq!(
            (found.tasks.name.as_str(), found.done.name.as_str()),
            ("Inbox", "Shipped")
        );
        let named = ColumnNames {
            tasks: "doing".to_owned(),
            done: "SHIPPED".to_owned(),
            add: "inbox".to_owned(),
        };
        let found = named
            .find(&board(&["Inbox", "Doing", "Shipped"]))
            .expect("found");
        assert_eq!(
            (
                found.tasks.name.as_str(),
                found.done.name.as_str(),
                found.add.name.as_str()
            ),
            ("Doing", "Shipped", "Inbox")
        );
        let missing = ColumnNames {
            tasks: "Later".to_owned(),
            ..ColumnNames::default()
        };
        assert!(matches!(
            missing.find(&board(&["Inbox"])),
            Err(SourceError::Invalid(_))
        ));
        assert!(defaults.find(&board(&[])).is_err());
    }
}
