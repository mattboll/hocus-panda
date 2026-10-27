//! The widget column (decision 0007): on the right of the primary screen,
//! from top to bottom the status row, the clock, the next meeting, the
//! active window with its controls and the line of this screen's desktops,
//! and the account. The desktops line and the account open popovers to the
//! column's left.
//!
//! The compositor gives the column a [`ColumnState`] when something it shows
//! changes, routes the pointer to it when over it, and does the
//! [`Action`]s that come back.

use std::{sync::Arc, time::Duration};

use hocus_ui::{
    AccessNode, Align, Button, ButtonKind, Canvas, Color, Direction, Draw, Event, Icon, Input,
    Insets, Justify, Kind, Length, Look, NodeId, Paint, PixelRect, Point, Rect, Role, Size, Style,
    Surface, TextStyle, Theme, Tree,
};

use crate::{i18n::Lang, icons, phone};

/// The column's width, from decision 0007.
pub const WIDTH: f32 = 320.0;
/// Between the column and the screen's edges.
pub const MARGIN: f32 = 16.0;
/// Above and below the card's content.
const CARD_PADDING: f32 = 16.0;
/// Between a popover and the column.
const POPOVER_GAP: f32 = 12.0;
/// Room around a popover's card for its shadow.
const SHADOW_ROOM: f32 = 24.0;
const ICON_STROKE: f32 = 2.1;
/// Notifications shown at most.
const NOTIFICATIONS: usize = 3;

/// The time on the wall, as the user reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WallTime {
    pub hour: u8,
    pub minute: u8,
    /// 0 for Monday.
    pub weekday: u8,
    pub day: u8,
    /// From 1.
    pub month: u8,
    /// 24 or 12 hours.
    pub format: hocus_core::config::ClockFormat,
}

impl WallTime {
    /// The time of day as the clock shows it.
    pub fn text(&self) -> String {
        self.format.time(self.hour, self.minute)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveWindow {
    pub title: String,
    pub maximized: bool,
}

/// One screen's desktops.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScreenDesktops {
    pub output: String,
    pub count: usize,
    /// From 1.
    pub shown: usize,
    /// The shown desktop's name, empty when it has none but its number.
    pub name: String,
    /// Every desktop's name, in order, empty for one that has none (#192).
    pub names: Vec<String>,
    /// What goes on on each desktop, in order (#192); none on the one
    /// shown.
    pub badges: Vec<crate::activity::Badge>,
}

impl ScreenDesktops {
    /// Desktop `i`'s (from 0) name, or its number.
    pub fn label(&self, lang: Lang, i: usize) -> String {
        lang.desktop_label(self.names.get(i).map_or("", String::as_str), i + 1)
    }

    /// The badges of its desktops, said: "Website: an agent waits for an
    /// answer", in order; empty when nothing goes on.
    pub fn activity(&self, lang: Lang) -> Vec<String> {
        self.badges
            .iter()
            .enumerate()
            .filter_map(|(i, b)| lang.desktop_badge(&self.label(lang, i), *b))
            .collect()
    }
}

/// What the column shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ColumnState {
    pub time: WallTime,
    pub active: Option<ActiveWindow>,
    /// Every screen, the column's first.
    pub screens: Vec<ScreenDesktops>,
    /// The keyboard layouts kept, in order (shared: the state is made
    /// after every turn of the loop); the one in use.
    pub layouts: Arc<[KeyboardLayout]>,
    pub layout: usize,
    /// The keys that switch to the next layout, when some do.
    pub switch_layout_keys: Option<Arc<str>>,
    pub user: String,
    /// The account menu offers hibernating only where it works.
    pub can_hibernate: bool,
    /// The latest notifications, newest first: application and summary.
    pub notifications: Vec<(String, String)>,
    pub status: Status,
    /// The tray's applications, in the order they came.
    pub tray: Vec<TrayApp>,
    /// The screen is being shared.
    pub sharing: bool,
    /// To-dos not done, with their place in the notes, and how many.
    pub todos: Vec<(usize, String)>,
    /// System updates waiting, and whether they are being installed.
    pub updates: Option<(usize, bool)>,
    /// The phone within reach (#85).
    pub phone: Option<PhoneLine>,
    /// The meeting going on or coming next (#75).
    pub meeting: Option<Meeting>,
    /// Calendars are set up: without a meeting, nothing else is planned.
    pub calendar_set: bool,
    pub agenda: Agenda,
    /// Coding agents (#41); none without an agent manager.
    pub agents: Option<crate::agents::AgentsView>,
    /// What MCP agents did (#176), once one connected; or that they are
    /// stopped.
    pub journal: Option<crate::journal::JournalView>,
}

/// A keyboard layout kept, as the column says it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyboardLayout {
    /// "French (AZERTY)".
    pub name: String,
    /// "FR", "BÉPO".
    pub short: String,
}

impl ColumnState {
    /// The layout in use.
    pub fn active_layout(&self) -> Option<&KeyboardLayout> {
        self.layouts.get(self.layout).or(self.layouts.first())
    }
}

/// The meeting card, as board Bureau draws it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Meeting {
    pub title: String,
    /// "In 12 min", "At 15:00", "In progress · 18 min".
    pub when: String,
    /// How close it is, in percent: the ring fills over the hour before.
    pub progress: u8,
    /// Join, when it has a call and starts soon; else Details.
    pub join: bool,
}

/// The phone's line: which, and its battery.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PhoneLine {
    /// Empty for a phone not known yet (only adb, nothing paired).
    pub name: String,
    /// Percent, when the phone shares it.
    pub battery: Option<u8>,
    pub charging: bool,
    /// Through KDE Connect: Ring, the clipboard, its files.
    pub connect: bool,
    /// Wireless debugging (#153), when on and adb is installed.
    pub adb: Option<AdbLine>,
}

/// What adb offers in the phone's popover.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AdbLine {
    /// The device adb reaches: its name, and whether over Wi-Fi.
    pub device: Option<(String, bool)>,
    /// scrcpy is installed: its screen can be mirrored.
    pub mirror: bool,
    /// A pairing going on: the popover shows its QR code.
    pub pairing: Option<Pairing>,
}

/// Pairing by a QR code, as the popover shows it.
#[derive(Clone, PartialEq, Eq)]
pub struct Pairing {
    /// The QR code's text. It holds the pairing password: it is drawn,
    /// never named to screen readers or logged.
    pub qr: String,
    pub stage: PairStage,
}

impl std::fmt::Debug for Pairing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pairing")
            .field("stage", &self.stage)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairStage {
    /// The code is shown; the phone has not scanned it.
    Waiting,
    /// Scanned: pairing, then joining it.
    Pairing,
    /// Why not, in the session's language.
    Failed(String),
}

/// The agenda's month and today's events.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Agenda {
    pub year: i16,
    /// The weekday of the month's first day, 0 for Monday.
    pub first_weekday: u8,
    pub days: u8,
    /// The ISO week number of each of the month's rows.
    pub weeks: Vec<u8>,
    /// The month's days with events.
    pub busy: Vec<u8>,
    /// Today's events: time and title.
    pub today: Vec<(String, String)>,
}

/// What the status row tells, once known.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Status {
    /// 0 saver, 1 balanced, 2 performance.
    pub profile: Option<usize>,
    /// On, and the network joined.
    pub network: Option<(bool, Option<String>)>,
    /// Volume, and muted.
    pub sound: Option<(u8, bool)>,
    /// Percent.
    pub battery: Option<u8>,
}

/// What the user asked for in the column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Minimize,
    ToggleMaximize,
    Close,
    /// Uses the kept layout of this index.
    SetLayout(usize),
    /// Shows desktop `desktop` (from 1) of `output`.
    ShowDesktop {
        output: String,
        desktop: usize,
    },
    Lock,
    Settings,
    Suspend,
    Hibernate,
    LogOut,
    Restart,
    ShutDown,
    /// The command field was clicked: the launcher opens.
    OpenLauncher,
    /// Every screen capture ends.
    StopSharing,
    /// The to-do at this place in the notes is ticked.
    TickTodo(usize),
    /// The next meeting's call.
    JoinMeeting,
    InstallUpdates,
    /// The phone rings, to be found.
    RingPhone,
    /// What was copied last goes to the phone: text to its clipboard,
    /// files as files.
    SendClipboardToPhone,
    /// The phone's files open.
    BrowsePhone,
    /// The phone's popover opened: what adb reaches is read again.
    PhoneOpened,
    /// Pairing a phone over Wi-Fi starts (or again): a QR code shows.
    PairPhone,
    CancelPairing,
    /// The device adb reaches shows its screen in a window (scrcpy).
    MirrorPhone,
    /// A tray application clicked.
    TrayActivate(String),
    /// A tray application's menu asked for.
    TrayMenu(String),
    /// An entry of its menu chosen.
    TrayChosen {
        key: String,
        id: i32,
    },
    /// Something asked of a coding agent.
    Agent {
        key: String,
        ask: crate::agents::AgentAsk,
    },
    /// Every MCP agent stopped at once (#176).
    StopAgents,
    /// Allowed again.
    AllowAgents,
    /// The user takes over from an MCP agent, by name (#43): its window
    /// gets their keyboard, and its actions are refused.
    TakeOver(String),
    /// And hands back.
    HandBack(String),
}

/// An application in the tray.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayApp {
    pub key: String,
    pub title: String,
    pub attention: bool,
    /// Width, height, premultiplied RGBA.
    pub icon: Option<(u16, u16, Arc<[u8]>)>,
}

/// An entry of a tray application's menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayMenuEntry {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub separator: bool,
    pub toggle: Option<bool>,
    pub children: Vec<TrayMenuEntry>,
}

/// The ring of the meeting card: how close the next meeting is.
struct Ring {
    progress: f32,
}

impl Draw for Ring {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, _now: Duration) {
        let center = Point::new(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        let r = rect.w / 2.0 - 3.0;
        let tau = std::f32::consts::TAU;
        canvas.arc(
            center,
            r,
            0.0,
            tau,
            5.0,
            theme.colors.border.over(theme.colors.surface),
        );
        if self.progress > 0.0 {
            canvas.arc(
                center,
                r,
                -tau / 4.0,
                tau * self.progress,
                5.0,
                theme.colors.accent,
            );
        }
    }

    fn size(&self) -> Size {
        Size::new(44.0, 44.0)
    }
}

/// A screen's desktops as dots, the shown one in the accent; one where
/// something goes on carries its badge (#192).
struct Dots {
    count: usize,
    shown: usize,
    badges: Vec<crate::activity::Badge>,
}

impl Draw for Dots {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, _now: Duration) {
        let size = 8.0;
        let y = rect.y + (rect.h - size) / 2.0;
        for i in 0..self.count {
            let x = rect.x + i as f32 * (size + 5.0);
            let dot = Rect::new(x, y, size, size);
            let badge = self.badges.get(i).copied().unwrap_or_default();
            if i + 1 != self.shown && badge.is_some() {
                badge.draw_dot(canvas, dot, size, theme);
                continue;
            }
            let color = if i + 1 == self.shown {
                theme.colors.accent
            } else {
                theme.colors.muted.with_alpha(0x66)
            };
            canvas.fill_rect(dot, size / 2.0, color);
        }
    }

    fn size(&self) -> Size {
        let n = self.count.max(1) as f32;
        Size::new(n * 8.0 + (n - 1.0) * 5.0, 16.0)
    }
}

fn stroked(path: &str, size: f32) -> Icon {
    // The icons are constants, tested to parse.
    Icon::stroked(path, size, ICON_STROKE).unwrap_or_else(|| unreachable!("icon {path}"))
}

