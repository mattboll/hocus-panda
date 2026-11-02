//! The launcher's surface (5.1, variant C): its field and its grid as the
//! mockup draws them, logical pixels, and what the pointer, the wheel and
//! the keys do on it.
//!
//! The panel is 760 px wide, centred, its top 54 px from the screen's: the
//! field (460 × 54, centred), 22 px, then the grid's area down to 98 px
//! from the screen's bottom. The grid's rows are its headings (24 px) and
//! its rows of eight 92 × 106 tiles, 6 px apart; the area shows the whole
//! rows that fit from the first one scrolled to, and a thin mark on its
//! right when there are more.
//!
//! The cards (5.2, variant A) are rows of their own, 736 px wide: a
//! result's (68 px), an address's (54 px) above the grid, and the web
//! search's (50 px) last, 8 px under each.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use iced::{
    Color, Pixels, Point, Rectangle, Renderer, Size, Theme, alignment, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke, gradient},
        text::{Alignment, LineHeight, Shaping},
    },
    window,
};

use super::{Card, Grid, Open, Row, Words};
use crate::{
    levels::draw::{DIM, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb},
    popover::kit::{self, MANROPE_SEMIBOLD, Wheel},
};

// --- The mockup's sizes ---------------------------------------------------------

const PANEL: f32 = hocus_link::LAUNCHER_WIDTH as f32;
const TOP: f32 = hocus_link::LAUNCHER_TOP as f32;
const BOTTOM: f32 = hocus_link::LAUNCHER_BOTTOM as f32;
const FIELD: Size = Size::new(460.0, 54.0);
const FIELD_RADIUS: f32 = 12.0;
const FIELD_PAD_LEFT: f32 = 14.0;
const FIELD_PAD_RIGHT: f32 = 12.0;
const MAGNIFIER: f32 = 18.0;
const FIELD_GAP: f32 = 10.0;
const QUERY: f32 = 19.0;
const HINT: f32 = 11.0;
const HINT_PAD: f32 = 6.0;
const HINT_HEIGHT: f32 = 21.0;
const HINT_RADIUS: f32 = 6.0;
/// The gap between the field and the grid.
const GAP: f32 = 22.0;
/// A heading's row, its letters 11 px 700 spaced by 0.08 em, 4 px in.
const HEAD: f32 = 24.0;
const HEAD_TEXT: f32 = 11.0;
const HEAD_SPACING: f32 = 0.08 * HEAD_TEXT;
const HEAD_LEFT: f32 = 4.0;
/// A tile, and a row of them with the gap under it.
pub const TILE: Size = Size::new(92.0, 106.0);
const TILE_ROW: f32 = 112.0;
const TILE_RADIUS: f32 = 12.0;
/// Its icon from its top, then its name 7 px under it, 4 px in from its
/// sides, two lines of 15 px at most.
const ICON_TOP: f32 = 9.0;
pub const ICON: f32 = 48.0;
const ICON_RADIUS: f32 = 12.0;
const LETTER: f32 = 19.0;
const NAME_TOP: f32 = ICON_TOP + ICON + 7.0;
const NAME: f32 = 11.5;
const NAME_LINE: f32 = 15.0;
const NAME_ROOM: f32 = TILE.width - 2.0 * 4.0;
/// The scroll mark, 14 px past the area's right.
const MARK_OUT: f32 = 14.0;
const MARK_WIDTH: f32 = 3.0;
/// The full name over a tile whose name is cut, after resting on it.
const TOOLTIP_AFTER: Duration = Duration::from_millis(500);
const TOOLTIP: f32 = 12.0;
/// A card: 736 px wide, radius 14, 8 px under it; 16 px in, its badge,
/// 14 px, its two lines (the first 26 px Martian Mono for a result, 15 px
/// 600 otherwise; then 12 px, 2 px under), 14 px, and what `Enter` does
/// (11.5 px 600) while it is selected.
const CARD_WIDTH: f32 = 736.0;
const CARD_UNDER: f32 = 8.0;
const CARD_RADIUS: f32 = 14.0;
const CARD_PAD: f32 = 16.0;
const CARD_GAP: f32 = 14.0;
const ANSWER_BADGE: f32 = 40.0;
const BADGE: f32 = 34.0;
const BADGE_ICON: f32 = 20.0;
const EQUALS: f32 = 20.0;
const RESULT: f32 = 26.0;
const CARD_MAIN: f32 = 15.0;
const CARD_SUB: f32 = 12.0;
const CARD_LINES_GAP: f32 = 2.0;
const CARD_HINT: f32 = 11.5;
const MONO: iced::Font = iced::Font::with_name("Martian Mono");

