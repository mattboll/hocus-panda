//! The geometry of windows on a screen, apart from any compositor: the
//! rectangles, the zones a window snaps to as it is dragged against an
//! edge (docs/design/windows.md §9), the edges a resize moves, and the
//! magnetic edges (§6, §8). The compositor places its windows with it,
//! and the Settings' trial area ("Bureaux et fenêtres", 6.14) moves its
//! drawn windows with the same code, so that the two never differ.

/// Space around windows the desktop places, and between the work area and
/// the output's edges or a bar's reserved space (docs/design/windows.md §2).
pub const GAP: i32 = 8;
/// The pointer this close to a vertical or top edge of the work area
/// snaps to a half or maximises.
pub const SNAP_EDGE: i32 = 24;
/// And this close to a corner, to a quarter.
pub const SNAP_CORNER: i32 = 48;
/// How far a snapped window is dragged before it comes out of its zone,
/// logical pixels.
pub const TEAR_OFF: f64 = 12.0;

/// A rectangle in the global logical space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Smaller by `by` on every side; never negative.
    pub fn inset(self, by: i32) -> Self {
        Self {
            x: self.x + by,
            y: self.y + by,
            width: (self.width - 2 * by).max(0),
            height: (self.height - 2 * by).max(0),
        }
    }

    pub const fn right(self) -> i32 {
        self.x + self.width
    }

    pub const fn bottom(self) -> i32 {
        self.y + self.height
    }

    pub fn contains(self, x: f64, y: f64) -> bool {
        x >= f64::from(self.x)
            && y >= f64::from(self.y)
            && x < f64::from(self.right())
            && y < f64::from(self.bottom())
    }

    pub fn centre(self) -> (f64, f64) {
        (
            f64::from(self.x) + f64::from(self.width) / 2.0,
            f64::from(self.y) + f64::from(self.height) / 2.0,
        )
    }
}

/// Where a window snaps: a half, a quarter, or the whole work area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Zone {
    Left,
    Right,
    Maximized,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Which sides of the work area a zone touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Start,
    End,
    Both,
}

impl Zone {
    /// The zone's rectangle in `area`: halves and quarters keep the gap
    /// between them.
    pub fn rect(self, area: Rect) -> Rect {
        let left_w = (area.width - GAP) / 2;
        let top_h = (area.height - GAP) / 2;
        let (x, w) = match self.horizontal() {
            Side::Start => (area.x, left_w),
            Side::End => (area.x + left_w + GAP, area.width - left_w - GAP),
            Side::Both => (area.x, area.width),
        };
        let (y, h) = match self.vertical() {
            Side::Start => (area.y, top_h),
            Side::End => (area.y + top_h + GAP, area.height - top_h - GAP),
            Side::Both => (area.y, area.height),
        };
        Rect::new(x, y, w.max(0), h.max(0))
    }

    pub fn horizontal(self) -> Side {
        match self {
            Self::Left | Self::TopLeft | Self::BottomLeft => Side::Start,
            Self::Right | Self::TopRight | Self::BottomRight => Side::End,
            Self::Maximized => Side::Both,
        }
    }

    pub fn vertical(self) -> Side {
        match self {
            Self::TopLeft | Self::TopRight => Side::Start,
            Self::BottomLeft | Self::BottomRight => Side::End,
            Self::Left | Self::Right | Self::Maximized => Side::Both,
        }
    }

    pub fn from_sides(horizontal: Side, vertical: Side) -> Self {
        match (horizontal, vertical) {
            (Side::Start, Side::Both) => Self::Left,
            (Side::End, Side::Both) => Self::Right,
            (Side::Start, Side::Start) => Self::TopLeft,
            (Side::End, Side::Start) => Self::TopRight,
            (Side::Start, Side::End) => Self::BottomLeft,
            (Side::End, Side::End) => Self::BottomRight,
            (Side::Both, _) => Self::Maximized,
        }
    }

