//! smithay's protocol handlers: what each protocol asks of the compositor.

mod compositor;
pub mod layer_shell;
mod protocols;
mod xdg_shell;
mod xwayland;

use std::os::fd::OwnedFd;

use smithay::{
    delegate_data_device, delegate_output, delegate_primary_selection, delegate_seat,
    input::{Seat, SeatHandler, SeatState, pointer::CursorImageStatus},
    reexports::wayland_server::{Resource, protocol::wl_surface::WlSurface},
    wayland::{
        output::OutputHandler,
        seat::WaylandFocus,
        selection::{
            SelectionHandler, SelectionSource, SelectionTarget,
            data_device::{
                ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
                set_data_device_focus,
            },
            primary_selection::{
                PrimarySelectionHandler, PrimarySelectionState, set_primary_focus,
            },
        },
    },
};

use crate::{State, focus::KeyboardFocus};

impl SeatHandler for State {
    type KeyboardFocus = KeyboardFocus;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<State> {
        &mut self.seat_state
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor = image;
        self.redraw = true;
    }

    /// The clipboard and the primary selection follow the keyboard: only
    /// the client with it may read them.
    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&KeyboardFocus>) {
        tracing::debug!(?focused, "keyboard focus");
        let client = focused
            .and_then(|f| f.wl_surface())
            .and_then(|s| self.display_handle.get_client(s.id()).ok());
        set_data_device_focus(&self.display_handle, seat, client.clone());
        set_primary_focus(&self.display_handle, seat, client);
    }
}
delegate_seat!(State);

impl SelectionHandler for State {
    /// What the compositor offers itself (a text the shell copies), or
    /// `None`: an X11 client's selection, which XWayland's window manager
    /// reads.
    type SelectionUserData = Option<std::sync::Arc<str>>;

    /// A Wayland client took a selection: X11 clients see it offered.
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
        user_data: &Option<std::sync::Arc<str>>,
    ) {
        if let Some(text) = user_data {
            // Written on a thread: the reader may be slow, or never read.
            let text = text.clone();
            crate::programs::on_a_thread("write the clipboard".to_owned(), move || {
                let mut file = std::fs::File::from(fd);
                if let Err(err) = std::io::Write::write_all(&mut file, text.as_bytes()) {
                    tracing::debug!(%err, "could not write the clipboard");
                }
            });
            return;
        }
        if let Some(xwm) = self.xwm.as_mut()
            && let Err(err) = xwm.send_selection(ty, mime_type, fd, self.loop_handle.clone())
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

impl PrimarySelectionHandler for State {
    fn primary_selection_state(&self) -> &PrimarySelectionState {
        &self.primary_selection_state
    }
}
delegate_primary_selection!(State);

impl OutputHandler for State {}
delegate_output!(State);
