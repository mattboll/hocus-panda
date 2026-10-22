//! The session itself, on a virtual terminal: the seat comes from libseat
//! (logind or seatd), the screens are driven through DRM/KMS on the primary
//! GPU and drawn with GLES, input comes from libinput. Switching to another
//! terminal pauses all of it; coming back draws everything again.

use std::{collections::HashMap, path::PathBuf, time::Instant};

use anyhow::Context;
use smithay::{
    backend::{
        allocator::{
            Fourcc,
            format::FormatSet,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        },
        drm::{
            DrmDevice, DrmDeviceFd, DrmEvent, DrmEventMetadata, DrmEventTime, DrmNode, NodeType,
            compositor::FrameFlags,
            exporter::gbm::GbmFramebufferExporter,
            output::{DrmOutput, DrmOutputManager, DrmOutputRenderElements},
        },
        egl::{EGLContext, EGLDevice, EGLDisplay},
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            ImportDma, ImportMemWl,
            gles::{GlesRenderer, GlesTexture},
        },
        session::{Event as SessionEvent, Session, libseat::LibSeatSession},
        udev::{UdevBackend, UdevEvent, all_gpus, primary_gpu},
    },
    desktop::Window,
    output::{Mode, Output, PhysicalProperties},
    reexports::{
        calloop::{
            LoopHandle, RegistrationToken,
            timer::{TimeoutAction, Timer},
        },
        drm::control::{self, Device as ControlDevice, ModeTypeFlags, connector, crtc, property},
        input::Libinput,
        rustix::fs::OFlags,
        wayland_server::backend::GlobalId,
    },
    utils::{DeviceFd, Transform},
    wayland::dmabuf::{DmabufFeedbackBuilder, DmabufGlobal},
};

use hocus_api::Event;

use crate::{
    App, BACKGROUND, BackendState, State,
    capture::{self, Pixels},
    flip::{self, Flip, Health},
    output_spec::DEFAULT_DESKTOPS,
    outputs::{Head, Setting},
};

type Allocator = GbmAllocator<DrmDeviceFd>;
type Exporter = GbmFramebufferExporter<DrmDeviceFd>;
type Outputs = DrmOutputManager<Allocator, Exporter, (), DrmDeviceFd>;
type Surface = DrmOutput<Allocator, Exporter, (), DrmDeviceFd>;

/// Scan-out formats, tried in order: 8 bits per channel, which every
/// display engine takes.
const COLOR_FORMATS: [Fourcc; 2] = [Fourcc::Argb8888, Fourcc::Abgr8888];

pub struct TtyBackend {
    session: LibSeatSession,
    libinput: Libinput,
    gpu: Gpu,
    /// The session is in the foreground: its terminal is the one shown.
    active: bool,
}

impl std::fmt::Debug for TtyBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TtyBackend")
            .field("seat", &self.session.seat())
            .field("gpu", &self.gpu.path)
            .field("active", &self.active)
            .field("screens", &self.gpu.screens.len())
            .finish()
    }
}

/// The primary GPU: its display engine, its renderer, its screens.
struct Gpu {
    node: DrmNode,
    path: PathBuf,
    renderer: GlesRenderer,
    outputs: Outputs,
    screens: HashMap<crtc::Handle, Screen>,
    /// Connected screens turned off: what is needed to light them again.
    off: Vec<Off>,
    /// For clients that draw on the GPU: the formats it can read.
    _dmabuf: DmabufGlobal,
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
}

