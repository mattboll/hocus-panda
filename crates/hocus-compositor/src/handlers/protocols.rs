//! The common batch of protocols applications and tools expect (#32):
//! activation, idle inhibitors, cursor shapes, shortcut inhibitors,
//! foreign handles, presentation time, fractional scale, viewports,
//! single-pixel buffers, alpha, content type, pointer constraints and
//! relative motion, gestures, the virtual keyboard, the primary selection
//! and clipboard managers' data control.
//!
//! Clients started through a security context (Flatpak and other
//! sandboxes) do not see the protocols that read or drive the rest of the
//! session: the virtual keyboard, data control, the list of windows, and
//! security contexts themselves.

use std::{collections::HashSet, sync::Arc, time::Duration};

use smithay::reexports::wayland_protocols::wp::text_input::zv3::server::zwp_text_input_v3;
use smithay::{
    delegate_alpha_modifier, delegate_content_type, delegate_cursor_shape,
    delegate_ext_data_control, delegate_fractional_scale, delegate_idle_inhibit,
    delegate_keyboard_shortcuts_inhibit, delegate_pointer_constraints, delegate_pointer_gestures,
    delegate_presentation, delegate_primary_selection, delegate_relative_pointer,
    delegate_security_context, delegate_single_pixel_buffer, delegate_viewporter,
    delegate_virtual_keyboard_manager, delegate_xdg_activation, delegate_xdg_foreign,
    input::pointer::PointerHandle,
    reexports::wayland_server::{Client, DisplayHandle, protocol::wl_surface::WlSurface},
    utils::{Logical, Point, SERIAL_COUNTER, Serial},
    wayland::{
        alpha_modifier::AlphaModifierState,
        compositor::with_states,
        content_type::ContentTypeState,
        cursor_shape::CursorShapeManagerState,
        fractional_scale::{
            FractionalScaleHandler, FractionalScaleManagerState, with_fractional_scale,
        },
        idle_inhibit::{IdleInhibitHandler, IdleInhibitManagerState},
        keyboard_shortcuts_inhibit::{
            KeyboardShortcutsInhibitHandler, KeyboardShortcutsInhibitState,
            KeyboardShortcutsInhibitor,
        },
        pointer_constraints::{
            PointerConstraintsHandler, PointerConstraintsState, with_pointer_constraint,
        },
        pointer_gestures::PointerGesturesState,
        presentation::PresentationState,
        relative_pointer::RelativePointerManagerState,
        security_context::{
            SecurityContext, SecurityContextHandler, SecurityContextListenerSource,
            SecurityContextState,
        },
        selection::{
            ext_data_control::{
                DataControlHandler as ExtDataControlHandler,
                DataControlState as ExtDataControlState,
            },
            primary_selection::{PrimarySelectionHandler, PrimarySelectionState},
        },
        single_pixel_buffer::SinglePixelBufferState,
        tablet_manager::TabletSeatHandler,
        viewporter::ViewporterState,
        virtual_keyboard::VirtualKeyboardManagerState,
        xdg_activation::{
            XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData,
        },
        xdg_foreign::{XdgForeignHandler, XdgForeignState},
    },
};

use crate::{State, state::ClientState};

/// How old an activation token may be and still bring a window forward.
const ACTIVATION_PATIENCE: Duration = Duration::from_secs(10);

/// An activation token handed out: when, on the compositor's clock, and
/// by whom.
#[derive(Debug, Clone, Copy)]
pub struct Issued {
    pub at: Duration,
    /// Made by the shell for what Alex started from it (the launcher, the
    /// dock, a notification, a shortcut): it always may take the focus.
    pub by_shell: bool,
}

/// Whether `client` runs outside any sandbox.
pub fn trusted(client: &Client) -> bool {
    client
        .get_data::<ClientState>()
        .is_none_or(|data| data.security_context.is_none())
}

