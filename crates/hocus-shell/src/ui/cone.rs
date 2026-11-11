//! The hover cone (bar.md §9, #785): a popover opened by hovering its bar's
//! item closes 300 ms after the pointer really left the item and the
//! popover, and stays while the pointer heads for it, within the cone
//! from where the pointer was to the popover's near corners. The bar's
//! surfaces report where the pointer goes, the popover's says when it is
//! in, and this decides whether a move keeps it.

use iced::{Point, Rectangle};

/// Where the pointer is, for a popover opened from a bar's item.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Cone {
    /// It is on the item or the popover.
    pub inside: bool,
    /// Where it was last seen off them, in the bar's frame.
    last: Option<Point>,
}

impl Cone {
    /// The pointer on the item or the popover.
    pub fn inside() -> Self {
        Self {
            inside: true,
            last: None,
        }
    }

    /// It came onto the item or the popover.
    pub fn entered(&mut self) {
        self.inside = true;
    }

    /// It left the item at `at`.
    pub fn left(&mut self, at: Point) {
        self.inside = false;
        self.last = Some(at);
    }

    /// It left the popover, whose frame is not the bar's: the next move
    /// seen on the bar starts the cone again.
    pub fn left_popover(&mut self) {
        self.inside = false;
        self.last = None;
    }

    /// It is seen at `at` on the bar: whether it moved toward `popover`
    /// from where it was, off the item and the popover.
    pub fn seen(&mut self, at: Point, popover: Rectangle) -> bool {
        let toward = !self.inside && self.last.is_some_and(|from| toward(from, at, popover));
        self.last = Some(at);
        toward
    }
}

/// Whether a move from `from` to `to` heads for `popover`, on the bar's
/// inner side: `to` is within the cone from `from` to the popover's near
/// corners (its right ones beside a bar on the right).
pub fn toward(from: Point, to: Point, popover: Rectangle) -> bool {
    let near = if crate::side::left() {
        popover.x
    } else {
        popover.x + popover.width
    };
    let (a, b) = (
        Point::new(near, popover.y),
        Point::new(near, popover.y + popover.height),
    );
    if from == to {
        return false;
    }
    let side = |p: Point, q: Point, r: Point| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
    let (d1, d2, d3) = (side(from, a, to), side(a, b, to), side(b, from, to));
    let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(negative && positive)
}

#[cfg(test)]
mod tests {
    use iced::Size;

    use super::*;

    #[test]
    fn the_cone_reaches_the_popovers_near_corners() {
        let popover = Rectangle::new(Point::new(-344.0, 800.0), Size::new(280.0, 264.0));
        let from = Point::new(-40.0, 1000.0);
        // Left and a little up: in. Straight up, or right: out.
        assert!(toward(from, Point::new(-50.0, 995.0), popover));
        assert!(toward(from, Point::new(-50.0, 960.0), popover));
        assert!(!toward(from, Point::new(-40.0, 950.0), popover));
        assert!(!toward(from, Point::new(-30.0, 1000.0), popover));
        // Up beyond its top corner's line: out.
        assert!(!toward(from, Point::new(-45.0, 900.0), popover));
        assert!(!toward(from, from, popover));
    }
}
