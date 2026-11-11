//! The actions palette's surface (#736, docs/design/actions.md §2), on
//! the widget kit (#772): the mockup's sizes, logical pixels.
//!
//! 720 wide (narrower screens: the work area less 32), radius 16, the
//! popovers' glass, a 1 px rim, 10 px in. The field (56 tall, radius 12,
//! its green border): the prompt, the query or the placeholder, the
//! target's chip and "Échap" on its right; in a step, the action's chip
//! before the query. 8 px under it, the list: headings 24 tall (11 px 800
//! capitals spaced 0.09 em), rows 44 tall (radius 10), 2 px apart, nine
//! rows' room at most, scrolling past it, then the footer under a line.
//!
//! The field holds the keyboard throughout: every key goes to the palette
//! ([`super::key_on`]), which moves the selection, runs it, or edits the
//! field. The pointer over a row selects it, a click runs it; the wheel
//! scrolls the list a row a notch. What it shows is said in the log
//! ("the actions palette shows it") and to screen readers, from its
//! controls' declarations.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock, PoisonError},
};

use hocus_a11y::Role;
use hocus_link::{
    ACTIONS_NAMESPACE,
    actions::{self as catalogue, Glyph, Inert, Side4, Step},
};
use iced::{
    Color, Element, Font, Length, Padding, Point, Rectangle, Renderer, Size, Theme,
    advanced::svg,
    alignment::{Horizontal, Vertical},
    mouse,
    widget::{
        Space, Stack, button,
        canvas::{self, Frame, Path, Stroke},
        column, container, mouse_area, pin, row, scrollable, space,
        text::LineHeight,
    },
    window,
};

use super::{
    Open, Phase, Words,
    list::{Elsewhere, Row, Target},
};
use crate::theme::Ink;
use crate::{
    field::{
        Entry, Took,
        view::{Look, Style},
    },
    levels::draw::{LINE, MANROPE, MANROPE_BOLD, TEXT, rgb},
    popover::{
        Pressed,
        kit::{self, MANROPE_SEMIBOLD},
    },
    ui::{
        self, Around, Later, control,
        field::Field,
        notched::Notched,
        parts::{drawn, drawn_out, gap, spaced_by},
        style::{self, said},
    },
};

type El<'a> = Element<'a, Msg>;

const WIDTH: f32 = hocus_link::ACTIONS_WIDTH as f32;
const RADIUS: f32 = hocus_link::ACTIONS_RADIUS as f32;
const BORDER: f32 = 1.0;
const PAD: f32 = 10.0;
const IN: f32 = BORDER + PAD;
/// The field.
const FIELD: f32 = 56.0;
const FIELD_RADIUS: f32 = 12.0;
const FIELD_LEFT: f32 = 14.0;
const FIELD_RIGHT: f32 = 10.0;
const PROMPT: f32 = 19.0;
const FIELD_GAP: f32 = 10.0;
const QUERY: f32 = 20.0;
const PLACEHOLDER: f32 = 18.0;
const CHIP_TEXT: f32 = 12.5;
const CHIP_PAD: Size = Size::new(9.0, 4.0);
const CHIP_RADIUS: f32 = 8.0;
const STEP_CHIP_TEXT: f32 = 15.0;
const STEP_CHIP_PAD: Size = Size::new(10.0, 4.0);
const HINT: f32 = 11.0;
const HINT_PAD: Size = Size::new(6.0, 3.0);
const HINT_RADIUS: f32 = 6.0;
/// The list.
const LIST_GAP: f32 = 8.0;
const HEAD: f32 = 24.0;
const HEAD_TEXT: f32 = 11.0;
const HEAD_SPACING: f32 = 0.09 * HEAD_TEXT;
const HEAD_LEFT: f32 = 10.0;
pub const ROW: f32 = 44.0;
const ROW_GAP: f32 = 2.0;
const ROW_RADIUS: f32 = 10.0;
const ROW_LEFT: f32 = 8.0;
const ROW_RIGHT: f32 = 10.0;
const ROW_GAP_IN: f32 = 12.0;
const TILE: f32 = 30.0;
const TILE_RADIUS: f32 = 8.0;
const GLYPH: f32 = 17.0;
const LABEL: f32 = 15.5;
const SUB: f32 = 12.0;
const CATEGORY: f32 = 12.5;
const STATE: f32 = 12.0;
const STATE_PAD: Size = Size::new(8.0, 2.0);
const REASON: f32 = 12.5;
const NOTHING: f32 = 14.0;
const NOTHING_ROW: f32 = 48.0;
/// Keycaps: Martian Mono 11 px, 5/6 px in, radius 6, 22 px at least.
const CAP: f32 = 11.0;
const CAP_HEIGHT: f32 = 22.0;
const CAP_PAD: f32 = 6.0;
const CAP_LEAST: f32 = 22.0;
const CAP_GAP: f32 = 3.0;
const CAP_RADIUS: f32 = 6.0;
/// A desktop's sketch, an agent's panda.
const SKETCH: Size = Size::new(44.0, 28.0);
const SKETCH_RADIUS: f32 = 5.0;
const PANDA: f32 = 26.0;
/// The rows' room at most, and the scroll mark.
pub const MOST_ROWS: usize = 9;
const MOST_LIST: f32 = MOST_ROWS as f32 * ROW + (MOST_ROWS - 1) as f32 * ROW_GAP;
const MARK_WIDTH: f32 = 3.0;
/// The footer: 8 px, a line, 8 px, its text (12 px), 2 px.
const FOOT_GAP: f32 = 8.0;
const FOOT: f32 = 12.0;
const FOOT_LINE: f32 = 16.0;
const FOOT_LEFT: f32 = 10.0;

const MONO: Font = Font::with_name("Martian Mono");
const MANROPE_EXTRABOLD: Font = Font {
    weight: iced::font::Weight::ExtraBold,
    ..MANROPE
};
const MANROPE_ITALIC: Font = Font {
    style: iced::font::Style::Italic,
    ..MANROPE
};

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, a)
}

