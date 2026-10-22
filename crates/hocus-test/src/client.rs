//! A test client: one window filled with a solid colour, which records the
//! events it receives, and can copy text to the clipboard and read it.

use std::{
    fmt,
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender, channel},
    },
    thread::JoinHandle,
};

use anyhow::Context;
use rustix::event::{PollFd, PollFlags, Timespec};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    data_device_manager::{
        DataDeviceManagerState, WritePipe,
        data_device::{DataDevice, DataDeviceHandler},
        data_offer::{DataOfferHandler, DragOffer},
        data_source::{CopyPasteSource, DataSourceHandler},
    },
    delegate_compositor, delegate_data_device, delegate_output, delegate_registry, delegate_shm,
    delegate_xdg_shell, delegate_xdg_window,
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
    protocol::{
        wl_data_device::WlDataDevice, wl_data_device_manager::DndAction,
        wl_data_source::WlDataSource, wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm,
        wl_surface,
    },
};
use wayland_protocols::{
    wp::keyboard_shortcuts_inhibit::zv1::client::{
        zwp_keyboard_shortcuts_inhibit_manager_v1::ZwpKeyboardShortcutsInhibitManagerV1,
        zwp_keyboard_shortcuts_inhibitor_v1::ZwpKeyboardShortcutsInhibitorV1,
    },
    xdg::activation::v1::client::{
        xdg_activation_token_v1::{self, XdgActivationTokenV1},
        xdg_activation_v1::XdgActivationV1,
    },
};

/// What the test asks of a running client.
#[derive(Debug)]
enum Command {
    Copy(String),
    /// Asks the compositor to bring its window forward (xdg-activation).
    Activate,
}

/// What a test client does besides showing its window.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClientOptions {
    /// Holds the compositor's shortcuts back while it has the keyboard.
    pub inhibit_shortcuts: bool,
}

/// The text formats the test client copies and reads.
const TEXT: [&str; 2] = ["text/plain;charset=utf-8", "UTF8_STRING"];

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
    clipboard: Arc<Mutex<Option<String>>>,
    commands: Sender<Command>,
    thread: JoinHandle<anyhow::Result<()>>,
}

impl TestClient {
    /// Takes the clipboard with `text`, as Ctrl+C would.
    pub fn copy(&self, text: &str) -> anyhow::Result<()> {
        self.commands
            .send(Command::Copy(text.to_owned()))
            .context("the test client is gone")
    }

    /// Asks for its window to come forward, with a token of its own.
    pub fn activate(&self) -> anyhow::Result<()> {
        self.commands
            .send(Command::Activate)
            .context("the test client is gone")
    }

