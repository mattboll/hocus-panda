//! Test clients: real Wayland clients on threads of their own, connected
//! to an in-process headless compositor that the test drives. Each test
//! binary uses part of it.
#![allow(dead_code)]

use std::{os::unix::net::UnixStream, path::Path, sync::mpsc, time::Duration};

use anyhow::Context;
use hocus_compositor::{Headless, HeadlessConfig, OutputSpec, RendererChoice, model::Rect};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm, delegate_xdg_popup, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
        },
        xdg::{
            XdgPositioner, XdgShell, XdgSurface,
            popup::{Popup, PopupConfigure, PopupHandler},
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

pub const WAIT: Duration = Duration::from_secs(10);
/// What test windows are filled with, ARGB.
pub const WINDOW_COLOUR: u32 = 0xff3a_7bd5;
/// A client-side shadow: black at 25 %, premultiplied.
pub const SHADOW_COLOUR: u32 = 0x4000_0000;
pub const BTN_LEFT: u32 = 0x110;
pub const KEY_LEFTMETA: u32 = 125;
pub const KEY_LEFTSHIFT: u32 = 42;
pub const KEY_UP: u32 = 103;
pub const KEY_LEFT: u32 = 105;
pub const KEY_RIGHT: u32 = 106;
pub const KEY_DOWN: u32 = 108;
pub const KEY_H: u32 = 35;
pub const KEY_1: u32 = 2;
pub const KEY_2: u32 = 3;
pub const KEY_3: u32 = 4;
pub const KEY_ESC: u32 = 1;
pub const KEY_TAB: u32 = 15;
pub const KEY_LEFTCTRL: u32 = 29;
pub const KEY_LEFTALT: u32 = 56;

pub fn compositor() -> anyhow::Result<Headless> {
    Headless::start(HeadlessConfig {
        // The same pixels on every machine, and no GPU needed.
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    })
}

/// Two screens side by side: 1920×1080, then 2560×1440 on its right.
pub fn two_screens() -> anyhow::Result<Headless> {
    Headless::start(HeadlessConfig {
        outputs: vec![OutputSpec::new(1920, 1080), OutputSpec::new(2560, 1440)],
        renderer: RendererChoice::Pixman,
    })
}

/// What a test client shows.
#[derive(Debug, Clone)]
pub enum Shows {
    /// An xdg toplevel of this size, with this app id; with `popup`, a
    /// press on it opens a menu that grabs the keyboard.
    Window {
        app_id: &'static str,
        size: (u32, u32),
        popup: bool,
    },
    /// A layer surface along the output's top edge, `height` high, which
    /// reserves its height.
    Bar { height: u32 },
    /// An xdg toplevel of this size drawn with a translucent shadow
    /// `margin` wide around it, outside its window geometry, as
    /// client-side decorated applications draw theirs.
    Shadowed {
        app_id: &'static str,
        size: (u32, u32),
        margin: u32,
    },
}

/// What the test asks of a client.
#[derive(Debug, Clone, Copy)]
pub enum Command {
    Fullscreen(bool),
}

/// What a client tells the test.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Globals(Vec<String>),
    PopupShown,
    PopupDone,
}

/// A client running on its thread.
#[derive(Debug)]
pub struct ClientHandle {
    pub events: mpsc::Receiver<Event>,
    pub commands: mpsc::Sender<Command>,
}

/// Starts a client on a thread of its own. It runs until the compositor
/// goes.
pub fn client(socket: &Path, shows: Option<Shows>) -> anyhow::Result<ClientHandle> {
    let stream = UnixStream::connect(socket)
        .with_context(|| format!("could not connect to {}", socket.display()))?;
    stream.set_nonblocking(true)?;
    let conn = Connection::from_socket(stream)?;
    let (events, events_rx) = mpsc::channel();
    let (commands_tx, commands) = mpsc::channel();
    std::thread::Builder::new()
        .name("test client".into())
        .spawn(move || {
            if let Err(err) = run_client(&conn, shows, &events, &commands) {
                // The compositor going ends every client.
                eprintln!("test client: {err:#}");
            }
        })?;
    Ok(ClientHandle {
        events: events_rx,
        commands: commands_tx,
    })
}

