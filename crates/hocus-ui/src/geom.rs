//! Logical geometry: what layout, hit testing and damage speak.

/// A point in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

impl Size {
    pub const fn new(w: f32, h: f32) -> Self {
        Self { w, h }
    }
}

/// A rectangle in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// The smallest rectangle holding both; an empty one is ignored.
    pub fn union(&self, other: &Rect) -> Rect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect::new(
            x,
            y,
            self.right().max(other.right()) - x,
            self.bottom().max(other.bottom()) - y,
        )
    }

    pub fn inset(&self, by: Insets) -> Rect {
        Rect::new(
            self.x + by.left,
            self.y + by.top,
            (self.w - by.left - by.right).max(0.0),
            (self.h - by.top - by.bottom).max(0.0),
        )
    }

    pub fn grow(&self, by: f32) -> Rect {
        Rect::new(
            self.x - by,
            self.y - by,
            self.w + 2.0 * by,
            self.h + 2.0 * by,
        )
    }

    pub fn translate(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    /// The physical pixels this rectangle touches at `scale`, rounded out.
    pub fn to_physical(&self, scale: f32) -> PixelRect {
        let x0 = (self.x * scale).floor().max(0.0);
        let y0 = (self.y * scale).floor().max(0.0);
        let x1 = (self.right() * scale).ceil().max(x0);
        let y1 = (self.bottom() * scale).ceil().max(y0);
        PixelRect {
            x: x0 as u32,
            y: y0 as u32,
            w: (x1 - x0) as u32,
            h: (y1 - y0) as u32,
        }
    }
}

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl PixelRect {
    /// This rectangle cut to a `w`×`h` surface.
    pub fn clamp(&self, w: u32, h: u32) -> PixelRect {
        let x = self.x.min(w);
        let y = self.y.min(h);
        PixelRect {
            x,
            y,
            w: self.w.min(w - x),
            h: self.h.min(h - y),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }
}

/// Space inside a box's edges.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Insets {
    pub const fn all(v: f32) -> Self {
        Self {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
    }

    pub const fn symmetric(vertical: f32, horizontal: f32) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_ignores_empty_rectangles() {
        let a = Rect::new(10.0, 10.0, 20.0, 20.0);
        assert_eq!(a.union(&Rect::default()), a);
        assert_eq!(
            a.union(&Rect::new(0.0, 25.0, 5.0, 10.0)),
            Rect::new(0.0, 10.0, 30.0, 25.0)
        );
    }

    #[test]
    fn physical_pixels_round_out() {
        let r = Rect::new(10.25, 0.5, 10.0, 10.0).to_physical(2.0);
        assert_eq!(
            r,
            PixelRect {
                x: 20,
                y: 1,
                w: 21,
                h: 20
            }
        );
    }
}
