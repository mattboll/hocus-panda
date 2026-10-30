//! A headless compositor driven from the same process, for tests: started
//! in a private runtime directory, dispatched by the test, with input
//! injected as a device would send it.

use std::{path::Path, time::Duration};

use anyhow::Context;
use smithay::{
    backend::input::{ButtonState, KeyState},
    input::keyboard::Keycode,
    reexports::calloop::EventLoop,
    utils::{IsAlive, Point},
    wayland::seat::WaylandFocus,
};

use crate::{
    App, BackendKind, Config, HeadlessConfig, Options, RuntimeDir, State,
    model::{Rect, WindowId},
};

/// Where a window is, shown or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub id: WindowId,
    pub app_id: Option<String>,
    /// Where it is, or comes back to when it does not show; global.
    pub rect: Rect,
    /// The output it is on.
    pub output: Option<String>,
    /// From 0, on its output.
    pub desktop: usize,
    pub mode: crate::model::Mode,
    pub minimized: bool,
    /// Not minimised, and its desktop is the one its output shows.
    pub shown: bool,
}

/// A headless compositor in a private runtime directory.
#[derive(Debug)]
pub struct Headless {
    event_loop: EventLoop<'static, App>,
    app: App,
    _runtime_dir: RuntimeDir,
    /// Milliseconds for input events' times.
    time: u32,
    /// Last, so dropped once the loop and its connections are.
    _shell: ShellEnd,
}

/// Waits, when the harness is dropped, for its shell to end: it does once
/// its connections close, a moment after the test that started it.
#[derive(Debug)]
struct ShellEnd(std::sync::Arc<crate::shell::Running>);

