//! Command-line tool to control and test Hocuspanda.

use std::io::Write as _;
use std::{ffi::OsString, path::PathBuf, process::ExitCode};

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use hocus_api::{Button, Client, Reply, Request, Target, WindowState};

/// Control and test Hocuspanda.
/// `println!`, but a reader that went away (`hocus msg ui | head`) ends
/// the command quietly instead of a panic.
macro_rules! out {
    ($($t:tt)*) => {
        if let Err(err) = writeln!(std::io::stdout(), $($t)*) {
            if err.kind() == std::io::ErrorKind::BrokenPipe {
                return Ok(Default::default());
            }
            return Err(err.into());
        }
    };
}

#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Send a request to the running compositor and print the reply as JSON.
    Msg {
        /// Control socket. Default: $HOCUS_SOCKET, or the one next to
        /// $WAYLAND_DISPLAY.
        #[arg(long, value_name = "PATH")]
        socket: Option<PathBuf>,

        #[command(subcommand)]
        message: Message,
    },
    /// Open the settings, on a page: `hocus settings keyboard`.
    Settings {
        /// `appearance`, `keyboard`, `screens`…; the home when left out.
        page: Option<String>,
        #[arg(long, value_name = "PATH")]
        socket: Option<PathBuf>,
    },
    /// The screen-cast portal's chooser (xdg-desktop-portal-wlr,
    /// `chooser_type = simple`): the shell asks what to share; prints
    /// `Monitor: NAME`, `Window: IDENTIFIER`, or nothing when declined.
    SharePick {
        #[arg(long, value_name = "PATH")]
        socket: Option<PathBuf>,
    },
    /// A Model Context Protocol server on stdin and stdout, for AI agents:
    /// windows, the shell's interface, captures, and clicks, typing and
    /// scrolls aimed at windows, as `[[agents.mcp]]` allows the agent.
    Mcp {
        /// The agent of `[[agents.mcp]]` this server is; without it, it
        /// only reads (acting needs a compositor started for tests).
        #[arg(long, value_name = "NAME")]
        agent: Option<String>,
        /// Control socket. Default: $HOCUS_SOCKET, or the one next to
        /// $WAYLAND_DISPLAY.
        #[arg(long, value_name = "PATH")]
        socket: Option<PathBuf>,
    },
    /// Run scenario files on headless compositors: `hocus scenario run FILE…`.
    ///
    /// Done by `hocus-scenario`, next to this program or in PATH, which keeps
    /// this tool free of the compositor.
    #[command(disable_help_flag = true)]
    Scenario {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
}