/// The states of the batch.
#[derive(Debug)]
pub struct Protocols {
    pub activation: XdgActivationState,
    pub idle_inhibit: IdleInhibitManagerState,
    /// Surfaces that keep the session awake while shown.
    pub inhibiting: HashSet<WlSurface>,
    /// The activation tokens handed out, by name.
    pub tokens: std::collections::HashMap<String, Issued>,
    /// The serial of the user's last key or button press: a token made
    /// from an older event was not asked by what Alex did last (#145).
    pub last_press: Option<Serial>,
    pub shortcuts_inhibit: KeyboardShortcutsInhibitState,
    /// The surface with the keyboard, as last told: the keyboard cannot
    /// be asked from within its own key filter.
    pub keyboard_focus: Option<WlSurface>,
    pub foreign: XdgForeignState,
    pub presentation: PresentationState,
    /// text-input-v3: the history and the emoji picker type into the
    /// focused application.
    pub _text_input: smithay::wayland::text_input::TextInputManagerState,
    /// What each text input says its field is for: an agent typing into a
    /// password field waits for the user (#43).
    pub purposes: Purposes,
    pub fractional_scale: FractionalScaleManagerState,
    pub primary_selection: PrimarySelectionState,
    pub data_control: ExtDataControlState,
    pub security_context: SecurityContextState,
    pub layer_shell: smithay::wayland::shell::wlr_layer::WlrLayerShellState,
    /// What layer surfaces leave of each output, as last arranged.
    pub zones: std::collections::HashMap<String, smithay::utils::Rectangle<i32, Logical>>,
    /// A layer surface has the keyboard.
    pub layer_focus: bool,
    _cursor_shape: CursorShapeManagerState,
    _viewporter: ViewporterState,
    _single_pixel: SinglePixelBufferState,
    _alpha: AlphaModifierState,
    _content_type: ContentTypeState,
    _constraints: PointerConstraintsState,
    _relative: RelativePointerManagerState,
    _gestures: PointerGesturesState,
    _virtual_keyboard: VirtualKeyboardManagerState,
}

impl Protocols {
    pub fn new(dh: &DisplayHandle) -> Self {
        let primary_selection = PrimarySelectionState::new::<State>(dh);
        let data_control =
            ExtDataControlState::new::<State, _>(dh, Some(&primary_selection), trusted);
        Self {
            activation: XdgActivationState::new::<State>(dh),
            idle_inhibit: IdleInhibitManagerState::new::<State>(dh),
            inhibiting: HashSet::new(),
            tokens: Default::default(),
            last_press: None,
            shortcuts_inhibit: KeyboardShortcutsInhibitState::new::<State>(dh),
            keyboard_focus: None,
            foreign: XdgForeignState::new::<State>(dh),
            // CLOCK_MONOTONIC, as the frame times.
            presentation: PresentationState::new::<State>(dh, 1),
            _text_input: smithay::wayland::text_input::TextInputManagerState::new::<State>(dh),
            purposes: Purposes::default(),
            fractional_scale: FractionalScaleManagerState::new::<State>(dh),
            primary_selection,
            data_control,
            security_context: SecurityContextState::new::<State, _>(dh, trusted),
            layer_shell: smithay::wayland::shell::wlr_layer::WlrLayerShellState::new_with_filter::<
                State,
                _,
            >(dh, trusted),
            zones: Default::default(),
            layer_focus: false,
            _cursor_shape: CursorShapeManagerState::new::<State>(dh),
            _viewporter: ViewporterState::new::<State>(dh),
            _single_pixel: SinglePixelBufferState::new::<State>(dh),
            _alpha: AlphaModifierState::new::<State>(dh),
            _content_type: ContentTypeState::new::<State>(dh),
            _constraints: PointerConstraintsState::new::<State>(dh),
            _relative: RelativePointerManagerState::new::<State>(dh),
            _gestures: PointerGesturesState::new::<State>(dh),
            _virtual_keyboard: VirtualKeyboardManagerState::new::<State, _>(dh, trusted),
        }
    }
}

impl XdgActivationHandler for State {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.protocols.activation
    }

    /// Notes when a token was made, on the compositor's clock (tests move
    /// it).
    fn token_created(&mut self, token: XdgActivationToken, _data: XdgActivationTokenData) -> bool {
        self.issue_token(token.to_string(), false);
        true
    }

    /// A window may take the focus when Alex asked for it: with a token
    /// the shell made, or one made from their last key or click (or
    /// later), recent, while a screen shows its desktop. Otherwise it asks
    /// for attention, and Alex goes there when they choose: a command
    /// done in the background, a token without a serial, an old one, one
    /// for a desktop no screen shows (#145, #192, #218).
    fn request_activation(
        &mut self,
        token: XdgActivationToken,
        token_data: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        let issued = self.protocols.tokens.remove(&token.to_string());
        let age = issued.map_or_else(
            || token_data.timestamp.elapsed(),
            |i| self.clock.now().saturating_sub(i.at),
        );
        let by_shell = issued.is_some_and(|i| i.by_shell);
        // A token from an agent's click never takes the user's keyboard
        // (#43): the window asks for attention.
        let by_agent = token_data
            .serial
            .as_ref()
            .is_some_and(|(_, seat)| self.is_agent_wl_seat(seat));
        let last_press = self.protocols.last_press;
        let from_last_input = !by_agent
            && token_data.serial.as_ref().is_some_and(|(serial, _)| {
                last_press.is_none_or(|last| serial.is_no_older_than(&last))
            });
        if let Some(id) = self.managed_id(&surface) {
            let shown = self.model.output_showing(id).is_some();
            let asked = by_shell || from_last_input;
            if age < ACTIVATION_PATIENCE && shown && asked {
                self.restore(id, SERIAL_COUNTER.next_serial());
            } else {
                tracing::debug!(
                    window = id.0,
                    ?age,
                    shown,
                    by_shell,
                    from_last_input,
                    "activation refused"
                );
                self.ask_attention(id);
            }
        }
        self.protocols.activation.remove_token(&token);
    }
}

