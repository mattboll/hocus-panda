//! Bluetooth's popover (bar.md §9, the 3.6 mockup, variant A): its title
//! and its switch; each device connected in a block of its own (its
//! kind's icon, its name, "Connecté · batterie 82 %", Disconnect); "MES
//! APPAREILS", the paired devices not connected, one row each, a click
//! connecting one; "AUTOUR", the devices found around while the popover is
//! open (named ones only), the strongest first, a click pairing one, those
//! asking for a code greyed "dans les Réglages"; past four, that list
//! scrolls.
//!
//! A row asked for says "Connexion…" or "Association…" with a spinner
//! until BlueZ is done, the others inert meanwhile; a failure says so
//! under its row. The nearby rows hold their order while the pointer is
//! over them, one holds the keyboard, or something runs: rows do not move
//! under the pointer; newcomers join at the end.
//!
//! One canvas draws it, as the Wi-Fi's: what changes draws this surface
//! alone. The nearby list is drawn in a frame of its own, cut to its
//! bounds.

use std::{
    sync::{Mutex, PoisonError},
    time::Instant,
};

use iced::{
    Point, Rectangle, Renderer, Theme, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry},
        text::Alignment,
    },
    window,
};

use super::{
    Event as Seen, Item, Pressed, event,
    kit::{
        self, Aside, Clip, HEADER, HEADER_TOP, ICON, ICON_GAP, Input, LEFT, List, NOTE_HEIGHT,
        PEACH, Pointer, RIGHT, ROW, ROW_GAP, ROW_OUT, ROW_PAD, ROW_RADIUS, ROW_TEXT, SCROLLBAR,
        SMALL, SMALL_PAD, SMALL_RADIUS, STATUS, SWITCH, Spin, Switch, Wheel, Widths, text,
    },
};
use crate::{
    bluetooth::{
        self, Ask, Bluetooth,
        draw::kind,
        model::{Action, Device, Failure, Step},
    },
    levels::{
        draw::{ACCENT, DIM, FAINT, LINE, MANROPE, TEXT, ring},
        model::Words,
    },
};

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
/// A notch of a mouse's wheel scrolls two rows.
const NOTCH: f32 = 2.0 * (ROW + ROW_GAP);

// --- What the popover keeps while it is open -----------------------------------

/// What can be pointed at, or hold the keyboard's focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Switch,
    /// A connected device's Disconnect, by its place.
    Disconnect(usize),
    /// A paired device's row.
    Mine(usize),
    /// A device around's row.
    Near(usize),
}

/// What it keeps between draws and between opening and closing.
#[derive(Debug, Default)]
struct Ui {
    /// The devices around, by path, in the order shown.
    order: Vec<String>,
    /// The device whose attempt failed, and why.
    failed: Option<(String, Failure)>,
    /// The last attempt reacted to.
    seen: u64,
    focus: Option<Target>,
    /// How far the nearby list is scrolled, px.
    scroll: f32,
    /// The pointer is over the nearby list.
    over_list: bool,
    /// The adapter's switch, until BlueZ says it.
    switch: Switch,
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
            Step::Failed(Failure::Code) | Step::Done | Step::Running(_) => None,
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

/// The popover's height for what it shows now.
pub fn height() -> u32 {
    let bt = bluetooth::snapshot();
    ui(|ui| {
        ui.relist(&bt);
        Layout::of(&bt, ui, &ui.nearby(&bt)).height()
    })
}

// --- Where everything is ---------------------------------------------------------

/// A list's part: the divider, the label, the note or the rows.
#[derive(Debug, Clone, PartialEq)]
struct Section {
    rule: f32,
    label: f32,
    note: Option<f32>,
    list: Option<List>,
    /// Each row's top and height (with its failure), in the list.
    rows: Vec<(f32, f32)>,
}

impl Section {
    fn scrolls(&self) -> bool {
        self.list.is_some_and(List::scrolls)
    }

    fn most_scroll(&self) -> f32 {
        self.list.map_or(0.0, List::most_scroll)
    }

