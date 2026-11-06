//! The session itself, on a virtual terminal: the seat comes from libseat
//! (logind or seatd), the screens are driven through DRM/KMS and drawn with
//! GLES, input comes from libinput. Switching to another terminal pauses
//! all of it; coming back draws everything again.
//!
//! The primary GPU's connected screens are lit, and follow what is plugged
//! and unplugged. Other GPUs' screens and VR headsets (drm-lease) come with
//! the several-screens slice.

use std::{collections::HashMap, path::PathBuf, time::Duration};

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
        egl::{EGLDevice, EGLDisplay},
        input::InputEvent,
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            ImportDma, ImportMemWl,
            gles::{GlesRenderer, GlesTexture},
            multigpu::{GpuManager, gbm::GbmGlesBackend},
        },
        session::{Event as SessionEvent, Session, libseat::LibSeatSession},
        udev::{UdevBackend, UdevEvent, all_gpus, primary_gpu},
    },
    output::{Mode, Output, PhysicalProperties},
    reexports::{
        calloop::{
            LoopHandle, RegistrationToken,
            timer::{TimeoutAction, Timer},
        },
        drm::control::{Device as ControlDevice, ModeTypeFlags, connector, crtc, property},
        input::{DeviceCapability, Libinput},
        rustix::fs::OFlags,
        wayland_server::backend::GlobalId,
    },
    utils::{DeviceFd, Transform},
};

use crate::{
    App, State,
    backend::{
        Backend, add_output,
        clips::clipped,
        dmabuf_global,
        flip::{self, Flip, Health},
        scale,
    },
    render::{BACKGROUND, OutputElement},
};

type Allocator = GbmAllocator<DrmDeviceFd>;
type Exporter = GbmFramebufferExporter<DrmDeviceFd>;
type Outputs = DrmOutputManager<Allocator, Exporter, (), DrmDeviceFd>;
type Surface = DrmOutput<Allocator, Exporter, (), DrmDeviceFd>;
type Api = GbmGlesBackend<GlesRenderer, DrmDeviceFd>;

/// Scan-out formats, tried in order: 8 bits per channel, which every
/// display engine takes.
const COLOR_FORMATS: [Fourcc; 2] = [Fourcc::Argb8888, Fourcc::Abgr8888];

pub struct TtyBackend {
    session: LibSeatSession,
    libinput: Libinput,
    /// The renderer of the primary GPU, which draws every screen. Kept in
    /// smithay's multi-GPU manager, which other GPUs will join.
    gpus: GpuManager<Api>,
    /// The primary GPU's render node, as `gpus` knows it.
    render_node: DrmNode,
    gpu: Gpu,
    /// The session is in the foreground: its terminal is the one shown.
    active: bool,
    /// Screen sharing's frames (5.11), on the primary GPU.
    share: crate::share::frames::Frames<GlesRenderer, GlesTexture>,
}

impl std::fmt::Debug for TtyBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TtyBackend")
            .field("seat", &self.session.seat())
            .field("gpu", &self.gpu.path)
            .field("screens", &self.gpu.screens.len())
            .field("active", &self.active)
            .finish()
    }
}

/// The primary GPU: its display engine and its screens.
struct Gpu {
    node: DrmNode,
    path: PathBuf,
    outputs: Outputs,
    screens: HashMap<crtc::Handle, Screen>,
}

/// One connector lit, through one CRTC.
struct Screen {
    connector: connector::Handle,
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
    /// Lit (DPMS on), or turned off by idle or the lid (7.2).
    powered: bool,
    /// Its connector's modes, as the kernel gives them.
    modes: Vec<smithay::reexports::drm::control::Mode>,
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
    let mut libinput =
        Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(session.clone().into());
    libinput
        .udev_assign_seat(&seat)
        .map_err(|()| anyhow::anyhow!("libinput could not take seat {seat}"))?;

    let (gpu, gpus, render_node) = open_primary(handle, &session, &path, state)?;
    let mut backend = TtyBackend {
        session: session.clone(),
        libinput: libinput.clone(),
        gpus,
        render_node,
        gpu,
        active: true,
        share: Default::default(),
    };

