//! The bell's history (5.4; by application since #923, the mockup
//! s30-bell), opened from the bell on the bar: 360 px wide, 600 px tall at
//! most, its bottom on the bar's. In this order:
//!
//! - "Notifications" and, when there is something to clear, "Tout effacer"
//!   / "Clear all", a ghost button;
//! - the do not disturb card: a moon, its name and what it does now, a
//!   switch; on, amber, with three chips: for an hour (the default), until
//!   the next morning (8:00), with no limit;
//! - the list: the agents' pins at the top, lit violet with the agent's
//!   name and reason; then each application with something new or noticed,
//!   one card each, the newest first (a lone line as 5.4's, its 28 px icon
//!   and "App · il y a 9 min"; several under a header with "3 de plus" and
//!   its ✕, the newest line and two strips for the rest); its seen ones
//!   under a fold, "12 vues"; under a rule, the applications with seen
//!   ones only, one 34 px line each. A line's blue dot while it is new, its
//!   ✕ and "⋯" under the pointer, its actions while its sender holds it. A
//!   click invokes its default (if it is held) and raises its application's
//!   window, then it is seen and the history closes; ✕ takes it out.
//! - Empty, nothing at all left: the resting panda, "Rien de neuf" and
//!   "Tout est lu.".
//!
//! "Clear all" slides the lines out to the right one after another, 35 ms
//! apart, in 180 ms each (✕ slides its own, an application's ✕ its
//! lines); without motion they go at once. What is unfolded is kept until
//! the shell restarts. The list scrolls with the wheel; `Tab` goes through
//! the buttons, the chips, the lines (each with its ✕ and its actions),
//! the headers' buttons and the folds, the arrows through the lines,
//! `Enter` or `Space` acts, `Esc` closes.

