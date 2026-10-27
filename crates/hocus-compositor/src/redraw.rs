//! Which outputs build their scene on the next frame (#244). A change asks
//! for the outputs it shows on: a window's, a panel's screen, the screen
//! under the pointer; what every screen shows (the theme, the accent, the
//! lock, the configuration) asks for all of them. An output that was not
//! asked for builds no scene, and its clients get no frame callback from
//! it: nothing of theirs was drawn there.

use smithay::{
    desktop::{Space, Window, WindowSurfaceType, find_popup_root_surface, layer_map_for_output},
    input::pointer::CursorImageStatus,
    output::Output,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle},
    wayland::compositor::get_parent,
    wayland::seat::WaylandFocus,
};

use crate::State;

/// How far around a window what is drawn of it reaches: its shadow, its
/// ring, its bar, the Matrix theme's trail. An output that close builds its
/// scene again when the window changes.
const WINDOW_REACH: i32 = 128;

/// How far around the pointer its image reaches, whatever its size and
/// hot spot.
const CURSOR_REACH: i32 = 256;

/// What the next frame redraws.
#[derive(Debug, Default)]
enum Wanted {
    /// Nothing changed.
    #[default]
    Nothing,
    /// The outputs listed.
    Outputs,
    /// Every output, those mirroring another included.
    All,
}

/// The outputs asked to build their scene on the next frame.
#[derive(Debug)]
pub struct Redraw {
    wanted: Wanted,
    /// The outputs asked for, while `wanted` is `Outputs`; the list keeps
    /// its room from frame to frame, so that asking allocates nothing.
    outputs: Vec<Output>,
    /// The outputs around the pointer too, found as the frame is drawn:
    /// asked from where the pointer cannot be read (its own handlers hold
    /// it).
    pointer: bool,
}

impl Default for Redraw {
    /// Every output draws its first frame.
    fn default() -> Self {
        Self {
            wanted: Wanted::All,
            outputs: Vec::new(),
            pointer: false,
        }
    }
}

impl Redraw {
    /// Every output redraws: what changed shows on all of them.
    pub fn all(&mut self) {
        self.wanted = Wanted::All;
        self.outputs.clear();
    }

    /// `output` redraws.
    pub fn output(&mut self, output: &Output) {
        match self.wanted {
            Wanted::All => {}
            Wanted::Nothing => {
                self.outputs.clear();
                self.outputs.push(output.clone());
                self.wanted = Wanted::Outputs;
            }
            Wanted::Outputs => {
                if !self.outputs.contains(output) {
                    self.outputs.push(output.clone());
                }
            }
        }
    }

    /// The output named `name` in `space` redraws; every output when no
    /// output there has that name (one mirroring another, one going).
    pub fn named(&mut self, space: &Space<Window>, name: &str) {
        match space.outputs().find(|o| o.name() == name) {
            Some(output) => self.output(output),
            None => self.all(),
        }
    }

    /// The outputs around the pointer redraw, wherever it is when the
    /// frame is drawn.
    pub fn around_pointer(&mut self) {
        self.pointer = true;
    }

    /// Whether any output is to redraw.
    pub fn is_pending(&self) -> bool {
        self.pointer || !matches!(self.wanted, Wanted::Nothing)
    }

    /// Whether `output` itself was asked for (not through a screen it
    /// mirrors: see [`State::redraws`]).
    pub fn wants(&self, output: &Output) -> bool {
        match self.wanted {
            Wanted::Nothing => false,
            Wanted::Outputs => self.outputs.contains(output),
            Wanted::All => true,
        }
    }

    /// A frame went out: what was asked is drawn.
    pub fn done(&mut self) {
        self.wanted = Wanted::Nothing;
        self.outputs.clear();
        self.pointer = false;
    }
}

impl State {
    /// Whether `output` builds its scene on this frame: asked for, or
    /// showing the picture of a screen that is (#125).
    pub fn redraws(&self, output: &Output) -> bool {
        self.asked(output)
            || self
                .mirrors
                .iter()
                .find(|m| m.output == *output)
                .is_some_and(|m| {
                    self.space
                        .outputs()
                        .any(|o| o.name() == m.source && self.asked(o))
                })
    }

    /// Whether `output` itself was asked for, or is around the pointer
    /// while that was.
    fn asked(&self, output: &Output) -> bool {
        self.redraw.wants(output) || (self.redraw.pointer && self.near_pointer(output))
    }

    /// Whether the pointer's image may reach `output`.
    fn near_pointer(&self, output: &Output) -> bool {
        let (Some(pointer), Some(area)) =
            (self.seat.get_pointer(), self.space.output_geometry(output))
        else {
            return false;
        };
        let at = Rectangle::new(pointer.current_location().to_i32_round(), (0, 0).into());
        reach(at, CURSOR_REACH).overlaps(area)
    }

    /// The outputs `rect` (global, logical) overlaps redraw.
    pub fn redraw_rect(&mut self, rect: Rectangle<i32, Logical>) {
        let Self { space, redraw, .. } = self;
        for output in space.outputs() {
            if space
                .output_geometry(output)
                .is_some_and(|area| area.overlaps(rect))
            {
                redraw.output(output);
            }
        }
    }