/// The nodes the state changes.
#[derive(Debug, Clone, Copy)]
struct Nodes {
    layout: NodeId,
    clock: NodeId,
    date: NodeId,
    title: NodeId,
    minimize: NodeId,
    maximize: NodeId,
    close: NodeId,
    desktops: NodeId,
    desktops_text: NodeId,
    dots: NodeId,
    command: NodeId,
    history: NodeId,
    account: NodeId,
    initial: NodeId,
    user: NodeId,
    /// Power profile, network, sound, battery.
    status: [NodeId; 4],
    status_row: NodeId,
    /// The tray's ring and chip, when it has applications.
    tray: Option<(NodeId, NodeId)>,
    /// The sharing line's room, and its stop while the screen is shared.
    sharing: NodeId,
    stop_sharing: Option<NodeId>,
    /// The to-dos' room.
    tasks: NodeId,
    /// The updates' room, and its Install.
    updates: NodeId,
    install: Option<NodeId>,
    /// The phone's room, its line (which opens its popover) and Ring.
    phone: NodeId,
    phone_line: Option<NodeId>,
    phone_ring: Option<NodeId>,
    /// The clock and date, which open the agenda.
    time: NodeId,
    /// The meeting card, its ring, and its Join or Details.
    meeting: NodeId,
    meeting_button: Option<NodeId>,
    /// The coding agents' room.
    agents: NodeId,
    /// The card everything sits in: the account must stay inside it.
    card: NodeId,
    /// The notifications' caption and list, hidden when there is no room.
    notes: NodeId,
    /// The agents' journal's room, its stop button, and its entries (which
    /// open the popover).
    journal: NodeId,
    journal_button: Option<NodeId>,
    journal_entries: Option<NodeId>,
}

/// The agents' seats the journal shows (#43): their buttons and window
/// slots.
#[derive(Debug, Clone, Default)]
struct Seats {
    buttons: Vec<(NodeId, String, bool)>,
    slots: Vec<(String, NodeId)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PopoverKind {
    Desktops,
    Keyboard,
    Account,
    Tray,
    TrayMenu,
    Agenda,
    Phone,
    Journal,
}

/// A popover: its own tree and pixels, placed by the column.
struct Popover {
    kind: PopoverKind,
    tree: Tree,
    surface: Surface,
    /// Its corner, relative to the column's.
    at: Point,
    targets: Vec<(NodeId, Action)>,
}

impl std::fmt::Debug for Popover {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Popover")
            .field("kind", &self.kind)
            .field("at", &self.at)
            .finish_non_exhaustive()
    }
}

/// Pixels to show, and where, relative to the column's corner.
#[derive(Debug)]
pub struct View<'a> {
    pub pixels: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub at: Point,
}

/// What changed in a frame, in each surface's physical pixels.
#[derive(Debug, Default)]
pub struct Damage {
    pub column: Vec<PixelRect>,
    pub popover: Vec<PixelRect>,
    /// A popover opened or closed: its old place needs drawing.
    pub popover_changed: bool,
}

#[derive(Debug)]
pub struct Column {
    lang: Lang,
    tree: Tree,
    surface: Surface,
    nodes: Nodes,
    state: ColumnState,
    popover: Option<Popover>,
    popover_changed: bool,
    /// The popover a press outside closed: its own button, released, must
    /// not open it again.
    closed_by_press: Option<PopoverKind>,
    /// The menu last received, and whose.
    tray_menu: Option<(String, Vec<TrayMenuEntry>)>,
    /// The to-dos shown, and their places in the notes.
    todo_rows: Vec<(NodeId, usize)>,
    /// How many notifications and journal entries are shown: fewer when
    /// the column has no room for all.
    fit: Fit,
    /// The agents' rows and buttons.
    agent_targets: Vec<(NodeId, Action)>,
    /// The MCP agents' seats in the journal (#43).
    seats: Seats,
    now: Duration,
}

impl Column {
    /// A column `height` logical pixels tall, drawn at `scale`.
    pub fn new(theme: Theme, lang: Lang, height: f32, scale: f32) -> Self {
        let size = Size::new(WIDTH, height);
        let mut tree = Tree::new(theme.clone(), size, scale);
        let nodes = build(&mut tree, &theme, lang);
        let mut column = Self {
            lang,
            tree,
            surface: Surface::new(size, scale),
            nodes,
            state: ColumnState::default(),
            popover: None,
            popover_changed: false,
            closed_by_press: None,
            tray_menu: None,
            todo_rows: Vec::new(),
            agent_targets: Vec::new(),
            seats: Seats::default(),
            fit: Fit::MOST,
            now: Duration::ZERO,
        };
        column.apply(&ColumnState::default(), true);
        column
    }

    /// Where each MCP agent's window shows in the journal, live, from the
    /// column's corner: the compositor draws it there.
    pub fn seat_slots(&self) -> Vec<(String, hocus_ui::Rect)> {
        self.seats
            .slots
            .iter()
            .filter_map(|(agent, id)| Some((agent.clone(), self.tree.rect(*id)?)))
            .filter(|(_, r)| r.h > 1.0)
            .collect()
    }

    /// What the journal's section filled in.
    fn take_journal(&mut self, filled: crate::journal::Filled) {
        self.nodes.journal_button = filled.button;
        self.nodes.journal_entries = filled.entries;
        self.seats = Seats {
            buttons: filled.seats,
            slots: filled.slots,
        };
    }

    pub fn size(&self) -> Size {
        self.tree.size()
    }

    /// Speaks `lang` from now on: built again, showing what it showed.
    pub fn set_lang(&mut self, lang: Lang) {
        if lang == self.lang {
            return;
        }
        let state = self.state.clone();
        let size = self.tree.size();
        *self = Self::new(self.tree.theme().clone(), lang, size.h, self.tree.scale());
        self.apply(&state, true);
    }

    pub fn resize(&mut self, height: f32, scale: f32) {
        let size = Size::new(WIDTH, height);
        if size != self.tree.size() || scale != self.tree.scale() {
            self.tree.resize(size, scale);
            self.surface = Surface::new(size, scale);
            self.close_popover();
            self.fit_room();
        }
    }

    /// Shows as many notifications, journal entries and agents as there is
    /// room for, the account inside the card: the notifications go first,
    /// down to one, then the journal's entries, down to one, then the
    /// other desktops' agents' lines, then the meeting card and the
    /// waiting agents' rows are said in short, then the notifications go
    /// altogether, then the agents shown in full, down to one; what that
    /// gave back goes to the notifications again.
    fn fit_room(&mut self) {
        let mut want = Fit::MOST;
        loop {
            self.fit_to(want);
            self.tree.layout();
            if self.account_inside() {
                break;
            }
            match want.less(&self.state) {
                Some(less) => want = less,
                None => return,
            }
        }
        // What an agent's row gave may leave room for notifications again.
        let most = NOTIFICATIONS.min(self.state.notifications.len().max(1));
        while want.notifications < most {
            let more = Fit {
                notifications: want.notifications + 1,
                ..want
            };
            self.fit_to(more);
            self.tree.layout();
            if !self.account_inside() {
                self.fit_to(want);
                self.tree.layout();
                return;
            }
            want = more;
        }
    }

    /// Builds the notifications and the journal again for `want`, if it
    /// differs from what they show.
    fn fit_to(&mut self, want: Fit) {
        let lang = self.lang;
        let theme = self.tree.theme().clone();
        let n = self.nodes;
        if want.notifications != self.fit.notifications {
            fill_history(
                &mut self.tree,
                (n.notes, n.history),
                &theme,
                lang,
                &self.state.notifications,
                want.notifications,
            );
        }
        if want.journal != self.fit.journal
            && let Some(view) = &self.state.journal
        {
            for child in self.tree.children(n.journal) {
                self.tree.remove(child);
            }
            let filled =
                crate::journal::fill(&mut self.tree, n.journal, &theme, lang, view, want.journal);
            self.take_journal(filled);
        }
        if want.compact_meeting != self.fit.compact_meeting {
            self.nodes.meeting_button = fill_meeting(
                &mut self.tree,
                n.meeting,
                &theme,
                lang,
                self.state.meeting.as_ref(),
                self.state.calendar_set,
                want.compact_meeting,
            );
        }
        if want.agents != self.fit.agents
            && let Some(view) = &self.state.agents
        {
            for child in self.tree.children(n.agents) {
                self.tree.remove(child);
            }
            self.agent_targets =
                crate::agents::fill(&mut self.tree, n.agents, &theme, lang, view, want.agents);
        }
        self.fit = want;
    }

