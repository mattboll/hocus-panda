//! Everything the compositor knows, owned by the event loop, and what it
//! does with windows: map them where the model says, focus them, unmap
//! them.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    time::Duration,
};

use anyhow::Context;
use smithay::{
    backend::{
        renderer::element::Id,
        session::{Session, libseat::LibSeatSession},
    },
    desktop::{PopupManager, Space, Window, WindowSurfaceType, layer_map_for_output},
    input::{
        Seat, SeatState,
        keyboard::{Keycode, XkbConfig},
        pointer::{CursorIcon, CursorImageStatus},
    },
    output::Output,
    reexports::{
        calloop::{LoopHandle, LoopSignal, RegistrationToken},
        wayland_server::{
            DisplayHandle,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::wl_surface::WlSurface,
        },
    },
    utils::{Clock, Logical, Monotonic, Point, Rectangle},
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        cursor_shape::CursorShapeManagerState,
        dmabuf::{DmabufGlobal, DmabufState},
        fractional_scale::FractionalScaleManagerState,
        output::OutputManagerState,
        pointer_gestures::PointerGesturesState,
        presentation::PresentationState,
        relative_pointer::RelativePointerManagerState,
        seat::WaylandFocus,
        selection::{data_device::DataDeviceState, primary_selection::PrimarySelectionState},
        shell::{
            wlr_layer::WlrLayerShellState,
            xdg::{XdgShellState, decoration::XdgDecorationState},
        },
        shm::ShmState,
        viewporter::ViewporterState,
        xdg_activation::XdgActivationState,
        xwayland_shell::XWaylandShellState,
    },
    xwayland::X11Wm,
};

use crate::{
    App, Config,
    cursor::CursorImages,
    decoration::Textures,
    desktops::Strip,
    model::{self, Model, Rect, WindowId},
    motion::Tween,
    overlay::Overlay,
    popups::PopupGrab,
    programs::Programs,
    shortcuts::Ending,
    snapshot::{Grabber, Snapshot},
    switcher::Switcher,
    windows::{Leaving, Managed, Preview, rect_of},
    wobble::Jelly,
};

/// Everything the compositor knows, owned by the event loop.
#[derive(Debug)]
pub struct State {
    pub display_handle: DisplayHandle,
    pub loop_handle: LoopHandle<'static, App>,
    pub loop_signal: LoopSignal,
    /// CLOCK_MONOTONIC, which frame callbacks and presentation times use.
    pub clock: Clock<Monotonic>,
    pub socket_path: PathBuf,
    pub runtime_dir: PathBuf,
    pub config: Config,
    /// In a window of another session: the desktop's shortcuts take
    /// `Alt+Super`, so that the host's own `Super` ones keep working
    /// (docs/design/windows.md §13).
    pub nested: bool,

    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub xdg_decoration_state: XdgDecorationState,
    pub layer_shell_state: WlrLayerShellState,
    pub shm_state: ShmState,
    pub dmabuf_state: DmabufState,
    /// The backend's linux-dmabuf global, made once it knows its renderer.
    pub dmabuf_global: Option<DmabufGlobal>,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<State>,
    pub data_device_state: DataDeviceState,
    pub primary_selection_state: PrimarySelectionState,
    pub activation_state: XdgActivationState,
    pub fractional_scale_state: FractionalScaleManagerState,
    pub presentation_state: PresentationState,
    pub xwayland_shell_state: XWaylandShellState,
    _viewporter: ViewporterState,
    _cursor_shape: CursorShapeManagerState,
    _pointer_gestures: PointerGesturesState,
    _relative_pointer: RelativePointerManagerState,

