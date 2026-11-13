//! The shell (`hocuspanda-shell`, crates/hocus-shell): the compositor starts
//! it once ready, restarts it when it ends (sooner or later as it keeps
//! ending), and talks to it over a private socket (`hocus-link`).
//!
//! `hocus msg shell restart [--program PATH]` ends it (`SIGTERM`: it
//! writes what it keeps) and starts it again as soon as it has ended,
//! without that delay, from the program given if one is ([`crate::given`],
//! docs/workflow.md "Development loop"). Its bars' space stays kept and
//! the windows stay where they are meanwhile.
//!
//! The socket sits next to the Wayland one, `<socket>.shell`, in the
//! session's runtime directory; its path goes to the shell in
//! `HOCUS_SHELL_SOCKET`. The compositor reads it from the loop without ever
//! waiting: lines come in as they are, and lines out are a few bytes,
//! dropped with the connection should the shell stop reading.
//!
//! The shell is started on the compositor's GPU (tech-choices, "The bar's
//! renderer (3.0)"): wgpu asks for the low-power adapter and the Vulkan
//! loader only loads that GPU's driver, so that a second GPU stays asleep;
//! Mesa is asked for an 8-bit-alpha surface first. Without a GPU, the shell
//! draws with tiny-skia.

use std::{
    io::{ErrorKind, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    process::ExitStatus,
    sync::{Arc, Condvar, Mutex, PoisonError},
    time::{Duration, Instant},
};

use anyhow::Context;
use hocus_link::{ToCompositor, ToShell};
use smithay::reexports::calloop::{
    Interest, LoopHandle, Mode, PostAction, RegistrationToken,
    channel::{self, Event, Sender},
    generic::Generic,
    timer::{TimeoutAction, Timer},
};

use crate::{App, State, backend::DevId, programs};

/// A shell that ran at least this long ended by accident, not in a loop:
/// it is started again at once.
const STEADY: Duration = Duration::from_secs(30);
/// The first restart's delay, doubled at each quick end, up to the last.
const FIRST_DELAY: Duration = Duration::from_millis(500);
const LAST_DELAY: Duration = Duration::from_secs(30);
/// How long a shell asked to end has to end, before `SIGKILL`.
const GRACE: Duration = Duration::from_secs(2);

/// The shell, as the compositor sees it.
#[derive(Debug, Default)]
pub struct Shell {
    /// Its program, the installed one; `None`: no shell (tests,
    /// `--no-shell`).
    pub program: Option<PathBuf>,
    /// The program given in its place for this session, if one is.
    pub given: crate::given::Given,
    phase: Phase,
    /// Starts so far: a run's number is its start's.
    pub starts: u64,
    /// What the shell is told once it is connected (again): a given
    /// program let go of.
    pub notices: Vec<ToShell>,
    /// The session bus it serves the notifications on; `None`: none.
    pub bus: Option<Bus>,
    /// The socket it reaches the compositor on.
    pub socket: Option<PathBuf>,
    link: Option<Link>,
    /// It draws on the CPU: its bars are opaque.
    pub software: bool,
    /// It is the session's polkit agent (7.5): the compositor is the
    /// session.
    pub auth_agent: bool,
    /// The active keyboard layout as last told; `None`: not told yet.
    pub layout: Option<u32>,
    /// The bars' screens' desktops as last told.
    pub desktops: crate::desktops::Told,
    /// Ends in a row that came quickly.
    quick_ends: u32,
    /// Shells in a row whose hello said another version of the line
    /// (`hocus_link::wire`): the first is started again from the
    /// installed program, the second leaves the session without a shell.
    mismatches: u32,
    /// The version the shell left out for it said (`None`: it said no
    /// hello), until a shell is asked for again: the status says it.
    refused: Option<Option<u32>>,
    ended: Option<Sender<Ended>>,
    /// The shells started and not ended yet.
    pub running: Arc<Running>,
    /// The shell's Wayland connection, made by the compositor and handed
    /// to it as it starts (`WAYLAND_SOCKET`): the only client whose lock
    /// surfaces show (7.1).
    pub client: Option<smithay::reexports::wayland_server::backend::ClientId>,
    /// `hocus msg a11y`'s askers, answered with the lines the shell says
    /// once it ends them.
    a11y: A11yAsked,
}

/// Where the shell's process is.
#[derive(Debug, Default)]
enum Phase {
    /// None runs: not started yet, no program, or the session ends.
    #[default]
    Off,
    /// Run `run` of `program`.
    Running {
        run: u64,
        program: PathBuf,
        started: crate::given::Started,
    },
    /// It ended by itself; started again by this timer.
    Waiting(RegistrationToken),
    /// Asked to end (`hocus msg shell restart`): started again as soon as
    /// run `run` has ended.
    Ending { run: u64 },
    /// Run `run` speaks another version of the line, as the one before it
    /// did: asked to end, and none is started after it.
    Refused { run: u64 },
}

/// A run ended, as its thread says it.
#[derive(Debug)]
struct Ended {
    run: u64,
    /// How long it ran; `None`: it did not start.
    ran: Option<Duration>,
}

/// The screen readers' tree asked of the shell: who waits, and what it said
/// so far.
#[derive(Debug, Default)]
struct A11yAsked {
    askers: Vec<std::sync::mpsc::Sender<String>>,
    said: String,
}

/// The session bus the shell serves the notifications on (5.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bus {
    /// The bus given (`--session-bus`), by its address.
    Given(String),
    /// The session's own: the compositor is the session.
    Session,
}

impl Bus {
    /// The word the shell takes ([`hocus_link::NOTIFICATIONS_BUS_ENV`]).
    fn word(&self) -> &str {
        match self {
            Self::Given(address) => address,
            Self::Session => "session",
        }
    }
}

/// How many shells run: the in-process harness waits for none to, so that
/// a test's shell does not outlive it (#467).
#[derive(Debug, Default)]
pub struct Running {
    count: Mutex<usize>,
    changed: Condvar,
    /// The shell's process while it runs: its Wayland connection's
    /// credentials are the compositor's own, which made it.
    process: Mutex<Process>,
}

/// The shell's process, and which runs were asked to end. Its thread
/// takes it out under the lock before it reaps it, and a signal is sent
/// under the same lock: it reaches that process or none, never another
/// that was given its number since.
#[derive(Debug, Default)]
struct Process {
    /// The run and its process, from its start until it ended.
    running: Option<(u64, u32)>,
    /// Every run up to this one was asked to end: one whose process
    /// starts after that is ended as it starts.
    ending: u64,
}

/// One shell, from before its thread starts until it ended (or its thread
/// never started).
struct Runs(Arc<Running>);

impl Running {
    fn start(this: &Arc<Self>) -> Runs {
        *this.count.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        Runs(this.clone())
    }

