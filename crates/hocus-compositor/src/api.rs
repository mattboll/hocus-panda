//! The control API on the compositor's side: what requests read and do, and
//! the events the compositor emits.

use base64::Engine;
use hocus_api::{
    ApiError, ConnectionId, ErrorCode, Event, Handler, Layer, OutputInfo, Point, Rect, Reply,
    Request, Scene, SceneOutput, Screenshot, Server, Target, VersionInfo, WindowInfo,
};
use smithay::{
    backend::input::{ButtonState, KeyState},
    desktop::Window,
    input::keyboard::Keycode,
    output::Output,
    reexports::calloop::channel::Sender,
    utils::{Logical, Rectangle},
    wayland::{compositor::with_states, shell::xdg::XdgToplevelSurfaceData},
};

use crate::{App, BackendState, State, capture::Pixels};

/// Evdev codes are offset by 8 in XKB.
const XKB_OFFSET: u32 = 8;

/// A screenshot encoded by a worker, on its way back to the loop.
pub type EncodedScreenshot = (ConnectionId, u64, Result<Screenshot, ApiError>);

/// A window's id for the API, given when its toplevel is created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowId(u64);

impl State {
    /// Gives `window` the next id.
    pub fn assign_window_id(&mut self, window: &Window) {
        self.next_window_id += 1;
        let id = self.next_window_id;
        window
            .user_data()
            .insert_if_missing_threadsafe(|| WindowId(id));
    }

    /// Sends `event` to subscribers; it is built only when there are some.
    pub fn emit(&mut self, event: impl FnOnce(&State) -> Event) {
        if self.api.has_subscribers() {
            let event = event(self);
            self.api.broadcast(&event);
        }
    }

    pub fn output_info(&self, output: &Output) -> OutputInfo {
        let geometry = self.space.output_geometry(output).unwrap_or_default();
        let pixels = output.current_mode().map(|m| m.size).unwrap_or_default();
        OutputInfo {
            name: output.name(),
            geometry: rect(geometry),
            pixel_width: pixels.w,
            pixel_height: pixels.h,
            scale: output.current_scale().fractional_scale(),
        }
    }

    pub fn window_info(&self, window: &Window) -> WindowInfo {
        let geometry = self.space.element_geometry(window).unwrap_or_default();
        let (app_id, title) = window
            .toplevel()
            .map(|toplevel| {
                with_states(toplevel.wl_surface(), |states| {
                    states
                        .data_map
                        .get::<XdgToplevelSurfaceData>()
                        .and_then(|data| data.lock().ok())
                        .map(|data| (data.app_id.clone(), data.title.clone()))
                        .unwrap_or_default()
                })
            })
            .unwrap_or_default();
        WindowInfo {
            id: window_id(window),
            app_id,
            title,
            output: self.output_of(window).map(|o| o.name()),
            geometry: rect(geometry),
            focused: self.focused_window().as_ref() == Some(window),
        }
    }

    /// The output showing most of `window`.
    fn output_of(&self, window: &Window) -> Option<&Output> {
        let geometry = self.space.element_geometry(window)?;
        self.space
            .outputs()
            .filter_map(|o| {
                let overlap = self.space.output_geometry(o)?.intersection(geometry)?;
                Some((overlap.size.w * overlap.size.h, o))
            })
            .max_by_key(|(area, _)| *area)
            .map(|(_, o)| o)
    }

    pub fn focused_window(&self) -> Option<Window> {
        let surface = self.seat.get_keyboard()?.current_focus()?;
        self.window_for(&surface)
    }

    fn scene(&self) -> Scene {
        let outputs = self
            .space
            .outputs()
            .map(|output| {
                let area = self.space.output_geometry(output).unwrap_or_default();
                let windows = self
                    .space
                    .elements()
                    .filter(|w| {
                        self.space
                            .element_geometry(w)
                            .is_some_and(|g| g.overlaps(area))
                    })
                    .map(|w| self.window_info(w))
                    .collect();
                SceneOutput {
                    output: self.output_info(output),
                    layers: vec![Layer {
                        name: "windows".into(),
                        windows,
                    }],
                }
            })
            .collect();
        let pointer = self
            .seat
            .get_pointer()
            .map(|p| p.current_location())
            .unwrap_or_default();
        Scene {
            outputs,
            focused: self.focused_window().map(|w| window_id(&w)),
            pointer: Point {
                x: pointer.x,
                y: pointer.y,
            },
        }
    }

    fn time_msec(&self) -> u32 {
        // Wraps after 49 days, like input devices' timestamps.
        self.clock.now().as_millis() as u32
    }
}

pub fn window_id(window: &Window) -> u64 {
    window
        .user_data()
        .get::<WindowId>()
        .map(|id| id.0)
        .unwrap_or_default()
}