const FIELD_FILL: Color = Color::from_rgba8(255, 255, 255, 0.05);
const FIELD_RIM: Color = Color::from_rgba8(108, 194, 135, 0.45);
const PLACEHOLDER: Color = rgb(0x7C8698);
const HINT_RIM: Color = Color::from_rgba8(255, 255, 255, 0.14);
const LABEL: Color = rgb(0xC9D0DC);
const SELECTED_FILL: Color = Color::from_rgba8(108, 194, 135, 0.16);
const SELECTED_RIM: Color = Color::from_rgba8(108, 194, 135, 0.6);
const MARK: Color = Color::from_rgba8(255, 255, 255, 0.22);
/// The neutral tile of an application without an icon.
const NEUTRAL: (Color, Color) = (rgb(0x5C6576), rgb(0x9AA3B4));
const NEUTRAL_RIM: Color = Color::from_rgba8(255, 255, 255, 0.14);
const TOOLTIP_FILL: Color = Color::from_rgba8(22, 27, 42, 0.96);
const TOOLTIP_RIM: Color = Color::from_rgba8(255, 255, 255, 0.12);
/// On the CPU: about the desktop's night blue under the compositor's dim.
const OPAQUE: Color = rgb(0x0A0E18);
const CARD_FILL: Color = Color::from_rgba8(255, 255, 255, 0.04);
const CARD_RIM: Color = Color::from_rgba8(255, 255, 255, 0.10);
const CARD_SUB_INK: Color = rgb(0x8C96A8);
const CARD_HINT_INK: Color = rgb(0x8FE0A8);
const BADGE_RIM: Color = Color::from_rgba8(255, 255, 255, 0.14);
const ANSWER_FILL: Color = rgb(0x6CC287);
const EQUALS_INK: Color = rgb(0x0B1020);
const ADDRESS_FILL: (Color, Color) = (rgb(0x2E7D6B), rgb(0x5CC0A6));
const WEB_FILL: (Color, Color) = (rgb(0x3B5BA9), rgb(0x6E8FE0));

impl Card {
    /// Its row's height: the card, and the gap under it.
    fn row(self) -> f32 {
        let card = match self {
            Self::Answer => 68.0,
            Self::Address => 54.0,
            Self::Web => 50.0,
        };
        card + CARD_UNDER
    }

    fn badge(self) -> f32 {
        match self {
            Self::Answer => ANSWER_BADGE,
            Self::Address | Self::Web => BADGE,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::Answer => "answer",
            Self::Address => "address",
            Self::Web => "web",
        }
    }
}

// --- Where things are -------------------------------------------------------------

/// The field on a surface `size` large.
fn field(size: Size) -> Rectangle {
    Rectangle::new(
        Point::new(((size.width - FIELD.width) / 2.0).round(), TOP),
        FIELD,
    )
}

/// The grid's area on a surface `size` large.
fn area(size: Size) -> Rectangle {
    let top = TOP + FIELD.height + GAP;
    let height = (size.height - BOTTOM - top).max(HEAD + TILE_ROW);
    Rectangle::new(
        Point::new(((size.width - PANEL) / 2.0).round(), top),
        Size::new(PANEL, height),
    )
}

fn height_of(row: &Row) -> f32 {
    match row {
        Row::Head(_) => HEAD,
        Row::Tiles(_) => TILE_ROW,
        Row::Card(card, _) => card.row(),
    }
}

/// The row after the last one shown from row `offset` in `height`: whole
/// rows only, one at least.
fn end_of(grid: &Grid, offset: usize, height: f32) -> usize {
    let mut sum = 0.0;
    let mut end = offset;
    while let Some(row) = grid.rows.get(end) {
        let h = height_of(row);
        if sum + h > height && end > offset {
            break;
        }
        sum += h;
        end += 1;
    }
    end
}

/// The furthest the grid scrolls in `height`: its last rows filling it.
fn most_offset(grid: &Grid, height: f32) -> usize {
    let mut sum = 0.0;
    for (i, row) in grid.rows.iter().enumerate().rev() {
        sum += height_of(row);
        if sum > height {
            return i + 1;
        }
    }
    0
}

/// The first row to show so that tile `selected` shows, from `offset`, on
/// a surface `size` large: a row above comes with its heading.
pub fn keep_in_view(grid: &Grid, selected: usize, offset: usize, size: Size) -> usize {
    let Some((row, _)) = grid.place(selected) else {
        return offset;
    };
    if row < offset {
        return if row > 0 && matches!(grid.rows.get(row - 1), Some(Row::Head(_))) {
            row - 1
        } else {
            row
        };
    }
    let height = area(size).height;
    let mut offset = offset;
    while end_of(grid, offset, height) <= row && offset < row {
        offset += 1;
    }
    offset
}

/// The grid scrolled `by` rows from `offset` (down positive), within its
/// ends.
pub fn scrolled(grid: &Grid, offset: usize, by: i32, size: Size) -> usize {
    let most = most_offset(grid, area(size).height);
    (offset as i64 + i64::from(by)).clamp(0, most as i64) as usize
}

/// What is where on the surface.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub field: Rectangle,
    pub area: Rectangle,
    pub heads: Vec<(&'static str, Rectangle)>,
    /// The tiles shown: their place in the grid's order, and where.
    pub tiles: Vec<(usize, Rectangle)>,
    /// The cards shown: their place in the grid's order, and where.
    pub cards: Vec<(usize, Card, Rectangle)>,
    /// The scroll mark, when the grid overflows.
    pub mark: Option<Rectangle>,
}

