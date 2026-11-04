//! The calendar drawn as laid out ([`super::layout`]), in the spec's
//! colours.

use iced::{
    Color, Pixels, Point, Rectangle, Size, alignment,
    widget::{
        canvas::{self, Frame, Path, Stroke, path::Arc},
        text::{Alignment, LineHeight, Shaping},
    },
};

use super::layout::{
    ALL_DAY_DOT, ALL_DAY_PAD, ALL_DAY_TITLE, BAR, Body, COUNT, DAY_NAME, DETAILS, DETAILS_LINE,
    DOT, END, EVENT_PAD_X, EVENT_PAD_Y, EVENT_TITLE, ICON, ICON_GAP, LEFT, LETTER, Laid, META,
    META_GAP, MONO_LINE, MARTIAN, NOTE, NUMBER, RELATIVE, RIGHT, Row, RowKind, SPAN, START,
    Stop, TIME_LINES_GAP, TITLE, TODAY_TEXT, Target, View, WEEK_NUMBER, WEEK_WORD,
};
use super::words::Words;
use crate::{
    clock::time::Lang,
    levels::draw::{ACCENT, DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, ring, rgb, stroke},
    popover::kit::{self, Clip, FILL, MANROPE_SEMIBOLD},
};

/// The arrows, and "Aujourd'hui", lit under the pointer.
const BUTTON_LIT: Color = Color::from_rgba8(255, 255, 255, 0.10);
const BUTTON_RADIUS: f32 = 7.0;
/// The arrows' chevrons: 16 px.
const CHEVRON: f32 = 16.0;
/// A day lit under the pointer; chosen; chosen and today.
const DAY_LIT: Color = Color::from_rgba8(255, 255, 255, 0.06);
const DAY_CHOSEN: Color = Color::from_rgba8(255, 255, 255, 0.16);
const TODAY_CHOSEN: Color = Color::from_rgba8(108, 194, 135, 0.24);
const DAY_RADIUS: f32 = 9.0;
/// Today's ring inside its button.
const TODAY_RING: f32 = 1.5;
/// The keyboard's outline: 2 px, 1 px outside, white.
const OUTLINE: f32 = 2.0;
const OUTLINE_OUT: f32 = 1.0;
const OUTLINE_COLOUR: Color = rgb(0xEEF1F6);
/// A day's dot, in the month and around it.
const DOT_IN: Color = Color::from_rgba8(238, 241, 246, 0.6);
const DOT_AROUND: Color = Color::from_rgba8(140, 150, 168, 0.6);
const WHITE: Color = Color::WHITE;
const RULE: Color = Color::from_rgba8(255, 255, 255, 0.08);
/// A row under the pointer; unfolded.
const ROW_LIT: Color = Color::from_rgba8(255, 255, 255, 0.06);
const ROW_UNFOLDED: Color = Color::from_rgba8(255, 255, 255, 0.05);
const ALL_DAY_RADIUS: f32 = 8.0;
const EVENT_RADIUS: f32 = 9.0;
/// An all-day row's tint: its calendar's colour at 14 %.
const TINT: f32 = 0x24 as f32 / 255.0;
/// A video call's address and its camera.
const VIDEO: Color = rgb(0xA9C4FF);
/// Today's events over: at half their opacity.
const PAST: f32 = 0.5;

/// The list's viewport on the popover, when the day has rows.
pub(super) fn list_view(laid: &Laid) -> Option<Rectangle> {
    match &laid.body {
        Body::Rows { list, .. } => Some(list.view),
        Body::Note { .. } => None,
    }
}

/// `content` at `size` in `font`, from `at` (`align` its side), its line
/// `line` em high and centred on `at.y`.
fn said(
    content: String,
    at: Point,
    (size, line): (f32, f32),
    font: iced::Font,
    color: Color,
    align: Alignment,
) -> canvas::Text {
    canvas::Text {
        content,
        position: at,
        max_width: f32::INFINITY,
        color,
        size: Pixels(size),
        line_height: LineHeight::Relative(line),
        font,
        align_x: align,
        align_y: alignment::Vertical::Center,
        shaping: Shaping::Advanced,
    }
}

/// `content` from its line's top at `at`.
fn from_top(mut text: canvas::Text, top: f32) -> canvas::Text {
    text.align_y = alignment::Vertical::Top;
    text.position.y = top;
    text
}

fn colour([r, g, b]: [u8; 3]) -> Color {
    Color::from_rgb8(r, g, b)
}

