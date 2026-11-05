//! The window (variant A, the sidebar): its state, what changes it, and
//! what it says in its log.
//!
//! Every change is written at once by the file's worker, off this thread;
//! the window shows the file as the worker last read it. Each change can
//! be taken back from its toast or with `Ctrl+Z` (`Ctrl+Shift+Z` redoes).
//! While the file does not read, nothing can be changed and the window
//! offers to take back the last version that read.
//!
//! At rest it draws nothing: motions (a toast rising, a row lit, a switch
//! sliding, a menu opening) ask for frames only while they move.

mod places;
mod sound;
mod style;
mod view;

use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant, SystemTime},
};

use iced::{
    Event, Subscription, Task, event,
    futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded},
    keyboard::{self, Key, Modifiers, key::Named},
    widget::{self, operation},
    window,
};

pub use places::Places;

use crate::{
    activate,
    lang::{Lang, Words, w},
    model::{Kind, SectionId, Setting, Source, Value, settings_of},
    search::{self, Found},
    single::Again,
    store::{FileState, Raw, Reading, Slot, Snapshot},
    worker::{self, Ask, Heard, Recent, Why, Worker},
};

/// The window's size, and its least (logical pixels).
pub const SIZE: (f32, f32) = (1100.0, 760.0);
pub const LEAST: (f32, f32) = (860.0, 560.0);

/// How long toasts stay.
const TOAST: Duration = Duration::from_secs(6);
const UNDONE: Duration = Duration::from_millis(3500);
/// A slider is written this long after it stops moving.
const SLIDER_REST: Duration = Duration::from_millis(450);
/// The motions.
const TOAST_IN: Duration = Duration::from_millis(160);
const MENU_IN: Duration = Duration::from_millis(140);
const KNOB: Duration = Duration::from_millis(160);
/// How long a closing window waits for the writes it asked before it
/// goes anyway (a file on a disk that hangs).
const CLOSE_WAIT: Duration = Duration::from_secs(5);
/// A found row: lit after 250 ms, held 1 s, gone by 2.4 s.
const LIGHT_WAIT: Duration = Duration::from_millis(250);
const LIGHT_HELD: Duration = Duration::from_millis(1250);
const LIGHT_END: Duration = Duration::from_millis(2400);

/// The search field's id.
const SEARCH: &str = "search";

/// The window's background (the content's) and its text.
pub const BACKGROUND: iced::Color = style::CONTENT;
pub const TEXT: iced::Color = style::TEXT;

/// The fonts the window draws with, and its default one.
pub fn fonts() -> Vec<&'static [u8]> {
    style::FONTS.to_vec()
}

pub fn default_font() -> iced::Font {
    style::MANROPE
}

/// What the window was started with.
#[derive(Debug, Clone)]
pub struct Start {
    pub lang: Lang,
    pub places: worker::Places,
    pub section: Option<SectionId>,
    pub user: String,
    /// Where second launches knock, taken once by the window.
    pub listener: std::sync::Arc<Mutex<Option<std::os::unix::net::UnixListener>>>,
}

/// Which page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Section(SectionId),
}

/// Which piece holds the keyboard (besides the search field, iced's own).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Nothing,
    Search,
    /// The sidebar's rows: 0 the home, then the sections.
    Nav(usize),
    /// The page's controls, in order.
    Control(usize),
}

/// What a toast says, and what its button does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub text: String,
    pub action: ToastAction,
    pub since: Instant,
    serial: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastAction {
    None,
    /// "Annuler": the change it says is taken back.
    Undo,
    /// "Voir": the row changed by hand is shown.
    Show(Slot),
    /// A sound device's toast: what its button does.
    Sound(sound::Act),
}

/// A change written: what was there, what is there now.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Change {
    slot: Slot,
    before: Raw,
    after: Raw,
}

/// What `Ctrl+Z` takes back: a change of the file, or a choice of the
/// Son page (in PipeWire).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Undo {
    File(Change),
    Sound(sound::Choice),
}

/// Why a write was asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Doing {
    New,
    Undo,
    Redo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Pending {
    slot: Slot,
    raw: Raw,
    doing: Doing,
    /// The change undone or redone, which goes on the other stack once
    /// written.
    change: Option<Change>,
}

/// Whether the window stays: asked to close, it goes once the writes it
/// asked are on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Life {
    Open,
    /// Waiting for the worker's last writes.
    Closing,
    /// Its close asked of iced.
    Closed,
}

/// A slider moving, before its value is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sliding {
    slot: Slot,
    value: i64,
    serial: u64,
}

/// What the file's worker says, read by the window's subscription.
static HEARD: Mutex<Option<UnboundedReceiver<Message>>> = Mutex::new(None);

