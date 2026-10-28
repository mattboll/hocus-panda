//! Moving and resizing windows with the pointer (docs/design/windows.md
//! §6–§9): a client's header, `Super`+drag (`Alt+Super` nested) or the
//! grip moves a window, wobbling, and snaps it where the preview shows;
//! the band around a window, a client's own resize request or
//! `Super`+right-drag resizes it, a snapped neighbour following.
//!
//! The grab structure follows smithay's examples (MIT).

use smithay::{
    input::{
        SeatHandler,
        pointer::{
            AxisFrame, ButtonEvent, CursorIcon, Focus, GestureHoldBeginEvent, GestureHoldEndEvent,
            GesturePinchBeginEvent, GesturePinchEndEvent, GesturePinchUpdateEvent,
            GestureSwipeBeginEvent, GestureSwipeEndEvent, GestureSwipeUpdateEvent, GrabStartData,
            MotionEvent, PointerGrab, PointerInnerHandle, RelativeMotionEvent,
        },
    },
    reexports::wayland_protocols::xdg::shell::server::xdg_toplevel,
    utils::{Logical, Point, Serial},
};

use crate::{
    State,
    model::{self, Edges, MIN_SIZE, Mode, Rect, WindowId, Zone},
    windows::Resizing,
    wobble::Jelly,
};

/// The left and right buttons, by their evdev codes.
pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;
/// How far a snapped or maximised window is pulled before it lets go.
const TEAR_OFF: f64 = 12.0;

