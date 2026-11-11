//! The panda peeking out of the bar's edge (bar.md §7 C, 4.5): when the
//! compositor says so (`peek`), the agent's panda (200 px since #649;
//! holding out its hat when it waits, its arms up when it finished) and its
//! bubble ("**site-web** t'attend · à l'instant", "**api-paiements** a
//! fini") on a layer surface of their own beside each bar, 18 px from it,
//! at the agent's row's height. The compositor moves the surface out from
//! under the bar and back, and says when it is back (`no-peek`): the
//! surfaces then go, and nothing is drawn at rest. While it is out, its
//! panda plays its mood (waiting: its hat sways, its ear rings, its "?"
//! bobs; done: it tosses its hat), from its first frame for the 8 s it is
//! out, then lies still; the winking one waves its hat twice. Not with
//! reduced motion, in power saver or on the CPU: the still drawing, drawn
//! once. A newer peek takes the surfaces of the one out.
//!
//! When windows move with a screen gone or back (`screens-moved`,
//! windows.md §12), the winking panda comes out the same way with a blue
//! bubble of two lines: "**L'écran externe 34 pouces est parti.**" /
//! "Firefox et foot sont ici, sur le bureau 1."; a click on it sends it
//! back.
//!
//! The surface takes the pointer only where it draws: the bubble and the
//! panda. A click on either goes to the agent (§8, 4.3), the streak from
//! the panda, and the compositor sends it back at once.
//!
//! On a GPU the surface is transparent: the bubble's and the panda's
//! shadows are drawn in it (SVG filters, as the mockup's CSS). On the CPU
//! its buffer has no alpha: the bubble's card holds the panda too, and the
//! compositor cuts it to its rounded corners.

use hocus_words::shell::agents_peek as txt;
use std::{
    collections::HashMap,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use hocus_link::{EDGE, Form, PEEK_NAMESPACE, Peek, PeekKind, ScreensMoved};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme,
    advanced::svg,
    mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path},
        text::Alignment,
    },
    window,
};

use super::{Words, now_us};
use crate::pandas::{self, Burst, Mood};
use crate::theme::Ink;
use crate::{
    clock::time::Lang,
    levels::draw::{FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb},
    popover::{self, Effect, kit},
};

// --- The mockup's sizes, at ×1 ----------------------------------------------------

/// The panda's box, and its gap to the bubble; the box's distance from
/// the bar.
pub const PANDA: f32 = 200.0;
/// How long its panda plays its mood from its first frame: the 8 s it is
/// out (the compositor's `peek::STAYS`), settling as it goes back; the
/// winking one waves its hat twice (two 1.1 s waves).
const PLAYS: Duration = Duration::from_millis(7700);
const WAVES: Duration = Duration::from_millis(2200);
const GAP: f32 = 8.0;
pub const FROM_BAR: f32 = 18.0;
/// The bubble: Manrope 13 px, 8 px and 12 px in, a 1 px border, radius 12.
const TEXT_SIZE: f32 = 13.0;
const PAD_Y: f32 = 8.0;
const PAD_X: f32 = 12.0;
const BORDER: f32 = 1.0;
const RADIUS: f32 = 12.0;
/// A name longer than this is cut with an ellipsis: the bubble stays
/// beside the bar.
const NAME_ROOM: f32 = 280.0;
/// `rgba(30,26,18,0.92)`, and under the pointer `rgba(40,32,18,0.96)`.
const BUBBLE: Ink = Ink::Dark(Color::from_rgba8(30, 26, 18, 0.92));
const BUBBLE_HOVER: Ink = Ink::Dark(Color::from_rgba8(40, 32, 18, 0.96));
/// Its border, the state's colour at 45 %: amber waiting, green finished.
const BORDER_WAITING: Ink = Ink::Dark(Color::from_rgba8(245, 180, 74, 0.45));
const BORDER_DONE: Ink = Ink::Accent(0.45);
/// Screens moved: blue, an information with nothing to do;
/// `rgba(16,24,44,0.95)`, its border `#8FB3FF` at 45 %, its first line
/// `#B9CEFF`.
const BUBBLE_SCREENS: Ink = Ink::Dark(Color::from_rgba8(16, 24, 44, 0.95));
const BUBBLE_SCREENS_HOVER: Ink = Ink::Dark(Color::from_rgba8(22, 32, 58, 0.97));
const BORDER_SCREENS: Ink = Ink::Dark(Color::from_rgba8(143, 179, 255, 0.45));
const WORD_SCREENS: Ink = Ink::Dark(rgb(0xB9CEFF));
/// A line of the screens' bubble longer than this is cut.
const SCREENS_ROOM: f32 = 340.0;
/// The word after the name, in the rows' colours.
const WORD_WAITING: Ink = Ink::Dark(rgb(0xF5C46E));
const WORD_DONE: Ink = Ink::Dark(rgb(0x7EE0A0));
/// The bubble's `box-shadow: 0 10px 30px rgba(0,0,0,0.35)` and the
/// panda's `drop-shadow(0 8px 16px rgba(0,0,0,0.4))`: their offset, their
/// deviation (half the CSS blur), their alpha.
const BUBBLE_SHADOW: (f32, f32, f32) = (10.0, 15.0, 0.35);
const PANDA_SHADOW: (f32, f32, f32) = (8.0, 8.0, 0.4);
/// Room around the bubble and the panda for their shadows (three
/// deviations, and the offset): left, top, right, bottom.
const ROOM: (f32, f32, f32, f32) = (45.0, 14.0, 24.0, 30.0);
/// On the CPU, the card's room right of the panda.
const CARD_END: f32 = 8.0;
/// The pandas' drawings' box (their view box, 126 × 120.5 units).
const PANDA_BOX: (f32, f32) = (126.0, 120.5);

