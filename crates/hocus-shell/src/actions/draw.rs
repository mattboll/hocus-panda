//! The actions palette's surface (#736, docs/design/actions.md §2): the
//! mockup's sizes, logical pixels, and what the pointer and the wheel do
//! on it.
//!
//! 720 wide (narrower screens: the work area less 32), radius 16, the
//! popovers' glass, a 1 px rim, 10 px in. The field (56 tall, radius 12,
//! its green border): the prompt, the query or the placeholder, the
//! target's chip and "Échap" on its right; in a step, the action's chip
//! before the query. 8 px under it, the list: headings 24 tall (11 px 800
//! capitals spaced 0.09 em), rows 44 tall (radius 10), 2 px apart, nine
//! rows' room at most, then the footer under a line.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock, PoisonError},
};

use hocus_link::actions::{self as catalogue, Glyph, Inert, Side4, Step};
use iced::{
    Color, Font, Point, Rectangle, Renderer, Size, Theme,
    advanced::svg,
    alignment, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke},
        text::{Alignment, LineHeight, Shaping},
    },
    window,
};

use super::{
    Open, Words,
    list::{Elsewhere, Row, Target},
};
use crate::theme::Ink;
use crate::{
    levels::draw::{LINE, MANROPE, MANROPE_BOLD, TEXT, rgb},
    popover::kit::{self, MANROPE_SEMIBOLD, Wheel},
};

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
const RIM: Ink = Ink::Dark(rgba(255, 255, 255, 0.12));
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

/// The row after the last one shown from `offset`, within nine rows'
/// room.
fn end_of(rows: &[Row], offset: usize) -> usize {
    let mut used = 0.0;
    let mut end = offset;
    while let Some(row) = rows.get(end) {
        let h = height_of(row) + if end > offset { ROW_GAP } else { 0.0 };
        if used + h > MOST_LIST + 0.01 {
            break;
        }
        used += h;
        end += 1;
    }
    end
}

/// The list's height from `offset`.
fn list_height(rows: &[Row], offset: usize) -> f32 {
    let end = end_of(rows, offset);
    let shown = &rows[offset.min(rows.len())..end];
    let gaps = shown.len().saturating_sub(1) as f32 * ROW_GAP;
    shown.iter().map(height_of).sum::<f32>() + gaps
}

/// The surface's size for `rows` from `offset`, `width` wide: as tall as
/// the whole rows shown, nine rows' room at most.
pub fn size(rows: &[Row], offset: usize, width: f32) -> (u32, u32) {
    let list = list_height(rows, offset);
    let height = IN + FIELD + LIST_GAP + list + FOOT_GAP + 1.0 + FOOT_GAP + FOOT_LINE + 2.0 + IN;
    (width.round() as u32, height.ceil() as u32)
}

/// The last row it may be scrolled to.
fn most_offset(rows: &[Row]) -> usize {
    let mut offset = rows.len();
    let mut used = 0.0;
    while offset > 0 {
        let h = height_of(&rows[offset - 1]) + if offset < rows.len() { ROW_GAP } else { 0.0 };
        if used + h > MOST_LIST + 0.01 {
            break;
        }
        used += h;
        offset -= 1;
    }
    offset
}

/// The first row to show so that row `selected` shows, from `offset`: with
/// its heading when it comes right after it.
pub fn keep_in_view(rows: &[Row], selected: usize, offset: usize) -> usize {
    let mut offset = offset.min(most_offset(rows));
    if selected < offset {
        offset = if selected > 0 && matches!(rows[selected - 1], Row::Head(_)) {
            selected - 1
        } else {
            selected
        };
    }
    while end_of(rows, offset) <= selected && offset < selected {
        offset += 1;
    }
    offset
}

/// Where everything is.
#[derive(Debug)]
pub struct Layout {
    pub field: Rectangle,
    pub list: Rectangle,
    /// The rows shown, by their index, and where.
    pub rows: Vec<(usize, Rectangle)>,
    pub footer: f32,
    pub width: f32,
}

