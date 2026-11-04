//! An account's worker: a thread of its own that reads its source and
//! sends owned [`Snapshot`]s back, only when they change. It reads at
//! start (its cache first, if it has one), then on its schedule, when
//! asked ([`Command::Refresh`]: on request, on resume, when a fixture's
//! file changes), and at once after a change. Between, it sleeps on its
//! channel: no wakeup until its schedule or a command.
//!
//! The bar's changes are queued, sent at once in their order, and kept
//! when the source refuses them, each with why: they are tried again each
//! time the source answers again. A card that is no more drops its change.

use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

use jiff::{Timestamp, civil::Date};
use serde::{Deserialize, Serialize};

use crate::{
    Clock,
    model::{
        Calendar, CalendarId, Colour, Event, Place, Task, TaskBoard, TaskId, sort_events,
        sort_tasks,
    },
    snapshot::{Account, Change, ChangeId, Pending, Snapshot, State},
    source::{CalendarSource, ColumnNames, Columns, KanbanSource, Range, SourceError, SyncToken},
};

/// What an account reads.
pub(crate) enum Source {
    Calendars {
        source: Box<dyn CalendarSource>,
        /// The configuration's `shown`: names or ids.
        shown: Option<Vec<String>>,
        colours: BTreeMap<String, Colour>,
    },
    Tasks {
        source: Box<dyn KanbanSource>,
        names: ColumnNames,
    },
}

/// A worker to start: its source is made on its thread (a file read
/// there, not where it is started).
pub(crate) struct Plan {
    pub account: Account,
    pub make: Box<dyn FnOnce() -> Source + Send>,
    /// Read again this often; `None`: only when asked.
    pub every: Option<Duration>,
    /// Its cache's file.
    pub cache: Option<PathBuf>,
    /// A file whose changes have it read again (a fixture's).
    pub watch: Option<PathBuf>,
}

pub(crate) enum Command {
    Refresh,
    Change(ChangeId, Change),
    /// Answered once what came before is done.
    Flush(mpsc::Sender<()>),
}

/// A running worker.
#[derive(Debug)]
pub(crate) struct Handle {
    pub account: crate::model::AccountId,
    commands: mpsc::Sender<Command>,
}

impl Handle {
    pub fn send(&self, command: Command) -> bool {
        self.commands.send(command).is_ok()
    }

    /// Waits up to `within` for what was sent before to be done.
    pub fn flush(&self, within: Duration) -> bool {
        let (done, wait) = mpsc::channel();
        self.send(Command::Flush(done)) && wait.recv_timeout(within).is_ok()
    }
}

/// Starts `plan`'s worker, sending its snapshots to `out`.
pub(crate) fn spawn(plan: Plan, clock: Clock, out: mpsc::Sender<Snapshot>) -> Option<Handle> {
    let (commands, incoming) = mpsc::channel();
    let account = plan.account.id.clone();
    if let Some(file) = plan.watch.clone() {
        watch(file, commands.clone(), &account);
    }
    let started = std::thread::Builder::new()
        .name(format!("day: {account}"))
        .spawn(move || {
            let Plan {
                account,
                make,
                every,
                cache,
                watch: _,
            } = plan;
            let mut worker = Worker {
                snapshot: Snapshot::loading(account),
                source: make(),
                cache,
                clock,
                out,
                sent: None,
                cached: None,
                tokens: BTreeMap::new(),
                window: None,
                columns: None,
                places: HashMap::new(),
            };
            worker.run(&incoming, every);
        });
    match started {
        Ok(_) => Some(Handle { account, commands }),
        Err(err) => {
            tracing::warn!(%account, %err, "no thread for an account of the day");
            None
        }
    }
}

/// What the cache keeps of an account.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Cached {
    fetched: Timestamp,
    #[serde(flatten)]
    data: Data,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Data {
    calendars: Vec<Calendar>,
    events: Vec<Event>,
    board: Option<TaskBoard>,
    tasks: Vec<Task>,
    tokens: BTreeMap<CalendarId, SyncToken>,
    /// The day the events' window was last listed around.
    window: Option<Date>,
}