    /// The zone the pointer at (`x`, `y`) is in while dragging a window:
    /// a corner (within [`SNAP_CORNER`] of it), the left or right edge (a
    /// half) or the top edge (maximised), from the work area's edges; the
    /// pointer may be past them, up to the output's edge.
    pub fn at(area: Rect, x: f64, y: f64) -> Option<Self> {
        let near = |d: f64, within: i32| d < f64::from(within);
        let left = x - f64::from(area.x);
        let right = f64::from(area.right()) - x;
        let top = y - f64::from(area.y);
        let bottom = f64::from(area.bottom()) - y;
        let at_left = near(left, SNAP_EDGE);
        let at_right = near(right, SNAP_EDGE);
        let at_top = near(top, SNAP_EDGE);
        let at_bottom = near(bottom, SNAP_EDGE);
        let corner_x = near(left, SNAP_CORNER) || near(right, SNAP_CORNER);
        let corner_y = near(top, SNAP_CORNER) || near(bottom, SNAP_CORNER);
        let side_x = if left < right { Side::Start } else { Side::End };
        let side_y = if top < bottom { Side::Start } else { Side::End };
        // A corner: at an edge, and within reach of the other one.
        if (at_left || at_right) && corner_y || (at_top || at_bottom) && corner_x {
            return Some(Self::from_sides(side_x, side_y));
        }
        if at_left || at_right {
            return Some(Self::from_sides(side_x, Side::Both));
        }
        at_top.then_some(Self::Maximized)
    }
}

/// Which edges a resize moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Edges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

impl Edges {
    pub const fn any(self) -> bool {
        self.left || self.right || self.top || self.bottom
    }

    /// Its CSS and cursor-shape name: `ew-resize`, `ns-resize`,
    /// `nwse-resize` or `nesw-resize` (§8).
    pub fn cursor_name(self) -> &'static str {
        match (self.left || self.right, self.top || self.bottom) {
            (true, false) => "ew-resize",
            (false, true) => "ns-resize",
            (true, true) if self.left == self.top => "nwse-resize",
            (true, true) => "nesw-resize",
            (false, false) => "default",
        }
    }

    /// The edges of the corner of `rect` nearest to (`x`, `y`): what
    /// `Super`+right-drag resizes.
    pub fn nearest_corner(rect: Rect, x: f64, y: f64) -> Self {
        let (cx, cy) = rect.centre();
        Self {
            left: x < cx,
            right: x >= cx,
            top: y < cy,
            bottom: y >= cy,
        }
    }
}

/// Where a window dragged out of a zone goes: its size before it snapped,
/// under the pointer at (`x`, `y`) at the same place across it (`along`,
/// from 0 at its left to 1 at its right), and as far down as it was held
/// (`held_down` of its `height` then): a header stays under the pointer,
/// and a point further down than the smaller window reaches keeps its
/// share of the height.
pub fn torn_off(
    restore: (i32, i32),
    along: f64,
    (held_down, height): (f64, i32),
    x: f64,
    y: f64,
) -> (i32, i32) {
    let left = x - along.clamp(0.0, 1.0) * f64::from(restore.0);
    let restored = f64::from(restore.1);
    let down = if held_down < restored {
        held_down.max(0.0)
    } else {
        held_down / f64::from(height.max(1)) * restored
    };
    (left.round() as i32, (y - down).round() as i32)
}

/// How near an edge comes before it sticks, logical pixels.
pub const MAGNET: i32 = 12;

/// How far the edges pulled move, each way: none when nothing is near.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pull {
    pub x: Option<i32>,
    pub y: Option<i32>,
}

impl Pull {
    pub fn offset(self) -> (i32, i32) {
        (self.x.unwrap_or(0), self.y.unwrap_or(0))
    }
}

