//! The emoji picker's surface (5.7), on the widget kit (#772): the
//! mockup's panel × [`ZOOM`] (the launcher's, 5.1b), logical pixels.
//!
//! In the mockup's sizes: 336 wide, 12 inside, its parts 8 apart, radius
//! 14. The top line (36): the search (its green border, radius 10) and the
//! 32 px round button of the skin tone; under it when it is open, the six
//! tones (22 px rounds, 6 apart, on the right); the tabs (30, one per
//! group, 2 apart, radius 8, their emoji 16 px, the lit one on
//! `rgba(255,255,255,0.10)`); the grid (252): headings 22 tall (10.5 px
//! 700 capitals spaced 0.08 em, `#7C8698`, 4 px in) and rows of eight
//! 38 px cells (radius 9, their emoji 22 px), scrolling a row a notch;
//! the footer under a line, 30 tall 6 under it: the selected emoji
//! (20 px), its name (12.5 px, `#C9D0DC`) and "Entrée insère" (10.5 px,
//! `#6A7384`) on the right.
//!
//! The search holds the keyboard while the picker does: every key goes to
//! the picker ([`super::key_on`]). The pointer over an emoji selects it, a
//! click inserts it (with `Shift`, the picker stays). Only the grid's rows
//! near its view are built ([`parts::lazy`]).

use hocus_a11y::Role;
use iced::{
    Color, Element, Font, Length, Padding, Point, Rectangle, Renderer, Size, Theme, alignment,
    alignment::{Horizontal, Vertical},
    mouse,
    widget::{
        Space, Stack, button,
        canvas::{self, Frame, Path, Stroke},
        column, container, mouse_area, pin, row, scrollable,
        text::{Alignment, LineHeight, Shaping, Wrapping},
    },
    window,
};

use super::{Grid, Heading, Open, Phase, Row, Words};
use crate::theme::Ink;
use crate::{
    field::{
        Entry, Took,
        view::{Look, Style},
    },
    levels::draw::{DIM, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb},
    popover::{Pressed, kit},
    ui::{
        self, Around, Later, control,
        field::Field,
        notched::Notched,
        parts::{self, drawn, gap, spaced_by},
        style::{self, said},
    },
};

type El<'a> = Element<'a, Msg>;

/// How many times the mockup's sizes the picker is drawn.
pub const ZOOM: f32 = hocus_link::EMOJI_ZOOM as f32;

const fn z(px: f32) -> f32 {
    px * ZOOM
}

/// The colour emoji font, the system's.
pub const EMOJI_FONT: Font = Font::with_name("Noto Color Emoji");

const WIDTH: f32 = z(336.0);
const BORDER: f32 = 1.0;
const RADIUS: f32 = hocus_link::EMOJI_RADIUS as f32;
const PAD: f32 = z(12.0);
const LEFT: f32 = BORDER + PAD;
const RIGHT: f32 = WIDTH - BORDER - PAD;
const GAP: f32 = z(8.0);
const TOP: f32 = BORDER + PAD;
/// The top line: the search, 8 px, the tone's button.
const TOP_LINE: f32 = z(36.0);
const FIELD_RADIUS: f32 = z(10.0);
const FIELD_PAD: f32 = z(10.0);
const MAGNIFIER: f32 = z(15.0);
const FIELD_GAP: f32 = z(8.0);
const QUERY: f32 = z(13.5);
const TONE_BUTTON: f32 = z(32.0);
const TONE_DOT: f32 = z(16.0);
/// The tones' row.
const TONES: f32 = z(22.0);
const TONE_GAP: f32 = z(6.0);
const TONE_RING: f32 = z(2.0);
/// The tabs.
const TABS: f32 = z(30.0);
const TAB_GAP: f32 = z(2.0);
const TAB_RADIUS: f32 = z(8.0);
const TAB_EMOJI: f32 = z(16.0);
/// The grid.
const GRID: f32 = z(252.0);
const HEAD: f32 = z(22.0);
const HEAD_TEXT: f32 = z(10.5);
const HEAD_SPACING: f32 = 0.08 * HEAD_TEXT;
const HEAD_LEFT: f32 = z(4.0);
pub const CELL: f32 = z(38.0);
const CELL_RADIUS: f32 = z(9.0);
const CELL_EMOJI: f32 = z(22.0);
/// Nothing found: 30 px down, 13 px.
const EMPTY_TOP: f32 = z(30.0);
const EMPTY: f32 = z(13.0);
/// The footer: a line, 6 px, its row.
const FOOT_PAD: f32 = z(6.0);
const FOOT: f32 = z(30.0);
const FOOT_EMOJI: f32 = z(20.0);
const FOOT_NAME: f32 = z(12.5);
const FOOT_HINT: f32 = z(10.5);

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, a)
}

