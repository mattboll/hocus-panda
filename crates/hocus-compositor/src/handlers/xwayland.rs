//! X11 applications through XWayland: their windows are mapped like
//! Wayland ones (placed at the centre, focused, moved), menus and tooltips
//! (override-redirect windows) show where they ask, and the clipboard goes
//! both ways.

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
            primary_selection::{
                clear_primary_selection, request_primary_client_selection, set_primary_selection,
            },
        },
        xwayland_shell::{XWaylandShellHandler, XWaylandShellState},
    },
    xwayland::{
        X11Surface, X11Wm, XwmHandler,
        xwm::{Reorder, ResizeEdge, WmWindowProperty, XwmId},
    },
};

use crate::{App, State, grabs::BTN_LEFT};

impl State {
    /// The window of an X11 surface, mapped or not.
    fn x11_window(&self, surface: &X11Surface) -> Option<Window> {
        self.windows
            .values()
            .map(|m| &m.window)
            .chain(self.unmapped.iter())
            .find(|w| w.x11_surface() == Some(surface))
            .cloned()
    }

    /// Menus and tooltips, which are in the space without being windows of
    /// the model.
    fn x11_overlay(&self, surface: &X11Surface) -> Option<Window> {
        self.space
            .elements()
            .find(|w| w.x11_surface() == Some(surface))
            .cloned()
    }

    /// Forgets an X11 window gone or hidden by its client.
    fn x11_gone(&mut self, surface: &X11Surface) {
        if let Some(wl) = surface.wl_surface() {
            self.surface_going(&wl);
        }
        if let Some(window) = self.x11_window(surface) {
            self.unmap_window(&window);
        } else if let Some(overlay) = self.x11_overlay(surface) {
            self.space.unmap_elem(&overlay);
            self.redraw = true;
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
        self.map_window(Window::new_x11_window(surface));
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, surface: X11Surface) {
        let location = surface.geometry().loc;
        self.space
            .map_element(Window::new_x11_window(surface), location, false);
        self.redraw = true;
    }

    fn unmapped_window(&mut self, _xwm: XwmId, surface: X11Surface) {
        self.x11_gone(&surface);
        if !surface.is_override_redirect()
            && let Err(err) = surface.set_mapped(false)
        {
            tracing::debug!(%err, "could not unmap an X11 window");
        }
    }

    fn destroyed_window(&mut self, _xwm: XwmId, surface: X11Surface) {
        self.x11_gone(&surface);
    }

    /// The desktop places windows: a mapped one may change its size, one
    /// not mapped yet anything.
    fn configure_request(
        &mut self,
        _xwm: XwmId,
        surface: X11Surface,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        _reorder: Option<Reorder>,
    ) {
        let mut geometry = surface.geometry();
        if let Some(w) = w {
            geometry.size.w = i32::try_from(w).unwrap_or(geometry.size.w);
        }
        if let Some(h) = h {
            geometry.size.h = i32::try_from(h).unwrap_or(geometry.size.h);
        }
        let mapped = self
            .x11_window(&surface)
            .and_then(|w| self.space.element_location(&w));
        match mapped {
            Some(location) => geometry.loc = location,
            None => {
                geometry.loc.x = x.unwrap_or(geometry.loc.x);
                geometry.loc.y = y.unwrap_or(geometry.loc.y);
            }
        }
        if let Err(err) = surface.configure(geometry) {
            tracing::debug!(%err, "could not configure an X11 window");
        }
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
        if let Some(overlay) = self.x11_overlay(&surface) {
            self.space.map_element(overlay, geometry.loc, false);
            self.redraw = true;
        }
    }

    fn resize_request(&mut self, _xwm: XwmId, surface: X11Surface, button: u32, edge: ResizeEdge) {
        if let Some(id) = self.x11_window(&surface).and_then(|w| self.id_of(&w)) {
            // X11 buttons count from 1; evdev's left button is BTN_LEFT.
            let button = BTN_LEFT + button.saturating_sub(1);
            let edges = crate::model::Edges::from_x11(edge);
            self.start_resize(id, edges, button, None, SERIAL_COUNTER.next_serial());
        }
    }

    fn move_request(&mut self, _xwm: XwmId, surface: X11Surface, button: u32) {
        if let Some(id) = self.x11_window(&surface).and_then(|w| self.id_of(&w)) {
            // X11 buttons count from 1; evdev's left button is BTN_LEFT.
            let button = BTN_LEFT + button.saturating_sub(1);
            self.start_move(id, button, None, SERIAL_COUNTER.next_serial());
        }
    }

    fn maximize_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Some(id) = self.x11_window(&surface).and_then(|w| self.id_of(&w)) {
            self.snap(id, crate::model::Zone::Maximized);
        }
    }

