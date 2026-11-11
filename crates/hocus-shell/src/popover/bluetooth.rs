//! Bluetooth's popover (bar.md §9, the 3.6 mockup, variant A): its title
//! and its switch; each device connected in a block of its own (its
//! kind's icon, its name, "Connecté · batterie 82 %", Disconnect); "MES
//! APPAREILS", the paired devices not connected, one row each, a click
//! connecting one; "AUTOUR", the devices found around while the popover is
//! open (named ones only), the strongest first, a click pairing one, those
//! asking for a code say "dans les Réglages": a click opens the Settings'
//! Bluetooth page, which pairs it (`--pair ADDRESS`); past four, that list
//! scrolls. Its last row, "Réglages du Bluetooth…", opens that page.
//!
//! A row asked for says "Connexion…" or "Association…" with a spinner
//! until BlueZ is done, the others inert meanwhile; a failure says so
//! under its row. The nearby rows hold their order while the pointer is
//! over them, one holds the keyboard, or something runs: rows do not move
//! under the pointer; newcomers join at the end.
//!
//! Built with the shell's widget kit ([`crate::ui`]), as the Wi-Fi's.

use std::{
    sync::{Mutex, PoisonError},
    time::Instant,
};

use hocus_a11y::Role;
use iced::{
    Color, Element, Padding, Point,
    advanced::widget::Id,
    alignment::Vertical,
    widget::{Space, column, container, row, space, text::LineHeight},
    window,
};

use super::{
    Item,
    kit::{
        self, HEADER, HEADER_TOP, ICON, ICON_GAP, LEFT, List, MANROPE_SEMIBOLD, NAME, NOTE_HEIGHT,
        PEACH, RIGHT, ROW, ROW_GAP, ROW_OUT, ROW_PAD, ROW_RADIUS, ROW_TEXT, SCROLLBAR, SMALL,
        SMALL_PAD, SMALL_RADIUS, STATUS, SWITCH, Switch,
    },
};
use crate::{
    bluetooth::{
        self, Ask, Bluetooth,
        draw::kind,
        model::{Action, Device, Failure, Step},
    },
    levels::{
        draw::{ACCENT, DIM, FAINT, LINE, MANROPE, TEXT},
        model::Words,
    },
    ui::{
        self, Around, Later, control,
        parts::{self, Knob, drawn, gap, note, spinner},
        style::said,
    },
};

type El<'a> = Element<'a, Msg>;

// --- The mockup's sizes, at ×1 ------------------------------------------------

const GAP: f32 = 6.0;
/// The divider, 1 px with 4 px around it.
const RULE: f32 = 4.0;
/// A label's line: its padding and the "Recherche…" beside it.
const LABEL_ROW: f32 = 2.0 + SMALL * LINE + 2.0;
/// The nearby list's height at most: four rows.
const LIST: f32 = 152.0;
/// A failure under its row: 12.5 px on 1.4 lines, 34 px in, 6 px under.
const ERROR: f32 = 12.5;
const ERROR_LINE: f32 = 1.4;
const ERROR_INDENT: f32 = 34.0;
const ERROR_BOTTOM: f32 = 6.0;
/// The room it wraps in: the row's width less its indent and padding.
const ERROR_ROOM: f32 = RIGHT - LEFT + 2.0 * ROW_OUT - ERROR_INDENT - ROW_PAD;
/// The Settings' link at the bottom (the sound's, 6.5): its divider, its
/// row.
const LINK: f32 = 36.0;

const LIST_ID: &str = "bluetooth:list";

// --- What the popover keeps while it is open -----------------------------------

/// What can hold the keyboard's ring, and be acted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Switch,
    /// A connected device's Disconnect, by its place.
    Disconnect(usize),
    /// A paired device's row.
    Mine(usize),
    /// A device around's row.
    Near(usize),
    /// "Réglages du Bluetooth…".
    Settings,
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    fn id(self) -> String {
        format!("bluetooth:{self:?}")
    }
}

/// What it keeps between views and between opening and closing.
#[derive(Debug, Default)]
struct Ui {
    /// The devices around, by path, in the order shown.
    order: Vec<String>,
    /// The device whose attempt failed, and why.
    failed: Option<(String, Failure)>,
    /// The last attempt reacted to.
    seen: u64,
    focus: Option<Target>,
    /// The pointer is over the nearby list.
    over_list: bool,
    /// The adapter's switch, until BlueZ says it.
    switch: Switch,
    /// The nearby rows as last said in the log.
    said: Vec<Device>,
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

impl Ui {
    /// Whether the nearby rows hold their order.
    fn holding(&self, bt: &Bluetooth) -> bool {
        self.over_list || matches!(self.focus, Some(Target::Near(_))) || bt.running().is_some()
    }

