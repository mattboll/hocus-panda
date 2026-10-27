//! Tasks (#255): the local list, a folder of iCalendar VTODO files
//! (`$XDG_DATA_HOME/hocuspanda/tasks`), read and written by a thread of its
//! own, which reads the folder again after each change it writes. The
//! column always shows them ("All done for today." without any, and the
//! field to add the first); the command field, the column's field and a
//! task's menu change them; the control API and MCP list and add them.
//!
//! CalDAV task lists (#256) come with the calendars' sync (the same
//! interval and triggers, [`crate::calendar`]): their tasks show with the
//! local ones, each with its list's name and colour, but for the lists
//! hidden in the settings. A sealed cache keeps them for when the server
//! cannot be reached, and the column then says it is offline. A change to
//! one is written back by a thread of its own, PUT or DELETE on the ETag it
//! was read with: changed on the server meanwhile (412), the server's
//! version is kept and a toast says so; not written (offline), the task
//! goes back to what its server has, and a toast says that too.
//!
//! Google Tasks (#257) is the same for a Google account of GNOME Online
//! Accounts: its lists come with its calendars' sync, read with the same
//! token from Google Tasks' API, and a change is written back by the same
//! thread, with a token GOA hands out then: POST for a new task (whose id
//! Google gives, its UID standing for it until then), PATCH of the fields
//! changed, DELETE. A token without the tasks scope leaves the account's
//! tasks out, and the settings say so; nothing else changes.
//!
//! The loop changes its own copy at once, so that a tick shows at once,
//! and takes the folder as read only once the thread has written every
//! change sent before; a sync started before a change was written keeps
//! the loop's copy of that task.

use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
};

use hocus_api::{TaskInfo, TaskListInfo};
use hocus_core::config::CalendarAccount;
use hocus_platform::{
    calendar::{
        caldav::{self, Expect, Resource, TaskList, WriteError},
        goa::{self, GoogleAccount},
        gtasks,
        ics::When,
    },
    sealed::Sealed,
    secrets::Secrets,
    tasks::{self, Source, Task},
};
use hocus_shell::{
    launcher::TaskListChoice,
    tasks::{Due, DueChoice, ListMark, ROWS, TaskRow, TasksView},
};
use jiff::{Timestamp, Zoned, civil::Date};
use smithay::reexports::calloop::{LoopHandle, channel};

use crate::{App, State};

/// What the thread is asked to do.
#[derive(Debug)]
enum Command {
    Write { file: PathBuf, text: String },
    Remove(PathBuf),
}

/// The folder as read, once `done` commands were carried out.
#[derive(Debug)]
struct Read {
    done: u64,
    tasks: io::Result<Option<Vec<Task>>>,
}

/// What [`TasksView`] is made from: made again when it changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ViewKey {
    generation: u64,
    minute: (i16, i8, i8, i8, i8),
    pandas: bool,
    /// The configuration's, for the lists hidden.
    config: u64,
    offline: bool,
}

/// Where a remote task list is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Service {
    CalDav,
    Google,
}

/// A remote task list (CalDAV, Google Tasks), as last synced (or cached),
/// with what was changed here since.
#[derive(Debug, Clone)]
struct RemoteList {
    /// Its account, by id.
    account: String,
    service: Service,
    /// Its address (CalDAV), or its id (Google Tasks).
    url: String,
    name: String,
    color: Option<String>,
    tasks: Vec<Task>,
}

/// The task lists' cache: each account's lists, with their address (or
/// id), name, colour and tasks (address or id, ETag, text: iCalendar's, or
/// Google's JSON for a GOA account's).
pub(crate) type Cache = HashMap<String, Vec<CachedList>>;
type CachedList = (
    String,
    String,
    Option<String>,
    Vec<(String, Option<String>, String)>,
);

/// A task changed here, while its server has not answered, and until a
/// sync started after it answered.
#[derive(Debug)]
struct Edit {
    account: String,
    /// Writes sent, not answered.
    in_flight: usize,
    /// The sync generation when the last one was answered.
    settled: Option<u64>,
    /// The last change deleted it.
    deleted: bool,
    /// The task as its server last took it; `None` for a new one.
    before: Option<Task>,
}

/// A remote account's task lists as its sync read them, their tasks made
/// on the sync's thread.
#[derive(Debug)]
pub(crate) enum SyncedTasks {
    Lists(Vec<SyncedList>),
    /// The account does not share its tasks: a Google token without the
    /// tasks scope.
    Unshared,
    /// Not read this time: the lists known stay, and say they are offline.
    Failed(String),
}

#[derive(Debug)]
pub(crate) struct SyncedList {
    service: Service,
    url: String,
    name: String,
    color: Option<String>,
    tasks: Vec<Task>,
}

/// A CalDAV account's task lists, with their tasks; on the sync's thread.
pub(crate) fn caldav_lists(account: &str, lists: Vec<TaskList>) -> Vec<SyncedList> {
    lists
        .into_iter()
        .map(|list| SyncedList {
            service: Service::CalDav,
            tasks: list_tasks(account, &list),
            url: list.url,
            name: list.name,
            color: list.color,
        })
        .collect()
}

/// A Google account's task lists, as Google Tasks' API answered (`None`:
/// not asked, in tests without its stand-in).
pub(crate) fn google_lists(read: Option<Result<Vec<gtasks::List>, gtasks::Error>>) -> SyncedTasks {
    match read {
        None => SyncedTasks::Lists(Vec::new()),
        Some(Ok(lists)) => SyncedTasks::Lists(
            lists
                .into_iter()
                .map(|list| SyncedList {
                    service: Service::Google,
                    url: list.id,
                    name: list.name,
                    color: None,
                    tasks: list.tasks,
                })
                .collect(),
        ),
        Some(Err(gtasks::Error::Unshared(why))) => {
            tracing::debug!(why, "Google Tasks are not shared");
            SyncedTasks::Unshared
        }
        Some(Err(err)) => SyncedTasks::Failed(err.to_string()),
    }
}

/// Where a remote task is written, with what it takes to reach it.
#[derive(Debug)]
pub(crate) enum Target {
    CalDav {
        account: CalendarAccount,
        secrets: Option<Secrets>,
    },
    /// A Google account of GOA's on `bus`, Google Tasks' API at `base`.
    Google {
        bus: String,
        account: GoogleAccount,
        base: String,
    },
}

/// A change for a remote server, to the thread that writes them.
#[derive(Debug)]
enum Remote {
    Put {
        /// The task as written.
        task: Task,
        /// iCalendar's text (CalDAV), the fields changed (Google).
        text: String,
        expect: Expect,
        /// A write of it was still on its way: the ETag that one brings
        /// back is expected, not this one (CalDAV); the id that one got is
        /// its own (Google).
        chained: bool,
        target: Target,
    },
    Delete {
        task: Task,
        expect: Expect,
        chained: bool,
        target: Target,
    },
}

/// What a server answered to a change.
#[derive(Debug)]
struct Written {
    /// The task as written, or deleted.
    task: Task,
    outcome: Outcome,
}

