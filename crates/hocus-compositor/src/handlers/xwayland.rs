//! X11 applications through XWayland: their windows join the model like
//! Wayland ones (placement, desktops, focus, snapping), menus and tooltips
//! (override-redirect windows) are shown where they ask, and the clipboard
//! goes both ways. Nothing in the desktop itself depends on X11.

use std::os::fd::OwnedFd;

use smithay::{
    delegate_xwayland_shell,
    desktop::Window,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Rectangle, SERIAL_COUNTER},
    wayland::{
        selection::{
            SelectionTarget,
            data_device::{
                clear_data_device_selection, request_data_device_client_selection,
                set_data_device_selection,
            },
        },
        xwayland_shell::{XWaylandShellHandler, XWaylandShellState},
    },
    xwayland::{
        X11Surface, X11Wm, XwmHandler,
        xwm::{Reorder, ResizeEdge as X11ResizeEdge, WmWindowProperty, XwmId},
    },
};

use crate::{
    State,
    desktops::model_id,
    grabs::{BTN_LEFT, Edges},
};

impl State {
    /// The managed window of an X11 surface.
    fn x11_window(&self, surface: &X11Surface) -> Option<Window> {
        self.windows
            .values()
            .map(|m| &m.window)
            .find(|w| w.x11_surface() == Some(surface))
            .cloned()
    }

    fn x11_id(&self, surface: &X11Surface) -> Option<hocus_core::WindowId> {
        self.x11_window(surface).as_ref().and_then(model_id)
    }

    /// Forgets an X11 window that is gone or hidden by its client.
    fn x11_gone(&mut self, surface: &X11Surface) {
        if let Some(window) = self.x11_window(surface) {
            self.unmanage(&window, SERIAL_COUNTER.next_serial());
            return;
        }
        let overlay = self
            .space
            .elements()
            .find(|w| w.x11_surface() == Some(surface))
            .cloned();
        if let Some(overlay) = overlay {
            self.space.unmap_elem(&overlay);
            self.needs_redraw = true;
        }
    }
}

impl XwmHandler for State {
    fn xwm_state(&mut self, _xwm: XwmId) -> &mut X11Wm {
        // The window manager only calls back once it exists: it is the one
        // calling.
        #[allow(clippy::expect_used)]
        self.xwm.as_mut().expect("an X11 window manager calls back")
    }

