//! xdg-shell, xdg-decoration and KDE's server decoration: application
//! windows and their popups. A toplevel is mapped on its first buffer, at
//! the centre of the work area; decorations are the server's by default:
//! the grip (windows.md §7).

use std::sync::Mutex;

use smithay::{
    backend::renderer::utils::with_renderer_surface_state,
    delegate_kde_decoration, delegate_xdg_decoration, delegate_xdg_shell,
    desktop::{PopupKind, Window, find_popup_root_surface, get_popup_toplevel_coords},
    input::{Seat, pointer::GrabStartData},
    reexports::{
        wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_protocols_misc::server_decoration::server::org_kde_kwin_server_decoration::{
            Mode as KdeMode, OrgKdeKwinServerDecoration,
        },
        wayland_server::{
            Resource, WEnum,
            protocol::{wl_output, wl_seat, wl_surface::WlSurface},
        },
    },
    utils::Serial,
    wayland::{
        compositor::{add_destruction_hook, with_states},
        seat::WaylandFocus,
        shell::{
            kde::decoration::{KdeDecorationHandler, KdeDecorationState},
            xdg::{
                PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
                decoration::XdgDecorationHandler,
            },
        },
    },
};

use crate::grabs::EdgesExt as _;
use crate::{
    State,
    model::{Edges, Mode, Zone},
};

impl XdgShellHandler for State {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.protocols.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        // What a window may take, so that one that chooses its own size
        // fits the work area it opens in.
        let bounds = self.output_under_pointer().and_then(|o| self.work_area(&o));
        surface.with_pending_state(|p| {
            if let Some(area) = bounds {
                p.bounds = Some((area.width, area.height).into());
            }
        });
        crate::windows::set_states(&surface, None, Mode::Floating, false);
        // Its picture as its surface goes, for the closing motion: a
        // client that quits may take its surface before its toplevel, and
        // this hook, added before the renderer's, runs before it drops the
        // textures.
        add_destruction_hook(surface.wl_surface(), |state: &mut State, surface| {
            state.surface_going(surface);
        });
        self.unmapped.push(Window::new_wayland_window(surface));
    }

    /// The bar's desktops' list shows titles, and the tiles icons.
    fn title_changed(&mut self, _surface: ToplevelSurface) {
        self.desktops_soon();
    }

    fn app_id_changed(&mut self, _surface: ToplevelSurface) {
        self.desktops_soon();
    }

    /// A dialog of its own client's window: above it, as one through
    /// xdg-foreign (#595).
    fn parent_changed(&mut self, surface: ToplevelSurface) {
        self.parent_changed_of(surface.wl_surface());
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
        if let Some(id) = self.window_for(wl_surface).and_then(|w| self.id_of(&w)) {
            let button = start_data.button;
            self.start_move(id, button, Some(start_data), serial);
        }
    }

    /// A drag on a client-side border.
    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        seat: wl_seat::WlSeat,
        serial: Serial,
        edges: xdg_toplevel::ResizeEdge,
    ) {
        let Some(seat) = Seat::<State>::from_resource(&seat) else {
            return;
        };
        let wl_surface = surface.wl_surface();
        let Some(start_data) = check_grab(&seat, wl_surface, serial) else {
            return;
        };
        if let Some(id) = self.window_for(wl_surface).and_then(|w| self.id_of(&w)) {
            let button = start_data.button;
            self.start_resize(id, Edges::from_xdg(edges), button, Some(start_data), serial);
        }
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        match self
            .window_for(surface.wl_surface())
            .and_then(|w| self.id_of(&w))
        {
            Some(id) => {
                if self.model.mode(id) != Mode::Snapped(Zone::Maximized) {
                    self.snap(id, Zone::Maximized);
                } else if let Some(rect) = self.target_rect(id) {
                    self.configure(id, rect);
                }
            }
            // Not mapped yet: it opens maximised.
            None => self.open_as(&surface, Some(Mode::Snapped(Zone::Maximized))),
        }
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        match self
            .window_for(surface.wl_surface())
            .and_then(|w| self.id_of(&w))
        {
            Some(id) => {
                if self.model.mode(id) == Mode::Snapped(Zone::Maximized) {
                    self.restore(id);
                } else if let Some(rect) = self.target_rect(id) {
                    self.configure(id, rect);
                }
            }
            None => self.open_as_unless(&surface, Mode::Snapped(Zone::Maximized)),
        }
    }

    fn fullscreen_request(
        &mut self,
        surface: ToplevelSurface,
        _output: Option<wl_output::WlOutput>,
    ) {
        match self
            .window_for(surface.wl_surface())
            .and_then(|w| self.id_of(&w))
        {
            Some(id) => self.set_fullscreen(id, true),
            // Not mapped yet (`foot --fullscreen`): it opens fullscreen.
            None => self.open_as(&surface, Some(Mode::Fullscreen)),
        }
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        match self
            .window_for(surface.wl_surface())
            .and_then(|w| self.id_of(&w))
        {
            Some(id) => self.set_fullscreen(id, false),
            None => self.open_as_unless(&surface, Mode::Fullscreen),
        }
    }

    fn minimize_request(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self
            .window_for(surface.wl_surface())
            .and_then(|w| self.id_of(&w))
        {
            self.minimize(id);
        }
    }

    /// A menu takes the keyboard, and closes on a press elsewhere.
    fn grab(&mut self, surface: PopupSurface, seat: wl_seat::WlSeat, serial: Serial) {
        let Some(seat) = Seat::<State>::from_resource(&seat) else {
            return;
        };
        let kind = PopupKind::Xdg(surface.clone());
        let Ok(root) = find_popup_root_surface(&kind) else {
            return;
        };
        // Only for a press or a key the client just had.
        let recent = seat.get_pointer().is_some_and(|p| p.has_grab(serial))
            || seat
                .get_keyboard()
                .and_then(|k| k.last_enter())
                .is_some_and(|last| serial.is_no_older_than(&last));
        let window = self.window_for(&root).and_then(|w| self.id_of(&w));
        match window {
            Some(id) if recent => self.grab_popup(id, surface),
            _ => surface.send_popup_done(),
        }
    }

    fn popup_destroyed(&mut self, surface: PopupSurface) {
        self.popup_gone(&surface);
    }
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
        XdgDecorationHandler::new_decoration(self, toplevel);
    }
}
delegate_xdg_decoration!(State);

