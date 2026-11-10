//! `hocus`: tells the Hocuspanda desktop what an agent is doing
//! (docs/agents.md).
//!
//! - `hocus agent NAME STATE [--pid PID]`, for any tool: STATE is
//!   working, waiting, done, failed or idle (or ended, to forget it). The
//!   agent is the process that ran the command (above its shells), unless
//!   `--pid` names it; its window is found above it.
//! - `hocus agent-hook TOOL [PAYLOAD]`, what Claude Code's and Codex's
//!   hooks run once plugged in: the hook's JSON (on standard input, or as
//!   the argument Codex's `notify` gives) made a state. It never fails and
//!   prints nothing a tool would take as an answer: a hook must not get in
//!   the agent's way, desktop or not.
//!
//! - `hocus msg lock`: locks the session now (docs/design/lock.md).
//!
//! - `hocus msg input-devices [--follow]`: the mice and touchpads plugged
//!   in and what they were given of `[pointer]` and `[touchpad]` (6.12);
//!   followed, each one plugged, changed or unplugged as it happens.
//!
//! - `hocus msg idle`: the session's idle state, its time left and what
//!   holds it off (docs/design/idle.md), as the compositor says it.
//!
//! - `hocus msg outputs`: the screens, their modes, places, scales and
//!   settings, as one line of JSON (docs/design/settings.md, "Écrans").
//!
//! - `hocus msg secretary status`, `hocus msg secretary graph NODE
//!   [--as-of DATE] [--hops N] [--notebook pro|perso]`: the secretary's
//!   state, or her graph around a node now or as of a date, as JSON
//!   (docs/design/secretary.md; in assistant mode only). These go to her
//!   own socket, beside the control socket (`<socket>.secretary`); the
//!   status says her process too, as the compositor supervises it.
//!
//! - `hocus msg shell status`, `hocus msg shell restart [--program PATH |
//!   --installed]`, and `hocus msg secretary process` and `hocus msg
//!   secretary restart` the same: which program the compositor runs as
//!   the shell (or the secretary) and since when, and that program ended
//!   and started again at once, from PATH when given, for this session
//!   only (docs/workflow.md, "Development loop").
//!
//! - `hocus day planka-token [ACCOUNT]`: a Planka board's access token,
//!   on standard input, checked and kept where the bar reads it
//!   (docs/design/day.md).
//!
//! - `hocus settings [SECTION]`: the Settings' window (6.1), on that
//!   section (`power`), or brought forward on it when it is open.
//!
//! - `hocus perf day [--date YYYY-MM-DD] [--json]`: a day of the
//!   desktop's CPU and memory, its peaks and its signals, as recorded in
//!   `$XDG_STATE_HOME/hocuspanda/perf` (docs/performance.md).
//!
//! The line goes to the compositor's control socket: `--socket`, else
//! `$HOCUS_SOCKET`, else beside `$WAYLAND_DISPLAY`'s socket.

use std::{
    io::{IsTerminal, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    process::ExitCode,
    time::Duration,
};

use clap::{Parser, Subcommand};
use hocus_agents::{ProgramAsk, Question, Report, Restart, Source, State, Supervised, hooks};

mod day;
mod perf;

/// `hocus-settings`, in `PATH` or beside this program, which it becomes.
fn settings(section: Option<&str>, join: Option<&str>) -> ExitCode {
    use std::os::unix::process::CommandExt as _;
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("hocus-settings")))
        .filter(|p| p.is_file());
    let in_path = std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| dir.join("hocus-settings").is_file())
    });
    let program = match (in_path, beside) {
        (false, Some(beside)) => beside,
        _ => PathBuf::from("hocus-settings"),
    };
    let mut command = std::process::Command::new(&program);
    command.args(section);
    if let Some(ssid) = join {
        command.args(["--join", ssid]);
    }
    let err = command.exec();
    eprintln!("hocus: {}: {err}", program.display());
    ExitCode::FAILURE
}

/// What the hook reads of its standard input at most.
const MOST: u64 = 4 << 20;
/// How long writing the line may take: a desktop that does not read is
/// not waited for.
const WRITE: Duration = Duration::from_millis(500);
/// How long an answer may take.
const ANSWER: Duration = Duration::from_secs(3);

