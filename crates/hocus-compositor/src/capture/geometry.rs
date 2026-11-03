//! The capture's area and its handles, in logical pixels of its screen:
//! drawn by a drag, moved from inside, resized by a corner or an edge,
//! nudged by the arrows, and always held inside its screen.

use crate::model::Edges;

/// The smallest area a drag draws, or a resize leaves (the mockup's: a
/// drag shorter than this keeps the area there was).
pub const SMALLEST: f64 = 6.0;

/// A rectangle on a screen, from its top left corner (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// What is under the pointer: where a press takes the area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// A corner's handle or an edge: a resize of those edges.
    Edges(Edges),
    /// Inside: a move.
    Inside,
    /// Away from it: a new area.
    Outside,
}

impl Area {
    pub fn right(self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(self) -> f64 {
        self.y + self.h
    }

    /// The area from `a` to `b`, whichever corners they are, held inside
    /// a screen `size` large.
    pub fn between(a: (f64, f64), b: (f64, f64), size: (f64, f64)) -> Self {
        let clamp = |(x, y): (f64, f64)| (x.clamp(0.0, size.0), y.clamp(0.0, size.1));
        let (a, b) = (clamp(a), clamp(b));
        Self {
            x: a.0.min(b.0),
            y: a.1.min(b.1),
            w: (a.0 - b.0).abs(),
            h: (a.1 - b.1).abs(),
        }
    }

    /// Big enough to be an area.
    pub fn drawn(self) -> bool {
        self.w >= SMALLEST && self.h >= SMALLEST
    }

    /// Held inside a screen `size` large: as large as it fits, moved in.
    pub fn held(self, size: (f64, f64)) -> Self {
        let w = self.w.clamp(SMALLEST.min(size.0), size.0);
        let h = self.h.clamp(SMALLEST.min(size.1), size.1);
        Self {
            x: self.x.clamp(0.0, size.0 - w),
            y: self.y.clamp(0.0, size.1 - h),
            w,
            h,
        }
    }

    /// In whole logical pixels, as the image will be cut.
    pub fn rounded(self) -> Self {
        let (x, y) = (self.x.round(), self.y.round());
        Self {
            x,
            y,
            w: (self.right().round() - x).max(1.0),
            h: (self.bottom().round() - y).max(1.0),
        }
    }

    /// What a press at (`x`, `y`) takes: a corner within `reach` of it
    /// (both its edges), an edge within `reach` of it along its length,
    /// the inside, or nothing.
    pub fn hit(self, x: f64, y: f64, reach: f64) -> Hit {
        let near = |a: f64, b: f64| (a - b).abs() <= reach;
        let along_x = x >= self.x - reach && x <= self.right() + reach;
        let along_y = y >= self.y - reach && y <= self.bottom() + reach;
        let edges = Edges {
            left: near(x, self.x) && along_y,
            right: near(x, self.right()) && along_y && !near(x, self.x),
            top: near(y, self.y) && along_x,
            bottom: near(y, self.bottom()) && along_x && !near(y, self.y),
        };
        if edges.any() {
            return Hit::Edges(edges);
        }
        if x > self.x && x < self.right() && y > self.y && y < self.bottom() {
            Hit::Inside
        } else {
            Hit::Outside
        }
    }

    /// Moved by (`dx`, `dy`), held inside a screen `size` large.
    pub fn moved(self, dx: f64, dy: f64, size: (f64, f64)) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
        .held(size)
    }

    /// Its `edges` moved by (`dx`, `dy`), never past the opposite edge
    /// (less [`SMALLEST`]) nor out of a screen `size` large.
    pub fn resized(self, edges: Edges, dx: f64, dy: f64, size: (f64, f64)) -> Self {
        let (mut left, mut top, mut right, mut bottom) =
            (self.x, self.y, self.right(), self.bottom());
        if edges.left {
            left = (left + dx).clamp(0.0, right - SMALLEST);
        }
        if edges.right {
            right = (right + dx).clamp(left + SMALLEST, size.0);
        }
        if edges.top {
            top = (top + dy).clamp(0.0, bottom - SMALLEST);
        }
        if edges.bottom {
            bottom = (bottom + dy).clamp(top + SMALLEST, size.1);
        }
        Self {
            x: left,
            y: top,
            w: right - left,
            h: bottom - top,
        }
    }

    /// Where its eight handles are: the corners, then the edges' middles.
    pub fn handles(self) -> [(f64, f64); 8] {
        let (cx, cy) = (self.x + self.w / 2.0, self.y + self.h / 2.0);
        [
            (self.x, self.y),
            (self.right(), self.y),
            (self.x, self.bottom()),
            (self.right(), self.bottom()),
            (cx, self.y),
            (cx, self.bottom()),
            (self.x, cy),
            (self.right(), cy),
        ]
    }