    handle
        .insert_source(LibinputInputBackend::new(libinput), |event, _, app| {
            match &event {
                InputEvent::DeviceAdded { device } => {
                    let pointer = device.has_capability(DeviceCapability::Pointer);
                    tracing::info!(device = device.name(), pointer, "input device");
                    // [pointer] and [touchpad] given as it comes (6.12).
                    app.state.input_device_added(device.clone());
                }
                InputEvent::DeviceRemoved { device } => app.state.input_device_removed(device),
                // A touchpad never said to sense fewer fingers than it
                // was seen gesturing with (#720).
                InputEvent::GestureSwipeBegin { event } => {
                    use smithay::backend::input::{Event as _, GestureBeginEvent as _};
                    app.state.gesture_seen(&event.device(), event.fingers());
                }
                InputEvent::GesturePinchBegin { event } => {
                    use smithay::backend::input::{Event as _, GestureBeginEvent as _};
                    app.state.gesture_seen(&event.device(), event.fingers());
                }
                InputEvent::GestureHoldBegin { event } => {
                    use smithay::backend::input::{Event as _, GestureBeginEvent as _};
                    app.state.gesture_seen(&event.device(), event.fingers());
                }
                _ => {}
            }
            app.state.process_input_event(event);
        })
        .map_err(|err| err.error)
        .context("could not watch input devices")?;

    handle
        .insert_source(notifier, |event, (), app| on_session(app, event))
        .map_err(|err| err.error)
        .context("could not watch the session")?;

    // Screens plugged and unplugged.
    let udev = UdevBackend::new(&seat).context("could not watch devices")?;
    handle
        .insert_source(udev, |event, (), app| on_device(app, event))
        .map_err(|err| err.error)
        .context("could not watch devices")?;

    backend.scan(state);
    anyhow::ensure!(!backend.gpu.screens.is_empty(), "no screen is connected");
    state.session = Some(session);
    Ok(backend)
}

/// Opens the primary GPU through the seat, with its renderer.
fn open_primary(
    handle: &LoopHandle<'static, App>,
    session: &LibSeatSession,
    path: &std::path::Path,
    state: &mut State,
) -> anyhow::Result<(Gpu, GpuManager<Api>, DrmNode)> {
    let fd = session
        .clone()
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
    let render_node = render_node(&gbm, node);

    let mut gpus = GpuManager::new(Api::default()).map_err(|err| anyhow::anyhow!("{err}"))?;
    gpus.as_mut()
        .add_node(render_node, gbm.clone())
        .context("could not open EGL")?;
    let render_formats: FormatSet = {
        let mut renderer = gpus
            .single_renderer(&render_node)
            .map_err(|err| anyhow::anyhow!("{err}"))
            .context("could not start GLES")?;
        let gles: &mut GlesRenderer = renderer.as_mut();
        state.shm_state.update_formats(gles.shm_formats());
        dmabuf_global(state, gles.dmabuf_formats(), Some(render_node.dev_id()));
        state.blur = true;
        gles.egl_context().dmabuf_render_formats().clone()
    };
    let allocator = GbmAllocator::new(
        gbm.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );
    let outputs = DrmOutputManager::new(
        drm,
        allocator,
        GbmFramebufferExporter::new(gbm.clone(), Some(render_node)),
        Some(gbm),
        COLOR_FORMATS,
        render_formats,
    );
    handle
        .insert_source(notifier, move |event, metadata, app| match event {
            DrmEvent::VBlank(crtc) => on_vblank(app, crtc, metadata),
            DrmEvent::Error(err) => tracing::warn!(%err, %node, "GPU error"),
        })
        .map_err(|err| err.error)
        .context("could not watch the GPU")?;
    tracing::info!(gpu = %path.display(), %render_node, "GPU ready");
    Ok((
        Gpu {
            node,
            path: path.to_path_buf(),
            outputs,
            screens: HashMap::new(),
        },
        gpus,
        render_node,
    ))
}

/// The render node a GPU's renderer goes by; a VM's GPU without 3D draws
/// with Mesa's software GL, slowly but correctly.
fn render_node(gbm: &GbmDevice<DrmDeviceFd>, node: DrmNode) -> DrmNode {
    let fallback = node
        .node_with_type(NodeType::Render)
        .and_then(Result::ok)
        .unwrap_or(node);
    // SAFETY: nothing else in the process creates EGL displays, so
    // smithay's tracking of them is accurate: this one, which only looks,
    // and the renderer's for the same device share one, terminated when the
    // last goes.
    #[allow(unsafe_code)]
    let Ok(display) = (unsafe { EGLDisplay::new(gbm.clone()) }) else {
        return fallback;
    };
    match EGLDevice::device_for_display(&display) {
        Ok(device) => {
            if device.is_software() {
                tracing::warn!("the GPU has no 3D acceleration: drawing in software");
            }
            device
                .try_get_render_node()
                .ok()
                .flatten()
                .unwrap_or(fallback)
        }
        Err(_) => fallback,
    }
}

