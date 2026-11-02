//! The notifications' history (5.4, variant B: by time), opened from the
//! bell on the bar: 360 px wide, 600 px tall at most, its bottom on the
//! bar's. In this order:
//!
//! - "Notifications" and, when there is something to clear, "Tout effacer"
//!   / "Clear all", a ghost button;
//! - the do not disturb card: a moon, its name and what it does now, a
//!   switch; on, amber, with three chips: for an hour (the default), until
//!   the next morning (8:00), with no limit;
//! - the list, newest first, under "RÉCENTES" (under an hour), "PLUS TÔT
//!   AUJOURD'HUI", "HIER" and "PLUS ANCIENNES": each its application's
//!   28 px icon, "App · il y a 9 min", a blue dot while not seen, its
//!   title on a line, its body on two at most, ✕ under the pointer, and
//!   its actions while its sender still holds it. A click invokes its
//!   default (if it is held) and raises its application's window, then it
//!   goes and the history closes; ✕ takes it out.
//! - Empty: the resting panda, "Rien de neuf" and "Tout est lu.".
//!
//! "Clear all" slides the items out to the right one after another, 35 ms
//! apart, in 180 ms each (✕ slides its own); without motion they go at once.
//! The list scrolls with the wheel; `Tab` goes through the buttons, the
//! chips and the items (each with its ✕ and its actions), the arrows through
//! the items, `Enter` or `Space` acts, `Esc` closes.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use iced::{
    Color, Font, Point, Rectangle, Renderer, Size, Theme, Vector,
    advanced::svg,
    keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke},
        text::Alignment,
    },
    window,
};
use jiff::Zoned;

use super::{
    Event as Seen, Item, Pressed, event,
    kit::{self, Clip, Input, Pointer, Switch, Wheel, text, text_in},
};
use crate::{
    clock::{motion::bezier, time::Lang},
    levels::{
        draw::{DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb, ring},
        icons::Icon,
    },
    notifications::{
        self,
        dnd::{Choice, Dnd},
        history::{self, Ago, Kept, Section},
        stack::Input as Said,
    },
    popover::kit::MANROPE_SEMIBOLD,
};

// --- The mockup's sizes, at ×1 --------------------------------------------------

pub const WIDTH: u32 = 360;
const W: f32 = WIDTH as f32;
/// The most it grows to.
const MOST: f32 = 600.0;
const BORDER: f32 = 1.0;
const RADIUS: f32 = 14.0;
/// The header: 14 px in, 10 px under; its title 15 px bold, "Clear all"
/// a 28 px ghost button, 10 px in, 12.5 px 600.
const SIDE: f32 = 14.0;
const HEADER_TOP: f32 = BORDER + 14.0;
const HEADER: f32 = 28.0;
const HEADER_BOTTOM: f32 = 10.0;
const TITLE: f32 = 15.0;
const GHOST_TEXT: f32 = 12.5;
const GHOST_PAD: f32 = 10.0;
const GHOST_RADIUS: f32 = 8.0;
/// The do not disturb card: 14 px from the sides, 10 px and 12 px in,
/// radius 12, a 1 px rim; its moon 18 px, 10 px before its words (13.5 px
/// 600 over 11.5 px, 1 px apart) and after them its switch, 38 × 22, its
/// knob 16 px 3 px in. The chips 8 px under: 26 px, 10 px in, radius 13,
/// 12 px 600, 6 px apart.
const CARD_LEFT: f32 = BORDER + SIDE;
const CARD_RIGHT: f32 = W - BORDER - SIDE;
const CARD_PAD: (f32, f32) = (10.0, 12.0);
const CARD_RADIUS: f32 = 12.0;
const MOON: f32 = 18.0;
const CARD_GAP: f32 = 10.0;
const DND_NAME: f32 = 13.5;
const DND_SAID: f32 = 11.5;
const SAID_GAP: f32 = 1.0;
const SWITCH: Size = Size::new(38.0, 22.0);
const KNOB: f32 = 16.0;
const CHIPS_GAP: f32 = 8.0;
const CHIP: f32 = 26.0;
const CHIP_PAD: f32 = 10.0;
const CHIP_GAP: f32 = 6.0;
const CHIP_TEXT: f32 = 12.0;
/// The list: 8 px in, 10 px under; its sections' labels 8 px, 8 px and
/// 2 px in, 11 px bold, spaced 0.08 em; everything 2 px apart.
const LIST_TOP: f32 = 8.0;
const LIST_SIDE: f32 = BORDER + 8.0;
const LIST_BOTTOM: f32 = 10.0;
const HEAD_PAD: (f32, f32) = (8.0, 2.0);
const HEAD: f32 = 11.0;
const HEAD_SPACING: f32 = 0.08 * HEAD;
const GAP: f32 = 2.0;
/// An item: 8 px in, 30 px on its right; its icon 28 px, 10 px before its
/// text: the first line 11 px, its title 13 px bold, its body 12 px on 1.4
/// lines, two at most, 2 px apart; its ✕ 22 px, 6 px from its right and
/// 8 px from its top; its actions 26 px, 6 px under it, 6 px apart.
const ITEM_PAD: f32 = 8.0;
const ITEM_RIGHT: f32 = 30.0;
const ITEM_RADIUS: f32 = 10.0;
const ICON: f32 = 28.0;
const ICON_GAP: f32 = 10.0;
const META: f32 = 11.0;
const META_GAP: f32 = 6.0;
const DOT: f32 = 6.0;
const ITEM_TITLE: f32 = 13.0;
const BODY: f32 = 12.0;
const BODY_LINE: f32 = 1.4;
const BODY_LINES: usize = 2;
const TEXT_GAP: f32 = 2.0;
const CLOSE: f32 = 22.0;
const CLOSE_IN: (f32, f32) = (6.0, 8.0);
const CLOSE_RADIUS: f32 = 6.0;
const ACTION: f32 = 26.0;
const ACTION_GAP: f32 = 6.0;
/// The empty state: the panda 96 px, its words 8 px apart.
const EMPTY: f32 = 220.0;
const PANDA: f32 = 96.0;
const EMPTY_GAP: f32 = 8.0;
const NOTHING: f32 = 14.0;
const ALL_READ: f32 = 12.0;
/// A notch of the wheel scrolls this far.
const NOTCH: f32 = 40.0;

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, a)
}

/// The bubbles' tint (the mockup's), opaque without glass.
const GLASS_TOP: Color = rgba(22, 27, 42, 0.95);
const GLASS_BOTTOM: Color = rgba(14, 17, 28, 0.97);
const OPAQUE_TOP: Color = rgb(0x161B2A);
const OPAQUE_BOTTOM: Color = rgb(0x0E111C);
const RIM: Color = rgba(255, 255, 255, 0.10);
const GHOST_HOVER: Color = rgba(255, 255, 255, 0.08);
const ITEM_HOVER: Color = rgba(255, 255, 255, 0.05);
const CARD_OFF: (Color, Color) = (rgba(255, 255, 255, 0.04), rgba(255, 255, 255, 0.08));
const CARD_ON: (Color, Color) = (rgba(245, 196, 110, 0.08), rgba(245, 196, 110, 0.35));
const SWITCH_ON: Color = rgb(0xF5B44A);
const SWITCH_OFF: Color = rgba(255, 255, 255, 0.18);
const CHIP_ON: (Color, Color, Color) = (
    rgba(245, 196, 110, 0.18),
    rgba(245, 196, 110, 0.6),
    rgb(0xF5C46E),
);
const CHIP_OFF: (Color, Color, Color) = (
    rgba(255, 255, 255, 0.05),
    rgba(255, 255, 255, 0.14),
    rgb(0xC9D0DC),
);
const QUIET: Color = rgb(0x7C8698);
const UNSEEN: Color = rgb(0x8FB3FF);
const NOTHING_INK: Color = rgb(0xC9D0DC);
const ACTION_FILL: Color = rgba(255, 255, 255, 0.06);
const ACTION_HOVER: Color = rgba(255, 255, 255, 0.12);
const ACTION_RIM: Color = rgba(255, 255, 255, 0.14);
/// The launcher's letter tile for an application without an icon.
const NEUTRAL: (Color, Color) = (rgb(0x5C6576), rgb(0x9AA3B4));

