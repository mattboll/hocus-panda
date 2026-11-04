//! What the workers send and the bar reads: each account's [`Snapshot`],
//! owned, and the [`Day`] that holds the latest of each.

use std::{collections::BTreeMap, fmt::Write as _};

use jiff::{Timestamp, Zoned, civil::Date, tz::TimeZone};

use crate::{
    model::{AccountId, Calendar, Event, Task, TaskBoard, TaskId, today},
    source::SourceError,
};

/// What kind of source an account is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// The local task list, `$XDG_STATE_HOME/hocuspanda/tasks.json`.
    Local,
    /// A file of the configuration's, for tests and screenshots.
    Fixture,
    /// Google Calendar, through GNOME Online Accounts.
    Google,
    /// A Planka board.
    Planka,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Fixture => "fixture",
            Self::Google => "google",
            Self::Planka => "planka",
        }
    }
}

/// What an account gives the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Holds {
    Calendars,
    Tasks,
}

/// An account, as the configuration sets it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: AccountId,
    pub kind: Kind,
    pub holds: Holds,
    /// What names it for the user: its address, its e-mail, its file.
    pub name: String,
}

/// How fresh an account's data is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Nothing read yet, nothing cached.
    Loading,
    /// The cache's, from `fetched`, while the source is asked again.
    Cached { fetched: Timestamp },
    /// Read from the source at `fetched` (wall clock, for the user to
    /// read: "up to date · 2 min ago").
    Fresh { fetched: Timestamp },
    /// The source did not give it; what was read last (at `fetched`, if
    /// ever) is kept.
    Failed {
        error: SourceError,
        fetched: Option<Timestamp>,
    },
}

/// How a tasks' account's server lets its users sign in, as it says
/// (Planka's `oidc`), for Day.1's form: the user name and password fields
/// show unless SSO is enforced, and the pasted token always.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Login {
    /// Its own sign-in only.
    Password,
    /// Its own, or SSO.
    PasswordOrSso,
    /// SSO only: a desktop program cannot complete it, a token is pasted.
    SsoOnly,
}

/// Where an account is with a month outside its window (Day.5b), asked
/// for as the calendar turns to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MonthState {
    /// Asked, not answered yet.
    Loading,
    /// Its events are in the snapshot's.
    Loaded,
    /// The source did not give it.
    Failed(SourceError),
}

/// A change the bar asks of a tasks' account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A new task at the top of the add column.
    Add { title: String },
    /// A task ticked: to the top of the done column.
    Tick { task: TaskId },
    /// A task ticked this session unticked: back where it was (else to
    /// the top of the tasks' column).
    Untick { task: TaskId },
}

/// A change's number, as [`crate::Workers::change`] gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChangeId(pub u64);

/// A change not carried out yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub id: ChangeId,
    pub change: Change,
    /// Why the source refused it last: it is tried again when the source
    /// answers again. `None` while it is being sent.
    pub refused: Option<SourceError>,
}

/// An account's data, as its worker last made it.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub account: Account,
    pub state: State,
    /// A calendars' account's calendars, shown or not.
    pub calendars: Vec<Calendar>,
    /// Their events in the window (2 months back, 12 ahead) and in the
    /// months read outside it ([`Self::months`]), those of hidden
    /// calendars left out, in the bar's order
    /// ([`crate::model::event_order`]).
    pub events: Vec<Event>,
    /// The months outside the window it was asked for, by their first
    /// day: read, being read, or not given.
    pub months: BTreeMap<Date, MonthState>,
    /// A tasks' account's board.
    pub board: Option<TaskBoard>,
    /// The tasks' column's cards, in its order.
    pub tasks: Vec<Task>,
    /// Changes queued, in their order.
    pub pending: Vec<Pending>,
    /// How its server signs in, once it said (read once a worker runs).
    pub login: Option<Login>,
}

impl Snapshot {
    /// Nothing yet.
    pub fn loading(account: Account) -> Self {
        Self {
            account,
            state: State::Loading,
            calendars: Vec::new(),
            events: Vec::new(),
            months: BTreeMap::new(),
            board: None,
            tasks: Vec::new(),
            pending: Vec::new(),
            login: None,
        }
    }