    /// The text of the clipboard as last offered to the client, read as
    /// soon as it was offered.
    pub fn clipboard(&self) -> Option<String> {
        self.clipboard.lock().ok().and_then(|c| c.clone())
    }

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
        self.spawn_with(socket, ClientOptions::default())
    }

    pub fn spawn_with(self, socket: &Path, options: ClientOptions) -> anyhow::Result<TestClient> {
        let stream = UnixStream::connect(socket)
            .with_context(|| format!("could not connect to {}", socket.display()))?;
        let conn = Connection::from_socket(stream)?;
        let events = Arc::new(Mutex::new(Vec::new()));
        let clipboard = Arc::new(Mutex::new(None));
        let (commands, to_do) = channel();
        let thread = std::thread::Builder::new()
            .name(format!("client {}", self.app_id))
            .spawn({
                let spec = self.clone();
                let shared = Shared {
                    events: events.clone(),
                    clipboard: clipboard.clone(),
                };
                move || spec.run(conn, shared, to_do, options)
            })?;
        Ok(TestClient {
            spec: self,
            events,
            clipboard,
            commands,
            thread,
        })
    }

    fn run(
        self,
        conn: Connection,
        shared: Shared,
        to_do: Receiver<Command>,
        options: ClientOptions,
    ) -> anyhow::Result<()> {
        let (globals, mut queue) = registry_queue_init(&conn)?;
        let qh = queue.handle();
        let compositor = CompositorState::bind(&globals, &qh).context("no wl_compositor")?;
        let xdg_shell = XdgShell::bind(&globals, &qh).context("no xdg_wm_base")?;
        let shm = Shm::bind(&globals, &qh).context("no wl_shm")?;
        let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=7, ()).context("no wl_seat")?;
        let data_devices =
            DataDeviceManagerState::bind(&globals, &qh).context("no wl_data_device_manager")?;
        let data_device = data_devices.get_data_device(&qh, &seat);

        let surface = compositor.create_surface(&qh);
        // Server-side decorations: the compositor draws corners, shadow and
        // ring.
        let window = xdg_shell.create_window(surface, WindowDecorations::RequestServer, &qh);
        window.set_app_id(self.app_id.clone());
        window.set_min_size(Some((self.width, self.height)));
        window.commit();
        if options.inhibit_shortcuts {
            let manager: ZwpKeyboardShortcutsInhibitManagerV1 = globals
                .bind(&qh, 1..=1, ())
                .context("no keyboard shortcuts inhibitor")?;
            manager.inhibit_shortcuts(window.wl_surface(), &seat, &qh, ());
        }
        let activation: Option<XdgActivationV1> = globals.bind(&qh, 1..=1, ()).ok();

        let pool = SlotPool::new((self.width * self.height * 4) as usize, &shm)?;
        let mut client = Client {
            spec: self,
            registry: RegistryState::new(&globals),
            outputs: OutputState::new(&globals, &qh),
            shm,
            pool,
            window,
            events: shared.events,
            clipboard: shared.clipboard,
            data_devices,
            data_device,
            copied: None,
            seat: seat.clone(),
            activation,
            serial: 0,
            drawn: None,
            closed: false,
        };
        while !client.closed {
            while let Ok(command) = to_do.try_recv() {
                match command {
                    Command::Copy(text) => client.copy(text, &qh),
                    Command::Activate => client.ask_activation(&qh),
                }
            }
            queue.flush()?;
            queue.dispatch_pending(&mut client)?;
            // Wakes up now and then for what the test asks.
            if let Some(guard) = queue.prepare_read() {
                let fd = guard.connection_fd();
                let mut fds = [PollFd::new(&fd, PollFlags::IN)];
                let timeout = Timespec {
                    tv_sec: 0,
                    tv_nsec: 10_000_000,
                };
                if rustix::event::poll(&mut fds, Some(&timeout))? > 0 {
                    guard.read()?;
                }
            }
        }
        Ok(())
    }
}

struct Shared {
    events: Arc<Mutex<Vec<ClientEvent>>>,
    clipboard: Arc<Mutex<Option<String>>>,
}

struct Client {
    spec: SolidWindow,
    registry: RegistryState,
    outputs: OutputState,
    shm: Shm,
    pool: SlotPool,
    window: Window,
    events: Arc<Mutex<Vec<ClientEvent>>>,
    clipboard: Arc<Mutex<Option<String>>>,
    data_devices: DataDeviceManagerState,
    data_device: DataDevice,
    /// What the client offers as the clipboard, while it holds it.
    copied: Option<(CopyPasteSource, String)>,
    seat: wl_seat::WlSeat,
    activation: Option<XdgActivationV1>,
    /// The last input serial, which taking the clipboard needs.
    serial: u32,
    /// The size of the last buffer, if one was attached.
    drawn: Option<(u32, u32)>,
    closed: bool,
}

impl Client {
    fn record(&self, event: ClientEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
    }

    /// A token for its own window, with its last input serial; used as
    /// soon as it comes.
    fn ask_activation(&mut self, qh: &QueueHandle<Self>) {
        let Some(activation) = &self.activation else {
            return;
        };
        let token = activation.get_activation_token(qh, ());
        token.set_serial(self.serial, &self.seat);
        token.set_surface(self.window.wl_surface());
        token.commit();
    }

    fn copy(&mut self, text: String, qh: &QueueHandle<Self>) {
        let source = self.data_devices.create_copy_paste_source(qh, TEXT);
        source.set_selection(&self.data_device, self.serial);
        self.copied = Some((source, text));
    }

