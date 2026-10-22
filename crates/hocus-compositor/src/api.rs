//! The control API on the compositor's side: what requests read and do, and
//! the events the compositor emits.

use base64::Engine;
use hocus_api::{
    ApiError, ConfigInfo, ConnectionId, DesktopInfo, ErrorCode, Event, Handler, Layer,
    OutputDesktops, OutputInfo, Point, Rect, Reply, Request, Scene, SceneOutput, Screenshot,
    Server, Target, VersionInfo, WindowInfo, WindowState,
};
use smithay::{
    backend::input::{ButtonState, KeyState},
    desktop::Window,
    input::keyboard::Keycode,
    output::Output,
    reexports::calloop::channel::Sender,
    utils::{Logical, Rectangle, SERIAL_COUNTER},
};

use crate::{
    App, BackendState, OutputSpec, State,
    capture::Pixels,
    desktops::{model_id, title_and_app_id},
    window_mode::WindowMode,
};

/// Evdev codes are offset by 8 in XKB.
const XKB_OFFSET: u32 = 8;

/// A screenshot encoded by a worker, on its way back to the loop.
pub type EncodedScreenshot = (ConnectionId, u64, Result<Screenshot, ApiError>);

impl State {
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
        // A hidden window is where it will show again.
        let geometry = self.space.element_geometry(window).unwrap_or_else(|| {
            let location = model_id(window)
                .and_then(|id| self.windows.get(&id))
                .map(|m| m.location)
                .unwrap_or_default();
            Rectangle::new(location, window.geometry().size)
        });
        let (title, app_id) = title_and_app_id(window);
        WindowInfo {
            id: window_id(window),
            app_id,
            title,
            output: self.output_of(window).map(|o| o.name()),
            geometry: rect(geometry),
            focused: self.focused_window().as_ref() == Some(window),
            desktop: model_id(window)
                .and_then(|id| self.model.desktop_of(id))
                .map(|d| d.0)
                .unwrap_or_default(),
            visible: self.space.element_location(window).is_some(),
            state: self.window_state(window),
            opacity: model_id(window)
                .and_then(|id| self.windows.get(&id))
                .map_or(1.0, |m| f64::from(m.opacity)),
            pinned: model_id(window).is_some_and(|id| self.model.is_pinned(id)),
        }
    }

    /// The output showing most of `window`.
    pub fn output_of(&self, window: &Window) -> Option<&Output> {
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

    fn window_state(&self, window: &Window) -> WindowState {
        let Some(id) = model_id(window) else {
            return WindowState::Normal;
        };
        if self.model.is_minimized(id) {
            return WindowState::Minimized;
        }
        match self.windows.get(&id).map(|m| m.mode) {
            Some(WindowMode::Maximized { .. }) => WindowState::Maximized,
            Some(WindowMode::Fullscreen { .. }) => WindowState::Fullscreen,
            Some(WindowMode::Snapped { .. }) => WindowState::Snapped,
            Some(WindowMode::Tiled { .. }) => WindowState::Tiled,
            _ => WindowState::Normal,
        }
    }

    pub fn find_output(&self, name: &str) -> Option<Output> {
        self.space.outputs().find(|o| o.name() == name).cloned()
    }

    fn desktops_info(&self) -> Vec<OutputDesktops> {
        self.model
            .outputs()
            .iter()
            .map(|output| OutputDesktops {
                output: output.name.0.clone(),
                desktops: output
                    .desktops
                    .iter()
                    .filter_map(|id| self.model.desktop(*id))
                    .map(|d| DesktopInfo {
                        id: d.id.0,
                        name: d.name.clone(),
                        home: d.home.0.clone(),
                        windows: d.windows.iter().map(|w| w.0).collect(),
                    })
                    .collect(),
                shown: output.shown.0,
            })
            .collect()
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

/// The window's id for the API: the model's.
pub fn window_id(window: &Window) -> u64 {
    model_id(window).map(|id| id.0).unwrap_or_default()
}

fn not_headless() -> ApiError {
    ApiError::new(
        ErrorCode::BadRequest,
        "only headless outputs are plugged and unplugged by request",
    )
}

fn no_output(name: &str) -> ApiError {
    ApiError::new(ErrorCode::NotFound, format!("no output named {name}"))
}

fn model_error(err: hocus_core::ModelError) -> ApiError {
    let code = match err {
        hocus_core::ModelError::UnknownOutput
        | hocus_core::ModelError::UnknownDesktop
        | hocus_core::ModelError::UnknownWindow => ErrorCode::NotFound,
        hocus_core::ModelError::NotOnOutput | hocus_core::ModelError::AlreadyPlugged => {
            ErrorCode::BadRequest
        }
    };
    ApiError::new(code, err.to_string())
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
        self.timed("control request", |app| app.answer(connection, id, request))
    }
}

impl App {
    fn answer(
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
            Request::Screens => return Some(Ok(Reply::Screens(self.screens()))),
            Request::IdentifyOutputs => {
                state.identify_outputs();
                Reply::Done
            }
            Request::Outputs => Reply::Outputs(
                state
                    .space
                    .outputs()
                    .map(|o| state.output_info(o))
                    .collect(),
            ),
            // Shown ones bottom to top, then hidden ones.
            Request::Windows => Reply::Windows(
                state
                    .space
                    .elements()
                    .chain(
                        state
                            .windows
                            .values()
                            .map(|m| &m.window)
                            .filter(|w| state.space.element_location(w).is_none()),
                    )
                    .map(|w| state.window_info(w))
                    .collect(),
            ),
            Request::Desktops => Reply::Desktops(state.desktops_info()),
            Request::Ui => Reply::Ui(state.shell_ui()),
            Request::Config => Reply::Config(ConfigInfo {
                toml: state.config.to_toml(),
                error: state.config_error.clone(),
                warnings: state.config_warnings.clone(),
                generation: state.config_generation,
                accent: state.shell.is_some().then(|| state.accent_hex()),
                path: state
                    .config_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned()),
            }),
            Request::SaveSession => {
                state.save_session_now();
                Reply::Done
            }
            Request::OfferSession => {
                state.offer_session();
                Reply::Done
            }
            Request::OpenSettings { page } => {
                state.open_settings(page);
                Reply::Done
            }
            Request::PlugOutput { spec, name } => {
                let spec: OutputSpec = match spec.parse() {
                    Ok(spec) => spec,
                    Err(err) => {
                        return Some(Err(ApiError::new(ErrorCode::BadRequest, format!("{err}"))));
                    }
                };
                let App { state, backend } = self;
                let BackendState::Headless(headless) = backend else {
                    return Some(Err(not_headless()));
                };
                return Some(
                    headless
                        .plug(&spec, name, state)
                        .map(|_| Reply::Done)
                        .map_err(|err| ApiError::new(ErrorCode::Failed, format!("{err:#}"))),
                );
            }
            Request::UnplugOutput { output } => {
                let App { state, backend } = self;
                let BackendState::Headless(headless) = backend else {
                    return Some(Err(not_headless()));
                };
                return Some(
                    headless
                        .unplug(&output, state)
                        .map(|()| Reply::Done)
                        .map_err(|err| ApiError::new(ErrorCode::NotFound, format!("{err:#}"))),
                );
            }
            Request::StallOutput { output } => {
                let BackendState::Headless(headless) = &mut self.backend else {
                    return Some(Err(not_headless()));
                };
                return Some(
                    headless
                        .stall(&output)
                        .map(|()| Reply::Done)
                        .map_err(|err| ApiError::new(ErrorCode::NotFound, format!("{err:#}"))),
                );
            }
            Request::SetPrimaryOutput { output } => {
                return Some(
                    state
                        .model
                        .set_primary(&hocus_core::OutputName(output))
                        .map(|()| Reply::Done)
                        .map_err(model_error),
                );
            }
            Request::ShowDesktop { output, index } => {
                let output = state.find_output(&output).ok_or_else(|| no_output(&output));
                let result = output.and_then(|output| {
                    state
                        .show_desktop(&output, index, SERIAL_COUNTER.next_serial())
                        .map_err(model_error)
                });
                return Some(result.map(|()| Reply::Done));
            }
            Request::SendWindow {
                window,
                output,
                desktop,
            } => {
                let output = state.find_output(&output).ok_or_else(|| no_output(&output));
                let result = output.and_then(|output| {
                    state
                        .send_window(
                            hocus_core::WindowId(window),
                            &output,
                            desktop,
                            SERIAL_COUNTER.next_serial(),
                        )
                        .map_err(model_error)
                });
                return Some(result.map(|()| Reply::Done));
            }
            Request::Scene => Reply::Scene(state.scene()),
            Request::Stats => {
                let names: Vec<String> = state.space.outputs().map(|o| o.name()).collect();
                Reply::Stats(state.measures.report(names.iter().map(String::as_str)))
            }
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
            Request::PointerScroll { dy } => {
                state.scroll(dy, state.time_msec());
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
            Request::SetWindowState {
                window,
                state: wanted,
            } => {
                let id = hocus_core::WindowId(window);
                if !state.windows.contains_key(&id) {
                    return Some(Err(ApiError::new(
                        ErrorCode::NotFound,
                        format!("no window with id {window}"),
                    )));
                }
                if matches!(wanted, WindowState::Snapped | WindowState::Tiled) {
                    return Some(Err(ApiError::new(
                        ErrorCode::BadRequest,
                        "snapping needs a zone: use snap_window",
                    )));
                }
                let serial = SERIAL_COUNTER.next_serial();
                // From whatever it is now to what is asked.
                if state.model.is_minimized(id) {
                    state.restore(id, serial);
                }
                match wanted {
                    WindowState::Normal => {
                        state.set_fullscreen(id, false, None, serial);
                        state.unlay(id, serial);
                    }
                    WindowState::Snapped | WindowState::Tiled => {}
                    WindowState::Maximized => {
                        state.set_fullscreen(id, false, None, serial);
                        state.set_maximized(id, true, serial);
                    }
                    WindowState::Fullscreen => state.set_fullscreen(id, true, None, serial),
                    WindowState::Minimized => state.minimize(id, serial),
                }
                Reply::Done
            }
            Request::SnapWindow { window, zone } => {
                let id = hocus_core::WindowId(window);
                let zone: hocus_core::Zone = match zone.parse() {
                    Ok(zone) => zone,
                    Err(err) => return Some(Err(ApiError::new(ErrorCode::BadRequest, err))),
                };
                if !state.windows.contains_key(&id) {
                    return Some(Err(ApiError::new(
                        ErrorCode::NotFound,
                        format!("no window with id {window}"),
                    )));
                }
                state.snap(id, zone, SERIAL_COUNTER.next_serial());
                Reply::Done
            }
            Request::CloseWindow { window: wanted } => {
                let window = state
                    .windows
                    .get(&hocus_core::WindowId(wanted))
                    .map(|m| m.window.clone());
                let Some(window) = window else {
                    return Some(Err(ApiError::new(
                        ErrorCode::NotFound,
                        format!("no window with id {wanted}"),
                    )));
                };
                crate::surface::close(&window);
                Reply::Done
            }
            Request::Screenshot { target, cursor } => {
                return match self.capture(&target, cursor) {
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
    fn capture(&mut self, target: &Target, cursor: bool) -> Result<Pixels, ApiError> {
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
                    BackendState::Winit(winit) => winit.capture_output(&output, state, cursor),
                    BackendState::Headless(headless) => {
                        headless.capture_output(&output, state, cursor)
                    }
                    BackendState::Tty(tty) => tty.capture_output(&output, state, cursor),
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
                    BackendState::Tty(tty) => tty.capture_window(&window, scale),
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