/// Takes the seat, opens the primary GPU, lights the connected screens and
/// starts reading input.
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
    let gpu = open_gpu(handle, &session, &path, state)?;

    let mut libinput =
        Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(session.clone().into());
    libinput
        .udev_assign_seat(&seat)
        .map_err(|()| anyhow::anyhow!("libinput could not take seat {seat}"))?;
    handle
        .insert_source(
            LibinputInputBackend::new(libinput.clone()),
            |event, _, app| {
                app.timed("input", |app| app.state.process_input_event(event));
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

    // Screens plugged and unplugged.
    let udev = UdevBackend::new(&seat).context("could not watch devices")?;
    handle
        .insert_source(udev, |event, (), app| {
            app.timed("devices", |app| on_device(app, event));
        })
        .map_err(|err| err.error)
        .context("could not watch devices")?;

    let mut backend = TtyBackend {
        session,
        libinput,
        gpu,
        active: true,
    };
    backend.scan(state);
    anyhow::ensure!(!backend.gpu.screens.is_empty(), "no screen is connected");
    state.vt_switch = Some(backend.session.clone());
    Ok(backend)
}

fn open_gpu(
    handle: &LoopHandle<'static, App>,
    session: &LibSeatSession,
    path: &std::path::Path,
    state: &mut State,
) -> anyhow::Result<Gpu> {
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
    let gbm = GbmDevice::new(fd).context("could not create a GBM device")?;

    // SAFETY: nothing else in the process creates EGL displays for this
    // device, so smithay's tracking of them is accurate.
    #[allow(unsafe_code)]
    let display = unsafe { EGLDisplay::new(gbm.clone()) }.context("could not open EGL")?;
    if EGLDevice::device_for_display(&display).is_ok_and(|d| d.is_software()) {
        // A VM without 3D: llvmpipe draws, slowly but correctly.
        tracing::warn!("the GPU has no 3D acceleration: drawing in software");
    }
    let context = EGLContext::new(&display).context("could not create an EGL context")?;
    // SAFETY: the context was just created and is used on this thread only.
    #[allow(unsafe_code)]
    let renderer = unsafe { GlesRenderer::new(context) }.context("could not start GLES")?;
    state.shm_state.update_formats(renderer.shm_formats());

    let render_formats: FormatSet = renderer.egl_context().dmabuf_render_formats().clone();
    let render_node = node
        .node_with_type(NodeType::Render)
        .and_then(Result::ok)
        .unwrap_or(node);
    let outputs = DrmOutputManager::new(
        drm,
        GbmAllocator::new(
            gbm.clone(),
            GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
        ),
        GbmFramebufferExporter::new(gbm.clone(), Some(render_node)),
        Some(gbm),
        COLOR_FORMATS,
        render_formats,
    );

    let dmabuf = {
        let feedback = DmabufFeedbackBuilder::new(render_node.dev_id(), renderer.dmabuf_formats())
            .build()
            .context("could not describe the GPU's buffer formats")?;
        let display = state.display_handle.clone();
        state
            .dmabuf_state
            .create_global_with_default_feedback::<State>(&display, &feedback)
    };

    handle
        .insert_source(notifier, move |event, metadata, app| match event {
            DrmEvent::VBlank(crtc) => app.timed("vblank", |app| on_vblank(app, crtc, metadata)),
            DrmEvent::Error(err) => tracing::warn!(%err, "GPU error"),
        })
        .map_err(|err| err.error)
        .context("could not watch the GPU")?;
    tracing::info!(gpu = %path.display(), %render_node, "GPU ready");

    Ok(Gpu {
        node,
        path: path.to_path_buf(),
        renderer,
        outputs,
        screens: HashMap::new(),
        off: Vec::new(),
        _dmabuf: dmabuf,
    })
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
            tty.gpu.outputs.pause();
        }
        SessionEvent::ActivateSession => {
            tracing::info!("session back");
            tty.active = true;
            if let Err(()) = tty.libinput.resume() {
                tracing::warn!("libinput could not resume");
            }
            if let Err(err) = tty.gpu.outputs.activate(false) {
                tracing::warn!(%err, "could not take the screens back");
            }
            // What the screens show was another terminal's: draw it all.
            for screen in tty.gpu.screens.values_mut() {
                screen.flip = Flip::Idle;
                screen.surface.with_compositor(|c| {
                    if let Err(err) = c.reset_state() {
                        tracing::warn!(%err, "could not reset a screen");
                    }
                });
                screen.surface.reset_buffers();
            }
            // Screens may have come or gone meanwhile.
            tty.scan(&mut app.state);
            app.state.needs_redraw = true;
        }
    }
}

fn on_device(app: &mut App, event: UdevEvent) {
    let BackendState::Tty(tty) = &mut app.backend else {
        return;
    };
    match event {
        UdevEvent::Changed { device_id } if device_id == tty.gpu.node.dev_id() => {
            tty.scan(&mut app.state);
        }
        UdevEvent::Added { path, .. } => {
            tracing::info!(device = %path.display(), "another GPU: its screens stay dark for now");
        }
        _ => {}
    }
}

