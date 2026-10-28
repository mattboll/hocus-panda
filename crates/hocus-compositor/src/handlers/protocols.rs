//! The smaller protocols applications expect: activation, fractional
//! scale, presentation time, viewports, cursor shapes, touchpad gestures
//! and relative pointer motion.

use smithay::{
    delegate_cursor_shape, delegate_fractional_scale, delegate_pointer_gestures,
    delegate_presentation, delegate_relative_pointer, delegate_viewporter, delegate_xdg_activation,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    wayland::{
        compositor::with_states,
        fractional_scale::{FractionalScaleHandler, with_fractional_scale},
        tablet_manager::TabletSeatHandler,
        xdg_activation::{
            XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData,
        },
    },
};

use crate::State;

/// How long a token is kept when no window uses it.
const TOKEN_KEPT: std::time::Duration = std::time::Duration::from_secs(60);

impl XdgActivationHandler for State {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.activation_state
    }

    /// Notes when a token was made: whether a window may take the focus
    /// with it (the user's last key or click, how recent) is task 2.4's.
    fn token_created(&mut self, token: XdgActivationToken, _data: XdgActivationTokenData) -> bool {
        let now = self.clock.now().into();
        self.activation_tokens
            .retain(|_, made: &mut std::time::Duration| now - *made < TOKEN_KEPT);
        self.activation_tokens.insert(token.to_string(), now);
        let kept = &self.activation_tokens;
        self.activation_state
            .retain_tokens(|t, _| kept.contains_key(t.as_str()));
        true
    }

    /// Recorded, not acted on yet: the focus policy is task 2.4's.
    fn request_activation(
        &mut self,
        token: XdgActivationToken,
        _token_data: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        let known = self.activation_tokens.remove(token.as_str()).is_some();
        let window = self.window_for(&surface).and_then(|w| self.id_of(&w));
        tracing::debug!(known, ?window, "activation asked");
        self.activation_state.remove_token(&token);
    }
}
delegate_xdg_activation!(State);

impl FractionalScaleHandler for State {
    /// The scale of the output the window is on, or under the pointer.
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        let scale = self
            .window_for(&surface)
            .and_then(|w| self.space.outputs_for_element(&w).into_iter().next())
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
