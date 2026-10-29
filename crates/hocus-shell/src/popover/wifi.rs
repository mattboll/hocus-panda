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
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{POPOVER_WIDTH, RADIUS};
use iced::{
    Color, Pixels, Point, Radians, Rectangle, Renderer, Size, Theme, alignment, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, gradient},
        text::{Alignment, LineHeight, Shaping},
    },
    window,
};

use super::{Event as Seen, Item, Pressed, event};
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

const BORDER: f32 = 1.0;
const PAD_TOP: f32 = 12.0;
const PAD_SIDE: f32 = 14.0;
const PAD_BOTTOM: f32 = 14.0;
const GAP: f32 = 6.0;
const LEFT: f32 = BORDER + PAD_SIDE;
const RIGHT: f32 = POPOVER_WIDTH as f32 - BORDER - PAD_SIDE;
const HEADER: f32 = 32.0;
const TITLE: f32 = 16.0;
const SWITCH: Size = Size::new(40.0, 22.0);
const KNOB: f32 = 18.0;
const WIRED: f32 = 32.0;
const NOTE: f32 = 13.5;
const NOTE_TOP: f32 = 4.0;
const NOTE_BOTTOM: f32 = 2.0;
const CURRENT: f32 = 48.0;
const NAME: f32 = 14.0;
const STATUS: f32 = 12.5;
const SMALL_BUTTON: f32 = 28.0;
const SMALL_PAD: f32 = 9.0;
const PORTAL: f32 = 34.0;
const PORTAL_TEXT: f32 = 13.5;
/// The divider, 1 px with 4 px around it.
const RULE: f32 = 4.0;
const LABEL: f32 = 11.5;
const LABEL_SPACING: f32 = 0.08 * LABEL;
const SMALL: f32 = 12.0;
/// The label's line: its padding and the "Searching…" beside it.
const LABEL_ROW: f32 = 2.0 + SMALL * LINE + 2.0;
const ROW: f32 = 36.0;
const ROW_GAP: f32 = 2.0;
const ROW_OUT: f32 = 6.0;
const ROW_PAD: f32 = 6.0;
const ROW_TEXT: f32 = 14.0;
const ICON: f32 = 18.0;
const ICON_GAP: f32 = 10.0;
const LOCK: f32 = 14.0;
const SPINNER: f32 = 12.0;
/// The list's height at most: six rows and a peek of the seventh.
const LIST: f32 = 236.0;
const SCROLLBAR: f32 = 6.0;
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

const HEADER_TOP: f32 = BORDER + PAD_TOP;

const GLASS_TOP: Color = Color::from_rgba8(22, 27, 42, 0.72);
const GLASS_BOTTOM: Color = Color::from_rgba8(14, 17, 28, 0.82);
const OPAQUE_TOP: Color = Color::from_rgba8(0x16, 0x1B, 0x2A, 1.0);
const OPAQUE_BOTTOM: Color = Color::from_rgba8(0x0E, 0x11, 0x1C, 1.0);
const RIM: Color = Color::from_rgba8(255, 255, 255, 0.10);
const FILL: Color = Color::from_rgba8(255, 255, 255, 0.06);
const FILL_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.08);
const ROW_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.05);
const RULE_COLOUR: Color = Color::from_rgba8(255, 255, 255, 0.08);
const SWITCH_OFF: Color = Color::from_rgba8(255, 255, 255, 0.18);
const KNOB_SHADOW: Color = Color::from_rgba8(0, 0, 0, 0.4);
const CURRENT_FILL: Color = Color::from_rgba8(108, 194, 135, 0.10);
const PORTAL_FILL: Color = Color::from_rgba8(229, 121, 63, 0.18);
const PORTAL_HOVER: Color = Color::from_rgba8(229, 121, 63, 0.26);
const PEACH: Color = rgb(0xF7B98A);
const ORANGE: Color = rgb(0xF2945B);
const FIELD_BORDER: Color = Color::from_rgba8(255, 255, 255, 0.16);
const INK: Color = rgb(0x0B1020);
const THUMB: Color = Color::from_rgba8(255, 255, 255, 0.16);

