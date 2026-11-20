//! A slider: the levels' track, fill and knob (the canvas's
//! [`kit::slider`], whose look iced's stock slider has not: a gradient to
//! the knob, the knob's shadow), taken by the pointer from anywhere in its
//! height and dragged. A slider may have a mark (the sound's 100 % with
//! boost, #897): a notch, the fill amber past it, and a soft stop there,
//! the knob sticking to it within a few pixels.

use iced::{
    Color, Length, Rectangle, Renderer, Theme, mouse,
    widget::canvas::{self, Action, Frame},
};

use crate::popover::kit;

/// A slider at `share` of its travel (0 to 1), filled with `fill`'s
/// gradient (none: grey, what it sets is off); its track `track` tall,
/// its knob `knob`, its `mark` (a share) if any. Dragging tells
/// `on_change` each share, then `on_release`.
pub struct Slider<'a, M> {
    pub share: f32,
    pub fill: Option<(Color, Color)>,
    pub track: f32,
    pub knob: f32,
    pub mark: Option<f32>,
    pub on_change: Box<dyn Fn(f32) -> M + 'a>,
    pub on_release: M,
}

impl<'a, M: Clone + 'a> Slider<'a, M> {
    /// As a widget across its room, `height` tall.
    pub fn view(self, height: f32) -> iced::Element<'a, M> {
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

/// A dragged knob this close to its slider's mark, in logical pixels,
/// sticks to it.
const STICK: f32 = 4.0;

/// `share` of a slider whose knob travels `travel` px, stuck to `mark`
/// when within [`STICK`] px of it.
pub(crate) fn stuck(share: f32, mark: Option<f32>, travel: f32) -> f32 {
    match mark {
        Some(mark) if ((share - mark) * travel).abs() <= STICK => mark,
        _ => share,
    }
}

impl<M: Clone> canvas::Program<M> for Slider<'_, M> {
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
                let share = stuck(track.share_at(at.x), self.mark, track.travel());
                Some(Action::publish((self.on_change)(share)).and_capture())
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) if *dragging => {
                let at = cursor.position_from(bounds.position())?;
                let share = stuck(track.share_at(at.x), self.mark, track.travel());
                Some(Action::publish((self.on_change)(share)).and_capture())
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
        kit::slider(&mut f, self.track(bounds), self.share, self.fill, self.mark);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dragged_knob_sticks_to_its_mark_within_a_few_pixels() {
        let mark = Some(2.0 / 3.0);
        // A 200 px travel: 3 px past the mark sticks, 6 px does not.
        assert_eq!(stuck(2.0 / 3.0 + 3.0 / 200.0, mark, 200.0), 2.0 / 3.0);
        assert_eq!(stuck(2.0 / 3.0 - 3.0 / 200.0, mark, 200.0), 2.0 / 3.0);
        let past = 2.0 / 3.0 + 6.0 / 200.0;
        assert_eq!(stuck(past, mark, 200.0), past);
        // Without a mark, where it is.
        assert_eq!(stuck(0.67, None, 200.0), 0.67);
        // Stuck, it is 100 %.
        assert_eq!(
            hocus_sound::boost::volume_at(stuck(0.675, mark, 200.0), true),
            100.0
        );
    }
}