use hocus_words::shell::popover_history as txt;
use std::{
    collections::{BTreeSet, HashMap},
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
        history::{self, Ago, Kept},
        stack::Input as Said,
    },
    popover::kit::MANROPE_SEMIBOLD,
    ui::{
        self, Around, Later, control,
        notched::Notched,
        parts::{drawn, drawn_lit, gap, moving},
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
/// The list: 8 px in, 10 px under; its cards 6 px apart, their lines
/// 2 px apart.
const LIST_TOP: f32 = 8.0;
const LIST_SIDE: f32 = BORDER + 8.0;
const LIST_BOTTOM: f32 = 10.0;
const CARD_APART: f32 = 6.0;
const GAP: f32 = 2.0;
/// An application's header (#923): 8 px in and above, 2 px under; its
/// icon 18 px, 8 px before its name (12 px bold) and "· 4"; "3 de plus" a
/// 22 px ghost button, 8 px in, 11.5 px 600; its ✕ 22 px.
const GROUP_PAD: (f32, f32, f32) = (8.0, 8.0, 2.0);
const SMALL_ICON: f32 = 18.0;
const SMALL_GAP: f32 = 8.0;
const GROUP_NAME: f32 = 12.0;
/// A phone's application's header: the phone's icon before its device's
/// name, and the gaps before it and after it (#957).
const DEVICE_ICON: f32 = 13.0;
const DEVICE_GAP: f32 = 8.0;
const DEVICE_ICON_GAP: f32 = 3.0;
const TOGGLE: f32 = 22.0;
const TOGGLE_PAD: f32 = 8.0;
const TOGGLE_TEXT: f32 = 11.5;
/// The two strips under a card's newest line: 4 px then 3 px, 2 px apart,
/// 14 px in, the second 8 px further in, 2 px above what follows.
const STRIPS: (f32, f32) = (4.0, 3.0);
const STRIPS_IN: (f32, f32) = (14.0, 8.0);
const STRIPS_UNDER: f32 = 2.0;
/// The fold, "12 vues": 26 px, 8 px in, its chevron 10 px, 6 px before its
/// words, 12 px 600; under a lone line, at its text's column.
const FOLD: f32 = 26.0;
const FOLD_PAD: f32 = 8.0;
const CHEVRON: f32 = 10.0;
const CHEVRON_GAP: f32 = 6.0;
const FOLD_TEXT: f32 = 12.0;
const FOLD_RADIUS: f32 = 8.0;
/// The rule above the applications with seen ones only, 8 px in, 2 px
/// from what is around it; their lines 34 px.
const RULE_IN: f32 = 8.0;
const RULE_AROUND: f32 = 2.0;
const SEEN_LINE: f32 = 34.0;
/// A pin's line under its body: 4 px under, its pin 12 px, 5 px apart,
/// 11.5 px.
const PIN_TOP: f32 = 4.0;
const PIN: f32 = 12.0;
const PIN_GAP: f32 = 5.0;
const PIN_TEXT: f32 = 11.5;
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
/// Its "⋯" under its ✕ (#899): an item is at least as tall as both, 2 px
/// under them.
const MORE_TOP: f32 = CLOSE_IN.1 + CLOSE;
const ITEM_LEAST: f32 = MORE_TOP + CLOSE + 2.0;
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
/// The agents' violet, a pin's light and rim (#923).
const AGENT: Ink = Ink::Token(|t| t.agent);
const PINNED: Ink = Ink::Dark(rgba(182, 156, 255, 0.07));
const PINNED_HOVER: Ink = Ink::Dark(rgba(182, 156, 255, 0.11));
const PINNED_RIM: Ink = Ink::Dark(rgba(182, 156, 255, 0.28));
/// The strips under a card, and the rule.
const STRIP_ONE: Ink = Ink::Dark(rgba(255, 255, 255, 0.07));
const STRIP_TWO: Ink = Ink::Dark(rgba(255, 255, 255, 0.04));
const RULE: Ink = Ink::Dark(rgba(255, 255, 255, 0.06));
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

    /// "3 de plus".
    fn more(self, n: usize) -> String {
        txt::n_more(self.lang, n)
    }

    fn collapse(self) -> &'static str {
        self.pick(txt::COLLAPSE)
    }

    /// "12 vues".
    fn seen(self, n: usize) -> String {
        txt::n_seen(self.lang, n as u64)
    }

    fn dismiss(self) -> &'static str {
        self.pick(txt::DISMISS)
    }

    fn clear_app(self, app: &str) -> String {
        txt::clear_app(self.lang, app)
    }

    fn pinned_by(self, agent: &str) -> String {
        txt::pinned_by(self.lang, agent)
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

mod layout;

pub use layout::height;
use layout::{
    App, Block, CUTS, Fold, Folds, Header, Laid, Look, Row, SeenLine, Target, folds, lay_out, shown,
};

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
    /// disturb, what is unfolded.
    from: Option<(u64, i64, Dnd, Folds)>,
    focus: Option<Target>,
    /// The line the pointer is over.
    over: Option<u64>,
    /// The line of seen ones only the pointer is over.
    over_app: Option<App>,
    /// How far the list is scrolled, as it said or was told.
    scroll: f32,
    switch: Switch,
    /// Lines sliding out.
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

/// Lines sliding out, in their order, since then; once they are all out,
/// the stack is told (`sent`) of them and of `also` (those not shown), and
/// they stay out until it takes them.
#[derive(Debug)]
struct Leaving {
    keys: Vec<u64>,
    also: Vec<u64>,
    since: Instant,
    sent: bool,
}

impl Ui {
    /// How far line `key` has slid out at `now`, 0 to 1.
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

    /// When the lines sliding out are all gone.
    fn leaving_end(&self) -> Option<Instant> {
        let Leaving { keys, since, .. } = self.leaving.as_ref()?;
        let shown = u32::try_from(keys.len().clamp(1, 41) - 1).unwrap_or(40);
        Some(*since + STAGGER * shown + SLIDE)
    }

    /// Whether what it shows moves at `now`: lines sliding out, the empty
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
    /// The pointer came over a line (`true`), or left it.
    Over(u64, bool),
    /// Over an application's line of seen ones only, or off it.
    OverApp(App, bool),
    /// `↓` (`true`) or `↑`: through the lines.
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
        ui(|ui| {
            ui.moving(Instant::now(), motion) || ui.from.as_ref().is_some_and(|f| f.1 != minute())
        })
    })
    .keys(arrows)
    .later(&LATER)
    .press(Msg::Pressed)
    .on(iced::widget::canvas(Tint(glass))
        .width(Length::Fill)
        .height(Length::Fill))
}

/// The arrows go through the lines.
fn arrows(pressed: &Pressed) -> Option<Msg> {
    use keyboard::{Key, key::Named};
    match pressed.key {
        Key::Named(Named::ArrowDown) => Some(Msg::Arrow(true)),
        Key::Named(Named::ArrowUp) => Some(Msg::Arrow(false)),
        _ => None,
    }
}

/// The reply field of the line unfolded for it (#957).
static REPLY: Mutex<Option<crate::field::Entry>> = Mutex::new(None);

