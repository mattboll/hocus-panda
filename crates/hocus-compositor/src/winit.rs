//! The nested backend: the compositor runs in a window of the current session.

use std::time::{Duration, Instant};

use anyhow::Context;
use smithay::{
    backend::{
        renderer::{
            damage::OutputDamageTracker,
            element::surface::WaylandSurfaceRenderElement,
            gles::{GlesRenderer, GlesTexture},
        },
        winit::{self, WinitEvent, WinitGraphicsBackend},
    },
    desktop::{Window, space::render_output},
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::LoopHandle,
    utils::Transform,
};

use hocus_api::Event;

use crate::{
    App, BACKGROUND, BackendState, State,
    capture::{self, Pixels},
    output_spec::DEFAULT_DESKTOPS,
};

/// winit does not report the refresh rate; a nominal 60 Hz.
const REFRESH_MHZ: i32 = 60_000;

pub struct WinitBackend {
    backend: WinitGraphicsBackend<GlesRenderer>,
    output: Output,
    damage_tracker: OutputDamageTracker,
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
    let (backend, winit_loop) = winit::init::<GlesRenderer>()
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
    let _global = output.create_global::<State>(&state.display_handle);
    // winit's GL surface is upside down compared to Wayland's.
    output.change_current_state(
        Some(mode),
        Some(Transform::Flipped180),
        None,
        Some((0, 0).into()),
    );
    output.set_preferred(mode);
    state.space.map_output(&output, (0, 0));
    state.plug_output(&output, DEFAULT_DESKTOPS);
    state.emit(|s| Event::OutputAdded {
        output: s.output_info(&output),
    });

    let damage_tracker = OutputDamageTracker::from_output(&output);

    handle
        .insert_source(winit_loop, |event, _, app| {
            app.timed("window events", |app| on_event(app, event));
        })
        .map_err(|err| err.error)
        .context("could not watch the window's events")?;

    Ok(WinitBackend {
        backend,
        output,
        damage_tracker,
    })
}

fn on_event(app: &mut App, event: WinitEvent) {
    match event {
        WinitEvent::Resized { size, .. } => {
            if let BackendState::Winit(winit) = &app.backend {
                winit.output.change_current_state(
                    Some(Mode {
                        size,
                        refresh: REFRESH_MHZ,
                    }),
                    None,
                    None,
                    None,
                );
                let output = winit.output.clone();
                app.state.output_moved(&output);
            }
            app.state.needs_redraw = true;
        }
        WinitEvent::Input(event) => app.state.process_input_event(event),
        WinitEvent::Redraw => {
            if let BackendState::Winit(winit) = &mut app.backend {
                winit.render(&mut app.state);
            }
        }
        WinitEvent::CloseRequested => {
            tracing::info!("window closed");
            app.state.loop_signal.stop();
        }
        WinitEvent::Focus(_) => {}
    }
}

impl WinitBackend {
    /// Asks for a frame only when something changed: nothing is drawn at rest.
    pub fn after_dispatch(&mut self, state: &mut State) {
        if state.needs_redraw {
            self.backend.window().request_redraw();
        }
    }

    pub fn capture_output(&mut self, output: &Output, state: &State) -> anyhow::Result<Pixels> {
        capture::output::<_, GlesTexture>(self.backend.renderer(), output, state)
    }

    pub fn capture_window(&mut self, window: &Window, scale: f64) -> anyhow::Result<Pixels> {
        capture::window::<_, GlesTexture>(self.backend.renderer(), window, scale)
    }

    fn render(&mut self, state: &mut State) {
        let start = Instant::now();
        let damage = {
            let (renderer, mut framebuffer) = match self.backend.bind() {
                Ok(bound) => bound,
                Err(err) => {
                    tracing::warn!(%err, "could not bind the window's surface");
                    return;
                }
            };
            match render_output::<_, WaylandSurfaceRenderElement<GlesRenderer>, _, _>(
                &self.output,
                renderer,
                &mut framebuffer,
                1.0,
                0,
                [&state.space],
                &[],
                &mut self.damage_tracker,
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
                Ok(()) => state.measures.frame(&self.output.name(), start.elapsed()),
                Err(err) => tracing::warn!(%err, "could not present the frame"),
            }
        }

        let elapsed = state.clock.now();
        for window in state.space.elements() {
            window.send_frame(&self.output, elapsed, Some(Duration::ZERO), |_, _| {
                Some(self.output.clone())
            });
        }
        state.space.refresh();
        state.popups.cleanup();
        state.needs_redraw = false;
    }
}