/// The bubble's height: `lines` of 13 px text, its padding and border.
fn bubble_height(lines: usize) -> f32 {
    lines.max(1) as f32 * TEXT_SIZE * LINE + 2.0 * (PAD_Y + BORDER)
}

/// Where everything is on a peek's surface, and where the surface is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    /// The surface's size, and its layer-shell margins (top, right,
    /// bottom, left; anchored to the top right).
    pub size: (u32, u32),
    pub margin: (i32, i32, i32, i32),
    /// On the surface.
    pub bubble: Rectangle,
    pub panda: Rectangle,
    /// The panda in the bar's frame: from the bar's right edge, its top.
    pub in_bar: Rectangle,
}

/// A peek of a bubble `width` wide and `lines` high beside a bar in
/// `form`, the panda's middle `middle` under the bar's top (the panda
/// kept below the bar's top); `card`: on the CPU.
pub fn placed(form: Form, middle: f32, width: f32, lines: usize, card: bool) -> Placed {
    // In whole pixels: the panda's box keeps its place from the bar.
    let width = width.ceil();
    let height = bubble_height(lines);
    let (left, top, right, bottom) = if card {
        (0.0, 0.0, CARD_END, 0.0)
    } else {
        ROOM
    };
    let bubble = Rectangle::new(
        Point::new(left, top + (PANDA - height) / 2.0),
        Size::new(width, height),
    );
    let panda = Rectangle::new(Point::new(left + width + GAP, top), Size::new(PANDA, PANDA));
    let size = (
        (panda.x + PANDA + right).ceil() as u32,
        (top + PANDA + bottom).ceil() as u32,
    );
    let bar = form.width() as f32;
    let panda_top = (middle - PANDA / 2.0).max(0.0);
    let margin = (
        (EDGE as f32 + panda_top - top).round() as i32,
        (EDGE as f32 + bar + FROM_BAR - right).round() as i32,
        0,
        0,
    );
    let in_bar = Rectangle::new(
        Point::new(-bar - FROM_BAR - PANDA, panda_top),
        Size::new(PANDA, PANDA),
    );
    let placed = Placed {
        size,
        margin,
        bubble,
        panda,
        in_bar,
    };
    if crate::side::left() {
        mirrored(placed)
    } else {
        placed
    }
}

/// `placed` beside a bar on the left: the surface's margins are mirrored
/// as it opens (`crate::side`), and what is on it here, the panda next to
/// the bar and the bubble past it; the panda in the bar's frame right of
/// its right edge.
fn mirrored(placed: Placed) -> Placed {
    let width = placed.size.0 as f32;
    let flip = |r: Rectangle| Rectangle::new(Point::new(width - r.x - r.width, r.y), r.size());
    Placed {
        bubble: flip(placed.bubble),
        panda: flip(placed.panda),
        in_bar: Rectangle::new(Point::new(FROM_BAR, placed.in_bar.y), placed.in_bar.size()),
        ..placed
    }
}

// --- What it says -------------------------------------------------------------------

/// The bubble's text: the name in bold, the word in the state's colour,
/// since when in `text.faint` (a waiting agent's); each piece's start on
/// its line, and its line (from 0).
#[derive(Debug, Clone, PartialEq)]
pub struct Said {
    pub pieces: Vec<(String, f32, Color, iced::Font, usize)>,
    /// The bubble's width.
    pub width: f32,
    pub lines: usize,
}

/// What a peek of `kind` for `name`, `minutes` since it began, says, in
/// `words`, `measure` giving a text's width at 13 px in a font.
pub fn said(
    words: Words,
    kind: PeekKind,
    name: &str,
    minutes: u64,
    mut measure: impl FnMut(&str, iced::Font) -> f32,
) -> Said {
    let name = kit::elide(name, NAME_ROOM, |t| measure(t, MANROPE_BOLD));
    let space = (measure("x x", MANROPE) - 2.0 * measure("x", MANROPE)).max(0.0);
    let (word, colour) = match kind {
        PeekKind::Waiting => (words.peek(kind), WORD_WAITING),
        PeekKind::Done => (words.peek(kind), WORD_DONE),
    };
    let mut pieces = Vec::new();
    let mut x = PAD_X + BORDER;
    let name_width = measure(&name, MANROPE_BOLD);
    pieces.push((name, x, TEXT.get(), MANROPE_BOLD, 0));
    x += name_width + space;
    pieces.push((word.to_owned(), x, colour.get(), MANROPE, 0));
    x += measure(word, MANROPE);
    if kind == PeekKind::Waiting {
        let since = words.since(minutes, false);
        x += space;
        pieces.push((since.clone(), x, FAINT.get(), MANROPE, 0));
        x += measure(&since, MANROPE);
    }
    Said {
        pieces,
        width: x + PAD_X + BORDER,
        lines: 1,
    }
}