/// The switch's knob slides over this, `ease-out`; the spinners turn once
/// in the other.
const SLIDE: Duration = Duration::from_millis(160);
const TURN: Duration = Duration::from_millis(900);
/// The switch shows what was asked this long at most, until
/// NetworkManager says it.
const ASKED_FOR: Duration = Duration::from_secs(3);

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
    /// The switch asked, and when.
    switch: Option<(bool, Instant)>,
    /// The knob's slide: from where (0 off, 1 on), since when.
    knob: Option<(f32, Instant)>,
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
        match self.switch {
            Some((asked, since)) if now < since + ASKED_FOR => asked,
            _ => network.enabled,
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
    /// The list's viewport, and its content's height.
    list: Option<(Rectangle, f32)>,
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
    /// Each row's top and height, in the list's content.
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

/// The words of the shell's language, for what the layout measures.
fn words() -> Words {
    Words::new(crate::clock::time::Lang::of_messages())
}

/// `text` at `size` in lines that fit `room`, as the mockup's text wraps;
/// measured once each.
fn wrapped(text: &'static str, size: f32, room: f32) -> Vec<String> {
    type Key = (&'static str, u32, u32);
    static WRAPPED: Mutex<Vec<(Key, Vec<String>)>> = Mutex::new(Vec::new());
    let key = (text, tenths(size), room as u32);
    let mut known = WRAPPED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((_, lines)) = known.iter().find(|(k, _)| *k == key) {
        return lines.clone();
    }
    let lines = wrap(text, room, |t| measure(t, size, MANROPE));
    known.push((key, lines.clone()));
    lines
}

/// What a failure says under the field, in lines.
fn error_lines(failure: Failure) -> Vec<String> {
    let words = words();
    let said = match failure {
        Failure::Secret => words.wrong_password(),
        Failure::Other => words.could_not_connect(),
    };
    wrapped(said, ERROR, ERROR_ROOM)
}

/// The network connected's state, under its name, in lines: "Connecté",
/// or "Page de connexion à ouvrir", which wraps beside Disconnect as the
/// mockup's does.
fn status_lines(portal: bool) -> Vec<String> {
    let words = words();
    let said = if portal {
        words.sign_in_waits()
    } else {
        words.connected()
    };
    let disconnect = measure(words.disconnect(), STATUS, MANROPE) + 2.0 * SMALL_PAD;
    let room = RIGHT - disconnect - ICON_GAP - (LEFT + ICON + ICON_GAP);
    wrapped(said, STATUS, room)
}

/// The network connected's block: 48 px, or more for its state's lines.
fn current_height(portal: bool) -> f32 {
    let lines = status_lines(portal).len().max(1) as f32;
    CURRENT.max(NAME * LINE + 1.0 + lines * STATUS * LINE)
}

/// `text` broken between its words into lines that fit `room` as `width`
/// measures them; a word too wide alone keeps its own line.
fn wrap(text: &str, room: f32, mut width: impl FnMut(&str) -> f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split(' ') {
        match lines.last_mut() {
            Some(line) if width(&format!("{line} {word}")) <= room => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_owned()),
        }
    }
    lines
}

fn note_height() -> f32 {
    NOTE_TOP + NOTE * LINE + NOTE_BOTTOM
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
        let note = (!enabled).then(|| next(note_height()));
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
        let mut tops = Vec::new();
        let mut content = 0.0;
        for i in 0..rows.len() {
            let height = match unfolded {
                Some((u, error)) if u == i => ROW + form_height(error),
                _ => ROW,
            };
            tops.push((content, height));
            content += height + ROW_GAP;
        }
        let content = (content - ROW_GAP).max(0.0);
        let section = enabled.then(|| {
            let rule = next(2.0 * RULE + 1.0) + RULE;
            let label = next(LABEL_ROW);
            let note = rows.is_empty().then(|| next(note_height()));
            let list = (!rows.is_empty()).then(|| {
                let height = content.min(LIST);
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
        (self.end + PAD_BOTTOM + BORDER).round() as u32
    }

    fn list(&self) -> Option<(Rectangle, f32)> {
        self.section.and_then(|s| s.list)
    }

    /// Whether the list scrolls.
    fn scrolls(&self) -> bool {
        self.list()
            .is_some_and(|(view, content)| content > view.height + 0.5)
    }

    /// How far the list can scroll.
    fn most_scroll(&self) -> f32 {
        self.list()
            .map_or(0.0, |(view, content)| (content - view.height).max(0.0))
    }

    /// Row `i`'s 36 px, on the popover, scrolled.
    fn row(&self, i: usize, scroll: f32) -> Option<Rectangle> {
        let (view, _) = self.list()?;
        let (top, _) = *self.rows.get(i)?;
        let right = if self.scrolls() {
            RIGHT - 2.0
        } else {
            view.x + view.width - 2.0
        };
        Some(Rectangle::new(
            Point::new(view.x, view.y + top - scroll),
            Size::new(right - view.x, ROW),
        ))
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
    /// Words measured: (text, size, bold) → width.
    widths: HashMap<(&'static str, u32, bool), f32>,
    /// Names cut to fit their room: (name, room) → shown.
    names: HashMap<(String, u32), String>,
    /// The rows' names as shown, and the current network's.
    row_names: Vec<String>,
    current_name: String,
    /// The password's width when shown (it is never kept as a key).
    shown_width: f32,
    /// The label's letters and their places.
    label: Vec<(char, f32)>,
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

/// A key for a measure: the size in tenths.
fn tenths(size: f32) -> u32 {
    (size * 10.0).round() as u32
}

/// `text`'s width, measured.
fn measure(text: &str, size: f32, font: iced::Font) -> f32 {
    match iced::advanced::graphics::text::font_system().write() {
        Ok(mut system) => super::sound::measure(system.raw(), text, font, size),
        Err(_) => text.chars().count() as f32 * size * 0.55,
    }
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

    /// `name` cut to fit `room` at `size` in `font`, once.
    fn fit(&mut self, name: &str, room: f32, font: iced::Font) -> String {
        let key = (name.to_owned(), room.max(0.0) as u32);
        if let Some(shown) = self.names.get(&key) {
            return shown.clone();
        }
        let shown = match iced::advanced::graphics::text::font_system().write() {
            Ok(mut system) => super::sound::elide(name, room, |t| {
                super::sound::measure(system.raw(), t, font, ROW_TEXT)
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
        self.background(&mut frame, bounds.size());
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
            if let Some((view, _)) = layout.list() {
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
                drawn.scrollbar(&mut frame, view);
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
        match state.hover {
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
        let keys = super::take_keys();
        if !keys.is_empty() && super::with(|c| c.is_open(self.id)) {
            for pressed in keys {
                let _: Option<canvas::Action<Message>> = self.key(state, &pressed);
            }
        }
        let network = network::snapshot();
        let hold = ui(|ui| {
            let hold = ui.react(&network);
            ui.relist(&network);
            if ui
                .switch
                .is_some_and(|(asked, since)| asked == network.enabled || now >= since + ASKED_FOR)
            {
                ui.switch = None;
            }
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
                state.hover = None;
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
        let spinning =
            self.motion && (state.network.scanning || state.network.connecting_to().is_some());
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
                // What is under the pointer now.
                state.hover = at.and_then(|at| self.target_at(state, &layout, at));
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
            rect.contains(at) && (!in_list || list.is_some_and(|(v, _)| v.contains(at)))
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
            state.width(self.words.disconnect(), STATUS, false) + 2.0 * SMALL_PAD,
            state.width(self.words.cancel(), BUTTON_TEXT, false) + 2.0 * BUTTON_PAD,
            state.width(join, BUTTON_TEXT, true) + 2.0 * BUTTON_PAD,
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
        let middle = HEADER_TOP + HEADER / 2.0;
        Some(match target {
            Target::Switch => Rectangle::new(
                Point::new(RIGHT - SWITCH.width, middle - SWITCH.height / 2.0),
                SWITCH,
            ),
            Target::Disconnect => {
                let (top, height) = layout.current?;
                Rectangle::new(
                    Point::new(RIGHT - disconnect, top + (height - SMALL_BUTTON) / 2.0),
                    Size::new(disconnect, SMALL_BUTTON),
                )
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
                let now = Instant::now();
                ui(|ui| {
                    let on = ui.enabled(&network, now);
                    let from = ui.knob_at(on, now, self.motion);
                    ui.switch = Some((!on, now));
                    ui.knob = Some((from, now));
                    network::ask(Ask::Enable(!on));
                });
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
        let layout = state.layout.clone()?;
        let network = state.network.clone();
        let rows = state.rows.clone();
        let order = ui(|ui| targets(&layout, ui, &rows, &network));
        let focus = ui(|ui| ui.focus);
        let at = focus.and_then(|f| order.iter().position(|t| *t == f));
        let step = |by: isize| {
            let n = order.len() as isize;
            if n == 0 {
                return None;
            }
            let next = match at {
                Some(i) => (i as isize + by).rem_euclid(n),
                None if by > 0 => 0,
                None => n - 1,
            };
            order.get(next as usize).copied()
        };
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
                let next = step(if modifiers.shift() { -1 } else { 1 });
                ui(|ui| ui.focus = next);
            }
            Key::Named(Named::ArrowUp | Named::ArrowDown) => {
                let Some(Target::Row(i)) = focus else {
                    return None;
                };
                let down = matches!(pressed.key, Key::Named(Named::ArrowDown));
                let enabled: Vec<usize> = order
                    .iter()
                    .filter_map(|t| match t {
                        Target::Row(r) => Some(*r),
                        _ => None,
                    })
                    .collect();
                let at = enabled.iter().position(|r| *r == i)?;
                let n = enabled.len();
                let next = if down { (at + 1) % n } else { (at + n - 1) % n };
                ui(|ui| ui.focus = Some(Target::Row(enabled[next])));
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
        let Some((view, _)) = layout.list() else {
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
        let Some(&(top, height)) = layout.rows.get(i) else {
            return;
        };
        let most = layout.most_scroll();
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

    fn background(&self, f: &mut Frame, size: Size) {
        let (top, bottom) = if self.glass {
            (GLASS_TOP, GLASS_BOTTOM)
        } else {
            (OPAQUE_TOP, OPAQUE_BOTTOM)
        };
        let radius = RADIUS as f32;
        let shape = Path::rounded_rectangle(Point::ORIGIN, size, radius.into());
        let tint = gradient::Linear::new(Point::ORIGIN, Point::new(0.0, size.height))
            .add_stop(0.0, top)
            .add_stop(1.0, bottom);
        f.fill(&shape, canvas::Fill::from(tint));
        let rim = Path::rounded_rectangle(
            Point::new(BORDER / 2.0, BORDER / 2.0),
            Size::new(size.width - BORDER, size.height - BORDER),
            (radius - BORDER / 2.0).into(),
        );
        f.stroke(
            &rim,
            canvas::Stroke {
                style: canvas::Style::Solid(RIM),
                width: BORDER,
                ..canvas::Stroke::default()
            },
        );
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
        self.state.hover == Some(target)
    }

    fn width(&self, text: &'static str, size: f32, bold: bool) -> f32 {
        self.state.known(text, size, bold)
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

    /// The spinners' turn now, radians.
    fn turn(&self) -> f32 {
        let since = self.state.since.unwrap_or(self.now);
        let t = self.now.saturating_duration_since(since).as_secs_f32() / TURN.as_secs_f32();
        t.fract() * std::f32::consts::TAU
    }

    fn header(&self, f: &mut Frame) {
        let middle = HEADER_TOP + HEADER / 2.0;
        let mut title = text(
            self.words().wifi().to_owned(),
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
        let on = self.ui.enabled(&self.state.network, self.now);
        let p = self.ui.knob_at(on, self.now, self.panel.motion);
        let fill = mix(SWITCH_OFF, ACCENT, p);
        f.fill(
            &Path::rounded_rectangle(rect.position(), rect.size(), (SWITCH.height / 2.0).into()),
            fill,
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
            f.fill_text(text(
                note.to_owned(),
                Point::new(LEFT, top + NOTE_TOP + NOTE * LINE / 2.0),
                NOTE,
                FAINT,
                Alignment::Left,
            ));
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
            f.fill(
                &Path::rounded_rectangle(rect.position(), rect.size(), 8.0.into()),
                fill,
            );
            let mut label = text(
                words.open_sign_in().to_owned(),
                rect.center(),
                PORTAL_TEXT,
                PEACH,
                Alignment::Center,
            );
            label.font = MANROPE_SEMIBOLD;
            f.fill_text(label);
        }
        let Some(section) = self.layout.section else {
            return;
        };
        f.fill_rectangle(
            Point::new(LEFT, section.rule),
            Size::new(RIGHT - LEFT, 1.0),
            RULE_COLOUR,
        );
        let middle = section.label + LABEL_ROW / 2.0;
        for (ch, x) in &self.state.label {
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
        if network.scanning {
            let label = words.searching();
            f.fill_text(text(
                label.to_owned(),
                Point::new(RIGHT, middle),
                SMALL,
                FAINT,
                Alignment::Right,
            ));
            if self.panel.motion {
                let at = RIGHT - self.width(label, SMALL, false) - 6.0 - SPINNER;
                spinner(
                    f,
                    Point::new(at, middle - SPINNER / 2.0),
                    FAINT,
                    self.turn(),
                );
            }
        }
        if let Some(top) = section.note {
            let note = if network.scanning {
                words.searching_networks()
            } else {
                words.no_network()
            };
            f.fill_text(text(
                note.to_owned(),
                Point::new(LEFT, top + NOTE_TOP + NOTE * LINE / 2.0),
                NOTE,
                FAINT,
                Alignment::Left,
            ));
        }
    }

    /// The network connected: its block, its name, its state, Disconnect.
    fn current(&self, f: &mut Frame, top: f32, height: f32, current: &Wifi) {
        let words = self.words();
        let block = Rectangle::new(
            Point::new(LEFT - ROW_OUT, top),
            Size::new(RIGHT - LEFT + 2.0 * ROW_OUT, height),
        );
        f.fill(
            &Path::rounded_rectangle(block.position(), block.size(), 8.0.into()),
            CURRENT_FILL,
        );
        let middle = block.center_y();
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
        let x = LEFT + ICON + ICON_GAP;
        // Name and state: 14 px over 12.5 px (a line or two), 1 px apart,
        // centred.
        let portal = self.state.network.portal;
        let lines = status_lines(portal);
        let column = NAME * LINE + 1.0 + lines.len() as f32 * STATUS * LINE;
        let name_middle = middle - column / 2.0 + NAME * LINE / 2.0;
        let name = self.state.current_name.clone();
        let mut label = text(
            name,
            Point::new(x, name_middle),
            NAME,
            TEXT,
            Alignment::Left,
        );
        label.font = MANROPE_SEMIBOLD;
        f.fill_text(label);
        let colour = if portal { PEACH } else { ACCENT };
        let first = name_middle + NAME * LINE / 2.0 + 1.0 + STATUS * LINE / 2.0;
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
                    f.fill(
                        &Path::rounded_rectangle(block.position(), block.size(), 8.0.into()),
                        ROW_HOVER,
                    );
                }
            } else if joinable && self.hover(Target::Row(i)) {
                f.fill(
                    &Path::rounded_rectangle(rect.position(), rect.size(), 8.0.into()),
                    ROW_HOVER,
                );
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
            let note = row_note(words, row, unfolded, busy, self.ui.failed.as_deref());
            if let Some(note) = note {
                self.list_text(
                    f,
                    text(
                        note.text.to_owned(),
                        Point::new(right, middle),
                        SMALL,
                        note.colour,
                        Alignment::Right,
                    ),
                );
                right -= self.width(note.text, SMALL, false);
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
            let x = icon.x + ICON + ICON_GAP;
            let name = self.state.row_names.get(i).cloned().unwrap_or_default();
            self.list_text(
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
        f.fill(
            &Path::rounded_rectangle(field.position(), field.size(), 8.0.into()),
            FILL,
        );
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
            for (i, line) in error_lines(error).into_iter().enumerate() {
                let middle = top + (i as f32 + 0.5) * ERROR * LINE;
                self.list_text(
                    f,
                    text(
                        line,
                        Point::new(field.x, middle),
                        ERROR,
                        PEACH,
                        Alignment::Left,
                    ),
                );
            }
        }
        let cancel = form.cancel;
        let hover = self.hover(Target::Cancel);
        f.fill(
            &Path::rounded_rectangle(cancel.position(), cancel.size(), 8.0.into()),
            if hover { FILL_HOVER } else { FILL },
        );
        self.list_text(
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
            &Path::rounded_rectangle(join.position(), join.size(), 8.0.into()),
            if ready { ACCENT } else { FILL_HOVER },
        );
        let mut label = text(
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
        label.font = MANROPE_BOLD;
        self.list_text(f, label);
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
            self.list_text(
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
            self.list_text(
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

    /// A text in the list: on the CPU, whose renderer does not cut text to
    /// a frame's bounds, only when it shows whole.
    fn list_text(&self, f: &mut Frame, text: canvas::Text) {
        if self.panel.software
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

    fn scrollbar(&self, f: &mut Frame, view: Rectangle) {
        let Some((_, content)) = self.layout.list() else {
            return;
        };
        if !self.layout.scrolls() {
            return;
        }
        let height = (view.height * view.height / content).max(20.0);
        let travel = view.height - height;
        let share = self.ui.scroll / self.layout.most_scroll().max(1.0);
        let thumb = Rectangle::new(
            Point::new(RIGHT, view.y + travel * share),
            Size::new(SCROLLBAR, height),
        );
        f.fill(
            &Path::rounded_rectangle(thumb.position(), thumb.size(), (SCROLLBAR / 2.0).into()),
            THUMB,
        );
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
            Target::Disconnect => 7.0,
            Target::Eye => 6.0,
            _ => 8.0,
        };
        ring(f, rect, radius);
    }
}

/// Manrope at 600, the mockup's for the network's name and the sign-in
/// page's button.
pub(crate) const MANROPE_SEMIBOLD: iced::Font = iced::Font {
    weight: iced::font::Weight::Semibold,
    ..MANROPE
};

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

fn text(
    content: String,
    position: Point,
    size: f32,
    color: Color,
    align: Alignment,
) -> canvas::Text {
    canvas::Text {
        content,
        position,
        max_width: f32::INFINITY,
        color,
        size: Pixels(size),
        line_height: LineHeight::Relative(LINE),
        font: MANROPE,
        align_x: align,
        align_y: alignment::Vertical::Center,
        shaping: Shaping::Advanced,
    }
}

/// The spinner: `M21 12a9 9 0 1 1-6.2-8.6`, three quarters and more of a
/// circle, turned by `turn`.
fn spinner(f: &mut Frame, at: Point, colour: Color, turn: f32) {
    let s = SPINNER / 24.0;
    let arc = Path::new(|b| {
        b.arc(canvas::path::Arc {
            center: Point::new(at.x + 12.0 * s, at.y + 12.0 * s),
            radius: 9.0 * s,
            start_angle: Radians(turn),
            end_angle: Radians(turn + 288.0_f32.to_radians()),
        });
    });
    f.stroke(&arc, stroke(3.0 * s, colour));
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

/// What a row says at its right, before its lock.
#[derive(Debug, Clone, Copy)]
struct Note {
    text: &'static str,
    colour: Color,
    /// With a spinner before it.
    spinner: bool,
}

/// `row`'s note: "Connexion…" while it joins, "dans les Réglages" when the
/// popover cannot join it, "échec" when it failed, "enregistré" when saved.
fn row_note(
    words: Words,
    row: &Wifi,
    unfolded: bool,
    busy: Option<&[u8]>,
    failed: Option<&[u8]>,
) -> Option<Note> {
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
    Some(Note {
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
            state.width(text, size, bold);
        }
        if state.label.is_empty() {
            let mut x = LEFT;
            state.label = words
                .networks()
                .chars()
                .map(|ch| {
                    let at = x;
                    x += measure(&ch.to_string(), LABEL, MANROPE_BOLD) + LABEL_SPACING;
                    (ch, at)
                })
                .collect();
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
                let mut room = rect.width - 2.0 * ROW_PAD - ICON - ICON_GAP;
                if row.security.locked() {
                    room -= LOCK + ICON_GAP;
                }
                let open = unfolded.as_ref() == Some(&row.ssid);
                if let Some(note) = row_note(words, row, open, busy.as_deref(), failed.as_deref()) {
                    room -= state.known(note.text, SMALL, false) + ICON_GAP;
                    if note.spinner && self.motion {
                        room -= 6.0 + SPINNER;
                    }
                }
                state.fit(&row.name, room, MANROPE)
            })
            .collect();
        if let Some(current) = state.network.current.clone() {
            let disconnect = state.known(words.disconnect(), STATUS, false) + 2.0 * SMALL_PAD;
            let room = RIGHT - disconnect - ICON_GAP - (LEFT + ICON + ICON_GAP);
            state.current_name = state.fit(&current.name, room, MANROPE_SEMIBOLD);
        }
        state.shown_width = ui(|ui| {
            if ui.shown && !ui.password.is_empty() {
                measure(ui.password.as_str(), FIELD_TEXT, MANROPE)
            } else {
                0.0
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_error_wraps_between_its_words() {
        let width = |t: &str| t.chars().count() as f32 * 6.0;
        assert_eq!(
            wrap(
                "Mot de passe incorrect. Vérifie-le et réessaie.",
                150.0,
                width
            ),
            ["Mot de passe incorrect.", "Vérifie-le et réessaie."]
        );
        assert_eq!(wrap("Short.", 214.0, width), ["Short."]);
        assert_eq!(wrap("a b", 1.0, width), ["a", "b"]);
    }

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
        assert_eq!(layout.list().map(|(v, _)| v.height), Some(LIST));
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
