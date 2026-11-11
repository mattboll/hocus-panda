//! Sliders, and the wheel's notches.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use iced::{
    Color, Point, Size, mouse,
    widget::canvas::{self, Frame, Path, gradient},
};

use crate::levels::draw::{MUTED_FILL, TEXT, TRACK};
use crate::theme::Ink;

const KNOB_SHADOW: Ink = Ink::Dark(Color::from_rgba8(0, 0, 0, 0.45));

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
    /// The share of its travel at `x`, from 0 to 1.
    pub(crate) fn share_at(self, x: f32) -> f32 {
        let travel = self.right - self.left - self.knob;
        ((x - self.left - self.knob / 2.0) / travel).clamp(0.0, 1.0)
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
        TRACK.get(),
    );
    let knob = left + travel * share.clamp(0.0, 1.0);
    let filled = Path::rounded_rectangle(
        Point::new(left, top),
        Size::new(knob + size / 2.0 - left, height),
        radius.into(),
    );
    match fill {
        None => f.fill(&filled, MUTED_FILL.get()),
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
    f.fill(&Path::circle(centre, size / 2.0), TEXT.get());
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

/// A touchpad's pixels `y` in logical pixels, on a surface at `scale`:
/// iced_layershell hands them multiplied by it too (#488).
fn logical(y: f32, scale: f32) -> f32 {
    y / scale.max(0.25)
}

/// The wheel, for every list and level that scrolls: a mouse's notches
/// come as lines (a fast spin gives several at once), then as the same
/// scrolling in pixels, which count for nothing; a touchpad sends pixels.
/// Both come scaled by the surface's scale, which it takes away.
#[derive(Debug, Default)]
pub(crate) struct Wheel {
    after_notch: bool,
    /// A touchpad's logical pixels toward the next notch.
    pixels: f32,
}

/// A touchpad's scrolling makes a notch each 15 logical px.
const NOTCH: f32 = 15.0;

impl Wheel {
    /// A scroll on a surface at `scale` as notches, up positive: a
    /// touchpad's pixels add up, afresh when it turns back.
    pub(crate) fn notches(&mut self, delta: &mouse::ScrollDelta, scale: f32) -> i32 {
        match delta {
            mouse::ScrollDelta::Lines { y, .. } => {
                self.after_notch = true;
                self.pixels = 0.0;
                lines(*y, scale)
            }
            // Sideways, or only where it comes from: nothing, and the
            // notch's pixels still to come.
            mouse::ScrollDelta::Pixels { y: 0.0, .. } => 0,
            mouse::ScrollDelta::Pixels { .. } if self.after_notch => {
                self.after_notch = false;
                0
            }
            mouse::ScrollDelta::Pixels { y, .. } => {
                let y = logical(*y, scale);
                if self.pixels * y < 0.0 {
                    self.pixels = 0.0;
                }
                self.pixels += y;
                let n = (self.pixels / NOTCH).trunc();
                self.pixels -= n * NOTCH;
                n as i32
            }
        }
    }

    /// A scroll on a surface at `scale` as logical pixels down a list,
    /// `notch` px a notch.
    pub(crate) fn down(&mut self, delta: &mouse::ScrollDelta, notch: f32, scale: f32) -> f32 {
        match delta {
            mouse::ScrollDelta::Lines { y, .. } => {
                self.after_notch = true;
                -(lines(*y, scale) as f32) * notch
            }
            mouse::ScrollDelta::Pixels { y: 0.0, .. } => 0.0,
            mouse::ScrollDelta::Pixels { .. } if self.after_notch => {
                self.after_notch = false;
                0.0
            }
            mouse::ScrollDelta::Pixels { y, .. } => -logical(*y, scale),
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

    #[test]
    fn a_touchpads_pixels_make_notches_and_scroll_in_logical_pixels() {
        let lines = |y| mouse::ScrollDelta::Lines { x: 0.0, y };
        let pixels = |y| mouse::ScrollDelta::Pixels { x: 0.0, y };
        let mut wheel = Wheel::default();
        // A wheel: a notch each, its pixels counting for nothing, nor
        // what says only where it comes from.
        assert_eq!(wheel.notches(&pixels(0.0), 1.0), 0);
        assert_eq!(wheel.notches(&lines(1.0), 1.0), 1);
        assert_eq!(wheel.notches(&pixels(0.0), 1.0), 0);
        assert_eq!(wheel.notches(&pixels(15.0), 1.0), 0);
        // A touchpad: 15 px a notch, the rest kept, a turn back starting
        // afresh.
        assert_eq!(wheel.notches(&pixels(10.0), 1.0), 0);
        assert_eq!(wheel.notches(&pixels(10.0), 1.0), 1);
        assert_eq!(wheel.notches(&pixels(40.0), 1.0), 3);
        assert_eq!(wheel.notches(&pixels(-4.0), 1.0), 0);
        assert_eq!(wheel.notches(&pixels(-12.0), 1.0), -1);
        // At ×2, iced hands 15 logical px as 30: still a notch.
        let mut wheel = Wheel::default();
        assert_eq!(wheel.notches(&pixels(30.0), 2.0), 1);
        assert_eq!(wheel.notches(&pixels(20.0), 2.0), 0);
        assert_eq!(wheel.notches(&pixels(10.0), 2.0), 1);
        // A list: its logical pixels, at any scale.
        assert_eq!(wheel.down(&pixels(-34.0), 10.0, 1.0), 34.0);
        assert_eq!(wheel.down(&pixels(-68.0), 10.0, 2.0), 34.0);
        assert_eq!(wheel.down(&pixels(-51.0), 10.0, 1.5), 34.0);
    }
}
