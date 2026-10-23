//! What MCP agents do (#42). A control connection names itself an agent of
//! `[[agents.mcp]]`, then clicks, types and scrolls aimed at a window or at
//! an element of the shell. Its rights are read from the configuration in
//! effect at each action and checked here, so that an agent's own client
//! cannot widen them, and taking them away applies at once.
//!
//! An agent acts through a seat of its own (#43, [`crate::seat`]); a test
//! connection's input still goes through the session's pointer and
//! keyboard, the window brought forward first, as the user's would.

use hocus_api::{Access, ActTarget, Action, AgentInfo, ApiError, ConnectionId, ErrorCode, Reply};
use hocus_core::{
    Combo, Key, WindowId,
    config::{AgentAccess, McpAgent},
};
use smithay::{
    backend::input::{ButtonState, KeyState},
    desktop::Window,
    input::keyboard::{FilterResult, KeyboardHandle, Keycode, Keysym, xkb},
    utils::{Logical, Point, SERIAL_COUNTER},
};

use crate::{State, handlers::layer_shell::ABOVE};

/// Evdev codes are offset by 8 in XKB.
const XKB_OFFSET: u32 = 8;
/// Evdev codes of the modifiers.
const KEY_LEFTCTRL: u32 = 29;
const KEY_LEFTSHIFT: u32 = 42;
const KEY_LEFTALT: u32 = 56;
const KEY_RIGHTALT: u32 = 100;
const KEY_LEFTMETA: u32 = 125;
/// Longest text typed at once: an agent writes in turns, and the loop
/// carries a whole request out before anything else.
const MAX_TEXT: usize = 4096;
/// A triple click selects a paragraph; more is a mistake.
const MAX_CLICKS: u32 = 3;

fn denied(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::PermissionDenied, message)
}

fn info(agent: &McpAgent) -> AgentInfo {
    AgentInfo {
        name: agent.name.clone(),
        access: match agent.access {
            AgentAccess::Read => Access::Read,
            AgentAccess::Act => Access::Act,
        },
        apps: agent.apps.clone(),
        shell: agent.shell,
    }
}

/// The keys that type a character: its key, and the level it is on (0
/// alone, 1 with Shift, 2 with AltGr, 3 with both).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stroke {
    code: u32,
    level: u32,
}

impl Stroke {
    /// The modifiers to hold for its level, in the order they are pressed.
    fn modifiers(self) -> &'static [u32] {
        match self.level {
            0 => &[],
            1 => &[KEY_LEFTSHIFT],
            2 => &[KEY_RIGHTALT],
            _ => &[KEY_LEFTSHIFT, KEY_RIGHTALT],
        }
    }
}

impl State {
    /// Connection `connection` is agent `name` from now on, if the
    /// configuration names it.
    pub fn name_agent(
        &mut self,
        connection: ConnectionId,
        name: String,
    ) -> Result<Reply, ApiError> {
        if let Some(already) = self.api.agent(connection) {
            return Err(ApiError::new(
                ErrorCode::BadRequest,
                format!("this connection is already agent {already:?}"),
            ));
        }
        let Some(agent) = self.config.agents.mcp_agent(&name) else {
            return Err(denied(format!(
                "no agent named {name:?} in [[agents.mcp]] of the configuration"
            )));
        };
        let info = info(agent);
        tracing::info!(agent = %info.name, access = ?info.access, "an MCP agent connected");
        self.seat_join(connection, &name);
        self.api.set_agent(connection, name);
        self.journal_met();
        Ok(Reply::Agent(info))
    }

    /// Carries out an agent's action through its own seat, kept in the
    /// journal (#176), or a test connection's (which may do anything: the
    /// server let it through with the test permission). `None` while an
    /// agent's waits: shown before a click, or for the user's confirmation.
    pub fn act(
        &mut self,
        connection: ConnectionId,
        id: u64,
        target: ActTarget,
        action: Action,
        intent: Option<String>,
    ) -> Option<Result<Reply, ApiError>> {
        if matches!(target, ActTarget::AppElement { .. }) {
            return self.act_on_element(connection, id, target, action, intent);
        }
        match self.api.agent(connection).map(str::to_owned) {
            Some(name) => self.agent_act(connection, id, &name, target, action, intent),
            None => Some(self.carry_out(target, action)),
        }
    }

