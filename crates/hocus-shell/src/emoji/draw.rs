//! The emoji picker's surface (5.7): the mockup's panel × [`ZOOM`] (the
//! launcher's, 5.1b), logical pixels, and what the pointer, the wheel and
//! the keys do on it.
//!
//! In the mockup's sizes: 336 wide, 12 inside, its parts 8 apart, radius
//! 14. The top line (36): the search (its green border, radius 10) and the
//! 32 px round button of the skin tone; under it when it is open, the six
//! tones (22 px rounds, 6 apart, on the right); the tabs (30, one per
//! group, 2 apart, radius 8, their emoji 16 px, the lit one on
//! `rgba(255,255,255,0.10)`); the grid (252): headings 22 tall (10.5 px
//! 700 capitals spaced 0.08 em, `#7C8698`, 4 px in) and rows of eight
//! 38 px cells (radius 9, their emoji 22 px), whole rows from the first
//! one scrolled to; the footer under a line, 30 tall 6 under it: the
//! selected emoji (20 px), its name (12.5 px, `#C9D0DC`) and "Entrée
//! insère" (10.5 px, `#6A7384`) on the right.

use iced::{
    Color, Font, Point, Rectangle, Renderer, Size, Theme, alignment, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke},
        text::{Alignment, LineHeight, Shaping},
    },
    window,
};

use super::{Grid, Heading, Open, Row, Words, data};
use crate::theme::Ink;
use crate::{
    levels::draw::{DIM, MANROPE, MANROPE_BOLD, TEXT, rgb},
    popover::kit::{self, Wheel},
};

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
const RIM: Ink = Ink::Dark(rgba(255, 255, 255, 0.10));
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

/// The last row it may be scrolled to: the one showing the grid's end.
pub fn most_offset(grid: &Grid, _tones_open: bool) -> usize {
    let mut offset = grid.rows.len();
    let mut used = 0.0;
    while offset > 0 {
        let h = height_of(&grid.rows[offset - 1]);
        if used + h > GRID + 0.01 {
            break;
        }
        used += h;
        offset -= 1;
    }
    offset
}

/// The first row to show so that place `selected` shows, from `offset`:
/// with its heading when it comes right after it.
pub fn keep_in_view(grid: &Grid, selected: usize, offset: usize, tones_open: bool) -> usize {
    let Some((row, _)) = grid.place(selected) else {
        return offset;
    };
    let mut offset = offset.min(most_offset(grid, tones_open));
    if row < offset {
        offset = if row > 0 && matches!(grid.rows[row - 1], Row::Head(_)) {
            row - 1
        } else {
            row
        };
    }
    while end_of(grid, offset) <= row && offset < row {
        offset += 1;
    }
    offset
}

/// What the pointer can be over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Field,
    ToneButton,
    Tone(usize),
    Tab(usize),
    Cell(usize),
}

/// Where everything is.
#[derive(Debug)]
pub struct Layout {
    pub field: Rectangle,
    pub tone_button: Rectangle,
    pub tones: Vec<Rectangle>,
    pub tabs: Vec<(Heading, Rectangle)>,
    pub grid: Rectangle,
    pub heads: Vec<(Heading, Rectangle)>,
    pub cells: Vec<(usize, Rectangle)>,
    pub footer: f32,
}

impl Layout {
    pub fn of(open: &Open) -> Self {
        let field = Rectangle::new(
            Point::new(LEFT, TOP),
            Size::new(RIGHT - LEFT - GAP - TONE_BUTTON, TOP_LINE),
        );
        let tone_button = Rectangle::new(
            Point::new(RIGHT - TONE_BUTTON, TOP + (TOP_LINE - TONE_BUTTON) / 2.0),
            Size::new(TONE_BUTTON, TONE_BUTTON),
        );
        let mut y = TOP + TOP_LINE + GAP;
        let mut tones = Vec::new();
        if open.tones_open {
            let n = data::TONES.len();
            for i in 0..n {
                let x = RIGHT - (n - i) as f32 * TONES - (n - 1 - i) as f32 * TONE_GAP;
                tones.push(Rectangle::new(Point::new(x, y), Size::new(TONES, TONES)));
            }
            y += TONES + GAP;
        }
        let headings = Heading::tabs();
        let n = headings.len() as f32;
        let tab_width = (RIGHT - LEFT - (n - 1.0) * TAB_GAP) / n;
        let tabs = headings
            .into_iter()
            .enumerate()
            .map(|(i, h)| {
                let x = LEFT + i as f32 * (tab_width + TAB_GAP);
                (
                    h,
                    Rectangle::new(Point::new(x, y), Size::new(tab_width, TABS)),
                )
            })
            .collect();
        y += TABS + GAP;
        let grid = Rectangle::new(Point::new(LEFT, y), Size::new(RIGHT - LEFT, GRID));
        let mut heads = Vec::new();
        let mut cells = Vec::new();
        let mut row_y = y;
        for row in
            &open.grid.rows[open.offset.min(open.grid.rows.len())..end_of(&open.grid, open.offset)]
        {
            match row {
                Row::Head(h) => heads.push((
                    *h,
                    Rectangle::new(Point::new(LEFT, row_y), Size::new(RIGHT - LEFT, HEAD)),
                )),
                Row::Cells(range) => {
                    for (column, at) in range.clone().enumerate() {
                        cells.push((
                            at,
                            Rectangle::new(
                                Point::new(LEFT + column as f32 * CELL, row_y),
                                Size::new(CELL, CELL),
                            ),
                        ));
                    }
                }
            }
            row_y += height_of(row);
        }
        Self {
            field,
            tone_button,
            tones,
            tabs,
            grid,
            heads,
            cells,
            footer: y + GRID + GAP,
        }
    }