impl State {
    /// Notes a token handed out; those never used are forgotten after a
    /// minute.
    fn issue_token(&mut self, name: String, by_shell: bool) {
        let now = self.clock.now();
        let tokens = &mut self.protocols.tokens;
        tokens.retain(|_, i| now.saturating_sub(i.at) < 6 * ACTIVATION_PATIENCE);
        tokens.insert(name, Issued { at: now, by_shell });
        // Smithay keeps every token until it is used: those forgotten go.
        self.protocols
            .activation
            .retain_tokens(|t, _| tokens.contains_key(t.as_str()));
    }

    /// A token of the shell's, for a program or an application it starts
    /// or wakes on Alex's behalf (`XDG_ACTIVATION_TOKEN`): its window may
    /// take the focus.
    pub fn shell_token(&mut self) -> String {
        let (token, _) = self
            .protocols
            .activation
            .create_external_token(XdgActivationTokenData::default());
        let name = token.to_string();
        self.issue_token(name.clone(), true);
        name
    }

    /// A key or button pressed by the user: what tokens are measured by.
    pub fn note_press(&mut self, serial: Serial) {
        self.protocols.last_press = Some(serial);
    }
}
delegate_xdg_activation!(State);

impl IdleInhibitHandler for State {
    fn inhibit(&mut self, surface: WlSurface) {
        self.protocols.inhibiting.insert(surface);
    }

    fn uninhibit(&mut self, surface: WlSurface) {
        self.protocols.inhibiting.remove(&surface);
    }
}
delegate_idle_inhibit!(State);

impl KeyboardShortcutsInhibitHandler for State {
    fn keyboard_shortcuts_inhibit_state(&mut self) -> &mut KeyboardShortcutsInhibitState {
        &mut self.protocols.shortcuts_inhibit
    }

    /// Granted at once: the window asked (a remote desktop, a game, the
    /// settings recording a shortcut); it ends when its window loses the
    /// keyboard.
    fn new_inhibitor(&mut self, inhibitor: KeyboardShortcutsInhibitor) {
        inhibitor.activate();
    }
}
delegate_keyboard_shortcuts_inhibit!(State);

impl State {
    /// Whether the window with the keyboard holds the compositor's
    /// shortcuts back.
    pub fn shortcuts_inhibited(&self) -> bool {
        use smithay::wayland::keyboard_shortcuts_inhibit::KeyboardShortcutsInhibitorSeat;
        self.protocols
            .keyboard_focus
            .as_ref()
            .and_then(|s| self.seat.keyboard_shortcuts_inhibitor_for_surface(s))
            .is_some_and(|i| i.is_active())
    }
}

impl XdgForeignHandler for State {
    fn xdg_foreign_state(&mut self) -> &mut XdgForeignState {
        &mut self.protocols.foreign
    }
}
delegate_xdg_foreign!(State);

impl FractionalScaleHandler for State {
    /// The scale of the screen the window is on, or under the pointer.
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        let scale = self
            .window_for(&surface)
            .and_then(|w| self.output_of(&w).cloned())
            .or_else(|| self.output_under_pointer())
            .map_or(1.0, |o| o.current_scale().fractional_scale());
        with_states(&surface, |states| {
            with_fractional_scale(states, |fs| fs.set_preferred_scale(scale));
        });
    }
}
delegate_fractional_scale!(State);

impl PrimarySelectionHandler for State {
    fn primary_selection_state(&self) -> &PrimarySelectionState {
        &self.protocols.primary_selection
    }
}
delegate_primary_selection!(State);

