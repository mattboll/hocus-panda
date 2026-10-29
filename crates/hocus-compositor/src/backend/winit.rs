//! The nested backend: the compositor runs in a window of the current
//! session, drawn with GLES. The host draws the cursor.

use anyhow::Context;
use smithay::{
    backend::{
        renderer::{ImportDma, ImportMemWl, damage::OutputDamageTracker, gles::GlesRenderer},
        winit::{self, WinitEvent, WinitGraphicsBackend},
    },
    input::pointer::CursorIcon,
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{calloop::LoopHandle, wayland_server::backend::GlobalId},
    utils::Transform,
};

use crate::{
    App, State,
    backend::{Backend, add_output, dmabuf_global, render_node_of, scale},
    cursor::Shown,
    render::BACKGROUND,
};

/// winit does not say the refresh rate; a nominal 60 Hz.
const REFRESH_MHZ: i32 = 60_000;

pub struct WinitBackend {
    backend: WinitGraphicsBackend<GlesRenderer>,
    output: Output,
    _global: GlobalId,
    damage_tracker: OutputDamageTracker,
    /// Frames presented, for presentation-time.
    presented: u64,
    /// The shape last asked of the host's cursor; none while hidden.
    host_cursor: Option<CursorIcon>,
}

impl std::fmt::Debug for WinitBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WinitBackend")
            .field("output", &self.output.name())
            .finish()
    }
}

/// Opens the window, creates its output and feeds its events to the loop.
pub fn init(handle: &LoopHandle<'static, App>, state: &mut State) -> anyhow::Result<WinitBackend> {
    let (mut backend, winit_loop) = winit::init::<GlesRenderer>()
        .map_err(|err| anyhow::anyhow!("{err}"))
        .context("could not open the window")?;

    let mode = Mode {
        size: backend.window_size(),
        refresh: REFRESH_MHZ,
    };
    let output = Output::new(
        "winit".to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Hocuspanda".into(),
            model: "Nested".into(),
        },
    );
    let global = output.create_global::<State>(&state.display_handle);
    // winit's GL surface is upside down compared to Wayland's.
    output.change_current_state(
        Some(mode),
        Some(Transform::Flipped180),
        Some(scale(backend.scale_factor())),
        Some((0, 0).into()),
    );
    output.set_preferred(mode);
    add_output(state, &output, (0, 0));

    let renderer = backend.renderer();
    state.shm_state.update_formats(renderer.shm_formats());
    let formats = renderer.dmabuf_formats();
    let device = render_node_of(renderer.egl_context().display());
    dmabuf_global(state, formats, device);
    state.blur = true;

    let damage_tracker = OutputDamageTracker::from_output(&output);
    handle
        .insert_source(winit_loop, |event, _, app| on_event(app, event))
        .map_err(|err| err.error)
        .context("could not watch the window's events")?;

    tracing::info!(size = ?mode.size, scale = backend.scale_factor(), "nested window open");
    Ok(WinitBackend {
        backend,
        output,
        _global: global,
        damage_tracker,
        presented: 0,
        host_cursor: Some(CursorIcon::Default),
    })
}

fn on_event(app: &mut App, event: WinitEvent) {
    let App { state, backend } = app;
    let Backend::Winit(winit) = backend else {
        return;
    };
    match event {
        WinitEvent::Resized { size, scale_factor } => {
            winit.output.change_current_state(
                Some(Mode {
                    size,
                    refresh: REFRESH_MHZ,
                }),
                None,
                Some(scale(scale_factor)),
                None,
            );
            // Bars and overlays follow the window's size.
            smithay::desktop::layer_map_for_output(&winit.output).arrange();
            state.rearrange();
            state.redraw = true;
        }
        WinitEvent::Input(event) => state.process_input_event(event),
        WinitEvent::Redraw => winit.render(state),
        WinitEvent::CloseRequested => {
            tracing::info!("window closed");
            state.loop_signal.stop();
        }
        WinitEvent::Focus(_) => {}
    }
}

impl WinitBackend {
    /// Asks for a frame only when something changed: nothing is drawn at
    /// rest.
    pub fn after_dispatch(&mut self, state: &mut State) {
        if state.redraw {
            self.backend.window().request_redraw();
        }
    }

    /// The host's cursor takes the shape the pointer shows: a client's own
    /// image is not passed on, the host's arrow stands for it.
    fn show_host_cursor(&mut self, state: &State) {
        let wanted = match state.shown_cursor() {
            Shown::Hidden => None,
            Shown::Surface(_) => Some(CursorIcon::Default),
            Shown::Named(icon) => Some(icon),
        };
        if wanted == self.host_cursor {
            return;
        }
        let window = self.backend.window();
        match wanted {
            Some(icon) => {
                window.set_cursor(icon);
                window.set_cursor_visible(true);
            }
            None => window.set_cursor_visible(false),
        }
        self.host_cursor = wanted;
    }

    fn render(&mut self, state: &mut State) {
        state.redraw = false;
        self.show_host_cursor(state);
        let age = self.backend.buffer_age().unwrap_or(0);
        let damage = {
            let elements = state.output_elements(self.backend.renderer(), &self.output, false);
            let (renderer, mut framebuffer) = match self.backend.bind() {
                Ok(bound) => bound,
                Err(err) => {
                    tracing::warn!(%err, "could not bind the window's surface");
                    return;
                }
            };
            match self.damage_tracker.render_output(
                renderer,
                &mut framebuffer,
                age,
                &elements,
                BACKGROUND,
            ) {
                Ok(result) => result.damage.cloned(),
                Err(err) => {
                    tracing::warn!(?err, "rendering failed");
                    return;
                }
            }
        };
        if let Some(damage) = damage {
            match self.backend.submit(Some(&damage)) {
                Ok(()) => {
                    self.presented += 1;
                    state.presented(&self.output, state.clock.now().into(), self.presented);
                }
                Err(err) => tracing::warn!(%err, "could not present the frame"),
            }
        }
        state.send_frames(&self.output, None);
        state.desktops_drawn(&self.output);
    }
}
