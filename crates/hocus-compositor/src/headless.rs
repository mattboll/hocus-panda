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
            Bind, ExportMem, ImportAll, ImportMem, Offscreen, Renderer, Texture,
            damage::OutputDamageTracker,
            element::RenderElementStates,
            gles::{GlesRenderer, GlesTexture},
            pixman::PixmanRenderer,
        },
    },
    desktop::Window,
    output::{Mode, Output, PhysicalProperties, Scale, Subpixel},
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

use hocus_api::Event;
use hocus_core::OutputName;

use crate::{
    App, BACKGROUND, State,
    capture::{self, Pixels},
    flip::{Flip, Health},
    output_spec::{self, OutputSpec},
    outputs::{Head, Setting},
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
    /// Physical pixels the last frame drawn repainted.
    pub damage: u64,
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
    format: Fourcc,
    outputs: Vec<CanvasOutput<T>>,
}

struct CanvasOutput<T> {
    output: Output,
    /// `None` while the output is off.
    global: Option<GlobalId>,
    /// Dark through output-power: in the layout, nothing drawn.
    dark: bool,
    spec: OutputSpec,
    position: (i32, i32),
    target: T,
    damage_tracker: OutputDamageTracker,
    frames: u64,
    /// Physical pixels the last frame drawn repainted.
    damage: u64,
    /// The frame drawn and not presented yet: presentation is immediate,
    /// unless the output is stalled.
    flip: Flip,
    /// Simulates a stuck output: frames are never presented.
    stalled: bool,
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
    Canvas::new(gles_renderer()?, Fourcc::Abgr8888, specs, state)
}

