//! The headless backend: no screen, outputs are offscreen buffers, drawn
//! with GLES on a GPU's render node or with pixman. Tests run on it.

use std::{path::PathBuf, time::Duration};

use anyhow::Context;
use smithay::{
    backend::{
        allocator::Fourcc,
        drm::DrmNode,
        egl::{EGLContext, EGLDevice, EGLDisplay},
        renderer::{
            Bind, ExportMem, ImportAll, ImportDma, ImportMem, ImportMemWl, Offscreen, Renderer,
            Texture,
            damage::OutputDamageTracker,
            gles::{GlesRenderer, GlesTexture},
            pixman::PixmanRenderer,
        },
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::{
            LoopHandle, RegistrationToken,
            timer::{TimeoutAction, Timer},
        },
        pixman,
        wayland_server::backend::GlobalId,
    },
    utils::{Physical, Rectangle, Transform},
};

use crate::{
    App, State,
    backend::{Backend, add_output, dmabuf_global, render_node_of, scale},
    magnifier::draw::{self as magnifier, Lens, Magnify},
    output_spec::{self, OutputSpec},
    render::BACKGROUND,
};

/// Headless outputs refresh at a nominal 60 Hz.
const REFRESH_MHZ: i32 = 60_000;
/// The time between two frames of a headless output.
const FRAME: Duration = Duration::from_nanos(1_000_000_000_000 / REFRESH_MHZ as u64);

/// Which renderer to draw with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RendererChoice {
    /// GLES on a render node when there is one, pixman otherwise.
    #[default]
    Auto,
    Gles,
    /// The software renderer: slower, but the same pixels on every machine.
    Pixman,
    /// GLES on Mesa's software EGL device (llvmpipe), never a GPU: tests of
    /// what only GLES does, on a machine whose GPU must not be touched.
    GlesSoftware,
}

/// What the headless backend draws.
#[derive(Debug, Clone, Default)]
pub struct HeadlessConfig {
    /// One 1920×1080 output when empty.
    pub outputs: Vec<OutputSpec>,
    pub renderer: RendererChoice,
    /// The GPU GLES draws on, by its render node; the first GPU with one
    /// when unset. A machine with two GPUs lists the one it does not
    /// display on first as often as not.
    pub render_node: Option<PathBuf>,
}

pub struct HeadlessBackend {
    canvas: Canvas,
    /// The frame to come, once something changed.
    frame_timer: Option<RegistrationToken>,
    /// Screen sharing's frames (5.11), with the renderer drawing.
    gles_share: Box<crate::share::frames::Frames<GlesRenderer, GlesTexture>>,
    pixman_share:
        Box<crate::share::frames::Frames<PixmanRenderer, pixman::Image<'static, 'static>>>,
    /// The night light's temperature the outputs were last handed, and
    /// its gamma ramps (#436): headless has no gamma to set, the tests
    /// read what a screen would get.
    night: Option<u32>,
    gamma: Option<Box<[Vec<u16>; 3]>>,
}

/// The gamma ramps' size headless, a common one of display engines.
pub const GAMMA_SIZE: usize = 256;

impl std::fmt::Debug for HeadlessBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessBackend")
            .field("renderer", &self.renderer())
            .field("frames", &self.frames())
            .finish()
    }
}

enum Canvas {
    Gles(Box<Outputs<GlesRenderer, GlesTexture>>),
    Pixman(Outputs<PixmanRenderer, pixman::Image<'static, 'static>>),
}

/// A renderer and the buffers it draws the outputs into.
struct Outputs<R: Magnify, T> {
    renderer: R,
    format: Fourcc,
    /// In their names' order.
    outputs: Vec<OutputBuffer<T, R::Canvas>>,
    /// Outputs unplugged, by name, with where they were: a test plugs
    /// them back.
    unplugged: Vec<Unplugged>,
}

/// An output unplugged: its name, what it was, where, and who.
type Unplugged = (
    String,
    OutputSpec,
    (i32, i32),
    Option<crate::screen_sets::Identity>,
);

