//! The Wi-Fi's popover (bar.md §9, the 3.5 mockup): its title and its
//! switch; "Connecté par câble" when a cable is in; the network connected,
//! a button to leave it, and with a sign-in page waiting, a button to open
//! it; then the networks around, strongest first, one row each (its
//! signal, its name, "enregistré" when saved, "dans les Réglages" for those
//! the popover cannot join, a lock when protected), scrolling past six.
//!
//! A saved or open network joins with a click; its row turns "Connexion…"
//! until NetworkManager says it is done. A protected one unfolds a
//! password field under its row: `Enter` or "Se connecter" (from 8
//! characters) tries it, `Esc` or "Annuler" folds it; a wrong password
//! says so under the field, which keeps it. A saved network whose password
//! NetworkManager now refuses unfolds the field the same way.
//!
//! **The password** comes from the keys as the compositor's keymap types
//! them (the user's layout, AltGr, dead keys: xkbcommon's text, with the
//! keymap and the modifiers the compositor sends), is kept in a
//! [`Secret`], shown as dots unless the eye is pressed, and goes to
//! NetworkManager only. `Ctrl+V` or `Shift+Insert` pastes the clipboard's
//! text at its end ([`crate::paste`]), without its line breaks. It is
//! cleared when its row folds, when the connection succeeds, and when the
//! popover closes.
//!
//! **The order** of the networks holds while it is worked in (the pointer
//! over the list, a row unfolded or focused, a connection under way): rows
//! do not move under the pointer; newcomers join at the end. Otherwise it
//! follows their strength.
//!
//! One canvas draws it, as the sound's: what changes draws this surface
//! alone. The list, and the field's text, are drawn in frames of their own,
//! cut to their bounds, which both renderers honour.

use std::{
    sync::{Mutex, PoisonError},
    time::Instant,
};

use iced::{
    Color, Point, Radians, Rectangle, Renderer, Size, Theme, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path},
        text::Alignment,
    },
    window,
};

use super::{
    Event as Seen, Item, Pressed, event,
    kit::{
        self, Aside, Clip, FILL, FILL_HOVER, HEADER, HEADER_TOP, ICON, ICON_GAP, Input, LEFT, List,
        MANROPE_SEMIBOLD, NOTE, NOTE_HEIGHT, PEACH, Pointer, RIGHT, ROW, ROW_GAP, ROW_OUT, ROW_PAD,
        ROW_RADIUS, ROW_TEXT, SMALL, SMALL_PAD, SMALL_RADIUS, STATUS, SWITCH, Spin, Switch, Wheel,
        Widths, rounded, text,
    },
};
use crate::{
    levels::{
        draw::{ACCENT, DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb, ring, stroke},
        model::Words,
    },
    network::{
        self, Ask,
        draw::{ethernet, wifi},
        model::{Failure, Network, Progress, Security, Wifi},
        secret::Secret,
    },
};

// --- The mockup's sizes, at ×1 ------------------------------------------------

const GAP: f32 = 6.0;
const WIRED: f32 = 32.0;
const PORTAL: f32 = 34.0;
const PORTAL_TEXT: f32 = 13.5;
/// The divider, 1 px with 4 px around it.
const RULE: f32 = 4.0;
/// The label's line: its padding and the "Searching…" beside it.
const LABEL_ROW: f32 = 2.0 + SMALL * LINE + 2.0;
/// The rows end 2 px short of the list's right while it does not scroll.
const ROW_END: f32 = 2.0;
const LOCK: f32 = 14.0;
/// The list's height at most: six rows and a peek of the seventh.
const LIST: f32 = 236.0;
/// The password's form under its row.
const FORM_TOP: f32 = 2.0;
const FORM_INDENT: f32 = 34.0;
const FORM_GAP: f32 = 8.0;
const FORM_BOTTOM: f32 = 10.0;
const FIELD: f32 = 34.0;
const FIELD_TEXT: f32 = 14.0;
const FIELD_PAD: f32 = 10.0;
const FIELD_RIGHT: f32 = 36.0;
const EYE: f32 = 28.0;
const EYE_ICON: f32 = 16.0;
const ERROR: f32 = 12.5;
const BUTTON: f32 = 30.0;
const BUTTON_PAD: f32 = 12.0;
const BUTTON_TEXT: f32 = 13.0;
/// A password's dots.
const DOT: f32 = 2.6;
const DOT_ADVANCE: f32 = 8.0;
/// A notch of a mouse's wheel scrolls two rows.
const NOTCH: f32 = 2.0 * (ROW + ROW_GAP);

const PORTAL_FILL: Color = Color::from_rgba8(229, 121, 63, 0.18);
const PORTAL_HOVER: Color = Color::from_rgba8(229, 121, 63, 0.26);
const ORANGE: Color = rgb(0xF2945B);
const FIELD_BORDER: Color = Color::from_rgba8(255, 255, 255, 0.16);
const INK: Color = rgb(0x0B1020);

// --- What the popover keeps while it is open -----------------------------------

/// What can be pointed at, or hold the keyboard's focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Switch,
    Disconnect,
    Portal,
    /// A network's row, by its place in the list.
    Row(usize),
    Field,
    Eye,
    Cancel,
    Join,
}

/// What it keeps between draws and between opening and closing: the
/// controller asks its height from it, and its rows' order holds.
#[derive(Debug, Default)]
struct Ui {
    /// The networks' SSIDs, in the order shown.
    order: Vec<Vec<u8>>,
    /// The row whose password field is unfolded.
    unfolded: Option<Vec<u8>>,
    password: Secret,
    /// The password shown, not dots.
    shown: bool,
    /// Why the last try with the field failed.
    error: Option<Failure>,
    /// A row joined with a click whose connection failed.
    failed: Option<Vec<u8>>,
    /// The last attempt reacted to.
    seen: u64,
    focus: Option<Target>,
    /// How far the list is scrolled, px.
    scroll: f32,
    /// The pointer is over the list.
    over_list: bool,
    /// The Wi-Fi's switch, until NetworkManager says it.
    switch: Switch,
    paste: Paste,
    /// Pastes asked, to tell a late one from the one awaited.
    pastes: u64,
}

/// A paste into the field: the clipboard's text, read off the loop, lands
/// only in the field that asked, still unfolded and holding the keyboard.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Paste {
    #[default]
    None,
    /// The text of paste `n` is on its way.
    Awaited(u64),
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