    /// The shell's process, while one runs.
    pub fn pid(&self) -> Option<u32> {
        let process = self.process.lock().unwrap_or_else(PoisonError::into_inner);
        process.running.map(|(_, pid)| pid)
    }

    /// Run `run`'s process started: ended at once if that was asked
    /// before it had.
    fn started(&self, run: u64, pid: u32) {
        let mut process = self.process.lock().unwrap_or_else(PoisonError::into_inner);
        process.running = Some((run, pid));
        if process.ending >= run {
            signal(pid, rustix::process::Signal::TERM);
        }
    }

    /// Asks run `run` (and any before it) to end, with `signal`.
    fn end(&self, run: u64, with: rustix::process::Signal) {
        let mut process = self.process.lock().unwrap_or_else(PoisonError::into_inner);
        process.ending = process.ending.max(run);
        if let Some((running, pid)) = process.running
            && running <= run
        {
            signal(pid, with);
        }
    }

    /// Waits for run `run`'s process to end. It is first waited for
    /// without being reaped, so that its number stays its own until it is
    /// taken out of [`Process`].
    fn wait(&self, run: u64, child: &mut std::process::Child) -> std::io::Result<ExitStatus> {
        use rustix::process::{Pid, WaitId, WaitIdOptions, waitid};
        let exited = i32::try_from(child.id())
            .ok()
            .and_then(Pid::from_raw)
            .is_some_and(|pid| {
                loop {
                    let options = WaitIdOptions::EXITED | WaitIdOptions::NOWAIT;
                    match waitid(WaitId::Pid(pid), options) {
                        Ok(_) => break true,
                        Err(rustix::io::Errno::INTR) => {}
                        Err(_) => break false,
                    }
                }
            });
        let mut process = self.process.lock().unwrap_or_else(PoisonError::into_inner);
        if process.running.is_some_and(|(running, _)| running == run) {
            process.running = None;
        }
        if !exited {
            // Not known to have ended: the wait may last, without the lock.
            drop(process);
        }
        child.wait()
    }

    /// Waits until no shell runs, `timeout` at most: whether none does.
    pub fn wait_none(&self, timeout: Duration) -> bool {
        let count = self.count.lock().unwrap_or_else(PoisonError::into_inner);
        let (count, _) = self
            .changed
            .wait_timeout_while(count, timeout, |n| *n > 0)
            .unwrap_or_else(PoisonError::into_inner);
        *count == 0
    }
}

impl Drop for Runs {
    fn drop(&mut self) {
        let mut count = self.0.count.lock().unwrap_or_else(PoisonError::into_inner);
        *count = count.saturating_sub(1);
        self.0.changed.notify_all();
    }
}

/// `with` to `pid`, the shell's process (see [`Process`]).
fn signal(pid: u32, with: rustix::process::Signal) {
    let Some(pid) = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    else {
        return;
    };
    if let Err(err) = rustix::process::kill_process(pid, with) {
        tracing::debug!(%err, "the shell is gone already");
    }
}

/// The most the shell is sent ahead of what it read: past that, it does
/// not read, and loses its connection.
const UNSENT_MOST: usize = 64 << 20;

/// The shell's connection.
#[derive(Debug)]
struct Link {
    writer: UnixStream,
    token: RegistrationToken,
    /// What the socket did not take yet (a clipboard's image, 5.6), and
    /// what writes it as the socket takes more.
    unsent: Vec<u8>,
    flushing: Option<RegistrationToken>,
    greeting: Greeting,
}

/// Where the line's hello is (`hocus_link::wire`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Greeting {
    /// The shell's first line is awaited: it is told nothing yet.
    Awaited,
    /// It speaks this compositor's version: it is fed, and read.
    Done,
    /// It speaks another: it is told nothing, and nothing of it is read.
    Refused,
}

/// The shell's socket's path next to the Wayland socket's.
pub fn socket_path(wayland: &Path) -> PathBuf {
    let mut path = wayland.as_os_str().to_owned();
    path.push(".shell");
    PathBuf::from(path)
}

/// Listens on the shell's socket, and learns when the shell ends.
pub fn listen(handle: &LoopHandle<'static, App>, state: &mut State) -> anyhow::Result<()> {
    let path = socket_path(&state.socket_path);
    // A socket left by a compositor that did not end well.
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("could not remove {}", path.display()))?;
    }
    let listener = UnixListener::bind(&path)
        .with_context(|| format!("could not listen on {}", path.display()))?;
    listener
        .set_nonblocking(true)
        .context("could not make the shell's socket non-blocking")?;
    handle
        .insert_source(
            Generic::new(listener, Interest::READ, Mode::Level),
            |_, listener, app| {
                loop {
                    match listener.accept() {
                        Ok((stream, _)) => app.state.shell_connected(stream),
                        Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                        Err(err) => {
                            tracing::warn!(%err, "could not take the shell's connection");
                            break;
                        }
                    }
                }
                Ok(PostAction::Continue)
            },
        )
        .map_err(|err| err.error)
        .context("could not watch the shell's socket")?;
    let (sender, ended) = channel::channel::<Ended>();
    handle
        .insert_source(ended, |event, _, app| {
            if let Event::Msg(ended) = event {
                app.state.shell_ended(ended);
            }
        })
        .map_err(|err| err.error)
        .context("could not watch the shell")?;
    state.shell.socket = Some(path);
    state.shell.ended = Some(sender);
    Ok(())
}

impl State {
    /// The program the shell runs from: the one given for this session,
    /// else the installed one.
    pub(crate) fn shell_program(&self) -> Option<PathBuf> {
        let given = self.shell.given.program().map(Path::to_path_buf);
        given.or_else(|| self.shell.program.clone())
    }

