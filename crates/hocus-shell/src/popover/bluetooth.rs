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
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::POPOVER_WIDTH;
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path},
        text::Alignment,
    },
    window,
};

use super::{
    Event as Seen, Item, Pressed, event,
    wifi::{MANROPE_SEMIBOLD, background, measure, mix, spinner, tenths, text, wrap},
};
use crate::{
    bluetooth::{
        self, Ask, Bluetooth,
        draw::kind,
        model::{Action, Device, Failure, Step},
    },
    levels::{
        draw::{ACCENT, DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb, ring},
        model::Words,
    },
};

// --- The mockup's sizes, at ×1 ------------------------------------------------

const BORDER: f32 = 1.0;
const PAD_TOP: f32 = 12.0;
const PAD_SIDE: f32 = 14.0;
const PAD_BOTTOM: f32 = 14.0;
const GAP: f32 = 6.0;
const LEFT: f32 = BORDER + PAD_SIDE;
const RIGHT: f32 = POPOVER_WIDTH as f32 - BORDER - PAD_SIDE;
const HEADER: f32 = 32.0;
const HEADER_TOP: f32 = BORDER + PAD_TOP;
const TITLE: f32 = 16.0;
const SWITCH: Size = Size::new(40.0, 22.0);
const KNOB: f32 = 18.0;
const NOTE: f32 = 13.5;
const NOTE_TOP: f32 = 4.0;
const NOTE_BOTTOM: f32 = 2.0;
/// A connected device's block, at least.
const BLOCK: f32 = 48.0;
const NAME: f32 = 14.0;
const STATUS: f32 = 12.5;
const SMALL_BUTTON: f32 = 28.0;
const SMALL_PAD: f32 = 9.0;
/// The divider, 1 px with 4 px around it.
const RULE: f32 = 4.0;
const LABEL: f32 = 11.5;
const LABEL_SPACING: f32 = 0.08 * LABEL;
const SMALL: f32 = 12.0;
/// A label's line: its padding and the "Recherche…" beside it.
const LABEL_ROW: f32 = 2.0 + SMALL * LINE + 2.0;
const ROW: f32 = 36.0;
const ROW_GAP: f32 = 2.0;
const ROW_OUT: f32 = 6.0;
const ROW_PAD: f32 = 6.0;
const ROW_TEXT: f32 = 14.0;
const ICON: f32 = 18.0;
const ICON_GAP: f32 = 10.0;
const SPINNER: f32 = 12.0;
/// The nearby list's height at most: four rows.
const LIST: f32 = 152.0;
const SCROLLBAR: f32 = 6.0;
/// A failure under its row: 12.5 px on 1.4 lines, 34 px in, 6 px under.
const ERROR: f32 = 12.5;
const ERROR_LINE: f32 = 1.4;
const ERROR_INDENT: f32 = 34.0;
const ERROR_BOTTOM: f32 = 6.0;
/// The room it wraps in: the row's width less its indent and padding.
const ERROR_ROOM: f32 = RIGHT - LEFT + 2.0 * ROW_OUT - ERROR_INDENT - ROW_PAD;
/// A notch of a mouse's wheel scrolls two rows.
const NOTCH: f32 = 2.0 * (ROW + ROW_GAP);

const FILL: Color = Color::from_rgba8(255, 255, 255, 0.06);
const FILL_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.08);
const ROW_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.05);
const RULE_COLOUR: Color = Color::from_rgba8(255, 255, 255, 0.08);
const SWITCH_OFF: Color = Color::from_rgba8(255, 255, 255, 0.18);
const KNOB_SHADOW: Color = Color::from_rgba8(0, 0, 0, 0.4);
const BLOCK_FILL: Color = Color::from_rgba8(108, 194, 135, 0.10);
const PEACH: Color = rgb(0xF7B98A);
const THUMB: Color = Color::from_rgba8(255, 255, 255, 0.16);