    /// Row `i`'s 36 px, on the popover, scrolled by `scroll`.
    fn row(&self, i: usize, scroll: f32) -> Option<Rectangle> {
        let (top, _) = *self.rows.get(i)?;
        Some(self.list?.row(top, scroll, 0.0))
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Layout {
    switch: bool,
    /// "Le Bluetooth est coupé."
    note: Option<f32>,
    /// Each connected device's block's top and height.
    blocks: Vec<(f32, f32)>,
    mine: Option<Section>,
    nearby: Option<Section>,
    end: f32,
}

/// What a failure says under its row, in lines.
fn error_lines(failure: Failure) -> Vec<String> {
    let words = kit::words();
    let said = match failure {
        Failure::NoAnswer => words.no_answer(),
        Failure::Pairing | Failure::Code => words.pairing_failed(),
    };
    kit::wrapped(said, ERROR, ERROR_ROOM)
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
    let disconnect = kit::measure(words.disconnect(), STATUS, MANROPE) + 2.0 * SMALL_PAD;
    let wraps = kit::measure(&text, STATUS, MANROPE) > kit::block_room(disconnect);
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
        let note = (!on).then(|| next(NOTE_HEIGHT));
        let blocks: Vec<(f32, f32)> = if on {
            bt.connected
                .iter()
                .map(|d| {
                    let height = kit::block_height(state_lines(d));
                    (next(height), height)
                })
                .collect()
        } else {
            Vec::new()
        };
        let section = |devices: &[Device], most: f32, next: &mut dyn FnMut(f32) -> f32| {
            let rule = next(2.0 * RULE + 1.0) + RULE;
            let label = next(LABEL_ROW);
            let (rows, content) =
                kit::stack(devices.iter().map(|d| row_height(ui.failure(&d.path))));
            let note = devices.is_empty().then(|| next(NOTE_HEIGHT));
            let list = (!devices.is_empty()).then(|| {
                let height = content.min(most);
                let top = next(height);
                kit::list(top, height, content)
            });
            Section {
                rule,
                label,
                note,
                list,
                rows,
            }
        };
        let mine = on.then(|| section(&bt.paired, f32::INFINITY, &mut next));
        let nearby = on.then(|| section(nearby, LIST, &mut next));
        Self {
            switch: bt.adapter,
            note,
            blocks,
            mine,
            nearby,
            end: y,
        }
    }

    fn height(&self) -> u32 {
        kit::height(self.end)
    }

    fn list(&self) -> Option<List> {
        self.nearby.as_ref().and_then(|s| s.list)
    }

    fn most_scroll(&self) -> f32 {
        self.nearby.as_ref().map_or(0.0, Section::most_scroll)
    }
}

// --- The canvas ------------------------------------------------------------------

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
    /// The shell draws on the CPU.
    pub software: bool,
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    bt: Bluetooth,
    nearby: Vec<Device>,
    layout: Option<Layout>,
    widths: Widths,
    /// The names as shown: connected, paired, around.
    shown: [Vec<String>; 3],
    /// The labels' letters and their places.
    labels: [Vec<(char, f32)>; 2],
    pointer: Pointer<Target>,
    spin: Spin,
    wheel: Wheel,
    now: Option<Instant>,
}

impl<Message> canvas::Program<Message> for Panel {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match kit::input(self.id, event, bounds, cursor, &mut state.pointer) {
            Input::Redraw(now) => self.refresh(state, now),
            Input::Mouse(event, at) => self.mouse(state, event, at),
            Input::Key(pressed) => self.key(state, &pressed),
            Input::Nothing => None,
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        kit::background(&mut frame, bounds.size(), self.glass);
        let Some(layout) = &state.layout else {
            return vec![frame.into_geometry()];
        };
        let now = state.now.unwrap_or_else(Instant::now);
        ui(|ui| {
            let drawn = Drawn {
                panel: self,
                state,
                ui,
                layout,
                now,
            };
            drawn.header(&mut frame);
            drawn.blocks(&mut frame);
            drawn.mine(&mut frame);
            drawn.nearby_head(&mut frame);
            let mut all = Vec::new();
            if let Some(list) = layout.list() {
                let mut frame_of_list = Frame::with_bounds(renderer, list.view);
                drawn.nearby(&mut frame_of_list);
                list.scrollbar(&mut frame, ui.scroll);
                all.push(frame_of_list.into_geometry());
            }
            drawn.focus(&mut frame);
            all.insert(0, frame.into_geometry());
            all
        })
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        state.pointer.interaction(false)
    }
}

impl Panel {
    /// What it shows, read afresh: the keys that reached another surface,
    /// Bluetooth, the nearby rows' order, the outcome of an attempt.
    fn refresh<Message>(&self, state: &mut State, now: Instant) -> Option<canvas::Action<Message>> {
        for pressed in kit::keys(self.id) {
            let _: Option<canvas::Action<Message>> = self.key(state, &pressed);
        }
        let bt = bluetooth::snapshot();
        ui(|ui| {
            ui.react(&bt);
            ui.relist(&bt);
            ui.switch.settle(bt.powered, now);
            state.nearby = ui.nearby(&bt);
            let layout = Layout::of(&bt, ui, &state.nearby);
            ui.scroll = ui.scroll.clamp(0.0, layout.most_scroll());
            if ui
                .focus
                .is_some_and(|f| !targets(&layout, &bt, &state.nearby).contains(&f))
            {
                ui.focus = None;
            }
            // Resized under a still pointer: what it is over is not known
            // until it moves.
            if state.layout.as_ref().map(Layout::height) != Some(layout.height()) {
                state.pointer.hover = None;
            }
            state.layout = Some(layout);
        });
        state.bt = bt;
        state.now = Some(now);
        self.measure_all(state);
        // A change of size reaches the controller, which resizes the
        // surface (without waking it again).
        super::refit(Item::Bluetooth);
        let spins = self.motion && (state.bt.searching || state.bt.running().is_some());
        let spinning = state.spin.run(spins, now);
        let sliding = self.motion && ui(|ui| ui.switch.sliding(now));
        (spinning || sliding).then(canvas::Action::request_redraw)
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let layout = state.layout.clone()?;
        let target = at.and_then(|at| self.target_at(state, &layout, at));
        let in_list = at.is_some_and(|at| layout.list().is_some_and(|l| l.view.contains(at)));
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        match seen {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                state.pointer.moved();
                let held = ui(|ui| std::mem::replace(&mut ui.over_list, in_list));
                if state.pointer.over(target) || held != in_list {
                    return redraw;
                }
                None
            }
            mouse::Event::CursorLeft => {
                state.pointer.left();
                ui(|ui| ui.over_list = false);
                redraw
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                state.pointer.pressed = target;
                ui(|ui| ui.focus = None);
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if let Some(clicked) = state.pointer.release(target) {
                    self.act(state, Some(clicked));
                }
                redraw
            }
            mouse::Event::WheelScrolled { delta } if in_list => {
                let by = state.wheel.down(delta, NOTCH);
                let most = layout.most_scroll();
                ui(|ui| ui.scroll = (ui.scroll + by).clamp(0.0, most));
                state.pointer.hover = at.and_then(|at| self.target_at(state, &layout, at));
                redraw
            }
            _ => None,
        }
    }

