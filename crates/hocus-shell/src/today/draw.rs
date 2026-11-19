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
//! - **The weather** (#814): on the header's right, its icon (15 px) and
//!   the temperature (Manrope 700, 11.5 px, `text`), 4 px apart; the rain's
//!   line in the rain's colour `#8FB3FF`, a 12 px rain icon in the dot's
//!   place; tomorrow's line "Demain", its 12 px icon and "3° / 11°". Each
//!   is lit under the pointer and opens the weather's popover. The mini
//!   form's item: 48 px wide, radius 10, its 22 px icon, the temperature
//!   (Manrope 700, 13 px), "dem." (9.5 px, `text.dim`) in the evening and
//!   three drops (4 px wide, 4, 8 and 6 px tall) when rain comes within
//!   3 h; lit `0.06` under the pointer, `0.08` while its popover is open.

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
    levels::draw::{DIM, FAINT, HOVER as MINI_HOVER, MANROPE, MANROPE_BOLD, OPEN, TEXT, faded},
    meeting::draw::MONO,
    popover::{Item, kit, weather::Spot},
    ui::items::{Control, Controls, Items, Press},
    weather::{self, icon},
};

/// The surface's width: the bar inside its rim.
const FULL_WIDTH: u32 = 194;
/// In the mini form: the bar inside its rim, and the weather's item.
const MINI_WIDTH: u32 = 54;
const MINI_ITEM: f32 = 48.0;
const MINI_RADIUS: f32 = 10.0;
const MINI_ICON: f32 = 22.0;
const MINI_TEMP: f32 = 13.0;
const MINI_SUB: f32 = 9.5;
/// The header's weather: its icon, the gap after it, its words' size.
const HEAD_ICON: f32 = 15.0;
const HEAD_GAP: f32 = 4.0;
/// A weather line's icon.
const LINE_ICON: f32 = 12.0;
const LINE_GAP: f32 = 6.0;
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

