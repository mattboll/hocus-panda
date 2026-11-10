//! The secretary's process (`hocus-secretary`, docs/design/secretary.md
//! §16, §21): started once the compositor is ready, only in `[secretary]
//! mode = "assistant"`, started again when it ends (later each time it
//! ends quickly, as the shell is), stopped when the mode goes back to
//! "classic". The compositor only supervises her: it never connects to her
//! socket nor waits for her. The shell and `hocus` talk to her.
//!
//! Her socket sits next to the Wayland one, `<socket>.secretary`; its path
//! goes to her in `HOCUS_SECRETARY_SOCKET`, with the session bus whose
//! keyring holds her memory's key (`HOCUS_SECRETARY_BUS`, the
//! notifications' bus; none: a key file). Her standard input is a pipe the
//! compositor holds and never writes: it closes when she is stopped, or
//! when the compositor ends however it ends, and she ends then
//! (`--until-stdin-closes`). One that does not is killed after a grace.
//!
//! The shell is told her state (`secretary off|unavailable|ready`): her
//! header says "indisponible" while she is down; the bar works the same.
//!
//! `hocus msg secretary restart [--program PATH]` ends her the same way
//! and starts her again as soon as she has ended, without the delay, from
//! the program given if one is ([`crate::given`], docs/workflow.md
//! "Development loop").

use std::{
    path::{Path, PathBuf},
    process::{ChildStdin, Stdio},
    time::{Duration, Instant},
};

use anyhow::Context;
use hocus_link::{SecretaryShown, ToShell};
use smithay::reexports::calloop::{
    LoopHandle, RegistrationToken,
    channel::{self, Event, Sender},
    timer::{TimeoutAction, Timer},
};

use crate::{App, State, config::SecretaryMode, programs};

/// One that ran at least this long ended by accident, not in a loop: she
/// is started again at once.
const STEADY: Duration = Duration::from_secs(30);
/// The first restart's delay, doubled at each quick end, up to the last.
const FIRST_DELAY: Duration = Duration::from_millis(500);
const LAST_DELAY: Duration = Duration::from_secs(30);
/// How long she has to end once her pipe closes, before `SIGKILL`.
const GRACE: Duration = Duration::from_secs(5);

/// Her process, as the compositor sees it.
#[derive(Debug, Default)]
pub struct Secretary {
    pub socket: Option<PathBuf>,
    phase: Phase,
    /// Starts so far (tests: a restart is one more).
    pub starts: u64,
    /// Ends in a row that came quickly.
    quick_ends: u32,
    /// A run's number, and how long it ran (`None`: it did not start).
    ended: Option<Sender<(u64, Option<Duration>)>>,
    /// The program given in place of the installed one for this session,
    /// if one is.
    pub given: crate::given::Given,
    /// What the shell was told last.
    pub told: Option<SecretaryShown>,
}

/// Where she is.
#[derive(Debug, Default)]
enum Phase {
    /// Classic mode, or no program: none runs.
    #[default]
    Off,
    /// She runs: her process (0: it did not start), its start's number,
    /// its program, the pipe whose end ends her.
    Running {
        pid: u32,
        run: u64,
        program: PathBuf,
        started: crate::given::Started,
        _stdin: Option<ChildStdin>,
    },
    /// She ended; started again by this timer.
    Restarting(RegistrationToken),
    /// Asked to end (classic mode, or a restart asked for): run `run` is
    /// still to end.
    Stopping { run: u64 },
}

/// Her socket's path next to the Wayland socket's.
pub fn socket_path(wayland: &std::path::Path) -> PathBuf {
    hocus_secretary_link::socket_path(wayland)
}

/// Learns when she ends.
pub fn listen(handle: &LoopHandle<'static, App>, state: &mut State) -> anyhow::Result<()> {
    let (sender, ended) = channel::channel::<(u64, Option<Duration>)>();
    handle
        .insert_source(ended, |event, _, app| {
            if let Event::Msg((run, ran)) = event {
                app.state.secretary_ended(run, ran);
            }
        })
        .map_err(|err| err.error)
        .context("could not watch the secretary")?;
    state.secretary.socket = Some(socket_path(&state.socket_path));
    state.secretary.ended = Some(sender);
    Ok(())
}

