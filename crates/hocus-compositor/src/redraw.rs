//! Which screens draw again after a client's commit (#826): only those
//! its surface shows on, not every one. A screen builds a scene, the
//! costly part of a frame, only when something of it changed; with two
//! screens, a video on one no longer has the other build its scene at
//! each of its frames to find nothing new.
//!
//! What the compositor changes itself asks every screen for a frame
//! (`State::redraw`), but for a motion of one screen alone, as the
//! garland across its top (`State::redraw_on`); so does a commit whenever its surface may show
//! elsewhere than where it is: the overview, Alt+Tab's thumbnails, the
//! actions palette, a window moving, the lock, or a surface the desktop
//! does not place itself.

use smithay::{
    desktop::{Window, WindowSurfaceType, layer_map_for_output},
    input::pointer::CursorImageStatus,
    output::Output,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle, Size},
};
use std::cell::RefCell;

use crate::{State, model::WindowId};

/// How far around the pointer a client's cursor may draw (logical
/// pixels): the largest cursors, at their hotspot's farthest.
const CURSOR_REACH: f64 = 256.0;

/// The screens commits, and the compositor's motions of one screen alone,
/// asked to draw again since the screens last drew, besides every screen
/// (`State::redraw`).
#[derive(Debug, Default)]
pub struct Commits {
    screens: Vec<Output>,
    /// A client committed, whether a screen shows it or not: what a
    /// share or a recording of a window looks at.
    any: bool,
}

impl Commits {
    /// Whether a client committed, or a screen was asked for, since the
    /// screens last drew.
    pub fn pending(&self) -> bool {
        self.any || !self.screens.is_empty()
    }

    /// Whether a screen was asked for since the screens last drew.
    pub fn on_screens(&self) -> bool {
        !self.screens.is_empty()
    }

    /// The screens asked for, forgotten with the commits.
    pub fn take(&mut self) -> Vec<Output> {
        self.any = false;
        std::mem::take(&mut self.screens)
    }

    fn on(&mut self, output: Output) {
        if !self.screens.contains(&output) {
            self.screens.push(output);
        }
    }
}

/// Where a window was drawn when it last committed, and the screens its
/// surfaces covered then: a commit that shrinks it, or closes a popup,
/// draws again where they were.
#[derive(Debug, Default)]
struct Drawn(RefCell<Option<Covered>>);

#[derive(Debug)]
struct Covered {
    visual: Rectangle<f64, Logical>,
    screens: Vec<Output>,
}

impl State {
    /// `output` draws again for what moves on it alone; the others build
    /// no scene for it.
    pub(crate) fn redraw_on(&mut self, output: Output) {
        if !self.redraw {
            self.commits.on(output);
        }
    }

    /// An effect moved on `screens`: they draw again (#826); none
    /// known, every screen.
    pub(crate) fn moved_on_screens(&mut self, screens: Vec<Output>) {
        if screens.is_empty() {
            let all: Vec<Output> = self.space.outputs().cloned().collect();
            self.frames.moved_on.extend(all);
        } else {
            self.frames.moved_on.extend(screens);
        }
    }

    /// Effect `which` moved on `screens`: they draw again, and those it
    /// moved on at its last step, which it may have left (#826).
    pub(crate) fn effect_moved(&mut self, which: &'static str, screens: Vec<Output>) {
        let before = self.fx.was_on.insert(which, screens.clone());
        let mut all = screens;
        for output in before.into_iter().flatten() {
            if !all.contains(&output) {
                all.push(output);
            }
        }
        self.moved_on_screens(all);
    }

    /// The screens what is drawn on windows `ids` reaches, `around` them
    /// (logical pixels): every screen while windows are drawn elsewhere
    /// than at their place.
    pub(crate) fn screens_around(
        &self,
        ids: impl IntoIterator<Item = WindowId>,
        around: f64,
    ) -> Vec<Output> {
        if self.drawn_elsewhere() {
            return self.space.outputs().cloned().collect();
        }
        let now = self.now();
        let mut screens: Vec<Output> = Vec::new();
        for id in ids {
            let Some(area) = self.window_area(id, now) else {
                continue;
            };
            let reach = Point::from((around, around));
            let area = Rectangle::new(
                area.loc - reach,
                area.size + Size::from((2.0 * around, 2.0 * around)),
            );
            for output in self.screens_over(area) {
                if !screens.contains(&output) {
                    screens.push(output);
                }
            }
        }
        screens
    }

    /// The screens whose wallpaper shows (on, not as good as unseen)
    /// and `also` takes: where a wallpaper's effect moves.
    pub(crate) fn walls_seen(&self, also: impl Fn(&Self, &Output) -> bool) -> Vec<Output> {
        self.space
            .outputs()
            .filter(|o| self.output_powered(o) && also(self, o))
            .cloned()
            .collect()
    }