fn on_session(app: &mut App, event: SessionEvent) {
    let App { state, backend } = app;
    let Backend::Tty(tty) = backend else {
        return;
    };
    match event {
        SessionEvent::PauseSession => {
            tracing::info!("session paused: another terminal is shown");
            tty.active = false;
            // The display manager's lock, or another user's session: a
            // recording stops, saved (5.10).
            state.record_screen_locks();
            tty.libinput.suspend();
            tty.gpu.outputs.pause();
        }
        SessionEvent::ActivateSession => {
            tracing::info!("session back");
            tty.active = true;
            if tty.libinput.resume().is_err() {
                tracing::warn!("libinput could not resume");
            }
            if let Err(err) = tty.gpu.outputs.activate(false) {
                tracing::warn!(%err, "could not take the screens back");
            }
            // The lid may have moved meanwhile, unseen.
            state.session_back();
            // What the screens show was another terminal's: draw it all.
            tty.relight();
            // Screens may have come or gone meanwhile.
            tty.scan(state);
        }
    }
}

fn on_device(app: &mut App, event: UdevEvent) {
    let App { state, backend } = app;
    let Backend::Tty(tty) = backend else {
        return;
    };
    match event {
        UdevEvent::Changed { device_id } if device_id == tty.gpu.node.dev_id() => tty.scan(state),
        UdevEvent::Removed { device_id } if device_id == tty.gpu.node.dev_id() => {
            tracing::error!("the GPU went: nothing draws the screens any more");
        }
        // Other GPUs come with the several-screens slice.
        _ => {}
    }
}