struct Worker {
    source: Source,
    snapshot: Snapshot,
    cache: Option<PathBuf>,
    clock: Clock,
    out: mpsc::Sender<Snapshot>,
    sent: Option<Snapshot>,
    /// What the cache holds, so that it is written only when that changes.
    cached: Option<Data>,
    tokens: BTreeMap<CalendarId, SyncToken>,
    window: Option<Date>,
    columns: Option<Columns>,
    /// Where each task ticked this session was, for its untick.
    places: HashMap<TaskId, Place>,
}

impl Worker {
    fn id(&self) -> &str {
        self.snapshot.account.id.as_str()
    }

    fn run(&mut self, incoming: &mpsc::Receiver<Command>, every: Option<Duration>) {
        self.read_cache();
        self.send();
        self.fetch_and_retry();
        self.send();
        let mut due = every.map(|every| Instant::now() + every);
        loop {
            let first = match due {
                Some(at) => {
                    match incoming.recv_timeout(at.saturating_duration_since(Instant::now())) {
                        Ok(command) => Some(command),
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
                None => match incoming.recv() {
                    Ok(command) => Some(command),
                    Err(_) => return,
                },
            };
            tracing::trace!(account = self.id(), "the day's worker wakes");
            // The schedule's time, or what was asked meanwhile, together.
            let mut fetch = first.is_none();
            let mut changes = false;
            let mut flushes = Vec::new();
            for command in first.into_iter().chain(incoming.try_iter()) {
                match command {
                    Command::Refresh => fetch = true,
                    Command::Change(id, change) => {
                        self.queue(id, change);
                        changes = true;
                    }
                    Command::Flush(done) => flushes.push(done),
                }
            }
            if changes {
                // Seen pending at once, then sent.
                self.send();
                if self.carry_out() {
                    fetch = true;
                }
            }
            if fetch {
                self.fetch_and_retry();
                due = every.map(|every| Instant::now() + every);
            }
            self.send();
            for done in flushes {
                let _ = done.send(());
            }
        }
    }

    /// The snapshot to the shell, if it changed since the last sent.
    fn send(&mut self) {
        if self.sent.as_ref() == Some(&self.snapshot) {
            return;
        }
        self.sent = Some(self.snapshot.clone());
        let _ = self.out.send(self.snapshot.clone());
    }

    // --- Changes ------------------------------------------------------------------

    /// `change` queued; a tick and its untick, both not sent, undo each
    /// other.
    fn queue(&mut self, id: ChangeId, change: Change) {
        let pending = &mut self.snapshot.pending;
        let undone = match &change {
            Change::Tick { task } => pending
                .iter()
                .position(|p| matches!(&p.change, Change::Untick { task: t } if t == task)),
            Change::Untick { task } => pending
                .iter()
                .position(|p| matches!(&p.change, Change::Tick { task: t } if t == task)),
            Change::Add { .. } => None,
        };
        match undone {
            Some(i) => {
                pending.remove(i);
            }
            None => pending.push(Pending {
                id,
                change,
                refused: None,
            }),
        }
    }

    /// Sends what is queued, in its order: whether any went through. The
    /// source not answering stops there, the rest kept with its error.
    fn carry_out(&mut self) -> bool {
        let mut done = false;
        let mut stop: Option<SourceError> = None;
        let pending = std::mem::take(&mut self.snapshot.pending);
        let mut kept = Vec::new();
        for mut p in pending {
            if let Some(error) = &stop {
                p.refused = Some(error.clone());
                kept.push(p);
                continue;
            }
            match self.apply(&p.change) {
                Ok(()) => {
                    tracing::debug!(account = self.id(), change = ?p.change, "a change carried out");
                    done = true;
                }
                Err(SourceError::Gone) => {
                    tracing::info!(account = self.id(), change = ?p.change, "a change dropped: its task is no more");
                    done = true;
                }
                Err(error) => {
                    tracing::info!(account = self.id(), change = ?p.change, %error, "a change refused: kept");
                    if matches!(error, SourceError::Offline(_) | SourceError::SignIn(_)) {
                        stop = Some(error.clone());
                    }
                    p.refused = Some(error);
                    kept.push(p);
                }
            }
        }
        self.snapshot.pending = kept;
        done
    }

    fn apply(&mut self, change: &Change) -> Result<(), SourceError> {
        let Source::Tasks { source, .. } = &mut self.source else {
            return Err(SourceError::Invalid("no tasks here".to_owned()));
        };
        let Some(columns) = &self.columns else {
            return Err(SourceError::Offline("the board is not read yet".to_owned()));
        };
        match change {
            Change::Add { title } => source.create(&columns.add.id, title).map(|_| ()),
            Change::Tick { task } => {
                if let Some(t) = self.snapshot.tasks.iter().find(|t| t.id == *task) {
                    self.places.insert(
                        task.clone(),
                        Place {
                            column: columns.tasks.id.clone(),
                            position: Some(t.position),
                        },
                    );
                }
                source.move_card(
                    task,
                    &Place {
                        column: columns.done.id.clone(),
                        position: None,
                    },
                )
            }
            Change::Untick { task } => {
                let place = self.places.get(task).cloned().unwrap_or(Place {
                    column: columns.tasks.id.clone(),
                    position: None,
                });
                source.move_card(task, &place)?;
                self.places.remove(task);
                Ok(())
            }
        }
    }

    // --- Reading ------------------------------------------------------------------

    /// Reads the source; if it answered, what it refused before is tried
    /// again, and read again if any went through.
    fn fetch_and_retry(&mut self) {
        if self.fetch() && !self.snapshot.pending.is_empty() && self.carry_out() {
            self.fetch();
        }
    }

    /// Reads the source: whether it answered.
    fn fetch(&mut self) -> bool {
        let read = match &mut self.source {
            Source::Calendars {
                source,
                shown,
                colours,
            } => read_calendars(
                source.as_mut(),
                shown.as_deref(),
                colours,
                &self.clock,
                Fetched {
                    events: &self.snapshot.events,
                    tokens: &self.tokens,
                    window: self.window,
                },
            )
            .map(|(calendars, events, tokens, window)| {
                self.snapshot.calendars = calendars;
                self.snapshot.events = events;
                self.tokens = tokens;
                self.window = Some(window);
            }),
            Source::Tasks { source, names } => {
                read_tasks(source.as_mut(), names).map(|(board, columns, tasks)| {
                    self.snapshot.board = Some(board);
                    self.snapshot.tasks = tasks;
                    self.columns = Some(columns);
                })
            }
        };
        let fetched = self.clock.now().timestamp();
        match read {
            Ok(()) => {
                tracing::debug!(account = self.id(), "the day's account read");
                self.snapshot.state = State::Fresh { fetched };
                self.write_cache(fetched);
                true
            }
            Err(error) => {
                let before = match &self.snapshot.state {
                    State::Cached { fetched } | State::Fresh { fetched } => Some(*fetched),
                    State::Failed { fetched, .. } => *fetched,
                    State::Loading => None,
                };
                if !matches!(&self.snapshot.state, State::Failed { error: e, .. } if *e == error) {
                    tracing::warn!(account = self.id(), %error, "the day's account is not read");
                }
                self.snapshot.state = State::Failed {
                    error,
                    fetched: before,
                };
                false
            }
        }
    }

    fn data(&self) -> Data {
        Data {
            calendars: self.snapshot.calendars.clone(),
            events: self.snapshot.events.clone(),
            board: self.snapshot.board.clone(),
            tasks: self.snapshot.tasks.clone(),
            tokens: self.tokens.clone(),
            window: self.window,
        }
    }

    fn read_cache(&mut self) {
        let Some(file) = &self.cache else {
            return;
        };
        let text = match std::fs::read_to_string(file) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
            Err(err) => {
                tracing::info!(file = %file.display(), %err, "the day's cache is not read");
                return;
            }
        };
        match serde_json::from_str::<Cached>(&text) {
            Ok(cached) => {
                self.snapshot.state = State::Cached {
                    fetched: cached.fetched,
                };
                self.snapshot.calendars = cached.data.calendars.clone();
                self.snapshot.events = cached.data.events.clone();
                self.snapshot.board = cached.data.board.clone();
                self.snapshot.tasks = cached.data.tasks.clone();
                self.tokens = cached.data.tokens.clone();
                self.window = cached.data.window;
                self.cached = Some(cached.data);
            }
            Err(err) => {
                tracing::info!(file = %file.display(), %err, "the day's cache does not read: read again");
            }
        }
    }

    /// The cache written, when what it keeps changed.
    fn write_cache(&mut self, fetched: Timestamp) {
        let Some(file) = &self.cache else {
            return;
        };
        let data = self.data();
        if self.cached.as_ref() == Some(&data) {
            return;
        }
        let cached = Cached { fetched, data };
        let written = serde_json::to_vec(&cached)
            .map_err(std::io::Error::other)
            .and_then(|bytes| crate::write_file(file, &bytes));
        match written {
            Ok(()) => self.cached = Some(cached.data),
            Err(err) => {
                tracing::warn!(file = %file.display(), %err, "the day's cache is not written")
            }
        }
    }
}

/// What an account had, for an incremental read.
struct Fetched<'a> {
    events: &'a [Event],
    tokens: &'a BTreeMap<CalendarId, SyncToken>,
    window: Option<Date>,
}