/// The mockup's tint, opaque without glass.
const GLASS_TOP: Ink = Ink::Dark(rgba(22, 27, 42, 0.97));
const GLASS_BOTTOM: Ink = Ink::Dark(rgba(14, 17, 28, 0.98));
const OPAQUE_TOP: Ink = Ink::Dark(rgb(0x161B2A));
const OPAQUE_BOTTOM: Ink = Ink::Dark(rgb(0x0E111C));
const RIM: Ink = Ink::Rim(rgba(255, 255, 255, 0.10));
const FIELD_FILL: Ink = Ink::Dark(rgba(255, 255, 255, 0.05));
const FIELD_RIM: Ink = Ink::Accent(0.45);
const QUIET: Ink = Ink::Dark(rgb(0x7C8698));
const BUTTON_RIM: Ink = Ink::Dark(rgba(255, 255, 255, 0.14));
const TAB_LIT: Ink = Ink::Dark(rgba(255, 255, 255, 0.10));
const HOVER: Ink = Ink::Dark(rgba(255, 255, 255, 0.08));
const SELECTED_FILL: Ink = Ink::Accent(0.16);
const SELECTED_RIM: Ink = Ink::Accent(0.6);
const GREEN: Ink = Ink::Accent(1.0);
const FAINT: Ink = Ink::Dark(rgb(0x8C96A8));
const NAME: Ink = Ink::Dark(rgb(0xC9D0DC));
const HINT: Ink = Ink::Dark(rgb(0x6A7384));
const RULE: Ink = Ink::Dark(rgba(255, 255, 255, 0.08));

/// The tones' colours, the mockup's: yellow, then light to dark.
pub const TONE_COLOURS: [Color; 6] = [
    rgb(0xF5C84B),
    rgb(0xF7DECE),
    rgb(0xE3C29F),
    rgb(0xC68E6A),
    rgb(0xA16A4A),
    rgb(0x5C3A2B),
];

/// Where the grid starts, with the tones' row open or not.
fn grid_top(tones_open: bool) -> f32 {
    let mut y = TOP + TOP_LINE + GAP;
    if tones_open {
        y += TONES + GAP;
    }
    y + TABS + GAP
}

/// The surface's size, logical pixels.
pub fn size(tones_open: bool) -> (u32, u32) {
    let end = grid_top(tones_open) + GRID + GAP + 1.0 + FOOT_PAD + FOOT;
    (WIDTH.round() as u32, (end + PAD + BORDER).ceil() as u32)
}

fn height_of(row: &Row) -> f32 {
    match row {
        Row::Head(_) => HEAD,
        Row::Cells(_) => CELL,
    }
}

/// Each row's top in the grid.
fn tops(grid: &Grid) -> Vec<f32> {
    let mut top = 0.0;
    grid.rows
        .iter()
        .map(|row| {
            let at = top;
            top += height_of(row);
            at
        })
        .collect()
}

/// How far the grid scrolls at most.
fn most_scroll(grid: &Grid) -> f32 {
    (grid.rows.iter().map(height_of).sum::<f32>() - GRID).max(0.0)
}

/// The first row wholly shown, the grid scrolled to `scroll`.
fn offset_at(grid: &Grid, scroll: f32) -> usize {
    tops(grid)
        .iter()
        .position(|top| *top >= scroll - 0.5)
        .unwrap_or(0)
}

