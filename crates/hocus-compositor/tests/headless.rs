//! The compositor's base, headless: real Wayland clients (on threads of
//! their own) connect to an in-process compositor, which the test drives.

use std::{os::unix::net::UnixStream, path::Path, sync::mpsc, time::Duration};

use anyhow::Context;
use hocus_compositor::{Headless, HeadlessConfig, RendererChoice, model::Rect};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
    delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
        },
        xdg::{
            XdgShell,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_shm, wl_surface},
};

const WAIT: Duration = Duration::from_secs(10);
const BTN_LEFT: u32 = 0x110;
const KEY_LEFTMETA: u32 = 125;

fn compositor() -> anyhow::Result<Headless> {
    Headless::start(HeadlessConfig {
        // The same pixels on every machine, and no GPU needed.
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    })
}

/// What a test client shows.
#[derive(Debug, Clone)]
enum Shows {
    /// An xdg toplevel of this size, with this app id.
    Window {
        app_id: &'static str,
        size: (u32, u32),
    },
    /// A layer surface along the output's top edge, `height` high, which
    /// reserves its height.
    Bar { height: u32 },
}

/// Starts a client on a thread of its own. It runs until the compositor
/// goes; it sends the names of the globals it saw once it has them.
fn client(socket: &Path, shows: Option<Shows>) -> anyhow::Result<mpsc::Receiver<Vec<String>>> {
    let stream = UnixStream::connect(socket)
        .with_context(|| format!("could not connect to {}", socket.display()))?;
    let conn = Connection::from_socket(stream)?;
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("test client".into())
        .spawn(move || {
            if let Err(err) = run_client(&conn, shows, &sender) {
                // The compositor going ends every client.
                eprintln!("test client: {err:#}");
            }
        })?;
    Ok(receiver)
}

fn run_client(
    conn: &Connection,
    shows: Option<Shows>,
    globals_seen: &mpsc::Sender<Vec<String>>,
) -> anyhow::Result<()> {
    let (globals, mut queue) = registry_queue_init(conn)?;
    let qh = queue.handle();
    let names = globals.contents().with_list(|list| {
        list.iter()
            .map(|g| format!("{} v{}", g.interface, g.version))
            .collect()
    });
    globals_seen.send(names)?;
    let compositor = CompositorState::bind(&globals, &qh).context("no wl_compositor")?;
    let shm = Shm::bind(&globals, &qh).context("no wl_shm")?;
    let mut client = Client {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        pool: SlotPool::new(512 * 512 * 4, &shm)?,
        shm,
        size: (1, 1),
        window: None,
        layer: None,
    };
    match shows {
        Some(Shows::Window { app_id, size }) => {
            let xdg = XdgShell::bind(&globals, &qh).context("no xdg_wm_base")?;
            let surface = compositor.create_surface(&qh);
            let window = xdg.create_window(surface, WindowDecorations::RequestServer, &qh);
            window.set_app_id(app_id);
            window.commit();
            client.size = size;
            client.window = Some(window);
        }
        Some(Shows::Bar { height }) => {
            let shell = LayerShell::bind(&globals, &qh).context("no layer shell")?;
            let surface = compositor.create_surface(&qh);
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
        queue.blocking_dispatch(&mut client)?;
    }
}

struct Client {
    registry: RegistryState,
    outputs: OutputState,
    shm: Shm,
    pool: SlotPool,
    size: (u32, u32),
    window: Option<Window>,
    layer: Option<LayerSurface>,
}

impl Client {
    /// Fills `surface` with a solid colour of the current size.
    fn draw(&mut self, surface: &wl_surface::WlSurface) {
        let (w, h) = (self.size.0.max(1) as i32, self.size.1.max(1) as i32);
        let Ok((buffer, canvas)) = self
            .pool
            .create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)
        else {
            return;
        };
        for pixel in canvas.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&0xff3a_7bd5_u32.to_le_bytes());
        }
        surface.damage_buffer(0, 0, w, h);
        if buffer.attach_to(surface).is_ok() {
            surface.commit();
        }
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
        self.draw(&surface);
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
        self.draw(&surface);
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
delegate_layer!(Client);
delegate_registry!(Client);

/// Opens a window and waits until the compositor has mapped it.
fn open(hp: &mut Headless, app_id: &'static str, size: (u32, u32)) -> anyhow::Result<Rect> {
    let before = hp.windows().len();
    let _globals = client(hp.socket(), Some(Shows::Window { app_id, size }))?;
    let mapped = hp.dispatch_until(WAIT, |hp| hp.windows().len() > before)?;
    anyhow::ensure!(mapped, "{app_id} was never mapped");
    hp.windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some(app_id))
        .map(|w| w.rect)
        .context("no such window")
}