/// The two lines saying windows moved with a screen, in `lang`, their
/// applications' names `names` (one per window, empty when unknown).
pub fn screens_lines(lang: Lang, moved: &ScreensMoved, names: &[String]) -> (String, String) {
    let fr = lang == Lang::French;
    let screen = match (moved.built_in, moved.inches) {
        (true, _) => txt::LAPTOP_SCREEN.get(lang).to_owned(),
        (false, 0) => txt::EXTERNAL_SCREEN.get(lang).to_owned(),
        (false, i) => txt::inch_screen(lang, i),
    };
    let first = match moved.gone {
        true => txt::the_left(lang, &screen),
        false => txt::welcome_back(lang, &screen),
    };
    let (who, one) = windows_named(lang, names);
    let second = match (moved.gone, fr, one) {
        (true, true, _) => {
            let verb = if one { "est" } else { "sont" };
            match moved.desktop {
                0 => format!("{who} {verb} ici."),
                d => format!("{who} {verb} ici, sur le bureau {d}."),
            }
        }
        (true, false, _) => {
            let verb = if one { "is" } else { "are" };
            match moved.desktop {
                0 => format!("{who} {verb} here."),
                d => format!("{who} {verb} here, on desktop {d}."),
            }
        }
        (false, true, true) => format!("{who} y retourne."),
        (false, true, false) => format!("{who} y retournent."),
        (false, false, true) => format!("{who} goes back to it."),
        (false, false, false) => format!("{who} go back to it."),
    };
    (first, second)
}

/// The windows named: "Firefox", "Firefox et foot", "Firefox, foot et 3
/// autres", "3 fenêtres foot"; and whether that is one window.
fn windows_named(lang: Lang, names: &[String]) -> (String, bool) {
    let unknown = txt::A_WINDOW.get(lang);
    let named: Vec<&str> = names
        .iter()
        .map(|n| if n.is_empty() { unknown } else { n.as_str() })
        .collect();
    let mut distinct: Vec<&str> = Vec::new();
    for n in &named {
        if !distinct.contains(n) {
            distinct.push(n);
        }
    }
    let and = txt::AND.get(lang);
    match (named.len(), distinct.as_slice()) {
        (0, _) => (unknown.to_owned(), true),
        (1, _) => (capital(named[0]), true),
        (n, [one]) if *one == unknown => (txt::windows(lang, n), false),
        (n, [one]) => (txt::windows_2(lang, txt::Windows2 { one, x_n: n }), false),
        (2, [a, b]) => (format!("{} {and} {b}", capital(a)), false),
        (n, [a, b, ..]) => {
            let others = n - named.iter().filter(|x| *x == a || *x == b).count();
            let rest = match others {
                1 => txt::N_1_OTHER.get(lang).to_owned(),
                k => txt::others(lang, k),
            };
            if others == 0 {
                (format!("{} {and} {b}", capital(a)), false)
            } else {
                (format!("{}, {b} {and} {rest}", capital(a)), false)
            }
        }
        _ => (unknown.to_owned(), true),
    }
}

/// `text` with its first letter up.
fn capital(text: &str) -> String {
    let mut letters = text.chars();
    letters
        .next()
        .map(|c| c.to_uppercase().chain(letters).collect())
        .unwrap_or_default()
}

/// What the screens' bubble says: its first line in bold, light blue, the
/// second in the text's colour, each cut to its room.
pub fn said_screens(
    lines: (String, String),
    mut measure: impl FnMut(&str, iced::Font) -> f32,
) -> Said {
    let first = kit::elide(&lines.0, SCREENS_ROOM, |t| measure(t, MANROPE_BOLD));
    let second = kit::elide(&lines.1, SCREENS_ROOM, |t| measure(t, MANROPE));
    let x = PAD_X + BORDER;
    let width = measure(&first, MANROPE_BOLD).max(measure(&second, MANROPE));
    Said {
        pieces: vec![
            (first, x, WORD_SCREENS.get(), MANROPE_BOLD, 0),
            (second, x, TEXT.get(), MANROPE, 1),
        ],
        width: x + width + PAD_X + BORDER,
        lines: 2,
    }
}

// --- The peek out, and its surfaces ---------------------------------------------

/// What is out: an agent's panda, or the winking one for screens moved.
#[derive(Debug, Clone)]
enum Shown {
    Agent(Peek),
    Screens,
}

impl Shown {
    /// The agent whose row the panda comes out at, if one.
    fn agent(&self) -> Option<u64> {
        match self {
            Self::Agent(peek) => Some(peek.id),
            Self::Screens => None,
        }
    }
}

#[derive(Debug, Clone)]
struct Out {
    shown: Shown,
    said: Said,
    /// Its surface beside each bar, and where.
    surfaces: HashMap<String, (window::Id, Placed)>,
    /// Its panda playing its mood, from its first frame (#649).
    burst: Option<Burst>,
    /// Its bubble and its panda under the pointer: the bubble lights.
    lit: [bool; 2],
}

static OUT: Mutex<Option<Out>> = Mutex::new(None);
/// The shell draws on the CPU: the peek is a card.
static CARD: AtomicBool = AtomicBool::new(false);

pub fn set_software(software: bool) {
    CARD.store(software, Ordering::Relaxed);
}

fn card() -> bool {
    CARD.load(Ordering::Relaxed)
}

fn out<T>(with: impl FnOnce(&mut Option<Out>) -> T) -> T {
    with(&mut OUT.lock().unwrap_or_else(PoisonError::into_inner))
}

