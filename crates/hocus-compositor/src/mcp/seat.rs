//! An MCP agent's own seat (docs/design/mcp.md, "In applications"): a
//! `wl_seat` named `agent-NAME` for each connection of an agent the file
//! names, with a pointer and a keyboard whose focus is its own. Its clicks
//! and keys reach the window asked while the user's pointer, keyboard focus
//! and stacking stay as they are; the keys go through `FilterResult::Forward`
//! straight to the window, never past the desktop's shortcuts.
//!
//! Ported from v0-first-build's `crates/hocus-compositor/src/seat.rs` (the
//! seat and its going, the point under which a window's surface is found
//! wherever it is, the keyboard following clicks, a seat not yet taken by
//! an application) and `act.rs` (the keys typing a text on the layout in
//! effect). New: the keys are looked up in a keymap of the compositor's
//! own, compiled from the user's layouts, rather than in the seat's
//! through smithay's unsafe accessor.
//!
//! A window on a desktop no screen shows is driven while an agent acted on
//! it in the last [`DRIVEN_FOR`]: its application gets a frame callback
//! every [`UNSEEN_FRAME`], on a timer of the loop that stops once none is.

use std::time::{Duration, Instant};

use smithay::{
    backend::input::{Axis, AxisSource, ButtonState, KeyState},
    desktop::WindowSurfaceType,
    input::{
        Seat,
        keyboard::{FilterResult, Keycode, Keysym, Layout, XkbConfig, xkb},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    reexports::{
        calloop::{
            RegistrationToken,
            timer::{TimeoutAction, Timer},
        },
        wayland_server::{Resource, backend::GlobalId},
    },
    utils::{Logical, Point, SERIAL_COUNTER},
    wayland::seat::WaylandFocus,
};

use crate::{State, focus::KeyboardFocus, model::WindowId};

/// Evdev codes are offset by 8 in XKB.
const XKB_OFFSET: u32 = 8;
/// A window no screen shows is driven this long after an agent acted on
/// it, and its pointer shows this long after it acted.
pub const DRIVEN_FOR: Duration = Duration::from_secs(10);
/// How often such a window gets a frame callback.
const UNSEEN_FRAME: Duration = Duration::from_millis(100);
/// How long a seat's global stays, disabled, after its agent went: a
/// client binding it meanwhile is not cut off.
const GLOBAL_LINGER: Duration = Duration::from_secs(5);
/// How long an application may take to take a seat that just came.
pub const BIND_WAIT: Duration = Duration::from_secs(2);

const KEY_LEFTSHIFT: u32 = 42;
const KEY_LEFTCTRL: u32 = 29;
const KEY_LEFTALT: u32 = 56;
const KEY_RIGHTALT: u32 = 100;
const KEY_LEFTMETA: u32 = 125;

/// One connection's seat.
#[derive(Debug)]
pub struct AgentSeat {
    pub session: u64,
    pub agent: String,
    pub seat: Seat<State>,
    /// When it came: applications take it a moment later.
    pub since: Instant,
    /// Where its pointer is (global, logical), on which window.
    pub at: Option<(Point<f64, Logical>, WindowId)>,
    /// When it last acted.
    pub last: Option<Instant>,
    /// Its pointer moved before the application under it took the seat:
    /// it leaves and comes in again, so that the application hears it
    /// come.
    pub unheard: bool,
}

/// The agents' seats.
#[derive(Default)]
pub struct Seats {
    pub list: Vec<AgentSeat>,
    next: u64,
    /// The keymap typing is looked up in, and the layouts it was made of.
    keymap: Option<(String, xkb::Keymap)>,
    unseen: Option<RegistrationToken>,
}

impl std::fmt::Debug for Seats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Seats")
            .field("list", &self.list)
            .field("keymap", &self.keymap.as_ref().map(|(k, _)| k))
            .finish_non_exhaustive()
    }
}

/// A key and the level of the layout it gives a keysym at.
#[derive(Debug, Clone, Copy)]
struct Stroke {
    code: u32,
    level: u32,
}

