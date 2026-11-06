//! The bar's to-do list (Day.4, issue #428, variant C; bar.md §4 group 5,
//! §5): the open tasks of the day's tasks' account ([`crate::day`]), up to
//! 3 in the full form's group ([`draw`]), all of them in the tasks'
//! popover ([`crate::popover::tasks`]), which the mini form's item and the
//! group's "+ N autres" open.
//!
//! What the views show is the account's tasks in its column's order, as
//! the last snapshot says, with what the user did here over it: a task
//! ticked stays 1.5 s, ticked (a second click in that time unticks it),
//! then its line goes; a change the source refused shows "Pas enregistré"
//! in place of the title for 3 s, the box as the source has it; a task
//! added shows at the top, its box inert, until the source made its card.
//! The source is told at once, off the loop (`day::change`); its answer
//! comes back in a later snapshot, which wakes the views
//! ([`crate::day::on_change`]): nothing here is polled.
//!
//! The field to add a task opens as the first line of the group or the
//! popover, one at a time; `Enter` adds, `Esc` closes, a click elsewhere
//! closes it when empty and lets the keyboard go when not.

pub mod draw;

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use hocus_day::{Change, ChangeId, Day, Kind, Snapshot, model::Task, model::TaskId};
use hocus_link::{POPOVER_NAMESPACE, TASKS_NAMESPACE};

use crate::{
    clock::time::Lang,
    popover::{self, Effect, Item, Pressed},
};

/// The group's lines in the full form; the popover's, past which it
/// scrolls.
pub const GROUP_LINES: usize = 3;
pub const POPOVER_LINES: usize = 6;
/// A task ticked stays this long, and a second click in that time unticks
/// it.
pub const STAYS: Duration = Duration::from_millis(1500);
/// Then its line fades and closes (with motion only).
pub const CLOSE: Duration = Duration::from_millis(300);
/// "Pas enregistré" shows this long in place of the title.
pub const UNSAVED: Duration = Duration::from_secs(3);
/// The longest title the field takes, in characters.
const LONGEST: usize = 200;

// --- What the views show ---------------------------------------------------------------

/// A line's key: a task of the source, or a task added not made yet.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    Task(TaskId),
    Adding(ChangeId),
}

/// How a line shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Open,
    /// Ticked here, for its 1.5 s (and its close).
    Ticked,
    /// Its change refused: "Pas enregistré", the box as the source has it.
    Unsaved {
        ticked: bool,
    },
    /// Added here, its card not made yet: its box does nothing.
    Adding,
}

impl Look {
    /// Whether its box is ticked.
    pub fn ticked(self) -> bool {
        matches!(self, Self::Ticked | Self::Unsaved { ticked: true })
    }

    /// Whether it counts as open.
    pub fn open(self) -> bool {
        !self.ticked()
    }
}

/// A line of the list.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub key: Key,
    pub title: String,
    pub look: Look,
    /// While it goes (with motion): its close's share of its way, 0 to 1.
    pub going: Option<f32>,
}

/// What the views show now.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lines {
    /// Whether the group and the mini item show: a board is set (Day.1),
    /// or the local list has a task.
    pub shows: bool,
    pub lines: Vec<Line>,
}

impl Lines {
    /// The open tasks: the mini item's and the popover's count.
    pub fn open(&self) -> usize {
        self.lines
            .iter()
            .filter(|l| l.going.is_none() && l.look.open())
            .count()
    }

    /// Whether a line moves.
    pub fn moving(&self) -> bool {
        self.lines.iter().any(|l| l.going.is_some())
    }
}