/// How far the `edges` of `rect` move to stick to the edges of `others`
/// (at [`GAP`] from them, or in line with their edges on the same side)
/// or of `area` (flush): the nearest within [`MAGNET`] wins, each way on
/// its own. A neighbour pulls only where the two overlap the other way,
/// or come within reach of sticking that way (the gap and [`MAGNET`]),
/// so a window far above does not.
pub fn pull(rect: Rect, edges: Edges, others: &[Rect], area: Rect) -> Pull {
    let reach = GAP + MAGNET;
    let near = |a0: i32, a1: i32, b0: i32, b1: i32| a0 < b1 + reach && b0 - reach < a1;
    let nearest = |best: &mut Option<i32>, from: i32, to: i32| {
        let d = to - from;
        if d.abs() <= MAGNET && best.is_none_or(|b| d.abs() < b.abs()) {
            *best = Some(d);
        }
    };
    let mut out = Pull::default();
    if edges.left {
        nearest(&mut out.x, rect.x, area.x);
    }
    if edges.right {
        nearest(&mut out.x, rect.right(), area.right());
    }
    if edges.top {
        nearest(&mut out.y, rect.y, area.y);
    }
    if edges.bottom {
        nearest(&mut out.y, rect.bottom(), area.bottom());
    }
    for o in others {
        if near(rect.y, rect.bottom(), o.y, o.bottom()) {
            if edges.left {
                nearest(&mut out.x, rect.x, o.right() + GAP);
                nearest(&mut out.x, rect.x, o.x);
            }
            if edges.right {
                nearest(&mut out.x, rect.right(), o.x - GAP);
                nearest(&mut out.x, rect.right(), o.right());
            }
        }
        if near(rect.x, rect.right(), o.x, o.right()) {
            if edges.top {
                nearest(&mut out.y, rect.y, o.bottom() + GAP);
                nearest(&mut out.y, rect.y, o.y);
            }
            if edges.bottom {
                nearest(&mut out.y, rect.bottom(), o.y - GAP);
                nearest(&mut out.y, rect.bottom(), o.bottom());
            }
        }
    }
    out
}

/// What of a stuck window's move (`moved`, from `before`'s pull to
/// `now`'s) the jelly is told: the pointer's motion, not the magnet's
/// jumps. A window held still by the magnet does not move; where it
/// catches or comes free, the jump goes over rigidly.
pub fn unpulled(moved: (i32, i32), before: Pull, now: Pull) -> (i32, i32) {
    let axis = |moved: i32, before: Option<i32>, now: Option<i32>| {
        if before.is_some() && now.is_some() && moved == 0 {
            0
        } else {
            moved - (now.unwrap_or(0) - before.unwrap_or(0))
        }
    };
    (
        axis(moved.0, before.x, now.x),
        axis(moved.1, before.y, now.y),
    )
}