impl Layout {
    pub fn of(size: Size, grid: &Grid, offset: usize) -> Self {
        let area = area(size);
        let end = end_of(grid, offset, area.height);
        let mut heads = Vec::new();
        let mut tiles = Vec::new();
        let mut cards = Vec::new();
        let left = area.x + (PANEL - super::COLUMNS as f32 * TILE.width) / 2.0;
        let mut y = area.y;
        for row in grid.rows.get(offset..end).unwrap_or_default() {
            match row {
                Row::Head(label) => heads.push((
                    *label,
                    Rectangle::new(Point::new(area.x, y), Size::new(PANEL, HEAD)),
                )),
                Row::Tiles(row) => {
                    for (column, at) in row.iter().enumerate() {
                        let x = left + column as f32 * TILE.width;
                        tiles.push((*at, Rectangle::new(Point::new(x, y), TILE)));
                    }
                }
                Row::Card(card, at) => cards.push((
                    *at,
                    *card,
                    Rectangle::new(
                        Point::new(area.x + (PANEL - CARD_WIDTH) / 2.0, y),
                        Size::new(CARD_WIDTH, card.row() - CARD_UNDER),
                    ),
                )),
            }
            y += height_of(row);
        }
        let total = grid.rows.len();
        let shown = end.saturating_sub(offset);
        let mark = (total > shown).then(|| {
            Rectangle::new(
                Point::new(
                    area.x + area.width + MARK_OUT - MARK_WIDTH,
                    area.y + area.height * offset as f32 / total as f32,
                ),
                Size::new(MARK_WIDTH, area.height * shown as f32 / total as f32),
            )
        });
        Self {
            field: field(size),
            area,
            heads,
            tiles,
            cards,
            mark,
        }
    }

    /// The tile or the card under `at`.
    pub fn item_at(&self, at: Point) -> Option<usize> {
        self.tiles
            .iter()
            .find(|(_, rect)| rect.contains(at))
            .map(|(i, _)| *i)
            .or_else(|| {
                self.cards
                    .iter()
                    .find(|(_, _, rect)| rect.contains(at))
                    .map(|(i, ..)| *i)
            })
    }

    fn rect_of(&self, tile: usize) -> Option<Rectangle> {
        self.tiles.iter().find(|(i, _)| *i == tile).map(|(_, r)| *r)
    }
}

// --- Names on two lines -----------------------------------------------------------

/// A tile's name as it shows: two lines at most, the second cut with an
/// ellipsis when the name is longer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub lines: Vec<String>,
    pub cut: bool,
}

/// `name` broken into lines `room` wide as `width` measures them: between
/// words, a word too wide alone broken between its letters; two lines at
/// most, the rest cut with "…".
pub fn label(name: &str, room: f32, mut width: impl FnMut(&str) -> f32) -> Label {
    let mut lines: Vec<String> = Vec::new();
    for word in name.split_whitespace() {
        let joined = lines.last().map(|line| format!("{line} {word}"));
        match joined {
            Some(joined) if width(&joined) <= room => {
                if let Some(line) = lines.last_mut() {
                    *line = joined;
                }
            }
            _ => {
                // A word too wide alone goes over lines, letter by letter.
                let mut piece = String::new();
                for ch in word.chars() {
                    piece.push(ch);
                    if width(&piece) > room && piece.chars().count() > 1 {
                        piece.pop();
                        lines.push(std::mem::take(&mut piece));
                        piece.push(ch);
                    }
                }
                lines.push(piece);
            }
        }
    }
    if lines.len() <= 2 {
        return Label { lines, cut: false };
    }
    let rest = lines[1..].join(" ");
    let second = kit::elide(&format!("{rest}…"), room, &mut width);
    // The ellipsis always shows: something was left out.
    let second = if second.ends_with('…') {
        second
    } else {
        format!("{second}…")
    };
    Label {
        lines: vec![lines[0].clone(), second],
        cut: true,
    }
}

/// The tiles' names as cut, by name: measured once.
static LABELS: Mutex<Option<HashMap<String, Label>>> = Mutex::new(None);

fn labels<T>(with: impl FnOnce(&mut HashMap<String, Label>) -> T) -> T {
    let mut labels = LABELS.lock().unwrap_or_else(PoisonError::into_inner);
    with(labels.get_or_insert_with(HashMap::new))
}

/// `name`'s label, if it was measured.
fn label_of(name: &str) -> Option<Label> {
    labels(|l| l.get(name).cloned())
}

/// The labels of the tiles in `names` not measured yet, measured.
fn measure_labels<'a>(names: impl IntoIterator<Item = &'a str>) {
    let missing: Vec<&str> = names
        .into_iter()
        .filter(|n| labels(|l| !l.contains_key(*n)))
        .collect();
    if missing.is_empty() {
        return;
    }
    let made: Vec<(String, Label)> = match iced::advanced::graphics::text::font_system().write() {
        Ok(mut system) => missing
            .iter()
            .map(|name| {
                let label = label(name, NAME_ROOM, |t| {
                    kit::measure_in(system.raw(), t, NAME, MANROPE)
                });
                ((*name).to_owned(), label)
            })
            .collect(),
        Err(_) => return,
    };
    labels(|l| l.extend(made));
}

// --- The surface ------------------------------------------------------------------

/// The launcher on its surface.
#[derive(Debug, Clone)]
pub struct View {
    pub id: window::Id,
    pub words: Words,
    /// The shell draws on the CPU, whose surfaces have no transparency:
    /// the desktop behind cannot show, an opaque dark shows instead.
    pub software: bool,
}

/// What the surface keeps between draws: the field's text as it fits, and
/// the wheel's notches.
#[derive(Debug, Default)]
pub struct State {
    wheel: Wheel,
    /// The query, how it shows, and where the caret is after it.
    field: (String, String, f32),
    /// The hint's width.
    hint: f32,
    /// What was last logged of what it shows.
    logged: String,
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
        let size = bounds.size();
        match event {
            iced::Event::Window(window::Event::RedrawRequested(_)) => {
                self.prepare(state, size);
                None
            }
            iced::Event::Mouse(seen) => self.mouse(state, seen, cursor.position_in(bounds), size),
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => super::key_on(self.id, key, *modifiers, text.as_deref())
                .then(|| canvas::Action::request_redraw().and_capture()),
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
        if self.software {
            frame.fill_rectangle(Point::ORIGIN, bounds.size(), OPAQUE);
        }
        super::with(|l| {
            if let Some(open) = l.open_on(self.id) {
                self.draw_open(&mut frame, state, open, bounds.size());
            }
        });
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let Some(at) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        super::with(|l| {
            let Some(open) = l.open_on(self.id) else {
                return mouse::Interaction::default();
            };
            let layout = Layout::of(bounds.size(), &open.grid, open.offset);
            if layout.item_at(at).is_some() {
                mouse::Interaction::Pointer
            } else if layout.field.contains(at) {
                mouse::Interaction::Text
            } else {
                mouse::Interaction::default()
            }
        })
    }
}