type CalendarsRead = (
    Vec<Calendar>,
    Vec<Event>,
    BTreeMap<CalendarId, SyncToken>,
    Date,
);

/// The calendars, the configuration's names and colours over the
/// account's, and their events in the window: listed whole once a day (the
/// window moves), else synced from their tokens.
fn read_calendars(
    source: &mut dyn CalendarSource,
    shown: Option<&[String]>,
    colours: &BTreeMap<String, Colour>,
    clock: &Clock,
    had: Fetched<'_>,
) -> Result<CalendarsRead, SourceError> {
    let now = clock.now();
    let zone = now.time_zone().clone();
    let window = Range::window(&now);
    let names = |c: &Calendar, name: &str| {
        c.id.as_str() == name || c.name.to_lowercase() == name.to_lowercase()
    };
    let mut calendars = source.calendars()?;
    for calendar in &mut calendars {
        if let Some(shown) = shown {
            calendar.shown = shown.iter().any(|name| names(calendar, name));
        }
        if let Some((_, colour)) = colours.iter().find(|(name, _)| names(calendar, name)) {
            calendar.colour = *colour;
        }
    }
    let whole = had.window != Some(now.date());
    let mut events = Vec::new();
    let mut tokens = BTreeMap::new();
    for calendar in calendars.iter().filter(|c| c.shown) {
        let synced = match had.tokens.get(&calendar.id).filter(|_| !whole) {
            Some(token) => match source.changes(&calendar.id, token) {
                Ok(changes) => {
                    let mut kept: Vec<Event> = had
                        .events
                        .iter()
                        .filter(|e| e.calendar == calendar.id)
                        .filter(|e| {
                            !changes.removed.contains(&e.id)
                                && !changes.changed.iter().any(|c| c.id == e.id)
                        })
                        .cloned()
                        .collect();
                    kept.extend(changes.changed);
                    Some((kept, changes.token))
                }
                Err(SourceError::Expired) => None,
                Err(error) => return Err(error),
            },
            None => None,
        };
        let (mut listed, token) = match synced {
            Some(synced) => synced,
            None => {
                let listing = source.events(&calendar.id, window)?;
                (listing.events, listing.token)
            }
        };
        for event in &mut listed {
            event.calendar = calendar.id.clone();
        }
        events.extend(
            listed
                .into_iter()
                .filter(|e| e.overlaps(window.from, window.to, &zone)),
        );
        if let Some(token) = token {
            tokens.insert(calendar.id.clone(), token);
        }
    }
    sort_events(&mut events, &zone);
    Ok((calendars, events, tokens, now.date()))
}