    fn new_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn new_override_redirect_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn map_window_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Err(err) = surface.set_mapped(true) {
            tracing::warn!(%err, "could not map an X11 window");
            return;
        }
        let window = Window::new_x11_window(surface.clone());
        let Some(output) = self.output_under_pointer() else {
            return;
        };
        self.manage(window, &output, SERIAL_COUNTER.next_serial());
        // Urgent from the start: it asks once it is there, unless it has
        // the keyboard.
        if surface.hints().is_some_and(|h| h.urgent)
            && let Some(id) = self.x11_id(&surface)
        {
            self.ask_attention(id);
        }
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, surface: X11Surface) {
        // Menus and tooltips: where they ask, above everything.
        let location = surface.geometry().loc;
        self.space
            .map_element(Window::new_x11_window(surface), location, true);
        self.needs_redraw = true;
    }

    fn unmapped_window(&mut self, _xwm: XwmId, surface: X11Surface) {
        self.x11_gone(&surface);
        if !surface.is_override_redirect() {
            let _ = surface.set_mapped(false);
        }
    }

    fn destroyed_window(&mut self, _xwm: XwmId, surface: X11Surface) {
        self.x11_gone(&surface);
    }

    fn configure_request(
        &mut self,
        _xwm: XwmId,
        surface: X11Surface,
        _x: Option<i32>,
        _y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        _reorder: Option<Reorder>,
    ) {
        // The window manager places windows; a managed one may change its
        // size, one not managed yet anything.
        let mut geometry = surface.geometry();
        if let Some(w) = w {
            geometry.size.w = i32::try_from(w).unwrap_or(geometry.size.w);
        }
        if let Some(h) = h {
            geometry.size.h = i32::try_from(h).unwrap_or(geometry.size.h);
        }
        if let Some(window) = self.x11_window(&surface)
            && let Some(location) = self.space.element_location(&window)
        {
            geometry.loc = location;
        }
        let _ = surface.configure(geometry);
    }

    fn configure_notify(
        &mut self,
        _xwm: XwmId,
        surface: X11Surface,
        geometry: Rectangle<i32, Logical>,
        _above: Option<smithay::xwayland::xwm::X11Window>,
    ) {
        if !surface.is_override_redirect() {
            return;
        }
        let overlay = self
            .space
            .elements()
            .find(|w| w.x11_surface() == Some(&surface))
            .cloned();
        if let Some(overlay) = overlay {
            self.space.map_element(overlay, geometry.loc, true);
            self.needs_redraw = true;
        }
    }

    /// An urgency hint asks for attention, as an activation does; taken
    /// back, it asks no more (#145).
    fn property_notify(&mut self, _xwm: XwmId, surface: X11Surface, property: WmWindowProperty) {
        if property != WmWindowProperty::Hints {
            return;
        }
        let Some(id) = self.x11_id(&surface) else {
            return;
        };
        if surface.hints().is_some_and(|h| h.urgent) {
            self.ask_attention(id);
        } else {
            self.drop_attention(id);
        }
    }

    fn maximize_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Some(id) = self.x11_id(&surface) {
            self.set_maximized(id, true, SERIAL_COUNTER.next_serial());
        }
    }

    fn unmaximize_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Some(id) = self.x11_id(&surface) {
            self.set_maximized(id, false, SERIAL_COUNTER.next_serial());
        }
    }

    fn fullscreen_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Some(id) = self.x11_id(&surface) {
            self.set_fullscreen(id, true, None, SERIAL_COUNTER.next_serial());
        }
    }

    fn unfullscreen_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Some(id) = self.x11_id(&surface) {
            self.set_fullscreen(id, false, None, SERIAL_COUNTER.next_serial());
        }
    }

    fn minimize_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Some(id) = self.x11_id(&surface) {
            self.minimize(id, SERIAL_COUNTER.next_serial());
        }
    }

    fn unminimize_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Some(id) = self.x11_id(&surface) {
            self.restore(id, SERIAL_COUNTER.next_serial());
        }
    }

    fn resize_request(
        &mut self,
        _xwm: XwmId,
        surface: X11Surface,
        button: u32,
        edge: X11ResizeEdge,
    ) {
        let Some(window) = self.x11_window(&surface) else {
            return;
        };
        use X11ResizeEdge as E;
        let edges = Edges {
            left: matches!(edge, E::Left | E::TopLeft | E::BottomLeft),
            right: matches!(edge, E::Right | E::TopRight | E::BottomRight),
            top: matches!(edge, E::Top | E::TopLeft | E::TopRight),
            bottom: matches!(edge, E::Bottom | E::BottomLeft | E::BottomRight),
        };
        // X11 buttons count from 1; evdev's left button is BTN_LEFT.
        let button = BTN_LEFT + button.saturating_sub(1);
        self.start_resize(&window, edges, button, None, SERIAL_COUNTER.next_serial());
    }

    fn move_request(&mut self, _xwm: XwmId, surface: X11Surface, button: u32) {
        if let Some(window) = self.x11_window(&surface) {
            let button = BTN_LEFT + button.saturating_sub(1);
            self.start_move(&window, button, None, SERIAL_COUNTER.next_serial());
        }
    }

    fn allow_selection_access(&mut self, _xwm: XwmId, _selection: SelectionTarget) -> bool {
        true
    }

    /// A Wayland client owns the selection and an X11 one wants it.
    fn send_selection(
        &mut self,
        _xwm: XwmId,
        selection: SelectionTarget,
        mime_type: String,
        fd: OwnedFd,
    ) {
        if selection != SelectionTarget::Clipboard {
            return;
        }
        // Text the shell copied is written here; a client's goes through
        // its own source.
        let own = smithay::wayland::selection::data_device::current_data_device_selection_userdata(
            &self.seat,
        )
        .and_then(|offer| match &*offer {
            crate::launcher::Offer::Text(text) => Some(Ok(text.clone())),
            crate::launcher::Offer::Png(png) => Some(Err(png.clone())),
            crate::launcher::Offer::X11 => None,
        });
        match own {
            Some(Ok(text)) => return crate::launcher::send_text(text, fd),
            Some(Err(png)) => return crate::launcher::send_bytes(png, fd),
            None => {}
        }
        if let Err(err) = request_data_device_client_selection(&self.seat, mime_type, fd) {
            tracing::debug!(?err, "no Wayland clipboard to give X11");
        }
    }

    /// An X11 client took the selection: Wayland clients see it offered.
    fn new_selection(&mut self, _xwm: XwmId, selection: SelectionTarget, mime_types: Vec<String>) {
        if selection == SelectionTarget::Clipboard {
            // A Wayland selection's read still waiting is dropped.
            self.selection_changed();
            // Kept in the history, read off the loop.
            if let Some((mime, fd)) = self.clipboard_reader(&mime_types)
                && let (Some(xwm), Some(handle)) = (self.xwm.as_mut(), self.loop_handle.clone())
                && let Err(err) = xwm.send_selection(selection, mime.to_owned(), fd, handle)
            {
                tracing::debug!(?err, "could not read the X11 clipboard");
            }
            set_data_device_selection(
                &self.display_handle,
                &self.seat,
                mime_types,
                crate::launcher::Offer::X11,
            );
        }
    }

    fn cleared_selection(&mut self, _xwm: XwmId, selection: SelectionTarget) {
        if selection == SelectionTarget::Clipboard {
            clear_data_device_selection(&self.display_handle, &self.seat);
        }
    }
}