/// Tells the Hocuspanda desktop what an agent is doing.
#[derive(Debug, Parser)]
#[command(name = "hocus", version)]
struct Args {
    /// The desktop's control socket. Default: $HOCUS_SOCKET, else beside
    /// $WAYLAND_DISPLAY's socket.
    #[arg(long, global = true, value_name = "PATH")]
    socket: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// An agent's state, from any tool.
    Agent {
        /// The name it shows under.
        name: String,
        /// working, waiting, done, failed, idle; or ended, to forget it.
        state: String,
        /// The agent's process (default: the one that ran this command,
        /// above its shells). Its window is found above it.
        #[arg(long)]
        pid: Option<u32>,
    },
    /// What Claude Code's and Codex's hooks run: the hook's JSON, on
    /// standard input or as an argument, made a state.
    #[command(hide = true)]
    AgentHook {
        /// claude-code or codex.
        tool: String,
        /// The hook's JSON (Codex's notify gives it here).
        payload: Option<String>,
    },
    /// Opens the Settings, on SECTION (power, keyboard…), or brings them
    /// forward on it.
    Settings {
        /// The section: appearance, wallpaper, bar, agenda, screens, sound,
        /// keyboard, mouse, network, bluetooth, desktops, applications,
        /// agents, captures, power, account, date-and-time, performance.
        section: Option<String>,
        /// On network: the Wi-Fi network whose sheet opens (6.6).
        #[arg(long)]
        join: Option<String>,
    },
    /// Your day's accounts (docs/design/day.md).
    Day {
        #[command(subcommand)]
        command: DayCommand,
    },
    /// The desktop's CPU and memory over a day (docs/performance.md).
    Perf {
        #[command(subcommand)]
        command: PerfCommand,
    },
    /// Asks the desktop what it knows.
    Msg {
        #[command(subcommand)]
        question: MsgCommand,
    },
}

