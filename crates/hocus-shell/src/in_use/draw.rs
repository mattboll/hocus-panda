//! The sign of what is in use, under the clock (5.11's sharing item): 48
//! wide, radius 12, `rgba(91,141,239,0.18)` in a 1 px
//! `rgba(91,141,239,0.6)` ring, a 20 px screen-share icon over "Partage"
//! (10 px bold), both `#B9CEFF`; lit under the pointer and while its
//! popover shows. The same in both forms, centred in the bar: in the mini
//! form where the agents' item was, in the full one over the agents'
//! group, which both move under it.

use hocus_link::{Form, IN_USE_NAMESPACE};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke},
        text::Alignment,
    },
};

use super::Words;
use crate::{
    Message,
    levels::draw::{MANROPE_BOLD, faded, pointer, rgb, ring},
    popover::{self, Item, kit},
};

/// The sign: 48 wide, 6 px in above and under its 20 px icon, 3 px over
/// its word's 10 px line.
pub const SIGN: Size = Size::new(48.0, 6.0 + 20.0 + 3.0 + 10.0 * 1.366 + 6.0);
/// Between it and what comes under it, as between the bar's items.
pub const AFTER: f32 = 14.0;
const RADIUS: f32 = 12.0;
const ICON: f32 = 20.0;
const PAD: f32 = 6.0;
const ICON_GAP: f32 = 3.0;
const WORD: f32 = 10.0;
const FILL: Color = Color::from_rgba8(91, 141, 239, 0.18);
const LIT: Color = Color::from_rgba8(91, 141, 239, 0.28);
const EDGE_RING: Color = Color::from_rgba8(91, 141, 239, 0.6);
const INK: Color = rgb(0xB9CEFF);
/// Around the sign on its surface: the keyboard's ring.
const ROOM: f32 = 5.0;
/// The surfaces' widths: the bar inside its rim.
const FULL_WIDTH: u32 = 194;
const MINI_WIDTH: u32 = 54;

/// The sign's top under the bar's in `form`: where the agents' group (the
/// full form) or the agents' item (the mini) was.
fn top(form: Form) -> f32 {
    match form {
        Form::Full => crate::agents::draw::TOP,
        Form::Mini => crate::agents::MINI_TOP,
    }
}

/// Its surface's size and its top under the bar's, in `form` (both forms'
/// places while the bar switches).
pub fn placement(form: Form, switching: bool) -> ((u32, u32), f32) {
    let tall = |h: f32| (h + 2.0 * ROOM).ceil() as u32;
    match (switching, form) {
        (true, _) => {
            let (a, b) = (top(Form::Full), top(Form::Mini));
            (
                (FULL_WIDTH, tall(a.max(b) - a.min(b) + SIGN.height)),
                a.min(b) - ROOM,
            )
        }
        (false, Form::Full) => ((FULL_WIDTH, tall(SIGN.height)), top(form) - ROOM),
        (false, Form::Mini) => ((MINI_WIDTH, tall(SIGN.height)), top(form) - ROOM),
    }
}

/// The sign of bar `bar` on its surface.
#[derive(Debug, Clone)]
pub struct Sign {
    pub bar: String,
    pub forms: Vec<(Form, f32)>,
    pub bar_width: f32,
    pub words: Words,
    /// On the CPU, the bar's colour under it.
    pub backdrop: Option<Color>,
}

#[derive(Debug, Default)]
pub struct State {
    at: Option<Point>,
    hover: bool,
}

impl Sign {
    /// The surface's top under the bar's, as placed for its forms now.
    fn surface_top(&self) -> f32 {
        match self.forms.as_slice() {
            [(form, _)] => placement(*form, false).1,
            _ => placement(Form::Full, true).1,
        }
    }

    /// The sign in `form` on a surface `size` large, centred in the bar as
    /// wide as it is drawn now.
    fn rect(&self, form: Form, size: Size) -> Rectangle {
        let centre = size.width + 1.0 - self.bar_width / 2.0;
        Rectangle::new(
            Point::new(centre - SIGN.width / 2.0, top(form) - self.surface_top()),
            SIGN,
        )
    }

    /// The sign alone (not while the bar switches), for the pointer.
    fn item(&self, size: Size) -> Option<Rectangle> {
        match self.forms.as_slice() {
            [(form, _)] => Some(self.rect(*form, size)),
            _ => None,
        }
    }

    /// `rect` (the surface's) in the bar's frame: from its right edge
    /// (negative inside it), from its top.
    fn in_bar(&self, rect: Rectangle, size: Size) -> Rectangle {
        Rectangle::new(
            Point::new(rect.x - size.width - 1.0, self.surface_top() + rect.y),
            rect.size(),
        )
    }
}