    /// What is under `at`: only what can be acted on.
    fn target_at(&self, state: &mut State, layout: &Layout, at: Point) -> Option<Target> {
        let scroll = ui(|ui| ui.scroll);
        let disconnect = self.disconnect_width(state);
        let list = layout.list();
        targets(layout, &state.bt, &state.nearby)
            .into_iter()
            .find(|t| {
                let Some(rect) = shape(*t, layout, scroll, disconnect) else {
                    return false;
                };
                let in_list = matches!(t, Target::Near(_));
                rect.contains(at) && (!in_list || list.is_some_and(|l| l.view.contains(at)))
            })
    }

    fn disconnect_width(&self, state: &mut State) -> f32 {
        state.widths.width(self.words.disconnect(), STATUS, false) + 2.0 * SMALL_PAD
    }

    /// `target` pressed, or `Enter` on it.
    fn act(&self, state: &mut State, target: Option<Target>) {
        let bt = state.bt.clone();
        let busy = bt.running().is_some();
        match target {
            Some(Target::Switch) => {
                let on = ui(|ui| ui.switch.flip(bt.powered, Instant::now(), self.motion));
                bluetooth::ask(Ask::Power(on));
            }
            Some(Target::Disconnect(i)) => {
                if let Some(device) = bt.connected.get(i) {
                    bluetooth::ask(Ask::Disconnect(device.path.clone()));
                }
            }
            Some(Target::Mine(i)) if !busy => {
                if let Some(device) = bt.paired.get(i) {
                    ui(|ui| ui.failed = None);
                    bluetooth::ask(Ask::Connect(device.path.clone()));
                }
            }
            Some(Target::Near(i)) if !busy => {
                if let Some(device) = state.nearby.get(i).filter(|d| !d.code) {
                    ui(|ui| ui.failed = None);
                    bluetooth::ask(Ask::Pair(device.path.clone()));
                }
            }
            _ => {}
        }
    }

