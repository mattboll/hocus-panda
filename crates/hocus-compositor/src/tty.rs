//! The session itself, on a virtual terminal: the seat comes from libseat
//! (logind or seatd), the screens are driven through DRM/KMS and drawn with
//! GLES, input comes from libinput. Switching to another terminal pauses
//! all of it; coming back draws everything again.
//!
//! Every GPU's screens are lit (#123): the primary GPU draws them all, and
//! the frames of another GPU's screens reach it as `gpus::Path` says,
//! scanned out from the primary's buffers or copied across by smithay's
//! multi-GPU renderer. GPUs come and go with their screens. A connector
//! that says it is no desktop (a VR headset) is never lit: it is offered
//! for lease (`leases`).

use std::{
    collections::HashMap,
    path::{Path as FsPath, PathBuf},
    time::Instant,
};

use anyhow::Context;
use smithay::{
    backend::{
        allocator::{
            Allocator as _, Fourcc, Modifier,
            dmabuf::{AsDmabuf, Dmabuf},
            format::FormatSet,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        },
        drm::{
            DrmDevice, DrmDeviceFd, DrmEvent, DrmEventMetadata, DrmEventTime, DrmNode, NodeType,
            compositor::FrameFlags,
            exporter::gbm::GbmFramebufferExporter,
            gbm::framebuffer_from_dmabuf,
            output::{DrmOutput, DrmOutputManager, DrmOutputRenderElements},
        },
        egl::{EGLDevice, EGLDisplay},
        input::InputEvent,
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            Bind, ImportDma, ImportMemWl,
            damage::OutputDamageTracker,
            element::{Id, RenderElementStates},
            gles::{GlesRenderer, GlesTexture},
            multigpu::{GpuManager, MultiRenderer, MultiTexture, gbm::GbmGlesBackend},
            utils::CommitCounter,
        },
        session::{Event as SessionEvent, Session, libseat::LibSeatSession},
        udev::{UdevBackend, UdevEvent, all_gpus, primary_gpu},
    },
    desktop::Window,
    output::{Mode, Output, PhysicalProperties, Scale},
    reexports::{
        calloop::{
            LoopHandle, RegistrationToken,
            timer::{TimeoutAction, Timer},
        },
        drm::control::{self, Device as ControlDevice, ModeTypeFlags, connector, crtc, property},
        input::Libinput,
        rustix::fs::{Dev, OFlags},
        wayland_server::backend::GlobalId,
    },
    utils::{DeviceFd, Physical, Size, Transform},
    wayland::dmabuf::{DmabufFeedbackBuilder, DmabufGlobal},
};

use hocus_api::Event;

use crate::{
    App, BACKGROUND, BackendState, State,
    capture::{self, Pixels},
    clips::clipped,
    copied::CopiedFrame,
    flip::{self, Flip, Health},
    gpus::{self, Change, Found, Known, Path, Probe, Use},
    leases::Offer,
    output_spec::DEFAULT_DESKTOPS,
    outputs::{Head, Setting},
    pointing::PointingDevices,
    scene::OutputElement,
};

type Allocator = GbmAllocator<DrmDeviceFd>;
type Exporter = GbmFramebufferExporter<DrmDeviceFd>;
type Outputs = DrmOutputManager<Allocator, Exporter, (), DrmDeviceFd>;
type Surface = DrmOutput<Allocator, Exporter, (), DrmDeviceFd>;
type Api = GbmGlesBackend<GlesRenderer, DrmDeviceFd>;
type Gpus = GpuManager<Api>;
type Multi<'a> = MultiRenderer<'a, 'a, Api, Api>;

/// Scan-out formats, tried in order: 8 bits per channel, which every
/// display engine takes.
const COLOR_FORMATS: [Fourcc; 2] = [Fourcc::Argb8888, Fourcc::Abgr8888];

/// What the primary GPU draws a copied screen's frames as.
const COPY_FORMAT: Fourcc = Fourcc::Argb8888;

pub struct TtyBackend {
    session: LibSeatSession,
    libinput: Libinput,
    /// Mice and touchpads, set as the configuration says.
    pointing: PointingDevices,
    /// The renderers: the primary GPU's, which draws every screen, and
    /// those of the GPUs its frames are copied to.
    gpus: Gpus,
    /// The primary GPU's renderer, as `gpus` knows it.
    primary: DrmNode,
    /// The primary GPU's buffers, which display-only GPUs scan out.
    primary_allocator: Allocator,
    /// Every GPU, by its node.
    devices: HashMap<DrmNode, Device>,
    /// GPUs opened so far, for their order.
    opened: u64,
    /// The session is in the foreground: its terminal is the one shown.
    active: bool,
    /// For clients that draw on the GPU: the formats the primary can read.
    _dmabuf: DmabufGlobal,
}

impl std::fmt::Debug for TtyBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TtyBackend")
            .field("seat", &self.session.seat())
            .field("primary", &self.primary)
            .field(
                "gpus",
                &self
                    .devices
                    .values()
                    .map(|d| (d.path.clone(), d.how, d.screens.len()))
                    .collect::<Vec<_>>(),
            )
            .field("active", &self.active)
            .finish()
    }
}

/// A GPU: its display engine and its screens.
struct Device {
    node: DrmNode,
    path: PathBuf,
    /// `card1`: its screens are told apart by it when their names clash.
    card: String,
    /// Opened before the GPUs with a higher one: the primary first.
    order: u64,
    /// Its connectors' names, connected or not.
    connectors: Vec<String>,
    how: Path,
    /// Its own renderer in `gpus`, which takes the copies.
    target: Option<DrmNode>,
    outputs: Outputs,
    screens: HashMap<crtc::Handle, Screen>,
    /// Connected screens turned off: what is needed to light them again.
    off: Vec<Off>,
    notifier: Option<RegistrationToken>,
}

/// A connected screen turned off.
struct Off {
    connector: connector::Handle,
    head: Head,
}

/// One connector lit, through one CRTC.
struct Screen {
    connector: connector::Handle,
    /// What the connector offers, and what the head says about it.
    modes: Vec<control::Mode>,
    head: Head,
    vrr: bool,
    /// Dark through output-power: nothing is drawn.
    dark: bool,
    output: Output,
    global: GlobalId,
    surface: Surface,
    flip: Flip,
    /// Something changed since its last frame.
    dirty: bool,
    /// Frames presented, for presentation-time.
    presented: u64,
    /// The look at its frame out, while there is one (see [`flip`]).
    health: Option<RegistrationToken>,
    /// An empty frame went nowhere: clients hear of it at the next refresh
    /// instead of at once, or a client redrawing nothing would spin.
    pace: Option<RegistrationToken>,
    /// Where the primary GPU draws its frames, for a GPU they are copied
    /// to; made at its first frame.
    copy: Option<CopyBuffer>,
}

/// The buffer the primary GPU draws a copied screen's frames into, and
/// what the copy reads it as.
struct CopyBuffer {
    size: Size<i32, Physical>,
    buffer: Dmabuf,
    tracker: OutputDamageTracker,
    /// 0 until drawn once; then it holds the last frame.
    age: usize,
    texture: Option<MultiTexture>,
    id: Id,
    commit: CommitCounter,
}

impl Screen {
    /// The next frame is drawn whole: what the screen showed is not ours
    /// any more.
    fn redraw_all(&mut self) {
        self.surface.reset_buffers();
        if let Some(copy) = &mut self.copy {
            copy.age = 0;
        }
    }
}

/// What drawing a screen gave.
struct Drawn {
    /// Nothing changed: no frame goes out.
    empty: bool,
    states: RenderElementStates,
}

