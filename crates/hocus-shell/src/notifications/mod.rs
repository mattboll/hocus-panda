//! Notifications (5.3, 5.4): the desktop's server,
//! `org.freedesktop.Notifications` on the session bus the compositor names
//! ([`server`]), their bubbles beside the bar ([`stack`] decides, [`draw`]
//! draws), their history ([`history`], its panel in
//! [`crate::popover::history`]), do not disturb ([`dnd`]) and the bell
//! that opens the history ([`bell`], one of the bar's footer's buttons).
//!
//! Each application's level (#899) is the stack's, the senders counted
//! for the Settings [`apps`]'.
//!
//! The bell keeps what still needs the user (#923): opening an
//! application folds its notifications ([`app_focus`]), agents read and
//! sort it through the compositor ([`ask`], [`undo`]), and every
//! notification served is written in the [`journal`] by the history's
//! thread, which answers an agent's search there too.
//!
//! The history is kept 7 days in `$XDG_STATE_HOME/hocuspanda/notifications`
//! and do not disturb for the session, in its runtime directory: read back
//! as the shell starts and written as they change, on threads of their own,
//! the last change written before the shell ends ([`flush`]); only where
//! notifications are served, never by a nested desktop beside the live
//! session's.
//!
//! The server runs on a thread of its own and hands each notification,
//! owned, to [`input`]; so do the bubbles' surfaces with what the pointer
//! does, and the shell with the bars and the focused window's screen. The
//! [`effects`] stream runs the stack on what comes and on its deadlines,
//! and carries out what it says: layer-shell actions (iced runs them
//! without drawing the other surfaces), lines to the compositor (a
//! bubble's `wake`, `closing`, `raise`), and the signals, back to the
//! server's thread. Nothing here goes through iced's messages, which would
//! draw every surface again.

pub mod apps;
pub mod bell;
pub mod bubble;
pub mod conversation;
pub mod dnd;
pub mod draw;
pub mod history;
pub mod journal;
pub mod note;
mod server;
pub mod stack;

use std::{
    path::PathBuf,
    sync::{Condvar, Mutex, PoisonError, mpsc},
    time::{Duration, Instant},
};

use hocus_link::{Form, ToCompositor};
use iced::{Element, Length, Rectangle, widget::canvas, window};
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings, OutputOption,
};
pub use stack::Input;
use stack::{Effect, Stack};

use self::{dnd::Dnd, history::Record};
use crate::clock::time::Wall;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::Message;

/// The stack, and how the shell draws.
#[derive(Debug)]
pub struct Center {
    pub stack: Stack,
    /// The shell draws on the CPU: the bubbles are their cards alone.
    pub card: bool,
}

static CENTER: Mutex<Option<Center>> = Mutex::new(None);
static INPUTS: Mutex<Option<UnboundedSender<Come>>> = Mutex::new(None);
static TAKEN: Mutex<Option<UnboundedReceiver<Come>>> = Mutex::new(None);

/// What the stack's stream takes, in the order it comes.
enum Come {
    Input(Input),
    /// Told once all that came before is handled, and what it changed
    /// written ([`flush`]).
    Flush(mpsc::Sender<()>),
}

/// `with` on the stack.
pub fn with<T>(with: impl FnOnce(&mut Center) -> T) -> T {
    let mut center = CENTER.lock().unwrap_or_else(PoisonError::into_inner);
    with(center.get_or_insert_with(|| Center {
        stack: Stack::new(false),
        card: false,
    }))
}

/// The wall clock the history reads: the system's time zone, and the
/// clock's shift for recordings (`HOCUS_CLOCK_AT`).
static WALL: Mutex<Option<Wall>> = Mutex::new(None);

/// The wall clock now.
pub fn wall() -> jiff::Zoned {
    let wall = WALL.lock().unwrap_or_else(PoisonError::into_inner).clone();
    match wall {
        Some(wall) => wall.now(),
        None => jiff::Zoned::now(),
    }
}

/// The wall clock now, milliseconds since the epoch.
pub fn wall_ms() -> i64 {
    wall().timestamp().as_millisecond()
}

/// The wall clock moved on by `by`, the system's clock untouched (tests:
/// `day slept`).
pub fn shift_by(by: jiff::SignedDuration) {
    if let Some(wall) = WALL.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
        wall.shift = wall.shift.saturating_add(by);
    }
}

/// The system's time zone changed.
pub fn set_zone(zone: jiff::tz::TimeZone) {
    if let Some(wall) = WALL.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
        wall.zone = zone;
    }
}