impl ExtDataControlHandler for State {
    fn data_control_state(&self) -> &ExtDataControlState {
        &self.protocols.data_control
    }
}
delegate_ext_data_control!(State);

impl PointerConstraintsHandler for State {
    /// A constraint takes effect at once when its surface has the pointer.
    fn new_constraint(&mut self, surface: &WlSurface, pointer: &PointerHandle<Self>) {
        if pointer.current_focus().as_ref() == Some(surface) {
            with_pointer_constraint(surface, pointer, |constraint| {
                if let Some(c) = constraint {
                    c.activate();
                }
            });
        }
    }

    fn cursor_position_hint(
        &mut self,
        _surface: &WlSurface,
        _pointer: &PointerHandle<Self>,
        _location: Point<f64, Logical>,
    ) {
    }
}
delegate_pointer_constraints!(State);

impl SecurityContextHandler for State {
    /// Clients of the sandbox connect through its listening socket: they
    /// are marked, and the sensitive globals hide from them.
    fn context_created(&mut self, source: SecurityContextListenerSource, context: SecurityContext) {
        let Some(handle) = self.loop_handle.clone() else {
            return;
        };
        let inserted = handle.insert_source(source, move |stream, _, app| {
            let data = ClientState {
                security_context: Some(context.clone()),
                ..ClientState::default()
            };
            if let Err(err) = app
                .state
                .display_handle
                .insert_client(stream, Arc::new(data))
            {
                tracing::warn!(%err, "a sandboxed client could not connect");
            }
        });
        if let Err(err) = inserted {
            tracing::warn!(err = %err.error, "could not listen for a security context");
        }
    }
}
delegate_security_context!(State);

impl TabletSeatHandler for State {}

delegate_cursor_shape!(State);
delegate_viewporter!(State);
delegate_single_pixel_buffer!(State);
delegate_alpha_modifier!(State);
delegate_content_type!(State);
delegate_relative_pointer!(State);
delegate_pointer_gestures!(State);
delegate_virtual_keyboard_manager!(State);
delegate_presentation!(State);
smithay::reexports::wayland_server::delegate_global_dispatch!(State: [
    smithay::reexports::wayland_protocols::wp::text_input::zv3::server::zwp_text_input_manager_v3::ZwpTextInputManagerV3: ()
] => smithay::wayland::text_input::TextInputManagerState);
smithay::reexports::wayland_server::delegate_dispatch!(State: [
    smithay::reexports::wayland_protocols::wp::text_input::zv3::server::zwp_text_input_manager_v3::ZwpTextInputManagerV3: ()
] => smithay::wayland::text_input::TextInputManagerState);

/// What a text input's field is for, as its client last committed it.
#[derive(Debug, Default, Clone, Copy)]
struct Purpose {
    enabled: bool,
    password: bool,
    pending_enable: Option<bool>,
    pending_password: Option<bool>,
}

/// Text inputs and their fields' purposes, by object.
#[derive(Debug, Default)]
pub struct Purposes {
    inputs:
        std::collections::HashMap<smithay::reexports::wayland_server::backend::ObjectId, Purpose>,
}

impl Purposes {
    fn note(
        &mut self,
        id: smithay::reexports::wayland_server::backend::ObjectId,
        request: &zwp_text_input_v3::Request,
    ) {
        use zwp_text_input_v3::{ContentPurpose, Request};
        let p = self.inputs.entry(id).or_default();
        match request {
            Request::Enable => p.pending_enable = Some(true),
            Request::Disable => p.pending_enable = Some(false),
            Request::SetContentType { purpose, .. } => {
                p.pending_password = Some(matches!(
                    purpose.into_result(),
                    Ok(ContentPurpose::Password | ContentPurpose::Pin)
                ));
            }
            // Enabling starts the field afresh: a purpose not said again
            // is the normal one.
            Request::Commit => match p.pending_enable.take() {
                Some(true) => {
                    p.enabled = true;
                    p.password = p.pending_password.take().unwrap_or(false);
                }
                Some(false) => {
                    p.enabled = false;
                    p.password = false;
                    p.pending_password = None;
                }
                None => {
                    if let Some(password) = p.pending_password.take() {
                        p.password = password;
                    }
                }
            },
            _ => {}
        }
    }
}