#[derive(Debug, Subcommand)]
enum MsgCommand {
    /// The session's idle state, its time left, and what holds it off
    /// (docs/design/idle.md).
    Idle,
    /// Locks the session now (docs/design/lock.md).
    Lock,
    /// The screens: each one's modes, current mode, scale, place, VRR and
    /// whether it is on, plugged and primary, as one line of JSON
    /// (docs/design/settings.md, "Écrans").
    Outputs,
    /// The mice and touchpads plugged in, and what they were given of
    /// [pointer] and [touchpad] (docs/design/settings.md).
    InputDevices {
        /// Stays, and says each device plugged, changed or unplugged.
        #[arg(long)]
        follow: bool,
    },
    /// What the bar and the shell's other surfaces show, as screen
    /// readers read it: each window's nodes, their roles, names, values,
    /// states, places and actions (docs/design/screen-reader.md).
    A11y,
    /// The secretary, in assistant mode (docs/design/secretary.md): asked
    /// on her own socket, beside the control socket.
    Secretary {
        #[command(subcommand)]
        question: SecretaryCommand,
    },
    /// The shell (the bar) as the compositor runs it: which program, since
    /// when; or restarted (docs/workflow.md, "Development loop").
    Shell {
        #[command(subcommand)]
        question: ShellCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ShellCommand {
    /// Which program runs as the shell, whether it was given, its process
    /// and since when, and how many screens show their bar.
    Status,
    /// Ends the shell and starts it again at once. The windows stay, and
    /// the bar's space stays kept.
    Restart(RestartArgs),
}

/// Which program a restart runs.
#[derive(Debug, clap::Args)]
struct RestartArgs {
    /// Run this program from now on, for this session only (an absolute
    /// path): nothing is written to the configuration, and the next
    /// session runs the installed one. One that does not start or keeps
    /// ending gives way to the installed one.
    #[arg(long, value_name = "PATH", conflicts_with = "installed")]
    program: Option<PathBuf>,
    /// Run the installed program again, after one was given.
    #[arg(long)]
    installed: bool,
}

impl RestartArgs {
    fn restart(self) -> Restart {
        match (self.program, self.installed) {
            (Some(program), _) => Restart::Given(program),
            (None, true) => Restart::Installed,
            (None, false) => Restart::Same,
        }
    }
}

#[derive(Debug, Subcommand)]
enum SecretaryCommand {
    /// Her state: her clock, the lock, her memory's counts, her sources'
    /// cursors, the last catch-up, her connections, her model, and her
    /// process as the compositor runs it.
    Status,
    /// Her process as the compositor runs it: which program, whether it
    /// was given, since when. Answered by the compositor, whether she
    /// runs or not.
    Process,
    /// Ends her and starts her again at once (assistant mode only).
    Restart(RestartArgs),
    /// Her graph around NODE (a name or an alias): the edges that count,
    /// now or as of a date, and where she has each from.
    Graph {
        node: String,
        /// YYYY-MM-DD (default: today).
        #[arg(long, value_name = "DATE")]
        as_of: Option<String>,
        /// How many edges away from NODE.
        #[arg(long, default_value_t = 2)]
        hops: u8,
        /// pro or perso (default: both).
        #[arg(long)]
        notebook: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum PerfCommand {
    /// A day's totals, peaks and signals, read from the files the desktop
    /// records (the same the Settings' Performances page shows).
    Day {
        /// The day (default: today), of the 14 kept.
        #[arg(long, value_name = "YYYY-MM-DD")]
        date: Option<String>,
        /// As JSON, for a script, an agent or a bug report.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum DayCommand {
    /// Keeps a Planka board's access token, read on standard input, where
    /// the bar reads it: the session's keyring, else a file of yours alone.
    /// It is checked with the server first.
    PlankaToken {
        /// The account (default: the configuration's Planka board), as the
        /// shell's log names it: planka-tasks-HOST-BOARD.
        account: Option<String>,
        /// The configuration file (default:
        /// $XDG_CONFIG_HOME/hocuspanda/config.toml).
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,
        /// The session bus whose keyring keeps it (default: the session's;
        /// none: a file).
        #[arg(long, value_name = "ADDRESS")]
        bus: Option<String>,
        /// Kept without asking the server first.
        #[arg(long)]
        no_check: bool,
    },
}

fn main() -> ExitCode {
    let args = Args::parse();
    match args.command {
        Command::Settings { section, join } => settings(section.as_deref(), join.as_deref()),
        Command::Perf {
            command: PerfCommand::Day { date, json },
        } => perf::day(date.as_deref(), json),
        Command::Agent { name, state, pid } => {
            let state = match state.as_str() {
                "ended" => None,
                word => match word.parse::<State>() {
                    Ok(state) => Some(state),
                    Err(err) => {
                        eprintln!("hocus: {err}");
                        return ExitCode::from(2);
                    }
                },
            };
            let report = Report {
                source: Source::Command,
                key: hocus_agents::key_word(&name),
                state,
                pid: Some(pid.unwrap_or_else(the_agent)),
                name,
                note: None,
            };
            match send(args.socket, &report) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("hocus: {err}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Msg {
            question:
                MsgCommand::Shell {
                    question: ShellCommand::Status,
                },
        } => program(args.socket, &ProgramAsk::Status(Supervised::Shell)),
        Command::Msg {
            question:
                MsgCommand::Shell {
                    question: ShellCommand::Restart(restart),
                },
        } => program(
            args.socket,
            &ProgramAsk::Restart(Supervised::Shell, restart.restart()),
        ),
        Command::Msg {
            question:
                MsgCommand::Secretary {
                    question: SecretaryCommand::Process,
                },
        } => program(args.socket, &ProgramAsk::Status(Supervised::Secretary)),
        Command::Msg {
            question:
                MsgCommand::Secretary {
                    question: SecretaryCommand::Restart(restart),
                },
        } => program(
            args.socket,
            &ProgramAsk::Restart(Supervised::Secretary, restart.restart()),
        ),
        Command::Msg {
            question: MsgCommand::Secretary { question },
        } => match secretary(args.socket, question) {
            Ok(answer) => {
                println!("{answer}");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("hocus: {err}");
                ExitCode::FAILURE
            }
        },
        Command::Msg { question } => {
            let question = match question {
                MsgCommand::Idle => Question::Idle,
                MsgCommand::Lock => Question::Lock,
                MsgCommand::Outputs => Question::Outputs,
                MsgCommand::A11y => Question::A11y,
                MsgCommand::InputDevices { follow: true } => {
                    return follow(args.socket, Question::InputDevices { follow: true });
                }
                MsgCommand::InputDevices { follow: false } => {
                    Question::InputDevices { follow: false }
                }
                MsgCommand::Secretary { .. } | MsgCommand::Shell { .. } => {
                    return ExitCode::FAILURE;
                }
            };
            match ask(args.socket, question.line()) {
                Ok(answer) => {
                    print!("{answer}");
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("hocus: {err}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::AgentHook { tool, payload } => {
            hook(&tool, payload, args.socket);
            ExitCode::SUCCESS
        }
        Command::Day {
            command:
                DayCommand::PlankaToken {
                    account,
                    config,
                    bus,
                    no_check,
                },
        } => match day::planka_token(day::PlankaToken {
            account,
            config,
            bus,
            no_check,
        }) {
            Ok(said) => {
                println!("{said}");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("hocus: {err}");
                ExitCode::FAILURE
            }
        },
    }
}

/// Asks the secretary on her socket, beside the control socket: her
/// answer as pretty JSON.
fn secretary(socket: Option<PathBuf>, question: SecretaryCommand) -> Result<String, String> {
    use hocus_secretary_link::{
        Client, Connection, FromSecretary, GraphQuery, Notebook, ToSecretary,
    };
    let control = socket_path(socket)
        .ok_or("no desktop to ask: neither HOCUS_SOCKET nor WAYLAND_DISPLAY is set")?;
    let path = hocus_secretary_link::beside_control(&control)
        .ok_or_else(|| format!("{}: not a control socket", control.display()))?;
    // Her process, as the compositor runs it: said with her status, and
    // in her place when she does not answer.
    let process = matches!(question, SecretaryCommand::Status)
        .then(|| {
            let line = ProgramAsk::Status(Supervised::Secretary).line();
            ask(Some(control.clone()), &line).ok()
        })
        .flatten();
    let mut her = Connection::open(&path, Client::Cli, ANSWER).map_err(|e| {
        let mut said =
            format!("the secretary does not answer ({e}): is [secretary] mode \"assistant\"?");
        if let Some(process) = &process {
            said.push_str("\nher process, as the compositor runs it:\n");
            said.push_str(process.trim_end());
        }
        said
    })?;
    let answer = match question {
        // The compositor's, answered before.
        SecretaryCommand::Process | SecretaryCommand::Restart(_) => {
            return Err("not hers to answer".to_owned());
        }
        SecretaryCommand::Status => her.request(|id| ToSecretary::Status { id })?,
        SecretaryCommand::Graph {
            node,
            as_of,
            hops,
            notebook,
        } => {
            let notebook = match notebook.as_deref() {
                None => None,
                Some(word) => {
                    Some(Notebook::of(word).ok_or_else(|| format!("{word:?}: pro or perso"))?)
                }
            };
            her.request(|id| ToSecretary::Graph {
                id,
                query: GraphQuery {
                    node,
                    as_of,
                    hops,
                    notebook,
                },
            })?
        }
    };
    let mut value = match answer {
        FromSecretary::Status { status, .. } => serde_json::to_value(status),
        FromSecretary::Graph { graph, .. } => serde_json::to_value(graph),
        other => return Err(format!("an unexpected answer: {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    if let (Some(process), Some(status)) = (process, value.as_object_mut()) {
        status.insert("process".to_owned(), words(&process));
    }
    serde_json::to_string_pretty(&value).map_err(|e| e.to_string())
}

/// The compositor's `WORD VALUE` lines as a JSON object.
fn words(lines: &str) -> serde_json::Value {
    lines
        .lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(word, value)| (word.to_owned(), serde_json::Value::from(value)))
        .collect::<serde_json::Map<_, _>>()
        .into()
}

/// Asks the compositor about the shell's or the secretary's program, or
/// to restart it: its answer, a refusal being a failure.
fn program(socket: Option<PathBuf>, asked: &ProgramAsk) -> ExitCode {
    match ask(socket, &asked.line()) {
        Ok(answer) => match answer.strip_prefix(hocus_agents::REFUSED) {
            Some(why) => {
                eprintln!("hocus: {}", why.trim_end());
                ExitCode::FAILURE
            }
            None => {
                print!("{answer}");
                ExitCode::SUCCESS
            }
        },
        Err(err) => {
            eprintln!("hocus: {err}");
            ExitCode::FAILURE
        }
    }
}

/// The agent that ran this: the nearest process above that is not a
/// shell.
fn the_agent() -> u32 {
    hocus_agents::process::agent_above(std::os::unix::process::parent_id())
}

/// A hook: whatever happens, quietly.
fn hook(tool: &str, payload: Option<String>, socket: Option<PathBuf>) {
    let Some(tool) = hooks::Tool::from_word(tool) else {
        return;
    };
    let text = match payload {
        Some(text) => text,
        None => {
            let mut text = String::new();
            let stdin = std::io::stdin();
            if !stdin.is_terminal() {
                let _ = stdin.lock().take(MOST).read_to_string(&mut text);
            }
            text
        }
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return;
    };
    let answer = hooks::answer(tool, &value);
    if !answer.is_empty() {
        let mut out = std::io::stdout();
        let _ = out.write_all(answer.as_bytes());
        let _ = out.flush();
    }
    let Some(said) = hooks::said(tool, &value) else {
        return;
    };
    let agent = the_agent();
    let report = Report {
        source: tool.source(),
        key: if said.key.is_empty() {
            format!("pid-{agent}")
        } else {
            said.key
        },
        state: said.state,
        pid: Some(agent),
        // An event without `cwd`: the agent's own directory, where it
        // started; never the tool's name.
        name: said
            .name
            .or_else(|| hocus_agents::process::folder_of(agent))
            .unwrap_or_default(),
        // What it asks for, or why it failed: one short line.
        note: said.note,
    };
    // Not in a Hocuspanda session, or it is not listening: nothing to do.
    let _ = send(socket, &report);
}

/// The control socket's path.
fn socket_path(given: Option<PathBuf>) -> Option<PathBuf> {
    if given.is_some() {
        return given;
    }
    if let Some(path) = std::env::var_os(hocus_agents::SOCKET_ENV).filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let display = PathBuf::from(std::env::var_os("WAYLAND_DISPLAY").filter(|d| !d.is_empty())?);
    let wayland = if display.is_absolute() {
        display
    } else {
        PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?).join(display)
    };
    Some(hocus_agents::socket_path(&wayland))
}

fn send(socket: Option<PathBuf>, report: &Report) -> Result<(), String> {
    let path = socket_path(socket)
        .ok_or("no desktop to tell: neither HOCUS_SOCKET nor WAYLAND_DISPLAY is set")?;
    let mut stream = UnixStream::connect(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    stream
        .set_write_timeout(Some(WRITE))
        .map_err(|e| e.to_string())?;
    stream
        .write_all(format!("{report}\n").as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Asks `question`, and prints what the desktop says as it says it, until
/// it closes the connection.
fn follow(socket: Option<PathBuf>, question: Question) -> ExitCode {
    let followed = (|| -> Result<(), String> {
        let path = socket_path(socket)
            .ok_or("no desktop to ask: neither HOCUS_SOCKET nor WAYLAND_DISPLAY is set")?;
        let place = |e: std::io::Error| format!("{}: {e}", path.display());
        let mut stream = UnixStream::connect(&path).map_err(place)?;
        stream.set_write_timeout(Some(WRITE)).map_err(place)?;
        stream
            .write_all(format!("{}\n", question.line()).as_bytes())
            .map_err(place)?;
        let mut out = std::io::stdout();
        let mut buffer = [0; 4096];
        loop {
            match stream.read(&mut buffer).map_err(place)? {
                0 => return Ok(()),
                n => {
                    out.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
                    out.flush().map_err(|e| e.to_string())?;
                }
            }
        }
    })();
    match followed {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("hocus: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Asks `question` (its line), and reads the answer until the desktop
/// closes the connection.
fn ask(socket: Option<PathBuf>, question: &str) -> Result<String, String> {
    let path = socket_path(socket)
        .ok_or("no desktop to ask: neither HOCUS_SOCKET nor WAYLAND_DISPLAY is set")?;
    let place = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut stream = UnixStream::connect(&path).map_err(place)?;
    stream.set_write_timeout(Some(WRITE)).map_err(place)?;
    stream.set_read_timeout(Some(ANSWER)).map_err(place)?;
    stream
        .write_all(format!("{question}\n").as_bytes())
        .map_err(place)?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer).map_err(place)?;
    if answer.is_empty() {
        return Err(format!("{}: no answer", path.display()));
    }
    Ok(answer)
}
