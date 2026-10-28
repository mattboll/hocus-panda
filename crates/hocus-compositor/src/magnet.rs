//! Magnetic edges (docs/design/windows.md §6, §8): while a window moves,
//! or an edge of it is dragged, an edge that comes within [`MAGNET`] of a
//! neighbour's sticks at the gap snapped windows keep, or in line with
//! the neighbour's edge on the same side, and an edge near the work
//! area's sticks flush with it. The pull is worked out afresh from where
//! the pointer would put the window, so dragging on frees it.

use smithay::{
    output::Output,
    utils::{Logical, Point},
};

use crate::{
    State,
    model::{Edges, GAP, Mode, Rect, WindowId, Zone},
};

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

const ALL: Edges = Edges {
    left: true,
    right: true,
    top: true,
    bottom: true,
};

impl State {
    /// How far window `id`, which the pointer at `pointer` would put at
    /// `rect`, moves to stick: nothing where the pointer would snap it
    /// (§9), which keeps priority.
    pub(crate) fn move_pull(&self, id: WindowId, rect: Rect, pointer: Point<f64, Logical>) -> Pull {
        let Some(output) = self.space.output_under(pointer).next().cloned() else {
            return Pull::default();
        };
        let Some(area) = self.work_area(&output) else {
            return Pull::default();
        };
        if Zone::at(area, pointer.x, pointer.y).is_some() {
            return Pull::default();
        }
        pull(rect, ALL, &self.magnet_neighbours(id, &output, &[]), area)
    }

    /// How far the `edges` of window `id` resized to `rect` move to stick;
    /// the neighbours that follow them (`follow`) do not pull.
    pub(crate) fn resize_pull(
        &self,
        id: WindowId,
        rect: Rect,
        edges: Edges,
        follow: &[WindowId],
    ) -> Pull {
        let Some(output) = self.output_of(id) else {
            return Pull::default();
        };
        let Some(area) = self.work_area(&output) else {
            return Pull::default();
        };
        pull(
            rect,
            edges,
            &self.magnet_neighbours(id, &output, follow),
            area,
        )
    }

    /// The other windows that show on `output`'s desktop, but `skip`:
    /// maximised and fullscreen ones cover the work area, whose own edges
    /// pull already.
    fn magnet_neighbours(&self, id: WindowId, output: &Output, skip: &[WindowId]) -> Vec<Rect> {
        self.windows
            .keys()
            .copied()
            .filter(|o| *o != id && !skip.contains(o) && self.model.is_shown(*o))
            .filter(|o| match self.model.mode(*o) {
                Mode::Floating => true,
                Mode::Snapped(zone) => zone != Zone::Maximized,
                Mode::Fullscreen => false,
            })
            .filter(|o| self.output_of(*o).as_ref() == Some(output))
            .filter_map(|o| self.rect_of(o))
            .collect()
    }
}

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
