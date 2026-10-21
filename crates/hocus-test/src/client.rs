//! A test client: one window filled with a solid colour, which records the
//! events it receives.

use std::{
    fmt,
    os::unix::net::UnixStream,
    path::Path,
    sync::{Arc, Mutex},
    thread::JoinHandle,
};

use anyhow::Context;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_output, delegate_registry, delegate_shm, delegate_xdg_shell,
    delegate_xdg_window,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        WaylandSurface,
        xdg::{
            XdgShell,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, Dispatch, QueueHandle, WEnum,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

/// A window of `width`×`height` pixels filled with `argb`.
#[derive(Debug, Clone)]
pub struct SolidWindow {
    pub app_id: String,
    pub width: u32,
    pub height: u32,
    pub argb: u32,
}

/// What the test client was told by the compositor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientEvent {
    /// A configure with the suggested size; 0 lets the client choose.
    Configure {
        width: u32,
        height: u32,
    },
    PointerEnter,
    PointerLeave,
    /// A pointer button by evdev code.
    Button {
        code: u32,
        pressed: bool,
    },
    KeyboardEnter,
    KeyboardLeave,
    /// A key by evdev code.
    Key {
        code: u32,
        pressed: bool,
    },
    /// The compositor asked the window to close; the client then exits.
    Close,
}

impl fmt::Display for ClientEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pressed = |p: bool| if p { "pressed" } else { "released" };
        match self {
            Self::Configure { width, height } => write!(f, "configure {width}x{height}"),
            Self::PointerEnter => write!(f, "pointer_enter"),
            Self::PointerLeave => write!(f, "pointer_leave"),
            Self::Button { code, pressed: p } => write!(f, "button {code} {}", pressed(*p)),
            Self::KeyboardEnter => write!(f, "keyboard_enter"),
            Self::KeyboardLeave => write!(f, "keyboard_leave"),
            Self::Key { code, pressed: p } => write!(f, "key {code} {}", pressed(*p)),
            Self::Close => write!(f, "close"),
        }
    }
}

/// A running test client.
#[derive(Debug)]
pub struct TestClient {
    pub spec: SolidWindow,
    events: Arc<Mutex<Vec<ClientEvent>>>,
    thread: JoinHandle<anyhow::Result<()>>,
}

impl TestClient {
    /// What the client received so far.
    pub fn events(&self) -> Vec<ClientEvent> {
        self.events.lock().map(|e| e.clone()).unwrap_or_default()
    }

    /// Whether the client is gone: closed, failed, or disconnected.
    pub fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }
}

impl SolidWindow {
    /// Connects to the compositor at `socket` and shows the window from a
    /// thread of its own, until the compositor closes it or goes away.
    pub fn spawn(self, socket: &Path) -> anyhow::Result<TestClient> {
        let stream = UnixStream::connect(socket)
            .with_context(|| format!("could not connect to {}", socket.display()))?;
        let conn = Connection::from_socket(stream)?;
        let events = Arc::new(Mutex::new(Vec::new()));
        let recorded = events.clone();
        let thread = std::thread::Builder::new()
            .name(format!("client {}", self.app_id))
            .spawn({
                let spec = self.clone();
                move || spec.run(conn, recorded)
            })?;
        Ok(TestClient {
            spec: self,
            events,
            thread,
        })
    }

    fn run(self, conn: Connection, events: Arc<Mutex<Vec<ClientEvent>>>) -> anyhow::Result<()> {
        let (globals, mut queue) = registry_queue_init(&conn)?;
        let qh = queue.handle();
        let compositor = CompositorState::bind(&globals, &qh).context("no wl_compositor")?;
        let xdg_shell = XdgShell::bind(&globals, &qh).context("no xdg_wm_base")?;
        let shm = Shm::bind(&globals, &qh).context("no wl_shm")?;
        let _seat: wl_seat::WlSeat = globals.bind(&qh, 1..=7, ()).context("no wl_seat")?;

        let surface = compositor.create_surface(&qh);
        let window = xdg_shell.create_window(surface, WindowDecorations::None, &qh);
        window.set_app_id(self.app_id.clone());
        window.set_min_size(Some((self.width, self.height)));
        window.commit();

        let pool = SlotPool::new((self.width * self.height * 4) as usize, &shm)?;
        let mut client = Client {
            spec: self,
            registry: RegistryState::new(&globals),
            outputs: OutputState::new(&globals, &qh),
            shm,
            pool,
            window,
            events,
            drawn: false,
            closed: false,
        };
        while !client.closed {
            queue.blocking_dispatch(&mut client)?;
        }
        Ok(())
    }
}

struct Client {
    spec: SolidWindow,
    registry: RegistryState,
    outputs: OutputState,
    shm: Shm,
    pool: SlotPool,
    window: Window,
    events: Arc<Mutex<Vec<ClientEvent>>>,
    drawn: bool,
    closed: bool,
}

impl Client {
    fn record(&self, event: ClientEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
    }

    fn draw(&mut self) -> anyhow::Result<()> {
        let (width, height, argb) = (self.spec.width, self.spec.height, self.spec.argb);
        let (buffer, canvas) = self.pool.create_buffer(
            width as i32,
            height as i32,
            width as i32 * 4,
            wl_shm::Format::Argb8888,
        )?;
        for pixel in canvas.as_chunks_mut::<4>().0 {
            *pixel = argb.to_le_bytes();
        }
        let surface = self.window.wl_surface();
        surface.damage_buffer(0, 0, width as i32, height as i32);
        buffer.attach_to(surface)?;
        self.window.commit();
        Ok(())
    }
}

impl WindowHandler for Client {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.record(ClientEvent::Close);
        self.closed = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        let (width, height) = configure.new_size;
        self.record(ClientEvent::Configure {
            width: width.map_or(0, |w| w.get()),
            height: height.map_or(0, |h| h.get()),
        });
        // The size stays what the test asked for: one buffer is enough.
        if !self.drawn {
            self.drawn = true;
            if let Err(err) = self.draw() {
                eprintln!("test client could not draw: {err:#}");
                self.closed = true;
            }
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for Client {
    fn event(
        _: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(caps),
        } = event
        {
            if caps.contains(wl_seat::Capability::Pointer) {
                seat.get_pointer(qh, ());
            }
            if caps.contains(wl_seat::Capability::Keyboard) {
                seat.get_keyboard(qh, ());
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for Client {
    fn event(
        client: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter { .. } => client.record(ClientEvent::PointerEnter),
            wl_pointer::Event::Leave { .. } => client.record(ClientEvent::PointerLeave),
            wl_pointer::Event::Button { button, state, .. } => client.record(ClientEvent::Button {
                code: button,
                pressed: state == WEnum::Value(wl_pointer::ButtonState::Pressed),
            }),
            _ => {}
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for Client {
    fn event(
        client: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The keymap's file descriptor is dropped with the event: codes are
        // enough for tests.
        match event {
            wl_keyboard::Event::Enter { .. } => client.record(ClientEvent::KeyboardEnter),
            wl_keyboard::Event::Leave { .. } => client.record(ClientEvent::KeyboardLeave),
            wl_keyboard::Event::Key { key, state, .. } => client.record(ClientEvent::Key {
                code: key,
                pressed: state == WEnum::Value(wl_keyboard::KeyState::Pressed),
            }),
            _ => {}
        }
    }
}

impl CompositorHandler for Client {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for Client {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for Client {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for Client {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }

    registry_handlers![OutputState];
}

delegate_compositor!(Client);
delegate_output!(Client);
delegate_shm!(Client);
delegate_xdg_shell!(Client);
delegate_xdg_window!(Client);
delegate_registry!(Client);
