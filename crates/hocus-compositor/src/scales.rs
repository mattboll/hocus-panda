//! Screens of different scales side by side (docs/design/windows.md §12,
//! "Screens of different scales", #419): the scale a window is asked to
//! draw at, a window too big for the screen it is dropped on, and the
//! pointer at an edge no screen touches.

use smithay::{desktop::Window, output::Output};

use crate::{
    model::{self, Mode, Rect, WindowId},
    state::State,
    windows::rect_of,
};

/// A window shrunk to fit a screen: its size before, and the size it was
/// given, by which a resize meanwhile is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fitted {
    pub before: (i32, i32),
    pub to: (i32, i32),
}

/// What a floating window of `size` dropped in `area` becomes: its new
/// size, and what it then remembers. `fitted` is what it remembered; a
/// size other than the one it was given means the user resized it since,
/// and its size from before is forgotten. `min` is its client's least
/// size, which it is never shrunk below.
pub fn fit(
    size: (i32, i32),
    fitted: Option<Fitted>,
    area: Rect,
    min: (i32, i32),
) -> ((i32, i32), Option<Fitted>) {
    let wanted = fitted.filter(|f| f.to == size).map_or(size, |f| f.before);
    let w = wanted.0.min(area.width).max(min.0.min(wanted.0));
    let h = wanted.1.min(area.height).max(min.1.min(wanted.1));
    let fitted = ((w, h) != wanted).then_some(Fitted {
        before: wanted,
        to: (w, h),
    });
    ((w, h), fitted)
}

/// Where the pointer moved from `from` to `to` goes on the screens at
/// `areas` (global): `to` on a screen; past an edge of its screen where no
/// screen touches, the nearest point of a screen beyond that edge's line
/// (screens of different heights side by side); elsewhere, the nearest
/// point of any screen, so that the layout's outer edges stop it.
pub fn onto_screens(areas: &[Rect], from: (f64, f64), to: (f64, f64)) -> (f64, f64) {
    if areas.iter().any(|a| a.contains(to.0, to.1)) {
        return to;
    }
    let nearest_in = |a: &Rect| {
        (
            to.0.clamp(f64::from(a.x), f64::from(a.right() - 1)),
            to.1.clamp(f64::from(a.y), f64::from(a.bottom() - 1)),
        )
    };
    let nearest = |points: &mut dyn Iterator<Item = (f64, f64)>| {
        points.min_by(|a, b| {
            let d = |p: &(f64, f64)| (p.0 - to.0).powi(2) + (p.1 - to.1).powi(2);
            d(a).total_cmp(&d(b))
        })
    };
    if let Some(on) = areas.iter().find(|a| a.contains(from.0, from.1)) {
        let (x, y) = to;
        let beyond = areas.iter().filter(|a| {
            *a != on
                && ((x < f64::from(on.x) && a.right() <= on.x)
                    || (x >= f64::from(on.right()) && a.x >= on.right())
                    || (y < f64::from(on.y) && a.bottom() <= on.y)
                    || (y >= f64::from(on.bottom()) && a.y >= on.bottom()))
        });
        if let Some(point) = nearest(&mut beyond.map(nearest_in)) {
            return point;
        }
    }
    nearest(&mut areas.iter().map(nearest_in)).unwrap_or(to)
}

impl State {
    /// The output whose scale and transform a window is told to draw at:
    /// of the screens its geometry overlaps, the one at the largest scale,
    /// so that it is sharp on each and reduced where the scale is smaller.
    /// `None` when it overlaps none.
    pub(crate) fn drawing_output(&self, window: &Window) -> Option<Output> {
        let rect = rect_of(self.space.element_geometry(window)?);
        self.space
            .outputs()
            .filter(|o| {
                self.space
                    .output_geometry(o)
                    .is_some_and(|g| overlaps(rect_of(g), rect))
            })
            .max_by(|a, b| {
                let s = |o: &Output| o.current_scale().fractional_scale();
                s(a).total_cmp(&s(b))
            })
            .cloned()
    }