impl XWaylandShellHandler for State {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        &mut self.xwayland_shell_state
    }

    /// An X11 window maps before XWayland gives it a surface: the keyboard
    /// can only go to it now.
    fn surface_associated(&mut self, _xwm: XwmId, _wl_surface: WlSurface, surface: X11Surface) {
        if self
            .x11_id(&surface)
            .is_some_and(|id| self.model.focused() == Some(id))
        {
            self.apply_focus(SERIAL_COUNTER.next_serial());
        }
    }
}
delegate_xwayland_shell!(State);

// The window manager calls back the loop's data, which holds the state.
impl XwmHandler for crate::App {
    fn xwm_state(&mut self, xwm: XwmId) -> &mut X11Wm {
        self.state.xwm_state(xwm)
    }
    fn new_window(&mut self, xwm: XwmId, window: X11Surface) {
        self.state.new_window(xwm, window);
    }
    fn new_override_redirect_window(&mut self, xwm: XwmId, window: X11Surface) {
        self.state.new_override_redirect_window(xwm, window);
    }
    fn map_window_request(&mut self, xwm: XwmId, window: X11Surface) {
        self.state.map_window_request(xwm, window);
    }
    fn mapped_override_redirect_window(&mut self, xwm: XwmId, window: X11Surface) {
        self.state.mapped_override_redirect_window(xwm, window);
    }
    fn unmapped_window(&mut self, xwm: XwmId, window: X11Surface) {
        self.state.unmapped_window(xwm, window);
    }
    fn destroyed_window(&mut self, xwm: XwmId, window: X11Surface) {
        self.state.destroyed_window(xwm, window);
    }
    fn configure_request(
        &mut self,
        xwm: XwmId,
        window: X11Surface,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        reorder: Option<Reorder>,
    ) {
        self.state
            .configure_request(xwm, window, x, y, w, h, reorder);
    }
    fn configure_notify(
        &mut self,
        xwm: XwmId,
        window: X11Surface,
        geometry: Rectangle<i32, Logical>,
        above: Option<smithay::xwayland::xwm::X11Window>,
    ) {
        self.state.configure_notify(xwm, window, geometry, above);
    }
    fn property_notify(&mut self, xwm: XwmId, window: X11Surface, property: WmWindowProperty) {
        self.state.property_notify(xwm, window, property);
    }
    fn maximize_request(&mut self, xwm: XwmId, window: X11Surface) {
        self.state.maximize_request(xwm, window);
    }
    fn unmaximize_request(&mut self, xwm: XwmId, window: X11Surface) {
        self.state.unmaximize_request(xwm, window);
    }
    fn fullscreen_request(&mut self, xwm: XwmId, window: X11Surface) {
        self.state.fullscreen_request(xwm, window);
    }
    fn unfullscreen_request(&mut self, xwm: XwmId, window: X11Surface) {
        self.state.unfullscreen_request(xwm, window);
    }
    fn minimize_request(&mut self, xwm: XwmId, window: X11Surface) {
        self.state.minimize_request(xwm, window);
    }
    fn unminimize_request(&mut self, xwm: XwmId, window: X11Surface) {
        self.state.unminimize_request(xwm, window);
    }
    fn resize_request(&mut self, xwm: XwmId, window: X11Surface, button: u32, edge: X11ResizeEdge) {
        self.state.resize_request(xwm, window, button, edge);
    }
    fn move_request(&mut self, xwm: XwmId, window: X11Surface, button: u32) {
        self.state.move_request(xwm, window, button);
    }
    fn allow_selection_access(&mut self, xwm: XwmId, selection: SelectionTarget) -> bool {
        self.state.allow_selection_access(xwm, selection)
    }
    fn send_selection(
        &mut self,
        xwm: XwmId,
        selection: SelectionTarget,
        mime_type: String,
        fd: OwnedFd,
    ) {
        self.state.send_selection(xwm, selection, mime_type, fd);
    }
    fn new_selection(&mut self, xwm: XwmId, selection: SelectionTarget, mime_types: Vec<String>) {
        self.state.new_selection(xwm, selection, mime_types);
    }
    fn cleared_selection(&mut self, xwm: XwmId, selection: SelectionTarget) {
        self.state.cleared_selection(xwm, selection);
    }
}

impl XWaylandShellHandler for crate::App {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        &mut self.state.xwayland_shell_state
    }
    fn surface_associated(&mut self, xwm: XwmId, wl_surface: WlSurface, surface: X11Surface) {
        self.state.surface_associated(xwm, wl_surface, surface);
    }
}