impl canvas::Program<Message> for Sign {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let size = bounds.size();
        match event {
            iced::Event::Mouse(seen) => {
                let at = pointer(seen, bounds, cursor, state.at);
                state.at = at;
                let rect = self.item(size)?;
                let over = at.is_some_and(|at| rect.contains(at));
                match seen {
                    mouse::Event::CursorEntered
                    | mouse::Event::CursorMoved { .. }
                    | mouse::Event::CursorLeft => {
                        if over == state.hover {
                            return None;
                        }
                        state.hover = over;
                        if over {
                            popover::event(popover::Event::ItemEnter {
                                bar: self.bar.clone(),
                                item: Item::Sharing,
                                rect: self.in_bar(rect, size),
                            });
                        } else {
                            let at = at.unwrap_or(Point::new(size.width / 2.0, size.height));
                            popover::event(popover::Event::ItemLeave {
                                at: self.in_bar(Rectangle::new(at, Size::ZERO), size).position(),
                            });
                        }
                        Some(canvas::Action::request_redraw())
                    }
                    mouse::Event::ButtonPressed(mouse::Button::Left) if over => {
                        popover::event(popover::Event::ItemPress {
                            bar: self.bar.clone(),
                            item: Item::Sharing,
                            rect: self.in_bar(rect, size),
                        });
                        Some(canvas::Action::request_redraw().and_capture())
                    }
                    _ => None,
                }
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => popover::key(key, *modifiers, text.as_deref())
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
        let size = bounds.size();
        if let Some(backdrop) = self.backdrop {
            frame.fill_rectangle(Point::ORIGIN, size, backdrop);
        }
        if let Some(rect) = self.item(size) {
            let in_bar = self.in_bar(rect, size);
            popover::with(|c| {
                c.set_items(&self.bar, IN_USE_NAMESPACE, vec![(Item::Sharing, in_bar)])
            });
        }
        if !super::snapshot().any() {
            return vec![frame.into_geometry()];
        }
        let still = self.forms.len() == 1;
        let (open, focused) = if still {
            popover::with(|c| {
                (
                    c.shows(&self.bar, Item::Sharing),
                    c.focused(&self.bar, Item::Sharing),
                )
            })
        } else {
            (false, false)
        };
        for &(form, alpha) in &self.forms {
            let rect = self.rect(form, size);
            let lit = still && (state.hover || open);
            draw_sign(&mut frame, rect, self.words, lit, alpha);
            if focused {
                ring(&mut frame, rect, RADIUS);
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.hover {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

/// The sign at `rect`: its tint and ring, the icon over the word.
pub fn draw_sign(f: &mut Frame, rect: Rectangle, words: Words, lit: bool, alpha: f32) {
    let shape = kit::rounded(rect, RADIUS);
    f.fill(&shape, faded(if lit { LIT } else { FILL }, alpha));
    f.stroke(
        &kit::rounded(
            Rectangle::new(
                Point::new(rect.x + 0.5, rect.y + 0.5),
                Size::new(rect.width - 1.0, rect.height - 1.0),
            ),
            RADIUS - 0.5,
        ),
        Stroke::default()
            .with_color(faded(EDGE_RING, alpha))
            .with_width(1.0),
    );
    let icon = Rectangle::new(
        Point::new(rect.center_x() - ICON / 2.0, rect.y + PAD),
        Size::new(ICON, ICON),
    );
    screen_icon(f, icon, faded(INK, alpha));
    f.fill_text(kit::text_in(
        MANROPE_BOLD,
        kit::text(
            words.sharing().to_owned(),
            Point::new(
                rect.center_x(),
                icon.y + ICON + ICON_GAP + WORD * 1.366 / 2.0,
            ),
            WORD,
            faded(INK, alpha),
            Alignment::Center,
        ),
    ));
}

/// The mockup's screen-share icon (its 24 px drawing) in `rect`: a screen,
/// its foot, and an arrow up in it.
pub fn screen_icon(f: &mut Frame, rect: Rectangle, colour: Color) {
    let k = rect.width / 24.0;
    let p = |x: f32, y: f32| Point::new(rect.x + x * k, rect.y + y * k);
    let path = Path::new(|b| {
        b.move_to(p(3.5, 5.0));
        b.line_to(p(20.5, 5.0));
        b.line_to(p(20.5, 16.0));
        b.line_to(p(3.5, 16.0));
        b.close();
        b.move_to(p(9.0, 20.0));
        b.line_to(p(15.0, 20.0));
        b.move_to(p(12.0, 16.0));
        b.line_to(p(12.0, 20.0));
        b.move_to(p(12.0, 8.0));
        b.line_to(p(12.0, 13.0));
        b.move_to(p(9.5, 10.5));
        b.line_to(p(12.0, 8.0));
        b.line_to(p(14.5, 10.5));
    });
    f.stroke(
        &path,
        Stroke::default()
            .with_color(colour)
            .with_width(1.8 * k)
            .with_line_join(LineJoin::Round)
            .with_line_cap(LineCap::Round),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sign_sits_where_the_agents_started_and_they_move_under_it() {
        let ((w, h), top) = placement(Form::Mini, false);
        assert_eq!(w, 54);
        assert_eq!(top, crate::agents::MINI_TOP - ROOM);
        assert!((h as f32 - (SIGN.height + 2.0 * ROOM)).abs() < 1.0);
        let ((w, _), top) = placement(Form::Full, false);
        assert_eq!((w, top), (194, crate::agents::draw::TOP - ROOM));
        // While switching, both forms' places on one surface.
        let ((_, h), top) = placement(Form::Full, true);
        assert_eq!(top, crate::agents::MINI_TOP - ROOM);
        assert!(top + h as f32 >= crate::agents::draw::TOP + SIGN.height);
    }
}
