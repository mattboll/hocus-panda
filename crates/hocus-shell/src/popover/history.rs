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

use hocus_words::shell::popover_history as txt;
use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_a11y::Role;
use iced::{
    Color, Element, Font, Length, Padding, Point, Rectangle, Renderer, Size, Theme,
    alignment::{Horizontal, Vertical},
    keyboard, mouse,
    widget::{
        Id, Space, Stack, button,
        canvas::{self, Frame, Path, Stroke},
        column, container, image, mouse_area, pin, row, scrollable, svg,
        text::LineHeight,
    },
    window,
};
use jiff::Zoned;

use super::{
    Event as Seen, Item, Pressed, event,
    kit::{self, Switch},
};
use crate::theme::Ink;
use crate::{
    clock::{motion::bezier, time::Lang},
    levels::{
        draw::{DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb},
        icons::Icon,
    },
    notifications::{
        self,
        dnd::{Choice, Dnd},
        history::{self, Ago, Kept, Section},
        stack::Input as Said,
    },
    popover::kit::MANROPE_SEMIBOLD,
    ui::{
        self, Around, Later, control,
        notched::Notched,
        parts::{drawn, gap, moving, spaced_by},
        style::{self, said},
    },
};

type El<'a> = Element<'a, Msg>;

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
const HEAD_SPACING: f32 = 0.08;
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
/// The empty state: the panda 140 px (#649), its words 8 px apart.
const EMPTY: f32 = 264.0;
const PANDA: f32 = 140.0;
const EMPTY_GAP: f32 = 8.0;
const NOTHING: f32 = 14.0;
const ALL_READ: f32 = 12.0;
/// A notch of the wheel scrolls this far.
const NOTCH: f32 = 40.0;

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, a)
}

/// The bubbles' tint (the mockup's), opaque without glass.
const GLASS_TOP: Ink = Ink::Dark(rgba(22, 27, 42, 0.95));
const GLASS_BOTTOM: Ink = Ink::Dark(rgba(14, 17, 28, 0.97));
const OPAQUE_TOP: Ink = Ink::Dark(rgb(0x161B2A));
const OPAQUE_BOTTOM: Ink = Ink::Dark(rgb(0x0E111C));
const RIM: Ink = Ink::Rim(rgba(255, 255, 255, 0.10));
const GHOST_HOVER: Ink = Ink::Dark(rgba(255, 255, 255, 0.08));
const ITEM_HOVER: Ink = Ink::Dark(rgba(255, 255, 255, 0.05));
const CARD_OFF: (Ink, Ink) = (
    Ink::Dark(rgba(255, 255, 255, 0.04)),
    Ink::Dark(rgba(255, 255, 255, 0.08)),
);
const CARD_ON: (Color, Color) = (rgba(245, 196, 110, 0.08), rgba(245, 196, 110, 0.35));
const SWITCH_ON: Ink = Ink::Dark(rgb(0xF5B44A));
const SWITCH_OFF: Ink = Ink::Dark(rgba(255, 255, 255, 0.18));
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
const QUIET: Ink = Ink::Dark(rgb(0x7C8698));
const UNSEEN: Ink = Ink::Dark(rgb(0x8FB3FF));
const NOTHING_INK: Ink = Ink::Dark(rgb(0xC9D0DC));
const ACTION_FILL: Ink = Ink::Dark(rgba(255, 255, 255, 0.06));
const ACTION_HOVER: Ink = Ink::Dark(rgba(255, 255, 255, 0.12));
const ACTION_RIM: Ink = Ink::Dark(rgba(255, 255, 255, 0.14));
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
    lang: Lang,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    fn pick(self, text: hocus_words::Text) -> &'static str {
        text.get(self.lang)
    }

    fn title(self) -> &'static str {
        "Notifications"
    }

    fn clear(self) -> &'static str {
        self.pick(txt::CLEAR_ALL)
    }

    fn dnd(self) -> &'static str {
        self.pick(txt::DO_NOT_DISTURB)
    }

    fn chip(self, choice: Choice) -> &'static str {
        match choice {
            Choice::Hour => self.pick(txt::FOR_1_HOUR),
            Choice::Morning => self.pick(txt::UNTIL_TOMORROW_MORNING),
            Choice::Always => self.pick(txt::NO_LIMIT),
        }
    }

    /// A time of day as the user reads it: "15 h 32", "8 h"; "3:32 PM",
    /// "8 AM".
    fn time(self, at: &Zoned) -> String {
        let (h, m) = (at.hour(), at.minute());
        if self.lang == Lang::French {
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
            return self.pick(txt::BUBBLES_SHOW).to_owned();
        };
        let until = until.and_then(|u| notifications::dnd::zoned(u.wall, now));
        let when = match (choice, until) {
            (Choice::Always, _) | (_, None) => self.pick(txt::UNTIL_YOU_TURN_IT_OFF).to_owned(),
            (_, Some(at)) => {
                let tomorrow = now.date().tomorrow().ok() == Some(at.date());
                match tomorrow {
                    true => txt::until_tomorrow(self.lang, self.time(&at)),
                    false => txt::until(self.lang, self.time(&at)),
                }
            }
        };
        format!("{when} · {}", self.pick(txt::ONLY_URGENT_ONES_COME_THROUGH))
    }

    fn section(self, section: Section) -> &'static str {
        match section {
            Section::Recent => self.pick(txt::RECENT),
            Section::Today => self.pick(txt::EARLIER_TODAY),
            Section::Yesterday => self.pick(txt::YESTERDAY),
            Section::Older => self.pick(txt::OLDER),
        }
    }

    fn ago(self, ago: Ago) -> String {
        match ago {
            Ago::Now => txt::NOW.get(self.lang).to_owned(),
            Ago::Minutes(n) => txt::min_ago(self.lang, n),
            Ago::Hours(n) => txt::h_ago(self.lang, n),
            Ago::Yesterday => txt::YESTERDAY_2.get(self.lang).to_owned(),
            Ago::Days(n) => txt::days_ago(self.lang, n),
        }
    }

    fn nothing(self) -> &'static str {
        self.pick(txt::NOTHING_NEW)
    }

    fn all_read(self) -> &'static str {
        self.pick(txt::ALL_READ)
    }
}