/// A GPU opened: its display engine and its buffers.
struct Opened {
    node: DrmNode,
    fd: DrmDeviceFd,
    drm: DrmDevice,
    notifier: smithay::backend::drm::DrmDeviceNotifier,
    gbm: GbmDevice<DrmDeviceFd>,
}

/// Takes the seat, opens its GPUs, lights the connected screens and starts
/// reading input.
pub fn init(handle: &LoopHandle<'static, App>, state: &mut State) -> anyhow::Result<TtyBackend> {
    let (session, notifier) = LibSeatSession::new()
        .map_err(|err| anyhow::anyhow!("{err}"))
        .context("could not take the seat (is logind or seatd running?)")?;
    let seat = session.seat();
    tracing::info!(seat, "seat taken");

    let path = primary_gpu(&seat)
        .ok()
        .flatten()
        .or_else(|| all_gpus(&seat).ok()?.into_iter().next())
        .context("no GPU on this seat")?;
    let mut libinput =
        Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(session.clone().into());
    libinput
        .udev_assign_seat(&seat)
        .map_err(|()| anyhow::anyhow!("libinput could not take seat {seat}"))?;
    let mut backend = open_primary(handle, &session, libinput.clone(), &path, state)?;
    handle
        .insert_source(
            LibinputInputBackend::new(libinput.clone()),
            |event, _, app| {
                app.timed("input", |app| {
                    let App { state, backend } = app;
                    if let BackendState::Tty(tty) = backend {
                        match &event {
                            InputEvent::DeviceAdded { device } => {
                                tty.pointing.added(device.clone(), &state.config);
                            }
                            InputEvent::DeviceRemoved { device } => tty.pointing.removed(device),
                            _ => {}
                        }
                    }
                    state.process_input_event(event);
                });
            },
        )
        .map_err(|err| err.error)
        .context("could not watch input devices")?;

    handle
        .insert_source(notifier, |event, (), app| {
            app.timed("session", |app| on_session(app, event));
        })
        .map_err(|err| err.error)
        .context("could not watch the session")?;

    // The other GPUs, then GPUs and screens plugged and unplugged.
    let udev = UdevBackend::new(&seat).context("could not watch devices")?;
    let mut others: Vec<PathBuf> = udev
        .device_list()
        .filter(|(id, _)| !backend.devices.values().any(|d| d.node.dev_id() == *id))
        .map(|(_, path)| path.to_path_buf())
        .collect();
    // In the same order from one start to the next, as their screens' names.
    others.sort();
    for path in others {
        if let Err(err) = backend.add_device(handle, &path, state) {
            tracing::warn!(gpu = %path.display(), "its screens stay dark: {err:#}");
        }
    }
    handle
        .insert_source(udev, |event, (), app| {
            app.timed("devices", |app| on_device(app, event));
        })
        .map_err(|err| err.error)
        .context("could not watch devices")?;

    // The primary's screens first: the others go to their right.
    for node in backend.nodes() {
        backend.scan(node, state);
    }
    anyhow::ensure!(
        backend.devices.values().any(|d| !d.screens.is_empty()),
        "no screen is connected"
    );
    state.vt_switch = Some(backend.session.clone());
    Ok(backend)
}

/// Opens a GPU through the seat.
fn open(session: &LibSeatSession, path: &FsPath) -> anyhow::Result<Opened> {
    let mut session = session.clone();
    let fd = session
        .open(
            path,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
        )
        .map_err(|err| anyhow::anyhow!("{err}"))
        .with_context(|| format!("could not open {}", path.display()))?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let node = DrmNode::from_file(&fd).context("not a DRM device")?;
    let (drm, notifier) = DrmDevice::new(fd.clone(), true).context("could not use the GPU")?;
    let gbm = GbmDevice::new(fd.clone()).context("could not create a GBM device")?;
    Ok(Opened {
        node,
        fd,
        drm,
        notifier,
        gbm,
    })
}

/// What a GPU draws with, and the node its renderer goes by.
fn probe_renderer(gbm: &GbmDevice<DrmDeviceFd>, node: DrmNode) -> (gpus::Renderer, DrmNode) {
    let render_node = node
        .node_with_type(NodeType::Render)
        .and_then(Result::ok)
        .unwrap_or(node);
    // SAFETY: nothing else in the process creates EGL displays, so
    // smithay's tracking of them is accurate: this one, which only looks,
    // and the renderer's for the same device share one, terminated when
    // the last goes.
    #[allow(unsafe_code)]
    let Ok(display) = (unsafe { EGLDisplay::new(gbm.clone()) }) else {
        return (gpus::Renderer::None, render_node);
    };
    match EGLDevice::device_for_display(&display) {
        Ok(device) if device.is_software() => (gpus::Renderer::Software, render_node),
        Ok(device) => (
            gpus::Renderer::Hardware,
            device
                .try_get_render_node()
                .ok()
                .flatten()
                .unwrap_or(render_node),
        ),
        Err(_) => (gpus::Renderer::Hardware, render_node),
    }
}

/// Whether a GPU takes a buffer the primary GPU drew as a framebuffer.
fn scans_out_shared(
    primary: &mut Allocator,
    fd: &DrmDeviceFd,
    gbm: &GbmDevice<DrmDeviceFd>,
) -> bool {
    let Ok(buffer) = primary.create_buffer(64, 64, COPY_FORMAT, &[Modifier::Linear]) else {
        return false;
    };
    let Ok(dmabuf) = buffer.export() else {
        return false;
    };
    match framebuffer_from_dmabuf(fd, gbm, &dmabuf, false, true) {
        Ok(_framebuffer) => true,
        Err(err) => {
            tracing::debug!(%err, "the GPU does not scan out the primary's buffers");
            false
        }
    }
}

/// Opens the primary GPU, whose renderer draws every screen.
fn open_primary(
    handle: &LoopHandle<'static, App>,
    session: &LibSeatSession,
    libinput: Libinput,
    path: &FsPath,
    state: &mut State,
) -> anyhow::Result<TtyBackend> {
    let opened = open(session, path)?;
    let (renderer, key) = probe_renderer(&opened.gbm, opened.node);
    if renderer == gpus::Renderer::Software {
        // A VM without 3D: llvmpipe draws, slowly but correctly.
        tracing::warn!("the GPU has no 3D acceleration: drawing in software");
    }
    let mut gpus = GpuManager::new(Api::default()).map_err(|err| anyhow::anyhow!("{err}"))?;
    gpus.as_mut()
        .add_node(key, opened.gbm.clone())
        .context("could not open EGL")?;
    let (render_formats, dmabuf) = {
        let mut renderer = gpus
            .single_renderer(&key)
            .map_err(|err| anyhow::anyhow!("{err}"))
            .context("could not start GLES")?;
        let gles: &mut GlesRenderer = renderer.as_mut();
        state.shm_state.update_formats(gles.shm_formats());
        let render_formats: FormatSet = gles.egl_context().dmabuf_render_formats().clone();
        let feedback = DmabufFeedbackBuilder::new(key.dev_id(), gles.dmabuf_formats())
            .build()
            .context("could not describe the GPU's buffer formats")?;
        let display = state.display_handle.clone();
        let dmabuf = state
            .dmabuf_state
            .create_global_with_default_feedback::<State>(&display, &feedback);
        (render_formats, dmabuf)
    };
    let allocator = GbmAllocator::new(
        opened.gbm.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );
    let outputs = DrmOutputManager::new(
        opened.drm,
        allocator.clone(),
        GbmFramebufferExporter::new(opened.gbm.clone(), Some(key)),
        Some(opened.gbm),
        COLOR_FORMATS,
        render_formats,
    );
    let node = opened.node;
    let notifier = watch(handle, node, opened.notifier)?;
    tracing::info!(gpu = %path.display(), render_node = %key, "GPU ready");
    let device = Device {
        node,
        path: path.to_path_buf(),
        card: card(path),
        order: 0,
        connectors: connector_names(outputs.device()),
        how: Path::Direct,
        target: None,
        outputs,
        screens: HashMap::new(),
        off: Vec::new(),
        notifier: Some(notifier),
    };
    state.leases.add_device(&state.display_handle, node);
    Ok(TtyBackend {
        session: session.clone(),
        libinput,
        pointing: PointingDevices::default(),
        gpus,
        primary: key,
        primary_allocator: allocator,
        devices: HashMap::from([(node, device)]),
        opened: 1,
        active: true,
        _dmabuf: dmabuf,
    })
}