struct OutputBuffer<T, C> {
    output: Output,
    /// Its picture while the magnifier is on it (#424).
    lens: Lens<C>,
    spec: OutputSpec,
    /// Who the screen is, as its EDID would say (tests give one).
    identity: Option<crate::screen_sets::Identity>,
    position: (i32, i32),
    global: GlobalId,
    target: T,
    damage_tracker: OutputDamageTracker,
    /// Frames that drew something.
    frames: u64,
    /// What those frames drew over, together, since a test last asked
    /// (pixels of the output).
    damaged: Option<Rectangle<i32, Physical>>,
}

/// Creates the renderer and the outputs, and maps them in the space.
pub fn init(
    config: &HeadlessConfig,
    _handle: &LoopHandle<'static, App>,
    state: &mut State,
) -> anyhow::Result<HeadlessBackend> {
    let specs = if config.outputs.is_empty() {
        vec![OutputSpec::default()]
    } else {
        config.outputs.clone()
    };
    let canvas = match config.renderer {
        RendererChoice::Pixman => Canvas::Pixman(pixman_outputs(&specs, state)?),
        RendererChoice::Gles => Canvas::Gles(Box::new(gles_outputs(
            &specs,
            config.render_node.as_deref(),
            state,
        )?)),
        RendererChoice::GlesSoftware => Canvas::Gles(Box::new(software_outputs(&specs, state)?)),
        RendererChoice::Auto => match gles_outputs(&specs, config.render_node.as_deref(), state) {
            Ok(gles) => Canvas::Gles(Box::new(gles)),
            Err(err) => {
                tracing::info!("no GLES renderer ({err:#}); drawing with pixman");
                Canvas::Pixman(pixman_outputs(&specs, state)?)
            }
        },
    };
    let backend = HeadlessBackend {
        canvas,
        frame_timer: None,
        gles_share: Default::default(),
        pixman_share: Default::default(),
        night: None,
        gamma: None,
    };
    tracing::info!(renderer = backend.renderer(), "headless backend ready");
    Ok(backend)
}

fn pixman_outputs(
    specs: &[OutputSpec],
    state: &mut State,
) -> anyhow::Result<Outputs<PixmanRenderer, pixman::Image<'static, 'static>>> {
    let renderer = PixmanRenderer::new().context("could not create the pixman renderer")?;
    state
        .protocols
        .shm_state
        .update_formats(renderer.shm_formats());
    dmabuf_global(state, renderer.dmabuf_formats(), None);
    Outputs::new(renderer, Fourcc::Argb8888, specs, state)
}

/// GLES on the first hardware render node; software EGL devices are left
/// to pixman, which is faster on a CPU and the same everywhere.
fn gles_outputs(
    specs: &[OutputSpec],
    render_node: Option<&std::path::Path>,
    state: &mut State,
) -> anyhow::Result<Outputs<GlesRenderer, GlesTexture>> {
    let renderer = gles_renderer(render_node)?;
    state
        .protocols
        .shm_state
        .update_formats(renderer.shm_formats());
    let device = render_node_of(renderer.egl_context().display());
    dmabuf_global(state, renderer.dmabuf_formats(), device);
    state.graphics.blur = true;
    Outputs::new(renderer, Fourcc::Abgr8888, specs, state)
}

/// GLES on Mesa's software EGL device: no render node is opened.
fn software_outputs(
    specs: &[OutputSpec],
    state: &mut State,
) -> anyhow::Result<Outputs<GlesRenderer, GlesTexture>> {
    anyhow::ensure!(egl_available(), "libEGL.so.1 is not installed");
    let device = EGLDevice::enumerate()
        .context("could not list EGL devices")?
        .find(EGLDevice::is_software)
        .context("no software EGL device")?;
    // SAFETY: nothing else in the process creates EGL displays, so smithay's
    // tracking of them is accurate.
    #[allow(unsafe_code)]
    let display = unsafe { EGLDisplay::new(device) }.context("could not open the EGL display")?;
    let context = EGLContext::new(&display).context("could not create an EGL context")?;
    // SAFETY: the context was just created and is used on this thread only.
    #[allow(unsafe_code)]
    let renderer = unsafe { GlesRenderer::new(context) }.context("could not start GLES")?;
    state
        .protocols
        .shm_state
        .update_formats(renderer.shm_formats());
    dmabuf_global(state, renderer.dmabuf_formats(), None);
    // Its shaders, the glass's and the halo's among them, as on a GPU.
    state.graphics.blur = true;
    Outputs::new(renderer, Fourcc::Abgr8888, specs, state)
}