/// The row after the last one wholly shown from `offset`.
fn end_of(grid: &Grid, offset: usize) -> usize {
    let mut used = 0.0;
    let mut end = offset;
    while let Some(row) = grid.rows.get(end) {
        let h = height_of(row);
        if used + h > GRID + 0.01 {
            break;
        }
        used += h;
        end += 1;
    }
    end
}

/// Where the row of place `at` is, and how tall, with its heading when it
/// comes right after one: what is scrolled into view.
fn span(grid: &Grid, at: usize) -> Option<(f32, f32)> {
    let (row, _) = grid.place(at)?;
    let tops = tops(grid);
    let from = match row.checked_sub(1) {
        Some(before) if matches!(grid.rows[before], Row::Head(_)) => tops[before],
        _ => tops[row],
    };
    Some((from, tops[row] + CELL - from))
}

/// The grid's scrollable.
const GRID_ID: &str = "emoji:grid";

/// What waits for the tree built after a key: the grid scrolled.
static LATER: Later = Later::new();

/// The grid scrolled to `y`, once its tree is built.
fn scroll_to(open: &mut Open, y: f32) {
    let y = y.clamp(0.0, most_scroll(&open.grid));
    if (y - open.scroll).abs() < 0.5 {
        return;
    }
    open.scroll = y;
    LATER.push(iced::advanced::widget::operation::scrollable::scroll_to(
        iced::widget::Id::new(GRID_ID),
        iced::advanced::widget::operation::scrollable::AbsoluteOffset {
            x: None,
            y: Some(y),
        },
    ));
}

/// The grid scrolled so that the selected emoji shows, with its heading
/// when its row is the first under it.
pub fn reveal(open: &mut Open) {
    let Some(span) = span(&open.grid, open.selected) else {
        return;
    };
    let to = kit::reveal(span, GRID, open.scroll, most_scroll(&open.grid));
    scroll_to(open, to);
}

/// The grid scrolled to row `row` (a heading), as far as it goes.
pub fn to_row(open: &mut Open, row: usize) {
    let top = tops(&open.grid).get(row).copied().unwrap_or(0.0);
    scroll_to(open, top);
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// An emoji clicked.
    Act(usize),
    /// The pointer came over an emoji: it is selected.
    Hover(usize),
    /// A tab clicked.
    Tab(usize),
    /// The tone's button (`None`), or a tone, clicked.
    Tones(Option<usize>),
    /// What the search took of an input method, or a press.
    Took(Took),
    /// The grid scrolled to there.
    Scrolled(f32),
    /// A key: every one is the picker's.
    Key(Pressed),
    /// What it shows changed under the view.
    Stale,
}

/// A control's id: what tests and screen readers know it by.
fn id_of(what: &str) -> String {
    format!("emoji:{what}")
}

/// The picker on its surface `id`, on the glass (else opaque).
pub fn view<'a>(id: window::Id, words: Words, glass: bool) -> Element<'a, crate::Message> {
    Around::new(id, build(id, words), move |m| handle(id, m), Msg::Stale)
        .own(hocus_link::EMOJI_NAMESPACE, move || {
            super::with(|p| p.open_on(id).is_some())
        })
        .rebuild(move || build(id, words))
        .keys(move |pressed| {
            super::with(|p| p.open_on(id).is_some_and(|o| o.holding))
                .then(|| Msg::Key(pressed.clone()))
        })
        .modifiers(move |held| {
            super::with(|p| {
                if let Some(open) = p.open_mut(id) {
                    open.shift = held.shift();
                }
            });
        })
        .later(&LATER)
        .on(iced::widget::canvas(Tint(glass))
            .width(Length::Fill)
            .height(Length::Fill))
}

fn handle(id: window::Id, message: Msg) {
    match message {
        Msg::Act(at) => {
            let keep = super::with(|p| p.open_on(id).is_some_and(|o| o.shift));
            super::act(id, at, keep);
        }
        Msg::Hover(at) => super::with(|p| {
            if let Some(open) = p.open_mut(id) {
                open.selected = at;
            }
        }),
        Msg::Tab(i) => {
            if let Some(heading) = Heading::tabs().get(i) {
                super::tab(id, *heading);
            }
        }
        Msg::Tones(tone) => super::tones(id, tone),
        Msg::Took(took) => super::took(id, took),
        Msg::Scrolled(y) => super::with(|p| {
            if let Some(open) = p.open_mut(id) {
                open.scroll = y;
            }
        }),
        Msg::Key(pressed) => {
            super::key_on(id, &pressed.key, pressed.modifiers, pressed.text.as_deref());
        }
        Msg::Stale => {}
    }
}

