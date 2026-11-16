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
//!
//! A window's commit that changes nothing it shows (#840: an application
//! committing hundreds of times a second, its frame callbacks alone or
//! its buffer again without damage) asks for no scene: its screens answer
//! its frame callbacks at their next refresh, without building one.

use smithay::{
    backend::renderer::utils::{CommitCounter, RendererSurfaceStateUserData, SurfaceView},
    desktop::{Window, WindowSurfaceType, layer_map_for_output},
    input::pointer::CursorImageStatus,
    output::Output,
    reexports::wayland_server::{Resource, backend::ObjectId, protocol::wl_surface::WlSurface},
    utils::{Logical, Point, Rectangle, Serial, Size},
    wayland::{
        compositor::{
            TraversalAction, get_children, get_parent, is_sync_subsurface, with_states,
            with_surface_tree_downward,
        },
        shell::xdg::{SurfaceCachedState, XdgToplevelSurfaceData},
    },
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
    /// The screens whose windows committed nothing new, with frame
    /// callbacks to answer at their next refresh, without a scene (#840).
    owed: Vec<Output>,
    /// A client committed, whether a screen shows it or not: what a
    /// share or a recording of a window looks at.
    any: bool,
}

/// What the commits asked of the screens since they last drew.
#[derive(Debug, Default)]
pub struct Asked {
    /// The screens that build a scene.
    pub scenes: Vec<Output>,
    /// Those that only tell their clients they refreshed, unless they
    /// build a scene anyway.
    pub owed: Vec<Output>,
}

impl Asked {
    /// Whether nothing was asked.
    pub fn is_empty(&self) -> bool {
        self.scenes.is_empty() && self.owed.is_empty()
    }
}

/// What a client's commit changed of what its surface shows (#840).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Change {
    /// Something, or what cannot be told: its screens draw again.
    Shown,
    /// Nothing of a window: its screens answer its frame callbacks at
    /// their next refresh, without a scene.
    Nothing,
    /// A synchronized subsurface's state, applied with its parent's
    /// commit, which looks at it then.
    WithParent,
}

impl Commits {
    /// Whether a client committed, or a screen was asked for, since the
    /// screens last drew.
    pub fn pending(&self) -> bool {
        self.any || !self.screens.is_empty() || !self.owed.is_empty()
    }

    /// Whether a screen was asked for a scene since the screens last drew.
    pub fn on_screens(&self) -> bool {
        !self.screens.is_empty()
    }

    /// The screens asked for, forgotten with the commits.
    pub fn take(&mut self) -> Asked {
        self.any = false;
        Asked {
            scenes: std::mem::take(&mut self.screens),
            owed: std::mem::take(&mut self.owed),
        }
    }

    fn on(&mut self, output: Output) {
        if !self.screens.contains(&output) {
            self.screens.push(output);
        }
    }

    fn owe(&mut self, output: Output) {
        if !self.owed.contains(&output) {
            self.owed.push(output);
        }
    }
}

/// What a surface showed at its last commit: what a commit can change
/// on the screen (#840).
#[derive(Debug, Clone, PartialEq)]
struct Look {
    /// Its damage, counted: new pixels.
    damage: CommitCounter,
    /// Its buffer's size, crop and scale, and its place in its parent;
    /// `None` without a buffer.
    view: Option<SurfaceView>,
    opaque: Vec<Rectangle<i32, Logical>>,
    /// Its subsurfaces, in their order.
    children: Vec<ObjectId>,
    /// A window's geometry and its bounds.
    geometry: Option<Rectangle<i32, Logical>>,
    bounds: (Size<i32, Logical>, Size<i32, Logical>),
    /// The last configure a window took.
    configure: Option<Serial>,
}

#[derive(Debug, Default)]
struct Seen(RefCell<Option<Look>>);

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

    /// What `surface`'s commit changed: nothing only for a mapped
    /// window's surfaces whose buffers, damage, sizes, places,
    /// subsurfaces and configure are as they were at their last commit.
    /// Popups, layer surfaces, cursors and the lock's draw again at each
    /// commit as before.
    pub(crate) fn commit_change(&self, surface: &WlSurface) -> Change {
        if is_sync_subsurface(surface) {
            return Change::WithParent;
        }
        // Each surface's look is kept at every commit, a window's or
        // not: the first commit of a window that was something else
        // compares with what it showed then.
        let same = looks_the_same(surface);
        let mut root = surface.clone();
        while let Some(parent) = get_parent(&root) {
            root = parent;
        }
        if same && self.window_of_surface(&root).is_some() {
            Change::Nothing
        } else {
            Change::Shown
        }
    }

    /// `surface`, a window's, committed nothing new: the screens it shows
    /// on answer its frame callbacks at their next refresh (#840).
    pub(crate) fn frames_owed_for(&mut self, surface: &WlSurface) {
        // A share or a recording of it answers them too.
        self.commits.any = true;
        if self.redraw {
            return;
        }
        let root = self.root_surface(surface);
        let window = self.window_of_surface(&root);
        let screens = match window {
            Some(id) if !self.drawn_elsewhere() => match self.window_area(id, self.now()) {
                Some(area) => self.screens_over(area),
                // Out of sight: nothing to answer, as at a frame drawn.
                None => Vec::new(),
            },
            _ => self.space.outputs().cloned().collect(),
        };
        for output in screens {
            self.commits.owe(output);
        }
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

/// Whether the surfaces of `surface`'s tree show what they showed at
/// their last commit, each look kept for the next.
fn looks_the_same(surface: &WlSurface) -> bool {
    let mut tree = Vec::new();
    with_surface_tree_downward(
        surface,
        (),
        |_, _, ()| TraversalAction::DoChildren(()),
        |s, _, ()| tree.push(s.clone()),
        |_, _, ()| true,
    );
    let mut same = true;
    for s in tree {
        // Read outside `with_states`, which holds the surface's lock.
        let children: Vec<ObjectId> = get_children(&s).iter().map(Resource::id).collect();
        same &= with_states(&s, |states| {
            let look = states
                .data_map
                .get::<RendererSurfaceStateUserData>()
                .and_then(|r| r.lock().ok())
                .map(|r| {
                    (
                        r.current_commit(),
                        r.view(),
                        r.opaque_regions().map(<[_]>::to_vec),
                    )
                });
            let Some((damage, view, opaque)) = look else {
                return false;
            };
            // A window's; none is made for a surface without one.
            let cached = if states.cached_state.has::<SurfaceCachedState>() {
                *states.cached_state.get::<SurfaceCachedState>().current()
            } else {
                SurfaceCachedState::default()
            };
            let configure = states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .and_then(|d| d.lock().ok().map(|d| d.current_serial))
                .flatten();
            let look = Look {
                damage,
                view,
                opaque: opaque.unwrap_or_default(),
                children,
                geometry: cached.geometry,
                bounds: (cached.min_size, cached.max_size),
                configure,
            };
            states.data_map.insert_if_missing(Seen::default);
            let Some(seen) = states.data_map.get::<Seen>() else {
                return false;
            };
            seen.0.replace(Some(look.clone())).as_ref() == Some(&look)
        });
    }
    same
}