impl View {
    /// Before it draws: its size kept, the names shown measured, the
    /// field's text fitted, and what it shows logged when it changed.
    fn prepare(&self, state: &mut State, size: Size) {
        let shown = super::with(|l| {
            let open = l.open_mut(self.id)?;
            open.size = size;
            let layout = Layout::of(size, &open.grid, open.offset);
            let names: Vec<String> = layout
                .tiles
                .iter()
                .filter_map(|(at, _)| open.app(*at).map(|a| a.entry.name.clone()))
                .collect();
            Some((open.query.clone(), names))
        });
        let Some((query, names)) = shown else {
            return;
        };
        measure_labels(names.iter().map(String::as_str));
        if state.hint == 0.0 {
            state.hint = kit::measure(self.words.escape(), HINT, MANROPE_SEMIBOLD);
        }
        if state.field.0 != query || state.field.1.is_empty() != query.is_empty() {
            let room = text_room(field(size), state.hint);
            let width = |t: &str| kit::measure(t, QUERY, MANROPE);
            let fitted = tail(&query, room, width);
            let caret = if query.is_empty() {
                0.0
            } else {
                width(&fitted)
            };
            state.field = (query, fitted, caret);
        }
        let said = super::with(|l| l.open_on(self.id).map(|open| self.said(open, size)));
        if let Some(said) = said
            && said != state.logged
        {
            tracing::debug!(said = %said, "the launcher shows it");
            state.logged = said;
        }
    }