/// The search, to the kit's field.
fn query(with: &mut dyn FnMut(&mut Entry)) {
    super::with(|p| {
        if let Phase::Open(open) = &mut p.phase {
            with(&mut open.query);
        }
    });
}

/// Its content, as the picker is now; said in the log when it changed.
fn build<'a>(id: window::Id, words: Words) -> El<'a> {
    super::with(|p| {
        let Some(open) = p.open_mut(id) else {
            return Space::new().into();
        };
        let now = shows(open, words);
        if now != open.logged {
            tracing::debug!(said = %now, "the emoji picker shows it");
            open.logged = now;
        }
        content(open, words)
    })
}

/// What it shows, in words, for the log (and the tests).
fn shows(open: &Open, words: Words) -> String {
    let offset = offset_at(&open.grid, open.scroll);
    let shown = &open.grid.rows[offset.min(open.grid.rows.len())..end_of(&open.grid, offset)];
    let mut rows: Vec<String> = Vec::new();
    let mut heads: Vec<&str> = Vec::new();
    for row in shown {
        match row {
            Row::Head(h) => heads.push(words.heading(*h)),
            Row::Cells(range) => {
                let line: Vec<String> = range
                    .clone()
                    .filter_map(|at| {
                        let (_, text) = open.emoji(at)?;
                        Some(if at == open.selected {
                            format!("*{text}")
                        } else {
                            text
                        })
                    })
                    .collect();
                rows.push(line.join(" "));
            }
        }
    }
    let lit = if open.query.is_empty() {
        open.grid.lit(offset).map(|h| words.heading(h))
    } else {
        None
    };
    let footer = open
        .emoji(open.selected)
        .map(|(e, text)| format!("{text} {}", e.name(words.lang)))
        .unwrap_or_default();
    format!(
        "query={:?} place={},{} tone={} tones={} lit={:?} heads={heads:?} rows={rows:?} \
         footer={footer:?} empty={} holding={}",
        open.query.text(),
        open.place.0,
        open.place.1,
        super::store::kept().tone,
        open.tones_open,
        lit.unwrap_or_default(),
        open.grid.flat.is_empty(),
        open.holding,
    )
}

/// `text` in the colour emoji font at `size`, its line as tall.
fn glyph<'a>(text: String, size: f32, alpha: f32) -> iced::widget::Text<'a> {
    iced::widget::text(text)
        .size(size)
        .font(EMOJI_FONT)
        .line_height(LineHeight::Relative(1.0))
        .shaping(Shaping::Advanced)
        .wrapping(Wrapping::None)
        .color(Color {
            a: alpha,
            ..TEXT.get()
        })
}