impl Layout {
    pub fn of(open: &Open) -> Self {
        let width = open.size.0 as f32;
        let right = width - IN;
        let field = Rectangle::new(Point::new(IN, IN), Size::new(right - IN, FIELD));
        let top = IN + FIELD + LIST_GAP;
        let height = list_height(&open.rows, open.offset);
        let list = Rectangle::new(Point::new(IN, top), Size::new(right - IN, height));
        let mut y = top;
        let mut rows = Vec::new();
        let offset = open.offset.min(open.rows.len());
        for (i, row) in open.rows[offset..end_of(&open.rows, offset)]
            .iter()
            .enumerate()
        {
            let h = height_of(row);
            rows.push((
                offset + i,
                Rectangle::new(Point::new(IN, y), Size::new(right - IN, h)),
            ));
            y += h + ROW_GAP;
        }
        Self {
            field,
            list,
            rows,
            footer: top + height + FOOT_GAP,
            width,
        }
    }

    /// The row under `at`, if one can be chosen there.
    pub fn row_at(&self, open: &Open, at: Point) -> Option<usize> {
        self.rows
            .iter()
            .find(|(i, r)| r.contains(at) && open.rows.get(*i).is_some_and(Row::selectable))
            .map(|(i, _)| *i)
    }
}

/// The palette on its surface.
#[derive(Debug, Clone)]
pub struct View {
    pub id: window::Id,
    pub words: Words,
    /// The compositor blurs behind: the tint over it, else opaque.
    pub glass: bool,
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    wheel: Wheel,
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
        match event {
            iced::Event::Window(window::Event::RedrawRequested(_)) => {
                self.prepare(state);
                None
            }
            iced::Event::Mouse(seen) => {
                let at = crate::levels::draw::pointer(
                    seen,
                    bounds,
                    cursor,
                    super::with(|p| p.open_on(self.id).and_then(|o| o.pointer)),
                );
                self.mouse(state, seen, at)
            }
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
        _state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        self.background(&mut frame, bounds.size());
        super::with(|p| {
            if let Some(open) = p.open_on(self.id) {
                self.draw_open(&mut frame, open);
            }
        });
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        super::with(|p| match p.open_on(self.id).and_then(|o| o.hover) {
            Some(_) => mouse::Interaction::Pointer,
            None => mouse::Interaction::default(),
        })
    }
}

impl View {
    fn background(&self, f: &mut Frame, size: Size) {
        let (top, bottom) = if self.glass {
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
    }

    /// Before it draws: what it shows logged when it changed, and said to
    /// screen readers.
    fn prepare(&self, state: &mut State) {
        let said = super::with(|p| p.open_on(self.id).map(|o| said(o, self.words)));
        let Some(said) = said else {
            return;
        };
        if said != state.logged {
            tracing::debug!(said = %said, "the actions palette shows it");
            state.logged = said;
        }
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        enum Then {
            Nothing,
            Draw,
            Act(usize),
        }
        let then = super::with(|p| {
            let open = p.open_mut(self.id)?;
            open.pointer = at;
            let layout = Layout::of(open);
            let target = at.and_then(|at| layout.row_at(open, at));
            Some(match seen {
                mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                    hovered(open, target)
                }
                mouse::Event::CursorLeft => {
                    open.hover = None;
                    Then::Draw
                }
                mouse::Event::ButtonPressed(mouse::Button::Left) => {
                    open.pressed = target;
                    Then::Nothing
                }
                mouse::Event::ButtonReleased(mouse::Button::Left) => {
                    match open.pressed.take().filter(|p| Some(*p) == target) {
                        Some(row) => Then::Act(row),
                        None => Then::Nothing,
                    }
                }
                mouse::Event::WheelScrolled { delta } => {
                    let notches = state.wheel.notches(delta, open.scale);
                    if notches == 0 {
                        return Some(Then::Nothing);
                    }
                    let most = most_offset(&open.rows) as i64;
                    open.offset = (open.offset as i64 - i64::from(notches)).clamp(0, most) as usize;
                    let layout = Layout::of(open);
                    let target = open.pointer.and_then(|at| layout.row_at(open, at));
                    hovered(open, target);
                    if let Some(resize) = open.refit() {
                        crate::popover::send(vec![resize]);
                    }
                    Then::Draw
                }
                _ => Then::Nothing,
            })
        });
        /// The pointer over row `target`: it is selected.
        fn hovered(open: &mut Open, target: Option<usize>) -> Then {
            if open.hover == target {
                return Then::Nothing;
            }
            open.hover = target;
            if let Some(row) = target {
                open.selected = row;
            }
            Then::Draw
        }
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        match then? {
            Then::Nothing => None,
            Then::Draw => redraw,
            Then::Act(row) => {
                super::act(self.id, row);
                redraw
            }
        }
    }

