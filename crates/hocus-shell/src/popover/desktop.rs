//! A desktop's list of windows (bar.md §4.4, §9; the 3.9b mockup), opened
//! by resting on its tile (or its dot) for 150 ms: "Bureau N" and "M
//! fenêtres" on the right; one 32 px row per window, front to back, its
//! application's icon and its title cut with an ellipsis; "Aucune
//! fenêtre." on an empty desktop. 240 px wide, its top on the tiles'.
//!
//! A click on a row (or `Enter`, `Space` on the one holding the keyboard)
//! goes to that desktop and focuses that window, and the list closes.
//! `Tab`, `Shift+Tab` and the arrows go through the rows once it holds the
//! keyboard (clicked); `Esc` closes it. It follows windows opening,
//! closing and retitling while it shows: the compositor's lines draw this
//! surface alone ([`crate::desktops`]).

use std::time::Instant;

use iced::{
    Point, Rectangle, Renderer, Size, Theme, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry},
        text::Alignment,
    },
    window,
};

use super::{
    Effect, Event as Seen, Item, Pressed, event,
    kit::{self, FILL, Input, Pointer, SMALL_RADIUS, text, text_in},
};
use crate::{
    desktops::{self, Row},
    levels::{
        draw::{FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, ring},
        model::Words,
    },
};

// --- The mockup's sizes, at ×1 ------------------------------------------------

/// Its width, narrower than the other popovers'.
pub const WIDTH: u32 = 240;
const BORDER: f32 = 1.0;
const PAD_TOP: f32 = 10.0;
const PAD_SIDE: f32 = 12.0;
const PAD_BOTTOM: f32 = 12.0;
const LEFT: f32 = BORDER + PAD_SIDE;
const RIGHT: f32 = WIDTH as f32 - BORDER - PAD_SIDE;
/// The header: "Bureau N" 14 px 700, "M fenêtres" 12 px; 26 px high, 4 px
/// above what follows.
const HEADER_TOP: f32 = BORDER + PAD_TOP;
const HEADER: f32 = 26.0;
const HEADER_MIDDLE: f32 = HEADER_TOP + HEADER / 2.0;
const TITLE: f32 = 14.0;
const COUNT: f32 = 12.0;
const GAP: f32 = 4.0;
const CONTENT_TOP: f32 = HEADER_TOP + HEADER + GAP;
/// "Aucune fenêtre.": 13 px, 2 px above and under; then the rows' (empty)
/// place, 4 px under it, as the mockup's column has it.
const EMPTY: f32 = 13.0;
const EMPTY_PAD: f32 = 2.0;
/// The rows: 32 px, 2 px apart, reaching 6 px past the content's sides;
/// their icon 16 px 6 px in, 9 px before their title (13.5 px).
const ROW: f32 = 32.0;
const ROW_GAP: f32 = 2.0;
const ROW_OUT: f32 = 6.0;
const ROW_PAD: f32 = 6.0;
const ICON: f32 = 16.0;
const ICON_RADIUS: f32 = 4.0;
const ICON_GAP: f32 = 9.0;
const NAME: f32 = 13.5;
/// An icon's letter, when the application has none.
const LETTER: f32 = 9.0;
/// The room a title has: from after the icon to the row's padding.
const TITLE_ROOM: f32 = RIGHT + ROW_OUT - ROW_PAD - (LEFT - ROW_OUT + ROW_PAD + ICON + ICON_GAP);

/// Where its content ends, `rows` windows long.
fn end(rows: usize) -> f32 {
    match rows {
        0 => CONTENT_TOP + EMPTY_PAD + EMPTY * LINE + EMPTY_PAD + GAP,
        n => CONTENT_TOP + n as f32 * ROW + (n - 1) as f32 * ROW_GAP,
    }
}

/// Its height for `rows` windows.
fn height_of(rows: usize) -> u32 {
    (end(rows) + PAD_BOTTOM + BORDER).round() as u32
}

/// Its height for desktop `index` of bar `bar` as it is now.
pub fn height(bar: &str, index: u8) -> u32 {
    height_of(desktops::rows(bar, usize::from(index)).len())
}