    /// A test connection's action, through the session's own pointer and
    /// keyboard: the window comes forward first, as the user would bring it.
    pub(crate) fn carry_out(
        &mut self,
        target: ActTarget,
        action: Action,
    ) -> Result<Reply, ApiError> {
        if self.locked() {
            return Err(denied("the session is locked: agents do not act on it"));
        }
        if self.auth_open() {
            return Err(denied(
                "a password dialog is open: agents do not act until it is answered",
            ));
        }
        let typing = matches!(action, Action::Type { .. } | Action::Keys { .. });
        let at = match target {
            ActTarget::Window { window, at } => {
                let window = self.forward_window(window)?;
                if typing {
                    return self.type_into(&window, &action).map(|()| Reply::Done);
                }
                self.point_in(&window, at)?
            }
            ActTarget::Element { label } => {
                if typing {
                    return Err(ApiError::new(
                        ErrorCode::BadRequest,
                        "typing goes to a window: aim at one",
                    ));
                }
                self.element_centre(&label)?
            }
            ActTarget::AppElement { .. } => {
                return Err(ApiError::new(
                    ErrorCode::BadRequest,
                    "an application's element is found before it is aimed at",
                ));
            }
        };
        let time = self.time_msec();
        self.pointer_motion(at, time);
        match action {
            Action::Click { button, count } => {
                let code = button.code();
                for _ in 0..count.clamp(1, MAX_CLICKS) {
                    self.pointer_button(code, ButtonState::Pressed, time);
                    self.pointer_button(code, ButtonState::Released, time);
                }
            }
            Action::Scroll { dy } => self.scroll(dy, time),
            Action::Type { .. } | Action::Keys { .. } => {}
        }
        Ok(Reply::Done)
    }