/// "Clear all": the items go 35 ms apart, each in 180 ms, `ease-in`.
const STAGGER: Duration = Duration::from_millis(35);
const SLIDE: Duration = Duration::from_millis(180);
const SLIDE_BY: f32 = 40.0;
const EASE_IN: (f32, f32, f32, f32) = (0.42, 0.0, 1.0, 1.0);
/// The empty state comes in over 260 ms, 8 px up, `ease-out`.
const REST_IN: Duration = Duration::from_millis(260);
const EASE_OUT: (f32, f32, f32, f32) = (0.0, 0.0, 0.58, 1.0);

// --- Words ----------------------------------------------------------------------

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

    fn pick(self, fr: &'static str, en: &'static str) -> &'static str {
        if self.fr { fr } else { en }
    }

    fn title(self) -> &'static str {
        "Notifications"
    }

    fn clear(self) -> &'static str {
        self.pick("Tout effacer", "Clear all")
    }

    fn dnd(self) -> &'static str {
        self.pick("Ne pas déranger", "Do not disturb")
    }

    fn chip(self, choice: Choice) -> &'static str {
        match choice {
            Choice::Hour => self.pick("Pendant 1\u{a0}h", "For 1 hour"),
            Choice::Morning => self.pick("Jusqu’à demain matin", "Until tomorrow morning"),
            Choice::Always => self.pick("Sans limite", "No limit"),
        }
    }

    /// A time of day as the user reads it: "15 h 32", "8 h"; "3:32 PM",
    /// "8 AM".
    fn time(self, at: &Zoned) -> String {
        let (h, m) = (at.hour(), at.minute());
        if self.fr {
            if m == 0 {
                format!("{h}\u{a0}h")
            } else {
                format!("{h}\u{a0}h\u{a0}{m:02}")
            }
        } else {
            let half = if h < 12 { "AM" } else { "PM" };
            let h12 = match h % 12 {
                0 => 12,
                h => h,
            };
            if m == 0 {
                format!("{h12}\u{a0}{half}")
            } else {
                format!("{h12}:{m:02}\u{a0}{half}")
            }
        }
    }

    /// What do not disturb does now, at `now`.
    fn said(self, dnd: Dnd, now: &Zoned) -> String {
        let Dnd::On { choice, until } = dnd else {
            return self
                .pick("Les bulles s’affichent", "Bubbles show")
                .to_owned();
        };
        let until = until.and_then(|u| notifications::dnd::zoned(u.wall, now));
        let when = match (choice, until) {
            (Choice::Always, _) | (_, None) => self
                .pick("Jusqu’à ce que tu l’arrêtes", "Until you turn it off")
                .to_owned(),
            (_, Some(at)) => {
                let tomorrow = now.date().tomorrow().ok() == Some(at.date());
                match (self.fr, tomorrow) {
                    (true, true) => format!("Jusqu’à demain, {}", self.time(&at)),
                    (true, false) => format!("Jusqu’à {}", self.time(&at)),
                    (false, true) => format!("Until tomorrow, {}", self.time(&at)),
                    (false, false) => format!("Until {}", self.time(&at)),
                }
            }
        };
        format!(
            "{when} · {}",
            self.pick(
                "seules les urgences passent",
                "only urgent ones come through"
            )
        )
    }

    fn section(self, section: Section) -> &'static str {
        match section {
            Section::Recent => self.pick("RÉCENTES", "RECENT"),
            Section::Today => self.pick("PLUS TÔT AUJOURD’HUI", "EARLIER TODAY"),
            Section::Yesterday => self.pick("HIER", "YESTERDAY"),
            Section::Older => self.pick("PLUS ANCIENNES", "OLDER"),
        }
    }

    fn ago(self, ago: Ago) -> String {
        match (ago, self.fr) {
            (Ago::Now, true) => "maintenant".to_owned(),
            (Ago::Now, false) => "now".to_owned(),
            (Ago::Minutes(n), true) => format!("il y a {n}\u{a0}min"),
            (Ago::Minutes(n), false) => format!("{n}\u{a0}min ago"),
            (Ago::Hours(n), true) => format!("il y a {n}\u{a0}h"),
            (Ago::Hours(n), false) => format!("{n}\u{a0}h ago"),
            (Ago::Yesterday, true) => "hier".to_owned(),
            (Ago::Yesterday, false) => "yesterday".to_owned(),
            (Ago::Days(n), true) => format!("il y a {n}\u{a0}jours"),
            (Ago::Days(n), false) => format!("{n}\u{a0}days ago"),
        }
    }

    fn nothing(self) -> &'static str {
        self.pick("Rien de neuf", "Nothing new")
    }

    fn all_read(self) -> &'static str {
        self.pick("Tout est lu.", "All read.")
    }
}

// --- Laying it out --------------------------------------------------------------

/// What can be acted on in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Clear,
    Switch,
    Chip(Choice),
    Item(u64),
    Close(u64),
    Action(u64, usize),
}

impl Target {
    /// The item it belongs to, if it is an item's.
    fn key(self) -> Option<u64> {
        match self {
            Self::Item(k) | Self::Close(k) | Self::Action(k, _) => Some(k),
            _ => None,
        }
    }
}

/// An item's words, cut to fit once: they change only with it.
#[derive(Debug, Clone, PartialEq)]
struct Cut {
    app: String,
    app_width: f32,
    title: String,
    body: Vec<String>,
    actions: Vec<(String, String, f32)>,
}

/// An item, laid out: its place in the list's content (from its top).
#[derive(Debug, Clone)]
struct Row {
    key: u64,
    rect: Rectangle,
    icon: Option<Icon>,
    letter: String,
    cut: Cut,
    when: String,
    when_x: f32,
    dot_x: Option<f32>,
    actions: Vec<(String, Rectangle)>,
    section: Section,
}

impl Row {
    fn close(&self) -> Rectangle {
        Rectangle::new(
            Point::new(
                self.rect.x + self.rect.width - CLOSE_IN.0 - CLOSE,
                self.rect.y + CLOSE_IN.1,
            ),
            Size::new(CLOSE, CLOSE),
        )
    }
}

/// The history, laid out.
#[derive(Debug, Clone)]
struct Laid {
    clear: Option<(Rectangle, f32)>,
    card: Rectangle,
    row_middle: f32,
    said: Vec<String>,
    said_top: f32,
    switch: Rectangle,
    chips: Vec<(Choice, Rectangle)>,
    dnd: Dnd,
    /// The list's viewport, and its content's height.
    view: Rectangle,
    content: f32,
    heads: Vec<(Section, f32)>,
    rows: Vec<Row>,
    height: f32,
}