impl Ui {
    /// Whether the rows hold their order.
    fn holding(&self, network: &Network) -> bool {
        self.over_list
            || self.unfolded.is_some()
            || matches!(
                self.focus,
                Some(Target::Row(_) | Target::Field | Target::Eye | Target::Cancel | Target::Join)
            )
            || network.connecting_to().is_some()
    }

    /// The rows' order for `network`: as it is while held (gone ones out,
    /// new ones last), else by strength.
    fn relist(&mut self, network: &Network) {
        let others: Vec<&Wifi> = network.others().collect();
        if self.holding(network) {
            self.order.retain(|s| others.iter().any(|n| n.ssid == *s));
            for n in &others {
                if !self.order.contains(&n.ssid) {
                    self.order.push(n.ssid.clone());
                }
            }
        } else {
            self.order = others.iter().map(|n| n.ssid.clone()).collect();
        }
        if let Some(unfolded) = &self.unfolded
            && !self.order.contains(unfolded)
        {
            self.fold();
        }
        if !network.enabled {
            self.fold();
        }
    }

    /// The rows shown, in order.
    fn rows(&self, network: &Network) -> Vec<Wifi> {
        self.order
            .iter()
            .filter_map(|s| network.networks.iter().find(|n| n.ssid == *s).cloned())
            .collect()
    }

    fn fold(&mut self) {
        self.unfolded = None;
        self.password.clear();
        self.shown = false;
        self.error = None;
        // A paste on its way lands nowhere.
        self.paste = Paste::None;
        if matches!(
            self.focus,
            Some(Target::Field | Target::Eye | Target::Cancel | Target::Join)
        ) {
            self.focus = None;
        }
    }

    /// Unfolds `ssid`'s field, empty, the keyboard in it.
    fn unfold(&mut self, ssid: Vec<u8>) {
        self.fold();
        self.unfolded = Some(ssid);
        self.focus = Some(Target::Field);
    }

    /// What the network's last attempt says, once.
    fn react(&mut self, network: &Network) -> bool {
        let Some(attempt) = &network.attempt else {
            return false;
        };
        if attempt.id <= self.seen || attempt.state == Progress::Connecting {
            return false;
        }
        self.seen = attempt.id;
        let ssid = attempt.ssid.clone();
        let mine = self.unfolded.as_ref() == Some(&ssid);
        match attempt.state {
            Progress::Connected => {
                if mine {
                    self.fold();
                }
                self.failed = None;
                false
            }
            Progress::Failed(Failure::Secret) => {
                if !mine {
                    self.unfold(ssid);
                }
                self.error = Some(Failure::Secret);
                self.focus = Some(Target::Field);
                true
            }
            Progress::Failed(Failure::Other) => {
                if mine {
                    self.error = Some(Failure::Other);
                } else {
                    self.failed = Some(ssid);
                }
                false
            }
            Progress::Connecting => false,
        }
    }

    /// The switch as shown: as asked, until NetworkManager says it.
    fn enabled(&self, network: &Network, now: Instant) -> bool {
        self.switch.shown(network.enabled, now)
    }
}

/// The popover opens: the networks in their order now, nothing unfolded;
/// a scan asked, whose results come while it is open.
pub fn opening() {
    let network = network::snapshot();
    ui(|ui| {
        *ui = Ui {
            seen: network.attempt.as_ref().map_or(0, |a| a.id),
            ..Ui::default()
        };
        ui.relist(&network);
    });
    if network.enabled && network.hardware {
        network::ask(Ask::Scan);
    }
}

/// The popover closes: what was typed goes.
pub fn closing() {
    ui(|ui| {
        ui.fold();
        ui.focus = None;
    });
}

/// Asks for the clipboard's text, for the field holding the keyboard: it
/// lands at the field's end as if typed (whole if it fits, or not at all),
/// and clears the error; the popover then draws again. One at a time.
fn paste() {
    let n = ui(|ui| {
        if ui.paste != Paste::None {
            return None;
        }
        ui.pastes += 1;
        ui.paste = Paste::Awaited(ui.pastes);
        Some(ui.pastes)
    });
    let Some(n) = n else {
        return;
    };
    let asked = crate::paste::request(move |text| {
        let landed = ui(|ui| {
            if ui.paste != Paste::Awaited(n) {
                return false;
            }
            ui.paste = Paste::None;
            let Some(text) = text.filter(|t| !t.is_empty()) else {
                return false;
            };
            if ui.unfolded.is_none() || ui.focus != Some(Target::Field) {
                return false;
            }
            let fits = ui.password.push(text);
            if fits {
                ui.error = None;
                tracing::debug!("the clipboard's text pasted into the field");
            } else {
                tracing::debug!("the clipboard's text does not fit in the field");
            }
            fits
        });
        if landed {
            super::changed(Item::Wifi);
        }
    });
    if !asked {
        ui(|ui| {
            if ui.paste == Paste::Awaited(n) {
                ui.paste = Paste::None;
            }
        });
    }
}

/// The popover's height for what it shows now.
pub fn height() -> u32 {
    let network = network::snapshot();
    ui(|ui| {
        ui.relist(&network);
        Layout::of(&network, ui, &ui.rows(&network), Instant::now()).height()
    })
}

// --- Where everything is ---------------------------------------------------------

/// The networks' part: the divider, the label, the note or the list.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Section {
    rule: f32,
    label: f32,
    /// "No network found." in place of the list.
    note: Option<f32>,
    list: Option<List>,
}

#[derive(Debug, Clone, PartialEq)]
struct Layout {
    switch: bool,
    wired: Option<f32>,
    /// "Wi-Fi is off.", or "No Wi-Fi adapter."
    note: Option<f32>,
    /// The network connected's block: its top and its height.
    current: Option<(f32, f32)>,
    portal: Option<f32>,
    section: Option<Section>,
    /// Each row's top and height, in the list.
    rows: Vec<(f32, f32)>,
    /// The unfolded row, and its error's lines.
    unfolded: Option<(usize, usize)>,
    end: f32,
}

/// The height of the unfolded part under a row, with `error` lines.
fn form_height(error: usize) -> f32 {
    let error = if error > 0 {
        error as f32 * ERROR * LINE + FORM_GAP
    } else {
        0.0
    };
    FORM_TOP + FIELD + FORM_GAP + error + BUTTON + FORM_BOTTOM
}

/// The room the error has under the field: the field's width in a list
/// that scrolls.
const ERROR_ROOM: f32 = RIGHT - 2.0 - (LEFT - ROW_OUT) - FORM_INDENT - ROW_PAD;