/// What the user did to a task, over what the source says.
#[derive(Debug, Clone)]
enum Mark {
    /// Ticked here at `at`, its line then at `index`: it stays 1.5 s.
    Ticked {
        task: Task,
        index: usize,
        at: Instant,
    },
    /// Its 1.5 s over, its line closing since then (with motion).
    Closing {
        task: Task,
        index: usize,
        since: Instant,
    },
    /// Its line gone: hidden while the source still lists it, until a
    /// later day says it is open with nothing pending (it came back).
    Gone { generation: u64 },
    /// Unticked here: shown at `index` until the source lists it again.
    Back {
        task: Task,
        index: usize,
        generation: u64,
    },
    /// The source refused its change at `at`: "Pas enregistré" for 3 s.
    Unsaved {
        task: Task,
        index: usize,
        at: Instant,
        ticked: bool,
    },
}

/// Where the field to add a task is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum At {
    /// The group of the bar on that screen.
    Group(String),
    Popover,
}

/// The field to add a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub at: At,
    pub text: String,
    /// It holds the keyboard (else it waits, its text kept, for a click).
    pub focused: bool,
}

#[derive(Debug, Default)]
struct Ui {
    marks: HashMap<TaskId, Mark>,
    /// Adds refused, since then: "Pas enregistré" on their lines.
    adds_unsaved: HashMap<ChangeId, Instant>,
    /// The refusals already said.
    seen: HashSet<ChangeId>,
    field: Option<Field>,
    /// A paste awaited (its number), and how many were asked.
    paste: Option<u64>,
    pastes: u64,
    /// What was said last, for the log.
    said: Option<(Lines, Option<Field>)>,
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);
/// The day's changes seen, counted on the day's thread.
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// The tasks as last seen by the day's waker: a change of the day that
/// leaves them as they are wakes nothing.
static SEEN: Mutex<Option<Seen>> = Mutex::new(None);

type Seen = (Kind, Vec<Task>, Vec<hocus_day::Pending>);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

/// Whether the shell moves things (not with reduced motion, nor on the
/// CPU): the popovers' controller knows.
fn motion() -> bool {
    popover::with(|c| c.motion)
}

/// From now on, a change of the day's tasks wakes the group's surfaces and
/// the tasks' popover, if open.
pub fn start() {
    crate::day::on_change(|| {
        GENERATION.fetch_add(1, Ordering::Relaxed);
        let day = crate::day::snapshot();
        let now: Option<Seen> = day
            .tasks()
            .map(|s| (s.account.kind, s.tasks.clone(), s.pending.clone()));
        let changed = {
            let mut seen = SEEN.lock().unwrap_or_else(PoisonError::into_inner);
            let changed = *seen != now;
            *seen = now;
            changed
        };
        if changed {
            wake();
        }
        true
    });
}

/// The group comes or goes (`[panel] tasks`, 6.4): its surfaces lay out
/// again.
pub fn shown_changed() {
    wake();
}

/// The group's surfaces and the tasks' popover draw again.
fn wake() {
    popover::send(vec![Effect::Wake(TASKS_NAMESPACE)]);
    popover::changed(Item::Tasks);
    popover::changed(Item::TasksMore);
}

/// The lines now, from the day's snapshot and what the user did.
pub fn lines() -> Lines {
    let day = crate::day::snapshot();
    let now = Instant::now();
    let motion = motion();
    ui(|ui| {
        settle(ui, &day, now, motion);
        let lines = lines_of(ui, &day, now);
        say(ui, &lines);
        lines
    })
}

/// The lines as last settled, without settling them: for what the
/// popovers' controller asks while it holds itself (a popover's height).
pub fn peek() -> Lines {
    let day = crate::day::snapshot();
    let now = Instant::now();
    ui(|ui| lines_of(ui, &day, now))
}

/// The field, if open at `at`, closes without a word to the popovers'
/// controller: it is the one closing the popover.
pub fn drop_field_at(at: &At) {
    let dropped = ui(|ui| {
        if ui.field.as_ref().is_some_and(|f| f.at == *at) {
            ui.paste = None;
            ui.field = None;
            true
        } else {
            false
        }
    });
    if dropped {
        tracing::debug!(?at, "the field to add a task closes with its popover");
        popover::send(vec![Effect::Wake(TASKS_NAMESPACE)]);
    }
}

