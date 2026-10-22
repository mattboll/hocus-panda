//! Pointer grabs: moving and resizing windows with the pointer.
//!
//! The grab structure follows smithay's `smallvil` example (MIT, Copyright
//! (c) 2017 Victor Berger and Victoria Brekenfeld).

use hocus_core::{WindowId, Zone, snap::magnet};
use smithay::{
    desktop::Window,
    input::pointer::{
        AxisFrame, ButtonEvent, Focus, GestureHoldBeginEvent, GestureHoldEndEvent,
        GesturePinchBeginEvent, GesturePinchEndEvent, GesturePinchUpdateEvent,
        GestureSwipeBeginEvent, GestureSwipeEndEvent, GestureSwipeUpdateEvent, GrabStartData,
        MotionEvent, PointerGrab, PointerInnerHandle, RelativeMotionEvent,
    },
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::protocol::wl_surface::WlSurface,
    },
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER, Serial, Size},
};

use crate::{
    State,
    desktops::{model_id, rect},
    window_mode::WindowMode,
};

/// `BTN_LEFT` and `BTN_RIGHT` from linux/input-event-codes.h.
pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;
/// How far the pointer must pull a maximized window before it lets go.
const TEAR_OFF: f64 = 32.0;
/// How close to an output's edge the pointer snaps a window.
const SNAP_EDGE: f64 = 8.0;
/// How close an edge must come to stick.
const MAGNET: i32 = 12;
/// The band around a window, outside it, that resizes (or moves, on top).
pub const FRAME: i32 = 8;
/// Windows are never resized below this.
const MIN_SIZE: (i32, i32) = (120, 80);

/// The pointer events a grab passes on as they are.
macro_rules! forward_other_events {
    () => {
        fn relative_motion(
            &mut self,
            state: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            focus: Option<(WlSurface, Point<f64, Logical>)>,
            event: &RelativeMotionEvent,
        ) {
            handle.relative_motion(state, focus, event);
        }

        fn axis(
            &mut self,
            state: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            details: AxisFrame,
        ) {
            handle.axis(state, details);
        }

        fn frame(&mut self, state: &mut State, handle: &mut PointerInnerHandle<'_, State>) {
            handle.frame(state);
        }

        fn gesture_swipe_begin(
            &mut self,
            state: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GestureSwipeBeginEvent,
        ) {
            handle.gesture_swipe_begin(state, event);
        }

        fn gesture_swipe_update(
            &mut self,
            state: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GestureSwipeUpdateEvent,
        ) {
            handle.gesture_swipe_update(state, event);
        }

        fn gesture_swipe_end(
            &mut self,
            state: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GestureSwipeEndEvent,
        ) {
            handle.gesture_swipe_end(state, event);
        }

        fn gesture_pinch_begin(
            &mut self,
            state: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GesturePinchBeginEvent,
        ) {
            handle.gesture_pinch_begin(state, event);
        }

        fn gesture_pinch_update(
            &mut self,
            state: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GesturePinchUpdateEvent,
        ) {
            handle.gesture_pinch_update(state, event);
        }

        fn gesture_pinch_end(
            &mut self,
            state: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GesturePinchEndEvent,
        ) {
            handle.gesture_pinch_end(state, event);
        }

        fn gesture_hold_begin(
            &mut self,
            state: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GestureHoldBeginEvent,
        ) {
            handle.gesture_hold_begin(state, event);
        }

        fn gesture_hold_end(
            &mut self,
            state: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GestureHoldEndEvent,
        ) {
            handle.gesture_hold_end(state, event);
        }
    };
}

/// Where the pointer took a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameHit {
    /// The top edge: no title bar, so the edge moves the window.
    Move,
    Resize(Edges),
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
    fn from_xdg(edge: xdg_toplevel::ResizeEdge) -> Self {
        use xdg_toplevel::ResizeEdge as E;
        Self {
            left: matches!(edge, E::Left | E::TopLeft | E::BottomLeft),
            right: matches!(edge, E::Right | E::TopRight | E::BottomRight),
            top: matches!(edge, E::Top | E::TopLeft | E::TopRight),
            bottom: matches!(edge, E::Bottom | E::BottomLeft | E::BottomRight),
        }
    }
}

/// A maximized window holds on until it is pulled far enough.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Hold {
    Free,
    Maximized,
}

/// Moves a window with the pointer until its button is released.
#[derive(Debug)]
pub struct MoveGrab {
    start_data: GrabStartData<State>,
    window: Window,
    initial_location: Point<i32, Logical>,
    hold: Hold,
}