    fn draw(&mut self, (width, height): (u32, u32)) -> anyhow::Result<()> {
        let argb = self.spec.argb;
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
        let (width, height) = (width.map_or(0, |w| w.get()), height.map_or(0, |h| h.get()));
        self.record(ClientEvent::Configure { width, height });
        // The size the compositor asks for, or the window's own.
        let size = if width > 0 && height > 0 {
            (width, height)
        } else {
            (self.spec.width, self.spec.height)
        };
        if self.drawn != Some(size) {
            self.drawn = Some(size);
            if let Err(err) = self.draw(size) {
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
            wl_pointer::Event::Button {
                button,
                state,
                serial,
                ..
            } => {
                client.serial = serial;
                client.record(ClientEvent::Button {
                    code: button,
                    pressed: state == WEnum::Value(wl_pointer::ButtonState::Pressed),
                });
            }
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
            wl_keyboard::Event::Enter { serial, .. } => {
                client.serial = serial;
                client.record(ClientEvent::KeyboardEnter);
            }
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

impl DataDeviceHandler for Client {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataDevice,
        _: f64,
        _: f64,
        _: &wl_surface::WlSurface,
    ) {
    }

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}

    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, _: f64, _: f64) {}

    /// Reads every text clipboard offered, from a thread: the source may be
    /// slow, and an X11 one goes through the compositor.
    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {
        if self.copied.is_some() {
            // Its own: reading it would wait on itself.
            return;
        }
        let Some(offer) = self.data_device.data().selection_offer() else {
            return;
        };
        let Some(mime) = offer.with_mime_types(|mimes| {
            TEXT.iter()
                .find(|t| mimes.iter().any(|m| m == *t))
                .map(|t| t.to_string())
        }) else {
            return;
        };
        let Ok(mut pipe) = offer.receive(mime) else {
            return;
        };
        let clipboard = self.clipboard.clone();
        std::thread::spawn(move || {
            let mut text = String::new();
            if pipe.read_to_string(&mut text).is_ok()
                && let Ok(mut clipboard) = clipboard.lock()
            {
                *clipboard = Some(text);
            }
        });
    }

    fn drop_performed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
}

impl DataOfferHandler for Client {
    fn source_actions(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }

    fn selected_action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }
}

impl DataSourceHandler for Client {
    fn accept_mime(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataSource,
        _: Option<String>,
    ) {
    }

    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataSource,
        _: String,
        mut pipe: WritePipe,
    ) {
        if let Some((_, text)) = &self.copied {
            let _ = pipe.write_all(text.as_bytes());
        }
    }

    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {
        self.copied = None;
    }

    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}

    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}

    fn action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: DndAction) {}
}

delegate_data_device!(Client);
delegate_compositor!(Client);
delegate_output!(Client);
delegate_shm!(Client);
delegate_xdg_shell!(Client);
delegate_xdg_window!(Client);
delegate_registry!(Client);

impl Dispatch<ZwpKeyboardShortcutsInhibitManagerV1, ()> for Client {
    fn event(
        _: &mut Self,
        _: &ZwpKeyboardShortcutsInhibitManagerV1,
        _: <ZwpKeyboardShortcutsInhibitManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpKeyboardShortcutsInhibitorV1, ()> for Client {
    fn event(
        _: &mut Self,
        _: &ZwpKeyboardShortcutsInhibitorV1,
        _: <ZwpKeyboardShortcutsInhibitorV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<XdgActivationV1, ()> for Client {
    fn event(
        _: &mut Self,
        _: &XdgActivationV1,
        _: <XdgActivationV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<XdgActivationTokenV1, ()> for Client {
    fn event(
        client: &mut Self,
        token: &XdgActivationTokenV1,
        event: xdg_activation_token_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_activation_token_v1::Event::Done { token: name } = event {
            if let Some(activation) = &client.activation {
                activation.activate(name, client.window.wl_surface());
            }
            token.destroy();
        }
    }
}