    /// Window `id`, floating, moved onto another screen: too big for its
    /// work area, it shrinks to fit, wholly inside it; shrunk before and
    /// fitting again, it takes its size back, unless the user resized it
    /// since.
    pub(crate) fn fit_to_screen(&mut self, id: WindowId) {
        if self.model.mode(id) != Mode::Floating {
            return;
        }
        let (Some(rect), Some(area)) = (self.target_rect(id), self.area_of(id)) else {
            return;
        };
        let Some(managed) = self.windows.get(&id) else {
            return;
        };
        let (min, _) = crate::windows::size_limits(&managed.window);
        let min = (min.0.max(model::MIN_SIZE.0), min.1.max(model::MIN_SIZE.1));
        let ((w, h), fitted) = fit((rect.width, rect.height), managed.fitted, area, min);
        if let Some(managed) = self.windows.get_mut(&id) {
            managed.fitted = fitted;
        }
        if (w, h) == (rect.width, rect.height) {
            return;
        }
        let x = rect.x.min(area.right() - w).max(area.x);
        let y = rect.y.min(area.bottom() - h).max(area.y);
        tracing::debug!(?id, from = ?(rect.width, rect.height), to = ?(w, h), "fitted to its screen");
        self.configure(id, Rect::new(x, y, w, h));
    }
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect::new(8, 8, 1208, 784);

    #[test]
    fn a_window_too_big_shrinks_and_takes_its_size_back() {
        let (size, fitted) = fit((1500, 1000), None, AREA, (120, 80));
        assert_eq!(size, (1208, 784));
        assert_eq!(
            fitted,
            Some(Fitted {
                before: (1500, 1000),
                to: (1208, 784)
            })
        );
        let wide = Rect::new(1288, 8, 1708, 1064);
        assert_eq!(fit(size, fitted, wide, (120, 80)), ((1500, 1000), None));
        // Too big there too: still shrunk, its size from before kept.
        let small = Rect::new(0, 0, 1000, 700);
        assert_eq!(
            fit(size, fitted, small, (120, 80)),
            (
                (1000, 700),
                Some(Fitted {
                    before: (1500, 1000),
                    to: (1000, 700)
                })
            )
        );
    }

    #[test]
    fn a_window_resized_since_keeps_its_new_size() {
        let fitted = Some(Fitted {
            before: (1500, 1000),
            to: (1208, 784),
        });
        let wide = Rect::new(1288, 8, 1708, 1064);
        assert_eq!(fit((900, 600), fitted, wide, (120, 80)), ((900, 600), None));
    }

    #[test]
    fn a_window_is_never_shrunk_below_its_least_size() {
        let (size, _) = fit((1500, 1000), None, AREA, (1300, 80));
        assert_eq!(size, (1300, 784));
    }

    /// A screen at 2, 1280×800, left of one at 1, 1920×1080, tops aligned.
    const SCREENS: [Rect; 2] = [Rect::new(0, 0, 1280, 800), Rect::new(1280, 0, 1920, 1080)];

    #[test]
    fn the_pointer_slides_onto_the_screen_beyond_a_step() {
        assert_eq!(
            onto_screens(&SCREENS, (1290.0, 1000.0), (1270.0, 1000.0)),
            (1270.0, 799.0)
        );
        // Across the edge they share, it simply goes on.
        assert_eq!(
            onto_screens(&SCREENS, (1290.0, 400.0), (1270.0, 400.0)),
            (1270.0, 400.0)
        );
    }

    #[test]
    fn the_layouts_outer_edges_stop_it() {
        assert_eq!(
            onto_screens(&SCREENS, (1290.0, 1075.0), (1290.0, 1090.0)),
            (1290.0, 1079.0)
        );
        assert_eq!(
            onto_screens(&SCREENS, (600.0, 790.0), (600.0, 820.0)),
            (600.0, 799.0)
        );
        assert_eq!(
            onto_screens(&SCREENS, (3190.0, 500.0), (3210.0, 500.0)),
            (3199.0, 500.0)
        );
    }
}