impl State {
    /// Her process as `[secretary]` and her program say: started, left
    /// alone, or stopped.
    pub(crate) fn secretary_follow(&mut self) {
        let wanted = self.secretary_restartable().is_ok();
        match (&self.secretary.phase, wanted) {
            // Still ending: started again once she has (`secretary_ended`).
            (Phase::Stopping { .. }, true) => {}
            (Phase::Off, true) => {
                self.secretary.quick_ends = 0;
                self.start_secretary();
            }
            (Phase::Running { .. } | Phase::Restarting(_), false) => self.stop_secretary(),
            _ => {}
        }
    }

    /// The program she runs from: the one given for this session, else
    /// the installed one.
    pub(crate) fn secretary_program(&self) -> Option<PathBuf> {
        let given = self.secretary.given.program().map(Path::to_path_buf);
        given.or_else(|| self.config.secretary_program.clone())
    }

    fn start_secretary(&mut self) {
        let (Some(program), Some(socket), Some(ended)) = (
            self.secretary_program(),
            self.secretary.socket.clone(),
            self.secretary.ended.clone(),
        ) else {
            return;
        };
        if let Phase::Restarting(token) = std::mem::take(&mut self.secretary.phase) {
            self.loop_handle.remove(token);
        }
        let env = programs::Env::of(self);
        let mut command = env.command(&program);
        command
            .arg("--until-stdin-closes")
            .env(hocus_secretary_link::SOCKET_ENV, &socket)
            .stdin(Stdio::piped());
        match &self.shell.bus {
            Some(crate::shell::Bus::Given(address)) => {
                command.env(hocus_secretary_link::BUS_ENV, address);
            }
            Some(crate::shell::Bus::Session) => {
                command.env(hocus_secretary_link::BUS_ENV, "session");
            }
            None => {
                command.env_remove(hocus_secretary_link::BUS_ENV);
            }
        }
        let given = self.secretary.given.program().is_some();
        // A fork and an exec, as for the screen reader; the wait is a
        // thread's.
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                tracing::warn!(program = %program.display(), given, %err, "the secretary did not start");
                let _ = ended.send((self.secretary.starts + 1, None));
                self.secretary.starts += 1;
                self.secretary.phase = Phase::Running {
                    pid: 0,
                    run: self.secretary.starts,
                    program,
                    started: crate::given::Started::now(),
                    _stdin: None,
                };
                return;
            }
        };
        self.secretary.starts += 1;
        let run = self.secretary.starts;
        let pid = child.id();
        tracing::info!(program = %program.display(), given, pid, "the secretary started");
        self.secretary.phase = Phase::Running {
            pid,
            run,
            program,
            started: crate::given::Started::now(),
            _stdin: child.stdin.take(),
        };
        let started = Instant::now();
        programs::on_a_thread("secretary".to_owned(), move || {
            match child.wait() {
                Ok(status) => tracing::warn!(%status, "the secretary ended"),
                Err(err) => tracing::warn!(%err, "could not wait for the secretary"),
            }
            // The loop is gone when the compositor is: nothing to restart.
            let _ = ended.send((run, Some(started.elapsed())));
        });
        self.tell_secretary_state(SecretaryShown::Ready(socket));
    }

    /// Classic mode: her pipe closes, which ends her; killed if she has
    /// not ended after [`GRACE`].
    fn stop_secretary(&mut self) {
        self.end_secretary();
        self.tell_secretary_state(SecretaryShown::Off);
    }

    /// Her pipe closes, which ends her (killed if she has not ended after
    /// [`GRACE`]); one waiting to start again no longer is.
    fn end_secretary(&mut self) {
        match std::mem::take(&mut self.secretary.phase) {
            Phase::Running {
                pid, run, _stdin, ..
            } => {
                tracing::info!(pid, "the secretary is stopped");
                drop(_stdin);
                self.secretary.phase = Phase::Stopping { run };
                let timer = Timer::from_duration(GRACE);
                let kept = self.loop_handle.insert_source(timer, move |_, _, app| {
                    if let Phase::Stopping { run: r, .. } = app.state.secretary.phase
                        && r == run
                    {
                        tracing::warn!(pid, "the secretary did not end: killed");
                        kill(pid);
                    }
                    TimeoutAction::Drop
                });
                if let Err(err) = kept {
                    tracing::warn!(%err.error, "no timer for the secretary's end");
                }
            }
            Phase::Restarting(token) => self.loop_handle.remove(token),
            other => self.secretary.phase = other,
        }
    }

    /// Why she cannot be started, or started again, if she cannot.
    pub(crate) fn secretary_restartable(&self) -> Result<(), &'static str> {
        if self.config.secretary_program.is_none() || self.secretary.ended.is_none() {
            return Err("no secretary in this session");
        }
        if self.config.secretary.mode != SecretaryMode::Assistant {
            return Err("the secretary is off: [secretary] mode is not \"assistant\"");
        }
        if self.ending.is_some() {
            return Err("the session ends");
        }
        Ok(())
    }

    /// `hocus msg secretary restart`: she is asked to end, and her end
    /// starts her again at once ([`State::secretary_ended`]); not running,
    /// she starts now. The loop waits for nothing.
    pub(crate) fn restart_secretary(&mut self) {
        let program = self.secretary_program().unwrap_or_default();
        tracing::info!(
            program = %program.display(),
            given = self.secretary.given.program().is_some(),
            "hocus msg secretary restart"
        );
        self.end_secretary();
        if matches!(self.secretary.phase, Phase::Stopping { .. }) {
            self.tell_secretary_state(SecretaryShown::Unavailable);
        }
        self.secretary_follow();
    }

    /// `hocus msg secretary status`, her process's part: which program
    /// runs, since when.
    pub(crate) fn secretary_status(&self) -> String {
        let secretary = &self.secretary;
        let program = self.secretary_program();
        let (state, running) = match &secretary.phase {
            Phase::Off => ("off", None),
            Phase::Running {
                pid,
                program,
                started,
                ..
            } if *pid != 0 => ("running", Some((*pid, program, started))),
            Phase::Running { .. } | Phase::Restarting(_) => ("waiting", None),
            Phase::Stopping { .. } => ("ending", None),
        };
        crate::given::Status {
            state,
            program: running.map(|(_, p, _)| p.as_path()).or(program.as_deref()),
            installed: self.config.secretary_program.as_deref(),
            given: &secretary.given,
            process: running.map(|(pid, _, started)| (Some(pid), started)),
            starts: secretary.starts,
        }
        .lines()
    }

    /// Run `run` ended after running `ran` (`None`: it did not start).
    fn secretary_ended(&mut self, run: u64, ran: Option<Duration>) {
        let program = match &self.secretary.phase {
            Phase::Stopping { run: r, .. } if *r == run => {
                self.secretary.phase = Phase::Off;
                self.remove_secretary_socket();
                // Assistant again meanwhile, or a restart asked for: she
                // starts again now.
                self.secretary_follow();
                return;
            }
            Phase::Running {
                run: r, program, ..
            } if *r == run => program.clone(),
            // An older run's end, already dealt with.
            _ => return,
        };
        if self.ending.is_some() {
            self.secretary.phase = Phase::Off;
            return;
        }
        // A given program that does not hold: the installed one, at once.
        if let Some(why) = self.secretary.given.ended(&program, ran, STEADY) {
            self.fell_back(hocus_agents::Supervised::Secretary, program, why);
            self.secretary.phase = Phase::Off;
            self.tell_secretary_state(SecretaryShown::Unavailable);
            self.secretary_follow();
            return;
        }
        if ran.is_some_and(|ran| ran >= STEADY) {
            self.secretary.quick_ends = 0;
        }
        let delay = FIRST_DELAY
            .saturating_mul(1 << self.secretary.quick_ends.min(8))
            .min(LAST_DELAY);
        self.secretary.quick_ends += 1;
        tracing::info!(?delay, "the secretary starts again");
        self.tell_secretary_state(SecretaryShown::Unavailable);
        let restart = self
            .loop_handle
            .insert_source(Timer::from_duration(delay), |_, _, app| {
                if matches!(app.state.secretary.phase, Phase::Restarting(_)) {
                    app.state.secretary.phase = Phase::Off;
                    app.state.secretary_follow();
                }
                TimeoutAction::Drop
            });
        match restart {
            Ok(token) => self.secretary.phase = Phase::Restarting(token),
            Err(err) => {
                tracing::warn!(%err.error, "could not schedule the secretary's restart");
                self.secretary.phase = Phase::Off;
            }
        }
    }

    fn remove_secretary_socket(&self) {
        if let Some(socket) = &self.secretary.socket
            && let Err(err) = std::fs::remove_file(socket)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(socket = %socket.display(), %err, "could not remove her socket");
        }
    }

    fn tell_secretary_state(&mut self, shown: SecretaryShown) {
        if self.secretary.told.as_ref() != Some(&shown) {
            self.secretary.told = Some(shown.clone());
            self.tell_shell(ToShell::Secretary(shown));
        }
    }

    /// The shell connected: it is told where she is.
    pub(crate) fn secretary_shell_connected(&mut self) {
        let shown = self.secretary.told.clone().unwrap_or(SecretaryShown::Off);
        self.tell_shell(ToShell::Secretary(shown));
    }

    /// `Super+S` (§4): the shell opens her drawer beside the bar of the
    /// focused window's screen (else the pointer's), as wide as that
    /// screen allows; open, it closes. Assistant mode only: classic has no
    /// drawer, and the chord is no shortcut there.
    pub(crate) fn toggle_secretary_drawer(&mut self) {
        if self.config.secretary.mode != SecretaryMode::Assistant {
            return;
        }
        if !self.shell_linked() {
            tracing::info!("no shell: no secretary's drawer");
            return;
        }
        let Some(output) = self.bubbles_screen() else {
            return;
        };
        self.tell_drawer(&output);
    }

    /// A click on her head on `output`'s bar: as `Super+S` there.
    pub(crate) fn secretary_drawer_on(&mut self, output: &str) {
        if self.config.secretary.mode != SecretaryMode::Assistant {
            return;
        }
        let found = self.space.outputs().find(|o| o.name() == output).cloned();
        if let Some(output) = found {
            self.tell_drawer(&output);
        }
    }

    fn tell_drawer(&mut self, output: &smithay::output::Output) {
        let width = self
            .space
            .output_geometry(output)
            .map_or(0, |g| u32::try_from(g.size.w).unwrap_or(0));
        self.tell_shell(ToShell::SecretaryDrawer {
            output: output.name(),
            width,
        });
    }

    /// Her process while it runs (tests: killed as a crash would).
    pub fn secretary_pid(&self) -> Option<u32> {
        match self.secretary.phase {
            Phase::Running { pid, .. } if pid != 0 => Some(pid),
            _ => None,
        }
    }

    /// `[secretary]` changed: started or stopped. Whether it did.
    pub(crate) fn secretary_config_changed(&mut self, new: &crate::config::Secretary) -> bool {
        if *new == self.config.secretary {
            return false;
        }
        self.config.secretary = new.clone();
        self.secretary_follow();
        true
    }
}

/// `SIGKILL` to her process, one the compositor started and has not seen
/// end.
fn kill(pid: u32) {
    let Some(pid) = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    else {
        return;
    };
    if let Err(err) = rustix::process::kill_process(pid, rustix::process::Signal::KILL) {
        tracing::debug!(%err, "the secretary is gone already");
    }
}