    fn draw_open(&self, f: &mut Frame, open: &Open) {
        let layout = Layout::of(open);
        self.field(f, open, &layout);
        for (i, rect) in &layout.rows {
            let Some(row) = open.rows.get(*i) else {
                continue;
            };
            let selected = *i == open.selected && row.selectable();
            self.row(f, open, row, *rect, selected);
        }
        // The scroll mark, when the rows are more than shown.
        let total = open.rows.len();
        let shown = layout.rows.len();
        if shown < total {
            let list = layout.list;
            let share = shown as f32 / total as f32;
            let start = open.offset as f32 / total as f32;
            f.fill(
                &kit::rounded(
                    Rectangle::new(
                        Point::new(layout.width - BORDER - 5.0, list.y + start * list.height),
                        Size::new(MARK_WIDTH, (share * list.height).max(12.0)),
                    ),
                    MARK_WIDTH / 2.0,
                ),
                MARK.get(),
            );
        }
        // The footer: a line, then what matches and the keys.
        f.fill_rectangle(
            Point::new(IN, layout.footer),
            Size::new(layout.width - 2.0 * IN, 1.0),
            RULE.get(),
        );
        let matched = if open.query.trim().is_empty() && open.step.is_none() {
            super::all()
        } else {
            open.matched()
        };
        let text = self
            .words
            .footer(matched, open.step.is_some(), open.query.trim().is_empty());
        f.fill_text(kit::text(
            kit::elide(&text, layout.width - 2.0 * IN - 2.0 * FOOT_LEFT, |t| {
                kit::measure(t, FOOT, MANROPE)
            }),
            Point::new(
                IN + FOOT_LEFT,
                layout.footer + 1.0 + FOOT_GAP + FOOT_LINE / 2.0,
            ),
            FOOT,
            QUIET.get(),
            Alignment::Left,
        ));
    }