/// The switch's knob slides over this; the spinners turn once in the
/// other.
const SLIDE: Duration = Duration::from_millis(160);
const TURN: Duration = Duration::from_millis(900);
/// The switch shows what was asked this long at most, until BlueZ says it.
const ASKED_FOR: Duration = Duration::from_secs(3);

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
    /// The switch asked, and when.
    switch: Option<(bool, Instant)>,
    /// The knob's slide: from where (0 off, 1 on), since when.
    knob: Option<(f32, Instant)>,
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

    /// The switch as shown: as asked, until BlueZ says it.
    fn powered(&self, bt: &Bluetooth, now: Instant) -> bool {
        match self.switch {
            Some((asked, since)) if now < since + ASKED_FOR => asked,
            _ => bt.powered,
        }
    }

    /// The knob's place, 0 (off) to 1 (on), at `now`.
    fn knob_at(&self, on: bool, now: Instant, motion: bool) -> f32 {
        let to = if on { 1.0 } else { 0.0 };
        match self.knob {
            Some((from, since)) if motion && now < since + SLIDE => {
                let t = now.duration_since(since).as_secs_f32() / SLIDE.as_secs_f32();
                from + (to - from) * crate::clock::motion::bezier((0.0, 0.0, 0.58, 1.0), t)
            }
            _ => to,
        }
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
    /// The rows' viewport, and their content's height.
    list: Option<(Rectangle, f32)>,
    /// Each row's top and height (with its failure), in the content.
    rows: Vec<(f32, f32)>,
}

impl Section {
    fn scrolls(&self) -> bool {
        self.list
            .is_some_and(|(view, content)| content > view.height + 0.5)
    }

    fn most_scroll(&self) -> f32 {
        self.list
            .map_or(0.0, |(view, content)| (content - view.height).max(0.0))
    }