fn on_vblank(app: &mut App, crtc: crtc::Handle, metadata: &mut Option<DrmEventMetadata>) {
    let App { state, backend } = app;
    let Backend::Tty(tty) = backend else {
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
    let time = metadata
        .as_ref()
        .and_then(|m| match m.time {
            DrmEventTime::Monotonic(time) if !time.is_zero() => Some(time),
            _ => None,
        })
        .unwrap_or_else(|| state.clock.now().into());
    state.presented(&screen.output, time, screen.presented);
    if screen.dirty && tty.active {
        tty.render(crtc, state);
    }
}

/// The mode a connector prefers, by its place in its modes.
fn preferred(info: &connector::Info) -> Option<usize> {
    info.modes()
        .iter()
        .position(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
}

fn connector_name(info: &connector::Info) -> String {
    format!("{}-{}", info.interface().as_str(), info.interface_id())
}

impl TtyBackend {
    /// Lights the screens newly connected and lets go of those unplugged.
    fn scan(&mut self, state: &mut State) {
        let drm = self.gpu.outputs.device();
        let Ok(resources) = drm.resource_handles() else {
            tracing::warn!(gpu = %self.gpu.path.display(), "could not list the GPU's connectors");
            return;
        };
        let connected: Vec<connector::Info> = resources
            .connectors()
            .iter()
            .filter_map(|c| drm.get_connector(*c, true).ok())
            .filter(|c| c.state() == connector::State::Connected && !c.modes().is_empty())
            .filter(|c| !non_desktop(drm, c.handle()))
            .collect();
        // Gone, or another screen on the same connector (swapped while
        // the machine slept): that one goes, the other lights.
        let gone: Vec<crtc::Handle> = self
            .gpu
            .screens
            .iter()
            .filter(|(_, s)| {
                !connected.iter().any(|c| c.handle() == s.connector)
                    || s.output
                        .user_data()
                        .get::<crate::screen_sets::Identity>()
                        .is_some_and(|was| {
                            let read = edid(drm, s.connector);
                            let (make, model) = read.as_deref().map(identity).unwrap_or_default();
                            let now = crate::screen_sets::Identity {
                                make,
                                model,
                                serial: read.as_deref().and_then(serial),
                            };
                            *was != now
                        })
            })
            .map(|(crtc, _)| *crtc)
            .collect();
        for crtc in gone {
            self.unplug(crtc, state);
        }
        for info in &connected {
            if self
                .gpu
                .screens
                .values()
                .any(|s| s.connector == info.handle())
            {
                continue;
            }
            let Some(crtc) = self.free_crtc(info) else {
                tracing::warn!(
                    connector = connector_name(info),
                    "no CRTC left for a screen"
                );
                continue;
            };
            match self.light(crtc, info, state) {
                Ok(screen) => {
                    // The windows that came from it go back.
                    state.output_back(&screen.output, true);
                    self.gpu.screens.insert(crtc, screen);
                }
                Err(err) => tracing::warn!("could not light a screen: {err:#}"),
            }
        }
        for screen in self.gpu.screens.values_mut() {
            screen.dirty = true;
        }
        state.redraw = true;
    }

    /// A screen leaves the desktop.
    fn unplug(&mut self, crtc: crtc::Handle, state: &mut State) {
        let Some(screen) = self.gpu.screens.remove(&crtc) else {
            return;
        };
        tracing::info!(output = screen.output.name(), "screen unplugged");
        screen.surface.with_compositor(|c| {
            if let Err(err) = c.clear() {
                tracing::debug!(%err, "could not clear the screen");
            }
        });
        // Its windows go to a screen still there, then its global.
        state.unplug_output(&screen.output, screen.global);
        for token in [screen.health, screen.pace].into_iter().flatten() {
            state.loop_handle.remove(token);
        }
        state.redraw = true;
    }

    /// A CRTC this connector's encoders can drive that no screen uses.
    fn free_crtc(&self, info: &connector::Info) -> Option<crtc::Handle> {
        let drm = self.gpu.outputs.device();
        let resources = drm.resource_handles().ok()?;
        info.encoders()
            .iter()
            .filter_map(|e| drm.get_encoder(*e).ok())
            .flat_map(|e| resources.filter_crtcs(e.possible_crtcs()))
            .find(|crtc| !self.gpu.screens.contains_key(crtc))
    }

    fn light(
        &mut self,
        crtc: crtc::Handle,
        info: &connector::Info,
        state: &mut State,
    ) -> anyhow::Result<Screen> {
        let name = connector_name(info);
        let drm = self.gpu.outputs.device();
        let read = edid(drm, info.handle());
        let (make, model) = read.as_deref().map(identity).unwrap_or_default();
        // Who it is (windows.md §12): its make, model and serial.
        let who = crate::screen_sets::Identity {
            make: make.clone(),
            model: model.clone(),
            serial: read.as_deref().and_then(serial),
        };
        let preferred_mode = preferred(info)
            .map(|i| info.modes()[i])
            .or_else(|| info.modes().first().copied())
            .context("no mode")?;
        let modes: Vec<crate::displays::Mode3> = crate::displays::sorted_modes(
            info.modes()
                .iter()
                .map(|m| crate::displays::mode3(Mode::from(*m)))
                .collect(),
        );
        let (width_mm, height_mm) = info.size().unwrap_or((0, 0));
        let mut facts = crate::displays::Facts {
            modes,
            preferred: crate::displays::mode3(Mode::from(preferred_mode)),
            vrr_capable: false,
            millimetres: (width_mm as i32, height_mm as i32),
            fixed_scale: None,
            placed: (0, 0),
            placed_size: (1, 1),
        };
        // Lit at once in the mode the file asks (6.9), else its preferred
        // size at its highest rate.
        let settings = state.joining_settings(&name, &who);
        let wanted = settings
            .as_ref()
            .and_then(|o| o.mode)
            .and_then(|m| m.pick(&facts.modes))
            .unwrap_or_else(|| facts.default_mode());
        let drm_mode = info
            .modes()
            .iter()
            .copied()
            .find(|m| crate::displays::mode3(Mode::from(*m)) == wanted)
            .unwrap_or(preferred_mode);
        let mode = Mode::from(drm_mode);
        let size_mm = (width_mm as i32, height_mm as i32);
        let output = Output::new(
            name.clone(),
            PhysicalProperties {
                size: size_mm.into(),
                subpixel: info.subpixel().into(),
                make: make.unwrap_or_else(|| "Unknown".into()),
                model: model.clone().unwrap_or_else(|| "Unknown".into()),
            },
        );
        output.user_data().insert_if_missing(|| who.clone());
        // The name people know it by, for the shell (its EDID's model).
        if let Some(model) = model {
            output
                .user_data()
                .insert_if_missing(|| crate::panel::ScreenModel(model));
        }
        let global = output.create_global::<State>(&state.display_handle);
        // To the right of the screens already lit, until the settings
        // arrange them (several-screens slice).
        let x = state
            .space
            .outputs()
            .filter_map(|o| state.space.output_geometry(o))
            .map(|g| g.loc.x + g.size.w)
            .max()
            .unwrap_or(0);
        // Its scale before it lights: lit at 1 first, its clients would
        // draw twice.
        let (output_scale, scale_from) =
            crate::config::output_scale(settings.as_ref(), (mode.size.w, mode.size.h), size_mm);
        output.change_current_state(
            Some(mode),
            Some(Transform::Normal),
            Some(scale(output_scale)),
            Some((x, 0).into()),
        );
        output.set_preferred(mode);

        let mut renderer = self
            .gpus
            .single_renderer(&self.render_node)
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let gles: &mut GlesRenderer = renderer.as_mut();
        let surface = self
            .gpu
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
            .with_context(|| format!("could not drive {name}"))?;

        facts.vrr_capable = surface.with_compositor(|c| {
            !matches!(
                c.vrr_supported(info.handle()),
                Ok(smithay::backend::drm::VrrSupport::NotSupported) | Err(_)
            )
        });
        facts.placed = (x, 0);
        facts.placed_size = crate::displays::logical_size(&output).unwrap_or((1, 1));
        output.user_data().insert_if_missing(|| facts);
        add_output(state, &output, (x, 0));
        tracing::info!(
            "output {name} {}x{}@{:.2} {width_mm}x{height_mm} mm scale {output_scale} {}",
            mode.size.w,
            mode.size.h,
            f64::from(mode.refresh) / 1000.0,
            scale_from.describe(&name),
        );
        Ok(Screen {
            connector: info.handle(),
            output,
            global,
            surface,
            flip: Flip::Idle,
            dirty: true,
            presented: 0,
            health: None,
            pace: None,
            powered: true,
            modes: info.modes().to_vec(),
        })
    }

    /// `output` driven in `mode` (6.9): the CRTC takes it with the next
    /// frame.
    pub fn set_mode(
        &mut self,
        state: &mut State,
        output: &Output,
        mode: crate::displays::Mode3,
    ) -> anyhow::Result<()> {
        let screen = self
            .gpu
            .screens
            .values_mut()
            .find(|s| s.output == *output)
            .with_context(|| format!("no screen {}", output.name()))?;
        let drm_mode = screen
            .modes
            .iter()
            .copied()
            .find(|m| crate::displays::mode3(Mode::from(*m)) == mode)
            .with_context(|| format!("{} does not offer it", output.name()))?;
        let mut renderer = self
            .gpus
            .single_renderer(&self.render_node)
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let gles: &mut GlesRenderer = renderer.as_mut();
        screen
            .surface
            .use_mode::<_, OutputElement<GlesRenderer>>(
                drm_mode,
                gles,
                &DrmOutputRenderElements::default(),
            )
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        output.change_current_state(Some(Mode::from(drm_mode)), None, None, None);
        screen.flip = Flip::Idle;
        screen.dirty = true;
        state.redraw = true;
        Ok(())
    }

    /// Variable refresh rate on `output`, or off.
    pub fn set_vrr(&mut self, output: &Output, on: bool) -> anyhow::Result<()> {
        let screen = self
            .gpu
            .screens
            .values_mut()
            .find(|s| s.output == *output)
            .with_context(|| format!("no screen {}", output.name()))?;
        screen
            .surface
            .with_compositor(|c| c.use_vrr(on))
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        screen.dirty = true;
        Ok(())
    }

    /// Every screen's state forgotten and drawn afresh, as after another
    /// terminal showed: the next frame commits its CRTC whole.
    fn relight(&mut self) {
        for screen in self.gpu.screens.values_mut() {
            screen.flip = Flip::Idle;
            screen.surface.with_compositor(|c| {
                if let Err(err) = c.reset_state() {
                    tracing::warn!(%err, "could not reset a screen");
                }
            });
            screen.surface.reset_buffers();
            screen.dirty = true;
            // Turned off again by `power` if it was.
            screen.powered = true;
        }
    }

    /// Turns the screens off or on as the state asks (7.2): off, its CRTC
    /// disabled (DPMS off) and nothing drawn; on, drawn afresh, the CRTC
    /// lit again with that frame.
    fn power(&mut self, state: &mut State) {
        for screen in self.gpu.screens.values_mut() {
            let powered = state.output_powered(&screen.output);
            if powered == screen.powered {
                continue;
            }
            screen.powered = powered;
            let output = screen.output.name();
            if powered {
                tracing::info!(output, "screen on");
                screen.flip = Flip::Idle;
                screen.surface.with_compositor(|c| {
                    if let Err(err) = c.reset_state() {
                        tracing::warn!(%err, "could not reset a screen");
                    }
                });
                screen.surface.reset_buffers();
                screen.dirty = true;
            } else {
                tracing::info!(output, "screen off");
                screen.surface.with_compositor(|c| {
                    if let Err(err) = c.clear() {
                        tracing::warn!(%err, "could not turn a screen off");
                    }
                });
                screen.flip = Flip::Idle;
                screen.dirty = false;
            }
        }
    }

    /// Draws the screens that changed and are free to take a frame.
    pub fn after_dispatch(&mut self, state: &mut State) {
        // The desktop's last frame as it locks (7.1), before anything else
        // draws it locked; none while another terminal shows.
        if state.lock.wants_freeze() {
            match self.gpus.single_renderer(&self.render_node) {
                Ok(mut renderer) if self.active => {
                    let gles: &mut GlesRenderer = renderer.as_mut();
                    crate::lock::draw::freeze::<_, GlesTexture>(gles, state);
                }
                _ => crate::lock::draw::no_freeze(state),
            }
        }
        // A capture asked for (5.8): drawn for it on the primary GPU, which
        // draws every screen; none while another terminal shows.
        if state.capture.has_pending() {
            if !self.active {
                tracing::info!("the session is in the background: nothing captured");
                state.capture.take_pending();
            } else {
                match self.gpus.single_renderer(&self.render_node) {
                    Ok(mut renderer) => {
                        let gles: &mut GlesRenderer = renderer.as_mut();
                        crate::capture::shoot::take::<_, GlesTexture>(gles, state);
                    }
                    Err(err) => {
                        tracing::warn!(%err, "no renderer: nothing captured");
                        state.capture.take_pending();
                    }
                }
            }
        }
        // Screen sharing's thumbnails and frames (5.11): none while another
        // terminal shows.
        if self.active
            && let Ok(mut renderer) = self.gpus.single_renderer(&self.render_node)
        {
            let gles: &mut GlesRenderer = renderer.as_mut();
            crate::share::frames::run::<_, GlesTexture>(gles, state, &mut self.share);
        }
        if self.active {
            // Back from sleep (7.2): a display engine may have lost what
            // it showed; every screen is lit and drawn afresh.
            if std::mem::take(&mut state.away.woke) {
                tracing::info!("the machine woke up: the screens drawn afresh");
                self.relight();
            }
            self.power(state);
        }
        if !state.redraw {
            return;
        }
        state.redraw = false;
        for screen in self.gpu.screens.values_mut() {
            screen.dirty = screen.powered;
        }
        if !self.active {
            return;
        }
        let ready: Vec<crtc::Handle> = self
            .gpu
            .screens
            .iter()
            .filter(|(_, s)| s.powered && s.dirty && !s.flip.is_queued() && s.pace.is_none())
            .map(|(crtc, _)| *crtc)
            .collect();
        for crtc in ready {
            self.render(crtc, state);
        }
    }

    /// Draws one screen and queues the frame for its next vblank.
    fn render(&mut self, crtc: crtc::Handle, state: &mut State) {
        let Self {
            gpus,
            gpu,
            render_node,
            ..
        } = self;
        let Some(screen) = gpu.screens.get_mut(&crtc) else {
            return;
        };
        screen.dirty = false;
        // Off (7.2): nothing is drawn until it is on again.
        if !screen.powered {
            return;
        }
        let mut renderer = match gpus.single_renderer(render_node) {
            Ok(renderer) => renderer,
            Err(err) => {
                tracing::warn!(%err, "no renderer");
                return;
            }
        };
        let gles: &mut GlesRenderer = renderer.as_mut();
        let elements = state.output_elements(gles, &screen.output, true);
        let elements = clipped(&elements);
        let result =
            match screen
                .surface
                .render_frame(gles, &elements, BACKGROUND, FrameFlags::DEFAULT)
            {
                Ok(result) => result,
                Err(err) => {
                    tracing::warn!(output = screen.output.name(), ?err, "rendering failed");
                    return;
                }
            };
        let empty = result.is_empty;
        let states = result.states;
        if !empty {
            match screen.surface.queue_frame(()) {
                Ok(()) => {
                    screen.flip.queued(state.clock.now().into());
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
        state.send_frames(&screen.output, Some(&states));
        state.desktops_drawn(&screen.output);
        state.read_under_grips(gles);
        if empty {
            pace(screen, crtc, state);
        }
    }
}

/// The time between two refreshes of a screen.
fn refresh(output: &Output) -> Duration {
    let mhz = output.current_mode().map_or(60_000, |m| m.refresh.max(1));
    Duration::from_micros(1_000_000_000 / mhz as u64)
}

/// Waits a refresh before the screen may draw again after an empty frame.
fn pace(screen: &mut Screen, crtc: crtc::Handle, state: &State) {
    if screen.pace.is_some() {
        return;
    }
    let timer = Timer::from_duration(refresh(&screen.output));
    let inserted = state.loop_handle.insert_source(timer, move |_, _, app| {
        let App { state, backend } = app;
        if let Backend::Tty(tty) = backend {
            let active = tty.active;
            if let Some(screen) = tty.gpu.screens.get_mut(&crtc) {
                screen.pace = None;
                if screen.dirty && active && !screen.flip.is_queued() {
                    tty.render(crtc, state);
                }
            }
        }
        TimeoutAction::Drop
    });
    match inserted {
        Ok(token) => screen.pace = Some(token),
        Err(err) => tracing::warn!(%err.error, "could not pace a screen"),
    }
}

/// Looks at the frame out once a second until it is presented, and
/// recovers the screen when it never is.
fn watch_flip(screen: &mut Screen, crtc: crtc::Handle, state: &State) {
    if screen.health.is_some() {
        return;
    }
    let timer = Timer::from_duration(flip::CHECK);
    let inserted = state.loop_handle.insert_source(timer, move |_, _, app| {
        let App { state, backend } = app;
        let Backend::Tty(tty) = backend else {
            return TimeoutAction::Drop;
        };
        let active = tty.active;
        let Some(screen) = tty.gpu.screens.get_mut(&crtc) else {
            return TimeoutAction::Drop;
        };
        // Another terminal is shown: nothing is presented, nothing is late.
        let health = if active {
            screen.flip.check(state.clock.now().into())
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
                tracing::warn!(
                    output = screen.output.name(),
                    missed,
                    "the screen stopped presenting frames; recovered"
                );
                // The lost flip counts as done; the CRTC is committed afresh
                // with the next frame.
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
                tty.render(crtc, state);
                TimeoutAction::Drop
            }
        }
    });
    match inserted {
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

/// A VR headset says it is no desktop: never lit.
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

/// The screen's serial number, from its EDID: its serial descriptor's
/// text, else the number in its header (none when 0).
pub fn serial(edid: &[u8]) -> Option<String> {
    const HEADER: [u8; 8] = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00];
    if edid.len() < 128 || edid[..8] != HEADER {
        return None;
    }
    let text = (0..4).find_map(|i| {
        let d = &edid[54 + i * 18..72 + i * 18];
        (d[..3] == [0, 0, 0] && d[3] == 0xff).then(|| {
            String::from_utf8_lossy(&d[5..])
                .split('\n')
                .next()
                .unwrap_or_default()
                .trim()
                .to_string()
        })
    });
    text.filter(|t| !t.is_empty()).or_else(|| {
        let number = u32::from_le_bytes([edid[12], edid[13], edid[14], edid[15]]);
        (number != 0).then(|| number.to_string())
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
    let letters = [letter(10), letter(5), letter(0)];
    let make = letters
        .iter()
        .all(char::is_ascii_uppercase)
        .then(|| letters.iter().collect());
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
        // No serial: none in the header, no serial descriptor.
        assert_eq!(serial(&edid), None);
        edid[12..16].copy_from_slice(&1234u32.to_le_bytes());
        assert_eq!(serial(&edid).as_deref(), Some("1234"));
        // The descriptor's text wins.
        let d = 54 + 2 * 18;
        edid[d..d + 5].copy_from_slice(&[0, 0, 0, 0xff, 0]);
        edid[d + 5..d + 18].copy_from_slice(b"7XY1234\n     ");
        assert_eq!(serial(&edid).as_deref(), Some("7XY1234"));
    }
}
