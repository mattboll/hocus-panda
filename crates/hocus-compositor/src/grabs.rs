//! Moving a window with the pointer: `Super`+drag (`Alt+Super` nested), or
//! a client's own move request from its header. The window follows the
//! pointer rigidly; the real motion (wobble, snapping) is task 2.4's.

use smithay::{
    desktop::Window,
    input::{
        SeatHandler,
        pointer::{
            AxisFrame, ButtonEvent, Focus, GestureHoldBeginEvent, GestureHoldEndEvent,
            GesturePinchBeginEvent, GesturePinchEndEvent, GesturePinchUpdateEvent,
            GestureSwipeBeginEvent, GestureSwipeEndEvent, GestureSwipeUpdateEvent, GrabStartData,
            MotionEvent, PointerGrab, PointerInnerHandle, RelativeMotionEvent,
        },
    },
    utils::{Logical, Point, Serial},
};

use crate::State;

/// The left button, by its evdev code.
pub const BTN_LEFT: u32 = 0x110;

#[derive(Debug)]
pub struct MoveGrab {
    start_data: GrabStartData<State>,
    window: Window,
    initial_location: Point<i32, Logical>,
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
        let delta = event.location - self.start_data.location;
        let location = self.initial_location.to_f64() + delta;
        let location = location.to_i32_round();
        state.space.map_element(self.window.clone(), location, true);
        if let Some(x11) = self.window.x11_surface() {
            let mut geometry = x11.geometry();
            geometry.loc = location;
            if let Err(err) = x11.configure(geometry) {
                tracing::debug!(%err, "could not move an X11 window");
            }
        }
        state.redraw = true;
    }

    fn relative_motion(
        &mut self,
        state: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        focus: Option<(<State as SeatHandler>::PointerFocus, Point<f64, Logical>)>,
        event: &RelativeMotionEvent,
    ) {
        handle.relative_motion(state, focus, event);
    }

    fn button(
        &mut self,
        state: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &ButtonEvent,
    ) {
        handle.button(state, event);
        if handle.current_pressed().is_empty() {
            handle.unset_grab(self, state, event.serial, event.time, true);
        }
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

    fn start_data(&self) -> &GrabStartData<State> {
        &self.start_data
    }

    fn unset(&mut self, _state: &mut State) {}
}

impl State {
    /// Starts moving `window` from the pointer's place. With `start_data`
    /// from a client's move request, the grab takes over the press that
    /// asked for it.
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
        let start_data = start_data.unwrap_or_else(|| GrabStartData {
            focus: None,
            button,
            location: pointer.current_location(),
        });
        let grab = MoveGrab {
            start_data,
            window: window.clone(),
            initial_location,
        };
        pointer.set_grab(self, grab, serial, Focus::Clear);
    }
}