/// What it shows, as the stack has it.
#[derive(Debug, Clone)]
struct Shown {
    items: Vec<Kept>,
    dnd: Dnd,
    version: u64,
}

fn shown() -> Shown {
    notifications::with(|c| Shown {
        items: c.stack.history.items().to_vec(),
        dnd: c.stack.dnd,
        version: c.stack.history.version(),
    })
}

/// A text's width at a size in a font.
type Measure<'a> = &'a mut dyn FnMut(&str, f32, Font) -> f32;

/// The text column's room in an item.
const COLUMN: f32 = W - 2.0 * LIST_SIDE - ITEM_PAD - ICON - ICON_GAP - ITEM_RIGHT;

/// `kept`'s words cut to fit.
fn cut(kept: &Kept, measure: Measure<'_>) -> Cut {
    let note = &kept.note;
    let app = kit::elide(note.app.trim(), COLUMN * 0.5, |t| {
        measure(t, META, MANROPE_BOLD)
    });
    let app_width = measure(&app, META, MANROPE_BOLD);
    let title = kit::elide(note.summary.trim(), COLUMN, |t| {
        measure(t, ITEM_TITLE, MANROPE_BOLD)
    });
    let mut body: Vec<String> = note
        .body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .flat_map(|l| kit::wrap(l, COLUMN, |t| measure(t, BODY, MANROPE)))
        .collect();
    let more = body.len() > BODY_LINES;
    body.truncate(BODY_LINES);
    let count = body.len();
    for (i, line) in body.iter_mut().enumerate() {
        let shown = if i + 1 == count && more {
            format!("{line}…")
        } else {
            line.clone()
        };
        *line = kit::elide(&shown, COLUMN, |t| measure(t, BODY, MANROPE));
    }
    let actions = note
        .actions
        .iter()
        .map(|(key, label)| {
            let label = kit::elide(label, COLUMN - 2.0 * CHIP_PAD, |t| {
                measure(t, CHIP_TEXT, MANROPE_SEMIBOLD)
            });
            let width = (measure(&label, CHIP_TEXT, MANROPE_SEMIBOLD) + 2.0 * CHIP_PAD).ceil();
            (key.clone(), label, width)
        })
        .collect();
    Cut {
        app,
        app_width,
        title,
        body,
        actions,
    }
}

/// The items' words as cut, by key and arrival: an item's change comes as
/// a new arrival.
type Cuts = HashMap<(u64, i64, String), Cut>;

static CUTS: Mutex<Option<Cuts>> = Mutex::new(None);

/// `shown` laid out at `now`, in `words`.
fn lay_out(
    shown: &Shown,
    words: Words,
    now: &Zoned,
    cuts: &mut Cuts,
    measure: Measure<'_>,
) -> Laid {
    // The header.
    let clear = (!shown.items.is_empty()).then(|| {
        let width = measure(words.clear(), GHOST_TEXT, MANROPE_SEMIBOLD);
        let w = width + 2.0 * GHOST_PAD;
        (
            Rectangle::new(
                Point::new(W - BORDER - SIDE - w, HEADER_TOP),
                Size::new(w, HEADER),
            ),
            width,
        )
    });
    // The card.
    let card_top = HEADER_TOP + HEADER + HEADER_BOTTOM;
    let inner_left = CARD_LEFT + BORDER + CARD_PAD.1;
    let inner_right = CARD_RIGHT - BORDER - CARD_PAD.1;
    let words_left = inner_left + MOON + CARD_GAP;
    let room = inner_right - SWITCH.width - CARD_GAP - words_left;
    let said = kit::wrap(&words.said(shown.dnd, now), room, |t| {
        measure(t, DND_SAID, MANROPE)
    });
    let column = DND_NAME * LINE + SAID_GAP + said.len() as f32 * DND_SAID * LINE;
    let row_top = card_top + BORDER + CARD_PAD.0;
    let row = column.max(SWITCH.height).max(MOON);
    let row_middle = row_top + row / 2.0;
    let said_top = row_middle - column / 2.0 + DND_NAME * LINE + SAID_GAP;
    let switch = Rectangle::new(
        Point::new(inner_right - SWITCH.width, row_middle - SWITCH.height / 2.0),
        SWITCH,
    );
    let mut y = row_top + row;
    let mut chips = Vec::new();
    if shown.dnd.is_on() {
        y += CHIPS_GAP;
        let mut x = inner_left;
        for choice in Choice::ALL {
            let width =
                (measure(words.chip(choice), CHIP_TEXT, MANROPE_SEMIBOLD) + 2.0 * CHIP_PAD).ceil();
            if x > inner_left && x + width > inner_right {
                x = inner_left;
                y += CHIP + CHIP_GAP;
            }
            chips.push((
                choice,
                Rectangle::new(Point::new(x, y), Size::new(width, CHIP)),
            ));
            x += width + CHIP_GAP;
        }
        y += CHIP;
    }
    let card_bottom = y + CARD_PAD.0 + BORDER;
    let card = Rectangle::new(
        Point::new(CARD_LEFT, card_top),
        Size::new(CARD_RIGHT - CARD_LEFT, card_bottom - card_top),
    );
    // The list, from its content's top.
    let list_top = card_bottom + LIST_TOP;
    let width = W - 2.0 * LIST_SIDE;
    let mut heads = Vec::new();
    let mut rows = Vec::new();
    let mut y = 0.0;
    let mut section = None;
    let mut first = true;
    for kept in &shown.items {
        let here = history::section(kept.note.at, now);
        if section != Some(here) {
            if !first {
                y += GAP;
            }
            heads.push((here, y + HEAD_PAD.0 + HEAD * LINE / 2.0));
            y += HEAD_PAD.0 + HEAD * LINE + HEAD_PAD.1;
            section = Some(here);
        }
        first = false;
        y += GAP;
        let id = (kept.key, kept.note.at, kept.note.summary.clone());
        let cut = match cuts.get(&id) {
            Some(cut) => cut.clone(),
            None => {
                let made = cut(kept, measure);
                cuts.insert(id, made.clone());
                made
            }
        };
        let when = words.ago(history::ago(kept.note.at, now));
        let x = LIST_SIDE + ITEM_PAD + ICON + ICON_GAP;
        let middot = measure("·", META, MANROPE);
        let when_x = x + cut.app_width + META_GAP + middot + META_GAP;
        let unseen = kept.seen == history::Sight::Unseen;
        let dot_x = unseen.then(|| when_x + measure(&when, META, MANROPE) + META_GAP);
        let mut column = META * LINE + TEXT_GAP + ITEM_TITLE * LINE;
        if !cut.body.is_empty() {
            column += TEXT_GAP + cut.body.len() as f32 * BODY * BODY_LINE;
        }
        // Its actions while its sender holds it.
        let mut actions = Vec::new();
        if kept.live.is_some() && !cut.actions.is_empty() {
            let mut ax = x;
            let mut ay = y + ITEM_PAD + column + ACTION_GAP;
            for (key, _, w) in &cut.actions {
                if ax > x && ax + w > x + COLUMN {
                    ax = x;
                    ay += ACTION + ACTION_GAP;
                }
                actions.push((
                    key.clone(),
                    Rectangle::new(Point::new(ax, ay), Size::new(*w, ACTION)),
                ));
                ax += w + ACTION_GAP;
            }
            column = actions
                .last()
                .map_or(column, |(_, r)| r.y + r.height - y - ITEM_PAD);
        }
        let height = 2.0 * ITEM_PAD + column.max(ICON);
        let letter = kept
            .note
            .app
            .chars()
            .find(|c| c.is_alphanumeric())
            .map(|c| c.to_uppercase().collect::<String>())
            .unwrap_or_default();
        rows.push(Row {
            key: kept.key,
            rect: Rectangle::new(Point::new(LIST_SIDE, y), Size::new(width, height)),
            icon: kept.note.icon.clone(),
            letter,
            cut,
            when,
            when_x,
            dot_x,
            actions,
            section: here,
        });
        y += height;
    }
    let content = if rows.is_empty() { EMPTY } else { y };
    let room = MOST - list_top - LIST_BOTTOM - BORDER;
    let view = Rectangle::new(
        Point::new(LIST_SIDE, list_top),
        Size::new(width, content.min(room)),
    );
    let height = (list_top + view.height + LIST_BOTTOM + BORDER).round();
    Laid {
        clear,
        card,
        row_middle,
        said,
        said_top,
        switch,
        chips,
        dnd: shown.dnd,
        view,
        content,
        heads,
        rows,
        height,
    }
}

