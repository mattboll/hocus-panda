//! A headless compositor driven from the same process, on a virtual clock:
//! what scenario tests run on. Frames happen only when the harness advances
//! time, so what a test sees does not depend on the machine's speed.

use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::Context;
use hocus_api::{ClientError, Event, Permission, Reply, Request, RequestLine, ServerLine};
use smithay::reexports::calloop::EventLoop;

use crate::{
    App, Backend, BackendState, Clock, HeadlessConfig, Options, RuntimeDir, Started, State,
    WatchdogConfig,
    headless::{FRAME, HeadlessBackend, OutputInfo, RendererKind},
};

/// How to start a [`Headless`] compositor.
#[derive(Debug, Clone)]
pub struct HarnessOptions {
    pub headless: HeadlessConfig,
    pub api_permission: Permission,
    /// The watchdog runs on real time: the harness must keep dispatching
    /// while it is on, or it takes the waits between steps for stalls.
    pub watchdog: Option<WatchdogConfig>,
    /// Starts XWayland, for X11 test clients.
    pub xwayland: bool,
    /// Shows the widget column.
    pub shell: bool,
    /// Serves notifications on this (private, test) bus.
    pub session_bus: Option<String>,
    /// Reads battery, network… from services on this (private, test) bus;
    /// without it, tests use a stub and never the machine's.
    pub system_bus: Option<String>,
    /// Where the launcher and the dock read desktop entries; none: no
    /// applications, never the machine's.
    pub data_dirs: Option<Vec<std::path::PathBuf>>,
    /// What started applications inherit.
    pub child_env: Option<Vec<(std::ffi::OsString, std::ffi::OsString)>>,
    /// A stand-in for polkit's helper, with a system bus (#83).
    pub polkit_helper: Option<std::path::PathBuf>,
    /// The first session's welcome opens (#82); otherwise the state
    /// directory says it was already offered.
    pub first_run: bool,
    /// Windows move as in a session (#33); otherwise they appear and go
    /// at once, so that a test that does not sample motion never catches
    /// one halfway.
    pub motion: bool,
    /// Google's CalDAV is this stand-in's (#137). Without one, tests never
    /// sync Google accounts: never Google itself.
    pub google_caldav: Option<String>,
    /// Google Tasks' API is this stand-in's (#257); without one, tests
    /// never read Google accounts' tasks.
    pub google_tasks: Option<String>,
}

/// Starts a compositor for `harness`, its configuration file at `config`
/// and its state beside it.
fn launch(harness: &HarnessOptions, config: &Path, clock: Clock) -> anyhow::Result<Started> {
    let state_dir = config.with_file_name("state");
    if !harness.first_run {
        // As after the first session: the welcome was offered.
        std::fs::create_dir_all(&state_dir)?;
        std::fs::write(state_dir.join("welcomed"), b"1\n")?;
    }
    let options = Options {
        backend: Backend::Headless(harness.headless.clone()),
        runtime_dir: None,
        spawn: Vec::new(),
        api_permission: harness.api_permission,
        watchdog: harness.watchdog.clone(),
        config: Some(config.to_path_buf()),
        config_poll: Duration::from_millis(20),
        xwayland: harness.xwayland,
        shell: harness.shell,
        session_bus: harness.session_bus.clone(),
        // A test gives its own (`set_a11y_bus`): never the session's.
        a11y_bus: None,
        system_bus: harness.system_bus.clone(),
        session_services: false,
        // Private: tests never touch the developer's saved session.
        state_dir: Some(state_dir),
        data_dirs: Some(harness.data_dirs.clone().unwrap_or_default()),
        child_env: harness.child_env.clone(),
        polkit_helper: harness.polkit_helper.clone(),
        // Tests give their own copy (`set_xkb_registry`): never the
        // machine's.
        xkb_registry: hocus_platform::xkb::Registry::Off,
    };
    let mut started = crate::start(&options, clock)?;
    started.app.state.instant_motion = !harness.motion;
    // Before the first dispatch: no sync has started yet.
    started
        .app
        .state
        .set_google_caldav(harness.google_caldav.clone());
    started
        .app
        .state
        .set_google_tasks(harness.google_tasks.clone());
    started
        .app
        .state
        .fix_pairing_code(hocus_platform::adb::Code {
            name: "hocuspanda-test".into(),
            password: "pandapassword".into(),
        });
    Ok(started)
}

