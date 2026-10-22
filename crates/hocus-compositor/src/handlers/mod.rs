//! smithay protocol handlers.

mod compositor;
mod workspace;
mod xdg_shell;
mod xwayland;

pub use workspace::WorkspaceState;

use smithay::{
    delegate_data_device, delegate_foreign_toplevel_list, delegate_output, delegate_seat,
    input::{Seat, SeatHandler, SeatState, pointer::CursorImageStatus},
    reexports::wayland_server::{Resource, protocol::wl_surface::WlSurface},
    wayland::{
        foreign_toplevel_list::{ForeignToplevelListHandler, ForeignToplevelListState},
        output::OutputHandler,
        selection::{
            SelectionHandler, SelectionSource, SelectionTarget,
            data_device::{
                ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
                set_data_device_focus,
            },
        },
    },
};

use std::os::fd::OwnedFd;

use crate::State;

impl SeatHandler for State {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<State> {
        &mut self.seat_state
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor = image;
        self.needs_redraw = true;
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        // The clipboard follows keyboard focus.
        let client = focused.and_then(|s| self.display_handle.get_client(s.id()).ok());
        set_data_device_focus(&self.display_handle, seat, client);
    }
}
delegate_seat!(State);

impl SelectionHandler for State {
    /// Selections the compositor offers are X11 clients': nothing to carry.
    type SelectionUserData = ();

    /// A Wayland client took the selection: X11 clients see it offered.
    fn new_selection(
        &mut self,
        ty: SelectionTarget,
        source: Option<SelectionSource>,
        _seat: Seat<Self>,
    ) {
        if let Some(xwm) = self.xwm.as_mut()
            && let Err(err) = xwm.new_selection(ty, source.map(|s| s.mime_types()))
        {
            tracing::warn!(?err, "could not offer the selection to X11");
        }
    }

    /// A Wayland client reads what an X11 client offered.
    fn send_selection(
        &mut self,
        ty: SelectionTarget,
        mime_type: String,
        fd: OwnedFd,
        _seat: Seat<Self>,
        _user_data: &(),
    ) {
        if let (Some(xwm), Some(handle)) = (self.xwm.as_mut(), self.loop_handle.clone())
            && let Err(err) = xwm.send_selection(ty, mime_type, fd, handle)
        {
            tracing::warn!(?err, "could not read the X11 selection");
        }
    }
}

impl DataDeviceHandler for State {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}
impl ClientDndGrabHandler for State {}
impl ServerDndGrabHandler for State {}
delegate_data_device!(State);

impl OutputHandler for State {}
delegate_output!(State);

impl ForeignToplevelListHandler for State {
    fn foreign_toplevel_list_state(&mut self) -> &mut ForeignToplevelListState {
        &mut self.foreign_toplevels
    }
}
delegate_foreign_toplevel_list!(State);