/// Starts the server, if the compositor named a bus, and reads back the
/// history and do not disturb; `software`: the shell draws on the CPU.
pub fn start(software: bool) {
    let zone = crate::clock::time::system_zone();
    let shift = std::env::var("HOCUS_CLOCK_AT")
        .ok()
        .and_then(|at| Wall::shift_to(&at, &zone))
        .unwrap_or_default();
    *WALL.lock().unwrap_or_else(PoisonError::into_inner) = Some(Wall { zone, shift });
    with(|c| {
        c.stack = Stack::new(software);
        c.card = software;
    });
    let (sender, receiver) = unbounded_channel();
    *INPUTS.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);
    *TAKEN.lock().unwrap_or_else(PoisonError::into_inner) = Some(receiver);
    // The history and do not disturb are kept only where notifications
    // are served: a nested or headless desktop without a bus of its own
    // never reads or writes the live session's.
    if server::start() {
        keep();
    } else {
        input(Input::Restored {
            notes: Vec::new(),
            dnd: Dnd::Off,
            journal: 0,
        });
    }
}

/// The wall clock's time zone.
pub fn zone() -> jiff::tz::TimeZone {
    WALL.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map_or_else(jiff::tz::TimeZone::system, |w| w.zone.clone())
}

/// A window took the user's focus (#923): its application's notifications
/// fold.
pub fn app_focus(focus: hocus_link::AppFocus) {
    input(Input::AppFocus(focus));
}

/// An agent's ask of the bell (#923), answered to the compositor.
pub fn ask(token: u64, ask: hocus_link::NotesAsk) {
    input(Input::Ask { token, ask });
}

/// « Annuler » of an agent's pins or tidying.
pub fn undo(befores: Vec<hocus_link::NoteBefore>) {
    input(Input::Undo(befores));
}

/// Whether server id `id` is one of the desktop's own notifications (a
/// capture, a meeting's reminder, a drive's, a developer's program let go
/// of): not served, never folded.
pub fn is_own(id: u32) -> bool {
    id >= DESKTOPS_OWN_FROM
}

/// The screen is shared (5.11), or no longer is: bubbles are held
/// meanwhile, then summed up.
pub fn sharing(on: bool) {
    input(Input::Sharing(on));
}

/// The session locked, or unlocked (7.1): new bubbles wait meanwhile.
pub fn locked(on: bool) {
    input(Input::Locked(on));
}

/// The wall clock may have moved apart from the monotonic clock: the
/// machine resumed (logind), or the clock was set. Do not disturb's end is
/// read again.
pub fn clock_moved() {
    input(Input::Clock { wall: wall_ms() });
}

/// Each application's level, as the configuration says (#899).
pub fn set_levels(levels: hocus_link::NotifyLevels) {
    input(Input::Levels(levels));
}

/// Do not disturb, as the Settings set it (#899): on for an hour, as the
/// bell's switch, or off.
pub fn set_dnd(on: bool) {
    // Already so: as it is, its end and all.
    if with(|c| c.stack.dnd.is_on()) == on {
        return;
    }
    let dnd = if on {
        Dnd::starting(dnd::Choice::Hour, Instant::now(), &wall())
    } else {
        Dnd::Off
    };
    input(Input::Dnd(dnd));
}

/// The compositor's line is there (again): it hears whether the user is
/// not to be disturbed (#899).
pub fn linked() {
    let on = with(|c| c.stack.dnd.is_on());
    crate::popover::tell(ToCompositor::Dnd(on));
}

/// The history's panel opened (`open`) or closed.
pub fn history_open(open: bool) {
    input(Input::History {
        open,
        wall: wall_ms(),
    });
}

// --- Kept: the history on disk, do not disturb for the session -----------------------

/// What to write.
enum Write {
    History(Vec<Record>),
    Dnd(Dnd),
    /// The applications that sent notifications (#899).
    Senders(hocus_link::NotifySenders),
    /// A line for the journal (#923), and when it came (milliseconds).
    Journal {
        line: String,
        at: i64,
    },
    /// An agent's search of the journal (#923), answered from there.
    Search {
        token: u64,
        search: journal::Search,
        states: Vec<(u64, hocus_link::NoteState)>,
    },
    /// Told once what was sent before is written.
    Flush(mpsc::Sender<()>),
}

static WRITE: Mutex<Option<mpsc::Sender<Write>>> = Mutex::new(None);

/// The history's file: `$XDG_STATE_HOME/hocuspanda/notifications`, else
/// `~/.local/state`'s.
fn history_file() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let state = var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    state
        .is_absolute()
        .then(|| state.join("hocuspanda/notifications"))
}

/// The senders' file (#899), beside the history's.
fn senders_file() -> Option<PathBuf> {
    history_file().map(|h| h.with_file_name(hocus_link::SENDERS_FILE))
}

/// Do not disturb's file, in the session's runtime directory.
fn dnd_file() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty())?);
    dir.is_absolute().then(|| dir.join("hocuspanda/dnd"))
}