/// Hears a GPU's vblanks.
fn watch(
    handle: &LoopHandle<'static, App>,
    node: DrmNode,
    notifier: smithay::backend::drm::DrmDeviceNotifier,
) -> anyhow::Result<RegistrationToken> {
    handle
        .insert_source(notifier, move |event, metadata, app| match event {
            DrmEvent::VBlank(crtc) => {
                app.timed_quietly("vblank", |app| on_vblank(app, node, crtc, metadata));
            }
            DrmEvent::Error(err) => tracing::warn!(%err, %node, "GPU error"),
        })
        .map_err(|err| err.error)
        .context("could not watch the GPU")
}

/// `card1`, from `/dev/dri/card1`.
fn card(path: &FsPath) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn connector_name(info: &connector::Info) -> String {
    format!("{}-{}", info.interface().as_str(), info.interface_id())
}

fn connector_names(device: &DrmDevice) -> Vec<String> {
    device
        .resource_handles()
        .map(|r| {
            r.connectors()
                .iter()
                .filter_map(|c| device.get_connector(*c, false).ok())
                .map(|info| connector_name(&info))
                .collect()
        })
        .unwrap_or_default()
}

fn on_session(app: &mut App, event: SessionEvent) {
    let BackendState::Tty(tty) = &mut app.backend else {
        return;
    };
    match event {
        SessionEvent::PauseSession => {
            tracing::info!("session paused: another terminal is shown");
            tty.active = false;
            tty.libinput.suspend();
            app.state.leases.suspend();
            for device in tty.devices.values_mut() {
                device.outputs.pause();
            }
        }
        SessionEvent::ActivateSession => {
            tracing::info!("session back");
            tty.active = true;
            if let Err(()) = tty.libinput.resume() {
                tracing::warn!("libinput could not resume");
            }
            for device in tty.devices.values_mut() {
                if let Err(err) = device.outputs.activate(false) {
                    tracing::warn!(%err, gpu = %device.path.display(), "could not take the screens back");
                }
                // What the screens show was another terminal's: draw it all.
                for screen in device.screens.values_mut() {
                    screen.flip = Flip::Idle;
                    screen.surface.with_compositor(|c| {
                        if let Err(err) = c.reset_state() {
                            tracing::warn!(%err, "could not reset a screen");
                        }
                    });
                    screen.redraw_all();
                }
            }
            app.state.leases.resume();
            // Screens may have come or gone meanwhile.
            for node in tty.nodes() {
                tty.scan(node, &mut app.state);
            }
            app.state.redraw.all();
        }
    }
}

fn on_device(app: &mut App, event: UdevEvent) {
    let App { state, backend } = app;
    let BackendState::Tty(tty) = backend else {
        return;
    };
    match event {
        UdevEvent::Changed { device_id } => {
            if let Some(node) = tty.node_of(device_id) {
                tty.scan(node, state);
            }
        }
        UdevEvent::Added { device_id, path } => {
            // A GPU comes with its render node too: the card is the one.
            let card = DrmNode::from_dev_id(device_id).is_ok_and(|n| n.ty() == NodeType::Primary);
            if !card || tty.node_of(device_id).is_some() {
                return;
            }
            let Some(handle) = state.loop_handle.clone() else {
                return;
            };
            match tty.add_device(&handle, &path, state) {
                Ok(node) => tty.scan(node, state),
                Err(err) => {
                    tracing::warn!(gpu = %path.display(), "its screens stay dark: {err:#}");
                }
            }
        }
        UdevEvent::Removed { device_id } => {
            if let Some(node) = tty.node_of(device_id) {
                tty.remove_device(node, state);
            }
        }
    }
}

fn on_vblank(
    app: &mut App,
    node: DrmNode,
    crtc: crtc::Handle,
    metadata: &mut Option<DrmEventMetadata>,
) {
    let App { state, backend } = app;
    let BackendState::Tty(tty) = backend else {
        return;
    };
    let Some(screen) = tty
        .devices
        .get_mut(&node)
        .and_then(|d| d.screens.get_mut(&crtc))
    else {
        return;
    };
    if let Err(err) = screen.surface.frame_submitted() {
        tracing::warn!(output = screen.output.name(), ?err, "frame not presented");
    }
    screen.flip.presented();
    screen.presented += 1;
    let now = metadata
        .as_ref()
        .and_then(|m| match m.time {
            DrmEventTime::Monotonic(time) if !time.is_zero() => Some(time),
            _ => None,
        })
        .unwrap_or_else(|| state.clock.now());
    state.presented(&screen.output, now, screen.presented);
    if screen.dirty && tty.active && !screen.dark {
        tty.render(node, crtc, state);
    }
}