fn content<'a>(open: &Open, words: Words) -> El<'a> {
    let tone = super::store::kept().tone;
    let width = RIGHT - LEFT;
    let offset = offset_at(&open.grid, open.scroll);
    let mut all = column![top_line(open, words, tone, width)];
    if open.tones_open {
        let mut tones = row![].spacing(TONE_GAP);
        for (i, colour) in TONE_COLOURS.iter().copied().enumerate() {
            let chosen = i == tone;
            let face = drawn(TONES, TONES, move |f, size| {
                let centre = Point::new(size.width / 2.0, size.height / 2.0);
                f.fill(&Path::circle(centre, size.width / 2.0), colour);
                if chosen {
                    f.stroke(
                        &Path::circle(centre, size.width / 2.0 - TONE_RING / 2.0),
                        Stroke::default()
                            .with_color(GREEN.get())
                            .with_width(TONE_RING),
                    );
                }
            });
            let pressed = button(face)
                .padding(0)
                .style(|_, _| style::clear())
                .on_press(Msg::Tones(Some(i)));
            tones = tones.push(
                control(
                    pressed,
                    id_of(&format!("Tone({i})")),
                    Role::RadioButton,
                    words.tone(i),
                )
                .node(|n| n.checked(chosen)),
            );
        }
        all = all
            .push(Space::new().height(GAP))
            .push(row![iced::widget::space::horizontal(), tones].width(width));
    }
    // The tabs: one per heading, the one the grid is at lit.
    let lit = if open.query.is_empty() {
        open.grid.lit(offset)
    } else {
        None
    };
    let headings = Heading::tabs();
    let n = headings.len() as f32;
    let tab_width = (width - (n - 1.0) * TAB_GAP) / n;
    let mut tabs = row![].spacing(TAB_GAP);
    for (i, heading) in headings.into_iter().enumerate() {
        let on = Some(heading) == lit;
        let dim = lit.is_some() && !on;
        let pressed = button(
            container(glyph(
                heading.icon().to_owned(),
                TAB_EMOJI,
                if dim { 0.7 } else { 1.0 },
            ))
            .center(Length::Fill),
        )
        .padding(0)
        .width(tab_width)
        .height(TABS)
        .style(move |_, status| {
            if on {
                style::filled(TAB_LIT.get(), TAB_RADIUS, TEXT.get())
            } else if style::lit(status) {
                style::hovered(HOVER.get(), TAB_RADIUS)
            } else {
                style::clear()
            }
        })
        .on_press(Msg::Tab(i));
        tabs = tabs.push(
            control(
                pressed,
                id_of(&format!("Tab({i})")),
                Role::RadioButton,
                words.heading(heading),
            )
            .node(|n| n.checked(on)),
        );
    }
    all = all.push(Space::new().height(GAP)).push(tabs);
    // The grid: its headings and its rows of emoji, those near the view
    // built.
    let heights: Vec<f32> = open.grid.rows.iter().map(height_of).collect();
    let rows = parts::lazy(&heights, open.scroll, GRID, |r| {
        grid_row(open, words, &open.grid.rows[r], width)
    });
    let scrolled = scrollable(container(rows).width(width))
        .id(iced::widget::Id::new(GRID_ID))
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new()
                .width(0)
                .scroller_width(0)
                .margin(0),
        ))
        .style(|_, _| style::scrollbar_in(0.0, Color::TRANSPARENT))
        .on_scroll(|v| Msg::Scrolled(v.absolute_offset().y))
        .width(width)
        .height(GRID);
    let mut grid: Vec<El<'a>> = vec![Notched::new(scrolled, CELL).into()];
    if open.grid.flat.is_empty() {
        let none = words.none(open.query.text().trim());
        grid.push(
            pin(ui::line(
                container(said_in(none.clone(), EMPTY, FAINT.get()))
                    .width(width)
                    .height(EMPTY * LINE)
                    .align_x(Horizontal::Center)
                    .align_y(Vertical::Center),
                none,
            ))
            .y(EMPTY_TOP)
            .into(),
        );
    }
    all = all
        .push(Space::new().height(GAP))
        .push(Stack::with_children(grid).width(width).height(GRID))
        .push(Space::new().height(GAP))
        .push(
            container(Space::new().width(width).height(1.0))
                .style(|_| style::boxed(RULE.get(), 0.0)),
        )
        .push(Space::new().height(FOOT_PAD))
        .push(footer(open, words, width));
    container(all)
        .padding(Padding {
            top: TOP,
            right: 0.0,
            bottom: 0.0,
            left: LEFT,
        })
        .into()
}

fn said_in<'a>(text: String, size: f32, colour: Color) -> iced::widget::Text<'a> {
    said(text, size, MANROPE, colour)
}