    /// The outputs around `at` (global, logical) redraw: the pointer's,
    /// and its neighbour's when its image reaches across.
    pub fn redraw_around(&mut self, at: Point<f64, Logical>) {
        let at = Rectangle::new(at.to_i32_round(), (0, 0).into());
        self.redraw_rect(reach(at, CURSOR_REACH));
    }

    /// The outputs `window` shows on redraw: where it is now, its shadow
    /// included, and where the last frame showed it, should it have moved
    /// since. A window shown elsewhere too (live in the switcher, in an
    /// agent's slot of the journal) redraws every output.
    pub fn redraw_window(&mut self, window: &Window) {
        if self.windows_shown_elsewhere() {
            self.redraw.all();
            return;
        }
        // Not in the layout (hidden, minimized, not placed yet): nothing
        // of it shows; what took it away redrew where it was.
        let Some(bbox) = self.space.element_bbox(window) else {
            return;
        };
        self.redraw_rect(reach(bbox, WINDOW_REACH));
        for output in self.space.outputs_for_element(window) {
            self.redraw.output(&output);
        }
    }

    /// `surface` committed: the outputs it shows on redraw. A subsurface
    /// shows with its parent, a popup with what it pops from; a surface
    /// not known here redraws every output.
    pub fn redraw_committed(&mut self, surface: &WlSurface) {
        let mut root = surface.clone();
        while let Some(parent) = get_parent(&root) {
            root = parent;
        }
        if let Some(popup) = self.popups.find_popup(&root) {
            match find_popup_root_surface(&popup) {
                Ok(parent) => root = parent,
                Err(_) => {
                    self.redraw.all();
                    return;
                }
            }
        }
        let window = self.window_for(&root).or_else(|| {
            // X11 menus and tooltips are in the layout alone.
            self.space
                .elements()
                .find(|w| w.wl_surface().is_some_and(|s| *s == root))
                .cloned()
        });
        if let Some(window) = window {
            self.redraw_window(&window);
            return;
        }
        let Self {
            space,
            redraw,
            cursor,
            ..
        } = self;
        let layer = space.outputs().find(|o| {
            layer_map_for_output(o)
                .layer_for_surface(&root, WindowSurfaceType::TOPLEVEL)
                .is_some()
        });
        if let Some(output) = layer {
            redraw.output(output);
            return;
        }
        if matches!(cursor, CursorImageStatus::Surface(s) if *s == root) {
            redraw.around_pointer();
            return;
        }
        let lock = self
            .space
            .outputs()
            .find(|o| self.lock_surface(o) == Some(&root))
            .cloned();
        match lock {
            Some(output) => self.redraw.output(&output),
            None => self.redraw.all(),
        }
    }

    /// The windows in the layout, bottom to top, and what each covers
    /// with its shadow: what [`State::redraw_places`] compares.
    pub(crate) fn window_places(&self) -> Vec<(Window, Rectangle<i32, Logical>)> {
        self.space
            .elements()
            .filter_map(|w| Some((w.clone(), reach(self.space.element_bbox(w)?, WINDOW_REACH))))
            .collect()
    }

    /// The outputs whose windows changed since `before` redraw: one came,
    /// went, moved, or took another place in the stack there.
    pub(crate) fn redraw_places(&mut self, before: &[(Window, Rectangle<i32, Logical>)]) {
        if self.windows_shown_elsewhere() {
            self.redraw.all();
            return;
        }
        let after = self.window_places();
        let Self { space, redraw, .. } = self;
        for output in space.outputs() {
            let Some(area) = space.output_geometry(output) else {
                continue;
            };
            let on = |places: &[(Window, Rectangle<i32, Logical>)]| {
                places
                    .iter()
                    .filter(|(_, r)| r.overlaps(area))
                    .map(|(w, r)| (w.clone(), *r))
                    .collect::<Vec<_>>()
            };
            if on(before) != on(&after) {
                redraw.output(output);
            }
        }
    }

    /// Whether windows show live away from their place: in the switcher
    /// or the overview, or in the agents' slots of the journal (#43).
    fn windows_shown_elsewhere(&self) -> bool {
        self.alt_tab_open() || self.alt_tab.closing.is_some() || !self.seats.is_empty()
    }
}

/// `rect` grown by `by` on each side.
fn reach(rect: Rectangle<i32, Logical>, by: i32) -> Rectangle<i32, Logical> {
    Rectangle::new(
        rect.loc - Point::from((by, by)),
        (rect.size.w + by * 2, rect.size.h + by * 2).into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay::output::{PhysicalProperties, Subpixel};

    fn output(name: &str) -> Output {
        Output::new(
            name.into(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "test".into(),
                model: "test".into(),
            },
        )
    }

    #[test]
    fn everything_draws_first() {
        let redraw = Redraw::default();
        assert!(redraw.wants(&output("A")));
    }

    #[test]
    fn only_what_was_asked_for_redraws_until_all_is() {
        let (a, b) = (output("A"), output("B"));
        let mut redraw = Redraw::default();
        redraw.done();
        assert!(!redraw.is_pending());
        redraw.output(&a);
        redraw.output(&a);
        assert!(redraw.wants(&a) && !redraw.wants(&b));
        redraw.all();
        redraw.output(&a);
        assert!(redraw.wants(&a) && redraw.wants(&b));
        redraw.done();
        assert!(!redraw.wants(&a) && !redraw.wants(&b));
    }
}
