//! A headless compositor driven from the same process, for tests: started
//! in a private runtime directory, dispatched by the test, with input
//! injected as a device would send it.

use std::{path::Path, time::Duration};

use anyhow::Context;
use smithay::{
    backend::input::{ButtonState, KeyState},
    desktop::Window,
    input::keyboard::Keycode,
    reexports::calloop::EventLoop,
    utils::{IsAlive, Point},
    wayland::{compositor::with_states, seat::WaylandFocus, shell::xdg::XdgToplevelSurfaceData},
};

use crate::{
    App, BackendKind, Config, HeadlessConfig, Options, RuntimeDir, State,
    model::{Rect, WindowId},
};

/// A headless compositor in a private runtime directory.
#[derive(Debug)]
pub struct Headless {
    event_loop: EventLoop<'static, App>,
    app: App,
    _runtime_dir: RuntimeDir,
    /// Milliseconds for input events' times.
    time: u32,
}

/// What a test can know about a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub id: WindowId,
    pub app_id: Option<String>,
    /// Its geometry (without client-side shadows), global coordinates.
    pub rect: Rect,
    /// The model's focus.
    pub focused: bool,
    /// Its surface has the keyboard.
    pub keyboard: bool,
}

impl Headless {
    /// Starts without XWayland and with the default configuration.
    pub fn start(config: HeadlessConfig) -> anyhow::Result<Self> {
        Self::start_with(config, Config::default())
    }

    pub fn start_with(headless: HeadlessConfig, config: Config) -> anyhow::Result<Self> {
        let started = crate::start(Options {
            backend: BackendKind::Headless(headless),
            runtime_dir: None,
            spawn: Vec::new(),
            xwayland: false,
            config,
        })?;
        Ok(Self {
            event_loop: started.event_loop,
            app: started.app,
            _runtime_dir: started.runtime_dir,
            time: 0,
        })
    }

    /// The Wayland socket, for `WAYLAND_DISPLAY` or a client to connect to.
    pub fn socket(&self) -> &Path {
        &self.app.state.socket_path
    }

    pub fn state(&self) -> &State {
        &self.app.state
    }

    /// The renderer drawing: `gles` or `pixman`.
    pub fn renderer(&self) -> &'static str {
        self.app.backend.headless().map_or("", |h| h.renderer())
    }

    /// Frames drawn so far, all outputs together.
    pub fn frames(&self) -> u64 {
        self.app.backend.headless().map_or(0, |h| h.frames())
    }

    /// One turn of the loop, waiting at most `timeout` for something.
    pub fn dispatch(&mut self, timeout: Duration) -> anyhow::Result<()> {
        self.event_loop
            .dispatch(Some(timeout), &mut self.app)
            .context("the loop failed")?;
        self.app.after_dispatch();
        Ok(())
    }

    /// Runs the loop until `done` holds, at most `timeout`; whether it did.
    pub fn dispatch_until(
        &mut self,
        timeout: Duration,
        mut done: impl FnMut(&Self) -> bool,
    ) -> anyhow::Result<bool> {
        let until = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < until {
            if done(self) {
                return Ok(true);
            }
            self.dispatch(Duration::from_millis(5))?;
        }
        Ok(done(self))
    }

    /// The mapped windows, bottom first.
    pub fn windows(&self) -> Vec<WindowInfo> {
        let state = &self.app.state;
        let keyboard = state.seat.get_keyboard().and_then(|k| k.current_focus());
        state
            .model
            .stack()
            .iter()
            .filter_map(|id| {
                let window = state.windows.get(id)?;
                let geometry = state.space.element_geometry(window)?;
                let surface = window.wl_surface().map(|s| s.into_owned());
                Some(WindowInfo {
                    id: *id,
                    app_id: app_id(window),
                    rect: Rect::new(
                        geometry.loc.x,
                        geometry.loc.y,
                        geometry.size.w,
                        geometry.size.h,
                    ),
                    focused: state.model.focused() == Some(*id),
                    keyboard: keyboard.as_ref().is_some_and(|k| {
                        k.alive() && k.wl_surface().map(|s| s.into_owned()) == surface
                    }),
                })
            })
            .collect()
    }

    /// The work area of the first output.
    pub fn work_area(&self) -> Option<Rect> {
        let state = &self.app.state;
        let output = state.space.outputs().next()?;
        state.work_area(output)
    }

    pub fn pointer_move(&mut self, x: f64, y: f64) {
        self.time += 10;
        let time = self.time;
        self.app.state.pointer_motion(Point::from((x, y)), time);
    }

    pub fn button(&mut self, button: u32, pressed: bool) {
        self.time += 10;
        let state = if pressed {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        };
        let time = self.time;
        self.app.state.pointer_button(button, state, time);
    }

    /// A key by its evdev code.
    pub fn key(&mut self, code: u32, pressed: bool) {
        self.time += 10;
        let state = if pressed {
            KeyState::Pressed
        } else {
            KeyState::Released
        };
        let time = self.time;
        // XKB keycodes are evdev's plus 8.
        self.app.state.key(Keycode::new(code + 8), state, time);
    }

    /// Whether the compositor offers the global named `interface`.
    pub fn has_dmabuf(&self) -> bool {
        self.app.state.dmabuf_global.is_some()
    }
}

/// A window's application id: xdg-shell's, or X11's class.
fn app_id(window: &Window) -> Option<String> {
    if let Some(x11) = window.x11_surface() {
        return Some(x11.class());
    }
    let toplevel = window.toplevel()?;
    with_states(toplevel.wl_surface(), |states| {
        states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .and_then(|d| d.lock().ok().and_then(|d| d.app_id.clone()))
    })
}