    /// The nearby rows' order for `bt`: as it is while held (gone ones
    /// out, new ones last), else BlueZ's (the strongest first).
    fn relist(&mut self, bt: &Bluetooth) {
        if self.holding(bt) {
            self.order
                .retain(|p| bt.nearby.iter().any(|d| d.path == *p));
            for d in &bt.nearby {
                if !self.order.contains(&d.path) {
                    self.order.push(d.path.clone());
                }
            }
        } else {
            self.order = bt.nearby.iter().map(|d| d.path.clone()).collect();
        }
        // What a click on a row does follows these, not the thread's
        // word: the tests wait for them.
        let nearby = self.nearby(bt);
        if nearby != self.said {
            let rows: Vec<String> = nearby
                .iter()
                .map(|d| {
                    let code = if d.code { " (code)" } else { "" };
                    format!("{}{code}", d.name)
                })
                .collect();
            tracing::debug!(rows = %rows.join(", "), "the popover's nearby rows");
            self.said = nearby;
        }
    }

    /// The nearby rows shown, in order.
    fn nearby(&self, bt: &Bluetooth) -> Vec<Device> {
        self.order
            .iter()
            .filter_map(|p| bt.nearby.iter().find(|d| d.path == *p).cloned())
            .collect()
    }

    /// What the last attempt says, once.
    fn react(&mut self, bt: &Bluetooth) {
        let Some(attempt) = &bt.attempt else {
            return;
        };
        if attempt.id <= self.seen || matches!(attempt.step, Step::Running(_)) {
            return;
        }
        self.seen = attempt.id;
        self.failed = match attempt.step {
            // A code asked: the row says "in Settings", nothing failed.
            Step::Failed(Failure::Code | Failure::Stopped) | Step::Done | Step::Running(_) => None,
            Step::Failed(failure) => Some((attempt.path.clone(), failure)),
        };
    }