/// Every edge: a window moved.
pub const ALL: Edges = Edges {
    left: true,
    right: true,
    top: true,
    bottom: true,
};

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect::new(8, 8, 1700, 1064);
    /// A neighbour in the middle of the area.
    const B: Rect = Rect::new(800, 300, 400, 300);

    fn moved(rect: Rect, others: &[Rect]) -> Rect {
        let (dx, dy) = pull(rect, ALL, others, AREA).offset();
        Rect::new(rect.x + dx, rect.y + dy, rect.width, rect.height)
    }

    #[test]
    fn a_window_near_another_sticks_at_the_gap() {
        // Its right edge 10 px short of the gap left of B.
        let a = Rect::new(800 - GAP - 10 - 300, 350, 300, 200);
        assert_eq!(moved(a, &[B]), Rect::new(800 - GAP - 300, 350, 300, 200));
        // Its left edge 5 px into the gap right of B.
        let a = Rect::new(B.right() + 3, 350, 300, 200);
        assert_eq!(moved(a, &[B]).x, B.right() + GAP);
        // Its top 12 px below the gap under B: still within reach.
        let a = Rect::new(900, B.bottom() + GAP + 12, 300, 200);
        assert_eq!(moved(a, &[B]).y, B.bottom() + GAP);
    }

    #[test]
    fn past_the_reach_it_is_free() {
        let a = Rect::new(800 - GAP - 13 - 300, 350, 300, 200);
        assert_eq!(moved(a, &[B]), a);
    }

    #[test]
    fn edges_line_up_with_a_neighbours_on_the_same_side() {
        // Below B at the gap, its left edge 7 px right of B's.
        let a = Rect::new(B.x + 7, B.bottom() + GAP, 300, 200);
        assert_eq!(moved(a, &[B]), Rect::new(B.x, B.bottom() + GAP, 300, 200));
        // Its right edge 4 px past B's.
        let a = Rect::new(B.right() + 4 - 300, B.bottom() + GAP + 2, 300, 200);
        assert_eq!(
            moved(a, &[B]),
            Rect::new(B.right() - 300, B.bottom() + GAP, 300, 200)
        );
    }

    #[test]
    fn the_work_areas_edges_pull_flush() {
        let a = Rect::new(AREA.x + 9, AREA.y - 6, 300, 200);
        assert_eq!(moved(a, &[]), Rect::new(AREA.x, AREA.y, 300, 200));
        let a = Rect::new(AREA.right() - 300 + 11, AREA.bottom() - 200 - 2, 300, 200);
        assert_eq!(
            moved(a, &[]),
            Rect::new(AREA.right() - 300, AREA.bottom() - 200, 300, 200)
        );
    }

    #[test]
    fn a_neighbour_far_the_other_way_does_not_pull() {
        // Right of B, but 120 px above it.
        let a = Rect::new(B.right() + GAP + 3, B.y - 120 - 200, 300, 200);
        assert_eq!(moved(a, &[B]), a);
        // Just out of reach of sticking above it, and just within.
        let a = Rect::new(B.right() + GAP + 3, B.y - GAP - MAGNET - 200, 300, 200);
        assert_eq!(moved(a, &[B]).x, a.x);
        let a = Rect::new(B.right() + GAP + 3, B.y - GAP - MAGNET + 1 - 200, 300, 200);
        assert_eq!(moved(a, &[B]).x, B.right() + GAP);
    }

    #[test]
    fn the_nearest_wins_and_each_way_on_its_own() {
        // B's gap 9 px away, C's 3 px away the same way; and B's bottom
        // aligned 6 px down.
        let c = Rect::new(B.x, B.bottom() + 40, 400, 300);
        let a = Rect::new(B.right() + GAP - 9, B.bottom() - 200 - 6, 300, 200);
        let pulled = pull(a, ALL, &[B, c], AREA);
        assert_eq!(
            pulled,
            Pull {
                x: Some(9),
                y: Some(6)
            }
        );
        let c = Rect::new(a.right() + 3 + GAP, B.y, 200, 300);
        let pulled = pull(a, ALL, &[B, c], AREA);
        assert_eq!(pulled.x, Some(3), "C's left side, 3 px from its right edge");
    }

    #[test]
    fn a_resize_pulls_only_the_edges_dragged() {
        let a = Rect::new(B.x - GAP - 300 - 10, B.y + 20, 300, 200);
        let right = Edges {
            right: true,
            ..Edges::default()
        };
        assert_eq!(
            pull(a, right, &[B], AREA),
            Pull {
                x: Some(10),
                y: None
            }
        );
        let left = Edges {
            left: true,
            ..Edges::default()
        };
        assert_eq!(pull(a, left, &[B], AREA), Pull::default());
    }

    #[test]
    fn the_jelly_is_told_the_pointers_motion_not_the_jumps() {
        let free = Pull::default();
        let stuck = |x| Pull {
            x: Some(x),
            y: None,
        };
        // Free: all of it.
        assert_eq!(unpulled((5, 2), free, free), (5, 2));
        // Catching 4 px ahead of a 3 px move: the 3 px.
        assert_eq!(unpulled((7, 0), free, stuck(4)), (3, 0));
        // Held still while the pointer goes on: nothing.
        assert_eq!(unpulled((0, 1), stuck(4), stuck(1)), (0, 1));
        // Coming free: the pointer's 5 px, not the jump back.
        assert_eq!(unpulled((17, 0), stuck(-12), free), (5, 0));
    }
}