/// The panda's middle, in bar `bar`'s frame, for agent `id`: its row's,
/// as the group lays it out now where the bar's column puts it; the
/// secretary's when it has none shown (under the "+ N" line) or for no
/// agent (screens moved); in the mini form, which has no rows, her head's
/// in its item (4.7).
fn middle(bar: &str, id: Option<u64>, form: Form) -> f32 {
    let top = super::draw::top(bar, form);
    if form == Form::Mini {
        return top + super::MINI_PAD_TOP + super::MINI_HEAD / 2.0;
    }
    // Its row's middle, under the rows above it (cards among them, #649).
    let row = match super::said().shows(now_us()) {
        super::Shows::Lines(lines) => {
            let at = lines.rows.iter().position(|r| Some(r.id) == id);
            at.map(|i| {
                let above: f32 = lines.rows[..i]
                    .iter()
                    .map(|r| super::row_height(r.look))
                    .sum();
                above + super::row_height(lines.rows[i].look) / 2.0
            })
        }
        _ => None,
    };
    match row {
        Some(middle) => top + super::HEADER + super::GAP + middle,
        None => top + super::HEADER / 2.0,
    }
}

/// The compositor says a panda peeks out: beside each bar, in place of
/// the one out if one is.
pub fn peek(peek: Peek) {
    let words = Words::new(Lang::of_messages());
    let minutes = now_us().saturating_sub(peek.since) / 60_000_000;
    let said = said(words, peek.kind, &peek.name, minutes, |t, font| {
        kit::measure(t, TEXT_SIZE, font)
    });
    show(Shown::Agent(peek), said);
}

/// The compositor says windows moved with a screen: the winking panda
/// says so beside each bar, in place of the one out.
pub fn screens(moved: ScreensMoved) {
    let names: Vec<String> = moved
        .apps
        .iter()
        .map(|app| {
            (!app.is_empty())
                .then(|| crate::share::app_name(app))
                .flatten()
                .unwrap_or_default()
        })
        .collect();
    let lines = screens_lines(Lang::of_messages(), &moved, &names);
    let said = said_screens(lines, |t, font| kit::measure(t, TEXT_SIZE, font));
    show(Shown::Screens, said);
}

/// `shown`, saying `said`, out beside each bar.
fn show(shown: Shown, said: Said) {
    let bars = popover::with(|c| c.bar_forms());
    let mut effects = Vec::new();
    out(|out| {
        let mut before = out.take().map(|o| o.surfaces).unwrap_or_default();
        let mut surfaces = HashMap::new();
        for (bar, form) in bars {
            let placed = placed(
                form,
                middle(&bar, shown.agent(), form),
                said.width,
                said.lines,
                card(),
            );
            let id = match before.remove(&bar) {
                Some((id, _)) => {
                    effects.push(Effect::Resize {
                        id,
                        size: placed.size,
                        margin: Some(placed.margin),
                    });
                    id
                }
                None => {
                    let id = window::Id::unique();
                    effects.push(Effect::Layer {
                        id,
                        bar: bar.clone(),
                        namespace: PEEK_NAMESPACE,
                        size: placed.size,
                        margin: placed.margin,
                    });
                    id
                }
            };
            effects.push(Effect::Region {
                id,
                rects: regions(&placed),
            });
            surfaces.insert(bar, (id, placed));
        }
        effects.extend(before.into_values().map(|(id, _)| Effect::Remove(id)));
        let text: Vec<&str> = said.pieces.iter().map(|p| p.0.as_str()).collect();
        match &shown {
            Shown::Agent(peek) => tracing::debug!(
                id = peek.id,
                kind = peek.kind.name(),
                said = %text.join(" "),
                bars = surfaces.len(),
                "the panda peeks out"
            ),
            Shown::Screens => tracing::debug!(
                said = %text.join(" "),
                bars = surfaces.len(),
                "the winking panda peeks out"
            ),
        }
        *out = Some(Out {
            shown,
            said,
            surfaces,
            burst: None,
            lit: [false; 2],
        });
    });
    // Its controls are built again for what it says.
    crate::ui::touch();
    effects.push(Effect::Wake(PEEK_NAMESPACE));
    popover::send(effects);
}

/// Where the surface takes the pointer: the bubble and the panda (the
/// whole card on the CPU).
fn regions(placed: &Placed) -> Vec<Rectangle> {
    if card() {
        let (w, h) = placed.size;
        return vec![Rectangle::new(Point::ORIGIN, Size::new(w as f32, h as f32))];
    }
    vec![placed.bubble, placed.panda]
}

/// The compositor says the panda is back: its surfaces go.
pub fn back() {
    let gone = out(Option::take);
    if let Some(gone) = gone {
        tracing::debug!(id = ?gone.shown.agent(), "the panda is back");
        popover::send(
            gone.surfaces
                .into_values()
                .map(|(id, _)| Effect::Remove(id))
                .collect(),
        );
    }
}

/// Bar `bar` went: its peek's surface goes too.
pub fn forget(bar: &str) {
    let gone = out(|out| out.as_mut()?.surfaces.remove(bar));
    if let Some((id, _)) = gone {
        popover::send(vec![Effect::Remove(id)]);
    }
}

/// The agents' group moved in the bar's column: each bar's peek's surface
/// stays beside its row.
pub fn moved() {
    for (bar, form) in popover::with(|c| c.bar_forms()) {
        self::form(&bar, form);
    }
}

/// Bar `bar` is now in `form`: its peek's surface stays beside it.
pub fn form(bar: &str, form: Form) {
    let moved = out(|out| {
        let out = out.as_mut()?;
        let placed = placed(
            form,
            middle(bar, out.shown.agent(), form),
            out.said.width,
            out.said.lines,
            card(),
        );
        let (id, was) = out.surfaces.get_mut(bar)?;
        (*was != placed)
            .then(|| {
                *was = placed;
                *id
            })
            .map(|id| (id, placed))
    });
    if let Some((id, placed)) = moved {
        popover::send(vec![
            Effect::Resize {
                id,
                size: placed.size,
                margin: Some(placed.margin),
            },
            Effect::Region {
                id,
                rects: regions(&placed),
            },
        ]);
    }
}

