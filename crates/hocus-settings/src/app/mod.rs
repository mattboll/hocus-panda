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

mod account;
mod agents_page;
mod appearance_page;
mod applications;
mod bar_page;
mod bluetooth;
mod captures_page;
mod desktops_page;
mod desktops_rules;
mod desktops_trial;
mod keyboard_page;
mod keys_in;
mod mouse;
mod network;
mod perf_page;
mod places;
mod power_page;
mod screens;
mod sound;
mod style;
mod time;
mod view;
mod wallpaper_page;
mod wheels;

use hocus_words::settings::app as words;
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

pub use bluetooth::stop as stop_bluetooth;
pub use places::Places;

use crate::{
    activate,
    lang::{Lang, Words, w},
    model::{Kind, SectionId, Setting, Source, Value, settings_of},
    search::{self, Found},
    single::Again,
    store::{FileState, KeyEdit, Raw, Reading, Slot, Snapshot},
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
/// Collections of the places in a row that may disagree before they are
/// said anyway ([`App::placed`]).
const UNSETTLED: u8 = 4;
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
pub fn background() -> iced::Color {
    style::CONTENT.get()
}

pub fn text() -> iced::Color {
    style::TEXT.get()
}

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
    /// A Bluetooth device to pair on its page, by its address.
    pub pair: Option<String>,
    /// A network whose sheet opens once it is around (`--join`, 6.6).
    pub join: Option<String>,
    /// What the search shows (`--search`, the actions palette, #736).
    pub search: Option<String>,
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
enum Change {
    Slot {
        slot: Slot,
        before: Raw,
        after: Raw,
        /// Changes made together (a shortcut taken from another action,
        /// 6.8) are taken back together.
        batch: Option<u64>,
    },
    /// Keys a page wrote itself (the screens'), and what it says of them.
    Keys {
        before: Vec<KeyEdit>,
        after: Vec<KeyEdit>,
        said: KeysSaid,
    },
}

impl Change {
    /// The batch it was made in, taken back with it.
    fn batch(&self) -> Option<u64> {
        match self {
            Self::Slot { batch, .. } => *batch,
            Self::Keys { .. } => None,
        }
    }
}

/// What the toasts say of keys a page wrote, done, undone or redone; the
/// row lit by an undo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeysSaid {
    done: String,
    undone: String,
    redone: String,
    row: &'static str,
}

/// What a write asks.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Write {
    Slot(Slot, Raw),
    Keys(Vec<KeyEdit>, KeysSaid),
}

/// What `Ctrl+Z` takes back: a change of the file, a choice of the
/// Son page (in PipeWire), or a picture of one's own removed (6.3).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Undo {
    File(Change),
    Sound(sound::Choice),
    /// Bluetooth turned off on its page (in BlueZ).
    Bluetooth(bluetooth::Choice),
    /// A time zone chosen (in timedated, 6.11).
    Time(time::Choice),
    /// A display name or an avatar set (in AccountsService, 6.11).
    Account(account::Choice),
    /// A change of the Réseau page's (in NetworkManager, rfkill).
    Network(network::Choice),
    /// The power mode (power-profiles-daemon's, 6.10).
    Power(power_page::ModeChoice),
    /// A tool plugged in or out, herdr followed or not (6.16).
    Agents(agents_page::Choice),
    /// A default application or a start-up one (mimeapps.list, autostart,
    /// 6.15).
    Apps(applications::Choice),
    Picture {
        path: PathBuf,
        bytes: std::sync::Arc<Vec<u8>>,
    },
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
    write: Write,
    doing: Doing,
    /// The change undone or redone, which goes on the other stack once
    /// written.
    change: Option<Change>,
    batch: Option<u64>,
    /// What its toast says instead of the setting and its value; empty:
    /// no toast (another write of its batch says it).
    say: Option<String>,
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
        physical: keyboard::key::Physical,
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
    /// The Écrans page's own (6.9).
    Screens(screens::Msg),
    Frame(Instant),
    /// The screen's scale the window draws at, changed.
    Rescaled(f32),
    Placed(Places),
    /// The page scrolled or the window resized: its places said again.
    Scrolled,
    /// A wheel the window has taken, the page scrolled by it or not.
    Wheeled,
    /// The mice and touchpads page's (6.12).
    Mouse(mouse::Msg),
    /// The keyboard's page and its shortcuts' (6.8).
    Keys(keyboard_page::Msg),
    /// The wallpaper's page (6.3).
    Wallpaper(wallpaper_page::Msg),
    /// The bar's page (6.4).
    Bar(bar_page::Msg),
    /// The Bluetooth page's (6.7).
    Bluetooth(bluetooth::Msg),
    /// The Date et heure page's (6.11).
    Time(time::Msg),
    /// The Compte page's (6.11).
    Account(account::Msg),
    /// The desktops and windows' page (6.14).
    Desktops(desktops_page::Msg),
    /// The screenshots and clipboard's page (6.17).
    Captures(captures_page::Msg),
    /// The network's page (6.6).
    Network(network::Msg),
    /// The Énergie page's (6.10).
    Power(power_page::Msg),
    /// The Agents page's (6.16).
    Agents(agents_page::Msg),
    /// The Applications page's (6.15).
    Apps(applications::Msg),
    /// The Performances page's (#655).
    Perf(perf_page::Msg),
    Nothing,
}