    /// Whether the account lies within the card's padding.
    fn account_inside(&self) -> bool {
        match (
            self.tree.rect(self.nodes.account),
            self.tree.rect(self.nodes.card),
        ) {
            (Some(account), Some(card)) => account.bottom() <= card.bottom() - CARD_PADDING + 0.5,
            _ => true,
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.tree.set_theme(theme.clone());
        if let Some(p) = &mut self.popover {
            p.tree.set_theme(theme);
        }
    }

    /// Shows `state`; only what differs is drawn again.
    pub fn set_state(&mut self, state: &ColumnState) {
        if *state != self.state {
            self.apply(state, false);
        }
    }

    fn apply(&mut self, state: &ColumnState, all: bool) {
        let lang = self.lang;
        let old = std::mem::replace(&mut self.state, state.clone());
        // The tray's chip comes and goes with its applications: the row is
        // built again, and what it shows told again.
        let rebuilt = !all && old.tray.is_empty() != state.tray.is_empty();
        if rebuilt {
            let theme = self.tree.theme().clone();
            let (tray, layout, status) = fill_status(
                &mut self.tree,
                self.nodes.status_row,
                &theme,
                lang,
                !state.tray.is_empty(),
            );
            self.nodes.tray = tray;
            self.nodes.layout = layout;
            self.nodes.status = status;
        }
        let all_status = all || rebuilt;
        let n = self.nodes;
        let fit = self.fit;
        let t = &mut self.tree;
        if let Some((ring, chip)) = n.tray
            && (all_status || old.tray != state.tray)
        {
            let count = state.tray.len();
            let waiting = state.tray.iter().filter(|a| a.attention).count();
            t.set_text(chip, &count.to_string());
            t.set_name(chip, &lang.tray_count(count, waiting));
            let theme = t.theme().clone();
            t.set_look(ring, tray_ring(&theme, waiting > 0));
        }
        if all_status || old.status != state.status {
            let s = &state.status;
            let [profile, network, sound, battery] = n.status;
            let profile_name = s
                .profile
                .map_or_else(|| lang.power_profile().to_owned(), |p| lang.profile_name(p));
            t.set_name(profile, &profile_name);
            let (network_name, on) = match &s.network {
                Some((wifi, name)) => (lang.network_name(*wifi, name.as_deref()), *wifi),
                None => (lang.network().to_owned(), true),
            };
            t.set_name(network, &network_name);
            t.set_icon_paint(network, if on { Paint::Text } else { Paint::Muted });
            let (sound_name, audible) = match s.sound {
                Some((volume, muted)) => (lang.sound_level(volume, muted), !muted),
                None => (lang.sound().to_owned(), true),
            };
            t.set_name(sound, &sound_name);
            t.set_icon_paint(sound, if audible { Paint::Text } else { Paint::Muted });
            let battery_name = s
                .battery
                .map_or_else(|| lang.battery().to_owned(), |p| lang.battery_level(p));
            t.set_name(battery, &battery_name);
            // Low: the waiting colour.
            let low = s.battery.is_some_and(|p| p <= 15);
            t.set_icon_paint(battery, if low { Paint::Waiting } else { Paint::Text });
        }
        if all || old.sharing != state.sharing {
            t.set_style(n.sharing, sharing_room(state.sharing));
            for child in t.children(n.sharing) {
                t.remove(child);
            }
            let theme = t.theme().clone();
            self.nodes.stop_sharing = state
                .sharing
                .then(|| fill_sharing(t, n.sharing, &theme, lang));
        }
        if all || old.todos != state.todos {
            t.set_style(n.tasks, tasks_room(!state.todos.is_empty()));
            for child in t.children(n.tasks) {
                t.remove(child);
            }
            self.todo_rows.clear();
            if !state.todos.is_empty() {
                let theme = t.theme().clone();
                self.todo_rows = fill_tasks(t, n.tasks, &theme, lang, &state.todos);
            }
        }
        if all || old.agents != state.agents {
            let shown = state.agents.as_ref();
            t.set_style(n.agents, crate::agents::room(shown.is_some()));
            for child in t.children(n.agents) {
                t.remove(child);
            }
            let theme = t.theme().clone();
            self.agent_targets = shown
                .map(|view| crate::agents::fill(t, n.agents, &theme, lang, view, fit.agents))
                .unwrap_or_default();
        }
        if all || old.journal != state.journal {
            let shown = state.journal.as_ref();
            t.set_style(n.journal, crate::journal::room(shown.is_some()));
            for child in t.children(n.journal) {
                t.remove(child);
            }
            let theme = t.theme().clone();
            let filled = shown
                .map(|view| crate::journal::fill(t, n.journal, &theme, lang, view, fit.journal))
                .unwrap_or_default();
            self.nodes.journal_button = filled.button;
            self.nodes.journal_entries = filled.entries;
            self.seats = Seats {
                buttons: filled.seats,
                slots: filled.slots,
            };
        }
        if all || old.updates != state.updates {
            t.set_style(n.updates, tasks_room(state.updates.is_some()));
            for child in t.children(n.updates) {
                t.remove(child);
            }
            let theme = t.theme().clone();
            self.nodes.install = state.updates.and_then(|(count, installing)| {
                fill_updates(t, n.updates, &theme, lang, count, installing)
            });
        }
        if all || old.phone != state.phone {
            t.set_style(n.phone, tasks_room(state.phone.is_some()));
            for child in t.children(n.phone) {
                t.remove(child);
            }
            let theme = t.theme().clone();
            (self.nodes.phone_line, self.nodes.phone_ring) = match &state.phone {
                Some(p) => {
                    let (line, ring) = fill_phone(t, n.phone, &theme, lang, p);
                    (Some(line), ring)
                }
                None => (None, None),
            };
        }
        if all || old.meeting != state.meeting || old.calendar_set != state.calendar_set {
            let theme = t.theme().clone();
            self.nodes.meeting_button = fill_meeting(
                t,
                n.meeting,
                &theme,
                lang,
                state.meeting.as_ref(),
                state.calendar_set,
                fit.compact_meeting,
            );
        }
        if all || old.time != state.time {
            t.set_text(n.clock, &state.time.text());
            let w = state.time;
            t.set_text(n.date, &lang.date(w.weekday, w.day.max(1), w.month.max(1)));
        }
        if all || old.active != state.active {
            let theme = t.theme().clone();
            match &state.active {
                Some(active) => {
                    t.set_text(n.title, &active.title);
                    t.set_text_style(n.title, theme.text.title.with_weight(600));
                    let (icon, name) = if active.maximized {
                        (icons::RESTORE, lang.restore())
                    } else {
                        (icons::MAXIMIZE, lang.maximize())
                    };
                    t.set_icon(n.maximize, stroked(icon, 16.0));
                    t.set_name(n.maximize, name);
                }
                None => {
                    t.set_text(n.title, lang.no_window());
                    t.set_text_style(n.title, theme.text.title.with_color(Paint::Muted));
                }
            }
            for b in [n.minimize, n.maximize, n.close] {
                t.set_enabled(b, state.active.is_some());
            }
        }
        if all || old.screens.first() != state.screens.first() {
            let (shown, count, name) = state
                .screens
                .first()
                .map_or((1, 1, ""), |s| (s.shown, s.count, s.name.as_str()));
            let line = if name.is_empty() {
                lang.desktop_of(shown, count)
            } else {
                lang.desktop_named(name, shown, count)
            };
            t.set_text(n.desktops_text, &line);
            // What goes on elsewhere, said after it.
            let activity = state
                .screens
                .first()
                .map(|s| s.activity(lang))
                .unwrap_or_default();
            let said = std::iter::once(line).chain(activity).collect::<Vec<_>>();
            t.set_name(n.desktops, &said.join(" · "));
            let badges = state
                .screens
                .first()
                .map(|s| s.badges.clone())
                .unwrap_or_default();
            t.set_draw(
                n.dots,
                Box::new(Dots {
                    count,
                    shown,
                    badges,
                }),
            );
        }
        if all_status || old.layouts != state.layouts || old.layout != state.layout {
            let active = state.active_layout().cloned().unwrap_or_default();
            t.set_text(n.layout, &active.short);
            t.set_name(n.layout, &lang.keyboard_layout(&active.name));
        }
        if all || old.notifications != state.notifications {
            let theme = t.theme().clone();
            fill_history(
                t,
                (n.notes, n.history),
                &theme,
                lang,
                &state.notifications,
                fit.notifications,
            );
        }
        if all || old.user != state.user {
            let initial: String = state
                .user
                .chars()
                .take(1)
                .flat_map(char::to_uppercase)
                .collect();
            t.set_text(n.initial, &initial);
            t.set_text(n.user, &state.user);
            t.set_name(n.account, &lang.account(&state.user));
        }
        // What the sections in between take may push the account out of
        // the card: the notifications, then the journal, give room back.
        if all || takes_other_room(&old, state) {
            self.fit_room();
        }
        // An open desktops popover follows the screens.
        if !all
            && old.screens != state.screens
            && self
                .popover
                .as_ref()
                .is_some_and(|p| p.kind == PopoverKind::Desktops)
        {
            self.open(PopoverKind::Desktops);
        }
        // An open keyboard menu follows the layouts.
        if !all
            && (old.layouts != state.layouts
                || old.layout != state.layout
                || old.switch_layout_keys != state.switch_layout_keys)
            && self
                .popover
                .as_ref()
                .is_some_and(|p| p.kind == PopoverKind::Keyboard)
        {
            self.open(PopoverKind::Keyboard);
        }
        // An open agenda follows the calendar and the day.
        if !all
            && (old.agenda != state.agenda || old.time.day != state.time.day)
            && self
                .popover
                .as_ref()
                .is_some_and(|p| p.kind == PopoverKind::Agenda)
        {
            self.open(PopoverKind::Agenda);
        }
        // An open phone popover follows the phone; it closes when it goes.
        if !all
            && old.phone != state.phone
            && self
                .popover
                .as_ref()
                .is_some_and(|p| p.kind == PopoverKind::Phone)
        {
            if state.phone.is_none() {
                self.close_popover();
            } else {
                self.open(PopoverKind::Phone);
            }
        }
        // An open journal follows the agents; it closes when it goes.
        if !all
            && old.journal != state.journal
            && self
                .popover
                .as_ref()
                .is_some_and(|p| p.kind == PopoverKind::Journal)
        {
            if state.journal.is_none() {
                self.close_popover();
            } else {
                self.open(PopoverKind::Journal);
            }
        }
        // An open tray follows its applications; it closes with the last.
        if !all
            && old.tray != state.tray
            && self
                .popover
                .as_ref()
                .is_some_and(|p| p.kind == PopoverKind::Tray)
        {
            if state.tray.is_empty() {
                self.close_popover();
            } else {
                self.open(PopoverKind::Tray);
            }
        }
    }

    /// A tray application's menu came: it shows where the tray was.
    pub fn show_tray_menu(&mut self, key: String, entries: Vec<TrayMenuEntry>) {
        if entries.is_empty() || self.nodes.tray.is_none() {
            return;
        }
        self.tray_menu = Some((key, entries));
        self.open(PopoverKind::TrayMenu);
    }

    /// The secondary button at `at`: over an application in the tray, its
    /// menu.
    pub fn secondary_click(&mut self, at: Point) -> Vec<Action> {
        let Some(p) = &self.popover else {
            return Vec::new();
        };
        if p.kind != PopoverKind::Tray || !self.in_popover(at) {
            return Vec::new();
        }
        let local = Point::new(at.x - p.at.x, at.y - p.at.y);
        p.targets
            .iter()
            .filter(|(id, _)| p.tree.rect(*id).is_some_and(|r| r.contains(local)))
            .find_map(|(_, action)| match action {
                Action::TrayActivate(key) | Action::TrayMenu(key) => {
                    Some(vec![Action::TrayMenu(key.clone())])
                }
                _ => None,
            })
            .unwrap_or_default()
    }

    // Popovers.

    fn open(&mut self, kind: PopoverKind) {
        // Opened again as the state changes: its anchor where it now is.
        self.tree.layout();
        let theme = self.tree.theme().clone();
        let scale = self.tree.scale();
        let anchor = match kind {
            PopoverKind::Desktops => self.nodes.desktops,
            PopoverKind::Keyboard => self.nodes.layout,
            PopoverKind::Account => self.nodes.account,
            PopoverKind::Tray | PopoverKind::TrayMenu => match self.nodes.tray {
                Some((ring, _)) => ring,
                None => return,
            },
            PopoverKind::Agenda => self.nodes.meeting,
            PopoverKind::Phone => match self.nodes.phone_line {
                Some(line) => line,
                None => return,
            },
            PopoverKind::Journal => match self.nodes.journal_entries {
                Some(entries) => entries,
                None => return,
            },
        };
        let anchor = self.tree.rect(anchor).unwrap_or_default();
        let (mut tree, targets) = match kind {
            PopoverKind::Desktops => {
                desktops_popover(&theme, self.lang, &self.state.screens, scale)
            }
            PopoverKind::Keyboard => keyboard_popover(&theme, self.lang, &self.state, scale),
            PopoverKind::Account => account_popover(
                &theme,
                self.lang,
                &self.state.user,
                self.state.can_hibernate,
                scale,
            ),
            PopoverKind::Tray => tray_popover(&theme, self.lang, &self.state.tray, scale),
            PopoverKind::TrayMenu => match &self.tray_menu {
                Some((key, entries)) => tray_menu_popover(&theme, key, entries, scale),
                None => return,
            },
            PopoverKind::Agenda => agenda_popover(&theme, self.lang, &self.state, scale),
            PopoverKind::Phone => match &self.state.phone {
                Some(p) => phone_popover(&theme, self.lang, p, scale),
                None => return,
            },
            PopoverKind::Journal => match &self.state.journal {
                Some(view) => journal_popover(&theme, self.lang, view, scale),
                None => return,
            },
        };
        let first = tree.update(self.now);
        let size = tree.size();
        // Left of the column; level with the line, or above the account.
        let y = match kind {
            PopoverKind::Desktops
            | PopoverKind::Keyboard
            | PopoverKind::Tray
            | PopoverKind::TrayMenu
            | PopoverKind::Agenda
            | PopoverKind::Phone
            | PopoverKind::Journal => anchor.y - SHADOW_ROOM,
            PopoverKind::Account => anchor.bottom() + SHADOW_ROOM - size.h,
        };
        let y = y.clamp(-MARGIN, (self.tree.size().h + MARGIN - size.h).max(-MARGIN));
        let at = Point::new(-(size.w - SHADOW_ROOM) - POPOVER_GAP, y);
        let mut surface = Surface::new(size, scale);
        surface.draw(&mut tree, &first);
        self.popover = Some(Popover {
            kind,
            tree,
            surface,
            at,
            targets,
        });
        self.popover_changed = true;
    }

    /// Whether the phone's popover is open: a pairing shown in it stops
    /// once it closes.
    pub fn phone_popover_open(&self) -> bool {
        self.popover
            .as_ref()
            .is_some_and(|p| p.kind == PopoverKind::Phone)
    }

    pub fn close_popover(&mut self) {
        if self.popover.take().is_some() {
            self.popover_changed = true;
        }
    }

    fn toggle(&mut self, kind: PopoverKind) {
        if self.closed_by_press.take() == Some(kind) {
            return;
        }
        if self.popover.as_ref().is_some_and(|p| p.kind == kind) {
            self.close_popover();
        } else {
            self.open(kind);
        }
    }

    // Input.

    fn in_column(&self, at: Point) -> bool {
        let s = self.tree.size();
        Rect::new(0.0, 0.0, s.w, s.h).contains(at)
    }

    fn in_popover(&self, at: Point) -> bool {
        self.popover.as_ref().is_some_and(|p| {
            let s = p.tree.size();
            // The card, not the room for its shadow.
            Rect::new(p.at.x, p.at.y, s.w, s.h)
                .inset(Insets::all(SHADOW_ROOM))
                .contains(at)
        })
    }

    /// Whether the pointer at `at` (relative to the column's corner) is over
    /// the column or its popover.
    pub fn hit(&self, at: Point) -> bool {
        self.in_column(at) || self.in_popover(at)
    }

    /// The pointer moved away from the column and its popover.
    pub fn pointer_left(&mut self) {
        self.tree.input(Input::PointerLeave);
        if let Some(p) = &mut self.popover {
            p.tree.input(Input::PointerLeave);
        }
    }

    /// The primary button pressed outside the column and its popover.
    pub fn clicked_elsewhere(&mut self) {
        self.close_popover();
    }

    /// Pointer input at `at`, relative to the column's corner.
    pub fn input(&mut self, at: Point, input: Input) -> Vec<Action> {
        let mut actions = Vec::new();
        if matches!(input, Input::PointerButton { pressed: true }) {
            self.closed_by_press = None;
        }
        let in_popover = self.in_popover(at);
        if let Some(p) = &mut self.popover {
            let local = Point::new(at.x - p.at.x, at.y - p.at.y);
            let forwarded = match input {
                Input::PointerMove(_) if in_popover => Input::PointerMove(local),
                Input::PointerMove(_) => Input::PointerLeave,
                other => other,
            };
            if in_popover || matches!(forwarded, Input::PointerLeave) {
                for event in p.tree.input(forwarded) {
                    let Event::Activated(id) = event else {
                        continue;
                    };
                    if let Some((_, action)) = p.targets.iter().find(|(t, _)| *t == id) {
                        actions.push(action.clone());
                    }
                }
            }
            if in_popover {
                // Pairing shows in the popover, which stays.
                let stays = actions
                    .iter()
                    .all(|a| matches!(a, Action::PairPhone | Action::CancelPairing));
                if !actions.is_empty() && !stays {
                    self.close_popover();
                }
                return actions;
            }
            if matches!(input, Input::PointerButton { pressed: true }) {
                self.closed_by_press = self.popover.as_ref().map(|p| p.kind);
                self.close_popover();
            }
        }
        let forwarded = match input {
            Input::PointerMove(_) if !self.in_column(at) => Input::PointerLeave,
            Input::PointerMove(_) => Input::PointerMove(at),
            other => other,
        };
        let n = self.nodes;
        for event in self.tree.input(forwarded) {
            let Event::Activated(id) = event else {
                continue;
            };
            if id == n.minimize {
                actions.push(Action::Minimize);
            } else if id == n.maximize {
                actions.push(Action::ToggleMaximize);
            } else if id == n.close {
                actions.push(Action::Close);
            } else if id == n.layout {
                self.toggle(PopoverKind::Keyboard);
            } else if id == n.command {
                actions.push(Action::OpenLauncher);
            } else if id == n.desktops {
                self.toggle(PopoverKind::Desktops);
            } else if id == n.account {
                self.toggle(PopoverKind::Account);
            } else if let Some((_, index)) = self.todo_rows.iter().find(|(row, _)| *row == id) {
                actions.push(Action::TickTodo(*index));
            } else if let Some((_, action)) = self.agent_targets.iter().find(|(t, _)| *t == id) {
                actions.push(action.clone());
            } else if id == n.time {
                self.toggle(PopoverKind::Agenda);
            } else if Some(id) == n.meeting_button {
                if self.state.meeting.as_ref().is_some_and(|m| m.join) {
                    actions.push(Action::JoinMeeting);
                } else {
                    self.toggle(PopoverKind::Agenda);
                }
            } else if Some(id) == n.install {
                actions.push(Action::InstallUpdates);
            } else if Some(id) == n.phone_ring {
                actions.push(Action::RingPhone);
            } else if Some(id) == n.phone_line {
                self.toggle(PopoverKind::Phone);
                if self
                    .popover
                    .as_ref()
                    .is_some_and(|p| p.kind == PopoverKind::Phone)
                {
                    actions.push(Action::PhoneOpened);
                }
            } else if Some(id) == n.journal_button {
                let stopped = self.state.journal.as_ref().is_some_and(|j| j.stopped);
                actions.push(if stopped {
                    Action::AllowAgents
                } else {
                    Action::StopAgents
                });
            } else if Some(id) == n.journal_entries {
                self.toggle(PopoverKind::Journal);
            } else if let Some((_, agent, taken)) =
                self.seats.buttons.iter().find(|(b, _, _)| *b == id)
            {
                actions.push(if *taken {
                    Action::HandBack(agent.clone())
                } else {
                    Action::TakeOver(agent.clone())
                });
            } else if Some(id) == n.stop_sharing {
                actions.push(Action::StopSharing);
            } else if n.tray.is_some_and(|(_, chip)| chip == id) {
                self.toggle(PopoverKind::Tray);
            }
        }
        actions
    }

    // Drawing.

    /// Draws what changed at `now`.
    pub fn draw(&mut self, now: Duration) -> Damage {
        self.now = now;
        let damage = self.tree.update(now);
        let column = self.surface.draw(&mut self.tree, &damage);
        let popover = match &mut self.popover {
            Some(p) => {
                let damage = p.tree.update(now);
                p.surface.draw(&mut p.tree, &damage)
            }
            None => Vec::new(),
        };
        Damage {
            column,
            popover,
            popover_changed: std::mem::take(&mut self.popover_changed),
        }
    }

    pub fn is_animating(&self, now: Duration) -> bool {
        self.tree.is_animating(now)
            || self
                .popover
                .as_ref()
                .is_some_and(|p| p.tree.is_animating(now))
    }

    pub fn view(&self) -> View<'_> {
        View {
            pixels: self.surface.pixels(),
            width: self.surface.width(),
            height: self.surface.height(),
            at: Point::default(),
        }
    }