#[derive(Debug)]
enum Outcome {
    /// Taken, with its new ETag when the server says it.
    Saved {
        etag: Option<String>,
        text: String,
    },
    Deleted,
    /// Changed on the server meanwhile: what it has now (`None`: deleted
    /// there), or why that could not be read.
    Conflict(Result<Option<Box<Task>>, String>),
    /// Not taken: unreachable, or refused.
    Failed(String),
}

#[derive(Debug, Default)]
pub struct TasksState {
    /// The local list's tasks.
    local: Vec<Task>,
    /// The local list's folder; `None` without one (a nested session).
    dir: Option<PathBuf>,
    worker: Option<mpsc::Sender<Command>>,
    /// Commands sent to the thread.
    sent: u64,
    /// Bumped at each change.
    generation: u64,
    view: Option<(ViewKey, Option<Arc<TasksView>>)>,
    /// The remote task lists, by account in the configuration's order,
    /// then GOA's.
    remote: Vec<RemoteList>,
    /// The remote tasks changed here lately, by address (CalDAV) or id
    /// (Google; their UID until Google gave them one).
    edits: HashMap<String, Edit>,
    /// The accounts whose tasks did not sync last time.
    unreachable: HashSet<String>,
    /// The accounts that do not share their tasks (#257).
    unshared: HashSet<String>,
    /// The UIDs that stood for tasks added to Google, by the ids Google
    /// gave them: what was shown with the one may come back.
    renamed: HashMap<String, String>,
    /// The thread writing to remote servers, once a first change was made.
    remote_worker: Option<mpsc::Sender<Remote>>,
    /// Its way back, until it is started.
    remote_news: Option<channel::Sender<Written>>,
    remote_cache: Option<PathBuf>,
    /// The cache was read: saving before would lose it.
    remote_cache_read: bool,
}

impl TasksState {
    pub(crate) fn set_remote_cache(&mut self, path: PathBuf) {
        self.remote_cache = Some(path);
    }
}

/// The task lists' cache, read at start; on the calendar's thread.
pub(crate) fn read_cache(path: &Path) -> Cache {
    match Sealed::new(path).read() {
        Ok(Some(bytes)) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Ok(None) => Cache::new(),
        Err(err) => {
            tracing::warn!(err, "the task lists' cache could not be read");
            Cache::new()
        }
    }
}

/// A CalDAV list's tasks, from its server's resources.
fn list_tasks(account: &str, list: &TaskList) -> Vec<Task> {
    list.items
        .iter()
        .filter_map(|r| remote_task(account, &list.url, &list.name, r))
        .collect()
}

fn remote_task(account: &str, list: &str, name: &str, r: &Resource) -> Option<Task> {
    let source = Source::CalDav {
        account: account.to_owned(),
        list: list.to_owned(),
        href: r.href.clone(),
        etag: r.etag.clone(),
    };
    tasks::parse(&r.ics, source, Some(name.to_owned()))
}

/// What names a remote task while it is changed: its address on its
/// CalDAV server, its id at Google (its UID until Google gave it one).
fn key(task: &Task) -> Option<&str> {
    match &task.source {
        Source::CalDav { href, .. } => Some(href),
        Source::Google { id, .. } => Some(id.as_deref().unwrap_or(&task.uid)),
        Source::Local { .. } => None,
    }
}

/// The list a remote task is in: its address, or its id.
fn list_of(task: &Task) -> Option<&str> {
    match &task.source {
        Source::CalDav { list, .. } | Source::Google { list, .. } => Some(list),
        Source::Local { .. } => None,
    }
}

/// What a write expects there, for a task as the loop has it.
fn expect_of(task: &Task, new: bool) -> Expect {
    match &task.source {
        _ if new => Expect::New,
        Source::CalDav {
            etag: Some(etag), ..
        } => Expect::Etag(etag.clone()),
        _ => Expect::Any,
    }
}

/// Writes the changes it is sent, one after the other, each on the ETag
/// the one before brought back when it was still on its way (CalDAV), or
/// to the id Google gave a task added here (Google Tasks).
fn write_remote(wanted: mpsc::Receiver<Remote>, news: channel::Sender<Written>) {
    let mut etags: HashMap<String, Option<String>> = HashMap::new();
    // Google's ids of the tasks added here, by their UID.
    let mut ids: HashMap<String, String> = HashMap::new();
    while let Ok(change) = wanted.recv() {
        let (mut task, text, expect, chained, target) = match change {
            Remote::Put {
                task,
                text,
                expect,
                chained,
                target,
            } => (task, Some(text), expect, chained, target),
            Remote::Delete {
                task,
                expect,
                chained,
                target,
            } => (task, None, expect, chained, target),
        };
        let outcome = match target {
            Target::CalDav { account, secrets } => {
                let Some(href) = key(&task).map(str::to_owned) else {
                    continue;
                };
                let expect = match etags.get(&href) {
                    Some(Some(etag)) if chained => Expect::Etag(etag.clone()),
                    Some(None) if chained => Expect::Any,
                    _ => expect,
                };
                let source = crate::calendar::source_of(account, secrets.as_ref());
                caldav_write(&source, &task, &href, text, &expect, &mut etags)
            }
            Target::Google { bus, account, base } => {
                let uid = task.uid.clone();
                let Source::Google { list, id, .. } = &mut task.source else {
                    continue;
                };
                if id.is_none() {
                    id.clone_from(&ids.get(&uid).cloned());
                }
                let (outcome, got) =
                    google_write(&bus, &account, &base, list, id.as_deref(), text, chained);
                if let Some(got) = got {
                    ids.insert(uid, got.clone());
                    *id = Some(got);
                }
                outcome
            }
        };
        if news.send(Written { task, outcome }).is_err() {
            return;
        }
    }
}

/// A change written to a CalDAV server at `href`: `text` put there, or
/// none, deleted.
fn caldav_write(
    source: &caldav::Source,
    task: &Task,
    href: &str,
    text: Option<String>,
    expect: &Expect,
    etags: &mut HashMap<String, Option<String>>,
) -> Outcome {
    let theirs = |r: Option<Resource>| {
        let (account, list) = match &task.source {
            Source::CalDav { account, list, .. } => (account.as_str(), list.as_str()),
            _ => ("", ""),
        };
        let name = task.list.clone().unwrap_or_default();
        r.and_then(|r| remote_task(account, list, &name, &r))
            .map(Box::new)
    };
    let Some(text) = text else {
        etags.remove(href);
        return match caldav::delete(source, href, expect) {
            Ok(()) => Outcome::Deleted,
            Err(WriteError::Conflict) => Outcome::Conflict(
                caldav::get(source, href)
                    .map(theirs)
                    .map_err(|e| e.to_string()),
            ),
            Err(err) => Outcome::Failed(err.to_string()),
        };
    };
    match caldav::put(source, href, &text, expect) {
        Ok(etag) => {
            // Not said: asked, for the next change.
            let etag = match etag {
                Some(etag) => Some(etag),
                None => caldav::get(source, href)
                    .ok()
                    .flatten()
                    .and_then(|r| r.etag),
            };
            etags.insert(href.to_owned(), etag.clone());
            Outcome::Saved { etag, text }
        }
        Err(WriteError::Conflict) => {
            etags.remove(href);
            Outcome::Conflict(
                caldav::get(source, href)
                    .map(theirs)
                    .map_err(|e| e.to_string()),
            )
        }
        Err(err) => {
            etags.remove(href);
            Outcome::Failed(err.to_string())
        }
    }
}