#[derive(Debug)]
pub struct App {
    lang: Lang,
    /// The session's language, which `[locale] language = "system"` means.
    system_lang: Lang,
    /// `[appearance] text-scale`: the window is drawn this much larger.
    text_scale: f32,
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
    /// The Écrans page (6.9).
    screens: screens::Page,
    /// The scale the window draws at, as the compositor told it.
    scale: f32,
    /// The mice and touchpads, and the test area (6.12).
    mouse: mouse::Mouse,
    /// The layouts, the shortcuts' sub-page, a shortcut recorded (6.8).
    keys: keyboard_page::Keys,
    /// What the log said last.
    said: String,
    said_places: String,
    /// The wheels the window has taken, said with its places (`wheels`).
    wheels: u64,
    /// The serial of the last "shows" line, said with the places collected
    /// after it (`shown`).
    shows_serial: u64,
    /// Collections of the places in a row that did not agree (see
    /// [`App::placed`]).
    unsettled: u8,
    /// The Son page: PipeWire as last heard.
    sound: sound::Sound,
    /// The wallpaper's page: its thumbnails, one's own pictures (6.3).
    wallpaper: wallpaper_page::State,
    /// The bar's page: its preview's motions and clock (6.4).
    bar: bar_page::State,
    /// The Bluetooth page: BlueZ as last heard (6.7).
    bluetooth: bluetooth::State,
    /// The Date et heure page: timedated as last heard (6.11).
    time: time::State,
    /// The Compte page: AccountsService as last heard, the password's
    /// sheet (6.11).
    account: account::State,
    /// The desktops and windows' page: the compositor's windows, a name
    /// changed, a desktop taken out waiting for it (6.14).
    desktops: desktops_page::State,
    /// The screenshots and clipboard's page: the history's count, a
    /// recording under way, "Effacer…" asking (6.17).
    captures: captures_page::State,
    /// The network's page: NetworkManager and the radios as last said
    /// (6.6).
    network: network::State,
    /// The Énergie page: the battery, the modes and logind as last said
    /// (6.10).
    power: power_page::State,
    /// The Agents page: the tools as the compositor says them, the
    /// consent sheet, the preview and the pandas' motions (6.16).
    agents: agents_page::State,
    /// The Applications page: what its thread read, its menus and sheet
    /// (6.15).
    apps: applications::State,
    /// The Performances page: the day its thread read (#655).
    perf: perf_page::State,
}

impl App {
    pub fn new(start: Start) -> (Self, Task<Message>) {
        style::load_fonts();
        let (sender, receiver) = unbounded();
        *HEARD.lock().unwrap_or_else(PoisonError::into_inner) = Some(receiver);
        let file = start.places.file.clone();
        let sound = sound::Sound::start(sender.clone());
        let bluetooth = bluetooth::State::start(sender.clone());
        let time = time::State::new(sender.clone());
        let account = account::State::new(sender.clone());
        let mut network = network::State::start(&sender, start.places.state.clone());
        network.joining = start.join.clone();
        let power = power_page::State::start(&sender);
        let agents = agents_page::State::with(&sender);
        let apps = applications::State::new(sender.clone());
        let perf = perf_page::State::new(&sender, start.places.state.clone());
        let to_worker = sender.clone();
        let worker = worker::start(start.places, move |heard| {
            to_worker.unbounded_send(Message::Heard(heard)).is_ok()
        });
        let listener = start
            .listener
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let to_screens = sender.clone();
        if let Some(socket) = crate::devices::socket() {
            let devices = sender.clone();
            crate::devices::follow(socket, move |list| {
                devices
                    .unbounded_send(Message::Mouse(mouse::Msg::Devices(list)))
                    .is_ok()
            });
        }
        let layouts = sender.clone();
        keyboard_page::read_registry(start.lang, move |registry, system| {
            let _ = layouts.unbounded_send(Message::Keys(keyboard_page::Msg::Registry(
                registry, system,
            )));
        });
        if let Some(listener) = listener {
            let again: UnboundedSender<Message> = sender;
            crate::single::listen(listener, move |ask| {
                again.unbounded_send(Message::Again(ask)).is_ok()
            });
        }
        let mut app = Self {
            lang: start.lang,
            system_lang: start.lang,
            text_scale: 1.0,
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
            screens: screens::Page::default(),
            scale: 1.0,
            mouse: mouse::Mouse::default(),
            keys: keyboard_page::Keys::default(),
            said: String::new(),
            said_places: String::new(),
            wheels: 0,
            shows_serial: 0,
            unsettled: 0,
            sound,
            wallpaper: wallpaper_page::State::default(),
            bar: bar_page::State::default(),
            bluetooth,
            time,
            account,
            desktops: desktops_page::State::default(),
            captures: captures_page::State::default(),
            network,
            power,
            agents,
            apps,
            perf,
        };
        if let Some(address) = start.pair {
            app.bluetooth_pair_address(address);
        }
        app.wallpaper_start(to_screens.clone());
        app.screens_open(to_screens);
        if start.section == Some(SectionId::Sound) {
            app.light = Some(("sound.output", Instant::now()));
        }
        let mut first = window::latest().map(Message::Window);
        if let Some(query) = start.search {
            tracing::info!("the Settings open searching");
            app.focus = Focus::Search;
            app.search(query);
            first = Task::batch([first, operation::focus(SEARCH)]);
        }
        (app, first)
    }