    /// The failure said under device `path`'s row, if one is.
    fn failure(&self, path: &str) -> Option<Failure> {
        self.failed
            .as_ref()
            .filter(|(p, _)| p == path)
            .map(|(_, f)| *f)
    }
}

/// The popover opens: nothing said yet; the search starts, and runs while
/// it is open.
pub fn opening() {
    let bt = bluetooth::snapshot();
    ui(|ui| {
        *ui = Ui {
            seen: bt.attempt.as_ref().map_or(0, |a| a.id),
            ..Ui::default()
        };
        ui.relist(&bt);
    });
    bluetooth::ask(Ask::Discover(true));
}

/// The popover closes: the search stops.
pub fn closing() {
    ui(|ui| {
        ui.focus = None;
        ui.over_list = false;
    });
    bluetooth::ask(Ask::Discover(false));
}

/// The popover's height for what it shows now, which the controller asks
/// before its view is built.
pub fn height() -> u32 {
    let bt = bluetooth::snapshot();
    ui(|ui| {
        ui.relist(&bt);
        Layout::of(&bt, ui, &ui.nearby(&bt)).height()
    })
}

// --- Its height ----------------------------------------------------------------------

/// A list's part: the divider, the label, the note or the rows.
#[derive(Debug, Clone, PartialEq)]
struct Section {
    note: Option<f32>,
    list: Option<List>,
}

impl Section {
    fn scrolls(&self) -> bool {
        self.list.is_some_and(List::scrolls)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Layout {
    /// Each connected device's block's height.
    blocks: Vec<f32>,
    mine: Option<Section>,
    nearby: Option<Section>,
    end: f32,
}

/// What a failure says under its row, in lines.
fn error_lines(failure: Failure) -> Vec<String> {
    let words = kit::words();
    let said = match failure {
        Failure::NoAnswer => words.no_answer(),
        Failure::Pairing | Failure::Code | Failure::Cancelled | Failure::Stopped => {
            words.pairing_failed()
        }
    };
    kit::wrapped(said, ERROR, ERROR_ROOM)
}

fn disconnect_width() -> f32 {
    kit::measure(kit::words().disconnect(), STATUS, MANROPE) + 2.0 * SMALL_PAD
}

/// Whether a connected device's state, "Connecté · batterie 82 %", wraps
/// beside Disconnect (as the mockup's text does at 280 px): then the
/// battery goes on a line of its own and the block grows. Measured once
/// each.
fn wraps(battery: Option<u8>) -> bool {
    static MEASURED: Mutex<Vec<(u8, bool)>> = Mutex::new(Vec::new());
    let Some(percent) = battery else {
        return false;
    };
    let mut known = MEASURED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((_, wraps)) = known.iter().find(|(p, _)| *p == percent) {
        return *wraps;
    }
    let words = kit::words();
    let text = format!("{} · {}", words.connected(), words.battery_of(percent));
    let wraps = kit::measure(&text, STATUS, MANROPE) > kit::block_room(disconnect_width());
    known.push((percent, wraps));
    wraps
}

/// A connected device's state's lines: two when its battery wraps.
fn state_lines(device: &Device) -> f32 {
    if wraps(device.battery) { 2.0 } else { 1.0 }
}

/// A row's height, with its failure's lines under it.
fn row_height(failure: Option<Failure>) -> f32 {
    match failure {
        Some(failure) => {
            ROW + error_lines(failure).len() as f32 * ERROR * ERROR_LINE + ERROR_BOTTOM
        }
        None => ROW,
    }
}

impl Layout {
    /// What shows of `bt`: as BlueZ says it, whatever the switch shows
    /// while it waits for BlueZ.
    fn of(bt: &Bluetooth, ui: &Ui, nearby: &[Device]) -> Self {
        let mut y = HEADER_TOP + HEADER;
        let mut next = |height: f32| {
            y += GAP;
            let top = y;
            y += height;
            top
        };
        let on = bt.powered && bt.adapter;
        if !on {
            next(NOTE_HEIGHT);
        }
        let blocks: Vec<f32> = if on {
            bt.connected
                .iter()
                .map(|d| {
                    let height = kit::block_height(state_lines(d));
                    next(height);
                    height
                })
                .collect()
        } else {
            Vec::new()
        };
        let section = |devices: &[Device], most: f32, next: &mut dyn FnMut(f32) -> f32| {
            next(2.0 * RULE + 1.0);
            next(LABEL_ROW);
            let (_, content) = kit::stack(devices.iter().map(|d| row_height(ui.failure(&d.path))));
            let note = devices.is_empty().then(|| next(NOTE_HEIGHT));
            let list = (!devices.is_empty()).then(|| {
                let height = content.min(most);
                let top = next(height);
                kit::list(top, height, content)
            });
            Section { note, list }
        };
        let mine = on.then(|| section(&bt.paired, f32::INFINITY, &mut next));
        let nearby = on.then(|| section(nearby, LIST, &mut next));
        Self {
            blocks,
            mine,
            nearby,
            end: y + RULE + 1.0 + RULE + LINK,
        }
    }

    fn height(&self) -> u32 {
        kit::height(self.end)
    }
}

// --- The view ------------------------------------------------------------------------

/// Bluetooth's popover on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    /// Its surface: it reacts only while open, not while it fades out.
    pub id: window::Id,
    pub words: Words,
    /// On the compositor's glass (else opaque).
    pub glass: bool,
    /// Its spinners turn and its switch slides.
    pub motion: bool,
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A control pressed, or `Enter` on it.
    Act(Target),
    /// The ring comes to it.
    Focus(Target),
    /// The pointer came over the nearby list, or left it.
    OverList(bool),
    /// A press anywhere on the popover.
    Pressed,
    /// Bluetooth changed under the view.
    Stale,
}

/// What waits for the tree built after a message: the list scrolled.
static LATER: Later = Later::new();

/// The popover on its surface.
pub fn view<'a>(panel: &Panel) -> Element<'a, crate::Message> {
    let bt = bluetooth::snapshot();
    let content = ui(|ui| content(panel, &bt, ui, Instant::now()));
    let motion = panel.motion;
    Around::new(panel.id, content, move |m| handle(m, motion), Msg::Stale)
        .press(Msg::Pressed)
        .later(&LATER)
        .on_glass(panel.glass)
}

fn content<'a>(panel: &Panel, bt: &Bluetooth, ui: &Ui, now: Instant) -> El<'a> {
    let words = panel.words;
    let on = bt.powered && bt.adapter;
    let nearby = ui.nearby(bt);
    let layout = Layout::of(bt, ui, &nearby);
    let running = bt.running().map(|(p, a)| (p.to_owned(), a));
    let focus = ui.focus;
    let block = |content: Option<El<'a>>| parts::block(GAP, content);
    let mut all: Vec<El<'a>> = vec![header(panel, bt, ui, now)];
    all.push(block((!on).then(|| note(words.bluetooth_is_off()))));
    // The connected blocks, each in its place.
    let mut connected = column![];
    for (i, device) in bt.connected.iter().enumerate().filter(|_| on) {
        connected = connected.push(block(Some(connected_block(words, device, i, focus))));
    }
    all.push(connected.into());
    let rows = |section: u8, devices: &[Device], scrolls: bool| -> El<'a> {
        let mut list = column![].spacing(ROW_GAP);
        for (i, device) in devices.iter().enumerate() {
            let target = if section == 0 {
                Target::Mine(i)
            } else {
                Target::Near(i)
            };
            let row = device_row(panel, ui, device, target, running.as_ref(), scrolls);
            list = list.push(container(row).id(Id::from(target.id() + ":row")));
        }
        // The rows reach the scrollbar's 2 px short, as the canvas's.
        container(list)
            .padding(Padding {
                right: if scrolls { 2.0 } else { 0.0 },
                ..Padding::ZERO
            })
            .into()
    };
    if let Some(mine) = &layout.mine {
        all.push(block(Some(parts::rule())));
        all.push(block(Some(parts::label(
            words.my_devices(),
            LABEL_ROW,
            None,
        ))));
        all.push(block(Some(if mine.note.is_some() {
            note(words.all_connected())
        } else {
            rows(0, &bt.paired, false)
        })));
    }
    if let Some(around) = &layout.nearby {
        let searching = bt.searching;
        let aside = searching.then(|| {
            let mut aside = row![].align_y(Vertical::Center);
            if panel.motion {
                aside = aside.push(spinner(FAINT.get())).push(gap(6.0));
            }
            aside
                .push(said(words.searching(), SMALL, MANROPE, FAINT.get()))
                .into()
        });
        all.push(block(Some(parts::rule())));
        all.push(block(Some(parts::label(words.nearby(), LABEL_ROW, aside))));
        all.push(block(Some(if around.note.is_some() {
            note(if searching {
                words.searching_devices()
            } else {
                words.no_new_device()
            })
        } else {
            parts::list(
                rows(1, &nearby, around.scrolls()),
                LIST_ID,
                LIST,
                Msg::OverList,
            )
        })));
    }
    let [rule, link] = parts::settings_link(
        words.bluetooth_settings(),
        &Target::Settings.id(),
        Msg::Act(Target::Settings),
        focus == Some(Target::Settings),
        Msg::Focus(Target::Settings),
    );
    all.push(rule);
    all.push(link);
    parts::content(all)
}

/// The title, and the switch.
fn header<'a>(panel: &Panel, bt: &Bluetooth, state: &Ui, now: Instant) -> El<'a> {
    let words = panel.words;
    let switch = bt.adapter.then(|| {
        let knob = Knob {
            knob: |now, motion| {
                let bt = bluetooth::snapshot();
                ui(|ui| {
                    let on = ui.switch.shown(bt.powered, now);
                    ui.switch.knob_at(on, now, motion)
                })
            },
            sliding: |now| ui(|ui| ui.switch.sliding(now)),
            motion: panel.motion,
        };
        control(
            knob.view(Msg::Act(Target::Switch)),
            Target::Switch.id(),
            Role::ToggleButton,
            words.bluetooth(),
        )
        .node(|n| n.checked(state.switch.shown(bt.powered, now)))
        .focus(
            state.focus == Some(Target::Switch),
            Msg::Focus(Target::Switch),
            SWITCH.height / 2.0,
        )
        .activate(Msg::Act(Target::Switch))
        .into()
    });
    parts::header(words.bluetooth(), switch)
}

/// A connected device: its kind, its name over its state and battery,
/// Disconnect.
fn connected_block<'a>(words: Words, device: &Device, i: usize, focus: Option<Target>) -> El<'a> {
    let name = kit::elide(&device.name, kit::block_room(disconnect_width()), |s| {
        kit::measure(s, NAME, MANROPE_SEMIBOLD)
    });
    let connected = said(words.connected(), STATUS, MANROPE, ACCENT.get());
    let state: El<'a> = match device.battery {
        None => connected.into(),
        Some(percent) => {
            // " · batterie 82 %" in the battery's colour, beside, or " ·"
            // beside and the battery under.
            let colour = if device.low() { PEACH } else { ACCENT }.get();
            let battery = words.battery_of(percent);
            if wraps(device.battery) {
                column![
                    row![connected, said(" ·", STATUS, MANROPE, colour)],
                    said(battery, STATUS, MANROPE, colour),
                ]
                .into()
            } else {
                row![
                    connected,
                    said(format!(" · {battery}"), STATUS, MANROPE, colour)
                ]
                .into()
            }
        }
    };
    let texts = column![
        said(name, NAME, MANROPE_SEMIBOLD, TEXT.get()),
        Space::new().height(1.0),
        state
    ]
    .width(iced::Length::Fill);
    let target = Target::Disconnect(i);
    let leave = control(
        parts::small_button(words.disconnect(), Msg::Act(target)),
        target.id(),
        Role::Button,
        format!("{} {}", words.disconnect(), device.name),
    )
    .focus(focus == Some(target), Msg::Focus(target), SMALL_RADIUS)
    .activate(Msg::Act(target));
    let kind_of = device.kind;
    let line = row![
        drawn(ICON, ICON, move |f, s| kind(
            f,
            Point::ORIGIN,
            s.width,
            kind_of,
            ACCENT.get()
        )),
        gap(ICON_GAP),
        texts,
        gap(ICON_GAP),
        leave,
    ]
    .align_y(Vertical::Center);
    let mut said = format!("{}, {}", device.name, words.connected());
    if let Some(percent) = device.battery {
        said = format!("{said}, {}", words.battery_of(percent));
    }
    ui::line(parts::connected(line), said).into()
}