impl PointerGrab<State> for MoveGrab {
    fn motion(
        &mut self,
        state: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        _focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        // No client has pointer focus while a window moves.
        handle.motion(state, None, event);
        let delta = event.location - self.start_data.location;
        if self.hold == Hold::Maximized {
            if delta.x.hypot(delta.y) < TEAR_OFF {
                return;
            }
            self.tear_off(state, event.location, event.serial);
        }
        let location = self.initial_location.to_f64() + (event.location - self.start_data.location);
        let mut location = location.to_i32_round();
        // At an edge, a zone shows where the window would snap; elsewhere,
        // its edges stick to the work area's and to other windows'.
        let pointer = event.location;
        let output = state.space.output_under(pointer).next().cloned();
        let zone = output.as_ref().and_then(|o| {
            let whole = rect(state.space.output_geometry(o)?);
            Zone::at(whole, pointer.x, pointer.y, SNAP_EDGE)
        });
        state.set_snap_preview(output.clone().zip(zone));
        if zone.is_none()
            && let Some(output) = output
        {
            let area = rect(state.work_area(&output));
            let others: Vec<_> = state
                .space
                .elements()
                .filter(|w| **w != self.window)
                .filter_map(|w| state.space.element_geometry(w))
                .map(rect)
                .collect();
            let size = self.window.geometry().size;
            let me = hocus_core::Rect {
                x: location.x,
                y: location.y,
                width: size.w,
                height: size.h,
            };
            let (x, y) = magnet(me, area, &others, state.config.windows.gaps, MAGNET);
            location = (x, y).into();
        }
        state.space.map_element(self.window.clone(), location, true);
        state.needs_redraw = true;
    }

    fn button(
        &mut self,
        state: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &ButtonEvent,
    ) {
        handle.button(state, event);
        if !handle.current_pressed().contains(&self.start_data.button) {
            handle.unset_grab(self, state, event.serial, event.time, true);
        }
    }

    forward_other_events!();

    fn start_data(&self) -> &GrabStartData<State> {
        &self.start_data
    }

    fn unset(&mut self, state: &mut State) {
        if let (Some((_, zone)), Some(id)) = (state.snap_preview.take(), model_id(&self.window)) {
            state.needs_redraw = true;
            state.snap(id, zone, SERIAL_COUNTER.next_serial());
            return;
        }
        state.sync_desktops();
    }
}

impl MoveGrab {
    /// The window leaves maximize under the pointer: it keeps the same
    /// share of its width on each side of the pointer.
    fn tear_off(&mut self, state: &mut State, pointer: Point<f64, Logical>, serial: Serial) {
        self.hold = Hold::Free;
        let Some(id) = model_id(&self.window) else {
            return;
        };
        let Some(restore) = state.windows.get(&id).and_then(|m| m.mode.restore()) else {
            return;
        };
        let current = state
            .space
            .element_geometry(&self.window)
            .unwrap_or(restore);
        // Where it was taken, as a share of its width and a height from its top.
        let taken = self.start_data.location;
        let share = if current.size.w > 0 {
            (taken.x - f64::from(current.loc.x)) / f64::from(current.size.w)
        } else {
            0.5
        };
        let grip_y = (taken.y - f64::from(current.loc.y)).clamp(0.0, f64::from(restore.size.h));
        state.unlay(id, serial);
        let location = Point::<f64, Logical>::from((
            pointer.x - share * f64::from(restore.size.w),
            pointer.y - grip_y,
        ));
        // From here on, motion moves it from where it now is.
        self.initial_location = location.to_i32_round();
        self.start_data.location = pointer;
    }
}

/// Resizes a window with the pointer until its button is released.
#[derive(Debug)]
pub struct ResizeGrab {
    start_data: GrabStartData<State>,
    id: WindowId,
    edges: Edges,
    initial: Rectangle<i32, Logical>,
    /// The last place and size asked for: the client may not have drawn it
    /// yet.
    last: Rectangle<i32, Logical>,
}

impl PointerGrab<State> for ResizeGrab {
    fn motion(
        &mut self,
        state: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        _focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(state, None, event);
        let delta = (event.location - self.start_data.location).to_i32_round::<i32>();
        let mut rect = self.initial;
        if self.edges.left {
            rect.size.w = (self.initial.size.w - delta.x).max(MIN_SIZE.0);
            rect.loc.x = self.initial.loc.x + self.initial.size.w - rect.size.w;
        } else if self.edges.right {
            rect.size.w = (self.initial.size.w + delta.x).max(MIN_SIZE.0);
        }
        if self.edges.top {
            rect.size.h = (self.initial.size.h - delta.y).max(MIN_SIZE.1);
            rect.loc.y = self.initial.loc.y + self.initial.size.h - rect.size.h;
        } else if self.edges.bottom {
            rect.size.h = (self.initial.size.h + delta.y).max(MIN_SIZE.1);
        }
        self.last = rect;
        state.resize_to(self.id, rect, true);
    }

    fn button(
        &mut self,
        state: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &ButtonEvent,
    ) {
        handle.button(state, event);
        if !handle.current_pressed().contains(&self.start_data.button) {
            handle.unset_grab(self, state, event.serial, event.time, true);
        }
    }

    forward_other_events!();

    fn start_data(&self) -> &GrabStartData<State> {
        &self.start_data
    }

