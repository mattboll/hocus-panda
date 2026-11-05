//! An account's worker: a thread of its own that reads its source and
//! sends owned [`Snapshot`]s back, only when they change. It reads at
//! start (its cache first, if it has one), then on its schedule, when
//! asked ([`Command::Refresh`]: on request, on resume, when a fixture's
//! file changes), and at once after a change. Between, it sleeps on its
//! channel: no wakeup until its schedule or a command.
//!
//! A calendars' account also reads the months outside its window it is
//! asked for ([`Command::Months`], as the calendar turns to them: Day.5b),
//! keeps them with its cache, and syncs them with the window: a month read
//! is not read again, unless the window was listed whole since (once a
//! day, a sync token expired, a source without tokens), and then only when
//! it is asked for again.
//!
//! The bar's changes are queued, sent at once in their order, and kept
//! when the source refuses them, each with why: they are tried again each
//! time the source answers again. A card that is no more drops its change.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError, mpsc},
    time::{Duration, Instant},
};

use jiff::{Timestamp, civil::Date};
use serde::{Deserialize, Serialize};

use crate::{
    Clock,
    config::Over,
    model::{
        Calendar, CalendarId, Event, EventId, Place, Task, TaskBoard, TaskId, sort_events,
        sort_tasks,
    },
    snapshot::{Account, Change, ChangeId, Holds, MonthState, Pending, Snapshot, State, Update},
    source::{CalendarSource, ColumnNames, Columns, KanbanSource, Range, SourceError, SyncToken},
};

/// What an account reads.
pub(crate) enum Source {
    Calendars {
        source: Box<dyn CalendarSource>,
        /// The configuration's `shown`, `featured` and `colors`.
        over: Over,
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
    /// These months (any of their days), outside the window, read if they
    /// are not already (a calendars' account's; the others ignore it).
    Months(Vec<Date>),
    Change(ChangeId, Change),
    /// Answered once what came before is done.
    Flush(mpsc::Sender<()>),
    /// The account is no more (removed, switched off): the worker ends,
    /// its cache forgotten if `forget`.
    Stop {
        forget: bool,
    },
}

/// A running worker.
#[derive(Debug, Clone)]
pub(crate) struct Handle {
    pub account: crate::model::AccountId,
    pub holds: Holds,
    commands: mpsc::Sender<Command>,
    /// Whether its snapshots still go to the day: cleared, under its lock,
    /// as its account is said gone, so that none comes after.
    alive: Arc<Mutex<bool>>,
}

impl Handle {
    pub fn send(&self, command: Command) -> bool {
        self.commands.send(command).is_ok()
    }

    /// The account said gone to `out`, nothing of its worker after that,
    /// and the worker ended once it is done with what it does.
    pub fn stop(&self, out: &mpsc::Sender<Update>, forget: bool) {
        {
            let mut alive = self.alive.lock().unwrap_or_else(PoisonError::into_inner);
            *alive = false;
            let _ = out.send(Update::Gone(self.account.clone()));
        }
        self.send(Command::Stop { forget });
    }

    /// Waits up to `within` for what was sent before to be done.
    pub fn flush(&self, within: Duration) -> bool {
        let (done, wait) = mpsc::channel();
        self.send(Command::Flush(done)) && wait.recv_timeout(within).is_ok()
    }
}

/// Starts `plan`'s worker, sending its snapshots to `out`.
pub(crate) fn spawn(plan: Plan, clock: Clock, out: mpsc::Sender<Update>) -> Option<Handle> {
    let (commands, incoming) = mpsc::channel();
    let account = plan.account.id.clone();
    let holds = plan.account.holds;
    let alive = Arc::new(Mutex::new(true));
    let still = alive.clone();
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
                alive: still,
                sent: None,
                cached: None,
                events: Vec::new(),
                far: BTreeMap::new(),
                failed: BTreeMap::new(),
                tokens: BTreeMap::new(),
                window: None,
                columns: None,
                places: HashMap::new(),
            };
            if let Some(Stopped { forget: true }) = worker.run(&incoming, every) {
                worker.forget_cache();
            }
        });
    match started {
        Ok(_) => Some(Handle {
            account,
            holds,
            commands,
            alive,
        }),
        Err(err) => {
            tracing::warn!(%account, %err, "no thread for an account of the day");
            None
        }
    }
}

