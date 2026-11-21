//! XWayland's surfaces by serial, let go once their X11 windows are gone
//! (#827).
//!
//! XWayland gives each X11 window it shows a Wayland surface of its own,
//! and pairs the two by a serial sent on both sides. smithay 0.7's
//! xwayland shell keeps each such surface in a map by its serial, to pair
//! it when the window's side comes second, and never takes one out: every
//! X11 window, menu and tooltip ever shown kept its surface's state
//! (about 5 KB) for the session, XWayland's connection staying open.
//! smithay's main branch takes a surface out as it goes, in no release
//! yet; until one, the map is emptied here, by going back to the state as
//! it was made (the same global).
//!
//! Emptied only when no surface waits for its window: a surface already
//! paired is not looked up again, and smithay puts a live one back at its
//! next commit. One that waits would never be paired if its window's
//! serial came after.

use std::collections::HashSet;

use smithay::{
    reexports::wayland_server::{Resource as _, Weak, protocol::wl_surface::WlSurface},
    wayland::{
        compositor::{get_role, with_states},
        xwayland_shell::{XWAYLAND_SHELL_ROLE, XWaylandShellCachedState, XWaylandShellState},
    },
};

/// Marks a surface of XWayland paired with its X11 window.
struct Paired;

/// The xwayland shell's state, and what it keeps.
#[derive(Debug)]
pub struct XWaylandShell {
    /// What smithay's handlers are given.
    pub state: XWaylandShellState,
    /// The state as it was made, its map empty: the same global.
    made: XWaylandShellState,
    /// The serials of the surfaces committed since the map was last
    /// emptied: those it may hold.
    held: HashSet<u64>,
    /// Surfaces with a serial, not paired with their window yet.
    waiting: Vec<Weak<WlSurface>>,
    /// Surfaces with a serial gone since the map was last emptied.
    gone: usize,
}

impl XWaylandShell {
    pub fn new(state: XWaylandShellState) -> Self {
        Self {
            made: state.clone(),
            state,
            held: HashSet::new(),
            waiting: Vec::new(),
            gone: 0,
        }
    }

    /// The surfaces the map holds, live or gone (tests).
    pub fn held(&self) -> usize {
        self.held
            .iter()
            .filter(|serial| self.state.surface_for_serial(**serial).is_some())
            .count()
    }

    /// The map, emptied if no surface waits for its window.
    fn let_go(&mut self) {
        if self.gone == 0 {
            return;
        }
        self.waiting.retain(|surface| {
            surface
                .upgrade()
                .is_ok_and(|surface| surface.is_alive() && !is_paired(&surface))
        });
        if !self.waiting.is_empty() {
            return;
        }
        tracing::debug!(
            gone = self.gone,
            held = self.held.len(),
            "XWayland's surfaces by serial let go"
        );
        self.state = self.made.clone();
        self.held.clear();
        self.gone = 0;
    }
}

/// The serial of a surface of XWayland, once it has one.
fn serial_of(surface: &WlSurface) -> Option<u64> {
    if get_role(surface) != Some(XWAYLAND_SHELL_ROLE) {
        return None;
    }
    with_states(surface, |states| {
        states
            .cached_state
            .get::<XWaylandShellCachedState>()
            .current()
            .serial
    })
}

fn is_paired(surface: &WlSurface) -> bool {
    with_states(surface, |states| states.data_map.get::<Paired>().is_some())
}

impl crate::State {
    /// A surface committed: one of XWayland's with a serial is in the map
    /// now (smithay's hook ran first), and waits if not paired yet.
    pub(crate) fn xwayland_surface_committed(&mut self, surface: &WlSurface) {
        let Some(serial) = serial_of(surface) else {
            return;
        };
        let shell = &mut self.protocols.xwayland_shell;
        shell.held.insert(serial);
        if !is_paired(surface) && !shell.waiting.iter().any(|w| w.id() == surface.id()) {
            shell.waiting.push(surface.downgrade());
        }
    }

    /// One of XWayland's surfaces is paired with its X11 window.
    pub(crate) fn xwayland_surface_paired(&mut self, surface: &WlSurface) {
        with_states(surface, |states| {
            states.data_map.insert_if_missing_threadsafe(|| Paired);
        });
        self.protocols.xwayland_shell.let_go();
    }

    /// A surface went: if it was one of XWayland's with a serial, the map
    /// lets go of it and the others gone.
    pub(crate) fn xwayland_surface_destroyed(&mut self, surface: &WlSurface) {
        if serial_of(surface).is_none() {
            return;
        }
        let shell = &mut self.protocols.xwayland_shell;
        shell.gone += 1;
        shell.let_go();
    }
}