// --- Laying it out --------------------------------------------------------------

/// What can be acted on in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Clear,
    Switch,
    Chip(Choice),
    Item(u64),
    Close(u64),
    Action(u64, usize),
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    fn id(self) -> String {
        format!("history:{self:?}").replace(' ', "")
    }

    /// The ring's corners on it.
    fn radius(self) -> f32 {
        match self {
            Self::Item(_) => ITEM_RADIUS,
            Self::Close(_) => CLOSE_RADIUS,
            Self::Chip(_) => CHIP / 2.0,
            Self::Switch => SWITCH.height / 2.0,
            Self::Clear => GHOST_RADIUS,
            Self::Action(..) => 8.0,
        }
    }

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
    dot_x: Option<f32>,
    actions: Vec<(String, Rectangle)>,
    section: Section,
}

impl Row {
    #[cfg(test)]
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
}

/// What it keeps while it is open.
#[derive(Debug, Default)]
struct Ui {
    laid: Option<Laid>,
    /// What it was laid out from: the history's version, the minute, do not
    /// disturb.
    from: Option<(u64, i64, Dnd)>,
    focus: Option<Target>,
    /// The item the pointer is over.
    over: Option<u64>,
    /// How far the list is scrolled, as it said or was told.
    scroll: f32,
    switch: Switch,
    /// Items sliding out.
    leaving: Option<Leaving>,
    /// Since when it is empty, for the panda's coming in.
    empty_since: Option<Instant>,
    /// The panda breathing as two "z" rise, as it opens empty or empties
    /// (#649).
    dozing: Option<crate::pandas::Burst>,
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);
static LATER: Later = Later::new();

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

/// It opens: nothing holds the ring, nothing slides.
pub fn opening() {
    ui(|ui| *ui = Ui::default());
}

/// The list's scrollable.
const LIST_ID: &str = "history:list";

/// Items sliding out, in their order, since then; once they are all out,
/// the stack is told (`sent`), and they stay out until it takes them.
#[derive(Debug)]
struct Leaving {
    keys: Vec<u64>,
    since: Instant,
    sent: bool,
}

impl Ui {
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

    /// Whether what it shows moves at `now`: items sliding out, the empty
    /// state coming in. Built again each frame then.
    fn moving(&self, now: Instant, motion: bool) -> bool {
        self.leaving.as_ref().is_some_and(|l| !l.sent)
            || self
                .empty_since
                .is_some_and(|since| motion && now < since + REST_IN + Duration::from_millis(16))
    }
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A button pressed, or `Enter` on it.
    Act(Target),
    /// The ring comes to it.
    Focus(Target),
    /// The pointer came over an item (`true`), or left it.
    Over(u64, bool),
    /// `↓` (`true`) or `↑`: through the items.
    Arrow(bool),
    /// The list scrolled to there.
    Scrolled(f32),
    /// A press anywhere on the popover.
    Pressed,
    /// The history changed under the view, or something moves.
    Stale,
}