/// Whether Mesa's software EGL device is there, for
/// [`RendererChoice::GlesSoftware`]; listing the devices opens none.
pub fn software_gles() -> bool {
    egl_available()
        && EGLDevice::enumerate().is_ok_and(|mut devices| devices.any(|d| d.is_software()))
}

/// A GLES renderer on the GPU of `render_node`, or on the first GPU with a
/// render node.
fn gles_renderer(render_node: Option<&std::path::Path>) -> anyhow::Result<GlesRenderer> {
    anyhow::ensure!(egl_available(), "libEGL.so.1 is not installed");
    let wanted = render_node
        .map(|path| {
            DrmNode::from_path(path)
                .with_context(|| format!("{} is not a DRM device", path.display()))
                .map(|node| node.dev_id())
        })
        .transpose()?;
    let device = EGLDevice::enumerate()
        .context("could not list EGL devices")?
        .find(|device| {
            let node = device.try_get_render_node().ok().flatten();
            !device.is_software()
                && node.is_some_and(|node| wanted.is_none_or(|wanted| node.dev_id() == wanted))
        })
        .with_context(|| match render_node {
            Some(path) => format!("no EGL device on {}", path.display()),
            None => "no EGL device with a render node".to_owned(),
        })?;
    // SAFETY: nothing else in the process creates EGL displays, so smithay's
    // tracking of them is accurate.
    #[allow(unsafe_code)]
    let display = unsafe { EGLDisplay::new(device) }.context("could not open the EGL display")?;
    let context = EGLContext::new(&display).context("could not create an EGL context")?;
    // SAFETY: the context was just created and is used on this thread only.
    #[allow(unsafe_code)]
    let renderer = unsafe { GlesRenderer::new(context) }.context("could not start GLES")?;
    Ok(renderer)
}

/// smithay panics when it cannot load libEGL: ask the loader first.
fn egl_available() -> bool {
    // SAFETY: loading libEGL runs its initialisers, the same ones smithay
    // runs when it loads the library right after; nothing else loads
    // libraries concurrently while the compositor starts.
    #[allow(unsafe_code)]
    let library = unsafe { libloading::Library::new("libEGL.so.1") };
    library.is_ok()
}

