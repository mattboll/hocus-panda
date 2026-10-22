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

use smithay::{
    delegate_alpha_modifier, delegate_content_type, delegate_cursor_shape,
    delegate_ext_data_control, delegate_fractional_scale, delegate_idle_inhibit,
    delegate_keyboard_shortcuts_inhibit, delegate_pointer_constraints, delegate_pointer_gestures,
    delegate_presentation, delegate_primary_selection, delegate_relative_pointer,
    delegate_security_context, delegate_single_pixel_buffer, delegate_viewporter,
    delegate_virtual_keyboard_manager, delegate_xdg_activation, delegate_xdg_foreign,
    input::pointer::PointerHandle,
    reexports::wayland_server::{Client, DisplayHandle, protocol::wl_surface::WlSurface},
    utils::{Logical, Point, SERIAL_COUNTER},
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
    pub shortcuts_inhibit: KeyboardShortcutsInhibitState,
    /// The surface with the keyboard, as last told: the keyboard cannot
    /// be asked from within its own key filter.
    pub keyboard_focus: Option<WlSurface>,
    pub foreign: XdgForeignState,
    pub presentation: PresentationState,
    pub fractional_scale: FractionalScaleManagerState,
    pub primary_selection: PrimarySelectionState,
    pub data_control: ExtDataControlState,
    pub security_context: SecurityContextState,
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
            shortcuts_inhibit: KeyboardShortcutsInhibitState::new::<State>(dh),
            keyboard_focus: None,
            foreign: XdgForeignState::new::<State>(dh),
            // CLOCK_MONOTONIC, as the frame times.
            presentation: PresentationState::new::<State>(dh, 1),
            fractional_scale: FractionalScaleManagerState::new::<State>(dh),
            primary_selection,
            data_control,
            security_context: SecurityContextState::new::<State, _>(dh, trusted),
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

    /// A recent token brings its window forward, on whatever desktop it
    /// is; an old one does nothing.
    fn request_activation(
        &mut self,
        token: XdgActivationToken,
        token_data: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        if token_data.timestamp.elapsed() < ACTIVATION_PATIENCE
            && let Some(id) = self.managed_id(&surface)
        {
            self.restore(id, SERIAL_COUNTER.next_serial());
        }
        self.protocols.activation.remove_token(&token);
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

impl State {
    /// Tells the windows on `output` that the frame drawn at `now` is on
    /// screen (presentation-time).
    pub fn presented(&self, output: &smithay::output::Output, now: Duration, seq: u64) {
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