fn run_client(
    conn: &Connection,
    shows: Option<Shows>,
    events: &mpsc::Sender<Event>,
    commands: &mpsc::Receiver<Command>,
) -> anyhow::Result<()> {
    let (globals, mut queue) = registry_queue_init(conn)?;
    let qh = queue.handle();
    let names = globals.contents().with_list(|list| {
        list.iter()
            .map(|g| format!("{} v{}", g.interface, g.version))
            .collect()
    });
    events.send(Event::Globals(names))?;
    let compositor = CompositorState::bind(&globals, &qh).context("no wl_compositor")?;
    let shm = Shm::bind(&globals, &qh).context("no wl_shm")?;
    let xdg = XdgShell::bind(&globals, &qh).ok();
    let mut client = Client {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        seats: SeatState::new(&globals, &qh),
        pool: SlotPool::new(512 * 512 * 4, &shm)?,
        shm,
        size: (1, 1),
        window: None,
        layer: None,
        popup: None,
        popup_on_press: false,
        margin: 0,
        seat: None,
        events: events.clone(),
        compositor,
        xdg,
    };
    match shows {
        Some(Shows::Window {
            app_id,
            size,
            popup,
        }) => {
            let xdg = client.xdg.as_ref().context("no xdg_wm_base")?;
            let surface = client.compositor.create_surface(&qh);
            let window = xdg.create_window(surface, WindowDecorations::RequestServer, &qh);
            window.set_app_id(app_id);
            window.commit();
            client.size = size;
            client.window = Some(window);
            client.popup_on_press = popup;
        }
        Some(Shows::Shadowed {
            app_id,
            size,
            margin,
        }) => {
            let xdg = client.xdg.as_ref().context("no xdg_wm_base")?;
            let surface = client.compositor.create_surface(&qh);
            let window = xdg.create_window(surface, WindowDecorations::RequestClient, &qh);
            window.set_app_id(app_id);
            window.commit();
            client.size = size;
            client.margin = margin;
            client.window = Some(window);
        }
        Some(Shows::Bar { height }) => {
            let shell = LayerShell::bind(&globals, &qh).context("no layer shell")?;
            let surface = client.compositor.create_surface(&qh);
            let layer = shell.create_layer_surface(&qh, surface, Layer::Top, Some("bar"), None);
            layer.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
            layer.set_size(0, height);
            layer.set_exclusive_zone(height as i32);
            layer.commit();
            client.size = (1, height);
            client.layer = Some(layer);
        }
        None => {}
    }
    loop {
        queue.dispatch_pending(&mut client)?;
        while let Ok(command) = commands.try_recv() {
            if let Some(window) = &client.window {
                match command {
                    Command::Fullscreen(true) => window.set_fullscreen(None),
                    Command::Fullscreen(false) => window.unset_fullscreen(),
                }
            }
        }
        queue.flush()?;
        if let Some(guard) = queue.prepare_read() {
            match guard.read() {
                Ok(_) => {}
                Err(wayland_client::backend::WaylandError::Io(err))
                    if err.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(err) => return Err(err.into()),
            }
        }
    }
}

struct Client {
    registry: RegistryState,
    outputs: OutputState,
    seats: SeatState,
    shm: Shm,
    pool: SlotPool,
    size: (u32, u32),
    window: Option<Window>,
    layer: Option<LayerSurface>,
    popup: Option<Popup>,
    popup_on_press: bool,
    /// The shadow around the window, outside its geometry.
    margin: u32,
    seat: Option<wl_seat::WlSeat>,
    events: mpsc::Sender<Event>,
    compositor: CompositorState,
    xdg: Option<XdgShell>,
}

impl Client {
    /// Fills `surface` with a solid colour, `size` large.
    fn draw(&mut self, surface: &wl_surface::WlSurface, size: (u32, u32)) {
        self.draw_framed(surface, size, 0);
    }

    /// A solid colour `size` large, within a translucent shadow `margin`
    /// wide.
    fn draw_framed(&mut self, surface: &wl_surface::WlSurface, size: (u32, u32), margin: u32) {
        let m = margin as i32;
        let (iw, ih) = (size.0.max(1) as i32, size.1.max(1) as i32);
        let (w, h) = (iw + 2 * m, ih + 2 * m);
        let Ok((buffer, canvas)) = self
            .pool
            .create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)
        else {
            return;
        };
        for (i, pixel) in canvas.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (x, y) = (i as i32 % w, i as i32 / w);
            let inside = x >= m && y >= m && x < m + iw && y < m + ih;
            let colour: u32 = if inside { WINDOW_COLOUR } else { SHADOW_COLOUR };
            pixel.copy_from_slice(&colour.to_le_bytes());
        }
        surface.damage_buffer(0, 0, w, h);
        if buffer.attach_to(surface).is_ok() {
            surface.commit();
        }
    }

    /// A menu under the press, grabbing the keyboard.
    fn open_popup(&mut self, qh: &QueueHandle<Self>, serial: u32) {
        let (Some(window), Some(xdg), Some(seat)) = (&self.window, &self.xdg, &self.seat) else {
            return;
        };
        let Ok(positioner) = XdgPositioner::new(xdg) else {
            return;
        };
        positioner.set_size(100, 80);
        positioner.set_anchor_rect(10, 10, 1, 1);
        let Ok(popup) = Popup::new(window.xdg_surface(), &positioner, qh, &self.compositor, xdg)
        else {
            return;
        };
        popup.xdg_popup().grab(seat, serial);
        popup.wl_surface().commit();
        self.popup = Some(popup);
    }
}