/// The peek's surface `id`, if it is one, and its bar.
pub fn bar_of(id: window::Id) -> Option<String> {
    out(|out| {
        out.as_ref()?
            .surfaces
            .iter()
            .find(|(_, (s, _))| *s == id)
            .map(|(bar, _)| bar.clone())
    })
}

// --- Drawing it -------------------------------------------------------------------

/// Which panda peeks out: waiting holds out its hat, finished arms up,
/// the winking one raises its hat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pose {
    Waiting,
    Done,
    Wink,
}

impl Shown {
    fn pose(&self) -> Pose {
        match self {
            Self::Agent(peek) => match peek.kind {
                PeekKind::Waiting => Pose::Waiting,
                PeekKind::Done => Pose::Done,
            },
            Self::Screens => Pose::Wink,
        }
    }
}

/// `mood`'s still drawing with its drop shadow, drawn [`PANDA_REACH`] px
/// larger on each side than its box.
fn panda(mood: Mood) -> svg::Handle {
    pandas::drawn(mood, None, Instant::now(), Some(shadowed))
}

/// How far the panda's drawing reaches out for its shadow, px at ×1.
const PANDA_REACH: f32 = 32.0;

/// A panda's drawing with the mockup's `drop-shadow`, its view box grown
/// by [`PANDA_REACH`] on each side to hold it.
fn shadowed(drawing: &str) -> String {
    let inner = drawing
        .find("<svg")
        .and_then(|at| drawing[at..].find('>').map(|end| at + end + 1))
        .zip(drawing.rfind("</svg>"))
        .map_or("", |(from, to)| &drawing[from..to]);
    // The drawing's units per pixel, drawn 170 px wide.
    let unit = PANDA_BOX.0 / PANDA;
    let (dy, deviation, alpha) = PANDA_SHADOW;
    let reach = PANDA_REACH * unit;
    let (x, y) = (-3.0 - reach, -3.5 - reach);
    let (w, h) = (PANDA_BOX.0 + 2.0 * reach, PANDA_BOX.1 + 2.0 * reach);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{x} {y} {w} {h}\" \
         width=\"{w}\" height=\"{h}\"><defs><filter id=\"shadow\" \
         filterUnits=\"userSpaceOnUse\" x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\">\
         <feDropShadow dx=\"0\" dy=\"{}\" stdDeviation=\"{}\" flood-color=\"#000\" \
         flood-opacity=\"{alpha}\"/></filter></defs><g filter=\"url(#shadow)\">{inner}</g></svg>",
        dy * unit,
        deviation * unit,
    )
}

/// The bubble's shadow on a surface of `size`: its rounded box offset and
/// blurred, outside the box only (CSS draws none under it).
fn bubble_shadow(size: Size, bubble: Rectangle) -> svg::Handle {
    let (dy, deviation, alpha) = BUBBLE_SHADOW;
    let (w, h) = (size.width, size.height);
    let Rectangle {
        x,
        y,
        width,
        height,
    } = bubble;
    let shadow = y + dy;
    svg::Handle::from_memory(
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" width=\"{w}\" \
             height=\"{h}\"><defs><filter id=\"blur\" filterUnits=\"userSpaceOnUse\" x=\"0\" \
             y=\"0\" width=\"{w}\" height=\"{h}\"><feGaussianBlur stdDeviation=\"{deviation}\"/>\
             </filter><mask id=\"outside\" maskUnits=\"userSpaceOnUse\" x=\"0\" y=\"0\" \
             width=\"{w}\" height=\"{h}\"><rect width=\"{w}\" height=\"{h}\" fill=\"#fff\"/>\
             <rect x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"{height}\" rx=\"{RADIUS}\" \
             fill=\"#000\"/></mask></defs><g mask=\"url(#outside)\"><rect x=\"{x}\" \
             y=\"{shadow}\" width=\"{width}\" height=\"{height}\" rx=\"{RADIUS}\" fill=\"#000\" \
             fill-opacity=\"{alpha}\" filter=\"url(#blur)\"/></g></svg>"
        )
        .into_bytes(),
    )
}

/// `colour` over `under`, opaque: the card's colours on the CPU.
fn over(colour: Color, under: Color) -> Color {
    Color::from_rgb(
        colour.r * colour.a + under.r * (1.0 - colour.a),
        colour.g * colour.a + under.g * (1.0 - colour.a),
        colour.b * colour.a + under.b * (1.0 - colour.a),
    )
}

/// What nothing holds on the peek beside bar `bar`: the bubble (lit
/// while the pointer is on it or on the panda), what it says, the panda
/// playing its mood. Its two controls are the kit's, over it ([`view`]).
#[derive(Debug, Clone)]
pub struct Back {
    pub bar: String,
}

/// What the canvas keeps between draws: when it last drew.
#[derive(Debug, Default)]
pub struct State {
    now: Option<Instant>,
}

