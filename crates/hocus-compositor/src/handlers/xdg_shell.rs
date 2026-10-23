use smithay::{
    delegate_xdg_decoration, delegate_xdg_shell,
    desktop::{PopupKind, Window, find_popup_root_surface, get_popup_toplevel_coords},
    input::{Seat, pointer::GrabStartData},
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{
            Resource,
            protocol::{wl_output, wl_seat, wl_surface::WlSurface},
        },
    },
    utils::{SERIAL_COUNTER, Serial},
    wayland::{
        compositor::with_states,
        shell::xdg::{
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
            XdgToplevelSurfaceData,
        },
    },
};

use smithay::{
    output::Output,
    reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
    wayland::shell::xdg::decoration::XdgDecorationHandler,
};

use crate::{State, desktops::model_id, grabs::edges_of};

impl State {
    /// The model's id of the managed window of `surface`.
    pub(crate) fn managed_id(&self, surface: &WlSurface) -> Option<hocus_core::WindowId> {
        self.window_for(surface).as_ref().and_then(model_id)
    }
}

impl XdgShellHandler for State {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        // What a window may take, so that one that chooses its size fits
        // the screen it opens on.
        if let Some(output) = self.output_under_pointer() {
            let area = self.work_area(&output);
            surface.with_pending_state(|p| p.bounds = Some(area.size));
        }
        // Its picture as its surface goes, for the closing motion (#34):
        // a client that quits may take its surface before its toplevel, and
        // this hook, added before the renderer's, runs before it drops the
        // textures.
        smithay::wayland::compositor::add_destruction_hook(
            surface.wl_surface(),
            |state: &mut State, surface| state.surface_going(surface),
        );
        // Mapped once its first buffer tells us its size.
        self.unplaced.push(Window::new_wayland_window(surface));
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        let wl_surface = surface.wl_surface();
        self.unplaced
            .retain(|w| w.toplevel().is_none_or(|t| t.wl_surface() != wl_surface));
        if let Some(window) = self.window_for(wl_surface) {
            self.unmanage(&window, SERIAL_COUNTER.next_serial());
        }
    }

    fn title_changed(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for(surface.wl_surface()) {
            self.window_renamed(&window);
        }
    }

    fn app_id_changed(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for(surface.wl_surface()) {
            self.window_renamed(&window);
        }
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        match self.managed_id(surface.wl_surface()) {
            Some(id) => self.set_maximized(id, true, SERIAL_COUNTER.next_serial()),
            // Applied when it is managed, on its first buffer.
            None => surface.with_pending_state(|p| {
                p.states.set(xdg_toplevel::State::Maximized);
            }),
        }
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.managed_id(surface.wl_surface()) {
            self.set_maximized(id, false, SERIAL_COUNTER.next_serial());
        }
    }

    fn fullscreen_request(
        &mut self,
        surface: ToplevelSurface,
        output: Option<wl_output::WlOutput>,
    ) {
        let output = output.as_ref().and_then(Output::from_resource);
        match self.managed_id(surface.wl_surface()) {
            Some(id) => self.set_fullscreen(id, true, output, SERIAL_COUNTER.next_serial()),
            None => surface.with_pending_state(|p| {
                p.states.set(xdg_toplevel::State::Fullscreen);
            }),
        }
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.managed_id(surface.wl_surface()) {
            self.set_fullscreen(id, false, None, SERIAL_COUNTER.next_serial());
        }
    }

    fn minimize_request(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.managed_id(surface.wl_surface()) {
            self.minimize(id, SERIAL_COUNTER.next_serial());
        }
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        self.unconstrain_popup(&surface);
        if let Err(err) = self.popups.track_popup(PopupKind::Xdg(surface)) {
            tracing::warn!(%err, "could not track popup");
        }
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|state| {
            state.geometry = positioner.get_geometry();
            state.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    fn move_request(&mut self, surface: ToplevelSurface, seat: wl_seat::WlSeat, serial: Serial) {
        // Moves and resizes take the user's pointer: an agent's click
        // never starts one (#43).
        let Some(seat) = Seat::<State>::from_resource(&seat).filter(|s| *s == self.seat) else {
            return;
        };
        let wl_surface = surface.wl_surface();
        let Some(start_data) = check_grab(&seat, wl_surface, serial) else {
            return;
        };
        if let Some(window) = self.window_for(wl_surface) {
            let button = start_data.button;
            self.start_move(&window, button, Some(start_data), serial);
        }
    }

    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        seat: wl_seat::WlSeat,
        serial: Serial,
        edges: xdg_toplevel::ResizeEdge,
    ) {
        let Some(seat) = Seat::<State>::from_resource(&seat).filter(|s| *s == self.seat) else {
            return;
        };
        let wl_surface = surface.wl_surface();
        let Some(start_data) = check_grab(&seat, wl_surface, serial) else {
            return;
        };
        if let Some(window) = self.window_for(wl_surface) {
            let button = start_data.button;
            self.start_resize(&window, edges_of(edges), button, Some(start_data), serial);
        }
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {
        // Popup grabs come with menu support (milestone M1).
    }
}
delegate_xdg_shell!(State);

/// Server-side by default: no title bar, but corners, shadow and ring. A
/// client that asks to draw its own keeps them.
impl XdgDecorationHandler for State {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(DecorationMode::ServerSide);
        });
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, mode: DecorationMode) {
        toplevel.with_pending_state(|state| state.decoration_mode = Some(mode));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        self.new_decoration(toplevel);
    }
}
delegate_xdg_decoration!(State);