/// `text` in `file`, readable by the user alone, whole or not at all.
fn write(file: &std::path::Path, text: &str) -> std::io::Result<()> {
    use std::{io::Write as _, os::unix::fs::OpenOptionsExt};
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let new = file.with_extension("new");
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&new)?;
    out.write_all(text.as_bytes())?;
    out.sync_all()?;
    std::fs::rename(&new, file)
}

/// Reads back the history and do not disturb, then writes them as they
/// change, on threads of their own.
fn keep() {
    let (history, dnd_at, senders_at) = (history_file(), dnd_file(), senders_file());
    let journal_at = history.as_deref().map(journal::file_beside);
    let (sender, writes) = mpsc::channel::<Write>();
    *WRITE.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);
    let read = std::thread::Builder::new()
        .name("notifications' history".to_owned())
        .spawn(move || {
            apps::restore(
                &senders_at
                    .as_deref()
                    .and_then(|file| std::fs::read_to_string(file).ok())
                    .unwrap_or_default(),
            );
            // The journal: its 30 days, and the number it ends on.
            let mut pruned_on = None;
            let last = journal_at.as_deref().map_or(0, |file| {
                prune_journal(file, &mut pruned_on);
                journal::largest(file)
            });
            let notes = read_history(history.as_deref());
            let dnd = dnd_at
                .as_deref()
                .and_then(|file| std::fs::read_to_string(file).ok())
                .map_or(Dnd::Off, |text| {
                    dnd::decode(&text, dnd::session(), Instant::now(), wall_ms())
                });
            input(Input::Restored {
                notes,
                dnd,
                journal: last,
            });
            while let Ok(first) = writes.recv() {
                // The latest of each sent meanwhile; the journal's lines
                // all, in their order, and the searches.
                let (mut records, mut dnd, mut senders, mut flushes) =
                    (None, None, None, Vec::new());
                let (mut lines, mut searches) = (Vec::new(), Vec::new());
                for write in std::iter::once(first).chain(writes.try_iter()) {
                    match write {
                        Write::History(r) => records = Some(r),
                        Write::Dnd(d) => dnd = Some(d),
                        Write::Senders(s) => senders = Some(s),
                        Write::Journal { line, at } => lines.push((line, at)),
                        Write::Search {
                            token,
                            search,
                            states,
                        } => searches.push((token, search, states)),
                        Write::Flush(done) => flushes.push(done),
                    }
                }
                if let Some(file) = &journal_at {
                    for (line, at) in lines {
                        // The first line of a new day: the 30 days first.
                        if pruned_on != day_of(at) {
                            prune_journal(file, &mut pruned_on);
                        }
                        if let Err(err) = journal::append(file, &line) {
                            tracing::warn!(file = %file.display(), %err, "the journal not written");
                        }
                    }
                }
                for (token, search, states) in searches {
                    answer_search(journal_at.as_deref(), token, &search, &states);
                }
                if let (Some(senders), Some(file)) = (senders, &senders_at)
                    && let Err(err) = self::write(file, &senders.encode())
                {
                    tracing::warn!(file = %file.display(), %err, "the senders not written");
                }
                if let (Some(records), Some(file)) = (records, &history) {
                    match self::write(file, &history::encode(&records)) {
                        Ok(()) => tracing::debug!(count = records.len(), "the history written"),
                        Err(err) => {
                            tracing::warn!(file = %file.display(), %err, "the history not written");
                        }
                    }
                }
                if let (Some(dnd), Some(file)) = (dnd, &dnd_at) {
                    let done = match dnd::encode(dnd, dnd::session()) {
                        Some(text) => self::write(file, &text),
                        None => match std::fs::remove_file(file) {
                            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                            done => done,
                        },
                    };
                    if let Err(err) = done {
                        tracing::warn!(file = %file.display(), %err, "do not disturb not written");
                    }
                }
                for done in flushes {
                    let _ = done.send(());
                }
            }
        });
    if let Err(err) = read {
        tracing::warn!(%err, "cannot keep the notifications' history");
        input(Input::Restored {
            notes: Vec::new(),
            dnd: Dnd::Off,
            journal: 0,
        });
    }
}

/// The wall clock's day of `at` (milliseconds).
fn day_of(at: i64) -> Option<jiff::civil::Date> {
    let stamp = jiff::Timestamp::from_millisecond(at).ok()?;
    Some(stamp.to_zoned(zone()).date())
}

/// The journal written again without what is older than 30 days (and its
/// oldest past 64 MB), today noted as done.
fn prune_journal(file: &std::path::Path, pruned_on: &mut Option<jiff::civil::Date>) {
    let now = wall_ms();
    *pruned_on = day_of(now);
    match journal::prune(file, now) {
        Ok((kept, gone)) if gone > 0 => tracing::info!(kept, gone, "the journal's 30 days"),
        Ok(_) => {}
        Err(err) => tracing::warn!(file = %file.display(), %err, "the journal not pruned"),
    }
}

