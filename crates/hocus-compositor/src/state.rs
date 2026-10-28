//! Everything the compositor knows, owned by the event loop, and what it
//! does with windows: map them where the model says, focus them, unmap
//! them.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};

use anyhow::Context;
use smithay::{
    backend::session::{Session, libseat::LibSeatSession},
    desktop::{PopupManager, Space, Window, WindowSurfaceType, layer_map_for_output},
    input::{
        Seat, SeatState,
        keyboard::{Keycode, XkbConfig},
        pointer::CursorImageStatus,
    },
    output::Output,
    reexports::{
        calloop::{LoopHandle, LoopSignal},
        wayland_server::{
            DisplayHandle,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::wl_surface::WlSurface,
        },
    },
    utils::{Clock, Logical, Monotonic, Point, Rectangle, SERIAL_COUNTER},
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
    config::xkb_lists,
    cursor::CursorImages,
    focus::KeyboardFocus,
    model::{self, Model, Rect, WindowId},
    programs::Programs,
    shortcuts::Ending,
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
    /// The mapped windows, by their id in the model.
    pub windows: BTreeMap<WindowId, Window>,
    /// Toplevels whose first buffer has not come: mapped once it does.
    pub unmapped: Vec<Window>,
    /// Activation tokens handed out, and when; what they let a window do
    /// is task 2.4's.
    pub activation_tokens: HashMap<String, std::time::Duration>,
    /// What the pointer shows, as the client under it sets it.
    pub cursor: CursorImageStatus,
    pub cursor_images: CursorImages,
    /// Something changed on screen since the last frame.
    pub redraw: bool,

    /// The X11 window manager once XWayland is ready, and its display.
    pub xwm: Option<X11Wm>,
    pub xdisplay: Option<u32>,
    pub programs: Programs,
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
        let (layout, variant) = xkb_lists(&keyboard.layouts);
        let xkb = XkbConfig {
            layout: &layout,
            variant: &variant,
            options: (!keyboard.options.is_empty()).then(|| keyboard.options.clone()),
            ..XkbConfig::default()
        };
        let added = seat.add_keyboard(xkb, keyboard.repeat_delay, keyboard.repeat_rate);
        if let Err(err) = added {
            // A layout XKB does not know: its defaults, rather than no
            // keyboard at all.
            tracing::warn!(?err, layouts = ?keyboard.layouts, "keyboard layouts refused; XKB's defaults");
            seat.add_keyboard(
                XkbConfig::default(),
                keyboard.repeat_delay,
                keyboard.repeat_rate,
            )
            .context("could not set up the keyboard")?;
        }
        seat.add_pointer();
        let primary_selection_state = PrimarySelectionState::new::<Self>(dh);

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
            cursor: CursorImageStatus::default_named(),
            cursor_images: CursorImages::default(),
            redraw: true,
            xwm: None,
            xdisplay: None,
            programs: Programs::default(),
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
    /// leave of it, 8 px in.
    pub fn work_area(&self, output: &Output) -> Option<Rect> {
        let zone = self.non_exclusive_zone(output)?;
        Some(model::work_area(Rect::new(
            zone.loc.x,
            zone.loc.y,
            zone.size.w,
            zone.size.h,
        )))
    }

    /// The window of `surface`, mapped or waiting for its first buffer.
    pub fn window_for(&self, surface: &WlSurface) -> Option<Window> {
        self.windows
            .values()
            .chain(self.unmapped.iter())
            .find(|w| w.wl_surface().is_some_and(|s| *s == *surface))
            .cloned()
    }

    /// The model's id of a mapped window.
    pub fn id_of(&self, window: &Window) -> Option<WindowId> {
        self.windows
            .iter()
            .find(|(_, w)| *w == window)
            .map(|(id, _)| *id)
    }

    /// Maps `window` at the centre of the work area of the output under the
    /// pointer, and gives it the focus.
    pub fn map_window(&mut self, window: Window) {
        let Some(output) = self.output_under_pointer() else {
            tracing::warn!("a window with no output to show on");
            self.unmapped.push(window);
            return;
        };
        let Some(area) = self.work_area(&output) else {
            self.unmapped.push(window);
            return;
        };
        let size = window.geometry().size;
        let (x, y) = model::centred(area, (size.w, size.h));
        let id = self.model.add();
        self.windows.insert(id, window.clone());
        self.space.map_element(window.clone(), (x, y), false);
        if let Some(x11) = window.x11_surface() {
            let geometry = Rectangle::new((x, y).into(), size);
            if let Err(err) = x11.configure(geometry) {
                tracing::warn!(%err, "could not place an X11 window");
            }
        }
        tracing::debug!(window = id.0, x, y, w = size.w, h = size.h, "window mapped");
        // Every new window takes the focus for now: which ones may is the
        // activation policy of task 2.4.
        self.focus(Some(id));
        self.redraw = true;
    }

    /// A window went, or its client hid it.
    pub fn unmap_window(&mut self, window: &Window) {
        self.unmapped.retain(|w| w != window);
        let Some(id) = self.id_of(window) else {
            return;
        };
        self.windows.remove(&id);
        self.space.unmap_elem(window);
        let had_focus = self.model.focused() == Some(id);
        self.model.remove(id);
        if had_focus {
            let next = self.model.focused();
            self.focus(next);
        }
        self.redraw = true;
    }

    /// Focuses and raises a window (none: no window has the keyboard), and
    /// tells every window whether it is the active one.
    pub fn focus(&mut self, id: Option<WindowId>) {
        self.model.focus(id);
        let focused = self.model.focused();
        for (wid, window) in &self.windows {
            let active = Some(*wid) == focused;
            if window.set_activated(active)
                && let Some(toplevel) = window.toplevel()
                && toplevel.is_initial_configure_sent()
            {
                toplevel.send_pending_configure();
            }
            if let Some(x11) = window.x11_surface()
                && let Err(err) = x11.set_activated(active)
            {
                tracing::debug!(%err, "could not tell an X11 window it is active");
            }
        }
        let target = focused.and_then(|id| self.windows.get(&id)).cloned();
        if let Some(window) = &target {
            self.space.raise_element(window, true);
            if let (Some(x11), Some(xwm)) = (window.x11_surface(), self.xwm.as_mut())
                && let Err(err) = xwm.raise_window(x11)
            {
                tracing::debug!(%err, "could not raise an X11 window");
            }
        }
        let focus = target.and_then(|w| match w.x11_surface() {
            Some(x11) => Some(KeyboardFocus::X11(x11.clone())),
            None => w
                .toplevel()
                .map(|t| KeyboardFocus::Wayland(t.wl_surface().clone())),
        });
        // A lock screen or a launcher holding the keyboard keeps it.
        if !self.keyboard_held_by_layer()
            && let Some(keyboard) = self.seat.get_keyboard()
        {
            keyboard.set_focus(self, focus, SERIAL_COUNTER.next_serial());
        }
        self.redraw = true;
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
