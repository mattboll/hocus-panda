//! Arranging screens on the Screens page: where each screen shows in the
//! page's area, and where a dragged screen lands, edge to edge with the
//! others.

use hocus_ui::{Point, Rect, Size};

/// Logical screen space onto the area, in its own logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mapping {
    /// Area pixels per screen pixel.
    pub k: f32,
    /// Where the screens' top-left corner lands in the area.
    pub origin: Point,
    /// The screens' top-left corner, in screen space.
    pub corner: (i32, i32),
}

/// A screen's place and size, logical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Placed {
    fn right(self) -> i32 {
        self.x + self.w
    }

    fn bottom(self) -> i32 {
        self.y + self.h
    }

    fn overlaps(self, o: Placed) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }
}

impl Mapping {
    /// Fits every screen in `area`, centred, with `margin` around and
    /// `below` kept free at the bottom (the area's buttons).
    pub fn fit(screens: &[Placed], area: Size, margin: f32, below: f32) -> Self {
        let left = screens.iter().map(|s| s.x).min().unwrap_or(0);
        let top = screens.iter().map(|s| s.y).min().unwrap_or(0);
        let right = screens.iter().map(|s| s.right()).max().unwrap_or(1);
        let bottom = screens.iter().map(|s| s.bottom()).max().unwrap_or(1);
        let (w, h) = ((right - left).max(1) as f32, (bottom - top).max(1) as f32);
        let room = Size::new(
            (area.w - 2.0 * margin).max(1.0),
            (area.h - 2.0 * margin - below).max(1.0),
        );
        let k = (room.w / w).min(room.h / h);
        Self {
            k,
            origin: Point::new(
                margin + (room.w - w * k) / 2.0,
                margin + (room.h - h * k) / 2.0,
            ),
            corner: (left, top),
        }
    }

    /// Where a screen shows in the area.
    pub fn rect(&self, s: Placed) -> Rect {
        Rect::new(
            self.origin.x + (s.x - self.corner.0) as f32 * self.k,
            self.origin.y + (s.y - self.corner.1) as f32 * self.k,
            s.w as f32 * self.k,
            s.h as f32 * self.k,
        )
    }

    /// The screen-space point under an area point.
    pub fn logical(&self, at: Point) -> (i32, i32) {
        (
            ((at.x - self.origin.x) / self.k).round() as i32 + self.corner.0,
            ((at.y - self.origin.y) / self.k).round() as i32 + self.corner.1,
        )
    }
}

/// Where `moving`, dropped where it is, lands: against the side of another
/// screen it is nearest to, sharing some of that edge, aligned with that
/// screen's nearest edge when within a quarter of its size, never over
/// another screen. Alone, it goes to 0, 0.
pub fn snap(moving: Placed, others: &[Placed]) -> (i32, i32) {
    // The nearest of `to` within `slack`, else `v` as it is.
    let align = |v: i32, to: [i32; 2], slack: i32| {
        to.into_iter()
            .filter(|t| (v - t).abs() <= slack)
            .min_by_key(|t| (v - t).abs())
            .unwrap_or(v)
    };
    let slack_x = moving.w / 4;
    let slack_y = moving.h / 4;
    let mut best: Option<((i32, i32), i64)> = None;
    for o in others {
        // Sharing at least one pixel of the edge, tops or bottoms (lefts or
        // rights) aligned when near.
        let y_along = moving.y.clamp(o.y - moving.h + 1, o.bottom() - 1);
        let x_along = moving.x.clamp(o.x - moving.w + 1, o.right() - 1);
        let y_along = align(y_along, [o.y, o.bottom() - moving.h], slack_y);
        let x_along = align(x_along, [o.x, o.right() - moving.w], slack_x);
        for (x, y) in [
            (o.right(), y_along),
            (o.x - moving.w, y_along),
            (x_along, o.bottom()),
            (x_along, o.y - moving.h),
        ] {
            let there = Placed { x, y, ..moving };
            if others.iter().any(|p| p.overlaps(there)) {
                continue;
            }
            let d = i64::from(x - moving.x).pow(2) + i64::from(y - moving.y).pow(2);
            if best.is_none_or(|(_, b)| d < b) {
                best = Some(((x, y), d));
            }
        }
    }
    best.map_or((0, 0), |(at, _)| at)
}

/// Positions moved so that the screens' top-left corner is 0, 0.
pub fn normalized(positions: &mut [(String, (i32, i32))]) {
    let left = positions.iter().map(|(_, p)| p.0).min().unwrap_or(0);
    let top = positions.iter().map(|(_, p)| p.1).min().unwrap_or(0);
    for (_, p) in positions {
        p.0 -= left;
        p.1 -= top;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAPTOP: Placed = Placed {
        x: 0,
        y: 0,
        w: 1280,
        h: 800,
    };

    #[test]
    fn screens_fit_the_area_and_points_map_back() {
        let external = Placed {
            x: 1280,
            y: 0,
            w: 3440,
            h: 1440,
        };
        let m = Mapping::fit(&[LAPTOP, external], Size::new(1000.0, 300.0), 20.0, 40.0);
        let r = m.rect(external);
        assert!(
            r.x + r.w <= 980.0 + 0.01 && r.y + r.h <= 260.0 + 0.01,
            "{r:?}"
        );
        let laptop = m.rect(LAPTOP);
        assert!((laptop.x + laptop.w - r.x).abs() < 0.01, "edge to edge");
        assert_eq!(m.logical(Point::new(r.x, r.y)), (1280, 0));
    }

    #[test]
    fn a_dropped_screen_lands_edge_to_edge_aligned() {
        // Dropped above the laptop, a little off: it sits on top of it,
        // left edges aligned.
        let external = Placed {
            x: 60,
            y: -1500,
            w: 2560,
            h: 1440,
        };
        assert_eq!(snap(external, &[LAPTOP]), (0, -1440));
        // Dropped far to the right, its bottom near the laptop's: against
        // the laptop's right side, bottoms aligned.
        let dropped = Placed {
            x: 2000,
            y: -200,
            w: 1920,
            h: 1080,
        };
        assert_eq!(snap(dropped, &[LAPTOP]), (1280, -280));
        // Dropped over it: out to the nearest side, never over it.
        let over = Placed {
            x: 100,
            y: 100,
            w: 800,
            h: 600,
        };
        let (x, y) = snap(over, &[LAPTOP]);
        assert!(!Placed { x, y, ..over }.overlaps(LAPTOP));
        assert_eq!(snap(over, &[]), (0, 0));
    }

    #[test]
    fn positions_start_at_the_corner() {
        let mut p = vec![("a".to_string(), (0, 1440)), ("b".to_string(), (-200, 0))];
        normalized(&mut p);
        assert_eq!(
            p,
            [("a".to_string(), (200, 1440)), ("b".to_string(), (0, 0))]
        );
    }
}