/// The tasks' snapshot, and whether the views show it: a board is set, or
/// the local list has a task (or shows one still).
fn tasks_of(day: &Day) -> Option<&Snapshot> {
    day.tasks()
}

fn lines_of(ui: &Ui, day: &Day, now: Instant) -> Lines {
    // Hidden (`[panel] tasks`, 6.4): nothing, as without a source.
    if !crate::column::groups().tasks {
        return Lines::default();
    }
    let Some(tasks) = tasks_of(day) else {
        return Lines::default();
    };
    let mut lines: Vec<Line> = Vec::new();
    // Tasks added here, their cards not made yet: the latest first, as
    // the source puts them at its column's top.
    for pending in tasks.pending.iter().rev() {
        if let Change::Add { title } = &pending.change {
            let unsaved = ui
                .adds_unsaved
                .get(&pending.id)
                .is_some_and(|at| now.saturating_duration_since(*at) < UNSAVED);
            lines.push(Line {
                key: Key::Adding(pending.id),
                title: title.clone(),
                look: if unsaved {
                    Look::Unsaved { ticked: false }
                } else {
                    Look::Adding
                },
                going: None,
            });
        }
    }
    let close = |since: Instant| {
        (now.saturating_duration_since(since).as_secs_f32() / CLOSE.as_secs_f32()).min(1.0)
    };
    for task in &tasks.tasks {
        let line = |look| Line {
            key: Key::Task(task.id.clone()),
            title: task.title.clone(),
            look,
            going: None,
        };
        match ui.marks.get(&task.id) {
            None | Some(Mark::Back { .. }) => lines.push(line(Look::Open)),
            Some(Mark::Ticked { .. }) => lines.push(line(Look::Ticked)),
            Some(Mark::Closing { since, .. }) => lines.push(Line {
                going: Some(close(*since)),
                ..line(Look::Ticked)
            }),
            Some(Mark::Unsaved { ticked, .. }) => {
                lines.push(line(Look::Unsaved { ticked: *ticked }));
            }
            Some(Mark::Gone { .. }) => {}
        }
    }
    // Those the source no longer lists (or not yet again), where they were.
    let mut kept: Vec<(usize, Line)> = ui
        .marks
        .iter()
        .filter(|(id, _)| !tasks.tasks.iter().any(|t| t.id == **id))
        .filter_map(|(id, mark)| {
            let (task, index, look, going) = match mark {
                Mark::Ticked { task, index, .. } => (task, *index, Look::Ticked, None),
                Mark::Closing { task, index, since } => {
                    (task, *index, Look::Ticked, Some(close(*since)))
                }
                Mark::Back { task, index, .. } => (task, *index, Look::Open, None),
                Mark::Unsaved {
                    task,
                    index,
                    ticked,
                    ..
                } => (task, *index, Look::Unsaved { ticked: *ticked }, None),
                Mark::Gone { .. } => return None,
            };
            Some((
                index,
                Line {
                    key: Key::Task(id.clone()),
                    title: task.title.clone(),
                    look,
                    going,
                },
            ))
        })
        .collect();
    kept.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.title.cmp(&b.1.title)));
    for (index, line) in kept {
        lines.insert(index.min(lines.len()), line);
    }
    let shows = tasks.account.kind != Kind::Local || !lines.is_empty();
    Lines { shows, lines }
}