/// One device's row, its failure under it.
fn device_row<'a>(
    panel: &Panel,
    ui: &Ui,
    device: &Device,
    target: Target,
    running: Option<&(String, Action)>,
    scrolls: bool,
) -> El<'a> {
    let words = panel.words;
    let idle = running.is_none();
    let aside = row_aside(words, device, running);
    let width = RIGHT - LEFT + 2.0 * ROW_OUT - if scrolls { SCROLLBAR + 2.0 } else { 0.0 };
    let room = kit::name_room(
        width,
        0.0,
        aside.map(|(text, _, spins)| (kit::measure(text, SMALL, MANROPE), spins && panel.motion)),
    );
    let name = kit::elide(&device.name, room, |s| kit::measure(s, ROW_TEXT, MANROPE));
    let kind_of = device.kind;
    let mut line = row![
        drawn(ICON, ICON, move |f, s| kind(
            f,
            Point::ORIGIN,
            s.width,
            kind_of,
            TEXT.get()
        )),
        gap(ICON_GAP),
        said(name, ROW_TEXT, MANROPE, TEXT.get()),
        space::horizontal(),
    ]
    .align_y(Vertical::Center);
    if let Some((text, colour, spins)) = aside {
        if spins && panel.motion {
            line = line.push(spinner(DIM.get())).push(gap(6.0));
        }
        line = line.push(said(text, SMALL, MANROPE, colour));
    }
    let pressed = parts::row_button(line, ROW, idle.then_some(Msg::Act(target)));
    let mut row = control(pressed, target.id(), Role::Button, device.name.clone())
        .node(|n| n.describe(state_of(words, ui, device, running)))
        .rows(0);
    if idle {
        row = row
            .focus(ui.focus == Some(target), Msg::Focus(target), ROW_RADIUS)
            .activate(Msg::Act(target));
    }
    match ui.failure(&device.path) {
        None => row.into(),
        Some(failure) => column![
            row,
            container(
                said(error_lines(failure).join("\n"), ERROR, MANROPE, PEACH.get())
                    .line_height(LineHeight::Relative(ERROR_LINE))
            )
            .padding(Padding {
                left: ERROR_INDENT,
                bottom: ERROR_BOTTOM,
                ..Padding::ZERO
            }),
        ]
        .into(),
    }
}