/// The calendar.
pub(super) fn calendar(
    f: &mut Frame,
    laid: &Laid,
    view: &View,
    hover: Option<Target>,
    clip: Clip,
) {
    header(f, laid, view, hover);
    month(f, laid, hover);
    f.fill_rectangle(Point::new(LEFT, laid.rule), Size::new(RIGHT - LEFT, 1.0), RULE);
    day_line(f, laid);
    match &laid.body {
        Body::Note { text, middle } => f.fill_text(said(
            text.clone(),
            Point::new((LEFT + RIGHT) / 2.0, *middle),
            (NOTE, LINE),
            MANROPE,
            DIM,
            Alignment::Center,
        )),
        Body::Rows { list, rows } => {
            let viewport = list.view;
            f.with_clip(viewport, |f| {
                for (i, row) in rows.iter().enumerate() {
                    let rect = laid.row_rect(*list, row);
                    if rect.intersection(&viewport).is_none() {
                        continue;
                    }
                    let lit = hover == Some(Target::Event(i));
                    let focused = view.focus == Some(Stop::Event(row.id.clone()));
                    event_row(f, row, rect, lit, focused, clip);
                }
            });
            if list.scrolls() {
                kit::List {
                    view: viewport,
                    content: list.content,
                }
                .scrollbar_at(f, laid.scroll, RIGHT + 3.0);
            }
        }
    }
}

fn header(f: &mut Frame, laid: &Laid, view: &View, hover: Option<Target>) {
    f.fill_text(said(
        laid.title.text.clone(),
        laid.title.at,
        (TITLE, LINE),
        MANROPE_BOLD,
        TEXT,
        Alignment::Left,
    ));
    let today = laid.today_button;
    f.fill(
        &kit::rounded(today, BUTTON_RADIUS),
        if hover == Some(Target::Today) {
            BUTTON_LIT
        } else {
            FILL
        },
    );
    f.fill_text(said(
        Words::new(Lang::of_messages())
            .today_button()
            .to_owned(),
        today.center(),
        (TODAY_TEXT, LINE),
        MANROPE_SEMIBOLD,
        if laid.today_faint { FAINT } else { TEXT },
        Alignment::Center,
    ));
    for (target, rect, back) in [
        (Target::Previous, laid.previous, true),
        (Target::Next, laid.next, false),
    ] {
        if hover == Some(target) {
            f.fill(&kit::rounded(rect, BUTTON_RADIUS), BUTTON_LIT);
        }
        chevron(f, rect.center(), back);
    }
    let focused = match view.focus {
        Some(Stop::Previous) => Some(laid.previous),
        Some(Stop::Next) => Some(laid.next),
        Some(Stop::Today) => Some(today),
        _ => None,
    };
    if let Some(rect) = focused {
        ring(f, rect, BUTTON_RADIUS);
    }
}

/// A 16 px chevron centred on `at`, pointing left (`back`) or right:
/// `M15 6 L9 12 L15 18` in 24.
fn chevron(f: &mut Frame, at: Point, back: bool) {
    let s = CHEVRON / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + (x - 12.0) * s, at.y + (y - 12.0) * s);
    let path = Path::new(|b| {
        if back {
            b.move_to(p(15.0, 6.0));
            b.line_to(p(9.0, 12.0));
            b.line_to(p(15.0, 18.0));
        } else {
            b.move_to(p(9.0, 6.0));
            b.line_to(p(15.0, 12.0));
            b.line_to(p(9.0, 18.0));
        }
    });
    f.stroke(&path, stroke(2.0 * s, DIM));
}