/// A change written to Google Tasks, to the task `id` of `list` (none: a
/// new one, added): `body`'s fields, or none, deleted; with the id Google
/// gave a task it added.
fn google_write(
    bus: &str,
    account: &GoogleAccount,
    base: &str,
    list: &str,
    id: Option<&str>,
    body: Option<String>,
    chained: bool,
) -> (Outcome, Option<String>) {
    let failed = |err: gtasks::Error| Outcome::Failed(err.to_string());
    match (body, id) {
        // Its first write adds it; a later one, when that one failed, has
        // nothing to change.
        (Some(_), None) if chained => (Outcome::Failed("it was never added".into()), None),
        (Some(body), None) => {
            match goa::with_tasks(bus, account, |token| {
                gtasks::insert(base, token, list, &body)
            }) {
                Ok(text) => {
                    let got = gtasks::id_of(&text);
                    (Outcome::Saved { etag: None, text }, got)
                }
                Err(err) => (failed(err), None),
            }
        }
        (Some(body), Some(id)) => {
            match goa::with_tasks(bus, account, |token| {
                gtasks::patch(base, token, list, id, &body)
            }) {
                Ok(Some(text)) => (Outcome::Saved { etag: None, text }, None),
                // Deleted at Google meanwhile.
                Ok(None) => (Outcome::Conflict(Ok(None)), None),
                Err(err) => (failed(err), None),
            }
        }
        (None, None) => (Outcome::Deleted, None),
        (None, Some(id)) => {
            match goa::with_tasks(bus, account, |token| gtasks::delete(base, token, list, id)) {
                Ok(()) => (Outcome::Deleted, None),
                Err(err) => (failed(err), None),
            }
        }
    }
}

/// The local list's folder: `$XDG_DATA_HOME/hocuspanda/tasks` in a session,
/// else in the state directory given (tests).
pub fn dir(is_session: bool, state_dir: Option<&Path>) -> Option<PathBuf> {
    if is_session {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .map(|d| d.join("hocuspanda").join("tasks"))
    } else {
        state_dir.map(|d| d.join("tasks"))
    }
}

pub fn start(handle: &LoopHandle<'static, App>, state: &mut State, dir: Option<PathBuf>) {
    // The remote servers' answers; the thread that writes to them starts
    // with the first change.
    let (written, answers) = channel::channel::<Written>();
    let inserted = handle.insert_source(answers, |event, _, app| {
        if let channel::Event::Msg(written) = event {
            app.timed("tasks", |app| app.state.remote_written(written));
        }
    });
    match inserted {
        Ok(_) => state.tasks.remote_news = Some(written),
        Err(err) => tracing::warn!(%err.error, "remote tasks will not be written"),
    }
    let Some(dir) = dir else {
        return;
    };
    let (read, received) = channel::channel::<Read>();
    let inserted = handle.insert_source(received, |event, _, app| {
        if let channel::Event::Msg(read) = event {
            app.timed("tasks", |app| app.state.tasks_read(read));
        }
    });
    if let Err(err) = inserted {
        tracing::warn!(%err.error, "no tasks");
        return;
    }
    let (worker, wanted) = mpsc::channel::<Command>();
    let folder = dir.clone();
    let spawned = std::thread::Builder::new()
        .name("tasks".into())
        .spawn(move || {
            let mut done = 0;
            let _ = read.send(Read {
                done,
                tasks: tasks::load(&folder),
            });
            while let Ok(command) = wanted.recv() {
                // What came meanwhile is written before reading again.
                for command in std::iter::once(command).chain(wanted.try_iter()) {
                    carry_out(command);
                    done += 1;
                }
                let read_again = Read {
                    done,
                    tasks: tasks::load(&folder),
                };
                if read.send(read_again).is_err() {
                    return;
                }
            }
        });
    match spawned {
        Ok(_) => {
            state.tasks.dir = Some(dir);
            state.tasks.worker = Some(worker);
        }
        Err(err) => tracing::warn!(%err, "the tasks will not be read"),
    }
}

fn carry_out(command: Command) {
    let (done, what) = match &command {
        Command::Write { file, text } => (tasks::write_local(file, text), "saved"),
        Command::Remove(file) => (tasks::remove_local(file), "removed"),
    };
    if let Err(err) = done {
        tracing::warn!(%err, "a task was not {what}");
    }
}

/// When `due` falls, by the user's clock: its day, and its time if it has
/// one.
/// A day, its time of day if it has one, and the moment.
type DueDay = (Date, Option<(u8, u8)>, Timestamp);

fn due_day(due: &When, now: &Zoned) -> Option<DueDay> {
    let tz = now.time_zone();
    let at = due.timestamp(tz)?;
    match due {
        When::Date(d) => Some((*d, None, at)),
        _ => {
            let local = at.to_zoned(tz.clone());
            Some((
                local.date(),
                Some((local.hour() as u8, local.minute() as u8)),
                at,
            ))
        }
    }
}

/// When a task is due, said from `today`.
fn due_label(day: Date, time: Option<(u8, u8)>, today: Date) -> Due {
    let days = (day - today).get_days();
    match (days, time) {
        (0, Some((hour, minute))) => Due::At { hour, minute },
        (0, None) => Due::Today,
        (-1, _) => Due::Yesterday,
        (1, _) => Due::Tomorrow,
        (2..=6, _) => Due::Weekday(day.weekday().to_monday_zero_offset() as u8),
        _ => Due::Day {
            day: day.day() as u8,
            month: day.month() as u8,
        },
    }
}

/// Whether a task due so is late at `now`: a date once its day is over, a
/// time once it has passed.
fn is_late(day: Date, time: Option<(u8, u8)>, at: Timestamp, now: &Zoned) -> bool {
    match time {
        Some(_) => at < now.timestamp(),
        None => day < now.date(),
    }
}

/// What the column shows of `tasks` at `now`: the late ones first, the
/// oldest first, then today's, then those due later, then those without a
/// date, the newest first; after them, those done today. Each says its
/// list as `mark` has it.
pub(crate) fn view(
    tasks: &[&Task],
    mark: impl Fn(&Task) -> Option<ListMark>,
    now: &Zoned,
    pandas: bool,
) -> TasksView {
    let today = now.date();
    let tz = now.time_zone();
    let mut open: Vec<(u8, i64, &Task, Option<Due>, bool)> = Vec::new();
    let mut done: Vec<(Timestamp, &Task)> = Vec::new();
    for &task in tasks {
        if task.done {
            if let Some(at) = task.completed
                && at.to_zoned(tz.clone()).date() == today
            {
                done.push((at, task));
            }
            continue;
        }
        match task.due.as_ref().and_then(|d| due_day(d, now)) {
            Some((day, time, at)) => {
                let late = is_late(day, time, at, now);
                let bucket = if late {
                    0
                } else if day == today {
                    1
                } else {
                    2
                };
                let label = due_label(day, time, today);
                open.push((bucket, at.as_second(), task, Some(label), late));
            }
            // The newest first.
            None => {
                let made = task.created.map_or(0, |c| c.as_second());
                open.push((3, -made, task, None, false));
            }
        }
    }
    open.sort_by(|a, b| (a.0, a.1, &a.2.summary).cmp(&(b.0, b.1, &b.2.summary)));
    done.sort_by_key(|(at, _)| *at);
    let late = open.iter().filter(|o| o.4).count();
    let row = |task: &Task, due: Option<Due>, late: bool| TaskRow {
        id: task.uid.clone(),
        text: task.summary.clone(),
        due,
        late,
        done: task.done,
        list: mark(task),
    };
    let rows = open
        .iter()
        .map(|(_, _, task, due, late)| row(task, *due, *late))
        .chain(done.iter().map(|(_, task)| row(task, None, false)))
        .take(ROWS)
        .collect();
    TasksView {
        rows,
        open: open.len(),
        late,
        done_today: done.len(),
        pandas,
        offline: false,
    }
}