/// An agent's search, read in the journal and answered to the compositor:
/// what it found, never in the log.
fn answer_search(
    file: Option<&std::path::Path>,
    token: u64,
    search: &journal::Search,
    states: &[(u64, hocus_link::NoteState)],
) {
    let found = match file {
        Some(file) => journal::search(file, search, |id| {
            states.iter().find(|(n, _)| *n == id).map(|(_, s)| *s)
        }),
        None => Ok(Vec::new()),
    };
    let answer = match found {
        Ok(found) => {
            tracing::info!(found = found.len(), "an agent searches the journal");
            hocus_link::NotesAnswer::Done {
                value: serde_json::json!({ "notifications": found }).to_string(),
                count: found.len() as u32,
                app: None,
                before: Vec::new(),
            }
        }
        Err(err) => hocus_link::NotesAnswer::Refused(format!("the journal does not read: {err}")),
    };
    crate::popover::tell(ToCompositor::Notes { token, answer });
}

/// How many times something the restored history's icons wait for came
/// ([`came`]), and what wakes the thread that waits for it.
static CAME: (Mutex<u64>, Condvar) = (Mutex::new(0), Condvar::new());

/// The launcher's applications were read, or a bar's form is known: the
/// history being restored looks again whether it has both. Called with
/// the caller's own locks held: it takes none of theirs.
pub fn came() {
    let (count, woken) = &CAME;
    *count.lock().unwrap_or_else(PoisonError::into_inner) += 1;
    woken.notify_all();
}

/// The history in `file`, younger than 7 days, its icons found again (once
/// the launcher's applications and the bars' scales are known, a few
/// seconds at most).
fn read_history(file: Option<&std::path::Path>) -> Vec<history::Restored> {
    let Some(file) = file else {
        return Vec::new();
    };
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => {
            tracing::warn!(file = %file.display(), %err, "the history not read");
            String::new()
        }
    };
    let (records, dropped) = history::decode(&text, wall_ms());
    tracing::info!(kept = records.len(), dropped, "the history read");
    if records.is_empty() {
        return Vec::new();
    }
    let ready = || {
        !crate::launcher::apps::catalog().apps.is_empty()
            && !crate::popover::with(|c| c.bar_forms()).is_empty()
    };
    // Woken when one of the two comes ([`came`]), not on a timer.
    let (count, woken) = &CAME;
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        // Read before looking: what comes after the look changes it.
        let seen = *count.lock().unwrap_or_else(PoisonError::into_inner);
        let left = until.saturating_duration_since(Instant::now());
        if ready() || left.is_zero() {
            break;
        }
        let waiting = count.lock().unwrap_or_else(PoisonError::into_inner);
        drop(
            woken
                .wait_timeout_while(waiting, left, |now| *now == seen)
                .unwrap_or_else(PoisonError::into_inner),
        );
    }
    let scale = server::screens_scale();
    records
        .into_iter()
        .map(|record| {
            let mut restored = record.restored();
            let note = &mut restored.note;
            note.icon = server::icon_of(server::Looked {
                app_icon: note.icon_from.app_icon.clone(),
                image_path: note.icon_from.image_path.clone(),
                image: None,
                entry: note.entry.clone(),
                app: note.app.clone(),
                scale,
            });
            restored
        })
        .collect()
}

fn send_write(write: Write) {
    if let Some(sender) = WRITE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        let _ = sender.send(write);
    }
}

/// What happened, for the stack.
pub fn input(input: Input) {
    if let Some(sender) = INPUTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        let _ = sender.send(Come::Input(input));
    }
}

/// The shell ends: returns once what the stack was told before is handled
/// and what it changed is on disk, the history and do not disturb. A panel
/// closed just before has its notifications seen, which a shell ended at
/// once would lose. Through the stack's stream while it runs, behind what
/// it has yet to handle; else (iced ended, or never ran it) straight to the
/// writer, behind what it has yet to write.
pub fn flush() {
    let (done, written) = mpsc::channel();
    let mut flush = Come::Flush(done);
    let running = TAKEN
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_none();
    if running
        && let Some(sender) = INPUTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
    {
        match sender.send(flush) {
            Ok(()) => {
                let _ = written.recv();
                return;
            }
            Err(gone) => flush = gone.0,
        }
    }
    if let Come::Flush(done) = flush {
        send_write(Write::Flush(done));
    }
    // At once if nothing writes: the sender is dropped.
    let _ = written.recv();
}

/// The compositor says the focused window's screen.
pub fn focus(screen: String) {
    input(Input::Focus(screen));
}

/// The bars changed: the bubbles keep beside them.
pub fn bars_changed() {
    let bars: Vec<(String, Form)> = crate::popover::with(|c| c.bar_forms());
    input(Input::Bars(bars));
}