    /// Starts the shell, on a thread that waits for it to end.
    pub fn start_shell(&mut self) {
        let (Some(program), Some(socket), Some(ended)) = (
            self.shell_program(),
            self.shell.socket.clone(),
            self.shell.ended.clone(),
        ) else {
            return;
        };
        if let Phase::Waiting(token) = std::mem::take(&mut self.shell.phase) {
            self.loop_handle.remove(token);
        }
        // A former shell's connection is no longer the shell's: whether
        // the shell is there is said of this one.
        if self.shell.link.is_some() {
            self.shell_gone();
        }
        let env = programs::Env::of(self);
        let gpu = self.graphics.gpu;
        let bus = self.shell.bus.clone();
        let auth_agent = self.shell.auth_agent;
        // Its Wayland connection, made here: the compositor knows it is the
        // shell's, which no other client can claim to be (7.1).
        let theirs = match self.shell_connection() {
            Ok(theirs) => theirs,
            Err(err) => {
                tracing::warn!("could not make the shell's connection: {err:#}");
                return;
            }
        };
        self.shell.starts += 1;
        let run = self.shell.starts;
        self.shell.phase = Phase::Running {
            run,
            program: program.clone(),
            started: crate::given::Started::now(),
        };
        let given = self.shell.given.program().is_some();
        let runs = Running::start(&self.shell.running);
        programs::on_a_thread("shell".to_owned(), move || {
            let _runs = runs;
            let started = Instant::now();
            let mut command = env.command(&program);
            hand_socket(&mut command, &theirs);
            command.env(hocus_link::SOCKET_ENV, &socket);
            match &bus {
                Some(bus) => command.env(hocus_link::NOTIFICATIONS_BUS_ENV, bus.word()),
                None => command.env_remove(hocus_link::NOTIFICATIONS_BUS_ENV),
            };
            if auth_agent {
                command.env(hocus_link::AUTH_AGENT_ENV, "1");
            } else {
                command.env_remove(hocus_link::AUTH_AGENT_ENV);
            }
            // Its configuration file (`HOCUS_CONFIG`) is the programs'
            // ([`programs::Env`]).
            for (name, value) in gpu_env(gpu) {
                command.env(name, value);
            }
            let spawned = command.spawn();
            // Its copy is the child's now.
            drop(theirs);
            let ran = match spawned {
                Ok(mut child) => {
                    tracing::info!(
                        program = %program.display(),
                        given,
                        pid = child.id(),
                        run,
                        "shell started"
                    );
                    _runs.0.started(run, child.id());
                    match _runs.0.wait(run, &mut child) {
                        Ok(status) => tracing::warn!(%status, run, "the shell ended"),
                        Err(err) => tracing::warn!(%err, "could not wait for the shell"),
                    }
                    Some(started.elapsed())
                }
                Err(err) => {
                    tracing::warn!(program = %program.display(), given, %err, "could not start the shell");
                    None
                }
            };
            // The loop is gone when the compositor is: nothing to restart.
            let _ = ended.send(Ended { run, ran });
        });
    }

    /// The shell's Wayland connection: the compositor's end taken as a
    /// client known to be the shell, the other end to hand it.
    fn shell_connection(&mut self) -> anyhow::Result<UnixStream> {
        let (ours, theirs) = UnixStream::pair().context("a socket pair")?;
        let client = self
            .display_handle
            .insert_client(
                ours,
                std::sync::Arc::new(crate::state::ClientState {
                    shell: true,
                    ..Default::default()
                }),
            )
            .context("the shell's client")?;
        // One shell at a time: a former one's connection is no longer it.
        if let Some(old) = self.shell.client.replace(client.id()) {
            self.display_handle.backend_handle().kill_client(
                old,
                smithay::reexports::wayland_server::backend::DisconnectReason::ConnectionClosed,
            );
        }
        Ok(theirs)
    }

    /// A run of the shell ended. One asked to end is started again at
    /// once; one that ended by itself, later each time it ends quickly,
    /// unless it is a given program's that does not hold: the installed
    /// one then starts at once. Its bars' space stays kept meanwhile.
    fn shell_ended(&mut self, ended: Ended) {
        let Ended { run, ran } = ended;
        match std::mem::take(&mut self.shell.phase) {
            Phase::Ending { run: asked } if asked == run => {
                if self.ending.is_none() {
                    tracing::info!("the shell ended as asked: it starts again");
                    self.start_shell();
                }
            }
            // The second of another version: none after it.
            Phase::Refused { run: asked } if asked == run => {}
            Phase::Running {
                run: running,
                program,
                ..
            } if running == run => {
                if self.ending.is_some() {
                    return;
                }
                if let Some(why) = self.shell.given.ended(&program, ran, STEADY) {
                    self.fell_back(hocus_agents::Supervised::Shell, program, why);
                    self.shell.quick_ends = 0;
                    self.start_shell();
                    return;
                }
                if ran.is_some_and(|ran| ran >= STEADY) {
                    self.shell.quick_ends = 0;
                }
                let delay = FIRST_DELAY
                    .saturating_mul(1 << self.shell.quick_ends.min(8))
                    .min(LAST_DELAY);
                self.shell.quick_ends += 1;
                tracing::info!(?delay, "the shell starts again");
                let restart =
                    self.loop_handle
                        .insert_source(Timer::from_duration(delay), |_, _, app| {
                            // This timer goes as it returns: not removed
                            // by the start.
                            if matches!(app.state.shell.phase, Phase::Waiting(_)) {
                                app.state.shell.phase = Phase::Off;
                                app.state.start_shell();
                            }
                            TimeoutAction::Drop
                        });
                match restart {
                    Ok(token) => self.shell.phase = Phase::Waiting(token),
                    Err(err) => {
                        tracing::warn!(%err.error, "could not schedule the shell's restart");
                    }
                }
            }
            // An older run's end, already dealt with.
            other => self.shell.phase = other,
        }
    }