/// The popovers' glass, opaque without it.
const GLASS_TOP: Ink = Ink::Dark(rgba(26, 31, 48, 0.97));
const GLASS_BOTTOM: Ink = Ink::Dark(rgba(16, 19, 30, 0.98));
const OPAQUE_TOP: Ink = Ink::Dark(rgb(0x1A1F30));
const OPAQUE_BOTTOM: Ink = Ink::Dark(rgb(0x10131E));
const RIM: Ink = Ink::Rim(rgba(255, 255, 255, 0.12));
const FIELD_FILL: Ink = Ink::Dark(rgba(255, 255, 255, 0.05));
const FIELD_RIM: Ink = Ink::Accent(0.45);
const PROMPT_INK: Ink = Ink::Dark(rgb(0xA9B2C2));
const QUIET: Ink = Ink::Dark(rgb(0x7C8698));
const HINT_RIM: Ink = Ink::Dark(rgba(255, 255, 255, 0.14));
const CHIP_FILL: Ink = Ink::Accent(0.12);
const CHIP_RIM: Ink = Ink::Accent(0.35);
const CHIP_INK: Ink = Ink::Dark(rgb(0xC9D0DC));
const GREEN: Ink = Ink::Accent(1.0);
const ON_GREEN: Ink = Ink::Fixed(rgb(0x0B1020));
const SELECTED_FILL: Ink = Ink::Accent(0.14);
const SELECTED_RIM: Ink = Ink::Accent(0.6);
const HOVER: Ink = Ink::Dark(rgba(255, 255, 255, 0.05));
const TILE_FILL: Ink = Ink::Dark(rgba(255, 255, 255, 0.06));
const WAITING_TILE: Ink = Ink::Fixed(rgba(245, 180, 74, 0.14));
const GLYPH_INK: Ink = Ink::Dark(rgb(0xC9D0DC));
const STATE_FILL: Ink = Ink::Dark(rgba(255, 255, 255, 0.05));
const STATE_OFF: Ink = Ink::Dark(rgb(0x8C96A8));
const REASON_INK: Ink = Ink::Dark(rgb(0xC9D0DC));
const FAINT: Ink = Ink::Dark(rgb(0x8C96A8));
const CAP_FILL: Ink = Ink::Dark(rgba(255, 255, 255, 0.06));
const CAP_RIM: Ink = Ink::Dark(rgba(255, 255, 255, 0.2));
const CAP_INK: Ink = Ink::Dark(rgb(0xD5DBE5));
const SKETCH_FILL: Ink = Ink::Dark(rgba(255, 255, 255, 0.05));
const SKETCH_RIM: Ink = Ink::Dark(rgba(255, 255, 255, 0.12));
const RULE: Ink = Ink::Dark(rgba(255, 255, 255, 0.07));
const MARK: Ink = Ink::Dark(rgba(255, 255, 255, 0.22));
/// The windows in a desktop's sketch, in turn.
const SKETCHED: [Color; 5] = [
    rgb(0xF2945B),
    rgb(0x7EE0A0),
    rgb(0x8FB3FF),
    rgb(0xB39DFF),
    rgb(0xF5B44A),
];
/// An inert row's opacity.
const INERT: f32 = 0.45;

/// The palette's width on a work area `area` wide.
pub fn width(area: f32) -> f32 {
    if area <= 0.0 {
        return WIDTH;
    }
    WIDTH.min(area - 32.0).max(320.0).round()
}

fn height_of(row: &Row) -> f32 {
    match row {
        Row::Head(_) => HEAD,
        Row::Nothing(_) => NOTHING_ROW,
        Row::Action(_) | Row::Target(_) | Row::Elsewhere(_) => ROW,
    }
}

/// The list's height: its whole rows from the first, within nine rows'
/// room.
fn list_height(rows: &[Row]) -> f32 {
    let mut used = 0.0;
    for (i, row) in rows.iter().enumerate() {
        let h = height_of(row) + if i > 0 { ROW_GAP } else { 0.0 };
        if used + h > MOST_LIST + 0.01 {
            break;
        }
        used += h;
    }
    used
}

/// The surface's size for `rows`, `width` wide: as tall as the whole rows
/// shown from the first, nine rows' room at most.
pub fn size(rows: &[Row], width: f32) -> (u32, u32) {
    let list = list_height(rows);
    let height = IN + FIELD + LIST_GAP + list + FOOT_GAP + 1.0 + FOOT_GAP + FOOT_LINE + 2.0 + IN;
    (width.round() as u32, height.ceil() as u32)
}

/// Where row `at` is in the list, and how tall, with its heading when it
/// comes right after one: what is scrolled into view.
fn span(rows: &[Row], at: usize) -> (f32, f32) {
    let mut top = 0.0;
    let mut tops = Vec::with_capacity(rows.len());
    for row in rows {
        tops.push(top);
        top += height_of(row) + ROW_GAP;
    }
    let Some(row) = rows.get(at) else {
        return (0.0, 0.0);
    };
    let bottom = tops[at] + height_of(row);
    let from = match at.checked_sub(1) {
        Some(before) if matches!(rows[before], Row::Head(_)) => tops[before],
        _ => tops[at],
    };
    (from, bottom - from)
}

/// How far the list scrolls at most.
fn most_scroll(rows: &[Row]) -> f32 {
    let content: f32 =
        rows.iter().map(height_of).sum::<f32>() + rows.len().saturating_sub(1) as f32 * ROW_GAP;
    (content - list_height(rows)).max(0.0)
}

/// The list's scrollable.
const LIST_ID: &str = "actions:list";

/// What waits for the tree built after a key: the list scrolled to the
/// selection.
static LATER: Later = Later::new();

/// The list scrolled so that the selected row shows, with its heading
/// when it is the first under it.
pub fn reveal(open: &mut Open) {
    let span = span(&open.rows, open.selected);
    let to = kit::reveal(
        span,
        list_height(&open.rows),
        open.scroll,
        most_scroll(&open.rows),
    );
    if (to - open.scroll).abs() < 0.5 {
        return;
    }
    open.scroll = to;
    LATER.push(iced::advanced::widget::operation::scrollable::scroll_to(
        iced::widget::Id::new(LIST_ID),
        iced::advanced::widget::operation::scrollable::AbsoluteOffset {
            x: None,
            y: Some(to),
        },
    ));
}

/// The list back at its top, as its rows change.
pub fn to_top(open: &mut Open) {
    if open.scroll.abs() < 0.5 {
        return;
    }
    open.scroll = 0.0;
    LATER.push(iced::advanced::widget::operation::scrollable::scroll_to(
        iced::widget::Id::new(LIST_ID),
        iced::advanced::widget::operation::scrollable::AbsoluteOffset {
            x: None,
            y: Some(0.0),
        },
    ));
}

/// A row's control: what tests and screen readers know it by.
fn row_id(at: usize) -> String {
    format!("actions:Row({at})")
}

/// The field's.
const FIELD_ID: &str = "actions:Field";

/// Row `key`'s place in the list, from its control's id.
pub fn row_of_id(key: &str) -> Option<usize> {
    key.strip_prefix("actions:Row(")?
        .strip_suffix(')')?
        .parse()
        .ok()
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A row clicked.
    Act(usize),
    /// The pointer came over a row: it is selected.
    Hover(usize),
    /// What the field took of an input method, or a press.
    Took(Took),
    /// The list scrolled to there.
    Scrolled(f32),
    /// A key: every one is the palette's.
    Key(Pressed),
    /// What it shows changed under the view.
    Stale,
}

