//! The pill on its surface, variant A's sizes (logical pixels, the
//! mockup's divided by its ×1.5): 300 × 80, radius 20, the popovers'
//! glass and a 1 px rim; 12 px in on the left, 18 on the right; the panda
//! (64 px since #649), 12 px, then two lines 2 px apart: "Verrouillage dans 7 s"
//! (Manrope 700, 15 px, `text`) over "Bouge la souris pour rester" (13 px,
//! `text.dim`); the countdown's line (2 px, radius 1,
//! `rgba(143,179,255,0.75)`), 7 px over the bottom, from 264 px to nothing
//! over the curtain.
//!
//! The panda falls asleep (motion.md, "Session moments"): its eyes open
//! for the first third, half closed for the second, closed with a "z" for
//! the last; its head tilts from 0° to 16° over the curtain, without
//! reduced motion. Nothing else moves.

use std::time::Instant;

use hocus_link::CURTAIN_FRAME;
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, Vector, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke, gradient},
        text::Alignment,
    },
    window,
};

use super::{Pill, Words};
use crate::{
    levels::draw::{DIM, MANROPE, MANROPE_BOLD, TEXT, rgb},
    lock::draw::{Eyes, panda_eyes},
    popover::kit,
};

const WIDTH: f32 = hocus_link::PILL_WIDTH as f32;
const HEIGHT: f32 = hocus_link::PILL_HEIGHT as f32;
const RADIUS: f32 = hocus_link::PILL_RADIUS as f32;
const LEFT: f32 = 12.0;
const RIGHT: f32 = 18.0;
const PANDA: f32 = 64.0;
const GAP: f32 = 12.0;
const TITLE: f32 = 15.0;
const LINE_GAP: f32 = 2.0;
const HINT: f32 = 13.0;
/// The lines' height, as the mockup's `line-height: 1.25`.
const LINE: f32 = 1.25;
const BAR: f32 = 2.0;
const BAR_BOTTOM: f32 = 7.0;
const BAR_WIDTH: f32 = WIDTH - 2.0 * RIGHT;
const TILT: f32 = 16.0;

const GLASS: (Color, Color) = (
    Color::from_rgba8(22, 27, 42, 0.86),
    Color::from_rgba8(14, 17, 28, 0.92),
);
/// On the CPU, whose buffers have no alpha.
const OPAQUE: (Color, Color) = (rgb(0x161B2A), rgb(0x0E111C));
const RIM: Color = Color::from_rgba8(255, 255, 255, 0.12);
const COUNTDOWN: Color = Color::from_rgba8(143, 179, 255, 0.75);
const Z: Color = rgb(0x8C96A8);

/// The pill on surface `id`.
#[derive(Debug, Clone)]
pub struct View {
    pub id: window::Id,
    pub words: Words,
    pub glass: bool,
}

impl<Message> canvas::Program<Message> for View {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &iced::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let iced::Event::Window(window::Event::RedrawRequested(now)) = event else {
            return None;
        };
        // The countdown, a tenth of a second on, until its end.
        let next = super::with(|p| {
            let p = p.as_ref().filter(|p| p.surface == self.id)?;
            (p.elapsed(*now) < p.length()).then(|| *now + CURTAIN_FRAME)
        })?;
        Some(canvas::Action::request_redraw_at(next))
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        if let Some(pill) = super::with(|p| p.clone().filter(|p| p.surface == self.id)) {
            draw(&mut frame, &pill, self.words, self.glass, Instant::now());
        }
        vec![frame.into_geometry()]
    }
}

/// The panda's eyes a third of the way at a time.
fn eyes(progress: f32) -> Eyes {
    if progress < 1.0 / 3.0 {
        Eyes::Open
    } else if progress < 2.0 / 3.0 {
        Eyes::Half
    } else {
        Eyes::Closed
    }
}

fn draw(f: &mut Frame, pill: &Pill, words: Words, glass: bool, now: Instant) {
    let size = Size::new(WIDTH, HEIGHT);
    let (top, bottom) = if glass { GLASS } else { OPAQUE };
    let tint = gradient::Linear::new(Point::ORIGIN, Point::new(0.0, HEIGHT))
        .add_stop(0.0, top)
        .add_stop(1.0, bottom);
    f.fill(
        &Path::rounded_rectangle(Point::ORIGIN, size, RADIUS.into()),
        canvas::Fill::from(tint),
    );
    let inset = Rectangle::new(Point::new(0.5, 0.5), Size::new(WIDTH - 1.0, HEIGHT - 1.0));
    f.stroke(
        &kit::rounded(inset, RADIUS - 0.5),
        Stroke::default().with_color(RIM).with_width(1.0),
    );
    let progress = pill.progress(now);
    // The panda, its head tilting about its middle.
    let centre = Point::new(LEFT + PANDA / 2.0, HEIGHT / 2.0);
    let tilt = if pill.shown.motion {
        TILT * progress
    } else {
        0.0
    };
    let eyes = eyes(progress);
    f.with_save(|f| {
        f.translate(Vector::new(centre.x, centre.y));
        f.rotate(tilt.to_radians());
        panda_eyes(f, Point::new(-PANDA / 2.0, -PANDA / 2.0), PANDA, eyes, 1.0);
    });
    if eyes == Eyes::Closed {
        f.fill_text(kit::text_in(
            MANROPE_BOLD,
            kit::text(
                "z".to_owned(),
                Point::new(LEFT + PANDA - 4.0, 6.0),
                12.0,
                Z,
                Alignment::Left,
            ),
        ));
    }
    // The two lines, centred together on the pill's height.
    let x = LEFT + PANDA + GAP;
    let block = TITLE * LINE + LINE_GAP + HINT * LINE;
    let y = ((HEIGHT - block) / 2.0).round();
    let seconds = pill.seconds_left(now);
    f.fill_text(kit::text_in(
        MANROPE_BOLD,
        kit::text(
            words.locking_in(seconds),
            Point::new(x, y),
            TITLE,
            TEXT.get(),
            Alignment::Left,
        ),
    ));
    f.fill_text(kit::text_in(
        MANROPE,
        kit::text(
            words.stay().to_owned(),
            Point::new(x, y + TITLE * LINE + LINE_GAP),
            HINT,
            DIM.get(),
            Alignment::Left,
        ),
    ));
    // The countdown's line, shrinking toward the left.
    let width = BAR_WIDTH * (1.0 - progress);
    if width > 0.5 {
        f.fill(
            &Path::rounded_rectangle(
                Point::new(RIGHT, HEIGHT - BAR_BOTTOM - BAR),
                Size::new(width, BAR),
                1.0.into(),
            ),
            COUNTDOWN,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panda_falls_asleep_a_third_at_a_time() {
        assert_eq!(eyes(0.0), Eyes::Open);
        assert_eq!(eyes(0.32), Eyes::Open);
        assert_eq!(eyes(0.4), Eyes::Half);
        assert_eq!(eyes(0.7), Eyes::Closed);
        assert_eq!(eyes(1.0), Eyes::Closed);
    }
}