    fn unset(&mut self, state: &mut State) {
        state.resize_to(self.id, self.last, false);
    }
}

impl State {
    /// Starts moving `window` with `button`, from the pointer's place. With
    /// `start_data` from a client's request, the grab takes that over.
    pub fn start_move(
        &mut self,
        window: &Window,
        button: u32,
        start_data: Option<GrabStartData<State>>,
        serial: Serial,
    ) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let Some(initial_location) = self.space.element_location(window) else {
            return;
        };
        let hold = match model_id(window)
            .and_then(|id| self.windows.get(&id))
            .map(|m| m.mode)
        {
            Some(
                WindowMode::Maximized { .. }
                | WindowMode::Snapped { .. }
                | WindowMode::Tiled { .. },
            ) => Hold::Maximized,
            // A fullscreen window does not move.
            Some(WindowMode::Fullscreen { .. }) => return,
            _ => Hold::Free,
        };
        let start_data = start_data.unwrap_or_else(|| GrabStartData {
            focus: None,
            button,
            location: pointer.current_location(),
        });
        let grab = MoveGrab {
            start_data,
            window: window.clone(),
            initial_location,
            hold,
        };
        pointer.set_grab(self, grab, serial, Focus::Clear);
    }

    /// Starts resizing `window` by `edges` with `button`.
    pub fn start_resize(
        &mut self,
        window: &Window,
        edges: Edges,
        button: u32,
        start_data: Option<GrabStartData<State>>,
        serial: Serial,
    ) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        // The frame, its bar included.
        let Some(id) = model_id(window) else {
            return;
        };
        let Some(initial) = self.current_rect(id) else {
            return;
        };
        if !matches!(
            self.windows.get(&id).map(|m| m.mode),
            Some(WindowMode::Normal)
        ) {
            return;
        }
        let start_data = start_data.unwrap_or_else(|| GrabStartData {
            focus: None,
            button,
            location: pointer.current_location(),
        });
        let grab = ResizeGrab {
            start_data,
            id,
            edges,
            initial,
            last: initial,
        };
        pointer.set_grab(self, grab, serial, Focus::Clear);
    }

    /// Asks the client for `rect`'s size and puts the window at its corner;
    /// `resizing` tells the client a resize is under way.
    pub fn resize_to(&mut self, id: WindowId, rect: Rectangle<i32, Logical>, resizing: bool) {
        let rect = self.client_rect(id, rect);
        let Some(managed) = self.windows.get_mut(&id) else {
            return;
        };
        crate::surface::request_size(
            &managed.window,
            rect,
            crate::surface::Shown::Normal,
            resizing,
        );
        managed.location = rect.loc;
        let window = managed.window.clone();
        self.space.map_element(window, rect.loc, true);
        self.needs_redraw = true;
    }

    /// The window whose frame band is under `pos`, and what that band does:
    /// the top edge moves, the others and the corners resize. Inside a
    /// window is the client's.
    pub fn frame_hit(&self, pos: Point<f64, Logical>) -> Option<(Window, FrameHit)> {
        for window in self.space.elements().rev() {
            let Some(mut rect) = self.space.element_geometry(window) else {
                continue;
            };
            // The bar is part of the frame.
            if let Some(bar) = model_id(window).map(|id| self.bar_height(id)) {
                rect.loc.y -= bar;
                rect.size.h += bar;
            }
            let outer = Rectangle::new(
                rect.loc - Point::from((FRAME, FRAME)),
                rect.size + Size::from((2 * FRAME, 2 * FRAME)),
            );
            if !outer.to_f64().contains(pos) {
                continue;
            }
            if rect.to_f64().contains(pos) {
                return None;
            }
            let edges = Edges {
                left: pos.x < f64::from(rect.loc.x),
                right: pos.x >= f64::from(rect.loc.x + rect.size.w),
                top: pos.y < f64::from(rect.loc.y),
                bottom: pos.y >= f64::from(rect.loc.y + rect.size.h),
            };
            let hit = if edges.top && !edges.left && !edges.right {
                FrameHit::Move
            } else {
                FrameHit::Resize(edges)
            };
            return Some((window.clone(), hit));
        }
        None
    }

    /// The edges Alt+right drag resizes: those of the window's quarter under
    /// the pointer.
    pub fn quarter_edges(&self, window: &Window, pos: Point<f64, Logical>) -> Edges {
        let Some(rect) = self.space.element_geometry(window) else {
            return Edges::default();
        };
        let centre = rect.to_f64().loc + rect.to_f64().size.to_point().downscale(2.0);
        Edges {
            left: pos.x < centre.x,
            right: pos.x >= centre.x,
            top: pos.y < centre.y,
            bottom: pos.y >= centre.y,
        }
    }
}

/// The edges of a client's resize request.
pub fn edges_of(edge: xdg_toplevel::ResizeEdge) -> Edges {
    Edges::from_xdg(edge)
}
