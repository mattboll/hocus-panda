use std::path::PathBuf;

use anyhow::Context;
use smithay::{
    desktop::{PopupManager, Space, Window, WindowSurfaceType},
    input::{Seat, SeatState},
    reexports::{
        calloop::{LoopSignal, channel::Sender},
        wayland_server::{
            DisplayHandle,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::wl_surface::WlSurface,
        },
    },
    utils::{Logical, Point, Serial},
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        output::OutputManagerState,
        selection::data_device::DataDeviceState,
        shell::xdg::XdgShellState,
        shm::ShmState,
    },
};

use hocus_api::Event;

use crate::{
    Clock,
    api::{self, EncodedScreenshot},
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
    /// The last window id given.
    pub next_window_id: u64,

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
            next_window_id: 0,
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

    /// The window whose toplevel surface is `surface`, placed or not.
    pub fn window_for(&self, surface: &WlSurface) -> Option<Window> {
        self.space
            .elements()
            .chain(self.unplaced.iter())
            .find(|w| w.toplevel().is_some_and(|t| t.wl_surface() == surface))
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

    /// Places the windows that just got their first buffer, centred on the
    /// first output, and gives them focus.
    pub fn place_new_windows(&mut self, serial: Serial) {
        let Some(area) = self
            .space
            .outputs()
            .next()
            .and_then(|o| self.space.output_geometry(o))
        else {
            return;
        };
        let (ready, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.unplaced)
            .into_iter()
            .partition(|w| !w.geometry().size.is_empty());
        self.unplaced = waiting;
        for window in ready {
            let size = window.geometry().size;
            let loc =
                area.loc + Point::from(((area.size.w - size.w) / 2, (area.size.h - size.h) / 2));
            self.space.map_element(window.clone(), loc, true);
            self.emit(|s| Event::WindowOpened {
                window: s.window_info(&window),
            });
            self.focus(Some(&window), serial);
        }
    }

    /// Raises `window` and gives it keyboard focus; `None` clears the focus.
    pub fn focus(&mut self, window: Option<&Window>, serial: Serial) {
        let before = self.focused_window();
        if let Some(window) = window {
            self.space.raise_element(window, true);
        }
        for w in self.space.elements() {
            w.set_activated(Some(w) == window);
            if let Some(toplevel) = w.toplevel() {
                toplevel.send_pending_configure();
            }
        }
        let surface = window
            .and_then(|w| w.toplevel())
            .map(|t| t.wl_surface().clone());
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, surface, serial);
        }
        if before.as_ref() != window {
            self.emit(|_| Event::WindowFocused {
                id: window.map(api::window_id),
            });
        }
        self.needs_redraw = true;
    }

    /// Gives focus to the topmost window, if any.
    pub fn focus_top(&mut self, serial: Serial) {
        let top = self.space.elements().last().cloned();
        self.focus(top.as_ref(), serial);
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