    /// What it shows, in words, for the log (and the tests).
    fn said(&self, open: &Open, size: Size) -> String {
        let layout = Layout::of(size, &open.grid, open.offset);
        let px = super::icons::pixels(ICON as u32, open.scale);
        let mut rows: Vec<String> = Vec::new();
        let mut last_y = f32::NAN;
        for (label, rect) in &layout.heads {
            rows.push(format!("{}@{}", label, rect.y));
        }
        let mut tiles: Vec<(f32, String)> = Vec::new();
        for (at, rect) in &layout.tiles {
            let Some(app) = open.app(*at) else {
                continue;
            };
            let label = label_of(&app.entry.name)
                .map_or_else(|| app.entry.name.clone(), |l| l.lines.join(" / "));
            let icon = match &app.entry.icon {
                None => "letter",
                Some(name) => match super::icons::of(name, px) {
                    Some(Some(_)) => "icon",
                    Some(None) => "letter",
                    None => "…",
                },
            };
            let mark = if *at == open.selected { "*" } else { "" };
            tiles.push((rect.y, format!("{mark}{label}[{icon}]")));
        }
        let mut lines: Vec<String> = Vec::new();
        for (y, tile) in tiles {
            if y != last_y {
                lines.push(String::new());
                last_y = y;
            }
            if let Some(line) = lines.last_mut() {
                if !line.is_empty() {
                    line.push_str(", ");
                }
                line.push_str(&tile);
            }
        }
        let cards: Vec<String> = layout
            .cards
            .iter()
            .map(|(at, card, _)| {
                let (main, sub) = self.lines(open, *card);
                let mark = if *at == open.selected { "*" } else { "" };
                format!("{mark}{}: {main} | {sub}", card.word())
            })
            .collect();
        let tooltip = tooltip_of(open).map(|(_, name)| name).unwrap_or_default();
        format!(
            "query={:?} heads={rows:?} tiles={lines:?} cards={cards:?} offset={} mark={} \
             tooltip=\"{tooltip}\"",
            open.query,
            open.offset,
            layout.mark.is_some(),
        )
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
        size: Size,
    ) -> Option<canvas::Action<Message>> {
        let now = Instant::now();
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        enum Then {
            Nothing,
            Draw,
            DrawAt,
            Close,
            Act(usize),
        }
        let then = super::with(|l| {
            let open = l.open_mut(self.id)?;
            let layout = Layout::of(size, &open.grid, open.offset);
            let target = at.and_then(|at| layout.item_at(at));
            Some(match seen {
                mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                    open.pointer = at;
                    hovered(open, target, now)
                }
                mouse::Event::CursorLeft => {
                    open.pointer = None;
                    open.hover = None;
                    Then::Draw
                }
                mouse::Event::ButtonPressed(mouse::Button::Left) => match target {
                    Some(tile) => {
                        open.pressed = Some(tile);
                        Then::Nothing
                    }
                    None if at.is_some_and(|at| layout.field.contains(at)) => Then::Nothing,
                    None => Then::Close,
                },
                mouse::Event::ButtonReleased(mouse::Button::Left) => {
                    match open.pressed.take().filter(|p| Some(*p) == target) {
                        Some(tile) => Then::Act(tile),
                        None => Then::Nothing,
                    }
                }
                mouse::Event::WheelScrolled { delta } => {
                    let notches = state.wheel.notches(delta, open.scale);
                    if notches == 0 {
                        return Some(Then::Nothing);
                    }
                    open.offset = scrolled(&open.grid, open.offset, -notches, size);
                    // What is under the pointer now.
                    let layout = Layout::of(size, &open.grid, open.offset);
                    let target = open.pointer.and_then(|at| layout.item_at(at));
                    match hovered(open, target, now) {
                        Then::Nothing => Then::Draw,
                        then => then,
                    }
                }
                _ => Then::Nothing,
            })
        });
        /// The pointer over `target`: it is selected, and its name shows
        /// whole a moment later when it is cut.
        fn hovered(open: &mut Open, target: Option<usize>, now: Instant) -> Then {
            if open.hover.map(|(t, _)| t) == target {
                return Then::Nothing;
            }
            open.hover = target.map(|t| (t, now));
            open.tooltip = None;
            let Some(tile) = target else {
                return Then::Draw;
            };
            open.selected = tile;
            let cut = open
                .app(tile)
                .and_then(|a| label_of(&a.entry.name))
                .is_some_and(|l| l.cut);
            if cut {
                open.tooltip = Some(now + TOOLTIP_AFTER);
                Then::DrawAt
            } else {
                Then::Draw
            }
        }
        match then? {
            Then::Nothing => None,
            Then::Draw => redraw,
            Then::DrawAt => {
                // The effects' loop wakes the surface when it is due.
                crate::popover::send(Vec::new());
                redraw
            }
            Then::Close => {
                super::close();
                redraw
            }
            Then::Act(at) => {
                super::act(self.id, at);
                redraw
            }
        }
    }

    fn draw_open(&self, f: &mut Frame, state: &State, open: &Open, size: Size) {
        let layout = Layout::of(size, &open.grid, open.offset);
        self.draw_field(f, state, &layout);
        for (label, rect) in &layout.heads {
            head(f, label, *rect);
        }
        let px = super::icons::pixels(ICON as u32, open.scale);
        for (at, rect) in &layout.tiles {
            let Some(app) = open.app(*at) else {
                continue;
            };
            if *at == open.selected {
                f.fill(&kit::rounded(*rect, TILE_RADIUS), SELECTED_FILL);
                f.stroke(
                    &kit::rounded(inset(*rect, 0.5), TILE_RADIUS - 0.5),
                    Stroke::default().with_color(SELECTED_RIM).with_width(1.0),
                );
            }
            let icon = Rectangle::new(
                Point::new(rect.center_x() - ICON / 2.0, rect.y + ICON_TOP),
                Size::new(ICON, ICON),
            );
            match &app.entry.icon {
                None => letter_tile(f, icon, &app.entry.name),
                Some(name) => match super::icons::of(name, px) {
                    Some(Some(handle)) => f.draw_image(icon, canvas::Image::new(handle)),
                    Some(None) => letter_tile(f, icon, &app.entry.name),
                    // Not read yet: it comes in a moment.
                    None => {}
                },
            }
            let lines =
                label_of(&app.entry.name).map_or_else(|| vec![app.entry.name.clone()], |l| l.lines);
            for (i, line) in lines.into_iter().enumerate() {
                f.fill_text(canvas::Text {
                    content: line,
                    position: Point::new(
                        rect.center_x(),
                        rect.y + NAME_TOP + NAME_LINE * (i as f32 + 0.5),
                    ),
                    max_width: f32::INFINITY,
                    color: LABEL,
                    size: Pixels(NAME),
                    line_height: LineHeight::Absolute(Pixels(NAME_LINE)),
                    font: MANROPE,
                    align_x: Alignment::Center,
                    align_y: alignment::Vertical::Center,
                    shaping: Shaping::Advanced,
                });
            }
        }
        for (at, card, rect) in &layout.cards {
            self.draw_card(f, open, *card, *rect, *at == open.selected);
        }
        if let Some(mark) = layout.mark {
            f.fill(&kit::rounded(mark, MARK_WIDTH / 2.0), MARK);
        }
        // The full name of a tile whose name is cut, rested on.
        if let Some((tile, name)) = tooltip_of(open)
            && let Some(rect) = layout.rect_of(tile)
        {
            tooltip(f, name, rect, size);
        }
    }

    /// A card's two lines: what it is, and under it what it says more.
    fn lines(&self, open: &Open, card: Card) -> (String, String) {
        let words = self.words;
        match card {
            Card::Answer => open
                .grid
                .answer
                .as_ref()
                .map(|a| (a.main.clone(), a.sub.clone()))
                .unwrap_or_default(),
            Card::Address => open
                .grid
                .address
                .as_ref()
                .map(|a| (words.open(&a.typed), words.in_browser(&a.url)))
                .unwrap_or_default(),
            Card::Web => (
                words.search_web(open.query.trim()),
                words.web_engine().to_owned(),
            ),
        }
    }

    fn draw_card(&self, f: &mut Frame, open: &Open, card: Card, rect: Rectangle, selected: bool) {
        let (fill, rim) = if selected {
            (SELECTED_FILL, SELECTED_RIM)
        } else {
            (CARD_FILL, CARD_RIM)
        };
        f.fill(&kit::rounded(rect, CARD_RADIUS), fill);
        f.stroke(
            &kit::rounded(inset(rect, 0.5), CARD_RADIUS - 0.5),
            Stroke::default().with_color(rim).with_width(1.0),
        );
        let size = card.badge();
        let badge = Rectangle::new(
            Point::new(rect.x + CARD_PAD, rect.center_y() - size / 2.0),
            Size::new(size, size),
        );
        badge_of(f, card, badge);
        // The hint keeps its room unselected, as the mockup's.
        let hint = self.words.hint(card);
        let hint_width = kit::measure(hint, CARD_HINT, MANROPE_SEMIBOLD);
        let right = rect.x + rect.width - CARD_PAD;
        if selected {
            f.fill_text(kit::text_in(
                MANROPE_SEMIBOLD,
                kit::text(
                    hint.to_owned(),
                    Point::new(right, rect.center_y()),
                    CARD_HINT,
                    CARD_HINT_INK,
                    Alignment::Right,
                ),
            ));
        }
        let x = badge.x + size + CARD_GAP;
        let room = (right - hint_width - CARD_GAP - x).max(0.0);
        let (main, sub) = self.lines(open, card);
        let (font, main_size) = match card {
            Card::Answer => (MONO, RESULT),
            Card::Address | Card::Web => (MANROPE_SEMIBOLD, CARD_MAIN),
        };
        let main_height = main_size * LINE;
        let sub_height = CARD_SUB * LINE;
        let top = rect.center_y() - (main_height + CARD_LINES_GAP + sub_height) / 2.0;
        let main = kit::elide(&main, room, |t| kit::measure(t, main_size, font));
        let sub = kit::elide(&sub, room, |t| kit::measure(t, CARD_SUB, MANROPE));
        f.fill_text(kit::text_in(
            font,
            kit::text(
                main,
                Point::new(x, top + main_height / 2.0),
                main_size,
                TEXT,
                Alignment::Left,
            ),
        ));
        f.fill_text(kit::text(
            sub,
            Point::new(x, top + main_height + CARD_LINES_GAP + sub_height / 2.0),
            CARD_SUB,
            CARD_SUB_INK,
            Alignment::Left,
        ));
    }

    fn draw_field(&self, f: &mut Frame, state: &State, layout: &Layout) {
        let field = layout.field;
        f.fill(&kit::rounded(field, FIELD_RADIUS), FIELD_FILL);
        f.stroke(
            &kit::rounded(inset(field, 0.5), FIELD_RADIUS - 0.5),
            Stroke::default().with_color(FIELD_RIM).with_width(1.0),
        );
        magnifier(
            f,
            Point::new(field.x + FIELD_PAD_LEFT, field.center_y() - MAGNIFIER / 2.0),
        );
        let x = field.x + FIELD_PAD_LEFT + MAGNIFIER + FIELD_GAP;
        let (query, shown, caret) = &state.field;
        let (content, colour) = if query.is_empty() {
            (self.words.placeholder().to_owned(), PLACEHOLDER)
        } else {
            (shown.clone(), TEXT)
        };
        f.fill_text(kit::text(
            content,
            Point::new(x, field.center_y()),
            QUERY,
            colour,
            Alignment::Left,
        ));
        // The caret after what is typed: the field has the keyboard.
        // Just before the placeholder's first letter, never on it; just
        // after the last one typed.
        let caret_x = if query.is_empty() {
            x - 2.0
        } else {
            x + caret + 1.0
        };
        let caret = Rectangle::new(
            Point::new(caret_x, field.center_y() - QUERY * 0.6),
            Size::new(1.0, QUERY * 1.2),
        );
        f.fill_rectangle(caret.position(), caret.size(), TEXT);
        // "Échap" in its frame, on the right.
        let width = state.hint + 2.0 * HINT_PAD + 2.0;
        let hint = Rectangle::new(
            Point::new(
                field.x + field.width - FIELD_PAD_RIGHT - width,
                field.center_y() - HINT_HEIGHT / 2.0,
            ),
            Size::new(width, HINT_HEIGHT),
        );
        f.stroke(
            &kit::rounded(inset(hint, 0.5), HINT_RADIUS - 0.5),
            Stroke::default().with_color(HINT_RIM).with_width(1.0),
        );
        f.fill_text(kit::text_in(
            MANROPE_SEMIBOLD,
            kit::text(
                self.words.escape().to_owned(),
                hint.center(),
                HINT,
                PLACEHOLDER,
                Alignment::Center,
            ),
        ));
    }
}