/// The search and the tone's button.
fn top_line<'a>(open: &Open, words: Words, tone: usize, width: f32) -> El<'a> {
    let field_width = width - GAP - TONE_BUTTON;
    let typed = Field {
        with: query,
        style: Style::one(QUERY, MANROPE),
        placeholder: words.placeholder().into(),
        look: Look::of(TEXT.get(), QUIET.get()),
        focused: open.holding,
        left: FIELD_PAD + MAGNIFIER + FIELD_GAP,
        right: FIELD_PAD,
        on_focus: Msg::Stale,
        on_took: Msg::Took,
    }
    .view(TOP_LINE);
    let search = container(Stack::with_children([
        pin(drawn(MAGNIFIER, MAGNIFIER, |f, _| {
            magnifier(f, Point::ORIGIN)
        }))
        .x(FIELD_PAD)
        .y((TOP_LINE - MAGNIFIER) / 2.0)
        .into(),
        typed,
    ]))
    .width(field_width)
    .height(TOP_LINE)
    .style(|_| container::Style {
        background: Some(FIELD_FILL.get().into()),
        border: iced::Border {
            color: FIELD_RIM.get(),
            width: 1.0,
            radius: FIELD_RADIUS.into(),
        },
        ..container::Style::default()
    });
    let search = control(search, id_of("Field"), Role::TextField, words.placeholder())
        .node(|n| n.value(open.query.text().to_owned()))
        .typing();
    let dot = TONE_COLOURS.get(tone).copied().unwrap_or(TONE_COLOURS[0]);
    let face = parts::drawn_lit(TONE_BUTTON, TONE_BUTTON, 0.0, move |f, size, lit| {
        let centre = Point::new(size.width / 2.0, size.height / 2.0);
        let round = Path::circle(centre, size.width / 2.0);
        f.fill(&round, FIELD_FILL.get());
        if lit {
            crate::theme::hover(f, &round, HOVER.get());
        }
        f.stroke(
            &Path::circle(centre, size.width / 2.0 - 0.5),
            Stroke::default()
                .with_color(BUTTON_RIM.get())
                .with_width(1.0),
        );
        f.fill(&Path::circle(centre, TONE_DOT / 2.0), dot);
    });
    let tones = button(face)
        .padding(0)
        .style(|_, _| style::clear())
        .on_press(Msg::Tones(None));
    let tones = control(tones, id_of("Tones"), Role::ToggleButton, words.tones())
        .node(|n| n.checked(open.tones_open));
    row![search, gap(GAP), tones]
        .height(TOP_LINE)
        .align_y(Vertical::Center)
        .into()
}

/// A row of the grid: a heading, or emoji the pointer selects and a click
/// inserts.
fn grid_row<'a>(open: &Open, words: Words, row: &Row, width: f32) -> El<'a> {
    match row {
        Row::Head(heading) => {
            let word = words.heading(*heading);
            ui::line(
                container(spaced_by(
                    &word.to_uppercase(),
                    HEAD_TEXT,
                    MANROPE_BOLD,
                    QUIET.get(),
                    HEAD_SPACING / HEAD_TEXT,
                ))
                .padding(Padding {
                    left: HEAD_LEFT,
                    ..Padding::ZERO
                })
                .width(width)
                .height(HEAD)
                .align_y(Vertical::Center),
                word,
            )
            .into()
        }
        Row::Cells(range) => {
            let mut cells = row![];
            for at in range.clone() {
                let Some((emoji, text)) = open.emoji(at) else {
                    continue;
                };
                let selected = at == open.selected;
                let pressed = button(container(glyph(text, CELL_EMOJI, 1.0)).center(CELL))
                    .padding(0)
                    .width(CELL)
                    .height(CELL)
                    .style(move |_, _| {
                        if selected {
                            iced::widget::button::Style {
                                background: Some(SELECTED_FILL.get().into()),
                                border: iced::Border {
                                    color: SELECTED_RIM.get(),
                                    width: 1.0,
                                    radius: CELL_RADIUS.into(),
                                },
                                ..style::clear()
                            }
                        } else {
                            style::clear()
                        }
                    })
                    .on_press(Msg::Act(at));
                let cell = control(
                    pressed,
                    id_of(&format!("Cell({at})")),
                    Role::Button,
                    emoji.name(words.lang),
                )
                .node(|n| n.selected(selected));
                cells = cells.push(mouse_area(cell).on_enter(Msg::Hover(at)));
            }
            cells.height(CELL).into()
        }
    }
}