impl<R, T> Outputs<R, T>
where
    R: Renderer
        + ImportAll
        + ImportMem
        + ExportMem
        + Offscreen<T>
        + Bind<T>
        + Magnify
        + crate::wobble::Bend
        + crate::going::Shaders
        + crate::glass::Glass,
    R::TextureId: Send + Clone + Texture + 'static,
    R::TextureMapping: 'static,
    R::Error: Send + Sync + 'static,
{
    fn new(
        renderer: R,
        format: Fourcc,
        specs: &[OutputSpec],
        state: &mut State,
    ) -> anyhow::Result<Self> {
        // Tests put the pointer where they want, from 0, 0.
        state.pointer.placed = true;
        let mut outputs = Self {
            renderer,
            format,
            outputs: Vec::with_capacity(specs.len()),
            unplugged: Vec::new(),
        };
        for (i, (spec, position)) in specs.iter().zip(output_spec::layout(specs)).enumerate() {
            outputs.add(format!("HEADLESS-{}", i + 1), *spec, position, None, state)?;
        }
        Ok(outputs)
    }

    /// Makes output `name` and maps it in the space at `position`.
    fn add(
        &mut self,
        name: String,
        spec: OutputSpec,
        position: (i32, i32),
        identity: Option<crate::screen_sets::Identity>,
        state: &mut State,
    ) -> anyhow::Result<()> {
        let target = self
            .renderer
            .create_buffer(self.format, (spec.width, spec.height).into())
            .with_context(|| format!("could not create a {}x{} buffer", spec.width, spec.height))?;
        let output = Output::new(
            name.clone(),
            PhysicalProperties {
                size: spec.physical_size.unwrap_or((0, 0)).into(),
                subpixel: Subpixel::Unknown,
                make: "Hocuspanda".into(),
                model: "Headless".into(),
            },
        );
        if let Some(identity) = &identity {
            let identity = identity.clone();
            output.user_data().insert_if_missing(|| identity);
        }
        let global = output.create_global::<State>(&state.display_handle);
        let mode = Mode {
            size: (spec.width, spec.height).into(),
            refresh: REFRESH_MHZ,
        };
        output.change_current_state(
            Some(mode),
            Some(Transform::Normal),
            Some(scale(spec.scale)),
            Some(position.into()),
        );
        output.set_preferred(mode);
        // What the Settings offer of it (6.9): its size, and the usual
        // ones under it, at 60 Hz.
        output
            .user_data()
            .insert_if_missing(|| crate::displays::Facts {
                modes: crate::displays::headless_modes(spec.width, spec.height, REFRESH_MHZ),
                preferred: (spec.width, spec.height, REFRESH_MHZ),
                vrr_capable: false,
                millimetres: spec.physical_size.unwrap_or((0, 0)),
                fixed_scale: Some(spec.scale),
                placed: position,
                placed_size: crate::displays::logical_size(&output).unwrap_or((1, 1)),
            });
        add_output(state, &output, position);
        tracing::info!(
            output = name,
            size = %format!("{}x{}", spec.width, spec.height),
            scale = spec.scale,
            ?position,
            "output"
        );
        self.outputs.push(OutputBuffer {
            lens: Lens::default(),
            damage_tracker: OutputDamageTracker::from_output(&output),
            output,
            spec,
            identity,
            position,
            global,
            target,
            frames: 0,
            damaged: None,
        });
        self.outputs.sort_by_key(|o| o.output.name());
        Ok(())
    }

    /// Output `output` drawn in `mode` from now on: a buffer of its size.
    fn set_mode(&mut self, output: &Output, mode: crate::displays::Mode3) -> anyhow::Result<()> {
        let out = self
            .outputs
            .iter_mut()
            .find(|o| o.output == *output)
            .with_context(|| format!("no output {}", output.name()))?;
        let target = self
            .renderer
            .create_buffer(self.format, (mode.0, mode.1).into())
            .with_context(|| format!("could not create a {}x{} buffer", mode.0, mode.1))?;
        out.target = target;
        output.change_current_state(Some(crate::displays::mode_of(mode)), None, None, None);
        out.damage_tracker = OutputDamageTracker::from_output(output);
        Ok(())
    }

    /// Output `name` goes, as a screen unplugged.
    fn unplug(&mut self, name: &str, state: &mut State) -> anyhow::Result<()> {
        let at = self
            .outputs
            .iter()
            .position(|o| o.output.name() == name)
            .with_context(|| format!("no output {name}"))?;
        let gone = self.outputs.remove(at);
        tracing::info!(output = name, "output unplugged");
        state.unplug_output(&gone.output, gone.global);
        self.unplugged
            .push((name.to_string(), gone.spec, gone.position, gone.identity));
        Ok(())
    }

    /// Output `name`, unplugged before, comes back where it was or at
    /// `at`; `screen`: another screen on that connector, or a new
    /// connector, and who it is.
    fn plug(
        &mut self,
        name: &str,
        at: Option<(i32, i32)>,
        screen: Option<(OutputSpec, Option<crate::screen_sets::Identity>)>,
        state: &mut State,
    ) -> anyhow::Result<()> {
        let index = self.unplugged.iter().position(|(n, ..)| n == name);
        let (name, spec, position, identity) = match (index, screen) {
            (Some(index), None) => self.unplugged.remove(index),
            (Some(index), Some((spec, identity))) => {
                let (name, _, position, _) = self.unplugged.remove(index);
                (name, spec, position, identity)
            }
            (None, Some((spec, identity))) => {
                // A connector new to the desktop: right of the others.
                let right = state
                    .space
                    .outputs()
                    .filter_map(|o| state.space.output_geometry(o))
                    .map(|g| g.loc.x + g.size.w)
                    .max()
                    .unwrap_or(0);
                (name.to_owned(), spec, (right, 0), identity)
            }
            (None, None) => anyhow::bail!("no output {name} unplugged"),
        };
        self.add(name.clone(), spec, at.unwrap_or(position), identity, state)?;
        let output = self
            .outputs
            .iter()
            .find(|o| o.output.name() == name)
            .map(|o| o.output.clone())
            .context("the output just made")?;
        state.output_back(&output, true);
        Ok(())
    }

    /// Draws every output where something changed, and tells the clients.
    fn render(&mut self, state: &mut State) {
        let Self {
            renderer, outputs, ..
        } = self;
        for out in outputs {
            // Turned off by the file (6.9): nothing drawn.
            if state.display_off(&out.output) {
                continue;
            }
            let started = std::time::Instant::now();
            let shown = magnifier::screen(renderer, state, &out.output, true, &mut out.lens);
            let elements = &shown.elements;
            let mut framebuffer = match renderer.bind(&mut out.target) {
                Ok(framebuffer) => framebuffer,
                Err(err) => {
                    tracing::warn!(output = out.output.name(), %err, "could not bind");
                    magnifier::after(renderer, &shown);
                    continue;
                }
            };
            // The buffer keeps its pixels between frames: an age of 1
            // redraws only what changed.
            let result = match out.damage_tracker.render_output(
                renderer,
                &mut framebuffer,
                1,
                elements,
                BACKGROUND,
            ) {
                Ok(result) => result,
                Err(err) => {
                    tracing::warn!(output = out.output.name(), ?err, "rendering failed");
                    magnifier::after(renderer, &shown);
                    continue;
                }
            };
            let drawn = result.damage.is_some_and(|d| !d.is_empty());
            if let Some(damage) = result.damage {
                for rect in damage {
                    out.damaged = Some(out.damaged.map_or(*rect, |d| d.merge(*rect)));
                }
            }
            let states = result.states;
            magnifier::after(renderer, &shown);
            if drawn {
                out.frames += 1;
                state.presented(&out.output, state.clock.now().into(), out.frames);
            }
            state.send_frames(&out.output, Some(shown.states.as_ref().unwrap_or(&states)));
            state.perf_scene(&out.output, started.elapsed());
            state.desktops_drawn(&out.output);
            state.read_under_grips(renderer);
        }
    }
}

