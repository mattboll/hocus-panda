//! The launcher's surface (5.1, variant C): its field and its grid as the
//! mockup draws them, × [`ZOOM`] (#518), logical pixels, and what the
//! pointer, the wheel and the keys do on it.
//!
//! The panel is 1140 px wide (760 on the mockup), centred, its top 81 px
//! from the screen's: the field (690 × 81, centred), 33 px, then the grid's
//! area down to 147 px from the screen's bottom. The grid's rows are its
//! headings (36 px) and its rows of eight 138 × 159 tiles, 9 px apart; the
//! area shows the whole rows that fit from the first one scrolled to, and
//! a thin mark on its right when there are more. A screen too narrow for
//! eight columns has fewer, the tiles keeping their size.
//!
//! The cards (5.2, variant A) are rows of their own, as wide as the
//! columns (1104 px for eight): a result's (102 px), an address's (81 px)
//! above the grid, and the web search's (75 px) last, 12 px under each.

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
use crate::theme::Ink;
use crate::{
    field::{
        Place,
        view::{Look, Style},
    },
    levels::draw::{DIM, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb},
    popover::kit::{self, MANROPE_SEMIBOLD, Wheel},
};

// --- The mockup's sizes ---------------------------------------------------------
//
// Each is the 5.1 or the 5.2 mockup's, × [`ZOOM`] (#518): one place to
// make the launcher larger or smaller.

/// How many times the mockups' sizes the launcher is drawn.
pub const ZOOM: f32 = hocus_link::LAUNCHER_ZOOM as f32;

/// A mockup's size `px`, zoomed.
const fn z(px: f32) -> f32 {
    px * ZOOM
}

/// A mockup's size `px`, zoomed, in whole pixels (the names' text and
/// lines, as #518 sets them: 17 px on 22 px lines).
const fn whole(px: f32) -> f32 {
    (px * ZOOM) as i32 as f32
}