#[test]
fn a_new_window_opens_at_the_centre_of_the_work_area() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    // 1920×1080: 8 px in on every side, and the full bar's place (196 px,
    // 8 px from the edge) kept on the right.
    assert_eq!(hp.work_area(), Some(Rect::new(8, 8, 1700, 1064)));
    let rect = open(&mut hp, "test.centre", (400, 300))?;
    assert_eq!(rect, Rect::new(8 + 650, 8 + 382, 400, 300));
    // A new window has the focus, and its surface the keyboard.
    let window = &hp.windows()[0];
    assert!(window.focused && window.keyboard, "{window:?}");
    // The frame was drawn once something changed.
    let drawn = hp.dispatch_until(WAIT, |hp| hp.frames() > 0)?;
    assert!(drawn, "no frame drawn");
    Ok(())
}

#[test]
fn a_click_focuses_and_raises_the_window_under_the_pointer() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let first = open(&mut hp, "test.first", (400, 300))?;
    let second = open(&mut hp, "test.second", (200, 100))?;
    let focused = |hp: &Headless| {
        hp.windows()
            .into_iter()
            .find(|w| w.focused)
            .and_then(|w| w.app_id)
    };
    assert_eq!(focused(&hp).as_deref(), Some("test.second"));
    // Within the first, clear of the second (centred over it).
    let (x, y) = (f64::from(first.x + 10), f64::from(first.y + 10));
    assert!(!(second.x..second.x + second.width).contains(&(first.x + 10)));
    hp.pointer_move(x, y);
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    hp.dispatch(Duration::from_millis(10))?;
    assert_eq!(focused(&hp).as_deref(), Some("test.first"));
    let windows = hp.windows();
    let top = windows.last().context("no window")?;
    assert_eq!(top.app_id.as_deref(), Some("test.first"), "raised");
    assert!(top.keyboard, "the keyboard follows the click");
    // A click where no window is changes nothing.
    hp.pointer_move(2.0, 2.0);
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    assert_eq!(focused(&hp).as_deref(), Some("test.first"));
    Ok(())
}

#[test]
fn super_and_drag_moves_a_window() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let start = open(&mut hp, "test.moved", (400, 300))?;
    let (x, y) = (f64::from(start.x + 200), f64::from(start.y + 150));
    hp.pointer_move(x, y);
    hp.key(KEY_LEFTMETA, true);
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x + 60.0, y + 40.0);
    hp.pointer_move(x + 100.0, y + 50.0);
    hp.button(BTN_LEFT, false);
    hp.key(KEY_LEFTMETA, false);
    let rect = hp.windows()[0].rect;
    assert_eq!(
        rect,
        Rect::new(start.x + 100, start.y + 50, 400, 300),
        "the window follows the pointer"
    );
    // Without Super, a drag is the application's.
    hp.button(BTN_LEFT, true);
    hp.pointer_move(x + 300.0, y + 50.0);
    hp.button(BTN_LEFT, false);
    assert_eq!(hp.windows()[0].rect, rect);
    Ok(())
}

#[test]
fn a_bars_exclusive_zone_comes_off_the_work_area() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let _bar = client(hp.socket(), Some(Shows::Bar { height: 40 }))?;
    let shrunk = hp.dispatch_until(WAIT, |hp| {
        hp.work_area() == Some(Rect::new(8, 48, 1700, 1024))
    })?;
    assert!(shrunk, "work area: {:?}", hp.work_area());
    // New windows open at the centre of what is left.
    let rect = open(&mut hp, "test.under-bar", (400, 300))?;
    assert_eq!(rect, Rect::new(8 + 650, 48 + 362, 400, 300));
    Ok(())
}

#[test]
fn the_protocols_of_the_base_are_offered() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let globals = client(hp.socket(), None)?;
    let mut seen = None;
    hp.dispatch_until(WAIT, |_| {
        if seen.is_none() {
            seen = globals.try_recv().ok();
        }
        seen.is_some()
    })?;
    let seen = seen.context("the client never listed the globals")?;
    let has = |name: &str| seen.iter().any(|g| g.split(' ').next() == Some(name));
    for name in [
        "wl_compositor",
        "wl_shm",
        "wl_seat",
        "wl_output",
        "zxdg_output_manager_v1",
        "xdg_wm_base",
        "zxdg_decoration_manager_v1",
        "zwlr_layer_shell_v1",
        "zwp_linux_dmabuf_v1",
        "wp_viewporter",
        "wp_fractional_scale_manager_v1",
        "wp_presentation",
        "xdg_activation_v1",
        "wp_cursor_shape_manager_v1",
        "wl_data_device_manager",
        "zwp_primary_selection_device_manager_v1",
        "zwp_pointer_gestures_v1",
        "zwp_relative_pointer_manager_v1",
    ] {
        assert!(has(name), "{name} is not offered: {seen:?}");
    }
    assert!(hp.has_dmabuf());
    // Headless with pixman, no device to give feedback for: version 3.
    // Nested and on screens, the GPU's render node gives version 4 or more.
    assert!(
        seen.iter().any(|g| g == "zwp_linux_dmabuf_v1 v3"),
        "{seen:?}"
    );
    Ok(())
}