/// The history on its surface.
pub fn view<'a>(panel: &Panel) -> Element<'a, crate::Message> {
    let (glass, motion) = (panel.glass, panel.motion);
    let minute = || notifications::wall().timestamp().as_second() / 60;
    Around::new(
        panel.id,
        build(motion),
        move |m| handle(motion, m),
        Msg::Stale,
    )
    .rebuild(move || build(motion))
    .stale_when(move || {
        ui(|ui| ui.moving(Instant::now(), motion) || ui.from.is_some_and(|f| f.1 != minute()))
    })
    .keys(arrows)
    .later(&LATER)
    .press(Msg::Pressed)
    .on(iced::widget::canvas(Tint(glass))
        .width(Length::Fill)
        .height(Length::Fill))
}

/// The arrows go through the items.
fn arrows(pressed: &Pressed) -> Option<Msg> {
    use keyboard::{Key, key::Named};
    match pressed.key {
        Key::Named(Named::ArrowDown) => Some(Msg::Arrow(true)),
        Key::Named(Named::ArrowUp) => Some(Msg::Arrow(false)),
        _ => None,
    }
}

/// Its content, as the history is now.
fn build<'a>(motion: bool) -> El<'a> {
    let now = Instant::now();
    let words = Words::new(Lang::of_messages());
    ui(|ui| {
        refresh(ui, now);
        let Some(laid) = ui.laid.clone() else {
            return Space::new().into();
        };
        content(&laid, words, ui, now, motion)
    })
}

/// What it shows, read afresh: laid out again when the history or do not
/// disturb changed, or the minute did.
fn refresh(ui: &mut Ui, now: Instant) {
    let shown = shown();
    let wall = notifications::wall();
    let minute = wall.timestamp().as_second() / 60;
    let from = (shown.version, minute, shown.dnd);
    if ui.from == Some(from) && ui.laid.is_some() {
        return;
    }
    let words = Words::new(Lang::of_messages());
    let laid = {
        let mut cuts = CUTS.lock().unwrap_or_else(PoisonError::into_inner);
        let cuts = cuts.get_or_insert_with(HashMap::new);
        // Only what it lists is kept cut.
        cuts.retain(|(key, _, _), _| shown.items.iter().any(|k| k.key == *key));
        lay_out(&shown, words, &wall, cuts, &mut kit::measure)
    };
    if ui.from.map(|f| (f.0, f.2)) != Some((shown.version, shown.dnd)) || ui.laid.is_none() {
        tracing::debug!(dnd = ?laid.dnd.choice(), history = %laid.said(words), "the history shows");
    }
    let was_empty = ui.laid.as_ref().is_some_and(|l| l.rows.is_empty());
    if laid.rows.is_empty() && !was_empty && ui.laid.is_some() {
        ui.empty_since = Some(now);
    }
    if laid.rows.is_empty() && !was_empty && crate::pandas::lively() {
        ui.dozing = Some(crate::pandas::Burst::of(crate::pandas::Mood::Resting, now));
    }
    let keys: Vec<u64> = laid.rows.iter().map(|r| r.key).collect();
    if ui.focus.is_some_and(|f| {
        f.key().is_some_and(|k| !keys.contains(&k)) || f == Target::Clear && laid.clear.is_none()
    }) {
        ui.focus = None;
    }
    // What slid out is gone from the list: it is over.
    if ui
        .leaving
        .as_ref()
        .is_some_and(|l| l.sent && !laid.rows.iter().any(|r| l.keys.contains(&r.key)))
    {
        ui.leaving = None;
    }
    ui.laid = Some(laid);
    ui.from = Some(from);
    ui.switch.settle(shown.dnd.is_on(), now);
}

/// `content` with its top left corner at `point`.
fn at<'a>(content: impl Into<El<'a>>, point: Point) -> El<'a> {
    pin(content).x(point.x).y(point.y).into()
}

/// A text at `size` in `font`, its lines `line` em apart.
fn text_of<'a>(content: impl Into<String>, size: f32, line: f32, font: Font, ink: Color) -> El<'a> {
    said(content.into(), size, font, ink)
        .line_height(LineHeight::Relative(line))
        .into()
}

/// `lines` one under the other, `size` px on lines `line` em apart.
fn lines<'a>(lines: &[String], size: f32, line: f32, ink: Color) -> El<'a> {
    let mut all = column![];
    for said_line in lines {
        all = all.push(text_of(said_line.clone(), size, line, MANROPE, ink));
    }
    all.into()
}

/// A rounded box of `fill` with a 1 px `rim`, as the canvas stroked it.
fn rimmed(fill: Color, rim: Color, radius: f32) -> container::Style {
    container::Style {
        background: Some(fill.into()),
        border: iced::Border {
            color: rim,
            width: 1.0,
            radius: radius.into(),
        },
        ..container::Style::default()
    }
}