const TOP: f32 = hocus_link::LAUNCHER_TOP as f32;
const BOTTOM: f32 = hocus_link::LAUNCHER_BOTTOM as f32;
const FIELD: Size = Size::new(z(460.0), z(54.0));
const FIELD_RADIUS: f32 = z(12.0);
const FIELD_PAD_LEFT: f32 = z(14.0);
const FIELD_PAD_RIGHT: f32 = z(12.0);
const MAGNIFIER: f32 = z(18.0);
const FIELD_GAP: f32 = z(10.0);
const QUERY: f32 = z(19.0);
const HINT: f32 = z(11.0);
const HINT_PAD: f32 = z(6.0);
const HINT_HEIGHT: f32 = z(21.0);
const HINT_RADIUS: f32 = z(6.0);
/// The gap between the field and the grid.
const GAP: f32 = z(22.0);
/// A heading's row, its letters 11 px 700 spaced by 0.08 em, 4 px in.
const HEAD: f32 = z(24.0);
const HEAD_TEXT: f32 = z(11.0);
const HEAD_SPACING: f32 = 0.08 * HEAD_TEXT;
const HEAD_LEFT: f32 = z(4.0);
/// A tile, and a row of them with the gap under it.
pub const TILE: Size = Size::new(z(92.0), z(106.0));
const TILE_ROW: f32 = z(112.0);
const TILE_RADIUS: f32 = z(12.0);
/// The panel's room on each side of its eight columns: 760 px in all.
const PANEL_PAD: f32 = z(12.0);
/// Its icon from its top, then its name 7 px under it, 4 px in from its
/// sides, two lines of 15 px at most.
const ICON_TOP: f32 = z(9.0);
pub const ICON: f32 = z(48.0);
const ICON_RADIUS: f32 = z(12.0);
const LETTER: f32 = z(19.0);
const NAME_TOP: f32 = ICON_TOP + ICON + z(7.0);
const NAME: f32 = whole(11.5);
const NAME_LINE: f32 = whole(15.0);
const NAME_ROOM: f32 = TILE.width - 2.0 * z(4.0);
/// The scroll mark, 14 px past the area's right.
const MARK_OUT: f32 = z(14.0);
const MARK_WIDTH: f32 = z(3.0);
/// The full name over a tile whose name is cut, after resting on it: 12 px
/// in a frame 8 px in across, 4 px under the tile.
const TOOLTIP_AFTER: Duration = Duration::from_millis(500);
const TOOLTIP: f32 = z(12.0);
const TOOLTIP_PAD: Size = Size::new(z(8.0), z(4.0));
const TOOLTIP_UNDER: f32 = z(4.0);
const TOOLTIP_RADIUS: f32 = z(6.0);
/// A card: as wide as eight tiles (736 px), radius 14, 8 px under it; 16 px
/// in, its badge, 14 px, its two lines (the first 26 px Martian Mono for a
/// result, 15 px 600 otherwise; then 12 px, 2 px under), 14 px, and what
/// `Enter` does (11.5 px 600) while it is selected.
const CARD_UNDER: f32 = z(8.0);
const CARD_RADIUS: f32 = z(14.0);
const CARD_PAD: f32 = z(16.0);
const CARD_GAP: f32 = z(14.0);
const ANSWER_CARD: f32 = z(68.0);
const ADDRESS_CARD: f32 = z(54.0);
const WEB_CARD: f32 = z(50.0);
const ANSWER_BADGE: f32 = z(40.0);
const BADGE: f32 = z(34.0);
const BADGE_ICON: f32 = z(20.0);
const EQUALS: f32 = z(20.0);
const RESULT: f32 = z(26.0);
const CARD_MAIN: f32 = z(15.0);
const CARD_SUB: f32 = z(12.0);
const CARD_LINES_GAP: f32 = z(2.0);
const CARD_HINT: f32 = z(11.5);
const MONO: iced::Font = iced::Font::with_name("Martian Mono");
/// The emoji row of a `:` query (5.7): a card 12 px by 16 px in, its
/// heading (11 px 700 capitals spaced 0.08 em, `#8FE0A8`), 8 px, its
/// emoji in 44 px cells 6 px apart (radius 10, the emoji 26 px), the
/// selected one in the green border and tint, the others on
/// `rgba(255,255,255,0.04)`.
const EMOJI_PAD: Size = Size::new(z(16.0), z(12.0));
const EMOJI_HEAD: f32 = z(11.0);
const EMOJI_HEAD_LINE: f32 = z(15.0);
const EMOJI_HEAD_GAP: f32 = z(8.0);
const EMOJI_CELL: f32 = z(44.0);
const EMOJI_CELL_GAP: f32 = z(6.0);
const EMOJI_CELL_RADIUS: f32 = z(10.0);
const EMOJI_SIZE: f32 = z(26.0);
const EMOJI_CARD: f32 = EMOJI_PAD.height * 2.0 + EMOJI_HEAD_LINE + EMOJI_HEAD_GAP + EMOJI_CELL;
const EMOJI_CARD_FILL: Ink = Ink::Accent(0.10);

const FIELD_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.05));
const FIELD_RIM: Ink = Ink::Accent(0.45);
const PLACEHOLDER: Ink = Ink::Dark(rgb(0x7C8698));
const HINT_RIM: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.14));
const LABEL: Ink = Ink::Dark(rgb(0xC9D0DC));
const SELECTED_FILL: Ink = Ink::Accent(0.16);
const SELECTED_RIM: Ink = Ink::Accent(0.6);
const MARK: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.22));
/// The neutral tile of an application without an icon.
const NEUTRAL: (Color, Color) = (rgb(0x5C6576), rgb(0x9AA3B4));
const NEUTRAL_RIM: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.14));
const TOOLTIP_FILL: Ink = Ink::Dark(Color::from_rgba8(22, 27, 42, 0.96));
const TOOLTIP_RIM: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.12));
/// On the CPU, whose surfaces have no transparency: the bar's dark blue
/// (bar.md §13), opaque, top to bottom, the compositor's tint without the
/// desktop under it.
const OPAQUE: (Ink, Ink) = (Ink::Dark(rgb(0x161B2A)), Ink::Dark(rgb(0x0E111C)));
const CARD_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.04));
const CARD_RIM: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
const CARD_SUB_INK: Ink = Ink::Dark(rgb(0x8C96A8));
const CARD_HINT_INK: Ink = Ink::Dark(rgb(0x8FE0A8));
const BADGE_RIM: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.14));
const ANSWER_FILL: Ink = Ink::Accent(1.0);
const EQUALS_INK: Ink = Ink::Dark(rgb(0x0B1020));
const ADDRESS_FILL: (Color, Color) = (rgb(0x2E7D6B), rgb(0x5CC0A6));
const WEB_FILL: (Color, Color) = (rgb(0x3B5BA9), rgb(0x6E8FE0));

