//! A day from a file, for tests and screenshots: calendars, events and a
//! board of tasks, their times relative to the shell's clock (so that
//! `HOCUS_CLOCK_AT` gives the day it says). Read again when the file
//! changes (its worker watches it); what the bar changes in its tasks is
//! kept in memory, never written to it.
//!
//! The file, TOML:
//!
//! ```toml
//! [[calendars]]
//! id = "work"               # default: its name
//! name = "Travail"
//! color = "#8FB3FF"         # default: the calendars' blue
//! read-only = false
//! shown = true              # as the account would mark it
//!
//! [[events]]
//! calendar = "work"         # default: the first calendar
//! title = "Point produit"
//! start = "now+25m"         # see the times below
//! end = "now+55m"           # default: an hour after its start
//! # or a whole day, or several: date = "today", days = 1
//! status = "confirmed"      # "tentative", "cancelled"
//! answer = "accepted"       # "declined", "unanswered" (default)
//! free = false              # true: shown free
//! place = "Salle Jupiter"
//! video = "https://meet.example.org/abc-defg"
//! page = "https://calendar.example.org/event/1"
//! description = "…"
//! alarms = [10]             # minutes before its start
//!
//! [tasks]
//! board = "Sprint"
//! columns = ["TODO", "DONE"]   # the default
//! refuse = false               # true: every change refused, as offline
//!
//! [[tasks.cards]]
//! title = "Relire la spec"
//! column = "TODO"           # default: the first column
//! due = "tomorrow 18:00"
//! ```
//!
//! A time is `now`, `now+1h30m` or `now-15m` (units `d`, `h`, `m`, `s`);
//! a day, `today`, `tomorrow`, `yesterday`, `today+3` or `today-2`, then
//! `HH:MM` (or `HH:MM:SS`) for a time of that day; or a local date and
//! time as `2026-11-04 14:00` (or `2026-11-04T14:00`), or a date. All in
//! the shell's time zone.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use jiff::{SignedDuration, Timestamp, Zoned, civil::Date};
use serde::Deserialize;

use crate::{
    Clock,
    model::{
        AccountId, Answer, BoardId, Calendar, CalendarId, Colour, Column, ColumnId, Event, EventId,
        FreeBusy, Place, Status, Task, TaskBoard, TaskId, When, sort_tasks,
    },
    source::{CalendarSource, KanbanSource, Listing, Range, Result, SourceError},
};

/// A time of the fixture, read against `now`: an instant, or a whole day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum At {
    Instant(Timestamp),
    Date(Date),
}

/// Reads `text` (see the module's times) against `now`.
pub fn at(text: &str, now: &Zoned) -> std::result::Result<At, String> {
    let bad = |why: &str| format!("{text:?}: {why}");
    let text = text.trim();
    let zone = now.time_zone();
    if let Some(rest) = text.strip_prefix("now") {
        let rest = rest.trim();
        if rest.is_empty() {
            return Ok(At::Instant(now.timestamp()));
        }
        let (sign, span) = match rest.split_at(1) {
            ("+", span) => (1, span),
            ("-", span) => (-1, span),
            _ => return Err(bad("now, then + or - and a span")),
        };
        let span = span_of(span.trim()).ok_or_else(|| bad("a span as 1h30m"))?;
        let shift = if sign < 0 { -span } else { span };
        return now
            .timestamp()
            .checked_add(shift)
            .map(At::Instant)
            .map_err(|_| bad("out of range"));
    }
    let (day_word, time) = match text.split_once([' ', 'T']) {
        Some((day, time)) => (day.trim(), Some(time.trim())),
        None => (text, None),
    };
    let date = relative_day(day_word, now.date())
        .or_else(|| day_word.parse::<Date>().ok())
        .ok_or_else(|| bad("a day: today, tomorrow, yesterday, today+N, or YYYY-MM-DD"))?;
    match time {
        None => Ok(At::Date(date)),
        Some(time) => {
            let time = time
                .parse::<jiff::civil::Time>()
                .map_err(|_| bad("a time as HH:MM"))?;
            date.to_datetime(time)
                .to_zoned(zone.clone())
                .map(|z| At::Instant(z.timestamp()))
                .map_err(|_| bad("not a time there"))
        }
    }
}