    /// Where window `id` is drawn (global), on its strip; `None` out of
    /// sight.
    fn window_area(
        &self,
        id: WindowId,
        now: std::time::Duration,
    ) -> Option<Rectangle<f64, Logical>> {
        self.strip_offset(id, now)
            .zip(self.look(id, now))
            .map(|((dx, _), look)| {
                let mut visual = look.visual;
                visual.loc.x += dx;
                visual
            })
    }

    /// `surface` committed: the screens it shows on draw again.
    pub(crate) fn redraw_for(&mut self, surface: &WlSurface) {
        self.commits.any = true;
        match self.commit_screens(surface) {
            Some(_) if self.redraw => {}
            Some(screens) => {
                for output in screens {
                    self.commits.on(output);
                }
            }
            None => self.redraw = true,
        }
    }

    /// The screens `surface` shows on; `None` when it may show on any.
    fn commit_screens(&self, surface: &WlSurface) -> Option<Vec<Output>> {
        let root = self.root_surface(surface);
        // Looked at first, whatever else is drawn: where a window's
        // surfaces are is kept for its next commit.
        let window = self
            .window_of_surface(&root)
            .and_then(|id| Some((id, &self.windows.get(&id)?.window)))
            .map(|(id, window)| self.window_screens(id, window));
        if self.drawn_elsewhere() {
            return None;
        }
        if let Some(screens) = window {
            return screens;
        }
        if let CursorImageStatus::Surface(cursor) = &self.pointer.cursor
            && *cursor == root
        {
            let at = self.seat.get_pointer()?.current_location();
            let reach = Point::from((CURSOR_REACH, CURSOR_REACH));
            let around = Rectangle::new(at - reach, Size::from((2.0, 2.0)).upscale(CURSOR_REACH));
            return Some(self.screens_over(around));
        }
        self.space
            .outputs()
            .find(|o| {
                layer_map_for_output(o)
                    .layer_for_surface(&root, WindowSurfaceType::TOPLEVEL)
                    .is_some()
            })
            .map(|o| vec![o.clone()])
    }

    /// Whether windows are drawn now other than at their place on their
    /// screen: in the overview, as Alt+Tab's thumbnails, beside the
    /// actions palette, moving, or pulled back by depth's camera; or
    /// locked.
    pub(crate) fn drawn_elsewhere(&self) -> bool {
        let now = self.now();
        self.overview.is_some()
            || self.switching.open.is_some()
            || self.switching.out.is_some()
            || self.actions.target.is_some()
            || self.moving.is_some()
            || self.wobble.is_some()
            || self.locked()
            || (self.fx.depth.is_some()
                && self.space.outputs().any(|o| self.depth_pull(o, now) < 1.0))
    }

    /// The screens window `id` covers with its surfaces (its popups and
    /// subsurfaces, a shadow it draws itself), and those it covered at
    /// its last commit; `None` when it is drawn elsewhere than then: its
    /// frame and its shadow moved with it.
    fn window_screens(&self, id: WindowId, window: &Window) -> Option<Vec<Output>> {
        let visual = self.window_area(id, self.now());
        let data = window.user_data();
        data.insert_if_missing(Drawn::default);
        let mut drawn = data.get::<Drawn>()?.0.borrow_mut();
        let before = drawn.take();
        let Some(visual) = visual else {
            // Out of sight: where it last was, once.
            return Some(before.map(|b| b.screens).unwrap_or_default());
        };
        // Its surfaces reach past its geometry by as much as they do at
        // its place.
        let (bbox, geometry) = (window.bbox_with_popups(), window.geometry());
        let left = f64::from((geometry.loc.x - bbox.loc.x).max(0));
        let top = f64::from((geometry.loc.y - bbox.loc.y).max(0));
        let right = f64::from((bbox.loc.x + bbox.size.w - geometry.loc.x - geometry.size.w).max(0));
        let bottom =
            f64::from((bbox.loc.y + bbox.size.h - geometry.loc.y - geometry.size.h).max(0));
        let area = Rectangle::new(
            visual.loc - Point::from((left + 1.0, top + 1.0)),
            visual.size + Size::from((left + right + 2.0, top + bottom + 2.0)),
        );
        let now_on = self.screens_over(area);
        *drawn = Some(Covered {
            visual,
            screens: now_on.clone(),
        });
        let mut screens = before.filter(|b| b.visual == visual)?.screens;
        for output in now_on {
            if !screens.contains(&output) {
                screens.push(output);
            }
        }
        Some(screens)
    }

    /// The screens `area` (global) overlaps.
    pub(crate) fn screens_over(&self, area: Rectangle<f64, Logical>) -> Vec<Output> {
        self.space
            .outputs()
            .filter(|o| {
                self.space
                    .output_geometry(o)
                    .is_some_and(|g| g.to_f64().overlaps(area))
            })
            .cloned()
            .collect()
    }
}
