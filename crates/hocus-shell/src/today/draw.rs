//! Today's surface (Day.3, variant A), first of the day's groups under the
//! card and the sign of what is in use.
//!
//! - **The header**: 18 px, "AUJOURD'HUI" / "TODAY" (Manrope 700, 11.5
//!   px, spaced 0.1 em, `text.dim`), as "AGENTS".
//! - **A line**: 20 px, 6 px past the group's sides (radius 6, 6 px in):
//!   its start in Martian Mono 11 px, `text.dim`, in a 38 px cell; 7 px, a
//!   7 px dot in its calendar's colour; 7 px, its title (Manrope 13 px,
//!   `text`, one line, cut with "…"). "+ N autres aujourd'hui" in 12.5 px
//!   `text.faint`. Under the pointer a line is lit `rgba(255,255,255,0.06)`
//!   and the pointer is a hand; resting 600 ms on an event shows its
//!   times and its whole title.
//! - **Time passing**: as a line goes (it became the card), it fades out
//!   (200 ms) and those under it move up 20 px (240 ms,
//!   `cubic-bezier(0.2, 0.9, 0.25, 1)`); a line coming (from "+ N") fades
//!   in. At once with reduced motion or on the CPU. Nothing draws between
//!   two changes.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_a11y::{Node, Role};
use hocus_link::{Form, TODAY_NAMESPACE};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Vector, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path},
        text::Alignment,
    },
};

use super::{HEADER, LINE, Row, Words};
use crate::theme::Ink;
use crate::{
    Message,
    clock::motion::bezier,
    levels::draw::{DIM, FAINT, MANROPE, MANROPE_BOLD, TEXT, faded},
    meeting::draw::MONO,
    popover::{Item, kit},
    ui::items::{Control, Controls, Items, Press},
};

/// The surface's width: the bar inside its rim.
const FULL_WIDTH: u32 = 194;
/// Around the group on its surface: the keyboard's ring.
const ROOM: f32 = 5.0;
/// The bar's sides' padding, the group's width, and how far a line goes
/// past it.
const SIDE: f32 = 12.0;
const GROUP_WIDTH: f32 = 172.0;
const OUT: f32 = 6.0;
const RADIUS: f32 = 6.0;
const LABEL: f32 = 11.5;
const SPACING: f32 = 0.1 * LABEL;
const TIME: f32 = 11.0;
const TIME_CELL: f32 = 38.0;
const DOT: f32 = 7.0;
const DOT_GAP: f32 = 7.0;
const TITLE: f32 = 13.0;
const MORE: f32 = 12.5;
const HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));
/// Resting this long on a line shows its tooltip.
pub const TIP: Duration = Duration::from_millis(600);
/// A line going fades out over 200 ms, those under it move up over 240.
const FADE: Duration = Duration::from_millis(200);
const SLIDE: Duration = Duration::from_millis(240);
const SLIDE_CURVE: (f32, f32, f32, f32) = (0.2, 0.9, 0.25, 1.0);
const STEP: Duration = Duration::from_millis(16);

/// The surface's size and its top under the bar's, for a group `height`
/// tall whose top is `top`.
pub fn placement(top: f32, height: f32) -> ((u32, u32), f32) {
    (
        (FULL_WIDTH, (height.max(1.0) + 2.0 * ROOM).ceil() as u32),
        top - ROOM,
    )
}

/// Row `index`'s place in the group, from its top.
fn row_top(index: usize) -> f32 {
    HEADER + LINE * index as f32
}

/// Today of bar `bar` on its surface.
#[derive(Debug, Clone)]
pub struct View {
    pub bar: String,
    pub forms: Vec<(Form, f32)>,
    /// On the CPU, the bar's colour under it.
    pub backdrop: Option<Color>,
    /// Its lines move (no reduced motion, not on the CPU).
    pub motion: bool,
    pub words: Words,
}

/// A row on its way: from where it was, since when; going (it left).
#[derive(Debug, Clone, PartialEq)]
struct Moving {
    from: Option<usize>,
    since: Instant,
}