    /// Why the shell cannot be restarted now, if it cannot.
    pub(crate) fn shell_restartable(&self) -> Result<(), &'static str> {
        if self.shell.program.is_none() || self.shell.ended.is_none() {
            return Err("no shell in this session");
        }
        if self.ending.is_some() {
            return Err("the session ends");
        }
        Ok(())
    }

    /// `hocus msg shell restart`: the shell that runs is asked to end, and
    /// its end starts the next one at once; none running, it starts now.
    /// The loop waits for nothing.
    pub(crate) fn restart_shell(&mut self) {
        let program = self.shell_program().unwrap_or_default();
        tracing::info!(
            program = %program.display(),
            given = self.shell.given.program().is_some(),
            "hocus msg shell restart"
        );
        self.shell.quick_ends = 0;
        // Asked for again: a shell of another version gets its two tries.
        self.shell.mismatches = 0;
        self.shell.refused = None;
        match std::mem::take(&mut self.shell.phase) {
            Phase::Running { run, .. } => {
                self.shell.phase = Phase::Ending { run };
                self.end_shell_run(run);
            }
            // Already asked to end: its end starts the program as it is
            // now.
            Phase::Ending { run } | Phase::Refused { run } => {
                self.shell.phase = Phase::Ending { run };
            }
            waiting @ (Phase::Waiting(_) | Phase::Off) => {
                self.shell.phase = waiting;
                self.start_shell();
            }
        }
    }

    /// Run `run` is asked to end, and killed if it has not after
    /// [`GRACE`]; what follows its end is its phase's to say.
    fn end_shell_run(&mut self, run: u64) {
        self.shell.running.end(run, rustix::process::Signal::TERM);
        let kill = self
            .loop_handle
            .insert_source(Timer::from_duration(GRACE), move |_, _, app| {
                if matches!(
                    app.state.shell.phase,
                    Phase::Ending { run: r } | Phase::Refused { run: r } if r == run
                ) {
                    tracing::warn!(run, "the shell did not end: killed");
                    app.state
                        .shell
                        .running
                        .end(run, rustix::process::Signal::KILL);
                }
                TimeoutAction::Drop
            });
        if let Err(err) = kill {
            tracing::warn!(%err.error, "no timer for the shell's end");
        }
    }

    /// `hocus msg shell status`: which program runs, since when.
    pub(crate) fn shell_status(&self) -> String {
        let shell = &self.shell;
        let program = self.shell_program();
        let (state, running) = match &shell.phase {
            Phase::Off => ("off", None),
            Phase::Running {
                program, started, ..
            } => (
                if self.shell_linked() {
                    "running"
                } else {
                    "starting"
                },
                Some((program, started)),
            ),
            Phase::Waiting(_) => ("waiting", None),
            Phase::Ending { .. } | Phase::Refused { .. } => ("ending", None),
        };
        let mut said = crate::given::Status {
            state,
            program: running.map(|(p, _)| p.as_path()).or(program.as_deref()),
            installed: shell.program.as_deref(),
            given: &shell.given,
            process: running.map(|(_, started)| (shell.running.pid(), started)),
            starts: shell.starts,
            live_reload: self.config.developer.live_reload,
        }
        .lines();
        said.push_str(&format!(
            "bars {} of {}\n",
            self.bars_drawn(),
            self.panel.bars.len()
        ));
        // Only when a shell was left out for it: no shell runs.
        if let Some(theirs) = shell.refused {
            let theirs = theirs.map_or_else(|| "none".to_owned(), |v| v.to_string());
            said.push_str(&format!(
                "mismatch shell {theirs} compositor {}\n",
                hocus_link::VERSION
            ));
        }
        said
    }

    /// How many screens show their bar: its surface has a buffer of its
    /// place's size.
    fn bars_drawn(&self) -> usize {
        use smithay::backend::renderer::utils::with_renderer_surface_state;
        self.space
            .outputs()
            .filter(|output| {
                let map = smithay::desktop::layer_map_for_output(output);
                map.layers().any(|layer| {
                    layer.namespace() == hocus_link::NAMESPACE
                        && map.layer_geometry(layer).is_some_and(|g| {
                            with_renderer_surface_state(layer.wl_surface(), |s| s.surface_size())
                                .flatten()
                                == Some(g.size)
                        })
                })
            })
            .count()
    }

    /// The shell connected (again): its hello is awaited
    /// ([`State::shell_hello`]).
    fn shell_connected(&mut self, stream: UnixStream) {
        self.shell_gone();
        if let Err(err) = stream.set_nonblocking(true) {
            tracing::warn!(%err, "could not make the shell's connection non-blocking");
            return;
        }
        let writer = match stream.try_clone() {
            Ok(writer) => writer,
            Err(err) => {
                tracing::warn!(%err, "could not keep the shell's connection");
                return;
            }
        };
        let mut pending = Vec::new();
        let token = self.loop_handle.insert_source(
            Generic::new(stream, Interest::READ, Mode::Level),
            move |_, stream, app| {
                // A clipboard's image comes in one long line (5.6).
                let mut buffer = [0u8; 16 * 1024];
                loop {
                    match (&**stream).read(&mut buffer) {
                        Ok(0) => {
                            app.state.shell_gone();
                            return Ok(PostAction::Remove);
                        }
                        Ok(n) => pending.extend_from_slice(buffer.get(..n).unwrap_or(&buffer)),
                        Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                        Err(err) if err.kind() == ErrorKind::Interrupted => {}
                        Err(err) => {
                            tracing::warn!(%err, "could not read from the shell");
                            app.state.shell_gone();
                            return Ok(PostAction::Remove);
                        }
                    }
                }
                while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                    let line: Vec<u8> = pending.drain(..=end).collect();
                    let line = String::from_utf8_lossy(&line);
                    app.state.shell_line(line.trim());
                }
                Ok(PostAction::Continue)
            },
        );
        let token = match token {
            Ok(token) => token,
            Err(err) => {
                tracing::warn!(%err.error, "could not read the shell's connection");
                return;
            }
        };
        tracing::debug!("the shell's connection: its hello is awaited");
        self.shell.link = Some(Link {
            writer,
            token,
            unsent: Vec::new(),
            flushing: None,
            greeting: Greeting::Awaited,
        });
    }

    /// A line from the shell: its hello first, then its messages. One
    /// that does not read (a kind this compositor does not know) is said
    /// and skipped.
    fn shell_line(&mut self, line: &str) {
        match self.shell.link.as_ref().map(|link| link.greeting) {
            None | Some(Greeting::Refused) => {}
            Some(Greeting::Awaited) => self.shell_hello(line),
            Some(Greeting::Done) => match ToCompositor::read(line) {
                Ok(message) => self.shell_says(message),
                Err(err) => tracing::warn!(%err, "a line from the shell is skipped"),
            },
        }
    }

    /// The shell's first line: its hello, answered with the compositor's
    /// own whatever it says. On the same version it is fed; on another
    /// (a package upgraded under the session pairs an old program with a
    /// new one), neither reads the other.
    fn shell_hello(&mut self, first: &str) {
        let theirs = hocus_link::wire::said_version(first);
        self.write_shell(hocus_link::wire::hello());
        let Some(link) = &mut self.shell.link else {
            return;
        };
        if theirs == Some(hocus_link::VERSION) {
            link.greeting = Greeting::Done;
            self.shell.mismatches = 0;
            self.shell.refused = None;
            tracing::info!("the shell is connected");
            self.feed_shell();
        } else {
            link.greeting = Greeting::Refused;
            self.shell_mismatched(theirs);
        }
    }

    /// The shell that connected speaks another version of the line. The
    /// first time, it is started again from the installed program (a
    /// program given for this session is let go of; an upgrade may have
    /// just put the right one in place). If that one mismatches too, the
    /// session runs without a shell, its bars' space kept: the log and
    /// `hocus msg shell status` say so (a notification is the shell's to
    /// draw).
    fn shell_mismatched(&mut self, theirs: Option<u32>) {
        let ours = hocus_link::VERSION;
        self.shell.mismatches += 1;
        let Phase::Running { run, program, .. } = &self.shell.phase else {
            // Not a shell this compositor started, or one already asked to
            // end: it is told nothing, and nothing more is done.
            tracing::warn!(shell = ?theirs, compositor = ours, "a shell of another version of the line: not fed");
            return;
        };
        let (run, program) = (*run, program.clone());
        if self.shell.mismatches > 1 || self.shell_restartable().is_err() {
            tracing::error!(
                shell = ?theirs,
                compositor = ours,
                program = %program.display(),
                "the shell still speaks another version of the line: the session runs without a shell"
            );
            self.shell.refused = Some(theirs);
            self.shell.phase = Phase::Refused { run };
            self.end_shell_run(run);
            return;
        }
        tracing::warn!(
            shell = ?theirs,
            compositor = ours,
            program = %program.display(),
            "the shell speaks another version of the line: not fed, started again from the installed program"
        );
        if self.shell.given.mismatched(&program) {
            self.fell_back(
                hocus_agents::Supervised::Shell,
                program,
                crate::given::Why::OtherVersion,
            );
        }
        self.shell.phase = Phase::Ending { run };
        self.end_shell_run(run);
    }

    /// What a shell is told as it connects: how bars and their clock
    /// look, where bars are, the other screens, the keyboard's layouts and
    /// the bars' screens' desktops.
    fn feed_shell(&mut self) {
        self.tell_shell(self.bar_look());
        self.wallpaper_accent_now();
        self.tell_shell(ToShell::Theme(self.theme_choice()));
        self.tell_shell(ToShell::Wallpaper(self.wallpapers()));
        self.tell_shell(ToShell::Locale(self.config.locale));
        self.tell_shell(ToShell::Clock(self.config.panel.clock.look()));
        self.tell_shell(ToShell::BarWindows(self.config.panel.windows));
        self.tell_shell(ToShell::ClipboardRules(self.config.clipboard.rules()));
        self.tell_shell(ToShell::AgentPandas(self.config.agents.pandas));
        self.tell_shell(ToShell::DesktopNames(self.config.desktops.shown_names()));
        self.tell_shell(ToShell::BarGroups(self.config.panel.groups()));
        self.tell_shell(ToShell::BarSide(self.bar_side()));
        let bars: Vec<ToShell> = self
            .panel
            .bars
            .iter()
            .map(|(output, bar)| ToShell::Bar {
                output: output.clone(),
                form: bar.form,
                since: 0,
                scale: self.bar_scale(output),
            })
            .collect();
        for bar in bars {
            self.tell_shell(bar);
        }
        self.tell_screens(true);
        self.tell_layouts();
        self.tell_desktops();
        self.tell_window_states(true);
        self.tell_agents();
        self.tell_mcp();
        self.tell_bubbles_screen();
        self.tell_fullscreen_again();
        self.hand_kept_levels();
        self.tell_sharing_again();
        self.auth_tell_caps();
        if self.record.recording() {
            self.tell_recording();
        }
        self.lock_shell_connected();
        self.away_shell_connected();
        self.leave_shell_connected();
        self.tell_notices();
    }

    /// The shell's connection ended.
    fn shell_gone(&mut self) {
        if let Some(link) = self.shell.link.take() {
            self.loop_handle.remove(link.token);
            if let Some(flushing) = link.flushing {
                self.loop_handle.remove(flushing);
            }
            tracing::info!("the shell is gone; its bars' space stays");
        }
        self.answer_a11y(Some("the shell went\n"));
        self.peek_shell_gone();
        self.auth_shell_gone();
        // The meeting's halo is the shell's to say: its next one says it
        // again.
        self.meeting_near(None);
        // And whether the music plays (#801).
        self.music_plays(false);
        if self.shell.software {
            self.shell.software = false;
            self.redraw = true;
        }
        self.lock_shell_gone();
        self.leave_shell_gone();
    }

    fn shell_says(&mut self, message: ToCompositor) {
        // Locked (7.1): nothing that shows or moves a window, starts or
        // copies anything; the lock's own asks.
        if self.locked() && !Self::lock_lets(&message) {
            tracing::info!("the session is locked: the shell's ask is refused");
            return;
        }
        match message {
            ToCompositor::Lock(ask) => self.lock_asked(ask),
            ToCompositor::Software => {
                tracing::info!("the shell draws on the CPU: no glass behind its bars");
                self.shell.software = true;
                self.redraw = true;
                self.tell_shell(self.bar_look());
            }
            ToCompositor::Wake(namespace) => self.wake_shell_surfaces(&namespace),
            ToCompositor::A11yLine(line) => {
                if !self.shell.a11y.askers.is_empty() {
                    self.shell.a11y.said.push_str(&line);
                    self.shell.a11y.said.push('\n');
                }
            }
            ToCompositor::A11yEnd => self.answer_a11y(None),
            ToCompositor::Closing(namespace) if hocus_link::slides_in(&namespace) => {
                self.bubble_closing(&namespace);
            }
            ToCompositor::Closing(namespace) => {
                self.popovers_closing(&namespace);
                self.launcher_closing(&namespace);
            }
            ToCompositor::NextLayout => self.next_layout(),
            ToCompositor::Desktop { output, to } => self.desktop_asked(&output, to),
            ToCompositor::Bring(id) => self.window_asked(id),
            ToCompositor::DeskTiles { bar, tiles } => self.desk_tiles_said(bar, tiles),
            ToCompositor::Overview => {
                tracing::debug!("the bar asks for the overview");
                self.toggle_overview();
            }
            ToCompositor::Close(id) => {
                tracing::debug!(window = id, "the shell asks a window to close");
                self.close_window(crate::model::WindowId(id));
            }
            ToCompositor::WindowAction { id, action } => {
                tracing::debug!(window = id, action = action.name(), "the bar's menu acts");
                self.run_window_action(crate::model::WindowId(id), action);
            }
            ToCompositor::SendTo {
                id,
                output,
                desktop,
            } => self.send_to_asked(id, &output, desktop),
            ToCompositor::WindowMenu(id) => self.window_menu_asked(id),
            ToCompositor::Action(run) => self.action_asked(run),
            ToCompositor::McpUndo(request) => self.mcp_undo(request),
            ToCompositor::McpStop(stop) => self.mcp_stop(stop),
            ToCompositor::McpReply { card, reply } => self.mcp_reply(card, reply),
            ToCompositor::McpHandBack(agent) => self.mcp_hand_back(&agent),
            ToCompositor::GoToAgent { id, row } => self.go_to_agent(id, row),
            ToCompositor::Ring(row) => self.ring_moved(&row),
            ToCompositor::Plug(tool) => match crate::agents::tool(&tool) {
                Some(tool) => self.agent_tools_asked(hocus_agents::watch::Ask::Plug(tool)),
                None => tracing::warn!(tool, "the shell plugs in a tool that is not one"),
            },
            ToCompositor::Unplug(tool) => match crate::agents::tool(&tool) {
                Some(tool) => self.agent_tools_asked(hocus_agents::watch::Ask::Unplug(tool)),
                None => tracing::warn!(tool, "the shell unplugs a tool that is not one"),
            },
            ToCompositor::Decline => self.agent_tools_asked(hocus_agents::watch::Ask::Decline),
            ToCompositor::Follow(tool) if tool == "herdr" => {
                self.agent_tools_asked(hocus_agents::watch::Ask::FollowHerdr);
            }
            ToCompositor::Follow(tool) => {
                tracing::warn!(tool, "the shell follows a tool that is not herdr");
            }
            ToCompositor::Briefing { open } => self.peek_briefing(open),
            ToCompositor::PeekClose => self.peek_closed(),
            ToCompositor::Meeting(near) => {
                tracing::debug!(?near, "the next meeting, for the halo");
                self.meeting_near(near.map(|near| match near {
                    hocus_link::MeetingNear::Ten => crate::halo::Meeting::InTen,
                    hocus_link::MeetingNear::Two => crate::halo::Meeting::InTwo,
                }));
            }
            ToCompositor::Music { playing } => self.music_plays(playing),
            ToCompositor::Copy(text) => self.copy_text(text),
            ToCompositor::Clip { paste, clip } => self.clip_asked(paste, clip),
            ToCompositor::ClipboardCount { items, pinned } => {
                self.clipboard_counted(items, pinned);
            }
            ToCompositor::Emoji { keep, text } => self.emoji_asked(keep, text),
            ToCompositor::Capture(ask) => self.capture_asked(ask),
            ToCompositor::Share(ask) => self.share_asked(ask),
            ToCompositor::Auth(ask) => self.auth_asked(ask),
            ToCompositor::Record(ask) => self.record_asked(ask),
            ToCompositor::Launch(launch) => self.launch(launch),
            ToCompositor::Raise { pid, app } => self.raise_app(pid, &app),
            ToCompositor::TrayClick { ask, pid } => self.tray_clicked(ask, pid),
            ToCompositor::Export { ask, namespace } => {
                let handle = self.export_for_shell(&namespace);
                self.tell_shell(ToShell::Exported { ask, handle });
            }
            ToCompositor::LogOut => {
                tracing::info!("the shell logs out");
                self.end(crate::Ending::LoggedOut);
            }
            ToCompositor::Leave(ask) => self.leave_asked(ask),
        }
    }

    /// A configure, as they are, to the shell's layer surfaces of
    /// `namespace`: iced then draws them alone ([`ToCompositor::Wake`]).
    fn wake_shell_surfaces(&mut self, namespace: &str) {
        for output in self.space.outputs() {
            let map = smithay::desktop::layer_map_for_output(output);
            for layer in map.layers().filter(|l| l.namespace() == namespace) {
                layer.layer_surface().send_configure();
            }
        }
    }

    /// Whether the shell is connected, and speaks this compositor's
    /// version of the line: it is told, and heard.
    pub fn shell_linked(&self) -> bool {
        matches!(&self.shell.link, Some(link) if link.greeting == Greeting::Done)
    }

    /// How bars look now.
    pub fn bar_look(&self) -> ToShell {
        ToShell::Look {
            glass: self.glass_on(),
            motion: !self.reduced_motion(),
            lively: self.pandas_lively(),
        }
    }

    /// Whether the compositor blurs behind the bars: it can, transparency
    /// is not reduced (high contrast reduces it too, #423), and the shell
    /// draws on a GPU.
    pub fn glass_on(&self) -> bool {
        self.graphics.blur
            && !self.config.appearance.reduce_transparency
            && !self.config.appearance.high_contrast
            && !self.shell.software
    }

    /// `hocus msg a11y`: the shell is asked what it shows for screen
    /// readers; `reply` hears it once the shell said it all (or went).
    pub(crate) fn ask_a11y(&mut self, reply: std::sync::mpsc::Sender<String>) {
        if !self.shell_linked() {
            let _ = reply.send("no shell\n".to_owned());
            return;
        }
        self.shell.a11y.askers.push(reply);
        if self.shell.a11y.askers.len() == 1 {
            self.shell.a11y.said.clear();
            self.tell_shell(ToShell::A11y);
        }
    }

    /// Answers whoever asked for the screen readers' tree: with what the
    /// shell said, or `instead`.
    fn answer_a11y(&mut self, instead: Option<&str>) {
        let said = std::mem::take(&mut self.shell.a11y.said);
        let answer = instead.map_or(said, str::to_owned);
        for asker in self.shell.a11y.askers.drain(..) {
            let _ = asker.send(answer.clone());
        }
    }

    /// Tells the shell, if it is there; a shell that does not read loses
    /// its connection rather than holding the loop.
    pub fn tell_shell(&mut self, message: ToShell) {
        // Not made for a shell that is not there: the desktops' go at
        // every frame they change.
        if self.shell_linked() {
            self.write_shell(message.line());
        }
    }

    /// Tells the shell a message's line made already (off the loop: a
    /// clipboard's copy, 5.6), if it is there.
    pub fn tell_shell_line(&mut self, line: String) {
        if self.shell_linked() {
            self.write_shell(line);
        }
    }

    /// Writes a line on the shell's connection, its hello's answer
    /// included. What the socket does not take at once is written as it
    /// takes more, never waited for.
    fn write_shell(&mut self, mut line: String) {
        let Some(link) = &mut self.shell.link else {
            return;
        };
        line.push('\n');
        if !link.unsent.is_empty() {
            if link.unsent.len() + line.len() > UNSENT_MOST {
                tracing::warn!("the shell does not read: dropping its connection");
                self.shell_gone();
            } else {
                link.unsent.extend_from_slice(line.as_bytes());
            }
            return;
        }
        let written = match write_some(&link.writer, line.as_bytes()) {
            Ok(written) => written,
            Err(err) => {
                tracing::warn!(%err, "could not write to the shell: dropping its connection");
                self.shell_gone();
                return;
            }
        };
        if written == line.len() {
            return;
        }
        link.unsent
            .extend_from_slice(line.as_bytes().get(written..).unwrap_or_default());
        let flushing = link.writer.try_clone().map(|writer| {
            self.loop_handle.insert_source(
                Generic::new(writer, Interest::WRITE, Mode::Level),
                |_, _, app| Ok(app.state.flush_shell()),
            )
        });
        match flushing {
            Ok(Ok(token)) => {
                if let Some(link) = &mut self.shell.link {
                    link.flushing = Some(token);
                }
            }
            Ok(Err(err)) => {
                tracing::warn!(%err.error, "could not write to the shell later: dropping its connection");
                self.shell_gone();
            }
            Err(err) => {
                tracing::warn!(%err, "could not write to the shell later: dropping its connection");
                self.shell_gone();
            }
        }
    }

    /// The shell's socket takes more: what was left is written.
    fn flush_shell(&mut self) -> PostAction {
        let Some(link) = &mut self.shell.link else {
            return PostAction::Remove;
        };
        match write_some(&link.writer, &link.unsent) {
            Ok(written) => {
                link.unsent.drain(..written);
                if link.unsent.is_empty() {
                    link.flushing = None;
                    PostAction::Remove
                } else {
                    PostAction::Continue
                }
            }
            Err(err) => {
                tracing::warn!(%err, "could not write to the shell: dropping its connection");
                // This source goes as it returns.
                link.flushing = None;
                self.shell_gone();
                PostAction::Remove
            }
        }
    }
}

