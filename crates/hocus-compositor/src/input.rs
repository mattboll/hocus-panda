//! Input from the backends, routed to clients or kept by the compositor:
//! keys (with the virtual terminal switch), the pointer (click to focus,
//! `Super`+drag to move), scrolling and touchpad gestures, which go to the
//! client under the pointer (pointer-gestures).

use smithay::{
    backend::input::{
        self as bi, AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, GestureBeginEvent,
        GestureEndEvent, InputBackend, InputEvent, KeyState, KeyboardKeyEvent, PointerAxisEvent,
        PointerButtonEvent, PointerMotionEvent,
    },
    input::{
        keyboard::{FilterResult, Keysym, ModifiersState},
        pointer::{
            AxisFrame, ButtonEvent, GestureHoldBeginEvent, GestureHoldEndEvent,
            GesturePinchBeginEvent, GesturePinchEndEvent, GesturePinchUpdateEvent,
            GestureSwipeBeginEvent, GestureSwipeEndEvent, GestureSwipeUpdateEvent, MotionEvent,
            RelativeMotionEvent,
        },
    },
    utils::{Logical, Point, SERIAL_COUNTER},
};

use crate::{State, grabs::BTN_LEFT};

/// What a key did, when the compositor kept it.
enum Kept {
    /// Ctrl+Alt+F1…F12: another virtual terminal.
    Vt(i32),
}

