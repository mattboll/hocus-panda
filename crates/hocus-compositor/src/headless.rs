//! The headless backend: no screen, outputs are offscreen buffers. Every
//! scenario test runs on it.

use std::{
    fmt,
    time::{Duration, Instant},
};

use anyhow::Context;
use smithay::{
    backend::{
        allocator::Fourcc,
        egl::{EGLContext, EGLDevice, EGLDisplay},
        renderer::{
            Bind, ImportAll, Offscreen, Renderer, Texture,
            damage::OutputDamageTracker,
            element::{
                RenderElementStates, default_primary_scanout_output_compare,
                surface::WaylandSurfaceRenderElement,
            },
            gles::{GlesRenderer, GlesTexture},
            pixman::PixmanRenderer,
        },
    },
    desktop::{
        Window,
        space::render_output,
        utils::{surface_primary_scanout_output, update_surface_primary_scanout_output},
    },
    output::{Mode, Output, PhysicalProperties, Scale, Subpixel},
    reexports::{
        calloop::{
            LoopHandle, RegistrationToken,
            timer::{TimeoutAction, Timer},
        },
        pixman,
    },
    utils::Transform,
};

use hocus_api::Event;

use crate::{
    App, BACKGROUND, State,
    capture::{self, Pixels},
    output_spec::{self, OutputSpec},
};

/// Headless outputs refresh at a nominal 60 Hz.
const REFRESH_MHZ: i32 = 60_000;
/// The time between two frames of a headless output.
pub const FRAME: Duration = Duration::from_nanos(1_000_000_000_000 / REFRESH_MHZ as u64);

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

/// The renderer that is drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererKind {
    Gles,
    Pixman,
}

/// What the headless backend draws.
#[derive(Debug, Clone, Default)]
pub struct HeadlessConfig {
    /// One 1920×1080 output when empty.
    pub outputs: Vec<OutputSpec>,
    pub renderer: RendererChoice,
}

/// What a test can know about a headless output.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputInfo {
    pub name: String,
    pub spec: OutputSpec,
    /// Logical position.
    pub position: (i32, i32),
    /// Frames that drew something.
    pub frames: u64,
}

pub struct HeadlessBackend {
    renderers: Renderers,
    /// The pending frame, when the clock is monotonic and something changed.
    frame_timer: Option<RegistrationToken>,
}

impl fmt::Debug for HeadlessBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeadlessBackend")
            .field("renderer", &self.renderer())
            .field("outputs", &self.outputs())
            .finish()
    }
}

enum Renderers {
    // A GLES renderer is large; there is one per compositor.
    Gles(Box<Canvas<GlesRenderer, GlesTexture>>),
    Pixman(Canvas<PixmanRenderer, pixman::Image<'static, 'static>>),
}

/// A renderer and the buffers it draws the outputs into.
struct Canvas<R, T> {
    renderer: R,
    outputs: Vec<CanvasOutput<T>>,
}

struct CanvasOutput<T> {
    output: Output,
    spec: OutputSpec,
    position: (i32, i32),
    target: T,
    damage_tracker: OutputDamageTracker,
    frames: u64,
}

/// Creates the renderer and the outputs, and maps them in the space.
pub fn init(config: &HeadlessConfig, state: &mut State) -> anyhow::Result<HeadlessBackend> {
    let specs = if config.outputs.is_empty() {
        vec![OutputSpec::default()]
    } else {
        config.outputs.clone()
    };
    let renderers = match config.renderer {
        RendererChoice::Pixman => Renderers::Pixman(pixman_offscreen(&specs, state)?),
        RendererChoice::Gles => Renderers::Gles(Box::new(gles_offscreen(&specs, state)?)),
        RendererChoice::Auto => match gles_offscreen(&specs, state) {
            Ok(gles) => Renderers::Gles(Box::new(gles)),
            Err(err) => {
                tracing::info!("no GLES renderer ({err:#}); using pixman");
                Renderers::Pixman(pixman_offscreen(&specs, state)?)
            }
        },
    };
    let backend = HeadlessBackend {
        renderers,
        frame_timer: None,
    };
    tracing::info!(renderer = ?backend.renderer(), "headless backend ready");
    for info in backend.outputs() {
        tracing::info!(
            output = info.name,
            size = %format!("{}x{}", info.spec.width, info.spec.height),
            scale = info.spec.scale,
            position = ?info.position,
            "output"
        );
    }
    Ok(backend)
}

