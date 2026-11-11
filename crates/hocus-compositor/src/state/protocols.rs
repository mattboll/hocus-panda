//! The Wayland protocols' server states, which smithay's handlers ask for.

use smithay::{
    input::SeatState,
    reexports::wayland_server::DisplayHandle,
    wayland::{
        compositor::CompositorState,
        cursor_shape::CursorShapeManagerState,
        dmabuf::{DmabufGlobal, DmabufState},
        fractional_scale::FractionalScaleManagerState,
        keyboard_shortcuts_inhibit::KeyboardShortcutsInhibitState,
        output::OutputManagerState,
        pointer_gestures::PointerGesturesState,
        presentation::PresentationState,
        relative_pointer::RelativePointerManagerState,
        selection::{data_device::DataDeviceState, primary_selection::PrimarySelectionState},
        shell::{
            kde::decoration::KdeDecorationState,
            wlr_layer::WlrLayerShellState,
            xdg::{XdgShellState, decoration::XdgDecorationState},
        },
        shm::ShmState,
        viewporter::ViewporterState,
        xdg_activation::XdgActivationState,
        xwayland_shell::XWaylandShellState,
    },
};

use super::State;

#[derive(Debug)]
pub struct Protocols {
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub xdg_decoration_state: XdgDecorationState,
    /// KDE's server decoration protocol, which GTK 3 clients use, having
    /// no xdg-decoration (#716).
    pub kde_decoration_state: KdeDecorationState,
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
    /// Clients that take every key, the desktop's shortcuts with them
    /// (the Settings while they record one, 6.8).
    pub shortcuts_inhibit_state: KeyboardShortcutsInhibitState,
}

impl Protocols {
    /// The protocols' globals, made in the order clients have always seen
    /// them: the seat's and the primary selection's are made before, by
    /// `State::new`.
    pub(super) fn new(
        dh: &DisplayHandle,
        seat_state: SeatState<State>,
        primary_selection_state: PrimarySelectionState,
    ) -> Self {
        Self {
            // Version 6: a surface is told the scale to draw at
            // (`preferred_buffer_scale`), the largest of the screens it is
            // on (docs/design/windows.md §12, #419).
            compositor_state: CompositorState::new_v6::<State>(dh),
            xdg_shell_state: XdgShellState::new::<State>(dh),
            xdg_decoration_state: XdgDecorationState::new::<State>(dh),
            kde_decoration_state: KdeDecorationState::new::<State>(
                dh,
                smithay::reexports::wayland_protocols_misc::server_decoration::server::org_kde_kwin_server_decoration_manager::Mode::Server,
            ),
            layer_shell_state: WlrLayerShellState::new::<State>(dh),
            shm_state: ShmState::new::<State>(dh, vec![]),
            dmabuf_state: DmabufState::new(),
            dmabuf_global: None,
            output_manager_state: OutputManagerState::new_with_xdg_output::<State>(dh),
            seat_state,
            data_device_state: DataDeviceState::new::<State>(dh),
            primary_selection_state,
            activation_state: XdgActivationState::new::<State>(dh),
            fractional_scale_state: FractionalScaleManagerState::new::<State>(dh),
            // CLOCK_MONOTONIC, as the frame times.
            presentation_state: PresentationState::new::<State>(dh, 1),
            xwayland_shell_state: XWaylandShellState::new::<State>(dh),
            _viewporter: ViewporterState::new::<State>(dh),
            _cursor_shape: CursorShapeManagerState::new::<State>(dh),
            _pointer_gestures: PointerGesturesState::new::<State>(dh),
            _relative_pointer: RelativePointerManagerState::new::<State>(dh),
            shortcuts_inhibit_state: KeyboardShortcutsInhibitState::new::<State>(dh),
        }
    }
}