impl WindowHandler for Client {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {}

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        window: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        if let (Some(w), Some(h)) = configure.new_size {
            self.size = (w.get(), h.get());
        }
        let surface = window.wl_surface().clone();
        let m = self.margin as i32;
        if m > 0 {
            window
                .xdg_surface()
                .set_window_geometry(m, m, self.size.0 as i32, self.size.1 as i32);
        }
        self.draw_framed(&surface, self.size, self.margin);
    }
}

impl PopupHandler for Client {
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        popup: &Popup,
        config: PopupConfigure,
    ) {
        let surface = popup.wl_surface().clone();
        let size = (config.width.max(1) as u32, config.height.max(1) as u32);
        self.draw(&surface, size);
        let _ = self.events.send(Event::PopupShown);
    }

    fn done(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Popup) {
        self.popup = None;
        let _ = self.events.send(Event::PopupDone);
    }
}

impl LayerShellHandler for Client {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {}

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        self.size = (configure.new_size.0.max(1), configure.new_size.1.max(1));
        let surface = layer.wl_surface().clone();
        self.draw(&surface, self.size);
    }
}

impl SeatHandler for Client {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.popup_on_press {
            let _ = self.seats.get_pointer(qh, &seat);
            self.seat = Some(seat);
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        _: Capability,
    ) {
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for Client {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if let PointerEventKind::Press { serial, .. } = event.kind
                && self.popup.is_none()
            {
                self.open_popup(qh, serial);
            }
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

    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(Client);
delegate_output!(Client);
delegate_shm!(Client);
delegate_seat!(Client);
delegate_pointer!(Client);
delegate_xdg_shell!(Client);
delegate_xdg_window!(Client);
delegate_xdg_popup!(Client);
delegate_layer!(Client);
delegate_registry!(Client);

/// Opens a window and waits until the compositor has mapped it.
pub fn open_with(
    hp: &mut Headless,
    app_id: &'static str,
    size: (u32, u32),
    popup: bool,
) -> anyhow::Result<(Rect, ClientHandle)> {
    open_showing(
        hp,
        app_id,
        Shows::Window {
            app_id,
            size,
            popup,
        },
    )
}

/// Starts a client showing `shows`, a window with `app_id`, and waits until
/// the compositor has mapped it.
pub fn open_showing(
    hp: &mut Headless,
    app_id: &'static str,
    shows: Shows,
) -> anyhow::Result<(Rect, ClientHandle)> {
    let before = hp.windows().len();
    let handle = client(hp.socket(), Some(shows))?;
    let mapped = hp.dispatch_until(WAIT, |hp| hp.windows().len() > before)?;
    anyhow::ensure!(mapped, "{app_id} was never mapped");
    let rect = rect_of(hp, app_id).context("no such window")?;
    Ok((rect, handle))
}

pub fn open(hp: &mut Headless, app_id: &'static str, size: (u32, u32)) -> anyhow::Result<Rect> {
    let (rect, handle) = open_with(hp, app_id, size, false)?;
    // The client runs as long as the compositor does.
    std::mem::forget(handle);
    Ok(rect)
}

/// A mapped window's geometry.
pub fn rect_of(hp: &Headless, app_id: &str) -> Option<Rect> {
    hp.windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some(app_id))
        .map(|w| w.rect)
}

/// Runs the compositor until `app_id`'s geometry is `rect`.
pub fn wait_for_rect(hp: &mut Headless, app_id: &str, rect: Rect) -> anyhow::Result<()> {
    let reached = hp.dispatch_until(WAIT, |hp| rect_of(hp, app_id) == Some(rect))?;
    anyhow::ensure!(
        reached,
        "{app_id} at {:?}, not {rect:?}",
        rect_of(hp, app_id)
    );
    Ok(())
}

/// Presses the keys in order, then releases them in reverse.
pub fn chord(hp: &mut Headless, keys: &[u32]) {
    for key in keys {
        hp.key(*key, true);
    }
    for key in keys.iter().rev() {
        hp.key(*key, false);
    }
}

/// Runs the compositor for `time` of real time.
pub fn wait(hp: &mut Headless, time: Duration) -> anyhow::Result<()> {
    hp.dispatch_until(time, |_| false)?;
    Ok(())
}
