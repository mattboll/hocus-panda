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
//! - `hocus day planka-token [ACCOUNT]`: a Planka board's access token,
//!   on standard input, checked and kept where the bar reads it
//!   (docs/design/day.md).
//!
//! - `hocus settings [SECTION]`: the Settings' window (6.1), on that
//!   section (`power`), or brought forward on it when it is open.
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
use hocus_agents::{Question, Report, Source, State, hooks};

mod day;

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
        /// agents, captures, power, account, date-and-time.
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
        Command::Msg { question } => {
            let question = match question {
                MsgCommand::Idle => Question::Idle,
                MsgCommand::Lock => Question::Lock,
                MsgCommand::Outputs => Question::Outputs,
                MsgCommand::InputDevices { follow: true } => {
                    return follow(args.socket, Question::InputDevices { follow: true });
                }
                MsgCommand::InputDevices { follow: false } => {
                    Question::InputDevices { follow: false }
                }
            };
            match ask(args.socket, question) {
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

/// Asks `question`, and reads the answer until the desktop closes the
/// connection.
fn ask(socket: Option<PathBuf>, question: Question) -> Result<String, String> {
    let path = socket_path(socket)
        .ok_or("no desktop to ask: neither HOCUS_SOCKET nor WAYLAND_DISPLAY is set")?;
    let place = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut stream = UnixStream::connect(&path).map_err(place)?;
    stream.set_write_timeout(Some(WRITE)).map_err(place)?;
    stream.set_read_timeout(Some(ANSWER)).map_err(place)?;
    stream
        .write_all(format!("{}\n", question.line()).as_bytes())
        .map_err(place)?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer).map_err(place)?;
    if answer.is_empty() {
        return Err(format!("{}: no answer", path.display()));
    }
    Ok(answer)
}