impl
    smithay::reexports::wayland_server::Dispatch<
        zwp_text_input_v3::ZwpTextInputV3,
        smithay::wayland::text_input::TextInputUserData,
    > for State
{
    fn request(
        state: &mut Self,
        client: &Client,
        resource: &zwp_text_input_v3::ZwpTextInputV3,
        request: zwp_text_input_v3::Request,
        data: &smithay::wayland::text_input::TextInputUserData,
        dh: &DisplayHandle,
        data_init: &mut smithay::reexports::wayland_server::DataInit<'_, Self>,
    ) {
        use smithay::reexports::wayland_server::Resource as _;
        state.protocols.purposes.note(resource.id(), &request);
        <smithay::wayland::text_input::TextInputManagerState as smithay::reexports::wayland_server::Dispatch<
            zwp_text_input_v3::ZwpTextInputV3,
            smithay::wayland::text_input::TextInputUserData,
            Self,
        >>::request(state, client, resource, request, data, dh, data_init);
    }

    fn destroyed(
        state: &mut Self,
        client: smithay::reexports::wayland_server::backend::ClientId,
        resource: &zwp_text_input_v3::ZwpTextInputV3,
        data: &smithay::wayland::text_input::TextInputUserData,
    ) {
        use smithay::reexports::wayland_server::Resource as _;
        state.protocols.purposes.inputs.remove(&resource.id());
        <smithay::wayland::text_input::TextInputManagerState as smithay::reexports::wayland_server::Dispatch<
            zwp_text_input_v3::ZwpTextInputV3,
            smithay::wayland::text_input::TextInputUserData,
            Self,
        >>::destroyed(state, client, resource, data);
    }
}

impl State {
    /// Whether `window`'s application says one of its fields with a text
    /// input on is for a password (or a PIN), on any seat: as an agent
    /// may type there, the field is taken as the one it types into.
    pub(crate) fn password_field_in(&self, window: &smithay::desktop::Window) -> bool {
        use smithay::{reexports::wayland_server::Resource as _, wayland::seat::WaylandFocus as _};
        let Some(surface) = window.wl_surface() else {
            return false;
        };
        let surface = surface.id();
        self.protocols
            .purposes
            .inputs
            .iter()
            .any(|(id, p)| p.enabled && p.password && id.same_client_as(&surface))
    }
}

impl State {
    /// Tells the windows on `output` that the frame drawn at `now` is on
    /// screen (presentation-time).
    pub fn presented(&mut self, output: &smithay::output::Output, now: Duration, seq: u64) {
        self.capture_frame_drawn(output);
        use smithay::{
            desktop::utils::OutputPresentationFeedback,
            reexports::wayland_protocols::wp::presentation_time::server::wp_presentation_feedback::Kind,
            utils::Monotonic, wayland::presentation::Refresh,
        };
        let mut feedback = OutputPresentationFeedback::new(output);
        for window in self.space.elements() {
            if self.space.outputs_for_element(window).contains(output) {
                window.take_presentation_feedback(
                    &mut feedback,
                    |_, _| Some(output.clone()),
                    |_, _| Kind::Vsync,
                );
            }
        }
        let refresh = output
            .current_mode()
            .filter(|m| m.refresh > 0)
            .map_or(Refresh::Unknown, |m| {
                Refresh::Fixed(Duration::from_secs_f64(1000.0 / f64::from(m.refresh)))
            });
        feedback.presented::<_, Monotonic>(now, refresh, seq, Kind::Vsync);
    }

    /// Where the pointer may go from `from` towards `to`: nowhere while a
    /// window locks it, within its region while one confines it.
    pub fn constrained(
        &self,
        from: Point<f64, Logical>,
        to: Point<f64, Logical>,
    ) -> Point<f64, Logical> {
        use smithay::wayland::pointer_constraints::PointerConstraint;
        let Some(pointer) = self.seat.get_pointer() else {
            return to;
        };
        let Some(surface) = pointer.current_focus() else {
            return to;
        };
        let Some(origin) = self.window_for(&surface).and_then(|w| {
            self.space
                .element_location(&w)
                .map(|l| l - w.geometry().loc)
        }) else {
            return to;
        };
        with_pointer_constraint(&surface, &pointer, |constraint| {
            let Some(constraint) = constraint.filter(|c| c.is_active()) else {
                return to;
            };
            match &*constraint {
                PointerConstraint::Locked(_) => from,
                PointerConstraint::Confined(c) => {
                    let local = to - origin.to_f64();
                    let inside = c.region().is_none_or(|r| r.contains(local.to_i32_round()));
                    if inside { to } else { from }
                }
            }
        })
    }
}