/// The shell's Wayland connection handed to it: `WAYLAND_SOCKET` names
/// the descriptor, which stays open across `exec` in the child alone (the
/// compositor's copy closes on exec, so no other program gets it).
#[allow(unsafe_code)]
fn hand_socket(command: &mut std::process::Command, socket: &UnixStream) {
    use std::os::{fd::AsRawFd, unix::process::CommandExt};
    let fd = socket.as_raw_fd();
    command.env("WAYLAND_SOCKET", fd.to_string());
    // SAFETY: the closure runs in the child between fork and exec, and only
    // clears the descriptor's close-on-exec flag with fcntl, which is
    // async-signal-safe and allocates nothing. `fd` is open in the child:
    // the parent holds `socket` until `spawn` returns.
    unsafe {
        command.pre_exec(move || {
            let fd = rustix::fd::BorrowedFd::borrow_raw(fd);
            rustix::io::fcntl_setfd(fd, rustix::io::FdFlags::empty())?;
            Ok(())
        });
    }
}

/// Writes what `writer` takes of `bytes` now: how much.
fn write_some(writer: &UnixStream, bytes: &[u8]) -> std::io::Result<usize> {
    let mut written = 0;
    while written < bytes.len() {
        match (&*writer).write(bytes.get(written..).unwrap_or_default()) {
            Ok(0) => return Err(ErrorKind::WriteZero.into()),
            Ok(n) => written += n,
            Err(err) if err.kind() == ErrorKind::WouldBlock => break,
            Err(err) if err.kind() == ErrorKind::Interrupted => {}
            Err(err) => return Err(err),
        }
    }
    Ok(written)
}

