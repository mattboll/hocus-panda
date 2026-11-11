//! The smaller protocols applications expect: activation, fractional
//! scale, presentation time, viewports, cursor shapes, touchpad gestures,
//! relative pointer motion and keyboard shortcuts inhibited.

use smithay::{
    delegate_cursor_shape, delegate_fractional_scale, delegate_keyboard_shortcuts_inhibit,
    delegate_pointer_gestures, delegate_presentation, delegate_relative_pointer,
    delegate_viewporter, delegate_xdg_activation,
    input::Seat,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
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

use crate::State;

/// How long a token is kept when no window uses it, and how long it lets
/// a window take the focus.
const TOKEN_KEPT: std::time::Duration = std::time::Duration::from_secs(60);
const TOKEN_FOCUSES: std::time::Duration = std::time::Duration::from_secs(10);

impl XdgActivationHandler for State {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.protocols.activation_state
    }

    /// A client asks for a token: it may focus a window if the client had
    /// the keyboard when the user pressed what asked for it.
    fn token_created(&mut self, token: XdgActivationToken, data: XdgActivationTokenData) -> bool {
        let valid = data.serial.as_ref().is_some_and(|(serial, seat)| {
            Seat::<State>::from_resource(seat)
                .and_then(|seat| seat.get_keyboard())
                .and_then(|k| k.last_enter())
                .is_some_and(|last| serial.is_no_older_than(&last))
        });
        self.keep_token(token.to_string(), valid);
        true
    }

    /// A window asks for the focus: with a token from the user's input (or
    /// one the desktop gave a program it started, or a tray click's), just
    /// after a click on its application's tray icon (5.14b), or as the
    /// dialog of the window with the focus or of the shell (the desktop
    /// portal's, whose token comes from no input of its own: #595), it
    /// comes forward; otherwise its rim pulses (§4).
    fn request_activation(
        &mut self,
        token: XdgActivationToken,
        _token_data: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        let now: std::time::Duration = self.clock.now().into();
        let by_token = self
            .activation_tokens
            .remove(token.as_str())
            .is_some_and(|(made, valid)| valid && now.saturating_sub(made) < TOKEN_FOCUSES);
        self.protocols.activation_state.remove_token(&token);
        let allowed = if by_token {
            self.tray_token_used(token.as_str());
            true
        } else if self.dialog_focuses(&surface) {
            true
        } else {
            let pid = self.client_pid(&surface);
            self.tray_click_focuses(pid)
        };
        let window = self.window_for(&surface).and_then(|w| self.id_of(&w));
        tracing::debug!(allowed, ?window, "activation asked");
        match window {
            Some(id) => self.activate(id, allowed),
            // Asked before its first frame: it opens focused.
            None if allowed && self.window_for(&surface).is_some() => {
                self.activate_on_map.push(surface);
            }
            None => {}
        }
    }
}

impl State {
    fn keep_token(&mut self, token: String, valid: bool) {
        let now = self.clock.now().into();
        self.activation_tokens
            .retain(|_, (made, _): &mut (std::time::Duration, bool)| now - *made < TOKEN_KEPT);
        self.activation_tokens.insert(token, (now, valid));
        let kept = &self.activation_tokens;
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