    /// Its see-through panels, from its corner, for the compositor to blur
    /// what is behind them (#37).
    pub fn glass(&self) -> Vec<hocus_ui::Glass> {
        self.tree.glass()
    }

    /// The popover's see-through panels, from the column's corner.
    pub fn popover_glass(&self) -> Vec<hocus_ui::Glass> {
        self.popover.as_ref().map_or_else(Vec::new, |p| {
            let mut glass = p.tree.glass();
            for g in &mut glass {
                g.rect = g.rect.translate(p.at.x, p.at.y);
            }
            glass
        })
    }

    pub fn popover_view(&self) -> Option<View<'_>> {
        self.popover.as_ref().map(|p| View {
            pixels: p.surface.pixels(),
            width: p.surface.width(),
            height: p.surface.height(),
            at: p.at,
        })
    }

    /// The column and its popover for screen readers, rectangles relative to
    /// the column's corner.
    pub fn access(&self) -> AccessNode {
        let mut children = vec![self.tree.access()];
        if let Some(p) = &self.popover {
            let mut popover = p.tree.access();
            translate(&mut popover, p.at);
            children.push(popover);
        }
        AccessNode {
            role: Role::Group,
            label: None,
            rect: Rect::new(0.0, 0.0, self.tree.size().w, self.tree.size().h),
            focused: false,
            disabled: false,
            selected: false,
            children,
        }
    }
}

/// How many notifications, journal entries and agents the column shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fit {
    notifications: usize,
    journal: usize,
    agents: crate::agents::Fit,
    /// The meeting card on one line.
    compact_meeting: bool,
}

impl Fit {
    const MOST: Self = Self {
        notifications: NOTIFICATIONS,
        journal: crate::journal::ROWS,
        agents: crate::agents::Fit::MOST,
        compact_meeting: false,
    };

    /// One less: a notification while more than one shows, else a journal
    /// entry while more than one shows, else an other desktop's agents'
    /// line, else the meeting card on one line, else the waiting agents'
    /// rows in short, else the notifications altogether, else an agent
    /// shown in full; `None` when nothing gives more.
    fn less(self, state: &ColumnState) -> Option<Self> {
        // "Nothing new" takes a line too.
        let notifications = self.notifications.min(state.notifications.len().max(1));
        if notifications > 1 {
            return Some(Self {
                notifications: notifications - 1,
                ..self
            });
        }
        let journal = state
            .journal
            .as_ref()
            .map_or(0, |j| self.journal.min(j.lines.len()));
        if journal > 1 {
            return Some(Self {
                notifications,
                journal: journal - 1,
                ..self
            });
        }
        // Then the other desktops' agents share a line, which goes; the
        // meeting card takes one line; the waiting agents' rows are said
        // in short; the notifications' section goes altogether; last, the
        // agents shown in full go down to one.
        let less = Self {
            notifications,
            journal,
            ..self
        };
        let agents = state.agents.as_ref();
        if let Some(agents) = agents.and_then(|a| self.agents.less_lines(a)) {
            return Some(Self { agents, ..less });
        }
        if !self.compact_meeting {
            return Some(Self {
                compact_meeting: true,
                ..less
            });
        }
        if let Some(agents) = agents.and_then(|a| self.agents.less_compact(a)) {
            return Some(Self { agents, ..less });
        }
        if notifications > 0 {
            return Some(Self {
                notifications: 0,
                ..less
            });
        }
        agents
            .and_then(|a| self.agents.less_rows(a))
            .map(|agents| Self { agents, ..less })
    }
}

/// Whether what changed may take more or less room in the column.
fn takes_other_room(old: &ColumnState, new: &ColumnState) -> bool {
    old.notifications != new.notifications
        || old.journal != new.journal
        || old.agents != new.agents
        || old.todos != new.todos
        || old.updates != new.updates
        || old.phone != new.phone
        || old.sharing != new.sharing
        || old.meeting != new.meeting
        || old.calendar_set != new.calendar_set
}

/// The notifications' section: shown, or taking no room.
fn notes_room(shown: bool) -> Style {
    Style {
        gap: 8.0,
        height: if shown { Length::Auto } else { Length::Px(0.0) },
        // Shown, it is drawn as before: a clip would cut its edges.
        clip: !shown,
        ..Style::default()
    }
}

/// The latest notifications, `limit` at most, or that there is none; with
/// no room for any, the section takes none.
fn fill_history(
    tree: &mut Tree,
    (notes, history): (NodeId, NodeId),
    theme: &Theme,
    lang: Lang,
    notifications: &[(String, String)],
    limit: usize,
) {
    tree.set_style(notes, notes_room(limit > 0));
    for row in tree.children(history) {
        tree.remove(row);
    }
    if notifications.is_empty() {
        let _ = tree.add(
            history,
            Style::default(),
            Kind::text(lang.nothing_new(), theme.text.body.with_color(Paint::Muted)),
        );
    }
    for (app, summary) in notifications.iter().take(limit) {
        let Some(row) = tree.add(
            history,
            Style {
                direction: Direction::Row,
                gap: 10.0,
                align: Align::Center,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        ) else {
            continue;
        };
        let _ = tree.add(
            row,
            Style {
                width: Length::Px(22.0),
                height: Length::Px(22.0),
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::Rgba(crate::toasts::app_color(app))),
                radius: 6.0,
                ..Look::default()
            }),
        );
        let _ = tree.add(
            row,
            Style {
                width: Length::Fill,
                ..Style::default()
            },
            Kind::text_line(format!("{app} · {summary}"), theme.text.body),
        );
    }
}

fn translate(node: &mut AccessNode, by: Point) {
    node.rect = node.rect.translate(by.x, by.y);
    for child in &mut node.children {
        translate(child, by);
    }
}

fn caption(theme: &Theme) -> TextStyle {
    theme.text.caption
}