/// The shell's environment for the compositor's GPU, `gpu` (its render
/// node's device number); `None`: no GPU, the CPU renderer.
fn gpu_env(gpu: Option<DevId>) -> Vec<(&'static str, String)> {
    let Some(dev) = gpu else {
        return vec![("ICED_BACKEND", "tiny-skia".to_owned())];
    };
    let (major, minor) = (
        smithay::reexports::rustix::fs::major(dev),
        smithay::reexports::rustix::fs::minor(dev),
    );
    let driver = std::fs::read_link(format!("/sys/dev/char/{major}:{minor}/device/driver"))
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
    let mut env = vec![
        ("WGPU_POWER_PREF", "low".to_owned()),
        // Mesa's surfaces offer 10-bit colour with 2-bit alpha first, which
        // iced takes: translucent parts would blend in thirds.
        ("vk_wsi_force_bgra8_unorm_first", "true".to_owned()),
    ];
    match driver.as_deref().and_then(vulkan_driver) {
        Some(manifest) => env.push(("VK_LOADER_DRIVERS_SELECT", manifest.to_owned())),
        None => tracing::info!(
            ?driver,
            "no Vulkan driver known for the GPU: the loader picks"
        ),
    }
    let mesa_egl = "/usr/share/glvnd/egl_vendor.d/50_mesa.json";
    if driver.as_deref() != Some("nvidia") && Path::new(mesa_egl).exists() {
        env.push(("__EGL_VENDOR_LIBRARY_FILENAMES", mesa_egl.to_owned()));
    }
    env
}