    pub seat: Seat<State>,
    pub space: Space<Window>,
    pub popups: PopupManager,
    /// The windows, apart from smithay.
    pub model: Model,
    /// The mapped windows (and the minimised ones), by their id in the
    /// model, with their motions.
    pub windows: BTreeMap<WindowId, Managed>,
    /// Toplevels whose first buffer has not come: mapped once it does.
    pub unmapped: Vec<Window>,
    /// Activation tokens handed out, when, and whether they may focus a
    /// window (made from the user's input, or by the desktop).
    pub activation_tokens: HashMap<String, (Duration, bool)>,
    /// The user started a program through the desktop then: its window
    /// takes the focus.
    pub spawned_at: Option<Duration>,
    /// Windows not mapped yet that asked for the focus with a valid token.
    pub activate_on_map: Vec<WlSurface>,
    /// Windows not mapped yet that asked to open maximised or fullscreen
    /// (§10): they show so from their first frame.
    pub opens_as: Vec<(WlSurface, model::Mode)>,
    /// What draws windows' frames, and their pictures as they leave.
    pub textures: Textures,
    pub grabber: Grabber,
    /// Windows' pictures taken as their surfaces went.
    pub pictures: HashMap<WindowId, Snapshot>,
    pub leaving: Vec<Leaving>,
    /// Where a dragged window would snap, and the one fading out.
    pub preview: Option<Preview>,
    pub preview_out: Option<(Rect, Tween)>,
    pub preview_ids: [Id; 9],
    /// The handle shown on the seam of a pair near the pointer,
    /// fading like a grip (§8).
    pub seam_handle: Option<(model::Seam, crate::windows::Grip)>,
    pub seam_handle_id: Id,
    /// The window moving, and the one wobbling.
    pub moving: Option<WindowId>,
    pub wobble: Option<(WindowId, Jelly)>,
    /// The cursor over a window's frame or grip, or during a grab: it
    /// wins over the client's.
    pub frame_cursor: Option<CursorIcon>,
    /// The last press on a grip, for a double click.
    pub last_grip_press: Option<(WindowId, Duration)>,
    /// Buttons whose press the desktop kept: their release too.
    pub kept_buttons: HashSet<u32>,
    pub popup_grab: Option<PopupGrab>,
    /// A frame is asked for at this time, after a pause.
    pub wake: Option<Duration>,
    /// When the last frame was drawn: motions are judged as of then (see
    /// `tick`). Set while drawing, which borrows the state shared.
    pub drawn_at: std::cell::Cell<Option<Duration>>,
    /// What the pointer shows, as the client under it sets it.
    pub cursor: CursorImageStatus,
    pub cursor_images: CursorImages,
    /// Something changed on screen since the last frame.
    pub redraw: bool,
    /// Each screen's desktops, by output name (§12).
    pub strips: HashMap<String, Strip>,
    /// The screen whose strip follows three fingers on the touchpad.
    pub swiping: Option<Output>,
    /// Alt+Tab, while it is held, and its panel vanishing (§11).
    pub switcher: Option<Switcher>,
    pub switcher_out: Option<(Switcher, Tween)>,
    /// The switcher's key held down, stepping at the keyboard's repeat
    /// delay and rate: its keycode and its timer (§11).
    pub switcher_repeat: Option<(Keycode, RegistrationToken)>,
    /// A level key held down (volume, brightness), and the presses kept
    /// for the shell while it is away.
    pub levels: crate::levels::Levels,
    /// What draws the switcher, the desktops' dots and their text.
    pub overlay: Overlay,

    /// The X11 window manager once XWayland is ready, and its display.
    pub xwm: Option<X11Wm>,
    pub xdisplay: Option<u32>,
    pub programs: Programs,
    /// The bars' places and forms (docs/design/bar.md §2, §3).
    pub panel: crate::panel::Panel,
    /// The shell drawing them.
    pub shell: crate::shell::Shell,
    /// The render node of the GPU the compositor draws with, which the
    /// shell draws with too; `None` without one.
    pub gpu: Option<crate::backend::DevId>,
    /// The renderer can blur what is behind the bars (GLES).
    pub blur: bool,
    /// The glass behind each bar, as last blurred.
    pub panes: std::cell::RefCell<crate::glass::Panes>,
    /// The desktop's background, by screen size in pixels.
    pub backdrops: std::cell::RefCell<
        HashMap<(i32, i32), smithay::backend::renderer::element::memory::MemoryRenderBuffer>,
    >,
    /// The seat's session on a virtual terminal, which switches terminals.
    pub session: Option<LibSeatSession>,
    /// Keys whose press was a shortcut: their release is kept from the
    /// clients too.
    pub kept_keys: HashSet<Keycode>,
    /// Set once the compositor is asked to end, and why.
    pub ending: Option<Ending>,
}

