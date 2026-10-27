//! A to-do's box: an outline, or filled with the accent and ticked; and
//! a choice's round mark, ringed with the accent and dotted when chosen.

use std::time::Duration;

use hocus_ui::{Canvas, Draw, Point, Rect, Size, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkbox(pub bool);

impl Draw for Checkbox {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, _: Duration) {
        let d = rect.w.min(rect.h).min(18.0);
        let r = Rect::new(
            rect.x + (rect.w - d) / 2.0,
            rect.y + (rect.h - d) / 2.0,
            d,
            d,
        );
        let c = &theme.colors;
        if self.0 {
            canvas.fill_rect(r, 5.0, c.accent);
            let at = |x: f32, y: f32| Point::new(r.x + x * d, r.y + y * d);
            canvas.polyline(
                &[at(0.25, 0.52), at(0.43, 0.7), at(0.76, 0.32)],
                2.0,
                c.on_accent,
            );
        } else {
            canvas.stroke_rect(r, 5.0, 1.5, c.muted);
        }
    }

    fn size(&self) -> Size {
        Size::new(18.0, 18.0)
    }
}

/// One choice among several, chosen or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Radio(pub bool);

impl Draw for Radio {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, _: Duration) {
        let d = rect.w.min(rect.h).min(18.0);
        let r = Rect::new(
            rect.x + (rect.w - d) / 2.0,
            rect.y + (rect.h - d) / 2.0,
            d,
            d,
        );
        let c = &theme.colors;
        if self.0 {
            canvas.stroke_rect(r, d / 2.0, 2.0, c.accent);
            let dot = d * 0.45;
            let inner = Rect::new(r.x + (d - dot) / 2.0, r.y + (d - dot) / 2.0, dot, dot);
            canvas.fill_rect(inner, dot / 2.0, c.accent);
        } else {
            canvas.stroke_rect(r, d / 2.0, 1.5, c.muted);
        }
    }

    fn size(&self) -> Size {
        Size::new(18.0, 18.0)
    }
}