/// The Vulkan loader's manifest names for a kernel driver, as
/// `VK_LOADER_DRIVERS_SELECT` takes them.
fn vulkan_driver(kernel: &str) -> Option<&'static str> {
    Some(match kernel {
        "i915" | "xe" => "*intel*",
        "amdgpu" | "radeon" => "*radeon*",
        "nouveau" => "*nouveau*",
        "nvidia" => "*nvidia*",
        "virtio_gpu" | "virtio-gpu" => "*virtio*",
        "msm" => "*freedreno*",
        "panfrost" | "panthor" => "*panfrost*",
        "v3d" => "*broadcom*",
        _ => return None,
    })
}

/// Sends one notification, `org.freedesktop.Notifications.Notify` on
/// `bus` (the shell serves it, 5.3), from `Hocuspanda` with `icon`, on a
/// thread of its own: a call to another process never holds the loop.
pub fn notify(bus: Bus, icon: &'static str, summary: String) {
    let started = std::thread::Builder::new()
        .name("notification".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(notify_on(bus, icon, summary)),
                Err(err) => tracing::warn!(%err, "no runtime for a notification"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "a notification is not sent");
    }
}

/// A low-urgency notification (to the history, no bubble), off the loop:
/// its thread waits for it to be opened, then calls `opened`, or for it to
/// close.
pub fn notify_quietly(
    bus: Bus,
    icon: &'static str,
    summary: String,
    opened: impl FnOnce() + Send + 'static,
) {
    let started = std::thread::Builder::new()
        .name("notification".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(notify_quietly_on(bus, icon, summary, opened)),
                Err(err) => tracing::warn!(%err, "no runtime for a notification"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "a notification is not sent");
    }
}

async fn notify_quietly_on(
    bus: Bus,
    icon: &str,
    summary: String,
    opened: impl FnOnce() + Send + 'static,
) {
    use futures_util::StreamExt as _;
    use zbus::{MatchRule, MessageStream, message::Type};
    const NOTIFICATIONS: &str = "org.freedesktop.Notifications";
    let builder = match &bus {
        Bus::Given(address) => zbus::connection::Builder::address(address.as_str()),
        Bus::Session => zbus::connection::Builder::session(),
    };
    let connection = match builder.build().await {
        Ok(connection) => connection,
        Err(err) => {
            tracing::warn!(%err, summary, "no session bus: a notification is not sent");
            return;
        }
    };
    // Listened to before it is sent, so that no answer is missed.
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(NOTIFICATIONS)
        .path("/org/freedesktop/Notifications")
        .build();
    let stream = match rule {
        Ok(rule) => MessageStream::for_match_rule(rule, &connection, None).await,
        Err(err) => Err(err),
    };
    let mut hints: std::collections::HashMap<&str, zbus::Variant<'_>> =
        std::collections::HashMap::new();
    hints.insert("urgency", zbus::Variant::from(0u8));
    hints.insert("desktop-entry", zbus::Variant::from("hocus-settings"));
    let sent = connection
        .call_method(
            Some(NOTIFICATIONS),
            "/org/freedesktop/Notifications",
            Some(NOTIFICATIONS),
            "Notify",
            &(
                "Hocuspanda",
                0u32,
                icon,
                summary.as_str(),
                "",
                vec!["default", ""],
                hints,
                -1i32,
            ),
        )
        .await;
    let id: u32 = match sent.and_then(|reply| reply.body().deserialize()) {
        Ok(id) => id,
        Err(err) => {
            tracing::warn!(%err, summary, "a notification is not sent");
            return;
        }
    };
    let Ok(mut stream) = stream else {
        return;
    };
    while let Some(Ok(message)) = stream.next().await {
        let header = message.header();
        let member = header.member().map(|m| m.as_str());
        if member == Some("ActionInvoked")
            && let Ok((of, key)) = message.body().deserialize::<(u32, String)>()
            && of == id
            && key == "default"
        {
            opened();
            return;
        }
        if member == Some("NotificationClosed")
            && message
                .body()
                .deserialize::<(u32, u32)>()
                .is_ok_and(|(of, _)| of == id)
        {
            return;
        }
    }
}

