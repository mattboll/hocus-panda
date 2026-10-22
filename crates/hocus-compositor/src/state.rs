use std::{collections::BTreeMap, path::PathBuf};

use anyhow::Context;
use smithay::{
    desktop::{PopupManager, Space, Window, WindowSurfaceType},
    input::{Seat, SeatState, pointer::CursorImageStatus},
    reexports::{
        calloop::{LoopSignal, channel::Sender},
        wayland_server::{
            DisplayHandle,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::wl_surface::WlSurface,
        },
    },
    utils::{Logical, Point, Serial},
    wayland::seat::WaylandFocus as _,
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        foreign_toplevel_list::ForeignToplevelListState,
        output::OutputManagerState,
        selection::data_device::DataDeviceState,
        shell::xdg::{XdgShellState, decoration::XdgDecorationState},
        shm::ShmState,
    },
};

use hocus_core::{Config, Model, Shortcuts, WindowId, Zone};
use smithay::{backend::renderer::element::solid::SolidColorBuffer, output::Output};

use crate::{
    Clock,
    api::EncodedScreenshot,
    config_watch::ConfigWatch,
    cursor::Arrows,
    decoration::{Style, Textures},
    desktops::{Managed, Places},
    handlers::WorkspaceState,
    stats::Measures,
    watchdog::Watchdog,
};

/// Everything the compositor knows, owned by the event loop.
#[derive(Debug)]
pub struct State {
    pub clock: Clock,
    pub display_handle: DisplayHandle,
    pub socket_path: PathBuf,
    pub loop_signal: LoopSignal,
    pub api: hocus_api::Server,
    /// Where screenshot workers send their PNG.
    pub screenshots: Sender<EncodedScreenshot>,
    /// Outputs, their desktops, their windows, and focus.
    pub model: Model,
    /// The windows of the model; those on hidden desktops are not in the
    /// space.
    pub windows: BTreeMap<WindowId, Managed>,
    /// The configuration in effect, and why the last file read was refused.
    pub config: Config,
    pub config_error: Option<String>,
    /// What the last configuration applied left out, and why.
    pub config_warnings: Vec<String>,
    pub shortcuts: Shortcuts,
    /// Counts configuration reads, applied or refused.
    pub config_generation: u64,
    pub config_watch: Option<ConfigWatch>,
    /// How windows are decorated, and the textures that draws with.
    pub style: Style,
    pub textures: Textures,
    pub xdg_decoration_state: XdgDecorationState,
    /// The X11 window manager, once XWayland is ready, and its display.
    pub xwm: Option<smithay::xwayland::X11Wm>,
    pub xdisplay: Option<u32>,
    pub xwayland_shell_state: smithay::wayland::xwayland_shell::XWaylandShellState,
    /// For what has to insert sources on the loop from a handler.
    pub(crate) loop_handle: Option<smithay::reexports::calloop::LoopHandle<'static, crate::App>>,
    /// Where a dragged window would snap, shown while it is dragged.
    pub snap_preview: Option<(Output, Zone)>,
    pub snap_preview_buffer: SolidColorBuffer,
    /// What the pointer looks like, as clients set it.
    pub cursor: CursorImageStatus,
    pub arrows: Arrows,
    /// Opacity set with Super+wheel, per app id.
    pub app_opacity: std::collections::HashMap<String, f32>,
    /// Where windows were, per layout of outputs.
    pub places: Places,
    /// ext-foreign-toplevel-list: windows for taskbars and switchers.
    pub foreign_toplevels: ForeignToplevelListState,
    /// ext-workspace: outputs as groups, desktops as workspaces.
    pub workspaces: WorkspaceState,
    pub measures: Measures,
    /// Stops with the state.
    pub watchdog: Option<Watchdog>,

    pub space: Space<Window>,
    pub popups: PopupManager,
    /// Windows whose size is not known yet: they are placed on their first buffer.
    pub unplaced: Vec<Window>,
    /// Something changed since the last frame.
    pub needs_redraw: bool,

    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<State>,
    pub data_device_state: DataDeviceState,
    pub seat: Seat<State>,
}

impl State {
    pub fn new(
        dh: &DisplayHandle,
        loop_signal: LoopSignal,
        socket_path: PathBuf,
        clock: Clock,
        api: hocus_api::Server,
        screenshots: Sender<EncodedScreenshot>,
    ) -> anyhow::Result<Self> {
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(dh, "seat0");
        // The layout comes from XKB_DEFAULT_* for now; configuration comes later.
        seat.add_keyboard(Default::default(), 200, 25)
            .context("could not set up the keyboard")?;
        seat.add_pointer();

        Ok(Self {
            clock,
            display_handle: dh.clone(),
            socket_path,
            loop_signal,
            api,
            screenshots,
            model: Model::new(),
            windows: BTreeMap::new(),
            places: Places::new(),
            app_opacity: Default::default(),
            config: Config::default(),
            config_error: None,
            config_warnings: Vec::new(),
            shortcuts: Config::default().shortcuts(),
            config_generation: 0,
            config_watch: None,
            style: Style::default(),
            textures: Textures::default(),
            xdg_decoration_state: XdgDecorationState::new::<Self>(dh),
            xwm: None,
            xdisplay: None,
            xwayland_shell_state: smithay::wayland::xwayland_shell::XWaylandShellState::new::<Self>(
                dh,
            ),
            loop_handle: None,
            snap_preview: None,
            snap_preview_buffer: SolidColorBuffer::default(),
            cursor: CursorImageStatus::default_named(),
            arrows: Arrows::default(),
            foreign_toplevels: ForeignToplevelListState::new::<Self>(dh),
            workspaces: WorkspaceState::new(dh),
            measures: Measures::default(),
            watchdog: None,
            space: Space::default(),
            popups: PopupManager::default(),
            unplaced: Vec::new(),
            needs_redraw: true,
            compositor_state: CompositorState::new::<Self>(dh),
            xdg_shell_state: XdgShellState::new::<Self>(dh),
            shm_state: ShmState::new::<Self>(dh, vec![]),
            output_manager_state: OutputManagerState::new_with_xdg_output::<Self>(dh),
            seat_state,
            data_device_state: DataDeviceState::new::<Self>(dh),
            seat,
        })
    }

    /// The window whose toplevel surface is `surface`: managed, shown or
    /// not, or waiting for its first buffer.
    pub fn window_for(&self, surface: &WlSurface) -> Option<Window> {
        self.windows
            .values()
            .map(|m| &m.window)
            .chain(self.unplaced.iter())
            .find(|w| w.wl_surface().is_some_and(|s| *s == *surface))
            .cloned()
    }

    /// The surface under `pos`, and where its origin is.
    pub fn surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        self.space
            .element_under(pos)
            .and_then(|(window, location)| {
                window
                    .surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)
                    .map(|(s, p)| (s, (p + location).to_f64()))
            })
    }

    /// Manages the windows that just got their first buffer, on the output
    /// under the pointer.
    pub fn place_new_windows(&mut self, serial: Serial) {
        let Some(output) = self.output_under_pointer() else {
            return;
        };
        let (ready, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.unplaced)
            .into_iter()
            .partition(|w| !w.geometry().size.is_empty());
        self.unplaced = waiting;
        for window in ready {
            self.manage(window, &output, serial);
        }
    }
}

/// What the compositor keeps per client.
#[derive(Debug, Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}