/// A headless compositor in a private runtime directory, on a virtual clock.
#[derive(Debug)]
pub struct Headless {
    event_loop: EventLoop<'static, App>,
    app: App,
    runtime_dir: RuntimeDir,
    /// The harness's own control connection, opened on first use.
    control: Option<Control>,
    /// The compositor's configuration file, in a directory of its own.
    config: std::path::PathBuf,
    _config_dir: tempfile::TempDir,
    /// What it was started with, to start again.
    harness: HarnessOptions,
    /// The clock it started on, as it was then.
    clock: Clock,
    /// The copy of XKB's data the layouts are named after.
    xkb_registry: Option<crate::XkbRegistry>,
}

/// A control connection read without blocking, so that the harness can run
/// the compositor while it waits for a reply.
#[derive(Debug)]
struct Control {
    stream: UnixStream,
    input: Vec<u8>,
    next_id: u64,
    events: VecDeque<Event>,
}

impl Control {
    fn connect(path: &Path) -> io::Result<Self> {
        let stream = UnixStream::connect(path)?;
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            input: Vec::new(),
            next_id: 1,
            events: VecDeque::new(),
        })
    }

    /// The next complete line, if one arrived.
    fn line(&mut self) -> Result<Option<ServerLine>, ClientError> {
        loop {
            if let Some(end) = self.input.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.input.drain(..=end).collect();
                return serde_json::from_slice(&line).map(Some).map_err(|_| {
                    ClientError::Protocol(String::from_utf8_lossy(&line).trim_end().into())
                });
            }
            let mut buf = [0u8; 64 * 1024];
            match self.stream.read(&mut buf) {
                Ok(0) => return Err(ClientError::Closed),
                Ok(n) => self.input.extend_from_slice(&buf[..n]),
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) => return Err(err.into()),
            }
        }
    }
}

/// How long the harness waits for a reply to its own request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

impl Headless {
    /// Starts with the test permission on the control socket.
    pub fn start(config: HeadlessConfig) -> anyhow::Result<Self> {
        Self::start_with(config, Permission::Test)
    }

    pub fn start_with(config: HeadlessConfig, api_permission: Permission) -> anyhow::Result<Self> {
        Self::start_options(HarnessOptions {
            headless: config,
            api_permission,
            watchdog: None,
            xwayland: false,
            shell: false,
            session_bus: None,
            system_bus: None,
            data_dirs: None,
            child_env: None,
            polkit_helper: None,
            motion: false,
            first_run: false,
            google_caldav: None,
            google_tasks: None,
        })
    }

    pub fn start_options(harness: HarnessOptions) -> anyhow::Result<Self> {
        // The configuration lives in the private runtime directory, which
        // exists once the compositor started: watched from the start, read
        // when a test writes it.
        let config_dir = tempfile::Builder::new().prefix("hocus-config-").tempdir()?;
        let clock = Clock::Virtual {
            now: Duration::ZERO,
        };
        Self::start_in(harness, config_dir, clock)
    }

    /// The compositor goes as a crash would (nothing saved, nothing
    /// unlocked) and a new one starts on the same configuration and state.
    pub fn restart(&mut self) -> anyhow::Result<()> {
        self.control = None;
        let Started {
            event_loop,
            app,
            runtime_dir,
        } = crate::allocations::counting(|| launch(&self.harness, &self.config, self.clock))?;
        let old = (
            std::mem::replace(&mut self.app, app),
            std::mem::replace(&mut self.event_loop, event_loop),
            std::mem::replace(&mut self.runtime_dir, runtime_dir),
        );
        crate::allocations::counting(|| drop(old));
        if let Some(registry) = self.xkb_registry.clone() {
            self.app.state.set_xkb_registry(registry);
        }
        self.ready()
    }

    /// Starts on the system's monotonic clock, as a session runs: timers
    /// wake the loop on real time and frames are drawn when they fall
    /// due, so the test must keep dispatching. For measuring what wakes
    /// the loop (#88); [`Headless::advance`] refuses.
    pub fn start_real_time(harness: HarnessOptions) -> anyhow::Result<Self> {
        let config_dir = tempfile::Builder::new().prefix("hocus-config-").tempdir()?;
        Self::start_in(harness, config_dir, Clock::monotonic())
    }

