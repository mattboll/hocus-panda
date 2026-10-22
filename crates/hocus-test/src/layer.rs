//! A layer-shell test client: one solid surface in a layer, with its
//! anchors, exclusive zone and keyboard interactivity; it records the
//! configures and keys it receives.

use std::{
    os::unix::net::UnixStream,
    path::Path,
    sync::{Arc, Mutex},
    thread::JoinHandle,
};

use anyhow::Context;
use rustix::event::{PollFd, PollFlags, Timespec};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, Dispatch, QueueHandle, WEnum,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_seat, wl_shm, wl_surface},
};

use crate::client::ClientEvent;

/// A layer surface to show.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerSpec {
    pub namespace: String,
    /// `background`, `bottom`, `top` or `overlay`.
    pub layer: String,
    /// Edges among `top`, `bottom`, `left`, `right`.
    #[serde(default)]
    pub anchor: Vec<String>,
    /// 0 along an axis anchored to both of its edges: the whole length.
    pub size: [u32; 2],
    #[serde(default)]
    pub exclusive_zone: i32,
    /// `none`, `exclusive` or `on_demand`.
    #[serde(default = "none")]
    pub keyboard: String,
    /// `#rrggbb`.
    pub color: String,
}

fn none() -> String {
    "none".into()
}

/// A running layer client.
#[derive(Debug)]
pub struct LayerClient {
    pub spec: LayerSpec,
    events: Arc<Mutex<Vec<ClientEvent>>>,
    stop: Arc<Mutex<bool>>,
    thread: JoinHandle<anyhow::Result<()>>,
}

impl LayerClient {
    pub fn events(&self) -> Vec<ClientEvent> {
        self.events.lock().map(|e| e.clone()).unwrap_or_default()
    }

    /// Whether its surface is shown: configured and drawn, and holding
    /// the keyboard when it asked for it always.
    pub fn is_drawn(&self) -> bool {
        let events = self.events();
        events
            .iter()
            .any(|e| matches!(e, ClientEvent::Configure { .. }))
            && (self.spec.keyboard != "exclusive" || events.contains(&ClientEvent::KeyboardEnter))
    }

    /// Destroys the surface and disconnects.
    pub fn close(&self) {
        if let Ok(mut stop) = self.stop.lock() {
            *stop = true;
        }
    }

    pub fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }
}

impl LayerSpec {
    pub fn spawn(self, socket: &Path, argb: u32) -> anyhow::Result<LayerClient> {
        let stream = UnixStream::connect(socket)
            .with_context(|| format!("could not connect to {}", socket.display()))?;
        let conn = Connection::from_socket(stream)?;
        let events = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(Mutex::new(false));
        let thread = std::thread::Builder::new()
            .name(format!("layer {}", self.namespace))
            .spawn({
                let spec = self.clone();
                let events = events.clone();
                let stop = stop.clone();
                move || spec.run(conn, argb, events, stop)
            })?;
        Ok(LayerClient {
            spec: self,
            events,
            stop,
            thread,
        })
    }

    fn run(
        self,
        conn: Connection,
        argb: u32,
        events: Arc<Mutex<Vec<ClientEvent>>>,
        stop: Arc<Mutex<bool>>,
    ) -> anyhow::Result<()> {
        let (globals, mut queue) = registry_queue_init(&conn)?;
        let qh = queue.handle();
        let compositor = CompositorState::bind(&globals, &qh).context("no wl_compositor")?;
        let shell = LayerShell::bind(&globals, &qh).context("no layer shell")?;
        let shm = Shm::bind(&globals, &qh).context("no wl_shm")?;
        let _seat: wl_seat::WlSeat = globals.bind(&qh, 1..=7, ()).context("no wl_seat")?;
        let layer = match self.layer.as_str() {
            "background" => Layer::Background,
            "bottom" => Layer::Bottom,
            "overlay" => Layer::Overlay,
            _ => Layer::Top,
        };
        let surface = compositor.create_surface(&qh);
        let layer_surface =
            shell.create_layer_surface(&qh, surface, layer, Some(self.namespace.clone()), None);
        let mut anchor = Anchor::empty();
        for edge in &self.anchor {
            anchor |= match edge.as_str() {
                "top" => Anchor::TOP,
                "bottom" => Anchor::BOTTOM,
                "left" => Anchor::LEFT,
                "right" => Anchor::RIGHT,
                other => anyhow::bail!("no edge {other}"),
            };
        }
        layer_surface.set_anchor(anchor);
        layer_surface.set_size(self.size[0], self.size[1]);
        layer_surface.set_exclusive_zone(self.exclusive_zone);
        layer_surface.set_keyboard_interactivity(match self.keyboard.as_str() {
            "exclusive" => KeyboardInteractivity::Exclusive,
            "on_demand" => KeyboardInteractivity::OnDemand,
            _ => KeyboardInteractivity::None,
        });
        layer_surface.commit();
        let pool = SlotPool::new(256 * 256 * 4, &shm)?;
        let mut client = Client {
            registry: RegistryState::new(&globals),
            outputs: OutputState::new(&globals, &qh),
            shm,
            pool,
            layer: Some(layer_surface),
            argb,
            events,
            closed: false,
        };
        while !client.closed {
            if stop.lock().map(|s| *s).unwrap_or(true) {
                // Gone: its surface with it.
                client.layer = None;
                queue.flush()?;
                break;
            }
            queue.flush()?;
            queue.dispatch_pending(&mut client)?;
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

struct Client {
    registry: RegistryState,
    outputs: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: Option<LayerSurface>,
    argb: u32,
    events: Arc<Mutex<Vec<ClientEvent>>>,
    closed: bool,
}

impl Client {
    fn record(&self, event: ClientEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
    }
}

impl LayerShellHandler for Client {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.closed = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let (w, h) = (configure.new_size.0.max(1), configure.new_size.1.max(1));
        let drawn = (|| -> anyhow::Result<()> {
            let (buffer, canvas) = self.pool.create_buffer(
                w as i32,
                h as i32,
                w as i32 * 4,
                wl_shm::Format::Argb8888,
            )?;
            for pixel in canvas.as_chunks_mut::<4>().0 {
                *pixel = self.argb.to_le_bytes();
            }
            let surface = layer.wl_surface();
            surface.damage_buffer(0, 0, w as i32, h as i32);
            buffer.attach_to(surface)?;
            layer.commit();
            Ok(())
        })();
        if let Err(err) = drawn {
            eprintln!("layer client could not draw: {err:#}");
            self.closed = true;
        }
        self.record(ClientEvent::Configure {
            width: w,
            height: h,
        });
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
            && caps.contains(wl_seat::Capability::Keyboard)
        {
            seat.get_keyboard(qh, ());
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
delegate_layer!(Client);
delegate_registry!(Client);