/// The board, its columns found, and the tasks' column's cards in order.
fn read_tasks(
    source: &mut dyn KanbanSource,
    names: &ColumnNames,
) -> Result<(TaskBoard, Columns, Vec<Task>), SourceError> {
    let board = source.board()?;
    let columns = names.find(&board)?;
    let mut tasks = source.cards(&columns.tasks.id)?;
    sort_tasks(&mut tasks);
    Ok((board, columns, tasks))
}

/// Watches `file` for changes on a thread of its own (inotify on its
/// folder: editors replace a file as often as they write it), asking for
/// a read on each; it sleeps meanwhile.
fn watch(file: PathBuf, commands: mpsc::Sender<Command>, account: &crate::model::AccountId) {
    use rustix::{
        fs::inotify::{self, CreateFlags, WatchFlags},
        io::Errno,
    };
    let (Some(dir), Some(name)) = (file.parent(), file.file_name()) else {
        return;
    };
    let (dir, name) = (dir.to_path_buf(), name.to_os_string());
    let watched = inotify::init(CreateFlags::CLOEXEC).and_then(|fd| {
        let flags = WatchFlags::CLOSE_WRITE
            | WatchFlags::MOVED_TO
            | WatchFlags::CREATE
            | WatchFlags::DELETE;
        inotify::add_watch(&fd, &dir, flags).map(|_| fd)
    });
    let fd = match watched {
        Ok(fd) => fd,
        Err(err) => {
            tracing::warn!(file = %file.display(), %err, "a fixture's file is not watched: read at start only");
            return;
        }
    };
    let started = std::thread::Builder::new()
        .name(format!("day: {account}'s file"))
        .spawn(move || {
            let mut buffer = [std::mem::MaybeUninit::uninit(); 1024];
            let mut events = inotify::Reader::new(&fd, &mut buffer);
            loop {
                match events.next() {
                    Ok(event) => {
                        let ours = event
                            .file_name()
                            .is_some_and(|n| n.to_bytes() == name.as_encoded_bytes());
                        if ours && commands.send(Command::Refresh).is_err() {
                            return;
                        }
                    }
                    Err(Errno::INTR) => {}
                    Err(err) => {
                        tracing::warn!(%err, "stopped watching a fixture's file");
                        return;
                    }
                }
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "no thread to watch a fixture's file");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::{
        model::{AccountId, BoardId, Column, ColumnId},
        snapshot::{Holds, Kind},
        source::Listing,
    };

    fn clock() -> Clock {
        let now = jiff::civil::date(2026, 11, 4)
            .at(14, 0, 0, 0)
            .to_zoned(jiff::tz::TimeZone::UTC)
            .expect("a time");
        Clock::new(move || now.clone())
    }

    /// A board whose answers the test sets.
    #[derive(Clone, Default)]
    struct Fake(Arc<Mutex<FakeState>>);

    #[derive(Default)]
    struct FakeState {
        offline: bool,
        todo: Vec<Task>,
        done: Vec<Task>,
        asked: Vec<String>,
    }

    impl KanbanSource for Fake {
        fn board(&mut self) -> Result<TaskBoard, SourceError> {
            let state = self.0.lock().expect("lock");
            if state.offline {
                return Err(SourceError::Offline("down".to_owned()));
            }
            Ok(TaskBoard {
                id: BoardId::new("b"),
                name: "Sprint".to_owned(),
                columns: ["TODO", "DONE"]
                    .iter()
                    .map(|n| Column {
                        id: ColumnId::new(*n),
                        name: (*n).to_owned(),
                    })
                    .collect(),
            })
        }

        fn cards(&mut self, column: &ColumnId) -> Result<Vec<Task>, SourceError> {
            let state = self.0.lock().expect("lock");
            Ok(if column.as_str() == "TODO" {
                state.todo.clone()
            } else {
                state.done.clone()
            })
        }

        fn create(&mut self, _: &ColumnId, title: &str) -> Result<Task, SourceError> {
            let mut state = self.0.lock().expect("lock");
            state.asked.push(format!("create {title}"));
            if state.offline {
                return Err(SourceError::Offline("down".to_owned()));
            }
            let task = Task {
                id: TaskId::new(title),
                title: title.to_owned(),
                due: None,
                position: crate::fixture::top(&state.todo),
            };
            state.todo.push(task.clone());
            Ok(task)
        }

        fn move_card(&mut self, card: &TaskId, place: &Place) -> Result<(), SourceError> {
            let mut state = self.0.lock().expect("lock");
            state.asked.push(format!("move {card} {}", place.column));
            if state.offline {
                return Err(SourceError::Offline("down".to_owned()));
            }
            Ok(())
        }
    }

    fn start(fake: Fake, every: Option<Duration>) -> (Handle, mpsc::Receiver<Snapshot>) {
        let (out, snapshots) = mpsc::channel();
        let plan = Plan {
            account: Account {
                id: AccountId::new("fake"),
                kind: Kind::Planka,
                holds: Holds::Tasks,
                name: String::new(),
            },
            make: Box::new(move || Source::Tasks {
                source: Box::new(fake),
                names: ColumnNames::default(),
            }),
            every,
            cache: None,
            watch: None,
        };
        (spawn(plan, clock(), out).expect("a worker"), snapshots)
    }

    /// The last snapshot sent once the worker is done with what it was
    /// sent.
    fn last(handle: &Handle, snapshots: &mpsc::Receiver<Snapshot>) -> Snapshot {
        assert!(handle.flush(Duration::from_secs(10)));
        snapshots.try_iter().last().expect("a snapshot")
    }

    #[test]
    fn a_change_refused_is_kept_and_tried_again_when_the_source_answers() {
        let fake = Fake::default();
        let (handle, snapshots) = start(fake.clone(), None);
        let first = last(&handle, &snapshots);
        assert!(matches!(first.state, State::Fresh { .. }));
        fake.0.lock().expect("lock").offline = true;
        handle.send(Command::Change(
            ChangeId(1),
            Change::Add {
                title: "Un".to_owned(),
            },
        ));
        let refused = last(&handle, &snapshots);
        assert_eq!(refused.pending.len(), 1);
        assert!(matches!(
            refused.pending[0].refused,
            Some(SourceError::Offline(_))
        ));
        // Back: read again, the change goes through, the task shows.
        fake.0.lock().expect("lock").offline = false;
        handle.send(Command::Refresh);
        let after = last(&handle, &snapshots);
        assert!(after.pending.is_empty(), "{after:?}");
        assert_eq!(after.tasks.len(), 1);
        assert_eq!(
            fake.0.lock().expect("lock").asked,
            ["create Un", "create Un"]
        );
    }

    #[test]
    fn a_tick_and_its_untick_both_unsent_undo_each_other() {
        let fake = Fake::default();
        fake.0.lock().expect("lock").todo.push(Task {
            id: TaskId::new("t"),
            title: "T".to_owned(),
            due: None,
            position: 1.0,
        });
        let (handle, snapshots) = start(fake.clone(), None);
        last(&handle, &snapshots);
        fake.0.lock().expect("lock").offline = true;
        let task = TaskId::new("t");
        handle.send(Command::Change(
            ChangeId(1),
            Change::Tick { task: task.clone() },
        ));
        let ticked = last(&handle, &snapshots);
        assert_eq!(ticked.pending.len(), 1);
        handle.send(Command::Change(ChangeId(2), Change::Untick { task }));
        let unticked = last(&handle, &snapshots);
        assert!(unticked.pending.is_empty());
        // Sent once, refused; nothing more asked.
        assert_eq!(fake.0.lock().expect("lock").asked, ["move t DONE"]);
    }

    #[test]
    fn ticked_then_unticked_goes_back_where_it_was() {
        let fake = Fake::default();
        fake.0.lock().expect("lock").todo.push(Task {
            id: TaskId::new("t"),
            title: "T".to_owned(),
            due: None,
            position: 7.0,
        });
        let (handle, snapshots) = start(fake.clone(), None);
        last(&handle, &snapshots);
        let task = TaskId::new("t");
        handle.send(Command::Change(
            ChangeId(1),
            Change::Tick { task: task.clone() },
        ));
        last(&handle, &snapshots);
        handle.send(Command::Change(ChangeId(2), Change::Untick { task }));
        last(&handle, &snapshots);
        assert_eq!(
            fake.0.lock().expect("lock").asked,
            ["move t DONE", "move t TODO"]
        );
    }

    /// A calendar that counts what it is asked, its listings with a token.
    struct Counting(Arc<Mutex<Vec<&'static str>>>);

    impl CalendarSource for Counting {
        fn calendars(&mut self) -> Result<Vec<Calendar>, SourceError> {
            Ok(vec![Calendar {
                id: CalendarId::new("c"),
                account: AccountId::new("cal"),
                name: "Travail".to_owned(),
                colour: Colour::DEFAULT,
                read_only: false,
                shown: true,
            }])
        }

        fn events(&mut self, _: &CalendarId, _: Range) -> Result<Listing, SourceError> {
            self.0.lock().expect("lock").push("list");
            Ok(Listing {
                events: Vec::new(),
                token: Some(SyncToken("t1".to_owned())),
            })
        }

        fn changes(
            &mut self,
            _: &CalendarId,
            token: &SyncToken,
        ) -> Result<crate::source::Changes, SourceError> {
            self.0.lock().expect("lock").push("sync");
            assert_eq!(token.0, "t1");
            Ok(crate::source::Changes {
                token: Some(token.clone()),
                ..crate::source::Changes::default()
            })
        }
    }

    #[test]
    fn calendars_list_once_then_sync_from_their_token_and_their_cache_shows_first() {
        let dir = tempfile::tempdir().expect("a folder");
        let cache = dir.path().join("day/cal.json");
        let asked = Arc::new(Mutex::new(Vec::new()));
        let run = |asked: Arc<Mutex<Vec<&'static str>>>| {
            let (out, snapshots) = mpsc::channel();
            let plan = Plan {
                account: Account {
                    id: AccountId::new("cal"),
                    kind: Kind::Google,
                    holds: Holds::Calendars,
                    name: String::new(),
                },
                make: Box::new(move || Source::Calendars {
                    source: Box::new(Counting(asked)),
                    shown: None,
                    colours: BTreeMap::from([("travail".to_owned(), Colour([1, 2, 3]))]),
                }),
                every: None,
                cache: Some(cache.clone()),
                watch: None,
            };
            (spawn(plan, clock(), out).expect("a worker"), snapshots)
        };
        let (handle, snapshots) = run(asked.clone());
        let read = last(&handle, &snapshots);
        assert_eq!(read.calendars[0].colour, Colour([1, 2, 3]));
        // Read again, the same: nothing sent.
        handle.send(Command::Refresh);
        assert!(handle.flush(Duration::from_secs(10)));
        assert!(snapshots.try_recv().is_err());
        assert_eq!(*asked.lock().expect("lock"), ["list", "sync"]);
        assert!(cache.exists());
        // Started again: the cache's day first, then synced from its token.
        let (handle, snapshots) = run(asked.clone());
        assert!(handle.flush(Duration::from_secs(10)));
        let sent: Vec<Snapshot> = snapshots.try_iter().collect();
        assert!(matches!(sent[0].state, State::Cached { .. }), "{sent:?}");
        assert_eq!(sent[0].calendars.len(), 1);
        assert_eq!(*asked.lock().expect("lock"), ["list", "sync", "sync"]);
    }
}