/// The mode a connector prefers, by its place in its modes.
fn preferred(info: &connector::Info) -> Option<usize> {
    info.modes()
        .iter()
        .position(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
}

fn core_mode(m: control::Mode) -> hocus_core::outputs::Mode {
    let wl = Mode::from(m);
    hocus_core::outputs::Mode {
        width: wl.size.w,
        height: wl.size.h,
        refresh: wl.refresh,
    }
}

impl TtyBackend {
    /// The GPUs, in the order they were opened.
    fn nodes(&self) -> Vec<DrmNode> {
        let mut devices: Vec<&Device> = self.devices.values().collect();
        devices.sort_by_key(|d| d.order);
        devices.iter().map(|d| d.node).collect()
    }

    fn node_of(&self, device_id: Dev) -> Option<DrmNode> {
        self.devices
            .values()
            .find(|d| d.node.dev_id() == device_id)
            .map(|d| d.node)
    }

    /// Opens another GPU; its screens are lit at its first scan.
    fn add_device(
        &mut self,
        handle: &LoopHandle<'static, App>,
        path: &FsPath,
        state: &mut State,
    ) -> anyhow::Result<DrmNode> {
        let opened = open(&self.session, path)?;
        let (renderer, key) = probe_renderer(&opened.gbm, opened.node);
        let shares = scans_out_shared(&mut self.primary_allocator, &opened.fd, &opened.gbm);
        let how = Probe {
            primary: false,
            renderer,
            scans_out_shared: shares,
        }
        .path()
        .context("it neither draws nor scans out the primary GPU's buffers")?;
        let (allocator, exporter, render_formats, target) = match how {
            Path::Copied => {
                self.gpus
                    .as_mut()
                    .add_node(key, opened.gbm.clone())
                    .context("could not open EGL")?;
                let mut renderer = self
                    .gpus
                    .single_renderer(&key)
                    .map_err(|err| anyhow::anyhow!("{err}"))
                    .context("could not start GLES")?;
                let gles: &mut GlesRenderer = renderer.as_mut();
                let formats = gles.egl_context().dmabuf_render_formats().clone();
                (
                    GbmAllocator::new(
                        opened.gbm.clone(),
                        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
                    ),
                    // Clients' buffers are the primary's: never scanned out here.
                    GbmFramebufferExporter::new(opened.gbm.clone(), None),
                    formats,
                    Some(key),
                )
            }
            Path::Shared | Path::Direct => {
                let mut renderer = self
                    .gpus
                    .single_renderer(&self.primary)
                    .map_err(|err| anyhow::anyhow!("{err}"))?;
                let gles: &mut GlesRenderer = renderer.as_mut();
                // What both read: no tiling only the primary knows.
                let formats: FormatSet = gles
                    .egl_context()
                    .dmabuf_render_formats()
                    .iter()
                    .filter(|f| f.modifier == Modifier::Linear)
                    .copied()
                    .collect();
                (
                    self.primary_allocator.clone(),
                    GbmFramebufferExporter::new(opened.gbm.clone(), None),
                    formats,
                    None,
                )
            }
        };
        let node = opened.node;
        let outputs = DrmOutputManager::new(
            opened.drm,
            allocator,
            exporter,
            Some(opened.gbm),
            COLOR_FORMATS,
            render_formats,
        );
        let notifier = watch(handle, node, opened.notifier)?;
        tracing::info!(gpu = %path.display(), ?how, ?renderer, "another GPU ready");
        self.devices.insert(
            node,
            Device {
                node,
                path: path.to_path_buf(),
                card: card(path),
                order: self.opened,
                connectors: connector_names(outputs.device()),
                how,
                target,
                outputs,
                screens: HashMap::new(),
                off: Vec::new(),
                notifier: Some(notifier),
            },
        );
        self.opened += 1;
        state.leases.add_device(&state.display_handle, node);
        Ok(node)
    }

    /// A GPU went: its screens leave the desktop, its leases end.
    fn remove_device(&mut self, node: DrmNode, state: &mut State) {
        let Some(device) = self.devices.get(&node) else {
            return;
        };
        tracing::info!(gpu = %device.path.display(), "GPU gone");
        if device.how == Path::Direct {
            tracing::warn!("the primary GPU went: nothing draws the screens any more");
        }
        let known = self.known(node, state);
        for change in gpus::scan(&[], &known) {
            self.carry_out(node, change, &[], state);
        }
        state.leases.remove_device(node);
        if let Some(device) = self.devices.remove(&node) {
            if let (Some(token), Some(handle)) = (device.notifier, &state.loop_handle) {
                handle.remove(token);
            }
            if let Some(target) = device.target {
                self.gpus.as_mut().remove_node(&target);
            }
        }
        state.redraw.all();
    }

    /// What the session does with each of a GPU's connectors now.
    fn known(&self, node: DrmNode, state: &State) -> Vec<(connector::Handle, Known)> {
        let Some(device) = self.devices.get(&node) else {
            return Vec::new();
        };
        let lit = device.screens.values().map(|s| (s.connector, Known::Lit));
        let off = device.off.iter().map(|o| (o.connector, Known::Off));
        let offered = state
            .leases
            .offered(node)
            .into_iter()
            .map(|c| (c, Known::Offered));
        lit.chain(off).chain(offered).collect()
    }

    /// Lights the screens newly connected, offers the headsets for lease,
    /// and lets go of what was unplugged.
    fn scan(&mut self, node: DrmNode, state: &mut State) {
        let Some(device) = self.devices.get(&node) else {
            return;
        };
        let drm = device.outputs.device();
        let Ok(resources) = drm.resource_handles() else {
            tracing::warn!(gpu = %device.path.display(), "could not list the GPU's connectors");
            return;
        };
        let connected: Vec<connector::Info> = resources
            .connectors()
            .iter()
            .filter_map(|c| drm.get_connector(*c, true).ok())
            .filter(|c| c.state() == connector::State::Connected && !c.modes().is_empty())
            .collect();
        let found: Vec<Found<connector::Handle>> = connected
            .iter()
            .map(|info| Found {
                connector: info.handle(),
                use_: Use::of(non_desktop(drm, info.handle())),
            })
            .collect();
        let known = self.known(node, state);
        for change in gpus::scan(&found, &known) {
            self.carry_out(node, change, &connected, state);
        }
        self.follow_modes(node, &connected, state);
        state.redraw.all();
    }

    /// The screens, lit or off, whose connector offers other modes now
    /// while it stays connected (#248): their heads say so, and the
    /// desktop settles them after this turn of the loop.
    fn follow_modes(&mut self, node: DrmNode, connected: &[connector::Info], state: &mut State) {
        let Some(device) = self.devices.get_mut(&node) else {
            return;
        };
        for info in connected {
            let modes: Vec<hocus_core::outputs::Mode> =
                info.modes().iter().map(|m| core_mode(*m)).collect();
            let preferred = preferred(info);
            let head = if let Some(screen) = device
                .screens
                .values_mut()
                .find(|s| s.connector == info.handle())
            {
                // The same sizes may come with other timings.
                screen.modes = info.modes().to_vec();
                if let Some(i) = preferred
                    && screen.head.preferred != preferred
                {
                    screen.output.set_preferred(Mode::from(info.modes()[i]));
                }
                &mut screen.head
            } else if let Some(off) = device.off.iter_mut().find(|o| o.connector == info.handle()) {
                &mut off.head
            } else {
                continue;
            };
            if head.modes == modes && head.preferred == preferred {
                continue;
            }
            let before = std::mem::replace(&mut head.modes, modes);
            let before_preferred = std::mem::replace(&mut head.preferred, preferred);
            state.modes_changed(
                &head.name,
                hocus_core::outputs::Offer {
                    modes: &before,
                    preferred: before_preferred,
                },
                hocus_core::outputs::Offer {
                    modes: &head.modes,
                    preferred: head.preferred,
                },
            );
        }
    }

    fn carry_out(
        &mut self,
        node: DrmNode,
        change: Change<connector::Handle>,
        connected: &[connector::Info],
        state: &mut State,
    ) {
        let info = |c: connector::Handle| connected.iter().find(|i| i.handle() == c);
        match change {
            Change::Unplug(connector) => self.unplug(node, connector, state),
            Change::Forget(connector) => {
                if let Some(device) = self.devices.get_mut(&node) {
                    device.off.retain(|o| {
                        let gone = o.connector == connector;
                        if gone {
                            state.output_unplugged(&o.head.name);
                        }
                        !gone
                    });
                }
            }
            Change::Withdraw(connector) => state.leases.withdraw(node, connector),
            Change::Light(connector) => {
                let Some(info) = info(connector) else {
                    return;
                };
                let Some(crtc) = self.free_crtc(node, info, state) else {
                    tracing::warn!(connector = ?info.handle(), "no CRTC left for a screen");
                    return;
                };
                match self.light(node, crtc, info, None, state) {
                    Ok(screen) => {
                        if let Some(device) = self.devices.get_mut(&node) {
                            device.screens.insert(crtc, screen);
                        }
                    }
                    Err(err) => tracing::warn!("could not light a screen: {err:#}"),
                }
            }
            Change::Offer(connector) => {
                if let Some(info) = info(connector) {
                    self.offer(node, info, state);
                }
            }
        }
    }

    /// A screen leaves the desktop: unplugged, or a headset after all.
    fn unplug(&mut self, node: DrmNode, connector: connector::Handle, state: &mut State) {
        let Some(device) = self.devices.get_mut(&node) else {
            return;
        };
        let Some(crtc) = device
            .screens
            .iter()
            .find(|(_, s)| s.connector == connector)
            .map(|(c, _)| *c)
        else {
            return;
        };
        let Some(screen) = device.screens.remove(&crtc) else {
            return;
        };
        tracing::info!(output = screen.output.name(), "screen unplugged");
        // A headset now: the CRTC is let go for its lease.
        screen.surface.with_compositor(|c| {
            if let Err(err) = c.clear() {
                tracing::debug!(%err, "could not clear the screen");
            }
        });
        state.unplug_output(&screen.output);
        state.output_unplugged(&screen.output.name());
        state.retire_global(screen.global);
        for token in [screen.health, screen.pace].into_iter().flatten() {
            if let Some(handle) = &state.loop_handle {
                handle.remove(token);
            }
        }
    }

    /// Offers a headset's connector for lease, with a CRTC no screen uses
    /// and its planes.
    fn offer(&mut self, node: DrmNode, info: &connector::Info, state: &mut State) {
        let Some(crtc) = self.free_crtc(node, info, state) else {
            tracing::warn!(connector = ?info.handle(), "no CRTC left to lease a headset");
            return;
        };
        let name = self.screen_name(node, info);
        let Some(device) = self.devices.get(&node) else {
            return;
        };
        let drm = device.outputs.device();
        let planes = match drm.planes(&crtc) {
            Ok(planes) => planes,
            Err(err) => {
                tracing::warn!(%err, "no planes to lease a headset");
                return;
            }
        };
        let claim = |list: &[smithay::backend::drm::PlaneInfo]| {
            list.iter()
                .find_map(|p| drm.claim_plane(p.handle, crtc).map(|c| (p.handle, c)))
        };
        let Some(primary) = claim(&planes.primary) else {
            tracing::warn!("no primary plane to lease a headset");
            return;
        };
        let mut claimed = vec![primary];
        claimed.extend(claim(&planes.cursor));
        let (make, model) = edid(drm, info.handle())
            .map(|e| identity(&e))
            .unwrap_or_default();
        let description = format!(
            "{} {}",
            make.unwrap_or_else(|| "Unknown".into()),
            model.unwrap_or_else(|| "Unknown".into())
        );
        state.leases.offer(
            node,
            Offer {
                connector: info.handle(),
                crtc,
                planes: claimed,
            },
            name,
            description,
        );
        state.leases.refill(node, drm);
    }

    /// A screen's name: its connector's, told apart from a GPU's before.
    fn screen_name(&self, node: DrmNode, info: &connector::Info) -> String {
        let name = connector_name(info);
        let Some(device) = self.devices.get(&node) else {
            return name;
        };
        let taken = self
            .devices
            .values()
            .any(|d| d.order < device.order && d.connectors.contains(&name));
        gpus::screen_name(&name, &device.card, taken)
    }

    /// A CRTC this connector's encoders can drive that no screen and no
    /// headset offered for lease uses.
    fn free_crtc(
        &self,
        node: DrmNode,
        info: &connector::Info,
        state: &State,
    ) -> Option<crtc::Handle> {
        let device = self.devices.get(&node)?;
        let drm = device.outputs.device();
        let resources = drm.resource_handles().ok()?;
        let possible: Vec<crtc::Handle> = info
            .encoders()
            .iter()
            .filter_map(|e| drm.get_encoder(*e).ok())
            .flat_map(|e| resources.filter_crtcs(e.possible_crtcs()))
            .collect();
        gpus::pick_crtc(possible, |crtc| {
            device.screens.contains_key(&crtc) || state.leases.holds_crtc(node, crtc)
        })
    }

    fn light(
        &mut self,
        node: DrmNode,
        crtc: crtc::Handle,
        info: &connector::Info,
        mode: Option<control::Mode>,
        state: &mut State,
    ) -> anyhow::Result<Screen> {
        let name = self.screen_name(node, info);
        let primary = self.primary;
        let Self { gpus, devices, .. } = self;
        let device = devices.get_mut(&node).context("the GPU is gone")?;
        let drm = device.outputs.device();
        let (make, model) = edid(drm, info.handle())
            .map(|e| identity(&e))
            .unwrap_or_default();
        let preferred = preferred(info);
        let drm_mode = mode
            .or_else(|| preferred.map(|i| info.modes()[i]))
            .or_else(|| info.modes().first().copied())
            .context("no mode")?;
        let mode = Mode::from(drm_mode);
        let (width_mm, height_mm) = info.size().unwrap_or((0, 0));
        let make = make.unwrap_or_else(|| "Unknown".into());
        let model = model.unwrap_or_else(|| "Unknown".into());
        let output = Output::new(
            name.clone(),
            PhysicalProperties {
                size: (width_mm as i32, height_mm as i32).into(),
                subpixel: info.subpixel().into(),
                make: make.clone(),
                model: model.clone(),
            },
        );
        let global = output.create_global::<State>(&state.display_handle);
        // To the right of the screens already lit, until the settings
        // arrange them (#111).
        let x = state
            .space
            .outputs()
            .filter_map(|o| state.space.output_geometry(o))
            .map(|g| g.loc.x + g.size.w)
            .max()
            .unwrap_or(0);
        // At the scale of its density, until the configuration sets one
        // (#250): lit at 1 first, its clients would draw twice.
        let scale = hocus_core::outputs::default_scale(
            (mode.size.w, mode.size.h),
            (width_mm as i32, height_mm as i32),
        );
        output.change_current_state(
            Some(mode),
            Some(Transform::Normal),
            Some(if scale.fract() == 0.0 {
                Scale::Integer(scale as i32)
            } else {
                Scale::Fractional(scale)
            }),
            Some((x, 0).into()),
        );
        output.set_preferred(mode);

        let initialized = match (device.how, device.target) {
            (Path::Copied, Some(target)) => {
                // Its first frame is cleared on its own GPU.
                let mut renderer = gpus
                    .single_renderer(&target)
                    .map_err(|err| anyhow::anyhow!("{err}"))?;
                device
                    .outputs
                    .initialize_output::<_, CopiedFrame<MultiTexture>>(
                        crtc,
                        drm_mode,
                        &[info.handle()],
                        &output,
                        None,
                        &mut renderer,
                        &DrmOutputRenderElements::default(),
                    )
                    .map_err(|err| anyhow::anyhow!("{err}"))
            }
            _ => {
                let mut renderer = gpus
                    .single_renderer(&primary)
                    .map_err(|err| anyhow::anyhow!("{err}"))?;
                let gles: &mut GlesRenderer = renderer.as_mut();
                device
                    .outputs
                    .initialize_output::<_, OutputElement<GlesRenderer>>(
                        crtc,
                        drm_mode,
                        &[info.handle()],
                        &output,
                        None,
                        gles,
                        &DrmOutputRenderElements::default(),
                    )
                    .map_err(|err| anyhow::anyhow!("{err}"))
            }
        };
        let surface = initialized.with_context(|| format!("could not drive {name}"))?;

        state.remember_places();
        state.space.map_output(&output, (x, 0));
        state.plug_output(&output, DEFAULT_DESKTOPS);
        state.emit(|s| Event::OutputAdded {
            output: s.output_info(&output),
        });
        tracing::info!(
            output = name,
            gpu = %device.path.display(),
            mode = %format!("{}x{}@{:.2}", mode.size.w, mode.size.h, f64::from(mode.refresh) / 1000.0),
            size_mm = %format!("{width_mm}x{height_mm}"),
            scale,
            "screen lit"
        );
        let vrr_capable = surface.with_compositor(|c| {
            c.vrr_supported(info.handle())
                .is_ok_and(|v| !matches!(v, smithay::backend::drm::VrrSupport::NotSupported))
        });
        let head = Head {
            name,
            make,
            model,
            physical_size: (width_mm as i32, height_mm as i32),
            modes: info.modes().iter().map(|m| core_mode(*m)).collect(),
            preferred,
            vrr_capable,
            now: Setting::of(&output, (x, 0), true, false),
        };
        Ok(Screen {
            connector: info.handle(),
            modes: info.modes().to_vec(),
            head,
            vrr: false,
            dark: false,
            output,
            global,
            surface,
            flip: Flip::Idle,
            dirty: true,
            presented: 0,
            health: None,
            pace: None,
            copy: None,
        })
    }

    /// Draws the screens that changed and are free to take a frame.
    pub fn after_dispatch(&mut self, state: &mut State) {
        self.pointing.settle(&state.config);
        // Lease builders for the next requests.
        for (node, device) in &self.devices {
            state.leases.refill(*node, device.outputs.device());
        }
        if !state.redraw.is_pending() {
            return;
        }
        // Only the screens asked for (#244); each stays dirty until it
        // draws, after a frame still out.
        for screen in self
            .devices
            .values_mut()
            .flat_map(|d| d.screens.values_mut())
        {
            screen.dirty |= state.redraws(&screen.output);
        }
        state.redraw.done();
        if !self.active {
            return;
        }
        let ready: Vec<(DrmNode, crtc::Handle)> = self
            .devices
            .iter()
            .flat_map(|(node, d)| {
                d.screens
                    .iter()
                    .filter(|(_, s)| s.dirty && !s.dark && !s.flip.is_queued() && s.pace.is_none())
                    .map(|(crtc, _)| (*node, *crtc))
            })
            .collect();
        for (node, crtc) in ready {
            self.render(node, crtc, state);
        }
        state.space.refresh();
        state.popups.cleanup();
    }

    /// The screen showing `output`.
    fn screen_of(&self, name: &str) -> Option<(DrmNode, crtc::Handle)> {
        self.devices.iter().find_map(|(node, d)| {
            d.screens
                .iter()
                .find(|(_, s)| s.output.name() == name)
                .map(|(crtc, _)| (*node, *crtc))
        })
    }

    /// Night light: `rgb` scales each channel of `output`'s gamma ramp.
    pub fn set_gamma(&mut self, output: &Output, rgb: [f32; 3]) {
        let Some((node, crtc)) = self.screen_of(&output.name()) else {
            return;
        };
        let Some(device) = self.devices.get(&node) else {
            return;
        };
        let drm = device.outputs.device();
        let length = match drm.get_crtc(crtc) {
            Ok(info) => info.gamma_length() as usize,
            Err(err) => {
                tracing::warn!(%err, "no gamma for night light");
                return;
            }
        };
        if length < 2 {
            return;
        }
        let ramp = |scale: f32| -> Vec<u16> {
            (0..length)
                .map(|i| (i as f32 / (length - 1) as f32 * 65535.0 * scale).round() as u16)
                .collect()
        };
        let (r, g, b) = (ramp(rgb[0]), ramp(rgb[1]), ramp(rgb[2]));
        if let Err(err) = drm.set_gamma(crtc, &r, &g, &b) {
            tracing::warn!(%err, output = output.name(), "night light: the gamma was refused");
        }
    }

    pub fn capture_output(
        &mut self,
        output: &Output,
        state: &State,
        cursor: bool,
    ) -> anyhow::Result<Pixels> {
        let mut renderer = self
            .gpus
            .single_renderer(&self.primary)
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let gles: &mut GlesRenderer = renderer.as_mut();
        capture::output::<_, GlesTexture>(gles, output, state, cursor)
    }

    /// Every connected screen, lit or off.
    pub fn heads(&self, state: &State) -> Vec<Head> {
        let mut heads: Vec<Head> = Vec::new();
        for device in self.devices.values() {
            heads.extend(device.screens.values().map(|s| {
                let position = state
                    .space
                    .output_geometry(&s.output)
                    .map_or((0, 0), |g| (g.loc.x, g.loc.y));
                Head {
                    now: Setting::of(&s.output, position, true, s.vrr),
                    ..s.head.clone()
                }
            }));
            heads.extend(device.off.iter().map(|o| o.head.clone()));
        }
        heads.sort_by(|a, b| a.name.cmp(&b.name));
        heads
    }

    /// Sets a screen as asked: its mode on the CRTC, adaptive sync, on or
    /// off; then the desktop follows.
    pub fn apply(
        &mut self,
        name: &str,
        setting: &Setting,
        state: &mut State,
    ) -> anyhow::Result<()> {
        let off = self.devices.iter().find_map(|(node, d)| {
            d.off
                .iter()
                .position(|o| o.head.name == name)
                .map(|i| (*node, i))
        });
        if let Some((node, i)) = off {
            if !setting.enabled {
                return Ok(());
            }
            let device = self.devices.get_mut(&node).context("the GPU is gone")?;
            let off = device.off.remove(i);
            let info = device
                .outputs
                .device()
                .get_connector(off.connector, false)
                .context("the screen is gone")?;
            let mode = info
                .modes()
                .iter()
                .find(|m| core_mode(**m) == setting.mode)
                .copied();
            let crtc = self.free_crtc(node, &info, state).context("no CRTC left")?;
            let screen = self.light(node, crtc, &info, mode, state)?;
            let output = screen.output.clone();
            if let Some(device) = self.devices.get_mut(&node) {
                device.screens.insert(crtc, screen);
            }
            state.set_output(&output, setting);
            return Ok(());
        }
        let (node, crtc) = self
            .screen_of(name)
            .with_context(|| format!("no screen named {name}"))?;
        let primary = self.primary;
        let Self { gpus, devices, .. } = self;
        let device = devices.get_mut(&node).context("the GPU is gone")?;
        let (how, target) = (device.how, device.target);
        let screen = device
            .screens
            .get_mut(&crtc)
            .with_context(|| format!("no screen named {name}"))?;
        if !setting.enabled {
            screen.surface.with_compositor(|c| {
                if let Err(err) = c.clear() {
                    tracing::warn!(%err, "could not turn the screen off");
                }
            });
            let Some(screen) = device.screens.remove(&crtc) else {
                return Ok(());
            };
            let position = state
                .space
                .output_geometry(&screen.output)
                .map_or((0, 0), |g| (g.loc.x, g.loc.y));
            state.disable_output(&screen.output);
            state.retire_global(screen.global);
            if let Some(handle) = &state.loop_handle {
                for token in [screen.health, screen.pace].into_iter().flatten() {
                    handle.remove(token);
                }
            }
            let mut head = screen.head;
            head.now = Setting {
                enabled: false,
                ..Setting::of(&screen.output, position, false, screen.vrr)
            };
            device.off.push(Off {
                connector: screen.connector,
                head,
            });
            return Ok(());
        }
        let current = core_mode(screen.surface.with_compositor(|c| c.pending_mode()));
        if setting.mode != current {
            let mode = screen
                .modes
                .iter()
                .find(|m| core_mode(**m) == setting.mode)
                .copied()
                .context("the screen has no such mode")?;
            match (how, target) {
                (Path::Copied, Some(target)) => {
                    let mut renderer = gpus
                        .single_renderer(&target)
                        .map_err(|err| anyhow::anyhow!("{err}"))?;
                    screen
                        .surface
                        .use_mode::<_, CopiedFrame<MultiTexture>>(
                            mode,
                            &mut renderer,
                            &DrmOutputRenderElements::default(),
                        )
                        .map_err(|err| anyhow::anyhow!("{err}"))?;
                }
                _ => {
                    let mut renderer = gpus
                        .single_renderer(&primary)
                        .map_err(|err| anyhow::anyhow!("{err}"))?;
                    let gles: &mut GlesRenderer = renderer.as_mut();
                    screen
                        .surface
                        .use_mode::<_, OutputElement<GlesRenderer>>(
                            mode,
                            gles,
                            &DrmOutputRenderElements::default(),
                        )
                        .map_err(|err| anyhow::anyhow!("{err}"))?;
                }
            }
        }
        if setting.vrr != screen.vrr {
            screen
                .surface
                .with_compositor(|c| c.use_vrr(setting.vrr))
                .map_err(|err| anyhow::anyhow!("{err}"))?;
            screen.vrr = setting.vrr;
        }
        screen.dirty = true;
        let output = screen.output.clone();
        state.set_output(&output, setting);
        Ok(())
    }

    /// Turns a screen's light off or on (output-power): off, the CRTC is
    /// cleared; on, it is committed afresh with a full frame.
    pub fn power(&mut self, name: &str, on: bool, state: &mut State) -> anyhow::Result<()> {
        let (node, crtc) = self
            .screen_of(name)
            .with_context(|| format!("no screen named {name}"))?;
        let screen = self
            .devices
            .get_mut(&node)
            .and_then(|d| d.screens.get_mut(&crtc))
            .with_context(|| format!("no screen named {name}"))?;
        if on == !screen.dark {
            return Ok(());
        }
        screen.dark = !on;
        screen.surface.with_compositor(|c| {
            let result = if on { c.reset_state() } else { c.clear() };
            if let Err(err) = result {
                tracing::warn!(%err, on, "could not change the screen's power");
            }
        });
        if on {
            screen.flip = Flip::Idle;
            screen.redraw_all();
            screen.dirty = true;
            if self.active {
                self.render(node, crtc, state);
            }
        }
        Ok(())
    }

    pub fn capture_window(&mut self, window: &Window, scale: f64) -> anyhow::Result<Pixels> {
        let mut renderer = self
            .gpus
            .single_renderer(&self.primary)
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let gles: &mut GlesRenderer = renderer.as_mut();
        capture::window::<_, GlesTexture>(gles, window, scale)
    }

    /// Draws one screen and queues the frame for its next vblank.
    fn render(&mut self, node: DrmNode, crtc: crtc::Handle, state: &mut State) {
        let primary = self.primary;
        let Self {
            gpus,
            devices,
            primary_allocator,
            ..
        } = self;
        let Some(device) = devices.get_mut(&node) else {
            return;
        };
        let (how, target) = (device.how, device.target);
        let Some(screen) = device.screens.get_mut(&crtc) else {
            return;
        };
        let start = Instant::now();
        state.measures.render(&screen.output.name());
        screen.dirty = false;
        let drawn = match (how, target) {
            (Path::Copied, Some(target)) => {
                draw_copied(gpus, primary, primary_allocator, target, screen, state)
            }
            _ => draw_direct(gpus, primary, screen, state),
        };
        let now = state.clock.now();
        let drawn = match drawn {
            Ok(drawn) => drawn,
            Err(err) => {
                tracing::warn!(output = screen.output.name(), "rendering failed: {err:#}");
                return;
            }
        };
        if !drawn.empty {
            match screen.surface.queue_frame(()) {
                Ok(()) => {
                    state.measures.frame(&screen.output.name(), start.elapsed());
                    screen.flip.queued(now);
                    watch_flip(screen, node, crtc, state);
                }
                Err(err) => {
                    tracing::warn!(
                        output = screen.output.name(),
                        ?err,
                        "could not queue the frame"
                    );
                }
            }
        }
        state.send_frames(&screen.output, now, Some(&drawn.states));
        if drawn.empty {
            pace(screen, node, crtc, state);
        }
    }
}

/// Draws a screen the primary GPU shows itself, or one that scans out its
/// buffers.
fn draw_direct(
    gpus: &mut Gpus,
    primary: DrmNode,
    screen: &mut Screen,
    state: &mut State,
) -> anyhow::Result<Drawn> {
    let mut renderer = gpus
        .single_renderer(&primary)
        .map_err(|err| anyhow::anyhow!("{err}"))?;
    let gles: &mut GlesRenderer = renderer.as_mut();
    // A wallpaper that moves, the blur behind panels and the Matrix
    // theme's GPU effects, before the scene shows them.
    state.paint_backdrop(gles, &screen.output);
    state.paint_glass(gles, &screen.output);
    state.paint_crt(gles, &screen.output);
    let elements = state.scene_elements(gles, &screen.output, true);
    let elements = clipped(&elements);
    let result = screen
        .surface
        .render_frame(gles, &elements, BACKGROUND, FrameFlags::DEFAULT)
        .map_err(|err| anyhow::anyhow!("{err:?}"))?;
    Ok(Drawn {
        empty: result.is_empty,
        states: result.states,
    })
}

/// Draws a screen of a GPU the frames are copied to: the primary GPU draws
/// the scene into a buffer of its own, then smithay's multi-GPU renderer
/// copies that buffer into the screen's GPU.
fn draw_copied(
    gpus: &mut Gpus,
    primary: DrmNode,
    allocator: &mut Allocator,
    target: DrmNode,
    screen: &mut Screen,
    state: &mut State,
) -> anyhow::Result<Drawn> {
    let Screen {
        output,
        surface,
        copy,
        ..
    } = screen;
    let size = output
        .current_mode()
        .context("the screen has no mode")?
        .size;
    let states;
    {
        let mut renderer = gpus
            .single_renderer(&primary)
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let gles: &mut GlesRenderer = renderer.as_mut();
        if copy.as_ref().is_none_or(|c| c.size != size) {
            *copy = Some(CopyBuffer::new(gles, allocator, primary, output, size)?);
        }
        let Some(copy) = copy.as_mut() else {
            return Err(anyhow::anyhow!("no buffer to draw into"));
        };
        state.paint_backdrop(gles, output);
        state.paint_glass(gles, output);
        state.paint_crt(gles, output);
        let elements = state.scene_elements(gles, output, true);
        let mut framebuffer = gles
            .bind(&mut copy.buffer)
            .context("could not draw into the copy buffer")?;
        let drawn = copy
            .tracker
            .render_output(gles, &mut framebuffer, copy.age, &elements, BACKGROUND)
            .map_err(|err| anyhow::anyhow!("{err:?}"))?;
        let damaged = drawn.damage.is_some_and(|d| !d.is_empty()) || copy.age == 0;
        states = drawn.states;
        copy.age = 1;
        if !damaged {
            return Ok(Drawn {
                empty: true,
                states,
            });
        }
        copy.commit.increment();
    }
    let Some(copy) = copy.as_mut() else {
        return Err(anyhow::anyhow!("no buffer to draw into"));
    };
    match copy_across(
        gpus,
        primary,
        target,
        surface,
        copy,
        output.current_transform(),
    ) {
        Ok(empty) => Ok(Drawn { empty, states }),
        Err(err) => {
            // The frame never got across: the next one is drawn whole.
            copy.age = 0;
            Err(err)
        }
    }
}

/// Copies the frame the primary drew into `copy` to the screen's GPU;
/// whether nothing went out.
fn copy_across(
    gpus: &mut Gpus,
    primary: DrmNode,
    target: DrmNode,
    surface: &mut Surface,
    copy: &mut CopyBuffer,
    transform: Transform,
) -> anyhow::Result<bool> {
    let mut renderer: Multi<'_> = gpus
        .renderer(&primary, &target, surface.format())
        .map_err(|err| anyhow::anyhow!("{err}"))?;
    let texture = match &copy.texture {
        Some(texture) => texture.clone(),
        None => {
            let texture = renderer
                .import_dmabuf(&copy.buffer, None)
                .map_err(|err| anyhow::anyhow!("{err}"))?;
            copy.texture = Some(texture.clone());
            texture
        }
    };
    let elements = [CopiedFrame::new(
        copy.id.clone(),
        copy.commit,
        texture,
        transform,
    )];
    let elements = clipped(&elements);
    let result = surface
        .render_frame(&mut renderer, &elements, BACKGROUND, FrameFlags::DEFAULT)
        .map_err(|err| anyhow::anyhow!("{err:?}"))?;
    Ok(result.is_empty)
}

impl CopyBuffer {
    fn new(
        gles: &GlesRenderer,
        allocator: &mut Allocator,
        primary: DrmNode,
        output: &Output,
        size: Size<i32, Physical>,
    ) -> anyhow::Result<Self> {
        let modifiers: Vec<Modifier> = gles
            .egl_context()
            .dmabuf_render_formats()
            .iter()
            .filter(|f| f.code == COPY_FORMAT)
            .map(|f| f.modifier)
            .collect();
        let buffer = allocator
            .create_buffer(size.w as u32, size.h as u32, COPY_FORMAT, &modifiers)
            .context("could not allocate the copy buffer")?
            .export()
            .context("could not share the copy buffer")?;
        buffer.set_node(primary);
        Ok(Self {
            size,
            buffer,
            tracker: OutputDamageTracker::from_output(output),
            age: 0,
            texture: None,
            id: Id::new(),
            commit: CommitCounter::default(),
        })
    }
}

/// The time between two refreshes of a screen.
fn refresh(output: &Output) -> std::time::Duration {
    let mhz = output.current_mode().map_or(60_000, |m| m.refresh.max(1));
    std::time::Duration::from_micros(1_000_000_000 / mhz as u64)
}

/// Waits a refresh before the screen may draw again after an empty frame.
fn pace(screen: &mut Screen, node: DrmNode, crtc: crtc::Handle, state: &State) {
    let Some(handle) = &state.loop_handle else {
        return;
    };
    if screen.pace.is_some() {
        return;
    }
    let timer = Timer::from_duration(refresh(&screen.output));
    match handle.insert_source(timer, move |_, _, app| {
        app.state.measures.ran("pace");
        let App { state, backend } = app;
        if let BackendState::Tty(tty) = backend {
            let active = tty.active;
            if let Some(screen) = tty
                .devices
                .get_mut(&node)
                .and_then(|d| d.screens.get_mut(&crtc))
            {
                screen.pace = None;
                if screen.dirty && active && !screen.dark && !screen.flip.is_queued() {
                    tty.render(node, crtc, state);
                }
            }
        }
        TimeoutAction::Drop
    }) {
        Ok(token) => screen.pace = Some(token),
        Err(err) => tracing::warn!(%err.error, "could not pace a screen"),
    }
}

/// Looks at the frame out once a second until it is presented, and
/// recovers the screen when it never is.
fn watch_flip(screen: &mut Screen, node: DrmNode, crtc: crtc::Handle, state: &State) {
    let Some(handle) = &state.loop_handle else {
        return;
    };
    if screen.health.is_some() {
        return;
    }
    let timer = Timer::from_duration(flip::CHECK);
    match handle.insert_source(timer, move |_, _, app| {
        app.state.measures.ran("flip check");
        let App { state, backend } = app;
        let BackendState::Tty(tty) = backend else {
            return TimeoutAction::Drop;
        };
        let active = tty.active;
        let Some(screen) = tty
            .devices
            .get_mut(&node)
            .and_then(|d| d.screens.get_mut(&crtc))
        else {
            return TimeoutAction::Drop;
        };
        // Another terminal is shown: nothing is presented, nothing is late.
        let health = if active {
            screen.flip.check(state.clock.now())
        } else {
            Health::Waiting { missed: 0 }
        };
        match health {
            Health::Waiting { .. } => TimeoutAction::ToDuration(flip::CHECK),
            Health::Idle => {
                screen.health = None;
                TimeoutAction::Drop
            }
            Health::Recover { missed } => {
                screen.health = None;
                // The lost flip counts as done; the CRTC is committed
                // afresh with the next frame.
                if let Err(err) = screen.surface.frame_submitted() {
                    tracing::debug!(?err, "no frame to drop");
                }
                screen.surface.with_compositor(|c| {
                    if let Err(err) = c.reset_state() {
                        tracing::warn!(%err, "could not reset the screen");
                    }
                });
                screen.redraw_all();
                screen.dirty = true;
                let output = screen.output.clone();
                state.output_recovered(&output, missed);
                tty.render(node, crtc, state);
                TimeoutAction::Drop
            }
        }
    }) {
        Ok(token) => screen.health = Some(token),
        Err(err) => tracing::warn!(%err.error, "could not watch a screen"),
    }
}

/// A connector's property by its name: its type and raw value.
fn connector_property(
    device: &DrmDevice,
    connector: connector::Handle,
    name: &str,
) -> Option<(property::ValueType, u64)> {
    let properties = device.get_properties(connector).ok()?;
    let (handles, values) = properties.as_props_and_values();
    handles.iter().zip(values).find_map(|(handle, value)| {
        let info = device.get_property(*handle).ok()?;
        (info.name().to_str() == Ok(name)).then(|| (info.value_type(), *value))
    })
}

/// A VR headset says it is no desktop.
fn non_desktop(device: &DrmDevice, connector: connector::Handle) -> bool {
    connector_property(device, connector, "non-desktop")
        .and_then(|(kind, value)| kind.convert_value(value).as_boolean())
        .unwrap_or(false)
}

/// The EDID a screen sent, from its connector's property.
fn edid(device: &DrmDevice, connector: connector::Handle) -> Option<Vec<u8>> {
    let (kind, value) = connector_property(device, connector, "EDID")?;
    let property::Value::Blob(blob) = kind.convert_value(value) else {
        return None;
    };
    device.get_property_blob(blob).ok()
}

/// The maker's three-letter code and the screen's name, from its EDID.
pub fn identity(edid: &[u8]) -> (Option<String>, Option<String>) {
    const HEADER: [u8; 8] = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00];
    if edid.len() < 128 || edid[..8] != HEADER {
        return (None, None);
    }
    // Three letters, five bits each, 1 for A.
    let code = u16::from_be_bytes([edid[8], edid[9]]);
    let letter = |shift: u16| char::from(b'A' - 1 + ((code >> shift) & 0x1f) as u8);
    let make = [letter(10), letter(5), letter(0)]
        .iter()
        .all(char::is_ascii_uppercase)
        .then(|| [letter(10), letter(5), letter(0)].iter().collect());
    // Four 18-byte descriptors; 0xfc holds the name, ended by a newline.
    let model = (0..4).find_map(|i| {
        let d = &edid[54 + i * 18..72 + i * 18];
        (d[..3] == [0, 0, 0] && d[3] == 0xfc).then(|| {
            String::from_utf8_lossy(&d[5..])
                .split('\n')
                .next()
                .unwrap_or_default()
                .trim()
                .to_string()
        })
    });
    (make, model.filter(|m| !m.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_screen_says_who_made_it_and_its_name() {
        let mut edid = vec![0u8; 128];
        edid[..8].copy_from_slice(&[0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00]);
        // "DEL": D=4, E=5, L=12.
        let code: u16 = (4 << 10) | (5 << 5) | 12;
        edid[8..10].copy_from_slice(&code.to_be_bytes());
        // The second descriptor names it.
        let d = 54 + 18;
        edid[d..d + 5].copy_from_slice(&[0, 0, 0, 0xfc, 0]);
        edid[d + 5..d + 18].copy_from_slice(b"U2723QE\n     ");
        assert_eq!(
            identity(&edid),
            (Some("DEL".to_string()), Some("U2723QE".to_string()))
        );
        assert_eq!(identity(&edid[..64]), (None, None));
    }

    #[test]
    fn a_gpu_is_named_by_its_card() {
        assert_eq!(card(FsPath::new("/dev/dri/card1")), "card1");
    }
}