/// Why a worker ended: its account stopped.
struct Stopped {
    forget: bool,
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
    /// The months read outside the window.
    #[serde(default)]
    far: Vec<Far>,
}

/// A month outside the window, as read (Day.5b).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Far {
    /// Its first day.
    month: Date,
    /// When it was read (the wall clock): the oldest are forgotten past
    /// [`FAR_KEPT`].
    read: Timestamp,
    /// The window was listed whole since, so changes to it may have gone
    /// unseen: read again the next time it is asked for (its events shown
    /// meanwhile).
    stale: bool,
    /// The calendars shown when it was read.
    calendars: Vec<CalendarId>,
    events: Vec<Event>,
}

/// How many months outside the window an account keeps.
const FAR_KEPT: usize = 24;

struct Worker {
    source: Source,
    snapshot: Snapshot,
    cache: Option<PathBuf>,
    clock: Clock,
    out: mpsc::Sender<Update>,
    alive: Arc<Mutex<bool>>,
    sent: Option<Snapshot>,
    /// What the cache holds, so that it is written only when that changes.
    cached: Option<Data>,
    /// The window's events; the snapshot's are these and the far months'.
    events: Vec<Event>,
    /// The months read outside the window, by their first day.
    far: BTreeMap<Date, Far>,
    /// The months outside the window the source did not give, and why.
    failed: BTreeMap<Date, SourceError>,
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