    /// The layout as the keys so far left it, not as last drawn: a failure
    /// cleared in the same frame already changes the rows' heights (#462).
    fn layout_now(state: &State) -> Layout {
        ui(|ui| Layout::of(&state.bt, ui, &state.nearby))
    }

    fn key<Message>(
        &self,
        state: &mut State,
        pressed: &Pressed,
    ) -> Option<canvas::Action<Message>> {
        use keyboard::{Key, key::Named};
        state.layout.as_ref()?;
        let order = targets(&Self::layout_now(state), &state.bt, &state.nearby);
        let focus = ui(|ui| ui.focus);
        match &pressed.key {
            Key::Named(Named::Escape) => event(Seen::Escape),
            Key::Named(Named::Tab) => {
                let next = kit::step(&order, focus, pressed.modifiers.shift());
                ui(|ui| ui.focus = next);
            }
            Key::Named(Named::ArrowUp | Named::ArrowDown) => {
                let rows: Vec<Target> = order
                    .iter()
                    .copied()
                    .filter(|t| matches!(t, Target::Mine(_) | Target::Near(_)))
                    .collect();
                let down = matches!(pressed.key, Key::Named(Named::ArrowDown));
                let next = kit::cycle(&rows, focus?, down)?;
                ui(|ui| ui.focus = Some(next));
            }
            Key::Named(Named::Enter | Named::Space) => self.act(state, focus),
            _ => return None,
        }
        self.reveal(state);
        Some(canvas::Action::request_redraw().and_capture())
    }

    /// The nearby list scrolled so that the row holding the keyboard shows,
    /// as the rows are now.
    fn reveal(&self, state: &State) {
        let Some(Target::Near(i)) = ui(|ui| ui.focus) else {
            return;
        };
        let layout = Self::layout_now(state);
        let Some(section) = &layout.nearby else {
            return;
        };
        let (Some(list), Some(&row)) = (section.list, section.rows.get(i)) else {
            return;
        };
        ui(|ui| ui.scroll = kit::reveal(row, list.view.height, ui.scroll, list.most_scroll()));
    }

    /// Measures, ahead of drawing, what the draw needs: the words once,
    /// the labels' letters, the names cut to their room.
    fn measure_all(&self, state: &mut State) {
        let words = self.words;
        for (text, size, bold) in [
            (words.disconnect(), STATUS, false),
            (words.connected(), STATUS, false),
            (words.connecting(), SMALL, false),
            (words.pairing(), SMALL, false),
            (words.searching(), SMALL, false),
            (words.in_settings(), SMALL, false),
        ] {
            state.widths.width(text, size, bold);
        }
        for (i, label) in [words.my_devices(), words.nearby()].into_iter().enumerate() {
            if state.labels[i].is_empty() {
                state.labels[i] = kit::label_letters(label);
            }
        }
        let disconnect = state.widths.known(words.disconnect(), STATUS, false) + 2.0 * SMALL_PAD;
        let block_room = kit::block_room(disconnect);
        let connected: Vec<String> = state.bt.connected.iter().map(|d| d.name.clone()).collect();
        state.shown[0] = connected
            .iter()
            .map(|n| state.widths.fit(n, block_room, true))
            .collect();
        let running = state.bt.running().map(|(p, a)| (p.to_owned(), a));
        let row_room = |state: &State, device: &Device, scrolls: bool| {
            let width = RIGHT - LEFT + 2.0 * ROW_OUT - if scrolls { SCROLLBAR + 2.0 } else { 0.0 };
            let aside = row_aside(words, device, running.as_ref()).map(|aside| {
                let width = state.widths.known(aside.text, SMALL, false);
                (width, aside.spinner && self.motion)
            });
            kit::name_room(width, 0.0, aside)
        };
        let paired = state.bt.paired.clone();
        state.shown[1] = paired
            .iter()
            .map(|d| {
                let room = row_room(state, d, false);
                state.widths.fit(&d.name, room, false)
            })
            .collect();
        let scrolls = state
            .layout
            .as_ref()
            .and_then(|l| l.nearby.as_ref())
            .is_some_and(Section::scrolls);
        let nearby = state.nearby.clone();
        state.shown[2] = nearby
            .iter()
            .map(|d| {
                let room = row_room(state, d, scrolls);
                state.widths.fit(&d.name, room, false)
            })
            .collect();
    }
}