fn pixman_offscreen(
    specs: &[OutputSpec],
    state: &mut State,
) -> anyhow::Result<Canvas<PixmanRenderer, pixman::Image<'static, 'static>>> {
    let renderer = PixmanRenderer::new().context("could not create the pixman renderer")?;
    Canvas::new(renderer, Fourcc::Argb8888, specs, state)
}

/// GLES on the first hardware render node; software EGL devices are left to
/// pixman, which is faster on a CPU and deterministic.
fn gles_offscreen(
    specs: &[OutputSpec],
    state: &mut State,
) -> anyhow::Result<Canvas<GlesRenderer, GlesTexture>> {
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
    Canvas::new(renderer, Fourcc::Abgr8888, specs, state)
}

impl<R, T> Canvas<R, T>
where
    R: Renderer + ImportAll + Offscreen<T>,
    R::TextureId: Clone + Texture + 'static,
    R::Error: Send + Sync + 'static,
{
    fn new(
        mut renderer: R,
        format: Fourcc,
        specs: &[OutputSpec],
        state: &mut State,
    ) -> anyhow::Result<Self> {
        let positions = output_spec::layout(specs);
        let mut outputs = Vec::with_capacity(specs.len());
        for (i, (spec, position)) in specs.iter().zip(positions).enumerate() {
            let target = renderer
                .create_buffer(format, (spec.width, spec.height).into())
                .with_context(|| {
                    format!("could not create a {}x{} buffer", spec.width, spec.height)
                })?;
            let output = create_output(&format!("HEADLESS-{}", i + 1), spec, position, state);
            outputs.push(CanvasOutput {
                damage_tracker: OutputDamageTracker::from_output(&output),
                output,
                spec: *spec,
                position,
                target,
                frames: 0,
            });
        }
        Ok(Self { renderer, outputs })
    }

    /// Draws every output that has damage and sends frame callbacks.
    fn render(&mut self, state: &mut State) {
        let now = state.clock.now();
        let Self { renderer, outputs } = self;
        for out in outputs {
            let start = Instant::now();
            let Some((states, drawn)) = render_one(renderer, out, state) else {
                continue;
            };
            if drawn {
                state.measures.frame(&out.output.name(), start.elapsed());
            }
            for window in state.space.elements() {
                window.with_surfaces(|surface, data| {
                    update_surface_primary_scanout_output(
                        surface,
                        &out.output,
                        data,
                        &states,
                        default_primary_scanout_output_compare,
                    );
                });
                window.send_frame(
                    &out.output,
                    now,
                    Some(Duration::ZERO),
                    surface_primary_scanout_output,
                );
            }
        }
    }
}

/// Draws one output, and says whether anything was drawn; `None` when
/// rendering failed.
fn render_one<R, T>(
    renderer: &mut R,
    out: &mut CanvasOutput<T>,
    state: &State,
) -> Option<(RenderElementStates, bool)>
where
    R: Renderer + ImportAll + Bind<T>,
    R::TextureId: Clone + Texture + 'static,
{
    let mut framebuffer = renderer
        .bind(&mut out.target)
        .inspect_err(|err| tracing::warn!(output = out.output.name(), %err, "could not bind"))
        .ok()?;
    // The buffer keeps its pixels between frames: an age of 1 redraws only
    // what changed.
    let result = render_output::<_, WaylandSurfaceRenderElement<R>, _, _>(
        &out.output,
        renderer,
        &mut framebuffer,
        1.0,
        1,
        [&state.space],
        &[],
        &mut out.damage_tracker,
        BACKGROUND,
    )
    .inspect_err(|err| tracing::warn!(output = out.output.name(), ?err, "rendering failed"))
    .ok()?;
    let drawn = result.damage.is_some_and(|damage| !damage.is_empty());
    if drawn {
        out.frames += 1;
    }
    Some((result.states, drawn))
}