/// The mode a surface's KDE server decoration is in; none once the client
/// released it.
struct KdeDecoration(Mutex<Option<KdeMode>>);

/// Whether the surface's client uses KDE's server decoration protocol
/// (`org_kde_kwin_server_decoration`) and leaves its header to the
/// desktop: then it draws none, and gets the grip.
pub fn kde_server_side(surface: &WlSurface) -> bool {
    // smithay's surface data is not there once a surface is gone.
    if !surface.is_alive() {
        return false;
    }
    with_states(surface, |states| {
        states
            .data_map
            .get::<KdeDecoration>()
            .and_then(|d| d.0.lock().ok().and_then(|mode| *mode))
            == Some(KdeMode::Server)
    })
}

fn set_kde_mode(surface: &WlSurface, mode: Option<KdeMode>) {
    if !surface.is_alive() {
        return;
    }
    with_states(surface, |states| {
        let data = states
            .data_map
            .get_or_insert_threadsafe(|| KdeDecoration(Mutex::new(None)));
        if let Ok(mut current) = data.0.lock() {
            *current = mode;
        }
    });
}

/// Server side by default, as xdg-decoration: GTK 3 windows without a
/// header bar, which use this protocol and not xdg-decoration, draw no
/// title bar then. A client asking to draw its own (GTK 3 with a header
/// bar) gets that and keeps its header. The mode is said as the decoration
/// is made, as the protocol asks.
impl KdeDecorationHandler for State {
    fn kde_decoration_state(&self) -> &KdeDecorationState {
        &self.protocols.kde_decoration_state
    }

    fn new_decoration(&mut self, surface: &WlSurface, decoration: &OrgKdeKwinServerDecoration) {
        set_kde_mode(surface, Some(KdeMode::Server));
        decoration.mode(KdeMode::Server);
    }