/// The tile whose full name shows over it, and that name: one whose name
/// is cut, rested on long enough.
fn tooltip_of(open: &Open) -> Option<(usize, &str)> {
    let (tile, since) = open.hover?;
    if Instant::now() < since + TOOLTIP_AFTER {
        return None;
    }
    let app = open.app(tile)?;
    label_of(&app.entry.name)
        .is_some_and(|l| l.cut)
        .then_some((tile, app.entry.name.as_str()))
}

/// The room the field's text has, between the magnifier and the hint
/// `hint` wide.
fn text_room(field: Rectangle, hint: f32) -> f32 {
    field.width
        - FIELD_PAD_LEFT
        - MAGNIFIER
        - FIELD_GAP
        - FIELD_GAP
        - (hint + 2.0 * HINT_PAD + 2.0)
        - FIELD_PAD_RIGHT
}

/// `text`'s end that fits `room`, after "…" when its start is left out:
/// what is typed last shows.
fn tail(text: &str, room: f32, width: impl Fn(&str) -> f32) -> String {
    if width(text) <= room {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    for skip in 1..chars.len() {
        let cut: String = std::iter::once('…')
            .chain(chars[skip..].iter().copied())
            .collect();
        if width(&cut) <= room {
            return cut;
        }
    }
    "…".to_owned()
}

fn inset(rect: Rectangle, by: f32) -> Rectangle {
    Rectangle::new(
        Point::new(rect.x + by, rect.y + by),
        Size::new(rect.width - 2.0 * by, rect.height - 2.0 * by),
    )
}

/// The magnifier, 18 px from `at`: the mockup's 24 px drawing.
fn magnifier(f: &mut Frame, at: Point) {
    let k = MAGNIFIER / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * k, at.y + y * k);
    let stroke = Stroke {
        line_cap: canvas::LineCap::Round,
        ..Stroke::default().with_color(DIM).with_width(2.2 * k)
    };
    f.stroke(&Path::circle(p(10.5, 10.5), 6.5 * k), stroke);
    f.stroke(&Path::line(p(15.5, 15.5), p(20.0, 20.0)), stroke);
}

