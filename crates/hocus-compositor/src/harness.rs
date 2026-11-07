//! A headless compositor driven from the same process, for tests: started
//! in a private runtime directory, dispatched by the test, with input
//! injected as a device would send it.

use std::{path::Path, time::Duration};

use anyhow::Context;
use smithay::{
    backend::input::{ButtonState, KeyState},
    input::keyboard::Keycode,
    output::Output,
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

impl Drop for Headless {
    /// With its shell and its programs still running: what drew on a GPU
    /// holds its render node open now.
    fn drop(&mut self) {
        crate::test_gpu::check_none_open();
    }
}

/// A window the overview lays out: its app id, its place (x, y, width,
/// height), global.
pub type OverviewWindow = (Option<String>, (f64, f64, f64, f64));
/// A window dragged in the overview: its app id, the thumbnail it is
/// over (screen, desktop).
pub type OverviewDrag = (Option<String>, Option<(String, usize)>);

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
        Self::start_on_bus(headless, config, shell, None)
    }

    /// The same, the session bus at `bus` (`--session-bus`): the shell
    /// serves the notifications there.
    pub fn start_on_bus(
        headless: HeadlessConfig,
        config: Config,
        shell: Option<std::path::PathBuf>,
        bus: Option<String>,
    ) -> anyhow::Result<Self> {
        Self::start_on_buses(headless, config, shell, bus, None)
    }

    /// The same, the system bus at `system` (`--system-bus`) too: logind
    /// there does the session menu's power actions (7.4) and says the
    /// sleeps (7.2), UPower the power source. Never the machine's own in a
    /// test: a private bus with a fake logind.
    pub fn start_on_buses(
        headless: HeadlessConfig,
        config: Config,
        shell: Option<std::path::PathBuf>,
        bus: Option<String>,
        system: Option<String>,
    ) -> anyhow::Result<Self> {
        Self::start_all(headless, config, shell, bus, system, false)
    }

    /// With XWayland, for X11 clients: its display is
    /// [`Headless::xdisplay`] once it is ready. Without the `Xwayland`
    /// program, it never is.
    pub fn start_with_xwayland(headless: HeadlessConfig) -> anyhow::Result<Self> {
        Self::start_all(headless, Config::default(), None, None, None, true)
    }

    fn start_all(
        headless: HeadlessConfig,
        config: Config,
        shell: Option<std::path::PathBuf>,
        bus: Option<String>,
        system: Option<String>,
        xwayland: bool,
    ) -> anyhow::Result<Self> {
        // A GPU only when the developer named one (#534).
        let headless = crate::test_gpu::resolve(headless)?;
        let started = crate::start(Options {
            backend: BackendKind::Headless(headless),
            runtime_dir: None,
            spawn: Vec::new(),
            xwayland,
            session: false,
            config,
            shell,
            session_bus: bus,
            system_bus: system,
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

    /// XWayland's display number, once it is ready.
    pub fn xdisplay(&self) -> Option<u32> {
        self.app.state.xdisplay
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

    /// The caret of `app_id`'s window's enabled text input (surface-local:
    /// x, y, width, height), as its client last committed it: `None`
    /// without one enabled, `Some(None)` with one that said no caret.
    pub fn text_input(&self, app_id: &str) -> Option<Option<(i32, i32, i32, i32)>> {
        let state = &self.app.state;
        let surface = state.windows.values().find_map(|m| {
            (crate::windows::app_id(&m.window).as_deref() == Some(app_id))
                .then(|| m.window.wl_surface().map(|s| s.into_owned()))
                .flatten()
        })?;
        if !state.emoji.inputs.enabled(&surface) {
            return None;
        }
        Some(
            state
                .emoji
                .inputs
                .caret(&surface)
                .map(|r| (r.loc.x, r.loc.y, r.size.w, r.size.h)),
        )
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

    /// Where screen `name`'s bar is drawn now, global: the place of its
    /// glass and its halo.
    pub fn bar_place(&self, name: &str) -> Option<Rect> {
        let state = &self.app.state;
        let output = state.space.outputs().find(|o| o.name() == name)?;
        let at = state.space.output_geometry(output)?.loc;
        let bar = state.bar_rect(output, state.now())?;
        Some(Rect::new(
            at.x + bar.loc.x.round() as i32,
            at.y + bar.loc.y.round() as i32,
            bar.size.w.round() as i32,
            bar.size.h.round() as i32,
        ))
    }

    /// Commits of the windows' surfaces so far, all windows together.
    pub fn window_commits(&self) -> u64 {
        self.app.state.window_commits
    }

    /// Starts `launch` as the launcher's applications start: off the loop,
    /// with an activation token from the desktop for its first window.
    pub fn launch(&mut self, launch: hocus_link::Launch) {
        self.app.state.launch(launch);
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

    /// Commits of the signs of what is in use so far (5.12).
    pub fn in_use_commits(&self) -> u64 {
        self.app.state.panel.in_use_commits
    }

    /// The commits of Today's surfaces (Day.3), all bars'.
    pub fn today_commits(&self) -> u64 {
        self.app.state.panel.today_commits
    }

    /// The commits of the windows' surfaces in the bar (#604), all bars'.
    pub fn windows_commits(&self) -> u64 {
        self.app.state.panel.windows_commits
    }

    /// The commits of the next meeting's surfaces (Day.2), all bars'.
    pub fn meeting_commits(&self) -> u64 {
        self.app.state.panel.meeting_commits
    }

    /// The commits of the meetings' reminders' tabs (Day.6), all bars'.
    pub fn reminder_commits(&self) -> u64 {
        self.app.state.panel.reminder_commits
    }

    /// The meetings' reminders' tabs (Day.6): their namespace, screen and
    /// place (global logical pixels), top first.
    pub fn reminder_surfaces(&self) -> Vec<(String, String, Rect)> {
        let state = &self.app.state;
        let mut surfaces = Vec::new();
        for output in state.space.outputs() {
            let Some(origin) = state.space.output_geometry(output).map(|g| g.loc) else {
                continue;
            };
            let map = smithay::desktop::layer_map_for_output(output);
            for layer in map.layers() {
                if !hocus_link::is_reminder(layer.namespace()) {
                    continue;
                }
                if let Some(g) = map.layer_geometry(layer) {
                    let rect =
                        Rect::new(origin.x + g.loc.x, origin.y + g.loc.y, g.size.w, g.size.h);
                    surfaces.push((layer.namespace().to_owned(), output.name(), rect));
                }
            }
        }
        surfaces.sort_by_key(|(_, _, r)| r.y);
        surfaces
    }

    /// Commits of the connections' surfaces so far.
    pub fn connections_commits(&self) -> u64 {
        self.app.state.panel.connections_commits
    }

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

    /// Going to an agent (4.3): its streak as the last frame drew it.
    pub fn streak(&self) -> Option<crate::going::Streak> {
        let state = &self.app.state;
        state.streak_at(state.drawn_at.get().unwrap_or_else(|| state.now()))
    }

    /// The halo (4.4) as the last frame drew it: `None` when it showed
    /// nothing (or is not drawn: on Pixman, without a bar).
    pub fn halo(&self) -> Option<crate::halo::Shown> {
        let state = &self.app.state;
        state.halo_at(state.drawn_at.get().unwrap_or_else(|| state.now()))
    }

    /// The panda peeking out (4.5) as the last frame drew it: how opaque,
    /// and how far toward the bar; `None` when it showed none.
    pub fn peek(&self) -> Option<crate::peek::Look> {
        let state = &self.app.state;
        state.peek_at(state.drawn_at.get().unwrap_or_else(|| state.now()))
    }

    /// Which agent's panda is out, and why, drawn or not; and whether it
    /// is going back.
    pub fn peek_out(&self) -> Option<(u64, hocus_link::PeekKind, bool)> {
        let peek = &self.app.state.peek;
        peek.out().map(|(id, kind)| (id, kind, peek.going_back()))
    }

    /// When the panda peeking out had its surface's first frame, which
    /// starts its motion and its 8 s, on the compositor's clock
    /// ([`Headless::now`]): `None` before, and when none is out.
    pub fn peek_since(&self) -> Option<Duration> {
        self.app.state.peek.since()
    }

    /// The peeks told the shell so far.
    pub fn peeks_told(&self) -> u64 {
        self.app.state.peek.told
    }

    /// Commits of the peek's surfaces so far.
    pub fn peek_commits(&self) -> u64 {
        self.app.state.panel.peek_commits
    }

    /// The peek's surfaces as the shell placed them: their screen, and
    /// where they are (global).
    pub fn peek_surfaces(&self) -> Vec<(String, Rect)> {
        self.layer_rects(hocus_link::PEEK_NAMESPACE)
    }

    /// What concerns the user most, as the halo has it, drawn or not.
    pub fn halo_concern(&self) -> Option<crate::halo::Concern> {
        self.app.state.halo.concern()
    }

    /// The halo's waves started so far.
    pub fn halo_waves(&self) -> u64 {
        self.app.state.halo.waves
    }

    /// The rim's pulse of the window of `app_id` (gone to from an agent's
    /// row, or opened without the keyboard), as the last frame drew it: its
    /// 2 px rim's alpha, and the ring's width when it grows around it.
    /// `None` without one.
    pub fn pulse(&self, app_id: &str) -> Option<(f32, Option<f64>)> {
        let state = &self.app.state;
        let managed = state.windows.values().find(|m| {
            m.pulse.is_some() && crate::windows::app_id(&m.window).as_deref() == Some(app_id)
        })?;
        let pulse = managed.pulse_look(state.drawn_at.get().unwrap_or_else(|| state.now()));
        Some((pulse.rim, pulse.ring.map(|(width, _)| width)))
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

    /// The bar's own surfaces asked for on no screen so far: the shell
    /// names one for each.
    pub fn bar_parts_asked_nowhere(&self) -> u64 {
        self.app.state.panel.asked_nowhere
    }

    /// Commits of the desktops' surfaces so far.
    pub fn desktops_commits(&self) -> u64 {
        self.app.state.panel.desktops_commits
    }

    /// Commits of the to-do list's surfaces so far (Day.4).
    pub fn tasks_commits(&self) -> u64 {
        self.app.state.panel.tasks_commits
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

    /// Commits of the launcher's surfaces so far.
    pub fn launcher_commits(&self) -> u64 {
        self.app.state.panel.launcher_commits
    }

    /// Whether the launcher shows at rest in the last frame drawn: its
    /// fade in over, which starts at the shell's first buffer, however
    /// late that comes on a loaded machine.
    pub fn launcher_at_rest(&self) -> bool {
        let state = &self.app.state;
        let drawn = state.drawn_at.get();
        drawn.is_some_and(|at| state.panel.launchers.at_rest(at))
    }

    /// When the launcher's toast was first drawn, on the compositor's
    /// clock ([`Headless::now`]), while it shows.
    pub fn launcher_toast_since(&self) -> Option<Duration> {
        self.app.state.panel.launchers.toast_since()
    }

    /// The launcher's surfaces as the shell placed them: their screen, and
    /// where they are (global).
    pub fn launcher_surfaces(&self) -> Vec<(String, Rect)> {
        self.layer_rects(hocus_link::LAUNCHER_NAMESPACE)
    }

    /// The launcher's toasts as the shell placed them (5.2): their screen,
    /// and where they are (global).
    pub fn launcher_toast_surfaces(&self) -> Vec<(String, Rect)> {
        self.layer_rects(hocus_link::LAUNCHER_TOAST_NAMESPACE)
    }

    /// The text the compositor itself put on the clipboard (the shell's
    /// copies), if the clipboard holds it.
    pub fn clipboard(&self) -> Option<String> {
        smithay::wayland::selection::data_device::current_data_device_selection_userdata(
            &self.app.state.seat,
        )
        .and_then(|bytes| {
            bytes
                .as_ref()
                .map(|b| String::from_utf8_lossy(b).into_owned())
        })
    }

    /// What the compositor itself put on the clipboard (the shell's copies,
    /// the clipboard's history's items), as its bytes.
    pub fn clipboard_bytes(&self) -> Option<Vec<u8>> {
        smithay::wayland::selection::data_device::current_data_device_selection_userdata(
            &self.app.state.seat,
        )
        .and_then(|bytes| bytes.as_ref().map(|b| b.to_vec()))
    }

    /// The clipboard's history holds `items`, `pinned` of them pinned, as
    /// a shell would say (6.17): for tests of the Settings without one.
    pub fn clipboard_counted(&mut self, items: u32, pinned: u32) {
        self.app.state.clipboard_counted(items, pinned);
    }

    /// The shares under way (5.11).
    pub fn shares(&self) -> Vec<crate::ShareInfo> {
        self.app.state.share.info()
    }

    /// The request whose chooser shows (5.11).
    /// The authentication dialog's surface's commits (7.5).
    pub fn auth_commits(&self) -> u64 {
        self.app.state.auth.commits
    }

    /// How the authentication dialog and its dim are drawn now, if it
    /// shows (7.5).
    pub fn auth_look(&self) -> Option<crate::auth::Look> {
        self.app.state.auth_look()
    }

    /// Whether the authentication dialog holds the keyboard and the
    /// pointer (7.5).
    pub fn auth_holds(&self) -> bool {
        self.app.state.auth_holds()
    }

    pub fn share_choosing(&self) -> Option<u64> {
        self.app.state.share.choosing()
    }

    /// The frames sharing handed to PipeWire so far (5.11).
    pub fn share_frames(&self) -> u64 {
        self.app.state.share.frames_sent()
    }

    /// The longest a frame's hand-over to PipeWire's thread took the loop
    /// so far (5.11): a timing, to report.
    pub fn share_lend_most(&self) -> Duration {
        self.app.state.share.lend_most()
    }

    /// The bytes of shared frames' pixels copied on the loop so far
    /// (5.11): its budget is 0.
    pub fn share_copied_on_loop(&self) -> u64 {
        self.app.state.share.copied_on_loop()
    }

    /// The recording under way (5.10).
    pub fn recording(&self) -> Option<crate::RecordInfo> {
        self.app.state.record.info()
    }

    /// Whether recordings stopped are still being ended.
    pub fn recording_ending(&self) -> bool {
        self.app.state.record.ending()
    }

    /// The last recording saved: its file, its size, how long it lasts,
    /// its first frame's picture and its encoder.
    pub fn recorded(&self) -> Option<crate::record::encode::Saved> {
        self.app.state.record.last_saved().cloned()
    }

    /// Why recordings failed, so far.
    pub fn record_failures(&self) -> Vec<String> {
        self.app.state.record.failures().to_vec()
    }

    /// The recording's frames handed to its recorder so far.
    pub fn record_frames(&self) -> u64 {
        self.app.state.record.frames_sent()
    }

    /// The longest a recording's frame's hand-over took the loop (a
    /// timing: never a budget).
    pub fn record_lend_most(&self) -> Duration {
        self.app.state.record.lend_most()
    }

    /// The bytes of recorded frames copied on the loop: a budget of 0.
    pub fn record_copied_on_loop(&self) -> u64 {
        self.app.state.record.copied_on_loop()
    }

    /// Starts the start-up applications of these autostart directories
    /// (the user's first) once the bar shows, as `--session` does those of
    /// the XDG directories; asked before the loop runs.
    pub fn autostart(&mut self, dirs: Vec<std::path::PathBuf>) {
        self.app.state.autostart = crate::session::autostart::Autostart::Waiting(dirs);
    }

    /// The session is saved in the configuration's state directory, and
    /// the last one saved there put back once the bar shows, as
    /// `--session` does (windows.md §14); asked before the loop runs.
    pub fn restore_session(&mut self) {
        self.app.state.session_begin();
    }

    /// Saves the session now, and waits until it is written.
    pub fn save_session(&mut self) -> anyhow::Result<()> {
        self.app.state.session_save();
        anyhow::ensure!(
            self.app.state.session_flush(Duration::from_secs(10)),
            "the session was not written"
        );
        Ok(())
    }

    /// The session ends, as the compositor's does: saved and written.
    pub fn end_session(&mut self) {
        self.app.state.session_end();
    }

    /// Whether the last session is still coming back.
    pub fn session_restoring(&self) -> bool {
        self.app.state.session_restoring()
    }

    /// The last session's applications that did not come back, by name.
    pub fn session_not_back(&self) -> Vec<String> {
        self.app.state.session_not_back().to_vec()
    }

    /// Runs `action` on window `id`, as its radial menu does (#723).
    pub fn window_action(&mut self, id: WindowId, action: hocus_link::WindowAction) {
        self.app.state.run_window_action(id, action);
    }

    /// Puts floating window `id` at `rect` (global), as a drag and a
    /// resize would.
    pub fn place_window(&mut self, id: WindowId, rect: Rect) {
        self.app.state.configure(id, rect);
    }

    /// The screen reader may start, as on a real session, its program
    /// `program` instead of Orca (#734).
    pub fn allow_screen_reader(&mut self, program: std::path::PathBuf) {
        self.app.state.screen_reader.allowed = true;
        self.app.state.screen_reader.program = Some(program);
    }

    /// The screen reader's process, while it runs.
    pub fn screen_reader(&self) -> Option<u32> {
        self.app.state.screen_reader.running()
    }

    /// Tells the shell `message`, as the compositor would.
    pub fn tell_shell(&mut self, message: hocus_link::ToShell) {
        self.app.state.tell_shell(message);
    }

    /// The shell asks something of the recording, as its bar's item does.
    pub fn record_ask(&mut self, ask: hocus_link::RecordAsk) {
        self.app.state.record_asked(ask);
    }

    /// The session leaves its terminal, as the tty backend says when the
    /// display manager's lock or another session shows (5.10's lock).
    pub fn session_paused(&mut self) {
        self.app.state.record_screen_locks();
    }

    /// What the screen capture shows (5.8).
    pub fn capture(&self) -> crate::capture::Shown {
        self.app.state.capture.shown()
    }

    /// What the shell's capture toolbar says when clicked, as if it was.
    pub fn capture_ask(&mut self, ask: hocus_link::CaptureAsk) {
        self.app.state.capture_asked(ask);
    }

    /// Captures made so far, and the last file written.
    pub fn captures(&self) -> (u64, Option<std::path::PathBuf>) {
        self.app.state.capture.taken()
    }

    /// The capture's toolbar's surfaces, by output name, and where they
    /// are (global).
    pub fn capture_toolbar_surfaces(&self) -> Vec<(String, Rect)> {
        self.layer_rects(hocus_link::CAPTURE_TOOLBAR_NAMESPACE)
    }

    /// The capture's thumbnail's surfaces (5.9), by output name, and where
    /// they are (global).
    pub fn capture_thumb_surfaces(&self) -> Vec<(String, Rect)> {
        self.layer_rects(hocus_link::CAPTURE_THUMB_NAMESPACE)
    }

    /// The xdg-foreign handle made for the shell's surface of `namespace`,
    /// as the shell asks for it before a portal's dialog (#595).
    pub fn export_for_shell(&mut self, namespace: &str) -> Option<String> {
        self.app.state.export_for_shell(namespace)
    }

    /// The capture's editor's surfaces (5.9).
    pub fn capture_editor_surfaces(&self) -> Vec<(String, Rect)> {
        self.layer_rects(hocus_link::CAPTURE_EDITOR_NAMESPACE)
    }

    /// Commits of the capture's editor's surface so far.
    pub fn capture_editor_commits(&self) -> u64 {
        self.app.state.capture.editor_commits()
    }

    /// The cursor the compositor shows over a frame, a grip or the
    /// capture, by its name (`crosshair`, `move`, `nwse-resize`…).
    pub fn frame_cursor(&self) -> Option<String> {
        self.app.state.frame_cursor.map(|c| c.name().to_owned())
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

    /// Whether the bar on `output` is opaque behind a maximised window, as
    /// the shell was told (bar.md §11).
    pub fn bar_opaque(&self, output: &str) -> bool {
        self.app.state.bar_opaque(output)
    }

    /// Whether the compositor blurs behind the bars.
    pub fn glass_on(&self) -> bool {
        self.app.state.glass_on()
    }

    /// The screen whose bar holds the keyboard (an item of it, or its
    /// popover), if one does (`Super+B`, bar.md §11).
    pub fn bar_keyboard(&self) -> Option<String> {
        self.app.state.bar_holding_keyboard()
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

    /// The notifications' bubbles as the shell placed them, top first:
    /// their namespace, their screen, and where they are (global).
    pub fn bubble_surfaces(&self) -> Vec<(String, String, Rect)> {
        let state = &self.app.state;
        let mut surfaces = Vec::new();
        for output in state.space.outputs() {
            let Some(origin) = state.space.output_geometry(output).map(|g| g.loc) else {
                continue;
            };
            let map = smithay::desktop::layer_map_for_output(output);
            for layer in map.layers() {
                if !hocus_link::is_bubble(layer.namespace()) {
                    continue;
                }
                if let Some(g) = map.layer_geometry(layer) {
                    let rect =
                        Rect::new(origin.x + g.loc.x, origin.y + g.loc.y, g.size.w, g.size.h);
                    surfaces.push((layer.namespace().to_owned(), output.name(), rect));
                }
            }
        }
        surfaces.sort_by_key(|(_, _, r)| r.y);
        surfaces
    }

    /// Commits of the bubbles' surfaces so far.
    pub fn bubble_commits(&self) -> u64 {
        self.app.state.bubbles.commits
    }

    /// The "+N waiting" chip under the bubbles, where it is.
    pub fn bubbles_waiting_surfaces(&self) -> Vec<(String, Rect)> {
        self.layer_rects(hocus_link::BUBBLES_WAITING_NAMESPACE)
    }

    /// The bells on the bars' notifications place.
    pub fn bell_surfaces(&self) -> Vec<(String, Rect)> {
        self.layer_rects(hocus_link::BELL_NAMESPACE)
    }

    /// The bars' surfaces as the shell placed them: their screen, and
    /// where they are (global).
    pub fn bar_surfaces(&self) -> Vec<(String, Rect)> {
        self.layer_rects(hocus_link::NAMESPACE)
    }

    /// Holds the motions' clock where it is: from now on it moves only by
    /// [`Headless::step`], each step drawing a frame at its time. A test
    /// sees a motion at the same moments then, however late a loaded
    /// machine draws; the frame callbacks and presentation times the
    /// clients get stay on the real clock.
    pub fn hold_clock(&mut self) {
        let state = &mut self.app.state;
        if state.held_clock.is_none() {
            state.held_clock = Some(state.now());
        }
    }

    /// Lets the motions' clock run again, from where it was held if that
    /// is ahead of the real one: it never goes back.
    pub fn release_clock(&mut self) {
        let state = &mut self.app.state;
        if let Some(held) = state.held_clock.take() {
            let real = Duration::from(state.clock.now());
            state.clock_ahead = held.saturating_sub(real);
        }
    }

    /// The held clock moved on by `by`, and a frame drawn at that time, as
    /// a screen refreshing: the motions under way are drawn as they are
    /// then. Holds the clock first if it was not.
    pub fn step(&mut self, by: Duration) -> anyhow::Result<()> {
        self.hold_clock();
        let state = &mut self.app.state;
        let at = state.held_clock.map(|t| t + by);
        state.held_clock = at;
        // What falls due on the way: the idle countdown runs on this
        // clock, its timer on the real one.
        state.idle_check();
        // The curtain, the lock after it and the screens off (7.2) too.
        state.away_check();
        // The lock's wait counts on this clock, its timer on the real one.
        state.lock_wait_tick();
        // So do the session menu's countdown and its windows' 5 s (7.4).
        state.leave_tick();
        // And a screen's mode tried from the Settings (6.9).
        state.trial_tick();
        // And the halo's reminder of an agent waiting (4.4), and the 8 s
        // of a panda peeking out (4.5).
        state.halo_reminder_due();
        state.peek_due();
        state.redraw = true;
        // Drawn at that time: the frame counts only when something changed
        // on the screen, but each frame drawn notes its time.
        let drawn = self.dispatch_until(Duration::from_secs(30), |hp| {
            hp.app.state.drawn_at.get() == at
        })?;
        anyhow::ensure!(drawn, "no frame drawn at {at:?}");
        Ok(())
    }

    /// Locks the session now, as `Super+L` or the idle will (7.1).
    pub fn lock(&mut self) {
        self.app.state.lock_now();
    }

    /// Where the session is, away from its user: the curtain, the screens
    /// off unlocked (7.2).
    pub fn away(&self) -> crate::away::Shown {
        self.app.state.away()
    }

    /// The curtain's work so far, and the sleeps let go.
    pub fn away_counts(&self) -> crate::away::Counts {
        self.app.state.away_counts()
    }

    /// The screens that are on, by name, in their order.
    pub fn screens_on(&self) -> Vec<String> {
        let state = &self.app.state;
        let mut names: Vec<String> = state
            .space
            .outputs()
            .filter(|o| state.output_powered(o))
            .map(Output::name)
            .collect();
        names.sort();
        names
    }

    /// The dim's opacity on the screen now, if the curtain draws one.
    pub fn curtain_dim(&self) -> Option<f32> {
        let state = &self.app.state;
        let since = state.curtain_since()?;
        let (dim, _) = state.curtain_looks(since, None, state.now());
        Some(dim)
    }

    /// The pill's surfaces the compositor takes as the shell's and draws,
    /// by screen.
    pub fn curtain_surfaces(&self) -> Vec<(String, Rect)> {
        let state = &self.app.state;
        let Some((pill, _)) = state.curtain_pill() else {
            return Vec::new();
        };
        state
            .space
            .outputs()
            .filter_map(|output| {
                let layer = state.curtain_layer(output, pill)?;
                let origin = state.space.output_geometry(output)?.loc;
                let g = smithay::desktop::layer_map_for_output(output).layer_geometry(&layer)?;
                Some((
                    output.name(),
                    Rect {
                        x: origin.x + g.loc.x,
                        y: origin.y + g.loc.y,
                        width: g.size.w,
                        height: g.size.h,
                    },
                ))
            })
            .collect()
    }

    /// The lid closes, or opens (a laptop's switch).
    pub fn lid(&mut self, closed: bool) {
        self.app.state.lid(closed);
    }

    /// The session back from another terminal, as the tty backend says
    /// it (what it does beyond the screens).
    pub fn session_back(&mut self) {
        self.app.state.session_back();
    }

    /// Output `name` is a laptop's own screen: the lid closes over it.
    pub fn set_internal(&mut self, name: &str) {
        if let Some(output) = self.app.state.space.outputs().find(|o| o.name() == name) {
            output
                .user_data()
                .insert_if_missing(|| crate::away::Internal);
        }
    }

    /// Whether logind's sleep inhibitor is held.
    pub fn sleep_inhibited(&self) -> bool {
        self.app.state.sleep_inhibited()
    }

    /// Whether UPower says the machine is on battery.
    pub fn on_battery(&self) -> bool {
        self.app.state.on_battery()
    }

    /// Where the lock is.
    pub fn lock_state(&self) -> crate::LockState {
        self.app.state.lock_state()
    }

    /// Whether every screen shows the lock, its fade over.
    pub fn lock_drawn(&self) -> bool {
        self.app.state.lock_drawn()
    }

    /// What the shell was last told of the lock.
    pub fn lock_shown(&self) -> Option<hocus_link::LockShown> {
        self.app.state.lock.told().cloned()
    }

    /// How many characters the lock's field holds (never what they are).
    pub fn lock_typed(&self) -> Option<usize> {
        self.app.state.lock_typed()
    }

    /// Commits of the shell's lock surfaces so far, and the passwords
    /// checked.
    pub fn lock_commits(&self) -> u64 {
        self.app.state.lock.commits
    }

    pub fn lock_checks(&self) -> u64 {
        self.app.state.lock.checks
    }

    /// The shell's lock surfaces' places, by screen: those the compositor
    /// takes as the shell's (of its own connection) and has drawn.
    pub fn lock_surfaces(&self) -> Vec<(String, Rect)> {
        let state = &self.app.state;
        state
            .space
            .outputs()
            .filter_map(|output| {
                let layer = state.shell_lock_layer(output)?;
                let origin = state.space.output_geometry(output)?.loc;
                let g = smithay::desktop::layer_map_for_output(output).layer_geometry(&layer)?;
                Some((
                    output.name(),
                    Rect {
                        x: origin.x + g.loc.x,
                        y: origin.y + g.loc.y,
                        width: g.size.w,
                        height: g.size.h,
                    },
                ))
            })
            .collect()
    }

    /// Whether an ext-session-lock client holds the lock.
    pub fn lock_external(&self) -> bool {
        self.app
            .state
            .lock
            .locked_state()
            .is_some_and(|l| matches!(l.holder, crate::lock::Holder::External(_)))
    }

    /// The screens where the shell's lock has drawn since it locked (its
    /// fade started there).
    pub fn lock_drawn_on(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .app
            .state
            .lock
            .locked_state()
            .map(|l| l.shown_at.keys().cloned().collect())
            .unwrap_or_default();
        names.sort();
        names
    }

    /// The shell's process, as the compositor started it.
    pub fn shell_pid(&self) -> Option<i32> {
        // Not its connection's credentials: the compositor made that
        // connection, they are its own.
        let pid = self.app.state.shell.running.pid()?;
        i32::try_from(pid).ok()
    }

    /// Ends the shell at once (`SIGKILL`), as a crash would: the process
    /// of its Wayland connection, which the test started.
    pub fn kill_shell(&self) -> anyhow::Result<()> {
        use rustix::process::{Pid, Signal, kill_process};
        let pid = self
            .shell_pid()
            .filter(|pid| u32::try_from(*pid).ok() != Some(std::process::id()))
            .and_then(Pid::from_raw)
            .context("no shell")?;
        kill_process(pid, Signal::KILL).with_context(|| format!("killing the shell ({pid:?})"))?;
        Ok(())
    }

    /// Whether strokes show on the screens, and whether the laser does
    /// (docs/design/ink.md).
    pub fn ink(&self) -> (bool, bool) {
        let ink = &self.app.state.ink;
        (ink.shows(), ink.laser_shown())
    }

    /// The session's idleness now, as `hocus msg idle` says it (7.3).
    pub fn idle(&self) -> crate::idle::Said {
        self.app.state.idle_said()
    }

    /// The idle timer's work so far.
    pub fn idle_counts(&self) -> crate::idle::Counts {
        self.app.state.idle_counts()
    }

    /// When a frame is asked for after a pause, if one is: a wakeup to
    /// come.
    pub fn wake(&self) -> Option<Duration> {
        self.app.state.wake
    }

    /// Whether nothing moved as of the last frame drawn (`tick`'s word).
    pub fn still(&self) -> bool {
        self.app.state.still
    }

    /// The motions' time now (held, or running).
    pub fn now(&self) -> Duration {
        self.app.state.now()
    }

    /// Whether the last frame drawn shows everything at rest: no motion
    /// under way as of that frame, no frame asked for since, and none due
    /// by now (a motion that starts after a pause, as Alt+Tab's panel
    /// does, wakes the loop by a timer, which a loaded machine runs late).
    /// A frame is only drawn after the loop turns, so on a loaded machine
    /// it can come well after a motion's end in time: read pixels once
    /// this holds.
    pub fn at_rest(&self) -> bool {
        let state = &self.app.state;
        state.still
            && !state.redraw
            && state.drawn_at.get().is_some()
            && state.wake.is_none_or(|at| at > state.now())
    }

    /// The layer surfaces of `namespace` that show what was asked of them:
    /// a buffer of their place's size. A layer surface has its place from
    /// its first commit, before any buffer: until it has one it shows
    /// nothing and takes no input, and on a loaded machine the shell's
    /// first buffer can come long after.
    pub fn drawn_layers(&self, namespace: &str) -> Vec<(String, Rect)> {
        use smithay::backend::renderer::utils::with_renderer_surface_state;
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
                let Some(g) = map.layer_geometry(layer) else {
                    continue;
                };
                let size =
                    with_renderer_surface_state(layer.wl_surface(), |s| s.surface_size()).flatten();
                if size != Some(g.size) {
                    continue;
                }
                let rect = Rect::new(origin.x + g.loc.x, origin.y + g.loc.y, g.size.w, g.size.h);
                surfaces.push((output.name(), rect));
            }
        }
        surfaces
    }

    /// The namespaces of the layer surfaces placed over the point (`x`,
    /// `y`) that do not show what was asked of them yet: no buffer, or one
    /// of another size than their place (a configure not drawn yet). A
    /// press there now goes to whatever is under them, or lands on a part
    /// of theirs that moves once their buffer comes.
    pub fn undrawn_at(&self, x: f64, y: f64) -> Vec<String> {
        use smithay::backend::renderer::utils::with_renderer_surface_state;
        let state = &self.app.state;
        let mut undrawn = Vec::new();
        for output in state.space.outputs() {
            let Some(origin) = state.space.output_geometry(output).map(|g| g.loc) else {
                continue;
            };
            let map = smithay::desktop::layer_map_for_output(output);
            for layer in map.layers() {
                let Some(g) = map.layer_geometry(layer) else {
                    continue;
                };
                let (left, top) = (f64::from(origin.x + g.loc.x), f64::from(origin.y + g.loc.y));
                let over = x >= left
                    && y >= top
                    && x < left + f64::from(g.size.w)
                    && y < top + f64::from(g.size.h);
                if !over {
                    continue;
                }
                let size =
                    with_renderer_surface_state(layer.wl_surface(), |s| s.surface_size()).flatten();
                if size != Some(g.size) {
                    undrawn.push(layer.namespace().to_owned());
                }
            }
        }
        undrawn
    }

    /// The layer surfaces of `namespace`: their screen, and where they
    /// are (global).
    pub fn layer_rects(&self, namespace: &str) -> Vec<(String, Rect)> {
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

    /// Holds the shell still (`SIGSTOP`), or lets it go on (`SIGCONT`):
    /// what the compositor sends it meanwhile is read at once as it goes
    /// on, as by a shell busy for a while. The shell is the client that
    /// drew the bars' surfaces, which the test started.
    pub fn hold_shell(&self, held: bool) -> anyhow::Result<()> {
        use rustix::process::{Pid, Signal, kill_process};
        let pid = self
            .shell_pid()
            .filter(|pid| u32::try_from(*pid).ok() != Some(std::process::id()))
            .and_then(Pid::from_raw)
            .context("no shell")?;
        let signal = if held { Signal::STOP } else { Signal::CONT };
        kill_process(pid, signal).with_context(|| format!("signalling the shell ({pid:?})"))?;
        Ok(())
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
        self.app.state.pointer_placed = true;
        let from = self
            .app
            .state
            .seat
            .get_pointer()
            .map(|p| p.current_location())
            .unwrap_or_default();
        let by = Point::from((x, y)) - from;
        if !self.app.state.felt(crate::away::Felt::Motion(by)) {
            return;
        }
        self.time += 10;
        let time = self.time;
        self.app.state.pointer_motion(Point::from((x, y)), time);
    }

    /// Moves the pointer by (`dx`, `dy`), as a mouse does: it stays on the
    /// outputs, sliding onto a screen beyond an edge no screen touches.
    pub fn pointer_by(&mut self, dx: f64, dy: f64) {
        let state = &self.app.state;
        let from = state
            .seat
            .get_pointer()
            .map(|p| p.current_location())
            .unwrap_or_default();
        let to = state.onto_outputs(from, from + Point::from((dx, dy)));
        self.pointer_move(to.x, to.y);
    }

    pub fn button(&mut self, button: u32, pressed: bool) {
        use crate::away::{Felt, Pressed};
        let felt = if pressed {
            Felt::Press(Pressed::Button(button))
        } else {
            Felt::Release(Pressed::Button(button))
        };
        if !self.app.state.felt(felt) {
            return;
        }
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

    /// The keyboard's repeat as the compositor last gave it to clients:
    /// (rate, delay).
    pub fn keyboard_repeat(&self) -> (i32, i32) {
        let keyboard = &self.app.state.config.keyboard;
        (keyboard.repeat_rate, keyboard.repeat_delay)
    }

    /// Num Lock is locked on the seat's keyboard.
    pub fn num_lock(&self) -> bool {
        self.app
            .state
            .seat
            .get_keyboard()
            .is_some_and(|k| k.modifier_state().num_lock)
    }

    /// The window holding the keyboard takes every key, the desktop's
    /// shortcuts with them (keyboard-shortcuts-inhibit).
    pub fn shortcuts_inhibited(&self) -> bool {
        self.app.state.shortcuts_inhibited()
    }

    /// A mouse or touchpad plugged in, as libinput would (6.12): given
    /// `[pointer]` or `[touchpad]` at once; its number.
    pub fn plug_device(&mut self, device: crate::input_devices::FakeDevice) -> u64 {
        self.app.state.fake_device_added(device)
    }

    pub fn unplug_device(&mut self, id: u64) {
        self.app.state.fake_device_removed(id);
    }

    /// The mice and touchpads, and what each was given.
    pub fn input_devices(&self) -> Vec<hocus_link::input::InputDevice> {
        self.app.state.input_devices.list()
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
        if !self.app.state.felt(crate::away::Felt::Other) {
            return;
        }
        self.time += 10;
        let mut frame = AxisFrame::new(self.time)
            .source(source)
            .value(Axis::Vertical, value);
        if let Some(v120) = v120 {
            frame = frame.v120(Axis::Vertical, v120);
        }
        self.app.state.pointer_axis(frame);
    }

    /// A key by its evdev code.
    pub fn key(&mut self, code: u32, pressed: bool) {
        use crate::away::{Felt, Pressed};
        // XKB keycodes are evdev's plus 8.
        let felt = if pressed {
            Felt::Press(Pressed::Key(code + 8))
        } else {
            Felt::Release(Pressed::Key(code + 8))
        };
        if !self.app.state.felt(felt) {
            return;
        }
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

    /// Each output's desktops, by name: how many, and the one it shows
    /// (from 0).
    pub fn strips(&self) -> Vec<(String, usize, usize)> {
        let state = &self.app.state;
        state
            .space
            .outputs()
            .map(|o| {
                let strip = state.strip(o);
                (o.name(), strip.count(), strip.current())
            })
            .collect()
    }

    /// The desktops' names as the compositor has them (6.14).
    pub fn desktop_names(&self) -> Vec<String> {
        self.app.state.config.desktops.names.clone()
    }

    /// The desktop the output under the pointer shows (from 0).
    pub fn desktop_under_pointer(&self) -> usize {
        let state = &self.app.state;
        state
            .output_under_pointer()
            .map_or(0, |o| state.strip(&o).current())
    }

    /// A window's radial menu while it is open (#723): its window, its
    /// centre, the sector lit, whether Close is armed.
    pub fn radial(&self) -> Option<crate::radial::Seen> {
        let state = &self.app.state;
        let radial = state.radial.as_ref()?;
        Some(crate::radial::Seen {
            window: radial.window,
            centre: radial.centre,
            lit: radial.lit,
            armed: radial.armed(state.now()),
            enabled: radial.enabled,
            on: radial.on,
        })
    }

    /// The overview (8.5 A): `Some(true)` open, `Some(false)` closing,
    /// `None` gone.
    pub fn overview(&self) -> Option<bool> {
        self.app.state.overview.as_ref().map(|o| o.open())
    }

    /// The overview's thumbnails: each screen's and desktop's (from 0),
    /// and its middle, global.
    pub fn overview_thumbs(&self) -> Vec<(String, usize, (f64, f64))> {
        self.app
            .state
            .overview_screens()
            .into_iter()
            .flat_map(|s| {
                let name = s.output.name();
                s.thumbs
                    .into_iter()
                    .enumerate()
                    .map(move |(d, t)| (name.clone(), d, t.centre()))
            })
            .collect()
    }

    /// The windows the overview lays out on the screens' shown desktops:
    /// each one's app id and its place (x, y, width, height), global.
    pub fn overview_windows(&self) -> Vec<OverviewWindow> {
        let state = &self.app.state;
        state
            .overview_slots_all()
            .into_iter()
            .map(|(id, a)| {
                let app = state
                    .windows
                    .get(&id)
                    .and_then(|m| crate::windows::app_id(&m.window));
                (app, (a.x, a.y, a.w, a.h))
            })
            .collect()
    }

    /// The overview's agents' badges: each window's app id, the state
    /// its badge shows and how many agents are in it.
    pub fn overview_badges(&self) -> Vec<(Option<String>, String, usize)> {
        let state = &self.app.state;
        let Some(overview) = &state.overview else {
            return Vec::new();
        };
        let mut out: Vec<_> = overview
            .badges
            .iter()
            .map(|(id, b)| {
                let app = state
                    .windows
                    .get(id)
                    .and_then(|m| crate::windows::app_id(&m.window));
                (app, format!("{:?}", b.state).to_lowercase(), b.count)
            })
            .collect();
        out.sort();
        out
    }

    /// The overview's thumbnails where a panda sleeps: their screen and
    /// desktop (from 0).
    pub fn overview_asleep(&self) -> Vec<(String, usize)> {
        self.app.state.overview_asleep()
    }

    /// The overview's selected window's app id, when its ring shows.
    pub fn overview_selected(&self) -> Option<String> {
        let state = &self.app.state;
        let overview = state.overview.as_ref().filter(|o| o.ring)?;
        let id = overview.selected?;
        state
            .windows
            .get(&id)
            .and_then(|m| crate::windows::app_id(&m.window))
    }

    /// The window dragged in the overview: its app id, and the thumbnail
    /// it is over (screen, desktop).
    pub fn overview_drag(&self) -> Option<OverviewDrag> {
        let state = &self.app.state;
        let drag = state.overview.as_ref()?.drag.as_ref()?;
        let app = state
            .windows
            .get(&drag.window)
            .and_then(|m| crate::windows::app_id(&m.window));
        Some((app, drag.over.clone()))
    }

    /// Whether the radial menu is still drawn going.
    pub fn radial_going(&self) -> bool {
        self.app.state.radial_out.is_some()
    }

    /// The windows on all desktops (#723).
    pub fn everywhere(&self) -> Vec<WindowId> {
        self.app.state.model.everywhere().collect()
    }

    /// Whether window `id` is kept above the others.
    pub fn above(&self, id: WindowId) -> bool {
        self.app.state.model.is_above(id)
    }

    /// The stack, bottom first.
    pub fn stack(&self) -> Vec<WindowId> {
        self.app.state.model.stack().to_vec()
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
        self.swipe_begin(fingers);
        for &(dx, dy) in moves {
            self.swipe_move(dx, dy, every);
        }
        self.swipe_end(every, false);
    }

    /// `fingers` come down on the touchpad.
    pub fn swipe_begin(&mut self, fingers: u32) {
        use crate::input::Gesture;
        self.time += 10;
        let time = self.time;
        self.app
            .state
            .gesture(Gesture::SwipeBegin { time, fingers });
    }

    /// The fingers down move by (`dx`, `dy`) (libinput's units), `after`
    /// milliseconds after their last event.
    pub fn swipe_move(&mut self, dx: f64, dy: f64, after: u32) {
        use crate::input::Gesture;
        self.time += after;
        let time = self.time;
        self.app.state.gesture(Gesture::SwipeUpdate {
            time,
            delta: (dx, dy).into(),
        });
    }

    /// The fingers lift, `after` milliseconds after their last event.
    pub fn swipe_end(&mut self, after: u32, cancelled: bool) {
        use crate::input::Gesture;
        self.time += after;
        let time = self.time;
        self.app
            .state
            .gesture(Gesture::SwipeEnd { time, cancelled });
    }

    /// Swipes given to clients so far (the desktop took none of them), and
    /// their moves (6.13).
    pub fn swipes_given(&self) -> (u64, u64) {
        self.app.state.swipes_given
    }

    /// How the launcher shows now, if it shows: its opacity and its
    /// panel's scale (6.13).
    pub fn launcher_look(&self) -> Option<(f32, f64)> {
        let state = &self.app.state;
        let now = state.now();
        let motion = state.launcher_motion();
        state.space.outputs().find_map(|output| {
            let layer = state.launcher_on(output)?;
            let look = state.panel.launchers.look(&layer, now, motion)?;
            Some((look.alpha, look.scale))
        })
    }

    /// Whether four fingers hold the launcher (6.13).
    pub fn launcher_held(&self) -> bool {
        self.app.state.panel.launchers.held()
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

    /// What `hocus msg outputs` says, as one line of JSON.
    pub fn outputs_said(&self) -> String {
        self.app.state.displays_json().to_string()
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
    /// is (its input region counts), if one does: none where a window
    /// over it does.
    pub fn pointer_layer(&self) -> Option<String> {
        use crate::handlers::layer_shell::Stand;
        let (x, y) = self.pointer();
        let state = &self.app.state;
        let at = (x, y).into();
        let (_, _, layer) = state.layer_surface_under(at, Stand::Above).or_else(|| {
            state
                .space
                .element_under(at)
                .is_none()
                .then(|| state.layer_surface_under(at, Stand::Below))
                .flatten()
        })?;
        Some(layer.namespace().to_owned())
    }

    /// The app id of the window that takes the pointer where it is, if
    /// one does: none where a layer surface above the windows does.
    pub fn pointer_window(&self) -> Option<String> {
        let (x, y) = self.pointer();
        let state = &self.app.state;
        let id = state.window_under((x, y).into())?;
        self.windows()
            .into_iter()
            .find(|w| w.id == id)
            .and_then(|w| w.app_id)
    }

    /// The screen whose bar is in front of the windows, brought there by
    /// the pointer pushed against its edge (#519).
    pub fn bar_in_front(&self) -> Option<String> {
        let state = &self.app.state;
        state
            .space
            .outputs()
            .find(|o| state.bar_in_front(o))
            .map(smithay::output::Output::name)
    }

    /// Bar `bar`'s desktops' tiles are where `tiles` say on its desktops'
    /// surface, as the shell says them (8.5 B).
    pub fn desk_tiles(&mut self, bar: &str, tiles: Vec<hocus_link::DeskTile>) {
        self.app.state.desk_tiles_said(bar.to_owned(), tiles);
    }

    /// The tile a moved window is over: the bar's screen, the tile's
    /// screen and desktop (from 0).
    pub fn drop_over(&self) -> Option<(String, String, usize)> {
        self.app
            .state
            .tile_drop
            .over
            .as_ref()
            .map(|o| (o.bar.clone(), o.output.clone(), o.desktop))
    }

    /// The bars' tiles as the shell said them, global: the bar's screen,
    /// the tile's screen and desktop (from 0), its middle.
    pub fn desk_tile_middles(&self) -> Vec<(String, String, usize, (f64, f64))> {
        self.app
            .state
            .tile_rects()
            .into_iter()
            .map(|(o, r)| {
                let middle = (r.loc.x + r.size.w / 2.0, r.loc.y + r.size.h / 2.0);
                (o.bar, o.output, o.desktop, middle)
            })
            .collect()
    }

    /// The pointer as a session finds it at its start, never moved by the
    /// user: screens lit from now on bring it to the primary's middle.
    pub fn pointer_unplaced(&mut self) {
        self.app.state.pointer_placed = false;
    }

    /// The window a pointer's move is moving, if any.
    pub fn moving(&self) -> Option<crate::model::WindowId> {
        self.app.state.moving
    }

    /// The moved window's share of its size as drawn now (8.5 B).
    pub fn moved_scale(&self) -> f64 {
        let state = &self.app.state;
        state.tile_drop.scale(state.now())
    }

    /// The next meeting is `near`, as the calendar will say: the halo
    /// lights without an agent.
    pub fn meeting_near(&mut self, near: Option<crate::halo::Meeting>) {
        self.app.state.meeting_near(near);
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
            .plug(name, None, None, state)
    }

    /// A screen of `spec`, who `identity` says (its EDID's make, model and
    /// serial; none: known by its connector), plugged on connector `name`:
    /// one unplugged before, in its place, or a new one, right of the
    /// others.
    pub fn plug_screen(
        &mut self,
        name: &str,
        spec: crate::OutputSpec,
        identity: Option<crate::screen_sets::Identity>,
    ) -> anyhow::Result<()> {
        let App { state, backend } = &mut self.app;
        backend.headless_mut().context("not headless")?.plug(
            name,
            None,
            Some((spec, identity)),
            state,
        )
    }

    /// The key screen `name` is known by (its identity's, windows.md §12).
    pub fn screen_key(&self, name: &str) -> String {
        self.app.state.screen_key(name)
    }

    /// The set of screens in use: their keys, sorted.
    pub fn screen_set(&self) -> Vec<String> {
        self.app.state.current_set()
    }

    /// The sets as the Settings are told them (6.9).
    pub fn screen_sets_said(&self) -> serde_json::Value {
        self.app.state.sets_json()
    }

    /// Whether the winking panda saying screens moved is out.
    pub fn screens_peek_out(&self) -> bool {
        self.app.state.peek.screens_out()
    }

    /// Whether the panda out goes back.
    pub fn peek_going_back(&self) -> bool {
        self.app.state.peek.going_back()
    }

    /// The Settings forget the set of `screens` (by their keys), as
    /// "Oublier cet ensemble" does (6.9).
    pub fn forget_screen_set(&mut self, screens: &[&str]) -> Result<(), String> {
        let keys: Vec<String> = screens.iter().map(|s| (*s).to_owned()).collect();
        self.app
            .state
            .forget_screen_set(&crate::screen_sets::set_name(&keys))
    }

    /// Output `name`, unplugged before, comes back with its top-left
    /// corner at (`x`, `y`).
    pub fn plug_at(&mut self, name: &str, x: i32, y: i32) -> anyhow::Result<()> {
        let App { state, backend } = &mut self.app;
        backend
            .headless_mut()
            .context("not headless")?
            .plug(name, Some((x, y)), None, state)
    }

    /// Why the compositor was asked to end, if it was.
    pub fn ending(&self) -> Option<crate::Ending> {
        self.app.state.ending
    }

    /// The session menu's confirmation under way, as the shell is told
    /// it (7.4).
    pub fn leave_shown(&self) -> Option<hocus_link::LeaveShown> {
        self.app.state.leave_shown_now()
    }

    /// Whether the backend made the linux-dmabuf global.
    pub fn has_dmabuf(&self) -> bool {
        self.app.state.dmabuf_global.is_some()
    }
}