impl State {
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        match event {
            InputEvent::Keyboard { event } => {
                self.key(event.key_code(), event.state(), event.time_msec());
            }
            InputEvent::PointerMotion { event } => self.on_pointer_motion::<I>(&event),
            InputEvent::PointerMotionAbsolute { event } => {
                self.on_pointer_motion_absolute::<I>(&event);
            }
            InputEvent::PointerButton { event } => {
                self.pointer_button(event.button_code(), event.state(), event.time_msec());
            }
            InputEvent::PointerAxis { event } => self.on_pointer_axis::<I>(&event),
            InputEvent::GestureSwipeBegin { event } => self.gesture(Gesture::SwipeBegin {
                time: event.time_msec(),
                fingers: event.fingers(),
            }),
            InputEvent::GestureSwipeUpdate { event } => {
                use bi::GestureSwipeUpdateEvent as _;
                self.gesture(Gesture::SwipeUpdate {
                    time: event.time_msec(),
                    delta: (event.delta_x(), event.delta_y()).into(),
                });
            }
            InputEvent::GestureSwipeEnd { event } => self.gesture(Gesture::SwipeEnd {
                time: event.time_msec(),
                cancelled: event.cancelled(),
            }),
            InputEvent::GesturePinchBegin { event } => self.gesture(Gesture::PinchBegin {
                time: event.time_msec(),
                fingers: event.fingers(),
            }),
            InputEvent::GesturePinchUpdate { event } => {
                use bi::GesturePinchUpdateEvent as _;
                self.gesture(Gesture::PinchUpdate {
                    time: event.time_msec(),
                    delta: (event.delta_x(), event.delta_y()).into(),
                    scale: event.scale(),
                    rotation: event.rotation(),
                });
            }
            InputEvent::GesturePinchEnd { event } => self.gesture(Gesture::PinchEnd {
                time: event.time_msec(),
                cancelled: event.cancelled(),
            }),
            InputEvent::GestureHoldBegin { event } => self.gesture(Gesture::HoldBegin {
                time: event.time_msec(),
                fingers: event.fingers(),
            }),
            InputEvent::GestureHoldEnd { event } => self.gesture(Gesture::HoldEnd {
                time: event.time_msec(),
                cancelled: event.cancelled(),
            }),
            _ => {}
        }
    }

    /// A key by its keycode, from a device or a test.
    pub fn key(&mut self, code: smithay::input::keyboard::Keycode, state: KeyState, time: u32) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        let kept = keyboard.input::<Kept, _>(self, code, state, serial, time, |_, _, keysym| {
            if state != KeyState::Pressed {
                return FilterResult::Forward;
            }
            let sym = keysym.modified_sym().raw();
            if (Keysym::XF86_Switch_VT_1.raw()..=Keysym::XF86_Switch_VT_12.raw()).contains(&sym) {
                let vt = (sym - Keysym::XF86_Switch_VT_1.raw() + 1) as i32;
                return FilterResult::Intercept(Kept::Vt(vt));
            }
            FilterResult::Forward
        });
        match kept {
            Some(Kept::Vt(vt)) => self.switch_vt(vt),
            None => {}
        }
    }

    /// Whether the modifiers held move a window under the pointer:
    /// `Super`, or `Alt+Super` nested, where the host takes `Super`.
    pub fn move_modifier(&self, modifiers: &ModifiersState) -> bool {
        modifiers.logo && (!self.nested || modifiers.alt)
    }

    /// A mouse or touchpad: the pointer moves by the motion, and stays on
    /// the outputs.
    fn on_pointer_motion<I: InputBackend>(&mut self, event: &I::PointerMotionEvent) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let from = pointer.current_location();
        let to = self.onto_outputs(from + event.delta());
        // Games and 3D tools read the motion itself (relative-pointer).
        let focus = self.surface_under(from);
        pointer.relative_motion(
            self,
            focus,
            &RelativeMotionEvent {
                delta: event.delta(),
                delta_unaccel: event.delta_unaccel(),
                utime: event.time(),
            },
        );
        self.pointer_motion(to, event.time_msec());
    }

    /// A tablet, a VM's pointer, or the nested window's: its surface spans
    /// every output.
    fn on_pointer_motion_absolute<I: InputBackend>(
        &mut self,
        event: &I::PointerMotionAbsoluteEvent,
    ) {
        let Some(area) = self
            .space
            .outputs()
            .filter_map(|o| self.space.output_geometry(o))
            .reduce(|a, b| a.merge(b))
        else {
            return;
        };
        let location = event.position_transformed(area.size) + area.loc.to_f64();
        self.pointer_motion(location, event.time_msec());
    }

    /// The nearest point of the outputs to `to`.
    fn onto_outputs(&self, to: Point<f64, Logical>) -> Point<f64, Logical> {
        let areas: Vec<_> = self
            .space
            .outputs()
            .filter_map(|o| self.space.output_geometry(o))
            .collect();
        if areas.iter().any(|a| a.to_f64().contains(to)) {
            return to;
        }
        areas
            .iter()
            .map(|a| {
                let x =
                    to.x.clamp(f64::from(a.loc.x), f64::from(a.loc.x + a.size.w - 1));
                let y =
                    to.y.clamp(f64::from(a.loc.y), f64::from(a.loc.y + a.size.h - 1));
                Point::from((x, y))
            })
            .min_by(|a: &Point<f64, Logical>, b| {
                let d = |p: &Point<f64, Logical>| (p.x - to.x).powi(2) + (p.y - to.y).powi(2);
                d(a).total_cmp(&d(b))
            })
            .unwrap_or(to)
    }

    /// Moves the pointer to `location` in the global logical space.
    pub fn pointer_motion(&mut self, location: Point<f64, Logical>, time: u32) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let under = self.surface_under(location);
        pointer.motion(
            self,
            under,
            &MotionEvent {
                location,
                serial: SERIAL_COUNTER.next_serial(),
                time,
            },
        );
        pointer.frame(self);
        // The compositor draws the cursor on screens of its own; nested,
        // the host does.
        if !self.nested {
            self.redraw = true;
        }
    }

    /// A pointer button by its evdev code. A press focuses the window
    /// under the pointer and goes to it; with the move modifier, the
    /// left button moves the window instead, and the client sees nothing.
    pub fn pointer_button(&mut self, button: u32, state: ButtonState, time: u32) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        let at = pointer.current_location();
        if state == ButtonState::Pressed && !pointer.is_grabbed() {
            if self.layer_click(at) {
                // A bar or an overlay: the click is its own.
            } else if let Some(id) = self.window_under(at) {
                self.focus(Some(id));
                let modifiers = self.seat.get_keyboard().map(|k| k.modifier_state());
                if button == BTN_LEFT
                    && modifiers.is_some_and(|m| self.move_modifier(&m))
                    && let Some(window) = self.windows.get(&id).cloned()
                {
                    self.start_move(&window, button, None, serial);
                    return;
                }
            }
        }
        pointer.button(
            self,
            &ButtonEvent {
                button,
                state,
                serial,
                time,
            },
        );
        pointer.frame(self);
    }

    fn on_pointer_axis<I: InputBackend>(&mut self, event: &I::PointerAxisEvent) {
        let source = event.source();
        let amount = |axis| {
            event
                .amount(axis)
                .unwrap_or_else(|| event.amount_v120(axis).unwrap_or(0.0) * 15.0 / 120.0)
        };
        let mut frame = AxisFrame::new(event.time_msec()).source(source);
        for axis in [Axis::Horizontal, Axis::Vertical] {
            let value = amount(axis);
            if value != 0.0 {
                frame = frame.value(axis, value);
                if let Some(discrete) = event.amount_v120(axis) {
                    frame = frame.v120(axis, discrete as i32);
                }
            }
            if source == AxisSource::Finger && event.amount(axis) == Some(0.0) {
                frame = frame.stop(axis);
            }
        }
        if let Some(pointer) = self.seat.get_pointer() {
            pointer.axis(self, frame);
            pointer.frame(self);
        }
    }

    /// A touchpad gesture, for the client under the pointer. The desktop's
    /// own (three fingers to switch desktops) come with task 2.5.
    fn gesture(&mut self, gesture: Gesture) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        match gesture {
            Gesture::SwipeBegin { time, fingers } => pointer.gesture_swipe_begin(
                self,
                &GestureSwipeBeginEvent {
                    serial,
                    time,
                    fingers,
                },
            ),
            Gesture::SwipeUpdate { time, delta } => {
                pointer.gesture_swipe_update(self, &GestureSwipeUpdateEvent { time, delta });
            }
            Gesture::SwipeEnd { time, cancelled } => pointer.gesture_swipe_end(
                self,
                &GestureSwipeEndEvent {
                    serial,
                    time,
                    cancelled,
                },
            ),
            Gesture::PinchBegin { time, fingers } => pointer.gesture_pinch_begin(
                self,
                &GesturePinchBeginEvent {
                    serial,
                    time,
                    fingers,
                },
            ),
            Gesture::PinchUpdate {
                time,
                delta,
                scale,
                rotation,
            } => pointer.gesture_pinch_update(
                self,
                &GesturePinchUpdateEvent {
                    time,
                    delta,
                    scale,
                    rotation,
                },
            ),
            Gesture::PinchEnd { time, cancelled } => pointer.gesture_pinch_end(
                self,
                &GesturePinchEndEvent {
                    serial,
                    time,
                    cancelled,
                },
            ),
            Gesture::HoldBegin { time, fingers } => pointer.gesture_hold_begin(
                self,
                &GestureHoldBeginEvent {
                    serial,
                    time,
                    fingers,
                },
            ),
            Gesture::HoldEnd { time, cancelled } => pointer.gesture_hold_end(
                self,
                &GestureHoldEndEvent {
                    serial,
                    time,
                    cancelled,
                },
            ),
        }
        pointer.frame(self);
    }
}

/// A touchpad gesture's step, as libinput gives it.
enum Gesture {
    SwipeBegin {
        time: u32,
        fingers: u32,
    },
    SwipeUpdate {
        time: u32,
        delta: Point<f64, Logical>,
    },
    SwipeEnd {
        time: u32,
        cancelled: bool,
    },
    PinchBegin {
        time: u32,
        fingers: u32,
    },
    PinchUpdate {
        time: u32,
        delta: Point<f64, Logical>,
        scale: f64,
        rotation: f64,
    },
    PinchEnd {
        time: u32,
        cancelled: bool,
    },
    HoldBegin {
        time: u32,
        fingers: u32,
    },
    HoldEnd {
        time: u32,
        cancelled: bool,
    },
}