#[derive(Debug, Clone)]
pub enum Message {
    Window(Option<window::Id>),
    Heard(Heard),
    Again(Again),
    Key {
        key: Key,
        modifiers: Modifiers,
        text: Option<String>,
        captured: bool,
    },
    Query(String),
    Submit,
    ClearSearch,
    Home,
    Section(SectionId),
    Result(usize),
    Source(Source),
    Toggle(Slot),
    Menu(Slot),
    CloseMenu,
    Choose(Slot, Value),
    Slide(Slot, i64),
    Settled(u64),
    Undo,
    Redo,
    ToastAct,
    ToastClose,
    ToastGone(u64),
    Restore,
    OpenFile,
    Close,
    /// Closing has waited long enough for the writes.
    CloseAnyway,
    Drag,
    /// The Son page's.
    Sound(sound::Msg),
    Frame(Instant),
    Placed(Places),
    Scrolled,
    Nothing,
}

#[derive(Debug)]
pub struct App {
    lang: Lang,
    user: String,
    window: Option<window::Id>,
    worker: Worker,
    file: Option<PathBuf>,
    snapshot: Option<Snapshot>,
    last_good: Option<SystemTime>,
    source: Source,
    battery: Option<u8>,
    page: Page,
    query: String,
    results: Vec<Found>,
    selected: usize,
    /// The result opened, by its target's id.
    opened: Option<String>,
    menu: Option<(Slot, Instant)>,
    menu_index: usize,
    focus: Focus,
    toast: Option<Toast>,
    serial: u64,
    /// The row lit, by its setting's id, since then.
    light: Option<(&'static str, Instant)>,
    /// A found row to scroll to once the page is laid out.
    scroll_to: Option<&'static str>,
    undo: Vec<Undo>,
    redo: Vec<Undo>,
    pending: BTreeMap<u64, Pending>,
    sliding: Option<Sliding>,
    /// Switches sliding, since then.
    knobs: BTreeMap<&'static str, Instant>,
    recent: Vec<Recent>,
    places: Places,
    /// The frame's time, for the motions.
    now: Instant,
    reduced_motion: bool,
    life: Life,
    /// What the log said last.
    said: String,
    said_places: String,
    /// The Son page: PipeWire as last heard.
    sound: sound::Sound,
}

impl App {
    pub fn new(start: Start) -> (Self, Task<Message>) {
        style::load_fonts();
        let (sender, receiver) = unbounded();
        *HEARD.lock().unwrap_or_else(PoisonError::into_inner) = Some(receiver);
        let file = start.places.file.clone();
        let sound = sound::Sound::start(sender.clone());
        let to_worker = sender.clone();
        let worker = worker::start(start.places, move |heard| {
            to_worker.unbounded_send(Message::Heard(heard)).is_ok()
        });
        let listener = start
            .listener
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(listener) = listener {
            let again: UnboundedSender<Message> = sender;
            crate::single::listen(listener, move |ask| {
                again.unbounded_send(Message::Again(ask)).is_ok()
            });
        }
        let app = Self {
            lang: start.lang,
            user: start.user,
            window: None,
            worker,
            file,
            snapshot: None,
            last_good: None,
            source: Source::Ac,
            battery: None,
            page: start.section.map_or(Page::Home, Page::Section),
            query: String::new(),
            results: Vec::new(),
            selected: 0,
            opened: None,
            menu: None,
            menu_index: 0,
            focus: Focus::Nothing,
            toast: None,
            serial: 0,
            light: None,
            scroll_to: None,
            undo: Vec::new(),
            redo: Vec::new(),
            pending: BTreeMap::new(),
            sliding: None,
            knobs: BTreeMap::new(),
            recent: Vec::new(),
            places: Places::default(),
            now: Instant::now(),
            reduced_motion: false,
            life: Life::Open,
            said: String::new(),
            said_places: String::new(),
            sound,
        };
        let mut app = app;
        if start.section == Some(SectionId::Sound) {
            app.light = Some(("sound.output", Instant::now()));
        }
        (app, window::latest().map(Message::Window))
    }

    pub fn title(&self) -> String {
        w("Réglages", "Settings").get(self.lang).to_owned()
    }

    fn say(&self, words: Words) -> &'static str {
        words.get(self.lang)
    }