/// A card's badge in `rect`: "=" on green for a result, a link for an
/// address, a globe for the web, each on its gradient and in a light rim.
fn badge_of(f: &mut Frame, card: Card, rect: Rectangle) {
    let radius = (rect.width * 0.28).round();
    let shape = kit::rounded(rect, radius);
    match card {
        Card::Answer => f.fill(&shape, ANSWER_FILL),
        Card::Address | Card::Web => {
            let (from, to) = if card == Card::Web {
                WEB_FILL
            } else {
                ADDRESS_FILL
            };
            let fill = gradient::Linear::new(
                rect.position(),
                Point::new(rect.x + rect.width, rect.y + rect.height),
            )
            .add_stop(0.0, from)
            .add_stop(1.0, to);
            f.fill(&shape, canvas::Fill::from(fill));
        }
    }
    f.stroke(
        &kit::rounded(inset(rect, 0.5), radius - 0.5),
        Stroke::default().with_color(BADGE_RIM).with_width(1.0),
    );
    let at = Point::new(
        rect.center_x() - BADGE_ICON / 2.0,
        rect.center_y() - BADGE_ICON / 2.0,
    );
    match card {
        Card::Answer => f.fill_text(kit::text_in(
            MONO,
            kit::text(
                "=".to_owned(),
                rect.center(),
                EQUALS,
                EQUALS_INK,
                Alignment::Center,
            ),
        )),
        Card::Address => link(f, at),
        Card::Web => globe(f, at),
    }
}

/// The mockup's 24 px globe, 20 px from `at`.
fn globe(f: &mut Frame, at: Point) {
    let k = BADGE_ICON / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * k, at.y + y * k);
    let stroke = |width: f32| Stroke::default().with_color(TEXT).with_width(width * k);
    f.stroke(&Path::circle(p(12.0, 12.0), 8.5 * k), stroke(1.8));
    let lines = Path::new(|b| {
        b.move_to(p(3.5, 12.0));
        b.line_to(p(20.5, 12.0));
        b.move_to(p(12.0, 3.5));
        b.bezier_curve_to(p(8.5, 7.0), p(8.5, 17.0), p(12.0, 20.5));
        b.move_to(p(12.0, 3.5));
        b.bezier_curve_to(p(15.5, 7.0), p(15.5, 17.0), p(12.0, 20.5));
    });
    f.stroke(&lines, stroke(1.6));
}

/// The mockup's 24 px chain link, 20 px from `at`: a bar between two
/// hooks, each a near half circle of radius 3.2.
fn link(f: &mut Frame, at: Point) {
    use std::f32::consts::PI;
    let k = BADGE_ICON / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * k, at.y + y * k);
    let stroke = Stroke {
        line_cap: canvas::LineCap::Round,
        ..Stroke::default().with_color(TEXT).with_width(1.8 * k)
    };
    let degrees = |d: f32| iced::Radians(d * PI / 180.0);
    let hook =
        |from: (f32, f32), start: (f32, f32), centre: (f32, f32), turn: f32, end: (f32, f32)| {
            Path::new(move |b| {
                b.move_to(p(from.0, from.1));
                b.line_to(p(start.0, start.1));
                b.arc(canvas::path::Arc {
                    center: p(centre.0, centre.1),
                    radius: 3.2 * k,
                    start_angle: degrees(turn),
                    end_angle: degrees(turn - 168.0),
                });
                b.line_to(p(end.0, end.1));
            })
        };
    f.stroke(&Path::line(p(10.0, 14.0), p(14.0, 10.0)), stroke);
    f.stroke(
        &hook((8.5, 11.5), (6.8, 13.2), (9.29, 15.21), 219.0, (13.0, 16.0)),
        stroke,
    );
    f.stroke(
        &hook((15.5, 12.5), (17.2, 10.8), (14.71, 8.79), 39.0, (11.0, 8.0)),
        stroke,
    );
}

/// A heading: its letters spaced, 4 px in, on its row's middle.
fn head(f: &mut Frame, label: &str, rect: Rectangle) {
    let mut x = rect.x + HEAD_LEFT;
    for ch in label.chars() {
        let letter = ch.to_string();
        let width = kit::measure(&letter, HEAD_TEXT, MANROPE_BOLD);
        f.fill_text(kit::text_in(
            MANROPE_BOLD,
            kit::text(
                letter,
                Point::new(x, rect.center_y()),
                HEAD_TEXT,
                PLACEHOLDER,
                Alignment::Left,
            ),
        ));
        x += width + HEAD_SPACING;
    }
}