/// What a failure says under the field, in lines.
fn error_lines(failure: Failure) -> Vec<String> {
    let words = kit::words();
    let said = match failure {
        Failure::Secret => words.wrong_password(),
        Failure::Other => words.could_not_connect(),
    };
    kit::wrapped(said, ERROR, ERROR_ROOM)
}

/// The network connected's state, under its name, in lines: "Connecté",
/// or "Page de connexion à ouvrir", which wraps beside Disconnect as the
/// mockup's does.
fn status_lines(portal: bool) -> Vec<String> {
    let words = kit::words();
    let said = if portal {
        words.sign_in_waits()
    } else {
        words.connected()
    };
    let disconnect = kit::measure(words.disconnect(), STATUS, MANROPE) + 2.0 * SMALL_PAD;
    kit::wrapped(said, STATUS, kit::block_room(disconnect))
}

/// The network connected's block: 48 px, or more for its state's lines.
fn current_height(portal: bool) -> f32 {
    kit::block_height(status_lines(portal).len().max(1) as f32)
}

impl Layout {
    fn of(network: &Network, ui: &Ui, rows: &[Wifi], now: Instant) -> Self {
        let mut y = HEADER_TOP + HEADER;
        let mut next = |height: f32| {
            y += GAP;
            let top = y;
            y += height;
            top
        };
        let enabled = ui.enabled(network, now) && network.hardware;
        let wired = network.wired.then(|| next(WIRED));
        let note = (!enabled).then(|| next(NOTE_HEIGHT));
        let current = network.current.as_ref().filter(|_| enabled).map(|_| {
            let height = current_height(network.portal);
            (next(height), height)
        });
        let portal = (current.is_some() && network.portal).then(|| next(PORTAL));
        let unfolded = ui
            .unfolded
            .as_ref()
            .and_then(|s| rows.iter().position(|n| n.ssid == *s))
            .map(|i| (i, ui.error.map_or(0, |e| error_lines(e).len())));
        let (tops, content) = kit::stack((0..rows.len()).map(|i| match unfolded {
            Some((u, error)) if u == i => ROW + form_height(error),
            _ => ROW,
        }));
        let section = enabled.then(|| {
            let rule = next(2.0 * RULE + 1.0) + RULE;
            let label = next(LABEL_ROW);
            let note = rows.is_empty().then(|| next(NOTE_HEIGHT));
            let list = (!rows.is_empty()).then(|| {
                let height = content.min(LIST);
                let top = next(height);
                kit::list(top, height, content)
            });
            Section {
                rule,
                label,
                note,
                list,
            }
        });
        Self {
            switch: network.hardware,
            wired,
            note,
            current,
            portal,
            section,
            rows: tops,
            unfolded,
            end: y,
        }
    }

    fn height(&self) -> u32 {
        kit::height(self.end)
    }

    fn list(&self) -> Option<List> {
        self.section.and_then(|s| s.list)
    }

    /// Whether the list scrolls.
    #[cfg(test)]
    fn scrolls(&self) -> bool {
        self.list().is_some_and(List::scrolls)
    }

    /// How far the list can scroll.
    fn most_scroll(&self) -> f32 {
        self.list().map_or(0.0, List::most_scroll)
    }

    /// Row `i`'s 36 px, on the popover, scrolled.
    fn row(&self, i: usize, scroll: f32) -> Option<Rectangle> {
        let (top, _) = *self.rows.get(i)?;
        Some(self.list()?.row(top, scroll, ROW_END))
    }

    /// The unfolded row's whole block (its row and its form).
    fn block(&self, i: usize, scroll: f32) -> Option<Rectangle> {
        let row = self.row(i, scroll)?;
        let (_, height) = *self.rows.get(i)?;
        Some(Rectangle::new(row.position(), Size::new(row.width, height)))
    }

    /// The unfolded form's parts: the field, the eye, the error's middle,
    /// Cancel and Connect (whose widths are measured: `cancel`, `join`).
    fn form(&self, scroll: f32, (cancel, join): (f32, f32)) -> Option<Form> {
        let (i, error) = self.unfolded?;
        let row = self.row(i, scroll)?;
        let left = row.x + FORM_INDENT;
        let right = row.x + row.width - ROW_PAD;
        let field = Rectangle::new(
            Point::new(left, row.y + ROW + FORM_TOP),
            Size::new(right - left, FIELD),
        );
        let eye = Rectangle::new(
            Point::new(
                field.x + field.width - 3.0 - EYE,
                field.center_y() - EYE / 2.0,
            ),
            Size::new(EYE, EYE),
        );
        let mut y = field.y + FIELD + FORM_GAP;
        let error = (error > 0).then(|| {
            let top = y;
            y += error as f32 * ERROR * LINE + FORM_GAP;
            top
        });
        let join = Rectangle::new(Point::new(right - join, y), Size::new(join, BUTTON));
        let cancel = Rectangle::new(
            Point::new(join.x - 6.0 - cancel, y),
            Size::new(cancel, BUTTON),
        );
        Some(Form {
            field,
            eye,
            error,
            cancel,
            join,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct Form {
    field: Rectangle,
    eye: Rectangle,
    /// The error's first line's top.
    error: Option<f32>,
    cancel: Rectangle,
    join: Rectangle,
}

// --- The canvas ------------------------------------------------------------------

/// The Wi-Fi's popover on its surface.
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
    network: Network,
    rows: Vec<Wifi>,
    layout: Option<Layout>,
    widths: Widths,
    /// The rows' names as shown, and the current network's.
    row_names: Vec<String>,
    current_name: String,
    /// The password's width when shown (it is never kept as a key).
    shown_width: f32,
    /// The label's letters and their places.
    label: Vec<(char, f32)>,
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
            let mut geometry = vec![];
            if let Some(shown) = layout.list() {
                let view = shown.view;
                let mut list = Frame::with_bounds(renderer, view);
                drawn.list(&mut list);
                let mut field = None;
                if let Some(form) = drawn.form() {
                    let room = Rectangle::new(
                        Point::new(form.field.x + FIELD_PAD, form.field.y),
                        Size::new(form.field.width - FIELD_PAD - FIELD_RIGHT, FIELD),
                    );
                    if let Some(room) = room.intersection(&view) {
                        let mut text = Frame::with_bounds(renderer, room);
                        drawn.field_text(&mut text, form.field);
                        field = Some(text.into_geometry());
                    }
                }
                shown.scrollbar(&mut frame, ui.scroll);
                geometry.push(list.into_geometry());
                geometry.extend(field);
            }
            drawn.focus(&mut frame);
            let mut all = vec![frame.into_geometry()];
            all.extend(geometry);
            all
        })
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match state.pointer.hover {
            Some(Target::Field) => mouse::Interaction::Text,
            Some(_) => mouse::Interaction::Pointer,
            None => mouse::Interaction::default(),
        }
    }
}