impl Stroke {
    /// The modifiers that reach its level: Shift for the second, AltGr
    /// for the third, both for the fourth.
    fn modifiers(self) -> &'static [u32] {
        match self.level {
            1 => &[KEY_LEFTSHIFT],
            2 => &[KEY_RIGHTALT],
            3 => &[KEY_LEFTSHIFT, KEY_RIGHTALT],
            _ => &[],
        }
    }
}

/// The keysym a character types; line breaks and tabs are keys.
fn char_keysym(c: char) -> Keysym {
    match c {
        '\n' | '\r' => Keysym::Return,
        '\t' => Keysym::Tab,
        c => xkb::utf32_to_keysym(u32::from(c)),
    }
}

impl State {
    /// Connection `session` of `agent` gets a seat of its own.
    pub(crate) fn mcp_seat_join(&mut self, session: u64, agent: &str) {
        let mut name = format!("agent-{agent}");
        while self.mcp.seats.list.iter().any(|s| s.seat.name() == name) {
            self.mcp.seats.next += 1;
            name = format!("agent-{agent}-{}", self.mcp.seats.next + 1);
        }
        let mut seat = self
            .protocols
            .seat_state
            .new_wl_seat(&self.display_handle, name);
        let names = self.config.xkb_names();
        let xkb = XkbConfig {
            model: &names.model,
            layout: &names.layout,
            variant: &names.variant,
            options: (!names.options.is_empty()).then(|| names.options.clone()),
            ..XkbConfig::default()
        };
        if seat.add_keyboard(xkb, 200, 25).is_err()
            && let Err(err) = seat.add_keyboard(XkbConfig::default(), 200, 25)
        {
            tracing::warn!(?err, agent, "an agent's seat has no keyboard");
        }
        seat.add_pointer();
        tracing::info!(agent, session, "an MCP agent's seat");
        self.mcp.seats.list.push(AgentSeat {
            session,
            agent: agent.to_owned(),
            seat,
            since: Instant::now(),
            at: None,
            last: None,
            unheard: false,
        });
    }

    /// The seat of connection `session` goes with it.
    pub(crate) fn mcp_seat_leave(&mut self, session: u64) {
        let Some(i) = self
            .mcp
            .seats
            .list
            .iter()
            .position(|s| s.session == session)
        else {
            return;
        };
        let mut gone = self.mcp.seats.list.remove(i);
        let serial = SERIAL_COUNTER.next_serial();
        if let Some(keyboard) = gone.seat.get_keyboard() {
            keyboard.set_focus(self, None, serial);
        }
        if let Some(pointer) = gone.seat.get_pointer() {
            let location = pointer.current_location();
            pointer.motion(
                self,
                None,
                &MotionEvent {
                    location,
                    serial,
                    time: self.mcp_time(),
                },
            );
            pointer.frame(self);
        }
        gone.seat.remove_keyboard();
        gone.seat.remove_pointer();
        if let Some(global) = gone.seat.global() {
            self.display_handle.disable_global::<State>(global.clone());
            let retire = move |app: &mut crate::App, global: GlobalId| {
                app.state.display_handle.remove_global::<State>(global);
            };
            let timer = self.loop_handle.insert_source(
                Timer::from_duration(GLOBAL_LINGER),
                move |_, _, app| {
                    retire(app, global.clone());
                    TimeoutAction::Drop
                },
            );
            if let Err(err) = timer {
                tracing::warn!(error = %err.error, "an agent's seat's global stays disabled");
            }
        }
        tracing::info!(agent = gone.agent, session, "an MCP agent's seat went");
        self.redraw = true;
    }

    fn mcp_time(&self) -> u32 {
        let now: Duration = self.clock.now().into();
        now.as_millis() as u32
    }

    pub(crate) fn mcp_seat(&self, session: u64) -> Option<&AgentSeat> {
        self.mcp.seats.list.iter().find(|s| s.session == session)
    }

    fn mcp_seat_mut(&mut self, session: u64) -> Option<&mut AgentSeat> {
        self.mcp
            .seats
            .list
            .iter_mut()
            .find(|s| s.session == session)
    }