impl Laid {
    fn most_scroll(&self) -> f32 {
        (self.content - self.view.height).max(0.0)
    }

    /// Row `row`'s rectangle on the popover, scrolled by `scroll`.
    fn on_popover(&self, rect: Rectangle, scroll: f32) -> Rectangle {
        Rectangle::new(
            Point::new(rect.x, self.view.y + rect.y - scroll),
            rect.size(),
        )
    }

    /// What can be acted on, in the order `Tab` goes, and where (on the
    /// popover, scrolled by `scroll`).
    fn targets(&self, scroll: f32) -> Vec<(Target, Rectangle)> {
        let mut targets = Vec::new();
        if let Some((rect, _)) = self.clear {
            targets.push((Target::Clear, rect));
        }
        targets.push((Target::Switch, self.switch));
        targets.extend(self.chips.iter().map(|(c, r)| (Target::Chip(*c), *r)));
        for row in &self.rows {
            targets.push((Target::Item(row.key), self.on_popover(row.rect, scroll)));
            targets.push((Target::Close(row.key), self.on_popover(row.close(), scroll)));
            for (i, (_, rect)) in row.actions.iter().enumerate() {
                targets.push((Target::Action(row.key, i), self.on_popover(*rect, scroll)));
            }
        }
        targets
    }

    /// What is under `at`, scrolled by `scroll`: the ✕ and the actions over
    /// their item; in the list only within its viewport.
    fn target_at(&self, at: Point, scroll: f32) -> Option<Target> {
        let in_list = self.view.contains(at);
        let mut found = None;
        for (target, rect) in self.targets(scroll) {
            if !rect.contains(at) || (target.key().is_some() && !in_list) {
                continue;
            }
            match (found, target) {
                // The ✕ or an action over its item.
                (Some(Target::Item(_)), Target::Close(_) | Target::Action(..)) | (None, _) => {
                    found = Some(target);
                }
                _ => {}
            }
        }
        found
    }

    /// For the log: the sections and their items, newest first.
    fn said(&self, words: Words) -> String {
        if self.rows.is_empty() {
            return words.nothing().to_owned();
        }
        let mut said: Vec<String> = Vec::new();
        let mut section = None;
        for row in &self.rows {
            if section != Some(row.section) {
                said.push(format!("{}:", words.section(row.section)));
                section = Some(row.section);
            }
            let seen = if row.dot_x.is_some() { " (unseen)" } else { "" };
            said.push(format!(
                "{} «{}» {}{seen}{}",
                row.cut.app,
                row.cut.title,
                row.when,
                if row.actions.is_empty() {
                    String::new()
                } else {
                    format!(" [{} actions]", row.actions.len())
                }
            ));
        }
        said.join(" ")
    }
}

/// Its height, as it shows now.
pub fn height() -> u32 {
    let shown = shown();
    let mut cuts = CUTS.lock().unwrap_or_else(PoisonError::into_inner);
    let cuts = cuts.get_or_insert_with(HashMap::new);
    let laid = lay_out(
        &shown,
        Words::new(Lang::of_messages()),
        &notifications::wall(),
        cuts,
        &mut kit::measure,
    );
    laid.height as u32
}

// --- The panel ------------------------------------------------------------------

/// The history on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    pub id: window::Id,
    pub glass: bool,
    pub motion: bool,
    pub software: bool,
}

/// What its surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    laid: Option<Laid>,
    /// What it was laid out from: the history's version, the minute, do not
    /// disturb.
    from: Option<(u64, i64, Dnd)>,
    pointer: Pointer<Target>,
    focus: Option<Target>,
    scroll: f32,
    wheel: Wheel,
    switch: Switch,
    /// Items sliding out.
    leaving: Option<Leaving>,
    /// Since when it is empty, for the panda's coming in.
    empty_since: Option<Instant>,
    now: Option<Instant>,
}

/// Items sliding out, in their order, since then; once they are all out,
/// the stack is told (`sent`), and they stay out until it takes them.
#[derive(Debug)]
struct Leaving {
    keys: Vec<u64>,
    since: Instant,
    sent: bool,
}

impl State {
    fn targets(&self) -> Vec<(Target, Rectangle)> {
        self.laid
            .as_ref()
            .map_or_else(Vec::new, |l| l.targets(self.scroll))
    }

    /// How far item `key` has slid out at `now`, 0 to 1.
    fn gone(&self, key: u64, now: Instant) -> f32 {
        let Some(Leaving { keys, since, .. }) = &self.leaving else {
            return 0.0;
        };
        let Some(i) = keys.iter().position(|k| *k == key) else {
            return 0.0;
        };
        let start = *since + STAGGER * u32::try_from(i.min(40)).unwrap_or(40);
        let t = now.saturating_duration_since(start).as_secs_f32() / SLIDE.as_secs_f32();
        bezier(EASE_IN, t.clamp(0.0, 1.0))
    }

