//! The launcher's surface (5.1, variant C), on the widget kit (#772): its
//! field and its grid as the mockup draws them, × [`ZOOM`] (#518),
//! logical pixels.
//!
//! The panel is 1140 px wide (760 on the mockup), centred, its top 81 px
//! from the screen's: the field (690 × 81, centred), 33 px, then the grid's
//! area down to 147 px from the screen's bottom. The grid's rows are its
//! headings (36 px) and its rows of eight 138 × 159 tiles, 9 px apart; the
//! area shows the whole rows that fit from the first one scrolled to (the
//! wheel scrolls by rows), and a thin mark on its right when there are
//! more. A screen too narrow for eight columns has fewer, the tiles
//! keeping their size.
//!
//! The cards (5.2, variant A) are rows of their own, as wide as the
//! columns (1104 px for eight): a result's (102 px), an address's (81 px)
//! above the grid, and the web search's (75 px) last, 12 px under each.
//!
//! Each tile, card and emoji is a button of the kit, placed where the grid
//! lays it out; a tile's icon and name stay drawn. The field holds the
//! keyboard throughout: every key goes to the launcher ([`super::key_on`]).

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_a11y::Role;
use hocus_link::LAUNCHER_NAMESPACE;
use iced::{
    Color, Element, Length, Padding, Pixels, Point, Rectangle, Renderer, Size, Theme, alignment,
    alignment::Vertical,
    mouse,
    widget::{
        Space, Stack, button, canvas,
        canvas::{Frame, Geometry, Path, Stroke, gradient},
        column, container, mouse_area, pin, row,
        text::{Alignment, LineHeight, Shaping},
    },
    window,
};

use super::{Card, Grid, Open, Row, Words};
use crate::theme::Ink;
use crate::{
    field::{
        Entry, Took,
        view::{Look, Style},
    },
    levels::draw::{DIM, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb},
    popover::{
        Pressed,
        kit::{self, MANROPE_SEMIBOLD},
    },
    ui::{
        self, Around, control,
        field::Field,
        parts::{drawn, gap, spaced_by},
        sized::Sized,
        style::{self, said},
    },
};

type El<'a> = Element<'a, Msg>;

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

/// What it shows, in words, for the log (and the tests).
fn shows(open: &Open, words: Words, size: Size) -> String {
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
            let (main, sub) = card_lines(words, open, *card);
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

/// A card's two lines: what it is, and under it what it says more.
fn card_lines(words: Words, open: &Open, card: Card) -> (String, String) {
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

// --- The surface, on the widget kit ------------------------------------------------

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A tile, a card or an emoji clicked.
    Act(usize),
    /// The pointer came over one: it is selected.
    Hover(usize),
    /// The pointer left one.
    Unhover(usize),
    /// The wheel: the grid scrolls by rows.
    Wheel(mouse::ScrollDelta),
    /// A press off the field and the tiles: it closes.
    Outside,
    /// What the field took of an input method, or a press.
    Took(Took),
    /// A key: every one is the launcher's.
    Key(Pressed),
    /// What it shows changed under the view.
    Stale,
}

/// A place's control: what tests and screen readers know it by.
fn id_of(at: usize) -> String {
    format!("launcher:Item({at})")
}

/// The launcher on its surface `id`; on the CPU (`software`), an opaque
/// dark behind it.
pub fn view<'a>(id: window::Id, words: Words, software: bool) -> Element<'a, crate::Message> {
    let back: Element<'a, crate::Message> = if software {
        canvas(Opaque)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        Space::new().into()
    };
    Around::new(
        id,
        build(id, words),
        move |m| handle(id, words, m),
        Msg::Stale,
    )
    .own(LAUNCHER_NAMESPACE, move || {
        super::with(|l| l.open_on(id).is_some())
    })
    .rebuild(move || build(id, words))
    .stale_when(move || super::with(|l| l.open_on(id).is_some_and(|o| o.size != o.built)))
    .keys(|pressed| Some(Msg::Key(pressed.clone())))
    .on(back)
}