/// Runs the stack on what comes and on its deadlines, and carries out what
/// it says.
pub fn effects() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::SinkExt;
    iced::stream::channel(16, async |mut output| {
        let Some(mut inputs) = TAKEN.lock().unwrap_or_else(PoisonError::into_inner).take() else {
            std::future::pending::<()>().await;
            return;
        };
        let handle = |come: Come| match come {
            Come::Input(input) => with(|c| c.stack.handle(input, Instant::now())),
            // Behind the writes of what came before, which are sent as
            // their effects are carried out, before this is taken.
            Come::Flush(done) => {
                send_write(Write::Flush(done));
                Vec::new()
            }
        };
        let mut timer_end = None;
        loop {
            let (deadline, end) = with(|c| (c.stack.deadline(), c.stack.dnd.wall_end()));
            let end = end.map(on_system_clock);
            if end != timer_end {
                timer_end = end;
                dnd_timer(end);
            }
            let effects = match deadline {
                Some(at) => {
                    use iced::futures::future::{Either, select};
                    let due = std::pin::pin!(tokio::time::sleep_until(at.into()));
                    let next = std::pin::pin!(inputs.recv());
                    match select(next, due).await {
                        Either::Left((Some(come), _)) => handle(come),
                        Either::Left((None, _)) => return,
                        Either::Right(((), _)) => with(|c| c.stack.tick(Instant::now())),
                    }
                }
                None => match inputs.recv().await {
                    Some(come) => handle(come),
                    None => return,
                },
            };
            for message in effects.into_iter().flat_map(carry_out) {
                if output.send(message).await.is_err() {
                    return;
                }
            }
        }
    })
}

// --- Do not disturb's wall-clock end ---------------------------------------------------

/// The morning's end on `CLOCK_REALTIME`, which goes on while the machine
/// sleeps: the monotonic deadline the stack waits for falls behind across a
/// suspend; this timer rings at the wall clock's 8:00, or as the machine
/// resumes past it, or when the clock is set (`CANCEL_ON_SET`), and the
/// stack reads its end again ([`Input::Clock`]). A thread of its own waits
/// on it; the stack's stream sets it.
struct DndTimer {
    fd: std::os::fd::OwnedFd,
    /// The end it is set to, on the system's clock (milliseconds).
    end: Mutex<Option<i64>>,
}

static DND_TIMER: std::sync::OnceLock<Option<DndTimer>> = std::sync::OnceLock::new();

/// `end` on the shell's wall clock (milliseconds), on the system's: the
/// shell's may read a time of its own (`HOCUS_CLOCK_AT`, moved on by `day
/// slept`), the timer is on the system's.
fn on_system_clock(end: i64) -> i64 {
    let shift = wall()
        .timestamp()
        .duration_since(jiff::Timestamp::now())
        .as_millis();
    // To the second: the two clocks are read a moment apart.
    let shift = i64::try_from(shift).unwrap_or(0).div_euclid(1000) * 1000;
    end.saturating_sub(shift)
}

/// Sets the timer to `end` (milliseconds on the system's clock), or unsets
/// it.
fn dnd_timer(end: Option<i64>) {
    let Some(timer) = DND_TIMER.get_or_init(start_dnd_timer) else {
        return;
    };
    *timer.end.lock().unwrap_or_else(PoisonError::into_inner) = end;
    set_dnd_timer(timer, end);
}

fn set_dnd_timer(timer: &DndTimer, end: Option<i64>) {
    use rustix::time::{Itimerspec, TimerfdTimerFlags, Timespec, timerfd_settime};
    let zero = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // Unset: zero. A time already past rings at once.
    let value = end.map(|end| end.max(1)).map_or(zero, |end| Timespec {
        tv_sec: end.div_euclid(1000),
        tv_nsec: end.rem_euclid(1000) * 1_000_000,
    });
    let flags = TimerfdTimerFlags::ABSTIME | TimerfdTimerFlags::CANCEL_ON_SET;
    let spec = Itimerspec {
        it_interval: zero,
        it_value: value,
    };
    if let Err(err) = timerfd_settime(&timer.fd, flags, &spec) {
        tracing::warn!(%err, "do not disturb's end is not watched on the wall clock");
    }
}

