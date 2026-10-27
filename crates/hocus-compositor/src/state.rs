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
    /// A configuration was read and applied: what it lists is known.
    pub config_read: bool,
    /// The configuration file's text as last read and applied, for the
    /// AI's proposals to show their change as a diff (#197); `None` until
    /// then.
    pub config_text: Option<String>,
    pub config_watch: Option<ConfigWatch>,
    /// How windows are decorated, and the textures that draws with.
    pub style: Style,
    pub textures: Textures,
    /// The Matrix theme's glyphs, once per scale.
    pub matrix_sheets: crate::matrix::Sheets,
    /// What the Matrix theme's brushes drew, kept from frame to frame.
    pub matrix_layers: crate::matrix_layers::Layers,
    /// The Matrix theme's window motions after opening and closing (#154).
    pub matrix_motions: crate::matrix_windows::Motions,
    /// The Matrix theme's shell moments: desktops, toasts, column,
    /// launcher (#155).
    pub matrix_shell: crate::matrix_shell::Moments,
    /// Wallpapers that move, painted by shaders (#108).
    pub backdrops: crate::backdrop::Backdrops,
    /// Frosted glass behind panels, blurred by shaders (#37).
    pub blurs: crate::blur::Blurs,
    /// The Matrix theme's refinements on the GPU (#169).
    pub crt: crate::crt::Crt,
    pub xdg_decoration_state: XdgDecorationState,
    /// The X11 window manager, once XWayland is ready, and its display.
    pub xwm: Option<smithay::xwayland::X11Wm>,
    pub xdisplay: Option<u32>,
    pub xwayland_shell_state: smithay::wayland::xwayland_shell::XWaylandShellState,
    /// For what has to insert sources on the loop from a handler.
    pub(crate) loop_handle: Option<smithay::reexports::calloop::LoopHandle<'static, crate::App>>,
    /// Where a dragged window would snap, shown while it is dragged.
    pub snap_preview: Option<(Output, Zone)>,
    /// The widget column, when the shell runs.
    pub shell: Option<crate::shell::Shell>,
    /// Notifications as toasts, when the shell runs.
    pub toasts: Option<crate::notify::ToastPanel>,
    /// The launcher, when the shell runs.
    pub launcher: Option<crate::launcher::LauncherPanel>,
    /// Quick settings, when the shell runs.
    pub quick: Option<crate::quick::QuickPanel>,
    /// Wallpapers, when the shell runs.
    pub wallpapers: Option<crate::wallpaper::Wallpapers>,
    /// The user's clock: the system's, or a fixed one under the virtual
    /// clock of tests.
    pub wall: crate::shell::WallClock,
    /// The session saved and put back (#31).
    pub sessions: crate::session::Sessions,
    /// Idle, the curtain and the lock (#29).
    pub locking: crate::lock::Locking,
    /// Third-party idle notifications (ext-idle-notify).
    pub idle_notifications: Vec<
        smithay::reexports::wayland_protocols::ext::idle_notify::v1::server::ext_idle_notification_v1::ExtIdleNotificationV1,
    >,
    /// Screenshot tools and screen sharing (#114).
    pub screen_capture: crate::screen_capture::ScreenCapture,
    /// The common batch of protocols (#32).
    pub protocols: crate::handlers::protocols::Protocols,
    /// A system bus to read battery, network… from instead of the
    /// machine's.
    pub system_bus: Option<String>,
    /// Running as the session itself (the DRM backend, #27), not nested
    /// nor headless: the machine's services are ours to use.
    pub is_session: bool,
    /// drm-lease: VR headsets lent to clients (#123), on a TTY.
    pub leases: crate::leases::Leases,
    /// Switches virtual terminals, on a TTY.
    pub vt_switch: Option<smithay::backend::session::libseat::LibSeatSession>,
    /// The clipboard history and the emoji picker (#71).
    pub clipboard: crate::clipboard::Clipboard,
    pub tray: crate::tray::TrayState,
    /// The screenshot tool (#69).
    pub shots: crate::screenshot::Screenshots,
    pub recorder: crate::recording::Recorder,
    /// System updates (#84).
    pub updates: crate::updates::Updates,
    /// The phone, through KDE Connect (#85).
    pub phone: crate::phone::PhoneState,
    pub calendar: crate::calendar::Calendar,
    /// Questions to the AI (#44).
    pub ai: crate::ai::Ai,
    /// Document search (#45).
    pub search: crate::search::SearchState,
    /// Notes and to-dos (#79).
    pub notes: crate::notes::NotesState,
    /// Focus mode, the magnifier, night light (#78).
    pub comfort: crate::comfort::Comfort,
    /// Gamma each screen should get, for backends that set it.
    pub gamma_wanted: Vec<(smithay::output::Output, [f32; 3])>,
    /// The portal's chooser waiting for the user (#116).
    pub share: Option<crate::share::Picking>,
    /// polkit's password requests and their dialog (#83).
    pub authorizer: crate::authorize::Authorizer,
    /// Leaving the session: its card, what the machine can do (#72).
    pub leaver: crate::leave::Leaver,
    /// Each screen's name card while "identify" shows them.
    pub identifying: Vec<(String, crate::shell::Panel<hocus_shell::identify::Identify>)>,
    /// When they go.
    pub identifying_until: Option<std::time::Duration>,
    /// What screens are set to, and what was asked (#28).
    pub output_settings: crate::outputs::Settings,
    /// Screens showing another's picture (#125).
    pub mirrors: Vec<crate::mirror::Mirror>,
    /// Windows appear and go at once: tests that do not sample motion.
    pub instant_motion: bool,
    /// Pictures of windows closing, minimizing, maximizing (#34).
    pub snapshots: crate::snapshot::Snapshots,
    /// Hot corners, Super alone, gestures (#86).
    pub triggers: crate::corners::Triggers,
    /// Drawing on the screen (#86).
    pub ink: crate::ink::Inking,
    pub circles: crate::circle::Circles,
    /// The window menu at the pointer (#40).
    pub radials: crate::radial::Radials,
    /// Wobbly windows and flicks (#35).
    pub wobbles: crate::wobble::Wobbles,
    pub pandas: crate::pandas::Pandas,
    /// Coding agents (#41).
    pub agents: crate::agents::Agents,
    /// What goes on on the desktops not shown (#192).
    pub activity: crate::activity::Activity,
    /// What MCP agents did (#176).
    pub journal: crate::journal::Journal,
    /// MCP agents' own seats (#43).
    pub seats: crate::seat::Seats,
    /// Applications' accessibility trees, for agents (#175).
    pub a11y: crate::a11y::A11y,
    /// The first session's welcome (#82).
    pub first_run: crate::first_run::FirstRun,
    /// Desktops sliding, screen by screen (#36).
    pub slides: crate::slide::Slides,
    /// The window switcher (#39).
    pub alt_tab: crate::alt_tab::AltTab,
    /// The discreet dock (#38).
    pub dock: crate::dock::Dock,
    /// wlr-output-management and output-power, for third-party tools.
    pub output_management: crate::handlers::output_management::OutputManagement,
    /// linux-dmabuf, whose global the TTY backend makes for its GPU.
    pub dmabuf_state: smithay::wayland::dmabuf::DmabufState,
    /// The configuration file, when there is one.
    pub config_path: Option<std::path::PathBuf>,
    /// When the settings app was started, until its window comes.
    pub settings_started: Option<std::time::Duration>,
    /// Title bars, when they are on.
    pub bars: crate::bars::Bars,
    /// The drop-down terminal.
    pub dropdown: crate::dropdown::Dropdown,
    /// The theme the shell was last given.
    pub applied_theme: Option<hocus_ui::Theme>,
    /// The theme in effect as last worked out, and what from: asked for
    /// after every turn of the loop, worked out again only when that
    /// changes (#88).
    pub theme_memo: std::cell::RefCell<Option<(u64, hocus_ui::Theme)>>,
    /// What started applications inherit, if not the compositor's own
    /// environment (tests).
    pub child_env: Option<Vec<(std::ffi::OsString, std::ffi::OsString)>>,
    /// The keyboard layout in use, by its place in the configuration.
    pub layout_index: usize,
    /// The names of the layouts kept, from XKB's registry.
    pub layout_names: crate::layout_names::LayoutNames,
    /// Who is logged in, as the column shows it.
    pub user: String,
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
            config_read: false,
            config_text: None,
            config_watch: None,
            style: Style::default(),
            textures: Textures::default(),
            matrix_sheets: Default::default(),
            matrix_layers: Default::default(),
            matrix_motions: Default::default(),
            matrix_shell: Default::default(),
            backdrops: Default::default(),
            blurs: Default::default(),
            crt: Default::default(),
            xdg_decoration_state: XdgDecorationState::new::<Self>(dh),
            xwm: None,
            xdisplay: None,
            xwayland_shell_state: smithay::wayland::xwayland_shell::XWaylandShellState::new::<Self>(
                dh,
            ),
            loop_handle: None,
            snap_preview: None,
            shell: None,
            toasts: None,
            launcher: None,
            quick: None,
            wallpapers: None,
            applied_theme: None,
            theme_memo: Default::default(),
            dropdown: Default::default(),
            bars: Default::default(),
            config_path: None,
            system_bus: None,
            is_session: false,
            leases: Default::default(),
            vt_switch: None,
            output_settings: Default::default(),
            mirrors: Vec::new(),
            instant_motion: false,
            snapshots: Default::default(),
            triggers: Default::default(),
            ink: Default::default(),
            circles: Default::default(),
            radials: Default::default(),
            wobbles: Default::default(),
            pandas: Default::default(),
            agents: Default::default(),
            activity: Default::default(),
            journal: Default::default(),
            seats: Default::default(),
            a11y: Default::default(),
            first_run: Default::default(),
            slides: Default::default(),
            alt_tab: Default::default(),
            dock: Default::default(),
            identifying: Vec::new(),
            identifying_until: None,
            leaver: Default::default(),
            authorizer: Default::default(),
            shots: Default::default(),
            recorder: Default::default(),
            comfort: Default::default(),
            notes: Default::default(),
            updates: Default::default(),
            phone: Default::default(),
            calendar: Default::default(),
            ai: Default::default(),
            search: Default::default(),
            gamma_wanted: Vec::new(),
            share: None,
            clipboard: Default::default(),
            tray: Default::default(),
            output_management: crate::handlers::output_management::OutputManagement::new(dh),
            dmabuf_state: smithay::wayland::dmabuf::DmabufState::new(),
            settings_started: None,
            child_env: None,
            layout_index: 0,
            layout_names: Default::default(),
            // Tests show the same name on every machine.
            user: match clock {
                Clock::Virtual { .. } => "Alex".into(),
                Clock::Monotonic { .. } => std::env::var("USER")
                    .or_else(|_| std::env::var("LOGNAME"))
                    .unwrap_or_default(),
            },
            snap_preview_buffer: SolidColorBuffer::default(),
            cursor: CursorImageStatus::default_named(),
            arrows: Arrows::default(),
            // Sandboxed clients do not see the list of windows.
            foreign_toplevels: ForeignToplevelListState::new_with_filter::<Self>(
                dh,
                crate::handlers::protocols::trusted,
            ),
            protocols: crate::handlers::protocols::Protocols::new(dh),
            screen_capture: Default::default(),
            locking: crate::lock::Locking::new(dh),
            sessions: Default::default(),
            wall: crate::shell::WallClock::new(match clock {
                Clock::Monotonic { .. } => crate::shell::WallSource::System,
                Clock::Virtual { .. } => crate::shell::WallSource::Virtual,
            }),
            idle_notifications: Vec::new(),
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
    /// The loop's handle, to wake it on real time when something falls
    /// due; none on a virtual clock, which needs no waking (see
    /// [`Clock::is_virtual`]).
    pub(crate) fn waker(
        &self,
    ) -> Option<&smithay::reexports::calloop::LoopHandle<'static, crate::App>> {
        self.loop_handle
            .as_ref()
            .filter(|_| !self.clock.is_virtual())
    }

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
        use smithay::wayland::shell::wlr_layer::Layer;
        let fullscreen = self
            .space
            .output_under(pos)
            .next()
            .is_some_and(|o| self.fullscreen_on(o));
        let layer = |layers: &[Layer]| {
            self.layer_surface_under(pos, layers)
                .map(|(s, p, _)| (s, p))
        };
        let above: &[Layer] = if fullscreen {
            &[Layer::Overlay]
        } else {
            &[Layer::Overlay, Layer::Top]
        };
        layer(above)
            .or_else(|| {
                self.space
                    .element_under(pos)
                    .and_then(|(window, location)| {
                        window
                            .surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)
                            .map(|(s, p)| (s, (p + location).to_f64()))
                    })
            })
            .or_else(|| layer(&[Layer::Bottom, Layer::Background]))
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
    /// The sandbox it connected from (security-context), if any.
    pub security_context: Option<smithay::wayland::security_context::SecurityContext>,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}