/// A due date as the API writes it: `2026-10-21`, or `2026-10-21T15:00`
/// in the user's time.
fn due_text(due: &When, now: &Zoned) -> Option<String> {
    match due {
        When::Date(d) => Some(d.strftime("%Y-%m-%d").to_string()),
        other => {
            let at = other.timestamp(now.time_zone())?;
            Some(
                at.to_zoned(now.time_zone().clone())
                    .strftime("%Y-%m-%dT%H:%M")
                    .to_string(),
            )
        }
    }
}

/// A due date as the API takes it: `today`, `tomorrow`, `2026-10-21`, or
/// `2026-10-21T15:00` in the user's time.
pub(crate) fn parse_due(text: &str, today: Date) -> Result<When, String> {
    let text = text.trim();
    match text {
        "today" => return Ok(When::Date(today)),
        "tomorrow" => return Ok(When::Date(today.tomorrow().unwrap_or(today))),
        _ => {}
    }
    if let Ok(d) = Date::strptime("%Y-%m-%d", text) {
        return Ok(When::Date(d));
    }
    for format in ["%Y-%m-%dT%H:%M", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(dt) = jiff::civil::DateTime::strptime(format, text) {
            return Ok(When::Local(dt, None));
        }
    }
    Err(format!(
        "{text:?} is not a due date: today, tomorrow, 2026-10-21 or 2026-10-21T15:00"
    ))
}