/// What the day and the time say of the marks: refusals, the 1.5 s over,
/// lines closed, tasks back.
fn settle(ui: &mut Ui, day: &Day, now: Instant, motion: bool) {
    let generation = GENERATION.load(Ordering::Relaxed);
    let Some(tasks) = tasks_of(day) else {
        ui.marks.clear();
        return;
    };
    let listed = |id: &TaskId| tasks.tasks.iter().find(|t| t.id == *id);
    let pending = |id: &TaskId, tick: bool| {
        tasks.pending.iter().any(|p| match &p.change {
            Change::Tick { task } => tick && task == id,
            Change::Untick { task } => !tick && task == id,
            Change::Add { .. } => false,
        })
    };
    // Refusals, each said once.
    let here: Vec<(usize, TaskId)> = lines_of(ui, day, now)
        .lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| match &l.key {
            Key::Task(id) => Some((i, id.clone())),
            Key::Adding(_) => None,
        })
        .collect();
    let index_of = |id: &TaskId| here.iter().find(|(_, t)| t == id).map(|(i, _)| *i);
    for refused in tasks.pending.iter().filter(|p| p.refused.is_some()) {
        if !ui.seen.insert(refused.id) {
            continue;
        }
        let why = refused
            .refused
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        let (task, ticked) = match &refused.change {
            Change::Add { .. } => {
                tracing::debug!(id = refused.id.0, why, "a task added was not saved");
                ui.adds_unsaved.insert(refused.id, now);
                continue;
            }
            Change::Tick { task } => (task, false),
            Change::Untick { task } => (task, true),
        };
        let known = listed(task).cloned().or_else(|| match ui.marks.get(task) {
            Some(
                Mark::Ticked { task, .. }
                | Mark::Closing { task, .. }
                | Mark::Back { task, .. }
                | Mark::Unsaved { task, .. },
            ) => Some(task.clone()),
            _ => None,
        });
        let Some(known) = known else {
            continue;
        };
        tracing::debug!(
            task = task.0,
            why,
            ticked,
            "a change of a task was not saved"
        );
        let index = index_of(task).unwrap_or(0);
        ui.marks.insert(
            task.clone(),
            Mark::Unsaved {
                task: known,
                index,
                at: now,
                ticked,
            },
        );
    }
    ui.seen
        .retain(|id| tasks.pending.iter().any(|p| p.id == *id));
    ui.adds_unsaved
        .retain(|id, _| tasks.pending.iter().any(|p| p.id == *id));
    // The marks' times, and what the source says of them.
    let ids: Vec<TaskId> = ui.marks.keys().cloned().collect();
    for id in ids {
        let Some(mark) = ui.marks.get(&id).cloned() else {
            continue;
        };
        // Some(None): done with, the source's word alone.
        let next: Option<Option<Mark>> = match mark {
            Mark::Ticked { task, index, at } if now.saturating_duration_since(at) >= STAYS => {
                tracing::debug!(
                    task = id.0,
                    after_ms = now.saturating_duration_since(at).as_millis() as u64,
                    "a ticked task's line goes"
                );
                if motion {
                    Some(Some(Mark::Closing {
                        task,
                        index,
                        since: now,
                    }))
                } else {
                    Some(Some(Mark::Gone { generation }))
                }
            }
            Mark::Closing { since, .. } if now.saturating_duration_since(since) >= CLOSE => {
                Some(Some(Mark::Gone { generation }))
            }
            Mark::Gone { generation: then } => {
                let back = listed(&id).is_some() && generation > then && !pending(&id, true);
                (listed(&id).is_none() || back).then_some(None)
            }
            Mark::Back {
                generation: then, ..
            } => (listed(&id).is_some() || (generation > then && !pending(&id, false)))
                .then_some(None),
            Mark::Unsaved { at, ticked, .. } if now.saturating_duration_since(at) >= UNSAVED => {
                tracing::debug!(
                    task = id.0,
                    shown_ms = now.saturating_duration_since(at).as_millis() as u64,
                    ticked,
                    "a task's \"not saved\" ends"
                );
                Some(None)
            }
            _ => None,
        };
        match next {
            Some(None) => {
                ui.marks.remove(&id);
            }
            Some(Some(next)) => {
                ui.marks.insert(id, next);
            }
            None => {}
        }
    }
}