    fn field(&self, f: &mut Frame, open: &Open, layout: &Layout) {
        let field = layout.field;
        f.fill(&kit::rounded(field, FIELD_RADIUS), FIELD_FILL.get());
        f.stroke(
            &kit::rounded(inset(field, 0.5), FIELD_RADIUS - 0.5),
            Stroke::default()
                .with_color(FIELD_RIM.get())
                .with_width(1.0),
        );
        let middle = field.center_y();
        glyph_path(
            f,
            "M5 7 L10 12 L5 17 M12 17 H19",
            Point::new(field.x + FIELD_LEFT, middle - PROMPT / 2.0),
            PROMPT,
            2.2,
            PROMPT_INK.get(),
        );
        // On the right: "Échap", and before it the target's chip.
        let mut right = field.x + field.width - FIELD_RIGHT;
        let hint = self.words.escape();
        let hint_width = kit::measure(hint, HINT, MANROPE_SEMIBOLD) + 2.0 * HINT_PAD.width;
        let hint_rect = Rectangle::new(
            Point::new(
                right - hint_width,
                middle - (HINT + 2.0 * HINT_PAD.height + 2.0) / 2.0,
            ),
            Size::new(hint_width, HINT + 2.0 * HINT_PAD.height + 2.0),
        );
        f.stroke(
            &kit::rounded(inset(hint_rect, 0.5), HINT_RADIUS),
            Stroke::default().with_color(HINT_RIM.get()).with_width(1.0),
        );
        f.fill_text(kit::text_in(
            MANROPE_SEMIBOLD,
            kit::text(
                hint.to_owned(),
                hint_rect.center(),
                HINT,
                QUIET.get(),
                Alignment::Center,
            ),
        ));
        right = hint_rect.x - FIELD_GAP;
        let left = field.x + FIELD_LEFT + PROMPT + FIELD_GAP;
        if !open.shown.title.is_empty() {
            let room = ((right - left) * 0.45).max(60.0);
            let title = kit::elide(&open.shown.title, room - 2.0 * CHIP_PAD.width - 19.0, |t| {
                kit::measure(t, CHIP_TEXT, MANROPE_SEMIBOLD)
            });
            let w = kit::measure(&title, CHIP_TEXT, MANROPE_SEMIBOLD) + 2.0 * CHIP_PAD.width + 19.0;
            let h = CHIP_TEXT * LINE + 2.0 * CHIP_PAD.height + 2.0;
            let chip = Rectangle::new(Point::new(right - w, middle - h / 2.0), Size::new(w, h));
            f.fill(&kit::rounded(chip, CHIP_RADIUS), CHIP_FILL.get());
            f.stroke(
                &kit::rounded(inset(chip, 0.5), CHIP_RADIUS - 0.5),
                Stroke::default().with_color(CHIP_RIM.get()).with_width(1.0),
            );
            // A window's glyph: 12 × 9, a 1.6 px green rim.
            let mark = Rectangle::new(
                Point::new(chip.x + CHIP_PAD.width, middle - 4.5),
                Size::new(12.0, 9.0),
            );
            f.stroke(
                &kit::rounded(mark, 2.0),
                Stroke::default().with_color(GREEN.get()).with_width(1.6),
            );
            f.fill_text(kit::text_in(
                MANROPE_SEMIBOLD,
                kit::text(
                    title,
                    Point::new(mark.x + 12.0 + 7.0, middle),
                    CHIP_TEXT,
                    CHIP_INK.get(),
                    Alignment::Left,
                ),
            ));
            right = chip.x - FIELD_GAP;
        }
        let mut x = left;
        // In a step, the action's chip first.
        if let Some(step) = &open.step {
            let said = format!("{} ›", step.def.stem(self.words.fr));
            let w = kit::measure(&said, STEP_CHIP_TEXT, MANROPE_BOLD) + 2.0 * STEP_CHIP_PAD.width;
            let h = STEP_CHIP_TEXT * LINE + 2.0 * STEP_CHIP_PAD.height;
            let chip = Rectangle::new(Point::new(x, middle - h / 2.0), Size::new(w, h));
            f.fill(&kit::rounded(chip, CHIP_RADIUS), GREEN.get());
            f.fill_text(kit::text_in(
                MANROPE_BOLD,
                kit::text(
                    said,
                    chip.center(),
                    STEP_CHIP_TEXT,
                    ON_GREEN.get(),
                    Alignment::Center,
                ),
            ));
            x = chip.x + w + 8.0;
        }
        let room = (right - x).max(0.0);
        let caret = |f: &mut Frame, at: f32| {
            f.fill_rectangle(
                Point::new(at, middle - 12.0),
                Size::new(2.0, 24.0),
                GREEN.get(),
            );
        };
        if open.query.is_empty() {
            caret(f, x);
            let placeholder = match &open.step {
                Some(step) => self
                    .words
                    .step_placeholder(step.def.step.unwrap_or(Step::Desktop)),
                None => self.words.placeholder(),
            };
            f.fill_text(kit::text(
                kit::elide(placeholder, room - 8.0, |t| {
                    kit::measure(t, PLACEHOLDER, MANROPE)
                }),
                Point::new(x + 8.0, middle),
                PLACEHOLDER,
                QUIET.get(),
                Alignment::Left,
            ));
        } else {
            let shown = tail(&open.query, room - 6.0, |t| kit::measure(t, QUERY, MANROPE));
            let w = kit::measure(&shown, QUERY, MANROPE);
            f.fill_text(kit::text(
                shown,
                Point::new(x, middle),
                QUERY,
                TEXT.get(),
                Alignment::Left,
            ));
            caret(f, x + w + 2.0);
        }
    }