#[derive(Debug, Subcommand)]
enum Message {
    /// Protocol and compositor versions.
    Version,
    /// Outputs with their logical geometry, pixel size and scale.
    Outputs,
    /// Every connected screen, on or off: modes, scale, primary, desktops,
    /// the layout in effect.
    Screens,
    /// Each screen shows its name on it for a few seconds.
    Identify,
    /// Windows with their id, app id, output, geometry and focus.
    Windows,
    /// The whole scene (needs the test permission).
    Scene,
    /// Frame times per output, slow loop handlers, loop stalls.
    Stats,
    /// The configuration in effect, and the last error reading the file.
    Config,
    /// Print events as they happen, one JSON object per line.
    Events,
    /// Move the pointer to a point of the global logical space.
    PointerMove { x: f64, y: f64 },
    /// Press and release a pointer button.
    Click {
        #[arg(value_enum, default_value_t = ButtonArg::Left)]
        button: ButtonArg,
    },
    /// Press or release a pointer button, for drags.
    Button {
        #[arg(value_enum)]
        button: ButtonArg,
        #[arg(value_enum)]
        action: PressArg,
    },
    /// Press and release keys, by evdev code (KEY_A is 30): several are
    /// pressed in order and released in reverse (`key 125 30` is Super+A);
    /// `--action press` or `release` does only that.
    Key {
        #[arg(required = true)]
        codes: Vec<u32>,
        #[arg(long, value_enum)]
        action: Option<PressArg>,
    },
    /// Ask a window to close, by id.
    Close { window: u64 },
    /// Maximize, make fullscreen, minimize or restore a window.
    State {
        window: u64,
        #[arg(value_enum)]
        state: StateArg,
    },
    /// Each output's desktops, their windows, and the one shown.
    Desktops,
    /// The shell's interface: roles, labels and places of its elements.
    Ui,
    /// The sound, network, Bluetooth, power profile and battery, as quick
    /// settings know them.
    Hardware,
    /// What the document search's indexer does: off, waiting, paused (on
    /// battery), indexing or ready, and how many documents.
    Search,
    /// The document search's folders looked at again now rather than at
    /// the next half hour: only what changed is read. Says the indexer's
    /// state after.
    SearchAgain,
    /// What MCP agents did this session, oldest first, one line each, and
    /// whether they are stopped.
    AgentsJournal {
        /// The entries as JSON instead.
        #[arg(long)]
        json: bool,
    },
    /// Do not disturb, and the applications that sent notifications this
    /// session.
    Notifications,
    /// The phone, through KDE Connect: whether it runs, the phones within
    /// reach and their battery.
    Phone,
    /// Keep an AI provider's key in the keyring (`claude`, `openai`,
    /// `llamacpp`), read from the standard input: `hocus msg ai-key claude
    /// < key.txt`. An empty one forgets it.
    AiKey { provider: String },
    /// Send files to the phone within reach.
    SendToPhone {
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Plug a headless output, WIDTHxHEIGHT[@SCALE][+X+Y][/DESKTOPS][:WIDTHxHEIGHTmm].
    Plug {
        spec: String,
        /// Bring back an output unplugged before.
        #[arg(long)]
        name: Option<String>,
    },
    /// Unplug an output.
    Unplug { output: String },
    /// Make an output the one that takes what unplugged outputs showed.
    Primary { output: String },
    /// Show an output's desktop, from 1.
    ShowDesktop { output: String, desktop: usize },
    /// Send a window to an output: its shown desktop, or `--desktop N`
    /// (from 1).
    Send {
        window: u64,
        output: String,
        #[arg(long, value_name = "N")]
        desktop: Option<usize>,
    },
    /// Save a PNG of an output or a window.
    Screenshot {
        #[arg(long, value_name = "NAME", conflicts_with = "window")]
        output: Option<String>,
        #[arg(long, value_name = "ID")]
        window: Option<u64>,
        /// Where to write the PNG.
        #[arg(short = 'o', long = "to", value_name = "FILE")]
        file: PathBuf,
        /// Draw the cursor too.
        #[arg(long)]
        cursor: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PressArg {
    Press,
    Release,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum StateArg {
    Normal,
    Maximized,
    Fullscreen,
    Minimized,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ButtonArg {
    Left,
    Right,
    Middle,
}

impl From<ButtonArg> for Button {
    fn from(button: ButtonArg) -> Self {
        match button {
            ButtonArg::Left => Self::Left,
            ButtonArg::Right => Self::Right,
            ButtonArg::Middle => Self::Middle,
        }
    }
}

fn main() -> anyhow::Result<ExitCode> {
    match Args::parse().command {
        Command::Msg { socket, message } => msg(socket, message).map(|()| ExitCode::SUCCESS),
        Command::Scenario { args } => scenario(&args),
        Command::Mcp { agent, socket } => {
            let socket = socket.or_else(hocus_api::default_socket).context(
                "no control socket: set HOCUS_SOCKET or WAYLAND_DISPLAY, or pass --socket",
            )?;
            // Stdout carries the protocol alone; errors go to stderr.
            hocus_api::mcp::Bridge::new(socket, agent)
                .serve(std::io::stdin().lock(), std::io::stdout().lock())?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Settings { page, socket } => {
            let socket = socket.or_else(hocus_api::default_socket).context(
                "no control socket: set HOCUS_SOCKET or WAYLAND_DISPLAY, or pass --socket",
            )?;
            let mut client = Client::connect(&socket)
                .with_context(|| format!("could not connect to {}", socket.display()))?;
            client.request(Request::OpenSettings { page })?;
            Ok(ExitCode::SUCCESS)
        }
        Command::SharePick { socket } => {
            let socket = socket.or_else(hocus_api::default_socket).context(
                "no control socket: set HOCUS_SOCKET or WAYLAND_DISPLAY, or pass --socket",
            )?;
            let mut client = Client::connect(&socket)
                .with_context(|| format!("could not connect to {}", socket.display()))?;
            // The user takes the time they need.
            client.set_timeout(None)?;
            if let Reply::Shared(Some(answer)) = client.request(Request::PickShare)? {
                println!("{answer}");
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn scenario(args: &[OsString]) -> anyhow::Result<ExitCode> {
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("hocus-scenario")))
        .filter(|path| path.exists());
    let program = sibling.unwrap_or_else(|| PathBuf::from("hocus-scenario"));
    let status = std::process::Command::new(&program)
        .args(args)
        .status()
        .with_context(|| {
            format!(
                "could not run {}; build it with `cargo build -p hocus-test`",
                program.display()
            )
        })?;
    Ok(match status.code() {
        Some(0) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    })
}

fn msg(socket: Option<PathBuf>, message: Message) -> anyhow::Result<()> {
    let socket = socket
        .or_else(hocus_api::default_socket)
        .context("no control socket: set HOCUS_SOCKET or WAYLAND_DISPLAY, or pass --socket")?;
    let mut client = Client::connect(&socket)
        .with_context(|| format!("could not connect to {}", socket.display()))?;
    match message {
        Message::Version => print(client.request(Request::Version)?),
        Message::Outputs => print(client.request(Request::Outputs)?),
        Message::Screens => print(client.request(Request::Screens)?),
        Message::Identify => {
            client.request(Request::IdentifyOutputs)?;
            Ok(())
        }
        Message::Windows => print(client.request(Request::Windows)?),
        Message::Scene => print(client.request(Request::Scene)?),
        Message::Stats => print(client.request(Request::Stats)?),
        Message::Config => print(client.request(Request::Config)?),
        Message::Events => {
            client.request(Request::Subscribe)?;
            loop {
                out!("{}", serde_json::to_string(&client.next_event()?)?);
            }
        }
        Message::PointerMove { x, y } => {
            client.request(Request::PointerMove { x, y })?;
            Ok(())
        }
        Message::Click { button } => {
            for pressed in [true, false] {
                client.request(Request::PointerButton {
                    button: button.into(),
                    pressed,
                })?;
            }
            Ok(())
        }
        Message::Button { button, action } => {
            client.request(Request::PointerButton {
                button: button.into(),
                pressed: matches!(action, PressArg::Press),
            })?;
            Ok(())
        }
        Message::Key { codes, action } => {
            let press = !matches!(action, Some(PressArg::Release));
            let release = !matches!(action, Some(PressArg::Press));
            if press {
                for code in &codes {
                    client.request(Request::Key {
                        code: *code,
                        pressed: true,
                    })?;
                }
            }
            if release {
                for code in codes.iter().rev() {
                    client.request(Request::Key {
                        code: *code,
                        pressed: false,
                    })?;
                }
            }
            Ok(())
        }
        Message::Desktops => print(client.request(Request::Desktops)?),
        Message::Hardware => print(client.request(Request::Hardware)?),
        Message::Search => print(client.request(Request::Search)?),
        Message::SearchAgain => {
            client.request(Request::SearchAgain)?;
            print(client.request(Request::Search)?)
        }
        Message::Notifications => print(client.request(Request::Notifications)?),
        Message::Phone => print(client.request(Request::Phone)?),
        Message::AiKey { provider } => {
            // Read, never an argument: arguments show in the process list.
            let mut key = String::new();
            std::io::stdin()
                .read_line(&mut key)
                .context("could not read the key")?;
            client.request(Request::AiSecret {
                provider,
                secret: key.trim().to_owned(),
            })?;
            Ok(())
        }
        Message::SendToPhone { files } => {
            let paths = files
                .iter()
                .map(|f| {
                    std::path::absolute(f)
                        .map(|p| p.to_string_lossy().into_owned())
                        .with_context(|| format!("{}", f.display()))
                })
                .collect::<anyhow::Result<_>>()?;
            print(client.request(Request::SendToPhone { paths })?)
        }
        Message::Ui => match client.request(Request::Ui)? {
            // One line per element, indented as the tree.
            Reply::Ui(nodes) => {
                for n in nodes {
                    let r = n.rect;
                    let label = n
                        .label
                        .map(|l| format!(" \"{}\"", l.replace('"', "\\\"")))
                        .unwrap_or_default();
                    let states = [
                        (n.focused, " focused"),
                        (n.selected, " selected"),
                        (n.disabled, " disabled"),
                    ]
                    .iter()
                    .filter(|(on, _)| *on)
                    .map(|(_, s)| *s)
                    .collect::<String>();
                    out!(
                        "{:indent$}{}{label} {},{} {}x{}{states}",
                        "",
                        n.role,
                        r.x,
                        r.y,
                        r.width,
                        r.height,
                        indent = n.depth as usize * 2
                    );
                }
                Ok(())
            }
            other => print(other),
        },
        Message::AgentsJournal { json } => match client.request(Request::AgentsJournal)? {
            Reply::AgentsJournal(journal) if !json => {
                if journal.stopped {
                    out!("Agents are stopped: their requests are refused until allowed again.");
                }
                for entry in &journal.entries {
                    out!("{entry}");
                }
                Ok(())
            }
            other => print(other),
        },
        Message::Plug { spec, name } => {
            client.request(Request::PlugOutput { spec, name })?;
            Ok(())
        }
        Message::Unplug { output } => {
            client.request(Request::UnplugOutput { output })?;
            Ok(())
        }
        Message::Primary { output } => {
            client.request(Request::SetPrimaryOutput { output })?;
            Ok(())
        }
        Message::ShowDesktop { output, desktop } => {
            client.request(Request::ShowDesktop {
                output,
                index: desktop.saturating_sub(1),
            })?;
            Ok(())
        }
        Message::Send {
            window,
            output,
            desktop,
        } => {
            client.request(Request::SendWindow {
                window,
                output,
                desktop: desktop.map(|n| n.saturating_sub(1)),
            })?;
            Ok(())
        }
        Message::State { window, state } => {
            let state = match state {
                StateArg::Normal => WindowState::Normal,
                StateArg::Maximized => WindowState::Maximized,
                StateArg::Fullscreen => WindowState::Fullscreen,
                StateArg::Minimized => WindowState::Minimized,
            };
            client.request(Request::SetWindowState { window, state })?;
            Ok(())
        }
        Message::Close { window } => {
            client.request(Request::CloseWindow { window })?;
            Ok(())
        }
        Message::Screenshot {
            output,
            window,
            file,
            cursor,
        } => {
            let target = match (output, window) {
                (Some(name), _) => Target::Output(name),
                (None, Some(id)) => Target::Window(id),
                (None, None) => anyhow::bail!("say which: --output NAME or --window ID"),
            };
            let Reply::Screenshot(shot) = client.request(Request::Screenshot { target, cursor })?
            else {
                anyhow::bail!("the compositor did not send a screenshot");
            };
            std::fs::write(&file, shot.png()?)
                .with_context(|| format!("could not write {}", file.display()))?;
            out!("{}x{} → {}", shot.width, shot.height, file.display());
            Ok(())
        }
    }
}

/// Prints what is inside the reply, without the reply's own tag.
fn print(reply: Reply) -> anyhow::Result<()> {
    let value = match serde_json::to_value(&reply)? {
        serde_json::Value::Object(map) if map.len() == 1 => {
            map.into_iter().next().map(|(_, v)| v).unwrap_or_default()
        }
        other => other,
    };
    out!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
