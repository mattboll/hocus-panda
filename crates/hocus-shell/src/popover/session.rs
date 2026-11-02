//! The footer's user's popover (5.5), until the session's menu (7.4)
//! takes its place: the user's name ([`crate::user`]) and "Se
//! déconnecter", which ends the session as its shortcut does (the
//! compositor's `log-out`). The desktop has no lock of its own yet: no
//! "Verrouiller" until it does.
//!
//! A click on the row, or `Enter` or `Space` once `Tab` gave it the
//! keyboard, logs out; `Esc` closes.

use hocus_link::{POPOVER_WIDTH, ToCompositor};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme,
    advanced::svg,
    keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry},
        text::Alignment,
    },
    window,
};

use super::{
    Effect, Event as Seen, Pressed, event,
    kit::{self, Input, Pointer, RIGHT, text, text_in},
};
use crate::{
    levels::draw::{MANROPE_BOLD, TEXT, ring},
    popover::clipboard::{LINE_TEXT, TILE, row},
};

pub const WIDTH: u32 = POPOVER_WIDTH.unsigned_abs();
const ROW_PAD: f32 = 8.0;
const ROW_RADIUS: f32 = 10.0;
const ROW_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.06);
const TILE_RADIUS: f32 = 8.0;
const TILE_FILL: Color = Color::from_rgba8(255, 255, 255, 0.08);
const TILE_GAP: f32 = 10.0;
const GLYPH: f32 = 18.0;

/// Its height: the header and its one row, as the clipboard's.
pub fn height() -> u32 {
    super::clipboard::height()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    fr: bool,
}

impl Words {
    pub fn new(lang: crate::clock::time::Lang) -> Self {
        Self {
            fr: lang == crate::clock::time::Lang::French,
        }
    }

    pub fn log_out(self) -> &'static str {
        if self.fr {
            "Se déconnecter"
        } else {
            "Log out"
        }
    }
}

/// The popover on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    /// Its surface: it reacts only while open, not while it fades out.
    pub id: window::Id,
    pub words: Words,
    /// On the compositor's glass (else opaque).
    pub glass: bool,
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    /// The name, as cut to the header.
    name: Option<String>,
    pointer: Pointer<()>,
    focused: bool,
}

/// The log-out glyph: a door and an arrow leaving it.
fn glyph() -> svg::Handle {
    static GLYPH: std::sync::OnceLock<svg::Handle> = std::sync::OnceLock::new();
    GLYPH
        .get_or_init(|| {
            svg::Handle::from_memory(
                format!(
                    "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" \
                     width=\"24\" height=\"24\"><path d=\"M10 5 H6.5 a1.5 1.5 0 0 0 -1.5 1.5 \
                     V17.5 a1.5 1.5 0 0 0 1.5 1.5 H10 M14 8 L18 12 L14 16 M18 12 H9.5\" \
                     fill=\"none\" stroke=\"{}\" stroke-width=\"1.8\" stroke-linecap=\"round\" \
                     stroke-linejoin=\"round\"/></svg>",
                    crate::notifications::bell::INK
                )
                .into_bytes(),
            )
        })
        .clone()
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
                self.refresh(state);
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
        if let Some(name) = &state.name {
            kit::title(f, name);
        }
        let row = row();
        if state.pointer.hover.is_some() {
            f.fill(&kit::rounded(row, ROW_RADIUS), ROW_HOVER);
        }
        if state.focused {
            ring(f, row, ROW_RADIUS);
        }
        let tile = Rectangle::new(
            Point::new(row.x + ROW_PAD, row.center_y() - TILE / 2.0),
            Size::new(TILE, TILE),
        );
        f.fill(&kit::rounded(tile, TILE_RADIUS), TILE_FILL);
        let centre = tile.center();
        f.draw_svg(
            Rectangle::new(
                Point::new(centre.x - GLYPH / 2.0, centre.y - GLYPH / 2.0),
                Size::new(GLYPH, GLYPH),
            ),
            svg::Svg::new(glyph()),
        );
        f.fill_text(text_in(
            kit::MANROPE_SEMIBOLD,
            text(
                self.words.log_out().to_owned(),
                Point::new(tile.x + TILE + TILE_GAP, row.center_y()),
                LINE_TEXT,
                TEXT,
                Alignment::Left,
            ),
        ));
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
    /// The name, read afresh while it is open, cut to the header.
    fn refresh(&self, state: &mut State) {
        if !super::with(|c| c.is_open(self.id)) {
            return;
        }
        let name = crate::user::name();
        let room = RIGHT - kit::LEFT;
        let name = match iced::advanced::graphics::text::font_system().write() {
            Ok(mut system) => kit::elide(&name, room, |t| {
                kit::measure_in(system.raw(), t, kit::TITLE, MANROPE_BOLD)
            }),
            Err(_) => name,
        };
        if state.name.as_ref() == Some(&name) {
            return;
        }
        tracing::debug!(
            name,
            rows = ?[self.words.log_out()],
            "the session's popover shows it"
        );
        state.name = Some(name);
    }

    /// The session ends; the popover closes.
    fn log_out() {
        tracing::info!("the user logs out from the footer");
        super::send(vec![Effect::Tell(ToCompositor::LogOut)]);
        event(Seen::Chosen);
    }

    fn mouse<Message>(
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let target = at.filter(|at| row().contains(*at)).map(|_| ());
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
                state.focused = false;
                state.pointer.pressed = target;
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if state.pointer.release(target).is_some() {
                    Self::log_out();
                }
                redraw
            }
            _ => None,
        }
    }

    fn key<Message>(state: &mut State, pressed: &Pressed) -> Option<canvas::Action<Message>> {
        use keyboard::{Key, key::Named};
        match &pressed.key {
            Key::Named(Named::Escape) => {
                event(Seen::Escape);
                return Some(canvas::Action::capture());
            }
            // Its one row holds the keyboard, round and round.
            Key::Named(Named::Tab | Named::ArrowDown | Named::ArrowUp) => state.focused = true,
            Key::Named(Named::Enter | Named::Space) if state.focused => Self::log_out(),
            _ => return None,
        }
        Some(canvas::Action::request_redraw().and_capture())
    }
}