    fn request_mode(
        &mut self,
        surface: &WlSurface,
        decoration: &OrgKdeKwinServerDecoration,
        mode: WEnum<KdeMode>,
    ) {
        // What the client asks, said back: it then knows it is the mode,
        // and asks no more (the protocol leaves the loop to the
        // compositor).
        let WEnum::Value(mode) = mode else {
            return;
        };
        set_kde_mode(surface, Some(mode));
        decoration.mode(mode);
    }

    fn release(&mut self, _decoration: &OrgKdeKwinServerDecoration, surface: &WlSurface) {
        set_kde_mode(surface, None);
    }
}
delegate_kde_decoration!(State);

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
        .enumerate()
        .find(|(_, w)| w.wl_surface().is_some_and(|s| *s == *surface))
        .map(|(index, w)| (index, w.clone()));
    if let Some((index, window)) = waiting {
        if let Some(toplevel) = window.toplevel() {
            if !toplevel.is_initial_configure_sent() {
                state.rule_size_first(&window, toplevel);
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
    if let Some(id) = state.window_for(surface).and_then(|w| state.id_of(&w)) {
        state.window_committed(id);
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
    /// A window not mapped yet asked to open maximised or fullscreen
    /// (`Some`), or as it is (`None`): its first configure (or the next,
    /// once that one went) says so with the size it will have, so that its
    /// first frame already fits, and it is placed so when it maps.
    fn open_as(&mut self, surface: &ToplevelSurface, mode: Option<Mode>) {
        use xdg_toplevel::State as S;
        let wl_surface = surface.wl_surface();
        self.opens_as
            .retain(|(s, _)| s.is_alive() && s != wl_surface);
        let output = self.output_under_pointer();
        let size = match mode {
            Some(Mode::Fullscreen) => output
                .as_ref()
                .and_then(|o| self.space.output_geometry(o))
                .map(|g| g.size),
            Some(Mode::Snapped(zone)) => output
                .as_ref()
                .and_then(|o| self.work_area(o))
                .map(|area| zone.rect(area))
                .map(|r| (r.width, r.height).into()),
            Some(Mode::Floating) | None => None,
        };
        if let Some(mode) = mode {
            self.opens_as.push((wl_surface.clone(), mode));
        }
        surface.with_pending_state(|s| {
            s.size = size;
            let fullscreen = mode == Some(Mode::Fullscreen);
            let maximized = mode == Some(Mode::Snapped(Zone::Maximized));
            for (state, on) in [(S::Fullscreen, fullscreen), (S::Maximized, maximized)] {
                if on {
                    s.states.set(state);
                } else {
                    s.states.unset(state);
                }
            }
        });
        // A request after the first configure is answered with another.
        if surface.is_initial_configure_sent() {
            surface.send_configure();
        }
    }

    /// A window's first configure: its application's rule's size (6.14,
    /// rules.rs), unless it asked to open as something already.
    fn rule_size_first(&mut self, window: &Window, toplevel: &ToplevelSurface) {
        let asked = self
            .opens_as
            .iter()
            .any(|(s, _)| s == toplevel.wl_surface());
        if asked {
            return;
        }
        if let Some(mode) = self.opening_rule(window).and_then(|r| r.size.mode()) {
            self.open_as(toplevel, Some(mode));
            return;
        }
        // A window of the last session: its saved size from its first frame
        // (windows.md §14).
        if let Some((w, h)) = self.session_first_size(window) {
            toplevel.with_pending_state(|s| s.size = Some((w, h).into()));
        }
    }

    /// A window not mapped yet no longer asks to open as `mode`.
    fn open_as_unless(&mut self, surface: &ToplevelSurface, mode: Mode) {
        let asked = self
            .opens_as
            .iter()
            .any(|(s, m)| s == surface.wl_surface() && *m == mode);
        if asked {
            self.open_as(surface, None);
        } else if surface.is_initial_configure_sent() {
            surface.send_configure();
        }
    }

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