/// A GLES renderer on the first GPU with a render node.
pub(crate) fn gles_renderer() -> anyhow::Result<GlesRenderer> {
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

impl<R, T> Canvas<R, T>
where
    R: Renderer + ImportAll + ImportMem + Offscreen<T>,
    R::TextureId: Send + Clone + Texture + 'static,
    R::Error: Send + Sync + 'static,
{
    fn new(
        renderer: R,
        format: Fourcc,
        specs: &[OutputSpec],
        state: &mut State,
    ) -> anyhow::Result<Self> {
        let mut canvas = Self {
            renderer,
            format,
            outputs: Vec::with_capacity(specs.len()),
        };
        for (i, (spec, position)) in specs.iter().zip(output_spec::layout(specs)).enumerate() {
            canvas.add_output(&format!("HEADLESS-{}", i + 1), spec, position, state)?;
        }
        Ok(canvas)
    }

    fn add_output(
        &mut self,
        name: &str,
        spec: &OutputSpec,
        position: (i32, i32),
        state: &mut State,
    ) -> anyhow::Result<()> {
        let target = self
            .renderer
            .create_buffer(self.format, (spec.width, spec.height).into())
            .with_context(|| format!("could not create a {}x{} buffer", spec.width, spec.height))?;
        let (output, global) = create_output(name, spec, position, state);
        self.outputs.push(CanvasOutput {
            damage_tracker: OutputDamageTracker::from_output(&output),
            output,
            global: Some(global),
            dark: false,
            spec: *spec,
            position,
            target,
            frames: 0,
            damage: 0,
            flip: Flip::Idle,
            stalled: false,
        });
        Ok(())
    }

    fn remove_output(&mut self, name: &str, state: &mut State) -> bool {
        let Some(index) = self.outputs.iter().position(|o| o.output.name() == name) else {
            return false;
        };
        let out = self.outputs.remove(index);
        if let Some(global) = out.global {
            state.unplug_output(&out.output);
            state.retire_global(global);
        }
        true
    }

    /// Draws every output that has damage and sends frame callbacks.
    fn render(&mut self, state: &mut State) {
        let now = state.clock.now();
        let Self {
            renderer, outputs, ..
        } = self;
        for out in outputs {
            // Off, dark, or a frame still out (as on a screen): nothing is
            // drawn there.
            if out.global.is_none() || out.dark || out.flip.is_queued() {
                continue;
            }
            let start = Instant::now();
            let Some((states, drawn)) = render_one(renderer, out, state) else {
                continue;
            };
            if drawn {
                state.measures.frame(&out.output.name(), start.elapsed());
                out.flip.queued(now);
                if out.stalled {
                    continue;
                }
                out.flip.presented();
            }
            state.send_frames(&out.output, now, Some(&states));
            if drawn {
                state.presented(&out.output, now, out.frames);
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
    R: Renderer + ImportAll + ImportMem + Bind<T>,
    R::TextureId: Send + Clone + Texture + 'static,
{
    let elements = state.scene_elements(renderer, &out.output, true);
    let mut framebuffer = renderer
        .bind(&mut out.target)
        .inspect_err(|err| tracing::warn!(output = out.output.name(), %err, "could not bind"))
        .ok()?;
    // The buffer keeps its pixels between frames: an age of 1 redraws only
    // what changed.
    let result = out
        .damage_tracker
        .render_output(renderer, &mut framebuffer, 1, &elements, BACKGROUND)
        .inspect_err(|err| tracing::warn!(output = out.output.name(), ?err, "rendering failed"))
        .ok()?;
    let damage = result.damage.map_or(0, |damage| area(damage));
    let drawn = damage > 0;
    if drawn {
        out.frames += 1;
        out.damage = damage;
    }
    Some((result.states, drawn))
}

/// The pixels `rects` cover, each counted once.
fn area(rects: &[Rectangle<i32, Physical>]) -> u64 {
    let mut seen: Vec<Rectangle<i32, Physical>> = Vec::with_capacity(rects.len());
    let mut total = 0;
    for rect in rects {
        total += Rectangle::subtract_rects_many([*rect], seen.iter().copied())
            .iter()
            .map(|r| r.size.w.max(0) as u64 * r.size.h.max(0) as u64)
            .sum::<u64>();
        seen.push(*rect);
    }
    total
}

fn create_output(
    name: &str,
    spec: &OutputSpec,
    position: (i32, i32),
    state: &mut State,
) -> (Output, GlobalId) {
    let output = Output::new(
        name.to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
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
    state.remember_places();
    state.space.map_output(&output, position);
    state.plug_output(&output, spec.desktops);
    state.emit(|s| Event::OutputAdded {
        output: s.output_info(&output),
    });
    (output, global)
}

/// Sizes a headless output offers besides its own, all at 60 Hz.
const MODES: [(i32, i32); 7] = [
    (3840, 2160),
    (2560, 1600),
    (2560, 1440),
    (1920, 1200),
    (1920, 1080),
    (1280, 800),
    (1280, 720),
];

impl<R, T> Canvas<R, T>
where
    R: Renderer + ImportAll + ImportMem + Offscreen<T>,
    R::TextureId: Send + Clone + Texture + 'static,
    R::Error: Send + Sync + 'static,
{
    fn heads(&self, state: &State) -> Vec<Head> {
        self.outputs
            .iter()
            .map(|o| {
                let own = (o.spec.width, o.spec.height);
                let modes = std::iter::once(own)
                    .chain(MODES.into_iter().filter(|m| *m != own))
                    .map(|(width, height)| hocus_core::outputs::Mode {
                        width,
                        height,
                        refresh: REFRESH_MHZ,
                    })
                    .collect();
                let position = state
                    .space
                    .output_geometry(&o.output)
                    .map_or(o.position, |g| (g.loc.x, g.loc.y));
                Head {
                    name: o.output.name(),
                    make: "Hocuspanda".into(),
                    model: "Headless".into(),
                    physical_size: (0, 0),
                    modes,
                    preferred: Some(0),
                    vrr_capable: false,
                    now: Setting::of(&o.output, position, o.global.is_some(), false),
                }
            })
            .collect()
    }

    fn apply(&mut self, name: &str, setting: &Setting, state: &mut State) -> anyhow::Result<()> {
        let Self {
            renderer,
            format,
            outputs,
        } = self;
        let out = outputs
            .iter_mut()
            .find(|o| o.output.name() == name)
            .with_context(|| format!("no output named {name}"))?;
        if !setting.enabled {
            if let Some(global) = out.global.take() {
                state.disable_output(&out.output);
                state.retire_global(global);
            }
            return Ok(());
        }
        let size = (setting.mode.width, setting.mode.height);
        if size != (out.spec.width, out.spec.height) {
            out.target = renderer
                .create_buffer(*format, size.into())
                .with_context(|| format!("could not create a {}x{} buffer", size.0, size.1))?;
            out.spec.width = size.0;
            out.spec.height = size.1;
        }
        if out.global.is_none() {
            out.global = Some(out.output.create_global::<State>(&state.display_handle));
            state.enable_output(&out.output, setting, out.spec.desktops);
        } else {
            state.set_output(&out.output, setting);
        }
        out.spec.scale = setting.scale;
        out.position = setting.position;
        out.damage_tracker = OutputDamageTracker::from_output(&out.output);
        Ok(())
    }
}

impl HeadlessBackend {
    /// Every output, on and off, and what it can do.
    pub fn heads(&self, state: &State) -> Vec<Head> {
        match &self.renderers {
            Renderers::Gles(canvas) => canvas.heads(state),
            Renderers::Pixman(canvas) => canvas.heads(state),
        }
    }

    /// Sets an output as asked: a mode is a buffer of that size.
    pub fn apply(
        &mut self,
        name: &str,
        setting: &Setting,
        state: &mut State,
    ) -> anyhow::Result<()> {
        match &mut self.renderers {
            Renderers::Gles(canvas) => canvas.apply(name, setting, state),
            Renderers::Pixman(canvas) => canvas.apply(name, setting, state),
        }
    }

    /// Turns an output's drawing off or on (output-power).
    pub fn power(&mut self, name: &str, on: bool, state: &mut State) -> anyhow::Result<()> {
        fn power<R, T>(canvas: &mut Canvas<R, T>, name: &str, on: bool) -> bool {
            let out = canvas.outputs.iter_mut().find(|o| o.output.name() == name);
            out.map(|o| o.dark = !on).is_some()
        }
        let found = match &mut self.renderers {
            Renderers::Gles(canvas) => power(canvas, name, on),
            Renderers::Pixman(canvas) => power(canvas, name, on),
        };
        anyhow::ensure!(found, "no output named {name}");
        state.needs_redraw = true;
        Ok(())
    }

    pub fn renderer(&self) -> RendererKind {
        match self.renderers {
            Renderers::Gles(_) => RendererKind::Gles,
            Renderers::Pixman(_) => RendererKind::Pixman,
        }
    }

    /// Plugs an output: named `name`, which brings back an output unplugged
    /// before, or the next free `HEADLESS-N`; placed where its spec says, or
    /// to the right of the others.
    pub fn plug(
        &mut self,
        spec: &OutputSpec,
        name: Option<String>,
        state: &mut State,
    ) -> anyhow::Result<String> {
        let taken: Vec<String> = self.outputs().into_iter().map(|o| o.name).collect();
        let name = match name {
            Some(name) if taken.contains(&name) => anyhow::bail!("{name} is already plugged"),
            Some(name) => name,
            None => (1..)
                .map(|n| format!("HEADLESS-{n}"))
                .find(|n| {
                    !taken.contains(n) && state.model.output(&OutputName(n.clone())).is_none()
                })
                .unwrap_or_default(),
        };
        let position = spec.position.unwrap_or_else(|| {
            let right = state
                .space
                .outputs()
                .filter_map(|o| state.space.output_geometry(o))
                .map(|g| g.loc.x + g.size.w)
                .max()
                .unwrap_or(0);
            (right, 0)
        });
        match &mut self.renderers {
            Renderers::Gles(canvas) => canvas.add_output(&name, spec, position, state)?,
            Renderers::Pixman(canvas) => canvas.add_output(&name, spec, position, state)?,
        }
        tracing::info!(output = name, ?position, "output plugged");
        Ok(name)
    }

    pub fn unplug(&mut self, name: &str, state: &mut State) -> anyhow::Result<()> {
        let removed = match &mut self.renderers {
            Renderers::Gles(canvas) => canvas.remove_output(name, state),
            Renderers::Pixman(canvas) => canvas.remove_output(name, state),
        };
        anyhow::ensure!(removed, "no output named {name}");
        tracing::info!(output = name, "output unplugged");
        Ok(())
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
                    damage: out.damage,
                })
                .collect()
        }
        match &self.renderers {
            Renderers::Gles(canvas) => infos(canvas),
            Renderers::Pixman(canvas) => infos(canvas),
        }
    }

    pub fn capture_output(
        &mut self,
        output: &Output,
        state: &State,
        cursor: bool,
    ) -> anyhow::Result<Pixels> {
        match &mut self.renderers {
            Renderers::Gles(canvas) => {
                capture::output::<_, GlesTexture>(&mut canvas.renderer, output, state, cursor)
            }
            Renderers::Pixman(canvas) => capture::output::<_, pixman::Image<'static, 'static>>(
                &mut canvas.renderer,
                output,
                state,
                cursor,
            ),
        }
    }

    /// What `output`'s buffer holds: every frame drawn so far, each over
    /// the last where it was damaged.
    pub fn shown(&mut self, output: &Output) -> anyhow::Result<Pixels> {
        fn read<R, T>(canvas: &mut Canvas<R, T>, output: &Output) -> anyhow::Result<Pixels>
        where
            R: Renderer + Bind<T> + ExportMem,
            R::Error: Send + Sync + 'static,
        {
            let out = canvas
                .outputs
                .iter_mut()
                .find(|o| o.output == *output)
                .with_context(|| format!("no output named {}", output.name()))?;
            let (width, height) = (out.spec.width, out.spec.height);
            let framebuffer = canvas.renderer.bind(&mut out.target)?;
            let mapping = canvas.renderer.copy_framebuffer(
                &framebuffer,
                Rectangle::from_size((width, height).into()),
                Fourcc::Abgr8888,
            )?;
            let rgba = canvas.renderer.map_texture(&mapping)?.to_vec();
            Ok(Pixels {
                width: width as u32,
                height: height as u32,
                rgba,
            })
        }
        match &mut self.renderers {
            Renderers::Gles(canvas) => read(canvas, output),
            Renderers::Pixman(canvas) => read(canvas, output),
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

    /// Stops presenting `name`'s frames until it is recovered.
    pub fn stall(&mut self, name: &str) -> anyhow::Result<()> {
        fn stall<R, T>(canvas: &mut Canvas<R, T>, name: &str) -> bool {
            let out = canvas.outputs.iter_mut().find(|o| o.output.name() == name);
            out.map(|o| o.stalled = true).is_some()
        }
        let found = match &mut self.renderers {
            Renderers::Gles(canvas) => stall(canvas, name),
            Renderers::Pixman(canvas) => stall(canvas, name),
        };
        anyhow::ensure!(found, "no output named {name}");
        tracing::info!(output = name, "output stalled");
        Ok(())
    }

    /// Looks at the outputs with a frame out, and recovers the stuck ones:
    /// for a headless output, presenting again from a full redraw.
    fn check_flips(&mut self, state: &mut State) {
        fn check<R, T>(canvas: &mut Canvas<R, T>, state: &mut State) {
            let now = state.clock.now();
            for out in &mut canvas.outputs {
                if let Health::Recover { missed } = out.flip.check(now) {
                    out.stalled = false;
                    out.damage_tracker = OutputDamageTracker::from_output(&out.output);
                    state.output_recovered(&out.output, missed);
                }
            }
        }
        match &mut self.renderers {
            Renderers::Gles(canvas) => check(canvas, state),
            Renderers::Pixman(canvas) => check(canvas, state),
        }
    }

    /// Draws a frame if something changed since the last one.
    pub fn frame(&mut self, state: &mut State) {
        self.check_flips(state);
        if !state.needs_redraw {
            return;
        }
        match &mut self.renderers {
            Renderers::Gles(canvas) => {
                // Wallpapers that move, the blur behind panels (over them)
                // and the Matrix theme's GPU effects, before the outputs
                // show them.
                for out in &canvas.outputs {
                    state.paint_backdrop(&mut canvas.renderer, &out.output);
                    state.paint_glass(&mut canvas.renderer, &out.output);
                    state.paint_crt(&mut canvas.renderer, &out.output);
                }
                canvas.render(state)
            }
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