impl Panel {
    /// What it shows, read afresh: the keys that reached another surface,
    /// the network, the rows' order, the outcome of a connection.
    fn refresh<Message>(&self, state: &mut State, now: Instant) -> Option<canvas::Action<Message>> {
        for pressed in kit::keys(self.id) {
            let _: Option<canvas::Action<Message>> = self.key(state, &pressed);
        }
        let network = network::snapshot();
        let hold = ui(|ui| {
            let hold = ui.react(&network);
            ui.relist(&network);
            ui.switch.settle(network.enabled, now);
            state.rows = ui.rows(&network);
            let layout = Layout::of(&network, ui, &state.rows, now);
            ui.scroll = ui.scroll.clamp(0.0, layout.most_scroll());
            if ui
                .focus
                .is_some_and(|f| !targets(&layout, ui, &state.rows, &network).contains(&f))
            {
                ui.focus = None;
            }
            // Resized under a still pointer: what it is over is not known
            // until it moves.
            if state.layout.as_ref().map(Layout::height) != Some(layout.height()) {
                state.pointer.hover = None;
            }
            state.layout = Some(layout);
            hold
        });
        if hold {
            event(Seen::Hold);
        }
        state.network = network;
        state.now = Some(now);
        self.measure_all(state);
        // A change of size reaches the controller, which resizes the
        // surface (without waking it again).
        super::refit(Item::Wifi);
        let spins =
            self.motion && (state.network.scanning || state.network.connecting_to().is_some());
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
                // The field takes the keyboard on the press, as a browser's.
                ui(|ui| {
                    ui.focus = match target {
                        Some(Target::Field) => Some(Target::Field),
                        _ if ui.focus == Some(Target::Field) && ui.unfolded.is_some() => {
                            Some(Target::Field)
                        }
                        _ => None,
                    };
                });
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if let Some(clicked) = state.pointer.release(target) {
                    self.act(state, Some(clicked));
                }
                redraw
            }
            mouse::Event::WheelScrolled { delta } if in_list => {
                let by = state.wheel.down(delta, NOTCH, kit::popover_scale());
                let most = layout.most_scroll();
                ui(|ui| ui.scroll = (ui.scroll + by).clamp(0.0, most));
                // What is under the pointer now.
                state.pointer.hover = at.and_then(|at| self.target_at(state, &layout, at));
                redraw
            }
            _ => None,
        }
    }

    /// What is under `at`.
    fn target_at(&self, state: &mut State, layout: &Layout, at: Point) -> Option<Target> {
        let network = state.network.clone();
        let rows = state.rows.clone();
        let (scroll, list) = (ui(|ui| ui.scroll), layout.list());
        let widths = self.button_widths(state);
        let all = ui(|ui| targets(layout, ui, &rows, &network));
        // Last first: the eye is inside the field.
        all.into_iter().rev().find(|t| {
            let Some(rect) = self.shape(*t, layout, scroll, widths) else {
                return false;
            };
            // What is in the list is there only where the list shows.
            let in_list = matches!(
                t,
                Target::Row(_) | Target::Field | Target::Eye | Target::Cancel | Target::Join
            );
            rect.contains(at) && (!in_list || list.is_some_and(|l| l.view.contains(at)))
        })
    }

    /// The widths of Disconnect, Cancel and Connect.
    fn button_widths(&self, state: &mut State) -> [f32; 3] {
        let busy = state.network.connecting_to().is_some();
        let join = if busy {
            self.words.connecting()
        } else {
            self.words.join()
        };
        [
            state.widths.width(self.words.disconnect(), STATUS, false) + 2.0 * SMALL_PAD,
            state.widths.width(self.words.cancel(), BUTTON_TEXT, false) + 2.0 * BUTTON_PAD,
            state.widths.width(join, BUTTON_TEXT, true) + 2.0 * BUTTON_PAD,
        ]
    }

    /// Where `target` is, and its corners' radius.
    fn shape(
        &self,
        target: Target,
        layout: &Layout,
        scroll: f32,
        [disconnect, cancel, join]: [f32; 3],
    ) -> Option<Rectangle> {
        Some(match target {
            Target::Switch => Switch::rect(),
            Target::Disconnect => {
                let (top, height) = layout.current?;
                kit::block_button(top, height, disconnect)
            }
            Target::Portal => Rectangle::new(
                Point::new(LEFT, layout.portal?),
                Size::new(RIGHT - LEFT, PORTAL),
            ),
            Target::Row(i) => layout.row(i, scroll)?,
            Target::Field => layout.form(scroll, (cancel, join))?.field,
            Target::Eye => layout.form(scroll, (cancel, join))?.eye,
            Target::Cancel => layout.form(scroll, (cancel, join))?.cancel,
            Target::Join => layout.form(scroll, (cancel, join))?.join,
        })
    }

    /// `target` pressed, or `Enter` on it.
    fn act(&self, state: &mut State, target: Option<Target>) {
        let network = state.network.clone();
        let busy = network.connecting_to().is_some();
        match target {
            Some(Target::Switch) => {
                let on = ui(|ui| ui.switch.flip(network.enabled, Instant::now(), self.motion));
                network::ask(Ask::Enable(on));
            }
            Some(Target::Disconnect) => network::ask(Ask::Disconnect),
            Some(Target::Portal) => {
                if let Some(uri) = network.check_uri.clone() {
                    tracing::info!("the sign-in page opens in the browser");
                    network::open_in_browser(uri);
                }
            }
            Some(Target::Row(i)) => {
                let Some(row) = state.rows.get(i).cloned() else {
                    return;
                };
                if busy || !row.security.joinable() {
                    return;
                }
                let unfold = matches!(row.security, Security::Psk { .. }) && !row.saved;
                let hold = ui(|ui| {
                    ui.failed = None;
                    if !unfold {
                        ui.fold();
                        network::ask(Ask::Connect {
                            ssid: row.ssid.clone(),
                        });
                        return false;
                    }
                    if ui.unfolded.as_ref() == Some(&row.ssid) {
                        ui.fold();
                        ui.focus = Some(Target::Row(i));
                        false
                    } else {
                        ui.unfold(row.ssid.clone());
                        true
                    }
                });
                // A popover opened by hovering stays, for the typing.
                if hold {
                    event(Seen::Hold);
                }
            }
            Some(Target::Field) => {}
            Some(Target::Eye) => ui(|ui| {
                ui.shown = !ui.shown;
            }),
            Some(Target::Cancel) => ui(|ui| {
                if ui.unfolded.as_deref().is_some() && busy {
                    network::ask(Ask::Abandon);
                }
                let row = ui
                    .unfolded
                    .as_ref()
                    .and_then(|s| state.rows.iter().position(|n| n.ssid == *s));
                ui.fold();
                ui.focus = row.map(Target::Row);
            }),
            Some(Target::Join) => self.submit(&network),
            None => {}
        }
    }

    /// The password tried, if it is long enough and nothing is under way.
    fn submit(&self, network: &Network) {
        if network.connecting_to().is_some() {
            return;
        }
        ui(|ui| {
            let Some(ssid) = ui.unfolded.clone() else {
                return;
            };
            if !ui.password.complete() {
                return;
            }
            ui.error = None;
            network::ask(Ask::ConnectWith {
                ssid,
                secret: ui.password.duplicate(),
            });
        });
    }

    fn key<Message>(
        &self,
        state: &mut State,
        pressed: &Pressed,
    ) -> Option<canvas::Action<Message>> {
        use keyboard::{Key, key::Named};
        state.layout.as_ref()?;
        let network = state.network.clone();
        let rows = state.rows.clone();
        // The layout as the keys before this one left it, not as last
        // drawn: a row unfolded or the switch turned in the same frame
        // already changes what Tab goes to.
        let (layout, order) = ui(|ui| {
            let layout = Layout::of(&network, ui, &rows, Instant::now());
            let order = targets(&layout, ui, &rows, &network);
            (layout, order)
        });
        let focus = ui(|ui| ui.focus);
        let typing = focus == Some(Target::Field);
        let modifiers = pressed.modifiers;
        match &pressed.key {
            Key::Named(Named::Escape) => {
                let folded = ui(|ui| {
                    let row = ui
                        .unfolded
                        .as_ref()
                        .and_then(|s| rows.iter().position(|n| n.ssid == *s))?;
                    if network.connecting_to() == ui.unfolded.as_deref() {
                        network::ask(Ask::Abandon);
                    }
                    ui.fold();
                    ui.focus = Some(Target::Row(row));
                    Some(())
                });
                if folded.is_none() {
                    event(Seen::Escape);
                }
            }
            Key::Named(Named::Tab) => {
                let next = kit::step(&order, focus, modifiers.shift());
                ui(|ui| ui.focus = next);
            }
            Key::Named(Named::ArrowUp | Named::ArrowDown) => {
                let Some(row @ Target::Row(_)) = focus else {
                    return None;
                };
                let down = matches!(pressed.key, Key::Named(Named::ArrowDown));
                let rows: Vec<Target> = order
                    .iter()
                    .copied()
                    .filter(|t| matches!(t, Target::Row(_)))
                    .collect();
                let next = kit::cycle(&rows, row, down)?;
                ui(|ui| ui.focus = Some(next));
            }
            Key::Named(Named::Enter) => match focus {
                Some(Target::Field | Target::Join) => self.submit(&network),
                other => self.act(state, other),
            },
            Key::Named(Named::Backspace) if typing => ui(|ui| {
                if modifiers.control() {
                    ui.password.clear();
                } else {
                    ui.password.pop();
                }
                ui.error = None;
            }),
            Key::Named(Named::Space) if !typing => self.act(state, focus),
            // Nothing shows until the text comes.
            _ if typing && crate::paste::asks(&pressed.key, modifiers) => {
                paste();
                return Some(canvas::Action::capture());
            }
            _ if typing && !(modifiers.control() || modifiers.logo() || modifiers.alt()) => {
                let text = pressed
                    .text
                    .as_deref()
                    .filter(|t| !t.is_empty() && t.chars().all(|c| !c.is_control()))?;
                ui(|ui| {
                    if ui.password.push(text) {
                        ui.error = None;
                    }
                });
            }
            _ => return None,
        }
        self.reveal(state, &layout);
        Some(canvas::Action::request_redraw().and_capture())
    }

    /// The list scrolled so that what holds the keyboard shows.
    fn reveal(&self, state: &mut State, layout: &Layout) {
        let Some(List { view, .. }) = layout.list() else {
            return;
        };
        let network = state.network.clone();
        let rows = state.rows.clone();
        let focus = ui(|ui| ui.focus);
        let i = match focus {
            Some(Target::Row(i)) => i,
            Some(Target::Field | Target::Eye | Target::Cancel | Target::Join) => {
                match ui(|ui| ui.unfolded.clone())
                    .and_then(|s| rows.iter().position(|n| n.ssid == s))
                {
                    Some(i) => i,
                    None => return,
                }
            }
            _ => return,
        };
        // The new layout (a row unfolded) before scrolling.
        let layout = ui(|ui| Layout::of(&network, ui, &rows, Instant::now()));
        let Some(&row) = layout.rows.get(i) else {
            return;
        };
        let most = layout.most_scroll();
        ui(|ui| ui.scroll = kit::reveal(row, view.height, ui.scroll, most));
    }
}