/// `today`, `tomorrow`, `yesterday`, `today+N`, `today-N`.
fn relative_day(word: &str, today: Date) -> Option<Date> {
    let (base, rest) = if let Some(rest) = word.strip_prefix("today") {
        (0, rest)
    } else if let Some(rest) = word.strip_prefix("tomorrow") {
        (1, rest)
    } else {
        (-1, word.strip_prefix("yesterday")?)
    };
    let offset: i32 = match rest {
        "" => 0,
        _ => {
            let (sign, n) = rest.split_at(1);
            let n: i32 = n.parse().ok()?;
            match sign {
                "+" => n,
                "-" => -n,
                _ => return None,
            }
        }
    };
    today
        .checked_add(jiff::Span::new().days(base + offset))
        .ok()
}

/// `1h30m`, `25m`, `2d`, `90s`.
fn span_of(text: &str) -> Option<SignedDuration> {
    if text.is_empty() {
        return None;
    }
    let mut total = 0i64;
    let mut number = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            number.push(c);
            continue;
        }
        let n: i64 = number.parse().ok()?;
        number.clear();
        let unit = match c {
            'd' => 86_400,
            'h' => 3_600,
            'm' => 60,
            's' => 1,
            _ => return None,
        };
        total = total.checked_add(n.checked_mul(unit)?)?;
    }
    number.is_empty().then(|| SignedDuration::from_secs(total))
}