fn create_output(name: &str, spec: &OutputSpec, position: (i32, i32), state: &mut State) -> Output {
    let output = Output::new(
        name.to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Hocuspanda".into(),
            model: "Headless".into(),
        },
    );
    let _global = output.create_global::<State>(&state.display_handle);
    let mode = Mode {
        size: (spec.width, spec.height).into(),
        refresh: REFRESH_MHZ,
    };
    let scale = if spec.scale.fract() == 0.0 {
        Scale::Integer(spec.scale as i32)
    } else {
        Scale::Fractional(spec.scale)
    };
    output.change_current_state(
        Some(mode),
        Some(Transform::Normal),
        Some(scale),
        Some(position.into()),
    );
    output.set_preferred(mode);
    state.space.map_output(&output, position);
    state.emit(|s| Event::OutputAdded {
        output: s.output_info(&output),
    });
    output
}

impl HeadlessBackend {
    pub fn renderer(&self) -> RendererKind {
        match self.renderers {
            Renderers::Gles(_) => RendererKind::Gles,
            Renderers::Pixman(_) => RendererKind::Pixman,
        }
    }

    pub fn outputs(&self) -> Vec<OutputInfo> {
        fn infos<R, T>(canvas: &Canvas<R, T>) -> Vec<OutputInfo> {
            canvas
                .outputs
                .iter()
                .map(|out| OutputInfo {
                    name: out.output.name(),
                    spec: out.spec,
                    position: out.position,
                    frames: out.frames,
                })
                .collect()
        }
        match &self.renderers {
            Renderers::Gles(canvas) => infos(canvas),
            Renderers::Pixman(canvas) => infos(canvas),
        }
    }

    pub fn capture_output(&mut self, output: &Output, state: &State) -> anyhow::Result<Pixels> {
        match &mut self.renderers {
            Renderers::Gles(canvas) => {
                capture::output::<_, GlesTexture>(&mut canvas.renderer, output, state)
            }
            Renderers::Pixman(canvas) => capture::output::<_, pixman::Image<'static, 'static>>(
                &mut canvas.renderer,
                output,
                state,
            ),
        }
    }

    pub fn capture_window(&mut self, window: &Window, scale: f64) -> anyhow::Result<Pixels> {
        match &mut self.renderers {
            Renderers::Gles(canvas) => {
                capture::window::<_, GlesTexture>(&mut canvas.renderer, window, scale)
            }
            Renderers::Pixman(canvas) => capture::window::<_, pixman::Image<'static, 'static>>(
                &mut canvas.renderer,
                window,
                scale,
            ),
        }
    }

    /// Draws a frame if something changed since the last one.
    pub fn frame(&mut self, state: &mut State) {
        if !state.needs_redraw {
            return;
        }
        match &mut self.renderers {
            Renderers::Gles(canvas) => canvas.render(state),
            Renderers::Pixman(canvas) => canvas.render(state),
        }
        state.space.refresh();
        state.popups.cleanup();
        state.needs_redraw = false;
    }

    /// On a monotonic clock, schedules the next frame when something changed:
    /// nothing wakes up at rest.
    pub fn after_dispatch(&mut self, handle: &LoopHandle<'static, App>, state: &State) {
        if !state.needs_redraw || self.frame_timer.is_some() {
            return;
        }
        let timer = handle.insert_source(Timer::from_duration(FRAME), |_, _, app| {
            app.timed("headless frame", |app| {
                if let crate::BackendState::Headless(headless) = &mut app.backend {
                    headless.frame_timer = None;
                    headless.frame(&mut app.state);
                }
            });
            TimeoutAction::Drop
        });
        match timer {
            Ok(token) => self.frame_timer = Some(token),
            Err(err) => tracing::warn!(%err.error, "could not schedule a frame"),
        }
    }
}