impl<R, T> Outputs<R, T>
where
    R: ExportMem + Bind<T> + Magnify,
    R::Error: Send + Sync + 'static,
{
    /// What output `index` showed last: its size in pixels, and its rows of
    /// ARGB8888 pixels.
    fn pixels(&mut self, index: usize) -> anyhow::Result<(i32, i32, Vec<u8>)> {
        let Self {
            renderer, outputs, ..
        } = self;
        let out = outputs.get_mut(index).context("no such output")?;
        let size = out
            .output
            .current_mode()
            .context("an output without a mode")?
            .size;
        let framebuffer = renderer.bind(&mut out.target).context("could not bind")?;
        let region = Rectangle::from_size((size.w, size.h).into());
        let mapping = renderer
            .copy_framebuffer(&framebuffer, region, Fourcc::Argb8888)
            .context("could not read the pixels")?;
        let bytes = renderer
            .map_texture(&mapping)
            .context("could not map the pixels")?;
        Ok((size.w, size.h, bytes.to_vec()))
    }
}

impl HeadlessBackend {
    /// What output `index` (in the order given, of those plugged) showed
    /// last: its size in pixels, and its rows of ARGB8888 pixels
    /// (little-endian: B, G, R, A).
    pub fn screenshot(&mut self, index: usize) -> anyhow::Result<(i32, i32, Vec<u8>)> {
        match &mut self.canvas {
            Canvas::Gles(c) => c.pixels(index),
            Canvas::Pixman(c) => c.pixels(index),
        }
    }

    /// The gamma ramps the outputs were handed last (red, green, blue):
    /// `None` for their own.
    pub fn gamma(&self) -> Option<&[Vec<u16>; 3]> {
        self.gamma.as_deref()
    }