impl State {
    pub fn new(
        dh: &DisplayHandle,
        loop_handle: LoopHandle<'static, App>,
        loop_signal: LoopSignal,
        socket_path: PathBuf,
        runtime_dir: PathBuf,
        config: Config,
        nested: bool,
    ) -> anyhow::Result<Self> {
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(dh, "seat0");
        let keyboard = &config.keyboard;
        let names = config.xkb_names();
        tracing::info!(
            model = names.model,
            layout = names.layout,
            variant = names.variant,
            options = names.options,
            from = names.from,
            "keyboard"
        );
        let xkb = XkbConfig {
            model: &names.model,
            layout: &names.layout,
            variant: &names.variant,
            options: (!names.options.is_empty()).then(|| names.options.clone()),
            ..XkbConfig::default()
        };
        let added = seat.add_keyboard(xkb, keyboard.repeat_delay, keyboard.repeat_rate);
        if let Err(err) = added {
            // A layout XKB does not know: its defaults, rather than no
            // keyboard at all.
            tracing::warn!(
                ?err,
                layout = names.layout,
                "keyboard layouts refused; XKB's defaults"
            );
            seat.add_keyboard(
                XkbConfig::default(),
                keyboard.repeat_delay,
                keyboard.repeat_rate,
            )
            .context("could not set up the keyboard")?;
        }
        seat.add_pointer();
        crate::overlay::load_font(&loop_handle);
        let appearance = &config.appearance;
        let cursor_images = CursorImages::start(
            &loop_handle,
            crate::cursor::Choice::of(
                appearance.cursor_theme.as_deref(),
                appearance.cursor_size,
                &config.cursor_env,
            ),
            config.cursor_env.search.clone(),
        );
        let primary_selection_state = PrimarySelectionState::new::<Self>(dh);
        let panel = crate::panel::Panel::load(config.state_dir.as_deref());

        Ok(Self {
            display_handle: dh.clone(),
            loop_handle,
            loop_signal,
            clock: Clock::new(),
            socket_path,
            runtime_dir,
            config,
            nested,
            compositor_state: CompositorState::new::<Self>(dh),
            xdg_shell_state: XdgShellState::new::<Self>(dh),
            xdg_decoration_state: XdgDecorationState::new::<Self>(dh),
            layer_shell_state: WlrLayerShellState::new::<Self>(dh),
            shm_state: ShmState::new::<Self>(dh, vec![]),
            dmabuf_state: DmabufState::new(),
            dmabuf_global: None,
            output_manager_state: OutputManagerState::new_with_xdg_output::<Self>(dh),
            seat_state,
            data_device_state: DataDeviceState::new::<Self>(dh),
            primary_selection_state,
            activation_state: XdgActivationState::new::<Self>(dh),
            fractional_scale_state: FractionalScaleManagerState::new::<Self>(dh),
            // CLOCK_MONOTONIC, as the frame times.
            presentation_state: PresentationState::new::<Self>(dh, 1),
            xwayland_shell_state: XWaylandShellState::new::<Self>(dh),
            _viewporter: ViewporterState::new::<Self>(dh),
            _cursor_shape: CursorShapeManagerState::new::<Self>(dh),
            _pointer_gestures: PointerGesturesState::new::<Self>(dh),
            _relative_pointer: RelativePointerManagerState::new::<Self>(dh),
            seat,
            space: Space::default(),
            popups: PopupManager::default(),
            model: Model::default(),
            windows: BTreeMap::new(),
            unmapped: Vec::new(),
            activation_tokens: HashMap::new(),
            spawned_at: None,
            activate_on_map: Vec::new(),
            opens_as: Vec::new(),
            textures: Textures::default(),
            grabber: Grabber::default(),
            pictures: HashMap::new(),
            leaving: Vec::new(),
            preview: None,
            preview_out: None,
            switcher_repeat: None,
            levels: crate::levels::Levels::default(),
            preview_ids: std::array::from_fn(|_| Id::new()),
            seam_handle: None,
            seam_handle_id: Id::new(),
            moving: None,
            wobble: None,
            frame_cursor: None,
            last_grip_press: None,
            kept_buttons: HashSet::new(),
            popup_grab: None,
            wake: None,
            drawn_at: std::cell::Cell::new(None),
            cursor: CursorImageStatus::default_named(),
            cursor_images,
            redraw: true,
            strips: HashMap::new(),
            swiping: None,
            switcher: None,
            switcher_out: None,
            overlay: Overlay::new(),
            xwm: None,
            xdisplay: None,
            programs: Programs::default(),
            panel,
            shell: crate::shell::Shell::default(),
            gpu: None,
            blur: false,
            panes: std::cell::RefCell::default(),
            backdrops: std::cell::RefCell::default(),
            session: None,
            kept_keys: HashSet::new(),
            ending: None,
        })
    }

