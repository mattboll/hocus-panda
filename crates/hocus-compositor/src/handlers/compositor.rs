//! wl_compositor, wl_shm and linux-dmabuf: surfaces and their buffers.

use smithay::{
    backend::{allocator::dmabuf::Dmabuf, renderer::utils::on_commit_buffer_handler},
    delegate_compositor, delegate_dmabuf, delegate_shm,
    reexports::wayland_server::{
        Client,
        protocol::{wl_buffer, wl_surface::WlSurface},
    },
    wayland::{
        buffer::BufferHandler,
        compositor::{
            BufferAssignment, CompositorClientState, CompositorHandler, CompositorState,
            SurfaceAttributes, get_parent, is_sync_subsurface, with_states,
        },
        dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
        shm::{ShmHandler, ShmState},
    },
};

use super::{layer_shell, xdg_shell};
use crate::{State, redraw::Change, state::ClientState};

/// For clients inserted without a [`ClientState`], which never happens;
/// but a compositor must not panic on it.
static NO_CLIENT_STATE: std::sync::LazyLock<CompositorClientState> =
    std::sync::LazyLock::new(CompositorClientState::default);

impl CompositorHandler for State {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.protocols.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        match client.get_data::<ClientState>() {
            Some(data) => &data.compositor_state,
            None => &NO_CLIENT_STATE,
        }
    }

    fn commit(&mut self, surface: &WlSurface) {
        // A window taking its buffer away: its picture first, for its
        // closing motion, while its textures are there.
        let removed = with_states(surface, |states| {
            matches!(
                states
                    .cached_state
                    .get::<SurfaceAttributes>()
                    .current()
                    .buffer,
                Some(BufferAssignment::Removed)
            )
        });
        if removed {
            self.surface_going(surface);
        }
        on_commit_buffer_handler::<Self>(surface);
        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(window) = self.window_for(&root) {
                window.on_commit();
                self.window_commits += 1;
                // An MCP agent's capture may wait for it to draw.
                self.mcp_committed(&root);
                self.perf.counters.app_commit();
                // Raining (#432): its picture is drawn again.
                if let Some(rain) = self
                    .windows
                    .values()
                    .filter(|m| m.window == window)
                    .find_map(|m| m.rain.as_ref())
                {
                    rain.stale.set(true);
                }
            }
        }
        xdg_shell::handle_commit(self, surface);
        self.popups.commit(surface);
        layer_shell::handle_commit(self, surface);
        // The screens it shows on, not every one (#826); a scene only when
        // it shows something new (#840).
        match self.commit_change(surface) {
            Change::Shown => self.redraw_for(surface),
            Change::Nothing => self.frames_owed_for(surface),
            Change::WithParent => {}
        }
    }

    /// A popup or a subsurface leaves the screen at once, without a
    /// commit to say where it was: every screen draws again. Its frame
    /// callbacks are answered.
    fn destroyed(&mut self, surface: &WlSurface) {
        self.redraw = true;
        // Its frame callbacks not yet answered are answered now: a
        // callback is gone only once answered (it has no request to
        // destroy it), and a surface that goes before its next frame (a
        // bubble, a menu) would leave one behind for as long as its client
        // runs, the shell's for the session (#827).
        let now: std::time::Duration = self.clock.now().into();
        let ms = u32::try_from(now.as_millis() % (u128::from(u32::MAX) + 1)).unwrap_or_default();
        with_states(surface, |data| {
            let mut attributes = data.cached_state.get::<SurfaceAttributes>();
            let pending = std::mem::take(&mut attributes.pending().frame_callbacks);
            let current = std::mem::take(&mut attributes.current().frame_callbacks);
            for callback in current.into_iter().chain(pending) {
                callback.done(ms);
            }
        });
    }
}

impl BufferHandler for State {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl ShmHandler for State {
    fn shm_state(&self) -> &ShmState {
        &self.protocols.shm_state
    }
}

delegate_compositor!(State);
delegate_shm!(State);

impl DmabufHandler for State {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.protocols.dmabuf_state
    }

    /// Taken as given: the global offers only what the renderer reads, and
    /// a buffer it still cannot read fails when it is drawn, showing
    /// nothing of that one surface.
    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        _dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        if notifier.successful::<State>().is_err() {
            tracing::debug!("a dmabuf's client went before it was imported");
        }
    }
}
delegate_dmabuf!(State);