/// Where `target` is.
fn shape(target: Target, layout: &Layout, scroll: f32, disconnect: f32) -> Option<Rectangle> {
    Some(match target {
        Target::Switch => Switch::rect(),
        Target::Disconnect(i) => {
            let (top, height) = *layout.blocks.get(i)?;
            kit::block_button(top, height, disconnect)
        }
        Target::Mine(i) => layout.mine.as_ref()?.row(i, 0.0)?,
        Target::Near(i) => layout.nearby.as_ref()?.row(i, scroll)?,
    })
}

/// What can hold the keyboard, in `Tab`'s order: the switch, each
/// Disconnect, then the rows that can be acted on (none while something
/// runs; not those asking a code).
fn targets(layout: &Layout, bt: &Bluetooth, nearby: &[Device]) -> Vec<Target> {
    let mut order = Vec::new();
    if layout.switch {
        order.push(Target::Switch);
    }
    order.extend((0..layout.blocks.len()).map(Target::Disconnect));
    if bt.running().is_some() {
        return order;
    }
    if layout.mine.as_ref().is_some_and(|s| s.list.is_some()) {
        order.extend((0..bt.paired.len()).map(Target::Mine));
    }
    if layout.list().is_some() {
        order.extend(
            nearby
                .iter()
                .enumerate()
                .filter(|(_, d)| !d.code)
                .map(|(i, _)| Target::Near(i)),
        );
    }
    order
}

/// What `device`'s row says on its right: "Connexion…" or "Association…"
/// while asked for, "dans les Réglages" when it asks a code.
fn row_aside(words: Words, device: &Device, running: Option<&(String, Action)>) -> Option<Aside> {
    match running {
        Some((path, action)) if *path == device.path => Some(Aside {
            text: match action {
                Action::Connect => words.connecting(),
                Action::Pair => words.pairing(),
            },
            colour: DIM,
            spinner: true,
        }),
        _ if device.code && !device.paired => Some(Aside {
            text: words.in_settings(),
            colour: FAINT,
            spinner: false,
        }),
        _ => None,
    }
}

// --- Drawing ---------------------------------------------------------------------

struct Drawn<'a> {
    panel: &'a Panel,
    state: &'a State,
    ui: &'a Ui,
    layout: &'a Layout,
    now: Instant,
}