fn handle(id: window::Id, words: Words, message: Msg) {
    match message {
        Msg::Act(at) => super::act(id, at),
        Msg::Hover(at) => {
            super::with(|l| {
                if let Some(open) = l.open_mut(id) {
                    hovered(open, Some(at), Instant::now());
                }
            });
            // The effects' loop wakes the surface when the name is due.
            crate::popover::send(Vec::new());
        }
        Msg::Unhover(at) => super::with(|l| {
            if let Some(open) = l.open_mut(id)
                && open.hover.is_some_and(|(h, _)| h == at)
            {
                hovered(open, None, Instant::now());
            }
        }),
        Msg::Wheel(delta) => super::with(|l| {
            if let Some(open) = l.open_mut(id) {
                let notches = open.wheel.notches(&delta, open.scale);
                if notches != 0 {
                    open.offset = scrolled(&open.grid, open.offset, -notches, open.size);
                }
            }
        }),
        Msg::Outside => super::close(),
        Msg::Took(took) => super::took(id, took, words),
        Msg::Key(pressed) => {
            super::key_on(id, &pressed.key, pressed.modifiers, pressed.text.as_deref());
        }
        Msg::Stale => {}
    }
}

/// The pointer over `target`: it is selected, and its name shows whole a
/// moment later when it is cut.
fn hovered(open: &mut Open, target: Option<usize>, now: Instant) {
    if open.hover.map(|(t, _)| t) == target {
        return;
    }
    open.hover = target.map(|t| (t, now));
    open.tooltip = None;
    let Some(tile) = target else {
        return;
    };
    open.selected = tile;
    let cut = open
        .app(tile)
        .and_then(|a| label_of(&a.entry.name))
        .is_some_and(|l| l.cut);
    if cut {
        open.tooltip = Some(now + TOOLTIP_AFTER);
    }
}

/// The field, to the kit's field.
fn query(with: &mut dyn FnMut(&mut Entry)) {
    super::with(|l| {
        if let super::Phase::Open(open) = &mut l.phase {
            with(&mut open.query);
        }
    });
}

/// Its content as the launcher is now, laid out on its surface (once its
/// size is known: built again then); said in the log when it changed.
fn build<'a>(id: window::Id, words: Words) -> El<'a> {
    let content = super::with(|l| {
        let open = l.open_mut(id)?;
        let size = open.size;
        open.built = size;
        if size.width > 0.0 {
            open.fit(columns(size.width));
        }
        let layout = Layout::of(size, &open.grid, open.offset);
        let names: Vec<String> = layout
            .tiles
            .iter()
            .filter_map(|(at, _)| open.app(*at).map(|a| a.entry.name.clone()))
            .collect();
        measure_labels(names.iter().map(String::as_str));
        let now = shows(open, words, size);
        // Said once laid out on its surface.
        if size.width > 0.0 && now != open.logged {
            tracing::debug!(said = %now, "the launcher shows it");
            open.logged = now;
        }
        Some(content(open, words, &layout, size))
    });
    let content = content.unwrap_or_else(|| Space::new().into());
    Sized::new(content, move |room| {
        super::with(|l| {
            if let Some(open) = l.open_mut(id) {
                open.size = room;
            }
        });
    })
    .into()
}

