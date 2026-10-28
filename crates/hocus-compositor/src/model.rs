//! The windows as the desktop knows them, apart from smithay: their ids,
//! their stacking order, which one has the focus, and where a new one goes.
//! Plain data and decisions, tested without a compositor; `State` carries
//! them out on smithay's windows. Slice 1's behaviours (desktops, snapping,
//! motion) build on this.

/// Space around windows the desktop places, and between the work area and
/// the output's edges or a bar's reserved space (docs/design/windows.md §2).
pub const GAP: i32 = 8;

/// A window, for as long as it is mapped. Ids are never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(pub u64);

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
}

/// What layer surfaces leave of an output (its non-exclusive zone, in the
/// global space) less the gap: where the desktop places windows.
pub fn work_area(non_exclusive: Rect) -> Rect {
    non_exclusive.inset(GAP)
}

/// Where a window of `size` goes to sit at the centre of `area`: its
/// top-left corner. A window larger than the area keeps its top-left corner
/// in it, so its header stays reachable.
pub fn centred(area: Rect, size: (i32, i32)) -> (i32, i32) {
    let along = |start: i32, room: i32, length: i32| start + ((room - length) / 2).max(0);
    (
        along(area.x, area.width, size.0),
        along(area.y, area.height, size.1),
    )
}

/// The mapped windows, bottom first, and the focus.
#[derive(Debug, Default)]
pub struct Model {
    stack: Vec<WindowId>,
    focused: Option<WindowId>,
    next: u64,
}

impl Model {
    /// A new window, on top. It does not take the focus by itself: the
    /// policy for that is the caller's (slice 1 §4).
    pub fn add(&mut self) -> WindowId {
        self.next += 1;
        let id = WindowId(self.next);
        self.stack.push(id);
        id
    }

    /// A window went; the focus goes to the one now on top.
    pub fn remove(&mut self, id: WindowId) {
        self.stack.retain(|w| *w != id);
        if self.focused == Some(id) {
            self.focused = self.stack.last().copied();
        }
    }

    /// Focuses and raises `id`; `None` takes the focus from every window.
    pub fn focus(&mut self, id: Option<WindowId>) {
        if let Some(id) = id {
            if !self.stack.contains(&id) {
                return;
            }
            self.stack.retain(|w| *w != id);
            self.stack.push(id);
        }
        self.focused = id;
    }

    pub fn focused(&self) -> Option<WindowId> {
        self.focused
    }

    /// Bottom first.
    pub fn stack(&self) -> &[WindowId] {
        &self.stack
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_work_area_keeps_a_gap_inside_what_layers_leave() {
        // A 1920×1080 output with a 40 px bar along its top.
        let zone = Rect::new(0, 40, 1920, 1040);
        assert_eq!(work_area(zone), Rect::new(8, 48, 1904, 1024));
        assert_eq!(Rect::new(0, 0, 10, 10).inset(8), Rect::new(8, 8, 0, 0));
    }

    #[test]
    fn a_new_window_sits_at_the_centre_of_the_work_area() {
        let area = Rect::new(8, 48, 1904, 1024);
        assert_eq!(centred(area, (800, 600)), (8 + 552, 48 + 212));
        // Too large: its top-left corner stays in the area.
        assert_eq!(centred(area, (3000, 2000)), (8, 48));
    }

    #[test]
    fn focus_raises_and_follows_the_top_when_its_window_goes() {
        let mut model = Model::default();
        let a = model.add();
        let b = model.add();
        assert_eq!(model.focused(), None);
        model.focus(Some(a));
        assert_eq!(model.stack(), &[b, a]);
        assert_eq!(model.focused(), Some(a));
        model.remove(a);
        assert_eq!(model.focused(), Some(b));
        // A window not in the model is never focused.
        model.focus(Some(WindowId(99)));
        assert_eq!(model.focused(), Some(b));
        model.focus(None);
        assert_eq!(model.focused(), None);
        assert_eq!(model.stack(), &[b]);
    }
}