    fn start_in(
        harness: HarnessOptions,
        config_dir: tempfile::TempDir,
        clock: Clock,
    ) -> anyhow::Result<Self> {
        let config = config_dir.path().join("config.toml");
        let Started {
            event_loop,
            app,
            runtime_dir,
        } = crate::allocations::counting(|| launch(&harness, &config, clock))?;
        let mut headless = Self {
            event_loop,
            app,
            runtime_dir,
            control: None,
            config,
            _config_dir: config_dir,
            harness,
            clock,
            xkb_registry: None,
        };
        headless.ready()?;
        Ok(headless)
    }

    /// The defaults are read before the first test step, and XWayland is
    /// ready when asked for.
    fn ready(&mut self) -> anyhow::Result<()> {
        self.dispatch_until(REQUEST_TIMEOUT, |h| h.app.state.config_generation > 0)?;
        if self.harness.xwayland {
            self.dispatch_until(REQUEST_TIMEOUT, |h| h.app.state.xdisplay.is_some())?;
        }
        Ok(())
    }

    /// The Wayland socket, to connect clients with `WAYLAND_DISPLAY`.
    pub fn socket_path(&self) -> &Path {
        &self.app.state.socket_path
    }

    /// Whether every screen shows the wallpaper it asked for: pictures are
    /// made off the loop.
    pub fn wallpapers_ready(&self) -> bool {
        self.app.state.wallpapers_ready()
    }

    /// Whether the applications read at start off the loop came in, when
    /// the shell reads them: the dock holds them from then on (#287).
    pub fn apps_read(&self) -> bool {
        self.app.state.launcher.is_none() || self.app.state.dock.entries_read
    }

    /// XWayland's display number, when it runs.
    pub fn x11_display(&self) -> Option<u32> {
        self.app.state.xdisplay
    }

    /// The control socket.
    pub fn api_socket(&self) -> &Path {
        self.app.state.api.path()
    }

    /// Reads applications' accessibility trees (#175) on this (private,
    /// test) bus from now on.
    pub fn set_a11y_bus(&mut self, address: &str) {
        self.app.state.set_a11y_bus(address.to_owned());
    }

    /// Keyboard layouts are named after this copy of XKB's data
    /// (`rules/evdev.xml`, `locale/`), never the machine's; again after a
    /// restart.
    pub fn set_xkb_registry(&mut self, dir: &Path) {
        let registry = crate::XkbRegistry::At(dir.to_owned());
        self.xkb_registry = Some(registry.clone());
        self.app.state.set_xkb_registry(registry);
    }

    /// The user's folders (pictures, videos) are named in this file, in
    /// place of `$XDG_CONFIG_HOME/user-dirs.dirs`: a test's own, never the
    /// machine's.
    pub fn set_user_dirs(&mut self, path: &Path) {
        self.app.state.user_dirs = Some(path.to_owned());
    }

    /// Each program the compositor starts waits on its thread for `gate`
    /// to return, in place of a slow `PATH` (a network home, an
    /// automount): the loop must not wait with it (#285).
    pub fn set_program_gate(&mut self, gate: impl Fn(&str) + Send + Sync + 'static) {
        self.app.state.set_program_gate(std::sync::Arc::new(gate));
    }

    /// How many programs were asked for and have not yet started (or
    /// failed to).
    pub fn programs_starting(&self) -> usize {
        self.app.state.programs_starting()
    }

    /// Where the session is saved.
    pub fn state_dir(&self) -> std::path::PathBuf {
        self.config.with_file_name("state")
    }

    pub fn runtime_dir(&self) -> &Path {
        self.runtime_dir.path()
    }

    pub fn renderer(&self) -> Option<RendererKind> {
        self.backend().map(HeadlessBackend::renderer)
    }

    pub fn outputs(&self) -> Vec<OutputInfo> {
        self.backend()
            .map(HeadlessBackend::outputs)
            .unwrap_or_default()
    }