/// Logged when what shows changes, for tests.
fn say(ui: &mut Ui, lines: &Lines) {
    let said = Some((lines.clone(), ui.field.clone()));
    if ui.said == said {
        return;
    }
    ui.said = said;
    let words = Words::new(Lang::of_messages());
    let shown: Vec<String> = lines
        .lines
        .iter()
        .map(|l| {
            let sign = match l.look {
                Look::Open => "[ ]",
                Look::Ticked => "[x]",
                Look::Unsaved { ticked: false } => "[!]",
                Look::Unsaved { ticked: true } => "[x!]",
                Look::Adding => "[+]",
            };
            let title = match l.look {
                Look::Unsaved { .. } => words.unsaved(),
                _ => &l.title,
            };
            format!("{sign} {title}")
        })
        .collect();
    let field = ui
        .field
        .as_ref()
        .map(|f| format!("{:?} «{}» focused={}", f.at, f.text, f.focused));
    tracing::debug!(
        shows = lines.shows,
        open = lines.open(),
        lines = shown.join("; "),
        field = field.as_deref().unwrap_or("none"),
        "the tasks show"
    );
}

// --- What the user does ----------------------------------------------------------------

/// The box of the line `key` clicked (or `Space` on it): ticked, or
/// unticked within its 1.5 s; the source told at once.
pub fn toggle(key: &Key) {
    let Key::Task(id) = key else {
        return;
    };
    let now = Instant::now();
    let generation = GENERATION.load(Ordering::Relaxed);
    let day = crate::day::snapshot();
    let all = ui(|ui| lines_of(ui, &day, now));
    let Some((index, line)) = all.lines.iter().enumerate().find(|(_, l)| l.key == *key) else {
        return;
    };
    if line.going.is_some() {
        return;
    }
    let listed = tasks_of(&day).and_then(|s| s.tasks.iter().find(|t| t.id == *id).cloned());
    let change = ui(|ui| {
        let known = |ui: &Ui| {
            listed.clone().or_else(|| match ui.marks.get(id) {
                Some(
                    Mark::Ticked { task, .. }
                    | Mark::Back { task, .. }
                    | Mark::Unsaved { task, .. }
                    | Mark::Closing { task, .. },
                ) => Some(task.clone()),
                _ => None,
            })
        };
        let task = known(ui)?;
        match ui.marks.get(id) {
            Some(Mark::Ticked { at, .. }) => {
                tracing::debug!(
                    task = id.0,
                    after_ms = now.saturating_duration_since(*at).as_millis() as u64,
                    "a task unticked"
                );
                ui.marks.insert(
                    id.clone(),
                    Mark::Back {
                        task,
                        index,
                        generation,
                    },
                );
                Some(Change::Untick { task: id.clone() })
            }
            Some(Mark::Unsaved { ticked: true, .. }) => None,
            _ => {
                tracing::debug!(task = id.0, "a task ticked");
                ui.marks.insert(
                    id.clone(),
                    Mark::Ticked {
                        task,
                        index,
                        at: now,
                    },
                );
                Some(Change::Tick { task: id.clone() })
            }
        }
    });
    if let Some(change) = change {
        crate::day::change(change);
    }
    // Drawn now; the effects' stream waits for the 1.5 s.
    wake();
}

/// The field to add a task, as it is.
pub fn field() -> Option<Field> {
    ui(|ui| ui.field.clone())
}