    /// Whether window `id`'s application took the seat of `session`: its
    /// keyboard for `keys`, else its pointer.
    pub(crate) fn mcp_seat_taken(&self, session: u64, id: WindowId, keys: bool) -> bool {
        let (Some(seat), Some(client)) = (
            self.mcp_seat(session).map(|s| &s.seat),
            self.windows
                .get(&id)
                .and_then(|m| m.window.wl_surface())
                .and_then(|s| s.client()),
        ) else {
            return false;
        };
        if keys {
            seat.get_keyboard()
                .is_some_and(|k| k.client_keyboards(&client).next().is_some())
        } else {
            seat.get_pointer()
                .is_some_and(|p| p.client_pointers(&client).next().is_some())
        }
    }

    /// A point of window `id` (from its top left; its centre when left
    /// out) in the global space, wherever the window is: on a screen or
    /// on a desktop no screen shows.
    pub(crate) fn mcp_point(
        &self,
        id: WindowId,
        x: Option<f64>,
        y: Option<f64>,
    ) -> Result<Point<f64, Logical>, String> {
        let rect = self
            .home_rect(id)
            .ok_or_else(|| format!("window {} has no place", id.0))?;
        let (w, h) = (f64::from(rect.width), f64::from(rect.height));
        let (x, y) = (x.unwrap_or(w / 2.0), y.unwrap_or(h / 2.0));
        if !(0.0..w).contains(&x) || !(0.0..h).contains(&y) {
            return Err(format!(
                "{x},{y} is outside window {}, which is {w}×{h}",
                id.0
            ));
        }
        Ok(Point::from((f64::from(rect.x) + x, f64::from(rect.y) + y)))
    }

    /// The pointer of `session` to `at` of window `id`: the window's own
    /// surface there, whatever is over it.
    pub(crate) fn mcp_pointer_to(&mut self, session: u64, id: WindowId, at: Point<f64, Logical>) {
        let Some(pointer) = self.mcp_seat(session).and_then(|s| s.seat.get_pointer()) else {
            return;
        };
        let (Some(window), Some(rect)) = (
            self.windows.get(&id).map(|m| m.window.clone()),
            self.home_rect(id),
        ) else {
            return;
        };
        // Its surfaces start where its geometry says, on a screen or not.
        let origin = Point::<i32, Logical>::from((rect.x, rect.y)) - window.geometry().loc;
        let focus = window
            .surface_under(at - origin.to_f64(), WindowSurfaceType::ALL)
            .map(|(s, p)| (s, (p + origin).to_f64()));
        let time = self.mcp_time();
        let taken = self.mcp_seat_taken(session, id, false);
        if taken && self.mcp_seat(session).is_some_and(|s| s.unheard) {
            pointer.motion(
                self,
                None,
                &MotionEvent {
                    location: at,
                    serial: SERIAL_COUNTER.next_serial(),
                    time,
                },
            );
        }
        pointer.motion(
            self,
            focus,
            &MotionEvent {
                location: at,
                serial: SERIAL_COUNTER.next_serial(),
                time,
            },
        );
        pointer.frame(self);
        if let Some(seat) = self.mcp_seat_mut(session) {
            seat.at = Some((at, id));
            seat.unheard = !taken;
        }
    }

    /// A click of `button`, `count` times, where the pointer of `session`
    /// is; its keyboard follows it into window `id`.
    pub(crate) fn mcp_click(&mut self, session: u64, id: WindowId, button: u32, count: u8) {
        self.mcp_keys_to(session, id);
        let Some(pointer) = self.mcp_seat(session).and_then(|s| s.seat.get_pointer()) else {
            return;
        };
        let time = self.mcp_time();
        for _ in 0..count.clamp(1, 3) {
            for state in [ButtonState::Pressed, ButtonState::Released] {
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
            }
        }
    }

    /// The wheel, `dy` logical pixels, where the pointer of `session` is.
    pub(crate) fn mcp_scroll(&mut self, session: u64, dy: f64) {
        let Some(pointer) = self.mcp_seat(session).and_then(|s| s.seat.get_pointer()) else {
            return;
        };
        let frame = AxisFrame::new(self.mcp_time())
            .source(AxisSource::Wheel)
            .value(Axis::Vertical, dy)
            .v120(Axis::Vertical, (dy / 15.0 * 120.0) as i32);
        pointer.axis(self, frame);
        pointer.frame(self);
    }

