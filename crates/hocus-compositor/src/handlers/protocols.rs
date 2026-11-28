//! The smaller protocols applications expect: activation, fractional
//! scale, presentation time, viewports, cursor shapes, touchpad gestures,
//! relative pointer motion and keyboard shortcuts inhibited.

use smithay::{
    delegate_cursor_shape, delegate_fractional_scale, delegate_keyboard_shortcuts_inhibit,
    delegate_pointer_gestures, delegate_presentation, delegate_relative_pointer,
    delegate_viewporter, delegate_xdg_activation,
    input::Seat,
    reexports::wayland_server::protocol::{wl_seat::WlSeat, wl_surface::WlSurface},
    utils::Serial,
    wayland::{
        compositor::with_states,
        fractional_scale::{FractionalScaleHandler, with_fractional_scale},
        keyboard_shortcuts_inhibit::{
            KeyboardShortcutsInhibitHandler, KeyboardShortcutsInhibitState,
            KeyboardShortcutsInhibitor, KeyboardShortcutsInhibitorSeat,
        },
        seat::WaylandFocus,
        tablet_manager::TabletSeatHandler,
        xdg_activation::{
            XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData,
        },
    },
};

use crate::{State, windows::Activated};

/// How long a token is kept when no window uses it, and how long it lets
/// a window take the focus.
const TOKEN_KEPT: std::time::Duration = std::time::Duration::from_secs(60);
const TOKEN_FOCUSES: std::time::Duration = std::time::Duration::from_secs(10);

impl XdgActivationHandler for State {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.protocols.activation_state
    }

    /// A client asks for a token: it may focus a window if the client had
    /// the keyboard when the user pressed what asked for it. Made from the
    /// user's click or key, it is for something the user opens (a link, a
    /// file): the pointer is busy until that shows (#966).
    fn token_created(&mut self, token: XdgActivationToken, data: XdgActivationTokenData) -> bool {
        let valid = data
            .serial
            .as_ref()
            .is_some_and(|(serial, seat)| self.serial_of_the_users_input(*serial, seat));
        tracing::debug!(
            valid,
            from_input = data.serial.is_some(),
            app = data.app_id.as_deref().unwrap_or_default(),
            "an activation token made"
        );
        self.keep_token(token.to_string(), valid);
        if valid {
            self.opening("an application's token");
        }
        true
    }

    /// A window asks for the focus: with a token from the user's input (or
    /// one the desktop gave a program it started, or a tray click's), just
    /// after a click on its application's tray icon (5.14b), or as the
    /// dialog of the window with the focus or of the shell (the desktop
    /// portal's, whose token comes from no input of its own: #595), it
    /// comes forward, from another desktop or screen too (#966);
    /// otherwise its rim pulses (§4). Each ask is said in the session's
    /// log: who, which window, the token, what was done.
    fn request_activation(
        &mut self,
        token: XdgActivationToken,
        _token_data: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        let now: std::time::Duration = self.clock.now().into();
        let said = match self.arrivals.activation_tokens.remove(token.as_str()) {
            Some((_, false)) => TokenSaid::NotFromInput,
            Some((made, true)) if now.saturating_sub(made) >= TOKEN_FOCUSES => TokenSaid::Stale,
            Some(_) => TokenSaid::Valid,
            None => TokenSaid::Unknown,
        };
        self.protocols.activation_state.remove_token(&token);
        let pid = self.client_pid(&surface);
        let allowed = if said == TokenSaid::Valid {
            self.tray_token_used(token.as_str());
            Some("token")
        } else if self.dialog_focuses(&surface) {
            Some("dialog")
        } else if self.tray_click_focuses(pid) {
            Some("tray click")
        } else {
            None
        };
        let mapped = self.window_for(&surface);
        let window = mapped.as_ref().and_then(|w| self.id_of(w));
        let app = mapped.as_ref().and_then(crate::windows::app_id);
        let done = match window {
            Some(id) => match self.activate(id, allowed.is_some()) {
                Activated::Forward { elsewhere: true } => "came forward, its desktop shown",
                Activated::Forward { elsewhere: false } => "came forward",
                Activated::Pulsed => "pulsed",
                Activated::HadTheFocus => "had the focus",
            },
            // Asked before its first frame: it opens focused.
            None if allowed.is_some() && mapped.is_some() => {
                self.arrivals.activate_on_map.push(surface);
                "focused when it maps"
            }
            None if mapped.is_some() => "opens behind",
            None => "refused: no window",
        };
        tracing::info!(
            app = app.as_deref().unwrap_or_default(),
            ?pid,
            window = window.map(|w| w.0),
            token = said.says(),
            by = allowed.unwrap_or("nothing"),
            done,
            "activation asked"
        );
    }
}