/// Control `target`, its ring when it holds it.
fn declared<'a>(
    widget: impl Into<El<'a>>,
    target: Target,
    role: Role,
    name: impl Into<String>,
    focus: Option<Target>,
) -> ui::control::Control<'a, Msg> {
    control(widget, target.id(), role, name)
        .focus(focus == Some(target), Msg::Focus(target), target.radius())
        .activate(Msg::Act(target))
}

fn content<'a>(laid: &Laid, words: Words, ui: &Ui, now: Instant, motion: bool) -> El<'a> {
    let mut layers: Vec<El<'a>> = vec![at(
        container(said(words.title(), TITLE, MANROPE_BOLD, TEXT.get()))
            .height(HEADER)
            .align_y(Vertical::Center),
        Point::new(BORDER + SIDE, HEADER_TOP),
    )];
    if let Some((rect, _)) = laid.clear {
        let clear = button(
            container(
                iced::widget::text(words.clear())
                    .size(GHOST_TEXT)
                    .font(MANROPE_SEMIBOLD)
                    .line_height(LineHeight::Relative(LINE))
                    .shaping(iced::widget::text::Shaping::Advanced)
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .center(Length::Fill),
        )
        .padding(0)
        .width(rect.width)
        .height(rect.height)
        .style(|_, status| {
            let lit = style::lit(status);
            let mut quiet = if lit {
                style::hovered(GHOST_HOVER.get(), GHOST_RADIUS)
            } else {
                style::clear()
            };
            quiet.text_color = if lit { TEXT.get() } else { DIM.get() };
            quiet
        })
        .on_press(Msg::Act(Target::Clear));
        layers.push(at(
            declared(clear, Target::Clear, Role::Button, words.clear(), ui.focus),
            rect.position(),
        ));
    }
    card(&mut layers, laid, words, ui, motion);
    layers.push(at(list(laid, words, ui, now, motion), laid.view.position()));
    Stack::with_children(layers)
        .width(W)
        .height(laid.height)
        .into()
}

/// The do not disturb card: its moon, its words, its switch, its chips.
fn card<'a>(layers: &mut Vec<El<'a>>, laid: &Laid, words: Words, ui: &Ui, motion: bool) {
    let on = laid.dnd.is_on();
    let (fill, rim) = if on {
        CARD_ON
    } else {
        (CARD_OFF.0.get(), CARD_OFF.1.get())
    };
    layers.push(at(
        container(Space::new().width(laid.card.width).height(laid.card.height))
            .style(move |_| rimmed(fill, rim, CARD_RADIUS)),
        laid.card.position(),
    ));
    let inner_left = CARD_LEFT + BORDER + CARD_PAD.1;
    layers.push(at(
        svg(moon(on)).width(MOON).height(MOON),
        Point::new(inner_left, laid.row_middle - MOON / 2.0),
    ));
    let column_top = laid.said_top - SAID_GAP - DND_NAME * LINE;
    layers.push(at(
        ui::line(
            column![
                said(words.dnd(), DND_NAME, MANROPE_SEMIBOLD, TEXT.get()),
                Space::new().height(SAID_GAP),
                lines(&laid.said, DND_SAID, LINE, FAINT.get()),
            ],
            format!("{}, {}", words.dnd(), laid.said.join(" ")),
        ),
        Point::new(inner_left + MOON + CARD_GAP, column_top),
    ));
    // The switch, its knob sliding.
    let switch = ui.switch;
    let knob = moving(
        SWITCH.width,
        SWITCH.height,
        move |f, s, now| {
            let shown = switch.shown(on, now);
            let p = switch.knob_at(shown, now, motion);
            let rect = Rectangle::new(Point::ORIGIN, s);
            f.fill(
                &kit::rounded(rect, SWITCH.height / 2.0),
                mix(SWITCH_OFF.get(), SWITCH_ON.get(), p),
            );
            let centre = Point::new(
                3.0 + (SWITCH.width - KNOB - 6.0) * p + KNOB / 2.0,
                rect.center_y(),
            );
            f.fill(&Path::circle(centre, KNOB / 2.0), Color::WHITE);
        },
        move |now| switch.sliding(now),
    );
    let flip = button(knob)
        .padding(0)
        .style(|_, _| style::clear())
        .on_press(Msg::Act(Target::Switch));
    layers.push(at(
        declared(
            flip,
            Target::Switch,
            Role::ToggleButton,
            words.dnd(),
            ui.focus,
        )
        .node(|n| n.checked(on)),
        laid.switch.position(),
    ));
    let chosen = laid.dnd.choice();
    let n = laid.chips.len();
    for (i, (choice, rect)) in laid.chips.iter().enumerate() {
        let picked = chosen == Some(*choice);
        let (fill, rim, ink) = if picked { CHIP_ON } else { CHIP_OFF };
        let chip = button(
            container(said(words.chip(*choice), CHIP_TEXT, MANROPE_SEMIBOLD, ink))
                .center(Length::Fill),
        )
        .padding(0)
        .width(rect.width)
        .height(rect.height)
        .style(move |_, status| {
            let fill = if style::lit(status) && !picked {
                ACTION_HOVER.get()
            } else {
                fill
            };
            let boxed = rimmed(fill, rim, CHIP / 2.0);
            iced::widget::button::Style {
                background: boxed.background,
                border: boxed.border,
                ..iced::widget::button::Style::default()
            }
        })
        .on_press(Msg::Act(Target::Chip(*choice)));
        layers.push(at(
            declared(
                chip,
                Target::Chip(*choice),
                Role::RadioButton,
                words.chip(*choice),
                ui.focus,
            )
            .node(|node| node.checked(picked).set(i + 1, n)),
            rect.position(),
        ));
    }
}