/// The pointer events a grab passes on as they are.
macro_rules! forward_other_events {
    () => {
        fn relative_motion(
            &mut self,
            state: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            focus: Option<(<State as SeatHandler>::PointerFocus, Point<f64, Logical>)>,
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

/// Whether a moved window is still in its zone.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Hold {
    Free,
    /// Snapped or maximised: it holds on until pulled far enough, and
    /// comes away held at the same place across it.
    Snapped {
        along: f64,
        down: f64,
        height: i32,
    },
}

#[derive(Debug)]
pub struct MoveGrab {
    start_data: GrabStartData<State>,
    id: WindowId,
    initial: Point<i32, Logical>,
    /// Where the pointer was when `initial` was.
    from: Point<f64, Logical>,
    hold: Hold,
    /// Torn off a zone: it keeps still, its new size not drawn yet.
    rigid: bool,
    /// Where it floated when the drag began.
    floating: Option<Rect>,
}

impl PointerGrab<State> for MoveGrab {
    fn motion(
        &mut self,
        state: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        _focus: Option<(<State as SeatHandler>::PointerFocus, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        // No client sees the pointer while a window moves.
        handle.motion(state, None, event);
        let Some(window) = state.windows.get(&self.id).map(|m| m.window.clone()) else {
            return;
        };
        let pointer = event.location;
        let delta = pointer - self.from;
        if let Hold::Snapped {
            along,
            down,
            height,
        } = self.hold
        {
            if delta.x.hypot(delta.y) < TEAR_OFF {
                return;
            }
            // Its size before it snapped, under the pointer.
            let Some(current) = state.rect_of(self.id) else {
                return;
            };
            state.start_settle(self.id);
            let restore = state.model.unsnap(self.id).unwrap_or(current);
            let (x, y) = model::torn_off(
                (restore.width, restore.height),
                along,
                (down, height),
                pointer.x,
                pointer.y,
            );
            state.configure(self.id, Rect::new(x, y, restore.width, restore.height));
            self.initial = (x, y).into();
            self.from = pointer;
            self.hold = Hold::Free;
            self.rigid = true;
            // Where it floated before it snapped stays where it goes back.
            self.floating = Some(restore);
            return;
        }
        let location = (self.initial.to_f64() + delta).to_i32_round();
        let before = state.space.element_location(&window);
        state.space.map_element(window.clone(), location, true);
        if let Some(x11) = window.x11_surface() {
            let mut geometry = x11.geometry();
            geometry.loc = location;
            if let Err(err) = x11.configure(geometry) {
                tracing::debug!(%err, "could not move an X11 window");
            }
        }
        // The jelly lags behind the move.
        if let Some(before) = before {
            let moved = location - before;
            let now = state.now();
            let wobbles = !self.rigid && !state.reduced_motion();
            if wobbles && state.wobble.as_ref().is_none_or(|(w, _)| *w != self.id) {
                let size = window.geometry().size;
                let held = self.from - self.initial.to_f64();
                state.wobble = Some((
                    self.id,
                    Jelly::new(
                        (f64::from(size.w), f64::from(size.h)),
                        (held.x, held.y),
                        now,
                    ),
                ));
            }
            if let Some((w, jelly)) = &mut state.wobble
                && *w == self.id
            {
                jelly.moved(f64::from(moved.x), f64::from(moved.y), now);
            }
        }
        // At an edge of the work area, where it would snap.
        let zone = state
            .space
            .output_under(pointer)
            .next()
            .cloned()
            .and_then(|o| state.work_area(&o))
            .and_then(|area| Some((Zone::at(area, pointer.x, pointer.y)?, area)))
            .map(|(zone, area)| (zone, zone.rect(area)));
        state.set_preview(zone);
        state.redraw = true;
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
        state.moving = None;
        state.frame_cursor = None;
        let now = state.now();
        let zone = state.preview.filter(|p| p.shown(now)).map(|p| p.zone);
        state.set_preview(None);
        if let Some((w, jelly)) = &mut state.wobble
            && *w == self.id
        {
            jelly.release();
        }
        if let Some(zone) = zone {
            // Restored, it goes back where the drag began.
            state.snap_from(self.id, zone, self.floating);
        }
        state.redraw = true;
    }
}

/// Resizes a window from its edges until the button is released.
#[derive(Debug)]
pub struct ResizeGrab {
    start_data: GrabStartData<State>,
    id: WindowId,
    edges: Edges,
    initial: Rect,
    min: (i32, i32),
    max: (i32, i32),
    /// Snapped neighbours that follow, with their edges that do and their
    /// geometry at the start.
    partners: Vec<(WindowId, Edges, Rect)>,
    /// A snapped window resized alone from an edge it shares with a
    /// neighbour leaves its zone once its size changes (§8).
    floats: bool,
    last: Rect,
}

impl PointerGrab<State> for ResizeGrab {
    fn motion(
        &mut self,
        state: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        _focus: Option<(<State as SeatHandler>::PointerFocus, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(state, None, event);
        let delta = (event.location - self.start_data.location).to_i32_round::<i32>();
        let rect = model::resized(
            self.initial,
            self.edges,
            (delta.x, delta.y),
            self.min,
            self.max,
        );
        if rect == self.last {
            return;
        }
        self.last = rect;
        if self.floats {
            self.floats = false;
            state.model.unsnap(self.id);
        }
        state.configure(self.id, rect);
        for (id, follow, initial) in &self.partners {
            state.configure(*id, model::follow(*initial, *follow, rect));
        }
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
        state.frame_cursor = None;
        let ids: Vec<WindowId> = std::iter::once(self.id)
            .chain(self.partners.iter().map(|(id, _, _)| *id))
            .collect();
        for id in ids {
            let target = state.target_rect(id);
            if let Some(managed) = state.windows.get_mut(&id)
                && let Some(resizing) = &mut managed.resizing
            {
                resizing.ended = true;
            }
            // The last size, without the resizing state.
            if let Some(rect) = target {
                state.configure(id, rect);
            }
        }
        state.redraw = true;
    }
}

impl Edges {
    pub fn from_xdg(edge: xdg_toplevel::ResizeEdge) -> Self {
        use xdg_toplevel::ResizeEdge as E;
        Self {
            left: matches!(edge, E::Left | E::TopLeft | E::BottomLeft),
            right: matches!(edge, E::Right | E::TopRight | E::BottomRight),
            top: matches!(edge, E::Top | E::TopLeft | E::TopRight),
            bottom: matches!(edge, E::Bottom | E::BottomLeft | E::BottomRight),
        }
    }

    pub fn from_x11(edge: smithay::xwayland::xwm::ResizeEdge) -> Self {
        use smithay::xwayland::xwm::ResizeEdge as E;
        Self {
            left: matches!(edge, E::Left | E::TopLeft | E::BottomLeft),
            right: matches!(edge, E::Right | E::TopRight | E::BottomRight),
            top: matches!(edge, E::Top | E::TopLeft | E::TopRight),
            bottom: matches!(edge, E::Bottom | E::BottomLeft | E::BottomRight),
        }
    }

    /// The cursor shape these edges show.
    pub fn cursor(self) -> CursorIcon {
        match self.cursor_name() {
            "ew-resize" => CursorIcon::EwResize,
            "ns-resize" => CursorIcon::NsResize,
            "nwse-resize" => CursorIcon::NwseResize,
            "nesw-resize" => CursorIcon::NeswResize,
            _ => CursorIcon::Default,
        }
    }
}

impl State {
    /// Starts moving window `id` from the pointer's place. With
    /// `start_data` from a client's move request, the grab takes over the
    /// press that asked for it.
    pub fn start_move(
        &mut self,
        id: WindowId,
        button: u32,
        start_data: Option<GrabStartData<State>>,
        serial: Serial,
    ) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let (Some(rect), Some(window)) = (
            self.rect_of(id),
            self.windows.get(&id).map(|m| m.window.clone()),
        ) else {
            return;
        };
        if self.model.mode(id) == Mode::Fullscreen {
            return;
        }
        let at = pointer.current_location();
        let hold = match self.model.mode(id) {
            Mode::Snapped(_) => Hold::Snapped {
                along: (at.x - f64::from(rect.x)) / f64::from(rect.width.max(1)),
                down: at.y - f64::from(rect.y),
                height: rect.height,
            },
            Mode::Floating | Mode::Fullscreen => Hold::Free,
        };
        let start_data = start_data.unwrap_or_else(|| GrabStartData {
            focus: None,
            button,
            location: at,
        });
        let initial = self
            .space
            .element_location(&window)
            .unwrap_or((rect.x, rect.y).into());
        let grab = MoveGrab {
            from: start_data.location,
            start_data,
            id,
            initial,
            hold,
            rigid: false,
            floating: (hold == Hold::Free).then_some(rect),
        };
        self.moving = Some(id);
        self.frame_cursor = Some(CursorIcon::Grabbing);
        pointer.set_grab(self, grab, serial, Focus::Clear);
        self.redraw = true;
    }

    /// Starts resizing window `id` from `edges`, alone: snapped, from an
    /// edge it shares with a neighbour, it floats (§8).
    pub fn start_resize(
        &mut self,
        id: WindowId,
        edges: Edges,
        button: u32,
        start_data: Option<GrabStartData<State>>,
        serial: Serial,
    ) {
        self.resize(id, edges, button, start_data, serial, false);
    }

    /// Starts resizing snapped window `id` from `edges` with the snapped
    /// neighbours across them following: the handle of a seam (§8).
    pub fn start_pair_resize(&mut self, id: WindowId, edges: Edges, button: u32, serial: Serial) {
        self.resize(id, edges, button, None, serial, true);
    }

    fn resize(
        &mut self,
        id: WindowId,
        edges: Edges,
        button: u32,
        start_data: Option<GrabStartData<State>>,
        serial: Serial,
        together: bool,
    ) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let (Some(initial), Some(window)) = (
            self.rect_of(id),
            self.windows.get(&id).map(|m| m.window.clone()),
        ) else {
            return;
        };
        let mode = self.model.mode(id);
        if mode == Mode::Fullscreen || !edges.any() {
            return;
        }
        // A snapped neighbour across a moved edge follows it when the
        // handle moves it; from the edge, the window leaves the pair (§8).
        let mut partners = Vec::new();
        if let Mode::Snapped(zone) = mode {
            if zone == Zone::Maximized {
                // Resized, it floats.
                self.model.unsnap(id);
            } else if let Some(output) = self.output_of(id) {
                let others: Vec<(WindowId, Rect)> = self
                    .windows
                    .keys()
                    .copied()
                    .filter(|o| *o != id && !self.model.is_minimized(*o))
                    .filter(
                        |o| matches!(self.model.mode(*o), Mode::Snapped(z) if z != Zone::Maximized),
                    )
                    .filter(|o| self.output_of(*o).as_ref() == Some(&output))
                    .filter_map(|o| Some((o, self.rect_of(o)?)))
                    .collect();
                for (pid, follow) in model::partners(initial, edges, &others) {
                    if let Some((_, r)) = others.iter().find(|(o, _)| *o == pid) {
                        partners.push((pid, follow, *r));
                    }
                }
            }
        }
        let floats = !together && !partners.is_empty();
        if floats {
            partners.clear();
            // The handle goes with the pair.
            self.seam_handle = None;
        }
        let (min, max) = crate::windows::size_limits(&window);
        let min = (min.0.max(MIN_SIZE.0), min.1.max(MIN_SIZE.1));
        if let Some(managed) = self.windows.get_mut(&id) {
            managed.resizing = Some(Resizing {
                edges,
                anchor: initial,
                ended: false,
            });
            managed.settle = None;
        }
        for (pid, follow, rect) in &partners {
            if let Some(managed) = self.windows.get_mut(pid) {
                managed.resizing = Some(Resizing {
                    edges: *follow,
                    anchor: *rect,
                    ended: false,
                });
            }
        }
        if self.wobble.as_ref().is_some_and(|(w, _)| *w == id) {
            self.wobble = None;
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
            min,
            max,
            partners,
            floats,
            last: initial,
        };
        self.frame_cursor = Some(edges.cursor());
        pointer.set_grab(self, grab, serial, Focus::Clear);
        self.redraw = true;
    }
}
