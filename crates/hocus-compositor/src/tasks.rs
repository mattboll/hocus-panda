//! Tasks (#255): the local list, a folder of iCalendar VTODO files
//! (`$XDG_DATA_HOME/hocuspanda/tasks`), read and written by a thread of its
//! own, which reads the folder again after each change it writes. The
//! column shows them once the folder is there; the command field, the
//! column's field and a task's menu change them; the control API and MCP
//! list and add them. CalDAV task lists (#256) and Google Tasks (#257) are
//! sources to come, of the same [`Task`].
//!
//! The loop changes its own copy at once, so that a tick shows at once,
//! and takes the folder as read only once the thread has written every
//! change sent before.

use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
};

use hocus_api::TaskInfo;
use hocus_platform::{
    calendar::ics::When,
    tasks::{self, Source, Task},
};
use hocus_shell::tasks::{Due, DueChoice, ROWS, TaskRow, TasksView};
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
}

#[derive(Debug, Default)]
pub struct TasksState {
    pub tasks: Vec<Task>,
    /// The list's folder is there: the column shows the section.
    pub exists: bool,
    /// The local list's folder; `None` without one (a nested session).
    dir: Option<PathBuf>,
    worker: Option<mpsc::Sender<Command>>,
    /// Commands sent to the thread.
    sent: u64,
    /// Bumped at each change.
    generation: u64,
    view: Option<(ViewKey, Option<Arc<TasksView>>)>,
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
/// date, the newest first; after them, those done today.
pub(crate) fn view(tasks: &[Task], now: &Zoned, pandas: bool) -> TasksView {
    let today = now.date();
    let tz = now.time_zone();
    let mut open: Vec<(u8, i64, &Task, Option<Due>, bool)> = Vec::new();
    let mut done: Vec<(Timestamp, &Task)> = Vec::new();
    for task in tasks {
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
            Ok(Some(tasks)) => {
                self.tasks.tasks = tasks;
                self.tasks.exists = true;
            }
            Ok(None) => {
                self.tasks.tasks.clear();
                self.tasks.exists = false;
            }
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

    /// Writes `task` as it is now.
    fn tasks_write(&mut self, task: &Task) {
        let text = task.to_ics(self.tasks_now().timestamp());
        match &task.source {
            Source::Local { file } => self.tasks_send(Command::Write {
                file: file.clone(),
                text,
            }),
        }
    }

    /// After each dispatch: the column's view of the tasks, made again
    /// only when they, the minute or the pandas changed.
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
        };
        if self.tasks.view.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        let shown = self.tasks.exists.then(|| {
            let now = self.tasks_now();
            Arc::new(view(&self.tasks.tasks, &now, key.pandas))
        });
        // The minute turning changes nothing most of the time: the same
        // view stays, and nothing is drawn again.
        let same = matches!(
            (&self.tasks.view, &shown),
            (Some((_, Some(old))), Some(new)) if old == new
        ) || matches!((&self.tasks.view, &shown), (Some((_, None)), None));
        if same {
            if let Some((k, _)) = &mut self.tasks.view {
                *k = key;
            }
            return;
        }
        self.tasks.view = Some((key, shown));
    }

    /// What the column shows of the tasks; `None` without a list.
    pub(crate) fn tasks_view(&self) -> Option<Arc<TasksView>> {
        self.tasks.view.as_ref().and_then(|(_, v)| v.clone())
    }

    /// Adds a task to the local list.
    pub(crate) fn add_task(
        &mut self,
        text: &str,
        due: Option<When>,
        list: Option<&str>,
    ) -> Result<Task, String> {
        let Some(dir) = self.tasks.dir.clone() else {
            return Err("there is no task list: no data directory".into());
        };
        if text.trim().is_empty() {
            return Err("a task needs a text".into());
        }
        let now = self.tasks_now().timestamp();
        let task = Task::new_local(&dir, text, due, list, now).map_err(|e| e.to_string())?;
        tracing::info!("a task added");
        self.tasks_write(&task);
        self.tasks.tasks.push(task.clone());
        self.tasks.exists = true;
        self.tasks_changed();
        Ok(task)
    }

    fn task_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.tasks.tasks.iter_mut().find(|t| t.uid == id)
    }

    /// Changes the task of this id with `change`, and writes it.
    fn change_task(&mut self, id: &str, change: impl FnOnce(&mut Task, &Zoned)) {
        let now = self.tasks_now();
        let Some(task) = self.task_mut(id) else {
            tracing::debug!(id, "no such task");
            return;
        };
        change(task, &now);
        let task = task.clone();
        self.tasks_write(&task);
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
        let Some(at) = self.tasks.tasks.iter().position(|t| t.uid == id) else {
            return;
        };
        let task = self.tasks.tasks.remove(at);
        match task.source {
            Source::Local { file } => self.tasks_send(Command::Remove(file)),
        }
        self.tasks_changed();
    }

    /// The launcher, to add a task or edit this one.
    pub(crate) fn open_task_card(&mut self, editing: Option<&str>) {
        let editing = editing.and_then(|id| {
            let task = self.tasks.tasks.iter().find(|t| t.uid == id)?;
            Some((task.uid.clone(), task.summary.clone()))
        });
        self.open_launcher();
        if let Some(panel) = self.launcher.as_mut() {
            panel.launcher.open_task(editing);
        }
    }

    /// What the launcher's task card gives back.
    pub(crate) fn save_task(&mut self, id: Option<String>, text: &str) {
        match id {
            Some(id) => self.edit_task(&id, text),
            None => {
                if let Err(err) = self.add_task(text, None, None) {
                    tracing::warn!(err, "the task was not added");
                }
            }
        }
    }

    /// "todo …" in the command field.
    pub(crate) fn add_task_typed(&mut self, text: &str) {
        if let Err(err) = self.add_task(text, None, None) {
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
            match self.add_task(&text, None, None) {
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
    /// orders them, then those done.
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
                },
            }
        };
        let shown = view(&self.tasks.tasks, &now, false);
        let mut out: Vec<TaskInfo> = Vec::with_capacity(self.tasks.tasks.len());
        // The column's order for those it shows, then the others.
        for row in &shown.rows {
            if let Some(task) = self.tasks.tasks.iter().find(|t| t.uid == row.id) {
                out.push(info(task));
            }
        }
        let mut rest: Vec<&Task> = self
            .tasks
            .tasks
            .iter()
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

    /// A task added through the control API, in the user's words.
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
        let task = self.add_task(text, due, list)?;
        let id = task.uid;
        self.tasks_info()
            .into_iter()
            .find(|t| t.id == id)
            .ok_or_else(|| "the task went at once".to_owned())
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
        let tasks = vec![
            task("undated", None, None),
            task("at three", at(15, 0), None),
            task("yesterday", Some(When::Date(date(2026, 10, 20))), None),
            task("done", None, Some(now().timestamp())),
            task("at noon", at(12, 0), None),
            task("old done", None, Some(Timestamp::UNIX_EPOCH)),
        ];
        let v = view(&tasks, &now(), true);
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
        let v = view(&tasks[..4], &now(), true);
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
