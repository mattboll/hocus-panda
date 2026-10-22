//! Input events from the backend, routed to clients or to the compositor.

use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent,
        KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
    },
    input::{
        keyboard::{FilterResult, Keycode},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    utils::{Logical, Point, SERIAL_COUNTER},
};

use hocus_core::Action;

use crate::{
    State,
    actions::combo_of,
    grabs::{BTN_LEFT, BTN_RIGHT, FrameHit},
};

impl State {
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        match event {
            InputEvent::Keyboard { event } => self.on_key::<I>(event),
            InputEvent::PointerMotionAbsolute { event } => {
                self.on_pointer_motion_absolute::<I>(event)
            }
            InputEvent::PointerButton { event } => self.on_pointer_button::<I>(event),
            InputEvent::PointerAxis { event } => self.on_pointer_axis::<I>(event),
            _ => {}
        }
    }

    fn on_key<I: InputBackend>(&mut self, event: I::KeyboardKeyEvent) {
        self.key(event.key_code(), event.state(), Event::time_msec(&event));
    }

    /// A key, from a device or injected through the control API.
    pub fn key(&mut self, code: Keycode, state: KeyState, time_msec: u32) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        let pressed = state == KeyState::Pressed;
        let action = keyboard.input::<Action, _>(
            self,
            code,
            state,
            serial,
            time_msec,
            |state, modifiers, keysym| {
                // Shortcuts act on press; the client never sees the press,
                // nor the release that follows it.
                if !pressed {
                    return FilterResult::Forward;
                }
                combo_of(modifiers, &keysym)
                    .and_then(|combo| state.shortcuts.action(&combo).cloned())
                    .map_or(FilterResult::Forward, FilterResult::Intercept)
            },
        );
        if let Some(action) = action {
            self.run_action(&action, serial);
        }
    }

    fn on_pointer_motion_absolute<I: InputBackend>(
        &mut self,
        event: I::PointerMotionAbsoluteEvent,
    ) {
        let Some(output_geo) = self
            .space
            .outputs()
            .next()
            .and_then(|o| self.space.output_geometry(o))
        else {
            return;
        };
        let location = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();
        self.pointer_motion(location, event.time_msec());
    }

    /// Moves the pointer to `location` in the global logical space.
    pub fn pointer_motion(&mut self, location: Point<f64, Logical>, time_msec: u32) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        // Over the column, windows do not see the pointer.
        let over_shell = !pointer.is_grabbed()
            && (self.toasts_pointer_motion(location) || self.shell_pointer_motion(location));
        let under = if over_shell {
            None
        } else {
            self.surface_under(location)
        };
        pointer.motion(
            self,
            under,
            &MotionEvent {
                location,
                serial: SERIAL_COUNTER.next_serial(),
                time: time_msec,
            },
        );
        pointer.frame(self);
        // The cursor moved.
        self.needs_redraw = true;
    }

    fn on_pointer_button<I: InputBackend>(&mut self, event: I::PointerButtonEvent) {
        self.pointer_button(event.button_code(), event.state(), event.time_msec());
    }

    /// A pointer button by its evdev code.
    pub fn pointer_button(&mut self, button: u32, state: ButtonState, time_msec: u32) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        let at = pointer.current_location();
        let pressed = state == ButtonState::Pressed;
        if button == BTN_LEFT
            && !pointer.is_grabbed()
            && (self.toasts_pointer_button(at, pressed) || self.shell_pointer_button(at, pressed))
        {
            return;
        }

        if state == ButtonState::Pressed && !pointer.is_grabbed() {
            let at = pointer.current_location();
            let alt = self
                .seat
                .get_keyboard()
                .is_some_and(|k| k.modifier_state().alt);
            let under = self.space.element_under(at).map(|(w, _)| w.clone());
            // Alt+drag anywhere on a window, or its frame band: the
            // compositor's, and the client sees no button.
            let grab = match (&under, alt, button) {
                (Some(window), true, BTN_LEFT) => Some((window.clone(), FrameHit::Move)),
                (Some(window), true, BTN_RIGHT) => Some((
                    window.clone(),
                    FrameHit::Resize(self.quarter_edges(window, at)),
                )),
                _ if button == BTN_LEFT => self.frame_hit(at),
                _ => None,
            };
            // Click to focus, and raise.
            let clicked = grab.as_ref().map(|(w, _)| w.clone()).or(under);
            self.focus(clicked.as_ref(), serial);
            match grab {
                Some((window, FrameHit::Move)) => self.start_move(&window, button, None, serial),
                Some((window, FrameHit::Resize(edges))) => {
                    self.start_resize(&window, edges, button, None, serial)
                }
                None => {}
            }
        }

        pointer.button(
            self,
            &ButtonEvent {
                button,
                state,
                serial,
                time: time_msec,
            },
        );
        pointer.frame(self);
    }

    fn on_pointer_axis<I: InputBackend>(&mut self, event: I::PointerAxisEvent) {
        let source = event.source();
        let amount = |axis| {
            event
                .amount(axis)
                .unwrap_or_else(|| event.amount_v120(axis).unwrap_or(0.0) * 15.0 / 120.0)
        };
        if self.opacity_wheel(amount(Axis::Vertical)) {
            return;
        }
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

    /// A wheel scroll, `dy` in logical pixels (15 per notch, down positive),
    /// from a device or injected through the control API.
    pub fn scroll(&mut self, dy: f64, time_msec: u32) {
        if self.opacity_wheel(dy) {
            return;
        }
        let frame = AxisFrame::new(time_msec)
            .source(AxisSource::Wheel)
            .value(Axis::Vertical, dy)
            .v120(Axis::Vertical, (dy / 15.0 * 120.0) as i32);
        if let Some(pointer) = self.seat.get_pointer() {
            pointer.axis(self, frame);
            pointer.frame(self);
        }
    }

    /// Super+wheel over a window changes its opacity, remembered for its
    /// app; the client sees no scroll.
    fn opacity_wheel(&mut self, dy: f64) -> bool {
        let logo = self
            .seat
            .get_keyboard()
            .is_some_and(|k| k.modifier_state().logo);
        if !logo || dy == 0.0 {
            return false;
        }
        let Some(pointer) = self.seat.get_pointer() else {
            return false;
        };
        let Some(window) = self
            .space
            .element_under(pointer.current_location())
            .map(|(w, _)| w.clone())
        else {
            return false;
        };
        let Some(id) = crate::desktops::model_id(&window) else {
            return false;
        };
        let step = if dy > 0.0 { -0.05 } else { 0.05 };
        let Some(managed) = self.windows.get_mut(&id) else {
            return false;
        };
        managed.opacity = ((managed.opacity + step) * 100.0)
            .round()
            .clamp(10.0, 100.0)
            / 100.0;
        let opacity = managed.opacity;
        if let Some(app_id) = crate::desktops::title_and_app_id(&window).1 {
            self.app_opacity.insert(app_id, opacity);
        }
        self.needs_redraw = true;
        true
    }
}