/// Row `i`'s place.
fn row_rect(i: usize) -> Rectangle {
    Rectangle::new(
        Point::new(LEFT - ROW_OUT, CONTENT_TOP + i as f32 * (ROW + ROW_GAP)),
        Size::new(RIGHT - LEFT + 2.0 * ROW_OUT, ROW),
    )
}

/// A desktop's list on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    /// Its surface: it reacts only while open, not while it fades out.
    pub id: window::Id,
    pub words: Words,
    /// On the compositor's glass (else opaque).
    pub glass: bool,
}

/// What it shows: whose, its rows and their titles as cut.
#[derive(Debug, Clone, PartialEq)]
struct Shown {
    bar: String,
    index: u8,
    rows: Vec<Row>,
    titles: Vec<String>,
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    shown: Option<Shown>,
    /// The windows' ids under the pointer and pressed.
    pointer: Pointer<u64>,
    focus: Option<u64>,
}

impl State {
    fn ids(&self) -> Vec<u64> {
        self.shown
            .as_ref()
            .map_or_else(Vec::new, |s| s.rows.iter().map(|r| r.id).collect())
    }

    fn rect_of(&self, id: u64) -> Option<Rectangle> {
        let i = self.ids().iter().position(|i| *i == id)?;
        Some(row_rect(i))
    }
}

impl<Message> canvas::Program<Message> for Panel {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match kit::input(self.id, event, bounds, cursor, &mut state.pointer) {
            Input::Redraw(now) => self.refresh(state, now),
            Input::Mouse(seen, at) => self.mouse(state, seen, at),
            Input::Key(pressed) => self.key(state, &pressed),
            Input::Nothing => None,
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
        kit::background(&mut frame, bounds.size(), self.glass);
        let Some(shown) = &state.shown else {
            return vec![frame.into_geometry()];
        };
        let f = &mut frame;
        f.fill_text(text_in(
            MANROPE_BOLD,
            text(
                self.words.desktop_title(usize::from(shown.index) + 1),
                Point::new(LEFT, HEADER_MIDDLE),
                TITLE,
                TEXT,
                Alignment::Left,
            ),
        ));
        if let Some(count) = self.words.windows(shown.rows.len()) {
            f.fill_text(text(
                count,
                Point::new(RIGHT, HEADER_MIDDLE),
                COUNT,
                FAINT,
                Alignment::Right,
            ));
        }
        if shown.rows.is_empty() {
            f.fill_text(text(
                self.words.no_window().to_owned(),
                Point::new(LEFT, CONTENT_TOP + EMPTY_PAD + EMPTY * LINE / 2.0),
                EMPTY,
                FAINT,
                Alignment::Left,
            ));
        }
        for (i, (row, title)) in shown.rows.iter().zip(&shown.titles).enumerate() {
            let rect = row_rect(i);
            if state.pointer.hover == Some(row.id) {
                f.fill(&kit::rounded(rect, SMALL_RADIUS), FILL);
            }
            let icon = Rectangle::new(
                Point::new(rect.x + ROW_PAD, rect.center_y() - ICON / 2.0),
                Size::new(ICON, ICON),
            );
            let found = desktops::icons::of(&row.app);
            let name = if row.app.is_empty() {
                row.title.as_str()
            } else {
                letter_of(&row.app)
            };
            kit::app_tile(f, icon, (found.as_ref(), name), (ICON_RADIUS, LETTER), 1.0);
            f.fill_text(text(
                title.clone(),
                Point::new(icon.x + ICON + ICON_GAP, rect.center_y()),
                NAME,
                TEXT,
                Alignment::Left,
            ));
        }
        if let Some(rect) = state.focus.and_then(|id| state.rect_of(id)) {
            ring(f, rect, SMALL_RADIUS);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        state.pointer.interaction(false)
    }
}

/// What an application's letter tile is named by: its id's last part
/// (`org.gnome.Nautilus`'s `Nautilus`), else what is given.
fn letter_of(name: &str) -> &str {
    name.rsplit('.')
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or(name)
}

impl Panel {
    /// What it shows, read afresh: whose list (the controller says), its
    /// rows and their titles, cut to their room once.
    fn refresh<Message>(
        &self,
        state: &mut State,
        _now: Instant,
    ) -> Option<canvas::Action<Message>> {
        for pressed in kit::keys(self.id) {
            let _: Option<canvas::Action<Message>> = self.key(state, &pressed);
        }
        // Closing, it shows what it showed.
        let (bar, index) = super::with(|c| match c.open_on() {
            Some((bar, Item::Desktop(index))) if c.is_open(self.id) => {
                Some((bar.to_owned(), index))
            }
            _ => None,
        })?;
        let rows = desktops::rows(&bar, usize::from(index));
        let same = state
            .shown
            .as_ref()
            .is_some_and(|s| s.bar == bar && s.index == index && s.rows == rows);
        if same {
            return None;
        }
        let titles = match iced::advanced::graphics::text::font_system().write() {
            Ok(mut system) => rows
                .iter()
                .map(|r| {
                    kit::elide(&r.title, TITLE_ROOM, |t| {
                        kit::measure_in(system.raw(), t, NAME, MANROPE)
                    })
                })
                .collect(),
            Err(_) => rows.iter().map(|r| r.title.clone()).collect(),
        };
        tracing::debug!(
            bar = %bar,
            title = %self.words.desktop_title(usize::from(index) + 1),
            count = %self.words.windows(rows.len()).unwrap_or_default(),
            empty = ?rows.is_empty().then(|| self.words.no_window()),
            rows = ?titles,
            "the desktop's list shows it"
        );
        state.shown = Some(Shown {
            bar,
            index,
            rows,
            titles,
        });
        state.pointer.hover = None;
        if state.focus.is_some_and(|f| !state.ids().contains(&f)) {
            state.focus = None;
        }
        // A change of size reaches the controller, which resizes the
        // surface (without waking it again).
        super::refit(Item::Desktop(index));
        None
    }