fn start_dnd_timer() -> Option<DndTimer> {
    use rustix::time::{TimerfdClockId, TimerfdFlags, timerfd_create};
    let fd = match timerfd_create(TimerfdClockId::Realtime, TimerfdFlags::CLOEXEC) {
        Ok(fd) => fd,
        Err(err) => {
            tracing::info!(%err, "do not disturb's end is not watched on the wall clock");
            return None;
        }
    };
    let started = std::thread::Builder::new()
        .name("do not disturb: clock".to_owned())
        .spawn(|| {
            let Some(timer) = DND_TIMER.wait() else {
                return;
            };
            loop {
                let mut ticks = [0u8; 8];
                match rustix::io::read(&timer.fd, &mut ticks) {
                    // Rang: the stack ends it (or sets it again).
                    Ok(_) => clock_moved(),
                    // The clock set, or the machine resumed: set again,
                    // the stack reading its end again.
                    Err(rustix::io::Errno::CANCELED) => {
                        let end = *timer.end.lock().unwrap_or_else(PoisonError::into_inner);
                        set_dnd_timer(timer, end);
                        clock_moved();
                    }
                    Err(rustix::io::Errno::INTR) => {}
                    Err(err) => {
                        tracing::info!(%err, "do not disturb's end is not watched on the wall clock");
                        return;
                    }
                }
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "no thread to watch do not disturb's end");
        return None;
    }
    Some(DndTimer {
        fd,
        end: Mutex::new(None),
    })
}

/// Surface `id` takes the pointer in `region` only, or nowhere.
pub fn region(id: window::Id, region: Option<Rectangle>) -> Message {
    crate::layer::input(id, region)
}

/// Tells the compositor; else (a shell alone) wakes every surface.
fn tell_or_wake(message: ToCompositor) -> Vec<Message> {
    if crate::popover::tell(message) {
        Vec::new()
    } else {
        vec![Message::Levels]
    }
}

/// An effect as messages for iced, a line to the compositor, or a signal.
fn carry_out(effect: Effect) -> Vec<Message> {
    match effect {
        Effect::Open {
            id,
            screen,
            namespace,
            size,
            margin,
            region: taken,
        } => {
            let pointer = taken.is_some();
            vec![
                crate::layer::open(
                    id,
                    NewLayerShellSettings {
                        size: crate::layer::size(size),
                        layer: Layer::Top,
                        anchor: Anchor::Right | Anchor::Top,
                        exclusive_zone: Some(0),
                        margin: Some(crate::layer::margin(margin)),
                        keyboard_interactivity: KeyboardInteractivity::None,
                        output_option: if screen == crate::ALONE {
                            OutputOption::Active
                        } else {
                            OutputOption::OutputName(screen)
                        },
                        blur_option: iced_exwlshell::reexport::BlurOption::None,
                        events_transparent: !pointer,
                        namespace: Some(namespace),
                    },
                ),
                region(id, taken),
            ]
        }
        Effect::Place {
            id,
            size,
            margin,
            region: taken,
        } => {
            let mut messages: Vec<Message> = crate::layer::resize(id, size).into_iter().collect();
            messages.push(crate::layer::moved(id, margin));
            if taken.is_some() {
                messages.push(region(id, taken));
            }
            messages
        }
        Effect::Wake(namespace) => tell_or_wake(ToCompositor::Wake(namespace)),
        Effect::Closing(namespace) => {
            crate::popover::tell(ToCompositor::Closing(namespace));
            Vec::new()
        }
        Effect::Remove(id) => vec![crate::layer::close(id)],
        // A capture's (5.9) is the shell's own: its actions here, nothing
        // to say on the bus.
        Effect::Signal(server::Signal::Action { id, key }) if capture_of(id).is_some() => {
            if let Some(capture) = capture_of(id) {
                crate::capture::from_history(capture, &key);
            }
            Vec::new()
        }
        Effect::Signal(server::Signal::Closed { id, .. }) if capture_of(id).is_some() => Vec::new(),
        // A meeting's reminder's (Day.6) is the shell's own too: its link.
        Effect::Signal(server::Signal::Action { id, .. }) if reminder_of(id).is_some() => {
            if let Some(serial) = reminder_of(id) {
                crate::reminder::from_history(serial);
            }
            Vec::new()
        }
        Effect::Signal(server::Signal::Closed { id, .. }) if reminder_of(id).is_some() => {
            Vec::new()
        }
        // A removable drive's (#898): open it, eject it, try again.
        Effect::Signal(server::Signal::Action { id, key }) if drive_of(id).is_some() => {
            if let Some(serial) = drive_of(id) {
                crate::removable::from_bubble(serial, &key);
            }
            Vec::new()
        }
        Effect::Signal(server::Signal::Closed { id, .. }) if drive_of(id).is_some() => Vec::new(),
        Effect::Signal(signal) => {
            server::signal(signal);
            Vec::new()
        }
        Effect::Raise { pid, app } => {
            crate::popover::tell(ToCompositor::Raise { pid, app });
            Vec::new()
        }
        Effect::Converse {
            pid,
            app,
            conversation,
        } => {
            crate::popover::tell(ToCompositor::Converse {
                pid,
                app,
                conversation,
            });
            Vec::new()
        }
        Effect::Changed => {
            crate::popover::changed(crate::popover::Item::Notifications);
            Vec::new()
        }
        Effect::Save(records) => {
            send_write(Write::History(records));
            Vec::new()
        }
        Effect::SaveDnd(dnd) => {
            send_write(Write::Dnd(dnd));
            Vec::new()
        }
        Effect::History(screen) => {
            crate::popover::open_history(&screen);
            Vec::new()
        }
        Effect::Sound(sound) => {
            apps::play(&sound);
            Vec::new()
        }
        Effect::Dnd(on) => {
            crate::popover::tell(ToCompositor::Dnd(on));
            Vec::new()
        }
        Effect::Journal(line) => {
            send_write(Write::Journal {
                line: line.text(&zone()),
                at: line.at,
            });
            Vec::new()
        }
        Effect::Answer { token, answer } => {
            crate::popover::tell(ToCompositor::Notes { token, answer });
            Vec::new()
        }
        Effect::Search {
            token,
            search,
            states,
        } => {
            let sent = WRITE
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .is_some_and(|w| {
                    w.send(Write::Search {
                        token,
                        search: search.clone(),
                        states: states.clone(),
                    })
                    .is_ok()
                });
            // No journal here (no notifications served): nothing found.
            if !sent {
                answer_search(None, token, &search, &states);
            }
            Vec::new()
        }
    }
}

// --- The captures in the history (5.9) ----------------------------------------------

/// The ids of the captures in the history: past any the server gives (it
/// counts from 1).
const CAPTURES_FROM: u32 = 0xC000_0000;

/// Capture `capture`'s id in the history.
pub fn capture_id(capture: u64) -> u32 {
    CAPTURES_FROM | (capture as u32 & !CAPTURES_FROM)
}

/// The capture whose id in the history is `id`, if it is one.
pub fn capture_of(id: u32) -> Option<u64> {
    (id & CAPTURES_FROM == CAPTURES_FROM).then(|| u64::from(id & !CAPTURES_FROM))
}

/// Capture `shot` goes to the history, the newest (in place of itself if
/// it was there): its image as its icon, its name, its size and folder,
/// "Annoter", "Ouvrir le dossier" and "Supprimer"; a click annotates it.
/// Seen: the user took it.
pub fn keep_capture(shot: &crate::capture::Shot, icon: Option<crate::levels::icons::Icon>) {
    let words = crate::capture::words();
    let icon = icon.or_else(|| crate::capture::icon(shot.id));
    // A recording's (5.10): its length for its size, "Ouvrir" for
    // "Annoter".
    let (app, what, first, image) = match &shot.video {
        Some(video) => (
            words.screen_recording(),
            crate::in_use::draw::elapsed(video.duration),
            ("open", words.open()),
            video.poster.display().to_string(),
        ),
        None => (
            words.screenshot(),
            format!("{} × {}", shot.width, shot.height),
            ("annotate", words.annotate()),
            shot.file.display().to_string(),
        ),
    };
    let note = note::Note {
        id: capture_id(shot.id),
        app: app.to_owned(),
        entry: None,
        icon,
        summary: shot.name(),
        body: if shot.folder.is_empty() {
            what
        } else {
            format!("{what} · {}", shot.folder)
        },
        actions: vec![
            (first.0.to_owned(), first.1.to_owned()),
            ("folder".to_owned(), words.open_folder().to_owned()),
            ("delete".to_owned(), words.delete().to_owned()),
        ],
        default: true,
        urgency: note::Urgency::Normal,
        timeout: note::Timeout::After(note::LIFE),
        progress: None,
        resident: true,
        pid: 0,
        agent: false,
        conversation: None,
        icon_from: note::IconFrom {
            app_icon: String::new(),
            image_path: Some(image),
        },
        at: wall_ms(),
        sound: None,
        number: 0,
        transient: false,
        ancestry: Vec::new(),
    };
    tracing::debug!(
        capture = shot.id,
        title = note.summary,
        "the capture goes to the history"
    );
    input(Input::Keep(Box::new(note)));
}

/// Capture `capture` deleted: out of the history.
pub fn forget_capture(capture: u64) {
    input(Input::Close(capture_id(capture)));
}

// --- The meetings' reminders in the history (Day.6) -----------------------------------

/// The ids of the meetings' reminders in the history: past any the server
/// gives, apart from the captures'.
const REMINDERS_FROM: u32 = 0x8000_0000;
const SHELLS_OWN: u32 = 0xC000_0000;

/// Reminder `serial`'s id in the history.
pub fn reminder_id(serial: u64) -> u32 {
    REMINDERS_FROM | (serial as u32 & !SHELLS_OWN)
}

/// The reminder whose id in the history is `id`, if it is one.
pub fn reminder_of(id: u32) -> Option<u64> {
    (id & SHELLS_OWN == REMINDERS_FROM).then(|| u64::from(id & !SHELLS_OWN))
}

/// A meeting's reminder that stayed its time goes to the history, the
/// newest: from `app` ("Agenda"), saying `summary` ("Revue de sprint · a
/// commencé à 10:00"), `join` its action (and its click) while the meeting
/// runs; kept at `at` (the wall clock's milliseconds: when its tab went).
/// Seen: its tab showed.
pub fn keep_reminder(serial: u64, app: &str, summary: String, join: Option<&str>, at: i64) {
    input(Input::Keep(Box::new(reminder_note(
        serial, app, summary, join, at,
    ))));
}

/// Its meeting ended: its action goes, in its place.
pub fn reminder_ended(serial: u64, app: &str, summary: String, at: i64) {
    input(Input::Update(Box::new(reminder_note(
        serial, app, summary, None, at,
    ))));
}

fn reminder_note(
    serial: u64,
    app: &str,
    summary: String,
    join: Option<&str>,
    at: i64,
) -> note::Note {
    note::Note {
        id: reminder_id(serial),
        app: app.to_owned(),
        entry: None,
        icon: None,
        summary,
        body: String::new(),
        actions: join
            .map(|label| vec![("join".to_owned(), label.to_owned())])
            .unwrap_or_default(),
        default: join.is_some(),
        urgency: note::Urgency::Normal,
        timeout: note::Timeout::After(note::LIFE),
        progress: None,
        resident: false,
        pid: 0,
        agent: false,
        conversation: None,
        icon_from: note::IconFrom::default(),
        at,
        sound: None,
        number: 0,
        transient: false,
        ancestry: Vec::new(),
    }
}

// --- A developer's program let go of (docs/workflow.md) --------------------------------

/// The ids of what the desktop says of itself: past any the server gives,
/// apart from the reminders' and the captures'.
const DESKTOPS_OWN_FROM: u32 = 0x4000_0000;

/// The icon theme's icon `name` at the bubbles' size, for a bubble of the
/// desktop's own; none when no theme has it.
pub fn themed_icon(name: &str) -> Option<crate::levels::icons::Icon> {
    server::icon_named(name, server::screens_scale())
}

/// The ids of the removable drives' bubbles (#898): among the desktop's
/// own, one a drive, each in place of the one before.
const DRIVES_FROM: u32 = DESKTOPS_OWN_FROM | 0x0010_0000;

/// Drive `serial`'s bubbles' id.
pub fn drive_id(serial: u32) -> u32 {
    DRIVES_FROM | (serial & 0xFFFF)
}

/// The drive whose bubble's id is `id`, if it is one.
pub fn drive_of(id: u32) -> Option<u32> {
    (id & 0xFFFF_0000 == DRIVES_FROM).then_some(id & 0xFFFF)
}

/// The program given for this session as the shell (`hocus msg shell
/// restart --program`) did not start or kept ending, and the installed
/// one runs again: a notification says so, the latest in place of the one
/// before.
pub fn fell_back(which: hocus_link::Supervised, program: &std::path::Path) {
    use crate::clock::time::Lang;
    use hocus_link::Supervised;
    let program = program.display();
    let lang = Lang::of_messages();
    let (number, summary, body) = match which {
        Supervised::Shell => (
            0,
            hocus_words::shell::notifications::DEV_SHELL_STOPPED.get(lang),
            hocus_words::shell::notifications::dev_shell_stopped_body(lang, &program),
        ),
    };
    tracing::info!(which = which.word(), %program, "a given program was let go of: said");
    input(Input::Notify(Box::new(note::Note {
        id: DESKTOPS_OWN_FROM | number,
        app: "Hocuspanda".to_owned(),
        entry: None,
        icon: None,
        summary: summary.to_owned(),
        body,
        actions: Vec::new(),
        default: false,
        urgency: note::Urgency::Normal,
        timeout: note::Timeout::After(note::LIFE),
        progress: None,
        resident: false,
        pid: 0,
        agent: false,
        conversation: None,
        icon_from: note::IconFrom::default(),
        at: wall_ms(),
        sound: None,
        number: 0,
        transient: false,
        ancestry: Vec::new(),
    })));
}

/// What surface `id` shows, if it is one of the notifications': a bubble,
/// or the chip. `glass`: the compositor blurs behind the shell.
pub fn view<'a>(id: window::Id, glass: bool) -> Option<Element<'a, Message>> {
    enum Is {
        Bubble,
        Chip,
    }
    let (is, card) = with(|c| {
        let is = if c.stack.bubble(id).is_some() {
            Some(Is::Bubble)
        } else if c.stack.chip(id).is_some() {
            Some(Is::Chip)
        } else {
            None
        };
        (is, c.card)
    });
    let element: Element<'a, Message> = match is? {
        Is::Bubble => bubble::view(id, glass, card),
        Is::Chip => canvas(draw::Chip { id, card })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    };
    Some(element)
}
