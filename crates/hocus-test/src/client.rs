//! A test client: one window filled with a solid colour.

use std::{os::unix::net::UnixStream, path::Path, thread::JoinHandle};

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
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_shm, wl_surface},
};

/// A window of `width`×`height` pixels filled with `argb`.
#[derive(Debug, Clone, Copy)]
pub struct SolidWindow {
    pub width: u32,
    pub height: u32,
    pub argb: u32,
}

impl SolidWindow {
    /// Connects to the compositor at `socket` and shows the window from a
    /// thread of its own, until the compositor closes it or goes away.
    pub fn spawn(self, socket: &Path) -> anyhow::Result<JoinHandle<anyhow::Result<()>>> {
        let stream = UnixStream::connect(socket)
            .with_context(|| format!("could not connect to {}", socket.display()))?;
        let conn = Connection::from_socket(stream)?;
        Ok(std::thread::spawn(move || self.run(conn)))
    }

    fn run(self, conn: Connection) -> anyhow::Result<()> {
        let (globals, mut queue) = registry_queue_init(&conn)?;
        let qh = queue.handle();
        let compositor = CompositorState::bind(&globals, &qh).context("no wl_compositor")?;
        let xdg_shell = XdgShell::bind(&globals, &qh).context("no xdg_wm_base")?;
        let shm = Shm::bind(&globals, &qh).context("no wl_shm")?;

        let surface = compositor.create_surface(&qh);
        let window = xdg_shell.create_window(surface, WindowDecorations::None, &qh);
        window.set_app_id("hocus-test.solid");
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
    drawn: bool,
    closed: bool,
}

impl Client {
    fn draw(&mut self) -> anyhow::Result<()> {
        let SolidWindow {
            width,
            height,
            argb,
        } = self.spec;
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
        self.closed = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &Window,
        _: WindowConfigure,
        _: u32,
    ) {
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