/// The list: its sections and items, scrolling past its room; empty, the
/// resting panda.
fn list<'a>(laid: &Laid, words: Words, ui: &Ui, now: Instant, motion: bool) -> El<'a> {
    let width = laid.view.width;
    if laid.rows.is_empty() {
        return empty(laid, words, ui, now, motion);
    }
    let mut all = column![].width(width);
    let mut end = 0.0;
    let mut heads = laid.heads.iter().peekable();
    for shown in &laid.rows {
        if let Some((section, middle)) = heads.next_if(|(s, _)| *s == shown.section) {
            let top = middle - HEAD * LINE / 2.0 - HEAD_PAD.0;
            all = all.push(Space::new().height(top - end));
            let word = words.section(*section);
            let mut chars = word.chars();
            let read = chars.next().map_or_else(String::new, |c| {
                c.to_string() + &chars.as_str().to_lowercase()
            });
            all = all.push(ui::line(
                container(spaced_by(
                    word,
                    HEAD,
                    MANROPE_BOLD,
                    QUIET.get(),
                    HEAD_SPACING,
                ))
                .padding(Padding {
                    top: HEAD_PAD.0,
                    right: 0.0,
                    bottom: HEAD_PAD.1,
                    left: HEAD_PAD.0,
                }),
                read,
            ));
            end = top + HEAD_PAD.0 + HEAD * LINE + HEAD_PAD.1;
        }
        all = all.push(Space::new().height(shown.rect.y - end));
        all = all.push(item(shown, words, ui, now));
        end = shown.rect.y + shown.rect.height;
    }
    let scrolled = scrollable(container(all))
        .id(Id::new(LIST_ID))
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new()
                .width(kit::SCROLLBAR)
                .scroller_width(kit::SCROLLBAR)
                .margin(0)
                .spacing(0),
        ))
        .style(|_, _| style::scrollbar(kit::SCROLLBAR))
        .on_scroll(|v| Msg::Scrolled(v.absolute_offset().y))
        .width(width + kit::SCROLLBAR)
        .height(laid.view.height);
    Notched::new(scrolled, NOTCH).into()
}

/// Nothing in it: the resting panda, "Rien de neuf" and "Tout est lu.",
/// coming in.
fn empty<'a>(laid: &Laid, words: Words, ui: &Ui, now: Instant, motion: bool) -> El<'a> {
    let p = match (ui.empty_since, motion) {
        (Some(since), true) => {
            let t = now.saturating_duration_since(since).as_secs_f32() / REST_IN.as_secs_f32();
            bezier(EASE_OUT, t.clamp(0.0, 1.0))
        }
        _ => 1.0,
    };
    let shift = 8.0 * (1.0 - p);
    let block = PANDA + EMPTY_GAP + NOTHING * LINE + EMPTY_GAP + ALL_READ * LINE;
    let top = (laid.view.height - block) / 2.0 + shift;
    let dozing = ui.dozing;
    // The panda's own proportions, within its room.
    let (pw, ph) = (PANDA, PANDA * 120.5 / 126.0);
    let panda = moving(
        PANDA,
        PANDA,
        move |f, _, at| {
            f.draw_svg(
                Rectangle::new(Point::new(0.0, (PANDA - ph) / 2.0), Size::new(pw, ph)),
                iced::advanced::svg::Svg::new(crate::pandas::drawing(
                    crate::pandas::Mood::Resting,
                    dozing,
                    at,
                ))
                .opacity(p),
            );
        },
        move |at| dozing.is_some_and(|d| d.plays(at)),
    );
    let said_lines = column![
        container(panda)
            .width(Length::Fill)
            .align_x(Horizontal::Center),
        Space::new().height(EMPTY_GAP),
        container(said(
            words.nothing(),
            NOTHING,
            MANROPE_SEMIBOLD,
            NOTHING_INK.get().scale_alpha(p)
        ))
        .width(Length::Fill)
        .align_x(Horizontal::Center),
        Space::new().height(EMPTY_GAP),
        container(said(
            words.all_read(),
            ALL_READ,
            MANROPE,
            QUIET.get().scale_alpha(p)
        ))
        .width(Length::Fill)
        .align_x(Horizontal::Center),
    ];
    container(ui::line(
        said_lines,
        format!("{} {}", words.nothing(), words.all_read()),
    ))
    .padding(Padding {
        top,
        ..Padding::ZERO
    })
    .width(W - 2.0 * LIST_SIDE)
    .height(laid.view.height)
    .into()
}