impl Back {
    /// Its panda's burst, started as the peek first draws (#649), when the
    /// pandas play: none drawn when they do not.
    fn burst(&self, now: Instant) -> Option<Burst> {
        out(|out| {
            let out = out.as_mut()?;
            if out.burst.is_none() && pandas::lively() && !card() {
                let window = match out.shown {
                    Shown::Agent(_) => PLAYS,
                    Shown::Screens => WAVES,
                };
                out.burst = Some(Burst::lasting(now, window));
            }
            out.burst
        })
    }

    /// What it shows now, and where.
    fn shown(&self) -> Option<(Shown, Said, Placed)> {
        shown_on(&self.bar)
    }
}

/// What the peek beside bar `bar` shows now, and where.
fn shown_on(bar: &str) -> Option<(Shown, Said, Placed)> {
    out(|out| {
        let out = out.as_ref()?;
        let (_, placed) = out.surfaces.get(bar)?;
        Some((out.shown.clone(), out.said.clone(), *placed))
    })
}

/// Whether the pointer is on the bubble or the panda.
fn lit() -> bool {
    out(|out| out.as_ref().is_some_and(|o| o.lit.iter().any(|l| *l)))
}

impl<Message> canvas::Program<Message> for Back {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let iced::Event::Window(window::Event::RedrawRequested(now)) = event else {
            return None;
        };
        state.now = Some(*now);
        // Drawn again while its panda plays, never after.
        self.burst(*now)
            .filter(|b| b.plays(*now))
            .map(|_| canvas::Action::request_redraw())
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let Some((shown, said, placed)) = self.shown() else {
            return vec![f.into_geometry()];
        };
        let pose = shown.pose();
        let fill = match (pose, lit()) {
            (Pose::Wink, false) => BUBBLE_SCREENS,
            (Pose::Wink, true) => BUBBLE_SCREENS_HOVER,
            (_, false) => BUBBLE,
            (_, true) => BUBBLE_HOVER,
        };
        let border = match pose {
            Pose::Waiting => BORDER_WAITING,
            Pose::Done => BORDER_DONE,
            Pose::Wink => BORDER_SCREENS,
        };
        let size = bounds.size();
        if card() {
            // No alpha: one opaque card, the bubble's, round both; the
            // compositor cuts its corners.
            let card = Rectangle::new(Point::ORIGIN, size);
            let fill = over(fill.get(), Color::BLACK);
            f.fill_rectangle(Point::ORIGIN, size, fill);
            stroke(&mut f, card, over(border.get(), fill));
        } else {
            f.draw_svg(
                Rectangle::new(Point::ORIGIN, size),
                svg::Svg::new(bubble_shadow(size, placed.bubble)),
            );
            f.fill(&kit::rounded(placed.bubble, RADIUS), fill.get());
            stroke(&mut f, placed.bubble, border.get());
        }
        let line = TEXT_SIZE * LINE;
        let first = placed.bubble.center_y() - (said.lines.max(1) - 1) as f32 * line / 2.0;
        for (text, x, colour, font, row) in &said.pieces {
            f.fill_text(kit::text_in(
                *font,
                kit::text(
                    text.clone(),
                    Point::new(placed.bubble.x + x, first + *row as f32 * line),
                    TEXT_SIZE,
                    *colour,
                    Alignment::Left,
                ),
            ));
        }
        // The drawing's box, 126 × 120.5, 170 px wide, in the middle of
        // its 170 px square; with its shadow's reach around it.
        let tall = PANDA * PANDA_BOX.1 / PANDA_BOX.0;
        let reach = if card() { 0.0 } else { PANDA_REACH };
        let drawn = Rectangle::new(
            Point::new(
                placed.panda.x - reach,
                placed.panda.y + (PANDA - tall) / 2.0 - reach,
            ),
            Size::new(PANDA + 2.0 * reach, tall + 2.0 * reach),
        );
        let mood = match pose {
            Pose::Waiting => Mood::Waiting,
            Pose::Done => Mood::Done,
            Pose::Wink => Mood::Wink,
        };
        let now = state.now.unwrap_or_else(Instant::now);
        let burst = out(|out| out.as_ref().and_then(|o| o.burst));
        let drawing = match (card(), burst.filter(|b| b.plays(now))) {
            (true, _) => pandas::still(mood),
            (false, None) => panda(mood),
            (false, Some(burst)) => pandas::drawn(mood, Some(burst), now, Some(shadowed)),
        };
        f.draw_svg(drawn, svg::Svg::new(drawing));
        vec![f.into_geometry()]
    }
}

// --- Its controls, on the widget kit (#772) ------------------------------------------

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The bubble or the panda clicked: to the agent, or back.
    Go,
    /// The pointer comes on (or leaves) the bubble (0) or the panda (1).
    Lit(usize, bool),
    Stale,
}

/// The peek's surface `id` beside bar `bar`: what [`Back`] draws, and over
/// it the bubble and the panda (the whole card on the CPU) to click.
pub fn view<'a>(id: window::Id, bar: String) -> iced::Element<'a, crate::Message> {
    let back = canvas::Canvas::new(Back { bar: bar.clone() })
        .width(iced::Length::Fill)
        .height(iced::Length::Fill);
    let (built, handled) = (bar.clone(), bar.clone());
    crate::ui::Around::new(id, build(&bar), move |m| handle(&handled, m), Msg::Stale)
        .own(PEEK_NAMESPACE, move || bar_of(id).is_some())
        .rebuild(move || build(&built))
        .on(back)
}