/// The palette on its surface `id`, on the glass (else opaque).
pub fn view<'a>(id: window::Id, words: Words, glass: bool) -> Element<'a, crate::Message> {
    Around::new(
        id,
        build(id, words),
        move |m| handle(id, words, m),
        Msg::Stale,
    )
    .own(ACTIONS_NAMESPACE, move || {
        super::with(|p| p.open_on(id).is_some())
    })
    .rebuild(move || build(id, words))
    .keys(|pressed| Some(Msg::Key(pressed.clone())))
    .later(&LATER)
    .on(iced::widget::canvas(Tint(glass))
        .width(Length::Fill)
        .height(Length::Fill))
}

fn handle(id: window::Id, words: Words, message: Msg) {
    match message {
        Msg::Act(at) => super::act(id, at),
        Msg::Hover(at) => super::with(|p| {
            if let Some(open) = p.open_mut(id)
                && open.rows.get(at).is_some_and(Row::selectable)
            {
                open.selected = at;
            }
        }),
        Msg::Took(took) => super::took(id, took, words),
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

/// The field, to the kit's field.
fn query(with: &mut dyn FnMut(&mut Entry)) {
    super::with(|p| {
        if let Phase::Open(open) = &mut p.phase {
            with(&mut open.query);
        }
    });
}

/// Its content, as the palette is now; said in the log when it changed.
fn build<'a>(id: window::Id, words: Words) -> El<'a> {
    super::with(|p| {
        let Some(open) = p.open_mut(id) else {
            return Space::new().into();
        };
        let now = shows(open, words);
        if now != open.logged {
            tracing::debug!(said = %now, "the actions palette shows it");
            open.logged = now;
        }
        content(open, words)
    })
}

fn content<'a>(open: &Open, words: Words) -> El<'a> {
    let width = open.size.0 as f32;
    let inner = width - 2.0 * IN;
    let list = list_height(&open.rows);
    let mut rows = column![].spacing(ROW_GAP).width(inner);
    for (at, row) in open.rows.iter().enumerate() {
        rows = rows.push(row_view(open, words, at, row, inner));
    }
    // In a container: a scrollable lays its content out as wide as its
    // viewport at least. Its scrollbar where the mockup's mark is, 5 px
    // in from the rim, past the rows.
    let scrolled = scrollable(container(rows))
        .id(iced::widget::Id::new(LIST_ID))
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new()
                .width(MARK_WIDTH)
                .scroller_width(MARK_WIDTH)
                .margin(0)
                .spacing(0),
        ))
        .style(|_, _| style::scrollbar_in(MARK_WIDTH, MARK.get()))
        .on_scroll(|v| Msg::Scrolled(v.absolute_offset().y))
        .width(width - BORDER - 2.0 - IN)
        .height(list);
    let matched = if open.query.text().trim().is_empty() && open.step.is_none() {
        super::all()
    } else {
        open.matched()
    };
    let footer = words.footer(
        matched,
        open.step.is_some(),
        open.query.text().trim().is_empty(),
    );
    let footer = kit::elide(&footer, inner - 2.0 * FOOT_LEFT, |t| {
        kit::measure(t, FOOT, MANROPE)
    });
    let all = column![
        field(open, words, inner),
        Space::new().height(LIST_GAP),
        Notched::new(scrolled, ROW + ROW_GAP),
        Space::new().height(FOOT_GAP),
        container(Space::new().width(inner).height(1.0)).style(|_| style::boxed(RULE.get(), 0.0)),
        // Its line a pixel taller, from a pixel higher: centred in it,
        // the text's top falls where the canvas drew it (iced rounds it).
        Space::new().height(FOOT_GAP - 1.0),
        ui::line(
            container(said(footer.clone(), FOOT, MANROPE, QUIET.get()))
                .padding([0.0, FOOT_LEFT])
                .height(FOOT_LINE + 1.0)
                .align_y(Vertical::Center),
            footer,
        ),
    ];
    container(all)
        .padding(Padding {
            top: IN,
            right: 0.0,
            bottom: 0.0,
            left: IN,
        })
        .into()
}