    fn row(&self, f: &mut Frame, open: &Open, row: &Row, rect: Rectangle, selected: bool) {
        let words = self.words;
        match row {
            Row::Head(label) => {
                head(f, label, rect);
                return;
            }
            Row::Nothing(said) => {
                f.fill_text(kit::text(
                    said.clone(),
                    rect.center(),
                    NOTHING,
                    FAINT.get(),
                    Alignment::Center,
                ));
                return;
            }
            _ => {}
        }
        let alpha = if row.inert().is_some() { INERT } else { 1.0 };
        let ink = |ink: Ink| ink.scale_alpha(alpha);
        if selected {
            f.fill(&kit::rounded(rect, ROW_RADIUS), ink(SELECTED_FILL));
            f.stroke(
                &kit::rounded(inset(rect, 0.5), ROW_RADIUS - 0.5),
                Stroke::default()
                    .with_color(ink(SELECTED_RIM))
                    .with_width(1.0),
            );
        } else if open.hover.is_some_and(|h| open.rows.get(h) == Some(row)) {
            f.fill(&kit::rounded(rect, ROW_RADIUS), ink(HOVER));
        }
        let middle = rect.center_y();
        let mut x = rect.x + ROW_LEFT;
        // The lead: a tile with a glyph, a desktop's sketch, a panda.
        match row {
            Row::Target(t) => match &t.target {
                Target::Desktop { windows, index } => {
                    // The desktop its screen shows, ringed.
                    let current = desks_current(open) == Some(*index);
                    sketch(
                        f,
                        Point::new(x, middle - SKETCH.height / 2.0),
                        windows,
                        current,
                        alpha,
                    );
                    x += SKETCH.width;
                }
                Target::Agent { state, .. } => {
                    let tile =
                        Rectangle::new(Point::new(x, middle - TILE / 2.0), Size::new(TILE, TILE));
                    let fill = if state == "waiting" {
                        WAITING_TILE
                    } else {
                        TILE_FILL
                    };
                    f.fill(&kit::rounded(tile, TILE_RADIUS), ink(fill));
                    let p = Rectangle::new(
                        Point::new(tile.center_x() - PANDA / 2.0, tile.center_y() - PANDA / 2.0),
                        Size::new(PANDA, PANDA),
                    );
                    f.draw_svg(p, svg::Svg::new(panda(state).clone()).opacity(alpha));
                    x += TILE;
                }
            },
            Row::Action(a) => {
                tile(f, Point::new(x, middle - TILE / 2.0), a.def.glyph, alpha);
                x += TILE;
            }
            Row::Elsewhere(to) => {
                let glyph = match to {
                    Elsewhere::Launcher => Glyph::Search,
                    Elsewhere::Settings => Glyph::Gear,
                };
                tile(f, Point::new(x, middle - TILE / 2.0), glyph, alpha);
                x += TILE;
            }
            Row::Head(_) | Row::Nothing(_) => {}
        }
        x += ROW_GAP_IN;
        // On the right: the shortcut, the switch's state, or the reason.
        let mut right = rect.x + rect.width - ROW_RIGHT;
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
        if let Some(why) = reason {
            let said = why.said(fr);
            let w = kit::measure(said, REASON, MANROPE_ITALIC);
            f.fill_text(kit::text_in(
                MANROPE_ITALIC,
                kit::text(
                    said.to_owned(),
                    Point::new(right, middle),
                    REASON,
                    REASON_INK.get(),
                    Alignment::Right,
                ),
            ));
            right -= w + ROW_GAP_IN;
        } else if let Some(on) = state {
            let said = words.state(on);
            let w = kit::measure(said, STATE, MANROPE_BOLD) + 2.0 * STATE_PAD.width;
            let h = STATE * LINE + 2.0 * STATE_PAD.height;
            let chip = Rectangle::new(Point::new(right - w, middle - h / 2.0), Size::new(w, h));
            f.fill(&kit::rounded(chip, 6.0), ink(STATE_FILL));
            f.fill_text(kit::text_in(
                MANROPE_BOLD,
                kit::text(
                    said.to_owned(),
                    chip.center(),
                    STATE,
                    if on { ink(GREEN) } else { ink(STATE_OFF) },
                    Alignment::Center,
                ),
            ));
            right = chip.x - ROW_GAP_IN;
        } else if !caps.is_empty() {
            let widths: Vec<f32> = caps
                .iter()
                .map(|c| (kit::measure(c, CAP, MONO) + 2.0 * CAP_PAD).max(CAP_LEAST))
                .collect();
            let total = widths.iter().sum::<f32>() + CAP_GAP * (widths.len() - 1) as f32;
            let mut cx = right - total;
            for (cap, w) in caps.iter().zip(&widths) {
                keycap(
                    f,
                    cap,
                    Rectangle::new(
                        Point::new(cx, middle - CAP_HEIGHT / 2.0),
                        Size::new(*w, CAP_HEIGHT),
                    ),
                    alpha,
                );
                cx += w + CAP_GAP;
            }
            right -= total + ROW_GAP_IN;
        }
        // The name, its marks, "›" for a step, the keyword; then its
        // category in what is left.
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
                words.elsewhere(*to, open.query.trim()),
                Vec::new(),
                false,
                None,
                words.elsewhere_line(*to).to_owned(),
            ),
            Row::Head(_) | Row::Nothing(_) => return,
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
        let label = if cut {
            kit::elide(&label, label_room, |t| {
                kit::measure(t, LABEL, MANROPE_SEMIBOLD)
            })
        } else {
            label
        };
        let mut lx = x;
        let marks = if cut { Vec::new() } else { marks };
        for (run, marked) in runs(&label, &marks) {
            let (font, colour) = if marked {
                (MANROPE_EXTRABOLD, ink(GREEN))
            } else {
                (MANROPE_SEMIBOLD, ink(TEXT))
            };
            f.fill_text(kit::text_in(
                font,
                kit::text(
                    run.clone(),
                    Point::new(lx, middle),
                    LABEL,
                    colour,
                    Alignment::Left,
                ),
            ));
            lx += kit::measure(&run, LABEL, font);
        }
        if step {
            lx += 8.0;
            f.fill_text(kit::text_in(
                MANROPE_BOLD,
                kit::text(
                    "›".to_owned(),
                    Point::new(lx, middle),
                    LABEL,
                    ink(QUIET),
                    Alignment::Left,
                ),
            ));
            lx += kit::measure("›", LABEL, MANROPE_BOLD);
        }
        if let Some(keyword) = keyword {
            lx += 8.0;
            f.fill_text(kit::text(
                keyword.clone(),
                Point::new(lx, middle),
                SUB,
                ink(QUIET),
                Alignment::Left,
            ));
            lx += kit::measure(&keyword, SUB, MANROPE);
        }
        let aside_x = lx + ROW_GAP_IN;
        let aside_room = right - aside_x;
        if aside_room > 24.0 && !aside.is_empty() {
            f.fill_text(kit::text(
                kit::elide(&aside, aside_room, |t| kit::measure(t, CATEGORY, MANROPE)),
                Point::new(aside_x, middle),
                CATEGORY,
                ink(QUIET),
                Alignment::Left,
            ));
        }
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
            words.elsewhere(*to, open.query.trim()),
            words.elsewhere_line(*to).to_owned(),
        ),
        Row::Head(h) => (h.clone(), String::new()),
        Row::Nothing(n) => (n.clone(), String::new()),
    }
}