/// What a device's row says to a screen reader beside its name.
fn state_of(words: Words, ui: &Ui, device: &Device, running: Option<&(String, Action)>) -> String {
    match running {
        Some((path, action)) if *path == device.path => match action {
            Action::Connect => words.connecting().to_owned(),
            Action::Pair => words.pairing().to_owned(),
        },
        _ => match ui.failure(&device.path) {
            Some(failure) => error_lines(failure).join(" "),
            None if device.code && !device.paired => words.in_settings().to_owned(),
            None => String::new(),
        },
    }
}

/// What `device`'s row says on its right: "Connexion…" or "Association…"
/// while asked for, "dans les Réglages" when it asks a code; its colour,
/// and whether a spinner turns before it.
fn row_aside(
    words: Words,
    device: &Device,
    running: Option<&(String, Action)>,
) -> Option<(&'static str, Color, bool)> {
    match running {
        Some((path, action)) if *path == device.path => Some((
            match action {
                Action::Connect => words.connecting(),
                Action::Pair => words.pairing(),
            },
            DIM.get(),
            true,
        )),
        _ if device.code && !device.paired => Some((words.in_settings(), FAINT.get(), false)),
        _ => None,
    }
}

// --- What a message does -------------------------------------------------------

/// `message`, handled at once; then what BlueZ says, the rows' order, and
/// the list scrolled to the row the ring went to.
fn handle(message: Msg, motion: bool) {
    let bt = bluetooth::snapshot();
    let before = ui(|ui| ui.focus);
    match message {
        Msg::Stale => {}
        Msg::Act(target) => act(target, &bt, motion),
        Msg::Focus(target) => ui(|ui| ui.focus = Some(target)),
        Msg::OverList(over) => ui(|ui| ui.over_list = over),
        Msg::Pressed => ui(|ui| ui.focus = None),
    }
    let now = Instant::now();
    let reveal = ui(|ui| {
        ui.react(&bt);
        ui.relist(&bt);
        ui.switch.settle(bt.powered, now);
        match ui.focus {
            Some(Target::Near(i)) if ui.focus != before => Some(i),
            _ => None,
        }
    });
    if let Some(i) = reveal {
        LATER.push(ui::reveal(
            Id::new(LIST_ID),
            Id::from(Target::Near(i).id() + ":row"),
        ));
    }
    super::refit(Item::Bluetooth);
}