/// An item: its icon, "App · when", its title, its body; under the
/// pointer or holding the keyboard, lit with its ✕; its actions while its
/// sender holds it; sliding out as it goes.
fn item<'a>(shown: &Row, words: Words, ui: &Ui, now: Instant) -> El<'a> {
    let key = shown.key;
    let gone = ui.gone(key, now);
    let alpha = 1.0 - gone;
    let fade = move |c: Color| c.scale_alpha(alpha);
    let over = ui.over == Some(key) && ui.leaving.is_none();
    let lit = over || ui.focus == Some(Target::Item(key));
    let size = shown.rect.size();
    let fr = words.lang == hocus_words::Lang::French;
    // The icon, else the application's first letter on a square.
    let icon: El<'a> = match &shown.icon {
        Some(Icon::Raster(handle)) => image(handle.clone())
            .width(ICON)
            .height(ICON)
            .opacity(alpha)
            .into(),
        Some(Icon::Vector(handle)) => svg(handle.clone())
            .width(ICON)
            .height(ICON)
            .opacity(alpha)
            .into(),
        None => {
            let square = drawn(ICON, ICON, move |f, s| {
                let rect = Rectangle::new(Point::ORIGIN, s);
                let fill = canvas::gradient::Linear::new(Point::ORIGIN, Point::new(ICON, ICON))
                    .add_stop(0.0, fade(NEUTRAL.0))
                    .add_stop(1.0, fade(NEUTRAL.1));
                f.fill(
                    &kit::rounded(rect, (ICON * 0.28).round()),
                    canvas::Fill::from(fill),
                );
            });
            Stack::with_children([
                square,
                container(said(
                    shown.letter.clone(),
                    ICON * 0.4,
                    MANROPE_BOLD,
                    fade(Color::WHITE),
                ))
                .center(ICON)
                .into(),
            ])
            .into()
        }
    };
    let mut meta = row![
        said(shown.cut.app.clone(), META, MANROPE_BOLD, fade(DIM.get())),
        gap(META_GAP),
        said("·", META, MANROPE, fade(QUIET.get())),
        gap(META_GAP),
        said(shown.when.clone(), META, MANROPE, fade(QUIET.get())),
    ]
    .align_y(Vertical::Center);
    if shown.dot_x.is_some() {
        meta = meta.push(gap(META_GAP)).push(drawn(DOT, DOT, move |f, s| {
            f.fill(
                &Path::circle(Point::new(s.width / 2.0, s.height / 2.0), DOT / 2.0),
                fade(UNSEEN.get()),
            );
        }));
    }
    let mut texts = column![
        container(meta)
            .height(META * LINE)
            .align_y(Vertical::Center),
        Space::new().height(TEXT_GAP),
        said(
            shown.cut.title.clone(),
            ITEM_TITLE,
            MANROPE_BOLD,
            fade(TEXT.get())
        ),
    ];
    if !shown.cut.body.is_empty() {
        texts = texts.push(Space::new().height(TEXT_GAP)).push(lines(
            &shown.cut.body,
            BODY,
            BODY_LINE,
            fade(DIM.get()),
        ));
    }
    let pressed = button(Space::new().width(size.width).height(size.height))
        .padding(0)
        .style(move |_, _| {
            if lit {
                style::hovered(fade(ITEM_HOVER.get()), ITEM_RADIUS)
            } else {
                style::clear()
            }
        })
        .on_press(Msg::Act(Target::Item(key)));
    let name = match (shown.cut.app.is_empty(), fr) {
        (true, _) => shown.cut.title.clone(),
        (false, true) => format!("{} : {}", shown.cut.app, shown.cut.title),
        (false, false) => format!("{}: {}", shown.cut.app, shown.cut.title),
    };
    let mut body = shown.cut.body.join(" ");
    if !shown.when.is_empty() {
        body = format!("{body} {}", shown.when).trim().to_owned();
    }
    let x = ITEM_PAD + ICON + ICON_GAP;
    let mut layers: Vec<El<'a>> = vec![
        declared(pressed, Target::Item(key), Role::ListItem, name, ui.focus)
            .node(|n| n.describe(body))
            .into(),
        at(icon, Point::new(ITEM_PAD, ITEM_PAD)),
        at(texts, Point::new(x, ITEM_PAD)),
    ];
    // Its ✕, under the pointer or holding the keyboard.
    let close_target = Target::Close(key);
    let shows_close = over || ui.focus == Some(close_target);
    let cross = drawn(CLOSE, CLOSE, move |f, s| {
        if !shows_close {
            return;
        }
        let c = Point::new(s.width / 2.0, s.height / 2.0);
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
                style: canvas::Style::Solid(fade(FAINT.get())),
                width: 1.4,
                line_cap: canvas::LineCap::Round,
                ..Stroke::default()
            },
        );
    });
    let close = button(cross)
        .padding(0)
        .style(move |_, status| {
            if shows_close && style::lit(status) {
                style::hovered(fade(GHOST_HOVER.get()), CLOSE_RADIUS)
            } else {
                style::clear()
            }
        })
        .on_press(Msg::Act(close_target));
    layers.push(at(
        declared(
            close,
            close_target,
            Role::Button,
            if fr { "Retirer" } else { "Dismiss" },
            ui.focus,
        ),
        Point::new(size.width - CLOSE_IN.0 - CLOSE, CLOSE_IN.1),
    ));
    // Its actions.
    for (i, ((_, rect), (_, label, _))) in shown.actions.iter().zip(&shown.cut.actions).enumerate()
    {
        let target = Target::Action(key, i);
        let action = button(
            container(said(
                label.clone(),
                CHIP_TEXT,
                MANROPE_SEMIBOLD,
                fade(TEXT.get()),
            ))
            .center(Length::Fill),
        )
        .padding(0)
        .width(rect.width)
        .height(rect.height)
        .style(move |_, status| {
            let fill = if style::lit(status) {
                ACTION_HOVER.get()
            } else {
                ACTION_FILL.get()
            };
            let boxed = rimmed(fade(fill), fade(ACTION_RIM.get()), 8.0);
            iced::widget::button::Style {
                background: boxed.background,
                border: boxed.border,
                ..iced::widget::button::Style::default()
            }
        })
        .on_press(Msg::Act(target));
        layers.push(at(
            declared(action, target, Role::Button, label.clone(), ui.focus),
            Point::new(rect.x - shown.rect.x, rect.y - shown.rect.y),
        ));
    }
    let whole = Stack::with_children(layers)
        .width(size.width)
        .height(size.height);
    let hovered = mouse_area(whole)
        .on_enter(Msg::Over(key, true))
        .on_exit(Msg::Over(key, false));
    row![gap(SLIDE_BY * gone), hovered].into()
}