fn handle(bar: &str, message: Msg) {
    match message {
        Msg::Go => {
            let Some((shown, _, placed)) = shown_on(bar) else {
                return;
            };
            match shown {
                Shown::Agent(peek) => {
                    tracing::debug!(id = peek.id, "the peek goes to its agent");
                    popover::send(vec![Effect::GoToAgent {
                        id: peek.id,
                        bar: bar.to_owned(),
                        row: placed.in_bar,
                    }]);
                }
                Shown::Screens => {
                    tracing::debug!("the winking panda is clicked: it goes back");
                    popover::send(vec![Effect::PeekClose]);
                }
            }
        }
        Msg::Lit(i, on) => out(|out| {
            if let Some(lit) = out.as_mut().and_then(|o| o.lit.get_mut(i)) {
                *lit = on;
            }
        }),
        Msg::Stale => {}
    }
}

/// The bubble and the panda where they are placed, each a control.
fn build<'a>(bar: &str) -> iced::Element<'a, Msg> {
    use iced::widget::{Space, Stack, mouse_area, pin};
    let Some((_, said, placed)) = shown_on(bar) else {
        return Space::new().into();
    };
    let name: String = said
        .pieces
        .iter()
        .map(|p| p.0.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let all: Vec<iced::Element<'a, Msg>> = regions(&placed)
        .into_iter()
        .enumerate()
        .map(|(i, rect)| {
            let spot = mouse_area(Space::new().width(rect.width).height(rect.height))
                .on_press(Msg::Go)
                .on_enter(Msg::Lit(i, true))
                .on_exit(Msg::Lit(i, false))
                .interaction(mouse::Interaction::Pointer);
            let key = if i == 0 { "peek:Bubble" } else { "peek:Panda" };
            pin(crate::ui::control(
                spot,
                key,
                hocus_a11y::Role::Button,
                name.clone(),
            ))
            .x(rect.x)
            .y(rect.y)
            .into()
        })
        .collect();
    Stack::with_children(all)
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .into()
}

/// A 1 px border inside `rect`, rounded as the bubble.
fn stroke(f: &mut Frame, rect: Rectangle, colour: Color) {
    let half = BORDER / 2.0;
    let path = Path::rounded_rectangle(
        Point::new(rect.x + half, rect.y + half),
        Size::new(rect.width - BORDER, rect.height - BORDER),
        (RADIUS - half).into(),
    );
    f.stroke(
        &path,
        canvas::Stroke {
            style: canvas::Style::Solid(colour),
            width: BORDER,
            ..canvas::Stroke::default()
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 7 px a character, 4 a space, bold a pixel wider.
    fn measure(text: &str, font: iced::Font) -> f32 {
        let bold = if font == MANROPE_BOLD { 1.0 } else { 0.0 };
        text.chars()
            .map(|c| if c == ' ' { 4.0 } else { 7.0 + bold })
            .sum()
    }

    #[test]
    fn it_says_who_waits_since_when_and_who_is_done() {
        let fr = Words::new(Lang::French);
        let waiting = said(fr, PeekKind::Waiting, "site-web", 0, measure);
        let texts: Vec<&str> = waiting.pieces.iter().map(|p| p.0.as_str()).collect();
        assert_eq!(texts, ["site-web", "t'attend", "· à l'instant"]);
        assert_eq!(waiting.pieces[0].3, MANROPE_BOLD);
        assert_eq!(waiting.pieces[1].2, WORD_WAITING.get());
        assert_eq!(waiting.pieces[2].2, FAINT.get());
        // 13 px in, a space between the pieces, 13 px after the last.
        assert_eq!(waiting.pieces[0].1, 13.0);
        assert_eq!(waiting.pieces[1].1, 13.0 + 64.0 + 4.0);
        let last = waiting.pieces[2].1 + measure("· à l'instant", MANROPE);
        assert_eq!(waiting.width, last + 13.0);
        // Its reminder, five minutes on.
        let reminded = said(fr, PeekKind::Waiting, "site-web", 5, measure);
        assert_eq!(reminded.pieces[2].0, "· 5\u{a0}min");
        let done = said(fr, PeekKind::Done, "api-paiements", 3, measure);
        let texts: Vec<&str> = done.pieces.iter().map(|p| p.0.as_str()).collect();
        assert_eq!(texts, ["api-paiements", "a fini"]);
        assert_eq!(done.pieces[1].2, WORD_DONE.get());
        let en = Words::new(Lang::English);
        let texts = |s: Said| s.pieces.into_iter().map(|p| p.0).collect::<Vec<_>>();
        assert_eq!(
            texts(said(en, PeekKind::Waiting, "site-web", 0, measure)),
            ["site-web", "is waiting", "· just now"]
        );
        assert_eq!(
            texts(said(en, PeekKind::Done, "api-paiements", 0, measure)),
            ["api-paiements", "is done"]
        );
        // A long name is cut.
        let long = said(fr, PeekKind::Done, &"x".repeat(80), 0, measure);
        assert!(long.pieces[0].0.ends_with('…'), "{long:?}");
        assert!(long.width < NAME_ROOM + 100.0, "{long:?}");
    }

    #[test]
    fn it_comes_out_18_px_from_the_bar_at_its_row() {
        // A 200 px bubble beside the full bar, the row's middle 171 px
        // under its top.
        let p = placed(Form::Full, 171.0, 200.0, 1, false);
        // The panda's box: 200 px (#649), its right 18 px from the bar,
        // its middle the row's.
        assert_eq!(
            p.in_bar,
            Rectangle::new(Point::new(-414.0, 71.0), Size::new(200.0, 200.0))
        );
        assert_eq!(p.panda.x + PANDA + ROOM.2, p.size.0 as f32);
        let right = p.margin.1 as f32 + ROOM.2;
        assert_eq!(right, 8.0 + 196.0 + 18.0);
        assert_eq!(p.margin.0 as f32 + ROOM.1, 8.0 + 71.0);
        // The bubble 8 px before it, in its middle.
        assert_eq!(p.bubble.x + p.bubble.width + GAP, p.panda.x);
        assert!((p.bubble.center_y() - p.panda.center_y()).abs() < 1e-3);
        assert!((p.bubble.height - (13.0 * LINE + 18.0)).abs() < 1e-3);
        // Beside the mini bar, as far from it.
        let mini = placed(Form::Mini, 171.0, 200.0, 1, false);
        assert_eq!(mini.margin.1 as f32 + ROOM.2, 8.0 + 56.0 + 18.0);
        assert_eq!(mini.in_bar.x, -56.0 - 18.0 - 200.0);
        // On the CPU: a card, the panda's box its height, 8 px after it.
        let card = placed(Form::Full, 171.0, 200.0, 1, true);
        assert_eq!(card.size, (416, 200));
        assert_eq!(card.margin, (79, 8 + 196 + 18 - 8, 0, 0));
        assert_eq!(card.in_bar, p.in_bar);
        // Near the bar's top, the panda stays below it.
        let high = placed(Form::Full, 20.0, 200.0, 2, false);
        assert_eq!(high.in_bar.y, 0.0);
        assert!((high.bubble.height - (2.0 * 13.0 * LINE + 18.0)).abs() < 1e-3);
    }

    #[test]
    fn the_winking_panda_says_which_screen_and_windows_moved() {
        let moved = |gone, built_in, inches, desktop| ScreensMoved {
            gone,
            built_in,
            inches,
            desktop,
            apps: Vec::new(),
        };
        let names = |n: &[&str]| n.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let fr = Lang::French;
        let en = Lang::English;
        assert_eq!(
            screens_lines(fr, &moved(true, false, 34, 1), &names(&["Firefox", "foot"])),
            (
                "L’écran externe 34 pouces est parti.".to_owned(),
                "Firefox et foot sont ici, sur le bureau 1.".to_owned()
            )
        );
        assert_eq!(
            screens_lines(en, &moved(true, false, 34, 1), &names(&["Firefox", "foot"])),
            (
                "The 34-inch screen left.".to_owned(),
                "Firefox and foot are here, on desktop 1.".to_owned()
            )
        );
        assert_eq!(
            screens_lines(
                fr,
                &moved(false, false, 34, 1),
                &names(&["Firefox", "foot"])
            ),
            (
                "Re-bonjour, écran externe 34 pouces\u{a0}!".to_owned(),
                "Firefox et foot y retournent.".to_owned()
            )
        );
        assert_eq!(
            screens_lines(en, &moved(false, false, 34, 1), &names(&["Firefox"])),
            (
                "Welcome back, 34-inch screen!".to_owned(),
                "Firefox goes back to it.".to_owned()
            )
        );
        // Past two windows, the others counted; the lid's built-in screen;
        // windows on several desktops.
        let many = names(&["Firefox", "foot", "Slack", "foot", "Zed"]);
        assert_eq!(
            screens_lines(fr, &moved(true, true, 14, 0), &many).1,
            "Firefox, foot et 2 autres sont ici."
        );
        assert_eq!(
            screens_lines(en, &moved(true, true, 14, 0), &many),
            (
                "The laptop screen left.".to_owned(),
                "Firefox, foot and 2 others are here.".to_owned()
            )
        );
        assert_eq!(
            screens_lines(fr, &moved(true, false, 0, 2), &names(&["foot", "foot"])).1,
            "2 fenêtres foot sont ici, sur le bureau 2."
        );
        assert_eq!(
            screens_lines(fr, &moved(false, false, 0, 2), &names(&[""])),
            (
                "Re-bonjour, écran externe\u{a0}!".to_owned(),
                "Une fenêtre y retourne.".to_owned()
            )
        );
        // Two lines, the first in bold light blue.
        let said = said_screens(
            (
                "L’écran externe est parti.".to_owned(),
                "Firefox est ici.".to_owned(),
            ),
            measure,
        );
        assert_eq!(said.lines, 2);
        assert_eq!(said.pieces[0].3, MANROPE_BOLD);
        assert_eq!(said.pieces[0].2, WORD_SCREENS.get());
        assert_eq!((said.pieces[0].4, said.pieces[1].4), (0, 1));
        let long = said_screens((String::new(), "x".repeat(200)), measure);
        assert!(
            long.width <= SCREENS_ROOM + 2.0 * (PAD_X + BORDER) + 1.0,
            "{long:?}"
        );
    }

    #[test]
    fn the_pandas_shadow_is_round_their_drawing() {
        let drawing = hocus_pandas::Mood::Waiting.still();
        let shadowed = shadowed(drawing);
        assert!(shadowed.contains("feDropShadow"), "{shadowed}");
        assert!(shadowed.contains("<g filter=\"url(#shadow)\">"));
        // The drawing's own shapes, once, inside the group.
        let inner =
            &drawing[drawing.find("<circle").unwrap_or(0)..drawing.rfind("</svg>").unwrap_or(0)];
        assert_eq!(shadowed.matches(inner.trim()).count(), 1);
        assert_eq!(shadowed.matches("<svg").count(), 1);
        assert!(shadowed.ends_with("</g></svg>"));
    }
}
