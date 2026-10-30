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
use hocus_agents::{Report, Source, State, hooks};

/// What the hook reads of its standard input at most.
const MOST: u64 = 4 << 20;
/// How long writing the line may take: a desktop that does not read is
/// not waited for.
const WRITE: Duration = Duration::from_millis(500);

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
}

fn main() -> ExitCode {
    let args = Args::parse();
    match args.command {
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
            };
            match send(args.socket, &report) {
                Ok(()) => ExitCode::SUCCESS,
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