/// The field, `width` wide: the prompt, the step's chip, what is typed,
/// the target's chip, "Échap".
fn field<'a>(open: &Open, words: Words, width: f32) -> El<'a> {
    let hint = words.escape();
    let hint_width = kit::measure(hint, HINT, MANROPE_SEMIBOLD) + 2.0 * HINT_PAD.width;
    let hint_height = HINT + 2.0 * HINT_PAD.height + 2.0;
    let hint = container(said(hint, HINT, MANROPE_SEMIBOLD, QUIET.get()))
        .center_x(hint_width)
        .center_y(hint_height)
        .style(|_| container::Style {
            border: iced::Border {
                color: HINT_RIM.get(),
                width: 1.0,
                radius: (HINT_RADIUS + 0.5).into(),
            },
            ..container::Style::default()
        });
    let left = FIELD_LEFT + PROMPT + FIELD_GAP;
    let mut right = width - FIELD_RIGHT - hint_width - FIELD_GAP;
    // The target's chip: a window's glyph and its title.
    let chip: Option<El<'a>> = (!open.shown.title.is_empty()).then(|| {
        let room = ((right - left) * 0.45).max(60.0);
        let title = kit::elide(&open.shown.title, room - 2.0 * CHIP_PAD.width - 19.0, |t| {
            kit::measure(t, CHIP_TEXT, MANROPE_SEMIBOLD)
        });
        let w = kit::measure(&title, CHIP_TEXT, MANROPE_SEMIBOLD) + 2.0 * CHIP_PAD.width + 19.0;
        let h = CHIP_TEXT * LINE + 2.0 * CHIP_PAD.height + 2.0;
        right -= w + FIELD_GAP;
        // A window's glyph: 12 × 9, a 1.6 px green rim.
        let mark = drawn_out(12.0, 9.0, 1.0, |f, size| {
            f.stroke(
                &kit::rounded(Rectangle::new(Point::ORIGIN, size), 2.0),
                Stroke::default().with_color(GREEN.get()).with_width(1.6),
            );
        });
        container(
            row![
                mark,
                gap(7.0),
                said(title, CHIP_TEXT, MANROPE_SEMIBOLD, CHIP_INK.get())
            ]
            .align_y(Vertical::Center),
        )
        .padding([0.0, CHIP_PAD.width])
        .width(w)
        .center_y(h)
        .style(|_| container::Style {
            background: Some(CHIP_FILL.get().into()),
            border: iced::Border {
                color: CHIP_RIM.get(),
                width: 1.0,
                radius: CHIP_RADIUS.into(),
            },
            ..container::Style::default()
        })
        .into()
    });
    let mut line = row![
        gap(FIELD_LEFT),
        drawn(PROMPT, PROMPT, |f, _| {
            glyph_path(
                f,
                "M5 7 L10 12 L5 17 M12 17 H19",
                Point::ORIGIN,
                PROMPT,
                2.2,
                PROMPT_INK.get(),
            );
        }),
        gap(FIELD_GAP),
    ]
    .align_y(Vertical::Center);
    let mut x = left;
    // In a step, the action's chip first.
    if let Some(step) = &open.step {
        let chip = format!("{} ›", step.def.stem(words.fr));
        let w = kit::measure(&chip, STEP_CHIP_TEXT, MANROPE_BOLD) + 2.0 * STEP_CHIP_PAD.width;
        let h = STEP_CHIP_TEXT * LINE + 2.0 * STEP_CHIP_PAD.height;
        line = line
            .push(
                container(said(chip, STEP_CHIP_TEXT, MANROPE_BOLD, ON_GREEN.get()))
                    .center_x(w)
                    .center_y(h)
                    .style(|_| style::boxed(GREEN.get(), CHIP_RADIUS)),
            )
            .push(gap(8.0));
        x += w + 8.0;
    }
    let room = (right - x).max(0.0);
    // What is typed, its selection and its green caret; the placeholder
    // smaller while it is empty.
    let placeholder = match &open.step {
        Some(step) => words.step_placeholder(step.def.step.unwrap_or(Step::Desktop)),
        None => words.placeholder(),
    };
    let placeholder = kit::elide(placeholder, room - 8.0, |t| {
        kit::measure(t, PLACEHOLDER, MANROPE)
    });
    let typed = Field {
        with: Box::new(query),
        style: Style {
            hint: (PLACEHOLDER, MANROPE),
            ..Style::one(QUERY, MANROPE)
        },
        placeholder,
        look: Look {
            caret: GREEN.get(),
            ..Look::of(TEXT.get(), QUIET.get())
        },
        focused: true,
        left: 2.0,
        right: 4.0,
        on_focus: Msg::Stale,
        on_took: Msg::Took,
    }
    .view(FIELD);
    line = line.push(container(typed).width(room)).push(gap(FIELD_GAP));
    if let Some(chip) = chip {
        line = line.push(chip).push(gap(FIELD_GAP));
    }
    line = line.push(hint).push(gap(FIELD_RIGHT));
    let mut said_field = control(
        container(line)
            .width(width)
            .height(FIELD)
            .style(|_| container::Style {
                background: Some(FIELD_FILL.get().into()),
                border: iced::Border {
                    color: FIELD_RIM.get(),
                    width: 1.0,
                    radius: FIELD_RADIUS.into(),
                },
                ..container::Style::default()
            }),
        FIELD_ID,
        Role::TextField,
        words.search(),
    )
    .node(|n| n.value(open.query.text().to_owned()))
    .typing();
    if let Some(step) = &open.step {
        let stem = format!("{} ›", step.def.stem(words.fr));
        said_field = said_field.node(|n| n.describe(stem));
    }
    said_field.into()
}