fn content<'a>(open: &Open, words: Words, layout: &Layout, size: Size) -> El<'a> {
    let mut all: Vec<El<'a>> = vec![
        mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
            .on_press(Msg::Outside)
            .into(),
    ];
    let place =
        |content: El<'a>, rect: Rectangle| -> El<'a> { pin(content).x(rect.x).y(rect.y).into() };
    all.push(place(field_view(open, words, layout.field), layout.field));
    for (label, rect) in &layout.heads {
        all.push(place(
            ui::line(
                container(spaced_by(
                    label,
                    HEAD_TEXT,
                    MANROPE_BOLD,
                    PLACEHOLDER.get(),
                    HEAD_SPACING / HEAD_TEXT,
                ))
                .padding(Padding {
                    left: HEAD_LEFT,
                    ..Padding::ZERO
                })
                .width(rect.width)
                .height(HEAD)
                .align_y(Vertical::Center),
                *label,
            )
            .into(),
            *rect,
        ));
    }
    let px = super::icons::pixels(ICON as u32, open.scale);
    for (at, rect) in &layout.tiles {
        if let Some(tile) = tile_view(open, *at, *rect, px) {
            all.push(place(tile, *rect));
        }
    }
    for (at, card, rect) in &layout.cards {
        all.push(place(card_view(open, words, *card, *at, *rect), *rect));
    }
    if let Some((card, cells)) = &layout.emoji {
        all.push(place(emoji_view(open, words, *card, cells), *card));
    }
    if let Some(mark) = layout.mark {
        all.push(place(
            container(Space::new().width(mark.width).height(mark.height))
                .style(|_| style::boxed(MARK.get(), MARK_WIDTH / 2.0))
                .into(),
            mark,
        ));
    }
    // The full name of a tile whose name is cut, rested on.
    if let Some((tile, name)) = tooltip_of(open)
        && let Some(rect) = layout.rect_of(tile)
    {
        let width = kit::measure(name, TOOLTIP, MANROPE) + 2.0 * TOOLTIP_PAD.width;
        let height = TOOLTIP * LINE + 2.0 * TOOLTIP_PAD.height;
        let edge = TOOLTIP_UNDER;
        let x = (rect.center_x() - width / 2.0).clamp(edge, (size.width - width - edge).max(edge));
        let tip = Rectangle::new(
            Point::new(x, rect.y + rect.height + TOOLTIP_UNDER),
            Size::new(width, height),
        );
        all.push(place(
            container(said(name.to_owned(), TOOLTIP, MANROPE, TEXT.get()))
                .center_x(width)
                .center_y(height)
                .style(|_| container::Style {
                    background: Some(TOOLTIP_FILL.get().into()),
                    border: iced::Border {
                        color: TOOLTIP_RIM.get(),
                        width: 1.0,
                        radius: TOOLTIP_RADIUS.into(),
                    },
                    ..container::Style::default()
                })
                .into(),
            tip,
        ));
    }
    mouse_area(
        Stack::with_children(all)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_scroll(Msg::Wheel)
    .into()
}

/// The field at `rect`: the magnifier, what is typed, "Échap".
fn field_view<'a>(open: &Open, words: Words, rect: Rectangle) -> El<'a> {
    let hint = words.escape();
    let hint_width = kit::measure(hint, HINT, MANROPE_SEMIBOLD) + 2.0 * HINT_PAD + 2.0;
    let typed = Field {
        with: Box::new(query),
        style: Style::one(QUERY, MANROPE),
        placeholder: words.placeholder().into(),
        look: Look::of(TEXT.get(), PLACEHOLDER.get()),
        focused: true,
        left: FIELD_PAD_LEFT + MAGNIFIER + FIELD_GAP,
        right: FIELD_GAP + hint_width + FIELD_PAD_RIGHT,
        on_focus: Msg::Stale,
        on_took: Msg::Took,
    }
    .view(rect.height);
    let escape = container(said(hint, HINT, MANROPE_SEMIBOLD, PLACEHOLDER.get()))
        .center_x(hint_width)
        .center_y(HINT_HEIGHT)
        .style(|_| container::Style {
            border: iced::Border {
                color: HINT_RIM.get(),
                width: 1.0,
                radius: HINT_RADIUS.into(),
            },
            ..container::Style::default()
        });
    let field = container(Stack::with_children([
        pin(drawn(MAGNIFIER, MAGNIFIER, |f, _| {
            magnifier(f, Point::ORIGIN)
        }))
        .x(FIELD_PAD_LEFT)
        .y((rect.height - MAGNIFIER) / 2.0)
        .into(),
        typed,
        pin(escape)
            .x(rect.width - FIELD_PAD_RIGHT - hint_width)
            .y((rect.height - HINT_HEIGHT) / 2.0)
            .into(),
    ]))
    .width(rect.width)
    .height(rect.height)
    .style(|_| container::Style {
        background: Some(FIELD_FILL.get().into()),
        border: iced::Border {
            color: FIELD_RIM.get(),
            width: 1.0,
            radius: FIELD_RADIUS.into(),
        },
        ..container::Style::default()
    });
    control(
        field,
        "launcher:Field",
        Role::TextField,
        words.placeholder(),
    )
    .node(|n| n.value(open.query.text().to_owned()))
    .typing()
    .into()
}