fn on_vblank(app: &mut App, crtc: crtc::Handle, metadata: &mut Option<DrmEventMetadata>) {
    let App { state, backend } = app;
    let BackendState::Tty(tty) = backend else {
        return;
    };
    let Some(screen) = tty.gpu.screens.get_mut(&crtc) else {
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
        tty.gpu.render(crtc, state);
    }
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
    /// Lights the screens newly connected and lets go of those unplugged.
    fn scan(&mut self, state: &mut State) {
        let device = self.gpu.outputs.device();
        let Ok(resources) = device.resource_handles() else {
            tracing::warn!("could not list the GPU's connectors");
            return;
        };
        let connected: Vec<connector::Info> = resources
            .connectors()
            .iter()
            .filter_map(|c| device.get_connector(*c, true).ok())
            .filter(|c| c.state() == connector::State::Connected && !c.modes().is_empty())
            .collect();

        self.gpu
            .off
            .retain(|o| connected.iter().any(|c| c.handle() == o.connector));
        // Gone: unplugged, or no longer connected.
        let gone: Vec<crtc::Handle> = self
            .gpu
            .screens
            .iter()
            .filter(|(_, s)| !connected.iter().any(|c| c.handle() == s.connector))
            .map(|(crtc, _)| *crtc)
            .collect();
        for crtc in gone {
            if let Some(screen) = self.gpu.screens.remove(&crtc) {
                tracing::info!(output = screen.output.name(), "screen unplugged");
                state.unplug_output(&screen.output);
                state.retire_global(screen.global);
                for token in [screen.health, screen.pace].into_iter().flatten() {
                    if let Some(handle) = &state.loop_handle {
                        handle.remove(token);
                    }
                }
            }
        }

        for info in connected {
            let known = self
                .gpu
                .screens
                .values()
                .any(|s| s.connector == info.handle())
                || self.gpu.off.iter().any(|o| o.connector == info.handle());
            if known {
                continue;
            }
            let Some(crtc) = self.free_crtc(&info) else {
                tracing::warn!(connector = ?info.handle(), "no CRTC left for a screen");
                continue;
            };
            match self.light(crtc, &info, None, state) {
                Ok(screen) => {
                    self.gpu.screens.insert(crtc, screen);
                }
                Err(err) => tracing::warn!("could not light a screen: {err:#}"),
            }
        }
        state.needs_redraw = true;
    }

    /// A CRTC this connector's encoders can drive and no screen uses.
    fn free_crtc(&self, info: &connector::Info) -> Option<crtc::Handle> {
        let device = self.gpu.outputs.device();
        let resources = device.resource_handles().ok()?;
        info.encoders()
            .iter()
            .filter_map(|e| device.get_encoder(*e).ok())
            .flat_map(|e| resources.filter_crtcs(e.possible_crtcs()))
            .find(|crtc| !self.gpu.screens.contains_key(crtc))
    }

    fn light(
        &mut self,
        crtc: crtc::Handle,
        info: &connector::Info,
        mode: Option<control::Mode>,
        state: &mut State,
    ) -> anyhow::Result<Screen> {
        let name = format!("{}-{}", info.interface().as_str(), info.interface_id());
        let device = self.gpu.outputs.device();
        let (make, model) = edid(device, info.handle())
            .map(|e| identity(&e))
            .unwrap_or_default();
        let preferred = info
            .modes()
            .iter()
            .position(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED));
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
        output.change_current_state(
            Some(mode),
            Some(Transform::Normal),
            None,
            Some((x, 0).into()),
        );
        output.set_preferred(mode);

        let surface = self
            .gpu
            .outputs
            .initialize_output::<_, crate::scene::OutputElement<GlesRenderer>>(
                crtc,
                drm_mode,
                &[info.handle()],
                &output,
                None,
                &mut self.gpu.renderer,
                &DrmOutputRenderElements::default(),
            )
            .map_err(|err| anyhow::anyhow!("{err}"))
            .with_context(|| format!("could not drive {name}"))?;

        state.remember_places();
        state.space.map_output(&output, (x, 0));
        state.plug_output(&output, DEFAULT_DESKTOPS);
        state.emit(|s| Event::OutputAdded {
            output: s.output_info(&output),
        });
        tracing::info!(
            output = name,
            mode = %format!("{}x{}@{:.2}", mode.size.w, mode.size.h, f64::from(mode.refresh) / 1000.0),
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
        })
    }

    /// Draws the screens that changed and are free to take a frame.
    pub fn after_dispatch(&mut self, state: &mut State) {
        if !state.needs_redraw {
            return;
        }
        state.needs_redraw = false;
        for screen in self.gpu.screens.values_mut() {
            screen.dirty = true;
        }
        if !self.active {
            return;
        }
        let ready: Vec<crtc::Handle> = self
            .gpu
            .screens
            .iter()
            .filter(|(_, s)| !s.dark && !s.flip.is_queued() && s.pace.is_none())
            .map(|(crtc, _)| *crtc)
            .collect();
        for crtc in ready {
            self.gpu.render(crtc, state);
        }
        state.space.refresh();
        state.popups.cleanup();
    }

    /// Night light: `rgb` scales each channel of `output`'s gamma ramp.
    pub fn set_gamma(&mut self, output: &Output, rgb: [f32; 3]) {
        let device = self.gpu.outputs.device();
        for (crtc, screen) in &self.gpu.screens {
            if screen.output != *output {
                continue;
            }
            let length = match device.get_crtc(*crtc) {
                Ok(info) => info.gamma_length() as usize,
                Err(err) => {
                    tracing::warn!(%err, "no gamma for night light");
                    continue;
                }
            };
            if length < 2 {
                continue;
            }
            let ramp = |scale: f32| -> Vec<u16> {
                (0..length)
                    .map(|i| (i as f32 / (length - 1) as f32 * 65535.0 * scale).round() as u16)
                    .collect()
            };
            let (r, g, b) = (ramp(rgb[0]), ramp(rgb[1]), ramp(rgb[2]));
            if let Err(err) = device.set_gamma(*crtc, &r, &g, &b) {
                tracing::warn!(%err, output = output.name(), "night light: the gamma was refused");
            }
        }
    }

    pub fn capture_output(
        &mut self,
        output: &Output,
        state: &State,
        cursor: bool,
    ) -> anyhow::Result<Pixels> {
        capture::output::<_, GlesTexture>(&mut self.gpu.renderer, output, state, cursor)
    }

    /// Every connected screen, lit or off.
    pub fn heads(&self, state: &State) -> Vec<Head> {
        let lit = self.gpu.screens.values().map(|s| {
            let position = state
                .space
                .output_geometry(&s.output)
                .map_or((0, 0), |g| (g.loc.x, g.loc.y));
            Head {
                now: Setting::of(&s.output, position, true, s.vrr),
                ..s.head.clone()
            }
        });
        let off = self.gpu.off.iter().map(|o| o.head.clone());
        let mut heads: Vec<Head> = lit.chain(off).collect();
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
        if let Some(i) = self.gpu.off.iter().position(|o| o.head.name == name) {
            if !setting.enabled {
                return Ok(());
            }
            let off = self.gpu.off.remove(i);
            let device = self.gpu.outputs.device();
            let info = device
                .get_connector(off.connector, false)
                .context("the screen is gone")?;
            let mode = info
                .modes()
                .iter()
                .find(|m| core_mode(**m) == setting.mode)
                .copied();
            let crtc = self.free_crtc(&info).context("no CRTC left")?;
            let screen = self.light(crtc, &info, mode, state)?;
            let output = screen.output.clone();
            self.gpu.screens.insert(crtc, screen);
            state.set_output(&output, setting);
            return Ok(());
        }
        let (crtc, screen) = self
            .gpu
            .screens
            .iter_mut()
            .find(|(_, s)| s.output.name() == name)
            .map(|(c, s)| (*c, s))
            .with_context(|| format!("no screen named {name}"))?;
        if !setting.enabled {
            screen.surface.with_compositor(|c| {
                if let Err(err) = c.clear() {
                    tracing::warn!(%err, "could not turn the screen off");
                }
            });
            let Some(screen) = self.gpu.screens.remove(&crtc) else {
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
            self.gpu.off.push(Off {
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
            screen
                .surface
                .use_mode::<_, crate::scene::OutputElement<GlesRenderer>>(
                    mode,
                    &mut self.gpu.renderer,
                    &DrmOutputRenderElements::default(),
                )
                .map_err(|err| anyhow::anyhow!("{err}"))?;
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
        let (crtc, screen) = self
            .gpu
            .screens
            .iter_mut()
            .find(|(_, s)| s.output.name() == name)
            .map(|(c, s)| (*c, s))
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
            screen.surface.reset_buffers();
            screen.dirty = true;
            if self.active {
                self.gpu.render(crtc, state);
            }
        }
        Ok(())
    }

    pub fn capture_window(&mut self, window: &Window, scale: f64) -> anyhow::Result<Pixels> {
        capture::window::<_, GlesTexture>(&mut self.gpu.renderer, window, scale)
    }
}

impl Gpu {
    /// Draws one screen and queues the frame for its next vblank.
    fn render(&mut self, crtc: crtc::Handle, state: &mut State) {
        let Self {
            renderer, screens, ..
        } = self;
        let Some(screen) = screens.get_mut(&crtc) else {
            return;
        };
        let start = Instant::now();
        screen.dirty = false;
        // A wallpaper that moves, then the blur behind panels, before the
        // scene shows them.
        state.paint_backdrop(renderer, &screen.output);
        state.paint_glass(renderer, &screen.output);
        let elements = state.scene_elements(renderer, &screen.output, true);
        let result =
            screen
                .surface
                .render_frame(renderer, &elements, BACKGROUND, FrameFlags::DEFAULT);
        let now = state.clock.now();
        let (empty, states) = match result {
            Ok(result) => (result.is_empty, result.states),
            Err(err) => {
                tracing::warn!(output = screen.output.name(), ?err, "rendering failed");
                return;
            }
        };
        if !empty {
            match screen.surface.queue_frame(()) {
                Ok(()) => {
                    state.measures.frame(&screen.output.name(), start.elapsed());
                    screen.flip.queued(now);
                    watch_flip(screen, crtc, state);
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
        state.send_frames(&screen.output, now, Some(&states));
        if empty {
            pace(screen, crtc, state);
        }
    }
}

/// The time between two refreshes of a screen.
fn refresh(output: &Output) -> std::time::Duration {
    let mhz = output.current_mode().map_or(60_000, |m| m.refresh.max(1));
    std::time::Duration::from_micros(1_000_000_000 / mhz as u64)
}

/// Waits a refresh before the screen may draw again after an empty frame.
fn pace(screen: &mut Screen, crtc: crtc::Handle, state: &State) {
    let Some(handle) = &state.loop_handle else {
        return;
    };
    if screen.pace.is_some() {
        return;
    }
    let timer = Timer::from_duration(refresh(&screen.output));
    match handle.insert_source(timer, move |_, _, app| {
        let App { state, backend } = app;
        if let BackendState::Tty(tty) = backend
            && let Some(screen) = tty.gpu.screens.get_mut(&crtc)
        {
            screen.pace = None;
            if screen.dirty && tty.active && !screen.dark && !screen.flip.is_queued() {
                tty.gpu.render(crtc, state);
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
fn watch_flip(screen: &mut Screen, crtc: crtc::Handle, state: &State) {
    let Some(handle) = &state.loop_handle else {
        return;
    };
    if screen.health.is_some() {
        return;
    }
    let timer = Timer::from_duration(flip::CHECK);
    match handle.insert_source(timer, move |_, _, app| {
        let App { state, backend } = app;
        let BackendState::Tty(tty) = backend else {
            return TimeoutAction::Drop;
        };
        let active = tty.active;
        let Some(screen) = tty.gpu.screens.get_mut(&crtc) else {
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
                screen.surface.reset_buffers();
                screen.dirty = true;
                let output = screen.output.clone();
                state.output_recovered(&output, missed);
                tty.gpu.render(crtc, state);
                TimeoutAction::Drop
            }
        }
    }) {
        Ok(token) => screen.health = Some(token),
        Err(err) => tracing::warn!(%err.error, "could not watch a screen"),
    }
}

/// The EDID a screen sent, from its connector's property.
fn edid(device: &DrmDevice, connector: connector::Handle) -> Option<Vec<u8>> {
    let properties = device.get_properties(connector).ok()?;
    let (handles, values) = properties.as_props_and_values();
    handles.iter().zip(values).find_map(|(handle, value)| {
        let info = device.get_property(*handle).ok()?;
        if info.name().to_str() != Ok("EDID") {
            return None;
        }
        let property::Value::Blob(blob) = info.value_type().convert_value(*value) else {
            return None;
        };
        device.get_property_blob(blob).ok()
    })
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
}