impl Drop for ShellEnd {
    fn drop(&mut self) {
        if !self.0.wait_none(Duration::from_secs(5)) {
            tracing::warn!("the shell still runs 5 s after its compositor stopped");
        }
    }
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
        Self::start_with_shell(headless, config, None)
    }

    /// With `shell`'s program started as the shell (none by default).
    pub fn start_with_shell(
        headless: HeadlessConfig,
        config: Config,
        shell: Option<std::path::PathBuf>,
    ) -> anyhow::Result<Self> {
        let started = crate::start(Options {
            backend: BackendKind::Headless(headless),
            runtime_dir: None,
            spawn: Vec::new(),
            xwayland: false,
            session: false,
            config,
            shell,
        })?;
        let shell = ShellEnd(started.app.state.shell.running.clone());
        Ok(Self {
            event_loop: started.event_loop,
            app: started.app,
            _runtime_dir: started.runtime_dir,
            time: 0,
            _shell: shell,
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

    /// What output `index` (in the order given, of those plugged) showed
    /// last: its size in pixels, and its rows of ARGB8888 pixels (B, G, R,
    /// A in memory).
    pub fn screenshot(&mut self, index: usize) -> anyhow::Result<(i32, i32, Vec<u8>)> {
        self.app
            .backend
            .headless_mut()
            .context("not headless")?
            .screenshot(index)
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
                let window = &state.windows.get(id)?.window;
                let geometry = state.space.element_geometry(window)?;
                let surface = window.wl_surface().map(|s| s.into_owned());
                Some(WindowInfo {
                    id: *id,
                    app_id: crate::windows::app_id(window),
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

    /// How many windows are minimised.
    pub fn minimized(&self) -> usize {
        let state = &self.app.state;
        state
            .windows
            .keys()
            .filter(|id| state.model.is_minimized(**id))
            .count()
    }

    /// The window `id`'s placement: floating, snapped or fullscreen.
    pub fn mode(&self, id: WindowId) -> crate::model::Mode {
        self.app.state.model.mode(id)
    }

    /// How far the jelly of the window moving bends at most, logical
    /// pixels: none when nothing wobbles.
    pub fn bend(&self) -> Option<f64> {
        self.app
            .state
            .wobble
            .as_ref()
            .map(|(_, jelly)| jelly.largest())
    }

    /// Whether the keyboard is on a surface that is no window: a menu.
    pub fn keyboard_on_popup(&self) -> bool {
        self.app.state.popup_focus().is_some()
    }

    /// The bars: their screen, their form, and their width as drawn now.
    pub fn bars(&self) -> Vec<(String, hocus_link::Form, f64)> {
        let state = &self.app.state;
        let now = state.now();
        state
            .panel
            .bars
            .iter()
            .map(|(name, bar)| (name.clone(), bar.form, bar.width_at(now)))
            .collect()
    }

    /// Commits of the bars' surfaces so far.
    pub fn bar_commits(&self) -> u64 {
        self.app.state.panel.commits
    }

    /// Commits of the clocks' surfaces so far.
    pub fn clock_commits(&self) -> u64 {
        self.app.state.panel.clock_commits
    }

    /// Commits of the levels' surfaces so far.
    pub fn levels_commits(&self) -> u64 {
        self.app.state.panel.levels_commits
    }

    /// Commits of the connections' surfaces so far.
    pub fn connections_commits(&self) -> u64 {
        self.app.state.panel.connections_commits
    }

    /// Commits of the desktops' surfaces so far.
    /// Commits of the agents' group's surfaces: none at rest.
    pub fn agents_commits(&self) -> u64 {
        self.app.state.agents.commits
    }

    /// The agents as the shell is told them.
    pub fn agents(&mut self) -> Vec<hocus_link::AgentShown> {
        self.app.state.agents_shown()
    }

    /// As if every agent had been in its state `by` longer (its "12 min"
    /// without waiting for them): the shell is told.
    pub fn backdate_agents(&mut self, by: Duration) {
        self.app.state.backdate_agents(by);
    }

    /// The agents as the shell was last told them.
    pub fn agents_peek(&self) -> Vec<hocus_link::AgentShown> {
        self.app.state.agents.told.clone().unwrap_or_default()
    }

    /// The agents' tools as the shell is told them.
    pub fn agent_tools(&self) -> Vec<hocus_link::AgentTool> {
        self.app.state.agent_tools()
    }

    /// The control socket `hocus agent` writes to.
    pub fn control_socket(&self) -> Option<&Path> {
        self.app.state.agents.socket.as_deref()
    }

    /// The agents' group's surfaces: their output and where they are.
    pub fn agents_surfaces(&self) -> Vec<(String, Rect)> {
        self.layer_rects(hocus_link::AGENTS_NAMESPACE)
    }

    pub fn desktops_commits(&self) -> u64 {
        self.app.state.panel.desktops_commits
    }

    /// Commits of the popovers' surfaces so far.
    pub fn popover_commits(&self) -> u64 {
        self.app.state.panel.popover_commits
    }

    /// The popovers' surfaces as the shell placed them: their screen, and
    /// where they are (global).
    pub fn popover_surfaces(&self) -> Vec<(String, Rect)> {
        self.layer_rects(hocus_link::POPOVER_NAMESPACE)
    }

    /// The keyboard's active layout (XKB's group).
    pub fn layout(&self) -> u32 {
        self.app
            .state
            .seat
            .get_keyboard()
            .map_or(0, |k| k.modifier_state().serialized.layout_effective)
    }

    /// The namespace of the layer surface holding the keyboard, if one does.
    pub fn keyboard_layer(&self) -> Option<String> {
        let state = &self.app.state;
        let focus = state.seat.get_keyboard()?.current_focus()?;
        let crate::focus::KeyboardFocus::Wayland(surface) = focus else {
            return None;
        };
        state.space.outputs().find_map(|output| {
            let map = smithay::desktop::layer_map_for_output(output);
            map.layers()
                .find(|l| l.wl_surface() == &surface)
                .map(|l| l.namespace().to_owned())
        })
    }

    /// What output `index` (in the order given) drew over since the last
    /// call, as one rectangle of its pixels; `None`: nothing.
    pub fn take_damage(&mut self, index: usize) -> Option<Rect> {
        let d = self.app.backend.headless_mut()?.take_damage(index)?;
        Some(Rect::new(d.loc.x, d.loc.y, d.size.w, d.size.h))
    }

    /// The clocks' surfaces as the shell placed them: their screen, and
    /// where they are (global).
    pub fn clock_surfaces(&self) -> Vec<(String, Rect)> {
        self.layer_rects(hocus_link::CLOCK_NAMESPACE)
    }

    /// The levels' surfaces as the shell placed them: their screen, and
    /// where they are (global).
    pub fn levels_surfaces(&self) -> Vec<(String, Rect)> {
        self.layer_rects(hocus_link::LEVELS_NAMESPACE)
    }

    /// The connections' surfaces as the shell placed them: their screen,
    /// and where they are (global).
    pub fn connections_surfaces(&self) -> Vec<(String, Rect)> {
        self.layer_rects(hocus_link::CONNECTIONS_NAMESPACE)
    }

    /// The desktops' surfaces as the shell placed them: their screen, and
    /// where they are (global).
    pub fn desktops_surfaces(&self) -> Vec<(String, Rect)> {
        self.layer_rects(hocus_link::DESKTOPS_NAMESPACE)
    }

    /// The bars' surfaces as the shell placed them: their screen, and
    /// where they are (global).
    pub fn bar_surfaces(&self) -> Vec<(String, Rect)> {
        self.layer_rects(hocus_link::NAMESPACE)
    }

    fn layer_rects(&self, namespace: &str) -> Vec<(String, Rect)> {
        let state = &self.app.state;
        let mut surfaces = Vec::new();
        for output in state.space.outputs() {
            let Some(origin) = state.space.output_geometry(output).map(|g| g.loc) else {
                continue;
            };
            let map = smithay::desktop::layer_map_for_output(output);
            for layer in map.layers() {
                if layer.namespace() != namespace {
                    continue;
                }
                if let Some(g) = map.layer_geometry(layer) {
                    let rect =
                        Rect::new(origin.x + g.loc.x, origin.y + g.loc.y, g.size.w, g.size.h);
                    surfaces.push((output.name(), rect));
                }
            }
        }
        surfaces
    }

    /// Where `app_id`'s window is drawn now: on its way, or where it is.
    pub fn drawn_rect(&self, app_id: &str) -> Option<Rect> {
        let state = &self.app.state;
        let id = self
            .placements()
            .into_iter()
            .find(|p| p.app_id.as_deref() == Some(app_id))?
            .id;
        let r = state.visual_rect(id, state.now())?;
        Some(Rect::new(
            r.loc.x.round() as i32,
            r.loc.y.round() as i32,
            r.size.w.round() as i32,
            r.size.h.round() as i32,
        ))
    }

    /// Whether the shell said it draws on the CPU.
    pub fn shell_on_cpu(&self) -> bool {
        self.app.state.shell.software
    }

    /// Whether the shell is connected.
    pub fn shell_connected(&self) -> bool {
        self.app.state.shell_linked()
    }

    /// The work area of the first output.
    pub fn work_area(&self) -> Option<Rect> {
        let state = &self.app.state;
        let output = state.space.outputs().next()?;
        state.work_area(output)
    }

    /// The work areas of the outputs, in the order they were made.
    pub fn work_areas(&self) -> Vec<Rect> {
        let state = &self.app.state;
        let mut outputs: Vec<_> = state.space.outputs().cloned().collect();
        outputs.sort_by_key(|o| state.space.output_geometry(o).map(|g| (g.loc.x, g.loc.y)));
        outputs.iter().filter_map(|o| state.work_area(o)).collect()
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

    /// The wheel turned `notches` (positive: toward the user, down), as a
    /// mouse's: 15 px and 120 units of value120 a notch.
    pub fn wheel(&mut self, notches: i32) {
        self.scroll(
            smithay::backend::input::AxisSource::Wheel,
            15.0 * f64::from(notches),
            Some(120 * notches),
        );
    }

    /// Two fingers on a touchpad moved `pixels` down, in one event.
    pub fn finger_scroll(&mut self, pixels: f64) {
        self.scroll(smithay::backend::input::AxisSource::Finger, pixels, None);
    }

    fn scroll(
        &mut self,
        source: smithay::backend::input::AxisSource,
        value: f64,
        v120: Option<i32>,
    ) {
        use smithay::{backend::input::Axis, input::pointer::AxisFrame};
        self.time += 10;
        let mut frame = AxisFrame::new(self.time)
            .source(source)
            .value(Axis::Vertical, value);
        if let Some(v120) = v120 {
            frame = frame.v120(Axis::Vertical, v120);
        }
        let state = &mut self.app.state;
        if let Some(pointer) = state.seat.get_pointer() {
            pointer.axis(state, frame);
            pointer.frame(state);
        }
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

    /// Every window, shown or not, bottom first: its id, app id, desktop
    /// (from 0), and whether it shows.
    pub fn all_windows(&self) -> Vec<(WindowId, Option<String>, usize, bool)> {
        let state = &self.app.state;
        state
            .model
            .stack()
            .iter()
            .filter_map(|id| {
                let window = &state.windows.get(id)?.window;
                Some((
                    *id,
                    crate::windows::app_id(window),
                    state.model.desktop(*id),
                    state.model.is_shown(*id),
                ))
            })
            .collect()
    }

    /// The desktop the first output shows (from 0), and where its strip is.
    pub fn desktop(&self) -> (usize, f64) {
        let state = &self.app.state;
        let Some(output) = state.space.outputs().next() else {
            return (0, 0.0);
        };
        let strip = state.strip(output);
        (strip.current(), strip.position(state.now()))
    }

    /// The desktop the output under the pointer shows (from 0).
    pub fn desktop_under_pointer(&self) -> usize {
        let state = &self.app.state;
        state
            .output_under_pointer()
            .map_or(0, |o| state.strip(&o).current())
    }

    /// Alt+Tab while held: its windows in order, the selected one, and
    /// whether its panel shows.
    pub fn switcher(&self) -> Option<(Vec<WindowId>, usize, bool)> {
        let state = &self.app.state;
        let switcher = state.switcher.as_ref()?;
        Some((
            switcher.items.iter().map(|i| i.0).collect(),
            switcher.selected,
            switcher.shown(state.now()),
        ))
    }

    /// The centre of Alt+Tab's item `at`, global, once its panel shows.
    pub fn switcher_item(&self, at: usize) -> Option<(f64, f64)> {
        let state = &self.app.state;
        let switcher = state.switcher.as_ref()?;
        let origin = state.space.output_geometry(&switcher.output)?.loc;
        let (_, items) = state.switcher_layout(switcher)?;
        let (x, y) = items.get(at)?.item.centre();
        Some((x + f64::from(origin.x), y + f64::from(origin.y)))
    }

    /// Where Alt+Tab's thumbnail `at` is, global (x, y, width, height),
    /// once its panel shows.
    pub fn switcher_thumb(&self, at: usize) -> Option<(f64, f64, f64, f64)> {
        let state = &self.app.state;
        let switcher = state.switcher.as_ref()?;
        let origin = state.space.output_geometry(&switcher.output)?.loc;
        let (_, items) = state.switcher_layout(switcher)?;
        let thumb = items.get(at)?.thumb;
        Some((
            thumb.x + f64::from(origin.x),
            thumb.y + f64::from(origin.y),
            thumb.w,
            thumb.h,
        ))
    }

    /// Where Alt+Tab's panel and item `at`'s title line are, global
    /// (x, y, width, height), once its panel shows.
    pub fn switcher_label(&self, at: usize) -> Option<[(f64, f64, f64, f64); 2]> {
        let state = &self.app.state;
        let switcher = state.switcher.as_ref()?;
        let origin = state.space.output_geometry(&switcher.output)?.loc;
        let (panel, items) = state.switcher_layout(switcher)?;
        let global = |a: crate::switcher::Area| {
            (
                a.x + f64::from(origin.x),
                a.y + f64::from(origin.y),
                a.w,
                a.h,
            )
        };
        Some([global(panel), global(items.get(at)?.label)])
    }

    /// Whether the titles' font, read on a thread of its own, has come.
    pub fn has_titles_font(&self) -> bool {
        self.app.state.overlay.has_font()
    }

    /// A touchpad's swipe with `fingers`, its moves `moves` milliseconds
    /// apart, then lifted.
    pub fn swipe(&mut self, fingers: u32, moves: &[(f64, f64)], every: u32) {
        use crate::input::Gesture;
        self.time += 10;
        let time = self.time;
        self.app
            .state
            .gesture(Gesture::SwipeBegin { time, fingers });
        for &(dx, dy) in moves {
            self.time += every;
            let time = self.time;
            self.app.state.gesture(Gesture::SwipeUpdate {
                time,
                delta: (dx, dy).into(),
            });
        }
        self.time += every;
        let time = self.time;
        self.app.state.gesture(Gesture::SwipeEnd {
            time,
            cancelled: false,
        });
    }

    /// Every window, shown or not, bottom first, and where it is.
    pub fn placements(&self) -> Vec<Placed> {
        let state = &self.app.state;
        state
            .model
            .stack()
            .iter()
            .filter_map(|id| {
                let window = &state.windows.get(id)?.window;
                let placement = state.model.placement(*id);
                Some(Placed {
                    id: *id,
                    app_id: crate::windows::app_id(window),
                    rect: state.target_rect(*id)?,
                    output: state.output_of(*id).map(|o| o.name()),
                    desktop: placement.desktop,
                    mode: placement.mode,
                    minimized: placement.minimized,
                    shown: state.model.is_shown(*id),
                })
            })
            .collect()
    }

    /// The outputs' names and their geometry (global), left to right.
    pub fn outputs(&self) -> Vec<(String, Rect)> {
        let state = &self.app.state;
        let mut outputs: Vec<(String, Rect)> = state
            .space
            .outputs()
            .filter_map(|o| {
                let g = state.space.output_geometry(o)?;
                Some((o.name(), crate::windows::rect_of(g)))
            })
            .collect();
        outputs.sort_by_key(|(_, g)| (g.x, g.y));
        outputs
    }

    /// The work area of output `name`.
    pub fn work_area_of(&self, name: &str) -> Option<Rect> {
        let state = &self.app.state;
        let output = state.space.outputs().find(|o| o.name() == name)?;
        state.work_area(output)
    }

    /// Where the pointer is, global.
    pub fn pointer(&self) -> (f64, f64) {
        let at = self
            .app
            .state
            .seat
            .get_pointer()
            .map(|p| p.current_location())
            .unwrap_or_default();
        (at.x, at.y)
    }

    /// The namespace of the layer surface that takes the pointer where it
    /// is (its input region counts), if one does.
    pub fn pointer_layer(&self) -> Option<String> {
        use smithay::wayland::shell::wlr_layer::Layer;
        let (x, y) = self.pointer();
        let (_, _, layer) = self
            .app
            .state
            .layer_surface_under((x, y).into(), &[Layer::Overlay, Layer::Top])?;
        Some(layer.namespace().to_owned())
    }

    /// Whether a press went on to a client: the seat holds the pointer in
    /// a click grab from a press it forwards until its release.
    pub fn pointer_grabbed(&self) -> bool {
        self.app
            .state
            .seat
            .get_pointer()
            .is_some_and(|p| p.is_grabbed())
    }

    /// Output `name` goes, as a screen unplugged.
    pub fn unplug(&mut self, name: &str) -> anyhow::Result<()> {
        let App { state, backend } = &mut self.app;
        backend
            .headless_mut()
            .context("not headless")?
            .unplug(name, state)
    }

    /// Output `name`, unplugged before, comes back where it was.
    pub fn plug(&mut self, name: &str) -> anyhow::Result<()> {
        let App { state, backend } = &mut self.app;
        backend
            .headless_mut()
            .context("not headless")?
            .plug(name, None, state)
    }

    /// Output `name`, unplugged before, comes back with its top-left
    /// corner at (`x`, `y`).
    pub fn plug_at(&mut self, name: &str, x: i32, y: i32) -> anyhow::Result<()> {
        let App { state, backend } = &mut self.app;
        backend
            .headless_mut()
            .context("not headless")?
            .plug(name, Some((x, y)), state)
    }

    /// Why the compositor was asked to end, if it was.
    pub fn ending(&self) -> Option<crate::Ending> {
        self.app.state.ending
    }

    /// Whether the backend made the linux-dmabuf global.
    pub fn has_dmabuf(&self) -> bool {
        self.app.state.dmabuf_global.is_some()
    }
}