fn month(f: &mut Frame, laid: &Laid, hover: Option<Target>) {
    let words = Words::new(Lang::of_messages());
    f.fill_text(said(
        words.week().to_owned(),
        Point::new(laid.week_centre, laid.letters_middle),
        (WEEK_WORD, LINE),
        MANROPE_SEMIBOLD,
        FAINT,
        Alignment::Center,
    ));
    for (letter, x) in words.letters().into_iter().zip(laid.columns) {
        f.fill_text(said(
            letter.to_owned(),
            Point::new(x, laid.letters_middle),
            (LETTER, LINE),
            MANROPE_BOLD,
            FAINT,
            Alignment::Center,
        ));
    }
    for week in &laid.weeks {
        f.fill_text(said(
            week.number.to_string(),
            Point::new(laid.week_centre, week.middle),
            (WEEK_NUMBER, MONO_LINE),
            MARTIAN,
            FAINT,
            Alignment::Center,
        ));
        for cell in &week.days {
            let shape = kit::rounded(cell.rect, DAY_RADIUS);
            if cell.selected {
                f.fill(
                    &shape,
                    if cell.today {
                        TODAY_CHOSEN
                    } else {
                        DAY_CHOSEN
                    },
                );
            } else if hover == Some(Target::Day(cell.date)) {
                f.fill(&shape, DAY_LIT);
            }
            if cell.today {
                let inset = TODAY_RING / 2.0;
                let ring = Rectangle::new(
                    Point::new(cell.rect.x + inset, cell.rect.y + inset),
                    Size::new(cell.rect.width - TODAY_RING, cell.rect.height - TODAY_RING),
                );
                f.stroke(
                    &kit::rounded(ring, DAY_RADIUS - inset),
                    Stroke {
                        width: TODAY_RING,
                        ..stroke(TODAY_RING, ACCENT)
                    },
                );
            }
            let number = if cell.selected {
                WHITE
            } else if !cell.in_month {
                FAINT
            } else if cell.weekend {
                DIM
            } else {
                TEXT
            };
            let centre = cell.rect.center_x();
            f.fill_text(said(
                cell.date.day().to_string(),
                Point::new(centre, cell.number_middle),
                (NUMBER, 1.0),
                MARTIAN,
                number,
                Alignment::Center,
            ));
            if cell.busy {
                f.fill(
                    &Path::circle(Point::new(centre, cell.dot_middle), DOT / 2.0),
                    if cell.in_month { DOT_IN } else { DOT_AROUND },
                );
            }
            if cell.outlined {
                let out = OUTLINE_OUT + OUTLINE / 2.0;
                let outline = Rectangle::new(
                    Point::new(cell.rect.x - out, cell.rect.y - out),
                    Size::new(cell.rect.width + 2.0 * out, cell.rect.height + 2.0 * out),
                );
                f.stroke(
                    &kit::rounded(outline, DAY_RADIUS + out),
                    stroke(OUTLINE, OUTLINE_COLOUR),
                );
            }
        }
    }
}

fn day_line(f: &mut Frame, laid: &Laid) {
    let line = &laid.day_line;
    let top = |text: canvas::Text, at: Point| from_top(text, at.y);
    f.fill_text(top(
        said(
            line.title.text.clone(),
            line.title.at,
            (DAY_NAME, LINE),
            MANROPE_BOLD,
            TEXT,
            Alignment::Left,
        ),
        line.title.at,
    ));
    if let Some(relative) = &line.relative {
        f.fill_text(top(
            said(
                relative.text.clone(),
                relative.at,
                (RELATIVE, LINE),
                MANROPE,
                DIM,
                Alignment::Left,
            ),
            relative.at,
        ));
    }
    if let Some(count) = &line.count {
        f.fill_text(top(
            said(
                count.text.clone(),
                count.at,
                (COUNT, LINE),
                MANROPE,
                FAINT,
                Alignment::Right,
            ),
            count.at,
        ));
    }
}