/// Row `at`, `width` wide: a heading, the line saying nothing matches, or
/// a row the pointer selects and a click runs.
fn row_view<'a>(open: &Open, words: Words, at: usize, row: &Row, width: f32) -> El<'a> {
    match row {
        Row::Head(label) => {
            return ui::line(
                container(spaced_by(
                    &label.to_uppercase(),
                    HEAD_TEXT,
                    MANROPE_EXTRABOLD,
                    QUIET.get(),
                    HEAD_SPACING / HEAD_TEXT,
                ))
                .padding(Padding {
                    left: HEAD_LEFT,
                    bottom: 4.0,
                    ..Padding::ZERO
                })
                .width(width)
                .height(HEAD)
                .align_y(Vertical::Bottom),
                label.clone(),
            )
            .into();
        }
        Row::Nothing(line) => {
            return ui::line(
                container(said(line.clone(), NOTHING, MANROPE, FAINT.get()))
                    .width(width)
                    .height(NOTHING_ROW)
                    .align_x(Horizontal::Center)
                    .align_y(Vertical::Center),
                line.clone(),
            )
            .into();
        }
        Row::Action(_) | Row::Target(_) | Row::Elsewhere(_) => {}
    }
    let selected = at == open.selected;
    let inert = row.inert().is_some();
    let alpha = if inert { INERT } else { 1.0 };
    let ink = move |ink: Ink| ink.scale_alpha(alpha);
    // The lead: a tile with a glyph, a desktop's sketch, a panda.
    let (lead, lead_width): (El<'a>, f32) = match row {
        Row::Target(t) => match &t.target {
            Target::Desktop { windows, index } => {
                // The desktop its screen shows, ringed.
                let current = desks_current(open) == Some(*index);
                let windows = windows.clone();
                (
                    drawn_out(SKETCH.width, SKETCH.height, 2.0, move |f, _| {
                        sketch(f, Point::ORIGIN, &windows, current, alpha);
                    }),
                    SKETCH.width,
                )
            }
            Target::Agent { state, .. } => {
                let state = state.clone();
                (
                    drawn(TILE, TILE, move |f, _| {
                        let tile = Rectangle::new(Point::ORIGIN, Size::new(TILE, TILE));
                        let fill = if state == "waiting" {
                            WAITING_TILE
                        } else {
                            TILE_FILL
                        };
                        f.fill(&kit::rounded(tile, TILE_RADIUS), fill.scale_alpha(alpha));
                        let p = Rectangle::new(
                            Point::new((TILE - PANDA) / 2.0, (TILE - PANDA) / 2.0),
                            Size::new(PANDA, PANDA),
                        );
                        f.draw_svg(p, svg::Svg::new(panda(&state)).opacity(alpha));
                    }),
                    TILE,
                )
            }
        },
        Row::Action(a) => {
            let glyph = a.def.glyph;
            (
                drawn(TILE, TILE, move |f, _| tile(f, Point::ORIGIN, glyph, alpha)),
                TILE,
            )
        }
        Row::Elsewhere(to) => {
            let glyph = match to {
                Elsewhere::Launcher => Glyph::Search,
                Elsewhere::Settings => Glyph::Gear,
            };
            (
                drawn(TILE, TILE, move |f, _| tile(f, Point::ORIGIN, glyph, alpha)),
                TILE,
            )
        }
        Row::Head(_) | Row::Nothing(_) => (Space::new().into(), 0.0),
    };
    let x = ROW_LEFT + lead_width + ROW_GAP_IN;
    // On the right: the shortcut, the switch's state, or the reason.
    let mut right = width - ROW_RIGHT;
    let fr = words.fr;
    let (caps, state, reason): (Vec<String>, Option<bool>, Option<Inert>) = match row {
        Row::Action(a) => {
            let caps = open
                .shown
                .chords(a.def)
                .and_then(|(chords, family)| {
                    let chord = chords.first()?;
                    Some(catalogue::caps(chord, fr, family && a.arg.is_none()))
                })
                .unwrap_or_default();
            if let Some(why) = a.inert {
                (Vec::new(), None, Some(why))
            } else if a.on.is_some() {
                (Vec::new(), a.on, None)
            } else {
                (caps, None, None)
            }
        }
        Row::Target(t) => match t.inert {
            Some(why) => (Vec::new(), None, Some(why)),
            None => (
                t.digit.map(|d| vec![d.to_string()]).unwrap_or_default(),
                None,
                None,
            ),
        },
        Row::Elsewhere(Elsewhere::Launcher) => (
            catalogue::find("launcher")
                .and_then(|d| open.shown.chords(d))
                .and_then(|(c, _)| c.first().map(|c| catalogue::caps(c, fr, false)))
                .unwrap_or_default(),
            None,
            None,
        ),
        _ => (Vec::new(), None, None),
    };
    let aside_right: El<'a> = if let Some(why) = reason {
        let said_why = why.said(fr);
        right -= kit::measure(said_why, REASON, MANROPE_ITALIC) + ROW_GAP_IN;
        said(said_why, REASON, MANROPE_ITALIC, ink(REASON_INK)).into()
    } else if let Some(on) = state {
        let word = words.state(on);
        let w = kit::measure(word, STATE, MANROPE_BOLD) + 2.0 * STATE_PAD.width;
        let h = STATE * LINE + 2.0 * STATE_PAD.height;
        right -= w + ROW_GAP_IN;
        container(said(
            word,
            STATE,
            MANROPE_BOLD,
            if on { ink(GREEN) } else { ink(STATE_OFF) },
        ))
        .center_x(w)
        .center_y(h)
        .style(move |_| style::boxed(STATE_FILL.scale_alpha(alpha), 6.0))
        .into()
    } else if !caps.is_empty() {
        let widths: Vec<f32> = caps
            .iter()
            .map(|c| (kit::measure(c, CAP, MONO) + 2.0 * CAP_PAD).max(CAP_LEAST))
            .collect();
        right -= widths.iter().sum::<f32>() + CAP_GAP * (widths.len() - 1) as f32 + ROW_GAP_IN;
        let mut keys = row![].spacing(CAP_GAP).align_y(Vertical::Center);
        for (cap, w) in caps.into_iter().zip(widths) {
            keys = keys.push(keycap(cap, w, alpha));
        }
        keys.into()
    } else {
        Space::new().into()
    };
    // The name, its marks, "›" for a step, the keyword; then its category
    // in what is left.
    let (label, marks, step, keyword, aside) = match row {
        Row::Action(a) => (
            a.label.clone(),
            a.marks.clone(),
            a.def.step.is_some() && a.arg.is_none(),
            a.keyword.as_deref().map(|k| words.keyword(k)),
            a.def.category.name(fr).to_owned(),
        ),
        Row::Target(t) => (t.label.clone(), Vec::new(), false, None, t.sub.clone()),
        Row::Elsewhere(to) => (
            words.elsewhere(*to, open.query.text().trim()),
            Vec::new(),
            false,
            None,
            words.elsewhere_line(*to).to_owned(),
        ),
        Row::Head(_) | Row::Nothing(_) => (String::new(), Vec::new(), false, None, String::new()),
    };
    let room = (right - x).max(0.0);
    let step_w = if step {
        8.0 + kit::measure("›", LABEL, MANROPE_BOLD)
    } else {
        0.0
    };
    let keyword_w = keyword
        .as_deref()
        .map_or(0.0, |k| 8.0 + kit::measure(k, SUB, MANROPE));
    let label_room = (room * 0.72 - step_w - keyword_w).max(room * 0.4);
    let full = kit::measure(&label, LABEL, MANROPE_SEMIBOLD);
    let cut = full > label_room;
    let shown = if cut {
        kit::elide(&label, label_room, |t| {
            kit::measure(t, LABEL, MANROPE_SEMIBOLD)
        })
    } else {
        label
    };
    let marks = if cut { Vec::new() } else { marks };
    let mut line = row![gap(ROW_LEFT), lead, gap(ROW_GAP_IN)].align_y(Vertical::Center);
    let mut lx = x;
    for (run, marked) in runs(&shown, &marks) {
        let (font, colour) = if marked {
            (MANROPE_EXTRABOLD, ink(GREEN))
        } else {
            (MANROPE_SEMIBOLD, ink(TEXT))
        };
        lx += kit::measure(&run, LABEL, font);
        line = line.push(said(run, LABEL, font, colour));
    }
    if step {
        lx += 8.0 + kit::measure("›", LABEL, MANROPE_BOLD);
        line = line
            .push(gap(8.0))
            .push(said("›", LABEL, MANROPE_BOLD, ink(QUIET)));
    }
    if let Some(keyword) = keyword {
        lx += 8.0 + kit::measure(&keyword, SUB, MANROPE);
        line = line
            .push(gap(8.0))
            .push(said(keyword, SUB, MANROPE, ink(QUIET)));
    }
    let aside_room = right - (lx + ROW_GAP_IN);
    if aside_room > 24.0 && !aside.is_empty() {
        line = line.push(gap(ROW_GAP_IN)).push(said(
            kit::elide(&aside, aside_room, |t| kit::measure(t, CATEGORY, MANROPE)),
            CATEGORY,
            MANROPE,
            ink(QUIET),
        ));
    }
    line = line
        .push(space::horizontal())
        .push(aside_right)
        .push(gap(ROW_RIGHT));
    let pressed = button(line.height(ROW))
        .padding(0)
        .width(width)
        .height(ROW)
        .style(move |_, status| {
            if selected {
                iced::widget::button::Style {
                    background: Some(SELECTED_FILL.scale_alpha(alpha).into()),
                    border: iced::Border {
                        color: SELECTED_RIM.scale_alpha(alpha),
                        width: 1.0,
                        radius: ROW_RADIUS.into(),
                    },
                    ..style::clear()
                }
            } else if style::lit(status) {
                style::filled(HOVER.scale_alpha(alpha), ROW_RADIUS, TEXT.get())
            } else {
                style::clear()
            }
        })
        .on_press(Msg::Act(at));
    let (name, describe) = spoken(open, row, words);
    let selectable: Vec<usize> = (0..open.rows.len())
        .filter(|i| open.rows[*i].selectable())
        .collect();
    let n = selectable.iter().position(|i| *i == at).unwrap_or(0);
    let item = control(pressed, row_id(at), Role::ListItem, name).node(|node| {
        node.describe(describe)
            .selected(selected)
            .disabled(inert)
            .set(n + 1, selectable.len())
    });
    mouse_area(item).on_enter(Msg::Hover(at)).into()
}