// --- The file ---------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct File {
    calendars: Vec<FileCalendar>,
    events: Vec<FileEvent>,
    tasks: Option<FileTasks>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct FileCalendar {
    id: Option<String>,
    name: String,
    #[serde(default)]
    color: Option<Colour>,
    #[serde(default)]
    read_only: bool,
    #[serde(default = "yes")]
    shown: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileEvent {
    id: Option<String>,
    calendar: Option<String>,
    title: String,
    start: Option<String>,
    end: Option<String>,
    date: Option<String>,
    #[serde(default)]
    days: Option<u32>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    answer: Option<String>,
    #[serde(default)]
    free: bool,
    place: Option<String>,
    video: Option<String>,
    page: Option<String>,
    description: Option<String>,
    #[serde(default)]
    alarms: Vec<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileTasks {
    #[serde(default = "board_name")]
    board: String,
    #[serde(default = "default_columns")]
    columns: Vec<String>,
    #[serde(default)]
    refuse: bool,
    #[serde(default)]
    cards: Vec<FileCard>,
}

fn board_name() -> String {
    "Fixture".to_owned()
}

fn default_columns() -> Vec<String> {
    vec!["TODO".to_owned(), "DONE".to_owned()]
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileCard {
    id: Option<String>,
    title: String,
    column: Option<String>,
    due: Option<String>,
}

/// The day a fixture's text says, read against `now`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Read {
    pub calendars: Vec<Calendar>,
    pub events: Vec<Event>,
    /// The board, its cards by column, and whether changes are refused.
    pub board: Option<(TaskBoard, HashMap<ColumnId, Vec<Task>>, bool)>,
}

/// Reads a fixture's `text` for account `account` against `now`.
pub fn read(text: &str, account: &AccountId, now: &Zoned) -> std::result::Result<Read, String> {
    let file: File = toml::from_str(text).map_err(|err| err.to_string())?;
    let calendars: Vec<Calendar> = file
        .calendars
        .iter()
        .map(|c| Calendar {
            id: CalendarId::new(c.id.clone().unwrap_or_else(|| c.name.clone())),
            account: account.clone(),
            name: c.name.clone(),
            colour: c.color.unwrap_or(Colour::DEFAULT),
            read_only: c.read_only,
            shown: c.shown,
        })
        .collect();
    let mut events = Vec::new();
    for (i, e) in file.events.iter().enumerate() {
        let calendar = match &e.calendar {
            Some(id) => calendars
                .iter()
                .find(|c| c.id.as_str() == id || c.name == *id)
                .ok_or_else(|| format!("event {:?}: no calendar {id:?}", e.title))?,
            None => calendars
                .first()
                .ok_or_else(|| format!("event {:?}: no calendar", e.title))?,
        };
        let when = when_of(e, now).map_err(|why| format!("event {:?}: {why}", e.title))?;
        let status = match e.status.as_deref().unwrap_or("confirmed") {
            "confirmed" => Status::Confirmed,
            "tentative" => Status::Tentative,
            "cancelled" => Status::Cancelled,
            other => return Err(format!("event {:?}: status {other:?}", e.title)),
        };
        let answer = match e.answer.as_deref().unwrap_or("unanswered") {
            "accepted" => Answer::Accepted,
            "declined" => Answer::Declined,
            "unanswered" => Answer::Unanswered,
            other => return Err(format!("event {:?}: answer {other:?}", e.title)),
        };
        let mut alarms = e.alarms.clone();
        alarms.sort_unstable_by(|a, b| b.cmp(a));
        events.push(Event {
            id: EventId::new(e.id.clone().unwrap_or_else(|| format!("e{}", i + 1))),
            calendar: calendar.id.clone(),
            title: e.title.clone(),
            when,
            status,
            answer,
            free_busy: if e.free {
                FreeBusy::Free
            } else {
                FreeBusy::Busy
            },
            place: e.place.clone(),
            video: e.video.clone(),
            page: e.page.clone(),
            description: e.description.clone(),
            alarms,
        });
    }
    let board = match file.tasks {
        None => None,
        Some(tasks) => {
            if tasks.columns.is_empty() {
                return Err("tasks: no column".to_owned());
            }
            let columns: Vec<Column> = tasks
                .columns
                .iter()
                .map(|name| Column {
                    id: ColumnId::new(name.to_lowercase()),
                    name: name.clone(),
                })
                .collect();
            let board = TaskBoard {
                id: BoardId::new(tasks.board.to_lowercase()),
                name: tasks.board.clone(),
                columns,
            };
            let mut cards: HashMap<ColumnId, Vec<Task>> = board
                .columns
                .iter()
                .map(|c| (c.id.clone(), Vec::new()))
                .collect();
            for (i, card) in tasks.cards.iter().enumerate() {
                let column = match &card.column {
                    Some(name) => board
                        .column(name)
                        .ok_or_else(|| format!("card {:?}: no column {name:?}", card.title))?,
                    None => &board.columns[0],
                };
                let due = match &card.due {
                    None => None,
                    Some(due) => Some(
                        match at(due, now).map_err(|w| format!("card {:?}: {w}", card.title))? {
                            At::Instant(t) => t,
                            At::Date(d) => crate::model::midnight(d, now.time_zone()),
                        },
                    ),
                };
                let list = cards.entry(column.id.clone()).or_default();
                list.push(Task {
                    id: TaskId::new(card.id.clone().unwrap_or_else(|| format!("c{}", i + 1))),
                    title: card.title.clone(),
                    due,
                    position: STEP * (list.len() + 1) as f64,
                });
            }
            Some((board, cards, tasks.refuse))
        }
    };
    Ok(Read {
        calendars,
        events,
        board,
    })
}

fn when_of(e: &FileEvent, now: &Zoned) -> std::result::Result<When, String> {
    match (&e.start, &e.date) {
        (Some(start), None) => {
            let At::Instant(start) = at(start, now)? else {
                return Err("start: a time, not a day (date = … for a whole day)".to_owned());
            };
            let end = match &e.end {
                Some(end) => match at(end, now)? {
                    At::Instant(end) => end,
                    At::Date(_) => return Err("end: a time".to_owned()),
                },
                None => start
                    .checked_add(SignedDuration::from_hours(1))
                    .map_err(|err| err.to_string())?,
            };
            if end < start {
                return Err("it ends before it starts".to_owned());
            }
            Ok(When::Timed { start, end })
        }
        (None, Some(date)) => {
            let At::Date(first) = at(date, now)? else {
                return Err("date: a day, without its time".to_owned());
            };
            let days = e.days.unwrap_or(1).max(1);
            let last = first
                .checked_add(jiff::Span::new().days(i64::from(days) - 1))
                .map_err(|err| err.to_string())?;
            Ok(When::AllDay { first, last })
        }
        _ => Err("start (and end), or date (and days)".to_owned()),
    }
}

/// The gap between two cards' positions, as Planka numbers them.
pub const STEP: f64 = 65_536.0;

// --- As sources -------------------------------------------------------------------

/// The fixture's file, read again only when its text changes, so that
/// its relative times stay where they were read.
#[derive(Debug)]
struct Watched {
    file: PathBuf,
    account: AccountId,
    clock: Clock,
    text: Option<String>,
    read: Read,
}

impl Watched {
    fn new(file: PathBuf, account: AccountId, clock: Clock) -> Self {
        Self {
            file,
            account,
            clock,
            text: None,
            read: Read::default(),
        }
    }

    /// The file read again: whether its text changed.
    fn refresh(&mut self) -> Result<bool> {
        let text = std::fs::read_to_string(&self.file)
            .map_err(|err| SourceError::Invalid(format!("{}: {err}", self.file.display())))?;
        if self.text.as_ref() == Some(&text) {
            return Ok(false);
        }
        let read = read(&text, &self.account, &self.clock.now())
            .map_err(|why| SourceError::Invalid(format!("{}: {why}", self.file.display())))?;
        self.text = Some(text);
        self.read = read;
        Ok(true)
    }
}

/// A fixture's calendars and events.
#[derive(Debug)]
pub struct FixtureCalendars(Watched);

impl FixtureCalendars {
    pub fn new(file: PathBuf, account: AccountId, clock: Clock) -> Self {
        Self(Watched::new(file, account, clock))
    }
}

impl CalendarSource for FixtureCalendars {
    fn calendars(&mut self) -> Result<Vec<Calendar>> {
        self.0.refresh()?;
        Ok(self.0.read.calendars.clone())
    }

    fn events(&mut self, calendar: &CalendarId, range: Range) -> Result<Listing> {
        let zone = self.0.clock.now().time_zone().clone();
        Ok(Listing {
            events: self
                .0
                .read
                .events
                .iter()
                .filter(|e| e.calendar == *calendar && e.overlaps(range.from, range.to, &zone))
                .cloned()
                .collect(),
            token: None,
        })
    }
}

/// A fixture's board: its cards in memory, as the file says them when it
/// changes, and as the bar moved them since.
#[derive(Debug)]
pub struct FixtureTasks {
    file: Watched,
    cards: HashMap<ColumnId, Vec<Task>>,
    made: u64,
}

impl FixtureTasks {
    pub fn new(file: PathBuf, account: AccountId, clock: Clock) -> Self {
        Self {
            file: Watched::new(file, account, clock),
            cards: HashMap::new(),
            made: 0,
        }
    }

    fn refuses(&self) -> Result<()> {
        match &self.file.read.board {
            Some((_, _, true)) => Err(SourceError::Offline(
                "the fixture refuses changes".to_owned(),
            )),
            _ => Ok(()),
        }
    }
}

impl KanbanSource for FixtureTasks {
    fn board(&mut self) -> Result<TaskBoard> {
        if self.file.refresh()? {
            self.cards = self
                .file
                .read
                .board
                .as_ref()
                .map(|(_, cards, _)| cards.clone())
                .unwrap_or_default();
        }
        self.file
            .read
            .board
            .as_ref()
            .map(|(board, _, _)| board.clone())
            .ok_or_else(|| {
                SourceError::Invalid(format!("{}: no [tasks]", self.file.file.display()))
            })
    }

    fn cards(&mut self, column: &ColumnId) -> Result<Vec<Task>> {
        let mut cards = self.cards.get(column).cloned().unwrap_or_default();
        sort_tasks(&mut cards);
        Ok(cards)
    }

    fn create(&mut self, column: &ColumnId, title: &str) -> Result<Task> {
        self.refuses()?;
        self.made += 1;
        let cards = self.cards.entry(column.clone()).or_default();
        let task = Task {
            id: TaskId::new(format!("new{}", self.made)),
            title: title.to_owned(),
            due: None,
            position: top(cards),
        };
        cards.push(task.clone());
        Ok(task)
    }

    fn move_card(&mut self, card: &TaskId, place: &Place) -> Result<()> {
        self.refuses()?;
        move_in(&mut self.cards, card, place)
    }
}

/// A position above every card of `cards`.
pub(crate) fn top(cards: &[Task]) -> f64 {
    cards
        .iter()
        .map(|t| t.position)
        .min_by(f64::total_cmp)
        .map_or(STEP, |first| first - STEP)
}

/// Moves `card` to `place` among `columns`.
pub(crate) fn move_in(
    columns: &mut HashMap<ColumnId, Vec<Task>>,
    card: &TaskId,
    place: &Place,
) -> Result<()> {
    let mut task = None;
    for cards in columns.values_mut() {
        if let Some(i) = cards.iter().position(|t| t.id == *card) {
            task = Some(cards.remove(i));
            break;
        }
    }
    let mut task = task.ok_or(SourceError::Gone)?;
    let cards = columns.entry(place.column.clone()).or_default();
    task.position = place.position.unwrap_or_else(|| top(cards));
    cards.push(task);
    Ok(())
}

/// Whether `path` names a file whose changes can be watched (its folder
/// exists).
pub fn watchable(path: &Path) -> bool {
    path.parent().is_some_and(Path::is_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::tz::TimeZone;

    fn now() -> Zoned {
        jiff::civil::date(2026, 11, 4)
            .at(14, 0, 0, 0)
            .to_zoned(TimeZone::UTC)
            .expect("a time")
    }

    fn instant(text: &str) -> Timestamp {
        match at(text, &now()).expect("a time") {
            At::Instant(t) => t,
            At::Date(d) => panic!("a day: {d}"),
        }
    }

    #[test]
    fn times_are_relative_to_the_clock() {
        assert_eq!(instant("now").to_string(), "2026-11-04T14:00:00Z");
        assert_eq!(instant("now+25m").to_string(), "2026-11-04T14:25:00Z");
        assert_eq!(instant("now-1h30m").to_string(), "2026-11-04T12:30:00Z");
        assert_eq!(instant("now+2d").to_string(), "2026-11-06T14:00:00Z");
        assert_eq!(instant("today 09:30").to_string(), "2026-11-04T09:30:00Z");
        assert_eq!(
            instant("tomorrow 08:00").to_string(),
            "2026-11-05T08:00:00Z"
        );
        assert_eq!(
            instant("yesterday 18:15:30").to_string(),
            "2026-11-03T18:15:30Z"
        );
        assert_eq!(instant("today+3 10:00").to_string(), "2026-11-07T10:00:00Z");
        assert_eq!(instant("today-2 10:00").to_string(), "2026-11-02T10:00:00Z");
        assert_eq!(
            instant("2026-12-24 20:00").to_string(),
            "2026-12-24T20:00:00Z"
        );
        assert_eq!(
            instant("2026-12-24T20:00").to_string(),
            "2026-12-24T20:00:00Z"
        );
        assert_eq!(
            at("tomorrow", &now()),
            Ok(At::Date(jiff::civil::date(2026, 11, 5)))
        );
        for bad in [
            "later",
            "now+",
            "now+5x",
            "now*5m",
            "today 25:00",
            "today+x",
        ] {
            assert!(at(bad, &now()).is_err(), "{bad}");
        }
    }

    #[test]
    fn times_follow_the_clocks_zone() {
        // 14:00 in Paris, in November: 13:00 UTC.
        let paris = TimeZone::posix("CET-1CEST,M3.5.0,M10.5.0/3").expect("a zone");
        let now = jiff::civil::date(2026, 11, 4)
            .at(14, 0, 0, 0)
            .to_zoned(paris)
            .expect("a time");
        assert_eq!(
            at("today 09:30", &now),
            Ok(At::Instant("2026-11-04T08:30:00Z".parse().expect("a time")))
        );
        assert_eq!(
            at("now+25m", &now),
            Ok(At::Instant("2026-11-04T13:25:00Z".parse().expect("a time")))
        );
    }

    #[test]
    fn a_fixture_reads_its_calendars_events_and_cards() {
        let text = r##"
            [[calendars]]
            id = "work"
            name = "Travail"
            color = "#6CC287"

            [[calendars]]
            name = "Perso"
            shown = false
            read-only = true

            [[events]]
            title = "Point produit"
            start = "now+25m"
            end = "now+55m"
            video = "https://meet.example.org/abc"
            alarms = [5, 10]

            [[events]]
            calendar = "Perso"
            title = "Congés"
            date = "tomorrow"
            days = 2
            answer = "declined"
            free = true

            [tasks]
            board = "Sprint"
            [[tasks.cards]]
            title = "Relire la spec"
            [[tasks.cards]]
            title = "Écrire le ticket"
            due = "tomorrow"
            [[tasks.cards]]
            title = "Fait hier"
            column = "done"
        "##;
        let read = read(text, &AccountId::new("fixture"), &now()).expect("read");
        assert_eq!(read.calendars.len(), 2);
        assert_eq!(read.calendars[0].colour, Colour([0x6C, 0xC2, 0x87]));
        assert_eq!(read.calendars[1].id.as_str(), "Perso");
        assert!(!read.calendars[1].shown && read.calendars[1].read_only);
        assert_eq!(
            read.events[0].when,
            When::Timed {
                start: "2026-11-04T14:25:00Z".parse().expect("a time"),
                end: "2026-11-04T14:55:00Z".parse().expect("a time"),
            }
        );
        assert_eq!(read.events[0].alarms, [10, 5]);
        assert_eq!(
            read.events[1].when,
            When::AllDay {
                first: jiff::civil::date(2026, 11, 5),
                last: jiff::civil::date(2026, 11, 6),
            }
        );
        assert_eq!(read.events[1].calendar.as_str(), "Perso");
        assert_eq!(read.events[1].answer, Answer::Declined);
        let (board, cards, refuse) = read.board.expect("a board");
        assert_eq!(board.columns.len(), 2);
        assert!(!refuse);
        let todo = &cards[&ColumnId::new("todo")];
        assert_eq!(todo.len(), 2);
        assert_eq!(
            todo[1].due,
            Some("2026-11-05T00:00:00Z".parse().expect("a time"))
        );
        assert_eq!(cards[&ColumnId::new("done")].len(), 1);
        for bad in [
            "[[events]]\ntitle = \"x\"\nstart = \"now\"\n",
            "[[calendars]]\nname = \"a\"\n[[events]]\ntitle = \"x\"\n",
            "[[calendars]]\nname = \"a\"\n[[events]]\ntitle = \"x\"\nstart = \"now\"\nend = \"now-1h\"\n",
            "[[calendars]]\nname = \"a\"\nsize = 3\n",
        ] {
            assert!(read_err(bad), "{bad}");
        }
    }

    fn read_err(text: &str) -> bool {
        read(text, &AccountId::new("fixture"), &now()).is_err()
    }
}
