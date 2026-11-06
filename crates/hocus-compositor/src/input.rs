//! Input from the backends, routed to clients or kept by the compositor:
//! keys (the virtual terminal switch, the shortcuts), the pointer (click to
//! focus, `Alt`+drag to move, `Super`+drag to draw on the screen),
//! scrolling and touchpad gestures, which go to the client under the
//! pointer (pointer-gestures).

use smithay::{
    backend::input::{
        self as bi, AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, GestureBeginEvent,
        GestureEndEvent, InputBackend, InputEvent, KeyState, KeyboardKeyEvent, PointerAxisEvent,
        PointerButtonEvent, PointerMotionEvent,
    },
    input::{
        keyboard::{FilterResult, Keysym, ModifiersState},
        pointer::{
            AxisFrame, ButtonEvent, CursorIcon, GestureHoldBeginEvent, GestureHoldEndEvent,
            GesturePinchBeginEvent, GesturePinchEndEvent, GesturePinchUpdateEvent,
            GestureSwipeBeginEvent, GestureSwipeEndEvent, GestureSwipeUpdateEvent, GrabStartData,
            MotionEvent, RelativeMotionEvent,
        },
    },
    utils::{Logical, Point, SERIAL_COUNTER},
};

use crate::grabs::EdgesExt as _;
use crate::{
    State,
    grabs::{BTN_LEFT, BTN_RIGHT},
    handlers::layer_shell::Stand,
    model::{self, Edges, Mode, Seam, WindowId},
    shortcuts::{Action, Chord},
    switcher::{SwitchKey, is_modifier, switch_key},
};

/// The top band of a window with no header where its grip shows (§7), and
/// how far around the grip a press still takes it.
const GRIP_BAND: i32 = 12;
const GRIP_SLOP: f64 = 8.0;
/// The handle of a pair's seam shows with the pointer this close
/// to the seam's centre line: the gap and a little of each window (§8).
const SEAM_REACH: f64 = 12.0;
/// How far around the handle's pill a press still takes it.
const HANDLE_SLOP: f64 = 4.0;
/// A right press that moves this far is a drag, not the radial menu's
/// click.
const RADIAL_SLOP: f64 = crate::radial::CLICK_SLOP;
/// Two presses on a grip this close together maximise or restore.
const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(400);

/// What a key did, when the compositor kept it.
#[derive(Debug)]
pub(crate) enum Kept {
    /// Ctrl+Alt+F1…F12: another virtual terminal.
    Vt(i32),
    /// One of the `[shortcuts]`, and its chord as held.
    Shortcut(Action, Chord),
    /// The release of a key whose press was kept.
    Release,
    /// A key while Alt+Tab is held.
    Switch(SwitchKey),
    /// A key while the capture is open (5.8).
    Capture(crate::capture::Key),
    /// `Esc` while strokes show: they go (docs/design/ink.md).
    InkClear,
    /// `Esc` while a window is moved: it goes back (8.5 B).
    CancelMove,
    /// A key on the lock's form (7.1): the session is locked.
    Lock(crate::lock::LockKey),
    /// A key while a window's radial menu is open (#723).
    Radial(crate::radial::Key),
    /// A key while the overview is open (8.5 A).
    Overview(crate::overview::Key),
}

/// The digit of a key of the keyboard's top row, whatever the layout puts
/// there: `Super+1` is the same key on every layout.
fn digit_key(code: smithay::input::keyboard::Keycode) -> Option<Keysym> {
    // XKB's keycodes are evdev's plus 8: KEY_1 is 2, KEY_0 is 11.
    match code.raw() {
        raw @ 10..=18 => Some(Keysym::new(Keysym::_1.raw() + raw - 10)),
        19 => Some(Keysym::_0),
        _ => None,
    }
}