    /// The account cannot be read at all.
    pub fn failed(account: Account, error: SourceError) -> Self {
        Self {
            state: State::Failed {
                error,
                fetched: None,
            },
            ..Self::loading(account)
        }
    }
}

/// What the workers send the day.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// An account's latest.
    Snapshot(Box<Snapshot>),
    /// An account no more (removed from Online Accounts, its calendar
    /// switched off): nothing of it comes after.
    Gone(AccountId),
}

/// An event shown, with its calendar (its colour, its name).
#[derive(Debug, Clone, Copy)]
pub struct Shown<'a> {
    pub event: &'a Event,
    pub calendar: &'a Calendar,
}

/// The day: every account's latest snapshot, in the configuration's order,
/// the local list last.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Day {
    accounts: Vec<Snapshot>,
}

impl Day {
    /// The accounts, each loading (or failed), before their workers say.
    pub fn new(accounts: Vec<Snapshot>) -> Self {
        Self { accounts }
    }

    pub fn accounts(&self) -> &[Snapshot] {
        &self.accounts
    }

    /// `snapshot` replaces its account's: whether anything changed.
    pub fn update(&mut self, snapshot: Snapshot) -> bool {
        match self
            .accounts
            .iter_mut()
            .find(|s| s.account.id == snapshot.account.id)
        {
            Some(kept) if *kept == snapshot => false,
            Some(kept) => {
                *kept = snapshot;
                true
            }
            None => {
                // An account that comes later (Online Accounts'): before
                // the local list, which stays last.
                let at = self
                    .accounts
                    .iter()
                    .position(|s| s.account.kind == Kind::Local)
                    .unwrap_or(self.accounts.len());
                self.accounts.insert(at, snapshot);
                true
            }
        }
    }

    /// `account` and its data left out: whether it was there.
    pub fn remove(&mut self, account: &AccountId) -> bool {
        let before = self.accounts.len();
        self.accounts.retain(|s| s.account.id != *account);
        self.accounts.len() != before
    }

    /// What a worker sent applied: whether anything changed.
    pub fn apply(&mut self, update: Update) -> bool {
        match update {
            Update::Snapshot(snapshot) => self.update(*snapshot),
            Update::Gone(account) => self.remove(&account),
        }
    }