    /// The keyboard of `session` into window `id`: whether it came now (its
    /// application then says what its field is for).
    pub(crate) fn mcp_keys_to(&mut self, session: u64, id: WindowId) -> bool {
        let Some(seat) = self.mcp_seat(session).map(|s| s.seat.clone()) else {
            return false;
        };
        let (Some(keyboard), Some(surface)) = (
            seat.get_keyboard(),
            self.windows
                .get(&id)
                .and_then(|m| m.window.wl_surface().map(|s| s.into_owned())),
        ) else {
            return false;
        };
        let focus = KeyboardFocus::Wayland(surface);
        if keyboard.current_focus().as_ref() == Some(&focus) {
            return false;
        }
        // The layout the user types with now.
        let layout = self
            .seat
            .get_keyboard()
            .map(|k| k.modifier_state().serialized.layout_effective);
        if let Some(layout) = layout {
            keyboard.with_xkb_state(self, |mut context| context.set_layout(Layout(layout)));
        }
        keyboard.set_focus(self, Some(focus), SERIAL_COUNTER.next_serial());
        true
    }

    /// Whether window `id`'s field that has the keyboard of `session` is
    /// for a password, as its application says.
    pub(crate) fn mcp_secret_field(&self, session: u64, id: WindowId) -> bool {
        let (Some(seat), Some(surface)) = (
            self.mcp_seat(session).map(|s| &s.seat),
            self.windows
                .get(&id)
                .and_then(|m| m.window.wl_surface().map(|s| s.into_owned())),
        ) else {
            return false;
        };
        self.emoji.inputs.secret_for(seat, &surface)
    }

    /// Presses `chords` (each modifiers first) into the window the keyboard
    /// of `session` is in.
    pub(crate) fn mcp_press(&mut self, session: u64, chords: &[Vec<u32>]) {
        let Some(keyboard) = self.mcp_seat(session).and_then(|s| s.seat.get_keyboard()) else {
            return;
        };
        let time = self.mcp_time();
        for chord in chords {
            let presses = chord
                .iter()
                .map(|c| (*c, KeyState::Pressed))
                .chain(chord.iter().rev().map(|c| (*c, KeyState::Released)));
            for (code, state) in presses {
                keyboard.input::<(), _>(
                    self,
                    Keycode::new(code + XKB_OFFSET),
                    state,
                    SERIAL_COUNTER.next_serial(),
                    time,
                    |_, _, _| FilterResult::Forward,
                );
            }
        }
    }

    /// The chords that type `text` on the layout in effect; or the
    /// character no key types. Every key is looked up before any is
    /// pressed: nothing half-typed.
    pub(crate) fn mcp_text_chords(&mut self, text: &str) -> Result<Vec<Vec<u32>>, String> {
        text.chars()
            .map(|c| {
                let stroke = self.mcp_stroke(char_keysym(c)).ok_or_else(|| {
                    format!("no key of the keyboard layout in effect types {c:?}")
                })?;
                let mut chord = stroke.modifiers().to_vec();
                chord.push(stroke.code);
                Ok(chord)
            })
            .collect()
    }

