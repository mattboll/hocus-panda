//! The shell's text field ([`crate::field`], #771) as a widget of the
//! kit: the field's model stays the surface's ([`Entry`]); this draws it
//! where the tree lays it out and hands it the pointer and, while it holds
//! the ring, the keys and the input method. What came of each is told to
//! the surface ([`Took`]): a paste to ask for, an error to clear.

use iced::{
    Element, Length, Point, Rectangle, Renderer, Size, Theme, mouse,
    widget::canvas::{self, Action, Frame},
};

use crate::field::{
    Entry, Place, Took,
    view::{Look, Style},
};

/// The surface's field, reached through its lock: what is to be done
/// with it is handed in.
pub type With = fn(&mut dyn FnMut(&mut Entry));

/// A field: its text from `left` to `right` px in from its sides.
pub struct Field<M> {
    pub with: With,
    pub style: Style,
    pub placeholder: String,
    pub look: Look,
    /// It holds the ring: its caret shows, the keys are its.
    pub focused: bool,
    pub left: f32,
    pub right: f32,
    /// A press in it takes the ring.
    pub on_focus: M,
    pub on_took: fn(Took) -> M,
}

impl<M: Clone + 'static> Field<M> {
    /// As a widget, `height` tall across its room.
    pub fn view<'a>(self, height: f32) -> Element<'a, M> {
        canvas(self).width(Length::Fill).height(height).into()
    }

    fn place(&self, size: Size) -> Place {
        Place {
            left: self.left,
            middle: size.height / 2.0,
            width: size.width - self.left - self.right,
            style: self.style,
        }
    }

    fn took(&self, took: Took) -> Option<Action<M>> {
        match took {
            Took::No => None,
            took => Some(Action::publish((self.on_took)(took)).and_capture()),
        }
    }
}

fn canvas<M, P: canvas::Program<M>>(program: P) -> canvas::Canvas<P, M> {
    iced::widget::canvas(program)
}

impl<M: Clone + 'static> canvas::Program<M> for Field<M> {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<M>> {
        let at = cursor.position_in(bounds);
        let one = |with: &mut dyn FnMut(&mut Entry) -> Took| {
            let mut took = Took::No;
            (self.with)(&mut |entry| took = with(entry));
            took
        };
        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let at = at?;
                one(&mut |entry| {
                    entry.press(at);
                    Took::Moved
                });
                Some(Action::publish(self.on_focus.clone()).and_capture())
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let at = cursor.position_from(bounds.position())?;
                let took = one(&mut |entry| {
                    if entry.pressed() && entry.drag(at) {
                        Took::Moved
                    } else {
                        Took::No
                    }
                });
                self.took(took)
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                one(&mut |entry| {
                    entry.release();
                    Took::No
                });
                None
            }
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) if self.focused => {
                let took = one(&mut |entry| entry.key(key, *modifiers, text.as_deref()));
                self.took(took)
            }
            iced::Event::InputMethod(event) if self.focused => {
                let took = one(&mut |entry| entry.input_method(event));
                self.took(took)
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let place = self.place(bounds.size());
        let room = Rectangle::new(
            Point::new(place.left, 0.0),
            Size::new(place.width.max(0.0), bounds.height),
        );
        // Drawn, it remembers where: a press finds its place in the text
        // from there.
        (self.with)(&mut |entry| {
            f.with_clip(room, |f| {
                entry.draw(f, place, &self.placeholder, &self.look, self.focused);
            });
        });
        vec![f.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &(),
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }
}
