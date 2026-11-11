//! smithay's protocol handlers: what each protocol asks of the compositor.

mod compositor;
pub mod layer_shell;
mod protocols;
mod xdg_shell;
mod xwayland;

pub use xdg_shell::kde_server_side;

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
        &mut self.protocols.seat_state
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.pointer.cursor = image;
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
        self.clipboard_focus_changed();
        self.emoji_focus_changed(focused);
        self.magnifier_focus_changed(focused);
    }
}
delegate_seat!(State);

impl SelectionHandler for State {
    /// What the compositor offers itself (a text the shell copies, an item
    /// of the clipboard's history), the same bytes for each of its types,
    /// or `None`: an X11 client's selection, which XWayland's window
    /// manager reads.
    type SelectionUserData = Option<std::sync::Arc<[u8]>>;

    /// A Wayland client took a selection: X11 clients see it offered, and
    /// the clipboard's history gets it (5.6).
    fn new_selection(
        &mut self,
        ty: SelectionTarget,
        source: Option<SelectionSource>,
        _seat: Seat<Self>,
    ) {
        let types = source.map(|s| s.mime_types());
        if ty == SelectionTarget::Clipboard
            && let Some(types) = &types
        {
            self.selection_taken(types.clone(), crate::clipboard::Source::Wayland);
        }
        if let Some(xwm) = self.xwm.as_mut()
            && let Err(err) = xwm.new_selection(ty, types)
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
        user_data: &Option<std::sync::Arc<[u8]>>,
    ) {
        if let Some(bytes) = user_data {
            // The emoji picker's paste, read: the clipboard goes back.
            self.emoji_read(bytes);
            // Written on a thread: the reader may be slow, or never read.
            let bytes = bytes.clone();
            crate::programs::on_a_thread("write the clipboard".to_owned(), move || {
                let mut file = std::fs::File::from(fd);
                if let Err(err) = std::io::Write::write_all(&mut file, &bytes) {
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
        &self.protocols.data_device_state
    }
}
impl ClientDndGrabHandler for State {}
/// The compositor's own drags: a capture's thumbnail (5.9).
impl ServerDndGrabHandler for State {
    fn send(&mut self, mime_type: String, fd: OwnedFd, _seat: Seat<Self>) {
        self.capture_drag_send(&mime_type, fd);
    }

    fn cancelled(&mut self, _seat: Seat<Self>) {
        self.capture_drag_ended();
    }

    fn finished(&mut self, _seat: Seat<Self>) {
        self.capture_drag_ended();
    }
}
delegate_data_device!(State);

impl PrimarySelectionHandler for State {
    fn primary_selection_state(&self) -> &PrimarySelectionState {
        &self.protocols.primary_selection_state
    }
}
delegate_primary_selection!(State);

impl OutputHandler for State {}
delegate_output!(State);
