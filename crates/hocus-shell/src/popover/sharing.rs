//! The popover of the sign of what is in use (5.11): for each share, who
//! sees the screen ("Firefox voit ton écran"), what it sees ("Tu partages
//! la fenêtre « … »") and "Arrêter le partage", which ends its session
//! (the compositor's `share stop`); under the last one, how many
//! notifications wait for the end. 250 wide, 12 px in, its parts 8 px
//! apart, the shares 12 px apart.

use hocus_link::{ShareAsk, ToCompositor};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, mouse,
    widget::{
        canvas::{self, Frame, Geometry},
        text::Alignment,
    },
    window,
};

use super::{
    Effect, Event as Seen, Pressed, event,
    kit::{self, Input, Pointer, text, text_in},
};
use crate::{
    in_use::Words,
    levels::draw::{DIM, FAINT, LINE, MANROPE_BOLD, TEXT, rgb, ring},
};

pub const WIDTH: u32 = 250;
const PAD: f32 = 12.0;
const GAP: f32 = 8.0;
const SHARE_GAP: f32 = 12.0;
const BORDER: f32 = 1.0;
const TITLE: f32 = 13.5;
const SUB: f32 = 12.0;
const HELD: f32 = 11.5;
const BUTTON: f32 = 32.0;
const BUTTON_RADIUS: f32 = 9.0;
const BUTTON_TEXT: f32 = 12.5;
const BLUE: Color = rgb(0x5B8DEF);
const BLUE_HOVER: Color = rgb(0x6E9BF2);

/// A share's place in the popover: its id, and its lines' middles and its
/// button.
#[derive(Debug, Clone, PartialEq)]
struct Row {
    id: u64,
    title: f32,
    sub: f32,
    held: Option<f32>,
    button: Rectangle,
}

fn rows(ids: &[u64]) -> (Vec<Row>, f32) {
    let left = BORDER + PAD;
    let mut y = left;
    let mut rows = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            y += SHARE_GAP;
        }
        let title = y + TITLE * LINE / 2.0;
        y += TITLE * LINE + GAP;
        let sub = y + SUB * LINE / 2.0;
        y += SUB * LINE + GAP;
        let held = (i + 1 == ids.len()).then(|| {
            let middle = y + HELD * LINE / 2.0;
            y += HELD * LINE + GAP;
            middle
        });
        let button = Rectangle::new(
            Point::new(left, y),
            Size::new(WIDTH as f32 - 2.0 * left, BUTTON),
        );
        y += BUTTON;
        rows.push(Row {
            id: *id,
            title,
            sub,
            held,
            button,
        });
    }
    (rows, y + PAD + BORDER)
}

/// Its height, for the shares now.
pub fn height() -> u32 {
    let ids: Vec<u64> = crate::in_use::snapshot()
        .shares
        .iter()
        .map(|s| s.id)
        .collect();
    rows(&ids).1.round() as u32
}

/// The popover on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    pub id: window::Id,
    pub words: Words,
    pub glass: bool,
}