#[derive(Debug, Default)]
pub struct State {
    drawn: Option<Instant>,
    /// The rows as last drawn, to move from.
    seen: Vec<Row>,
    moving: HashMap<String, Moving>,
    /// Rows gone, fading out where they were.
    going: Vec<(Row, usize, Instant)>,
}

/// What it last said it shows at rest, for the log.
static SAID: Mutex<Option<String>> = Mutex::new(None);

impl State {
    /// The rows changed from those last seen, at `now`: what moves.
    fn follow(&mut self, rows: &[Row], now: Instant, motion: bool) {
        if rows == self.seen.as_slice() {
            return;
        }
        if motion && !self.seen.is_empty() {
            let before: HashMap<&str, usize> = self
                .seen
                .iter()
                .enumerate()
                .map(|(i, r)| (r.key(), i))
                .collect();
            for (i, row) in rows.iter().enumerate() {
                let from = before.get(row.key()).copied();
                if from != Some(i) {
                    self.moving
                        .insert(row.key().to_owned(), Moving { from, since: now });
                }
            }
            for (i, row) in self.seen.iter().enumerate() {
                if !rows.iter().any(|r| r.key() == row.key()) {
                    self.going.push((row.clone(), i, now));
                }
            }
        }
        self.seen = rows.to_vec();
    }

    fn still(&mut self, now: Instant) -> bool {
        self.moving
            .retain(|_, m| now.saturating_duration_since(m.since) < SLIDE.max(FADE));
        self.going
            .retain(|(_, _, since)| now.saturating_duration_since(*since) < FADE);
        self.moving.is_empty() && self.going.is_empty()
    }
}

impl View {
    /// The group's top on its surface.
    fn origin(&self, size: Size) -> Point {
        Point::new(size.width + 1.0 - SIDE - GROUP_WIDTH, ROOM)
    }

    /// Row `index`'s line on a surface `size` large.
    fn line_rect(&self, size: Size, index: usize) -> Rectangle {
        let o = self.origin(size);
        Rectangle::new(
            Point::new(o.x - OUT, o.y + row_top(index)),
            Size::new(GROUP_WIDTH + 2.0 * OUT, LINE),
        )
    }

    /// The full form's opacity now (it fades with the bar's switch).
    fn alpha(&self) -> f32 {
        self.forms
            .iter()
            .find(|(f, _)| *f == Form::Full)
            .map_or(0.0, |(_, a)| *a)
    }
}

impl Items for View {
    type State = State;

    fn bar(&self) -> &str {
        &self.bar
    }