    /// The window `id`, brought forward on whatever desktop it is,
    /// minimized or not, as activating it would; it gets the keyboard.
    fn forward_window(&mut self, id: u64) -> Result<Window, ApiError> {
        let window = self
            .windows
            .get(&WindowId(id))
            .map(|m| m.window.clone())
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("no window with id {id}")))?;
        self.restore(WindowId(id), SERIAL_COUNTER.next_serial());
        Ok(window)
    }

    /// A point of `window` relative to its top-left corner (its centre when
    /// `None`), in the global space, if a click there reaches it.
    fn point_in(
        &self,
        window: &Window,
        at: Option<hocus_api::Point>,
    ) -> Result<Point<f64, Logical>, ApiError> {
        let geometry = self.space.element_geometry(window).ok_or_else(|| {
            ApiError::new(ErrorCode::Failed, "the window is not shown on any screen")
        })?;
        let (w, h) = (f64::from(geometry.size.w), f64::from(geometry.size.h));
        let (x, y) = at.map_or((w / 2.0, h / 2.0), |p| (p.x, p.y));
        if !(0.0..w).contains(&x) || !(0.0..h).contains(&y) {
            return Err(ApiError::new(
                ErrorCode::BadRequest,
                format!("{x},{y} is outside the window, which is {w}×{h}"),
            ));
        }
        let global = geometry.loc.to_f64() + Point::from((x, y));
        // What the click would reach instead: a bar or overlay, the shell,
        // a window above.
        let fullscreen = self
            .space
            .output_under(global)
            .next()
            .is_some_and(|o| self.fullscreen_on(o));
        let above = if fullscreen { &ABOVE[..1] } else { &ABOVE[..] };
        if self.layer_surface_under(global, above).is_some() {
            return Err(covered("a bar or an overlay of another program"));
        }
        if let Some(element) = self
            .shell_ui()
            .iter()
            .rev()
            .find(|n| n.rect.contains(global.x, global.y))
        {
            let what = element.label.as_deref().unwrap_or(&element.role);
            return Err(covered(&format!("the shell's {what:?}")));
        }
        match self.space.element_under(global) {
            Some((under, _)) if under == window => Ok(global),
            _ => Err(covered("another window")),
        }
    }

    /// The centre of the shell's element labelled `label`; never an
    /// agent's own card.
    pub(crate) fn element_centre(&self, label: &str) -> Result<Point<f64, Logical>, ApiError> {
        let mut seats = Vec::new();
        self.seat_ui(&mut seats);
        let node = self
            .shell_ui()
            .into_iter()
            .filter(|n| !seats.contains(n))
            .find(|n| n.label.as_deref() == Some(label))
            .ok_or_else(|| {
                ApiError::new(
                    ErrorCode::NotFound,
                    format!("the shell shows no element labelled {label:?}"),
                )
            })?;
        if node.disabled {
            return Err(ApiError::new(
                ErrorCode::Failed,
                format!("{label:?} is disabled"),
            ));
        }
        let r = node.rect;
        Ok(Point::from((
            f64::from(r.x) + f64::from(r.width) / 2.0,
            f64::from(r.y) + f64::from(r.height) / 2.0,
        )))
    }

    /// Types text or presses keys in `window`, which has just been given
    /// the keyboard. The keys go to it alone: never a shortcut of the
    /// compositor's, whatever they are.
    fn type_into(&mut self, window: &Window, action: &Action) -> Result<(), ApiError> {
        if self.focused_window().as_ref() != Some(window) {
            return Err(ApiError::new(
                ErrorCode::Failed,
                "the window did not get the keyboard",
            ));
        }
        let Some(keyboard) = self.seat.get_keyboard() else {
            return Err(ApiError::new(ErrorCode::Failed, "no keyboard"));
        };
        // Every key is looked up before any is pressed: nothing half-typed.
        let chords: Vec<Vec<u32>> = match action {
            Action::Type { text } => self.text_chords(&keyboard, text)?,
            Action::Keys { keys } => vec![self.chord_on(&keyboard, keys)?],
            Action::Click { .. } | Action::Scroll { .. } => Vec::new(),
        };
        for chord in chords {
            for code in &chord {
                self.forward_key(*code, KeyState::Pressed);
            }
            for code in chord.iter().rev() {
                self.forward_key(*code, KeyState::Released);
            }
        }
        Ok(())
    }

    /// The chords that type `text` on `keyboard`'s layout, modifiers first.
    pub(crate) fn text_chords(
        &mut self,
        keyboard: &KeyboardHandle<State>,
        text: &str,
    ) -> Result<Vec<Vec<u32>>, ApiError> {
        if text.chars().count() > MAX_TEXT {
            return Err(ApiError::new(
                ErrorCode::BadRequest,
                format!("at most {MAX_TEXT} characters at once"),
            ));
        }
        text.chars()
            .map(|c| {
                let stroke = self.stroke_for(keyboard, char_keysym(c)).ok_or_else(|| {
                    ApiError::new(
                        ErrorCode::BadRequest,
                        format!("no key of the keyboard layout in effect types {c:?}"),
                    )
                })?;
                let mut chord = stroke.modifiers().to_vec();
                chord.push(stroke.code);
                Ok(chord)
            })
            .collect()
    }

    /// The keys of a combination as the shortcuts write it (`Ctrl+A`,
    /// `Shift+Tab`, `Return`), modifiers first, on `keyboard`'s layout.
    pub(crate) fn chord_on(
        &mut self,
        keyboard: &KeyboardHandle<State>,
        keys: &str,
    ) -> Result<Vec<u32>, ApiError> {
        let combo: Combo = keys
            .parse()
            .map_err(|err: String| ApiError::new(ErrorCode::BadRequest, err))?;
        let m = combo.modifiers;
        let mut chord: Vec<u32> = [
            (m.logo, KEY_LEFTMETA),
            (m.ctrl, KEY_LEFTCTRL),
            (m.alt, KEY_LEFTALT),
            (m.shift, KEY_LEFTSHIFT),
        ]
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, code)| *code)
        .collect();
        let code = match &combo.key {
            // By position, as the shortcuts read them: KEY_1 is 2, KEY_0 11.
            Key::Digit(0) => 11,
            Key::Digit(d) => u32::from(*d) + 1,
            Key::Named(name) => {
                let keysym = xkb::keysym_from_name(name, xkb::KEYSYM_CASE_INSENSITIVE);
                if keysym.raw() == xkb::keysyms::KEY_NoSymbol {
                    return Err(ApiError::new(
                        ErrorCode::BadRequest,
                        format!("{keys:?}: no key is named {name:?}"),
                    ));
                }
                let stroke = self.stroke_for(keyboard, keysym).ok_or_else(|| {
                    ApiError::new(
                        ErrorCode::BadRequest,
                        format!("{keys:?}: no key of the keyboard layout in effect is {name:?}"),
                    )
                })?;
                for code in stroke.modifiers() {
                    if !chord.contains(code) {
                        chord.push(*code);
                    }
                }
                stroke.code
            }
        };
        chord.push(code);
        Ok(chord)
    }

    /// The key and level of the active layout that give `keysym`, the
    /// lowest level first.
    fn stroke_for(&mut self, keyboard: &KeyboardHandle<State>, keysym: Keysym) -> Option<Stroke> {
        keyboard.with_xkb_state(self, |context| {
            let xkb = context.xkb().lock().ok()?;
            let layout = xkb.active_layout().0;
            // SAFETY: the keymap is only read here, under the lock; no
            // reference to it is kept.
            #[allow(unsafe_code)]
            let keymap = unsafe { xkb.keymap() };
            let (min, max) = (keymap.min_keycode().raw(), keymap.max_keycode().raw());
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
        })
    }

    /// A key straight to the window with the keyboard, past the
    /// compositor's shortcuts.
    fn forward_key(&mut self, code: u32, state: KeyState) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let time = self.time_msec();
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

fn covered(by: &str) -> ApiError {
    ApiError::new(
        ErrorCode::Failed,
        format!("{by} is over that point of the window: the click would not reach it"),
    )
}

/// The keysym a character types; line breaks and tabs are keys.
fn char_keysym(c: char) -> Keysym {
    match c {
        '\n' | '\r' => Keysym::Return,
        '\t' => Keysym::Tab,
        c => xkb::utf32_to_keysym(u32::from(c)),
    }
}