    /// Ctrl+Alt+F`vt`: shows another virtual terminal, on a session of its
    /// own.
    pub fn switch_vt(&mut self, vt: i32) {
        if let Some(session) = &mut self.session
            && let Err(err) = session.change_vt(vt)
        {
            tracing::warn!(vt, %err, "could not switch terminal");
        }
    }

    /// The output under the pointer, or the first one.
    pub fn output_under_pointer(&self) -> Option<Output> {
        let at = self.seat.get_pointer()?.current_location();
        self.space
            .output_under(at)
            .next()
            .or_else(|| self.space.outputs().next())
            .cloned()
    }

    /// What layer surfaces leave of `output`, global coordinates.
    pub fn non_exclusive_zone(&self, output: &Output) -> Option<Rectangle<i32, Logical>> {
        let geometry = self.space.output_geometry(output)?;
        let zone = layer_map_for_output(output).non_exclusive_zone();
        Some(Rectangle::new(geometry.loc + zone.loc, zone.size))
    }

    /// Where the desktop places windows on `output`: what layer surfaces
    /// leave of it less the bar's space when it has a bar, 8 px in.
    pub fn work_area(&self, output: &Output) -> Option<Rect> {
        let zone = self.non_exclusive_zone(output)?;
        let geometry = self.space.output_geometry(output)?;
        let bar = self.panel.width(&output.name());
        Some(model::work_area(rect_of(zone), rect_of(geometry), bar))
    }

    /// The window of `surface`, mapped or waiting for its first buffer.
    pub fn window_for(&self, surface: &WlSurface) -> Option<Window> {
        self.windows
            .values()
            .map(|m| &m.window)
            .chain(self.unmapped.iter())
            .find(|w| w.wl_surface().is_some_and(|s| *s == *surface))
            .cloned()
    }

    /// The model's id of a mapped window.
    pub fn id_of(&self, window: &Window) -> Option<WindowId> {
        self.windows
            .iter()
            .find(|(_, m)| m.window == *window)
            .map(|(id, _)| *id)
    }

    /// The surface under `pos`, and where its origin is: layer surfaces of
    /// the overlay and top layers, then windows, then the bottom and
    /// background layers.
    pub fn surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        use smithay::wayland::shell::wlr_layer::Layer;
        let layer = |layers: &[Layer]| {
            self.layer_surface_under(pos, layers)
                .map(|(s, p, _)| (s, p))
        };
        layer(&[Layer::Overlay, Layer::Top])
            .or_else(|| {
                // Where its surface's origin is, its geometry's corner less
                // its client-side shadow.
                self.space.element_under(pos).and_then(|(window, origin)| {
                    window
                        .surface_under(pos - origin.to_f64(), WindowSurfaceType::ALL)
                        .map(|(s, p)| (s, (p + origin).to_f64()))
                })
            })
            .or_else(|| layer(&[Layer::Bottom, Layer::Background]))
    }

    /// The window under `pos`, if no layer surface above the windows is.
    pub fn window_under(&self, pos: Point<f64, Logical>) -> Option<WindowId> {
        use smithay::wayland::shell::wlr_layer::Layer;
        if self
            .layer_surface_under(pos, &[Layer::Overlay, Layer::Top])
            .is_some()
        {
            return None;
        }
        let (window, _) = self.space.element_under(pos)?;
        self.id_of(window)
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