/// What can hold the keyboard, in `Tab`'s order: the switch, Disconnect,
/// the sign-in page's button, each network that can be joined, then in an
/// unfolded row its field, its eye, Cancel and Connect (once it can).
fn targets(layout: &Layout, ui: &Ui, rows: &[Wifi], network: &Network) -> Vec<Target> {
    let mut order = Vec::new();
    if layout.switch {
        order.push(Target::Switch);
    }
    if layout.current.is_some() {
        order.push(Target::Disconnect);
    }
    if layout.portal.is_some() && network.check_uri.is_some() {
        order.push(Target::Portal);
    }
    if layout.list().is_none() {
        return order;
    }
    let busy = network.connecting_to().is_some();
    for (i, row) in rows.iter().enumerate() {
        if row.security.joinable() && !busy {
            order.push(Target::Row(i));
        }
        if layout.unfolded.is_some_and(|(u, _)| u == i) {
            order.extend([Target::Field, Target::Eye, Target::Cancel]);
            if ui.password.complete() && !busy {
                order.push(Target::Join);
            }
        }
    }
    order
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

    fn width(&self, text: &'static str, size: f32, bold: bool) -> f32 {
        self.state.widths.known(text, size, bold)
    }

    fn widths(&self) -> [f32; 3] {
        let busy = self.state.network.connecting_to().is_some();
        let join = if busy {
            self.words().connecting()
        } else {
            self.words().join()
        };
        [
            self.width(self.words().disconnect(), STATUS, false) + 2.0 * SMALL_PAD,
            self.width(self.words().cancel(), BUTTON_TEXT, false) + 2.0 * BUTTON_PAD,
            self.width(join, BUTTON_TEXT, true) + 2.0 * BUTTON_PAD,
        ]
    }

    fn shape(&self, target: Target) -> Option<Rectangle> {
        self.panel
            .shape(target, self.layout, self.ui.scroll, self.widths())
    }

    fn form(&self) -> Option<Form> {
        let [_, cancel, join] = self.widths();
        self.layout.form(self.ui.scroll, (cancel, join))
    }

    /// The spinners' turn now, if they turn.
    fn turn(&self) -> Option<f32> {
        self.panel.motion.then(|| self.state.spin.turn(self.now))
    }

    /// Text in the list: cut to it on the CPU.
    fn clip(&self) -> Clip {
        Clip::new(self.panel.software, self.layout.list().map(|l| l.view))
    }

    fn header(&self, f: &mut Frame) {
        kit::title(f, self.words().wifi());
        if !self.layout.switch {
            return;
        }
        let on = self.ui.enabled(&self.state.network, self.now);
        Switch::draw(f, self.ui.switch.knob_at(on, self.now, self.panel.motion));
    }

    /// The parts above the list.
    fn blocks(&self, f: &mut Frame) {
        let words = self.words();
        let network = &self.state.network;
        if let Some(top) = self.layout.wired {
            let middle = top + WIRED / 2.0;
            ethernet(f, Point::new(LEFT, middle - ICON / 2.0), ICON, DIM);
            f.fill_text(text(
                words.by_cable().to_owned(),
                Point::new(LEFT + ICON + ICON_GAP, middle),
                NOTE,
                DIM,
                Alignment::Left,
            ));
        }
        if let Some(top) = self.layout.note {
            let note = if network.hardware {
                words.wifi_is_off()
            } else {
                words.no_adapter()
            };
            f.fill_text(kit::note(note.to_owned(), top));
        }
        if let (Some((top, height)), Some(current)) = (self.layout.current, &network.current) {
            self.current(f, top, height, current);
        }
        if let Some(top) = self.layout.portal {
            let rect = Rectangle::new(Point::new(LEFT, top), Size::new(RIGHT - LEFT, PORTAL));
            let fill = if self.hover(Target::Portal) {
                PORTAL_HOVER
            } else {
                PORTAL_FILL
            };
            f.fill(&rounded(rect, ROW_RADIUS), fill);
            f.fill_text(kit::text_in(
                MANROPE_SEMIBOLD,
                text(
                    words.open_sign_in().to_owned(),
                    rect.center(),
                    PORTAL_TEXT,
                    PEACH,
                    Alignment::Center,
                ),
            ));
        }
        let Some(section) = self.layout.section else {
            return;
        };
        kit::rule(f, section.rule);
        let middle = section.label + LABEL_ROW / 2.0;
        kit::label(f, &self.state.label, middle);
        if network.scanning {
            let label = words.searching();
            let width = self.width(label, SMALL, false);
            kit::searching(f, label, width, middle, self.turn());
        }
        if let Some(top) = section.note {
            let note = if network.scanning {
                words.searching_networks()
            } else {
                words.no_network()
            };
            f.fill_text(kit::note(note.to_owned(), top));
        }
    }

    /// The network connected: its block, its name, its state, Disconnect.
    fn current(&self, f: &mut Frame, top: f32, height: f32, current: &Wifi) {
        let words = self.words();
        let middle = kit::block(f, top, height).center_y();
        wifi(
            f,
            Point::new(LEFT, middle - ICON / 2.0),
            ICON,
            current.bars(),
            ACCENT,
            false,
            1.0,
        );
        if let Some(button) = self.shape(Target::Disconnect) {
            let hover = self.hover(Target::Disconnect);
            kit::small_button(f, button, words.disconnect(), hover);
        }
        let x = LEFT + ICON + ICON_GAP;
        // Name and state: 14 px over 12.5 px (a line or two), 1 px apart,
        // centred.
        let portal = self.state.network.portal;
        let lines = status_lines(portal);
        let name = self.state.current_name.clone();
        let first = kit::block_name(f, name, middle, lines.len() as f32);
        let colour = if portal { PEACH } else { ACCENT };
        for (i, line) in lines.into_iter().enumerate() {
            f.fill_text(text(
                line,
                Point::new(x, first + i as f32 * STATUS * LINE),
                STATUS,
                colour,
                Alignment::Left,
            ));
        }
    }

    /// The networks, in the list's own frame.
    fn list(&self, f: &mut Frame) {
        let words = self.words();
        let network = &self.state.network;
        let busy = network.connecting_to();
        for (i, row) in self.state.rows.iter().enumerate() {
            let Some(rect) = self.layout.row(i, self.ui.scroll) else {
                continue;
            };
            let unfolded = self.layout.unfolded.is_some_and(|(u, _)| u == i);
            let joinable = row.security.joinable() && busy.is_none();
            if unfolded {
                if let Some(block) = self.layout.block(i, self.ui.scroll) {
                    kit::row_fill(f, block, false, true);
                }
            } else {
                kit::row_fill(f, rect, false, joinable && self.hover(Target::Row(i)));
            }
            let enterprise = !row.security.joinable();
            let colour = if enterprise { FAINT } else { TEXT };
            let middle = rect.center_y();
            let icon = Point::new(rect.x + ROW_PAD, middle - ICON / 2.0);
            wifi(f, icon, ICON, row.bars(), colour, false, 1.0);
            let mut right = rect.x + rect.width - ROW_PAD;
            if row.security.locked() {
                lock(f, Point::new(right - LOCK, middle - LOCK / 2.0));
                right -= LOCK + ICON_GAP;
            }
            let aside = row_aside(words, row, unfolded, busy, self.ui.failed.as_deref());
            if let Some(aside) = aside {
                let width = self.width(aside.text, SMALL, false);
                kit::aside(f, self.clip(), aside, (right, middle), width, self.turn());
            }
            let x = icon.x + ICON + ICON_GAP;
            let name = self.state.row_names.get(i).cloned().unwrap_or_default();
            self.clip().fill(
                f,
                text(
                    name,
                    Point::new(x, middle),
                    ROW_TEXT,
                    colour,
                    Alignment::Left,
                ),
            );
            if unfolded {
                self.form_parts(f);
            }
        }
    }

    /// The unfolded row's field (but its text), eye, error and buttons.
    fn form_parts(&self, f: &mut Frame) {
        let Some(form) = self.form() else {
            return;
        };
        let words = self.words();
        let error = self.ui.error;
        let border = if error.is_some() {
            ORANGE
        } else {
            FIELD_BORDER
        };
        let field = form.field;
        f.fill(&rounded(field, ROW_RADIUS), FILL);
        f.stroke(
            &Path::rounded_rectangle(
                Point::new(field.x + 0.5, field.y + 0.5),
                Size::new(field.width - 1.0, field.height - 1.0),
                7.5.into(),
            ),
            canvas::Stroke {
                style: canvas::Style::Solid(border),
                width: 1.0,
                ..canvas::Stroke::default()
            },
        );
        eye(f, form.eye, self.ui.shown);
        if let (Some(top), Some(error)) = (form.error, error) {
            let lines = error_lines(error);
            for said in kit::lines(&lines, (field.x, top), ERROR, LINE, PEACH) {
                self.clip().fill(f, said);
            }
        }
        let cancel = form.cancel;
        let hover = self.hover(Target::Cancel);
        f.fill(
            &rounded(cancel, ROW_RADIUS),
            if hover { FILL_HOVER } else { FILL },
        );
        self.clip().fill(
            f,
            text(
                words.cancel().to_owned(),
                cancel.center(),
                BUTTON_TEXT,
                if hover { TEXT } else { DIM },
                Alignment::Center,
            ),
        );
        let busy = self.state.network.connecting_to().is_some();
        let ready = self.ui.password.complete() && !busy;
        let join = form.join;
        f.fill(
            &rounded(join, ROW_RADIUS),
            if ready { ACCENT } else { FILL_HOVER },
        );
        let label = text(
            if busy {
                words.connecting()
            } else {
                words.join()
            }
            .to_owned(),
            join.center(),
            BUTTON_TEXT,
            if ready { INK } else { FAINT },
            Alignment::Center,
        );
        self.clip().fill(f, kit::text_in(MANROPE_BOLD, label));
    }

    /// The field's text, in its own frame: dots, or the password when the
    /// eye shows it, else the placeholder; the caret when it has the
    /// keyboard.
    fn field_text(&self, f: &mut Frame, field: Rectangle) {
        let left = field.x + FIELD_PAD;
        let room = field.width - FIELD_PAD - FIELD_RIGHT;
        let middle = field.center_y();
        let password = &self.ui.password;
        let caret = self.ui.focus == Some(Target::Field);
        if password.is_empty() {
            self.clip().fill(
                f,
                text(
                    self.words().password().to_owned(),
                    Point::new(left, middle),
                    FIELD_TEXT,
                    FAINT,
                    Alignment::Left,
                ),
            );
            if caret {
                f.fill_rectangle(Point::new(left, middle - 8.0), Size::new(1.0, 16.0), TEXT);
            }
            return;
        }
        let width = if self.ui.shown {
            self.state.shown_width
        } else {
            password.chars() as f32 * DOT_ADVANCE
        };
        // The end in view, as a field scrolls to its caret.
        let x = left - (width + 2.0 - room).max(0.0);
        if self.ui.shown {
            self.clip().fill(
                f,
                text(
                    password.as_str().to_owned(),
                    Point::new(x, middle),
                    FIELD_TEXT,
                    TEXT,
                    Alignment::Left,
                ),
            );
        } else {
            for i in 0..password.chars() {
                let centre = Point::new(x + DOT_ADVANCE * (i as f32 + 0.5), middle);
                f.fill(&Path::circle(centre, DOT), TEXT);
            }
        }
        if caret {
            f.fill_rectangle(
                Point::new(x + width + 1.0, middle - 8.0),
                Size::new(1.0, 16.0),
                TEXT,
            );
        }
    }

    /// The keyboard's ring, around what holds it (not the field, whose
    /// caret says so).
    fn focus(&self, f: &mut Frame) {
        let Some(target) = self.ui.focus.filter(|t| *t != Target::Field) else {
            return;
        };
        let Some(rect) = self.shape(target) else {
            return;
        };
        let radius = match target {
            Target::Switch => SWITCH.height / 2.0,
            Target::Disconnect => SMALL_RADIUS,
            Target::Eye => 6.0,
            _ => ROW_RADIUS,
        };
        ring(f, rect, radius);
    }
}