/// The column's nodes, empty: [`Column::apply`] fills them.
fn build(tree: &mut Tree, theme: &Theme, lang: Lang) -> Nodes {
    let root = tree.root();
    let missing = || unreachable!("adding to a node of this tree");
    tree.set_look(root, Look::default());
    let card = tree
        .add(
            root,
            Style {
                width: Length::Fill,
                height: Length::Fill,
                padding: Insets::symmetric(CARD_PADDING, 18.0),
                gap: 16.0,
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::Glass),
                radius: 24.0,
                border: Some(Paint::Border),
                shadow: 0.0,
            }),
        )
        .unwrap_or_else(missing);
    let row = |gap: f32| Style {
        direction: Direction::Row,
        gap,
        align: Align::Center,
        ..Style::default()
    };
    let add = |tree: &mut Tree, parent, style, kind| {
        tree.add(parent, style, kind).unwrap_or_else(missing)
    };

    // Status row: the keyboard layout works; the others are placeholders
    // until their modules come (sound, network, power, battery).
    // The status row, and under it the sharing line when there is one:
    // hidden, it takes no room.
    let top = add(tree, card, Style::default(), Kind::Box(Look::default()));
    let status = add(
        tree,
        top,
        Style {
            justify: Justify::SpaceBetween,
            ..row(6.0)
        },
        Kind::Box(Look::default()),
    );
    let (tray, layout, status_icons) = fill_status(tree, status, theme, lang, false);

    // While the screen is shared: said, with a way to stop it.
    let room = add(tree, top, sharing_room(false), Kind::Box(Look::default()));

    // Clock and date.
    let time = add(
        tree,
        card,
        Style {
            gap: 2.0,
            ..Style::default()
        },
        Kind::Box(Look::default()),
    );
    // The month opens from the clock.
    tree.set_clickable(time, lang.agenda());
    let clock = add(
        tree,
        time,
        Style::default(),
        Kind::text("", theme.text.display.with_weight(600)),
    );
    let date = add(
        tree,
        time,
        Style::default(),
        Kind::text("", theme.text.body.with_color(Paint::Muted)),
    );

    // The next meeting: filled with the calendar's.
    let meeting = add(
        tree,
        card,
        Style {
            padding: Insets::all(14.0),
            align: Align::Center,
            ..row(14.0)
        },
        Kind::Box(Look {
            background: Some(Paint::SurfaceHover),
            radius: theme.radii.large,
            ..Look::default()
        }),
    );

    // The active window and this screen's desktops.
    let active = add(
        tree,
        card,
        Style {
            gap: 10.0,
            ..Style::default()
        },
        Kind::Box(Look::default()),
    );
    add(
        tree,
        active,
        Style::default(),
        Kind::text(lang.active_window(), caption(theme)),
    );
    let controls = add(tree, active, row(8.0), Kind::Box(Look::default()));
    let title = add(
        tree,
        controls,
        Style {
            width: Length::Fill,
            ..Style::default()
        },
        Kind::text_line("", theme.text.title.with_weight(600)),
    );
    let square = Style {
        width: Length::Px(44.0),
        height: Length::Px(44.0),
        ..Style::default()
    };
    let control = |tree: &mut Tree, path: &str, name: &str| {
        add(
            tree,
            controls,
            square,
            Kind::icon_button(stroked(path, 16.0), name, ButtonKind::Chip, theme),
        )
    };
    let minimize = control(tree, icons::MINIMIZE, lang.minimize());
    let maximize = control(tree, icons::MAXIMIZE, lang.maximize());
    let close = control(tree, icons::CLOSE, lang.close());
    let desktops = add(
        tree,
        active,
        Style {
            justify: Justify::SpaceBetween,
            padding: Insets::symmetric(6.0, 8.0),
            ..row(8.0)
        },
        Kind::Box(Look {
            radius: theme.radii.small,
            ..Look::default()
        }),
    );
    tree.set_clickable(desktops, "");
    let desktops_text = add(
        tree,
        desktops,
        Style::default(),
        Kind::text(
            "",
            theme.text.label.with_weight(500).with_color(Paint::Muted),
        ),
    );
    let dots = add(
        tree,
        desktops,
        Style::default(),
        Kind::Custom {
            draw: Box::new(Dots {
                count: 1,
                shown: 1,
                badges: Vec::new(),
            }),
            label: None,
        },
    );

    // The command field: it opens the launcher.
    let command = add(
        tree,
        card,
        Style {
            height: Length::Px(52.0),
            padding: Insets::symmetric(0.0, 14.0),
            ..row(10.0)
        },
        Kind::Box(Look {
            background: Some(Paint::SurfaceHover),
            radius: 14.0,
            border: Some(Paint::Border),
            shadow: 0.0,
        }),
    );
    tree.set_clickable(command, lang.command_field());
    add(
        tree,
        command,
        Style::default(),
        Kind::Icon {
            icon: stroked(icons::SEARCH, 18.0),
            color: Paint::Muted,
            label: None,
        },
    );
    add(
        tree,
        command,
        Style {
            width: Length::Fill,
            ..Style::default()
        },
        Kind::text_line(
            lang.command_placeholder(),
            theme.text.title.with_weight(500).with_color(Paint::Muted),
        ),
    );

    // The coding agents and the to-dos when there are some (they take no
    // room otherwise), then the latest notifications.
    let feed = add(tree, card, Style::default(), Kind::Box(Look::default()));
    let agents = add(
        tree,
        feed,
        crate::agents::room(false),
        Kind::Box(Look::default()),
    );
    let journal = add(
        tree,
        feed,
        crate::journal::room(false),
        Kind::Box(Look::default()),
    );
    let tasks = add(tree, feed, tasks_room(false), Kind::Box(Look::default()));
    // Board D13: the phone, the updates, then the notifications.
    let phone = add(tree, feed, tasks_room(false), Kind::Box(Look::default()));
    let updates = add(tree, feed, tasks_room(false), Kind::Box(Look::default()));
    let notes = add(tree, feed, notes_room(true), Kind::Box(Look::default()));
    add(
        tree,
        notes,
        Style::default(),
        Kind::text(lang.notifications(), caption(theme)),
    );
    let history = add(
        tree,
        notes,
        Style {
            gap: 8.0,
            ..Style::default()
        },
        Kind::Box(Look::default()),
    );

    // What is left, then the account at the bottom.
    add(
        tree,
        card,
        Style {
            height: Length::Fill,
            ..Style::default()
        },
        Kind::Box(Look::default()),
    );
    let account = add(
        tree,
        card,
        Style {
            height: Length::Px(56.0),
            padding: Insets::symmetric(0.0, 10.0),
            ..row(12.0)
        },
        Kind::Box(Look {
            background: Some(Paint::SurfaceHover),
            radius: theme.radii.large,
            ..Look::default()
        }),
    );
    tree.set_clickable(account, "");
    let avatar = add(
        tree,
        account,
        Style {
            width: Length::Px(36.0),
            height: Length::Px(36.0),
            align: Align::Center,
            justify: Justify::Center,
            ..Style::default()
        },
        Kind::Box(Look {
            background: Some(Paint::Accent),
            radius: 18.0,
            ..Look::default()
        }),
    );
    let initial = add(
        tree,
        avatar,
        Style::default(),
        Kind::text(
            "",
            theme
                .text
                .title
                .with_weight(700)
                .with_color(Paint::OnAccent),
        ),
    );
    let user = add(
        tree,
        account,
        Style {
            width: Length::Fill,
            ..Style::default()
        },
        Kind::text_line("", theme.text.title),
    );
    add(
        tree,
        account,
        Style::default(),
        Kind::Icon {
            icon: stroked(icons::CHEVRON_UP, 16.0),
            color: Paint::Muted,
            label: None,
        },
    );
    Nodes {
        layout,
        clock,
        date,
        title,
        minimize,
        maximize,
        close,
        desktops,
        desktops_text,
        dots,
        command,
        history,
        account,
        initial,
        user,
        status: status_icons,
        status_row: status,
        tray,
        sharing: room,
        stop_sharing: None,
        tasks,
        updates,
        install: None,
        phone,
        phone_line: None,
        phone_ring: None,
        time,
        meeting,
        meeting_button: None,
        agents,
        card,
        notes,
        journal,
        journal_button: None,
        journal_entries: None,
    }
}