impl Card {
    /// Its row's height: the card, and the gap under it.
    fn row(self) -> f32 {
        let card = match self {
            Self::Answer => ANSWER_CARD,
            Self::Address => ADDRESS_CARD,
            Self::Web => WEB_CARD,
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

/// The panel's width with `columns` columns of tiles.
fn panel(columns: usize) -> f32 {
    columns as f32 * TILE.width + 2.0 * PANEL_PAD
}

/// The columns of tiles on a surface `width` wide: eight, or as many as fit
/// with the panel's sides and the scroll mark on either side; fewer
/// columns, never smaller tiles.
pub fn columns(width: f32) -> usize {
    (1..=super::COLUMNS)
        .rev()
        .find(|n| panel(*n) + 2.0 * MARK_OUT <= width)
        .unwrap_or(1)
}

/// The field on a surface `size` large, over `columns` columns: no wider
/// than the panel.
fn field(size: Size, columns: usize) -> Rectangle {
    let width = FIELD.width.min(panel(columns));
    Rectangle::new(
        Point::new(((size.width - width) / 2.0).round(), TOP),
        Size::new(width, FIELD.height),
    )
}

/// The grid's area on a surface `size` large, `columns` wide.
fn area(size: Size, columns: usize) -> Rectangle {
    let top = TOP + FIELD.height + GAP;
    let height = (size.height - BOTTOM - top).max(HEAD + TILE_ROW);
    let width = panel(columns);
    Rectangle::new(
        Point::new(((size.width - width) / 2.0).round(), top),
        Size::new(width, height),
    )
}

fn height_of(row: &Row) -> f32 {
    match row {
        Row::Head(_) => HEAD,
        Row::Tiles(_) => TILE_ROW,
        Row::Card(card, _) => card.row(),
        Row::Emoji(_) => EMOJI_CARD + CARD_UNDER,
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
    let height = area(size, grid.columns).height;
    let mut offset = offset;
    while end_of(grid, offset, height) <= row && offset < row {
        offset += 1;
    }
    offset
}

/// The grid scrolled `by` rows from `offset` (down positive), within its
/// ends.
pub fn scrolled(grid: &Grid, offset: usize, by: i32, size: Size) -> usize {
    let most = most_offset(grid, area(size, grid.columns).height);
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
    /// The emoji row's card, and its emoji: their places, and where.
    pub emoji: Option<(Rectangle, Vec<(usize, Rectangle)>)>,
    /// The scroll mark, when the grid overflows.
    pub mark: Option<Rectangle>,
}

impl Layout {
    pub fn of(size: Size, grid: &Grid, offset: usize) -> Self {
        let area = area(size, grid.columns);
        let end = end_of(grid, offset, area.height);
        let mut heads = Vec::new();
        let mut tiles = Vec::new();
        let mut cards = Vec::new();
        let mut emoji = None;
        let left = area.x + PANEL_PAD;
        // A card is as wide as the tiles' columns.
        let card_width = grid.columns as f32 * TILE.width;
        let mut y = area.y;
        for row in grid.rows.get(offset..end).unwrap_or_default() {
            match row {
                Row::Head(label) => heads.push((
                    *label,
                    Rectangle::new(Point::new(area.x, y), Size::new(area.width, HEAD)),
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
                        Point::new(left, y),
                        Size::new(card_width, card.row() - CARD_UNDER),
                    ),
                )),
                Row::Emoji(places) => {
                    let card =
                        Rectangle::new(Point::new(left, y), Size::new(card_width, EMOJI_CARD));
                    let top = y + EMOJI_PAD.height + EMOJI_HEAD_LINE + EMOJI_HEAD_GAP;
                    let cells = places
                        .iter()
                        .enumerate()
                        .map(|(i, at)| {
                            let x =
                                left + EMOJI_PAD.width + i as f32 * (EMOJI_CELL + EMOJI_CELL_GAP);
                            (
                                *at,
                                Rectangle::new(
                                    Point::new(x, top),
                                    Size::new(EMOJI_CELL, EMOJI_CELL),
                                ),
                            )
                        })
                        .collect();
                    emoji = Some((card, cells));
                }
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
            field: field(size, grid.columns),
            area,
            heads,
            tiles,
            cards,
            emoji,
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
            .or_else(|| {
                let (_, cells) = self.emoji.as_ref()?;
                cells
                    .iter()
                    .find(|(_, rect)| rect.contains(at))
                    .map(|(i, _)| *i)
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

/// What the surface keeps between draws: the hint's width and the
/// wheel's notches.
#[derive(Debug, Default)]
pub struct State {
    wheel: Wheel,
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
            iced::Event::InputMethod(event) => super::input_method(self.id, event)
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
            let fill = gradient::Linear::new(Point::ORIGIN, Point::new(0.0, bounds.height))
                .add_stop(0.0, OPAQUE.0.get())
                .add_stop(1.0, OPAQUE.1.get());
            frame.fill(
                &Path::rectangle(Point::ORIGIN, bounds.size()),
                canvas::Fill::from(fill),
            );
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
    /// Before it draws: its size kept, its columns fitted to it, the names
    /// shown measured, and what it shows logged when it changed.
    fn prepare(&self, state: &mut State, size: Size) {
        let shown = super::with(|l| {
            let open = l.open_mut(self.id)?;
            open.size = size;
            open.fit(columns(size.width));
            let layout = Layout::of(size, &open.grid, open.offset);
            let names: Vec<String> = layout
                .tiles
                .iter()
                .filter_map(|(at, _)| open.app(*at).map(|a| a.entry.name.clone()))
                .collect();
            Some(names)
        });
        let Some(names) = shown else {
            return;
        };
        measure_labels(names.iter().map(String::as_str));
        if state.hint == 0.0 {
            state.hint = kit::measure(self.words.escape(), HINT, MANROPE_SEMIBOLD);
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
        let tone = crate::emoji::store::kept().tone;
        let table = crate::emoji::data::table();
        let emoji: Vec<String> = layout
            .emoji
            .iter()
            .flat_map(|(_, cells)| cells)
            .filter_map(|(at, _)| {
                let super::Item::Emoji(e) = open.grid.flat.get(*at)? else {
                    return None;
                };
                let mark = if *at == open.selected { "*" } else { "" };
                Some(format!("{mark}{}", table.emoji.get(*e)?.with_tone(tone)))
            })
            .collect();
        format!(
            "query={:?} heads={rows:?} tiles={lines:?} cards={cards:?} offset={} mark={} \
             tooltip=\"{tooltip}\" emoji={emoji:?}",
            open.query.text(),
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
                    // A press held in the field selects as it drags.
                    if open.query.pressed() {
                        return Some(match at {
                            Some(at) if open.query.drag(at) => Then::Draw,
                            _ => Then::Nothing,
                        });
                    }
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
                    None => match at.filter(|at| layout.field.contains(*at)) {
                        Some(at) => {
                            open.query.press(at);
                            Then::Draw
                        }
                        None => Then::Close,
                    },
                },
                mouse::Event::ButtonReleased(mouse::Button::Left) => {
                    open.query.release();
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
        self.draw_field(f, state, open, &layout);
        for (label, rect) in &layout.heads {
            head(f, label, *rect);
        }
        let px = super::icons::pixels(ICON as u32, open.scale);
        for (at, rect) in &layout.tiles {
            let Some(app) = open.app(*at) else {
                continue;
            };
            if *at == open.selected {
                f.fill(&kit::rounded(*rect, TILE_RADIUS), SELECTED_FILL.get());
                f.stroke(
                    &kit::rounded(inset(*rect, 0.5), TILE_RADIUS - 0.5),
                    Stroke::default()
                        .with_color(SELECTED_RIM.get())
                        .with_width(1.0),
                );
            }
            // On the screen's pixels, so that the icon's show one for one.
            let snap = |v: f32| (v * open.scale).round() / open.scale;
            let icon = Rectangle::new(
                Point::new(snap(rect.center_x() - ICON / 2.0), snap(rect.y + ICON_TOP)),
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
                    color: LABEL.get(),
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
        if let Some((card, cells)) = &layout.emoji {
            self.draw_emoji(f, open, *card, cells);
        }
        if let Some(mark) = layout.mark {
            f.fill(&kit::rounded(mark, MARK_WIDTH / 2.0), MARK.get());
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
                words.search_web(open.query.text().trim()),
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
        f.fill(&kit::rounded(rect, CARD_RADIUS), fill.get());
        f.stroke(
            &kit::rounded(inset(rect, 0.5), CARD_RADIUS - 0.5),
            Stroke::default().with_color(rim.get()).with_width(1.0),
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
                    CARD_HINT_INK.get(),
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
                TEXT.get(),
                Alignment::Left,
            ),
        ));
        f.fill_text(kit::text(
            sub,
            Point::new(x, top + main_height + CARD_LINES_GAP + sub_height / 2.0),
            CARD_SUB,
            CARD_SUB_INK.get(),
            Alignment::Left,
        ));
    }

    /// The emoji row's card (5.7): green while one of its emoji is
    /// selected.
    fn draw_emoji(
        &self,
        f: &mut Frame,
        open: &Open,
        card: Rectangle,
        cells: &[(usize, Rectangle)],
    ) {
        let chosen = cells.iter().any(|(at, _)| *at == open.selected);
        let (fill, rim) = if chosen {
            (EMOJI_CARD_FILL, SELECTED_RIM)
        } else {
            (CARD_FILL, CARD_RIM)
        };
        f.fill(&kit::rounded(card, CARD_RADIUS), fill.get());
        f.stroke(
            &kit::rounded(inset(card, 0.5), CARD_RADIUS - 0.5),
            Stroke::default().with_color(rim.get()).with_width(1.0),
        );
        let heading = crate::emoji::Words::new(self.words.lang()).launcher_row();
        let mut x = card.x + EMOJI_PAD.width;
        let middle = card.y + EMOJI_PAD.height + EMOJI_HEAD_LINE / 2.0;
        for ch in heading.to_uppercase().chars() {
            let letter = ch.to_string();
            let width = kit::measure(&letter, EMOJI_HEAD, MANROPE_BOLD);
            f.fill_text(kit::text_in(
                MANROPE_BOLD,
                kit::text(
                    letter,
                    Point::new(x, middle),
                    EMOJI_HEAD,
                    CARD_HINT_INK.get(),
                    Alignment::Left,
                ),
            ));
            x += width + 0.08 * EMOJI_HEAD;
        }
        let tone = crate::emoji::store::kept().tone;
        let table = crate::emoji::data::table();
        for (at, rect) in cells {
            let Some(super::Item::Emoji(e)) = open.grid.flat.get(*at) else {
                continue;
            };
            let Some(emoji) = table.emoji.get(*e) else {
                continue;
            };
            if *at == open.selected {
                f.fill(&kit::rounded(*rect, EMOJI_CELL_RADIUS), SELECTED_FILL.get());
                f.stroke(
                    &kit::rounded(inset(*rect, 0.5), EMOJI_CELL_RADIUS - 0.5),
                    Stroke::default()
                        .with_color(SELECTED_RIM.get())
                        .with_width(1.0),
                );
            } else {
                f.fill(&kit::rounded(*rect, EMOJI_CELL_RADIUS), CARD_FILL.get());
            }
            crate::emoji::view::emoji(f, &emoji.with_tone(tone), rect.center(), EMOJI_SIZE, 1.0);
        }
    }

    fn draw_field(&self, f: &mut Frame, state: &State, open: &Open, layout: &Layout) {
        let field = layout.field;
        f.fill(&kit::rounded(field, FIELD_RADIUS), FIELD_FILL.get());
        f.stroke(
            &kit::rounded(inset(field, 0.5), FIELD_RADIUS - 0.5),
            Stroke::default()
                .with_color(FIELD_RIM.get())
                .with_width(1.0),
        );
        magnifier(
            f,
            Point::new(field.x + FIELD_PAD_LEFT, field.center_y() - MAGNIFIER / 2.0),
        );
        // What is typed, its selection and its caret: the field has the
        // keyboard.
        open.query.draw(
            f,
            Place {
                left: field.x + FIELD_PAD_LEFT + MAGNIFIER + FIELD_GAP,
                middle: field.center_y(),
                width: text_room(field, state.hint),
                style: Style::one(QUERY, MANROPE),
            },
            self.words.placeholder(),
            &Look::of(TEXT.get(), PLACEHOLDER.get()),
            true,
        );
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
            Stroke::default().with_color(HINT_RIM.get()).with_width(1.0),
        );
        f.fill_text(kit::text_in(
            MANROPE_SEMIBOLD,
            kit::text(
                self.words.escape().to_owned(),
                hint.center(),
                HINT,
                PLACEHOLDER.get(),
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

fn inset(rect: Rectangle, by: f32) -> Rectangle {
    Rectangle::new(
        Point::new(rect.x + by, rect.y + by),
        Size::new(rect.width - 2.0 * by, rect.height - 2.0 * by),
    )
}

/// The magnifier, [`MAGNIFIER`] wide from `at`: the mockup's 24 px drawing.
fn magnifier(f: &mut Frame, at: Point) {
    let k = MAGNIFIER / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * k, at.y + y * k);
    let stroke = Stroke {
        line_cap: canvas::LineCap::Round,
        ..Stroke::default().with_color(DIM.get()).with_width(2.2 * k)
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
        Card::Answer => f.fill(&shape, ANSWER_FILL.get()),
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
        Stroke::default()
            .with_color(BADGE_RIM.get())
            .with_width(1.0),
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
                EQUALS_INK.get(),
                Alignment::Center,
            ),
        )),
        Card::Address => link(f, at),
        Card::Web => globe(f, at),
    }
}

/// The mockup's 24 px globe, [`BADGE_ICON`] wide from `at`.
fn globe(f: &mut Frame, at: Point) {
    let k = BADGE_ICON / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * k, at.y + y * k);
    let stroke = |width: f32| {
        Stroke::default()
            .with_color(TEXT.get())
            .with_width(width * k)
    };
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

/// The mockup's 24 px chain link, [`BADGE_ICON`] wide from `at`: a bar between two
/// hooks, each a near half circle of radius 3.2.
fn link(f: &mut Frame, at: Point) {
    use std::f32::consts::PI;
    let k = BADGE_ICON / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * k, at.y + y * k);
    let stroke = Stroke {
        line_cap: canvas::LineCap::Round,
        ..Stroke::default().with_color(TEXT.get()).with_width(1.8 * k)
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

/// A heading: its letters spaced, [`HEAD_LEFT`] in, on its row's middle.
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
                PLACEHOLDER.get(),
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
        Stroke::default()
            .with_color(NEUTRAL_RIM.get())
            .with_width(1.0),
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
    let width = kit::measure(name, TOOLTIP, MANROPE) + 2.0 * TOOLTIP_PAD.width;
    let height = TOOLTIP * LINE + 2.0 * TOOLTIP_PAD.height;
    let edge = TOOLTIP_UNDER;
    let x = (tile.center_x() - width / 2.0).clamp(edge, (size.width - width - edge).max(edge));
    let rect = Rectangle::new(
        Point::new(x, tile.y + tile.height + TOOLTIP_UNDER),
        Size::new(width, height),
    );
    f.fill(&kit::rounded(rect, TOOLTIP_RADIUS), TOOLTIP_FILL.get());
    f.stroke(
        &kit::rounded(inset(rect, 0.5), TOOLTIP_RADIUS - 0.5),
        Stroke::default()
            .with_color(TOOLTIP_RIM.get())
            .with_width(1.0),
    );
    f.fill_text(kit::text(
        name.to_owned(),
        rect.center(),
        TOOLTIP,
        TEXT.get(),
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
        // The mockup's laptop, × 1.5: an area 720 px high.
        let size = Size::new(1272.0, 1062.0);
        assert_eq!(area(size, 8).height, 720.0);
        // 36 + 168 + 36 + 168 + 168 = 576; one more row would be 744.
        assert_eq!(end_of(&grid, 0, 720.0), 5);
        // From the second heading, the rest fits: 36 + 4 × 168 = 708.
        assert_eq!(most_offset(&grid, 720.0), 2);
        assert_eq!(scrolled(&grid, 0, 1, size), 1);
        assert_eq!(scrolled(&grid, 1, 5, size), 2);
        assert_eq!(scrolled(&grid, 1, -4, size), 0);
        let layout = Layout::of(size, &grid, 0);
        assert_eq!(layout.heads.len(), 2);
        assert_eq!(layout.tiles.len(), 24);
        // Centred: 1140 px, the eight 138 × 159 tiles 18 px in.
        assert_eq!(layout.area.x, 66.0);
        assert_eq!(layout.area.width, 1140.0);
        assert_eq!(layout.tiles[0].1.x, 84.0);
        assert_eq!(layout.tiles[0].1.size(), Size::new(138.0, 159.0));
        assert_eq!(layout.tiles[1].1.x, 84.0 + 138.0);
        assert_eq!(
            layout.field,
            Rectangle::new(Point::new(291.0, 81.0), Size::new(690.0, 81.0))
        );
        assert!(layout.mark.is_some());
    }

    #[test]
    fn a_narrow_screen_has_fewer_columns_never_smaller_tiles() {
        // 1366 × 768 at scale 1: the eight columns fit, 113 px on each
        // side.
        assert_eq!(columns(1366.0), 8);
        assert_eq!(columns(2560.0), 8);
        // At 1.25, it is 1093 logical pixels wide: seven.
        assert_eq!(columns(1366.0 / 1.25), 7);
        assert_eq!(columns(1024.0), 6);
        assert_eq!(columns(800.0), 5);
        assert_eq!(columns(100.0), 1);
        // Seven columns: the tiles at their size, the panel and the cards
        // as wide as they are, the field narrower than none of them (on a
        // screen tall enough for the card under the tiles).
        let size = Size::new(1366.0 / 1.25, 768.0);
        let grid = Grid {
            rows: vec![
                Row::Head("A"),
                Row::Tiles((0..7).collect()),
                Row::Card(Card::Web, 7),
            ],
            flat: (0..7)
                .map(super::super::Item::App)
                .chain([super::super::Item::Card(Card::Web)])
                .collect(),
            columns: columns(size.width),
            ..Grid::default()
        };
        let layout = Layout::of(size, &grid, 0);
        assert_eq!(layout.tiles.len(), 7);
        assert!(layout.tiles.iter().all(|(_, t)| t.size() == TILE));
        let (_, last) = layout.tiles[6];
        assert!(last.x + last.width + MARK_OUT <= size.width, "{last:?}");
        assert!(layout.area.x >= MARK_OUT, "{:?}", layout.area);
        assert_eq!(layout.area.width, 7.0 * 138.0 + 36.0);
        assert_eq!(layout.cards[0].2.width, 7.0 * 138.0);
        assert_eq!(layout.cards[0].2.x, layout.tiles[0].1.x);
        assert_eq!(layout.field.width, 690.0);
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
        let size = Size::new(1272.0, 1062.0);
        // Down into the fourth row of tiles: the grid scrolls one row.
        assert_eq!(keep_in_view(&grid, 25, 0, size), 1);
        // Back up to the first: its heading with it.
        assert_eq!(keep_in_view(&grid, 2, 2, size), 0);
        assert_eq!(keep_in_view(&grid, 9, 4, size), 2);
        assert_eq!(keep_in_view(&grid, 9, 3, size), 3);
    }
}