/// The footer: the selected emoji, its name, the key.
fn footer<'a>(open: &Open, words: Words, width: f32) -> El<'a> {
    let hint = words.inserts();
    let hint_width = kit::measure(hint, FOOT_HINT, MANROPE);
    let mut line = row![].height(FOOT).align_y(Vertical::Center);
    let mut selected = None;
    if let Some((e, text)) = open.emoji(open.selected) {
        let x = FOOT_EMOJI + z(8.0);
        let room = (width - hint_width - z(8.0) - x).max(0.0);
        let name = kit::elide(e.name(words.lang), room, |t| {
            kit::measure(t, FOOT_NAME, MANROPE)
        });
        line = line
            .push(gap(x))
            .push(ui::line(said_in(name.clone(), FOOT_NAME, NAME.get()), name));
        selected = Some(text);
    }
    line = line.push(iced::widget::space::horizontal()).push(ui::line(
        said_in(hint.to_owned(), FOOT_HINT, HINT.get()),
        hint,
    ));
    let mut footer: Vec<El<'a>> = vec![container(line).width(width).into()];
    // The emoji centred 1 px past its room's middle, wider than its room.
    if let Some(text) = selected {
        let wide = kit::measure(&text, FOOT_EMOJI, EMOJI_FONT);
        footer.push(
            pin(glyph(text, FOOT_EMOJI, 1.0))
                .x(1.0 + FOOT_EMOJI / 2.0 - wide / 2.0)
                .y((FOOT - FOOT_EMOJI) / 2.0)
                .into(),
        );
    }
    Stack::with_children(footer)
        .width(width)
        .height(FOOT)
        .into()
}

/// Its background: its tint (opaque without glass) and its rim.
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
        let tint = canvas::gradient::Linear::new(Point::ORIGIN, Point::new(0.0, size.height))
            .add_stop(0.0, top.get())
            .add_stop(1.0, bottom.get());
        f.fill(
            &Path::rounded_rectangle(Point::ORIGIN, size, RADIUS.into()),
            canvas::Fill::from(tint),
        );
        f.stroke(
            &Path::rounded_rectangle(
                Point::new(BORDER / 2.0, BORDER / 2.0),
                Size::new(size.width - BORDER, size.height - BORDER),
                (RADIUS - BORDER / 2.0).into(),
            ),
            Stroke::default().with_color(RIM.get()).with_width(BORDER),
        );
        vec![f.into_geometry()]
    }
}

/// `text` in the colour emoji font, `size` px, centred on `at`.
pub fn emoji(f: &mut Frame, text: &str, at: Point, size: f32, alpha: f32) {
    f.fill_text(canvas::Text {
        content: text.to_owned(),
        position: at,
        max_width: f32::INFINITY,
        color: Color {
            a: alpha,
            ..TEXT.get()
        },
        size: iced::Pixels(size),
        line_height: LineHeight::Relative(1.0),
        font: EMOJI_FONT,
        align_x: Alignment::Center,
        align_y: alignment::Vertical::Center,
        shaping: Shaping::Advanced,
    });
}

/// The magnifier, the mockup's 24 px path at [`MAGNIFIER`].
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emoji::data;

    #[test]
    fn the_panel_is_the_mockups_zoomed() {
        // 336 × 405 in the mockup, × 1.5; 30 more with the tones.
        assert_eq!(size(false), (504, 606));
        assert_eq!(size(true), (504, 651));
        // Eight cells fit across.
        const { assert!(8.0 * CELL <= RIGHT - LEFT) };
    }

    #[test]
    fn the_grid_shows_whole_rows_and_a_row_shows_with_its_heading() {
        let table = data::table();
        let grid = Grid::of(table, "", &[]);
        // A heading and the rest cells: 33 + 6 × 57 = 375 of 378.
        assert_eq!(end_of(&grid, 0), 7);
        // The first emoji's row, with its heading; the next one alone.
        assert_eq!(span(&grid, 0), Some((0.0, HEAD + CELL)));
        assert_eq!(span(&grid, 8), Some((HEAD + CELL, CELL)));
        // Scrolled a row down: the first row wholly shown is the second.
        assert_eq!(offset_at(&grid, HEAD), 1);
        assert_eq!(offset_at(&grid, 0.0), 0);
        assert!(most_scroll(&grid) > 0.0);
    }
}