    fn namespace(&self) -> &'static str {
        TODAY_NAMESPACE
    }

    fn controls(&self, _state: &State, size: Size) -> Controls {
        let (rows, top) = super::shown(&self.bar);
        let to_bar = Vector::new(-size.width - 1.0, top - ROOM);
        if self.forms.len() != 1 || self.alpha() <= 0.0 {
            return Controls {
                to_bar,
                ..Controls::default()
            };
        }
        let said = crate::a11y::Words::new(self.words.lang);
        let controls: Vec<Control> = rows
            .iter()
            .enumerate()
            .filter_map(|(i, row)| {
                let name = match row {
                    Row::Event(line) => self.words.name(line),
                    Row::More(n) => said.more_today(*n),
                };
                let item = item_of(&rows, i)?;
                Some(Control::new(
                    item,
                    self.line_rect(size, i),
                    Role::Button,
                    name,
                    RADIUS,
                ))
            })
            .collect();
        let events = rows
            .iter()
            .map(|r| match r {
                Row::Event(_) => 1,
                Row::More(n) => *n,
            })
            .sum();
        Controls {
            to_bar,
            group: (controls.len() > 1).then(|| Node::new("today", Role::List, said.today(events))),
            controls,
        }
    }

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        _bounds: Rectangle,
        _at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let iced::Event::Window(iced::window::Event::RedrawRequested(now)) = event else {
            return None;
        };
        state.drawn = Some(*now);
        let (rows, _) = super::shown(&self.bar);
        state.follow(&rows, *now, self.motion);
        (!state.still(*now)).then(|| canvas::Action::request_redraw_at(*now + STEP))
    }

    /// A line under the pointer shows its times and its title whole.
    fn hovered(&self, _state: &mut State, on: Option<(Item, Rectangle)>) {
        let (rows, _) = super::shown(&self.bar);
        let line = on.and_then(|(item, rect)| {
            let i = (0..rows.len()).find(|i| item_of(&rows, *i) == Some(item))?;
            match rows.get(i)? {
                Row::Event(line) => Some((line, rect)),
                Row::More(_) => None,
            }
        });
        match line {
            Some((line, rect)) => {
                let (name, tip) = Words::tip(line);
                crate::tray::tip::hover_text(&self.bar, rect, TIP, name, tip);
            }
            None => crate::tray::tip::leave(),
        }
    }

    fn pressed(
        &self,
        _state: &mut State,
        _button: mouse::Button,
        _on: Option<(Item, Rectangle)>,
    ) -> Press {
        crate::tray::tip::leave();
        Press::Kit
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        bounds: Rectangle,
        lit: Option<Item>,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = bounds.size();
        let under = crate::backdrop::under(renderer, size, self.backdrop);
        let (rows, top) = super::shown(&self.bar);
        let alpha = self.alpha();
        if rows.is_empty() || alpha <= 0.0 {
            return crate::backdrop::over(under, frame);
        }
        let still = self.forms.len() == 1;
        let hover = lit.and_then(|lit| (0..rows.len()).find(|i| item_of(&rows, *i) == Some(lit)));
        let f = &mut frame;
        let o = self.origin(size);
        let mut x = o.x;
        for ch in self.words.header().chars() {
            let ch = ch.to_string();
            f.fill_text(kit::text_in(
                MANROPE_BOLD,
                kit::text(
                    ch.clone(),
                    Point::new(x, o.y + HEADER / 2.0),
                    LABEL,
                    faded(DIM.get(), alpha),
                    Alignment::Left,
                ),
            ));
            x += kit::measure(&ch, LABEL, MANROPE_BOLD) + SPACING;
        }
        let now = state.drawn.unwrap_or_else(Instant::now);
        for (row, from, since) in &state.going {
            let t = now.saturating_duration_since(*since).as_secs_f32() / FADE.as_secs_f32();
            let rect = self.line_rect(size, *from);
            draw_row(
                f,
                row,
                rect,
                self.words,
                alpha * (1.0 - t.clamp(0.0, 1.0)),
                false,
            );
        }
        for (i, row) in rows.iter().enumerate() {
            let mut rect = self.line_rect(size, i);
            let mut a = alpha;
            if let Some(moving) = state.moving.get(row.key()) {
                let t = now.saturating_duration_since(moving.since);
                match moving.from {
                    Some(from) => {
                        let k = bezier(
                            SLIDE_CURVE,
                            (t.as_secs_f32() / SLIDE.as_secs_f32()).clamp(0.0, 1.0),
                        );
                        rect.y += (row_top(from) - row_top(i)) * (1.0 - k);
                    }
                    None => {
                        a *= (t.as_secs_f32() / FADE.as_secs_f32()).clamp(0.0, 1.0);
                    }
                }
            }
            let lit = still && hover == Some(i);
            draw_row(f, row, rect, self.words, a, lit);
        }
        if still && state.moving.is_empty() && state.going.is_empty() {
            let names: Vec<String> = rows
                .iter()
                .map(|r| match r {
                    Row::Event(line) => self.words.name(line),
                    Row::More(n) => self.words.more(*n),
                })
                .collect();
            let said = format!("{} lit={:?}", self.words.said(&rows), hover);
            let mut kept = SAID.lock().unwrap_or_else(PoisonError::into_inner);
            if kept.as_deref() != Some(said.as_str()) {
                tracing::debug!(
                    shows = %said,
                    names = ?names,
                    top,
                    "today is drawn at rest"
                );
                *kept = Some(said);
            }
        }
        crate::backdrop::over(under, frame)
    }
}