/// An application without an icon: a neutral rounded square and its first
/// letter.
fn letter_tile(f: &mut Frame, rect: Rectangle, name: &str) {
    let shape = kit::rounded(rect, ICON_RADIUS);
    let fill = gradient::Linear::new(
        rect.position(),
        Point::new(rect.x + rect.width, rect.y + rect.height),
    )
    .add_stop(0.0, NEUTRAL.0)
    .add_stop(1.0, NEUTRAL.1);
    f.fill(&shape, canvas::Fill::from(fill));
    f.stroke(
        &kit::rounded(inset(rect, 0.5), ICON_RADIUS - 0.5),
        Stroke::default().with_color(NEUTRAL_RIM).with_width(1.0),
    );
    let first = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_default();
    f.fill_text(kit::text_in(
        MANROPE_BOLD,
        kit::text(
            first,
            rect.center(),
            LETTER,
            Color::WHITE,
            Alignment::Center,
        ),
    ));
}

/// `name` whole in a small frame under `tile`, inside the surface `size`.
fn tooltip(f: &mut Frame, name: &str, tile: Rectangle, size: Size) {
    let width = kit::measure(name, TOOLTIP, MANROPE) + 16.0;
    let height = TOOLTIP * LINE + 8.0;
    let x = (tile.center_x() - width / 2.0).clamp(4.0, (size.width - width - 4.0).max(4.0));
    let rect = Rectangle::new(
        Point::new(x, tile.y + tile.height + 4.0),
        Size::new(width, height),
    );
    f.fill(&kit::rounded(rect, 6.0), TOOLTIP_FILL);
    f.stroke(
        &kit::rounded(inset(rect, 0.5), 5.5),
        Stroke::default().with_color(TOOLTIP_RIM).with_width(1.0),
    );
    f.fill_text(kit::text(
        name.to_owned(),
        rect.center(),
        TOOLTIP,
        TEXT,
        Alignment::Center,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character 6 px wide, a space too.
    fn six(text: &str) -> f32 {
        text.chars().count() as f32 * 6.0
    }

    #[test]
    fn a_name_takes_two_whole_lines_at_most() {
        // 14 characters to a line.
        let short = label("Firefox", 84.0, six);
        assert_eq!(short.lines, ["Firefox"]);
        assert!(!short.cut);
        let two = label("Visual Studio Code", 84.0, six);
        assert_eq!(two.lines, ["Visual Studio", "Code"]);
        assert!(!two.cut);
        let long = label("Gestionnaire de mises à jour du système", 84.0, six);
        assert_eq!(long.lines.len(), 2);
        assert_eq!(long.lines[0], "Gestionnaire");
        assert!(long.lines[1].ends_with('…'), "{:?}", long.lines);
        assert!(six(&long.lines[1]) <= 84.0);
        assert!(long.cut);
        // A word too long alone goes over its lines.
        let word = label("Supercalifragilisticexpialidocious", 84.0, six);
        assert_eq!(word.lines[0], "Supercalifragi");
        assert!(word.cut);
    }

    #[test]
    fn the_grid_shows_whole_rows_and_scrolls_by_rows() {
        let tiles = |n: usize| Row::Tiles(vec![0; n]);
        let grid = Grid {
            rows: vec![
                Row::Head("A"),
                tiles(8),
                Row::Head("B"),
                tiles(8),
                tiles(8),
                tiles(8),
                tiles(3),
            ],
            ..Grid::default()
        };
        // The mockup's laptop: an area 480 px high.
        let size = Size::new(848.0, 708.0);
        assert_eq!(area(size).height, 480.0);
        // 24 + 112 + 24 + 112 + 112 = 384; one more row would be 496.
        assert_eq!(end_of(&grid, 0, 480.0), 5);
        // From the second heading, the rest fits: 24 + 4 × 112 = 472.
        assert_eq!(most_offset(&grid, 480.0), 2);
        assert_eq!(scrolled(&grid, 0, 1, size), 1);
        assert_eq!(scrolled(&grid, 1, 5, size), 2);
        assert_eq!(scrolled(&grid, 1, -4, size), 0);
        let layout = Layout::of(size, &grid, 0);
        assert_eq!(layout.heads.len(), 2);
        assert_eq!(layout.tiles.len(), 24);
        // Centred: 760 px, the eight tiles 12 px in.
        assert_eq!(layout.area.x, 44.0);
        assert_eq!(layout.tiles[0].1.x, 56.0);
        assert_eq!(layout.field, Rectangle::new(Point::new(194.0, 54.0), FIELD));
        assert!(layout.mark.is_some());
    }

    #[test]
    fn the_selection_stays_in_view() {
        let grid = Grid {
            rows: vec![
                Row::Head("A"),
                Row::Tiles((0..8).collect()),
                Row::Head("B"),
                Row::Tiles((8..16).collect()),
                Row::Tiles((16..24).collect()),
                Row::Tiles((24..32).collect()),
            ],
            flat: (0..32).map(super::super::Item::App).collect(),
            ..Grid::default()
        };
        let size = Size::new(848.0, 708.0);
        // Down into the fourth row of tiles: the grid scrolls one row.
        assert_eq!(keep_in_view(&grid, 25, 0, size), 1);
        // Back up to the first: its heading with it.
        assert_eq!(keep_in_view(&grid, 2, 2, size), 0);
        assert_eq!(keep_in_view(&grid, 9, 4, size), 2);
        assert_eq!(keep_in_view(&grid, 9, 3, size), 3);
    }

    #[test]
    fn the_field_shows_what_was_typed_last() {
        assert_eq!(tail("firefox", 100.0, six), "firefox");
        assert_eq!(tail("abcdefghij", 30.0, six), "…ghij");
    }
}