    /// The parts of a screen `size` large around it, which darken: above,
    /// below, and on its left and right.
    pub fn around(self, size: (f64, f64)) -> [Self; 4] {
        let (w, h) = size;
        [
            Self {
                x: 0.0,
                y: 0.0,
                w,
                h: self.y,
            },
            Self {
                x: 0.0,
                y: self.bottom(),
                w,
                h: h - self.bottom(),
            },
            Self {
                x: 0.0,
                y: self.y,
                w: self.x,
                h: self.h,
            },
            Self {
                x: self.right(),
                y: self.y,
                w: w - self.right(),
                h: self.h,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: (f64, f64) = (1920.0, 1080.0);
    const AREA: Area = Area {
        x: 100.0,
        y: 200.0,
        w: 600.0,
        h: 380.0,
    };

    fn edges(left: bool, right: bool, top: bool, bottom: bool) -> Edges {
        Edges {
            left,
            right,
            top,
            bottom,
        }
    }

    #[test]
    fn a_press_takes_a_corner_an_edge_the_inside_or_nothing() {
        let reach = 9.0;
        assert_eq!(
            AREA.hit(100.0, 200.0, reach),
            Hit::Edges(edges(true, false, true, false))
        );
        assert_eq!(
            AREA.hit(705.0, 584.0, reach),
            Hit::Edges(edges(false, true, false, true))
        );
        assert_eq!(
            AREA.hit(400.0, 196.0, reach),
            Hit::Edges(edges(false, false, true, false))
        );
        assert_eq!(
            AREA.hit(700.0, 400.0, reach),
            Hit::Edges(edges(false, true, false, false))
        );
        assert_eq!(AREA.hit(400.0, 400.0, reach), Hit::Inside);
        assert_eq!(AREA.hit(50.0, 400.0, reach), Hit::Outside);
        // Along an edge's line, but past the area: nothing.
        assert_eq!(AREA.hit(800.0, 200.0, reach), Hit::Outside);
    }

    #[test]
    fn a_corner_resizes_both_its_sides_and_an_edge_its_own() {
        let corner = AREA.resized(edges(false, true, false, true), 40.0, 20.0, SCREEN);
        assert_eq!(
            corner,
            Area {
                w: 640.0,
                h: 400.0,
                ..AREA
            }
        );
        let left = AREA.resized(edges(true, false, false, false), -30.0, 50.0, SCREEN);
        assert_eq!(
            left,
            Area {
                x: 70.0,
                w: 630.0,
                ..AREA
            }
        );
        // Never past the other edge, nor out of the screen.
        let squashed = AREA.resized(edges(true, false, false, false), 2000.0, 0.0, SCREEN);
        assert_eq!(squashed.w, SMALLEST);
        assert_eq!(squashed.right(), AREA.right());
        let out = AREA.resized(edges(false, false, true, false), 0.0, -900.0, SCREEN);
        assert_eq!((out.y, out.bottom()), (0.0, AREA.bottom()));
    }

    #[test]
    fn it_moves_and_stays_inside_its_screen() {
        assert_eq!(
            AREA.moved(10.0, -5.0, SCREEN),
            Area {
                x: 110.0,
                y: 195.0,
                ..AREA
            }
        );
        let pushed = AREA.moved(5000.0, 5000.0, SCREEN);
        assert_eq!((pushed.right(), pushed.bottom()), SCREEN);
        assert_eq!((pushed.w, pushed.h), (AREA.w, AREA.h));
        // Larger than a smaller screen it is kept on: as large as it fits.
        let small = Area { w: 3000.0, ..AREA }.held(SCREEN);
        assert_eq!((small.x, small.w), (0.0, SCREEN.0));
    }

    #[test]
    fn a_drag_draws_from_either_corner_inside_the_screen() {
        let a = Area::between((700.0, 580.0), (100.0, 200.0), SCREEN);
        assert_eq!(a, AREA);
        let b = Area::between((1800.0, 1000.0), (2500.0, 1300.0), SCREEN);
        assert_eq!(
            b,
            Area {
                x: 1800.0,
                y: 1000.0,
                w: 120.0,
                h: 80.0
            }
        );
        assert!(!Area::between((5.0, 5.0), (8.0, 30.0), SCREEN).drawn());
    }

    #[test]
    fn what_darkens_is_around_it() {
        let parts = AREA.around(SCREEN);
        let total: f64 = parts.iter().map(|p| p.w * p.h).sum();
        assert_eq!(total + AREA.w * AREA.h, SCREEN.0 * SCREEN.1);
    }
}
