//! Input events from the backend, routed to clients or to the compositor.

use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent,
        KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
    },
    input::{
        keyboard::{FilterResult, Keycode, Keysym},
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

/// What a key did, when the compositor kept it.
enum Handled {
    Shortcut(Action),
    /// Ctrl+Alt+F1…F12: another virtual terminal.
    Vt(i32),
    /// Keys pressed in the action palette: what it looks for.
    PaletteKeys(hocus_core::shortcuts::Combo),
    /// The modifier holding the window switcher was let go.
    AltTabDone,
    /// A key for the open launcher or quick settings.
    Panel(
        smithay::input::keyboard::Keysym,
        smithay::input::keyboard::ModifiersState,
    ),
}

impl State {
    /// A panel that takes the keys is open: the launcher, quick settings,
    /// a chooser, a card, polkit's dialog, the window switcher, a reply
    /// typed into a toast.
    pub fn modal_open(&self) -> bool {
        self.launcher_open()
            || self.toast_replying()
            || self.share_open()
            || self.quick_open()
            || self.showcase_open()
            || self.card_open()
            || self.leave_open()
            || self.auth_open()
            || self.circle_open()
            || self.picking()
            || self.alt_tab_open()
    }

    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        match event {
            InputEvent::Keyboard { event } => self.on_key::<I>(event),
            InputEvent::PointerMotionAbsolute { event } => {
                self.on_pointer_motion_absolute::<I>(event)
            }
            InputEvent::PointerMotion { event } => self.on_pointer_motion::<I>(event),
            InputEvent::PointerButton { event } => self.on_pointer_button::<I>(event),
            InputEvent::PointerAxis { event } => self.on_pointer_axis::<I>(event),
            // Three fingers sideways swipe desktops (#36); three and four
            // up and down act as set (#86).
            InputEvent::GestureSwipeBegin { event } => {
                use smithay::backend::input::GestureBeginEvent;
                self.gesture_begin(event.fingers());
            }
            InputEvent::GestureSwipeUpdate { event } => {
                use smithay::backend::input::GestureSwipeUpdateEvent;
                self.gesture_update(event.delta_x(), event.delta_y());
            }
            InputEvent::GestureSwipeEnd { event } => {
                use smithay::backend::input::GestureEndEvent;
                self.gesture_end(event.cancelled());
            }
            // A pinch closing in opens the window switcher (#39).
            InputEvent::GesturePinchBegin { event } => {
                use smithay::backend::input::GestureBeginEvent;
                self.pinch_begin(event.fingers());
            }
            InputEvent::GesturePinchUpdate { event } => {
                use smithay::backend::input::GesturePinchUpdateEvent;
                self.pinch_update(event.scale());
            }
            InputEvent::GesturePinchEnd { event } => {
                use smithay::backend::input::GestureEndEvent;
                self.pinch_end(event.cancelled());
            }
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
        self.activity();
        let handled = keyboard.input::<Handled, _>(
            self,
            code,
            state,
            serial,
            time_msec,
            |state, modifiers, keysym| {
                // Super alone (#86): acted on once the key is handled; let
                // go, the drawing on the screen fades.
                let is_super = matches!(keysym.modified_sym(), Keysym::Super_L | Keysym::Super_R);
                state.super_key(
                    pressed,
                    is_super,
                    modifiers.ctrl || modifiers.alt || modifiers.shift,
                );
                if !pressed && is_super {
                    state.ink_lift();
                }
                // The window switcher picks when what held it is let go.
                if !pressed && state.alt_tab_released(modifiers) {
                    return FilterResult::Intercept(Handled::AltTabDone);
                }
                // Shortcuts act on press; the client never sees the press,
                // nor the release that follows it.
                if !pressed {
                    return FilterResult::Forward;
                }
                // Always, even locked: the other terminal is not this
                // session, and a stuck session must not hold the machine.
                let sym = keysym.modified_sym().raw();
                if (Keysym::XF86_Switch_VT_1.raw()..=Keysym::XF86_Switch_VT_12.raw()).contains(&sym)
                {
                    let vt = (sym - Keysym::XF86_Switch_VT_1.raw() + 1) as i32;
                    return FilterResult::Intercept(Handled::Vt(vt));
                }
                // Locked: no shortcut; our lock screen takes the keys, a
                // lock client gets them as the surface with the keyboard.
                if state.locked() {
                    return FilterResult::Intercept(Handled::Panel(
                        keysym.modified_sym(),
                        *modifiers,
                    ));
                }
                // Shown in a recording, when asked; never while locked.
                state.recording_key(modifiers, keysym.modified_sym());
                // A window may hold the shortcuts back while it has the
                // keyboard (keyboard-shortcuts-inhibit).
                let combo = combo_of(modifiers, &keysym);
                // Stopping the agents is never held back: not by a window
                // that holds the shortcuts, nor by an open panel.
                if combo
                    .as_ref()
                    .and_then(|c| state.shortcuts.action(c))
                    .is_some_and(|a| *a == Action::StopAgents)
                {
                    return FilterResult::Intercept(Handled::Shortcut(Action::StopAgents));
                }
                // In the palette, a combination is a search: shortcuts and
                // keys with Super, Ctrl or Alt; its own closes it.
                if state.palette_open()
                    && let Some(combo) = &combo
                {
                    let bound = state.shortcuts.action(combo).cloned();
                    let m = combo.modifiers;
                    if bound != Some(Action::Palette)
                        && (bound.is_some() || m.logo || m.ctrl || m.alt)
                    {
                        return FilterResult::Intercept(Handled::PaletteKeys(combo.clone()));
                    }
                }
                let action = combo
                    .filter(|_| !state.shortcuts_inhibited())
                    .and_then(|combo| state.shortcuts.action(&combo).cloned());
                let modal = state.modal_open();
                match action {
                    // A panel's own shortcut closes it; others still work.
                    Some(action)
                        if !modal
                            || matches!(
                                action,
                                Action::Launcher
                                    | Action::QuickSettings
                                    | Action::ClipboardHistory
                                    | Action::Emoji
                                    | Action::Palette
                                    | Action::Notes
                            ) =>
                    {
                        FilterResult::Intercept(Handled::Shortcut(action))
                    }
                    // While a panel is open, keys are for it.
                    _ if modal => {
                        FilterResult::Intercept(Handled::Panel(keysym.modified_sym(), *modifiers))
                    }
                    _ => FilterResult::Forward,
                }
            },
        );
        self.super_fired();
        match handled {
            Some(Handled::Shortcut(action)) => {
                // A circle's menu gives way to what was asked for.
                self.close_circle();
                self.run_action(&action, serial);
            }
            Some(Handled::PaletteKeys(combo)) => self.palette_keys(&combo),
            Some(Handled::Vt(vt)) => {
                use smithay::backend::session::Session as _;
                if let Some(session) = &mut self.vt_switch
                    && let Err(err) = session.change_vt(vt)
                {
                    tracing::warn!(vt, %err, "could not switch terminal");
                }
            }
            Some(Handled::Panel(sym, modifiers)) if self.locked() => {
                self.lock_key(sym, &modifiers);
            }
            Some(Handled::AltTabDone) => self.alt_tab_pick(serial),
            Some(Handled::Panel(sym, modifiers)) if self.alt_tab_open() => {
                self.alt_tab_key(sym, &modifiers, serial);
            }
            Some(Handled::Panel(sym, _)) if self.share_open() => {
                self.share_key(sym);
            }
            Some(Handled::Panel(sym, _)) if self.picking() => {
                self.picker_key(sym);
            }
            Some(Handled::Panel(sym, modifiers)) if self.auth_open() => {
                self.auth_key(sym, &modifiers);
            }
            Some(Handled::Panel(sym, modifiers)) if self.circle_open() => {
                self.circle_key(sym, &modifiers);
            }
            Some(Handled::Panel(sym, _)) if self.leave_open() => {
                self.leave_key(sym);
            }
            Some(Handled::Panel(sym, _)) if self.card_open() => {
                self.card_key(sym);
            }
            // A reply typed into a toast has the keys, unless a panel
            // opened over it.
            Some(Handled::Panel(sym, modifiers))
                if self.toast_replying()
                    && !self.launcher_open()
                    && !self.quick_open()
                    && !self.showcase_open() =>
            {
                self.toast_key(sym, &modifiers);
            }
            Some(Handled::Panel(sym, modifiers))
                if !self.showcase_key(sym) && !self.launcher_key(sym, &modifiers) =>
            {
                self.quick_key(sym);
            }
            Some(Handled::Panel(..)) => {}
            None => {}
        }
    }

    /// A tablet, or a VM's pointer: its whole surface spans every output.
    fn on_pointer_motion_absolute<I: InputBackend>(
        &mut self,
        event: I::PointerMotionAbsoluteEvent,
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

    /// A mouse or touchpad: the pointer moves by the motion, and stays on
    /// the outputs.
    fn on_pointer_motion<I: InputBackend>(&mut self, event: I::PointerMotionEvent) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let to = pointer.current_location() + event.delta();
        let location = self.onto_outputs(to);
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
    pub fn pointer_motion(&mut self, location: Point<f64, Logical>, time_msec: u32) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        self.activity();
        // Locked: only a lock client's surface sees the pointer.
        if self.locked() {
            let under = self.lock_surface_under(location);
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
            self.needs_redraw = true;
            return;
        }
        // Games and 3D tools read the motion itself (relative-pointer),
        // and may hold the pointer still or within their window.
        let from = pointer.current_location();
        // The motion asked for, even when the pointer stays still.
        let delta = location - from;
        let location = self.constrained(from, location);
        self.corner_pointer(location);
        self.ink_motion(location);
        self.circle_motion(location);
        if !pointer.is_grabbed() {
            let focus = pointer
                .current_focus()
                .and_then(|s| self.surface_under(from).filter(|(u, _)| *u == s));
            pointer.relative_motion(
                self,
                focus,
                &smithay::input::pointer::RelativeMotionEvent {
                    delta,
                    delta_unaccel: delta,
                    utime: self.clock.now().as_micros() as u64,
                },
            );
        }
        // Over the column, windows do not see the pointer.
        let over_shell = !pointer.is_grabbed()
            && (self.alt_tab_pointer(location, None, SERIAL_COUNTER.next_serial())
                || self.dock_pointer(location, None)
                || self.share_pointer(location, None)
                || self.recording_pointer(location, None)
                || self.picker_pointer(location, None)
                || self.auth_pointer(location, None)
                || self.circle_pointer_motion(location)
                || self.leave_pointer(location, None)
                || self.card_pointer(location, None)
                || self.showcase_pointer(None)
                || self.quick_pointer_motion(location)
                || self.launcher_pointer_motion(location)
                || self.toasts_pointer_motion(location)
                || self.shell_pointer_motion(location)
                || self.bars_pointer_motion(location));
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
        self.activity();
        // Locked: a lock client's surface gets the button, nothing else.
        if self.locked() {
            if self.lock_surface_under(at).is_some() {
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
            return;
        }
        // Super with a button is no longer Super alone (#86); with the
        // left one it draws on the screen, and no client sees it.
        self.super_spoiled();
        // With Shift too, it circles something to ask about it (#46).
        if !pointer.is_grabbed() && self.circle_button(at, button, pressed) {
            self.needs_redraw = true;
            return;
        }
        if !pointer.is_grabbed() && self.ink_button(at, button, pressed) {
            self.needs_redraw = true;
            return;
        }
        self.recording_click(at, pressed);
        if button == BTN_LEFT
            && !pointer.is_grabbed()
            && (self.alt_tab_pointer(at, Some(pressed), serial)
                || self.dock_pointer(at, Some(pressed))
                || self.share_pointer(at, Some(pressed))
                || self.recording_pointer(at, Some(pressed))
                || self.picker_pointer(at, Some(pressed))
                || self.auth_pointer(at, Some(pressed))
                || self.leave_pointer(at, Some(pressed))
                || self.card_pointer(at, Some(pressed))
                || self.showcase_pointer(Some(pressed))
                || self.quick_pointer_button(at, pressed)
                || self.launcher_pointer_button(at, pressed)
                || self.toasts_pointer_button(at, pressed)
                || self.shell_pointer_button(at, pressed))
        {
            return;
        }
        // Over the shell, the secondary button opens a tray menu or
        // nothing.
        if button == crate::grabs::BTN_RIGHT
            && !pointer.is_grabbed()
            && pressed
            && self.shell_secondary_click(at)
        {
            return;
        }
        // Title bars take any button.
        if !pointer.is_grabbed() && self.bars_pointer_button(at, button, pressed) {
            return;
        }

        // Over a third-party bar or overlay, a click is the surface's: no
        // window is focused or grabbed.
        let on_layer = state == ButtonState::Pressed
            && !pointer.is_grabbed()
            && self
                .layer_surface_under(at, &crate::handlers::layer_shell::ABOVE)
                .is_some();
        if on_layer {
            self.layer_click(at);
        } else if state == ButtonState::Pressed && !pointer.is_grabbed() {
            self.layer_release_keyboard();
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
        self.activity();
        self.super_spoiled();
        let dy = amount(Axis::Vertical);
        if self.opacity_wheel(dy) || self.circle_wheel(dy) || self.launcher_wheel(dy) {
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
        self.activity();
        self.super_spoiled();
        if self.opacity_wheel(dy) || self.circle_wheel(dy) || self.launcher_wheel(dy) {
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
        if self.locked() {
            return false;
        }
        let (logo, alt) = self
            .seat
            .get_keyboard()
            .map(|k| k.modifier_state())
            .map_or((false, false), |m| (m.logo, m.alt));
        if !logo || dy == 0.0 {
            return false;
        }
        // Super+Alt+wheel: the magnifier, closer upwards.
        if alt {
            let notches = (-dy / 15.0).clamp(-4.0, 4.0);
            self.set_zoom(self.comfort.zoom * 1.1f64.powf(notches));
            return true;
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
