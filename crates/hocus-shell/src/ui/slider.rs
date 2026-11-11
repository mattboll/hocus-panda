//! A volume's slider: the levels' track, fill and knob (the canvas's
//! [`kit::slider`], whose look iced's stock slider has not: a gradient to
//! the knob, the knob's shadow), taken by the pointer from anywhere in its
//! height and dragged.

use iced::{
    Length, Rectangle, Renderer, Theme, mouse,
    widget::canvas::{self, Action, Frame},
};

use crate::{levels::draw::SOUND, popover::kit};

/// A slider at `volume` (0 to 100), grey when what it sets is `grey`;
/// its track `track` tall, its knob `knob`. Dragging tells `on_change`
/// each volume, then `on_release`.
pub struct Slider<M> {
    pub volume: f32,
    pub grey: bool,
    pub track: f32,
    pub knob: f32,
    pub on_change: fn(f32) -> M,
    pub on_release: M,
}

impl<M: Clone + 'static> Slider<M> {
    /// As a widget across its room, `height` tall.
    pub fn view<'a>(self, height: f32) -> iced::Element<'a, M> {
        iced::widget::canvas(self)
            .width(Length::Fill)
            .height(height)
            .into()
    }

    fn track(&self, bounds: Rectangle) -> kit::Track {
        kit::Track {
            left: 0.0,
            right: bounds.width,
            middle: bounds.height / 2.0,
            height: self.track,
            knob: self.knob,
        }
    }
}

impl<M: Clone + 'static> canvas::Program<M> for Slider<M> {
    /// Dragged.
    type State = bool;

    fn update(
        &self,
        dragging: &mut bool,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<M>> {
        let track = self.track(bounds);
        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let at = cursor.position_in(bounds)?;
                *dragging = true;
                Some(Action::publish((self.on_change)(track.volume_at(at.x))).and_capture())
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) if *dragging => {
                let at = cursor.position_from(bounds.position())?;
                Some(Action::publish((self.on_change)(track.volume_at(at.x))).and_capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if *dragging => {
                *dragging = false;
                Some(Action::publish(self.on_release.clone()).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _dragging: &bool,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        kit::slider(
            &mut f,
            self.track(bounds),
            self.volume / 100.0,
            (!self.grey).then_some(SOUND),
        );
        vec![f.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        dragging: &bool,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if *dragging {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(bounds) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}
