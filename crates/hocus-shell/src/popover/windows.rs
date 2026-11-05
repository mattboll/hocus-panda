//! The mini form's windows' popover (#604): the windows of the bar's
//! screen's current desktop as the full form lists them over its tiles,
//! up to 3 lines (30 px: the icon, the title, the ✕) and "+ N autres"; a
//! 150 ms hover or a click on the mini item opens it, 240 px wide, the
//! popovers' glass, its bottom on the item's.
//!
//! A click on a line focuses and raises its window and the popover closes;
//! its ✕ asks the window to close, the popover staying (it follows the
//! windows as they go); "+ N" gives way to the current desktop's list.
//! `Tab`, `Shift+Tab` and the arrows go through the lines, their ✕ and
//! "+ N" once it holds the keyboard; `Enter` or `Space` acts, `Esc`
//! closes it.

use std::time::Instant;

use hocus_link::ToCompositor;
use iced::{
    Point, Rectangle, Renderer, Theme, keyboard, mouse,
    widget::canvas::{self, Frame, Geometry},
    window,
};

use super::{
    Effect, Event as Seen, Item, Pressed, event,
    kit::{self, Input, Pointer},
};
use crate::{
    levels::draw::ring,
    windows::{
        self, Row, Words,
        draw::{Lit, OUT, RADIUS, cut, draw_line, draw_more, row_items, row_rects},
    },
};

pub const WIDTH: u32 = 240;
const BORDER: f32 = 1.0;
const PAD: f32 = 8.0;
/// The lines' sides: the popover's 12 px padding, 6 px past it.
const SIDE: f32 = BORDER + 12.0 - OUT;
const TOP: f32 = BORDER + PAD;

/// What it lists for bar `bar`: the group's rows, as many as there are.
fn rows_of(bar: &str) -> Vec<Row> {
    windows::fit(&windows::lines(bar), f32::INFINITY)
}

/// Its height for `rows`.
fn height_of(rows: &[Row]) -> u32 {
    (TOP + windows::rows_height(rows) + PAD + BORDER).round() as u32
}

/// Its height for bar `bar`'s windows now.
pub fn height(bar: &str) -> u32 {
    height_of(&rows_of(bar))
}

fn rects(rows: &[Row]) -> Vec<Rectangle> {
    row_rects(rows, SIDE, TOP, WIDTH as f32 - 2.0 * SIDE)
}

/// The windows' popover on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    pub id: window::Id,
    pub words: Words,
    pub glass: bool,
}

#[derive(Debug, Default)]
pub struct State {
    bar: Option<String>,
    rows: Vec<Row>,
    titles: Vec<String>,
    pointer: Pointer<Item>,
    focus: Option<Item>,
}

impl State {
    fn items(&self) -> Vec<(Item, Rectangle)> {
        row_items(&self.rows, &rects(&self.rows))
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
        let f = &mut frame;
        let rects = rects(&state.rows);
        for ((row, rect), title) in state.rows.iter().zip(&rects).zip(&state.titles) {
            match row {
                Row::Window(line) => {
                    let lit = Lit {
                        line: state.pointer.hover == Some(Item::Window(line.id)),
                        close: state.pointer.hover == Some(Item::WindowClose(line.id)),
                    };
                    draw_line(f, *rect, line, title, lit, 1.0);
                }
                Row::More(_) => draw_more(
                    f,
                    *rect,
                    title.clone(),
                    state.pointer.hover == Some(Item::WindowsMore),
                    1.0,
                ),
            }
        }
        if let Some((item, rect)) = state
            .focus
            .and_then(|focus| state.items().into_iter().find(|(i, _)| *i == focus))
        {
            let radius = if matches!(item, Item::WindowClose(_)) {
                6.0
            } else {
                RADIUS
            };
            ring(f, rect, radius);
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

impl Panel {
    /// What it shows, read afresh (closing, what it showed).
    fn refresh<Message>(
        &self,
        state: &mut State,
        _now: Instant,
    ) -> Option<canvas::Action<Message>> {
        for pressed in kit::keys(self.id) {
            let _: Option<canvas::Action<Message>> = self.key(state, &pressed);
        }
        let bar = super::with(|c| match c.open_on() {
            Some((bar, Item::Windows)) if c.is_open(self.id) => Some(bar.to_owned()),
            _ => None,
        })?;
        let rows = rows_of(&bar);
        if state.bar.as_deref() == Some(bar.as_str()) && state.rows == rows {
            return None;
        }
        let width = WIDTH as f32 - 2.0 * SIDE;
        state.titles = rows
            .iter()
            .map(|row| match row {
                Row::Window(line) => cut(&line.title, width).0,
                Row::More(n) => self.words.more(*n),
            })
            .collect();
        tracing::debug!(
            bar = %bar,
            shows = %self.words.said(&rows),
            "the windows' popover shows it"
        );
        state.bar = Some(bar);
        state.rows = rows;
        let items: Vec<Item> = state.items().into_iter().map(|(i, _)| i).collect();
        if state.focus.is_some_and(|f| !items.contains(&f)) {
            state.focus = None;
        }
        state.pointer.hover = None;
        super::refit(Item::Windows);
        None
    }

    /// What `item` does: a line brings its window and the popover closes;
    /// a ✕ asks its window to close; "+ N", the desktop's list.
    fn act(item: Item) {
        match item {
            Item::Window(id) => {
                tracing::debug!(window = id, "the windows' popover brings a window");
                super::send(vec![Effect::Bring(id)]);
                event(Seen::Chosen);
            }
            Item::WindowClose(id) => {
                tracing::debug!(window = id, "the windows' popover asks a window to close");
                super::send(vec![Effect::Tell(ToCompositor::Close(id))]);
            }
            Item::WindowsMore => event(Seen::Replace(Item::WindowsMore)),
            _ => {}
        }
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let target =
            at.and_then(|at| crate::windows::draw::over(&state.items(), at).map(|(i, _)| i));
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
                if let Some(item) = state.pointer.release(target) {
                    Self::act(item);
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
        let order: Vec<Item> = state.items().into_iter().map(|(i, _)| i).collect();
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
                if let Some(item) = state.focus {
                    Self::act(item);
                }
            }
            _ => return None,
        }
        Some(canvas::Action::request_redraw().and_capture())
    }
}

#[cfg(test)]
mod tests {
    use iced::Size;

    use super::*;
    use crate::windows::Line;

    fn line(id: u64) -> Row {
        Row::Window(Line {
            id,
            app: String::new(),
            title: format!("w{id}"),
            focused: false,
            minimized: false,
        })
    }

    #[test]
    fn its_height_is_its_lines_and_plus_n_in_its_padding() {
        assert_eq!(height_of(&[line(1), line(2)]), 1 + 8 + 60 + 8 + 1);
        assert_eq!(
            height_of(&[line(1), line(2), line(3), Row::More(2)]),
            1 + 8 + 110 + 8 + 1
        );
        // The lines span the popover less 7 px each side.
        let r = rects(&[line(1)]);
        assert_eq!(
            r[0],
            Rectangle::new(Point::new(7.0, 9.0), Size::new(226.0, 30.0))
        );
    }
}
