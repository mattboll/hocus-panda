//! `capture`: a PNG of a window for an MCP agent (docs/design/mcp.md, "In
//! applications"), drawn by the backend as the user's window captures are
//! (`crate::capture::shoot`), at its screen's scale, and made into a PNG
//! off the loop. A window on a desktop no screen shows is woken first: a
//! frame callback, then the capture waits for its commit, [`WAIT`] at most
//! (v0-first-build's "A desktop it works on, unseen").

use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::json;
use smithay::{
    backend::renderer::{Bind, ExportMem, ImportAll, ImportMem, Offscreen, Renderer, Texture},
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Physical, Size},
};

use crate::{State, agents::Heard, going::Shaders, model::WindowId, wobble::Bend};

/// How long a capture waits for a window no screen shows to draw.
const WAIT: Duration = Duration::from_millis(200);

/// A capture for the backend to draw.
#[derive(Debug, Clone)]
pub struct Shot {
    pub session: u64,
    pub id: u64,
    pub window: WindowId,
}

/// A capture waiting for its window to draw.
#[derive(Debug)]
pub struct Wait {
    pub session: u64,
    shot: Shot,
    until: Instant,
}

impl State {
    /// Window `window` captured for call `id`: at once when a screen shows
    /// it, else once it drew.
    pub(super) fn mcp_capture(&mut self, session: u64, id: u64, window: WindowId) {
        let shot = Shot {
            session,
            id,
            window,
        };
        self.mcp_drive(window);
        if self.model.is_shown(window) {
            self.mcp.shots.push(shot);
            self.redraw = true;
            return;
        }
        if let (Some(managed), Some(output)) = (self.windows.get(&window), self.output_of(window)) {
            let now: Duration = self.clock.now().into();
            managed
                .window
                .send_frame(&output, now, Some(Duration::ZERO), |_, _| {
                    Some(output.clone())
                });
        }
        self.mcp.shot_waits.push(Wait {
            session,
            shot,
            until: Instant::now() + WAIT,
        });
        let timer = self.loop_handle.insert_source(
            smithay::reexports::calloop::timer::Timer::from_duration(WAIT),
            |_, _, app| {
                app.state.mcp_shots_due(None);
                smithay::reexports::calloop::timer::TimeoutAction::Drop
            },
        );
        if let Err(err) = timer {
            tracing::warn!(error = %err.error, "a capture does not wait");
            self.mcp_shots_due(None);
        }
    }

    /// A window's surface committed: a capture waiting for it goes on.
    pub(crate) fn mcp_committed(&mut self, root: &WlSurface) {
        if self.mcp.shot_waits.is_empty() {
            return;
        }
        let window = self
            .windows
            .iter()
            .find(|(_, m)| m.window.wl_surface().as_deref() == Some(root))
            .map(|(id, _)| *id);
        if let Some(window) = window {
            self.mcp_shots_due(Some(window));
        }
    }

    /// The captures that may be drawn now: their window drew, went, or the
    /// wait is over.
    fn mcp_shots_due(&mut self, drawn: Option<WindowId>) {
        let now = Instant::now();
        let windows = &self.windows;
        let (due, kept): (Vec<Wait>, Vec<Wait>) = std::mem::take(&mut self.mcp.shot_waits)
            .into_iter()
            .partition(|w| {
                Some(w.shot.window) == drawn
                    || now >= w.until
                    || !windows.contains_key(&w.shot.window)
            });
        self.mcp.shot_waits = kept;
        if !due.is_empty() {
            self.mcp.shots.extend(due.into_iter().map(|w| w.shot));
            self.redraw = true;
        }
    }

    /// Whether a capture waits for the backend.
    pub fn mcp_shots_pending(&self) -> bool {
        !self.mcp.shots.is_empty()
    }

    /// Every capture waiting refused: nothing can draw now (another
    /// terminal shows).
    pub fn mcp_shots_refused(&mut self, why: &str) {
        for shot in std::mem::take(&mut self.mcp.shots) {
            self.mcp_answer(shot.session, shot.id, Err(why.to_owned()));
        }
    }
}

use smithay::wayland::seat::WaylandFocus as _;

/// Draws the captures asked for with the backend's renderer, each made
/// into a PNG on a thread of its own, its answer sent back to the loop.
pub fn take<R, T>(renderer: &mut R, state: &mut State)
where
    R: Renderer + ImportAll + ImportMem + Offscreen<T> + Bind<T> + ExportMem + Bend + Shaders,
    R::TextureId: Send + Clone + Texture + 'static,
    R::Error: Send + Sync + 'static,
{
    for shot in std::mem::take(&mut state.mcp.shots) {
        let scale = state
            .output_of(shot.window)
            .map_or(1.0, |o| o.current_scale().fractional_scale());
        let Some((elements, rect)) = state.window_alone(renderer, shot.window, scale) else {
            state.mcp_answer(shot.session, shot.id, Err("the window went".into()));
            continue;
        };
        let size: Size<i32, Physical> = rect.size.to_physical_precise_round(scale);
        let pixels =
            match crate::capture::shoot::draw::<R, T>(renderer, size, scale, &elements, [0.0; 4]) {
                Ok(pixels) => pixels,
                Err(err) => {
                    state.mcp_answer(shot.session, shot.id, Err(format!("not drawn: {err:#}")));
                    continue;
                }
            };
        let sender = state.agents.sender.clone();
        let (w, h) = (size.w.max(1) as u32, size.h.max(1) as u32);
        let logical = (rect.size.w.round() as i32, rect.size.h.round() as i32);
        crate::programs::on_a_thread("mcp capture".to_owned(), move || {
            let result = png_of(pixels, w, h).map(|png| {
                json!({
                    "window": shot.window.0,
                    "width": logical.0,
                    "height": logical.1,
                    "scale": scale,
                    "png": base64::engine::general_purpose::STANDARD.encode(png),
                })
            });
            if let Some(sender) = sender {
                let _ = sender.send(Heard::Mcp(super::FromAgent::Png {
                    session: shot.session,
                    id: shot.id,
                    result,
                }));
            }
        });
    }
}

/// ARGB8888 pixels (B, G, R, A in memory) as a PNG.
fn png_of(mut pixels: Vec<u8>, w: u32, h: u32) -> Result<Vec<u8>, String> {
    for px in pixels.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    let image = image::RgbaImage::from_raw(w, h, pixels).ok_or("the capture's size is off")?;
    let mut out = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}
