//! The panda peeking out of the bar's edge (bar.md §7 C, 4.5): when the
//! compositor says so (`peek`), the agent's panda (96 px; holding out its
//! hat when it waits, its arms up when it finished) and its bubble ("**site-web**
//! t'attend · à l'instant", "**api-paiements** a fini") on a layer surface
//! of their own beside each bar, 18 px from it, at the agent's row's
//! height. The shell draws them once; the compositor moves the surface out
//! from under the bar and back, and says when it is back (`no-peek`): the
//! surfaces then go, and nothing is drawn at rest. A newer peek takes the
//! surfaces of the one out.
//!
//! The surface takes the pointer only where it draws: the bubble and the
//! panda. A click on either goes to the agent (§8, 4.3), the streak from
//! the panda, and the compositor sends it back at once.
//!
//! On a GPU the surface is transparent: the bubble's and the panda's
//! shadows are drawn in it (SVG filters, as the mockup's CSS). On the CPU
//! its buffer has no alpha: the bubble's card holds the panda too, and the
//! compositor cuts it to its rounded corners.

use std::{
    collections::HashMap,
    sync::{
        Mutex, OnceLock, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

use hocus_link::{EDGE, Form, PEEK_NAMESPACE, Peek, PeekKind};
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
use crate::{
    clock::time::Lang,
    levels::draw::{FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, pointer, rgb},
    popover::{self, Effect, kit},
};

// --- The mockup's sizes, at ×1 ----------------------------------------------------

/// The panda's box, and its gap to the bubble; the box's distance from
/// the bar.
pub const PANDA: f32 = 96.0;
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
const BUBBLE: Color = Color::from_rgba8(30, 26, 18, 0.92);
const BUBBLE_HOVER: Color = Color::from_rgba8(40, 32, 18, 0.96);
/// Its border, the state's colour at 45 %: amber waiting, green finished.
const BORDER_WAITING: Color = Color::from_rgba8(245, 180, 74, 0.45);
const BORDER_DONE: Color = Color::from_rgba8(108, 194, 135, 0.45);
/// The word after the name, in the rows' colours.
const WORD_WAITING: Color = rgb(0xF5C46E);
const WORD_DONE: Color = rgb(0x7EE0A0);
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

/// The bubble's height: a line of 13 px text, its padding and border.
fn bubble_height() -> f32 {
    TEXT_SIZE * LINE + 2.0 * (PAD_Y + BORDER)
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

/// A peek of a bubble `width` wide beside a bar in `form`, the panda's
/// middle `middle` under the bar's top; `card`: on the CPU.
pub fn placed(form: Form, middle: f32, width: f32, card: bool) -> Placed {
    // In whole pixels: the panda's box keeps its place from the bar.
    let width = width.ceil();
    let height = bubble_height();
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
    let panda_top = middle - PANDA / 2.0;
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
    Placed {
        size,
        margin,
        bubble,
        panda,
        in_bar,
    }
}

// --- What it says -------------------------------------------------------------------

/// The bubble's text: the name in bold, the word in the state's colour,
/// since when in `text.faint` (a waiting agent's); each piece's start.
#[derive(Debug, Clone, PartialEq)]
pub struct Said {
    pub pieces: Vec<(String, f32, Color, iced::Font)>,
    /// The bubble's width.
    pub width: f32,
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
    pieces.push((name, x, TEXT, MANROPE_BOLD));
    x += name_width + space;
    pieces.push((word.to_owned(), x, colour, MANROPE));
    x += measure(word, MANROPE);
    if kind == PeekKind::Waiting {
        let since = words.since(minutes, false);
        x += space;
        pieces.push((since.clone(), x, FAINT, MANROPE));
        x += measure(&since, MANROPE);
    }
    Said {
        pieces,
        width: x + PAD_X + BORDER,
    }
}

// --- The peek out, and its surfaces ---------------------------------------------

#[derive(Debug, Clone)]
struct Out {
    peek: Peek,
    said: Said,
    /// Its surface beside each bar, and where.
    surfaces: HashMap<String, (window::Id, Placed)>,
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

/// The panda's middle, in the bar's frame, for agent `id`: its row's, as
/// the group lays it out now; the secretary's when it has none shown
/// (under the "+ N" line); in the mini form, which has no rows, her head's
/// in its item (4.7).
fn middle(id: u64, form: Form) -> f32 {
    if form == Form::Mini {
        return super::MINI_TOP + super::MINI_PAD_TOP + super::HEAD / 2.0;
    }
    let row = match super::said().shows(now_us()) {
        super::Shows::Lines(lines) => lines.rows.iter().position(|r| r.id == id),
        _ => None,
    };
    let top = super::draw::TOP;
    match row {
        Some(i) => top + super::HEADER + super::GAP + (i as f32 + 0.5) * super::AGENT_ROW,
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
    let bars = popover::with(|c| c.bar_forms());
    let mut effects = Vec::new();
    out(|out| {
        let mut before = out.take().map(|o| o.surfaces).unwrap_or_default();
        let mut surfaces = HashMap::new();
        for (bar, form) in bars {
            let placed = placed(form, middle(peek.id, form), said.width, card());
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
        tracing::debug!(
            id = peek.id,
            kind = peek.kind.name(),
            said = %text.join(" "),
            bars = surfaces.len(),
            "the panda peeks out"
        );
        *out = Some(Out {
            peek,
            said,
            surfaces,
        });
    });
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
        tracing::debug!(id = gone.peek.id, "the panda is back");
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

/// Bar `bar` is now in `form`: its peek's surface stays beside it.
pub fn form(bar: &str, form: Form) {
    let moved = out(|out| {
        let out = out.as_mut()?;
        let placed = placed(form, middle(out.peek.id, form), out.said.width, card());
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

/// The panda for `kind`, with its drop shadow, drawn `ROOM` px larger on
/// each side than its box: waiting holds out its hat, finished arms up.
fn panda(kind: PeekKind) -> &'static svg::Handle {
    static PANDAS: OnceLock<[svg::Handle; 2]> = OnceLock::new();
    let pandas = PANDAS.get_or_init(|| {
        [
            include_str!("../../assets/pandas/waiting.svg"),
            include_str!("../../assets/pandas/done.svg"),
        ]
        .map(|drawing| svg::Handle::from_memory(shadowed(drawing).into_bytes()))
    });
    &pandas[match kind {
        PeekKind::Waiting => 0,
        PeekKind::Done => 1,
    }]
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
    // The drawing's units per pixel, drawn 96 px wide.
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

/// The peek's surface beside bar `bar`.
#[derive(Debug, Clone)]
pub struct View {
    pub bar: String,
}

/// What the surface keeps between draws: where the pointer is.
#[derive(Debug, Default)]
pub struct State {
    pointer: Option<Point>,
    hover: bool,
}

impl View {
    /// What it shows now, and where.
    fn shown(&self) -> Option<(Peek, Said, Placed)> {
        out(|out| {
            let out = out.as_ref()?;
            let (_, placed) = out.surfaces.get(&self.bar)?;
            Some((out.peek.clone(), out.said.clone(), *placed))
        })
    }
}

impl<Message> canvas::Program<Message> for View {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let iced::Event::Mouse(event) = event else {
            return None;
        };
        let (peek, _, placed) = self.shown()?;
        state.pointer = pointer(event, bounds, cursor, state.pointer);
        let over = state
            .pointer
            .is_some_and(|at| regions(&placed).iter().any(|r| r.contains(at)));
        match event {
            mouse::Event::ButtonPressed(mouse::Button::Left) if over => {
                tracing::debug!(id = peek.id, "the peek goes to its agent");
                popover::send(vec![Effect::GoToAgent {
                    id: peek.id,
                    bar: self.bar.clone(),
                    row: placed.in_bar,
                }]);
                Some(canvas::Action::capture())
            }
            _ if over != state.hover => {
                state.hover = over;
                Some(canvas::Action::request_redraw())
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
        let mut f = Frame::new(renderer, bounds.size());
        let Some((peek, said, placed)) = self.shown() else {
            return vec![f.into_geometry()];
        };
        let fill = if state.hover { BUBBLE_HOVER } else { BUBBLE };
        let border = match peek.kind {
            PeekKind::Waiting => BORDER_WAITING,
            PeekKind::Done => BORDER_DONE,
        };
        let size = bounds.size();
        if card() {
            // No alpha: one opaque card, the bubble's, round both; the
            // compositor cuts its corners.
            let card = Rectangle::new(Point::ORIGIN, size);
            let fill = over(fill, Color::BLACK);
            f.fill_rectangle(Point::ORIGIN, size, fill);
            stroke(&mut f, card, over(border, fill));
        } else {
            f.draw_svg(
                Rectangle::new(Point::ORIGIN, size),
                svg::Svg::new(bubble_shadow(size, placed.bubble)),
            );
            f.fill(&kit::rounded(placed.bubble, RADIUS), fill);
            stroke(&mut f, placed.bubble, border);
        }
        let middle = placed.bubble.center_y();
        for (text, x, colour, font) in &said.pieces {
            f.fill_text(kit::text_in(
                *font,
                kit::text(
                    text.clone(),
                    Point::new(placed.bubble.x + x, middle),
                    TEXT_SIZE,
                    *colour,
                    Alignment::Left,
                ),
            ));
        }
        // The drawing's box, 126 × 120.5, 96 px wide, in the middle of its
        // 96 px square; with its shadow's reach around it.
        let tall = PANDA * PANDA_BOX.1 / PANDA_BOX.0;
        let reach = if card() { 0.0 } else { PANDA_REACH };
        let drawn = Rectangle::new(
            Point::new(
                placed.panda.x - reach,
                placed.panda.y + (PANDA - tall) / 2.0 - reach,
            ),
            Size::new(PANDA + 2.0 * reach, tall + 2.0 * reach),
        );
        let drawing = if card() {
            super::draw::panda(match peek.kind {
                PeekKind::Waiting => super::Look::Waiting,
                PeekKind::Done => super::Look::ToSee,
            })
            .clone()
        } else {
            panda(peek.kind).clone()
        };
        f.draw_svg(drawn, svg::Svg::new(drawing));
        vec![f.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.hover {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
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
        assert_eq!(waiting.pieces[1].2, WORD_WAITING);
        assert_eq!(waiting.pieces[2].2, FAINT);
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
        assert_eq!(done.pieces[1].2, WORD_DONE);
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
        let p = placed(Form::Full, 171.0, 200.0, false);
        // The panda's box: 96 px, its right 18 px from the bar, its middle
        // the row's.
        assert_eq!(
            p.in_bar,
            Rectangle::new(Point::new(-310.0, 123.0), Size::new(96.0, 96.0))
        );
        assert_eq!(p.panda.x + PANDA + ROOM.2, p.size.0 as f32);
        let right = p.margin.1 as f32 + ROOM.2;
        assert_eq!(right, 8.0 + 196.0 + 18.0);
        assert_eq!(p.margin.0 as f32 + ROOM.1, 8.0 + 123.0);
        // The bubble 8 px before it, in its middle.
        assert_eq!(p.bubble.x + p.bubble.width + GAP, p.panda.x);
        assert!((p.bubble.center_y() - p.panda.center_y()).abs() < 1e-3);
        assert!((p.bubble.height - (13.0 * LINE + 18.0)).abs() < 1e-3);
        // Beside the mini bar, as far from it.
        let mini = placed(Form::Mini, 171.0, 200.0, false);
        assert_eq!(mini.margin.1 as f32 + ROOM.2, 8.0 + 56.0 + 18.0);
        assert_eq!(mini.in_bar.x, -56.0 - 18.0 - 96.0);
        // On the CPU: a card, the panda's box its height, 8 px after it.
        let card = placed(Form::Full, 171.0, 200.0, true);
        assert_eq!(card.size, (312, 96));
        assert_eq!(card.margin, (131, 8 + 196 + 18 - 8, 0, 0));
        assert_eq!(card.in_bar, p.in_bar);
    }

    #[test]
    fn the_pandas_shadow_is_round_their_drawing() {
        let drawing = include_str!("../../assets/pandas/waiting.svg");
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