/// A keycap `width` wide: Martian Mono, its bottom edge drawn 2 px up.
fn keycap<'a>(cap: String, width: f32, alpha: f32) -> El<'a> {
    let face = container(
        iced::widget::text(cap)
            .size(CAP)
            .font(MONO)
            .line_height(LineHeight::Relative(1.0))
            .shaping(iced::widget::text::Shaping::Advanced)
            .wrapping(iced::widget::text::Wrapping::None)
            .color(CAP_INK.scale_alpha(alpha)),
    )
    .center_x(width)
    .center_y(CAP_HEIGHT)
    .style(move |_| container::Style {
        background: Some(CAP_FILL.scale_alpha(alpha).into()),
        border: iced::Border {
            color: CAP_RIM.scale_alpha(alpha),
            width: 1.0,
            radius: CAP_RADIUS.into(),
        },
        ..container::Style::default()
    });
    let edge = container(Space::new().width(width - CAP_RADIUS).height(1.0))
        .style(move |_| style::boxed(CAP_RIM.scale_alpha(alpha), 0.0));
    Stack::with_children([
        face.into(),
        pin(edge).x(CAP_RADIUS / 2.0).y(CAP_HEIGHT - 2.0).into(),
    ])
    .width(width)
    .height(CAP_HEIGHT)
    .into()
}

/// Its background: the popovers' glass (opaque without it) and its rim.
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

/// The desktop the palette's screen shows, as the bar knows it.
fn desks_current(open: &Open) -> Option<u8> {
    crate::desktops::snapshot(&open.shown.output).map(|d| d.current)
}

/// `label` cut in runs of marked and unmarked characters.
fn runs(label: &str, marks: &[usize]) -> Vec<(String, bool)> {
    let mut runs: Vec<(String, bool)> = Vec::new();
    for (i, c) in label.chars().enumerate() {
        let marked = marks.contains(&i);
        match runs.last_mut() {
            Some((run, m)) if *m == marked => run.push(c),
            _ => runs.push((c.to_string(), marked)),
        }
    }
    runs
}

/// A row as a screen reader says it: its name, and what it adds (its
/// category, the keyword that found it, its shortcut, its state, or why it
/// cannot run).
pub fn spoken(open: &Open, row: &Row, words: Words) -> (String, String) {
    let fr = words.fr;
    match row {
        Row::Action(a) => {
            let name = if a.def.step.is_some() && a.arg.is_none() {
                a.def.stem(fr).to_owned() + "…"
            } else {
                a.label.clone()
            };
            let mut aside = vec![a.def.category.name(fr).to_owned()];
            if let Some(k) = &a.keyword {
                aside.push(words.keyword(k));
            }
            if let Some(why) = a.inert {
                aside.push(why.said(fr).to_owned());
            } else if let Some(on) = a.on {
                aside.push(words.state(on).to_owned());
            } else if let Some((chords, family)) = open.shown.chords(a.def)
                && let Some(chord) = chords.first()
            {
                aside.push(catalogue::caps(chord, fr, family && a.arg.is_none()).join(" "));
            }
            (name, aside.join(", "))
        }
        Row::Target(t) => {
            let mut aside = vec![t.sub.clone()];
            if let Some(why) = t.inert {
                aside.push(why.said(fr).to_owned());
            }
            (t.label.clone(), aside.join(", "))
        }
        Row::Elsewhere(to) => (
            words.elsewhere(*to, open.query.text().trim()),
            words.elsewhere_line(*to).to_owned(),
        ),
        Row::Head(h) => (h.clone(), String::new()),
        Row::Nothing(n) => (n.clone(), String::new()),
    }
}

/// What the palette shows, in words, for the log (and the tests).
pub fn shows(open: &Open, words: Words) -> String {
    let rows: Vec<String> = open
        .rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let selected = if i == open.selected && row.selectable() {
                "*"
            } else {
                ""
            };
            match row {
                Row::Head(h) => format!("# {h}"),
                Row::Action(a) => {
                    let mut said = format!("{selected}{}", a.label);
                    if !a.marks.is_empty() {
                        let marked: String = a
                            .label
                            .chars()
                            .enumerate()
                            .filter(|(i, _)| a.marks.contains(i))
                            .map(|(_, c)| c)
                            .collect();
                        said.push_str(&format!(" [{marked}]"));
                    }
                    if let Some(k) = &a.keyword {
                        said.push_str(&format!(" ({})", words.keyword(k)));
                    }
                    if let Some(on) = a.on {
                        said.push_str(&format!(" = {}", words.state(on)));
                    }
                    if let Some(why) = a.inert {
                        said.push_str(&format!(" ~ {}", why.said(words.fr)));
                    } else if let Some((chords, family)) = open.shown.chords(a.def) {
                        let caps = chords
                            .first()
                            .map(|c| {
                                catalogue::caps(c, words.fr, family && a.arg.is_none()).join(" ")
                            })
                            .unwrap_or_default();
                        said.push_str(&format!(" <{caps}>"));
                    }
                    said
                }
                Row::Target(t) => {
                    let mut said = format!("{selected}{} · {}", t.label, t.sub);
                    if let Some(why) = t.inert {
                        said.push_str(&format!(" ~ {}", why.said(words.fr)));
                    }
                    said
                }
                Row::Nothing(n) => n.clone(),
                Row::Elsewhere(to) => {
                    format!(
                        "{selected}{}",
                        words.elsewhere(*to, open.query.text().trim())
                    )
                }
            }
        })
        .collect();
    format!(
        "query={:?} step={:?} target={:?} place={},{} size={}x{} rows={rows:?}",
        open.query.text(),
        open.step.as_ref().map(|s| s.def.id),
        open.shown.title,
        open.place.0,
        open.place.1,
        open.size.0,
        open.size.1,
    )
}

