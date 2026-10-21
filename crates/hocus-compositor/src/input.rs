//! Input events from the backend, routed to clients or to the compositor.

use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent,
        KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
    },
    input::{
        keyboard::{FilterResult, Keycode, Keysym},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    utils::{Logical, Point, SERIAL_COUNTER},
};

use crate::State;

/// Evdev codes of the digit row, 1 to 9: by position, so that the same keys
/// work whatever the layout (on AZERTY, `&` to `ç`).
const KEY_1: u32 = 2;
const KEY_9: u32 = 10;
/// Evdev codes are offset by 8 in XKB.
const XKB_OFFSET: u32 = 8;

/// What a key press means to the compositor itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Quit,
    /// Super+digit: show that desktop on the output under the pointer.
    ShowDesktop(usize),
    /// Super+Shift+digit: send the focused window to that desktop of the
    /// output under the pointer.
    SendToDesktop(usize),
    /// Super+Up: maximize or restore the focused window.
    ToggleMaximized,
    /// Super+Down: minimize the focused window.
    Minimize,
}

const KEY_UP: u32 = 103;
const KEY_DOWN: u32 = 108;

/// The desktop index of a digit-row key, from 0.
fn digit(code: Keycode) -> Option<usize> {
    let evdev = code.raw().checked_sub(XKB_OFFSET)?;
    (KEY_1..=KEY_9)
        .contains(&evdev)
        .then(|| (evdev - KEY_1) as usize)
}

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
            |_, modifiers, keysym| {
                // Built in until shortcuts are configurable (#17).
                if !pressed {
                    return FilterResult::Forward;
                }
                if modifiers.ctrl && modifiers.alt && keysym.modified_sym() == Keysym::BackSpace {
                    return FilterResult::Intercept(Action::Quit);
                }
                let evdev = keysym.raw_code().raw().saturating_sub(XKB_OFFSET);
                match (modifiers.logo, modifiers.shift, digit(keysym.raw_code())) {
                    (true, false, Some(n)) => FilterResult::Intercept(Action::ShowDesktop(n)),
                    (true, true, Some(n)) => FilterResult::Intercept(Action::SendToDesktop(n)),
                    (true, false, None) if evdev == KEY_UP => {
                        FilterResult::Intercept(Action::ToggleMaximized)
                    }
                    (true, false, None) if evdev == KEY_DOWN => {
                        FilterResult::Intercept(Action::Minimize)
                    }
                    _ => FilterResult::Forward,
                }
            },
        );
        match action {
            Some(Action::Quit) => {
                tracing::info!("quit requested");
                self.loop_signal.stop();
            }
            Some(Action::ShowDesktop(n)) => {
                if let Some(output) = self.output_under_pointer() {
                    // An output with fewer desktops, or one, does not switch.
                    let _ = self.show_desktop(&output, n, serial);
                }
            }
            Some(Action::SendToDesktop(n)) => {
                if let (Some(output), Some(window)) =
                    (self.output_under_pointer(), self.model.focused())
                {
                    let _ = self.send_window(window, &output, Some(n), serial);
                }
            }
            Some(Action::ToggleMaximized) => {
                if let Some(window) = self.model.focused() {
                    self.toggle_maximized(window, serial);
                }
            }
            Some(Action::Minimize) => {
                if let Some(window) = self.model.focused() {
                    self.minimize(window, serial);
                }
            }
            None => {}
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
        let under = self.surface_under(location);
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

        // Click to focus, and raise.
        if state == ButtonState::Pressed && !pointer.is_grabbed() {
            let clicked = self
                .space
                .element_under(pointer.current_location())
                .map(|(w, _)| w.clone());
            self.focus(clicked.as_ref(), serial);
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
}