    /// When the items sliding out are all gone.
    fn leaving_end(&self) -> Option<Instant> {
        let Leaving { keys, since, .. } = self.leaving.as_ref()?;
        let shown = u32::try_from(keys.len().clamp(1, 41) - 1).unwrap_or(40);
        Some(*since + STAGGER * shown + SLIDE)
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
        match kit::input(self.id, event, bounds, cursor, &mut state.pointer) {
            Input::Redraw(now) => self.refresh(state, now),
            Input::Mouse(seen, at) => self.mouse(state, seen, at),
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
        self.background(&mut frame, bounds.size());
        let Some(laid) = &state.laid else {
            return vec![frame.into_geometry()];
        };
        let words = Words::new(Lang::of_messages());
        let now = state.now.unwrap_or_else(Instant::now);
        let hover = state.pointer.hover;
        self.header(&mut frame, laid, words, hover);
        self.card(&mut frame, laid, words, state, now);
        let mut list = Frame::with_bounds(renderer, laid.view);
        let clip = Clip::new(self.software, Some(laid.view));
        if laid.rows.is_empty() {
            self.empty(&mut list, laid, words, state, now);
        }
        for (section, middle) in &laid.heads {
            let middle = laid.view.y + middle - state.scroll;
            if middle < laid.view.y - HEAD || middle > laid.view.y + laid.view.height + HEAD {
                continue;
            }
            head(&mut list, clip, words.section(*section), middle);
        }
        for row in &laid.rows {
            let rect = laid.on_popover(row.rect, state.scroll);
            if rect.y > laid.view.y + laid.view.height || rect.y + rect.height < laid.view.y {
                continue;
            }
            let gone = state.gone(row.key, now);
            self.row(&mut list, clip, laid, row, state, gone);
        }
        if laid.content > laid.view.height + 0.5 {
            let list = kit::list(laid.view.y, laid.view.height, laid.content);
            list.scrollbar_at(&mut frame, state.scroll, W - BORDER - kit::SCROLLBAR - 2.0);
        }
        if let Some((target, rect)) = state
            .focus
            .and_then(|f| state.targets().into_iter().find(|(t, _)| *t == f))
        {
            let radius = match target {
                Target::Item(_) => ITEM_RADIUS,
                Target::Close(_) => CLOSE_RADIUS,
                Target::Chip(_) => CHIP / 2.0,
                Target::Switch => SWITCH.height / 2.0,
                Target::Clear => GHOST_RADIUS,
                Target::Action(..) => 8.0,
            };
            ring(&mut frame, rect, radius);
        }
        vec![frame.into_geometry(), list.into_geometry()]
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

/// `colour` at `alpha` of its own.
fn faded(colour: Color, alpha: f32) -> Color {
    colour.scale_alpha(alpha)
}

/// A section's label, spaced, its line centred on `middle`.
fn head(f: &mut Frame, clip: Clip, word: &str, middle: f32) {
    let mut x = LIST_SIDE + HEAD_PAD.0;
    for ch in word.chars() {
        let letter = ch.to_string();
        let width = kit::measure(&letter, HEAD, MANROPE_BOLD);
        clip.fill(
            f,
            text_in(
                MANROPE_BOLD,
                text(letter, Point::new(x, middle), HEAD, QUIET, Alignment::Left),
            ),
        );
        x += width + HEAD_SPACING;
    }
}

impl Panel {
    fn background(&self, f: &mut Frame, size: Size) {
        let (top, bottom) = if self.glass {
            (GLASS_TOP, GLASS_BOTTOM)
        } else {
            (OPAQUE_TOP, OPAQUE_BOTTOM)
        };
        let shape = Path::rounded_rectangle(Point::ORIGIN, size, RADIUS.into());
        let tint = canvas::gradient::Linear::new(Point::ORIGIN, Point::new(0.0, size.height))
            .add_stop(0.0, top)
            .add_stop(1.0, bottom);
        f.fill(&shape, canvas::Fill::from(tint));
        f.stroke(
            &Path::rounded_rectangle(
                Point::new(BORDER / 2.0, BORDER / 2.0),
                Size::new(size.width - BORDER, size.height - BORDER),
                (RADIUS - BORDER / 2.0).into(),
            ),
            Stroke {
                style: canvas::Style::Solid(RIM),
                width: BORDER,
                ..Stroke::default()
            },
        );
    }

    fn header(&self, f: &mut Frame, laid: &Laid, words: Words, hover: Option<Target>) {
        let middle = HEADER_TOP + HEADER / 2.0;
        f.fill_text(text_in(
            MANROPE_BOLD,
            text(
                words.title().to_owned(),
                Point::new(BORDER + SIDE, middle),
                TITLE,
                TEXT,
                Alignment::Left,
            ),
        ));
        if let Some((rect, _)) = laid.clear {
            let lit = hover == Some(Target::Clear);
            if lit {
                f.fill(&kit::rounded(rect, GHOST_RADIUS), GHOST_HOVER);
            }
            f.fill_text(text_in(
                MANROPE_SEMIBOLD,
                text(
                    words.clear().to_owned(),
                    rect.center(),
                    GHOST_TEXT,
                    if lit { TEXT } else { DIM },
                    Alignment::Center,
                ),
            ));
        }
    }

    fn card(&self, f: &mut Frame, laid: &Laid, words: Words, state: &State, now: Instant) {
        let on = laid.dnd.is_on();
        let (fill, rim) = if on { CARD_ON } else { CARD_OFF };
        f.fill(&kit::rounded(laid.card, CARD_RADIUS), fill);
        f.stroke(
            &Path::rounded_rectangle(
                Point::new(laid.card.x + 0.5, laid.card.y + 0.5),
                Size::new(laid.card.width - 1.0, laid.card.height - 1.0),
                (CARD_RADIUS - 0.5).into(),
            ),
            Stroke {
                style: canvas::Style::Solid(rim),
                width: BORDER,
                ..Stroke::default()
            },
        );
        let inner_left = CARD_LEFT + BORDER + CARD_PAD.1;
        f.draw_svg(
            Rectangle::new(
                Point::new(inner_left, laid.row_middle - MOON / 2.0),
                Size::new(MOON, MOON),
            ),
            svg::Svg::new(moon(on)),
        );
        let x = inner_left + MOON + CARD_GAP;
        let column = laid.said_top - SAID_GAP - DND_NAME * LINE;
        f.fill_text(text_in(
            MANROPE_SEMIBOLD,
            text(
                words.dnd().to_owned(),
                Point::new(x, column + DND_NAME * LINE / 2.0),
                DND_NAME,
                TEXT,
                Alignment::Left,
            ),
        ));
        for said in kit::lines(&laid.said, (x, laid.said_top), DND_SAID, LINE, FAINT) {
            f.fill_text(said);
        }
        // The switch, its knob sliding.
        let shown = state.switch.shown(on, now);
        let p = state.switch.knob_at(shown, now, self.motion);
        let rect = laid.switch;
        f.fill(
            &kit::rounded(rect, SWITCH.height / 2.0),
            mix(SWITCH_OFF, SWITCH_ON, p),
        );
        let centre = Point::new(
            rect.x + 3.0 + (SWITCH.width - KNOB - 6.0) * p + KNOB / 2.0,
            rect.center_y(),
        );
        f.fill(&Path::circle(centre, KNOB / 2.0), Color::WHITE);
        let chosen = laid.dnd.choice();
        for (choice, rect) in &laid.chips {
            let (fill, rim, ink) = if chosen == Some(*choice) {
                CHIP_ON
            } else {
                CHIP_OFF
            };
            let fill =
                if state.pointer.hover == Some(Target::Chip(*choice)) && chosen != Some(*choice) {
                    ACTION_HOVER
                } else {
                    fill
                };
            f.fill(&kit::rounded(*rect, CHIP / 2.0), fill);
            f.stroke(
                &Path::rounded_rectangle(
                    Point::new(rect.x + 0.5, rect.y + 0.5),
                    Size::new(rect.width - 1.0, rect.height - 1.0),
                    (CHIP / 2.0 - 0.5).into(),
                ),
                Stroke {
                    style: canvas::Style::Solid(rim),
                    width: 1.0,
                    ..Stroke::default()
                },
            );
            f.fill_text(text_in(
                MANROPE_SEMIBOLD,
                text(
                    words.chip(*choice).to_owned(),
                    rect.center(),
                    CHIP_TEXT,
                    ink,
                    Alignment::Center,
                ),
            ));
        }
    }

    fn empty(&self, f: &mut Frame, laid: &Laid, words: Words, state: &State, now: Instant) {
        let p = match (state.empty_since, self.motion) {
            (Some(since), true) => {
                let t = now.saturating_duration_since(since).as_secs_f32() / REST_IN.as_secs_f32();
                bezier(EASE_OUT, t.clamp(0.0, 1.0))
            }
            _ => 1.0,
        };
        let shift = 8.0 * (1.0 - p);
        let block = PANDA + EMPTY_GAP + NOTHING * LINE + EMPTY_GAP + ALL_READ * LINE;
        let top = laid.view.y + (laid.view.height - block) / 2.0 + shift;
        let centre = W / 2.0;
        // The panda's own proportions, within 96 px.
        let (pw, ph) = (PANDA, PANDA * 120.5 / 126.0);
        f.draw_svg(
            Rectangle::new(
                Point::new(centre - pw / 2.0, top + (PANDA - ph) / 2.0),
                Size::new(pw, ph),
            ),
            svg::Svg::new(panda()).opacity(0.9 * p),
        );
        let y = top + PANDA + EMPTY_GAP + NOTHING * LINE / 2.0;
        f.fill_text(text_in(
            MANROPE_SEMIBOLD,
            text(
                words.nothing().to_owned(),
                Point::new(centre, y),
                NOTHING,
                faded(NOTHING_INK, p),
                Alignment::Center,
            ),
        ));
        let y = y + NOTHING * LINE / 2.0 + EMPTY_GAP + ALL_READ * LINE / 2.0;
        f.fill_text(text(
            words.all_read().to_owned(),
            Point::new(centre, y),
            ALL_READ,
            faded(QUIET, p),
            Alignment::Center,
        ));
    }

    fn row(&self, f: &mut Frame, clip: Clip, laid: &Laid, row: &Row, state: &State, gone: f32) {
        let alpha = 1.0 - gone;
        if alpha <= 0.0 {
            return;
        }
        let shift = Vector::new(SLIDE_BY * gone, 0.0);
        let rect = laid.on_popover(row.rect, state.scroll) + shift;
        let hover = state.pointer.hover;
        let over = hover.and_then(Target::key) == Some(row.key) && state.leaving.is_none();
        if over || state.focus == Some(Target::Item(row.key)) {
            f.fill(&kit::rounded(rect, ITEM_RADIUS), faded(ITEM_HOVER, alpha));
        }
        // The icon, else the application's first letter on a square.
        let icon = Rectangle::new(
            Point::new(rect.x + ITEM_PAD, rect.y + ITEM_PAD),
            Size::new(ICON, ICON),
        );
        match &row.icon {
            Some(Icon::Raster(handle)) => {
                f.draw_image(icon, canvas::Image::new(handle.clone()).opacity(alpha));
            }
            Some(Icon::Vector(handle)) => {
                f.draw_svg(icon, svg::Svg::new(handle.clone()).opacity(alpha));
            }
            None => {
                let radius = (ICON * 0.28).round();
                let fill = canvas::gradient::Linear::new(
                    icon.position(),
                    Point::new(icon.x + ICON, icon.y + ICON),
                )
                .add_stop(0.0, faded(NEUTRAL.0, alpha))
                .add_stop(1.0, faded(NEUTRAL.1, alpha));
                f.fill(&kit::rounded(icon, radius), canvas::Fill::from(fill));
                clip.fill(
                    f,
                    text_in(
                        MANROPE_BOLD,
                        text(
                            row.letter.clone(),
                            icon.center(),
                            ICON * 0.4,
                            faded(Color::WHITE, alpha),
                            Alignment::Center,
                        ),
                    ),
                );
            }
        }
        let x = rect.x + ITEM_PAD + ICON + ICON_GAP;
        let dx = rect.x - row.rect.x;
        let mut y = rect.y + ITEM_PAD;
        let meta = y + META * LINE / 2.0;
        clip.fill(
            f,
            text_in(
                MANROPE_BOLD,
                text(
                    row.cut.app.clone(),
                    Point::new(x, meta),
                    META,
                    faded(DIM, alpha),
                    Alignment::Left,
                ),
            ),
        );
        clip.fill(
            f,
            text(
                "·".to_owned(),
                Point::new(x + row.cut.app_width + META_GAP, meta),
                META,
                faded(QUIET, alpha),
                Alignment::Left,
            ),
        );
        clip.fill(
            f,
            text(
                row.when.clone(),
                Point::new(row.when_x + dx, meta),
                META,
                faded(QUIET, alpha),
                Alignment::Left,
            ),
        );
        if let Some(dot_x) = row.dot_x {
            f.fill(
                &Path::circle(Point::new(dot_x + dx + DOT / 2.0, meta), DOT / 2.0),
                faded(UNSEEN, alpha),
            );
        }
        y += META * LINE + TEXT_GAP;
        clip.fill(
            f,
            text_in(
                MANROPE_BOLD,
                text(
                    row.cut.title.clone(),
                    Point::new(x, y + ITEM_TITLE * LINE / 2.0),
                    ITEM_TITLE,
                    faded(TEXT, alpha),
                    Alignment::Left,
                ),
            ),
        );
        y += ITEM_TITLE * LINE;
        if !row.cut.body.is_empty() {
            y += TEXT_GAP;
            for line in kit::lines(&row.cut.body, (x, y), BODY, BODY_LINE, faded(DIM, alpha)) {
                clip.fill(f, line);
            }
        }
        // Its actions.
        for (i, ((_, rect), (_, label, _))) in row.actions.iter().zip(&row.cut.actions).enumerate()
        {
            let rect = laid.on_popover(*rect, state.scroll) + shift;
            let lit = hover == Some(Target::Action(row.key, i));
            f.fill(
                &kit::rounded(rect, 8.0),
                faded(if lit { ACTION_HOVER } else { ACTION_FILL }, alpha),
            );
            f.stroke(
                &Path::rounded_rectangle(
                    Point::new(rect.x + 0.5, rect.y + 0.5),
                    Size::new(rect.width - 1.0, rect.height - 1.0),
                    7.5.into(),
                ),
                Stroke {
                    style: canvas::Style::Solid(faded(ACTION_RIM, alpha)),
                    width: 1.0,
                    ..Stroke::default()
                },
            );
            clip.fill(
                f,
                text_in(
                    MANROPE_SEMIBOLD,
                    text(
                        label.clone(),
                        rect.center(),
                        CHIP_TEXT,
                        faded(TEXT, alpha),
                        Alignment::Center,
                    ),
                ),
            );
        }
        // Its ✕, under the pointer or holding the keyboard.
        let focused = state.focus == Some(Target::Close(row.key));
        if over || focused {
            let close = laid.on_popover(row.close(), state.scroll) + shift;
            let lit = hover == Some(Target::Close(row.key));
            if lit {
                f.fill(
                    &kit::rounded(close, CLOSE_RADIUS),
                    faded(GHOST_HOVER, alpha),
                );
            }
            let c = close.center();
            let arm = 3.5;
            let cross = Path::new(|p| {
                p.move_to(Point::new(c.x - arm, c.y - arm));
                p.line_to(Point::new(c.x + arm, c.y + arm));
                p.move_to(Point::new(c.x + arm, c.y - arm));
                p.line_to(Point::new(c.x - arm, c.y + arm));
            });
            f.stroke(
                &cross,
                Stroke {
                    style: canvas::Style::Solid(faded(if lit { TEXT } else { FAINT }, alpha)),
                    width: 1.4,
                    line_cap: canvas::LineCap::Round,
                    ..Stroke::default()
                },
            );
        }
    }

    /// What it shows, read afresh: laid out again when the history or do not
    /// disturb changed, or the minute did; what slides out goes at its end.
    fn refresh<Message>(&self, state: &mut State, now: Instant) -> Option<canvas::Action<Message>> {
        for pressed in kit::keys(self.id) {
            let _: Option<canvas::Action<Message>> = self.key(state, &pressed);
        }
        state.now = Some(now);
        if let Some(end) = state.leaving_end()
            && now >= end
            && let Some(leaving) = state.leaving.as_mut().filter(|l| !l.sent)
        {
            leaving.sent = true;
            notifications::input(Said::Remove(leaving.keys.clone()));
        }
        let shown = shown();
        let wall = notifications::wall();
        let minute = wall.timestamp().as_second() / 60;
        let from = (shown.version, minute, shown.dnd);
        if state.from != Some(from) || state.laid.is_none() {
            let words = Words::new(Lang::of_messages());
            let laid = {
                let mut cuts = CUTS.lock().unwrap_or_else(PoisonError::into_inner);
                let cuts = cuts.get_or_insert_with(HashMap::new);
                // Only what it lists is kept cut.
                cuts.retain(|(key, _, _), _| shown.items.iter().any(|k| k.key == *key));
                lay_out(&shown, words, &wall, cuts, &mut kit::measure)
            };
            if state.from.map(|f| (f.0, f.2)) != Some((shown.version, shown.dnd))
                || state.laid.is_none()
            {
                tracing::debug!(dnd = ?laid.dnd.choice(), history = %laid.said(words), "the history shows");
                let items: Vec<String> = laid
                    .targets(0.0)
                    .iter()
                    .map(|(t, r)| {
                        let name = format!("{t:?}").replace(' ', "");
                        format!("{name}@{:.0},{:.0}", r.center().x, r.center().y)
                    })
                    .collect();
                tracing::debug!(height = laid.height, items = %items.join(" "), "the history's items");
            }
            let was_empty = state.laid.as_ref().is_some_and(|l| l.rows.is_empty());
            if laid.rows.is_empty() && !was_empty && state.laid.is_some() {
                state.empty_since = Some(now);
            }
            state.scroll = state.scroll.clamp(0.0, laid.most_scroll());
            let targets: Vec<Target> = laid
                .targets(state.scroll)
                .into_iter()
                .map(|(t, _)| t)
                .collect();
            if state.focus.is_some_and(|f| !targets.contains(&f)) {
                state.focus = None;
            }
            if state.laid.as_ref().map(|l| l.height) != Some(laid.height) {
                state.pointer.hover = None;
            }
            // What slid out is gone from the list: it is over.
            if state
                .leaving
                .as_ref()
                .is_some_and(|l| l.sent && !laid.rows.iter().any(|r| l.keys.contains(&r.key)))
            {
                state.leaving = None;
            }
            state.laid = Some(laid);
            state.from = Some(from);
            state.switch.settle(shown.dnd.is_on(), now);
            super::refit(Item::Notifications);
        }
        // Frames while something moves.
        let moving = state.leaving.as_ref().is_some_and(|l| !l.sent)
            || state.switch.sliding(now)
            || state
                .empty_since
                .is_some_and(|since| self.motion && now < since + REST_IN);
        moving.then(|| canvas::Action::request_redraw_at(now + Duration::from_millis(16)))
    }

    /// Items `keys` go: sliding out one after another, or at once without
    /// motion.
    fn take_out(&self, state: &mut State, keys: Vec<u64>, now: Instant) {
        if keys.is_empty() || state.leaving.is_some() {
            return;
        }
        if self.motion {
            state.leaving = Some(Leaving {
                keys,
                since: now,
                sent: false,
            });
        } else {
            notifications::input(Said::Remove(keys));
        }
    }

    fn act(&self, state: &mut State, target: Target, now: Instant) {
        tracing::debug!(?target, "the history");
        let Some(laid) = &state.laid else {
            return;
        };
        match target {
            Target::Clear => {
                // The items in view first, in their order: they slide out.
                let keys: Vec<u64> = laid.rows.iter().map(|r| r.key).collect();
                let first = laid
                    .rows
                    .iter()
                    .position(|r| r.rect.y + r.rect.height > state.scroll)
                    .unwrap_or(0);
                let mut keys = keys;
                keys.rotate_left(first);
                self.take_out(state, keys, now);
            }
            Target::Switch => {
                let on = laid.dnd.is_on();
                state.switch.flip(on, now, self.motion);
                let dnd = if on {
                    Dnd::Off
                } else {
                    Dnd::starting(Choice::Hour, now, &notifications::wall())
                };
                notifications::input(Said::Dnd(dnd));
            }
            Target::Chip(choice) => {
                notifications::input(Said::Dnd(Dnd::starting(
                    choice,
                    now,
                    &notifications::wall(),
                )));
            }
            Target::Item(key) => {
                if state.leaving.is_some() {
                    return;
                }
                notifications::input(Said::Open(key));
                event(Seen::Chosen);
            }
            Target::Close(key) => self.take_out(state, vec![key], now),
            Target::Action(key, i) => {
                let action = laid
                    .rows
                    .iter()
                    .find(|r| r.key == key)
                    .and_then(|r| r.actions.get(i))
                    .map(|(k, _)| k.clone());
                if let Some(action) = action {
                    notifications::input(Said::Act { key, action });
                }
            }
        }
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let target = at.and_then(|at| {
            state
                .laid
                .as_ref()
                .and_then(|l| l.target_at(at, state.scroll))
        });
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        let now = Instant::now();
        match seen {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                state.pointer.moved();
                state.pointer.over(target).then_some(redraw).flatten()
            }
            mouse::Event::CursorLeft => {
                state.pointer.left();
                redraw
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                state.focus = None;
                state.pointer.pressed = target;
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if let Some(target) = state.pointer.release(target) {
                    self.act(state, target, now);
                }
                redraw
            }
            mouse::Event::WheelScrolled { delta } => {
                let laid = state.laid.as_ref()?;
                if !at.is_some_and(|at| laid.view.contains(at)) {
                    return None;
                }
                let by = state.wheel.down(delta, NOTCH, kit::popover_scale());
                state.scroll = (state.scroll + by).clamp(0.0, laid.most_scroll());
                state.pointer.hover = at.and_then(|at| laid.target_at(at, state.scroll));
                redraw
            }
            _ => None,
        }
    }

    fn key<Message>(
        &self,
        state: &mut State,
        pressed: &Pressed,
    ) -> Option<canvas::Action<Message>> {
        use keyboard::{Key, key::Named};
        let order: Vec<Target> = state.targets().into_iter().map(|(t, _)| t).collect();
        let items: Vec<Target> = order
            .iter()
            .copied()
            .filter(|t| matches!(t, Target::Item(_)))
            .collect();
        match &pressed.key {
            Key::Named(Named::Escape) => {
                event(Seen::Escape);
                return Some(canvas::Action::capture());
            }
            Key::Named(Named::Tab) => {
                state.focus = kit::step(&order, state.focus, pressed.modifiers.shift());
            }
            Key::Named(arrow @ (Named::ArrowDown | Named::ArrowUp)) => {
                let down = *arrow == Named::ArrowDown;
                let current = state.focus.and_then(Target::key).map(Target::Item);
                state.focus = match current {
                    Some(item) => kit::cycle(&items, item, down),
                    None => kit::step(&items, None, !down),
                };
            }
            Key::Named(Named::Enter | Named::Space) => {
                if let Some(target) = state.focus.filter(|t| order.contains(t)) {
                    self.act(state, target, Instant::now());
                }
            }
            _ => return None,
        }
        self.reveal(state);
        Some(canvas::Action::request_redraw().and_capture())
    }

    /// The list scrolled so that what holds the keyboard shows.
    fn reveal(&self, state: &mut State) {
        let (Some(laid), Some(key)) = (&state.laid, state.focus.and_then(Target::key)) else {
            return;
        };
        let Some(row) = laid.rows.iter().find(|r| r.key == key) else {
            return;
        };
        state.scroll = kit::reveal(
            (row.rect.y, row.rect.height),
            laid.view.height,
            state.scroll,
            laid.most_scroll(),
        );
    }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

/// The moon, in amber while on.
fn moon(on: bool) -> svg::Handle {
    static MOONS: std::sync::OnceLock<[svg::Handle; 2]> = std::sync::OnceLock::new();
    let make = |colour: &str| {
        svg::Handle::from_memory(
            format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" width=\"24\" \
                 height=\"24\"><path d=\"M19 14.5 A7.5 7.5 0 0 1 9.5 5 A7.5 7.5 0 1 0 19 14.5 Z\" \
                 fill=\"none\" stroke=\"{colour}\" stroke-width=\"1.8\" \
                 stroke-linejoin=\"round\"/></svg>"
            )
            .into_bytes(),
        )
    };
    let moons = MOONS.get_or_init(|| [make("#A9B2C2"), make(notifications::bell::AMBER)]);
    moons[usize::from(on)].clone()
}

/// The panda asleep, for the empty history.
fn panda() -> svg::Handle {
    static PANDA: std::sync::OnceLock<svg::Handle> = std::sync::OnceLock::new();
    PANDA
        .get_or_init(|| {
            svg::Handle::from_memory(include_bytes!("../../assets/pandas/resting.svg").as_slice())
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::{
        history::{History, Sight as Was},
        note::{IconFrom, Note, Timeout, Urgency},
    };

    fn note(id: u32, app: &str, title: &str, at: i64) -> Note {
        Note {
            id,
            app: app.to_owned(),
            entry: None,
            icon: None,
            summary: title.to_owned(),
            body: "J’ai mis à jour la maquette du lanceur, tu peux jeter un œil quand tu as deux \
                   minutes ? Et puis encore des mots pour une troisième ligne."
                .to_owned(),
            actions: vec![("reply".to_owned(), "Répondre".to_owned())],
            default: true,
            urgency: Urgency::Normal,
            timeout: Timeout::Never,
            progress: None,
            resident: false,
            pid: 0,
            agent: false,
            icon_from: IconFrom::default(),
            at,
        }
    }

    fn measure(t: &str, size: f32, _font: Font) -> f32 {
        t.chars().count() as f32 * size * 0.5
    }

    fn now() -> Zoned {
        "2026-11-02T14:32:00+01:00[Europe/Paris]"
            .parse()
            .expect("a time")
    }

    fn shown(history: &History, dnd: Dnd) -> Shown {
        Shown {
            items: history.items().to_vec(),
            dnd,
            version: history.version(),
        }
    }

    #[test]
    fn it_lists_by_sections_newest_first_each_item_as_the_mockup_lays_it() {
        let now = now();
        let ms = now.timestamp().as_millisecond();
        let mut h = History::default();
        h.add(
            note(1, "Thunderbird", "Facture", ms - 26 * 3600 * 1000),
            Was::Seen,
        );
        h.add(
            note(2, "Firefox", "Téléchargement terminé", ms - 70 * 60_000),
            Was::Seen,
        );
        h.add(
            note(3, "Slack", "Hugo dans #dev", ms - 9 * 60_000),
            Was::Unseen,
        );
        h.add(
            note(4, "Slack", "Camille dans #design", ms - 2 * 60_000),
            Was::Unseen,
        );
        h.closed(1);
        let laid = lay_out(
            &shown(&h, Dnd::Off),
            Words::new(Lang::French),
            &now,
            &mut Cuts::new(),
            &mut measure,
        );
        let heads: Vec<Section> = laid.heads.iter().map(|(s, _)| *s).collect();
        assert_eq!(heads, [Section::Recent, Section::Today, Section::Yesterday]);
        let said = laid.said(Words::new(Lang::French));
        assert!(
            said.starts_with(
                "RÉCENTES: Slack «Camille dans #design» il y a 2\u{a0}min (unseen) [1 actions] \
                 Slack «Hugo dans #dev» il y a 9\u{a0}min (unseen)"
            ),
            "{said}"
        );
        assert!(said.contains("PLUS TÔT AUJOURD’HUI: Firefox"), "{said}");
        assert!(said.ends_with("HIER: Thunderbird «Facture» hier"), "{said}");
        let first = &laid.rows[0];
        // 342 px wide, 8 px in from the popover's rim; its body on two
        // lines, cut; its ✕ 6 px from its right and 8 px from its top.
        assert_eq!(first.rect.x, 9.0);
        assert_eq!(first.rect.width, 342.0);
        assert_eq!(first.cut.body.len(), 2);
        assert!(first.cut.body[1].ends_with('…'));
        assert_eq!(first.close().x + 22.0 + 6.0, 351.0);
        assert_eq!(first.close().y, first.rect.y + 8.0);
        // Its sender holds no more the oldest: no actions.
        assert!(laid.rows[3].actions.is_empty());
        // The header's "Tout effacer", and at most 600 px.
        assert!(laid.clear.is_some());
        assert!(laid.height <= 600.0);
    }

    #[test]
    fn do_not_disturb_says_until_when_and_offers_its_chips_while_on() {
        let now = now();
        let words = Words::new(Lang::French);
        let h = History::default();
        let off = lay_out(
            &shown(&h, Dnd::Off),
            words,
            &now,
            &mut Cuts::new(),
            &mut measure,
        );
        assert_eq!(off.said, ["Les bulles s’affichent"]);
        assert!(off.chips.is_empty());
        assert!(off.clear.is_none());
        let at = Instant::now();
        let hour = Dnd::starting(Choice::Hour, at, &now);
        assert_eq!(
            words.said(hour, &now),
            "Jusqu’à 15\u{a0}h\u{a0}32 · seules les urgences passent"
        );
        let morning = Dnd::starting(Choice::Morning, at, &now);
        assert!(
            words
                .said(morning, &now)
                .starts_with("Jusqu’à demain, 8\u{a0}h ·")
        );
        let english = Words::new(Lang::English);
        assert_eq!(
            english.said(hour, &now),
            "Until 3:32\u{a0}PM · only urgent ones come through"
        );
        assert!(
            english
                .said(morning, &now)
                .starts_with("Until tomorrow, 8\u{a0}AM")
        );
        let on = lay_out(
            &shown(&h, hour),
            words,
            &now,
            &mut Cuts::new(),
            &mut measure,
        );
        let chips: Vec<Choice> = on.chips.iter().map(|(c, _)| *c).collect();
        assert_eq!(chips, Choice::ALL);
        assert!(on.card.height > off.card.height + 26.0);
        // Empty: the panda's room.
        assert_eq!(on.content, EMPTY);
    }
}
