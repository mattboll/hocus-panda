//! What a window can be asked, whatever its client speaks: xdg-shell for
//! Wayland applications, X11 through XWayland.

use smithay::{
    desktop::{Window, WindowSurface},
    reexports::wayland_protocols::xdg::{
        decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
        shell::server::xdg_toplevel,
    },
    utils::{Logical, Rectangle},
    wayland::{compositor::with_states, shell::xdg::XdgToplevelSurfaceData},
};

/// How a window is shown, as far as its client is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    Normal,
    Maximized,
    Fullscreen,
}

/// What the client says its window is: title and app id (the X11 class).
pub fn title_and_app_id(window: &Window) -> (Option<String>, Option<String>) {
    match window.underlying_surface() {
        WindowSurface::Wayland(toplevel) => with_states(toplevel.wl_surface(), |states| {
            states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .and_then(|data| data.lock().ok())
                .map(|data| (data.title.clone(), data.app_id.clone()))
                .unwrap_or_default()
        }),
        WindowSurface::X11(x11) => {
            let non_empty = |s: String| (!s.is_empty()).then_some(s);
            (non_empty(x11.title()), non_empty(x11.class()))
        }
    }
}

/// Feeds a window's title and app id to `hasher`, as
/// [`title_and_app_id`] reads them, without copying them.
pub fn hash_title_and_app_id(window: &Window, hasher: &mut impl std::hash::Hasher) {
    use std::hash::Hash;
    match window.underlying_surface() {
        WindowSurface::Wayland(toplevel) => with_states(toplevel.wl_surface(), |states| {
            if let Some(data) = states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .and_then(|data| data.lock().ok())
            {
                (&data.title, &data.app_id).hash(hasher);
            }
        }),
        WindowSurface::X11(_) => title_and_app_id(window).hash(hasher),
    }
}

/// Asks the client to show its window at `rect`'s size, in `shown` mode;
/// `resizing` while an interactive resize is under way.
pub fn request_size(window: &Window, rect: Rectangle<i32, Logical>, shown: Shown, resizing: bool) {
    match window.underlying_surface() {
        WindowSurface::Wayland(toplevel) => {
            toplevel.with_pending_state(|pending| {
                pending.size = Some(rect.size);
                let flags = [
                    (xdg_toplevel::State::Maximized, shown == Shown::Maximized),
                    (xdg_toplevel::State::Fullscreen, shown == Shown::Fullscreen),
                    (xdg_toplevel::State::Resizing, resizing),
                ];
                for (state, on) in flags {
                    if on {
                        pending.states.set(state);
                    } else {
                        pending.states.unset(state);
                    }
                }
            });
            toplevel.send_pending_configure();
        }
        WindowSurface::X11(x11) => {
            // X11 windows are placed by the window manager: position and size.
            if let Err(err) = x11.configure(Some(rect)) {
                tracing::debug!(?err, "could not configure an X11 window");
            }
            let _ = x11.set_maximized(shown == Shown::Maximized);
            let _ = x11.set_fullscreen(shown == Shown::Fullscreen);
        }
    }
}

/// Tells the client whether its window is the focused one.
pub fn activate(window: &Window, focused: bool) {
    window.set_activated(focused);
    match window.underlying_surface() {
        WindowSurface::Wayland(toplevel) => {
            toplevel.send_pending_configure();
        }
        WindowSurface::X11(x11) => {
            let _ = x11.set_activated(focused);
        }
    }
}

/// Asks the client to close its window, as its close button would.
pub fn close(window: &Window) {
    match window.underlying_surface() {
        WindowSurface::Wayland(toplevel) => toplevel.send_close(),
        WindowSurface::X11(x11) => {
            if let Err(err) = x11.close() {
                tracing::debug!(%err, "could not close an X11 window");
            }
        }
    }
}

/// Whether the compositor draws the window's frame: Wayland windows that
/// asked for server-side decorations, and every managed X11 window.
pub fn server_side(window: &Window) -> bool {
    match window.underlying_surface() {
        WindowSurface::Wayland(toplevel) => {
            toplevel.current_state().decoration_mode == Some(DecorationMode::ServerSide)
        }
        WindowSurface::X11(x11) => !x11.is_override_redirect(),
    }
}

/// What the client asked for before the window was shown.
pub fn wanted(window: &Window) -> Shown {
    match window.underlying_surface() {
        WindowSurface::Wayland(toplevel) => {
            let states = toplevel.with_pending_state(|p| p.states.clone());
            if states.contains(xdg_toplevel::State::Fullscreen) {
                Shown::Fullscreen
            } else if states.contains(xdg_toplevel::State::Maximized) {
                Shown::Maximized
            } else {
                Shown::Normal
            }
        }
        WindowSurface::X11(x11) if x11.is_fullscreen() => Shown::Fullscreen,
        WindowSurface::X11(x11) if x11.is_maximized() => Shown::Maximized,
        WindowSurface::X11(_) => Shown::Normal,
    }
}
