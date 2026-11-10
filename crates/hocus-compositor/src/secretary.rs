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

use std::{
    path::PathBuf,
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
    ended: Option<Sender<(u64, Duration)>>,
    /// What the shell was told last.
    pub told: Option<SecretaryShown>,
}

/// Where she is.
#[derive(Debug, Default)]
enum Phase {
    /// Classic mode, or no program: none runs.
    #[default]
    Off,
    /// She runs: her process, its start's number, the pipe whose end
    /// ends her.
    Running {
        pid: u32,
        run: u64,
        _stdin: Option<ChildStdin>,
    },
    /// She ended; started again by this timer.
    Restarting(RegistrationToken),
    /// Asked to end (classic mode): run `run` is still to end.
    Stopping { run: u64 },
}

/// Her socket's path next to the Wayland socket's.
pub fn socket_path(wayland: &std::path::Path) -> PathBuf {
    hocus_secretary_link::socket_path(wayland)
}

/// Learns when she ends.
pub fn listen(handle: &LoopHandle<'static, App>, state: &mut State) -> anyhow::Result<()> {
    let (sender, ended) = channel::channel::<(u64, Duration)>();
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
        let wanted = self.config.secretary.mode == SecretaryMode::Assistant
            && self.config.secretary_program.is_some()
            && self.secretary.ended.is_some();
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

    fn start_secretary(&mut self) {
        let (Some(program), Some(socket), Some(ended)) = (
            self.config.secretary_program.clone(),
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
        // A fork and an exec, as for the screen reader; the wait is a
        // thread's.
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                tracing::warn!(program = %program.display(), %err, "the secretary did not start");
                let _ = ended.send((self.secretary.starts + 1, Duration::ZERO));
                self.secretary.starts += 1;
                self.secretary.phase = Phase::Running {
                    pid: 0,
                    run: self.secretary.starts,
                    _stdin: None,
                };
                return;
            }
        };
        self.secretary.starts += 1;
        let run = self.secretary.starts;
        let pid = child.id();
        tracing::info!(program = %program.display(), pid, "the secretary started");
        self.secretary.phase = Phase::Running {
            pid,
            run,
            _stdin: child.stdin.take(),
        };
        let started = Instant::now();
        programs::on_a_thread("secretary".to_owned(), move || {
            match child.wait() {
                Ok(status) => tracing::warn!(%status, "the secretary ended"),
                Err(err) => tracing::warn!(%err, "could not wait for the secretary"),
            }
            // The loop is gone when the compositor is: nothing to restart.
            let _ = ended.send((run, started.elapsed()));
        });
        self.tell_secretary_state(SecretaryShown::Ready(socket));
    }

    /// Classic mode: her pipe closes, which ends her; killed if she has
    /// not ended after [`GRACE`].
    fn stop_secretary(&mut self) {
        match std::mem::take(&mut self.secretary.phase) {
            Phase::Running { pid, run, _stdin } => {
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
        self.tell_secretary_state(SecretaryShown::Off);
    }

    /// Run `run` ended after running `ran`.
    fn secretary_ended(&mut self, run: u64, ran: Duration) {
        match self.secretary.phase {
            Phase::Stopping { run: r, .. } if r == run => {
                self.secretary.phase = Phase::Off;
                self.remove_secretary_socket();
                // Assistant again meanwhile: she starts again now.
                self.secretary_follow();
                return;
            }
            Phase::Running { run: r, .. } if r == run => {}
            // An older run's end, already dealt with.
            _ => return,
        }
        if self.ending.is_some() {
            self.secretary.phase = Phase::Off;
            return;
        }
        if ran >= STEADY {
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