    pub fn renderer(&self) -> &'static str {
        match self.canvas {
            Canvas::Gles(_) => "gles",
            Canvas::Pixman(_) => "pixman",
        }
    }

    /// What output `index` drew over since the last call, as one
    /// rectangle of its pixels; `None`: nothing.
    pub fn take_damage(&mut self, index: usize) -> Option<Rectangle<i32, Physical>> {
        match &mut self.canvas {
            Canvas::Gles(c) => c.outputs.get_mut(index)?.damaged.take(),
            Canvas::Pixman(c) => c.outputs.get_mut(index)?.damaged.take(),
        }
    }

    /// Frames drawn on output `index` (in the order given).
    pub fn frames_on(&self, index: usize) -> u64 {
        match &self.canvas {
            Canvas::Gles(c) => c.outputs.get(index).map_or(0, |o| o.frames),
            Canvas::Pixman(c) => c.outputs.get(index).map_or(0, |o| o.frames),
        }
    }

    /// Frames drawn, all outputs together.
    pub fn frames(&self) -> u64 {
        match &self.canvas {
            Canvas::Gles(c) => c.outputs.iter().map(|o| o.frames).sum(),
            Canvas::Pixman(c) => c.outputs.iter().map(|o| o.frames).sum(),
        }
    }

    fn frame(&mut self, state: &mut State) {
        state.redraw = false;
        match &mut self.canvas {
            Canvas::Gles(c) => c.render(state),
            Canvas::Pixman(c) => c.render(state),
        }
    }

    /// Output `output` drawn in `mode` (6.9).
    pub fn set_mode(
        &mut self,
        state: &mut State,
        output: &Output,
        mode: crate::displays::Mode3,
    ) -> anyhow::Result<()> {
        let done = match &mut self.canvas {
            Canvas::Gles(c) => c.set_mode(output, mode),
            Canvas::Pixman(c) => c.set_mode(output, mode),
        };
        state.redraw = true;
        done
    }

    /// Output `name` goes, as a screen unplugged; its windows go to a
    /// screen still there (docs/design/windows.md §12).
    pub fn unplug(&mut self, name: &str, state: &mut State) -> anyhow::Result<()> {
        match &mut self.canvas {
            Canvas::Gles(c) => c.unplug(name, state),
            Canvas::Pixman(c) => c.unplug(name, state),
        }
    }

    /// Output `name`, unplugged before, comes back where it was (or at
    /// `at`), and the windows that came from it go back.
    pub fn plug(
        &mut self,
        name: &str,
        at: Option<(i32, i32)>,
        screen: Option<(OutputSpec, Option<crate::screen_sets::Identity>)>,
        state: &mut State,
    ) -> anyhow::Result<()> {
        match &mut self.canvas {
            Canvas::Gles(c) => c.plug(name, at, screen, state),
            Canvas::Pixman(c) => c.plug(name, at, screen, state),
        }
    }

    /// Schedules the next frame when something changed: nothing wakes up
    /// at rest.
    pub fn after_dispatch(&mut self, state: &mut State) {
        // The night light's ramps (#436), as a screen would get them.
        let night = state.night_light_kelvin();
        if night != self.night {
            self.night = night;
            self.gamma = night.map(|k| Box::new(crate::night_light::ramp(GAMMA_SIZE, k)));
        }
        // The desktop's last frame as it locks (7.1), before anything else
        // draws it locked.
        if state.lock.wants_freeze() {
            match &mut self.canvas {
                Canvas::Gles(c) => {
                    crate::lock::draw::freeze::<_, GlesTexture>(&mut c.renderer, state);
                }
                Canvas::Pixman(c) => {
                    crate::lock::draw::freeze::<_, pixman::Image<'static, 'static>>(
                        &mut c.renderer,
                        state,
                    )
                }
            }
        }
        // Screen sharing's thumbnails and frames (5.11).
        match &mut self.canvas {
            Canvas::Gles(c) => crate::share::frames::run::<_, GlesTexture>(
                &mut c.renderer,
                state,
                &mut self.gles_share,
            ),
            Canvas::Pixman(c) => crate::share::frames::run::<_, pixman::Image<'static, 'static>>(
                &mut c.renderer,
                state,
                &mut self.pixman_share,
            ),
        }
        // A capture asked for (5.8): drawn for it before the next frame.
        if state.capture.has_pending() {
            match &mut self.canvas {
                Canvas::Gles(c) => {
                    crate::capture::shoot::take::<_, GlesTexture>(&mut c.renderer, state);
                }
                Canvas::Pixman(c) => crate::capture::shoot::take::<
                    _,
                    pixman::Image<'static, 'static>,
                >(&mut c.renderer, state),
            }
        }
        if !state.redraw || self.frame_timer.is_some() {
            return;
        }
        let timer = state
            .loop_handle
            .insert_source(Timer::from_duration(FRAME), |_, _, app| {
                let App { state, backend } = app;
                if let Backend::Headless(headless) = backend {
                    headless.frame_timer = None;
                    headless.frame(state);
                }
                TimeoutAction::Drop
            });
        match timer {
            Ok(token) => self.frame_timer = Some(token),
            Err(err) => tracing::warn!(%err.error, "could not schedule a frame"),
        }
    }
}