/// An event's row at `rect` (on the popover): lit under the pointer,
/// greyed when over, its details under it when unfolded.
fn event_row(f: &mut Frame, row: &Row, rect: Rectangle, lit: bool, focused: bool, clip: Clip) {
    let alpha = if row.past { PAST } else { 1.0 };
    let fade = |c: Color| c.scale_alpha(alpha);
    let calendar = colour(row.colour);
    let x = rect.x;
    match &row.kind {
        RowKind::AllDay { span } => {
            let shape = kit::rounded(rect, ALL_DAY_RADIUS);
            f.fill(&shape, fade(calendar.scale_alpha(TINT)));
            if lit {
                f.fill(&shape, ROW_LIT);
            }
            let middle = rect.y + row.base / 2.0;
            f.fill(
                &Path::circle(
                    Point::new(x + ALL_DAY_PAD + ALL_DAY_DOT / 2.0, middle),
                    ALL_DAY_DOT / 2.0,
                ),
                fade(calendar),
            );
            clip.fill(
                f,
                said(
                    row.title.clone(),
                    Point::new(x + row.inset, middle),
                    (ALL_DAY_TITLE, LINE),
                    MANROPE_SEMIBOLD,
                    fade(TEXT),
                    Alignment::Left,
                ),
            );
            clip.fill(
                f,
                said(
                    span.clone(),
                    Point::new(rect.x + rect.width - ALL_DAY_PAD, middle),
                    (SPAN, LINE),
                    MANROPE,
                    fade(DIM),
                    Alignment::Right,
                ),
            );
        }
        RowKind::Timed { start, end, meta } => {
            let shape = kit::rounded(rect, EVENT_RADIUS);
            if !row.details.is_empty() {
                f.fill(&shape, ROW_UNFOLDED);
            }
            if lit {
                f.fill(&shape, ROW_LIT);
            }
            let top = rect.y + EVENT_PAD_Y;
            let left = x + EVENT_PAD_X;
            clip.fill(
                f,
                from_top(
                    said(
                        start.clone(),
                        Point::new(left, top),
                        (START, MONO_LINE),
                        MARTIAN,
                        fade(TEXT),
                        Alignment::Left,
                    ),
                    top,
                ),
            );
            let end_top = top + START * MONO_LINE + TIME_LINES_GAP;
            clip.fill(
                f,
                from_top(
                    said(
                        end.clone(),
                        Point::new(left, end_top),
                        (END, MONO_LINE),
                        MARTIAN,
                        fade(FAINT),
                        Alignment::Left,
                    ),
                    end_top,
                ),
            );
            // The bar, between the column and the title.
            let bar = Rectangle::new(
                Point::new(x + row.inset - 10.0 - BAR, top),
                Size::new(BAR, row.base - 2.0 * EVENT_PAD_Y),
            );
            f.fill(&kit::rounded(bar, BAR / 2.0), fade(calendar));
            let title_x = x + row.inset;
            clip.fill(
                f,
                from_top(
                    said(
                        row.title.clone(),
                        Point::new(title_x, top),
                        (EVENT_TITLE, LINE),
                        MANROPE_SEMIBOLD,
                        fade(TEXT),
                        Alignment::Left,
                    ),
                    top,
                ),
            );
            if let Some(meta) = meta {
                let middle = top + EVENT_TITLE * LINE + META_GAP + META * LINE / 2.0;
                let tint = if meta.video { VIDEO } else { DIM };
                let icon = Point::new(title_x + ICON / 2.0, middle);
                if meta.video {
                    camera(f, icon, fade(tint));
                } else {
                    pin(f, icon, fade(tint));
                }
                clip.fill(
                    f,
                    said(
                        meta.text.clone(),
                        Point::new(title_x + ICON + ICON_GAP, middle),
                        (META, LINE),
                        MANROPE,
                        fade(tint),
                        Alignment::Left,
                    ),
                );
            }
        }
    }
    if !row.details.is_empty() && matches!(row.kind, RowKind::AllDay { .. }) {
        let block = Rectangle::new(
            Point::new(rect.x, rect.y + row.base),
            Size::new(rect.width, row.height - row.base),
        );
        f.fill(&kit::rounded(block, ALL_DAY_RADIUS), ROW_UNFOLDED);
    }
    let details_top = rect.y + row.base - EVENT_PAD_Y + 2.0;
    for (i, line) in row.details.iter().enumerate() {
        let middle = details_top + (i as f32 + 0.5) * DETAILS * DETAILS_LINE;
        clip.fill(
            f,
            said(
                line.clone(),
                Point::new(x + row.inset, middle),
                (DETAILS, DETAILS_LINE),
                MANROPE,
                DIM,
                Alignment::Left,
            ),
        );
    }
    if focused {
        ring(
            f,
            rect,
            if matches!(row.kind, RowKind::AllDay { .. }) {
                ALL_DAY_RADIUS
            } else {
                EVENT_RADIUS
            },
        );
    }
}

/// The 13 px camera centred on `at`: `M3.5 7 H15 V17 H3.5 Z M15 10.5
/// L20.5 7.5 V16.5 L15 13.5` in 24.
fn camera(f: &mut Frame, at: Point, tint: Color) {
    let s = ICON / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + (x - 12.0) * s, at.y + (y - 12.0) * s);
    let path = Path::new(|b| {
        b.move_to(p(3.5, 7.0));
        b.line_to(p(15.0, 7.0));
        b.line_to(p(15.0, 17.0));
        b.line_to(p(3.5, 17.0));
        b.close();
        b.move_to(p(15.0, 10.5));
        b.line_to(p(20.5, 7.5));
        b.line_to(p(20.5, 16.5));
        b.line_to(p(15.0, 13.5));
    });
    f.stroke(&path, stroke(2.0 * s, tint));
}

/// The 13 px pin centred on `at`: a drop from `(12, 21)` round a 7 px
/// circle at `(12, 9.5)`, a 2 px dot in it, in 24.
fn pin(f: &mut Frame, at: Point, tint: Color) {
    let s = ICON / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + (x - 12.0) * s, at.y + (y - 12.0) * s);
    let path = Path::new(|b| {
        b.move_to(p(12.0, 21.0));
        b.bezier_curve_to(p(12.0, 21.0), p(5.0, 14.5), p(5.0, 9.5));
        b.arc(Arc {
            center: p(12.0, 9.5),
            radius: 7.0 * s,
            start_angle: iced::Radians(std::f32::consts::PI),
            end_angle: iced::Radians(2.0 * std::f32::consts::PI),
        });
        b.bezier_curve_to(p(19.0, 14.5), p(12.0, 21.0), p(12.0, 21.0));
        b.close();
        b.circle(p(12.0, 7.5), 2.0 * s);
    });
    f.stroke(&path, stroke(2.0 * s, tint));
}