/// The mini form's weather item's surface: its size and its top under the
/// bar's, for an item `height` tall whose top is `top`.
pub fn mini_placement(top: f32, height: f32) -> ((u32, u32), f32) {
    (
        (MINI_WIDTH, (height.max(1.0) + 2.0 * ROOM).ceil() as u32),
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
    /// The bar's width as drawn now (the mini item is centred in it).
    pub bar_width: f32,
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
        self.alpha_of(Form::Full)
    }

    fn alpha_of(&self, form: Form) -> f32 {
        self.forms
            .iter()
            .find(|(f, _)| *f == form)
            .map_or(0.0, |(_, a)| *a)
    }

    /// The mini form's weather item on a surface `size` large.
    fn mini_rect(&self, size: Size, mini: &weather::Mini) -> Rectangle {
        let centre = size.width + 1.0 - self.bar_width / 2.0;
        Rectangle::new(
            Point::new(centre - MINI_ITEM / 2.0, ROOM),
            Size::new(MINI_ITEM, super::mini_height_of(Some(mini))),
        )
    }

    /// The header's weather on a surface `size` large: where it is lit and
    /// heard, 6 px past it.
    fn head_rect(&self, size: Size, head: &weather::Head) -> Rectangle {
        let o = self.origin(size);
        let width = HEAD_ICON + HEAD_GAP + kit::measure(&head.said, LABEL, MANROPE_BOLD);
        let right = o.x + GROUP_WIDTH;
        Rectangle::new(
            Point::new(right - width - OUT, o.y),
            Size::new(width + 2.0 * OUT, HEADER),
        )
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
        let shown = super::shown(&self.bar);
        let rows = shown.rows;
        let mini = shown.form == Some(Form::Mini);
        let top = if mini { shown.mini_top } else { shown.top };
        let to_bar = Vector::new(-size.width - 1.0, top - ROOM);
        let words = weather::Words::new(self.words.lang);
        if mini {
            let controls = match (&shown.mini, self.forms.as_slice()) {
                (Some(item), [(Form::Mini, alpha)]) if *alpha > 0.0 => {
                    vec![Control::new(
                        Item::Weather(Spot::Mini),
                        self.mini_rect(size, item),
                        Role::Button,
                        format!("{}, {}", words.weather(), item.said),
                        MINI_RADIUS,
                    )]
                }
                _ => Vec::new(),
            };
            return Controls {
                to_bar,
                controls,
                ..Controls::default()
            };
        }
        if self.forms.len() != 1 || self.alpha() <= 0.0 {
            return Controls {
                to_bar,
                ..Controls::default()
            };
        }
        let said = crate::a11y::Words::new(self.words.lang);
        let mut controls: Vec<Control> = rows
            .iter()
            .enumerate()
            .filter_map(|(i, row)| {
                let name = match row {
                    Row::Event(line) => self.words.name(line),
                    Row::More(n) => said.more_today(*n),
                    Row::Rain(rain) => format!("{} {}", rain.start, rain.said).trim().to_owned(),
                    Row::Tomorrow(t) => format!("{} {}", words.tomorrow(), t.said),
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
        if let Some(head) = &shown.head {
            controls.insert(
                0,
                Control::new(
                    Item::Weather(Spot::Head),
                    self.head_rect(size, head),
                    Role::Button,
                    format!("{}, {}", words.weather(), head.said),
                    RADIUS,
                ),
            );
        }
        let events = rows
            .iter()
            .map(|r| match r {
                Row::Event(_) => 1,
                Row::More(n) => *n,
                Row::Rain(_) | Row::Tomorrow(_) => 0,
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
        let rows = super::shown(&self.bar).rows;
        state.follow(&rows, *now, self.motion);
        (!state.still(*now)).then(|| canvas::Action::request_redraw_at(*now + STEP))
    }

    /// A line under the pointer shows its times and its title whole.
    fn hovered(&self, _state: &mut State, on: Option<(Item, Rectangle)>) {
        let rows = super::shown(&self.bar).rows;
        let line = on.and_then(|(item, rect)| {
            let i = (0..rows.len()).find(|i| item_of(&rows, *i) == Some(item))?;
            match rows.get(i)? {
                Row::Event(line) => Some((line, rect)),
                Row::More(_) | Row::Rain(_) | Row::Tomorrow(_) => None,
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
        let shown = super::shown(&self.bar);
        if shown.form == Some(Form::Mini) {
            if let Some(mini) = &shown.mini {
                let alpha = self.alpha_of(Form::Mini);
                let open = crate::popover::with(|c| c.shows(&self.bar, Item::Weather(Spot::Mini)));
                let lit = match (open, lit == Some(Item::Weather(Spot::Mini))) {
                    (true, _) => Some(OPEN),
                    (false, true) => Some(MINI_HOVER),
                    (false, false) => None,
                };
                draw_mini(&mut frame, mini, self.mini_rect(size, mini), lit, alpha);
                said_once(format!(
                    "mini {} {:?} drops={}",
                    mini.said, mini.tomorrow, mini.drops
                ));
            }
            return crate::backdrop::over(under, frame);
        }
        let (rows, top) = (shown.rows, shown.top);
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
        if let Some(head) = &shown.head {
            let rect = self.head_rect(size, head);
            if still && lit == Some(Item::Weather(Spot::Head)) {
                crate::theme::hover(f, &kit::rounded(rect, RADIUS), faded(HOVER.get(), alpha));
            }
            let x = rect.x + OUT;
            let middle = o.y + HEADER / 2.0;
            icon::draw(
                f,
                head.icon,
                Rectangle::new(
                    Point::new(x, middle - HEAD_ICON / 2.0),
                    Size::new(HEAD_ICON, HEAD_ICON),
                ),
                alpha,
            );
            f.fill_text(kit::text_in(
                MANROPE_BOLD,
                kit::text(
                    head.said.clone(),
                    Point::new(x + HEAD_ICON + HEAD_GAP, middle),
                    LABEL,
                    faded(TEXT.get(), alpha),
                    Alignment::Left,
                ),
            ));
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
                    Row::Rain(rain) => rain.said.clone(),
                    Row::Tomorrow(t) => t.said.clone(),
                })
                .collect();
            let head = shown.head.as_ref().map(|h| h.said.as_str());
            let said = format!("{} lit={:?} head={head:?}", self.words.said(&rows), hover);
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

/// Row `i`'s item: an event's line, the "+ N", the weather's.
fn item_of(rows: &[Row], i: usize) -> Option<Item> {
    Some(match rows.get(i)? {
        Row::Event(_) => Item::TodayLine(u8::try_from(i).ok()?),
        Row::More(_) => Item::TodayMore,
        Row::Rain(_) => Item::Weather(Spot::Rain),
        Row::Tomorrow(_) => Item::Weather(Spot::Tomorrow),
    })
}

/// What the mini item last said it shows, for the log.
static MINI_SAID: Mutex<Option<String>> = Mutex::new(None);

fn said_once(said: String) {
    let mut kept = MINI_SAID.lock().unwrap_or_else(PoisonError::into_inner);
    if kept.as_deref() != Some(said.as_str()) {
        tracing::debug!(shows = %said, "today's weather is drawn in the mini form");
        *kept = Some(said);
    }
}

/// The mini form's weather item at `rect`.
fn draw_mini(f: &mut Frame, mini: &weather::Mini, rect: Rectangle, lit: Option<Ink>, alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    if let Some(lit) = lit {
        f.fill(&kit::rounded(rect, MINI_RADIUS), faded(lit.get(), alpha));
    }
    let cx = rect.center_x();
    let mut y = rect.y + 5.0;
    icon::draw(
        f,
        mini.icon,
        Rectangle::new(
            Point::new(cx - MINI_ICON / 2.0, y),
            Size::new(MINI_ICON, MINI_ICON),
        ),
        alpha,
    );
    y += MINI_ICON + 1.0 + 9.0;
    f.fill_text(kit::text_in(
        MANROPE_BOLD,
        kit::text(
            mini.said.clone(),
            Point::new(cx, y),
            MINI_TEMP,
            faded(TEXT.get(), alpha),
            Alignment::Center,
        ),
    ));
    y += 9.0;
    if let Some(tomorrow) = &mini.tomorrow {
        f.fill_text(kit::text(
            tomorrow.clone(),
            Point::new(cx, y + super::MINI_TOMORROW / 2.0),
            MINI_SUB,
            faded(DIM.get(), alpha),
            Alignment::Center,
        ));
        y += super::MINI_TOMORROW;
    }
    if mini.drops {
        // Three drops, 4 px wide, 2 px apart, their feet on a line.
        let feet = y + 2.0 + 8.0;
        for (i, tall) in [4.0f32, 8.0, 6.0].into_iter().enumerate() {
            let x = cx + 6.0 * (i as f32 - 1.0);
            f.fill(
                &kit::rounded(
                    Rectangle::new(Point::new(x - 2.0, feet - tall), Size::new(4.0, tall)),
                    1.0,
                ),
                faded(icon::RAIN.get(), alpha),
            );
        }
    }
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
    let dot = x + TIME_CELL + DOT_GAP;
    match row {
        Row::Rain(rain) => {
            let colour = faded(icon::RAIN.get(), alpha);
            f.fill_text(kit::text_in(
                MONO,
                kit::text(
                    rain.start.clone(),
                    Point::new(x, middle),
                    TIME,
                    colour,
                    Alignment::Left,
                ),
            ));
            let centre = dot + DOT / 2.0;
            icon::draw(
                f,
                rain.icon,
                Rectangle::new(
                    Point::new(centre - LINE_ICON / 2.0, middle - LINE_ICON / 2.0),
                    Size::new(LINE_ICON, LINE_ICON),
                ),
                alpha,
            );
            let tx = dot + DOT + DOT_GAP;
            let said = kit::elide(&rain.said, (right - tx).max(0.0), |t| {
                kit::measure(t, TITLE, MANROPE)
            });
            f.fill_text(kit::text(
                said,
                Point::new(tx, middle),
                TITLE,
                colour,
                Alignment::Left,
            ));
            return;
        }
        Row::Tomorrow(tomorrow) => {
            let colour = faded(TEXT.get(), alpha);
            let name = words_tomorrow(words);
            f.fill_text(kit::text(
                name.to_owned(),
                Point::new(x, middle),
                MORE,
                colour,
                Alignment::Left,
            ));
            let ix = (x + kit::measure(name, MORE, MANROPE) + LINE_GAP)
                .max(dot + DOT / 2.0 - LINE_ICON / 2.0);
            icon::draw(
                f,
                tomorrow.icon,
                Rectangle::new(
                    Point::new(ix, middle - LINE_ICON / 2.0),
                    Size::new(LINE_ICON, LINE_ICON),
                ),
                alpha,
            );
            f.fill_text(kit::text(
                tomorrow.said.clone(),
                Point::new(ix + LINE_ICON + LINE_GAP, middle),
                TITLE,
                colour,
                Alignment::Left,
            ));
            return;
        }
        Row::Event(_) | Row::More(_) => {}
    }
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
        Row::Rain(_) | Row::Tomorrow(_) => {}
    }
}

/// "Demain" / "Tomorrow", in Today's language.
fn words_tomorrow(words: Words) -> &'static str {
    weather::Words::new(words.lang).tomorrow()
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
            at: 0,
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