impl Drawn<'_> {
    fn words(&self) -> Words {
        self.panel.words
    }

    fn hover(&self, target: Target) -> bool {
        self.state.pointer.hover == Some(target)
    }

    fn width(&self, text: &'static str, size: f32) -> f32 {
        self.state.widths.known(text, size, false)
    }

    fn shape(&self, target: Target) -> Option<Rectangle> {
        let disconnect = self.width(self.words().disconnect(), STATUS) + 2.0 * SMALL_PAD;
        shape(target, self.layout, self.ui.scroll, disconnect)
    }

    fn running(&self) -> Option<(String, Action)> {
        self.state.bt.running().map(|(p, a)| (p.to_owned(), a))
    }

    /// The spinners' turn now, if they turn.
    fn turn(&self) -> Option<f32> {
        self.panel.motion.then(|| self.state.spin.turn(self.now))
    }

    fn header(&self, f: &mut Frame) {
        kit::title(f, self.words().bluetooth());
        if !self.layout.switch {
            return;
        }
        let on = self.ui.switch.shown(self.state.bt.powered, self.now);
        Switch::draw(f, self.ui.switch.knob_at(on, self.now, self.panel.motion));
    }

    /// "Le Bluetooth est coupé.", and the connected devices' blocks.
    fn blocks(&self, f: &mut Frame) {
        let words = self.words();
        if let Some(top) = self.layout.note {
            f.fill_text(kit::note(words.bluetooth_is_off().to_owned(), top));
        }
        for (i, ((top, height), device)) in self
            .layout
            .blocks
            .iter()
            .zip(&self.state.bt.connected)
            .enumerate()
        {
            let middle = kit::block(f, *top, *height).center_y();
            kind(
                f,
                Point::new(LEFT, middle - ICON / 2.0),
                ICON,
                device.kind,
                ACCENT,
            );
            if let Some(button) = self.shape(Target::Disconnect(i)) {
                let hover = self.hover(Target::Disconnect(i));
                kit::small_button(f, button, words.disconnect(), hover);
            }
            // Name and state: 14 px over 12.5 px (a line or two), 1 px
            // apart, centred.
            let x = LEFT + ICON + ICON_GAP;
            let lines = state_lines(device);
            let name = self.state.shown[0].get(i).cloned().unwrap_or_default();
            let status = kit::block_name(f, name, middle, lines);
            f.fill_text(text(
                words.connected().to_owned(),
                Point::new(x, status),
                STATUS,
                ACCENT,
                Alignment::Left,
            ));
            if let Some(percent) = device.battery {
                // " · batterie 82 %" in the battery's colour, beside, or
                // " ·" beside and the battery under.
                let colour = if device.low() { PEACH } else { ACCENT };
                let after = x + self.width(words.connected(), STATUS);
                let battery = words.battery_of(percent);
                let (beside, under) = if lines > 1.0 {
                    (" ·".to_owned(), Some(battery))
                } else {
                    (format!(" · {battery}"), None)
                };
                f.fill_text(text(
                    beside,
                    Point::new(after, status),
                    STATUS,
                    colour,
                    Alignment::Left,
                ));
                if let Some(under) = under {
                    f.fill_text(text(
                        under,
                        Point::new(x, status + STATUS * LINE),
                        STATUS,
                        colour,
                        Alignment::Left,
                    ));
                }
            }
        }
    }

    /// A section's divider and label (its letters spaced), and what is
    /// said in place of its rows.
    fn section(&self, f: &mut Frame, section: &Section, label: usize, note: &'static str) {
        kit::rule(f, section.rule);
        kit::label(
            f,
            &self.state.labels[label],
            section.label + LABEL_ROW / 2.0,
        );
        if let Some(top) = section.note {
            f.fill_text(kit::note(note.to_owned(), top));
        }
    }

    /// "MES APPAREILS" and the paired devices.
    fn mine(&self, f: &mut Frame) {
        let Some(section) = &self.layout.mine else {
            return;
        };
        self.section(f, section, 0, self.words().all_connected());
        let running = self.running();
        for (i, device) in self.state.bt.paired.iter().enumerate() {
            let Some(rect) = section.row(i, 0.0) else {
                continue;
            };
            let name = self.state.shown[1].get(i).cloned().unwrap_or_default();
            let row = Row {
                rect,
                device,
                name,
                target: Target::Mine(i),
            };
            self.row(f, row, running.as_ref(), Clip::new(false, None));
        }
    }

    /// What it found around, in the list's own frame.
    fn nearby(&self, list: &mut Frame) {
        let Some(section) = &self.layout.nearby else {
            return;
        };
        let running = self.running();
        let clip = Clip::new(self.panel.software, section.list.map(|l| l.view));
        for (i, device) in self.state.nearby.iter().enumerate() {
            let Some(rect) = section.row(i, self.ui.scroll) else {
                continue;
            };
            let name = self.state.shown[2].get(i).cloned().unwrap_or_default();
            let row = Row {
                rect,
                device,
                name,
                target: Target::Near(i),
            };
            self.row(list, row, running.as_ref(), clip);
        }
    }

    /// One device's row, its failure under it.
    fn row(&self, f: &mut Frame, row: Row<'_>, running: Option<&(String, Action)>, clip: Clip) {
        let Row {
            rect,
            device,
            name,
            target,
        } = row;
        let enabled = running.is_none() && !(device.code && !device.paired);
        kit::row_fill(f, rect, false, enabled && self.hover(target));
        let colour = if device.code && !device.paired {
            FAINT
        } else {
            TEXT
        };
        let middle = rect.center_y();
        let icon = Point::new(rect.x + ROW_PAD, middle - ICON / 2.0);
        kind(f, icon, ICON, device.kind, colour);
        if let Some(aside) = row_aside(self.words(), device, running) {
            let right = rect.x + rect.width - ROW_PAD;
            let width = self.width(aside.text, SMALL);
            kit::aside(f, clip, aside, (right, middle), width, self.turn());
        }
        clip.fill(
            f,
            text(
                name,
                Point::new(icon.x + ICON + ICON_GAP, middle),
                ROW_TEXT,
                colour,
                Alignment::Left,
            ),
        );
        if let Some(failure) = self.ui.failure(&device.path) {
            let at = (rect.x + ERROR_INDENT, rect.y + ROW);
            for said in kit::lines(&error_lines(failure), at, ERROR, ERROR_LINE, PEACH) {
                clip.fill(f, said);
            }
        }
    }

    /// "AUTOUR", its divider, its search, and its note.
    fn nearby_head(&self, f: &mut Frame) {
        let words = self.words();
        if let Some(section) = &self.layout.nearby {
            let searching = self.state.bt.searching;
            let note = if searching {
                words.searching_devices()
            } else {
                words.no_new_device()
            };
            self.section(f, section, 1, note);
            if searching {
                let middle = section.label + LABEL_ROW / 2.0;
                let label = words.searching();
                let width = self.width(label, SMALL);
                kit::searching(f, label, width, middle, self.turn());
            }
        }
    }

    /// The keyboard's ring, around what holds it.
    fn focus(&self, f: &mut Frame) {
        let Some(target) = self.ui.focus else {
            return;
        };
        let Some(rect) = self.shape(target) else {
            return;
        };
        let radius = match target {
            Target::Switch => SWITCH.height / 2.0,
            Target::Disconnect(_) => SMALL_RADIUS,
            Target::Mine(_) | Target::Near(_) => ROW_RADIUS,
        };
        ring(f, rect, radius);
    }
}