/// What became of a notification asking something ([`notify_asking`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// It is out: the server took it.
    Shown,
    /// One of its actions was chosen, by its key.
    Chose(String),
    /// Closed unanswered, or it could not be sent.
    Gone,
}

/// The longest a notification asking something waits for the server to
/// be there: at the session's start, the shell is not yet.
const SERVER_WAIT: Duration = Duration::from_secs(120);

/// A notification of normal urgency asking `summary`, its `actions` (key,
/// label) as its buttons, off the loop: once the server is there (it
/// waits for it a while), then until it is answered or closed. Its thread
/// tells `said` what became of it, [`Asked::Shown`] first.
pub fn notify_asking(
    bus: Bus,
    summary: String,
    actions: Vec<(&'static str, String)>,
    said: impl Fn(Asked) + Send + 'static,
) {
    let started = std::thread::Builder::new()
        .name("notification".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(notify_asking_on(bus, summary, actions, &said)),
                Err(err) => {
                    tracing::warn!(%err, "no runtime for a notification");
                    said(Asked::Gone);
                }
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "a notification is not sent");
    }
}

async fn notify_asking_on(
    bus: Bus,
    summary: String,
    actions: Vec<(&'static str, String)>,
    said: &(impl Fn(Asked) + Send),
) {
    use futures_util::StreamExt as _;
    use zbus::{MatchRule, MessageStream, message::Type};
    const NOTIFICATIONS: &str = "org.freedesktop.Notifications";
    let builder = match &bus {
        Bus::Given(address) => zbus::connection::Builder::address(address.as_str()),
        Bus::Session => zbus::connection::Builder::session(),
    };
    let connection = match builder.build().await {
        Ok(connection) => connection,
        Err(err) => {
            tracing::warn!(%err, summary, "no session bus: a notification is not sent");
            said(Asked::Gone);
            return;
        }
    };
    // The server there (the shell takes its name as it starts).
    let since = Instant::now();
    loop {
        let owned = connection
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "NameHasOwner",
                &(NOTIFICATIONS,),
            )
            .await
            .and_then(|reply| reply.body().deserialize::<bool>());
        if owned.unwrap_or(false) {
            break;
        }
        if since.elapsed() > SERVER_WAIT {
            tracing::warn!(
                summary,
                "no notifications' server: a notification is not sent"
            );
            said(Asked::Gone);
            return;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    // Listened to before it is sent, so that no answer is missed.
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(NOTIFICATIONS)
        .path("/org/freedesktop/Notifications")
        .build();
    let stream = match rule {
        Ok(rule) => MessageStream::for_match_rule(rule, &connection, None).await,
        Err(err) => Err(err),
    };
    let mut hints: std::collections::HashMap<&str, zbus::Variant<'_>> =
        std::collections::HashMap::new();
    hints.insert("urgency", zbus::Variant::from(1u8));
    hints.insert("desktop-entry", zbus::Variant::from("hocus-settings"));
    let listed: Vec<&str> = actions
        .iter()
        .flat_map(|(key, label)| [*key, label.as_str()])
        .collect();
    let sent = connection
        .call_method(
            Some(NOTIFICATIONS),
            "/org/freedesktop/Notifications",
            Some(NOTIFICATIONS),
            "Notify",
            &(
                "Hocuspanda",
                0u32,
                "",
                summary.as_str(),
                "",
                listed,
                hints,
                -1i32,
            ),
        )
        .await;
    let id: u32 = match sent.and_then(|reply| reply.body().deserialize()) {
        Ok(id) => id,
        Err(err) => {
            tracing::warn!(%err, summary, "a notification is not sent");
            said(Asked::Gone);
            return;
        }
    };
    said(Asked::Shown);
    let Ok(mut stream) = stream else {
        said(Asked::Gone);
        return;
    };
    while let Some(Ok(message)) = stream.next().await {
        let header = message.header();
        let member = header.member().map(|m| m.as_str());
        if member == Some("ActionInvoked")
            && let Ok((of, key)) = message.body().deserialize::<(u32, String)>()
            && of == id
        {
            said(Asked::Chose(key));
            return;
        }
        if member == Some("NotificationClosed")
            && message
                .body()
                .deserialize::<(u32, u32)>()
                .is_ok_and(|(of, _)| of == id)
        {
            said(Asked::Gone);
            return;
        }
    }
    said(Asked::Gone);
}

async fn notify_on(bus: Bus, icon: &str, summary: String) {
    let builder = match &bus {
        Bus::Given(address) => zbus::connection::Builder::address(address.as_str()),
        Bus::Session => zbus::connection::Builder::session(),
    };
    let connection = match builder.build().await {
        Ok(connection) => connection,
        Err(err) => {
            tracing::warn!(%err, summary, "no session bus: a notification is not sent");
            return;
        }
    };
    let hints: std::collections::HashMap<&str, zbus::Variant<'_>> =
        std::collections::HashMap::new();
    let sent = connection
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                "Hocuspanda",
                0u32,
                icon,
                summary.as_str(),
                "",
                Vec::<&str>::new(),
                hints,
                -1i32,
            ),
        )
        .await;
    if let Err(err) = sent {
        tracing::warn!(%err, summary, "a notification is not sent");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_gpu_is_the_cpu_renderer() {
        assert_eq!(
            gpu_env(None),
            vec![("ICED_BACKEND", "tiny-skia".to_owned())]
        );
        assert_eq!(vulkan_driver("i915"), Some("*intel*"));
        assert_eq!(vulkan_driver("simpledrm"), None);
    }

    #[test]
    fn the_shells_socket_is_beside_the_wayland_one() {
        assert_eq!(
            socket_path(Path::new("/run/user/1000/hocus-0")),
            PathBuf::from("/run/user/1000/hocus-0.shell")
        );
    }
}