#[derive(Debug, Default)]
pub struct State {
    pointer: Pointer<u64>,
    focused: Option<u64>,
    said: Option<Vec<String>>,
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
            Input::Redraw(_) => {
                for pressed in kit::keys(self.id) {
                    let _: Option<canvas::Action<Message>> = Self::key(state, &pressed);
                }
                self.say(state);
                None
            }
            Input::Mouse(seen, at) => Self::mouse(state, seen, at),
            Input::Key(pressed) => Self::key(state, &pressed),
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
        let uses = crate::in_use::snapshot();
        let ids: Vec<u64> = uses.shares.iter().map(|s| s.id).collect();
        let (rows, _) = rows(&ids);
        let held = crate::notifications::with(|c| c.stack.held_while_sharing());
        let left = BORDER + PAD;
        let room = WIDTH as f32 - 2.0 * left;
        for (row, share) in rows.iter().zip(&uses.shares) {
            let (title, sub) = self.lines(share, room);
            f.fill_text(text_in(
                MANROPE_BOLD,
                text(
                    title,
                    Point::new(left, row.title),
                    TITLE,
                    TEXT,
                    Alignment::Left,
                ),
            ));
            f.fill_text(text(
                sub,
                Point::new(left, row.sub),
                SUB,
                DIM,
                Alignment::Left,
            ));
            if let Some(middle) = row.held {
                f.fill_text(text(
                    self.words.held(held),
                    Point::new(left, middle),
                    HELD,
                    FAINT,
                    Alignment::Left,
                ));
            }
            let hover = state.pointer.hover == Some(row.id);
            f.fill(
                &kit::rounded(row.button, BUTTON_RADIUS),
                if hover { BLUE_HOVER } else { BLUE },
            );
            if state.focused == Some(row.id) {
                ring(f, row.button, BUTTON_RADIUS);
            }
            f.fill_text(text_in(
                MANROPE_BOLD,
                text(
                    self.words.stop().to_owned(),
                    row.button.center(),
                    BUTTON_TEXT,
                    Color::WHITE,
                    Alignment::Center,
                ),
            ));
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
    /// A share's two lines, cut to `room`.
    fn lines(&self, share: &hocus_link::ShareShown, room: f32) -> (String, String) {
        let app = crate::share::app_name(&share.app);
        let title = self.words.sees(app.as_deref());
        let sub = self.words.shares(&share.thing);
        match iced::advanced::graphics::text::font_system().write() {
            Ok(mut system) => (
                kit::elide(&title, room, |t| {
                    kit::measure_in(system.raw(), t, TITLE, MANROPE_BOLD)
                }),
                kit::elide(&sub, room, |t| {
                    kit::measure_in(system.raw(), t, SUB, crate::levels::draw::MANROPE)
                }),
            ),
            Err(_) => (title, sub),
        }
    }

    /// What it shows, once, for the log.
    fn say(&self, state: &mut State) {
        let uses = crate::in_use::snapshot();
        let held = crate::notifications::with(|c| c.stack.held_while_sharing());
        let mut said: Vec<String> = uses
            .shares
            .iter()
            .flat_map(|s| {
                let (title, sub) = self.lines(s, f32::INFINITY);
                [title, sub]
            })
            .collect();
        said.push(self.words.held(held));
        said.push(self.words.stop().to_owned());
        if state.said.as_ref() != Some(&said) {
            tracing::debug!(?said, "the sharing popover shows it");
            state.said = Some(said);
        }
    }

    fn stop(id: u64) {
        tracing::info!(id, "sharing stopped from the bar");
        super::send(vec![Effect::Tell(ToCompositor::Share(ShareAsk::Stop {
            id,
        }))]);
    }

    fn mouse<Message>(
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let ids: Vec<u64> = crate::in_use::snapshot()
            .shares
            .iter()
            .map(|s| s.id)
            .collect();
        let (rows, _) = rows(&ids);
        let target = at.and_then(|at| rows.iter().find(|r| r.button.contains(at)).map(|r| r.id));
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
                state.focused = None;
                state.pointer.pressed = target;
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if let Some(id) = state.pointer.release(target) {
                    Self::stop(id);
                }
                redraw
            }
            _ => None,
        }
    }

    fn key<Message>(state: &mut State, pressed: &Pressed) -> Option<canvas::Action<Message>> {
        use iced::keyboard::{Key, key::Named};
        let ids: Vec<u64> = crate::in_use::snapshot()
            .shares
            .iter()
            .map(|s| s.id)
            .collect();
        match &pressed.key {
            Key::Named(Named::Escape) => {
                event(Seen::Escape);
                return Some(canvas::Action::capture());
            }
            Key::Named(Named::Tab | Named::ArrowDown) => {
                state.focused = kit::step(&ids, state.focused, pressed.modifiers.shift());
            }
            Key::Named(Named::ArrowUp) => state.focused = kit::step(&ids, state.focused, true),
            Key::Named(Named::Enter | Named::Space) => {
                if let Some(id) = state.focused {
                    Self::stop(id);
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
    fn each_share_has_its_row_and_the_held_line_is_under_the_last() {
        let (one, h1) = rows(&[1]);
        assert_eq!(one.len(), 1);
        assert!(one[0].held.is_some());
        assert_eq!(one[0].button.width, WIDTH as f32 - 2.0 * (BORDER + PAD));
        let (two, h2) = rows(&[1, 2]);
        assert!(two[0].held.is_none() && two[1].held.is_some());
        assert!(two[1].button.y > two[0].button.y + BUTTON);
        assert!(h2 > h1);
    }
}