fn reply_entry(with: &mut dyn FnMut(&mut crate::field::Entry)) {
    let mut reply = REPLY.lock().unwrap_or_else(PoisonError::into_inner);
    with(reply.get_or_insert_with(|| crate::field::Entry::new(crate::field::Kind::Line, 2000)));
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

/// What it shows, read afresh: laid out again when the history, do not
/// disturb, what is unfolded or the minute changed.
fn refresh(ui: &mut Ui, now: Instant) {
    let shown = shown();
    let wall = notifications::wall();
    let minute = wall.timestamp().as_second() / 60;
    let from = (shown.version, minute, shown.dnd, shown.folds.clone());
    if ui.from.as_ref() == Some(&from) && ui.laid.is_some() {
        return;
    }
    let words = Words::new(Lang::of_messages());
    let laid = {
        let mut cuts = CUTS.lock().unwrap_or_else(PoisonError::into_inner);
        let cuts = cuts.get_or_insert_with(HashMap::new);
        // Only what it lists is kept cut.
        cuts.retain(|(key, _, _, _), _| shown.items.iter().any(|k| k.key == *key));
        lay_out(&shown, words, &wall, cuts, &mut kit::measure)
    };
    let said_again = ui
        .from
        .as_ref()
        .is_none_or(|f| (f.0, f.2, &f.3) != (shown.version, shown.dnd, &shown.folds));
    if said_again || ui.laid.is_none() {
        tracing::debug!(dnd = ?laid.dnd.choice(), history = %laid.said(words), "the history shows");
    }
    let was_empty = ui.laid.as_ref().is_some_and(|l| l.blocks.is_empty());
    if laid.blocks.is_empty() && !was_empty && ui.laid.is_some() {
        ui.empty_since = Some(now);
    }
    if laid.blocks.is_empty() && !was_empty && crate::pandas::lively() {
        ui.dozing = Some(crate::pandas::Burst::of(crate::pandas::Mood::Resting, now));
    }
    if ui.focus.is_some_and(|f| !laid.has(f)) {
        ui.focus = None;
    }
    // What slid out is gone from the list: it is over.
    if ui
        .leaving
        .as_ref()
        .is_some_and(|l| l.sent && !laid.rows().any(|r| l.keys.contains(&r.key)))
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
        let clear = ghost(words.clear(), GHOST_TEXT, rect.size(), GHOST_RADIUS, DIM)
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

/// A ghost button saying `word`, `size` px 600 in `ink`: lit and in the
/// text's colour under the pointer.
fn ghost<'a>(
    word: &str,
    size_px: f32,
    size: Size,
    radius: f32,
    ink: Ink,
) -> iced::widget::Button<'a, Msg> {
    button(
        container(
            iced::widget::text(word.to_owned())
                .size(size_px)
                .font(MANROPE_SEMIBOLD)
                .line_height(LineHeight::Relative(LINE))
                .shaping(iced::widget::text::Shaping::Advanced)
                .wrapping(iced::widget::text::Wrapping::None),
        )
        .center(Length::Fill),
    )
    .padding(0)
    .width(size.width)
    .height(size.height)
    .style(move |_, status| {
        let lit = style::lit(status);
        let mut quiet = if lit {
            style::hovered(GHOST_HOVER.get(), radius)
        } else {
            style::clear()
        };
        quiet.text_color = if lit { TEXT.get() } else { ink.get() };
        quiet
    })
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

/// The list: its pins, its applications' cards and folds, scrolling past
/// its room; empty, the resting panda.
fn list<'a>(laid: &Laid, words: Words, ui: &Ui, now: Instant, motion: bool) -> El<'a> {
    let width = laid.view.width;
    if laid.blocks.is_empty() {
        return empty(laid, words, ui, now, motion);
    }
    let mut all = column![].width(width);
    let mut end = 0.0;
    for block in &laid.blocks {
        let rect = block.rect();
        all = all.push(Space::new().height((rect.y - end).max(0.0)));
        all = all.push(match block {
            Block::Line(row) => item(row, words, ui, now),
            Block::Header(header) => group_header(header, words, ui),
            Block::Strips(rect, _) => strips(*rect),
            Block::Fold(fold) => fold_line(fold, ui),
            Block::Rule(rect) => rule(*rect),
            Block::SeenLine(line) => seen_line(line, words, ui),
        });
        end = rect.y + rect.height;
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

/// An application's icon, `size` px, else its first letter on a square.
fn app_icon<'a>(icon: Option<&Icon>, letter: &str, size: f32, alpha: f32) -> El<'a> {
    let fade = move |c: Color| c.scale_alpha(alpha);
    match icon {
        Some(Icon::Raster(handle)) => image(handle.clone())
            .width(size)
            .height(size)
            .opacity(alpha)
            .into(),
        Some(Icon::Vector(handle)) => svg(handle.clone())
            .width(size)
            .height(size)
            .opacity(alpha)
            .into(),
        None => {
            let square = drawn(size, size, move |f, s| {
                let rect = Rectangle::new(Point::ORIGIN, s);
                let fill = canvas::gradient::Linear::new(Point::ORIGIN, Point::new(size, size))
                    .add_stop(0.0, fade(NEUTRAL.0))
                    .add_stop(1.0, fade(NEUTRAL.1));
                f.fill(
                    &kit::rounded(rect, (size * 0.28).round()),
                    canvas::Fill::from(fill),
                );
            });
            Stack::with_children([
                square,
                container(said(
                    letter.to_owned(),
                    size * 0.4,
                    MANROPE_BOLD,
                    fade(Color::WHITE),
                ))
                .center(size)
                .into(),
            ])
            .into()
        }
    }
}

/// A cross, `ink`, drawn in a `CLOSE` px box while `shows`.
fn cross<'a>(shows: bool, ink: Color) -> El<'a> {
    drawn(CLOSE, CLOSE, move |f, s| {
        if !shows {
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
                style: canvas::Style::Solid(ink),
                width: 1.4,
                line_cap: canvas::LineCap::Round,
                ..Stroke::default()
            },
        );
    })
}

/// A ✕ or "⋯" button, `drawn`, lit under the pointer while it `shows`.
fn small<'a>(
    drawn: El<'a>,
    shows: bool,
    alpha: f32,
    target: Target,
) -> iced::widget::Button<'a, Msg> {
    button(drawn)
        .padding(0)
        .style(move |_, status| {
            if shows && style::lit(status) {
                style::hovered(GHOST_HOVER.get().scale_alpha(alpha), CLOSE_RADIUS)
            } else {
                style::clear()
            }
        })
        .on_press(Msg::Act(target))
}