/// The field opens at `at`, empty, holding the keyboard; open there
/// already, it takes the keyboard again. One at a time: one open elsewhere
/// closes.
pub fn open_field(at: At) {
    let before = ui(|ui| {
        let before = ui.field.clone();
        match &mut ui.field {
            Some(field) if field.at == at => field.focused = true,
            _ => {
                ui.field = Some(Field {
                    at: at.clone(),
                    text: String::new(),
                    focused: true,
                });
                ui.paste = None;
            }
        }
        before
    });
    // Its surface, for the keys' log (`hocuspanda_shell::keys`).
    let surface = match &at {
        At::Group(bar) => draw::surface(bar),
        At::Popover => None,
    };
    tracing::debug!(?at, ?surface, "the field to add a task opens");
    let mut effects = Vec::new();
    if let Some(before) = before.filter(|b| b.at != at) {
        effects.extend(let_keyboard_go(&before.at));
    }
    match &at {
        At::Group(bar) => {
            if let Some(id) = draw::surface(bar) {
                effects.push(Effect::Keyboard(
                    id,
                    iced_layershell::reexport::KeyboardInteractivity::Exclusive,
                ));
            }
        }
        // Opened by hovering, the popover takes the keyboard now.
        At::Popover => popover::event(popover::Event::Hold),
    }
    popover::send(effects);
    wake();
}

/// What lets the keyboard go from a field at `at`: the group's surface's,
/// unless its `+` holds it (the field was opened from it with `Enter`).
fn let_keyboard_go(at: &At) -> Vec<Effect> {
    let At::Group(bar) = at else {
        return Vec::new();
    };
    if popover::with(|c| c.focused(bar, Item::TaskAdd)) {
        return Vec::new();
    }
    draw::surface(bar)
        .map(|id| Effect::Keyboard(id, iced_layershell::reexport::KeyboardInteractivity::None))
        .into_iter()
        .collect()
}

/// The field closes (`why`, for the log), what was typed gone.
pub fn close_field(why: &str) {
    let closed = ui(|ui| {
        ui.paste = None;
        ui.field.take()
    });
    let Some(closed) = closed else {
        return;
    };
    tracing::debug!(why, at = ?closed.at, "the field to add a task closes");
    popover::send(let_keyboard_go(&closed.at));
    wake();
}

/// The field at `at` closes if it is there (its group's bar changed form,
/// the popover closed).
pub fn close_field_at(at: &At, why: &str) {
    if field().is_some_and(|f| f.at == *at) {
        close_field(why);
    }
}

/// A press anywhere but in the field: empty, it closes; else it keeps its
/// text and lets the keyboard go.
pub fn pressed_elsewhere() {
    let Some(field) = field() else {
        return;
    };
    if field.text.trim().is_empty() {
        close_field("a press elsewhere");
        return;
    }
    if !field.focused {
        return;
    }
    ui(|ui| {
        if let Some(f) = &mut ui.field {
            f.focused = false;
        }
        ui.paste = None;
    });
    tracing::debug!("the field to add a task lets the keyboard go, its text kept");
    popover::send(let_keyboard_go(&field.at));
    wake();
}

/// A key for the field at `at`, while it holds the keyboard: whether it
/// took it.
pub fn field_key(at: &At, pressed: &Pressed) -> bool {
    use iced::keyboard::{Key as K, key::Named};
    let Some(field) = field().filter(|f| f.at == *at && f.focused) else {
        return false;
    };
    let modifiers = pressed.modifiers;
    match &pressed.key {
        K::Named(Named::Escape) => close_field("Escape"),
        K::Named(Named::Enter) => {
            let title = field.text.trim().to_owned();
            if !title.is_empty() {
                tracing::debug!(title, "a task added");
                crate::day::change(Change::Add { title });
            }
            close_field("Enter");
        }
        K::Named(Named::Backspace) => {
            ui(|ui| {
                if let Some(f) = &mut ui.field {
                    if modifiers.control() {
                        f.text.clear();
                    } else {
                        f.text.pop();
                    }
                }
            });
            wake();
        }
        // The keyboard stays in the field.
        K::Named(Named::Tab) => {}
        _ if crate::paste::asks(&pressed.key, modifiers) => paste(),
        _ if !(modifiers.control() || modifiers.logo() || modifiers.alt()) => {
            let Some(text) = pressed
                .text
                .as_deref()
                .filter(|t| !t.is_empty() && t.chars().all(|c| !c.is_control()))
            else {
                return false;
            };
            if push(text) {
                wake();
            }
        }
        _ => return false,
    }
    true
}