/// A device's row to draw: where, which, its name as it fits, and what it
/// is to the pointer and the keyboard.
struct Row<'a> {
    rect: Rectangle,
    device: &'a Device,
    name: String,
    target: Target,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::model::{Attempt, Kind};

    fn device(name: &str, paired: bool, connected: bool) -> Device {
        Device {
            path: format!("/org/bluez/hci0/dev_{name}"),
            name: name.to_owned(),
            kind: Kind::Headphones,
            paired,
            connected,
            battery: None,
            code: false,
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
        // Off: the header and its line.
        assert_eq!(Layout::of(&off, &ui, &[]).height(), 90);
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

    #[test]
    fn nothing_can_be_asked_while_something_runs_nor_a_device_asking_a_code() {
        let mut bt = bt();
        bt.nearby[1].code = true;
        let ui = Ui::default();
        let layout = Layout::of(&bt, &ui, &bt.nearby);
        let order = targets(&layout, &bt, &bt.nearby);
        assert_eq!(
            order[..5],
            [
                Target::Switch,
                Target::Disconnect(0),
                Target::Mine(0),
                Target::Mine(1),
                Target::Near(0)
            ]
        );
        assert!(!order.contains(&Target::Near(1)));
        bt.attempt = Some(Attempt {
            id: 1,
            path: bt.paired[0].path.clone(),
            step: Step::Running(Action::Connect),
        });
        let order = targets(&layout, &bt, &bt.nearby);
        assert_eq!(order, [Target::Switch, Target::Disconnect(0)]);
    }
}