    pub fn target_at(&self, at: Point) -> Option<Target> {
        if let Some((place, _)) = self.cells.iter().find(|(_, r)| r.contains(at)) {
            return Some(Target::Cell(*place));
        }
        if let Some(i) = self.tabs.iter().position(|(_, r)| r.contains(at)) {
            return Some(Target::Tab(i));
        }
        if let Some(i) = self.tones.iter().position(|r| r.contains(at)) {
            return Some(Target::Tone(i));
        }
        if self.tone_button.contains(at) {
            return Some(Target::ToneButton);
        }
        self.field.contains(at).then_some(Target::Field)
    }
}

/// The picker on its surface.
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
    shift: bool,
    /// The query and how it shows, and where the caret is after it.
    field: (String, String, f32),
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
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.shift = modifiers.shift();
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
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
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
        self.background(&mut frame, bounds.size());
        super::with(|p| {
            if let Some(open) = p.open_on(self.id) {
                self.draw_open(&mut frame, state, open);
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
            Some(Target::Field) => mouse::Interaction::Text,
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

    /// Before it draws: the field's text fitted, and what it shows logged
    /// when it changed.
    fn prepare(&self, state: &mut State) {
        let shown = super::with(|p| p.open_on(self.id).map(|o| (o.query.clone(), self.said(o))));
        let Some((query, said)) = shown else {
            return;
        };
        if state.field.0 != query || (state.field.1.is_empty() && !query.is_empty()) {
            let room = RIGHT - LEFT - GAP - TONE_BUTTON - 2.0 * FIELD_PAD - MAGNIFIER - FIELD_GAP;
            let width = |t: &str| kit::measure(t, QUERY, MANROPE);
            let shown = tail(&query, room, width);
            let caret = if query.is_empty() { 0.0 } else { width(&shown) };
            state.field = (query, shown, caret);
        }
        if said != state.logged {
            tracing::debug!(said = %said, "the emoji picker shows it");
            state.logged = said;
        }
    }

    /// What it shows, in words, for the log (and the tests).
    fn said(&self, open: &Open) -> String {
        let layout = Layout::of(open);
        let mut rows: Vec<String> = Vec::new();
        let mut last_y = f32::NAN;
        for (at, rect) in &layout.cells {
            if rect.y != last_y {
                rows.push(String::new());
                last_y = rect.y;
            }
            let Some((_, text)) = open.emoji(*at) else {
                continue;
            };
            if let Some(row) = rows.last_mut() {
                if !row.is_empty() {
                    row.push(' ');
                }
                if *at == open.selected {
                    row.push('*');
                }
                row.push_str(&text);
            }
        }
        let heads: Vec<&str> = layout
            .heads
            .iter()
            .map(|(h, _)| self.words.heading(*h))
            .collect();
        let lit = if open.query.is_empty() {
            open.grid.lit(open.offset).map(|h| self.words.heading(h))
        } else {
            None
        };
        let footer = open
            .emoji(open.selected)
            .map(|(e, text)| format!("{text} {}", e.name(self.words.lang)))
            .unwrap_or_default();
        format!(
            "query={:?} place={},{} tone={} tones={} lit={:?} heads={heads:?} rows={rows:?} \
             footer={footer:?} empty={} holding={}",
            open.query,
            open.place.0,
            open.place.1,
            super::store::kept().tone,
            open.tones_open,
            lit.unwrap_or_default(),
            open.grid.flat.is_empty(),
            open.holding,
        )
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
            Act(usize, bool),
            Tab(Heading),
            Tones(Option<usize>),
        }
        let then = super::with(|p| {
            let open = p.open_mut(self.id)?;
            open.pointer = at;
            let layout = Layout::of(open);
            let target = at.and_then(|at| layout.target_at(at));
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
                        Some(Target::Cell(place)) => Then::Act(place, state.shift),
                        Some(Target::Tab(i)) => match Heading::tabs().get(i) {
                            Some(h) => Then::Tab(*h),
                            None => Then::Nothing,
                        },
                        Some(Target::ToneButton) => Then::Tones(None),
                        Some(Target::Tone(i)) => Then::Tones(Some(i)),
                        Some(Target::Field) | None => Then::Nothing,
                    }
                }
                mouse::Event::WheelScrolled { delta } => {
                    let notches = state.wheel.notches(delta, open.scale);
                    if notches == 0 {
                        return Some(Then::Nothing);
                    }
                    let most = most_offset(&open.grid, open.tones_open) as i64;
                    open.offset = (open.offset as i64 - i64::from(notches)).clamp(0, most) as usize;
                    let layout = Layout::of(open);
                    let target = open.pointer.and_then(|at| layout.target_at(at));
                    hovered(open, target);
                    Then::Draw
                }
                _ => Then::Nothing,
            })
        });
        /// The pointer over `target`: an emoji it is over is selected.
        fn hovered(open: &mut Open, target: Option<Target>) -> Then {
            if open.hover == target {
                return Then::Nothing;
            }
            open.hover = target;
            if let Some(Target::Cell(place)) = target {
                open.selected = place;
            }
            Then::Draw
        }
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        match then? {
            Then::Nothing => None,
            Then::Draw => redraw,
            Then::Act(place, keep) => {
                super::act(self.id, place, keep);
                redraw
            }
            Then::Tab(heading) => {
                super::tab(self.id, heading);
                redraw
            }
            Then::Tones(tone) => {
                super::tones(self.id, tone);
                redraw
            }
        }
    }

    fn draw_open(&self, f: &mut Frame, state: &State, open: &Open) {
        let layout = Layout::of(open);
        let tone = super::store::kept().tone;
        self.field(f, state, open, &layout);
        // The tone's button: its colour in a round.
        let b = layout.tone_button;
        f.fill(&Path::circle(b.center(), b.width / 2.0), FIELD_FILL.get());
        if open.hover == Some(Target::ToneButton) {
            crate::theme::hover(f, &Path::circle(b.center(), b.width / 2.0), HOVER.get());
        }
        f.stroke(
            &Path::circle(b.center(), b.width / 2.0 - 0.5),
            Stroke::default()
                .with_color(BUTTON_RIM.get())
                .with_width(1.0),
        );
        f.fill(
            &Path::circle(b.center(), TONE_DOT / 2.0),
            TONE_COLOURS.get(tone).copied().unwrap_or(TONE_COLOURS[0]),
        );
        for (i, rect) in layout.tones.iter().enumerate() {
            f.fill(
                &Path::circle(rect.center(), rect.width / 2.0),
                TONE_COLOURS[i],
            );
            if i == tone {
                f.stroke(
                    &Path::circle(rect.center(), rect.width / 2.0 - TONE_RING / 2.0),
                    Stroke::default()
                        .with_color(GREEN.get())
                        .with_width(TONE_RING),
                );
            }
        }
        let lit = if open.query.is_empty() {
            open.grid.lit(open.offset)
        } else {
            None
        };
        for (i, (heading, rect)) in layout.tabs.iter().enumerate() {
            if Some(*heading) == lit {
                f.fill(&kit::rounded(*rect, TAB_RADIUS), TAB_LIT.get());
            } else if open.hover == Some(Target::Tab(i)) {
                crate::theme::hover(f, &kit::rounded(*rect, TAB_RADIUS), HOVER.get());
            }
            let dim = lit.is_some() && Some(*heading) != lit;
            emoji(
                f,
                heading.icon(),
                rect.center(),
                TAB_EMOJI,
                if dim { 0.7 } else { 1.0 },
            );
        }
        for (heading, rect) in &layout.heads {
            head(f, self.words.heading(*heading), *rect);
        }
        for (at, rect) in &layout.cells {
            let Some((_, text)) = open.emoji(*at) else {
                continue;
            };
            if *at == open.selected {
                f.fill(&kit::rounded(*rect, CELL_RADIUS), SELECTED_FILL.get());
                f.stroke(
                    &kit::rounded(inset(*rect, 0.5), CELL_RADIUS - 0.5),
                    Stroke::default()
                        .with_color(SELECTED_RIM.get())
                        .with_width(1.0),
                );
            }
            emoji(f, &text, rect.center(), CELL_EMOJI, 1.0);
        }
        if open.grid.flat.is_empty() {
            f.fill_text(kit::text(
                self.words.none(open.query.trim()),
                Point::new(
                    layout.grid.center_x(),
                    layout.grid.y + EMPTY_TOP + EMPTY * crate::levels::draw::LINE / 2.0,
                ),
                EMPTY,
                FAINT.get(),
                Alignment::Center,
            ));
        }
        // The footer: a line, the selected emoji, its name, the key.
        f.fill_rectangle(
            Point::new(LEFT, layout.footer),
            Size::new(RIGHT - LEFT, 1.0),
            RULE.get(),
        );
        let middle = layout.footer + 1.0 + FOOT_PAD + FOOT / 2.0;
        let hint = self.words.inserts();
        let hint_width = kit::measure(hint, FOOT_HINT, MANROPE);
        f.fill_text(kit::text(
            hint.to_owned(),
            Point::new(RIGHT, middle),
            FOOT_HINT,
            HINT.get(),
            Alignment::Right,
        ));
        if let Some((e, text)) = open.emoji(open.selected) {
            emoji(
                f,
                &text,
                Point::new(LEFT + FOOT_EMOJI / 2.0 + 1.0, middle),
                FOOT_EMOJI,
                1.0,
            );
            let x = LEFT + FOOT_EMOJI + z(8.0);
            let room = (RIGHT - hint_width - z(8.0) - x).max(0.0);
            let name = kit::elide(e.name(self.words.lang), room, |t| {
                kit::measure(t, FOOT_NAME, MANROPE)
            });
            f.fill_text(kit::text(
                name,
                Point::new(x, middle),
                FOOT_NAME,
                NAME.get(),
                Alignment::Left,
            ));
        }
    }

    fn field(&self, f: &mut Frame, state: &State, open: &Open, layout: &Layout) {
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
            Point::new(field.x + FIELD_PAD, field.center_y() - MAGNIFIER / 2.0),
        );
        let x = field.x + FIELD_PAD + MAGNIFIER + FIELD_GAP;
        let (query, shown, caret) = &state.field;
        let (content, colour) = if open.query.is_empty() || query.is_empty() {
            (self.words.placeholder().to_owned(), QUIET)
        } else {
            (shown.clone(), TEXT)
        };
        f.fill_text(kit::text(
            content,
            Point::new(x, field.center_y()),
            QUERY,
            colour.get(),
            Alignment::Left,
        ));
        if open.holding {
            let caret_x = if open.query.is_empty() {
                x - z(2.0)
            } else {
                x + caret + z(1.0)
            };
            f.fill_rectangle(
                Point::new(caret_x, field.center_y() - QUERY * 0.6),
                Size::new(1.0, QUERY * 1.2),
                TEXT.get(),
            );
        }
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