/// Row `i`'s item: an event's line, or the "+ N".
fn item_of(rows: &[Row], i: usize) -> Option<Item> {
    Some(match rows.get(i)? {
        Row::Event(_) => Item::TodayLine(u8::try_from(i).ok()?),
        Row::More(_) => Item::TodayMore,
    })
}

/// Row `row` at `rect` (its line, 6 px past the group's sides).
fn draw_row(f: &mut Frame, row: &Row, rect: Rectangle, words: Words, alpha: f32, lit: bool) {
    if alpha <= 0.0 {
        return;
    }
    if lit {
        crate::theme::hover(f, &kit::rounded(rect, RADIUS), faded(HOVER.get(), alpha));
    }
    let x = rect.x + OUT;
    let middle = rect.center_y();
    let right = rect.x + rect.width - OUT;
    match row {
        Row::Event(line) => {
            f.fill_text(kit::text_in(
                MONO,
                kit::text(
                    line.start.clone(),
                    Point::new(x, middle),
                    TIME,
                    faded(DIM.get(), alpha),
                    Alignment::Left,
                ),
            ));
            let dot = x + TIME_CELL + DOT_GAP;
            let [r, g, b] = line.colour;
            f.fill(
                &Path::circle(Point::new(dot + DOT / 2.0, middle), DOT / 2.0),
                faded(Color::from_rgb8(r, g, b), alpha),
            );
            let tx = dot + DOT + DOT_GAP;
            let title = kit::elide(&line.title, (right - tx).max(0.0), |t| {
                kit::measure(t, TITLE, MANROPE)
            });
            f.fill_text(kit::text(
                title,
                Point::new(tx, middle),
                TITLE,
                faded(TEXT.get(), alpha),
                Alignment::Left,
            ));
        }
        Row::More(n) => {
            f.fill_text(kit::text(
                words.more(*n),
                Point::new(x, middle),
                MORE,
                faded(FAINT.get(), alpha),
                Alignment::Left,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str) -> Row {
        Row::Event(super::super::Line {
            id: id.to_owned(),
            start: "15:00".to_owned(),
            end: "16:00".to_owned(),
            colour: [0, 0, 0],
            title: id.to_owned(),
            video: None,
            page: None,
        })
    }

    #[test]
    fn the_first_line_going_fades_out_and_those_under_it_move_up() {
        let t = Instant::now();
        let mut state = State::default();
        let (a, b, c, d) = (event("a"), event("b"), event("c"), event("d"));
        state.follow(&[a.clone(), b.clone(), Row::More(2)], t, true);
        assert!(state.still(t));
        // "a" became the card: it fades out where it was, "b" moves up from
        // the second line, "c" comes out of "+ N" (one left in it).
        state.follow(&[b.clone(), c.clone(), Row::More(1)], t, true);
        assert_eq!(state.going.len(), 1);
        assert_eq!((state.going[0].0.key(), state.going[0].1), ("a", 0));
        assert_eq!(state.moving["b"].from, Some(1));
        assert_eq!(state.moving["c"].from, None);
        assert!(!state.still(t + Duration::from_millis(239)));
        assert!(state.still(t + SLIDE));
        // Without motion: at once.
        let mut state = State::default();
        state.follow(&[a, b.clone()], t, false);
        state.follow(&[b, d], t, false);
        assert!(state.going.is_empty() && state.moving.is_empty());
    }

    #[test]
    fn its_surface_holds_its_group_and_the_rings_room() {
        // Three lines: 18 + 60, 5 px of room around.
        assert_eq!(placement(158.0, 78.0), ((194, 88), 153.0));
        assert_eq!(row_top(0), 18.0);
        assert_eq!(row_top(2), 58.0);
    }
}