/// The meeting card: its ring, its words and its Join or Details, as board
/// Bureau draws it; `compact`, one line (when · title, and the button) for
/// a column short of room.
fn fill_meeting(
    tree: &mut Tree,
    card: NodeId,
    theme: &Theme,
    lang: Lang,
    meeting: Option<&Meeting>,
    calendar_set: bool,
    compact: bool,
) -> Option<NodeId> {
    let missing = || unreachable!("adding to a node of this tree");
    for child in tree.children(card) {
        tree.remove(child);
    }
    tree.set_style(
        card,
        Style {
            direction: Direction::Row,
            gap: if compact { 10.0 } else { 14.0 },
            align: Align::Center,
            padding: if compact {
                Insets::symmetric(8.0, 14.0)
            } else {
                Insets::all(14.0)
            },
            ..Style::default()
        },
    );
    if compact {
        let line = match meeting {
            Some(m) => format!("{} · {}", m.when, m.title),
            None => lang.no_meeting().to_owned(),
        };
        let paint = if meeting.is_some() {
            Paint::Text
        } else {
            Paint::Muted
        };
        tree.add(
            card,
            Style {
                width: Length::Fill,
                ..Style::default()
            },
            Kind::text_line(line, theme.text.title.with_weight(600).with_color(paint)),
        )
        .unwrap_or_else(missing);
        if !calendar_set {
            return None;
        }
        return Some(meeting_button(tree, card, theme, lang, meeting, 36.0));
    }
    let progress = meeting.map_or(0, |m| m.progress.min(100));
    tree.add(
        card,
        Style::default(),
        Kind::Custom {
            draw: Box::new(Ring {
                progress: f32::from(progress) / 100.0,
            }),
            label: None,
        },
    )
    .unwrap_or_else(missing);
    let words = tree
        .add(
            card,
            Style {
                width: Length::Fill,
                gap: 2.0,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        )
        .unwrap_or_else(missing);
    let (title, when) = match meeting {
        Some(m) => (m.title.as_str(), m.when.as_str()),
        None if calendar_set => (lang.no_meeting(), lang.nothing_planned()),
        None => (lang.no_meeting(), lang.calendar_not_set()),
    };
    // A meeting's title on one line, however long; the empty card's words
    // wrap.
    let style = theme.text.title.with_weight(700);
    let title = if meeting.is_some() {
        Kind::text_line(title, style)
    } else {
        Kind::text(title, style)
    };
    tree.add(words, Style::default(), title)
        .unwrap_or_else(missing);
    tree.add(
        words,
        Style::default(),
        Kind::text_line(when, theme.text.body.with_color(Paint::Muted)),
    )
    .unwrap_or_else(missing);
    if !calendar_set {
        return None;
    }
    Some(meeting_button(tree, card, theme, lang, meeting, 44.0))
}

/// Join when the meeting has a call and starts soon, else Details.
fn meeting_button(
    tree: &mut Tree,
    card: NodeId,
    theme: &Theme,
    lang: Lang,
    meeting: Option<&Meeting>,
    height: f32,
) -> NodeId {
    let (label, kind) = match meeting {
        Some(m) if m.join => (lang.join(), ButtonKind::Primary),
        _ => (lang.details(), ButtonKind::Plain),
    };
    tree.add(
        card,
        Style {
            height: Length::Px(height),
            padding: Insets::symmetric(0.0, 12.0),
            ..Style::default()
        },
        Kind::Button(Button::new(label, kind, theme)),
    )
    .unwrap_or_else(|| unreachable!("adding to a node of this tree"))
}

/// The to-dos' room above the notifications: some, or none.
fn tasks_room(shown: bool) -> Style {
    Style {
        gap: 8.0,
        padding: Insets {
            bottom: if shown { 18.0 } else { 0.0 },
            ..Insets::default()
        },
        height: if shown { Length::Auto } else { Length::Px(0.0) },
        clip: true,
        ..Style::default()
    }
}

/// The to-dos not done, three at most, as board D13 shows them: each a
/// box to tick.
fn fill_tasks(
    tree: &mut Tree,
    room: NodeId,
    theme: &Theme,
    lang: Lang,
    todos: &[(usize, String)],
) -> Vec<(NodeId, usize)> {
    let missing = || unreachable!("adding to a node of this tree");
    tree.add(
        room,
        Style::default(),
        Kind::text(
            lang.tasks_caption(todos.len()).to_uppercase(),
            caption(theme),
        ),
    )
    .unwrap_or_else(missing);
    let mut rows = Vec::new();
    for (index, text) in todos.iter().take(3) {
        let row = tree
            .add(
                room,
                Style {
                    direction: hocus_ui::Direction::Row,
                    gap: 10.0,
                    align: Align::Center,
                    height: Length::Px(28.0),
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(missing);
        tree.set_clickable(row, text);
        tree.add(
            row,
            Style {
                width: Length::Px(18.0),
                height: Length::Px(18.0),
                ..Style::default()
            },
            Kind::Custom {
                draw: Box::new(crate::checkbox::Checkbox(false)),
                label: None,
            },
        )
        .unwrap_or_else(missing);
        tree.add(
            row,
            Style {
                width: Length::Fill,
                ..Style::default()
            },
            Kind::text_line(text.clone(), theme.text.body),
        )
        .unwrap_or_else(missing);
        rows.push((row, *index));
    }
    rows
}

/// The updates' line, as board D13 draws it: how many, and Install;
/// while they install, said so.
fn fill_updates(
    tree: &mut Tree,
    room: NodeId,
    theme: &Theme,
    lang: Lang,
    count: usize,
    installing: bool,
) -> Option<NodeId> {
    let missing = || unreachable!("adding to a node of this tree");
    let line = tree
        .add(
            room,
            Style {
                direction: hocus_ui::Direction::Row,
                gap: 10.0,
                align: Align::Center,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        )
        .unwrap_or_else(missing);
    tree.add(
        line,
        Style {
            width: Length::Px(22.0),
            height: Length::Px(22.0),
            ..Style::default()
        },
        Kind::Box(Look {
            background: Some(Paint::Accent),
            radius: 6.0,
            ..Look::default()
        }),
    )
    .unwrap_or_else(missing);
    let text = if installing {
        lang.updates_installing().to_owned()
    } else {
        lang.updates_ready(count)
    };
    tree.add(
        line,
        Style {
            width: Length::Fill,
            ..Style::default()
        },
        Kind::text_line(text, theme.text.body),
    )
    .unwrap_or_else(missing);
    (!installing).then(|| {
        tree.add(
            line,
            Style {
                height: Length::Px(36.0),
                padding: Insets::symmetric(0.0, 10.0),
                ..Style::default()
            },
            Kind::Button(Button::new(lang.install(), ButtonKind::Plain, theme)),
        )
        .unwrap_or_else(missing)
    })
}

/// The phone's square, as board D13 colours it.
const PHONE_COLOR: Color = Color::rgb(0x2f, 0x6f, 0xd6);

/// The phone's line, as board D13 draws it: "Phone · 82 %", and Ring
/// through KDE Connect. The line opens the phone's popover; its line and
/// Ring, returned.
fn fill_phone(
    tree: &mut Tree,
    room: NodeId,
    theme: &Theme,
    lang: Lang,
    phone: &PhoneLine,
) -> (NodeId, Option<NodeId>) {
    let missing = || unreachable!("adding to a node of this tree");
    let row = |gap| Style {
        direction: hocus_ui::Direction::Row,
        gap,
        align: Align::Center,
        ..Style::default()
    };
    let line = tree
        .add(room, row(10.0), Kind::Box(Look::default()))
        .unwrap_or_else(missing);
    let open = tree
        .add(
            line,
            Style {
                width: Length::Fill,
                height: Length::Px(36.0),
                ..row(10.0)
            },
            Kind::Box(Look {
                radius: theme.radii.small,
                ..Look::default()
            }),
        )
        .unwrap_or_else(missing);
    tree.set_clickable(
        open,
        &lang.phone_name(&phone.name, phone.battery, phone.charging),
    );
    tree.add(
        open,
        Style {
            width: Length::Px(22.0),
            height: Length::Px(22.0),
            ..Style::default()
        },
        Kind::Box(Look {
            background: Some(Paint::Rgba(PHONE_COLOR)),
            radius: 6.0,
            ..Look::default()
        }),
    )
    .unwrap_or_else(missing);
    tree.add(
        open,
        Style {
            width: Length::Fill,
            ..Style::default()
        },
        Kind::text_line(lang.phone_line(phone.battery), theme.text.body),
    )
    .unwrap_or_else(missing);
    let ring = phone.connect.then(|| {
        tree.add(
            line,
            Style {
                height: Length::Px(36.0),
                padding: Insets::symmetric(0.0, 10.0),
                ..Style::default()
            },
            Kind::Button(Button::new(lang.ring_phone(), ButtonKind::Plain, theme)),
        )
        .unwrap_or_else(missing)
    });
    (open, ring)
}

/// The agents' journal, every entry the column holds.
fn journal_popover(
    theme: &Theme,
    lang: Lang,
    view: &crate::journal::JournalView,
    scale: f32,
) -> (Tree, Vec<(NodeId, Action)>) {
    let height = crate::journal::popover_height(view.lines.len());
    let (mut tree, card) = popover_tree(theme, 320.0, height, scale);
    crate::journal::fill_popover(&mut tree, card, theme, lang, view);
    (tree, Vec::new())
}

/// The phone's popover: its name and battery, then what it can do; while
/// pairing over Wi-Fi, the QR code instead.
fn phone_popover(
    theme: &Theme,
    lang: Lang,
    phone: &PhoneLine,
    scale: f32,
) -> (Tree, Vec<(NodeId, Action)>) {
    if let Some(pairing) = phone.adb.as_ref().and_then(|a| a.pairing.as_ref()) {
        return pairing_popover(theme, lang, pairing, scale);
    }
    let mut items = Vec::new();
    let mut lines = Vec::new();
    if phone.connect {
        items.extend([
            (phone::RING, lang.ring_phone(), Action::RingPhone),
            (
                phone::SEND,
                lang.send_clipboard(),
                Action::SendClipboardToPhone,
            ),
            (phone::FOLDER, lang.browse_phone(), Action::BrowsePhone),
        ]);
        lines.push(lang.phone_battery(phone.battery, phone.charging));
    }
    if let Some(adb) = &phone.adb {
        match &adb.device {
            Some((name, wireless)) => {
                lines.push(lang.adb_device(name, *wireless));
                if adb.mirror {
                    items.push((phone::MIRROR, lang.mirror_phone(), Action::MirrorPhone));
                }
            }
            None if !phone.connect => lines.push(lang.adb_none().to_owned()),
            None => {}
        }
        items.push((phone::QR, lang.pair_phone(), Action::PairPhone));
    }
    // Padding, the name and its lines, the buttons.
    let height =
        16.0 * 2.0 + 56.0 + lines.len().saturating_sub(1) as f32 * 22.0 + items.len() as f32 * 44.0;
    let (mut tree, card) = popover_tree(theme, 260.0, height, scale);
    let missing = || unreachable!("adding to a node of this tree");
    tree.set_style(
        card,
        Style {
            height: Length::Fill,
            padding: Insets::all(16.0),
            gap: 2.0,
            ..Style::default()
        },
    );
    let title = if phone.name.is_empty() {
        lang.phone_line(None)
    } else {
        phone.name.clone()
    };
    tree.add(
        card,
        Style {
            padding: Insets {
                left: 12.0,
                ..Insets::default()
            },
            ..Style::default()
        },
        Kind::text_line(title, theme.text.title.with_weight(700)),
    )
    .unwrap_or_else(missing);
    let last = lines.len().saturating_sub(1);
    for (i, line) in lines.into_iter().enumerate() {
        tree.add(
            card,
            Style {
                padding: Insets {
                    bottom: if i == last { 12.0 } else { 0.0 },
                    left: 12.0,
                    ..Insets::default()
                },
                ..Style::default()
            },
            Kind::text_line(line, theme.text.body.with_color(Paint::Muted)),
        )
        .unwrap_or_else(missing);
    }
    let mut targets = Vec::new();
    for (path, label, action) in items {
        let button = Button::new(label, ButtonKind::Ghost, theme)
            .with_icon(stroked(path, 18.0))
            .at_start();
        let id = tree
            .add(
                card,
                Style {
                    height: Length::Px(44.0),
                    ..Style::default()
                },
                Kind::Button(button),
            )
            .unwrap_or_else(missing);
        targets.push((id, action));
    }
    (tree, targets)
}

/// Pairing over Wi-Fi (#153): where to scan, the QR code, how it goes,
/// and Cancel (with Try again once it failed).
fn pairing_popover(
    theme: &Theme,
    lang: Lang,
    pairing: &Pairing,
    scale: f32,
) -> (Tree, Vec<(NodeId, Action)>) {
    const QR_SIDE: f32 = 196.0;
    let steps = lang.pair_steps();
    // Padding, the title, the steps, the code, its state, the buttons.
    let height = 16.0 * 2.0 + 28.0 + steps.len() as f32 * 20.0 + QR_SIDE + 22.0 + 40.0 + 4.0 * 10.0;
    let (mut tree, card) = popover_tree(theme, 300.0, height, scale);
    let missing = || unreachable!("adding to a node of this tree");
    tree.set_style(
        card,
        Style {
            height: Length::Fill,
            padding: Insets::all(16.0),
            gap: 10.0,
            ..Style::default()
        },
    );
    tree.add(
        card,
        Style::default(),
        Kind::text_line(lang.pair_phone(), theme.text.title.with_weight(700)),
    )
    .unwrap_or_else(missing);
    let said = tree
        .add(
            card,
            Style {
                gap: 0.0,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        )
        .unwrap_or_else(missing);
    for step in steps {
        tree.add(
            said,
            Style::default(),
            Kind::text_line(*step, theme.text.body.with_color(Paint::Muted)),
        )
        .unwrap_or_else(missing);
    }
    let code = tree
        .add(
            card,
            Style {
                align: Align::Center,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        )
        .unwrap_or_else(missing);
    if let Some(qr) = crate::showcase::QrCode::new(&pairing.qr) {
        tree.add(
            code,
            Style {
                width: Length::Px(QR_SIDE),
                height: Length::Px(QR_SIDE),
                ..Style::default()
            },
            Kind::Custom {
                draw: Box::new(qr),
                label: Some(lang.pair_code().to_owned()),
            },
        )
        .unwrap_or_else(missing);
    }
    let said = match &pairing.stage {
        PairStage::Waiting => lang.pair_waiting().to_owned(),
        PairStage::Pairing => lang.pairing().to_owned(),
        PairStage::Failed(why) => why.clone(),
    };
    tree.add(
        card,
        Style::default(),
        Kind::text_line(said, theme.text.body),
    )
    .unwrap_or_else(missing);
    let buttons = tree
        .add(
            card,
            Style {
                direction: Direction::Row,
                gap: 8.0,
                justify: Justify::End,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        )
        .unwrap_or_else(missing);
    let mut targets = Vec::new();
    if matches!(pairing.stage, PairStage::Failed(_)) {
        let again = tree
            .add(
                buttons,
                Style {
                    height: Length::Px(36.0),
                    padding: Insets::symmetric(0.0, 14.0),
                    ..Style::default()
                },
                Kind::Button(Button::new(lang.try_again(), ButtonKind::Plain, theme)),
            )
            .unwrap_or_else(missing);
        targets.push((again, Action::PairPhone));
    }
    let cancel = tree
        .add(
            buttons,
            Style {
                height: Length::Px(36.0),
                padding: Insets::symmetric(0.0, 14.0),
                ..Style::default()
            },
            Kind::Button(Button::new(lang.cancel_pairing(), ButtonKind::Plain, theme)),
        )
        .unwrap_or_else(missing);
    targets.push((cancel, Action::CancelPairing));
    (tree, targets)
}

/// The sharing line: said, with its stop.
fn fill_sharing(tree: &mut Tree, room: NodeId, theme: &Theme, lang: Lang) -> NodeId {
    let missing = || unreachable!("adding to a node of this tree");
    let add = |tree: &mut Tree, parent, style, kind| {
        tree.add(parent, style, kind).unwrap_or_else(missing)
    };
    let sharing = add(tree, room, sharing_style(), Kind::Box(sharing_look()));
    add(
        tree,
        sharing,
        Style {
            width: Length::Px(10.0),
            height: Length::Px(10.0),
            ..Style::default()
        },
        Kind::Box(Look {
            background: Some(Paint::Rgba(crate::recording::RED)),
            radius: 5.0,
            ..Look::default()
        }),
    );
    add(
        tree,
        sharing,
        Style {
            width: Length::Fill,
            ..Style::default()
        },
        Kind::text_line(lang.sharing_now(), theme.text.label.with_weight(600)),
    );
    add(
        tree,
        sharing,
        Style {
            height: Length::Px(34.0),
            padding: Insets::symmetric(0.0, 12.0),
            ..Style::default()
        },
        Kind::Button(
            Button::new(lang.stop(), ButtonKind::Plain, theme).with_name(lang.stop_sharing()),
        ),
    )
}

/// The sharing line's room under the status row: some, or none.
fn sharing_room(shown: bool) -> Style {
    Style {
        padding: Insets {
            top: if shown { 10.0 } else { 0.0 },
            ..Insets::default()
        },
        height: if shown { Length::Auto } else { Length::Px(0.0) },
        clip: true,
        ..Style::default()
    }
}

fn sharing_style() -> Style {
    Style {
        direction: hocus_ui::Direction::Row,
        align: Align::Center,
        gap: 10.0,
        padding: Insets {
            left: 14.0,
            right: 6.0,
            top: 6.0,
            bottom: 6.0,
        },
        ..Style::default()
    }
}

fn sharing_look() -> Look {
    Look {
        background: Some(Paint::Rgba(crate::recording::RED.with_alpha(0x24))),
        radius: 14.0,
        border: Some(Paint::Rgba(crate::recording::RED.with_alpha(0x80))),
        ..Look::default()
    }
}

/// The status row's chips, the tray's first when there are applications
/// in it; built again when that changes.
fn fill_status(
    tree: &mut Tree,
    status: NodeId,
    theme: &Theme,
    lang: Lang,
    with_tray: bool,
) -> (Option<(NodeId, NodeId)>, NodeId, [NodeId; 4]) {
    let missing = || unreachable!("adding to a node of this tree");
    let add = |tree: &mut Tree, parent, style, kind| {
        tree.add(parent, style, kind).unwrap_or_else(missing)
    };
    for child in tree.children(status) {
        tree.remove(child);
    }
    let chip = Style {
        height: Length::Px(44.0),
        ..Style::default()
    };
    // The chip sits in a ring, drawn when an application waits.
    let tray = with_tray.then(|| {
        let ring = add(
            tree,
            status,
            Style {
                width: Length::Px(52.0),
                height: Length::Px(44.0),
                padding: Insets::all(1.5),
                ..Style::default()
            },
            Kind::Box(tray_ring(theme, false)),
        );
        let chip = add(
            tree,
            ring,
            Style {
                width: Length::Fill,
                height: Length::Fill,
                ..Style::default()
            },
            Kind::Button(
                Button::new("", ButtonKind::Chip, theme)
                    .with_icon(stroked(icons::TRAY, 18.0))
                    .with_style(theme.text.label.with_weight(700)),
            ),
        );
        (ring, chip)
    });
    let layout = add(
        tree,
        status,
        Style {
            width: Length::Px(if with_tray { 56.0 } else { 64.0 }),
            ..chip
        },
        Kind::Button(
            Button::new("", ButtonKind::Chip, theme)
                .with_icon(stroked(icons::KEYBOARD, 18.0))
                .with_style(theme.text.label.with_weight(700)),
        ),
    );
    let mut status_icons = Vec::new();
    for (path, name) in [
        (icons::LEAF, lang.power_profile()),
        (icons::WIFI, lang.network()),
        (icons::SOUND, lang.sound()),
        (icons::BATTERY, lang.battery()),
    ] {
        let side = if with_tray { 38.0 } else { 44.0 };
        let holder = add(
            tree,
            status,
            Style {
                width: Length::Px(side),
                height: Length::Px(44.0),
                align: Align::Center,
                justify: Justify::Center,
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                radius: theme.radii.medium,
                ..Look::default()
            }),
        );
        status_icons.push(add(
            tree,
            holder,
            Style::default(),
            Kind::Icon {
                icon: stroked(path, 18.0),
                color: Paint::Text,
                label: Some(name.to_owned()),
            },
        ));
    }
    let status_icons: [NodeId; 4] = status_icons
        .try_into()
        .unwrap_or_else(|_| unreachable!("four status icons"));
    (tray, layout, status_icons)
}

fn tray_ring(theme: &Theme, waiting: bool) -> Look {
    Look {
        border: waiting.then_some(Paint::Waiting),
        radius: theme.radii.medium + 1.5,
        ..Look::default()
    }
}

/// The tray's applications: each with its icon, Enter or a click
/// activates it, its menu button (or a right click) shows its menu.
fn tray_popover(
    theme: &Theme,
    lang: Lang,
    apps: &[TrayApp],
    scale: f32,
) -> (Tree, Vec<(NodeId, Action)>) {
    let rows = apps.len().max(1) as f32;
    let height = 16.0 * 2.0 + 30.0 + rows * 48.0;
    let (mut tree, card) = popover_tree(theme, 300.0, height, scale);
    let missing = || unreachable!("adding to a node of this tree");
    tree.set_style(
        card,
        Style {
            height: Length::Fill,
            padding: Insets::all(16.0),
            gap: 4.0,
            ..Style::default()
        },
    );
    tree.add(
        card,
        Style {
            padding: Insets {
                bottom: 8.0,
                left: 8.0,
                ..Insets::default()
            },
            ..Style::default()
        },
        Kind::text(lang.tray_title(), theme.text.title.with_weight(700)),
    )
    .unwrap_or_else(missing);
    let mut targets = Vec::new();
    for app in apps {
        // Waiting for attention: a ring, drawn once, never blinking.
        let line = tree
            .add(
                card,
                Style {
                    direction: hocus_ui::Direction::Row,
                    height: Length::Px(44.0),
                    gap: 6.0,
                    align: Align::Center,
                    padding: Insets {
                        left: 8.0,
                        ..Insets::default()
                    },
                    ..Style::default()
                },
                Kind::Box(Look {
                    border: app.attention.then_some(Paint::Waiting),
                    radius: theme.radii.medium,
                    ..Look::default()
                }),
            )
            .unwrap_or_else(missing);
        if let Some((w, h, pixels)) = &app.icon
            && let Some(image) = Kind::image(*w, *h, pixels, &app.title)
        {
            tree.add(
                line,
                Style {
                    width: Length::Px(24.0),
                    height: Length::Px(24.0),
                    ..Style::default()
                },
                image,
            )
            .unwrap_or_else(missing);
        }
        let button = Button::new(app.title.as_str(), ButtonKind::Ghost, theme)
            .with_style(theme.text.body)
            .at_start();
        let row = tree
            .add(
                line,
                Style {
                    width: Length::Fill,
                    height: Length::Px(44.0),
                    ..Style::default()
                },
                Kind::Button(button),
            )
            .unwrap_or_else(missing);
        targets.push((row, Action::TrayActivate(app.key.clone())));
        let menu = tree
            .add(
                line,
                Style {
                    width: Length::Px(36.0),
                    height: Length::Px(36.0),
                    ..Style::default()
                },
                Kind::icon_button(
                    stroked(icons::MORE, 16.0),
                    &lang.menu_of(&app.title),
                    ButtonKind::Ghost,
                    theme,
                ),
            )
            .unwrap_or_else(missing);
        targets.push((menu, Action::TrayMenu(app.key.clone())));
    }
    (tree, targets)
}

/// An application's menu, from its dbusmenu.
fn tray_menu_popover(
    theme: &Theme,
    key: &str,
    entries: &[TrayMenuEntry],
    scale: f32,
) -> (Tree, Vec<(NodeId, Action)>) {
    fn flat(entries: &[TrayMenuEntry], depth: usize, out: &mut Vec<(usize, TrayMenuEntry)>) {
        for e in entries {
            out.push((depth, e.clone()));
            flat(&e.children, depth + 1, out);
        }
    }
    let mut lines = Vec::new();
    flat(entries, 0, &mut lines);
    let height = 16.0 * 2.0
        + lines
            .iter()
            .map(|(_, e)| if e.separator { 9.0 } else { 40.0 })
            .sum::<f32>();
    let (mut tree, card) = popover_tree(theme, 280.0, height, scale);
    let missing = || unreachable!("adding to a node of this tree");
    tree.set_style(
        card,
        Style {
            height: Length::Fill,
            padding: Insets::all(16.0),
            gap: 1.0,
            ..Style::default()
        },
    );
    let mut targets = Vec::new();
    for (depth, entry) in lines {
        if entry.separator {
            let room = tree
                .add(
                    card,
                    Style {
                        height: Length::Px(8.0),
                        padding: Insets::symmetric(3.5, 8.0),
                        ..Style::default()
                    },
                    Kind::Box(Look::default()),
                )
                .unwrap_or_else(missing);
            tree.add(
                room,
                Style {
                    width: Length::Fill,
                    height: Length::Px(1.0),
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Border),
                    ..Look::default()
                }),
            )
            .unwrap_or_else(missing);
            continue;
        }
        let label = match entry.toggle {
            Some(true) => format!("✓ {}", entry.label),
            Some(false) => format!("   {}", entry.label),
            None => entry.label.clone(),
        };
        let mut button = Button::new(label, ButtonKind::Ghost, theme)
            .with_style(theme.text.body)
            .at_start();
        button.enabled = entry.enabled && entry.children.is_empty();
        let id = tree
            .add(
                card,
                Style {
                    height: Length::Px(39.0),
                    padding: Insets {
                        left: depth as f32 * 16.0,
                        ..Insets::default()
                    },
                    ..Style::default()
                },
                Kind::Button(button),
            )
            .unwrap_or_else(missing);
        targets.push((
            id,
            Action::TrayChosen {
                key: key.to_owned(),
                id: entry.id,
            },
        ));
    }
    (tree, targets)
}

/// The agenda: the month, its week numbers, and today's events.
fn agenda_popover(
    theme: &Theme,
    lang: Lang,
    state: &ColumnState,
    scale: f32,
) -> (Tree, Vec<(NodeId, Action)>) {
    const CELL: f32 = 34.0;
    const ROW: f32 = 30.0;
    const SHOWN: usize = 6;
    let a = &state.agenda;
    let events = a.today.len().clamp(1, SHOWN);
    let height = 16.0 * 2.0
        + 28.0
        + 12.0
        + ROW * (1 + a.weeks.len()) as f32
        + 12.0
        + 22.0
        + 28.0 * events as f32;
    let (mut tree, card) = popover_tree(theme, 8.0 * CELL + 32.0, height, scale);
    let missing = || unreachable!("adding to a node of this tree");
    let add = |tree: &mut Tree, parent, style, kind| {
        tree.add(parent, style, kind).unwrap_or_else(missing)
    };
    add(
        &mut tree,
        card,
        Style::default(),
        Kind::text(
            lang.month_year(state.time.month, a.year),
            theme.text.title.with_weight(700),
        ),
    );
    let grid = add(
        &mut tree,
        card,
        Style::default(),
        Kind::Box(Look::default()),
    );
    let cell = |justify| Style {
        width: Length::Px(CELL),
        height: Length::Px(ROW),
        align: Align::Center,
        justify,
        ..Style::default()
    };
    let line = |tree: &mut Tree| {
        add(
            tree,
            grid,
            Style {
                direction: Direction::Row,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        )
    };
    // A word in the middle of its cell, over room for a day's dot: level
    // with the days.
    let dot_room = Style {
        width: Length::Px(4.0),
        height: Length::Px(4.0),
        ..Style::default()
    };
    let centered = |tree: &mut Tree, row, kind| {
        let style = Style {
            gap: 1.0,
            ..cell(Justify::Center)
        };
        let place = add(tree, row, style, Kind::Box(Look::default()));
        add(tree, place, Style::default(), kind);
        add(tree, place, dot_room, Kind::Box(Look::default()));
    };
    let small = theme.text.caption.with_color(Paint::Muted);
    let heading = line(&mut tree);
    centered(&mut tree, heading, Kind::text(lang.week_short(), small));
    for letter in lang.weekday_letters() {
        centered(
            &mut tree,
            heading,
            Kind::text(letter, small.with_weight(700)),
        );
    }
    for (row, week) in a.weeks.iter().enumerate() {
        let days = line(&mut tree);
        centered(&mut tree, days, Kind::text(week.to_string(), small));
        for column in 0..7u8 {
            let slot = row as i32 * 7 + i32::from(column) - i32::from(a.first_weekday) + 1;
            let day = u8::try_from(slot).ok().filter(|d| (1..=a.days).contains(d));
            let today = day == Some(state.time.day);
            let look = Look {
                background: today.then_some(Paint::Accent),
                radius: ROW / 2.0,
                ..Look::default()
            };
            let place = add(
                &mut tree,
                days,
                Style {
                    gap: 1.0,
                    ..cell(Justify::Center)
                },
                Kind::Box(look),
            );
            let Some(day) = day else {
                continue;
            };
            let busy = a.busy.contains(&day);
            let ink = if today { Paint::OnAccent } else { Paint::Text };
            let weight = if busy { 700 } else { 400 };
            add(
                &mut tree,
                place,
                Style::default(),
                Kind::text(
                    day.to_string(),
                    theme.text.body.with_color(ink).with_weight(weight),
                ),
            );
            // A day with events: a dot under it, in the day's ink on today.
            let dot = if today {
                Paint::OnAccent
            } else {
                Paint::Accent
            };
            add(
                &mut tree,
                place,
                dot_room,
                Kind::Box(Look {
                    background: busy.then_some(dot),
                    radius: 2.0,
                    ..Look::default()
                }),
            );
        }
    }
    add(
        &mut tree,
        card,
        Style::default(),
        Kind::text(lang.today_upper(), small.with_weight(700)),
    );
    if a.today.is_empty() {
        add(
            &mut tree,
            card,
            Style::default(),
            Kind::text(
                lang.nothing_today(),
                theme.text.body.with_color(Paint::Muted),
            ),
        );
    }
    for (time, title) in a.today.iter().take(SHOWN) {
        let row = add(
            &mut tree,
            card,
            Style {
                direction: Direction::Row,
                gap: 10.0,
                align: Align::Center,
                height: Length::Px(24.0),
                ..Style::default()
            },
            Kind::Box(Look::default()),
        );
        add(
            &mut tree,
            row,
            Style {
                width: Length::Px(64.0),
                ..Style::default()
            },
            Kind::text_line(time.clone(), theme.text.body.with_color(Paint::Muted)),
        );
        add(
            &mut tree,
            row,
            Style {
                width: Length::Fill,
                ..Style::default()
            },
            Kind::text_line(title.clone(), theme.text.body),
        );
    }
    (tree, Vec::new())
}

/// A popover's tree: a card with room around it for its shadow.
fn popover_tree(theme: &Theme, width: f32, height: f32, scale: f32) -> (Tree, NodeId) {
    let size = Size::new(width + 2.0 * SHADOW_ROOM, height + 2.0 * SHADOW_ROOM);
    let mut tree = Tree::new(theme.clone(), size, scale);
    let root = tree.root();
    tree.set_look(root, Look::default());
    tree.set_style(
        root,
        Style {
            padding: Insets::all(SHADOW_ROOM),
            ..Style::default()
        },
    );
    let card = tree
        .add(
            root,
            Style {
                height: Length::Fill,
                padding: Insets::all(16.0),
                gap: 12.0,
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::Glass),
                radius: 20.0,
                border: Some(Paint::Border),
                shadow: 18.0,
            }),
        )
        .unwrap_or_else(|| unreachable!("the root takes children"));
    (tree, card)
}

fn desktops_popover(
    theme: &Theme,
    lang: Lang,
    screens: &[ScreenDesktops],
    scale: f32,
) -> (Tree, Vec<(NodeId, Action)>) {
    let height = 16.0 * 2.0 + 24.0 + screens.len() as f32 * (12.0 + 20.0 + 6.0 + 40.0);
    let (mut tree, card) = popover_tree(theme, 300.0, height, scale);
    let missing = || unreachable!("adding to a node of this tree");
    tree.add(
        card,
        Style::default(),
        Kind::text(lang.desktops_title(), theme.text.title.with_weight(700)),
    )
    .unwrap_or_else(missing);
    let mut targets = Vec::new();
    for screen in screens {
        let block = tree
            .add(
                card,
                Style {
                    gap: 6.0,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(missing);
        let head = tree
            .add(
                block,
                Style {
                    direction: Direction::Row,
                    justify: Justify::SpaceBetween,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(missing);
        tree.add(
            head,
            Style::default(),
            Kind::text(screen.output.as_str(), theme.text.label),
        )
        .unwrap_or_else(missing);
        tree.add(
            head,
            Style::default(),
            Kind::text(
                lang.desktops_count(screen.count),
                theme.text.label.with_color(Paint::Muted),
            ),
        )
        .unwrap_or_else(missing);
        let chips = tree
            .add(
                block,
                Style {
                    direction: Direction::Row,
                    gap: 6.0,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(missing);
        for d in 1..=screen.count {
            let badge = screen.badges.get(d - 1).copied().unwrap_or_default();
            let named = screen.names.get(d - 1).is_some_and(|n| !n.is_empty());
            let mut name = format!(
                "{} · {}",
                screen.output,
                if named {
                    screen.label(lang, d - 1)
                } else {
                    lang.desktop(d)
                }
            );
            if let Some(what) = lang.badge(badge) {
                name = format!("{name} · {what}");
            }
            let mut button = Button::new(d.to_string(), ButtonKind::Chip, theme).with_name(name);
            button.selected = d == screen.shown;
            let chip = Style {
                width: Length::Px(58.0),
                height: Length::Px(40.0),
                ..Style::default()
            };
            let id = tree
                .add(chips, chip, Kind::Button(button))
                .unwrap_or_else(missing);
            // Its badge, over its corner.
            if badge.is_some() {
                tree.add(
                    id,
                    Style {
                        at: Some(Point::new(46.0, 4.0)),
                        width: Length::Px(8.0),
                        height: Length::Px(8.0),
                        ..Style::default()
                    },
                    Kind::Custom {
                        draw: Box::new(crate::activity::BadgeDot(badge)),
                        label: None,
                    },
                )
                .unwrap_or_else(missing);
            }
            targets.push((
                id,
                Action::ShowDesktop {
                    output: screen.output.clone(),
                    desktop: d,
                },
            ));
        }
    }
    (tree, targets)
}

/// Board D13's keyboard menu: each layout kept with its short name, the
/// one in use marked; the keys that go to the next one.
fn keyboard_popover(
    theme: &Theme,
    lang: Lang,
    state: &ColumnState,
    scale: f32,
) -> (Tree, Vec<(NodeId, Action)>) {
    const ROW: f32 = 48.0;
    const GAP: f32 = 12.0;
    let rows = state.layouts.len().max(1) as f32;
    let keys = state.switch_layout_keys.as_deref();
    let height =
        20.0 * 2.0 + 24.0 + rows * (GAP + ROW) + if keys.is_some() { GAP + 28.0 } else { 0.0 };
    let (mut tree, card) = popover_tree(theme, 360.0, height, scale);
    let missing = || unreachable!("adding to a node of this tree");
    tree.set_style(
        card,
        Style {
            height: Length::Fill,
            padding: Insets::all(20.0),
            gap: GAP,
            ..Style::default()
        },
    );
    tree.add(
        card,
        Style::default(),
        Kind::text(lang.keyboard_title(), theme.text.title.with_weight(700)),
    )
    .unwrap_or_else(missing);
    let mono = TextStyle {
        typeface: hocus_ui::Typeface::Mono,
        ..theme.text.label
    }
    .with_color(Paint::Muted);
    let mut targets = Vec::new();
    let active = if state.layouts.is_empty() {
        None
    } else {
        Some(state.layout.min(state.layouts.len() - 1))
    };
    for (i, layout) in state.layouts.iter().enumerate() {
        let on = active == Some(i);
        let line = tree
            .add(
                card,
                Style {
                    direction: Direction::Row,
                    align: Align::Center,
                    gap: 12.0,
                    width: Length::Fill,
                    height: Length::Px(ROW),
                    padding: Insets::symmetric(0.0, 12.0),
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: on.then(|| Paint::Rgba(theme.colors.accent.with_alpha(0x1a))),
                    radius: 12.0,
                    ..Look::default()
                }),
            )
            .unwrap_or_else(missing);
        tree.set_clickable(line, &layout.name);
        tree.set_selected_box(line, on);
        tree.add(
            line,
            Style {
                width: Length::Px(18.0),
                height: Length::Px(18.0),
                ..Style::default()
            },
            Kind::Custom {
                draw: Box::new(crate::checkbox::Radio(on)),
                label: None,
            },
        )
        .unwrap_or_else(missing);
        tree.add(
            line,
            Style {
                width: Length::Fill,
                ..Style::default()
            },
            Kind::text_line(layout.name.as_str(), theme.text.body),
        )
        .unwrap_or_else(missing);
        tree.add(
            line,
            Style::default(),
            Kind::text_line(layout.short.as_str(), mono),
        )
        .unwrap_or_else(missing);
        targets.push((line, Action::SetLayout(i)));
    }
    if let Some(keys) = keys {
        let line = tree
            .add(
                card,
                Style {
                    direction: Direction::Row,
                    align: Align::Center,
                    justify: Justify::SpaceBetween,
                    gap: 12.0,
                    width: Length::Fill,
                    height: Length::Px(28.0),
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(missing);
        tree.add(
            line,
            Style::default(),
            Kind::text_line(
                lang.switch_layout(),
                theme.text.label.with_color(Paint::Muted),
            ),
        )
        .unwrap_or_else(missing);
        let cap = tree
            .add(
                line,
                Style {
                    padding: Insets::symmetric(4.0, 10.0),
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::SurfaceHover),
                    radius: 8.0,
                    ..Look::default()
                }),
            )
            .unwrap_or_else(missing);
        tree.add(
            cap,
            Style::default(),
            Kind::text_line(keys, mono.with_color(Paint::Text)),
        )
        .unwrap_or_else(missing);
    }
    (tree, targets)
}

fn account_popover(
    theme: &Theme,
    lang: Lang,
    user: &str,
    can_hibernate: bool,
    scale: f32,
) -> (Tree, Vec<(NodeId, Action)>) {
    let mut items = vec![
        (icons::LOCK, lang.lock(), Action::Lock),
        (icons::MOON, lang.suspend(), Action::Suspend),
    ];
    if can_hibernate {
        items.push((icons::MOON, lang.hibernate(), Action::Hibernate));
    }
    items.extend([
        (icons::SETTINGS, lang.settings(), Action::Settings),
        (icons::LOG_OUT, lang.log_out(), Action::LogOut),
        (icons::RESTART, lang.restart(), Action::Restart),
        (icons::POWER, lang.shut_down(), Action::ShutDown),
    ]);
    let height = 16.0 * 2.0 + 24.0 + 12.0 + items.len() as f32 * 44.0 + 12.0;
    let (mut tree, card) = popover_tree(theme, 260.0, height, scale);
    let missing = || unreachable!("adding to a node of this tree");
    tree.set_style(
        card,
        Style {
            height: Length::Fill,
            padding: Insets::all(16.0),
            gap: 2.0,
            ..Style::default()
        },
    );
    tree.add(
        card,
        Style {
            padding: Insets {
                bottom: 12.0,
                left: 12.0,
                ..Insets::default()
            },
            ..Style::default()
        },
        Kind::text(user, theme.text.title.with_weight(700)),
    )
    .unwrap_or_else(missing);
    let mut targets = Vec::new();
    for (path, label, action) in items {
        // Shutting down in the error colour, as the design has it.
        let ink = if action == Action::ShutDown {
            theme.text.body.with_color(Paint::Error)
        } else {
            theme.text.body
        };
        let button = Button::new(label, ButtonKind::Ghost, theme)
            .with_icon(stroked(path, 18.0))
            .with_style(ink)
            .at_start();
        let id = tree
            .add(
                card,
                Style {
                    height: Length::Px(44.0),
                    ..Style::default()
                },
                Kind::Button(button),
            )
            .unwrap_or_else(missing);
        targets.push((id, action));
    }
    (tree, targets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_parses() {
        for path in [
            icons::MINIMIZE,
            icons::FILE,
            icons::MAXIMIZE,
            icons::RESTORE,
            icons::CLOSE,
            icons::KEYBOARD,
            icons::LEAF,
            icons::WIFI,
            icons::SOUND,
            icons::BATTERY,
            icons::CHEVRON_UP,
            icons::LOCK,
            icons::SETTINGS,
            icons::LOG_OUT,
            icons::RESTART,
            icons::POWER,
            icons::MOON,
        ] {
            assert!(Icon::stroked(path, 16.0, 2.0).is_some(), "{path}");
        }
    }
}