/// A selected thing's look: the green tint and border.
fn chosen(fill: Ink, rim: Ink, radius: f32) -> button::Style {
    button::Style {
        background: Some(fill.get().into()),
        border: iced::Border {
            color: rim.get(),
            width: 1.0,
            radius: radius.into(),
        },
        ..style::clear()
    }
}

/// Tile `at` at `rect`: its icon (or its letter) and its name, drawn; the
/// pointer over it selects it, a click starts it.
fn tile_view<'a>(open: &Open, at: usize, rect: Rectangle, px: u32) -> Option<El<'a>> {
    let app = open.app(at)?;
    let name = app.entry.name.clone();
    let selected = at == open.selected;
    // On the screen's pixels, so that the icon's show one for one.
    let scale = open.scale;
    let snap = |v: f32| (v * scale).round() / scale;
    let icon = Rectangle::new(
        Point::new(
            snap(rect.center_x() - ICON / 2.0) - rect.x,
            snap(rect.y + ICON_TOP) - rect.y,
        ),
        Size::new(ICON, ICON),
    );
    let image = match &app.entry.icon {
        None => Some(None),
        Some(icon) => super::icons::of(icon, px),
    };
    let lines = label_of(&name).map_or_else(|| vec![name.clone()], |l| l.lines);
    let letter = name.clone();
    let face = drawn(rect.width, rect.height, move |f, size| {
        match &image {
            Some(Some(handle)) => f.draw_image(icon, canvas::Image::new(handle.clone())),
            Some(None) => letter_tile(f, icon, &letter),
            // Not read yet: it comes in a moment.
            None => {}
        }
        for (i, line) in lines.iter().enumerate() {
            f.fill_text(canvas::Text {
                content: line.clone(),
                position: Point::new(size.width / 2.0, NAME_TOP + NAME_LINE * (i as f32 + 0.5)),
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
    });
    let pressed = button(face)
        .padding(0)
        .style(move |_, _| {
            if selected {
                chosen(SELECTED_FILL, SELECTED_RIM, TILE_RADIUS)
            } else {
                style::clear()
            }
        })
        .on_press(Msg::Act(at));
    let tile = control(pressed, id_of(at), Role::Button, name).node(|n| n.selected(selected));
    Some(
        mouse_area(tile)
            .on_enter(Msg::Hover(at))
            .on_exit(Msg::Unhover(at))
            .into(),
    )
}

/// Card `card` (place `at`) at `rect`: its badge, its two lines, what
/// `Enter` does while it is selected.
fn card_view<'a>(open: &Open, words: Words, card: Card, at: usize, rect: Rectangle) -> El<'a> {
    let selected = at == open.selected;
    let size = card.badge();
    // The hint keeps its room unselected, as the mockup's.
    let hint = words.hint(card);
    let hint_width = kit::measure(hint, CARD_HINT, MANROPE_SEMIBOLD);
    let room =
        (rect.width - CARD_PAD - hint_width - CARD_GAP - (CARD_PAD + size + CARD_GAP)).max(0.0);
    let (main, sub) = card_lines(words, open, card);
    let (font, main_size) = match card {
        Card::Answer => (MONO, RESULT),
        Card::Address | Card::Web => (MANROPE_SEMIBOLD, CARD_MAIN),
    };
    let main = kit::elide(&main, room, |t| kit::measure(t, main_size, font));
    let sub = kit::elide(&sub, room, |t| kit::measure(t, CARD_SUB, MANROPE));
    let mut line = row![
        gap(CARD_PAD),
        drawn(size, size, move |f, s| badge_of(
            f,
            card,
            Rectangle::new(Point::ORIGIN, s)
        )),
        gap(CARD_GAP),
        column![
            said(main.clone(), main_size, font, TEXT.get()),
            Space::new().height(CARD_LINES_GAP),
            said(sub.clone(), CARD_SUB, MANROPE, CARD_SUB_INK.get()),
        ],
        iced::widget::space::horizontal(),
    ]
    .height(rect.height)
    .align_y(Vertical::Center);
    if selected {
        line = line.push(said(hint, CARD_HINT, MANROPE_SEMIBOLD, CARD_HINT_INK.get()));
    }
    line = line.push(gap(CARD_PAD));
    let pressed = button(line)
        .padding(0)
        .width(rect.width)
        .height(rect.height)
        .style(move |_, _| {
            if selected {
                chosen(SELECTED_FILL, SELECTED_RIM, CARD_RADIUS)
            } else {
                chosen(CARD_FILL, CARD_RIM, CARD_RADIUS)
            }
        })
        .on_press(Msg::Act(at));
    let control = control(pressed, id_of(at), Role::Button, main)
        .node(|n| n.describe(sub).selected(selected));
    mouse_area(control).on_enter(Msg::Hover(at)).into()
}

/// The emoji row's card (5.7) at `card`: green while one of its emoji is
/// selected.
fn emoji_view<'a>(
    open: &Open,
    words: Words,
    card: Rectangle,
    cells: &[(usize, Rectangle)],
) -> El<'a> {
    let lit = cells.iter().any(|(at, _)| *at == open.selected);
    let heading = crate::emoji::Words::new(words.lang()).launcher_row();
    let tone = crate::emoji::store::kept().tone;
    let table = crate::emoji::data::table();
    let mut row = row![].spacing(EMOJI_CELL_GAP);
    for (at, _) in cells {
        let Some(super::Item::Emoji(e)) = open.grid.flat.get(*at) else {
            continue;
        };
        let Some(emoji) = table.emoji.get(*e) else {
            continue;
        };
        let at = *at;
        let selected = at == open.selected;
        let glyph = iced::widget::text(emoji.with_tone(tone))
            .size(EMOJI_SIZE)
            .font(crate::emoji::view::EMOJI_FONT)
            .line_height(LineHeight::Relative(1.0))
            .shaping(Shaping::Advanced)
            .wrapping(iced::widget::text::Wrapping::None)
            .color(TEXT.get());
        let pressed = button(container(glyph).center(EMOJI_CELL))
            .padding(0)
            .width(EMOJI_CELL)
            .height(EMOJI_CELL)
            .style(move |_, _| {
                if selected {
                    chosen(SELECTED_FILL, SELECTED_RIM, EMOJI_CELL_RADIUS)
                } else {
                    style::filled(CARD_FILL.get(), EMOJI_CELL_RADIUS, TEXT.get())
                }
            })
            .on_press(Msg::Act(at));
        let cell = control(pressed, id_of(at), Role::Button, emoji.name(words.lang()))
            .node(|n| n.selected(selected));
        row = row.push(mouse_area(cell).on_enter(Msg::Hover(at)));
    }
    container(column![
        ui::line(
            container(spaced_by(
                &heading.to_uppercase(),
                EMOJI_HEAD,
                MANROPE_BOLD,
                CARD_HINT_INK.get(),
                0.08,
            ))
            .height(EMOJI_HEAD_LINE)
            .align_y(Vertical::Center),
            heading,
        ),
        Space::new().height(EMOJI_HEAD_GAP),
        row,
    ])
    .padding([EMOJI_PAD.height, EMOJI_PAD.width])
    .width(card.width)
    .height(card.height)
    .style(move |_| {
        let (fill, rim) = if lit {
            (EMOJI_CARD_FILL, SELECTED_RIM)
        } else {
            (CARD_FILL, CARD_RIM)
        };
        container::Style {
            background: Some(fill.get().into()),
            border: iced::Border {
                color: rim.get(),
                width: 1.0,
                radius: CARD_RADIUS.into(),
            },
            ..container::Style::default()
        }
    })
    .into()
}

/// On the CPU: the bar's dark blue, opaque, top to bottom.
#[derive(Debug, Clone, Copy)]
struct Opaque;

impl<M> canvas::Program<M> for Opaque {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let fill = gradient::Linear::new(Point::ORIGIN, Point::new(0.0, bounds.height))
            .add_stop(0.0, OPAQUE.0.get())
            .add_stop(1.0, OPAQUE.1.get());
        frame.fill(
            &Path::rectangle(Point::ORIGIN, bounds.size()),
            canvas::Fill::from(fill),
        );
        vec![frame.into_geometry()]
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
