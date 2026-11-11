//! Magnetic edges (docs/design/windows.md §6, §8): while a window moves,
//! or an edge of it is dragged, an edge that comes within [`MAGNET`](hocus_geometry::MAGNET) of a
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
    model::{Edges, Mode, Rect, WindowId, Zone},
};

/// The magnet's geometry, shared with the Settings' trial area (6.14).
pub use hocus_geometry::{ALL, Pull, pull, unpulled};

impl State {
    /// How far window `id`, which the pointer at `pointer` would put at
    /// `rect`, moves to stick: nothing where the pointer would snap it
    /// (§9), which keeps priority.
    /// The zone a window dragged with the pointer at `pointer` snaps to
    /// in work area `area`: none with `[windows] snap-by-drag` off (6.14).
    pub(crate) fn snap_zone(&self, area: Rect, pointer: Point<f64, Logical>) -> Option<Zone> {
        self.config
            .windows
            .snap_by_drag
            .then(|| Zone::at(area, pointer.x, pointer.y))
            .flatten()
    }

    pub(crate) fn move_pull(&self, id: WindowId, rect: Rect, pointer: Point<f64, Logical>) -> Pull {
        let Some(output) = self.space.output_under(pointer).next().cloned() else {
            return Pull::default();
        };
        let Some(area) = self.work_area(&output) else {
            return Pull::default();
        };
        if !self.config.windows.magnet || self.snap_zone(area, pointer).is_some() {
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
        if !self.config.windows.magnet {
            return Pull::default();
        }
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