/// `text` at the field's end, whole if it fits: whether it did.
fn push(text: &str) -> bool {
    ui(|ui| {
        let Some(f) = &mut ui.field else {
            return false;
        };
        if f.text.chars().count() + text.chars().count() > LONGEST {
            return false;
        }
        f.text.push_str(text);
        true
    })
}

/// The clipboard's text at the field's end on one line (Day.4b): its line
/// breaks and runs of white space one space, its ends trimmed, whole if it
/// fits or not at all; it
/// lands only in the field that asked, open and holding the keyboard.
fn paste() {
    let (n, at) = match ui(|ui| {
        if ui.paste.is_some() {
            return None;
        }
        let at = ui.field.as_ref().filter(|f| f.focused)?.at.clone();
        ui.pastes += 1;
        ui.paste = Some(ui.pastes);
        Some((ui.pastes, at))
    }) {
        Some(asked) => asked,
        None => return,
    };
    let asked = crate::paste::request(crate::paste::Clean::Spaced, move |text| {
        let awaited = ui(|ui| {
            let awaited =
                ui.paste == Some(n) && ui.field.as_ref().is_some_and(|f| f.at == at && f.focused);
            if ui.paste == Some(n) {
                ui.paste = None;
            }
            awaited
        });
        let Some(text) = text.filter(|t| !t.is_empty()) else {
            return;
        };
        if !awaited {
            return;
        }
        if push(text) {
            tracing::debug!("the clipboard's text pasted into the field to add a task");
            wake();
        } else {
            tracing::debug!("the clipboard's text does not fit in the field to add a task");
        }
    });
    if !asked {
        ui(|ui| {
            if ui.paste == Some(n) {
                ui.paste = None;
            }
        });
    }
}

/// What an item of the group does, clicked or with `Enter` or `Space`
/// while it holds the keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// The `+` of bar `bar`'s group: the field opens.
    Add { bar: String },
    /// The box of line `slot` of bar `bar`'s group.
    Toggle { bar: String, slot: u8 },
}

/// Carries out `act`.
pub fn act(act: Act) {
    match act {
        // With `Enter`, its `+` keeps the keyboard's ring for when the
        // field closes; the field takes the keys meanwhile.
        Act::Add { bar } => open_field(At::Group(bar)),
        Act::Toggle { bar, slot } => {
            if let Some(key) = draw::slot(&bar, slot) {
                toggle(&key);
            }
        }
    }
}

// --- Time ------------------------------------------------------------------------------

/// When something is due: a ticked line's 1.5 s, a close, a "not saved"'s
/// 3 s.
pub fn deadline() -> Option<Instant> {
    ui(|ui| {
        let marks = ui.marks.values().filter_map(|m| match m {
            Mark::Ticked { at, .. } => Some(*at + STAYS),
            Mark::Closing { since, .. } => Some(*since + CLOSE),
            Mark::Unsaved { at, .. } => Some(*at + UNSAVED),
            Mark::Gone { .. } | Mark::Back { .. } => None,
        });
        let adds = ui.adds_unsaved.values().map(|at| *at + UNSAVED);
        marks.chain(adds).min()
    })
}