fn rect(r: Rectangle<i32, Logical>) -> Rect {
    Rect {
        x: r.loc.x,
        y: r.loc.y,
        width: r.size.w,
        height: r.size.h,
    }
}

impl Handler for App {
    fn server(&mut self) -> &mut Server {
        &mut self.state.api
    }

    fn handle(
        &mut self,
        connection: ConnectionId,
        id: u64,
        request: Request,
    ) -> Option<Result<Reply, ApiError>> {
        let state = &mut self.state;
        let reply = match request {
            Request::Version => Reply::Version(VersionInfo {
                protocol: hocus_api::PROTOCOL_VERSION,
                compositor: env!("CARGO_PKG_VERSION").into(),
            }),
            Request::Outputs => Reply::Outputs(
                state
                    .space
                    .outputs()
                    .map(|o| state.output_info(o))
                    .collect(),
            ),
            Request::Windows => Reply::Windows(
                state
                    .space
                    .elements()
                    .map(|w| state.window_info(w))
                    .collect(),
            ),
            Request::Scene => Reply::Scene(state.scene()),
            // The server answers subscriptions itself.
            Request::Subscribe => Reply::Done,
            Request::PointerMove { x, y } => {
                state.pointer_motion((x, y).into(), state.time_msec());
                Reply::Done
            }
            Request::PointerButton { button, pressed } => {
                let button_state = if pressed {
                    ButtonState::Pressed
                } else {
                    ButtonState::Released
                };
                state.pointer_button(button.code(), button_state, state.time_msec());
                Reply::Done
            }
            Request::Key { code, pressed } => {
                let key_state = if pressed {
                    KeyState::Pressed
                } else {
                    KeyState::Released
                };
                state.key(
                    Keycode::new(code + XKB_OFFSET),
                    key_state,
                    state.time_msec(),
                );
                Reply::Done
            }
            Request::CloseWindow { window: wanted } => {
                let toplevel = state
                    .space
                    .elements()
                    .find(|w| window_id(w) == wanted)
                    .and_then(|w| w.toplevel().cloned());
                let Some(toplevel) = toplevel else {
                    return Some(Err(ApiError::new(
                        ErrorCode::NotFound,
                        format!("no window with id {wanted}"),
                    )));
                };
                toplevel.send_close();
                Reply::Done
            }
            Request::Screenshot { target } => {
                return match self.capture(&target) {
                    Ok(pixels) => {
                        encode_in_worker(self.state.screenshots.clone(), connection, id, pixels)
                            .err()
                            .map(Err)
                    }
                    Err(err) => Some(Err(err)),
                };
            }
        };
        Some(Ok(reply))
    }
}

impl App {
    fn capture(&mut self, target: &Target) -> Result<Pixels, ApiError> {
        let state = &self.state;
        let result = match target {
            Target::Output(name) => {
                let output = state
                    .space
                    .outputs()
                    .find(|o| o.name() == *name)
                    .cloned()
                    .ok_or_else(|| {
                        ApiError::new(ErrorCode::NotFound, format!("no output named {name}"))
                    })?;
                match &mut self.backend {
                    BackendState::Winit(winit) => winit.capture_output(&output, state),
                    BackendState::Headless(headless) => headless.capture_output(&output, state),
                }
            }
            Target::Window(id) => {
                let window = state
                    .space
                    .elements()
                    .find(|w| window_id(w) == *id)
                    .cloned()
                    .ok_or_else(|| {
                        ApiError::new(ErrorCode::NotFound, format!("no window with id {id}"))
                    })?;
                let scale = state
                    .output_of(&window)
                    .map_or(1.0, |o| o.current_scale().fractional_scale());
                match &mut self.backend {
                    BackendState::Winit(winit) => winit.capture_window(&window, scale),
                    BackendState::Headless(headless) => headless.capture_window(&window, scale),
                }
            }
        };
        result.map_err(|err| ApiError::new(ErrorCode::Failed, format!("{err:#}")))
    }
}

/// PNG encoding takes tens of milliseconds for a large output: off the loop.
fn encode_in_worker(
    sender: Sender<EncodedScreenshot>,
    connection: ConnectionId,
    id: u64,
    pixels: Pixels,
) -> Result<(), ApiError> {
    std::thread::Builder::new()
        .name("screenshot".into())
        .spawn(move || {
            let result = pixels
                .encode_png()
                .map(|png| Screenshot {
                    width: pixels.width,
                    height: pixels.height,
                    png_base64: base64::engine::general_purpose::STANDARD.encode(png),
                })
                .map_err(|err| ApiError::new(ErrorCode::Failed, format!("{err:#}")));
            // The loop is gone if this fails: nobody waits for the reply.
            let _ = sender.send((connection, id, result));
        })
        .map(drop)
        .map_err(|err| {
            ApiError::new(
                ErrorCode::Failed,
                format!("could not start the screenshot worker: {err}"),
            )
        })
}