/// The lock, 14 px: `rect x5 y11 w14 h10 rx2` and `M8 11V8a4 4 0 0 1 8 0v3`.
fn lock(f: &mut Frame, at: Point) {
    let s = LOCK / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let line = stroke(2.2 * s, FAINT);
    f.stroke(
        &Path::rounded_rectangle(
            p(5.0, 11.0),
            Size::new(14.0 * s, 10.0 * s),
            (2.0 * s).into(),
        ),
        line,
    );
    let shackle = Path::new(|b| {
        b.move_to(p(8.0, 11.0));
        b.line_to(p(8.0, 8.0));
        b.arc(canvas::path::Arc {
            center: p(12.0, 8.0),
            radius: 4.0 * s,
            start_angle: Radians(std::f32::consts::PI),
            end_angle: Radians(std::f32::consts::TAU),
        });
        b.line_to(p(16.0, 11.0));
    });
    f.stroke(&shackle, line);
}

/// The eye in its 28 px button, 16 px: the almond `M2 12s3.6-7 10-7 10 7
/// 10 7-3.6 7-10 7S2 12 2 12z`, its pupil, and a slash while the password
/// shows.
fn eye(f: &mut Frame, button: Rectangle, shown: bool) {
    let s = EYE_ICON / 24.0;
    let at = Point::new(
        button.center_x() - EYE_ICON / 2.0,
        button.center_y() - EYE_ICON / 2.0,
    );
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let line = stroke(2.0 * s, DIM);
    let almond = Path::new(|b| {
        b.move_to(p(2.0, 12.0));
        b.bezier_curve_to(p(2.0, 12.0), p(5.6, 5.0), p(12.0, 5.0));
        b.bezier_curve_to(p(18.4, 5.0), p(22.0, 12.0), p(22.0, 12.0));
        b.bezier_curve_to(p(22.0, 12.0), p(18.4, 19.0), p(12.0, 19.0));
        b.bezier_curve_to(p(5.6, 19.0), p(2.0, 12.0), p(2.0, 12.0));
        b.close();
    });
    f.stroke(&almond, line);
    f.stroke(&Path::circle(p(12.0, 12.0), 3.0 * s), line);
    if shown {
        f.stroke(&Path::line(p(3.0, 3.0), p(21.0, 21.0)), line);
    }
}