// --- What a message does -------------------------------------------------------

fn handle(motion: bool, message: Msg) {
    let now = Instant::now();
    match message {
        Msg::Stale => {
            // What slid out is all gone: the stack is told.
            let out = ui(|ui| {
                let end = ui.leaving_end()?;
                let leaving = ui.leaving.as_mut().filter(|l| !l.sent && now >= end)?;
                leaving.sent = true;
                Some(leaving.keys.clone())
            });
            if let Some(keys) = out {
                notifications::input(Said::Remove(keys));
            }
        }
        Msg::Pressed => ui(|ui| ui.focus = None),
        Msg::Focus(target) => ui(|ui| {
            ui.focus = Some(target);
            reveal(ui);
        }),
        Msg::Over(key, true) => ui(|ui| ui.over = Some(key)),
        Msg::Over(key, false) => ui(|ui| {
            if ui.over == Some(key) {
                ui.over = None;
            }
        }),
        Msg::Scrolled(y) => ui(|ui| ui.scroll = y),
        Msg::Arrow(down) => ui(|ui| {
            let items: Vec<Target> = ui
                .laid
                .as_ref()
                .map(|l| l.rows.iter().map(|r| Target::Item(r.key)).collect())
                .unwrap_or_default();
            let current = ui.focus.and_then(Target::key).map(Target::Item);
            ui.focus = match current {
                Some(item) => kit::cycle(&items, item, down),
                None => kit::step(&items, None, !down),
            };
            reveal(ui);
        }),
        Msg::Act(target) => act(target, now, motion),
    }
    super::refit(Item::Notifications);
}