/// A row's tile with `glyph` at `at`.
fn tile(f: &mut Frame, at: Point, glyph: Glyph, alpha: f32) {
    let rect = Rectangle::new(at, Size::new(TILE, TILE));
    f.fill(
        &kit::rounded(rect, TILE_RADIUS),
        TILE_FILL.scale_alpha(alpha),
    );
    let at = Point::new(rect.center_x() - GLYPH / 2.0, rect.center_y() - GLYPH / 2.0);
    glyph_path(
        f,
        path_of(glyph),
        at,
        GLYPH,
        1.8,
        GLYPH_INK.scale_alpha(alpha),
    );
}

/// A desktop's sketch at `at`: its windows; the current one ringed.
fn sketch(f: &mut Frame, at: Point, windows: &[[u16; 4]], current: bool, alpha: f32) {
    let rect = Rectangle::new(at, SKETCH);
    f.fill(
        &kit::rounded(rect, SKETCH_RADIUS),
        SKETCH_FILL.scale_alpha(alpha),
    );
    if current {
        f.stroke(
            &kit::rounded(inset(rect, -1.0), SKETCH_RADIUS + 1.0),
            Stroke::default()
                .with_color(Ink::Accent(alpha).get())
                .with_width(2.0),
        );
    } else {
        f.stroke(
            &kit::rounded(inset(rect, 0.5), SKETCH_RADIUS - 0.5),
            Stroke::default()
                .with_color(SKETCH_RIM.scale_alpha(alpha))
                .with_width(1.0),
        );
    }
    let share = f32::from(hocus_link::SHARE);
    for (i, [x, y, w, h]) in windows.iter().enumerate() {
        let r = Rectangle::new(
            Point::new(
                rect.x + f32::from(*x) / share * rect.width,
                rect.y + f32::from(*y) / share * rect.height,
            ),
            Size::new(
                (f32::from(*w) / share * rect.width).max(2.0),
                (f32::from(*h) / share * rect.height).max(2.0),
            ),
        );
        let colour = SKETCHED[i % SKETCHED.len()];
        f.fill(
            &kit::rounded(r, 2.0),
            Color {
                a: 0.8 * alpha,
                ..colour
            },
        );
    }
}

/// A glyph's path, the mockup's 24-unit drawings (`M`, `L`, `H`, `V`,
/// `Z`, and `A` for circles and arcs).
fn path_of(glyph: Glyph) -> &'static str {
    match glyph {
        Glyph::Close => "M6 6 L18 18 M18 6 L6 18",
        Glyph::Minimize => "M6 17 H18",
        Glyph::Maximize => "M4 4 H20 V20 H4 Z M4 8.5 H20",
        Glyph::Restore => "M7 7 H20 V20 H7 Z M4 16 V4 H16",
        Glyph::HalfLeft => "M4 4 H20 V20 H4 Z M12 4 V20 M7 7 H10 M7 10 H10",
        Glyph::HalfRight => "M4 4 H20 V20 H4 Z M12 4 V20 M14 7 H17 M14 10 H17",
        Glyph::Quarter => "M4 4 H20 V20 H4 Z M12 4 V20 M4 12 H20",
        Glyph::Arrow(Side4::Left) => "M20 12 H5 M9.5 7.5 L5 12 L9.5 16.5",
        Glyph::Arrow(Side4::Right) => "M4 12 H19 M14.5 7.5 L19 12 L14.5 16.5",
        Glyph::Arrow(Side4::Up) => "M12 20 V5 M7.5 9.5 L12 5 L16.5 9.5",
        Glyph::Arrow(Side4::Down) => "M12 4 V19 M7.5 14.5 L12 19 L16.5 14.5",
        Glyph::Switch => "M4 8 H18 M14.5 4.5 L18 8 L14.5 11.5 M20 16 H6 M9.5 12.5 L6 16 L9.5 19.5",
        Glyph::Send => "M3 6 H13 V18 H3 Z M13 12 H21 M18 9 L21 12 L18 15",
        Glyph::Desktops => "M3 7 H15 V17 H3 Z M7 4 H21 V14 H18",
        Glyph::Grid | Glyph::Everywhere => {
            "M4 4 H10 V10 H4 Z M14 4 H20 V10 H14 Z M4 14 H10 V20 H4 Z M14 14 H20 V20 H14 Z"
        }
        Glyph::Screen => "M3 5 H21 V16 H3 Z M8 20 H16 M12 16 V20",
        Glyph::Above => "M5 4 H19 M12 8.5 V20 M7.5 13 L12 8.5 L16.5 13",
        Glyph::Agent => {
            "M12 7 A6.5 6.5 0 1 0 12 20 A6.5 6.5 0 1 0 12 7 Z M6.5 4 A2.5 2.5 0 1 0 6.5 9 A2.5 2.5 0 1 0 6.5 4 Z M17.5 4 A2.5 2.5 0 1 0 17.5 9 A2.5 2.5 0 1 0 17.5 4 Z"
        }
        Glyph::Terminal => "M3 5 H21 V19 H3 Z M7 9 L10 12 L7 15 M12 15 H16",
        Glyph::Search => {
            "M10.5 4 A6.5 6.5 0 1 0 10.5 17 A6.5 6.5 0 1 0 10.5 4 Z M15.5 15.5 L20.5 20.5"
        }
        Glyph::Bar => "M4 4 H20 V20 H4 Z M15 4 V20",
        Glyph::Clipboard => "M8 4.5 H16 V7 H8 Z M6.5 5.5 H5 V21 H19 V5.5 H17.5",
        Glyph::Emoji => {
            "M12 3.5 A8.5 8.5 0 1 0 12 20.5 A8.5 8.5 0 1 0 12 3.5 Z M8.5 14.5 L12 16.5 L15.5 14.5 M9 9.5 V10.5 M15 9.5 V10.5"
        }
        Glyph::Meeting => "M3 7 H15 V17 H3 Z M15 10.5 L21 7.5 V16.5 L15 13.5",
        Glyph::Bell => {
            "M6 16.5 V11 A6 6 0 0 1 18 11 V16.5 L19.5 18 H4.5 Z M10 20.5 A2 2 0 0 0 14 20.5"
        }
        Glyph::Moon => "M19 14.5 A7.5 7.5 0 1 1 9.5 5 A6 6 0 0 0 19 14.5 Z",
        Glyph::Calendar => "M4 6 H20 V20 H4 Z M4 10 H20 M8 3.5 V7.5 M16 3.5 V7.5",
        Glyph::Task => "M4 4 H20 V20 H4 Z M8 12 L11 15 L16.5 9",
        Glyph::Sound => {
            "M4 9.5 H7.5 L12 5.5 V18.5 L7.5 14.5 H4 Z M15.5 9 A4 4 0 0 1 15.5 15 M18 6.5 A7.5 7.5 0 0 1 18 17.5"
        }
        Glyph::Wifi => {
            "M3 9 A13 13 0 0 1 21 9 M6 12.5 A8.5 8.5 0 0 1 18 12.5 M9 16 A4 4 0 0 1 15 16 M12 19.5 V19.6"
        }
        Glyph::Brightness => {
            "M12 8 A4 4 0 1 0 12 16 A4 4 0 1 0 12 8 Z M12 2.5 V5 M12 19 V21.5 M2.5 12 H5 M19 12 H21.5 M5.3 5.3 L7 7 M17 17 L18.7 18.7 M5.3 18.7 L7 17 M17 7 L18.7 5.3"
        }
        Glyph::Camera => {
            "M3 7.5 H8 L10 5 H14 L16 7.5 H21 V19 H3 Z M12 10 A3.5 3.5 0 1 0 12 17 A3.5 3.5 0 1 0 12 10 Z"
        }
        Glyph::Record => {
            "M12 3.5 A8.5 8.5 0 1 0 12 20.5 A8.5 8.5 0 1 0 12 3.5 Z M12 9 A3 3 0 1 0 12 15 A3 3 0 1 0 12 9 Z"
        }
        Glyph::Magnifier => {
            "M10.5 4 A6.5 6.5 0 1 0 10.5 17 A6.5 6.5 0 1 0 10.5 4 Z M15.5 15.5 L20.5 20.5 M8 10.5 H13 M10.5 8 V13"
        }
        Glyph::Speech => {
            "M4 9.5 H7.5 L12 5.5 V18.5 L7.5 14.5 H4 Z M16 8 L20 8 M16 12 H21 M16 16 H20"
        }
        Glyph::Contrast => "M12 3.5 A8.5 8.5 0 1 0 12 20.5 A8.5 8.5 0 1 0 12 3.5 Z M12 3.5 V20.5",
        Glyph::Motion => "M3 12 H7 L9.5 6 L14.5 18 L17 12 H21",
        Glyph::Keyboard => "M3 6 H21 V18 H3 Z M7 10 H7.1 M11 10 H11.1 M15 10 H15.1 M8 14.5 H16",
        Glyph::Lock => "M6 11 H18 V20.5 H6 Z M8.5 11 V8 A3.5 3.5 0 0 1 15.5 8 V11",
        Glyph::LogOut => "M10 4 H5 V20 H10 M14 8 L18 12 L14 16 M18 12 H9",
        Glyph::Restart => "M19.5 12 A7.5 7.5 0 1 1 17 6.4 M17 3 V6.5 H13.5",
        Glyph::Power => "M12 3 V11 M7 6.5 A7.5 7.5 0 1 0 17 6.5",
        Glyph::Gear => {
            "M12 9 A3 3 0 1 0 12 15 A3 3 0 1 0 12 9 Z M12 2.5 V5 M12 19 V21.5 M2.5 12 H5 M19 12 H21.5 M5.3 5.3 L7 7 M17 17 L18.7 18.7 M5.3 18.7 L7 17 M17 7 L18.7 5.3"
        }
    }
}