/// What is due at `now`: the views draw again.
pub fn tick(now: Instant) -> Vec<Effect> {
    let mut effects = Vec::new();
    let due = ui(|ui| {
        let marks = ui.marks.values().any(|m| match m {
            Mark::Ticked { at, .. } => *at + STAYS <= now,
            Mark::Closing { since, .. } => *since + CLOSE <= now,
            Mark::Unsaved { at, .. } => *at + UNSAVED <= now,
            Mark::Gone { .. } | Mark::Back { .. } => false,
        });
        marks || ui.adds_unsaved.values().any(|at| *at + UNSAVED <= now)
    });
    if due {
        // Settled now, so that the deadline moves on.
        let day = crate::day::snapshot();
        let motion = motion();
        ui(|ui| settle(ui, &day, now, motion));
        effects.push(Effect::Wake(TASKS_NAMESPACE));
        let open = popover::with(|c| {
            matches!(c.open_item(), Some(Item::Tasks | Item::TasksMore)).then(|| c.refit())
        });
        if let Some(refit) = open {
            effects.extend(refit);
            effects.push(Effect::Wake(POPOVER_NAMESPACE));
        }
    }
    effects
}

// --- Words -----------------------------------------------------------------------------

/// The list's words, in the bar's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    fr: bool,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self {
            fr: lang == Lang::French,
        }
    }

    /// The group's header.
    pub fn header(self) -> &'static str {
        if self.fr { "À FAIRE" } else { "TO DO" }
    }

    /// The popover's title.
    pub fn title(self) -> &'static str {
        if self.fr { "À faire" } else { "To do" }
    }

    /// The `+`'s name.
    pub fn add(self) -> &'static str {
        if self.fr {
            "Ajouter une tâche"
        } else {
            "Add a task"
        }
    }

    pub fn placeholder(self) -> &'static str {
        if self.fr {
            "Nouvelle tâche…"
        } else {
            "New task…"
        }
    }

    pub fn nothing(self) -> &'static str {
        if self.fr {
            "Rien à faire."
        } else {
            "Nothing to do."
        }
    }

    /// "+ 4 autres", "+ 1 autre".
    pub fn more(self, n: usize) -> String {
        match (self.fr, n) {
            (true, 1) => "+ 1 autre".to_owned(),
            (true, n) => format!("+ {n} autres"),
            (false, n) => format!("+ {n} more"),
        }
    }

    /// A box's name: what a click does.
    pub fn tick(self, title: &str, ticked: bool) -> String {
        match (self.fr, ticked) {
            (true, false) => format!("Cocher « {title} »"),
            (true, true) => format!("Décocher « {title} »"),
            (false, false) => format!("Tick \"{title}\""),
            (false, true) => format!("Untick \"{title}\""),
        }
    }

    pub fn unsaved(self) -> &'static str {
        if self.fr {
            "Pas enregistré"
        } else {
            "Not saved"
        }
    }

    /// The mini item's name: how many tasks are open.
    pub fn count(self, n: usize) -> String {
        match (self.fr, n) {
            (true, 1) => "À faire : 1 tâche".to_owned(),
            (true, n) => format!("À faire : {n} tâches"),
            (false, 1) => "To do: 1 task".to_owned(),
            (false, n) => format!("To do: {n} tasks"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_words_in_french_and_english() {
        let fr = Words { fr: true };
        let en = Words { fr: false };
        assert_eq!(fr.header(), "À FAIRE");
        assert_eq!(en.header(), "TO DO");
        assert_eq!(fr.more(4), "+ 4 autres");
        assert_eq!(fr.more(1), "+ 1 autre");
        assert_eq!(en.more(4), "+ 4 more");
        assert_eq!(en.more(1), "+ 1 more");
        assert_eq!(fr.tick("Relire", false), "Cocher « Relire »");
        assert_eq!(fr.tick("Relire", true), "Décocher « Relire »");
        assert_eq!(en.tick("Read", false), "Tick \"Read\"");
        assert_eq!(en.tick("Read", true), "Untick \"Read\"");
        assert_eq!(fr.unsaved(), "Pas enregistré");
        assert_eq!(en.unsaved(), "Not saved");
        assert_eq!(fr.placeholder(), "Nouvelle tâche…");
        assert_eq!(en.nothing(), "Nothing to do.");
        assert_eq!(fr.count(3), "À faire : 3 tâches");
    }
}