/// The list scrolled so that what holds the keyboard shows.
fn reveal(ui: &mut Ui) {
    let (Some(laid), Some(key)) = (&ui.laid, ui.focus.and_then(Target::key)) else {
        return;
    };
    let Some(row) = laid.rows.iter().find(|r| r.key == key) else {
        return;
    };
    let to = kit::reveal(
        (row.rect.y, row.rect.height),
        laid.view.height,
        ui.scroll,
        laid.most_scroll(),
    );
    if (to - ui.scroll).abs() < 0.5 {
        return;
    }
    ui.scroll = to;
    LATER.push(iced::advanced::widget::operation::scrollable::scroll_to(
        Id::new(LIST_ID),
        iced::advanced::widget::operation::scrollable::AbsoluteOffset {
            x: None,
            y: Some(to),
        },
    ));
}

/// Items `keys` go: sliding out one after another, or at once without
/// motion.
fn take_out(ui: &mut Ui, keys: Vec<u64>, now: Instant, motion: bool) -> Option<Vec<u64>> {
    if keys.is_empty() || ui.leaving.is_some() {
        return None;
    }
    if motion {
        ui.leaving = Some(Leaving {
            keys,
            since: now,
            sent: false,
        });
        None
    } else {
        Some(keys)
    }
}

fn act(target: Target, now: Instant, motion: bool) {
    tracing::debug!(?target, "the history");
    let said: Option<Said> = ui(|ui| {
        let laid = ui.laid.clone()?;
        match target {
            Target::Clear => {
                // The items in view first, in their order: they slide out.
                let mut keys: Vec<u64> = laid.rows.iter().map(|r| r.key).collect();
                let first = laid
                    .rows
                    .iter()
                    .position(|r| r.rect.y + r.rect.height > ui.scroll)
                    .unwrap_or(0);
                keys.rotate_left(first);
                take_out(ui, keys, now, motion).map(Said::Remove)
            }
            Target::Switch => {
                let on = laid.dnd.is_on();
                ui.switch.flip(on, now, motion);
                Some(Said::Dnd(if on {
                    Dnd::Off
                } else {
                    Dnd::starting(Choice::Hour, now, &notifications::wall())
                }))
            }
            Target::Chip(choice) => Some(Said::Dnd(Dnd::starting(
                choice,
                now,
                &notifications::wall(),
            ))),
            Target::Item(key) => (ui.leaving.is_none()).then_some(Said::Open(key)),
            Target::Close(key) => take_out(ui, vec![key], now, motion).map(Said::Remove),
            Target::Action(key, i) => laid
                .rows
                .iter()
                .find(|r| r.key == key)
                .and_then(|r| r.actions.get(i))
                .map(|(action, _)| Said::Act {
                    key,
                    action: action.clone(),
                }),
        }
    });
    if let Some(said) = said {
        let open = matches!(said, Said::Open(_));
        notifications::input(said);
        if open {
            event(Seen::Chosen);
        }
    }
}

/// Its background: the bubbles' tint (opaque without glass) and its rim.
#[derive(Debug, Clone, Copy)]
struct Tint(bool);

impl<M> canvas::Program<M> for Tint {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let size = bounds.size();
        let mut f = Frame::new(renderer, size);
        let (top, bottom) = if self.0 {
            (GLASS_TOP, GLASS_BOTTOM)
        } else {
            (OPAQUE_TOP, OPAQUE_BOTTOM)
        };
        let shape = Path::rounded_rectangle(Point::ORIGIN, size, RADIUS.into());
        let tint = canvas::gradient::Linear::new(Point::ORIGIN, Point::new(0.0, size.height))
            .add_stop(0.0, top.get())
            .add_stop(1.0, bottom.get());
        f.fill(&shape, canvas::Fill::from(tint));
        f.stroke(
            &Path::rounded_rectangle(
                Point::new(BORDER / 2.0, BORDER / 2.0),
                Size::new(size.width - BORDER, size.height - BORDER),
                (RADIUS - BORDER / 2.0).into(),
            ),
            Stroke {
                style: canvas::Style::Solid(RIM.get()),
                width: BORDER,
                ..Stroke::default()
            },
        );
        vec![f.into_geometry()]
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
fn moon(on: bool) -> iced::advanced::svg::Handle {
    static MOONS: std::sync::OnceLock<[iced::advanced::svg::Handle; 2]> =
        std::sync::OnceLock::new();
    let make = |colour: &str| {
        iced::advanced::svg::Handle::from_memory(
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