/// What a token asking for the focus was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenSaid {
    /// Made from the user's input, or by the desktop, in the last 10 s.
    Valid,
    /// Made from the user's input or by the desktop, but too long ago.
    Stale,
    /// Made from no input of the user's.
    NotFromInput,
    /// Never made, or used already.
    Unknown,
}

impl TokenSaid {
    fn says(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Stale => "stale",
            Self::NotFromInput => "not from input",
            Self::Unknown => "unknown",
        }
    }
}

impl State {
    /// Whether `serial` is of the user's input to the client with the
    /// keyboard: no older than the keyboard's entering it, or the press
    /// that gave it the keyboard, which comes just before (a link clicked
    /// in a window without the focus).
    fn serial_of_the_users_input(&self, serial: Serial, seat: &WlSeat) -> bool {
        let Some(keyboard) = Seat::<State>::from_resource(seat).and_then(|s| s.get_keyboard())
        else {
            return false;
        };
        let Some(entered) = keyboard.last_enter() else {
            return false;
        };
        let since = match self.pointer.focusing_press {
            Some((press, enter)) if enter == entered => press,
            _ => entered,
        };
        serial.is_no_older_than(&since)
    }

    fn keep_token(&mut self, token: String, valid: bool) {
        let now = self.clock.now().into();
        self.arrivals
            .activation_tokens
            .retain(|_, (made, _): &mut (std::time::Duration, bool)| now - *made < TOKEN_KEPT);
        self.arrivals.activation_tokens.insert(token, (now, valid));
        let kept = &self.arrivals.activation_tokens;
        self.protocols
            .activation_state
            .retain_tokens(|t, _| kept.contains_key(t.as_str()));
    }

    /// A token for a program the user starts through the desktop: its
    /// first window takes the focus with it.
    pub fn activation_token(&mut self) -> String {
        let (token, _) = self.protocols.activation_state.create_external_token(None);
        let token = token.to_string();
        self.keep_token(token.clone(), true);
        token
    }
}
delegate_xdg_activation!(State);

impl FractionalScaleHandler for State {
    /// The largest scale of the outputs the window is on (§12, #419), or
    /// the one under the pointer.
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        let scale = self
            .window_for(&surface)
            .and_then(|w| self.drawing_output(&w))
            .or_else(|| self.output_under_pointer())
            .map_or(1.0, |o| o.current_scale().fractional_scale());
        with_states(&surface, |states| {
            with_fractional_scale(states, |fs| fs.set_preferred_scale(scale));
        });
    }
}
delegate_fractional_scale!(State);

impl TabletSeatHandler for State {}

delegate_cursor_shape!(State);
delegate_viewporter!(State);
delegate_presentation!(State);
delegate_pointer_gestures!(State);
delegate_relative_pointer!(State);

/// A client asking for every key (the Settings while they record a
/// shortcut, 6.8) gets them at once: the desktop's shortcuts wait while
/// its surface has the keyboard, and come back when it lets go.
impl KeyboardShortcutsInhibitHandler for State {
    fn keyboard_shortcuts_inhibit_state(&mut self) -> &mut KeyboardShortcutsInhibitState {
        &mut self.protocols.shortcuts_inhibit_state
    }

    fn new_inhibitor(&mut self, inhibitor: KeyboardShortcutsInhibitor) {
        tracing::info!("a client takes the desktop's shortcuts");
        inhibitor.activate();
    }

    fn inhibitor_destroyed(&mut self, _: KeyboardShortcutsInhibitor) {
        tracing::info!("the desktop's shortcuts are back");
    }
}

delegate_keyboard_shortcuts_inhibit!(State);

impl State {
    /// The surface holding the keyboard has its shortcuts' inhibitor on.
    pub fn shortcuts_inhibited(&self) -> bool {
        let Some(focus) = self.seat.get_keyboard().and_then(|k| k.current_focus()) else {
            return false;
        };
        let Some(surface) = focus.wl_surface() else {
            return false;
        };
        self.seat
            .keyboard_shortcuts_inhibitor_for_surface(&surface)
            .is_some_and(|i| i.is_active())
    }
}