impl State {
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        use crate::away::{Felt, Pressed};
        // Any of the user's input, devices added and removed aside, is
        // activity (7.3): it only notes the time. What lifts the curtain,
        // or wakes the screens of a session not locked, goes no further
        // (7.2).
        let felt = match &event {
            InputEvent::DeviceAdded { .. } | InputEvent::DeviceRemoved { .. } => None,
            InputEvent::SwitchToggle { event } => {
                use bi::{Switch, SwitchState, SwitchToggleEvent};
                if event.switch() == Some(Switch::Lid) {
                    self.lid(event.state() == SwitchState::On);
                }
                return;
            }
            InputEvent::Keyboard { event } => {
                let code = event.key_code().raw();
                Some(match event.state() {
                    KeyState::Pressed => Felt::Press(Pressed::Key(code)),
                    KeyState::Released => Felt::Release(Pressed::Key(code)),
                })
            }
            InputEvent::PointerMotion { event } => Some(Felt::Motion(event.delta())),
            InputEvent::PointerMotionAbsolute { event } => {
                let from = self
                    .seat
                    .get_pointer()
                    .map(|p| p.current_location())
                    .unwrap_or_default();
                let to = self.absolute_location::<I>(event).unwrap_or(from);
                Some(Felt::Motion(to - from))
            }
            InputEvent::PointerButton { event } => {
                let code = event.button_code();
                Some(match event.state() {
                    ButtonState::Pressed => Felt::Press(Pressed::Button(code)),
                    ButtonState::Released => Felt::Release(Pressed::Button(code)),
                })
            }
            InputEvent::GestureSwipeBegin { .. }
            | InputEvent::GesturePinchBegin { .. }
            | InputEvent::GestureHoldBegin { .. } => Some(Felt::Press(Pressed::Gesture)),
            InputEvent::GestureSwipeEnd { .. }
            | InputEvent::GesturePinchEnd { .. }
            | InputEvent::GestureHoldEnd { .. } => Some(Felt::Release(Pressed::Gesture)),
            InputEvent::GestureSwipeUpdate { .. } | InputEvent::GesturePinchUpdate { .. } => {
                Some(Felt::GestureUpdate)
            }
            _ => Some(Felt::Other),
        };
        if let Some(felt) = felt
            && !self.felt(felt)
        {
            return;
        }
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
        // The power button, when logind left it to the desktop (6.10).
        if self.power_key(code.raw(), state == KeyState::Pressed) {
            return;
        }
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        let kept =
            keyboard.input::<Kept, _>(self, code, state, serial, time, |this, mods, keysym| {
                // Locked (7.1): the lock decides first, every key.
                if let Some(kept) =
                    this.lock_key_filter(code, state == KeyState::Pressed, mods, &keysym)
                {
                    return kept;
                }
                // Another key than a modifier with `Super`+`Alt`: a
                // shortcut's, not the laser.
                if state == KeyState::Pressed
                    && !is_modifier(keysym.modified_sym())
                    && this.ink.other_key()
                {
                    this.redraw = true;
                }
                // The authentication dialog holds the keyboard (7.5): of
                // the compositor's keys, only the virtual terminals' and
                // the levels' work; everything else goes to the dialog.
                let auth = this.auth_holds();
                // A window's radial menu open (#723): its keys are its
                // own, the modifiers go on.
                if this.radial.is_some() && !auth {
                    let sym = keysym.modified_sym();
                    if is_modifier(sym) {
                        return FilterResult::Forward;
                    }
                    if state != KeyState::Pressed {
                        return if this.kept_keys.remove(&code) {
                            FilterResult::Intercept(Kept::Release)
                        } else {
                            FilterResult::Forward
                        };
                    }
                    this.kept_keys.insert(code);
                    return FilterResult::Intercept(Kept::Radial(crate::radial::Key::of(
                        sym, mods.shift,
                    )));
                }
                // The overview open (8.5 A): its keys are its own, the
                // modifiers go on.
                if this.overview_open() && !auth {
                    let sym = keysym.modified_sym();
                    if is_modifier(sym) {
                        return FilterResult::Forward;
                    }
                    if state != KeyState::Pressed {
                        return if this.kept_keys.remove(&code) {
                            FilterResult::Intercept(Kept::Release)
                        } else {
                            FilterResult::Forward
                        };
                    }
                    this.kept_keys.insert(code);
                    let shortcuts = &this.config.shortcuts;
                    let action = keysym
                        .raw_latin_sym_or_raw_current_sym()
                        .and_then(|key| shortcuts.action(mods, key, this.nested))
                        .or_else(|| {
                            digit_key(code).and_then(|key| shortcuts.action(mods, key, this.nested))
                        })
                        .map(|(action, _)| action);
                    return FilterResult::Intercept(Kept::Overview(crate::overview::Key::of(
                        sym, mods.shift, action,
                    )));
                }
                if let Some(switcher_key) = this.switcher.as_ref().map(|s| s.key).filter(|_| !auth)
                {
                    // Alt+Tab held: its keys are its own, the modifiers
                    // go on to the application.
                    let sym = keysym.modified_sym();
                    if is_modifier(sym) {
                        return FilterResult::Forward;
                    }
                    if state != KeyState::Pressed {
                        return if this.kept_keys.remove(&code) {
                            FilterResult::Intercept(Kept::Release)
                        } else {
                            FilterResult::Forward
                        };
                    }
                    let key = keysym.raw_latin_sym_or_raw_current_sym().unwrap_or(sym);
                    this.kept_keys.insert(code);
                    return FilterResult::Intercept(Kept::Switch(switch_key(
                        switcher_key,
                        key,
                        sym,
                        mods.shift,
                    )));
                }
                if state != KeyState::Pressed {
                    return if this.kept_keys.remove(&code) {
                        FilterResult::Intercept(Kept::Release)
                    } else {
                        FilterResult::Forward
                    };
                }
                // The capture open keeps the keys (5.8), the modifiers
                // going on, as Alt+Tab's.
                if this.capture.choosing() && !auth {
                    let sym = keysym.modified_sym();
                    if is_modifier(sym) {
                        return FilterResult::Forward;
                    }
                    this.kept_keys.insert(code);
                    return FilterResult::Intercept(Kept::Capture(crate::capture::Key::of(
                        sym, mods.shift,
                    )));
                }
                // No key is logged, not even at trace level: what is typed
                // may be a password (a terminal's, the Wi-Fi's).
                let sym = keysym.modified_sym().raw();
                if (Keysym::XF86_Switch_VT_1.raw()..=Keysym::XF86_Switch_VT_12.raw()).contains(&sym)
                {
                    let vt = (sym - Keysym::XF86_Switch_VT_1.raw() + 1) as i32;
                    return FilterResult::Intercept(Kept::Vt(vt));
                }
                // `Esc` clears the strokes while they show, with whatever
                // modifiers are still held from drawing; otherwise it goes
                // to the window.
                // `Esc` while a window is moved puts it back (8.5 B).
                if sym == Keysym::Escape.raw() && this.moving.is_some() && !auth {
                    this.kept_keys.insert(code);
                    return FilterResult::Intercept(Kept::CancelMove);
                }
                if sym == Keysym::Escape.raw() && this.ink.shows() && !auth {
                    this.kept_keys.insert(code);
                    return FilterResult::Intercept(Kept::InkClear);
                }
                // A client that takes every key (the Settings recording a
                // shortcut, 6.8): none of the desktop's, the media keys too
                // (they are shortcuts the user may record). The virtual
                // terminal's switch above stays.
                let inhibited = this.shortcuts_inhibited();
                // The key of the layout's first level, Latin when the keyboard
                // has a Latin layout: a shortcut stays on its key whatever the
                // active layout.
                let shortcuts = &this.config.shortcuts;
                let action = keysym
                    .raw_latin_sym_or_raw_current_sym()
                    .and_then(|key| shortcuts.action(mods, key, this.nested))
                    .or_else(|| {
                        digit_key(code).and_then(|key| shortcuts.action(mods, key, this.nested))
                    })
                    .or_else(|| {
                        // A key the layout types with Shift (`.` on
                        // AZERTY): the chord without Shift, Shift held.
                        let typed = keysym.modified_sym();
                        let raw = keysym.raw_latin_sym_or_raw_current_sym()?;
                        if !mods.shift || typed == raw || typed == Keysym::NoSymbol {
                            return None;
                        }
                        let mut unshifted = *mods;
                        unshifted.shift = false;
                        shortcuts.action(&unshifted, typed, this.nested)
                    });
                let action =
                    action.filter(|(a, _)| !inhibited && (!auth || matches!(a, Action::Level(_))));
                if let Some((action, chord)) = action {
                    this.kept_keys.insert(code);
                    return FilterResult::Intercept(Kept::Shortcut(action, chord));
                }
                FilterResult::Forward
            });
        if state == KeyState::Released && self.switcher_repeat.is_some_and(|(c, _)| c == code) {
            self.stop_switcher_repeat();
        }
        if state == KeyState::Released {
            self.level_key_released(code);
        }
        match kept {
            Some(Kept::Vt(vt)) => self.switch_vt(vt),
            Some(Kept::Shortcut(action, chord)) => {
                self.run_action(action, chord);
                // Alt+Tab held: Tab held steps on (§11).
                if let Action::SwitchWindows { backward } = action {
                    let key = if backward {
                        SwitchKey::Previous
                    } else {
                        SwitchKey::Next
                    };
                    self.repeat_switcher_key(code, key);
                }
                // A volume or brightness key held steps on.
                if let Action::Level(key) = action {
                    self.repeat_level_key(code, key);
                }
            }
            Some(Kept::Switch(key)) => {
                // Another key stops the one held, as in an application.
                self.stop_switcher_repeat();
                self.switcher_key(key);
                if matches!(key, SwitchKey::Next | SwitchKey::Previous) {
                    self.repeat_switcher_key(code, key);
                }
            }
            Some(Kept::Capture(key)) => self.capture_key(key),
            Some(Kept::InkClear) => self.ink_clear(),
            Some(Kept::CancelMove) => self.cancel_move(),
            Some(Kept::Lock(key)) => self.lock_key(key),
            Some(Kept::Radial(key)) => self.radial_key(key),
            Some(Kept::Overview(key)) => self.overview_key(key),
            Some(Kept::Release) | None => {}
        }
        let mods = keyboard.modifier_state();
        // Locked: Caps Lock as it is now, nothing else of the desktop's.
        if self.locked() {
            self.lock_modifiers(&mods);
            self.layout_maybe_changed();
            return;
        }
        // `Super`+`Alt` held: the laser.
        self.ink_modifiers(&mods);
        // Alt let go: Alt+Tab goes to its selection.
        if self.switcher.is_some() {
            let mods = keyboard.modifier_state();
            self.switcher_modifiers(&mods);
        }
        // A group-switch option of the user's XKB options may have changed
        // the layout: the bar's row follows.
        self.layout_maybe_changed();
        // Caps Lock, for the authentication dialog's sign (7.5).
        self.auth_caps(mods.caps_lock);
    }

    /// Whether the modifiers held move a window under the pointer with its
    /// left button: `[windows] move-modifier` (6.14), `Alt` by default.
    pub fn move_modifier(&self, modifiers: &ModifiersState) -> bool {
        self.drag_key_held(self.config.windows.move_modifier, modifiers)
    }

    /// Whether the modifiers held resize a window under the pointer with
    /// its right button: `[windows] resize-modifier`, `Super` by default.
    pub fn resize_modifier(&self, modifiers: &ModifiersState) -> bool {
        self.drag_key_held(self.config.windows.resize_modifier, modifiers)
    }

    /// Whether `key` is held for a drag in a window: `Alt` without
    /// `Super` (`Super`+drag draws, docs/design/ink.md); `Super` without
    /// `Ctrl`, nested `Alt+Super`, where the host takes `Super`.
    fn drag_key_held(&self, key: crate::config::DragKey, modifiers: &ModifiersState) -> bool {
        use crate::config::DragKey;
        match key {
            DragKey::Alt => modifiers.alt && !modifiers.logo,
            DragKey::Super => modifiers.logo && !modifiers.ctrl && (!self.nested || modifiers.alt),
            DragKey::None => false,
        }
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
        let Some(location) = self.absolute_location::<I>(event) else {
            return;
        };
        self.pointer_motion(location, event.time_msec());
    }

    /// Where an absolute motion goes: its surface spans every output.
    fn absolute_location<I: InputBackend>(
        &self,
        event: &I::PointerMotionAbsoluteEvent,
    ) -> Option<Point<f64, Logical>> {
        let area = self
            .space
            .outputs()
            .filter_map(|o| self.space.output_geometry(o))
            .reduce(|a, b| a.merge(b))?;
        Some(event.position_transformed(area.size) + area.loc.to_f64())
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

    /// Moves the pointer to `location` in the global logical space. Over
    /// a window's frame or its grip, no client sees it: the cursor shows
    /// what a press there does.
    pub fn pointer_motion(&mut self, location: Point<f64, Logical>, time: u32) {
        // Locked (7.1): to the lock's surface under it, nothing else.
        if self.locked() {
            self.lock_pointer_motion(location, time);
            return;
        }
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        // The laser's trail follows, over everything.
        self.ink_pointer(location);
        // A right press that moved: Super and a drag resizes, as it always
        // did, from where it was pressed; from the grip, nothing (#723).
        if let Some(press) = self
            .radial_press
            .filter(|p| (location.x - p.at.x).hypot(location.y - p.at.y) >= RADIAL_SLOP)
        {
            self.radial_press = None;
            if press.resizes
                && let Some(rect) = self.rect_of(press.window)
            {
                self.kept_buttons.remove(&press.button);
                let edges = Edges::nearest_corner(rect, press.at.x, press.at.y);
                let start = GrabStartData {
                    focus: None,
                    button: press.button,
                    location: press.at,
                };
                self.start_resize(press.window, edges, press.button, Some(start), press.serial);
            }
        }
        // A window's radial menu open: no client sees the pointer.
        if self.radial.is_some() {
            pointer.motion(
                self,
                None,
                &MotionEvent {
                    location,
                    serial: SERIAL_COUNTER.next_serial(),
                    time,
                },
            );
            pointer.frame(self);
            self.radial_motion(location);
            if !self.nested {
                self.redraw = true;
            }
            return;
        }
        // The overview open (8.5 A): no client sees the pointer but over
        // the bar.
        if !self.auth_holds() && self.overview_takes(location) {
            pointer.motion(
                self,
                None,
                &MotionEvent {
                    location,
                    serial: SERIAL_COUNTER.next_serial(),
                    time,
                },
            );
            pointer.frame(self);
            self.frame_cursor = None;
            self.overview_motion(location);
            if !self.nested {
                self.redraw = true;
            }
            return;
        }
        // The authentication dialog keeps it from what it dims (7.5);
        // the capture open takes it, but over its toolbar (5.8).
        if !self.auth_keeps_pointer(location) && self.capture_motion(location, time) {
            return;
        }
        // The share's chooser keeps it from what it darkens (5.11).
        if self.auth_keeps_pointer(location) || self.share_keeps_pointer(location) {
            pointer.motion(
                self,
                None,
                &MotionEvent {
                    location,
                    serial: SERIAL_COUNTER.next_serial(),
                    time,
                },
            );
            pointer.frame(self);
            self.frame_cursor = None;
            self.redraw = true;
            return;
        }
        // Against a bar's edge, or out of the bar in front (#519): first,
        // as it decides what is under the pointer.
        self.bar_front_follow(location);
        let mut under = self.surface_under(location);
        if !pointer.is_grabbed() {
            let seam = self.seam_near(location);
            self.hover_seam(seam);
            let handle = seam.filter(|s| on_handle(s, location));
            let frame = seam
                .and_then(|s| s.edge_at(location.x, location.y))
                .or_else(|| self.frame_under(location));
            let grip = self.grip_hover(location);
            self.hover_grip(grip);
            let on_grip = self.grip_hit(location).is_some();
            let cursor = match (handle, frame) {
                (Some(seam), _) => Some(seam.edges.cursor()),
                (None, Some((_, edges))) => Some(edges.cursor()),
                (None, None) if on_grip => Some(CursorIcon::Grab),
                (None, None) => None,
            };
            if cursor.is_some() {
                under = None;
            }
            if cursor != self.frame_cursor {
                self.frame_cursor = cursor;
                self.redraw = true;
            }
        }
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

    /// The window whose frame is under `at`, and the edges a press there
    /// resizes: the band outside a window, or the first pixels of the top
    /// of one with no header of its own; the window in front wins.
    pub fn frame_under(&self, at: Point<f64, Logical>) -> Option<(WindowId, Edges)> {
        if self.layer_surface_under(at, Stand::Above).is_some() {
            return None;
        }
        let in_front = self.window_under(at);
        for id in self.model.stack().iter().rev().copied() {
            if self.model.is_minimized(id) || self.model.mode(id) == Mode::Fullscreen {
                continue;
            }
            let Some(rect) = self.rect_of(id) else {
                continue;
            };
            let top_inside = if self.has_own_header(id) {
                0
            } else {
                model::TOP_INSIDE
            };
            let hit = model::frame_hit(rect, at.x, at.y, top_inside);
            if hit.is_some() || rect.contains(at.x, at.y) {
                return hit.map(|edges| (id, edges));
            }
            // A client's popup or its shadow over the band is its own.
            if in_front == Some(id) {
                return None;
            }
        }
        None
    }

    /// The seam of a pair near `at`, where it shows: its handle shows
    /// (§8).
    fn seam_near(&self, at: Point<f64, Logical>) -> Option<Seam> {
        if self.layer_surface_under(at, Stand::Above).is_some() {
            return None;
        }
        let under = self.window_under(at);
        self.seams()
            .into_iter()
            .filter(|s| s.near(at.x, at.y, SEAM_REACH))
            .find(|s| under.is_none_or(|w| !self.covers(w, s)))
    }

    /// Whether window `id` is over `seam`: in front of both its windows.
    /// One behind them, showing through the gap, is not.
    fn covers(&self, id: WindowId, seam: &Seam) -> bool {
        let stack = self.model.stack();
        let place = |w: WindowId| stack.iter().position(|s| *s == w);
        match (place(id), place(seam.first).max(place(seam.second))) {
            (Some(at), Some(pair)) => at > pair,
            _ => false,
        }
    }

    /// The window with no header whose top band the pointer is in: its
    /// grip shows (§7).
    fn grip_hover(&self, at: Point<f64, Logical>) -> Option<WindowId> {
        let id = self.window_under(at)?;
        if self.has_own_header(id) || self.model.mode(id) == Mode::Fullscreen {
            return None;
        }
        let rect = self.rect_of(id)?;
        (at.y < f64::from(rect.y + GRIP_BAND) && rect.contains(at.x, at.y)).then_some(id)
    }

    /// The window whose grip is under the pointer: a press there moves it.
    pub fn grip_hit(&self, at: Point<f64, Logical>) -> Option<WindowId> {
        let id = self.grip_hover(at)?;
        let rect = self.rect_of(id)?;
        let (cx, _) = rect.centre();
        let half = crate::decoration::GRIP_SIZE.0 / 2.0 + GRIP_SLOP;
        let top = f64::from(rect.y + model::TOP_INSIDE);
        ((at.x - cx).abs() <= half && at.y >= top && at.y < f64::from(rect.y + GRIP_BAND))
            .then_some(id)
    }

    /// The pointer's focus to what is under it now, if that is no longer
    /// what has it: a surface came, went or moved under the pointer since
    /// it last moved, and a press would go to what was there, not to what
    /// is shown (#692). Not while a grab holds the pointer, nor over a
    /// window's frame, where no client has it.
    fn pointer_refocus(&mut self, time: u32) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        if pointer.is_grabbed() || self.frame_cursor.is_some() {
            return;
        }
        let at = pointer.current_location();
        let under = self.surface_under(at).map(|(surface, _)| surface);
        if under != pointer.current_focus() {
            self.pointer_motion(at, time);
        }
    }

    /// A pointer button by its evdev code. A press focuses the window
    /// under the pointer and goes to it; with `Alt` the left button moves
    /// the window, with `Super` the right one resizes it, and the client
    /// sees nothing; `Super` and the left button draw, anywhere. A press on a window's frame resizes it, on its grip
    /// moves it; a press away from an open menu closes it; a left press on
    /// a bar's edge switches its form.
    pub fn pointer_button(&mut self, button: u32, state: ButtonState, time: u32) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        // Locked (7.1): to the lock's surface under the pointer alone (its
        // focus), the desktop doing nothing with it.
        if self.locked() {
            if state == ButtonState::Released && self.kept_buttons.remove(&button) {
                return;
            }
            pointer.button(
                self,
                &ButtonEvent {
                    button,
                    state,
                    serial: SERIAL_COUNTER.next_serial(),
                    time,
                },
            );
            pointer.frame(self);
            return;
        }
        // A window's radial menu (#723): the press that opens it, let go
        // where it was; then the menu takes every press, no client seeing
        // them.
        if state == ButtonState::Released
            && let Some(press) = self.radial_press.filter(|p| p.button == button)
        {
            self.radial_press = None;
            self.kept_buttons.remove(&button);
            self.open_radial(press.window, pointer.current_location());
            return;
        }
        if self.radial.is_some() && !self.auth_holds() {
            if state == ButtonState::Pressed {
                self.kept_buttons.insert(button);
                if matches!(button, BTN_LEFT | BTN_RIGHT) {
                    self.radial_press(pointer.current_location());
                }
            } else {
                self.kept_buttons.remove(&button);
            }
            return;
        }
        // The overview (8.5 A): its presses, but over the bar.
        if !self.auth_holds() && self.overview.is_some() {
            let at = pointer.current_location();
            if state == ButtonState::Released && self.overview_release(button, at) {
                self.kept_buttons.remove(&button);
                return;
            }
            if state == ButtonState::Pressed && self.overview_takes(at) {
                self.kept_buttons.insert(button);
                self.overview_press(button, at);
                return;
            }
        }
        if state == ButtonState::Pressed {
            self.pointer_refocus(time);
        }
        // A press away from the authentication dialog does nothing (7.5).
        if state == ButtonState::Pressed && self.auth_keeps_pointer(pointer.current_location()) {
            self.kept_buttons.insert(button);
            return;
        }
        // The capture open takes it, but over its toolbar (5.8).
        if !self.auth_holds() && self.capture_button(button, state, time) {
            return;
        }
        let serial = SERIAL_COUNTER.next_serial();
        let at = pointer.current_location();
        if state == ButtonState::Released && self.kept_buttons.remove(&button) {
            return;
        }
        // The share's chooser keeps the presses off what it darkens.
        if state == ButtonState::Pressed && self.share_keeps_pointer(at) {
            self.kept_buttons.insert(button);
            return;
        }
        // Alt+Tab's panel takes the click.
        if state == ButtonState::Pressed && self.switcher_click(at.x, at.y) {
            self.kept_buttons.insert(button);
            return;
        }
        // `Super`+drag draws, wherever it starts: over a window, the bar
        // or another shell surface (docs/design/ink.md).
        if state == ButtonState::Pressed
            && button == BTN_LEFT
            && !pointer.is_grabbed()
            && !self.auth_holds()
            && let Some(shape) = self
                .seat
                .get_keyboard()
                .and_then(|k| self.ink_shape(&k.modifier_state()))
        {
            self.start_ink(shape, button, serial);
            return;
        }
        if state == ButtonState::Pressed && !pointer.is_grabbed() {
            if self.dismiss_popups_at(at) || self.press_outside_shell(at) {
                self.kept_buttons.insert(button);
                return;
            }
            if button == BTN_LEFT
                && let Some(output) = self.bar_edge_at(at)
            {
                // The bar's edge switches its form; no client sees the press.
                self.toggle_bar(&output);
                self.kept_buttons.insert(button);
                return;
            }
            let seam = self.seam_near(at);
            if self.layer_click(at) {
                // A bar or an overlay: the click is its own.
            } else if let Some(seam) = seam.filter(|s| on_handle(s, at)) {
                // The handle: the boundary moves, both windows follow.
                if button == BTN_LEFT {
                    self.start_pair_resize(seam.grab, seam.edges, button, serial);
                } else {
                    self.kept_buttons.insert(button);
                }
                return;
            } else if let Some((id, edges)) = seam
                .and_then(|s| s.edge_at(at.x, at.y))
                .or_else(|| self.frame_under(at))
            {
                self.focus(Some(id));
                if button == BTN_LEFT {
                    self.start_resize(id, edges, button, None, serial);
                } else {
                    self.kept_buttons.insert(button);
                }
                return;
            } else if let Some(id) = self.grip_hit(at) {
                self.focus(Some(id));
                if button == BTN_LEFT {
                    let now = self.now();
                    let double = self
                        .last_grip_press
                        .take()
                        .is_some_and(|(w, t)| w == id && now.saturating_sub(t) < DOUBLE_CLICK);
                    if double {
                        self.toggle_maximized(id);
                        self.kept_buttons.insert(button);
                    } else {
                        self.last_grip_press = Some((id, now));
                        self.start_move(id, button, None, serial);
                    }
                } else {
                    // A right click on the grip: the window's radial menu,
                    // as it is let go (#723).
                    if button == BTN_RIGHT {
                        self.radial_press = Some(crate::radial::Press {
                            window: id,
                            at,
                            button,
                            serial,
                            resizes: false,
                        });
                    }
                    self.kept_buttons.insert(button);
                }
                return;
            } else if let Some(id) = self.window_under(at) {
                self.focus(Some(id));
                let modifiers = self.seat.get_keyboard().map(|k| k.modifier_state());
                if button == BTN_LEFT && modifiers.is_some_and(|m| self.move_modifier(&m)) {
                    self.start_move(id, button, None, serial);
                    return;
                }
                // `Super` and the right button: a click opens the window's
                // radial menu, a drag resizes it (#723).
                if button == BTN_RIGHT && modifiers.is_some_and(|m| self.resize_modifier(&m)) {
                    self.radial_press = Some(crate::radial::Press {
                        window: id,
                        at,
                        button,
                        serial,
                        resizes: true,
                    });
                    self.kept_buttons.insert(button);
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
            // Nothing under the authentication dialog's dim scrolls (7.5).
            if self.auth_keeps_pointer(pointer.current_location()) {
                return;
            }
            pointer.axis(self, frame);
            pointer.frame(self);
        }
    }

    /// A touchpad gesture: three fingers across move the desktops (§12),
    /// four up or down the launcher (6.13), as `[touchpad]` says
    /// (`crate::gestures`); the others go to the client under the
    /// pointer.
    pub(crate) fn gesture(&mut self, gesture: Gesture) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        // Nothing under the authentication dialog's dim sees one (7.5).
        if self.auth_keeps_pointer(pointer.current_location()) {
            return;
        }
        // Locked (7.1): no swipe of the desktop's starts; the lock's
        // surface has the pointer.
        let starts = matches!(gesture, Gesture::SwipeBegin { .. });
        if !(starts && self.locked()) && self.take_swipe(gesture, pointer.is_grabbed()) {
            return;
        }
        self.gesture_to_client(gesture);
    }

    /// A touchpad gesture for the client under the pointer.
    pub(crate) fn gesture_to_client(&mut self, gesture: Gesture) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        match gesture {
            Gesture::SwipeBegin { .. } => self.swipes_given.0 += 1,
            Gesture::SwipeUpdate { .. } => self.swipes_given.1 += 1,
            _ => {}
        }
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

/// Whether `at` is on the handle of `seam`, or close enough around it.
fn on_handle(seam: &Seam, at: Point<f64, Logical>) -> bool {
    use crate::decoration::{HANDLE_LENGTH, HANDLE_THICKNESS};
    seam.on_handle(
        at.x,
        at.y,
        HANDLE_LENGTH + 2.0 * HANDLE_SLOP,
        HANDLE_THICKNESS + 2.0 * HANDLE_SLOP,
    )
}

/// A touchpad gesture's step, as libinput gives it.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Gesture {
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