    /// The file reads and can be written.
    fn writable(&self) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(|s| s.state == FileState::Reads)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            Subscription::run(heard),
            event::listen_with(|event, status, _| match event {
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key,
                    modifiers,
                    text,
                    ..
                }) => Some(Message::Key {
                    key,
                    modifiers,
                    text: text.map(|t| t.to_string()),
                    captured: status == event::Status::Captured,
                }),
                Event::Window(window::Event::Resized(_)) => Some(Message::Scrolled),
                // A close the compositor asks: what is pending is written
                // first, as for ours.
                Event::Window(window::Event::CloseRequested) => Some(Message::Close),
                _ => None,
            }),
        ];
        if self.moving(Instant::now()) {
            subscriptions.push(window::frames().map(Message::Frame));
        }
        Subscription::batch(subscriptions)
    }

    /// Whether something moves at `now`, and frames are needed.
    fn moving(&self, now: Instant) -> bool {
        let within = |since: Instant, d: Duration| now.saturating_duration_since(since) < d;
        if self.reduced_motion {
            return self
                .light
                .is_some_and(|(_, since)| within(since, LIGHT_END));
        }
        self.toast
            .as_ref()
            .is_some_and(|t| within(t.since, TOAST_IN))
            || self.menu.is_some_and(|(_, since)| within(since, MENU_IN))
            || self
                .sound
                .menu
                .as_ref()
                .is_some_and(|(_, since)| within(*since, MENU_IN))
            || self
                .light
                .is_some_and(|(_, since)| within(since, LIGHT_END))
            || self.knobs.values().any(|since| within(*since, KNOB))
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let place = !matches!(message, Message::Frame(_) | Message::Placed(_));
        let task = self.handle(message);
        self.sound_listen();
        self.now = Instant::now();
        self.knobs
            .retain(|_, since| self.now.saturating_duration_since(*since) < KNOB);
        if self
            .light
            .is_some_and(|(_, since)| self.now.saturating_duration_since(since) >= LIGHT_END)
        {
            self.light = None;
        }
        self.log();
        let task = if self.life == Life::Closing && self.pending.is_empty() {
            Task::batch([task, self.close_now()])
        } else {
            task
        };
        if place {
            Task::batch([task, places()])
        } else {
            task
        }
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Window(id) => {
                self.window = id;
                Task::none()
            }
            Message::Heard(heard) => self.heard(heard),
            Message::Again(again) => self.again(again),
            Message::Key {
                key,
                modifiers,
                text,
                captured,
            } => self.key(&key, modifiers, text.as_deref(), captured),
            Message::Query(query) => {
                self.focus = Focus::Search;
                self.search(query);
                Task::none()
            }
            Message::Submit => self.open_result(self.selected),
            Message::ClearSearch => {
                self.search(String::new());
                Task::none()
            }
            Message::Home => {
                self.go(Page::Home);
                Task::none()
            }
            Message::Section(id) => {
                self.go(Page::Section(id));
                Task::none()
            }
            Message::Result(i) => self.open_result(i),
            Message::Source(source) => {
                self.source = source;
                self.menu = None;
                Task::none()
            }
            Message::Toggle(slot) => {
                let on = self.value(slot) == Some(Value::Bool(true));
                self.knobs.insert(slot.setting, Instant::now());
                self.change(slot, Value::Bool(!on))
            }
            Message::Menu(slot) => {
                if !self.writable() {
                    return Task::none();
                }
                self.menu = match self.menu {
                    Some((open, _)) if open == slot => None,
                    _ => Some((slot, Instant::now())),
                };
                self.menu_index = self.choice_index(slot).unwrap_or(0);
                Task::none()
            }
            Message::CloseMenu => {
                self.menu = None;
                self.sound.menu = None;
                Task::none()
            }
            Message::Choose(slot, value) => {
                self.menu = None;
                self.change(slot, value)
            }
            Message::Slide(slot, value) => self.slide(slot, value),
            Message::Settled(serial) => match self.sliding {
                Some(sliding) if sliding.serial == serial => {
                    self.sliding = None;
                    self.change(sliding.slot, Value::Int(sliding.value))
                }
                _ => Task::none(),
            },
            Message::Undo => self.undo(),
            Message::Redo => self.redo(),
            Message::ToastAct => match self.toast.as_ref().map(|t| t.action) {
                Some(ToastAction::Undo) => self.undo(),
                Some(ToastAction::Show(slot)) => {
                    self.toast = None;
                    self.show_row(slot.setting)
                }
                Some(ToastAction::Sound(act)) => self.sound_update(sound::Msg::Act(act)),
                _ => Task::none(),
            },
            Message::ToastClose => {
                self.toast = None;
                Task::none()
            }
            Message::ToastGone(serial) => {
                if self.toast.as_ref().is_some_and(|t| t.serial == serial) {
                    self.toast = None;
                }
                Task::none()
            }
            Message::Restore => {
                self.worker.ask(Ask::Restore);
                Task::none()
            }
            Message::OpenFile => {
                self.open_file();
                Task::none()
            }
            Message::Close => self.close(),
            Message::CloseAnyway => {
                if self.life == Life::Closing {
                    tracing::warn!(
                        writes = self.pending.len(),
                        "the file's worker has not written yet: the window closes anyway"
                    );
                    self.close_now()
                } else {
                    Task::none()
                }
            }
            Message::Drag => self.window.map_or_else(Task::none, window::drag),
            Message::Sound(msg) => self.sound_update(msg),
            Message::Frame(_) | Message::Scrolled | Message::Nothing => Task::none(),
            Message::Placed(places) => self.placed(places),
        }
    }

    // --- The file ---------------------------------------------------------------------

    fn heard(&mut self, heard: Heard) -> Task<Message> {
        match heard {
            Heard::Power { source, battery } => {
                self.source = source;
                self.battery = battery;
                Task::none()
            }
            Heard::Recent(recent) => {
                self.recent = recent;
                Task::none()
            }
            Heard::Failed { id, why } => {
                if let Some(id) = id {
                    self.pending.remove(&id);
                }
                let text = format!(
                    "{} {why}",
                    self.say(w("Rien n’est écrit :", "Nothing is written:"))
                );
                self.toast(text, ToastAction::None, TOAST)
            }
            Heard::Read {
                snapshot,
                why,
                before,
                last_good,
            } => {
                let was_broken = self
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| matches!(s.state, FileState::Broken { .. }));
                self.last_good = last_good;
                if snapshot.state == FileState::Reads {
                    self.reduced_motion = snapshot.reduced_motion;
                }
                self.snapshot = Some(snapshot);
                if !self.writable() {
                    self.menu = None;
                    self.sliding = None;
                }
                match why {
                    Why::Start => Task::none(),
                    Why::Written(id) => self.written(id, before),
                    Why::ByHand(changed) => self.by_hand(&changed, was_broken),
                    Why::Restored => {
                        let text = self
                            .say(w(
                                "Dernière version lisible reprise ; l’autre est gardée dans config.toml.broken.",
                                "The last readable version is back; the other is kept as config.toml.broken.",
                            ))
                            .to_owned();
                        self.toast(text, ToastAction::None, TOAST)
                    }
                }
            }
        }
    }

    /// A change of ours written: kept for its undo, and said.
    fn written(&mut self, id: u64, before: Vec<(Slot, Raw)>) -> Task<Message> {
        let Some(pending) = self.pending.remove(&id) else {
            return Task::none();
        };
        let before = before
            .into_iter()
            .find(|(slot, _)| *slot == pending.slot)
            .map_or(Raw::Remove, |(_, raw)| raw);
        let change = pending.change.clone().unwrap_or(Change {
            slot: pending.slot,
            before,
            after: pending.raw.clone(),
        });
        let Some(setting) = pending.slot.setting() else {
            return Task::none();
        };
        let name = self.slot_name(setting, pending.slot);
        let now = self.said_value(pending.slot);
        match pending.doing {
            Doing::New => {
                self.undo.push(Undo::File(change));
                self.redo.clear();
                let text = match self.lang {
                    Lang::French => format!("{name} : {now}"),
                    Lang::English => format!("{name}: {now}"),
                };
                self.toast(text, ToastAction::Undo, TOAST)
            }
            Doing::Undo | Doing::Redo => {
                if pending.doing == Doing::Undo {
                    self.redo.push(Undo::File(change));
                } else {
                    self.undo.push(Undo::File(change));
                }
                self.light = Some((setting.id, Instant::now()));
                if let Some(source) = pending.slot.source {
                    self.source = source;
                }
                let text = match self.lang {
                    Lang::French => format!("Annulé : {name} revient à {now}."),
                    Lang::English => format!("Undone: {name} is back to {now}."),
                };
                let text = if pending.doing == Doing::Redo {
                    match self.lang {
                        Lang::French => format!("Rétabli : {name} : {now}."),
                        Lang::English => format!("Redone: {name}: {now}."),
                    }
                } else {
                    text
                };
                self.toast(text, ToastAction::None, UNDONE)
            }
        }
    }

    /// The file changed on disk: every page shows what it holds now; the
    /// one row changed on the page that is open lights.
    fn by_hand(&mut self, changed: &[Slot], was_broken: bool) -> Task<Message> {
        if !self.writable() {
            self.toast = None;
            return Task::none();
        }
        let here: Vec<Slot> = changed
            .iter()
            .copied()
            .filter(|slot| {
                slot.setting()
                    .is_some_and(|s| Page::Section(s.section) == self.page)
            })
            .collect();
        let action = match here.as_slice() {
            [one] => {
                self.light = Some((one.setting, Instant::now()));
                if let Some(source) = one.source {
                    self.source = source;
                }
                ToastAction::Show(*one)
            }
            _ => ToastAction::None,
        };
        let text = if was_broken {
            w(
                "config.toml se relit : modifiable à nouveau.",
                "config.toml reads again: editable again.",
            )
        } else {
            w(
                "config.toml a changé : relu.",
                "config.toml changed: read again.",
            )
        };
        self.toast(self.say(text).to_owned(), action, TOAST)
    }

    /// What applies to `slot` now: the slider's value while it moves.
    fn value(&self, slot: Slot) -> Option<Value> {
        if let Some(sliding) = self.sliding
            && sliding.slot == slot
        {
            return Some(Value::Int(sliding.value));
        }
        self.snapshot.as_ref()?.value(slot)
    }

    fn reading(&self, slot: Slot) -> Reading {
        self.snapshot
            .as_ref()
            .map_or(Reading::Absent, |s| s.reading(slot).clone())
    }

    fn said_value(&self, slot: Slot) -> String {
        match (slot.setting(), self.value(slot)) {
            (Some(setting), Some(value)) => setting.say(&value, self.lang),
            _ => String::new(),
        }
    }

    /// `Mettre en veille après, sur batterie`.
    fn slot_name(&self, setting: &Setting, slot: Slot) -> String {
        let name = setting.name.get(self.lang);
        match slot.source {
            Some(source) => format!("{name}{}", source.aside().get(self.lang)),
            None => name.to_owned(),
        }
    }

    /// `value` written at `slot`, at once.
    fn change(&mut self, slot: Slot, value: Value) -> Task<Message> {
        if !self.writable() {
            return Task::none();
        }
        let understood = matches!(self.reading(slot), Reading::Value(_));
        if understood && self.value(slot) == Some(value.clone()) {
            return Task::none();
        }
        self.ask_write(slot, Raw::Set(value), Doing::New, None);
        Task::none()
    }

    fn ask_write(&mut self, slot: Slot, raw: Raw, doing: Doing, change: Option<Change>) {
        self.serial += 1;
        let id = self.serial;
        self.pending.insert(
            id,
            Pending {
                slot,
                raw: raw.clone(),
                doing,
                change,
            },
        );
        if !self.worker.ask(Ask::Write { slot, raw, id }) {
            self.pending.remove(&id);
        }
    }

    fn undo(&mut self) -> Task<Message> {
        let change = match self.undo.last() {
            Some(Undo::Sound(_)) => {
                let Some(Undo::Sound(choice)) = self.undo.pop() else {
                    return Task::none();
                };
                self.toast = None;
                return self.sound_undo(choice, false);
            }
            Some(Undo::File(_)) if self.writable() => match self.undo.pop() {
                Some(Undo::File(change)) => change,
                _ => return Task::none(),
            },
            _ => return Task::none(),
        };
        self.toast = None;
        self.ask_write(
            change.slot,
            change.before.clone(),
            Doing::Undo,
            Some(change),
        );
        Task::none()
    }

    fn redo(&mut self) -> Task<Message> {
        let change = match self.redo.last() {
            Some(Undo::Sound(_)) => {
                let Some(Undo::Sound(choice)) = self.redo.pop() else {
                    return Task::none();
                };
                return self.sound_undo(choice, true);
            }
            Some(Undo::File(_)) if self.writable() => match self.redo.pop() {
                Some(Undo::File(change)) => change,
                _ => return Task::none(),
            },
            _ => return Task::none(),
        };
        self.ask_write(change.slot, change.after.clone(), Doing::Redo, Some(change));
        Task::none()
    }

    fn slide(&mut self, slot: Slot, value: i64) -> Task<Message> {
        if !self.writable() {
            return Task::none();
        }
        self.serial += 1;
        let serial = self.serial;
        self.sliding = Some(Sliding {
            slot,
            value,
            serial,
        });
        Task::perform(tokio::time::sleep(SLIDER_REST), move |()| {
            Message::Settled(serial)
        })
    }

    fn toast(&mut self, text: String, action: ToastAction, lasts: Duration) -> Task<Message> {
        self.serial += 1;
        let serial = self.serial;
        tracing::debug!(text, "a toast");
        self.toast = Some(Toast {
            text,
            action,
            since: Instant::now(),
            serial,
        });
        Task::perform(tokio::time::sleep(lasts), move |()| {
            Message::ToastGone(serial)
        })
    }

    /// ✕, `Esc`, `Ctrl+W` or the compositor's close: a slider still moving
    /// is written now, and the window goes once the worker has written
    /// what it was asked (never waiting on this thread).
    fn close(&mut self) -> Task<Message> {
        if self.life != Life::Open {
            return Task::none();
        }
        if let Some(sliding) = self.sliding.take() {
            let _ = self.change(sliding.slot, Value::Int(sliding.value));
        }
        self.menu = None;
        if self.pending.is_empty() {
            return self.close_now();
        }
        tracing::info!(
            writes = self.pending.len(),
            "closing once the file is written"
        );
        self.life = Life::Closing;
        Task::perform(tokio::time::sleep(CLOSE_WAIT), |()| Message::CloseAnyway)
    }

    fn close_now(&mut self) -> Task<Message> {
        self.life = Life::Closed;
        self.sound.stop();
        self.window.map_or_else(iced::exit, window::close)
    }

    fn open_file(&self) {
        let Some(file) = self.file.clone() else {
            return;
        };
        let started = std::thread::Builder::new()
            .name("open the file".to_owned())
            .spawn(move || {
                match std::process::Command::new("xdg-open")
                    .arg(&file)
                    .stdin(std::process::Stdio::null())
                    .spawn()
                {
                    Ok(mut child) => {
                        let _ = child.wait();
                    }
                    Err(err) => tracing::warn!(%err, "xdg-open could not start"),
                }
            });
        if let Err(err) = started {
            tracing::warn!(%err, "the file is not opened");
        }
    }

    // --- Pages and the search ---------------------------------------------------------

    fn go(&mut self, page: Page) {
        if self.page != page {
            self.page = page;
            self.light = None;
            self.menu = None;
            self.sound.menu = None;
        }
        self.opened = None;
    }

    fn search(&mut self, query: String) {
        self.results = search::find(&query, self.lang);
        self.query = query;
        self.selected = 0;
        self.opened = None;
    }

    fn open_result(&mut self, i: usize) -> Task<Message> {
        let Some(found) = self.results.get(i).cloned() else {
            return Task::none();
        };
        self.selected = i;
        self.go(Page::Section(found.target.section()));
        self.opened = Some(found.target.id());
        match found.target {
            search::Target::Setting(setting) => {
                tracing::info!(setting = setting.id, "a result opened");
                self.show_row(setting.id)
            }
            search::Target::Outside(outside) => {
                tracing::info!(setting = outside.id, "a result opened");
                self.show_row(outside.id)
            }
            search::Target::Section(_) => {
                operation::snap_to(places::PAGE, widget::scrollable::RelativeOffset::START)
            }
        }
    }

    /// `setting`'s row centred on its page, then lit.
    fn show_row(&mut self, setting: &'static str) -> Task<Message> {
        if let Some(s) = crate::model::setting(setting) {
            self.go(Page::Section(s.section));
        } else if let Some(o) = crate::model::outside(setting) {
            self.go(Page::Section(o.section));
        }
        self.scroll_to = Some(setting);
        self.light = Some((setting, Instant::now()));
        Task::none()
    }

    /// Where things are now: a found row is scrolled to.
    fn placed(&mut self, places: Places) -> Task<Message> {
        self.places = places;
        self.log_places();
        let Some(setting) = self.scroll_to.take() else {
            return Task::none();
        };
        let (Some(row), Some(page)) = (
            self.places.at.get(&format!("row:{setting}")).copied(),
            self.places.page,
        ) else {
            return Task::none();
        };
        // In the content's own coordinates.
        let y = row.y + page.offset - page.viewport.y;
        let most = (page.content.height - page.viewport.height).max(0.0);
        let to = (y + row.height / 2.0 - page.viewport.height / 2.0).clamp(0.0, most);
        if (to - page.offset).abs() < 0.5 {
            return Task::none();
        }
        operation::scroll_to(
            places::PAGE,
            widget::scrollable::AbsoluteOffset { x: 0.0, y: to },
        )
    }

    // --- A second launch --------------------------------------------------------------

    fn again(&mut self, again: Again) -> Task<Message> {
        tracing::info!(section = ?again.section, "launched again: the window comes forward");
        if let Some(section) = again.section.as_deref().and_then(SectionId::from_slug) {
            self.go(Page::Section(section));
            // From the sound popover's link: the output in use lights.
            if section == SectionId::Sound {
                self.light = Some(("sound.output", Instant::now()));
            }
        }
        let (Some(id), Some(token)) = (self.window, again.token) else {
            return Task::none();
        };
        window::run(id, move |window| {
            if let Some(surface) = activate::Surface::of(window) {
                surface.activate(token);
            }
        })
        .map(|()| Message::Nothing)
    }

    // --- The keyboard -----------------------------------------------------------------

    /// The page's controls, in order: the segmented control first.
    fn controls(&self) -> Vec<Control> {
        let Page::Section(section) = self.page else {
            return Vec::new();
        };
        if section == SectionId::Sound {
            return self.sound_stops().into_iter().map(Control::Sound).collect();
        }
        let settings = settings_of(section);
        let mut out = Vec::new();
        if settings.iter().any(|s| s.group.per_source) {
            out.push(Control::Segmented);
        }
        for setting in settings {
            out.push(Control::Setting(Slot::of(setting, self.source)));
        }
        out
    }

    fn choice_index(&self, slot: Slot) -> Option<usize> {
        let setting = slot.setting()?;
        let Kind::Menu(choices) = &setting.kind else {
            return None;
        };
        let value = self.value(slot)?;
        choices.iter().position(|c| c.value == value)
    }

    fn key(
        &mut self,
        key: &Key,
        modifiers: Modifiers,
        text: Option<&str>,
        captured: bool,
    ) -> Task<Message> {
        let ctrl = modifiers.control();
        match key.as_ref() {
            Key::Character(c) if ctrl && c.eq_ignore_ascii_case("f") => {
                self.focus = Focus::Search;
                self.menu = None;
                return Task::batch([operation::focus(SEARCH), operation::select_all(SEARCH)]);
            }
            Key::Character(c) if ctrl && c.eq_ignore_ascii_case("w") => {
                return self.close();
            }
            Key::Character(c) if ctrl && c.eq_ignore_ascii_case("z") => {
                return if modifiers.shift() {
                    self.redo()
                } else {
                    self.undo()
                };
            }
            Key::Named(Named::Escape) => {
                if self.menu.take().is_some() || self.sound.menu.take().is_some() {
                    return Task::none();
                }
                if !self.query.is_empty() {
                    self.search(String::new());
                    self.focus = Focus::Search;
                    return operation::focus(SEARCH);
                }
                // The field that had the keyboard lets it go (iced's own
                // Esc, captured), then the window closes.
                if captured || self.focus == Focus::Search {
                    self.focus = Focus::Nothing;
                    return operation::focus("nothing");
                }
                return self.close();
            }
            _ => {}
        }
        if let Some((slot, _)) = self.menu {
            return self.menu_key(slot, key);
        }
        if self.sound.menu.is_some() {
            return self.sound_menu_key(key);
        }
        match key.as_ref() {
            Key::Named(Named::Tab) => return self.tab(modifiers.shift()),
            Key::Named(Named::ArrowDown | Named::ArrowUp) if !self.query.is_empty() => {
                let down = key.as_ref() == Key::Named(Named::ArrowDown);
                let last = self.results.len().saturating_sub(1);
                self.selected = if down {
                    (self.selected + 1).min(last)
                } else {
                    self.selected.saturating_sub(1)
                };
                return Task::none();
            }
            _ => {}
        }
        match self.focus {
            Focus::Nav(i) => return self.nav_key(i, key),
            Focus::Control(i) => return self.control_key(i, key, modifiers),
            Focus::Search | Focus::Nothing => {}
        }
        // A letter typed while nothing has the keyboard starts the search
        // (and one typed before the field takes the keyboard goes in it).
        if let Some(text) = text
            && !captured
            && !ctrl
            && !modifiers.alt()
            && text.chars().all(|c| !c.is_control())
            && text.chars().any(char::is_alphanumeric)
            && !text.is_empty()
        {
            self.focus = Focus::Search;
            let query = format!("{}{text}", self.query);
            self.search(query);
            return Task::batch([
                operation::focus(SEARCH),
                operation::move_cursor_to_end(SEARCH),
            ]);
        }
        Task::none()
    }

    fn tab(&mut self, back: bool) -> Task<Message> {
        // The search field, the sidebar's rows (or none while searching),
        // then the page's controls.
        let nav = if self.query.is_empty() {
            1 + SectionId::ALL.len()
        } else {
            0
        };
        let controls = self.controls().len();
        let mut order = vec![Focus::Search];
        order.extend((0..nav).map(Focus::Nav));
        order.extend((0..controls).map(Focus::Control));
        let at = order.iter().position(|f| *f == self.focus);
        let next = match (at, back) {
            (None, false) => 0,
            (None, true) => order.len() - 1,
            (Some(i), false) => (i + 1) % order.len(),
            (Some(i), true) => (i + order.len() - 1) % order.len(),
        };
        self.focus = order[next];
        if self.focus == Focus::Search {
            operation::focus(SEARCH)
        } else {
            operation::focus("nothing")
        }
    }

    fn nav_key(&mut self, i: usize, key: &Key) -> Task<Message> {
        let last = SectionId::ALL.len();
        match key.as_ref() {
            Key::Named(Named::ArrowDown) => self.focus = Focus::Nav((i + 1).min(last)),
            Key::Named(Named::ArrowUp) => self.focus = Focus::Nav(i.saturating_sub(1)),
            Key::Named(Named::Enter | Named::Space) => match i.checked_sub(1) {
                None => self.go(Page::Home),
                Some(s) => {
                    if let Some(id) = SectionId::ALL.get(s) {
                        self.go(Page::Section(*id));
                    }
                }
            },
            _ => {}
        }
        Task::none()
    }

    fn control_key(&mut self, i: usize, key: &Key, modifiers: Modifiers) -> Task<Message> {
        let Some(control) = self.controls().get(i).copied() else {
            return Task::none();
        };
        let named = match key.as_ref() {
            Key::Named(named) => named,
            _ => return Task::none(),
        };
        match control {
            Control::Sound(stop) => self.sound_key(stop, named, modifiers),
            Control::Segmented => {
                match named {
                    Named::ArrowLeft => self.source = Source::Battery,
                    Named::ArrowRight => self.source = Source::Ac,
                    _ => {}
                }
                Task::none()
            }
            Control::Setting(slot) => {
                let Some(setting) = slot.setting() else {
                    return Task::none();
                };
                if self.inert(setting) {
                    return Task::none();
                }
                match (&setting.kind, named) {
                    (Kind::Switch, Named::Space | Named::Enter) => {
                        self.update(Message::Toggle(slot))
                    }
                    (Kind::Menu(_), Named::Space | Named::Enter) => {
                        self.update(Message::Menu(slot))
                    }
                    (Kind::Menu(_), Named::ArrowDown) if modifiers.alt() => {
                        self.update(Message::Menu(slot))
                    }
                    (Kind::Slider { min, max, step, .. }, _) => {
                        let now = match self.value(slot) {
                            Some(Value::Int(n)) => n,
                            _ => *min,
                        };
                        let quarter = ((max - min) / 4 / step).max(1) * step;
                        let to = match named {
                            Named::ArrowLeft => now - step,
                            Named::ArrowRight => now + step,
                            Named::PageDown => now - quarter,
                            Named::PageUp => now + quarter,
                            _ => return Task::none(),
                        };
                        self.slide(slot, to.clamp(*min, *max))
                    }
                    _ => Task::none(),
                }
            }
        }
    }

    fn menu_key(&mut self, slot: Slot, key: &Key) -> Task<Message> {
        let Some(Kind::Menu(choices)) = slot.setting().map(|s| &s.kind) else {
            return Task::none();
        };
        match key.as_ref() {
            Key::Named(Named::ArrowDown) => {
                self.menu_index = (self.menu_index + 1).min(choices.len().saturating_sub(1));
            }
            Key::Named(Named::ArrowUp) => self.menu_index = self.menu_index.saturating_sub(1),
            Key::Named(Named::Enter | Named::Space) => {
                if let Some(choice) = choices.get(self.menu_index) {
                    let value = choice.value.clone();
                    return self.update(Message::Choose(slot, value));
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// Whether `setting`'s control does not apply now: the file does not
    /// read, or the switch it needs is off.
    fn inert(&self, setting: &Setting) -> bool {
        if !self.writable() {
            return true;
        }
        setting.needs.is_some_and(|needs| {
            crate::model::setting(needs).is_some_and(|switch| {
                self.value(Slot::of(switch, self.source)) == Some(Value::Bool(false))
            })
        })
    }

    /// The row lit, and how much (0 to 1), at the frame's time.
    fn lit(&self, setting: &str) -> f32 {
        let Some((id, since)) = self.light else {
            return 0.0;
        };
        if id != setting {
            return 0.0;
        }
        let t = self.now.saturating_duration_since(since);
        if self.reduced_motion {
            return (1.0 - t.as_secs_f32() / LIGHT_END.as_secs_f32()).clamp(0.0, 1.0);
        }
        if t < LIGHT_WAIT {
            return 0.0;
        }
        if t < LIGHT_HELD {
            return 1.0;
        }
        let fading = (t - LIGHT_HELD).as_secs_f32() / (LIGHT_END - LIGHT_HELD).as_secs_f32();
        (1.0 - fading).clamp(0.0, 1.0)
    }

    // --- What the log says ------------------------------------------------------------

    /// What the window shows, said in the log as JSON when it changes:
    /// tests read it, as screen readers will (#11 of the bar's spec).
    fn log(&mut self) {
        let shown = view::shown(self);
        let text = shown.to_string();
        if text != self.said {
            tracing::info!(target: "hocus_settings::shows", "shows {text}");
            self.said = text;
        }
    }

    fn log_places(&mut self) {
        let at: serde_json::Map<String, serde_json::Value> = self
            .places
            .at
            .iter()
            .filter(|(_, r)| r.width > 0.0 && r.height > 0.0)
            .map(|(id, r)| {
                (
                    id.clone(),
                    serde_json::json!([
                        r.x.round(),
                        r.y.round(),
                        r.width.round(),
                        r.height.round()
                    ]),
                )
            })
            .collect();
        let text = serde_json::Value::Object(at).to_string();
        if text != self.said_places {
            tracing::debug!(target: "hocus_settings::places", "places {text}");
            self.said_places = text;
        }
    }
}

/// A control of the page that takes the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Segmented,
    Setting(Slot),
    Sound(sound::Stop),
}

/// The places of what the window draws, after it is laid out.
fn places() -> Task<Message> {
    iced::advanced::widget::operate(places::Collect::default()).map(Message::Placed)
}

/// What the worker and second launches say, as messages.
fn heard() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::{SinkExt as _, StreamExt as _};
    iced::stream::channel(8, async |mut output| {
        let taken = HEARD.lock().unwrap_or_else(PoisonError::into_inner).take();
        let Some(mut heard) = taken else {
            std::future::pending::<()>().await;
            return;
        };
        while let Some(message) = heard.next().await {
            if output.send(message).await.is_err() {
                return;
            }
        }
    })
}
