//! A drm-lease client, as a VR runtime is, for trying the session's leases
//! (#123) on a real GPU, in the VM:
//!
//!   hocus-lease globals [--sandboxed]   the globals a client sees; through
//!                                       a security context with --sandboxed
//!   hocus-lease take [--color #rrggbb] [--seconds N]
//!                                       leases the first connector offered,
//!                                       lights it in a colour itself, holds
//!                                       it, then lets go
//!
//! It talks to the compositor of `$WAYLAND_DISPLAY` in `$XDG_RUNTIME_DIR`.

use std::{
    os::{
        fd::{AsFd, BorrowedFd, OwnedFd},
        unix::net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::Context;
use clap::{Parser, Subcommand};
use drm::control::{Device as _, ModeTypeFlags, connector};
use wayland_client::{
    Connection, Dispatch, QueueHandle, event_created_child,
    globals::{GlobalListContents, registry_queue_init},
    protocol::wl_registry,
};
use wayland_protocols::wp::{
    drm_lease::v1::client::{
        wp_drm_lease_connector_v1::{self, WpDrmLeaseConnectorV1},
        wp_drm_lease_device_v1::{self, WpDrmLeaseDeviceV1},
        wp_drm_lease_request_v1::WpDrmLeaseRequestV1,
        wp_drm_lease_v1::{self, WpDrmLeaseV1},
    },
    security_context::v1::client::{
        wp_security_context_manager_v1::WpSecurityContextManagerV1,
        wp_security_context_v1::WpSecurityContextV1,
    },
};

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Lists the globals a client sees, one interface a line.
    Globals {
        /// As a client started through a security context (a sandbox).
        #[arg(long)]
        sandboxed: bool,
    },
    /// Leases the first connector offered and lights it.
    Take {
        /// `#rrggbb`.
        #[arg(long, default_value = "#cc3333")]
        color: String,
        /// How long the lease is held.
        #[arg(long, default_value_t = 30)]
        seconds: u64,
    },
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let runtime = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").context("no XDG_RUNTIME_DIR")?);
    let socket = runtime.join(std::env::var_os("WAYLAND_DISPLAY").context("no WAYLAND_DISPLAY")?);
    match args.command {
        Command::Globals { sandboxed: false } => {
            for interface in interfaces(&socket)? {
                println!("{interface}");
            }
        }
        Command::Globals { sandboxed: true } => {
            let dir = tempfile::tempdir()?;
            for interface in sandboxed(&socket, dir.path())? {
                println!("{interface}");
            }
        }
        Command::Take { color, seconds } => {
            let rgb = u32::from_str_radix(color.trim_start_matches('#'), 16)?;
            take(&socket, rgb, Duration::from_secs(seconds))?;
        }
    }
    Ok(())
}

#[derive(Default)]
struct Client {
    connectors: Vec<Offered>,
    /// Lease devices (one a GPU) that told all they offer.
    devices_done: usize,
    lease_fd: Option<OwnedFd>,
    finished: bool,
}

#[derive(Debug)]
struct Offered {
    /// The lease device offering it, by index.
    device: usize,
    proxy: WpDrmLeaseConnectorV1,
    name: String,
    description: String,
    id: u32,
    withdrawn: bool,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Client {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpSecurityContextManagerV1, ()> for Client {
    fn event(
        _: &mut Self,
        _: &WpSecurityContextManagerV1,
        _: <WpSecurityContextManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpSecurityContextV1, ()> for Client {
    fn event(
        _: &mut Self,
        _: &WpSecurityContextV1,
        _: <WpSecurityContextV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpDrmLeaseDeviceV1, usize> for Client {
    fn event(
        state: &mut Self,
        _: &WpDrmLeaseDeviceV1,
        event: wp_drm_lease_device_v1::Event,
        device: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            // The GPU's node, for what is leased from it: not needed here.
            wp_drm_lease_device_v1::Event::DrmFd { .. } => {}
            wp_drm_lease_device_v1::Event::Connector { id } => state.connectors.push(Offered {
                device: *device,
                proxy: id,
                name: String::new(),
                description: String::new(),
                id: 0,
                withdrawn: false,
            }),
            wp_drm_lease_device_v1::Event::Done => state.devices_done += 1,
            _ => {}
        }
    }

    event_created_child!(Client, WpDrmLeaseDeviceV1, [
        wp_drm_lease_device_v1::EVT_CONNECTOR_OPCODE => (WpDrmLeaseConnectorV1, ()),
    ]);
}

impl Dispatch<WpDrmLeaseConnectorV1, ()> for Client {
    fn event(
        state: &mut Self,
        proxy: &WpDrmLeaseConnectorV1,
        event: wp_drm_lease_connector_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(offered) = state.connectors.iter_mut().find(|c| c.proxy == *proxy) else {
            return;
        };
        match event {
            wp_drm_lease_connector_v1::Event::Name { name } => offered.name = name,
            wp_drm_lease_connector_v1::Event::Description { description } => {
                offered.description = description;
            }
            wp_drm_lease_connector_v1::Event::ConnectorId { connector_id } => {
                offered.id = connector_id;
            }
            wp_drm_lease_connector_v1::Event::Withdrawn => offered.withdrawn = true,
            _ => {}
        }
    }
}

impl Dispatch<WpDrmLeaseRequestV1, ()> for Client {
    fn event(
        _: &mut Self,
        _: &WpDrmLeaseRequestV1,
        _: <WpDrmLeaseRequestV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpDrmLeaseV1, ()> for Client {
    fn event(
        state: &mut Self,
        _: &WpDrmLeaseV1,
        event: wp_drm_lease_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wp_drm_lease_v1::Event::LeaseFd { leased_fd } => state.lease_fd = Some(leased_fd),
            wp_drm_lease_v1::Event::Finished => state.finished = true,
            _ => {}
        }
    }
}

fn interfaces(socket: &Path) -> anyhow::Result<Vec<String>> {
    let conn = Connection::from_socket(UnixStream::connect(socket)?)?;
    let (globals, _queue) = registry_queue_init::<Client>(&conn)?;
    Ok(globals
        .contents()
        .clone_list()
        .into_iter()
        .map(|g| g.interface)
        .collect())
}

/// What a client started through a security context sees.
fn sandboxed(socket: &Path, dir: &Path) -> anyhow::Result<Vec<String>> {
    let conn = Connection::from_socket(UnixStream::connect(socket)?)?;
    let (globals, mut queue) = registry_queue_init::<Client>(&conn)?;
    let qh = queue.handle();
    let manager: WpSecurityContextManagerV1 = globals
        .bind(&qh, 1..=1, ())
        .context("no security contexts")?;
    let path = dir.join("sandbox");
    let listener = UnixListener::bind(&path)?;
    // The sandbox lives until this end closes.
    let (close_read, _close_write) = rustix::pipe::pipe()?;
    let context = manager.create_listener(listener.as_fd(), close_read.as_fd(), &qh, ());
    context.set_sandbox_engine("org.hocuspanda.test".into());
    context.set_app_id("sandboxed".into());
    context.commit();
    queue.roundtrip(&mut Client::default())?;
    interfaces(&path)
}

/// The leased objects, through the fd the lease came with.
struct Card(OwnedFd);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl drm::Device for Card {}
impl drm::control::Device for Card {}

fn take(socket: &Path, rgb: u32, hold: Duration) -> anyhow::Result<()> {
    let conn = Connection::from_socket(UnixStream::connect(socket)?)?;
    let (globals, mut queue) = registry_queue_init::<Client>(&conn)?;
    let qh = queue.handle();
    // One lease device a GPU.
    let devices: Vec<WpDrmLeaseDeviceV1> = globals
        .contents()
        .clone_list()
        .into_iter()
        .filter(|g| g.interface == "wp_drm_lease_device_v1")
        .enumerate()
        .map(|(i, g)| globals.registry().bind(g.name, 1, &qh, i))
        .collect();
    anyhow::ensure!(!devices.is_empty(), "no drm-lease device offered");
    let mut client = Client::default();
    while client.devices_done < devices.len() {
        queue.blocking_dispatch(&mut client)?;
    }
    for c in &client.connectors {
        println!(
            "offered: {} ({}), connector {}",
            c.name, c.description, c.id
        );
    }
    let offered = client
        .connectors
        .iter()
        .find(|c| !c.withdrawn)
        .context("no connector offered for lease")?;
    let name = offered.name.clone();
    let device = &devices[offered.device];
    let request = device.create_lease_request(&qh, ());
    request.request_connector(&offered.proxy);
    let lease = request.submit(&qh, ());
    while client.lease_fd.is_none() && !client.finished {
        queue.blocking_dispatch(&mut client)?;
    }
    let fd = client.lease_fd.take().context("the lease was refused")?;
    println!("leased: {name}");
    let card = Card(fd);
    light(&card, rgb).context("could not light the leased connector")?;
    println!("lit: {name}");
    let end = Instant::now() + hold;
    while Instant::now() < end && !client.finished {
        // Hears whether the compositor ended the lease.
        queue.roundtrip(&mut client)?;
        std::thread::sleep(Duration::from_millis(100));
    }
    if client.finished {
        println!("revoked: {name}");
    }
    lease.destroy();
    for device in &devices {
        device.release();
    }
    queue.roundtrip(&mut client)?;
    println!("released: {name}");
    Ok(())
}

/// Lights the one leased connector in one colour, through its CRTC: the
/// lessee drives it, not the compositor.
fn light(card: &Card, rgb: u32) -> anyhow::Result<()> {
    let resources = card.resource_handles()?;
    let connector = resources
        .connectors()
        .iter()
        .filter_map(|c| card.get_connector(*c, true).ok())
        .find(|c| c.state() == connector::State::Connected)
        .context("no connected connector in the lease")?;
    let crtc = *resources.crtcs().first().context("no CRTC in the lease")?;
    let mode = connector
        .modes()
        .iter()
        .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
        .or_else(|| connector.modes().first())
        .copied()
        .context("no mode")?;
    let (w, h) = mode.size();
    let mut buffer =
        card.create_dumb_buffer((w.into(), h.into()), drm::buffer::DrmFourcc::Xrgb8888, 32)?;
    {
        let mut map = card.map_dumb_buffer(&mut buffer)?;
        for pixel in map.as_chunks_mut::<4>().0 {
            *pixel = (0xff00_0000 | rgb).to_le_bytes();
        }
    }
    let framebuffer = card.add_framebuffer(&buffer, 24, 32)?;
    card.set_crtc(
        crtc,
        Some(framebuffer),
        (0, 0),
        &[connector.handle()],
        Some(mode),
    )?;
    Ok(())
}