/// A heading, its letters in capitals, spaced, in `rect`.
fn head(f: &mut Frame, label: &str, rect: Rectangle) {
    let mut x = rect.x + HEAD_LEFT;
    for ch in label.to_uppercase().chars() {
        let letter = ch.to_string();
        let width = kit::measure(&letter, HEAD_TEXT, MANROPE_BOLD);
        f.fill_text(kit::text_in(
            MANROPE_BOLD,
            kit::text(
                letter,
                Point::new(x, rect.center_y()),
                HEAD_TEXT,
                QUIET.get(),
                Alignment::Left,
            ),
        ));
        x += width + HEAD_SPACING;
    }
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

    #[test]
    fn the_panel_is_the_mockups_zoomed() {
        // 336 × 405 in the mockup, × 1.5; 30 more with the tones.
        assert_eq!(size(false), (504, 606));
        assert_eq!(size(true), (504, 651));
        // Eight cells fit across.
        const { assert!(8.0 * CELL <= RIGHT - LEFT) };
    }

    #[test]
    fn the_grid_shows_whole_rows_and_keeps_the_selection_in_view() {
        let table = data::table();
        let grid = Grid::of(table, "", &[]);
        // A heading and the rest cells: 33 + 6 × 57 = 375 of 378.
        assert_eq!(end_of(&grid, 0), 7);
        let far = grid.flat.len() - 1;
        let offset = keep_in_view(&grid, far, 0, false);
        assert_eq!(offset, most_offset(&grid, false));
        assert!(end_of(&grid, offset) == grid.rows.len());
        // Back to the first: with its heading.
        assert_eq!(keep_in_view(&grid, 0, offset, false), 0);
    }
}