/// Where a task is added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListTarget<'a> {
    /// The local list: this list of it, or its default one.
    Local(Option<&'a str>),
    /// The remote task list at this address (CalDAV) or id (Google).
    Remote(&'a str),
}

impl State {
    /// Now, by the user's clock, in the user's time zone.
    fn tasks_now(&self) -> Zoned {
        let tz = self.wall.zone();
        self.wall_civil()
            .to_zoned(tz)
            .unwrap_or_else(|_| Zoned::now())
    }

    fn tasks_read(&mut self, read: Read) {
        // Changes sent since are not in it: the next read has them.
        if read.done != self.tasks.sent {
            return;
        }
        match read.tasks {
            Ok(Some(tasks)) => self.tasks.local = tasks,
            // No folder yet: none, until the first is added.
            Ok(None) => self.tasks.local.clear(),
            Err(err) => {
                tracing::warn!(%err, "the tasks do not read; those known stay");
                return;
            }
        }
        self.tasks_changed();
    }

    fn tasks_changed(&mut self) {
        self.tasks.generation += 1;
        self.needs_redraw = true;
    }

    fn tasks_send(&mut self, command: Command) {
        if let Some(worker) = &self.tasks.worker {
            if worker.send(command).is_ok() {
                self.tasks.sent += 1;
            } else {
                tracing::warn!("the tasks' thread is gone: the change is not saved");
            }
        }
    }

    /// Writes `task` as it is now; `before`, a remote task as it was
    /// before this change.
    fn tasks_write(&mut self, task: &Task, before: Option<Task>) {
        match &task.source {
            Source::Local { file } => {
                let text = task.to_ics(self.tasks_now().timestamp());
                self.tasks_send(Command::Write {
                    file: file.clone(),
                    text,
                });
            }
            Source::CalDav { .. } | Source::Google { .. } => {
                self.remote_send(task, before, false, false)
            }
        }
    }

    /// Whether the list `name` of `account` is shown with the tasks.
    fn list_shown(&self, account: &str, name: &str) -> bool {
        self.config
            .calendars
            .iter()
            .find(|a| a.id() == account)
            .is_none_or(|a| a.shows_list(name))
    }

    /// Every task shown: the local list's, then those of the remote lists
    /// not hidden.
    fn shown_tasks(&self) -> Vec<&Task> {
        let mut out: Vec<&Task> = self.tasks.local.iter().collect();
        for list in &self.tasks.remote {
            if self.list_shown(&list.account, &list.name) {
                out.extend(list.tasks.iter());
            }
        }
        out
    }

    /// The list a task says it is in, but the local default one.
    fn list_mark(&self, task: &Task) -> Option<ListMark> {
        match &task.source {
            Source::Local { .. } => task.list.as_ref().map(|name| ListMark {
                name: name.clone(),
                color: None,
            }),
            Source::CalDav { account, list, .. } | Source::Google { account, list, .. } => {
                let found = self
                    .tasks
                    .remote
                    .iter()
                    .find(|l| l.url == *list && l.account == *account)?;
                Some(ListMark {
                    name: found.name.clone(),
                    color: found
                        .color
                        .as_deref()
                        .and_then(hocus_ui::Color::hex)
                        .map(|c| c.with_alpha(255)),
                })
            }
        }
    }

    /// Some remote tasks are those of a sync that did not reach their
    /// server.
    fn tasks_offline(&self) -> bool {
        self.tasks.remote.iter().any(|l| {
            self.calendar_unreachable(&l.account) || self.tasks.unreachable.contains(&l.account)
        })
    }

    /// The account of this id does not share its tasks (#257).
    pub(crate) fn tasks_unshared(&self, account: &str) -> bool {
        self.tasks.unshared.contains(account)
    }

    /// After each dispatch: the column's view of the tasks, made again
    /// only when they, the minute, the pandas, the configuration or the
    /// servers' reach changed.
    pub fn refresh_tasks(&mut self) {
        let civil = self.wall_civil();
        let key = ViewKey {
            generation: self.tasks.generation,
            minute: (
                civil.year(),
                civil.month(),
                civil.day(),
                civil.hour(),
                civil.minute(),
            ),
            pandas: self.pandas_on(),
            config: self.config_generation,
            offline: self.tasks_offline(),
        };
        if self.tasks.view.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        // Shown wherever tasks can be kept, none yet included: "All done
        // for today." and the field to add the first.
        let shown = (self.tasks.dir.is_some() || !self.tasks.remote.is_empty()).then(|| {
            let now = self.tasks_now();
            let tasks = self.shown_tasks();
            let mut shown = view(&tasks, |t| self.list_mark(t), &now, key.pandas);
            shown.offline = key.offline;
            shown
        });
        // The minute turning changes nothing most of the time: the same
        // view stays, nothing is drawn again, nothing more is held.
        let same = match (&self.tasks.view, &shown) {
            (Some((_, Some(old))), Some(new)) => **old == *new,
            (Some((_, None)), None) => true,
            _ => false,
        };
        if same {
            if let Some((k, _)) = &mut self.tasks.view {
                *k = key;
            }
            return;
        }
        self.tasks.view = Some((key, shown.map(Arc::new)));
    }

    /// What the column shows of the tasks; `None` where none can be kept
    /// (no data directory, no task list).
    pub(crate) fn tasks_view(&self) -> Option<Arc<TasksView>> {
        self.tasks.view.as_ref().and_then(|(_, v)| v.clone())
    }

    /// Adds a task to `list`.
    pub(crate) fn add_task(
        &mut self,
        text: &str,
        due: Option<When>,
        list: ListTarget<'_>,
    ) -> Result<Task, String> {
        if text.trim().is_empty() {
            return Err("a task needs a text".into());
        }
        let now = self.tasks_now().timestamp();
        let task = match list {
            ListTarget::Local(list) => {
                let Some(dir) = self.tasks.dir.clone() else {
                    return Err("there is no task list: no data directory".into());
                };
                let task =
                    Task::new_local(&dir, text, due, list, now).map_err(|e| e.to_string())?;
                self.tasks_write(&task, None);
                self.tasks.local.push(task.clone());
                task
            }
            ListTarget::Remote(url) => {
                let Some(at) = self.tasks.remote.iter().position(|l| l.url == url) else {
                    return Err(format!("there is no task list at {url}"));
                };
                let list = &self.tasks.remote[at];
                let task = match list.service {
                    Service::CalDav => {
                        Task::new_caldav(&list.account, &list.url, &list.name, text, due, now)
                    }
                    Service::Google => {
                        Task::new_google(&list.account, &list.url, &list.name, text, due, now)
                    }
                }
                .map_err(|e| e.to_string())?;
                self.remote_send(&task, None, false, true);
                if let Some(list) = self.tasks.remote.get_mut(at) {
                    list.tasks.push(task.clone());
                }
                task
            }
        };
        tracing::info!("a task added");
        self.tasks_changed();
        Ok(task)
    }

    fn task_mut(&mut self, id: &str) -> Option<&mut Task> {
        let TasksState {
            local,
            remote,
            renamed,
            ..
        } = &mut self.tasks;
        let id = renamed.get(id).map_or(id, String::as_str);
        local
            .iter_mut()
            .chain(remote.iter_mut().flat_map(|l| l.tasks.iter_mut()))
            .find(|t| t.uid == id)
    }

    fn task(&self, id: &str) -> Option<&Task> {
        let id = self.tasks.renamed.get(id).map_or(id, String::as_str);
        self.tasks
            .local
            .iter()
            .chain(self.tasks.remote.iter().flat_map(|l| l.tasks.iter()))
            .find(|t| t.uid == id)
    }

    /// Changes the task of this id with `change`, and writes it.
    fn change_task(&mut self, id: &str, change: impl FnOnce(&mut Task, &Zoned)) {
        let now = self.tasks_now();
        let Some(task) = self.task_mut(id) else {
            tracing::debug!(id, "no such task");
            return;
        };
        let before = task.clone();
        change(task, &now);
        let task = task.clone();
        self.tasks_write(&task, Some(before));
        self.tasks_changed();
    }

    /// Ticks the task, or unticks it.
    pub(crate) fn toggle_task(&mut self, id: &str) {
        self.change_task(id, |task, now| task.set_done(!task.done, now.timestamp()));
    }

    pub(crate) fn edit_task(&mut self, id: &str, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        self.change_task(id, |task, _| task.set_summary(text));
    }

    pub(crate) fn set_task_due(&mut self, id: &str, due: DueChoice) {
        self.change_task(id, |task, now| {
            let today = now.date();
            task.due = match due {
                DueChoice::Today => Some(When::Date(today)),
                DueChoice::Tomorrow => Some(When::Date(today.tomorrow().unwrap_or(today))),
                DueChoice::None => None,
            };
        });
    }

    pub(crate) fn delete_task(&mut self, id: &str) {
        let id = self
            .tasks
            .renamed
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.to_owned());
        let id = id.as_str();
        if let Some(at) = self.tasks.local.iter().position(|t| t.uid == id) {
            let task = self.tasks.local.remove(at);
            if let Source::Local { file } = task.source {
                self.tasks_send(Command::Remove(file));
            }
            self.tasks_changed();
            return;
        }
        let found = self.tasks.remote.iter_mut().find_map(|l| {
            let at = l.tasks.iter().position(|t| t.uid == id)?;
            Some(l.tasks.remove(at))
        });
        if let Some(task) = found {
            self.remote_send(&task, Some(task.clone()), true, false);
            self.tasks_changed();
        }
    }

    /// The lists a task can be added to, for the launcher: the local one
    /// first, its own lists, then the remote lists shown.
    fn task_list_choices(&self) -> Vec<TaskListChoice> {
        let lang = self.shell_lang();
        let here = lang.on_this_computer().to_owned();
        let mut out = Vec::new();
        if self.tasks.dir.is_some() {
            out.push(TaskListChoice {
                key: "local".into(),
                name: lang.local_tasks().into(),
                place: here.clone(),
                color: None,
            });
            let mut names: Vec<&String> = self
                .tasks
                .local
                .iter()
                .filter_map(|t| t.list.as_ref())
                .collect();
            names.sort();
            names.dedup();
            out.extend(names.into_iter().map(|name| TaskListChoice {
                key: format!("local:{name}"),
                name: name.clone(),
                place: here.clone(),
                color: None,
            }));
        }
        for list in &self.tasks.remote {
            if !self.list_shown(&list.account, &list.name) {
                continue;
            }
            let place = self.calendar_name(&list.account).unwrap_or_default();
            let service = match list.service {
                Service::CalDav => "caldav",
                Service::Google => "google",
            };
            out.push(TaskListChoice {
                key: format!("{service}:{}", list.url),
                name: list.name.clone(),
                place,
                color: list
                    .color
                    .as_deref()
                    .and_then(hocus_ui::Color::hex)
                    .map(|c| c.with_alpha(255)),
            });
        }
        out
    }

    /// The launcher, to add a task or edit this one.
    pub(crate) fn open_task_card(&mut self, editing: Option<&str>) {
        let editing = editing.and_then(|id| {
            let task = self.task(id)?;
            Some((task.uid.clone(), task.summary.clone()))
        });
        let lists = self.task_list_choices();
        self.open_launcher();
        if let Some(panel) = self.launcher.as_mut() {
            panel.launcher.set_task_lists(lists);
            panel.launcher.open_task(editing);
        }
    }

    /// What the launcher's task card gives back: the list chosen by its
    /// key.
    pub(crate) fn save_task(&mut self, id: Option<String>, text: &str, list: Option<&str>) {
        if let Some(id) = id {
            self.edit_task(&id, text);
            return;
        }
        let target = match list {
            Some(key) if key.starts_with("caldav:") => ListTarget::Remote(&key["caldav:".len()..]),
            Some(key) if key.starts_with("google:") => ListTarget::Remote(&key["google:".len()..]),
            Some(key) if key.starts_with("local:") => {
                ListTarget::Local(Some(&key["local:".len()..]))
            }
            _ => ListTarget::Local(None),
        };
        if let Err(err) = self.add_task(text, None, target) {
            tracing::warn!(err, "the task was not added");
        }
    }

    /// "todo …" in the command field.
    pub(crate) fn add_task_typed(&mut self, text: &str) {
        if let Err(err) = self.add_task(text, None, ListTarget::Local(None)) {
            tracing::warn!(err, "the task was not added");
        }
    }

    /// The notes' to-dos (#79) become tasks: those of a notes file written
    /// before the tasks were, or typed as `- [ ] …` in the notes. Whether
    /// some moved out of the notes.
    pub(crate) fn tasks_from_notes(&mut self, todos: Vec<(bool, String)>) -> bool {
        if self.tasks.dir.is_none() || todos.is_empty() {
            return false;
        }
        let now = self.tasks_now().timestamp();
        for (done, text) in todos {
            match self.add_task(&text, None, ListTarget::Local(None)) {
                Ok(task) if done => {
                    // Done before it was a task: when is not known.
                    let id = task.uid.clone();
                    self.change_task(&id, |task, _| {
                        task.set_done(true, now);
                        task.completed = None;
                    });
                }
                Ok(_) => {}
                Err(err) => tracing::warn!(err, "a to-do of the notes was not made a task"),
            }
        }
        true
    }

    /// The tasks, for the control API: those not done first, as the column
    /// orders them, then those done; those of hidden lists left out.
    pub fn tasks_info(&self) -> Vec<TaskInfo> {
        let now = self.tasks_now();
        let info = |task: &Task| {
            let late = !task.done
                && task
                    .due
                    .as_ref()
                    .and_then(|d| due_day(d, &now))
                    .is_some_and(|(day, time, at)| is_late(day, time, at, &now));
            TaskInfo {
                id: task.uid.clone(),
                text: task.summary.clone(),
                done: task.done,
                due: task.due.as_ref().and_then(|d| due_text(d, &now)),
                late,
                list: task.list.clone(),
                source: match task.source {
                    Source::Local { .. } => "local".into(),
                    Source::CalDav { .. } => "caldav".into(),
                    Source::Google { .. } => "google".into(),
                },
            }
        };
        let tasks = self.shown_tasks();
        let shown = view(&tasks, |_| None, &now, false);
        let mut out: Vec<TaskInfo> = Vec::with_capacity(tasks.len());
        // The column's order for those it shows, then the others.
        for row in &shown.rows {
            if let Some(task) = tasks.iter().find(|t| t.uid == row.id) {
                out.push(info(task));
            }
        }
        let mut rest: Vec<&Task> = tasks
            .iter()
            .copied()
            .filter(|t| !shown.rows.iter().any(|r| r.id == t.uid))
            .collect();
        rest.sort_by_key(|t| {
            (
                t.done,
                t.due.as_ref().and_then(|d| d.timestamp(now.time_zone())),
            )
        });
        out.extend(rest.into_iter().map(info));
        out
    }

    /// A task added through the control API: by an agent (#42) only when
    /// its access is to act, and kept in the agents' journal.
    pub fn add_task_asked(
        &mut self,
        connection: hocus_api::ConnectionId,
        text: &str,
        due: Option<&str>,
        list: Option<&str>,
    ) -> Result<TaskInfo, hocus_api::ApiError> {
        use hocus_api::{ApiError, ErrorCode};
        let agent = self.api.agent(connection).map(str::to_owned);
        if let Some(agent) = &agent {
            let acts = self
                .config
                .agents
                .mcp_agent(agent)
                .is_some_and(|a| a.access == hocus_core::config::AgentAccess::Act);
            if !acts {
                let refusal = ApiError::new(
                    ErrorCode::PermissionDenied,
                    format!(
                        "agent {agent:?} may only read: adding a task needs access = \"act\" \
                         in [[agents.mcp]]"
                    ),
                );
                self.journal_task(agent, text, Some(&refusal));
                return Err(refusal);
            }
        }
        let added = self
            .add_task_info(text, due, list)
            .map_err(|why| ApiError::new(ErrorCode::BadRequest, why));
        if let Some(agent) = &agent {
            self.journal_task(agent, text, added.as_ref().err());
        }
        added
    }

    /// A task added through the control API, in the user's words: to the
    /// remote list shown of that name, else to the local list's.
    pub fn add_task_info(
        &mut self,
        text: &str,
        due: Option<&str>,
        list: Option<&str>,
    ) -> Result<TaskInfo, String> {
        let today = self.tasks_now().date();
        let due = due
            .filter(|d| !d.trim().is_empty())
            .map(|d| parse_due(d, today))
            .transpose()?;
        let list = list.map(str::trim).filter(|l| !l.is_empty());
        let remote = list.and_then(|name| {
            self.tasks
                .remote
                .iter()
                .find(|l| l.name == name && self.list_shown(&l.account, &l.name))
                .map(|l| l.url.clone())
        });
        let target = match &remote {
            Some(url) => ListTarget::Remote(url),
            None => ListTarget::Local(list),
        };
        let task = self.add_task(text, due, target)?;
        let id = task.uid;
        self.tasks_info()
            .into_iter()
            .find(|t| t.id == id)
            .ok_or_else(|| "the task went at once".to_owned())
    }

    /// An account's task lists, for the settings.
    pub(crate) fn task_lists_info(&self, account: &str) -> Vec<TaskListInfo> {
        self.tasks
            .remote
            .iter()
            .filter(|l| l.account == account)
            .map(|l| TaskListInfo {
                name: l.name.clone(),
                color: l.color.clone(),
                open: l.tasks.iter().filter(|t| !t.done).count(),
                hidden: !self.list_shown(account, &l.name),
            })
            .collect()
    }

    /// Sends a change of a remote task to the thread that writes them,
    /// started with the first: `task` as it is now (or as it was, deleted),
    /// `before` as it was before this change (`None` for a new one).
    fn remote_send(&mut self, task: &Task, before: Option<Task>, delete: bool, new: bool) {
        let Some(account) = task.source.account().map(str::to_owned) else {
            return;
        };
        let Some(at) = key(task).map(str::to_owned) else {
            return;
        };
        let Some(target) = self.task_target(&account) else {
            tracing::warn!("the account of a task is gone: the change is not written");
            return;
        };
        if self.tasks.remote_worker.is_none() {
            let Some(news) = self.tasks.remote_news.take() else {
                return;
            };
            let (worker, wanted) = mpsc::channel::<Remote>();
            let spawned = std::thread::Builder::new()
                .name("task writes".into())
                .spawn(move || write_remote(wanted, news));
            match spawned {
                Ok(_) => self.tasks.remote_worker = Some(worker),
                Err(err) => {
                    tracing::warn!(%err, "remote tasks will not be written");
                    return;
                }
            }
        }
        let text = match &task.source {
            // Only what this change changed: what changed elsewhere stays.
            Source::Google { .. } => gtasks::body(task, before.as_ref()),
            _ => task.to_ics(self.tasks_now().timestamp()),
        };
        let edit = self.tasks.edits.entry(at).or_insert_with(|| Edit {
            account,
            in_flight: 0,
            settled: None,
            deleted: false,
            before: None,
        });
        // What the server has now is the baseline, once all it was sent
        // was answered.
        if edit.in_flight == 0 {
            edit.before = before;
        }
        let chained = edit.in_flight > 0;
        edit.in_flight += 1;
        edit.deleted = delete;
        let expect = expect_of(task, new);
        let change = if delete {
            Remote::Delete {
                task: task.clone(),
                expect,
                chained,
                target,
            }
        } else {
            Remote::Put {
                task: task.clone(),
                text,
                expect,
                chained,
                target,
            }
        };
        let sent = self
            .tasks
            .remote_worker
            .as_ref()
            .is_some_and(|w| w.send(change).is_ok());
        if !sent {
            tracing::warn!("the thread writing tasks is gone: the change is not written");
            edit.in_flight -= 1;
        }
    }

    /// A task added to Google Tasks got its id: it, and its change, go by
    /// it from now on; a sync that already brought it does not show it
    /// twice.
    fn google_task_named(&mut self, uid: &str, id: &str) {
        if let Some(edit) = self.tasks.edits.remove(uid) {
            match self.tasks.edits.get_mut(id) {
                Some(known) => {
                    known.in_flight += edit.in_flight;
                    known.settled = known.settled.max(edit.settled);
                }
                None => {
                    self.tasks.edits.insert(id.to_owned(), edit);
                }
            }
        }
        for list in &mut self.tasks.remote {
            if !list.tasks.iter().any(|t| key(t) == Some(uid)) {
                continue;
            }
            list.tasks.retain(|t| key(t) != Some(id));
            if let Some(task) = list.tasks.iter_mut().find(|t| key(t) == Some(uid)) {
                if let Source::Google { id: known, .. } = &mut task.source {
                    *known = Some(id.to_owned());
                }
                task.uid = id.to_owned();
            }
        }
        self.tasks.renamed.insert(uid.to_owned(), id.to_owned());
        self.tasks_changed();
    }

    /// A remote server's answer to a change.
    fn remote_written(&mut self, mut written: Written) {
        if let Source::Google { id: Some(id), .. } = &written.task.source
            && *id != written.task.uid
        {
            let (uid, id) = (written.task.uid.clone(), id.clone());
            self.google_task_named(&uid, &id);
            written.task.uid = id;
        }
        let Some(at) = key(&written.task).map(str::to_owned) else {
            return;
        };
        let Some(list) = list_of(&written.task).map(str::to_owned) else {
            return;
        };
        let generation = self.calendar_generation();
        if let Some(edit) = self.tasks.edits.get_mut(&at) {
            edit.in_flight = edit.in_flight.saturating_sub(1);
            edit.settled = Some(generation);
        }
        let lang = self.shell_lang();
        match written.outcome {
            Outcome::Saved { etag, text } => {
                let mut taken = written.task;
                taken.raw = text;
                if let Source::CalDav { etag: e, .. } = &mut taken.source {
                    e.clone_from(&etag);
                }
                if let Some(task) = self.remote_task_mut(&at) {
                    task.raw.clone_from(&taken.raw);
                    if let Source::CalDav { etag: e, .. } = &mut task.source {
                        e.clone_from(&etag);
                    }
                }
                if let Some(edit) = self.tasks.edits.get_mut(&at) {
                    edit.before = Some(taken);
                }
                self.save_remote_cache();
            }
            Outcome::Deleted => {
                if let Some(edit) = self.tasks.edits.get_mut(&at) {
                    edit.before = None;
                }
                self.save_remote_cache();
            }
            Outcome::Conflict(Ok(theirs)) => {
                let theirs = theirs.map(|t| *t);
                tracing::info!("a task changed on its server meanwhile: its version is kept");
                if let Some(edit) = self.tasks.edits.get_mut(&at) {
                    edit.before.clone_from(&theirs);
                    edit.deleted = theirs.is_none();
                }
                self.replace_remote(&list, &at, theirs);
                self.shell_toast(
                    &lang.task_conflict(&written.task.summary),
                    lang.task_conflict_body(),
                );
            }
            Outcome::Conflict(Err(err)) | Outcome::Failed(err) => {
                tracing::warn!(err, "a task was not written to its server: it goes back");
                let before = self.tasks.edits.get_mut(&at).and_then(|e| {
                    e.deleted = e.before.is_none();
                    e.before.clone()
                });
                self.replace_remote(&list, &at, before);
                self.shell_toast(
                    &lang.task_not_saved(&written.task.summary),
                    lang.task_not_saved_body(),
                );
            }
        }
    }

    fn remote_task_mut(&mut self, at: &str) -> Option<&mut Task> {
        self.tasks
            .remote
            .iter_mut()
            .flat_map(|l| l.tasks.iter_mut())
            .find(|t| key(t) == Some(at))
    }

    /// The task at `at` of the list at `list` is `with` from now on, or
    /// none.
    fn replace_remote(&mut self, list: &str, at: &str, with: Option<Task>) {
        if let Some(found) = self.tasks.remote.iter_mut().find(|l| l.url == list) {
            found.tasks.retain(|t| key(t) != Some(at));
            found.tasks.extend(with);
        }
        self.save_remote_cache();
        self.tasks_changed();
    }

    /// An account's task lists as its sync of `generation` read them; the
    /// tasks changed here since it started stay as the loop has them.
    pub(crate) fn remote_tasks_synced(
        &mut self,
        account: &str,
        generation: u64,
        synced: SyncedTasks,
    ) {
        let lists = match synced {
            SyncedTasks::Lists(lists) => {
                self.tasks.unreachable.remove(account);
                if self.tasks.unshared.remove(account) {
                    tracing::info!("an account shares its tasks again");
                }
                lists
            }
            SyncedTasks::Unshared => {
                self.tasks.unreachable.remove(account);
                if self.tasks.unshared.insert(account.to_owned()) {
                    tracing::info!(
                        "a Google account does not share its tasks: its token lacks {}",
                        gtasks::SCOPE
                    );
                }
                Vec::new()
            }
            SyncedTasks::Failed(err) => {
                tracing::warn!(err, "an account's tasks did not sync; those known stay");
                self.tasks.unreachable.insert(account.to_owned());
                self.remote_tasks_changed();
                return;
            }
        };
        self.tasks.edits.retain(|_, e| {
            e.account != account || e.in_flight > 0 || e.settled.is_some_and(|g| g >= generation)
        });
        let (old, others): (Vec<RemoteList>, Vec<RemoteList>) =
            std::mem::take(&mut self.tasks.remote)
                .into_iter()
                .partition(|l| l.account == account);
        self.tasks.remote = others;
        let edits = &self.tasks.edits;
        for list in lists {
            let mut tasks: Vec<Task> = list
                .tasks
                .into_iter()
                .filter(|t| key(t).is_none_or(|k| !edits.contains_key(k)))
                .collect();
            let changed_here = old
                .iter()
                .filter(|l| l.url == list.url)
                .flat_map(|l| l.tasks.iter())
                .filter(|t| {
                    key(t)
                        .and_then(|k| edits.get(k))
                        .is_some_and(|e| !e.deleted)
                });
            tasks.extend(changed_here.cloned());
            self.tasks.remote.push(RemoteList {
                account: account.to_owned(),
                service: list.service,
                url: list.url,
                name: list.name,
                color: list.color,
                tasks,
            });
        }
        self.sort_remote();
        self.save_remote_cache();
        self.tasks_changed();
    }

    /// The task lists of the cache read at start: of the accounts `kept`
    /// says, not synced yet.
    pub(crate) fn remote_tasks_cached(&mut self, cache: Cache, kept: impl Fn(&str) -> bool) {
        let mut dropped = false;
        for (account, lists) in cache {
            if !kept(&account) {
                dropped = true;
                continue;
            }
            if self.tasks.remote.iter().any(|l| l.account == account) {
                continue;
            }
            let service = if crate::calendar::is_goa(&account) {
                Service::Google
            } else {
                Service::CalDav
            };
            for (url, name, color, items) in lists {
                let tasks = match service {
                    Service::CalDav => list_tasks(
                        &account,
                        &TaskList {
                            url: url.clone(),
                            name: name.clone(),
                            color: color.clone(),
                            items: items
                                .into_iter()
                                .map(|(href, etag, ics)| Resource { href, etag, ics })
                                .collect(),
                        },
                    ),
                    Service::Google => items
                        .iter()
                        .filter_map(|(_, _, text)| gtasks::parse(text, &account, &url, &name))
                        .collect(),
                };
                self.tasks.remote.push(RemoteList {
                    account: account.clone(),
                    service,
                    url,
                    name,
                    color,
                    tasks,
                });
            }
        }
        self.sort_remote();
        self.tasks.remote_cache_read = true;
        if dropped {
            self.save_remote_cache();
        }
        self.tasks_changed();
    }

    /// The accounts not among `ids` take their task lists with them; GOA's
    /// stay while `keep_goa` (it has not answered yet).
    pub(crate) fn forget_remote_tasks(&mut self, ids: &[String], keep_goa: bool) {
        let kept =
            |id: &str| ids.iter().any(|i| i == id) || (keep_goa && crate::calendar::is_goa(id));
        let before = self.tasks.remote.len();
        self.tasks.remote.retain(|l| kept(&l.account));
        self.tasks.edits.retain(|_, e| kept(&e.account));
        self.tasks.unreachable.retain(|a| kept(a));
        self.tasks.unshared.retain(|a| kept(a));
        if self.tasks.remote.len() != before {
            self.save_remote_cache();
            self.tasks_changed();
        }
    }

    /// A sync did not reach its server: the column says so.
    pub(crate) fn remote_tasks_changed(&mut self) {
        if !self.tasks.remote.is_empty() {
            self.tasks_changed();
        }
    }

    /// In the configuration's order of their accounts, GOA's after them, a
    /// sort that keeps each account's lists as its server listed them.
    fn sort_remote(&mut self) {
        let order: Vec<String> = self.config.calendars.iter().map(|a| a.id()).collect();
        self.tasks.remote.sort_by_key(|l| {
            order
                .iter()
                .position(|id| *id == l.account)
                .unwrap_or(usize::MAX)
        });
    }

    /// The task lists to the sealed cache, off the loop.
    fn save_remote_cache(&self) {
        let Some(path) = self
            .tasks
            .remote_cache
            .clone()
            .filter(|_| self.tasks.remote_cache_read)
        else {
            return;
        };
        let mut cache = Cache::new();
        for list in &self.tasks.remote {
            let items = list
                .tasks
                .iter()
                .filter(|t| !t.raw.is_empty())
                .filter_map(|t| match &t.source {
                    Source::CalDav { href, etag, .. } => {
                        Some((href.clone(), etag.clone(), t.raw.clone()))
                    }
                    Source::Google { id: Some(id), .. } => Some((id.clone(), None, t.raw.clone())),
                    _ => None,
                })
                .collect();
            cache.entry(list.account.clone()).or_default().push((
                list.url.clone(),
                list.name.clone(),
                list.color.clone(),
                items,
            ));
        }
        let spawned = std::thread::Builder::new()
            .name("tasks cache".into())
            .spawn(move || {
                let saved = serde_json::to_vec(&cache)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| Sealed::new(&path).write(&bytes));
                if let Err(err) = saved {
                    tracing::warn!(err, "the task lists' cache was not saved");
                }
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "could not start a thread");
        }
    }
}