    /// The events of shown calendars overlapping `[from, to)`, every
    /// account's, in the bar's order.
    pub fn events(&self, from: Timestamp, to: Timestamp, zone: &TimeZone) -> Vec<Shown<'_>> {
        let mut shown: Vec<Shown<'_>> = self
            .accounts
            .iter()
            .flat_map(|account| {
                account.events.iter().filter_map(|event| {
                    let calendar = account.calendars.iter().find(|c| c.id == event.calendar)?;
                    (calendar.shown && event.overlaps(from, to, zone))
                        .then_some(Shown { event, calendar })
                })
            })
            .collect();
        shown.sort_by(|a, b| crate::model::event_order(a.event, b.event, zone));
        shown
    }

    /// Where the calendars' accounts are with `month` (its first day), a
    /// month outside their window: loading while one of them has not
    /// answered (or not been asked yet), else failed if one did not give
    /// it, else loaded. An account that never read anything (its source
    /// cannot be read at all) counts as failed. `None` without a
    /// calendars' account.
    pub fn month(&self, month: Date) -> Option<MonthState> {
        let mut calendars = self
            .accounts
            .iter()
            .filter(|s| s.account.holds == Holds::Calendars)
            .peekable();
        calendars.peek()?;
        let mut failed = None;
        for account in calendars {
            match (account.months.get(&month), &account.state) {
                (Some(MonthState::Loaded), _) => {}
                (Some(MonthState::Failed(error)), _)
                | (
                    None,
                    State::Failed {
                        error,
                        fetched: None,
                    },
                ) => {
                    failed.get_or_insert_with(|| error.clone());
                }
                (Some(MonthState::Loading) | None, _) => return Some(MonthState::Loading),
            }
        }
        Some(failed.map_or(MonthState::Loaded, MonthState::Failed))
    }

    /// The bar's tasks' account: the first board set, else the local list
    /// (Day.1). `None` only before the local list is started.
    pub fn tasks(&self) -> Option<&Snapshot> {
        let tasks = || {
            self.accounts
                .iter()
                .filter(|s| s.account.holds == Holds::Tasks)
        };
        tasks()
            .find(|s| s.account.kind != Kind::Local)
            .or_else(|| tasks().next())
    }

    /// What the day holds, on one line, for the log and tests: each
    /// account's state, today's events in `now`'s zone (start, title,
    /// calendar), and the tasks with their ids and the changes pending.
    pub fn summary(&self, now: &Zoned) -> String {
        let zone = now.time_zone();
        let mut line = String::from("accounts=[");
        for (i, s) in self.accounts.iter().enumerate() {
            let state = match &s.state {
                State::Loading => "loading".to_owned(),
                State::Cached { .. } => "cached".to_owned(),
                State::Fresh { .. } => "fresh".to_owned(),
                State::Failed { error, .. } => format!("failed ({error})"),
            };
            let _ = write!(
                line,
                "{}{} {state}",
                if i > 0 { "; " } else { "" },
                s.account.id
            );
        }
        let (from, to) = today(now);
        let events = self.events(from, to, zone);
        let _ = write!(line, "] today={}=[", events.len());
        for (i, shown) in events.iter().enumerate() {
            let start = match shown.event.when {
                crate::model::When::Timed { start, .. } => {
                    start.to_zoned(zone.clone()).strftime("%H:%M").to_string()
                }
                crate::model::When::AllDay { .. } => "all-day".to_owned(),
            };
            let _ = write!(
                line,
                "{}{start} «{}» ({})",
                if i > 0 { "; " } else { "" },
                shown.event.title,
                shown.calendar.name
            );
        }
        line.push(']');
        if let Some(tasks) = self.tasks() {
            let _ = write!(line, " tasks={}=[", tasks.tasks.len());
            for (i, task) in tasks.tasks.iter().enumerate() {
                let _ = write!(
                    line,
                    "{}{} «{}»",
                    if i > 0 { "; " } else { "" },
                    task.id,
                    task.title
                );
            }
            let _ = write!(line, "] pending={}", tasks.pending.len());
            for pending in &tasks.pending {
                if let Some(why) = &pending.refused {
                    let _ = write!(line, " refused={} ({why})", pending.id.0);
                }
            }
        }
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Answer, CalendarId, Colour, EventId, FreeBusy, Status, When};

    fn account(id: &str, kind: Kind, holds: Holds) -> Account {
        Account {
            id: AccountId::new(id),
            kind,
            holds,
            name: id.to_owned(),
        }
    }

    #[test]
    fn the_bars_tasks_are_a_boards_else_the_local_lists() {
        let local = Snapshot::loading(account("local", Kind::Local, Holds::Tasks));
        let board = Snapshot::loading(account("planka-1", Kind::Planka, Holds::Tasks));
        let calendars = Snapshot::loading(account("fixture-1", Kind::Fixture, Holds::Calendars));
        let day = Day::new(vec![calendars.clone(), local.clone()]);
        assert_eq!(day.tasks().map(|s| s.account.id.as_str()), Some("local"));
        let day = Day::new(vec![calendars, board, local]);
        assert_eq!(day.tasks().map(|s| s.account.id.as_str()), Some("planka-1"));
    }

    #[test]
    fn an_account_coming_later_goes_before_the_local_list_and_can_go() {
        let local = Snapshot::loading(account("local", Kind::Local, Holds::Tasks));
        let google = Snapshot::loading(account("google-1", Kind::Google, Holds::Calendars));
        let mut day = Day::new(vec![local]);
        assert!(day.apply(Update::Snapshot(Box::new(google))));
        let ids: Vec<&str> = day
            .accounts()
            .iter()
            .map(|s| s.account.id.as_str())
            .collect();
        assert_eq!(ids, ["google-1", "local"]);
        assert!(day.apply(Update::Gone(AccountId::new("google-1"))));
        assert!(!day.apply(Update::Gone(AccountId::new("google-1"))));
        assert_eq!(day.accounts().len(), 1);
    }

    #[test]
    fn a_snapshot_changes_the_day_only_when_it_differs() {
        let local = Snapshot::loading(account("local", Kind::Local, Holds::Tasks));
        let mut day = Day::new(vec![local.clone()]);
        assert!(!day.update(local.clone()));
        let fresh = Snapshot {
            state: State::Fresh {
                fetched: Timestamp::UNIX_EPOCH,
            },
            ..local
        };
        assert!(day.update(fresh.clone()));
        assert!(!day.update(fresh));
    }

    #[test]
    fn a_month_outside_the_window_loads_until_every_calendars_account_answers() {
        let month = jiff::civil::date(2028, 1, 1);
        let calendars = |id: &str, state: Option<MonthState>| Snapshot {
            state: State::Fresh {
                fetched: Timestamp::UNIX_EPOCH,
            },
            months: state.into_iter().map(|s| (month, s)).collect(),
            ..Snapshot::loading(account(id, Kind::Google, Holds::Calendars))
        };
        let local = Snapshot::loading(account("local", Kind::Local, Holds::Tasks));
        assert_eq!(Day::new(vec![local.clone()]).month(month), None);
        let day = |accounts: Vec<Snapshot>| Day::new(accounts).month(month);
        // Not asked yet, or asked: loading.
        assert_eq!(day(vec![calendars("a", None)]), Some(MonthState::Loading));
        assert_eq!(
            day(vec![
                calendars("a", Some(MonthState::Loaded)),
                calendars("b", Some(MonthState::Loading)),
            ]),
            Some(MonthState::Loading)
        );
        assert_eq!(
            day(vec![
                calendars("a", Some(MonthState::Loaded)),
                local.clone()
            ]),
            Some(MonthState::Loaded)
        );
        let offline = SourceError::Offline("down".to_owned());
        assert_eq!(
            day(vec![
                calendars("a", Some(MonthState::Loaded)),
                calendars("b", Some(MonthState::Failed(offline.clone()))),
            ]),
            Some(MonthState::Failed(offline.clone()))
        );
        // An account that never read anything does not keep it loading.
        let never = Snapshot::failed(
            account("c", Kind::Fixture, Holds::Calendars),
            offline.clone(),
        );
        assert_eq!(
            day(vec![calendars("a", Some(MonthState::Loaded)), never]),
            Some(MonthState::Failed(offline))
        );
    }

    #[test]
    fn hidden_calendars_are_left_out() {
        let zone = TimeZone::UTC;
        let calendar = |id: &str, shown: bool| Calendar {
            id: CalendarId::new(id),
            account: AccountId::new("fixture-1"),
            name: id.to_owned(),
            colour: Colour::DEFAULT,
            read_only: false,
            shown,
        };
        let event = |id: &str, calendar: &str, hour: i8| Event {
            id: EventId::new(id),
            calendar: CalendarId::new(calendar),
            title: id.to_owned(),
            when: When::Timed {
                start: jiff::civil::date(2026, 11, 4)
                    .at(hour, 0, 0, 0)
                    .to_zoned(zone.clone())
                    .expect("a time")
                    .timestamp(),
                end: jiff::civil::date(2026, 11, 4)
                    .at(hour + 1, 0, 0, 0)
                    .to_zoned(zone.clone())
                    .expect("a time")
                    .timestamp(),
            },
            status: Status::Confirmed,
            answer: Answer::Accepted,
            free_busy: FreeBusy::Busy,
            place: None,
            video: None,
            page: None,
            description: None,
            alarms: Vec::new(),
        };
        let snapshot = Snapshot {
            calendars: vec![calendar("work", true), calendar("home", false)],
            events: vec![event("swim", "home", 7), event("standup", "work", 9)],
            ..Snapshot::loading(account("fixture-1", Kind::Fixture, Holds::Calendars))
        };
        let day = Day::new(vec![snapshot]);
        let now = jiff::civil::date(2026, 11, 4)
            .at(8, 0, 0, 0)
            .to_zoned(zone.clone())
            .expect("a time");
        let (from, to) = today(&now);
        let shown: Vec<&str> = day
            .events(from, to, &zone)
            .iter()
            .map(|s| s.event.title.as_str())
            .collect();
        assert_eq!(shown, ["standup"]);
    }
}