    pub fn title(&self) -> String {
        w(words::SETTINGS).get(self.lang).to_owned()
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
            // The keys are not heard here but from the window's widgets, in
            // their order (`keys_in`).
            event::listen_with(|event, _, _| match event {
                Event::Window(window::Event::Resized(_)) => Some(Message::Scrolled),
                Event::Window(window::Event::Rescaled(scale)) => Some(Message::Rescaled(scale)),
                // The test area says which device scrolled (6.12).
                Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
                    Some(Message::Mouse(mouse::Msg::Wheel(delta)))
                }
                // A close the compositor asks: what is pending is written
                // first, as for ours.
                Event::Window(window::Event::CloseRequested) => Some(Message::Close),
                // The gestures' diagrams move only while it has the focus
                // (6.13).
                Event::Window(window::Event::Focused) => {
                    Some(Message::Mouse(mouse::Msg::Focused(true)))
                }
                Event::Window(window::Event::Unfocused) => {
                    Some(Message::Mouse(mouse::Msg::Focused(false)))
                }
                _ => None,
            }),
        ];
        // The display name's field let go (6.11).
        if let Some(pressed) = self.account_subscription() {
            subscriptions.push(pressed);
        }
        // The terminal's command let go (6.15).
        if let Some(pressed) = self.apps_subscription() {
            subscriptions.push(pressed);
        }
        if self.moving(Instant::now()) {
            subscriptions.push(window::frames().map(Message::Frame));
        }
        // A pairing code's countdown, each second while its sheet shows.
        if self.bluetooth_ticking() {
            subscriptions.push(iced::time::every(Duration::from_secs(1)).map(|_| Message::Nothing));
        }
        // The Écrans page's drawing: a screen pressed, then following the
        // pointer wherever it goes in the window.
        if self.screens_shown_now() {
            subscriptions.push(event::listen_with(|event, _, _| match event {
                Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::Screens(screens::Msg::Pointer(position)))
                }
                Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                    Some(Message::Screens(screens::Msg::PressAt))
                }
                Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                    Some(Message::Screens(screens::Msg::Release))
                }
                _ => None,
            }));
        }
        Subscription::batch(subscriptions)
    }

    /// Whether something moves at `now`: frames are needed.
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
            || self.screens_moving(now)
            || self.mouse.moving(now)
            || self.gestures_moving()
            || self.bar_moving(now)
            || self.bluetooth_moving()
            || self.bluetooth_menu_moving(now)
            || self.network_moving()
            || self.trial_moving(now)
            || self.rules_moving(now)
            || self.agents_moving(now)
            || self.apps_moving(now)
            || self.captures_moving(now)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let mut place = !matches!(message, Message::Frame(_) | Message::Placed(_));
        let shown = self.shows_serial;
        let task = self.handle(message);
        let task = Task::batch([
            task,
            self.bar_after(),
            self.time_after(),
            self.account_after(),
            self.apps_after(),
        ]);
        self.sound_listen();
        self.bluetooth_want();
        self.wallpaper_want();
        self.network_visit();
        self.agents_visit();
        self.perf_after();
        self.network_after_key();
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
        // Every "shows" line is followed by places at least as recent,
        // whatever changed it (a light fading out at a frame too): a test
        // waits for those before it clicks (#684).
        place |= self.shows_serial != shown;
        let task = if self.life == Life::Closing && self.pending.is_empty() {
            Task::batch([task, self.close_now()])
        } else {
            task
        };
        if place {
            Task::batch([task, places(self.wheels, self.shows_serial)])
        } else {
            task
        }
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        // A desktop's name being changed is written as the pointer goes
        // elsewhere (6.14).
        if self.desktops_editing()
            && matches!(
                message,
                Message::Toggle(_)
                    | Message::Menu(_)
                    | Message::Section(_)
                    | Message::Home
                    | Message::Query(_)
                    | Message::Close
                    | Message::Result(_)
            )
        {
            self.desktops_commit();
        }
        match message {
            Message::Window(id) => {
                self.window = id;
                Task::none()
            }
            Message::Heard(heard) => self.heard(heard),
            Message::Again(again) => self.again(again),
            Message::Key {
                key,
                physical,
                modifiers,
                text,
                captured,
            } => {
                // A shortcut recorded takes every key (6.8).
                if self.keys.recording.is_some() {
                    return self.record_key(&key, &physical, modifiers);
                }
                if self.keys.conflict.is_some() && key.as_ref() == Key::Named(Named::Escape) {
                    return self.keys_update(keyboard_page::Msg::Cancel);
                }
                if self.keys.adding.is_some() && key.as_ref() == Key::Named(Named::Escape) {
                    self.keys.adding = None;
                    return Task::none();
                }
                // The Agents page's consent sheet takes every key (6.16).
                if let Some(task) = self.agents_key_first(&key) {
                    return task;
                }
                // The time zone picker's, the time's fields' (6.11).
                if let Some(task) = self.time_key_first(&key, captured) {
                    return task;
                }
                // The avatar's popover, the password's sheet (6.11).
                if let Some(task) = self.account_key_first(&key) {
                    return task;
                }
                // The Applications page's menus, sheet and field (6.15).
                if let Some(task) = self.apps_key_first(&key, modifiers) {
                    return task;
                }
                // A menu of the rules: `Esc` closes it.
                if key.as_ref() == Key::Named(Named::Escape) && self.rules_escape() {
                    return Task::none();
                }
                // "+ Ajouter"'s menu, "Effacer…"'s confirmation (6.17).
                if key.as_ref() == Key::Named(Named::Escape)
                    && self.page == Page::Section(SectionId::Captures)
                    && self.captures_escape()
                {
                    return Task::none();
                }
                // A desktop's name: `Esc` keeps the old one, `Tab` writes.
                if self.desktops_editing() {
                    match key.as_ref() {
                        Key::Named(Named::Escape) => {
                            self.desktops_cancel();
                            return operation::focus("nothing");
                        }
                        Key::Named(Named::Tab) => self.desktops_commit(),
                        _ => {}
                    }
                }
                self.key(&key, modifiers, text.as_deref(), captured)
            }
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
                self.keys.sub = false;
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
                self.agents_chose(slot, &Value::Bool(!on));
                let say = self.captures_toggle_toast(slot, !on);
                self.change_saying(slot, Value::Bool(!on), say)
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
                self.bluetooth.menu = None;
                Task::none()
            }
            Message::Choose(slot, value) => {
                self.menu = None;
                if self.theme_chose(slot, &value) {
                    return Task::none();
                }
                self.bar_chose(slot, &value);
                self.agents_chose(slot, &value);
                let say = self.desktops_toast(slot, &value);
                self.change_saying(slot, value, say)
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
            Message::Screens(msg) => self.screens_update(msg),
            Message::Frame(_) | Message::Scrolled | Message::Nothing => Task::none(),
            Message::Wheeled => {
                self.wheels += 1;
                Task::none()
            }
            Message::Rescaled(scale) => {
                self.scale = scale;
                Task::none()
            }
            Message::Placed(places) => self.placed(places),
            Message::Mouse(msg) => self.mouse_update(msg),
            Message::Wallpaper(msg) => self.wallpaper_update(msg),
            Message::Bar(msg) => self.bar_update(msg),
            Message::Bluetooth(msg) => self.bluetooth_update(msg),
            Message::Time(msg) => self.time_update(msg),
            Message::Account(msg) => self.account_update(msg),
            Message::Desktops(msg) => self.desktops_update(msg),
            Message::Captures(msg) => self.captures_update(msg),
            Message::Keys(msg) => {
                // The trial area's move key, held for real (6.14).
                if let keyboard_page::Msg::Mods(mods) = &msg {
                    self.trial_mods(*mods);
                }
                self.keys_update(msg)
            }
            Message::Network(msg) => self.network_update(msg),
            Message::Power(msg) => self.power_update(msg),
            Message::Agents(msg) => self.agents_update(msg),
            Message::Apps(msg) => self.apps_update(msg),
            Message::Perf(msg) => self.perf_update(msg),
        }
    }

    // --- The file ---------------------------------------------------------------------

    fn heard(&mut self, heard: Heard) -> Task<Message> {
        match heard {
            Heard::Power { source, battery } => {
                self.power.kernel_on_battery = source == Source::Battery;
                // UPower, once heard, says it better (`power_page`).
                if self.power.now.as_ref().is_none_or(|n| !n.upower) {
                    self.source = source;
                }
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
                let text = format!("{} {why}", self.say(w(words::NOTHING_IS_WRITTEN)));
                self.toast(text, ToastAction::None, TOAST)
            }
            Heard::Read {
                snapshot,
                why,
                before,
                before_keys,
                last_good,
            } => {
                let was_broken = self
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| matches!(s.state, FileState::Broken { .. }));
                self.last_good = last_good;
                if snapshot.state == FileState::Reads {
                    self.reduced_motion = snapshot.reduced_motion;
                    let lang = snapshot.language.unwrap_or(self.system_lang);
                    if lang != self.lang {
                        tracing::info!(?lang, "the window's language");
                        self.lang = lang;
                    }
                    self.text_scale = snapshot.text_scale as f32 / 100.0;
                }
                let reads = snapshot.state == FileState::Reads;
                self.snapshot = Some(*snapshot);
                // The theme, the wallpaper's accent the primary screen's.
                if reads {
                    self.wallpaper_theme();
                }
                if !self.writable() {
                    self.menu = None;
                    self.sliding = None;
                }
                match why {
                    Why::Start => Task::none(),
                    Why::Written(id) => self.written(id, before, before_keys),
                    Why::ByHand(changed) => self.by_hand(&changed, was_broken),
                    Why::Restored => {
                        let text = self
                            .say(w(words::THE_LAST_READABLE_VERSION_IS_BACK))
                            .to_owned();
                        self.toast(text, ToastAction::None, TOAST)
                    }
                }
            }
        }
    }

    /// A change of ours written: kept for its undo, and said.
    fn written(
        &mut self,
        id: u64,
        before: Vec<(Slot, Raw)>,
        before_keys: Vec<KeyEdit>,
    ) -> Task<Message> {
        let Some(pending) = self.pending.remove(&id) else {
            return Task::none();
        };
        let (slot, raw) = match pending.write {
            Write::Slot(slot, raw) => (slot, raw),
            Write::Keys(edits, said) => {
                let change = pending.change.clone().unwrap_or(Change::Keys {
                    before: before_keys,
                    after: edits,
                    said: said.clone(),
                });
                return match pending.doing {
                    Doing::New => {
                        self.undo.push(Undo::File(change));
                        self.redo.clear();
                        self.toast(said.done, ToastAction::Undo, TOAST)
                    }
                    Doing::Undo => {
                        self.redo.push(Undo::File(change));
                        self.light = Some((said.row, Instant::now()));
                        self.toast(said.undone, ToastAction::None, UNDONE)
                    }
                    Doing::Redo => {
                        self.undo.push(Undo::File(change));
                        self.light = Some((said.row, Instant::now()));
                        self.toast(said.redone, ToastAction::None, UNDONE)
                    }
                };
            }
        };
        let before = before
            .into_iter()
            .find(|(s, _)| *s == slot)
            .map_or(Raw::Remove, |(_, raw)| raw);
        let change = pending.change.clone().unwrap_or(Change::Slot {
            slot,
            before,
            after: raw,
            batch: pending.batch,
        });
        let Some(setting) = slot.setting() else {
            return Task::none();
        };
        let name = self.slot_name(setting, slot);
        let now = self.said_value(slot);
        match pending.doing {
            Doing::New => {
                self.undo.push(Undo::File(change));
                self.redo.clear();
                let text = match (pending.say, self.lang) {
                    (Some(said), _) if said.is_empty() => return Task::none(),
                    (Some(said), _) => said,
                    (None, Lang::French) => format!("{name} : {now}"),
                    (None, Lang::English) => format!("{name}: {now}"),
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
                if let Some(source) = slot.source {
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
            w(words::CONFIG_TOML_READS_AGAIN_EDITABLE_AGAIN)
        } else {
            w(words::CONFIG_TOML_CHANGED_READ_AGAIN)
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
        let snapshot = self.snapshot.as_ref()?;
        match snapshot.reading(slot) {
            Reading::Value(value) => Some(value.clone()),
            _ => Some(self.default_of(slot)?),
        }
    }

    /// What applies to `slot` when the file says nothing: its default, or
    /// what a device says (the touchpad's own right click).
    fn default_of(&self, slot: Slot) -> Option<Value> {
        self.device_default(slot)
            .or_else(|| self.keyboard_default(slot))
            .or_else(|| slot.place().map(|p| p.default.clone()))
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
        match (slot.source, crate::mouse::aside(setting.id)) {
            (Some(source), _) => format!("{name}{}", source.aside().get(self.lang)),
            // "Défilement naturel, pavé tactile".
            (None, Some(device)) => format!("{name}{}", device.get(self.lang)),
            (None, None) => name.to_owned(),
        }
    }

    /// `value` written at `slot`, at once.
    fn change(&mut self, slot: Slot, value: Value) -> Task<Message> {
        self.change_saying(slot, value, None)
    }

    /// The same, its toast saying `say` instead of the setting and its
    /// value, when given.
    fn change_saying(&mut self, slot: Slot, value: Value, say: Option<String>) -> Task<Message> {
        if !self.writable() {
            return Task::none();
        }
        let understood = matches!(self.reading(slot), Reading::Value(_));
        if understood && self.value(slot) == Some(value.clone()) {
            return Task::none();
        }
        let batch = None;
        self.ask(slot, Raw::Set(value), Doing::New, None, batch, say);
        Task::none()
    }

    /// `value` written at `slot` as one of a batch taken back together,
    /// its toast saying `say` (empty: none).
    fn write_batch(&mut self, slot: Slot, value: Value, batch: u64, say: Option<String>) {
        if self.writable() {
            self.ask(slot, Raw::Set(value), Doing::New, None, Some(batch), say);
        }
    }

    /// `slot`'s line taken out of the file: its default applies, and
    /// follows a changed default later ("Rétablir", 6.8).
    fn remove(&mut self, slot: Slot, batch: Option<u64>) {
        if self.writable() {
            self.ask(slot, Raw::Remove, Doing::New, None, batch, None);
        }
    }

    fn ask_write(&mut self, slot: Slot, raw: Raw, doing: Doing, change: Option<Change>) {
        let batch = change.as_ref().and_then(Change::batch);
        self.ask(slot, raw, doing, change, batch, None);
    }

    fn ask(
        &mut self,
        slot: Slot,
        raw: Raw,
        doing: Doing,
        change: Option<Change>,
        batch: Option<u64>,
        say: Option<String>,
    ) {
        self.serial += 1;
        let id = self.serial;
        self.pending.insert(
            id,
            Pending {
                write: Write::Slot(slot, raw.clone()),
                doing,
                change,
                batch,
                say,
            },
        );
        if !self.worker.ask(Ask::Write { slot, raw, id }) {
            self.pending.remove(&id);
        }
    }

    /// Keys a page writes itself, together, with what the toasts say.
    fn ask_keys(
        &mut self,
        edits: Vec<KeyEdit>,
        said: KeysSaid,
        doing: Doing,
        change: Option<Change>,
    ) {
        if !self.writable() {
            return;
        }
        self.serial += 1;
        let id = self.serial;
        self.pending.insert(
            id,
            Pending {
                write: Write::Keys(edits.clone(), said),
                doing,
                change,
                batch: None,
                say: None,
            },
        );
        if !self.worker.ask(Ask::Keys { edits, id }) {
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
            Some(Undo::Bluetooth(_)) => {
                let Some(Undo::Bluetooth(choice)) = self.undo.pop() else {
                    return Task::none();
                };
                self.toast = None;
                return self.bluetooth_undo(choice, false);
            }
            Some(Undo::Time(_)) => {
                let Some(Undo::Time(choice)) = self.undo.pop() else {
                    return Task::none();
                };
                self.toast = None;
                return self.time_undo(choice, false);
            }
            Some(Undo::Account(_)) => {
                let Some(Undo::Account(choice)) = self.undo.pop() else {
                    return Task::none();
                };
                self.toast = None;
                return self.account_undo(choice, false);
            }
            Some(Undo::Network(_)) => {
                let Some(Undo::Network(choice)) = self.undo.pop() else {
                    return Task::none();
                };
                self.toast = None;
                return self.network_undo(choice, false);
            }
            Some(Undo::Power(_)) => {
                let Some(Undo::Power(choice)) = self.undo.pop() else {
                    return Task::none();
                };
                self.toast = None;
                return self.power_undo(choice, false);
            }
            Some(Undo::Agents(_)) => {
                let Some(Undo::Agents(choice)) = self.undo.pop() else {
                    return Task::none();
                };
                self.toast = None;
                return self.agents_undo(choice, false);
            }
            Some(Undo::Apps(_)) => {
                let Some(Undo::Apps(choice)) = self.undo.pop() else {
                    return Task::none();
                };
                self.toast = None;
                return self.apps_undo(choice, false);
            }
            Some(Undo::Picture { .. }) => {
                let Some(Undo::Picture { path, bytes }) = self.undo.pop() else {
                    return Task::none();
                };
                self.toast = None;
                self.wallpaper_put_back(path, bytes);
                return Task::none();
            }
            Some(Undo::File(_)) if self.writable() => match self.undo.pop() {
                Some(Undo::File(change)) => change,
                _ => return Task::none(),
            },
            _ => return Task::none(),
        };
        self.toast = None;
        // Its batch's others with it, the last first.
        let batch = change.batch();
        let mut changes = vec![change];
        while batch.is_some()
            && self
                .undo
                .last()
                .is_some_and(|c| matches!(c, Undo::File(c) if c.batch() == batch))
        {
            if let Some(Undo::File(change)) = self.undo.pop() {
                changes.push(change);
            }
        }
        for change in changes {
            // A desktop taken out comes back with its windows first (6.14).
            if let Some(task) = self.desktops_again(&change, true) {
                return task;
            }
            match &change {
                Change::Slot { slot, before, .. } => {
                    let (slot, before) = (*slot, before.clone());
                    self.ask_write(slot, before, Doing::Undo, Some(change));
                }
                Change::Keys { before, said, .. } => {
                    let (before, said) = (before.clone(), said.clone());
                    self.ask_keys(before, said, Doing::Undo, Some(change));
                }
            }
        }
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
            Some(Undo::Bluetooth(_)) => {
                let Some(Undo::Bluetooth(choice)) = self.redo.pop() else {
                    return Task::none();
                };
                return self.bluetooth_undo(choice, true);
            }
            Some(Undo::Time(_)) => {
                let Some(Undo::Time(choice)) = self.redo.pop() else {
                    return Task::none();
                };
                return self.time_undo(choice, true);
            }
            Some(Undo::Account(_)) => {
                let Some(Undo::Account(choice)) = self.redo.pop() else {
                    return Task::none();
                };
                return self.account_undo(choice, true);
            }
            Some(Undo::Network(_)) => {
                let Some(Undo::Network(choice)) = self.redo.pop() else {
                    return Task::none();
                };
                return self.network_undo(choice, true);
            }
            Some(Undo::Power(_)) => {
                let Some(Undo::Power(choice)) = self.redo.pop() else {
                    return Task::none();
                };
                return self.power_undo(choice, true);
            }
            Some(Undo::Agents(_)) => {
                let Some(Undo::Agents(choice)) = self.redo.pop() else {
                    return Task::none();
                };
                return self.agents_undo(choice, true);
            }
            Some(Undo::Apps(_)) => {
                let Some(Undo::Apps(choice)) = self.redo.pop() else {
                    return Task::none();
                };
                return self.apps_undo(choice, true);
            }
            Some(Undo::File(_)) if self.writable() => match self.redo.pop() {
                Some(Undo::File(change)) => change,
                _ => return Task::none(),
            },
            _ => return Task::none(),
        };
        let batch = change.batch();
        let mut changes = vec![change];
        while batch.is_some()
            && self
                .redo
                .last()
                .is_some_and(|c| matches!(c, Undo::File(c) if c.batch() == batch))
        {
            if let Some(Undo::File(change)) = self.redo.pop() {
                changes.push(change);
            }
        }
        for change in changes {
            if let Some(task) = self.desktops_again(&change, false) {
                return task;
            }
            match &change {
                Change::Slot { slot, after, .. } => {
                    let (slot, after) = (*slot, after.clone());
                    self.ask_write(slot, after, Doing::Redo, Some(change));
                }
                Change::Keys { after, said, .. } => {
                    let (after, said) = (after.clone(), said.clone());
                    self.ask_keys(after, said, Doing::Redo, Some(change));
                }
            }
        }
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
        self.bluetooth_close();
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
            self.stop_recording();
            self.keys.conflict = None;
            self.keys.adding = None;
            self.keys.sub = false;
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
            // A shortcut's row is on the shortcuts' sub-page.
            self.keys.sub = s.kind == Kind::Shortcut;
        } else if let Some(o) = crate::model::outside(setting) {
            self.go(Page::Section(o.section));
        }
        self.scroll_to = Some(setting);
        self.light = Some((setting, Instant::now()));
        Task::none()
    }

    /// Where things are now: a found row is scrolled to.
    ///
    /// Some of what the window draws is placed from these places (the bar's
    /// preview): until two collections in a row agree, the window is not
    /// laid out as they say, so they are collected again (a few times at
    /// most) and said only then, settled (#660).
    fn placed(&mut self, places: Places) -> Task<Message> {
        let settled = places == self.places;
        self.places = places;
        let again = if settled || self.unsettled >= UNSETTLED {
            self.unsettled = 0;
            self.log_places();
            Task::none()
        } else {
            self.unsettled += 1;
            self::places(self.wheels, self.shows_serial)
        };
        Task::batch([again, self.scroll_to_found()])
    }

    /// A found row scrolled to the page's middle, once placed.
    fn scroll_to_found(&mut self) -> Task<Message> {
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
            // From the Bluetooth popover's row of a device with a code.
            if let Some(address) = again.pair.clone() {
                self.bluetooth_pair_address(address);
            }
            // From the sound popover's link: the output in use lights.
            if section == SectionId::Sound {
                self.light = Some(("sound.output", Instant::now()));
            }
        }
        // From the actions palette's last row (#736): what the search
        // finds.
        let searched = if let Some(query) = again.search {
            self.focus = Focus::Search;
            self.search(query);
            operation::focus(SEARCH)
        } else {
            Task::none()
        };
        // From the Wi-Fi popover's "dans les Réglages": that network's
        // sheet (6.6).
        let join = if let Some(name) = again.join {
            self.network.joining = Some(name);
            self.network_try_join()
        } else {
            Task::none()
        };
        let join = Task::batch([join, searched]);
        let (Some(id), Some(token)) = (self.window, again.token) else {
            return join;
        };
        let raise = window::run(id, move |window| {
            if let Some(surface) = activate::Surface::of(window) {
                surface.activate(token);
            }
        })
        .map(|()| Message::Nothing);
        Task::batch([raise, join])
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
        if section == SectionId::Bluetooth {
            return self
                .bluetooth_stops()
                .into_iter()
                .map(Control::Bluetooth)
                .collect();
        }
        if section == SectionId::DateTime {
            return self.time_stops().into_iter().map(Control::Time).collect();
        }
        if section == SectionId::Account {
            return self
                .account_stops()
                .into_iter()
                .map(Control::Account)
                .collect();
        }
        if section == SectionId::Apps {
            return self.apps_controls();
        }
        if section == SectionId::Keyboard {
            if self.keys.sub {
                return Vec::new();
            }
            return settings_of(section)
                .iter()
                .filter(|s| !keyboard_page::own_kind(&s.kind))
                .map(|s| Control::Setting(Slot::of(s, self.source)))
                .collect();
        }
        if section == SectionId::Screens {
            // The night light's rows after the page's own (#436).
            let night = crate::screens::NIGHT_ROWS
                .iter()
                .filter_map(|id| crate::model::setting(id))
                .map(|s| Control::Setting(Slot::of(s, self.source)));
            return self
                .screens_controls()
                .into_iter()
                .map(Control::Screens)
                .chain(night)
                .collect();
        }
        if section == SectionId::Wallpaper {
            return self.wallpaper_controls();
        }
        if section == SectionId::Bar {
            return self.bar_controls();
        }
        if section == SectionId::Desktops {
            return self.desktops_controls();
        }
        if section == SectionId::Power {
            return self.power_controls();
        }
        if section == SectionId::Agents {
            return self.agents_controls();
        }
        if section == SectionId::Captures {
            return self.captures_controls();
        }
        if section == SectionId::Network {
            return self
                .network_controls()
                .into_iter()
                .map(Control::Network)
                .collect();
        }
        if section == SectionId::Mouse {
            let mut out: Vec<Control> = self
                .mouse_visible()
                .into_iter()
                // A note is not a control (6.13).
                .filter(|s| !matches!(s.kind, Kind::Note(_)))
                .map(|s| Control::Setting(Slot::of(s, self.source)))
                .collect();
            out.extend([Control::Try(TryPart::List), Control::Try(TryPart::Target)]);
            return out;
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

    /// How much larger the window is drawn: `[appearance] text-scale`.
    pub fn text_scale(&self) -> f32 {
        self.text_scale
    }

    fn choice_index(&self, slot: Slot) -> Option<usize> {
        let setting = slot.setting()?;
        let Kind::Menu(choices) = &setting.kind else {
            return None;
        };
        let value = self.value(slot)?;
        choices.iter().position(|c| c.value == value)
    }

    /// Whether `Ctrl+F` goes to the search now: not while a shortcut is
    /// recorded, nor while the screens' "Garder ?" or menu take every key
    /// (as `Message::Key` and [`App::key`] have it).
    fn ctrl_f_searches(&self) -> bool {
        self.keys.recording.is_none() && !self.screens_take_keys()
    }

    fn key(
        &mut self,
        key: &Key,
        modifiers: Modifiers,
        text: Option<&str>,
        captured: bool,
    ) -> Task<Message> {
        let ctrl = modifiers.control();
        // The screens' "Garder ?" (anywhere), the page's menu.
        if let Some(task) = self.screens_key_first(key) {
            return task;
        }
        // The Bluetooth page's sheets, rename field and menu (6.7).
        if let Some(task) = self.bluetooth_key_first(key) {
            return task;
        }
        match key.as_ref() {
            _ if keys_in::is_ctrl_f(key, modifiers) => {
                self.focus = Focus::Search;
                self.menu = None;
                // The field already has the keyboard, its text selected:
                // the window's keys gave it as the key came, before the
                // keys after it (`keys_in`). An operation now would apply
                // after them: a selection would take them (#637, #669).
                return Task::none();
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
                // The network's sheet, a password typed (6.6).
                if let Some(task) = self.network_escape() {
                    return task;
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
            // The network's sheet keeps the keyboard (6.6).
            Key::Named(Named::Tab) if self.network.sheet.is_some() => {
                return self.network_sheet_tab(modifiers.shift());
            }
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
        // Into the network's sheet or a password typed anew, before their
        // field holds the keyboard (6.6).
        if let Some(text) = text
            && !captured
            && !ctrl
            && !modifiers.alt()
            && !text.is_empty()
            && text.chars().all(|c| !c.is_control())
            && let Some(task) = self.network_typed(text)
        {
            return task;
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
            // Into the layouts' popover's search while it is open (6.8).
            if let Some(query) = &self.keys.adding {
                let query = format!("{query}{text}");
                return Task::batch([
                    self.keys_update(keyboard_page::Msg::AddQuery(query)),
                    self.keys_update(keyboard_page::Msg::FocusAdd),
                ]);
            }
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
        let Some(control) = self.controls().get(i).cloned() else {
            return Task::none();
        };
        let named = match key.as_ref() {
            Key::Named(named) => named,
            _ => return Task::none(),
        };
        match control {
            Control::Wallpaper(stop) => self.wallpaper_key(&stop, named),
            Control::Bar(stop) => self.bar_key(&stop, named),
            Control::Desktops(stop) => self.desktops_key(&stop, named),
            Control::Captures(stop) => self.captures_key(&stop, named),
            Control::Sound(stop) => self.sound_key(stop, named, modifiers),
            Control::Bluetooth(stop) => self.bluetooth_key(stop, named, modifiers),
            Control::Time(stop) => self.time_key(stop, named),
            Control::Account(stop) => self.account_key(stop, named),
            Control::Network(stop) => self.network_key(stop, named),
            Control::Power(stop) => self.power_key(stop, named),
            Control::Agents(stop) => self.agents_key(stop, named),
            Control::Apps(stop) => self.apps_key(stop, named, modifiers),
            Control::Screens(control) => self.screens_control_key(&control, named, modifiers.alt()),
            Control::Segmented => {
                match named {
                    Named::ArrowLeft => self.source = Source::Battery,
                    Named::ArrowRight => self.source = Source::Ac,
                    _ => {}
                }
                Task::none()
            }
            Control::Try(TryPart::List) => {
                let by = match named {
                    Named::ArrowDown => 30.0,
                    Named::ArrowUp => -30.0,
                    _ => return Task::none(),
                };
                operation::scroll_by(
                    mouse::TRY_LIST,
                    widget::scrollable::AbsoluteOffset { x: 0.0, y: by },
                )
            }
            Control::Try(TryPart::Target) => match named {
                Named::Space | Named::Enter => {
                    self.update(Message::Mouse(mouse::Msg::Press(mouse::Press::Left)))
                }
                _ => Task::none(),
            },
            Control::Setting(slot) => {
                let Some(setting) = slot.setting() else {
                    return Task::none();
                };
                if self.inert(setting) {
                    return Task::none();
                }
                // 6.4's: Alt+↑ / Alt+↓ on a movable group's switch.
                if modifiers.alt()
                    && let Some(task) = self.bar_alt_move(slot, named)
                {
                    return task;
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
                    // 6.2's: ← and → move and choose.
                    (Kind::Segmented(choices), Named::ArrowLeft | Named::ArrowRight) => {
                        let values: Vec<Value> = choices.iter().map(|c| c.value.clone()).collect();
                        self.step_choice(slot, &values, named == Named::ArrowRight)
                    }
                    // 6.3's plain colour: six swatches, a radio group.
                    (Kind::Menu(choices), Named::ArrowLeft | Named::ArrowRight)
                        if setting.id == crate::wallpaper::COLOR =>
                    {
                        let values: Vec<Value> = choices.iter().map(|c| c.value.clone()).collect();
                        self.step_choice(slot, &values, named == Named::ArrowRight)
                    }
                    (Kind::Swatches, Named::ArrowLeft | Named::ArrowRight) => {
                        let values: Vec<Value> = self
                            .accents_shown()
                            .iter()
                            .map(|a| Value::Word(a.word()))
                            .collect();
                        self.step_choice(slot, &values, named == Named::ArrowRight)
                    }
                    (Kind::Link(to), Named::Space | Named::Enter) => {
                        self.update(Message::Section(*to))
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
                            Named::Home => *min,
                            Named::End => *max,
                            _ => return Task::none(),
                        };
                        self.slide(slot, to.clamp(*min, *max))
                    }
                    // A speed: a tenth, a half, the ends (6.12).
                    (Kind::Speed { min, max }, _) => {
                        let now = match self.value(slot) {
                            Some(Value::Int(n)) => n,
                            _ => 0,
                        };
                        let to = match named {
                            Named::ArrowLeft => now - 1,
                            Named::ArrowRight => now + 1,
                            Named::PageDown => now - 5,
                            Named::PageUp => now + 5,
                            Named::Home => *min,
                            Named::End => *max,
                            _ => return Task::none(),
                        };
                        self.slide(slot, to.clamp(*min, *max))
                    }
                    _ => Task::none(),
                }
            }
        }
    }

    /// The choice before or after the current one among `values`, chosen.
    fn step_choice(&mut self, slot: Slot, values: &[Value], next: bool) -> Task<Message> {
        let at = self
            .value(slot)
            .and_then(|v| values.iter().position(|c| *c == v))
            .unwrap_or(0);
        let to = if next {
            (at + 1).min(values.len().saturating_sub(1))
        } else {
            at.saturating_sub(1)
        };
        match values.get(to) {
            Some(value) if to != at => self.update(Message::Choose(slot, value.clone())),
            _ => Task::none(),
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

    /// Whether `setting` is held on by another: reduced transparency
    /// while high contrast is on (#423). Its switch shows on and takes no
    /// click; the file keeps its own value.
    pub(super) fn implied(&self, setting: &Setting) -> bool {
        setting.id == crate::accessibility::REDUCE_TRANSPARENCY
            && crate::model::setting(crate::accessibility::HIGH_CONTRAST).is_some_and(|contrast| {
                self.value(Slot::of(contrast, self.source)) == Some(Value::Bool(true))
            })
    }

    /// Whether `setting`'s control does not apply now: the file does not
    /// read, the switch it needs is off, or another holds it on.
    fn inert(&self, setting: &Setting) -> bool {
        if !self.writable()
            || self.implied(setting)
            || self.unavailable(setting).is_some()
            || self.desktops_inert(setting)
            || self.power_inert(setting)
            || self.agents_inert(setting)
        {
            return true;
        }
        setting.needs.is_some_and(|needs| {
            crate::model::setting(needs).is_some_and(|switch| {
                self.value(Slot::of(switch, self.source)) == Some(Value::Bool(false))
            })
        })
    }

    /// Why a row does not apply now: no device plugged takes it (6.12), or
    /// another setting takes its key (6.8).
    pub(super) fn unavailable(&self, setting: &Setting) -> Option<&'static str> {
        self.not_here(setting)
            .or_else(|| self.keyboard_not_here(setting))
    }

    /// The row lit, and how much (0 to 1), at the frame's time.
    fn lit(&self, setting: &str) -> f32 {
        let Some((id, since)) = self.light else {
            return 0.0;
        };
        if Self::bar_lit_alias(id) != setting {
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
    /// Each line has its `serial`, which the places collected after it
    /// carry (`shown`): a test reads where to click from places at least as
    /// recent as what it saw (#660).
    fn log(&mut self) {
        let mut shown = view::shown(self);
        let text = shown.to_string();
        if text != self.said {
            self.shows_serial += 1;
            if let Some(fields) = shown.as_object_mut() {
                fields.insert("serial".to_owned(), self.shows_serial.into());
            }
            tracing::info!(target: "hocus_settings::shows", "shows {shown}");
            self.said = text;
        }
    }

    /// Each id's place, `wheels`: how many wheels the window had taken when
    /// they were read, so that a test knows the page has taken its wheel,
    /// whether it moved or not, and `shown`: the serial of the "shows" line
    /// they follow (#660).
    fn log_places(&mut self) {
        let mut at: serde_json::Map<String, serde_json::Value> = self
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
        at.insert("wheels".to_owned(), self.places.wheels.into());
        at.insert("shown".to_owned(), self.places.shown.into());
        let text = serde_json::Value::Object(at).to_string();
        if text != self.said_places {
            tracing::debug!(target: "hocus_settings::places", "places {text}");
            self.said_places = text;
        }
    }
}

/// A control of the page that takes the keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Control {
    Segmented,
    Setting(Slot),
    Sound(sound::Stop),
    /// The Écrans page's own.
    Screens(screens::Control),
    /// The mice and touchpads page's test area.
    Try(TryPart),
    /// The wallpaper's page's own (6.3).
    Wallpaper(wallpaper_page::Stop),
    /// The bar's page's own (6.4).
    Bar(bar_page::Stop),
    /// The Bluetooth page's own (6.7).
    Bluetooth(bluetooth::Stop),
    /// The Date et heure page's own (6.11).
    Time(time::Stop),
    /// The Compte page's own (6.11).
    Account(account::Stop),
    /// The desktops and windows' page's own (6.14).
    Desktops(desktops_page::Stop),
    /// The screenshots and clipboard's page's own (6.17).
    Captures(captures_page::Stop),
    /// The network's page's own (6.6).
    Network(network::Stop),
    /// The Énergie page's own (6.10).
    Power(power_page::Stop),
    /// The Agents page's own (6.16).
    Agents(agents_page::Stop),
    /// The Applications page's own (6.15).
    Apps(applications::Stop),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TryPart {
    List,
    Target,
}

/// The places of what the window draws, after it is laid out, with the
/// wheels it had taken then and the serial of the last "shows" line.
/// `folder` opened in the user's file manager (`xdg-open`), made first
/// when `make`: on a thread of its own, which the window never waits for.
fn open_folder(folder: PathBuf, make: bool) {
    let started = std::thread::Builder::new()
        .name("settings: folder".to_owned())
        .spawn(move || {
            if make {
                let _ = std::fs::create_dir_all(&folder);
            }
            let opened = std::process::Command::new("xdg-open")
                .arg(&folder)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            if let Err(err) = opened {
                tracing::warn!(%err, "xdg-open does not run");
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "no thread to open the folder from");
    }
}

fn places(wheels: u64, shown: u64) -> Task<Message> {
    iced::advanced::widget::operate(places::Collect::after(wheels, shown)).map(Message::Placed)
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