#[cfg(test)]
mod tests {
    use jiff::civil::datetime;

    use super::*;

    fn now() -> Zoned {
        datetime(2026, 10, 21, 14, 37, 0, 0)
            .to_zoned(jiff::tz::TimeZone::UTC)
            .expect("a time")
    }

    fn task(uid: &str, due: Option<When>, done: Option<Timestamp>) -> Task {
        Task {
            uid: uid.into(),
            summary: uid.into(),
            done: done.is_some(),
            completed: done,
            due,
            created: None,
            list: None,
            source: Source::Local {
                file: PathBuf::from(format!("/tasks/{uid}.ics")),
            },
            raw: String::new(),
        }
    }

    #[test]
    fn late_ones_come_first_then_today_then_the_rest_then_done_today() {
        let at = |h, m| Some(When::Local(datetime(2026, 10, 21, h, m, 0, 0), None));
        let tasks = [
            task("undated", None, None),
            task("at three", at(15, 0), None),
            task("yesterday", Some(When::Date(date(2026, 10, 20))), None),
            task("done", None, Some(now().timestamp())),
            task("at noon", at(12, 0), None),
            task("old done", None, Some(Timestamp::UNIX_EPOCH)),
        ];
        let refs: Vec<&Task> = tasks.iter().collect();
        let v = view(&refs, |_| None, &now(), true);
        let order: Vec<_> = v.rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(order, ["yesterday", "at noon", "at three", "undated"]);
        assert_eq!((v.open, v.late, v.done_today), (4, 2, 1));
        assert_eq!(v.rows[0].due, Some(Due::Yesterday));
        assert!(v.rows[0].late && v.rows[1].late && !v.rows[2].late);
        assert_eq!(
            v.rows[2].due,
            Some(Due::At {
                hour: 15,
                minute: 0
            })
        );
        // With room, the one done today follows.
        let v = view(&refs[..4], |_| None, &now(), true);
        let order: Vec<_> = v.rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(order, ["yesterday", "at three", "undated", "done"]);
    }

    #[test]
    fn dates_say_tomorrow_a_weekday_then_the_day() {
        let today = date(2026, 10, 21);
        assert_eq!(due_label(date(2026, 10, 22), None, today), Due::Tomorrow);
        assert_eq!(due_label(date(2026, 10, 23), None, today), Due::Weekday(4));
        assert_eq!(
            due_label(date(2026, 10, 30), None, today),
            Due::Day { day: 30, month: 10 }
        );
        assert_eq!(
            due_label(date(2026, 10, 12), None, today),
            Due::Day { day: 12, month: 10 }
        );
    }

    #[test]
    fn the_api_takes_dates_and_times() {
        let today = date(2026, 10, 21);
        assert_eq!(
            parse_due("tomorrow", today),
            Ok(When::Date(date(2026, 10, 22)))
        );
        assert_eq!(
            parse_due("2026-10-30", today),
            Ok(When::Date(date(2026, 10, 30)))
        );
        assert_eq!(
            parse_due("2026-10-21T15:00", today),
            Ok(When::Local(datetime(2026, 10, 21, 15, 0, 0, 0), None))
        );
        assert!(parse_due("soon", today).is_err());
    }

    use jiff::civil::date;
}