/// A 24-unit path drawn `size` px at `at`, stroked `width` units.
fn glyph_path(f: &mut Frame, d: &str, at: Point, size: f32, width: f32, colour: Color) {
    let handle = glyph_svg(d, width);
    f.draw_svg(
        Rectangle::new(at, Size::new(size, size)),
        svg::Svg::new(handle)
            .color(Color { a: 1.0, ..colour })
            .opacity(colour.a),
    );
}

/// A path's SVG, white (tinted as drawn), made once.
fn glyph_svg(d: &str, width: f32) -> svg::Handle {
    static MADE: OnceLock<Mutex<HashMap<(String, u32), svg::Handle>>> = OnceLock::new();
    let made = MADE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut made = made.lock().unwrap_or_else(PoisonError::into_inner);
    made.entry((d.to_owned(), (width * 10.0) as u32))
        .or_insert_with(|| {
            svg::Handle::from_memory(format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"24\" height=\"24\" viewBox=\"0 0 24 24\" \
                 fill=\"none\" stroke=\"#ffffff\" stroke-width=\"{width}\" stroke-linecap=\"round\" \
                 stroke-linejoin=\"round\"><path d=\"{d}\"/></svg>"
            )
            .into_bytes())
        })
        .clone()
}

/// An agent's panda for its state, in the theme's dress.
fn panda(state: &str) -> svg::Handle {
    crate::pandas::still(crate::pandas::mood(crate::agents::Look::of(state)))
}

fn inset(rect: Rectangle, by: f32) -> Rectangle {
    Rectangle::new(
        Point::new(rect.x + by, rect.y + by),
        Size::new(rect.width - 2.0 * by, rect.height - 2.0 * by),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(n: usize) -> Vec<Row> {
        let def = catalogue::find("close").expect("close");
        let mut rows = vec![Row::Head("Fenêtre".into())];
        rows.extend((0..n).map(|_| {
            Row::Action(super::super::list::ActionRow {
                def,
                arg: None,
                label: "Fermer".into(),
                marks: Vec::new(),
                keyword: None,
                inert: None,
                on: None,
            })
        }));
        rows
    }

    #[test]
    fn nine_rows_room_at_most_and_a_row_revealed_with_its_heading() {
        // A heading and three rows: as tall as they are.
        let short = rows(3);
        let (w, h) = size(&short, 720.0);
        assert_eq!(w, 720);
        let list = HEAD + 3.0 * ROW + 3.0 * ROW_GAP;
        assert_eq!(
            h,
            (2.0 * IN + FIELD + LIST_GAP + list + 2.0 * FOOT_GAP + 1.0 + FOOT_LINE + 2.0).ceil()
                as u32
        );
        // Many: the whole rows that fit in nine rows' room.
        let long = rows(40);
        let most =
            (2.0 * IN + FIELD + LIST_GAP + MOST_LIST + 2.0 * FOOT_GAP + 1.0 + FOOT_LINE + 2.0)
                .ceil() as u32;
        assert!(size(&long, 720.0).1 <= most);
        // The first row with its heading; another alone.
        assert_eq!(span(&long, 1), (0.0, HEAD + ROW_GAP + ROW));
        assert_eq!(
            span(&long, 3),
            (HEAD + ROW_GAP + 2.0 * (ROW + ROW_GAP), ROW)
        );
        // A narrow work area: 32 px less.
        assert_eq!(width(600.0), 568.0);
        assert_eq!(width(3440.0), 720.0);
    }

    #[test]
    fn marks_cut_a_label_in_runs() {
        assert_eq!(
            runs("Bureau de gauche", &[0, 1, 2]),
            [
                ("Bur".to_owned(), true),
                ("eau de gauche".to_owned(), false)
            ]
        );
    }
}