    /// The keys of a combination as the shortcuts write it (`Ctrl+A`,
    /// `Shift+Tab`, `Return`), modifiers first.
    pub(crate) fn mcp_chord(&mut self, keys: &str) -> Result<Vec<u32>, String> {
        let chord: crate::shortcuts::Chord = keys.parse().map_err(|e| format!("{e}"))?;
        let mut codes: Vec<u32> = [
            (chord.logo, KEY_LEFTMETA),
            (chord.ctrl, KEY_LEFTCTRL),
            (chord.alt, KEY_LEFTALT),
            (chord.shift, KEY_LEFTSHIFT),
        ]
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, code)| *code)
        .collect();
        let raw = chord.key.raw();
        let code = if (u32::from('0')..=u32::from('9')).contains(&raw) {
            // By position, as the shortcuts read them: KEY_1 is 2, KEY_0 11.
            match raw - u32::from('0') {
                0 => 11,
                d => d + 1,
            }
        } else {
            let stroke = self.mcp_stroke(chord.key).ok_or_else(|| {
                format!("{keys:?}: no key of the keyboard layout in effect gives it")
            })?;
            for code in stroke.modifiers() {
                if !codes.contains(code) {
                    codes.push(*code);
                }
            }
            stroke.code
        };
        codes.push(code);
        Ok(codes)
    }

    /// The key and level of the active layout that give `keysym`, the
    /// lowest level first, in a keymap of the user's layouts compiled once
    /// for them.
    fn mcp_stroke(&mut self, keysym: Keysym) -> Option<Stroke> {
        let names = self.config.xkb_names();
        let made_of = format!(
            "{}|{}|{}|{}",
            names.model, names.layout, names.variant, names.options
        );
        if self.mcp.seats.keymap.as_ref().map(|(k, _)| k) != Some(&made_of) {
            let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
            let keymap = xkb::Keymap::new_from_names(
                &context,
                "",
                &names.model,
                &names.layout,
                &names.variant,
                (!names.options.is_empty()).then(|| names.options.clone()),
                xkb::KEYMAP_COMPILE_NO_FLAGS,
            )?;
            self.mcp.seats.keymap = Some((made_of, keymap));
        }
        let layout = self
            .seat
            .get_keyboard()
            .map_or(0, |k| k.modifier_state().serialized.layout_effective);
        let (_, keymap) = self.mcp.seats.keymap.as_ref()?;
        let (min, max) = (keymap.min_keycode().raw(), keymap.max_keycode().raw());
        let layout = if layout < keymap.num_layouts() {
            layout
        } else {
            0
        };
        (0..4).find_map(|level| {
            (min.max(XKB_OFFSET)..=max).find_map(|raw| {
                let code = Keycode::new(raw);
                (level < keymap.num_levels_for_key(code, layout)
                    && keymap
                        .key_get_syms_by_level(code, layout, level)
                        .contains(&keysym))
                .then_some(Stroke {
                    code: raw - XKB_OFFSET,
                    level,
                })
            })
        })
    }

    /// `session` acted on window `id` now: its pointer shows, and the
    /// window is driven if no screen shows it.
    pub(crate) fn mcp_acted(&mut self, session: u64, id: WindowId) {
        let now = Instant::now();
        if let Some(seat) = self.mcp_seat_mut(session) {
            seat.last = Some(now);
            if seat.at.is_none_or(|(_, on)| on != id) {
                seat.at = None;
            }
        }
        self.mcp_drive(id);
    }

    /// Window `id` is driven for a while, if no screen shows it.
    pub(crate) fn mcp_drive(&mut self, id: WindowId) {
        self.mcp.driven.retain(|(w, _)| *w != id);
        self.mcp.driven.push((id, Instant::now()));
        if self.mcp.seats.unseen.is_none() && !self.model.is_shown(id) {
            self.mcp_unseen_frames();
        }
    }

    /// The windows driven that no screen shows get a frame callback; again
    /// in [`UNSEEN_FRAME`] while there are some.
    fn mcp_unseen_frames(&mut self) {
        self.mcp.driven.retain(|(_, at)| at.elapsed() < DRIVEN_FOR);
        let driven: Vec<WindowId> = self
            .mcp
            .driven
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| self.windows.contains_key(id) && !self.model.is_shown(*id))
            .collect();
        if driven.is_empty() {
            self.mcp.seats.unseen = None;
            return;
        }
        let now: Duration = self.clock.now().into();
        for id in driven {
            let (Some(window), Some(output)) = (
                self.windows.get(&id).map(|m| m.window.clone()),
                self.output_of(id),
            ) else {
                continue;
            };
            window.send_frame(&output, now, Some(Duration::ZERO), |_, _| {
                Some(output.clone())
            });
        }
        let timer =
            self.loop_handle
                .insert_source(Timer::from_duration(UNSEEN_FRAME), |_, _, app| {
                    app.state.mcp_unseen_frames();
                    TimeoutAction::Drop
                });
        self.mcp.seats.unseen = match timer {
            Ok(token) => Some(token),
            Err(err) => {
                tracing::warn!(error = %err.error, "an unseen window is not driven");
                None
            }
        };
    }
}
