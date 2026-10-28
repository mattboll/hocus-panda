//! The headless backend: no screen, outputs are offscreen buffers, drawn
//! with GLES on a GPU's render node or with pixman. Tests run on it.

use std::time::Duration;

use anyhow::Context;
use smithay::{
    backend::{
        allocator::Fourcc,
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
    utils::{Rectangle, Transform},
};

use crate::{
    App, State,
    backend::{Backend, add_output, dmabuf_global, render_node_of, scale},
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
}

/// What the headless backend draws.
#[derive(Debug, Clone, Default)]
pub struct HeadlessConfig {
    /// One 1920×1080 output when empty.
    pub outputs: Vec<OutputSpec>,
    pub renderer: RendererChoice,
}

pub struct HeadlessBackend {
    canvas: Canvas,
    /// The frame to come, once something changed.
    frame_timer: Option<RegistrationToken>,
}

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
struct Outputs<R, T> {
    renderer: R,
    format: Fourcc,
    /// In their names' order.
    outputs: Vec<OutputBuffer<T>>,
    /// Outputs unplugged, by name, with where they were: a test plugs
    /// them back.
    unplugged: Vec<(String, OutputSpec, (i32, i32))>,
}

struct OutputBuffer<T> {
    output: Output,
    spec: OutputSpec,
    position: (i32, i32),
    global: GlobalId,
    target: T,
    damage_tracker: OutputDamageTracker,
    /// Frames that drew something.
    frames: u64,
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
        RendererChoice::Gles => Canvas::Gles(Box::new(gles_outputs(&specs, state)?)),
        RendererChoice::Auto => match gles_outputs(&specs, state) {
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
    };
    tracing::info!(renderer = backend.renderer(), "headless backend ready");
    Ok(backend)
}

fn pixman_outputs(
    specs: &[OutputSpec],
    state: &mut State,
) -> anyhow::Result<Outputs<PixmanRenderer, pixman::Image<'static, 'static>>> {
    let renderer = PixmanRenderer::new().context("could not create the pixman renderer")?;
    state.shm_state.update_formats(renderer.shm_formats());
    dmabuf_global(state, renderer.dmabuf_formats(), None);
    Outputs::new(renderer, Fourcc::Argb8888, specs, state)
}

/// GLES on the first hardware render node; software EGL devices are left
/// to pixman, which is faster on a CPU and the same everywhere.
fn gles_outputs(
    specs: &[OutputSpec],
    state: &mut State,
) -> anyhow::Result<Outputs<GlesRenderer, GlesTexture>> {
    let renderer = gles_renderer()?;
    state.shm_state.update_formats(renderer.shm_formats());
    let device = render_node_of(renderer.egl_context().display());
    dmabuf_global(state, renderer.dmabuf_formats(), device);
    Outputs::new(renderer, Fourcc::Abgr8888, specs, state)
}

/// A GLES renderer on the first GPU with a render node.
fn gles_renderer() -> anyhow::Result<GlesRenderer> {
    anyhow::ensure!(egl_available(), "libEGL.so.1 is not installed");
    let device = EGLDevice::enumerate()
        .context("could not list EGL devices")?
        .find(|device| {
            !device.is_software() && device.try_get_render_node().ok().flatten().is_some()
        })
        .context("no EGL device with a render node")?;
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
    R: Renderer + ImportAll + ImportMem + Offscreen<T> + Bind<T> + crate::wobble::Bend,
    R::TextureId: Send + Clone + Texture + 'static,
    R::Error: Send + Sync + 'static,
{
    fn new(
        renderer: R,
        format: Fourcc,
        specs: &[OutputSpec],
        state: &mut State,
    ) -> anyhow::Result<Self> {
        let mut outputs = Self {
            renderer,
            format,
            outputs: Vec::with_capacity(specs.len()),
            unplugged: Vec::new(),
        };
        for (i, (spec, position)) in specs.iter().zip(output_spec::layout(specs)).enumerate() {
            outputs.add(format!("HEADLESS-{}", i + 1), *spec, position, state)?;
        }
        Ok(outputs)
    }

    /// Makes output `name` and maps it in the space at `position`.
    fn add(
        &mut self,
        name: String,
        spec: OutputSpec,
        position: (i32, i32),
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
        add_output(state, &output, position);
        tracing::info!(
            output = name,
            size = %format!("{}x{}", spec.width, spec.height),
            scale = spec.scale,
            ?position,
            "output"
        );
        self.outputs.push(OutputBuffer {
            damage_tracker: OutputDamageTracker::from_output(&output),
            output,
            spec,
            position,
            global,
            target,
            frames: 0,
        });
        self.outputs.sort_by_key(|o| o.output.name());
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
        state.output_gone(&gone.output);
        state.display_handle.remove_global::<State>(gone.global);
        self.unplugged
            .push((name.to_string(), gone.spec, gone.position));
        Ok(())
    }

    /// Output `name`, unplugged before, comes back where it was or at
    /// `at`.
    fn plug(
        &mut self,
        name: &str,
        at: Option<(i32, i32)>,
        state: &mut State,
    ) -> anyhow::Result<()> {
        let index = self
            .unplugged
            .iter()
            .position(|(n, _, _)| n == name)
            .with_context(|| format!("no output {name} unplugged"))?;
        let (name, spec, position) = self.unplugged.remove(index);
        self.add(name.clone(), spec, at.unwrap_or(position), state)?;
        let output = self
            .outputs
            .iter()
            .find(|o| o.output.name() == name)
            .map(|o| o.output.clone())
            .context("the output just made")?;
        state.output_back(&output);
        Ok(())
    }

    /// Draws every output where something changed, and tells the clients.
    fn render(&mut self, state: &mut State) {
        let Self {
            renderer, outputs, ..
        } = self;
        for out in outputs {
            let elements = state.output_elements(renderer, &out.output, true);
            let mut framebuffer = match renderer.bind(&mut out.target) {
                Ok(framebuffer) => framebuffer,
                Err(err) => {
                    tracing::warn!(output = out.output.name(), %err, "could not bind");
                    continue;
                }
            };
            // The buffer keeps its pixels between frames: an age of 1
            // redraws only what changed.
            let result = match out.damage_tracker.render_output(
                renderer,
                &mut framebuffer,
                1,
                &elements,
                BACKGROUND,
            ) {
                Ok(result) => result,
                Err(err) => {
                    tracing::warn!(output = out.output.name(), ?err, "rendering failed");
                    continue;
                }
            };
            let drawn = result.damage.is_some_and(|d| !d.is_empty());
            let states = result.states;
            if drawn {
                out.frames += 1;
                state.presented(&out.output, state.clock.now().into(), out.frames);
            }
            state.send_frames(&out.output, Some(&states));
        }
    }
}

impl<R, T> Outputs<R, T>
where
    R: ExportMem + Bind<T>,
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

    pub fn renderer(&self) -> &'static str {
        match self.canvas {
            Canvas::Gles(_) => "gles",
            Canvas::Pixman(_) => "pixman",
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
        state: &mut State,
    ) -> anyhow::Result<()> {
        match &mut self.canvas {
            Canvas::Gles(c) => c.plug(name, at, state),
            Canvas::Pixman(c) => c.plug(name, at, state),
        }
    }

    /// Schedules the next frame when something changed: nothing wakes up
    /// at rest.
    pub fn after_dispatch(&mut self, state: &mut State) {
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