    /// What `output` shows, as the frames drawn so far left its buffer, and
    /// the same moment drawn afresh in full: RGBA both. They differ where a
    /// frame did not repaint what changed.
    pub fn shown_and_redrawn(&mut self, output: &str) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
        let App { state, backend } = &mut self.app;
        let BackendState::Headless(headless) = backend else {
            anyhow::bail!("not headless");
        };
        let output = state
            .space
            .outputs()
            .find(|o| o.name() == output)
            .cloned()
            .with_context(|| format!("no output named {output}"))?;
        let shown = headless.shown(&output)?;
        let redrawn = headless.capture_output(&output, state, true)?;
        Ok((shown.rgba, redrawn.rgba))
    }

    /// Time on the virtual clock since the start.
    pub fn now(&self) -> Duration {
        self.app.state.clock.now()
    }

    /// Whether a tool shares the screen, as the compositor sees it.
    pub fn sharing(&self) -> bool {
        self.app.state.sharing()
    }

    /// Windows shown on the outputs.
    pub fn window_count(&self) -> usize {
        self.app.state.space.elements().count()
    }

    /// The clipboard selections seen, and whether their reads have ended.
    pub fn clipboard_quiet(&self) -> (u64, bool) {
        self.app.state.clipboard_quiet()
    }

    /// Handles what clients sent, waiting up to `timeout` of real time for
    /// something to arrive. The virtual clock does not move.
    pub fn dispatch(&mut self, timeout: Duration) -> anyhow::Result<()> {
        crate::allocations::counting(|| {
            self.event_loop.dispatch(Some(timeout), &mut self.app)?;
            self.app.state.refresh_shell();
            self.app.serve_captures();
            self.app.serve_agent_captures();
            self.app.serve_screenshots();
            self.app.serve_circle();
            self.app.serve_recording();
            self.app.apply_gamma();
            self.app.settle_outputs();
            if !self.app.state.clock.is_virtual() {
                // On real time, frames fall due as in a session.
                let handle = self.event_loop.handle();
                let App { state, backend } = &mut self.app;
                if let BackendState::Headless(headless) = backend {
                    headless.after_dispatch(&handle, state);
                }
            }
            self.app.state.display_handle.flush_clients()?;
            Ok(())
        })
    }

    /// The stats, as the control API gives them, read without a request
    /// (which would count as one).
    pub fn stats(&self) -> hocus_api::Stats {
        let names: Vec<String> = self.app.state.space.outputs().map(|o| o.name()).collect();
        self.app
            .state
            .measures
            .report(names.iter().map(String::as_str))
    }

    /// Dispatches until `done` holds, for at most `timeout` of real time:
    /// clients run in their own threads or processes.
    pub fn dispatch_until(
        &mut self,
        timeout: Duration,
        mut done: impl FnMut(&Self) -> bool,
    ) -> anyhow::Result<()> {
        let deadline = Instant::now() + timeout;
        while !done(self) {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                anyhow::bail!("condition not met after {timeout:?}");
            }
            self.dispatch(left.min(Duration::from_millis(10)))?;
        }
        Ok(())
    }

    /// How many MCP agents have a seat now (#43).
    pub fn agent_seats(&self) -> usize {
        self.app.state.seat_count()
    }

    /// Actions MCP agents wait on, and whether one waits for its window's
    /// application to take the agent's seat.
    pub fn agent_pending(&self) -> (usize, bool) {
        self.app.state.seat_pending()
    }

    /// Whether every window's application took every agent's seat.
    pub fn agent_seats_taken(&self) -> bool {
        self.app.state.seats_taken()
    }

    /// When an MCP agent's action lands on its own (shown before a click),
    /// and whether one waits for the user's confirmation instead (#43).
    pub fn agent_deadline(&self) -> (Option<Duration>, bool) {
        self.app.state.seat_deadline()
    }

    /// When an agent's capture waiting for its window to draw is taken
    /// anyway (#229).
    pub fn agent_capture_deadline(&self) -> Option<Duration> {
        self.app.state.capture_deadline()
    }

    /// logind's signals the compositor follows (lock, unlock, sleep: 3
    /// once all are), on a system bus given: a signal before is missed.
    pub fn logind_following(&self) -> usize {
        self.app.state.locking.logind_following
    }

    /// Whether the compositor took in that the machine runs on its battery
    /// (UPower's news, off the loop): the battery's delays apply (#265).
    pub fn on_battery(&self) -> bool {
        self.app.state.power_source() == hocus_core::config::PowerSource::Battery
    }

    /// Whether every session save asked is on disk: the file is written
    /// off the loop (#277).
    pub fn session_saved(&self) -> bool {
        self.app.state.session_saved()
    }

    /// Whether the saved session was read, off the loop, and offered
    /// (#277).
    pub fn session_offered(&self) -> bool {
        self.app.state.session_offered()
    }

    /// Whether logind said what the machine can do (suspend, hibernate).
    pub fn power_caps_known(&self) -> bool {
        self.app.state.leaver.caps_known
    }

    /// Idle notifications clients asked for (ext-idle-notify).
    pub fn idle_watchers(&self) -> usize {
        self.app.state.idle_notifications.len()
    }

    /// Activations clients asked with a token (xdg-activation), as the
    /// compositor handled them (#235).
    pub fn activations_asked(&self) -> u64 {
        self.app.state.protocols.activations
    }

    /// AI keys a worker kept or forgot, as the compositor heard (#235).
    pub fn ai_keys_stored(&self) -> u64 {
        self.app.state.ai.stored
    }

    /// herdr's status changes the compositor took in (#41): each comes
    /// off a worker thread in its own time (#235).
    pub fn agent_statuses_seen(&self) -> u64 {
        self.app.state.agents.statuses_seen
    }

    /// Whether every application an agent's wait asked to answer (a
    /// capture's frame callback, the agent's keyboard before typing) did:
    /// it handled what it was sent, in its own time (#235).
    pub fn apps_answered(&self) -> bool {
        self.app.state.apps_answered()
    }

    /// Asks the application with the keyboard to answer once it handled
    /// what it was sent (keys typed, a pointer moved):
    /// [`Headless::apps_answered`] says when it did, however long its
    /// thread took (#292). One that draws before it reads again sends its
    /// answer with the frame it drew of them. Nothing is asked when no
    /// window has the keyboard.
    pub fn ask_keyboard_answer(&mut self) {
        let focused = self.app.state.focused_window();
        if let Some(id) = focused.as_ref().and_then(crate::desktops::model_id) {
            self.app.state.ask_answer(id);
        }
    }

    /// The same for window `id`'s application.
    pub fn ask_window_answer(&mut self, id: u64) {
        self.app.state.ask_answer(hocus_core::WindowId(id));
    }

    /// Whether window `id`'s application drew the size it was last asked
    /// for, in its own time: the clock moves on once it did, or what a
    /// frame shows would depend on how fast its thread ran (#235). Windows
    /// gone and X11 ones count as done.
    pub fn window_caught_up(&self, id: u64) -> bool {
        use smithay::wayland::{compositor::with_states, shell::xdg::XdgToplevelSurfaceData};
        let Some(managed) = self.app.state.windows.get(&hocus_core::WindowId(id)) else {
            return true;
        };
        let Some(toplevel) = managed.window.toplevel() else {
            return true;
        };
        // Whether it acknowledged the last configure, and the size it asked.
        let asked = with_states(toplevel.wl_surface(), |states| {
            let data = states
                .data_map
                .get::<XdgToplevelSurfaceData>()?
                .lock()
                .ok()?;
            let acked = data.pending_configures().is_empty();
            Some((acked, data.last_acked.as_ref().and_then(|s| s.size)))
        });
        match asked {
            Some((false, _)) => false,
            Some((true, Some(size))) => managed.window.geometry().size == size,
            Some((true, None)) | None => true,
        }
    }

    /// Moves the virtual clock forward by `by`, drawing a frame at each frame
    /// boundary it crosses if something changed.
    pub fn advance(&mut self, by: Duration) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.app.state.clock.is_virtual(),
            "the clock is real time: it moves by itself"
        );
        // Applications asked to answer before an agent's wait ends do
        // first, or the wait would end at a moment of their threads (#235).
        self.dispatch_until(REQUEST_TIMEOUT, Self::apps_answered)
            .context("an application never answered before the clock moved")?;
        let end = self.now() + by;
        loop {
            let next = next_frame_after(self.now());
            if next > end {
                break;
            }
            self.dispatch(Duration::ZERO)?;
            self.app.state.clock.advance_to(next);
            crate::allocations::counting(|| {
                // The column's clock follows the virtual time.
                self.app.state.refresh_shell();
                let App { state, backend } = &mut self.app;
                if let BackendState::Headless(headless) = backend {
                    headless.frame(state);
                }
                self.app.state.display_handle.flush_clients()
            })?;
        }
        self.app.state.clock.advance_to(end);
        Ok(())
    }

    /// Sends `request` over the control socket, as any client would, and
    /// runs the compositor until the reply arrives. The virtual clock does not
    /// move.
    pub fn request(&mut self, request: Request) -> Result<Reply, ClientError> {
        let mut control = match self.control.take() {
            Some(control) => control,
            None => Control::connect(self.api_socket())?,
        };
        let result = self.exchange(&mut control, request);
        self.control = Some(control);
        result
    }

    /// Events received since the last call; [`Request::Subscribe`] first.
    pub fn take_events(&mut self) -> Vec<Event> {
        self.control
            .as_mut()
            .map(|c| c.events.drain(..).collect())
            .unwrap_or_default()
    }

    fn exchange(&mut self, control: &mut Control, request: Request) -> Result<Reply, ClientError> {
        let id = control.next_id;
        control.next_id += 1;
        let mut line = serde_json::to_vec(&RequestLine { id, request })
            .map_err(|err| ClientError::Protocol(err.to_string()))?;
        line.push(b'\n');
        // A request line is far smaller than the socket's buffer.
        control.stream.write_all(&line)?;
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        loop {
            while let Some(line) = control.line()? {
                match line {
                    ServerLine::Reply { id: got, ok } if got == id => return Ok(ok),
                    ServerLine::Error {
                        id: Some(got),
                        error,
                    } if got == id => return Err(ClientError::Api(error)),
                    ServerLine::Error { id: None, error } => return Err(ClientError::Api(error)),
                    ServerLine::Event { event } => control.events.push_back(event),
                    ServerLine::Reply { .. } | ServerLine::Error { .. } => {}
                }
            }
            if Instant::now() > deadline {
                return Err(io::Error::from(io::ErrorKind::TimedOut).into());
            }
            self.dispatch(Duration::from_millis(5))
                .map_err(|err| ClientError::Io(io::Error::other(format!("{err:#}"))))?;
        }
    }

    /// Writes the configuration file and runs the compositor until it has
    /// read it; returns the error if it was refused.
    /// The configuration file the compositor reads.
    pub fn config_path(&self) -> &Path {
        &self.config
    }

    pub fn write_config(&mut self, text: &str) -> anyhow::Result<Option<String>> {
        let before = self.app.state.config_generation;
        write_whole(&self.config, text.as_bytes())?;
        self.dispatch_until(REQUEST_TIMEOUT, |h| h.app.state.config_generation > before)?;
        Ok(self.app.state.config_error.clone())
    }

    /// The text of the QR code a phone pairing shows (#153), as a phone
    /// scanning it would read it; `None` while none is shown.
    pub fn pairing_code(&self) -> Option<String> {
        self.app.state.phone_pairing_code()
    }

    /// Runs `handler` on the loop at the next dispatch, timed like the
    /// compositor's own handlers.
    pub fn on_loop(&mut self, name: &'static str, handler: impl FnOnce(&mut State) + 'static) {
        self.event_loop.handle().insert_idle(move |app| {
            app.timed(name, |app| handler(&mut app.state));
        });
    }

    fn backend(&self) -> Option<&HeadlessBackend> {
        match &self.app.backend {
            BackendState::Headless(headless) => Some(headless),
            BackendState::Winit(_) | BackendState::Tty(_) => None,
        }
    }
}

/// Writes `path` as an editor saving it does: the new text whole, never
/// a file emptied and then written, which the configuration's watcher
/// could read in between and apply, empty, as all the defaults (#212).
pub fn write_whole(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".new");
    let new = path.with_file_name(name);
    std::fs::write(&new, contents)?;
    std::fs::rename(&new, path)
}

/// Frames fall on multiples of [`FRAME`].
fn next_frame_after(now: Duration) -> Duration {
    let frames = now.as_nanos() / FRAME.as_nanos() + 1;
    FRAME * u32::try_from(frames).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_fall_on_the_frame_grid() {
        assert_eq!(next_frame_after(Duration::ZERO), FRAME);
        assert_eq!(next_frame_after(FRAME / 2), FRAME);
        assert_eq!(next_frame_after(FRAME), FRAME * 2);
    }
}