    /// Row `i`'s 36 px, on the popover, scrolled by `scroll`.
    fn row(&self, i: usize, scroll: f32) -> Option<Rectangle> {
        let (view, _) = self.list?;
        let (top, _) = *self.rows.get(i)?;
        let right = if self.scrolls() {
            RIGHT - 2.0
        } else {
            view.x + view.width
        };
        Some(Rectangle::new(
            Point::new(view.x, view.y + top - scroll),
            Size::new(right - view.x, ROW),
        ))
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

fn note_height() -> f32 {
    NOTE_TOP + NOTE * LINE + NOTE_BOTTOM
}

/// The words of the shell's language, for what the layout measures.
fn words() -> Words {
    Words::new(crate::clock::time::Lang::of_messages())
}

/// What a failure says under its row, in lines; measured once each.
fn error_lines(failure: Failure) -> Vec<String> {
    static WRAPPED: Mutex<Vec<(&'static str, Vec<String>)>> = Mutex::new(Vec::new());
    let words = words();
    let said = match failure {
        Failure::NoAnswer => words.no_answer(),
        Failure::Pairing | Failure::Code => words.pairing_failed(),
    };
    let mut known = WRAPPED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((_, lines)) = known.iter().find(|(k, _)| *k == said) {
        return lines.clone();
    }
    let lines = wrap(said, ERROR_ROOM, |t| measure(t, ERROR, MANROPE));
    known.push((said, lines.clone()));
    lines
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
    let words = words();
    let text = format!("{} · {}", words.connected(), words.battery_of(percent));
    let disconnect = measure(words.disconnect(), STATUS, MANROPE) + 2.0 * SMALL_PAD;
    let room = RIGHT - disconnect - ICON_GAP - (LEFT + ICON + ICON_GAP);
    let wraps = measure(&text, STATUS, MANROPE) > room;
    known.push((percent, wraps));
    wraps
}

/// A connected device's block: 48 px, or more for its state's two lines.
fn block_height(device: &Device) -> f32 {
    let lines = if wraps(device.battery) { 2.0 } else { 1.0 };
    BLOCK.max(NAME * LINE + 1.0 + lines * STATUS * LINE)
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
        let note = (!on).then(|| next(note_height()));
        let blocks: Vec<(f32, f32)> = if on {
            bt.connected
                .iter()
                .map(|d| {
                    let height = block_height(d);
                    (next(height), height)
                })
                .collect()
        } else {
            Vec::new()
        };
        let section = |devices: &[Device], most: f32, next: &mut dyn FnMut(f32) -> f32| {
            let rule = next(2.0 * RULE + 1.0) + RULE;
            let label = next(LABEL_ROW);
            let mut rows = Vec::new();
            let mut content = 0.0;
            for d in devices {
                let height = row_height(ui.failure(&d.path));
                rows.push((content, height));
                content += height + ROW_GAP;
            }
            let content = (content - ROW_GAP).max(0.0);
            let note = devices.is_empty().then(|| next(note_height()));
            let list = (!devices.is_empty()).then(|| {
                let height = content.min(most);
                let top = next(height);
                (
                    Rectangle::new(
                        Point::new(LEFT - ROW_OUT, top),
                        Size::new(RIGHT - LEFT + 2.0 * ROW_OUT, height),
                    ),
                    content,
                )
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
        (self.end + PAD_BOTTOM + BORDER).round() as u32
    }

    fn list(&self) -> Option<(Rectangle, f32)> {
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
    /// Words measured: (text, size, bold) → width.
    widths: HashMap<(&'static str, u32, bool), f32>,
    /// Names cut to fit their room: (name, room, font) → shown.
    names: HashMap<(String, u32, bool), String>,
    /// The names as shown: connected, paired, around.
    shown: [Vec<String>; 3],
    /// The labels' letters and their places.
    labels: [Vec<(char, f32)>; 2],
    hover: Option<Target>,
    pressed: Option<Target>,
    pointer: Option<Point>,
    inside: bool,
    /// When the spinners started, for their turn.
    since: Option<Instant>,
    /// A wheel's notch comes as lines, then as pixels, which count for
    /// nothing.
    after_notch: bool,
    now: Option<Instant>,
}

impl State {
    /// A word's width, measured once.
    fn width(&mut self, text: &'static str, size: f32, bold: bool) -> f32 {
        let key = (text, tenths(size), bold);
        if let Some(width) = self.widths.get(&key) {
            return *width;
        }
        let width = measure(text, size, if bold { MANROPE_BOLD } else { MANROPE });
        self.widths.insert(key, width);
        width
    }

    /// A word's width as measured (an estimate if it was not).
    fn known(&self, text: &'static str, size: f32, bold: bool) -> f32 {
        self.widths
            .get(&(text, tenths(size), bold))
            .copied()
            .unwrap_or(text.chars().count() as f32 * size * 0.55)
    }

    /// `name` cut to fit `room`, once.
    fn fit(&mut self, name: &str, room: f32, semibold: bool) -> String {
        let key = (name.to_owned(), room.max(0.0) as u32, semibold);
        if let Some(shown) = self.names.get(&key) {
            return shown.clone();
        }
        let font = if semibold { MANROPE_SEMIBOLD } else { MANROPE };
        let size = if semibold { NAME } else { ROW_TEXT };
        let shown = match iced::advanced::graphics::text::font_system().write() {
            Ok(mut system) => super::sound::elide(name, room, |t| {
                super::sound::measure(system.raw(), t, font, size)
            }),
            Err(_) => name.to_owned(),
        };
        self.names.insert(key, shown.clone());
        shown
    }
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
        match event {
            iced::Event::Window(window::Event::RedrawRequested(now)) => self.refresh(state, *now),
            _ if !super::with(|c| c.is_open(self.id)) => None,
            iced::Event::Mouse(event) => {
                let at = crate::levels::draw::pointer(event, bounds, cursor, state.pointer);
                state.pointer = at;
                self.mouse(state, event, at)
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => {
                let pressed = Pressed {
                    key: key.clone(),
                    modifiers: *modifiers,
                    text: text.as_deref().map(str::to_owned),
                };
                self.key(state, &pressed)
            }
            _ => None,
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
        background(&mut frame, bounds.size(), self.glass);
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
            if let Some((view, _)) = layout.list() {
                let mut list = Frame::with_bounds(renderer, view);
                drawn.nearby(&mut list);
                drawn.scrollbar(&mut frame, view);
                all.push(list.into_geometry());
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
        if state.hover.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

impl Panel {
    /// What it shows, read afresh: the keys that reached another surface,
    /// Bluetooth, the nearby rows' order, the outcome of an attempt.
    fn refresh<Message>(&self, state: &mut State, now: Instant) -> Option<canvas::Action<Message>> {
        let keys = super::take_keys();
        if !keys.is_empty() && super::with(|c| c.is_open(self.id)) {
            for pressed in keys {
                let _: Option<canvas::Action<Message>> = self.key(state, &pressed);
            }
        }
        let bt = bluetooth::snapshot();
        ui(|ui| {
            ui.react(&bt);
            ui.relist(&bt);
            if ui
                .switch
                .is_some_and(|(asked, since)| asked == bt.powered || now >= since + ASKED_FOR)
            {
                ui.switch = None;
            }
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
                state.hover = None;
            }
            state.layout = Some(layout);
        });
        state.bt = bt;
        state.now = Some(now);
        self.measure_all(state);
        // A change of size reaches the controller, which resizes the
        // surface (without waking it again).
        super::refit(Item::Bluetooth);
        let spinning = self.motion && (state.bt.searching || state.bt.running().is_some());
        if spinning {
            state.since.get_or_insert(now);
        } else {
            state.since = None;
        }
        let sliding = self.motion && ui(|ui| ui.knob.is_some_and(|(_, since)| now < since + SLIDE));
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
        let in_list = at.is_some_and(|at| layout.list().is_some_and(|(v, _)| v.contains(at)));
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        match seen {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                if !state.inside && at.is_some() {
                    state.inside = true;
                    event(Seen::PopoverEnter);
                }
                let held = ui(|ui| std::mem::replace(&mut ui.over_list, in_list));
                if state.hover != target || held != in_list {
                    state.hover = target;
                    return redraw;
                }
                None
            }
            mouse::Event::CursorLeft => {
                state.inside = false;
                event(Seen::PopoverLeave);
                state.hover = None;
                ui(|ui| ui.over_list = false);
                redraw
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                state.pressed = target;
                ui(|ui| ui.focus = None);
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                let pressed = state.pressed.take();
                if pressed.is_some() && pressed == target {
                    self.act(state, pressed);
                }
                redraw
            }
            mouse::Event::WheelScrolled { delta } if in_list => {
                let by = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => {
                        state.after_notch = true;
                        -y.signum() * NOTCH
                    }
                    mouse::ScrollDelta::Pixels { .. } if state.after_notch => {
                        state.after_notch = false;
                        0.0
                    }
                    mouse::ScrollDelta::Pixels { y, .. } => -y,
                };
                let most = layout.most_scroll();
                ui(|ui| ui.scroll = (ui.scroll + by).clamp(0.0, most));
                state.hover = at.and_then(|at| self.target_at(state, &layout, at));
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
                rect.contains(at) && (!in_list || list.is_some_and(|(v, _)| v.contains(at)))
            })
    }

    fn disconnect_width(&self, state: &mut State) -> f32 {
        state.width(self.words.disconnect(), STATUS, false) + 2.0 * SMALL_PAD
    }

    /// `target` pressed, or `Enter` on it.
    fn act(&self, state: &mut State, target: Option<Target>) {
        let bt = state.bt.clone();
        let busy = bt.running().is_some();
        match target {
            Some(Target::Switch) => {
                let now = Instant::now();
                ui(|ui| {
                    let on = ui.powered(&bt, now);
                    let from = ui.knob_at(on, now, self.motion);
                    ui.switch = Some((!on, now));
                    ui.knob = Some((from, now));
                });
                let on = ui(|ui| ui.switch.is_some_and(|(asked, _)| asked));
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

    fn key<Message>(
        &self,
        state: &mut State,
        pressed: &Pressed,
    ) -> Option<canvas::Action<Message>> {
        use keyboard::{Key, key::Named};
        let layout = state.layout.clone()?;
        let order = targets(&layout, &state.bt, &state.nearby);
        let focus = ui(|ui| ui.focus);
        let at = focus.and_then(|f| order.iter().position(|t| *t == f));
        match &pressed.key {
            Key::Named(Named::Escape) => event(Seen::Escape),
            Key::Named(Named::Tab) => {
                let n = order.len() as isize;
                if n == 0 {
                    return None;
                }
                let by = if pressed.modifiers.shift() { -1 } else { 1 };
                let next = match at {
                    Some(i) => (i as isize + by).rem_euclid(n),
                    None if by > 0 => 0,
                    None => n - 1,
                };
                let next = order.get(next as usize).copied();
                ui(|ui| ui.focus = next);
            }
            Key::Named(Named::ArrowUp | Named::ArrowDown) => {
                let rows: Vec<Target> = order
                    .iter()
                    .copied()
                    .filter(|t| matches!(t, Target::Mine(_) | Target::Near(_)))
                    .collect();
                let i = rows.iter().position(|t| Some(*t) == focus)?;
                let n = rows.len();
                let down = matches!(pressed.key, Key::Named(Named::ArrowDown));
                let next = if down { (i + 1) % n } else { (i + n - 1) % n };
                ui(|ui| ui.focus = Some(rows[next]));
            }
            Key::Named(Named::Enter | Named::Space) => self.act(state, focus),
            _ => return None,
        }
        self.reveal(&layout);
        Some(canvas::Action::request_redraw().and_capture())
    }

    /// The nearby list scrolled so that the row holding the keyboard shows.
    fn reveal(&self, layout: &Layout) {
        let Some(Target::Near(i)) = ui(|ui| ui.focus) else {
            return;
        };
        let Some(section) = &layout.nearby else {
            return;
        };
        let (Some((view, _)), Some(&(top, height))) = (section.list, section.rows.get(i)) else {
            return;
        };
        let most = section.most_scroll();
        ui(|ui| {
            let mut scroll = ui.scroll;
            if top < scroll {
                scroll = top;
            } else if top + height > scroll + view.height {
                scroll = top + height - view.height;
            }
            ui.scroll = scroll.clamp(0.0, most);
        });
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
            state.width(text, size, bold);
        }
        for (i, label) in [words.my_devices(), words.nearby()].into_iter().enumerate() {
            if state.labels[i].is_empty() {
                let mut x = LEFT;
                state.labels[i] = label
                    .chars()
                    .map(|ch| {
                        let at = x;
                        x += measure(&ch.to_string(), LABEL, MANROPE_BOLD) + LABEL_SPACING;
                        (ch, at)
                    })
                    .collect();
            }
        }
        let disconnect = state.known(words.disconnect(), STATUS, false) + 2.0 * SMALL_PAD;
        let block_room = RIGHT - disconnect - ICON_GAP - (LEFT + ICON + ICON_GAP);
        let connected: Vec<String> = state.bt.connected.iter().map(|d| d.name.clone()).collect();
        state.shown[0] = connected
            .iter()
            .map(|n| state.fit(n, block_room, true))
            .collect();
        let running = state.bt.running().map(|(p, a)| (p.to_owned(), a));
        let row_room = |state: &State, device: &Device, scrolls: bool| {
            let width = RIGHT - LEFT + 2.0 * ROW_OUT - if scrolls { SCROLLBAR + 2.0 } else { 0.0 };
            let mut room = width - 2.0 * ROW_PAD - ICON - ICON_GAP;
            if let Some(note) = row_note(words, device, running.as_ref()) {
                room -= state.known(note.text, SMALL, false) + ICON_GAP;
                if note.spinner && self.motion {
                    room -= 6.0 + SPINNER;
                }
            }
            room
        };
        let paired = state.bt.paired.clone();
        state.shown[1] = paired
            .iter()
            .map(|d| {
                let room = row_room(state, d, false);
                state.fit(&d.name, room, false)
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
                state.fit(&d.name, room, false)
            })
            .collect();
    }
}

/// Where `target` is.
fn shape(target: Target, layout: &Layout, scroll: f32, disconnect: f32) -> Option<Rectangle> {
    let middle = HEADER_TOP + HEADER / 2.0;
    Some(match target {
        Target::Switch => Rectangle::new(
            Point::new(RIGHT - SWITCH.width, middle - SWITCH.height / 2.0),
            SWITCH,
        ),
        Target::Disconnect(i) => {
            let (top, height) = *layout.blocks.get(i)?;
            Rectangle::new(
                Point::new(RIGHT - disconnect, top + (height - SMALL_BUTTON) / 2.0),
                Size::new(disconnect, SMALL_BUTTON),
            )
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

/// What a row says at its right.
#[derive(Debug, Clone, Copy)]
struct Note {
    text: &'static str,
    colour: Color,
    /// With a spinner before it.
    spinner: bool,
}

/// `device`'s note: "Connexion…" or "Association…" while asked for, "dans
/// les Réglages" when it asks a code.
fn row_note(words: Words, device: &Device, running: Option<&(String, Action)>) -> Option<Note> {
    match running {
        Some((path, action)) if *path == device.path => Some(Note {
            text: match action {
                Action::Connect => words.connecting(),
                Action::Pair => words.pairing(),
            },
            colour: DIM,
            spinner: true,
        }),
        _ if device.code && !device.paired => Some(Note {
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
        self.state.hover == Some(target)
    }

    fn width(&self, text: &'static str, size: f32) -> f32 {
        self.state.known(text, size, false)
    }

    fn shape(&self, target: Target) -> Option<Rectangle> {
        let disconnect = self.width(self.words().disconnect(), STATUS) + 2.0 * SMALL_PAD;
        shape(target, self.layout, self.ui.scroll, disconnect)
    }

    fn running(&self) -> Option<(String, Action)> {
        self.state.bt.running().map(|(p, a)| (p.to_owned(), a))
    }

    /// The spinners' turn now, radians.
    fn turn(&self) -> f32 {
        let since = self.state.since.unwrap_or(self.now);
        let t = self.now.saturating_duration_since(since).as_secs_f32() / TURN.as_secs_f32();
        t.fract() * std::f32::consts::TAU
    }

    fn header(&self, f: &mut Frame) {
        let middle = HEADER_TOP + HEADER / 2.0;
        let mut title = text(
            self.words().bluetooth().to_owned(),
            Point::new(LEFT, middle),
            TITLE,
            TEXT,
            Alignment::Left,
        );
        title.font = MANROPE_BOLD;
        f.fill_text(title);
        if !self.layout.switch {
            return;
        }
        let Some(rect) = self.shape(Target::Switch) else {
            return;
        };
        let on = self.ui.powered(&self.state.bt, self.now);
        let p = self.ui.knob_at(on, self.now, self.panel.motion);
        f.fill(
            &Path::rounded_rectangle(rect.position(), rect.size(), (SWITCH.height / 2.0).into()),
            mix(SWITCH_OFF, ACCENT, p),
        );
        let centre = Point::new(
            rect.x + 2.0 + (SWITCH.width - KNOB - 4.0) * p + KNOB / 2.0,
            rect.center_y(),
        );
        // `box-shadow: 0 1px 3px`: rings fading out, 1 px down.
        for (grow, share) in [(2.0_f32, 0.15_f32), (1.2, 0.25), (0.5, 0.35)] {
            f.fill(
                &Path::circle(Point::new(centre.x, centre.y + 1.0), KNOB / 2.0 + grow),
                KNOB_SHADOW.scale_alpha(share),
            );
        }
        f.fill(&Path::circle(centre, KNOB / 2.0), TEXT);
    }

    /// "Le Bluetooth est coupé.", and the connected devices' blocks.
    fn blocks(&self, f: &mut Frame) {
        let words = self.words();
        if let Some(top) = self.layout.note {
            f.fill_text(text(
                words.bluetooth_is_off().to_owned(),
                Point::new(LEFT, top + NOTE_TOP + NOTE * LINE / 2.0),
                NOTE,
                FAINT,
                Alignment::Left,
            ));
        }
        for (i, ((top, height), device)) in self
            .layout
            .blocks
            .iter()
            .zip(&self.state.bt.connected)
            .enumerate()
        {
            let block = Rectangle::new(
                Point::new(LEFT - ROW_OUT, *top),
                Size::new(RIGHT - LEFT + 2.0 * ROW_OUT, *height),
            );
            f.fill(
                &Path::rounded_rectangle(block.position(), block.size(), 8.0.into()),
                BLOCK_FILL,
            );
            let middle = block.center_y();
            kind(
                f,
                Point::new(LEFT, middle - ICON / 2.0),
                ICON,
                device.kind,
                ACCENT,
            );
            if let Some(button) = self.shape(Target::Disconnect(i)) {
                let hover = self.hover(Target::Disconnect(i));
                f.fill(
                    &Path::rounded_rectangle(button.position(), button.size(), 7.0.into()),
                    if hover { FILL_HOVER } else { FILL },
                );
                f.fill_text(text(
                    words.disconnect().to_owned(),
                    button.center(),
                    STATUS,
                    if hover { TEXT } else { DIM },
                    Alignment::Center,
                ));
            }
            // Name and state: 14 px over 12.5 px (a line or two), 1 px
            // apart, centred.
            let x = LEFT + ICON + ICON_GAP;
            let two = wraps(device.battery);
            let lines = if two { 2.0 } else { 1.0 };
            let column = NAME * LINE + 1.0 + lines * STATUS * LINE;
            let name_middle = middle - column / 2.0 + NAME * LINE / 2.0;
            let mut name = text(
                self.state.shown[0].get(i).cloned().unwrap_or_default(),
                Point::new(x, name_middle),
                NAME,
                TEXT,
                Alignment::Left,
            );
            name.font = MANROPE_SEMIBOLD;
            f.fill_text(name);
            let status = name_middle + NAME * LINE / 2.0 + 1.0 + STATUS * LINE / 2.0;
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
                let (beside, under) = if two {
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
        f.fill_rectangle(
            Point::new(LEFT, section.rule),
            Size::new(RIGHT - LEFT, 1.0),
            RULE_COLOUR,
        );
        let middle = section.label + LABEL_ROW / 2.0;
        for (ch, x) in &self.state.labels[label] {
            let mut letter = text(
                ch.to_string(),
                Point::new(*x, middle),
                LABEL,
                DIM,
                Alignment::Left,
            );
            letter.font = MANROPE_BOLD;
            f.fill_text(letter);
        }
        if let Some(top) = section.note {
            f.fill_text(text(
                note.to_owned(),
                Point::new(LEFT, top + NOTE_TOP + NOTE * LINE / 2.0),
                NOTE,
                FAINT,
                Alignment::Left,
            ));
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
            self.row(
                f,
                rect,
                device,
                name,
                Target::Mine(i),
                running.as_ref(),
                false,
            );
        }
    }

    /// "AUTOUR", its search, and what it found, in the list's own frame.
    fn nearby(&self, list: &mut Frame) {
        let Some(section) = &self.layout.nearby else {
            return;
        };
        let running = self.running();
        for (i, device) in self.state.nearby.iter().enumerate() {
            let Some(rect) = section.row(i, self.ui.scroll) else {
                continue;
            };
            let name = self.state.shown[2].get(i).cloned().unwrap_or_default();
            self.row(
                list,
                rect,
                device,
                name,
                Target::Near(i),
                running.as_ref(),
                true,
            );
        }
    }

    /// One device's row at `rect`, its failure under it.
    #[allow(clippy::too_many_arguments)]
    fn row(
        &self,
        f: &mut Frame,
        rect: Rectangle,
        device: &Device,
        name: String,
        target: Target,
        running: Option<&(String, Action)>,
        in_list: bool,
    ) {
        let words = self.words();
        let enabled = running.is_none() && !(device.code && !device.paired);
        if enabled && self.hover(target) {
            f.fill(
                &Path::rounded_rectangle(rect.position(), rect.size(), 8.0.into()),
                ROW_HOVER,
            );
        }
        let colour = if device.code && !device.paired {
            FAINT
        } else {
            TEXT
        };
        let middle = rect.center_y();
        let icon = Point::new(rect.x + ROW_PAD, middle - ICON / 2.0);
        kind(f, icon, ICON, device.kind, colour);
        let mut right = rect.x + rect.width - ROW_PAD;
        if let Some(note) = row_note(words, device, running) {
            self.text_in(
                f,
                text(
                    note.text.to_owned(),
                    Point::new(right, middle),
                    SMALL,
                    note.colour,
                    Alignment::Right,
                ),
                in_list,
            );
            right -= self.width(note.text, SMALL);
            if note.spinner && self.panel.motion {
                right -= 6.0 + SPINNER;
                spinner(
                    f,
                    Point::new(right, middle - SPINNER / 2.0),
                    DIM,
                    self.turn(),
                );
            }
        }
        self.text_in(
            f,
            text(
                name,
                Point::new(icon.x + ICON + ICON_GAP, middle),
                ROW_TEXT,
                colour,
                Alignment::Left,
            ),
            in_list,
        );
        if let Some(failure) = self.ui.failure(&device.path) {
            let top = rect.y + ROW;
            for (i, line) in error_lines(failure).into_iter().enumerate() {
                let line_middle = top + (i as f32 + 0.5) * ERROR * ERROR_LINE;
                let mut said = text(
                    line,
                    Point::new(rect.x + ERROR_INDENT, line_middle),
                    ERROR,
                    PEACH,
                    Alignment::Left,
                );
                said.line_height = iced::widget::text::LineHeight::Relative(ERROR_LINE);
                self.text_in(f, said, in_list);
            }
        }
    }

    /// A text, in the nearby list: on the CPU, whose renderer does not cut
    /// text to a frame's bounds, only when it shows whole.
    fn text_in(&self, f: &mut Frame, text: canvas::Text, in_list: bool) {
        if in_list
            && self.panel.software
            && let Some((view, _)) = self.layout.list()
        {
            let half = text.size.0 * LINE / 2.0;
            if text.position.y - half < view.y - 1.0
                || text.position.y + half > view.y + view.height + 1.0
            {
                return;
            }
        }
        f.fill_text(text);
    }

    /// The nearby list's thumb, when it scrolls.
    fn scrollbar(&self, f: &mut Frame, view: Rectangle) {
        let Some(section) = &self.layout.nearby else {
            return;
        };
        let Some((_, content)) = section.list else {
            return;
        };
        if !section.scrolls() {
            return;
        }
        let height = (view.height * view.height / content).max(20.0);
        let travel = view.height - height;
        let share = self.ui.scroll / section.most_scroll().max(1.0);
        let thumb = Rectangle::new(
            Point::new(RIGHT, view.y + travel * share),
            Size::new(SCROLLBAR, height),
        );
        f.fill(
            &Path::rounded_rectangle(thumb.position(), thumb.size(), (SCROLLBAR / 2.0).into()),
            THUMB,
        );
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
                f.fill_text(text(
                    label.to_owned(),
                    Point::new(RIGHT, middle),
                    SMALL,
                    FAINT,
                    Alignment::Right,
                ));
                if self.panel.motion {
                    let at = RIGHT - self.width(label, SMALL) - 6.0 - SPINNER;
                    spinner(
                        f,
                        Point::new(at, middle - SPINNER / 2.0),
                        FAINT,
                        self.turn(),
                    );
                }
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
            Target::Disconnect(_) => 7.0,
            Target::Mine(_) | Target::Near(_) => 8.0,
        };
        ring(f, rect, radius);
    }
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
        assert_eq!(section.list.map(|(v, _)| v.height), Some(LIST));
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