/// What the palette shows, in words, for the log (and the tests).
pub fn said(open: &Open, words: Words) -> String {
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
                    format!("{selected}{}", words.elsewhere(*to, open.query.trim()))
                }
            }
        })
        .collect();
    format!(
        "query={:?} step={:?} target={:?} place={},{} size={}x{} rows={rows:?}",
        open.query,
        open.step.as_ref().map(|s| s.def.id),
        open.shown.title,
        open.place.0,
        open.place.1,
        open.size.0,
        open.size.1,
    )
}

/// A heading, its letters in capitals, spaced, in `rect`.
fn head(f: &mut Frame, label: &str, rect: Rectangle) {
    let mut x = rect.x + HEAD_LEFT;
    let y = rect.y + rect.height - 4.0 - HEAD_TEXT * LINE / 2.0;
    for ch in label.to_uppercase().chars() {
        let letter = ch.to_string();
        let width = kit::measure(&letter, HEAD_TEXT, MANROPE_EXTRABOLD);
        f.fill_text(kit::text_in(
            MANROPE_EXTRABOLD,
            kit::text(
                letter,
                Point::new(x, y),
                HEAD_TEXT,
                QUIET.get(),
                Alignment::Left,
            ),
        ));
        x += width + HEAD_SPACING;
    }
}