    fn unmaximize_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Some(id) = self.x11_window(&surface).and_then(|w| self.id_of(&w)) {
            self.restore(id);
        }
    }

    fn fullscreen_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Some(id) = self.x11_window(&surface).and_then(|w| self.id_of(&w)) {
            self.set_fullscreen(id, true);
        }
    }

    fn unfullscreen_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Some(id) = self.x11_window(&surface).and_then(|w| self.id_of(&w)) {
            self.set_fullscreen(id, false);
        }
    }

    fn minimize_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Some(id) = self.x11_window(&surface).and_then(|w| self.id_of(&w)) {
            self.minimize(id);
        }
    }

    /// Its title or its class (its application's id): the bar's desktops
    /// show them.
    fn property_notify(&mut self, _xwm: XwmId, _surface: X11Surface, property: WmWindowProperty) {
        if matches!(property, WmWindowProperty::Title | WmWindowProperty::Class) {
            self.desktops_soon();
        }
    }

    fn allow_selection_access(&mut self, _xwm: XwmId, _selection: SelectionTarget) -> bool {
        true
    }

    /// A Wayland client owns the selection and an X11 one reads it.
    fn send_selection(
        &mut self,
        _xwm: XwmId,
        selection: SelectionTarget,
        mime_type: String,
        fd: OwnedFd,
    ) {
        let seat = &self.seat;
        let read = match selection {
            SelectionTarget::Clipboard => request_data_device_client_selection(seat, mime_type, fd)
                .map_err(|e| format!("{e:?}")),
            SelectionTarget::Primary => {
                request_primary_client_selection(seat, mime_type, fd).map_err(|e| format!("{e:?}"))
            }
        };
        if let Err(err) = read {
            tracing::debug!(err, "no Wayland selection to give X11");
        }
    }

    /// An X11 client took a selection: Wayland clients see it offered.
    fn new_selection(&mut self, _xwm: XwmId, selection: SelectionTarget, mime_types: Vec<String>) {
        let state = &*self;
        match selection {
            SelectionTarget::Clipboard => {
                set_data_device_selection(&state.display_handle, &state.seat, mime_types, None);
            }
            SelectionTarget::Primary => {
                set_primary_selection(&state.display_handle, &state.seat, mime_types, None);
            }
        }
    }

    fn cleared_selection(&mut self, _xwm: XwmId, selection: SelectionTarget) {
        let state = &*self;
        match selection {
            SelectionTarget::Clipboard => {
                clear_data_device_selection(&state.display_handle, &state.seat);
            }
            SelectionTarget::Primary => clear_primary_selection(&state.display_handle, &state.seat),
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
        let focused = self.model.focused();
        let window = self.x11_window(&surface).and_then(|w| self.id_of(&w));
        if window.is_some() && window == focused {
            self.focus(focused);
        }
    }
}
delegate_xwayland_shell!(State);

// The window manager is driven from the loop, whose data holds the state:
// it calls the loop's data, which passes everything on.
impl XwmHandler for App {
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
    fn resize_request(&mut self, xwm: XwmId, window: X11Surface, button: u32, edge: ResizeEdge) {
        self.state.resize_request(xwm, window, button, edge);
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
    fn property_notify(&mut self, xwm: XwmId, window: X11Surface, property: WmWindowProperty) {
        self.state.property_notify(xwm, window, property);
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

impl XWaylandShellHandler for App {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        &mut self.state.xwayland_shell_state
    }

    fn surface_associated(&mut self, xwm: XwmId, wl_surface: WlSurface, surface: X11Surface) {
        self.state.surface_associated(xwm, wl_surface, surface);
    }
}