/// What `row` says on its right, before its lock: "Connexion…" while it
/// joins, "dans les Réglages" when the popover cannot join it, "échec" when
/// it failed, "enregistré" when saved.
fn row_aside(
    words: Words,
    row: &Wifi,
    unfolded: bool,
    busy: Option<&[u8]>,
    failed: Option<&[u8]>,
) -> Option<Aside> {
    let this = Some(row.ssid.as_slice());
    let (text, colour, spinner) = if busy == this && !unfolded {
        (words.connecting(), DIM, true)
    } else if !row.security.joinable() {
        (words.in_settings(), FAINT, false)
    } else if failed == this {
        (words.failed(), PEACH, false)
    } else if row.saved && busy != this {
        (words.saved(), FAINT, false)
    } else {
        return None;
    };
    Some(Aside {
        text,
        colour,
        spinner,
    })
}

impl Panel {
    /// Measures, ahead of drawing, what the draw needs: it cannot measure
    /// (it reads the state). The words once; the names cut to their room;
    /// the password's width when it shows (never kept).
    fn measure_all(&self, state: &mut State) {
        let words = self.words;
        for (text, size, bold) in [
            (words.disconnect(), STATUS, false),
            (words.cancel(), BUTTON_TEXT, false),
            (words.join(), BUTTON_TEXT, true),
            (words.connecting(), BUTTON_TEXT, true),
            (words.searching(), SMALL, false),
            (words.saved(), SMALL, false),
            (words.in_settings(), SMALL, false),
            (words.failed(), SMALL, false),
            (words.connecting(), SMALL, false),
        ] {
            state.widths.width(text, size, bold);
        }
        if state.label.is_empty() {
            state.label = kit::label_letters(words.networks());
        }
        let Some(layout) = state.layout.clone() else {
            return;
        };
        let (unfolded, failed, scroll) =
            ui(|ui| (ui.unfolded.clone(), ui.failed.clone(), ui.scroll));
        let busy = state.network.connecting_to().map(<[u8]>::to_vec);
        let rows = state.rows.clone();
        state.row_names = rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let Some(rect) = layout.row(i, scroll) else {
                    return row.name.clone();
                };
                let lock = if row.security.locked() {
                    LOCK + ICON_GAP
                } else {
                    0.0
                };
                let open = unfolded.as_ref() == Some(&row.ssid);
                let aside =
                    row_aside(words, row, open, busy.as_deref(), failed.as_deref()).map(|aside| {
                        let width = state.widths.known(aside.text, SMALL, false);
                        (width, aside.spinner && self.motion)
                    });
                let room = kit::name_room(rect.width, lock, aside);
                state.widths.fit(&row.name, room, false)
            })
            .collect();
        if let Some(current) = state.network.current.clone() {
            let disconnect =
                state.widths.known(words.disconnect(), STATUS, false) + 2.0 * SMALL_PAD;
            let room = kit::block_room(disconnect);
            state.current_name = state.widths.fit(&current.name, room, true);
        }
        state.shown_width = ui(|ui| {
            if ui.shown && !ui.password.is_empty() {
                kit::measure(ui.password.as_str(), FIELD_TEXT, MANROPE)
            } else {
                0.0
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wifi(name: &str, strength: u8, security: Security) -> Wifi {
        Wifi {
            ssid: name.as_bytes().to_vec(),
            name: name.to_owned(),
            strength,
            security,
            saved: false,
            ap: format!("/{name}"),
        }
    }

    fn network(names: &[(&str, u8)]) -> Network {
        Network {
            manager: true,
            hardware: true,
            enabled: true,
            networks: names
                .iter()
                .map(|(n, s)| wifi(n, *s, Security::Psk { sae: false }))
                .collect(),
            ..Network::default()
        }
    }

    #[test]
    fn the_rows_hold_their_order_while_worked_in() {
        let mut ui = Ui::default();
        ui.relist(&network(&[("a", 80), ("b", 60), ("c", 40)]));
        assert_eq!(ui.order, [b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]);
        // The pointer over the list: c grows stronger, d comes; neither
        // moves a row.
        ui.over_list = true;
        let changed = network(&[("c", 90), ("a", 80), ("d", 70), ("b", 60)]);
        ui.relist(&changed);
        assert_eq!(
            ui.order,
            [b"a".to_vec(), b"b".to_vec(), b"c".to_vec(), b"d".to_vec()]
        );
        // Gone, it goes; the pointer away, strength again.
        ui.relist(&network(&[("c", 90), ("a", 80), ("b", 60)]));
        assert_eq!(ui.order.len(), 3);
        ui.over_list = false;
        ui.relist(&network(&[("c", 90), ("a", 80), ("b", 60)]));
        assert_eq!(ui.order, [b"c".to_vec(), b"a".to_vec(), b"b".to_vec()]);
    }

    #[test]
    fn its_height_follows_what_it_shows() {
        let now = Instant::now();
        let mut ui = Ui::default();
        // Off: the header and its line.
        let mut off = network(&[]);
        off.enabled = false;
        assert_eq!(Layout::of(&off, &ui, &[], now).height(), 90);
        // Eight networks: six and a peek, the list at its most.
        let eight: Vec<(&str, u8)> = ["a", "b", "c", "d", "e", "f", "g", "h"]
            .iter()
            .map(|n| (*n, 50))
            .collect();
        let around = network(&eight);
        ui.relist(&around);
        let rows = ui.rows(&around);
        let layout = Layout::of(&around, &ui, &rows, now);
        assert_eq!(layout.list().map(|l| l.view.height), Some(LIST));
        assert!(layout.scrolls());
        // Two: no scrolling, and a row unfolded grows it.
        let two = network(&[("a", 80), ("b", 60)]);
        ui.relist(&two);
        let rows = ui.rows(&two);
        let small = Layout::of(&two, &ui, &rows, now).height();
        ui.unfold(b"b".to_vec());
        let unfolded = Layout::of(&two, &ui, &rows, now);
        assert_eq!(unfolded.height(), small + form_height(0).round() as u32);
        assert!(!unfolded.scrolls());
        // Folding clears what was typed.
        ui.password.push("azerty&1@ê");
        ui.fold();
        assert!(ui.password.is_empty() && ui.unfolded.is_none());
    }
}