/// `target` pressed, or `Enter` on it.
fn act(target: Target, bt: &Bluetooth, motion: bool) {
    let busy = bt.running().is_some();
    match target {
        Target::Switch => {
            let on = ui(|ui| ui.switch.flip(bt.powered, Instant::now(), motion));
            bluetooth::ask(Ask::Power(on));
        }
        Target::Disconnect(i) => {
            if let Some(device) = bt.connected.get(i) {
                bluetooth::ask(Ask::Disconnect(device.path.clone()));
            }
        }
        Target::Mine(i) if !busy => {
            if let Some(device) = bt.paired.get(i) {
                ui(|ui| ui.failed = None);
                bluetooth::ask(Ask::Connect(device.path.clone()));
            }
        }
        Target::Near(i) if !busy => {
            let Some(device) = ui(|ui| ui.nearby(bt).get(i).cloned()) else {
                return;
            };
            ui(|ui| ui.failed = None);
            if device.code {
                // A code to show: the Settings pair it, on their page.
                super::session::open_settings_with(vec![
                    "bluetooth".to_owned(),
                    "--pair".to_owned(),
                    device.address.clone(),
                ]);
            } else {
                bluetooth::ask(Ask::Pair(device.path.clone()));
            }
        }
        Target::Settings => super::session::open_settings(Some("bluetooth")),
        Target::Mine(_) | Target::Near(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::model::Kind;

    fn device(name: &str, paired: bool, connected: bool) -> Device {
        Device {
            path: format!("/org/bluez/hci0/dev_{name}"),
            name: name.to_owned(),
            kind: Kind::Headphones,
            paired,
            connected,
            ..Device::default()
        }
    }

    fn bt() -> Bluetooth {
        Bluetooth {
            adapter: true,
            powered: true,
            connected: vec![device("sony", true, true)],
            paired: vec![device("mx", true, false), device("jbl", true, false)],
            nearby: (0..6)
                .map(|i| device(&format!("n{i}"), false, false))
                .collect(),
            ..Bluetooth::default()
        }
    }

    #[test]
    fn its_height_follows_what_it_shows() {
        let mut ui = Ui::default();
        let mut off = bt();
        off.powered = false;
        // Off: the header, its line, and the Settings' link (45 px).
        assert_eq!(Layout::of(&off, &ui, &[]).height(), 135);
        let on = bt();
        ui.relist(&on);
        let nearby = ui.nearby(&on);
        let layout = Layout::of(&on, &ui, &nearby);
        assert_eq!(layout.blocks.len(), 1);
        // Six around: four show, the list scrolls.
        let section = layout.nearby.clone().expect("nearby");
        assert_eq!(section.list.map(|l| l.view.height), Some(LIST));
        assert!(section.scrolls());
        assert!(!layout.mine.clone().expect("mine").scrolls());
        // A failure under a paired row grows the popover by its lines.
        ui.failed = Some((on.paired[0].path.clone(), Failure::NoAnswer));
        let failed = Layout::of(&on, &ui, &nearby);
        assert!(failed.height() > layout.height() + 20);
        // Nothing around: the note.
        let mut alone = bt();
        alone.nearby.clear();
        let mut ui = Ui::default();
        ui.relist(&alone);
        let layout = Layout::of(&alone, &ui, &[]);
        assert!(layout.nearby.and_then(|s| s.note).is_some());
    }

    #[test]
    fn nothing_can_be_asked_while_something_runs_but_the_settings_link() {
        use hocus_bluetooth::Attempt;
        let panel = Panel {
            id: window::Id::unique(),
            words: Words::new(crate::clock::time::Lang::French),
            glass: false,
            motion: false,
        };
        let mut bt = bt();
        bt.nearby[1].code = true;
        let mut ui = Ui::default();
        ui.relist(&bt);
        // What the ring goes to, and what `Enter` acts on, in order.
        let held = |bt: &Bluetooth, ui: &Ui| -> Vec<(String, bool)> {
            let content = content(&panel, bt, ui, Instant::now());
            ui::walk::declared(content, hocus_link::POPOVER_WIDTH as f32)
                .into_iter()
                .filter(|(_, d)| d.node.key.starts_with("bluetooth:"))
                .map(|(_, d)| {
                    (
                        d.node.key.clone(),
                        d.on_focus.is_some() && d.on_activate.is_some(),
                    )
                })
                .collect()
        };
        let idle = held(&bt, &ui);
        let ring: Vec<&str> = idle
            .iter()
            .filter(|(_, held)| *held)
            .map(|(id, _)| id.as_str())
            .collect();
        assert_eq!(
            ring[..5],
            [
                "bluetooth:Switch",
                "bluetooth:Disconnect(0)",
                "bluetooth:Mine(0)",
                "bluetooth:Mine(1)",
                "bluetooth:Near(0)"
            ]
        );
        // A device asking a code goes to the Settings: it can be chosen.
        assert!(ring.contains(&"bluetooth:Near(1)"));
        assert_eq!(ring.last(), Some(&"bluetooth:Settings"));
        // A connection under way: the rows are inert, still declared.
        bt.attempt = Some(Attempt {
            id: 1,
            path: bt.paired[0].path.clone(),
            step: Step::Running(Action::Connect),
        });
        let running = held(&bt, &ui);
        let ring: Vec<&str> = running
            .iter()
            .filter(|(_, held)| *held)
            .map(|(id, _)| id.as_str())
            .collect();
        assert_eq!(
            ring,
            [
                "bluetooth:Switch",
                "bluetooth:Disconnect(0)",
                "bluetooth:Settings"
            ]
        );
        assert!(
            running
                .iter()
                .any(|(id, held)| id == "bluetooth:Mine(0)" && !held)
        );
    }

    #[test]
    fn the_nearby_rows_hold_their_order_while_worked_in() {
        let mut ui = Ui::default();
        let mut bt = bt();
        ui.relist(&bt);
        let first = ui.order.clone();
        // The pointer over the list: a stronger newcomer joins the end.
        ui.over_list = true;
        bt.nearby.insert(0, device("new", false, false));
        ui.relist(&bt);
        assert_eq!(ui.order[..first.len()], first[..]);
        assert_eq!(
            ui.order.last().map(String::as_str),
            Some(bt.nearby[0].path.as_str())
        );
        // Away, BlueZ's order again.
        ui.over_list = false;
        ui.relist(&bt);
        assert_eq!(ui.order[0], bt.nearby[0].path);
    }
}