    /// Until its account stops (`Some`) or the day ends (`None`).
    fn run(
        &mut self,
        incoming: &mpsc::Receiver<Command>,
        every: Option<Duration>,
    ) -> Option<Stopped> {
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
                        Err(mpsc::RecvTimeoutError::Disconnected) => return None,
                    }
                }
                None => match incoming.recv() {
                    Ok(command) => Some(command),
                    Err(_) => return None,
                },
            };
            tracing::trace!(account = self.id(), "the day's worker wakes");
            // The schedule's time, or what was asked meanwhile, together.
            let mut fetch = first.is_none();
            let mut changes = false;
            let mut months = BTreeSet::new();
            let mut flushes = Vec::new();
            for command in first.into_iter().chain(incoming.try_iter()) {
                match command {
                    Command::Refresh => fetch = true,
                    Command::Months(asked) => {
                        months.extend(asked.iter().map(|m| m.first_of_month()));
                    }
                    Command::Change(id, change) => {
                        self.queue(id, change);
                        changes = true;
                    }
                    Command::Flush(done) => flushes.push(done),
                    Command::Stop { forget } => return Some(Stopped { forget }),
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
            if !months.is_empty() {
                self.read_months(months);
            }
            self.send();
            for done in flushes {
                let _ = done.send(());
            }
        }
    }

    /// The snapshot to the shell, if it changed since the last sent and
    /// its account is not said gone.
    fn send(&mut self) {
        if self.sent.as_ref() == Some(&self.snapshot) {
            return;
        }
        let alive = self.alive.lock().unwrap_or_else(PoisonError::into_inner);
        if !*alive {
            return;
        }
        self.sent = Some(self.snapshot.clone());
        let _ = self
            .out
            .send(Update::Snapshot(Box::new(self.snapshot.clone())));
    }

    /// Its cache removed: the account is no more here.
    fn forget_cache(&self) {
        let Some(file) = &self.cache else {
            return;
        };
        match std::fs::remove_file(file) {
            Ok(()) => tracing::debug!(account = self.id(), "the day's cache forgotten"),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                tracing::warn!(file = %file.display(), %err, "the day's cache is not removed")
            }
        }
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
            Source::Calendars { source, over } => read_calendars(
                source.as_mut(),
                over,
                &self.clock,
                Fetched {
                    events: &self.events,
                    tokens: &self.tokens,
                    window: self.window,
                },
            )
            .map(|read| {
                self.snapshot.calendars = read.calendars;
                self.events = read.events;
                self.tokens = read.tokens;
                self.window = Some(read.window);
                self.follow_far(read.whole, &read.changed, &read.removed);
                self.compose();
            }),
            Source::Tasks { source, names } => {
                read_tasks(source.as_mut(), names).map(|(board, columns, tasks)| {
                    self.snapshot.board = Some(board);
                    self.snapshot.tasks = tasks;
                    self.columns = Some(columns);
                    if self.snapshot.login.is_none() {
                        self.snapshot.login = source.login();
                    }
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
                let before = self.fetched();
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

    /// When the source was last read, if ever.
    fn fetched(&self) -> Option<Timestamp> {
        match &self.snapshot.state {
            State::Cached { fetched } | State::Fresh { fetched } => Some(*fetched),
            State::Failed { fetched, .. } => *fetched,
            State::Loading => None,
        }
    }

    // --- Months outside the window --------------------------------------------------

    /// The far months after the window was read: those it now holds
    /// forgotten; the others stale if it was listed whole, else with what
    /// its syncs said changed.
    fn follow_far(&mut self, whole: bool, changed: &[Event], removed: &[(CalendarId, EventId)]) {
        let now = self.clock.now();
        let zone = now.time_zone().clone();
        let window = Range::window(&now);
        let outside = |month: &Date| !window.covers(&Range::month(*month, &zone));
        self.far.retain(|month, _| outside(month));
        self.failed.retain(|month, _| outside(month));
        for far in self.far.values_mut() {
            far.stale |= whole;
            let month = Range::month(far.month, &zone);
            far.events.retain(|e| {
                let same = |c: &Event| c.calendar == e.calendar && c.id == e.id;
                !changed.iter().any(same)
                    && !removed
                        .iter()
                        .any(|(calendar, id)| *calendar == e.calendar && *id == e.id)
            });
            far.events.extend(
                changed
                    .iter()
                    .filter(|e| e.overlaps(month.from, month.to, &zone))
                    .cloned(),
            );
        }
    }

    /// The snapshot's events (the window's and the far months', those of
    /// hidden calendars left out) and its months.
    fn compose(&mut self) {
        let zone = self.clock.now().time_zone().clone();
        let shown: HashSet<&CalendarId> = self
            .snapshot
            .calendars
            .iter()
            .filter(|c| c.shown)
            .map(|c| &c.id)
            .collect();
        let mut events = self.events.clone();
        let mut seen: HashSet<(&CalendarId, &EventId)> =
            self.events.iter().map(|e| (&e.calendar, &e.id)).collect();
        for event in self.far.values().flat_map(|f| &f.events) {
            if shown.contains(&event.calendar) && seen.insert((&event.calendar, &event.id)) {
                events.push(event.clone());
            }
        }
        sort_events(&mut events, &zone);
        self.snapshot.events = events;
        let mut months: BTreeMap<Date, MonthState> = self
            .failed
            .iter()
            .map(|(month, error)| (*month, MonthState::Failed(error.clone())))
            .collect();
        months.extend(self.far.keys().map(|month| (*month, MonthState::Loaded)));
        self.snapshot.months = months;
    }

    /// The months `asked` (their first days) read, unless the window holds
    /// them or they are read already: listed for every shown calendar, a
    /// range for each run of months that follow each other. Seen loading
    /// first; a month the source does not give is failed, unless it was
    /// read before (kept as it was).
    fn read_months(&mut self, asked: BTreeSet<Date>) {
        if !matches!(self.source, Source::Calendars { .. }) {
            return;
        }
        let now = self.clock.now();
        let zone = now.time_zone().clone();
        let window = Range::window(&now);
        let shown: Vec<CalendarId> = self
            .snapshot
            .calendars
            .iter()
            .filter(|c| c.shown)
            .map(|c| c.id.clone())
            .collect();
        let wanted: Vec<Date> = asked
            .into_iter()
            .filter(|month| !window.covers(&Range::month(*month, &zone)))
            .filter(|month| {
                self.far
                    .get(month)
                    .is_none_or(|far| far.stale || far.calendars != shown)
            })
            .collect();
        if wanted.is_empty() {
            return;
        }
        let said: Vec<String> = wanted
            .iter()
            .map(|m| m.strftime("%Y-%m").to_string())
            .collect();
        tracing::debug!(
            account = self.id(),
            months = %said.join(" "),
            "the day's account reads months outside its window"
        );
        // Its calendars never read: nothing to list them with.
        if self.snapshot.calendars.is_empty()
            && let State::Failed { error, .. } = &self.snapshot.state
        {
            let error = error.clone();
            for month in wanted {
                self.failed.insert(month, error.clone());
            }
            self.compose();
            return;
        }
        for month in &wanted {
            if !self.far.contains_key(month) {
                self.snapshot.months.insert(*month, MonthState::Loading);
            }
        }
        self.send();
        let read = now.timestamp();
        for run in runs(&wanted) {
            let (Some(first), Some(last)) = (run.first(), run.last()) else {
                continue;
            };
            let range = Range {
                from: Range::month(*first, &zone).from,
                to: Range::month(*last, &zone).to,
            };
            let Source::Calendars { source, .. } = &mut self.source else {
                return;
            };
            let mut listed = Vec::new();
            let mut error = None;
            for calendar in &shown {
                match source.events(calendar, range) {
                    Ok(listing) => listed.extend(listing.events.into_iter().map(|mut e| {
                        e.calendar = calendar.clone();
                        e
                    })),
                    Err(e) => {
                        error = Some(e);
                        break;
                    }
                }
            }
            match error {
                None => {
                    for month in run {
                        let within = Range::month(*month, &zone);
                        self.failed.remove(month);
                        self.far.insert(
                            *month,
                            Far {
                                month: *month,
                                read,
                                stale: false,
                                calendars: shown.clone(),
                                events: listed
                                    .iter()
                                    .filter(|e| e.overlaps(within.from, within.to, &zone))
                                    .cloned()
                                    .collect(),
                            },
                        );
                    }
                }
                Some(error) => {
                    tracing::info!(account = self.id(), %error, "months outside the window are not read");
                    for month in run {
                        if !self.far.contains_key(month) {
                            self.failed.insert(*month, error.clone());
                        }
                    }
                }
            }
        }
        while self.far.len() > FAR_KEPT {
            let Some(oldest) = self.far.values().min_by_key(|f| f.read).map(|f| f.month) else {
                break;
            };
            self.far.remove(&oldest);
        }
        self.compose();
        self.write_cache(self.fetched().unwrap_or(read));
    }

    fn data(&self) -> Data {
        Data {
            calendars: self.snapshot.calendars.clone(),
            events: self.events.clone(),
            board: self.snapshot.board.clone(),
            tasks: self.snapshot.tasks.clone(),
            tokens: self.tokens.clone(),
            window: self.window,
            far: self.far.values().cloned().collect(),
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
                // The configuration as it is now, which may have changed
                // since the cache was written.
                if let Source::Calendars { over, .. } = &self.source {
                    for calendar in &mut self.snapshot.calendars {
                        over.apply(calendar);
                    }
                }
                self.events = cached.data.events.clone();
                self.far = cached
                    .data
                    .far
                    .iter()
                    .map(|far| (far.month, far.clone()))
                    .collect();
                self.snapshot.board = cached.data.board.clone();
                self.snapshot.tasks = cached.data.tasks.clone();
                self.tokens = cached.data.tokens.clone();
                self.window = cached.data.window;
                self.cached = Some(cached.data);
                self.compose();
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

/// What a read of the calendars gives.
struct CalendarsRead {
    calendars: Vec<Calendar>,
    /// The window's events, in order.
    events: Vec<Event>,
    tokens: BTreeMap<CalendarId, SyncToken>,
    /// The day the window was read around.
    window: Date,
    /// A calendar was listed whole (not synced from its token).
    whole: bool,
    /// What the syncs said changed, and removed, for the far months.
    changed: Vec<Event>,
    removed: Vec<(CalendarId, EventId)>,
}

/// The calendars, the configuration's names and colours over the
/// account's, and their events in the window: listed whole once a day (the
/// window moves), else synced from their tokens.
fn read_calendars(
    source: &mut dyn CalendarSource,
    over: &Over,
    clock: &Clock,
    had: Fetched<'_>,
) -> Result<CalendarsRead, SourceError> {
    let now = clock.now();
    let zone = now.time_zone().clone();
    let window = Range::window(&now);
    let mut calendars = source.calendars()?;
    for calendar in &mut calendars {
        over.apply(calendar);
    }
    let whole = had.window != Some(now.date());
    let mut events = Vec::new();
    let mut tokens = BTreeMap::new();
    let (mut listed_whole, mut changed, mut removed) = (false, Vec::new(), Vec::new());
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
                    changed.extend(changes.changed.iter().cloned().map(|mut e| {
                        e.calendar = calendar.id.clone();
                        e
                    }));
                    removed.extend(
                        changes
                            .removed
                            .iter()
                            .map(|id| (calendar.id.clone(), id.clone())),
                    );
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
                listed_whole = true;
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
    Ok(CalendarsRead {
        calendars,
        events,
        tokens,
        window: now.date(),
        whole: listed_whole,
        changed,
        removed,
    })
}

/// `months` (in order) in runs of months that follow each other.
fn runs(months: &[Date]) -> Vec<&[Date]> {
    let mut runs = Vec::new();
    let mut start = 0;
    for i in 1..=months.len() {
        let follows = months.get(i).is_some_and(|month| {
            months[i - 1]
                .checked_add(jiff::Span::new().months(1))
                .is_ok_and(|next| next == *month)
        });
        if !follows {
            runs.push(&months[start..i]);
            start = i;
        }
    }
    runs
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
        model::{AccountId, BoardId, Colour, Column, ColumnId},
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

    /// The snapshots `updates` holds now.
    fn snapshots(updates: &mpsc::Receiver<Update>) -> Vec<Snapshot> {
        updates
            .try_iter()
            .filter_map(|u| match u {
                Update::Snapshot(s) => Some(*s),
                Update::Gone(_) => None,
            })
            .collect()
    }

    fn start(fake: Fake, every: Option<Duration>) -> (Handle, mpsc::Receiver<Update>) {
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
    fn last(handle: &Handle, updates: &mpsc::Receiver<Update>) -> Snapshot {
        assert!(handle.flush(Duration::from_secs(10)));
        snapshots(updates).pop().expect("a snapshot")
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
                featured: true,
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
                    over: Over {
                        colours: BTreeMap::from([("travail".to_owned(), Colour([1, 2, 3]))]),
                        ..Over::default()
                    },
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
        let sent: Vec<Snapshot> = super::tests::snapshots(&snapshots);
        assert!(matches!(sent[0].state, State::Cached { .. }), "{sent:?}");
        assert_eq!(sent[0].calendars.len(), 1);
        assert_eq!(*asked.lock().expect("lock"), ["list", "sync", "sync"]);
    }

    /// A calendar with one event a month, from 2026 to 2029, that says
    /// the ranges it lists; its listings with a token.
    struct Monthly(Arc<Mutex<Vec<Range>>>);

    impl CalendarSource for Monthly {
        fn calendars(&mut self) -> Result<Vec<Calendar>, SourceError> {
            Counting(Arc::default()).calendars()
        }

        fn events(&mut self, _: &CalendarId, range: Range) -> Result<Listing, SourceError> {
            self.0.lock().expect("lock").push(range);
            let zone = jiff::tz::TimeZone::UTC;
            let events = (2026..2030)
                .flat_map(|year| (1..=12).map(move |month| jiff::civil::date(year, month, 12)))
                .map(|day| Event {
                    id: crate::model::EventId::new(day.to_string()),
                    calendar: CalendarId::new("c"),
                    title: format!("Comité {day}"),
                    when: crate::model::When::AllDay {
                        first: day,
                        last: day,
                    },
                    status: crate::model::Status::Confirmed,
                    answer: crate::model::Answer::Accepted,
                    free_busy: crate::model::FreeBusy::Busy,
                    place: None,
                    video: None,
                    page: None,
                    description: None,
                    alarms: Vec::new(),
                })
                .filter(|e| e.overlaps(range.from, range.to, &zone))
                .collect();
            Ok(Listing {
                events,
                token: Some(SyncToken("t1".to_owned())),
            })
        }

        fn changes(
            &mut self,
            _: &CalendarId,
            token: &SyncToken,
        ) -> Result<crate::source::Changes, SourceError> {
            Ok(crate::source::Changes {
                token: Some(token.clone()),
                ..crate::source::Changes::default()
            })
        }
    }

    #[test]
    fn months_outside_the_window_are_read_once_kept_in_the_cache_and_shown_first() {
        let dir = tempfile::tempdir().expect("a folder");
        let cache = dir.path().join("day/cal.json");
        let ranges = Arc::new(Mutex::new(Vec::new()));
        let run = |ranges: Arc<Mutex<Vec<Range>>>| {
            let (out, snapshots) = mpsc::channel();
            let plan = Plan {
                account: Account {
                    id: AccountId::new("cal"),
                    kind: Kind::Google,
                    holds: Holds::Calendars,
                    name: String::new(),
                },
                make: Box::new(move || Source::Calendars {
                    source: Box::new(Monthly(ranges)),
                    over: Over::default(),
                }),
                every: None,
                cache: Some(cache.clone()),
                watch: None,
            };
            (spawn(plan, clock(), out).expect("a worker"), snapshots)
        };
        let utc = jiff::tz::TimeZone::UTC;
        let month = |y, m| jiff::civil::date(y, m, 1);
        let titles = |s: &Snapshot| -> Vec<String> {
            s.events
                .iter()
                .map(|e| e.title.clone())
                .filter(|t| t.contains("2027-12") || t.contains("2028"))
                .collect()
        };
        let (handle, snapshots) = run(ranges.clone());
        let first = last(&handle, &snapshots);
        // The window only: up to 4 November 2027.
        assert!(titles(&first).is_empty(), "{:?}", titles(&first));
        assert_eq!(ranges.lock().expect("lock").len(), 1);
        // Asked for December 2027 to February 2028 (and a month of the
        // window, which is not read): one range, seen loading, then read.
        handle.send(Command::Months(vec![
            jiff::civil::date(2027, 12, 27),
            month(2028, 1),
            month(2028, 2),
            month(2027, 3),
        ]));
        assert!(handle.flush(Duration::from_secs(10)));
        let sent = super::tests::snapshots(&snapshots);
        assert_eq!(
            sent.first().map(|s| s.months.clone()),
            Some(BTreeMap::from([
                (month(2027, 12), MonthState::Loading),
                (month(2028, 1), MonthState::Loading),
                (month(2028, 2), MonthState::Loading),
            ]))
        );
        let read = sent.last().expect("a snapshot");
        assert!(read.months.values().all(|m| *m == MonthState::Loaded));
        assert_eq!(
            titles(read),
            [
                "Comité 2027-12-12",
                "Comité 2028-01-12",
                "Comité 2028-02-12"
            ]
        );
        assert_eq!(
            ranges.lock().expect("lock")[1],
            Range {
                from: Range::month(month(2027, 12), &utc).from,
                to: Range::month(month(2028, 2), &utc).to,
            }
        );
        // Asked again, and read again on schedule (synced): nothing listed.
        handle.send(Command::Months(vec![month(2028, 1)]));
        handle.send(Command::Refresh);
        assert!(handle.flush(Duration::from_secs(10)));
        assert_eq!(ranges.lock().expect("lock").len(), 2);
        // Started again: the cache's months at once, not read again.
        drop(handle);
        let (handle, snapshots) = run(ranges.clone());
        assert!(handle.flush(Duration::from_secs(10)));
        let sent = super::tests::snapshots(&snapshots);
        assert!(matches!(sent[0].state, State::Cached { .. }), "{sent:?}");
        assert_eq!(titles(&sent[0]).len(), 3);
        handle.send(Command::Months(vec![month(2028, 1)]));
        assert!(handle.flush(Duration::from_secs(10)));
        assert_eq!(ranges.lock().expect("lock").len(), 2);
    }
}
