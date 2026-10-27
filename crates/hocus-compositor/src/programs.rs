//! Programs started for Alex or for the session (#285). The loop makes
//! each one's command: its arguments, its environment, its activation
//! token. A thread of its own starts it (the search through `PATH`, the
//! fork, the exec and the wait for it), says how that went through a
//! channel, then waits for the program to end. A slow directory in `PATH`
//! (a network home, an automount) holds back no frame.

use std::{collections::BTreeMap, fmt, process::Command, sync::Arc};

use smithay::reexports::calloop::{LoopHandle, channel};

use crate::{App, State};

/// Who a program is started for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// Alex, from the shell (a click, a key): its window may take the
    /// focus.
    ByUser,
    /// The session, starting what it always does: nobody waits for those.
    BySession,
    /// The session putting an application back (its app id): one that
    /// cannot start is said, and its windows no longer waited for.
    Restoring(String),
}

/// Run on the thread that starts each program, before it does, with the
/// program: a test's stand-in for a slow `PATH`.
pub type Gate = Arc<dyn Fn(&str) + Send + Sync>;

/// The programs being started.
#[derive(Default)]
pub struct Programs {
    answers: Option<channel::Sender<Answer>>,
    /// Each program asked for, from when it is asked for until its thread
    /// says whether it started.
    starting: BTreeMap<u64, Starting>,
    next: u64,
    gate: Option<Gate>,
}

impl fmt::Debug for Programs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Programs")
            .field("starting", &self.starting)
            .field("next", &self.next)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct Starting {
    program: String,
    asked: Asked,
}

/// What a program's thread says once it tried.
#[derive(Debug)]
struct Answer {
    id: u64,
    outcome: Outcome,
}

#[derive(Debug)]
enum Outcome {
    /// Started: its process id.
    Running(u32),
    /// Not started, and why.
    Failed(String),
}

/// Where the programs' threads answer.
pub fn start(handle: &LoopHandle<'static, App>, state: &mut State) {
    let (answers, received) = channel::channel::<Answer>();
    let inserted = handle.insert_source(received, |event, _, app| {
        if let channel::Event::Msg(answer) = event {
            app.timed("program started", |app| app.state.program_answered(answer));
        }
    });
    match inserted {
        Ok(_) => state.programs.answers = Some(answers),
        Err(err) => tracing::warn!(err = %err.error, "programs cannot be started"),
    }
}

impl State {
    /// Starts a program, for Alex or for the session, in a clean
    /// environment: what the session has (or a test gives), without what
    /// must not leak into applications, with this compositor's sockets.
    /// One Alex asked for gets an activation token of the shell's
    /// (`XDG_ACTIVATION_TOKEN`, and `DESKTOP_STARTUP_ID` for X11 ones), so
    /// that its window, or an instance already running that it hands the
    /// token to, takes the focus (#145). A thread starts it and waits for
    /// it: the loop does neither.
    pub fn start_program(&mut self, program: &str, args: &[String], asked: Asked) {
        let cmd = self.program_command(program, args, &asked);
        let id = self.programs.next;
        self.programs.next += 1;
        self.programs.starting.insert(
            id,
            Starting {
                program: program.to_owned(),
                asked,
            },
        );
        let Some(answers) = self.programs.answers.clone() else {
            self.program_answered(Answer {
                id,
                outcome: Outcome::Failed("no thread answers".into()),
            });
            return;
        };
        let gate = self.programs.gate.clone();
        let name = program.to_owned();
        let spawned = std::thread::Builder::new()
            .name("child".into())
            .spawn(move || run(cmd, &name, id, gate, &answers));
        if let Err(err) = spawned {
            self.program_answered(Answer {
                id,
                outcome: Outcome::Failed(format!("no thread to start it: {err}")),
            });
        }
    }

    /// The command, with its environment and its token: made on the loop,
    /// which owns the sockets and the tokens.
    fn program_command(&mut self, program: &str, args: &[String], asked: &Asked) -> Command {
        let mut cmd = Command::new(program);
        cmd.args(args);
        if let Some(env) = &self.child_env {
            cmd.env_clear().envs(env.iter().map(|(k, v)| (k, v)));
        }
        for leak in ["LD_PRELOAD", "LD_AUDIT", "WAYLAND_SOCKET", "DISPLAY"] {
            cmd.env_remove(leak);
        }
        cmd.env("WAYLAND_DISPLAY", &self.socket_path)
            .env(hocus_api::SOCKET_ENV, self.api.path())
            .env("XDG_CURRENT_DESKTOP", hocus_platform::apps::DESKTOP)
            .env("XDG_SESSION_TYPE", "wayland");
        if let Some(dir) = self.socket_path.parent() {
            cmd.env("XDG_RUNTIME_DIR", dir);
        }
        if let Some(n) = self.xdisplay {
            cmd.env("DISPLAY", format!(":{n}"));
        }
        // The session's environment may carry a token of its own, used
        // already.
        cmd.env_remove("XDG_ACTIVATION_TOKEN")
            .env_remove("DESKTOP_STARTUP_ID");
        if *asked == Asked::ByUser {
            let token = self.shell_token();
            cmd.env("XDG_ACTIVATION_TOKEN", &token)
                .env("DESKTOP_STARTUP_ID", &token);
        }
        cmd
    }

    /// A program's thread said whether it started.
    fn program_answered(&mut self, answer: Answer) {
        let Some(Starting { program, asked }) = self.programs.starting.remove(&answer.id) else {
            return;
        };
        match answer.outcome {
            Outcome::Running(pid) => tracing::info!(program, pid, "started"),
            Outcome::Failed(err) => {
                tracing::warn!(err, program, "could not start");
                if let Asked::Restoring(app_id) = asked {
                    self.restore_failed(&app_id, &err);
                }
            }
        }
    }

    /// How many programs are asked for and not yet started (or failed).
    pub fn programs_starting(&self) -> usize {
        self.programs.starting.len()
    }

    /// Each program is started only once `gate` returned (tests).
    pub fn set_program_gate(&mut self, gate: Gate) {
        self.programs.gate = Some(gate);
    }
}

/// On the program's own thread: starts it, says how it went, and waits
/// for it to end so that it is reaped.
fn run(
    mut cmd: Command,
    name: &str,
    id: u64,
    gate: Option<Gate>,
    answers: &channel::Sender<Answer>,
) {
    if let Some(gate) = gate {
        gate(name);
    }
    match cmd.spawn() {
        Ok(mut child) => {
            let _ = answers.send(Answer {
                id,
                outcome: Outcome::Running(child.id()),
            });
            let _ = child.wait();
        }
        Err(err) => {
            let _ = answers.send(Answer {
                id,
                outcome: Outcome::Failed(err.to_string()),
            });
        }
    }
}
