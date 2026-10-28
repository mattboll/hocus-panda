//! xdg-shell and xdg-decoration: application windows and their popups.
//! A toplevel is mapped on its first buffer, at the centre of the work
//! area; decorations are the server's by default (none drawn yet: the grip
//! is task 2.4's).

use smithay::{
    backend::renderer::utils::with_renderer_surface_state,
    delegate_xdg_decoration, delegate_xdg_shell,
    desktop::{PopupKind, Window, find_popup_root_surface, get_popup_toplevel_coords},
    input::{Seat, pointer::GrabStartData},
    reexports::{
        wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
        wayland_server::{
            Resource,
            protocol::{wl_seat, wl_surface::WlSurface},
        },
    },
    utils::Serial,
    wayland::{
        seat::WaylandFocus,
        shell::xdg::{
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
            decoration::XdgDecorationHandler,
        },
    },
};

use crate::State;

impl XdgShellHandler for State {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        // What a window may take, so that one that chooses its own size
        // fits the work area it opens in.
        if let Some(area) = self.output_under_pointer().and_then(|o| self.work_area(&o)) {
            surface.with_pending_state(|p| p.bounds = Some((area.width, area.height).into()));
        }
        self.unmapped.push(Window::new_wayland_window(surface));
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for(surface.wl_surface()) {
            self.unmap_window(&window);
        }
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        self.unconstrain_popup(&surface);
        if let Err(err) = self.popups.track_popup(PopupKind::Xdg(surface)) {
            tracing::warn!(%err, "could not track a popup");
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

    /// A drag on a client-side header.
    fn move_request(&mut self, surface: ToplevelSurface, seat: wl_seat::WlSeat, serial: Serial) {
        let Some(seat) = Seat::<State>::from_resource(&seat) else {
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

    /// Popup grabs (menus closing on a click elsewhere, keys going to the
    /// menu) come with the shell's popovers: the menu shows meanwhile.
    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {}
}
delegate_xdg_shell!(State);

/// Server-side by default: the desktop draws its frame. A client that asks
/// to draw its own keeps its header.
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

/// The pointer grab a client's move request takes over, if the request
/// comes from the press that holds it.
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

/// Configures new toplevels and popups, maps a toplevel on its first
/// buffer and unmaps it when its client takes its buffer away.
pub fn handle_commit(state: &mut State, surface: &WlSurface) {
    let waiting = state
        .unmapped
        .iter()
        .position(|w| w.wl_surface().is_some_and(|s| *s == *surface));
    if let Some(index) = waiting {
        let window = state.unmapped[index].clone();
        if let Some(toplevel) = window.toplevel() {
            if !toplevel.is_initial_configure_sent() {
                toplevel.send_configure();
                return;
            }
            if has_buffer(surface) && !window.geometry().size.is_empty() {
                state.unmapped.remove(index);
                state.map_window(window);
            }
        }
        return;
    }
    if let Some(window) = state.window_for(surface)
        && window.toplevel().is_some()
        && !has_buffer(surface)
    {
        // Mapped again, at the centre, when its next buffer comes.
        state.unmap_window(&window);
        state.unmapped.push(window);
        return;
    }
    if let Some(PopupKind::Xdg(popup)) = state.popups.find_popup(surface)
        && !popup.is_initial_configure_sent()
        && let Err(err) = popup.send_configure()
    {
        tracing::warn!(?err, "could not configure a popup");
    }
}

fn has_buffer(surface: &WlSurface) -> bool {
    with_renderer_surface_state(surface, |s| s.buffer().is_some()).unwrap_or(false)
}

impl State {
    /// Keeps a popup inside the output its window is on.
    fn unconstrain_popup(&self, popup: &PopupSurface) {
        let kind = PopupKind::Xdg(popup.clone());
        let Ok(root) = find_popup_root_surface(&kind) else {
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
        target.loc -= get_popup_toplevel_coords(&kind);
        target.loc -= window_geo.loc;
        popup.with_pending_state(|state| {
            state.geometry = state.positioner.get_unconstrained_geometry(target);
        });
    }
}
