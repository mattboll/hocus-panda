//! smithay protocol handlers.

mod compositor;
pub mod layer_shell;
pub mod output_management;
pub mod protocols;
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

    /// The user's cursor, as the client under it sets it; an agent's
    /// pointer is drawn the compositor's way (#43).
    fn cursor_image(&mut self, seat: &Seat<Self>, image: CursorImageStatus) {
        if *seat != self.seat {
            return;
        }
        self.cursor = image;
        self.needs_redraw = true;
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        // An agent's seat has its own focus, clipboard and selection
        // (#43); what follows the user's keyboard does not follow it.
        if *seat == self.seat {
            self.protocols.keyboard_focus = focused.cloned();
        }
        // The clipboard follows keyboard focus.
        let client = focused.and_then(|s| self.display_handle.get_client(s.id()).ok());
        set_data_device_focus(&self.display_handle, seat, client.clone());
        smithay::wayland::selection::primary_selection::set_primary_focus(
            &self.display_handle,
            seat,
            client,
        );
    }
}
delegate_seat!(State);

impl SelectionHandler for State {
    /// What the compositor itself offers: an X11 client's selection, or
    /// text from the shell.
    type SelectionUserData = crate::launcher::Offer;

    /// A Wayland client took the selection: X11 clients see it offered.
    fn new_selection(
        &mut self,
        ty: SelectionTarget,
        source: Option<SelectionSource>,
        seat: Seat<Self>,
    ) {
        // An agent's seat has a clipboard of its own: never kept in the
        // history, nor offered to X11.
        if seat != self.seat {
            return;
        }
        // Kept in the history, read off the loop; asked once the selection
        // is in place, after this request. Asking reads whatever the
        // clipboard holds then: if a newer selection came meanwhile (a
        // password manager's, marked secret, that must never be read), this
        // one is not read at all.
        let asked = (ty == SelectionTarget::Clipboard).then(|| self.selection_changed());
        if let Some(asked) = asked
            && let Some(source) = &source
            && let Some((mime, fd)) = self.clipboard_reader(&source.mime_types())
            && let Some(handle) = &self.loop_handle
        {
            handle.insert_idle(move |app| {
                if !app.state.selection_is(asked) {
                    return;
                }
                if let Err(err) =
                    smithay::wayland::selection::data_device::request_data_device_client_selection(
                        &app.state.seat,
                        mime.to_owned(),
                        fd,
                    )
                {
                    tracing::debug!(?err, "could not read the clipboard");
                }
            });
        }
        let mimes = source.as_ref().map(|s| s.mime_types());
        if let Some(xwm) = self.xwm.as_mut()
            && let Err(err) = xwm.new_selection(ty, mimes)
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
        offer: &crate::launcher::Offer,
    ) {
        match offer {
            crate::launcher::Offer::Text(text) => crate::launcher::send_text(text.clone(), fd),
            crate::launcher::Offer::Png(png) => crate::launcher::send_bytes(png.clone(), fd),
            crate::launcher::Offer::X11 => {
                if let (Some(xwm), Some(handle)) = (self.xwm.as_mut(), self.loop_handle.clone())
                    && let Err(err) = xwm.send_selection(ty, mime_type, fd, handle)
                {
                    tracing::warn!(?err, "could not read the X11 selection");
                }
            }
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