/// A keycap at `rect`.
fn keycap(f: &mut Frame, cap: &str, rect: Rectangle, alpha: f32) {
    f.fill(&kit::rounded(rect, CAP_RADIUS), CAP_FILL.scale_alpha(alpha));
    f.stroke(
        &kit::rounded(inset(rect, 0.5), CAP_RADIUS - 0.5),
        Stroke::default()
            .with_color(CAP_RIM.scale_alpha(alpha))
            .with_width(1.0),
    );
    // Its bottom 2 px.
    f.fill_rectangle(
        Point::new(rect.x + CAP_RADIUS / 2.0, rect.y + rect.height - 2.0),
        Size::new(rect.width - CAP_RADIUS, 1.0),
        CAP_RIM.scale_alpha(alpha),
    );
    f.fill_text(canvas::Text {
        content: cap.to_owned(),
        position: Point::new(rect.center_x(), rect.center_y()),
        max_width: f32::INFINITY,
        color: CAP_INK.scale_alpha(alpha),
        size: iced::Pixels(CAP),
        line_height: LineHeight::Relative(1.0),
        font: MONO,
        align_x: Alignment::Center,
        align_y: alignment::Vertical::Center,
        shaping: Shaping::Advanced,
    });
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

/// An agent's panda for its state.
fn panda(state: &str) -> &'static svg::Handle {
    static PANDAS: OnceLock<[svg::Handle; 5]> = OnceLock::new();
    let pandas = PANDAS.get_or_init(|| {
        [
            hocus_pandas::Mood::Waiting.still().as_bytes(),
            hocus_pandas::Mood::Stuck.still().as_bytes(),
            hocus_pandas::Mood::Done.still().as_bytes(),
            hocus_pandas::Mood::Working.still().as_bytes(),
            hocus_pandas::Mood::Resting.still().as_bytes(),
        ]
        .map(svg::Handle::from_memory)
    });
    &pandas[crate::agents::Look::of(state) as usize]
}

/// `query`'s end that fits `room`: what was typed last shows.
fn tail(query: &str, room: f32, mut width: impl FnMut(&str) -> f32) -> String {
    if width(query) <= room {
        return query.to_owned();
    }
    let chars: Vec<char> = query.chars().collect();
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
    fn nine_rows_room_at_most_the_selection_kept_in_view() {
        // A heading and three rows: as tall as they are.
        let short = rows(3);
        let (w, h) = size(&short, 0, 720.0);
        assert_eq!(w, 720);
        let list = HEAD + 3.0 * ROW + 3.0 * ROW_GAP;
        assert_eq!(
            h,
            (2.0 * IN + FIELD + LIST_GAP + list + 2.0 * FOOT_GAP + 1.0 + FOOT_LINE + 2.0).ceil()
                as u32
        );
        let long = rows(40);
        // Many: nine rows from wherever it is scrolled to.
        let tall = size(&long, 1, 720.0);
        assert_eq!(size(&long, 20, 720.0), tall);
        let most =
            (2.0 * IN + FIELD + LIST_GAP + MOST_LIST + 2.0 * FOOT_GAP + 1.0 + FOOT_LINE + 2.0)
                .ceil() as u32;
        assert_eq!(tall.1, most);
        assert!(size(&long, 0, 720.0).1 <= most);
        let offset = keep_in_view(&long, 30, 0);
        assert!(end_of(&long, offset) > 30 && offset > 0, "{offset}");
        assert_eq!(keep_in_view(&long, 1, offset), 0);
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