/// The chevron of a fold: pointing right, or down while unfolded.
fn chevron<'a>(open: bool, ink: Color) -> El<'a> {
    drawn(CHEVRON, CHEVRON, move |f, s| {
        let k = s.width / 10.0;
        let path = Path::new(|p| {
            if open {
                p.move_to(Point::new(2.0 * k, 3.5 * k));
                p.line_to(Point::new(5.0 * k, 6.5 * k));
                p.line_to(Point::new(8.0 * k, 3.5 * k));
            } else {
                p.move_to(Point::new(3.5 * k, 2.0 * k));
                p.line_to(Point::new(6.5 * k, 5.0 * k));
                p.line_to(Point::new(3.5 * k, 8.0 * k));
            }
        });
        f.stroke(
            &path,
            Stroke {
                style: canvas::Style::Solid(ink),
                width: 1.6 * k,
                line_cap: canvas::LineCap::Round,
                line_join: canvas::LineJoin::Round,
                ..Stroke::default()
            },
        );
    })
}

/// A line: its icon and "App · when" (a lone one, a pin) or its age first
/// (under its application's header or fold), its title, its body; under
/// the pointer or holding the keyboard, lit with its ✕ and "⋯"; its
/// actions while its sender holds it; a pin's agent and reason; sliding
/// out as it goes.
fn item<'a>(shown: &Row, words: Words, ui: &Ui, now: Instant) -> El<'a> {
    let key = shown.key;
    let gone = ui.gone(key, now);
    let alpha = 1.0 - gone;
    let fade = move |c: Color| c.scale_alpha(alpha);
    let over = ui.over == Some(key) && ui.leaving.is_none();
    let lit = over || ui.focus == Some(Target::Item(key));
    let size = shown.rect.size();
    let fr = words.lang == hocus_words::Lang::French;
    let full = shown.look == Look::Full;
    let mut meta = row![].align_y(Vertical::Center);
    if full {
        meta = meta
            .push(said(
                shown.cut.app.clone(),
                META,
                MANROPE_BOLD,
                fade(DIM.get()),
            ))
            .push(gap(META_GAP))
            .push(said("·", META, MANROPE, fade(QUIET.get())))
            .push(gap(META_GAP));
    }
    meta = meta.push(said(shown.when.clone(), META, MANROPE, fade(QUIET.get())));
    if shown.dot {
        meta = meta.push(gap(META_GAP)).push(drawn(DOT, DOT, move |f, s| {
            f.fill(
                &Path::circle(Point::new(s.width / 2.0, s.height / 2.0), DOT / 2.0),
                fade(UNSEEN.get()),
            );
        }));
    }
    // A seen one is quieter.
    let (title_font, title_ink, body_ink) = if shown.seen {
        (MANROPE_SEMIBOLD, DIM.get(), QUIET.get())
    } else {
        (MANROPE_BOLD, TEXT.get(), DIM.get())
    };
    let mut texts = column![
        container(meta)
            .height(META * LINE)
            .align_y(Vertical::Center),
        Space::new().height(TEXT_GAP),
        said(
            shown.cut.title.clone(),
            ITEM_TITLE,
            title_font,
            fade(title_ink)
        ),
    ];
    if !shown.cut.body.is_empty() {
        texts = texts.push(Space::new().height(TEXT_GAP)).push(lines(
            &shown.cut.body,
            BODY,
            BODY_LINE,
            fade(body_ink),
        ));
    }
    // A pin: its pin, the agent's name, its reason.
    if let Some(agent) = &shown.pin {
        let mut by = row![
            svg(pin_icon()).width(PIN).height(PIN).opacity(alpha),
            gap(PIN_GAP),
            said(agent.clone(), PIN_TEXT, MANROPE_BOLD, fade(AGENT.get())),
        ]
        .align_y(Vertical::Center);
        if let Some(reason) = shown.cut.reason.as_ref().filter(|r| !r.is_empty()) {
            by = by.push(gap(PIN_GAP)).push(said(
                format!("· {reason}"),
                PIN_TEXT,
                MANROPE,
                fade(DIM.get()),
            ));
        }
        texts = texts.push(Space::new().height(PIN_TOP)).push(
            container(by)
                .height(PIN_TEXT * LINE)
                .align_y(Vertical::Center),
        );
    }
    let pinned = shown.pin.is_some();
    let pressed = button(Space::new().width(size.width).height(size.height))
        .padding(0)
        .style(move |_, _| match (pinned, lit) {
            (true, _) => {
                let fill = if lit { PINNED_HOVER } else { PINNED };
                let boxed = rimmed(fade(fill.get()), fade(PINNED_RIM.get()), ITEM_RADIUS);
                iced::widget::button::Style {
                    background: boxed.background,
                    border: boxed.border,
                    ..iced::widget::button::Style::default()
                }
            }
            (false, true) => style::hovered(fade(ITEM_HOVER.get()), ITEM_RADIUS),
            (false, false) => style::clear(),
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
    if let Some(agent) = &shown.pin {
        body = format!("{body}. {}", words.pinned_by(agent));
    }
    let mut layers: Vec<El<'a>> = vec![
        declared(pressed, Target::Item(key), Role::ListItem, name, ui.focus)
            .node(|n| n.describe(body))
            .into(),
    ];
    if full {
        layers.push(at(
            app_icon(shown.icon.as_ref(), &shown.letter, ICON, alpha),
            Point::new(ITEM_PAD, ITEM_PAD),
        ));
    }
    layers.push(at(texts, Point::new(shown.text_x(), ITEM_PAD)));
    // Its ✕, under the pointer or holding the keyboard.
    let close_target = Target::Close(key);
    let shows_close = over || ui.focus == Some(close_target);
    layers.push(at(
        declared(
            small(
                cross(shows_close, fade(FAINT.get())),
                shows_close,
                alpha,
                close_target,
            ),
            close_target,
            Role::Button,
            words.dismiss(),
            ui.focus,
        ),
        Point::new(size.width - CLOSE_IN.0 - CLOSE, CLOSE_IN.1),
    ));
    // Its "⋯" under it (#899): not for the desktop's own (a capture, a
    // meeting's reminder), whose application has no level.
    if shown.leveled {
        let more_target = Target::More(key);
        let shows_more = over || ui.focus == Some(more_target);
        let dots = drawn(CLOSE, CLOSE, move |f, s| {
            if !shows_more {
                return;
            }
            for dx in [-4.0, 0.0, 4.0] {
                f.fill(
                    &Path::circle(Point::new(s.width / 2.0 + dx, s.height / 2.0), 1.2),
                    fade(FAINT.get()),
                );
            }
        });
        layers.push(at(
            declared(
                small(dots, shows_more, alpha, more_target),
                more_target,
                Role::Button,
                hocus_words::shell::notifications_draw::MORE.get(words.lang),
                ui.focus,
            ),
            Point::new(size.width - CLOSE_IN.0 - CLOSE, MORE_TOP),
        ));
    }
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
    // A phone's reply field, unfolded by its "Répondre" (#957).
    if let Some(rect) = shown.reply {
        let field_target = Target::ReplyField(key);
        let send_target = Target::ReplySend(key);
        let focused = ui.focus == Some(field_target);
        let lang = words.lang;
        let field = crate::ui::field::Field {
            with: Box::new(reply_entry),
            style: crate::field::view::Style::one(12.5, MANROPE),
            placeholder: hocus_words::shell::phone::REPLY.get(lang).to_owned(),
            look: crate::field::view::Look::of(TEXT.get(), FAINT.get()),
            focused,
            left: 10.0,
            right: 10.0,
            on_focus: Msg::Focus(field_target),
            on_took: |_| Msg::Stale,
        }
        .view(layout::REPLY_ROW);
        let boxed = container(field)
            .width(rect.width - layout::REPLY_ROW - 6.0)
            .style(move |_| iced::widget::container::Style {
                background: Some(crate::notifications::draw::REPLY_FILL.get().into()),
                border: iced::Border {
                    color: if focused {
                        crate::notifications::draw::REPLY_RIM_FOCUSED.get()
                    } else {
                        crate::notifications::draw::REPLY_RIM.get()
                    },
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..iced::widget::container::Style::default()
            });
        layers.push(at(
            control(
                boxed,
                field_target.id(),
                Role::TextField,
                hocus_words::shell::phone::REPLY_LINE.get(lang),
            )
            .focus(focused, Msg::Focus(field_target), field_target.radius())
            .activate(Msg::Act(send_target))
            .typing(),
            Point::new(rect.x - shown.rect.x, rect.y - shown.rect.y),
        ));
        let send = button(drawn_lit(
            layout::REPLY_ROW,
            layout::REPLY_ROW,
            0.0,
            crate::notifications::draw::draw_send,
        ))
        .padding(0)
        .style(|_, _| style::clear())
        .on_press(Msg::Act(send_target));
        layers.push(at(
            declared(
                send,
                send_target,
                Role::Button,
                hocus_words::shell::phone::SEND_REPLY.get(lang),
                ui.focus,
            ),
            Point::new(
                rect.x + rect.width - layout::REPLY_ROW - shown.rect.x,
                rect.y - shown.rect.y,
            ),
        ));
    }
    let whole = Stack::with_children(layers)
        .width(size.width)
        .height(size.height);
    let mut hovered = mouse_area(whole)
        .on_enter(Msg::Over(key, true))
        .on_exit(Msg::Over(key, false));
    if shown.leveled {
        hovered = hovered.on_right_press(Msg::Act(Target::More(key)));
    }
    row![gap(SLIDE_BY * gone), hovered].into()
}

/// An application's header: its icon, its name and "· 4", "3 de plus" (or
/// "Réduire") and its ✕.
fn group_header<'a>(header: &Header, words: Words, ui: &Ui) -> El<'a> {
    let size = header.rect.size();
    let mut name = row![
        said(header.name.clone(), GROUP_NAME, MANROPE_BOLD, DIM.get()),
        said(
            format!(" · {}", header.count),
            GROUP_NAME,
            MANROPE,
            QUIET.get()
        ),
    ]
    .align_y(Vertical::Center);
    // A phone's application: the phone's 13 px icon and its device (#957).
    if let Some(device) = &header.device {
        let icon = drawn(DEVICE_ICON, DEVICE_ICON, |f, s| {
            crate::phone::draw::kind_icon(
                f,
                Point::ORIGIN,
                s.width,
                crate::phone::Kind::Phone,
                FAINT.get(),
            );
        });
        name = name
            .push(gap(DEVICE_GAP))
            .push(icon)
            .push(gap(DEVICE_ICON_GAP))
            .push(said(device.clone(), GROUP_NAME, MANROPE, FAINT.get()));
    }
    let (label, toggle) = &header.toggle;
    let expand = Target::Expand(header.app);
    let close = Target::CloseApp(header.app);
    let mut layers: Vec<El<'a>> = vec![
        Space::new().width(size.width).height(size.height).into(),
        at(
            app_icon(header.icon.as_ref(), &header.letter, SMALL_ICON, 1.0),
            Point::new(GROUP_PAD.0, GROUP_PAD.1 + (TOGGLE - SMALL_ICON) / 2.0),
        ),
        at(
            container(name).height(TOGGLE).align_y(Vertical::Center),
            Point::new(GROUP_PAD.0 + SMALL_ICON + SMALL_GAP, GROUP_PAD.1),
        ),
    ];
    let expanded = label == words.collapse();
    layers.push(at(
        declared(
            ghost(label, TOGGLE_TEXT, toggle.size(), CLOSE_RADIUS, FAINT)
                .on_press(Msg::Act(expand)),
            expand,
            Role::Button,
            label.clone(),
            ui.focus,
        )
        .node(|n| n.expanded(expanded)),
        toggle.position(),
    ));
    layers.push(at(
        declared(
            small(cross(true, FAINT.get()), true, 1.0, close),
            close,
            Role::Button,
            words.clear_app(&header.whole),
            ui.focus,
        ),
        header.close.position(),
    ));
    Stack::with_children(layers)
        .width(size.width)
        .height(size.height)
        .into()
}

/// The strips under a card's newest line: 4 px then 3 px, rounded below.
fn strips<'a>(rect: Rectangle) -> El<'a> {
    let strips = drawn(rect.width, rect.height, move |f, s| {
        let rounded_below = |r: Rectangle, radius: f32| {
            Path::new(|p| {
                p.move_to(Point::new(r.x, r.y));
                p.line_to(Point::new(r.x + r.width, r.y));
                p.line_to(Point::new(r.x + r.width, r.y + r.height - radius));
                p.quadratic_curve_to(
                    Point::new(r.x + r.width, r.y + r.height),
                    Point::new(r.x + r.width - radius, r.y + r.height),
                );
                p.line_to(Point::new(r.x + radius, r.y + r.height));
                p.quadratic_curve_to(
                    Point::new(r.x, r.y + r.height),
                    Point::new(r.x, r.y + r.height - radius),
                );
                p.close();
            })
        };
        let one = Rectangle::new(Point::ORIGIN, Size::new(s.width, STRIPS.0));
        f.fill(&rounded_below(one, STRIPS.0), STRIP_ONE.get());
        let two = Rectangle::new(
            Point::new(STRIPS_IN.1, STRIPS.0 + GAP),
            Size::new(s.width - 2.0 * STRIPS_IN.1, STRIPS.1),
        );
        f.fill(&rounded_below(two, STRIPS.1), STRIP_TWO.get());
    });
    row![gap(rect.x - LIST_SIDE), strips].into()
}

/// A fold: "12 vues", its chevron; lit under the pointer.
fn fold_line<'a>(fold: &Fold, ui: &Ui) -> El<'a> {
    let target = Target::Fold(fold.app);
    let open = fold.open;
    let focused = ui.focus == Some(target);
    let said_now = fold.said.clone();
    let pressed = button(
        container(
            row![
                chevron(open, FAINT.get()),
                gap(CHEVRON_GAP),
                iced::widget::text(said_now.clone())
                    .size(FOLD_TEXT)
                    .font(MANROPE_SEMIBOLD)
                    .line_height(LineHeight::Relative(LINE))
                    .shaping(iced::widget::text::Shaping::Advanced)
                    .wrapping(iced::widget::text::Wrapping::None),
            ]
            .align_y(Vertical::Center),
        )
        .padding(Padding {
            left: FOLD_PAD,
            right: FOLD_PAD,
            ..Padding::ZERO
        })
        .center_y(Length::Fill),
    )
    .padding(0)
    .width(fold.rect.width)
    .height(fold.rect.height)
    .style(move |_, status| {
        let lit = style::lit(status) || focused;
        let mut quiet = if lit {
            style::hovered(ITEM_HOVER.get(), FOLD_RADIUS)
        } else {
            style::clear()
        };
        quiet.text_color = if lit { DIM.get() } else { FAINT.get() };
        quiet
    })
    .on_press(Msg::Act(target));
    let x = fold.rect.x - LIST_SIDE;
    row![
        gap(x),
        declared(pressed, target, Role::Button, said_now, ui.focus).node(|n| n.expanded(open)),
    ]
    .into()
}

/// The rule above the applications with seen ones only.
fn rule<'a>(rect: Rectangle) -> El<'a> {
    row![
        gap(rect.x - LIST_SIDE),
        container(Space::new().width(rect.width).height(rect.height))
            .style(|_| style::boxed(RULE.get(), 0.0)),
    ]
    .into()
}

/// An application with seen ones only: its icon, its name, "· 6 vues",
/// its newest's age and the chevron; its ✕ under the pointer.
fn seen_line<'a>(line: &SeenLine, words: Words, ui: &Ui) -> El<'a> {
    let size = line.rect.size();
    let target = Target::Fold(line.app);
    let close = Target::CloseApp(line.app);
    let open = line.open;
    let focused = ui.focus == Some(target);
    let pressed = button(
        container(
            row![
                app_icon(line.icon.as_ref(), &line.letter, SMALL_ICON, 1.0),
                gap(SMALL_GAP),
                said(line.name.clone(), GROUP_NAME, MANROPE_BOLD, DIM.get()),
                gap(SMALL_GAP),
                said(line.said.clone(), GROUP_NAME, MANROPE, QUIET.get()),
                Space::new().width(Length::Fill),
                said(line.when.clone(), GROUP_NAME, MANROPE, QUIET.get()),
                gap(SMALL_GAP),
                chevron(open, QUIET.get()),
            ]
            .align_y(Vertical::Center),
        )
        .padding(Padding {
            left: ITEM_PAD,
            right: ITEM_RIGHT,
            ..Padding::ZERO
        })
        .center_y(Length::Fill),
    )
    .padding(0)
    .width(size.width)
    .height(size.height)
    .style(move |_, status| {
        if style::lit(status) || focused {
            style::hovered(ITEM_HOVER.get(), ITEM_RADIUS)
        } else {
            style::clear()
        }
    })
    .on_press(Msg::Act(target));
    let name = format!("{} {}", line.name, line.said.trim_start_matches("· "));
    let shows_close = ui.over_app == Some(line.app) || ui.focus == Some(close);
    let whole = Stack::with_children([
        declared(pressed, target, Role::Button, name, ui.focus)
            .node(|n| n.expanded(open))
            .into(),
        at(
            declared(
                small(cross(shows_close, FAINT.get()), shows_close, 1.0, close),
                close,
                Role::Button,
                words.clear_app(&line.name),
                ui.focus,
            ),
            Point::new(size.width - CLOSE_IN.0 - CLOSE, (SEEN_LINE - CLOSE) / 2.0),
        ),
    ])
    .width(size.width)
    .height(size.height);
    mouse_area(whole)
        .on_enter(Msg::OverApp(line.app, true))
        .on_exit(Msg::OverApp(line.app, false))
        .into()
}

/// A pin, in the agents' violet.
fn pin_icon() -> iced::advanced::svg::Handle {
    static PIN_SVG: std::sync::OnceLock<iced::advanced::svg::Handle> = std::sync::OnceLock::new();
    PIN_SVG
        .get_or_init(|| {
            iced::advanced::svg::Handle::from_memory(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" width=\"24\" \
                 height=\"24\"><path d=\"M9 3h6l-1 6 4 4H6l4-4z M12 13v8\" fill=\"none\" \
                 stroke=\"#B69CFF\" stroke-width=\"2\" stroke-linejoin=\"round\" \
                 stroke-linecap=\"round\"/></svg>"
                    .as_bytes()
                    .to_vec(),
            )
        })
        .clone()
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
                Some(
                    leaving
                        .keys
                        .iter()
                        .chain(&leaving.also)
                        .copied()
                        .collect::<Vec<u64>>(),
                )
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
        Msg::OverApp(app, true) => ui(|ui| ui.over_app = Some(app)),
        Msg::OverApp(app, false) => ui(|ui| {
            if ui.over_app == Some(app) {
                ui.over_app = None;
            }
        }),
        Msg::Scrolled(y) => ui(|ui| ui.scroll = y),
        Msg::Arrow(down) => ui(|ui| {
            let items: Vec<Target> = ui
                .laid
                .as_ref()
                .map(|l| l.rows().map(|r| Target::Item(r.key)).collect())
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
    let (Some(laid), Some(focus)) = (&ui.laid, ui.focus) else {
        return;
    };
    let Some(rect) = laid.place_of(focus) else {
        return;
    };
    let to = kit::reveal(
        (rect.y, rect.height),
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

/// Lines `keys` go, and `also` with them (not shown): sliding out one
/// after another, or at once without motion.
fn take_out(
    ui: &mut Ui,
    keys: Vec<u64>,
    also: Vec<u64>,
    now: Instant,
    motion: bool,
) -> Option<Vec<u64>> {
    if (keys.is_empty() && also.is_empty()) || ui.leaving.is_some() {
        return None;
    }
    if motion && !keys.is_empty() {
        ui.leaving = Some(Leaving {
            keys,
            also,
            since: now,
            sent: false,
        });
        None
    } else {
        Some(keys.into_iter().chain(also).collect())
    }
}

/// The keys of application `app`'s lines but the pinned ones, newest
/// first.
fn keys_of_app(app: App) -> Vec<u64> {
    let id = app.id();
    notifications::with(|c| {
        c.stack
            .history
            .items()
            .iter()
            .filter(|k| k.pin.is_none() && k.app_id().to_lowercase() == id)
            .map(|k| k.key)
            .collect()
    })
}

fn act(target: Target, now: Instant, motion: bool) {
    tracing::debug!(?target, "the history");
    let mut more = None;
    let said: Option<Said> = ui(|ui| {
        let laid = ui.laid.clone()?;
        match target {
            Target::Clear => {
                // The lines in view first, in their order: they slide out;
                // the folded and the hidden with them.
                let mut keys: Vec<u64> = laid.rows().map(|r| r.key).collect();
                let first = laid
                    .rows()
                    .position(|r| r.rect.y + r.rect.height > ui.scroll)
                    .unwrap_or(0);
                keys.rotate_left(first);
                let also: Vec<u64> = notifications::with(|c| {
                    c.stack
                        .history
                        .items()
                        .iter()
                        .map(|k| k.key)
                        .filter(|k| !keys.contains(k))
                        .collect()
                });
                take_out(ui, keys, also, now, motion).map(Said::Remove)
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
            Target::Close(key) => {
                take_out(ui, vec![key], Vec::new(), now, motion).map(Said::Remove)
            }
            Target::More(key) => {
                // Its application's levels, in the history's place, the
                // menu's top on its "⋯"'s.
                let row = laid.rows().find(|r| r.key == key)?;
                more = Some((laid.view.y + row.rect.y - ui.scroll + MORE_TOP, key));
                None
            }
            Target::Action(key, i) => {
                let action = laid
                    .rows()
                    .find(|r| r.key == key)
                    .and_then(|r| r.actions.get(i))
                    .map(|(action, _)| action.clone())?;
                // A phone's "Répondre" unfolds its field, or folds it.
                if action == layout::REPLY_KEY {
                    let unfolds = folds(|f| {
                        f.replying = (f.replying != Some(key)).then_some(key);
                        f.replying.is_some()
                    });
                    reply_entry(&mut |entry| entry.clear());
                    ui.focus = unfolds.then_some(Target::ReplyField(key));
                    return None;
                }
                Some(Said::Act { key, action })
            }
            Target::ReplyField(_) => None,
            Target::ReplySend(key) => {
                let mut text = String::new();
                reply_entry(&mut |entry| text = entry.take());
                if text.trim().is_empty() {
                    return None;
                }
                let id = notifications::with(|c| c.stack.history.get(key).map(|k| k.note.id));
                if let Some(id) = id {
                    notifications::reply(id, text, None);
                }
                folds(|f| f.replying = None);
                ui.focus = None;
                None
            }
            Target::Expand(app) => {
                folds(|f| {
                    if !f.expanded.remove(&app) {
                        f.expanded.insert(app);
                    }
                });
                None
            }
            Target::Fold(app) => {
                folds(|f| {
                    if !f.unfolded.remove(&app) {
                        f.unfolded.insert(app);
                    }
                });
                None
            }
            // All its lines show, then slide out.
            Target::CloseApp(app) => {
                folds(|f| {
                    f.expanded.insert(app);
                    f.unfolded.insert(app);
                });
                take_out(ui, keys_of_app(app), Vec::new(), now, motion).map(Said::Remove)
            }
        }
    });
    if let Some(said) = said {
        let open = matches!(said, Said::Open(_));
        notifications::input(said);
        if open {
            event(Seen::Chosen);
        }
    }
    let app = more.and_then(|(top, key)| {
        notifications::with(|c| {
            c.stack
                .history
                .get(key)
                .map(|k| (top, k.note.app_id(), k.note.app.clone()))
        })
    });
    if let Some((top, id, name)) = app {
        super::notify_levels::from_history(top, &id, &name);
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