    /// Window `id`: its desktop, and the focus; the list closes.
    fn bring(id: u64) {
        tracing::debug!(window = id, "a desktop's list brings a window");
        super::send(vec![Effect::Bring(id)]);
        event(Seen::Chosen);
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let target = at.and_then(|at| {
            state
                .ids()
                .into_iter()
                .enumerate()
                .find(|(i, _)| row_rect(*i).contains(at))
                .map(|(_, id)| id)
        });
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        match seen {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                state.pointer.moved();
                state.pointer.over(target).then_some(redraw).flatten()
            }
            mouse::Event::CursorLeft => {
                state.pointer.left();
                redraw
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                state.focus = None;
                state.pointer.pressed = target;
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if let Some(id) = state.pointer.release(target) {
                    Self::bring(id);
                }
                redraw
            }
            _ => None,
        }
    }

    fn key<Message>(
        &self,
        state: &mut State,
        pressed: &Pressed,
    ) -> Option<canvas::Action<Message>> {
        use keyboard::{Key, key::Named};
        let order = state.ids();
        match &pressed.key {
            Key::Named(Named::Escape) => {
                event(Seen::Escape);
                return Some(canvas::Action::capture());
            }
            Key::Named(Named::Tab) => {
                state.focus = kit::step(&order, state.focus, pressed.modifiers.shift());
            }
            Key::Named(arrow @ (Named::ArrowDown | Named::ArrowUp)) => {
                let down = *arrow == Named::ArrowDown;
                state.focus = match state.focus {
                    Some(focus) => kit::cycle(&order, focus, down),
                    None => kit::step(&order, None, !down),
                };
            }
            Key::Named(Named::Enter | Named::Space) => {
                if let Some(id) = state.focus {
                    Self::bring(id);
                }
            }
            _ => return None,
        }
        Some(canvas::Action::request_redraw().and_capture())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_height_follows_its_rows() {
        // The mockup's: 10 + 26 + 4 inside the rim, 12 under; 32 px rows
        // 2 px apart.
        assert_eq!(height_of(1), 1 + 10 + 26 + 4 + 32 + 12 + 1);
        assert_eq!(height_of(3), 1 + 10 + 26 + 4 + 3 * 32 + 2 * 2 + 12 + 1);
        // Empty: its line, and the rows' empty place under it.
        assert_eq!(height_of(0), 80);
        assert_eq!(TITLE_ROOM, 189.0);
    }
}