/// The pointer grab that a client's move request can take over, if the
/// request comes from the button press that started it.
fn check_grab(
    seat: &Seat<State>,
    surface: &WlSurface,
    serial: Serial,
) -> Option<GrabStartData<State>> {
    let pointer = seat.get_pointer()?;
    if !pointer.has_grab(serial) {
        return None;
    }
    let start_data = pointer.grab_start_data()?;
    let (focus, _) = start_data.focus.as_ref()?;
    if !focus.id().same_client_as(&surface.id()) {
        return None;
    }
    Some(start_data)
}

/// Sends the initial configure of new toplevels and popups.
pub fn handle_commit(state: &mut State, surface: &WlSurface) {
    if let Some(window) = state.window_for(surface) {
        let initial_configure_sent = with_states(surface, |states| {
            states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .and_then(|data| data.lock().ok().map(|d| d.initial_configure_sent))
                .unwrap_or(true)
        });
        if !initial_configure_sent && let Some(toplevel) = window.toplevel() {
            toplevel.send_configure();
        }
    }

    state.popups.commit(surface);
    if let Some(PopupKind::Xdg(popup)) = state.popups.find_popup(surface)
        && !popup.is_initial_configure_sent()
        && let Err(err) = popup.send_configure()
    {
        tracing::warn!(?err, "initial popup configure failed");
    }
}

impl State {
    /// Keeps a popup inside the output of its window.
    fn unconstrain_popup(&self, popup: &PopupSurface) {
        let Ok(root) = find_popup_root_surface(&PopupKind::Xdg(popup.clone())) else {
            return;
        };
        let Some(window) = self.window_for(&root) else {
            return;
        };
        let Some(output) = self.space.outputs_for_element(&window).into_iter().next() else {
            return;
        };
        let (Some(output_geo), Some(window_geo)) = (
            self.space.output_geometry(&output),
            self.space.element_geometry(&window),
        ) else {
            return;
        };
        // The positioner works relative to the parent's geometry.
        let mut target = output_geo;
        target.loc -= get_popup_toplevel_coords(&PopupKind::Xdg(popup.clone()));
        target.loc -= window_geo.loc;
        popup.with_pending_state(|state| {
            state.geometry = state.positioner.get_unconstrained_geometry(target);
        });
    }
}
