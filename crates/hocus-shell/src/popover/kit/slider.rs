//! Sliders, and the wheel's notches.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use iced::{
    Color, Point, Rectangle, Size, mouse,
    widget::canvas::{self, Frame, Path, gradient},
};

use crate::levels::draw::{MUTED_FILL, TEXT, TRACK};

const KNOB_SHADOW: Color = Color::from_rgba8(0, 0, 0, 0.45);

/// A slider's track, from `left` to `right` and centred on `middle`: its
/// height, its knob's size.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Track {
    pub left: f32,
    pub right: f32,
    pub middle: f32,
    pub height: f32,
    pub knob: f32,
}

impl Track {
    /// Its volume at `x`, in percent: the knob's centre under the pointer,
    /// its travel the track less the knob.
    pub(crate) fn volume_at(self, x: f32) -> f32 {
        (self.share_at(x) * 100.0).round()
    }

    /// The share of its travel at `x`, from 0 to 1.
    pub(crate) fn share_at(self, x: f32) -> f32 {
        let travel = self.right - self.left - self.knob;
        ((x - self.left - self.knob / 2.0) / travel).clamp(0.0, 1.0)
    }

    /// What the pointer takes and the keyboard's ring goes round: the
    /// track's length, `height` tall.
    pub(crate) fn rect(self, height: f32) -> Rectangle {
        Rectangle::new(
            Point::new(self.left, self.middle - height / 2.0),
            Size::new(self.right - self.left, height),
        )
    }
}

/// A slider at `share` of its travel: its track, its fill to the knob
/// (`fill`'s gradient, else grey), its knob.
pub(crate) fn slider(f: &mut Frame, track: Track, share: f32, fill: Option<(Color, Color)>) {
    let Track {
        left,
        right,
        middle,
        height,
        knob: size,
    } = track;
    let travel = right - left - size;
    let top = middle - height / 2.0;
    let radius = height / 2.0;
    f.fill(
        &Path::rounded_rectangle(
            Point::new(left, top),
            Size::new(right - left, height),
            radius.into(),
        ),
        TRACK,
    );
    let knob = left + travel * share.clamp(0.0, 1.0);
    let filled = Path::rounded_rectangle(
        Point::new(left, top),
        Size::new(knob + size / 2.0 - left, height),
        radius.into(),
    );
    match fill {
        None => f.fill(&filled, MUTED_FILL),
        Some((from, to)) => {
            let fill = gradient::Linear::new(
                Point::new(left, middle),
                Point::new(knob + size / 2.0, middle),
            )
            .add_stop(0.0, from)
            .add_stop(1.0, to);
            f.fill(&filled, canvas::Fill::from(fill));
        }
    }
    let centre = Point::new(knob + size / 2.0, middle);
    // `box-shadow: 0 1px 4px`: rings fading out under the knob, 1 px down.
    for (grow, share) in [(2.5_f32, 0.12_f32), (1.6, 0.2), (0.8, 0.3)] {
        f.fill(
            &Path::circle(Point::new(centre.x, centre.y + 1.0), size / 2.0 + grow),
            KNOB_SHADOW.scale_alpha(share),
        );
    }
    f.fill(&Path::circle(centre, size / 2.0), TEXT);
}

/// The bars' screens' scales, by screen, as iced says them.
static SCALES: Mutex<Option<HashMap<String, f32>>> = Mutex::new(None);

/// Bar `bar`'s screen's scale is now `scale`.
pub(crate) fn set_scale(bar: &str, scale: f32) {
    SCALES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_default()
        .insert(bar.to_owned(), scale);
}

pub(crate) fn forget_scale(bar: &str) {
    if let Some(scales) = SCALES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        scales.remove(bar);
    }
}

/// The scale of bar `bar`'s screen, which all its surfaces share: 1 until
/// it is known.
pub(crate) fn scale(bar: &str) -> f32 {
    SCALES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .and_then(|scales| scales.get(bar).copied())
        .unwrap_or(1.0)
}

/// The scale of the open popover's bar's screen.
pub(crate) fn popover_scale() -> f32 {
    crate::popover::with(|c| c.open_on().map(|(bar, _)| scale(bar))).unwrap_or(1.0)
}

/// The notches of a wheel's lines `y`, on a surface at `scale`:
/// iced_layershell hands them multiplied by it. At least one when it
/// moved, should the scale be out of date.
fn lines(y: f32, scale: f32) -> i32 {
    if y == 0.0 {
        return 0;
    }
    let notches = (y.abs() / scale.max(0.25)).round().max(1.0);
    (notches * y.signum()) as i32
}

/// The wheel: a mouse's notches come as lines (a fast spin gives several
/// at once), then as the same scrolling in pixels, which count for
/// nothing; a touchpad sends pixels.
#[derive(Debug, Default)]
pub(crate) struct Wheel {
    after_notch: bool,
    /// A touchpad's pixels toward the next notch.
    pixels: f32,
}

/// A touchpad's scrolling makes a notch each 15 px.
const NOTCH: f32 = 15.0;

impl Wheel {
    /// A scroll on a surface at `scale` as notches, up positive: a
    /// touchpad's pixels add up.
    pub(crate) fn notches(&mut self, delta: &mouse::ScrollDelta, scale: f32) -> i32 {
        match delta {
            mouse::ScrollDelta::Lines { y, .. } => {
                self.after_notch = true;
                self.pixels = 0.0;
                lines(*y, scale)
            }
            mouse::ScrollDelta::Pixels { .. } if self.after_notch => {
                self.after_notch = false;
                0
            }
            mouse::ScrollDelta::Pixels { y, .. } => {
                self.pixels += y;
                let n = (self.pixels / NOTCH).trunc();
                self.pixels -= n * NOTCH;
                n as i32
            }
        }
    }

    /// A scroll on a surface at `scale` as pixels down a list, `notch` px
    /// a notch.
    pub(crate) fn down(&mut self, delta: &mouse::ScrollDelta, notch: f32, scale: f32) -> f32 {
        match delta {
            mouse::ScrollDelta::Lines { y, .. } => {
                self.after_notch = true;
                -(lines(*y, scale) as f32) * notch
            }
            mouse::ScrollDelta::Pixels { .. } if self.after_notch => {
                self.after_notch = false;
                0.0
            }
            mouse::ScrollDelta::Pixels { y, .. } => -y,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wheels_lines_count_each_notch_at_any_scale() {
        let lines = |y: f32, scale: f32| {
            Wheel::default().notches(&mouse::ScrollDelta::Lines { x: 0.0, y }, scale)
        };
        assert_eq!(lines(1.0, 1.0), 1);
        // A fast spin: several notches in one event.
        assert_eq!(lines(-2.0, 1.0), -2);
        assert_eq!(lines(3.0, 1.0), 3);
        // As iced_layershell hands them on a scaled screen.
        assert_eq!(lines(2.0, 2.0), 1);
        assert_eq!(lines(-3.0, 1.5), -2);
        // A scale out of date: still a notch.
        assert_eq!(lines(1.0, 2.0), 1);
        assert_eq!(lines(0.0, 1.0), 0);
        let mut wheel = Wheel::default();
        let down = wheel.down(&mouse::ScrollDelta::Lines { x: 0.0, y: -2.0 }, 10.0, 1.0);
        assert_eq!(down, 20.0);
    }
}
