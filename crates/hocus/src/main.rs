//! Command-line tool to control and test Hocuspanda.

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use hocus_api::{Button, Client, Reply, Request, Target};

/// Control and test Hocuspanda.
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
}

#[derive(Debug, Subcommand)]
enum Message {
    /// Protocol and compositor versions.
    Version,
    /// Outputs with their logical geometry, pixel size and scale.
    Outputs,
    /// Windows with their id, app id, output, geometry and focus.
    Windows,
    /// The whole scene (needs the test permission).
    Scene,
    /// Print events as they happen, one JSON object per line.
    Events,
    /// Move the pointer to a point of the global logical space.
    PointerMove { x: f64, y: f64 },
    /// Press and release a pointer button.
    Click {
        #[arg(value_enum, default_value_t = ButtonArg::Left)]
        button: ButtonArg,
    },
    /// Press and release a key, by evdev code (KEY_A is 30).
    Key { code: u32 },
    /// Save a PNG of an output or a window.
    Screenshot {
        #[arg(long, value_name = "NAME", conflicts_with = "window")]
        output: Option<String>,
        #[arg(long, value_name = "ID")]
        window: Option<u64>,
        /// Where to write the PNG.
        #[arg(short = 'o', long = "to", value_name = "FILE")]
        file: PathBuf,
    },
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

fn main() -> anyhow::Result<()> {
    let Command::Msg { socket, message } = Args::parse().command;
    let socket = socket
        .or_else(hocus_api::default_socket)
        .context("no control socket: set HOCUS_SOCKET or WAYLAND_DISPLAY, or pass --socket")?;
    let mut client = Client::connect(&socket)
        .with_context(|| format!("could not connect to {}", socket.display()))?;
    match message {
        Message::Version => print(client.request(Request::Version)?),
        Message::Outputs => print(client.request(Request::Outputs)?),
        Message::Windows => print(client.request(Request::Windows)?),
        Message::Scene => print(client.request(Request::Scene)?),
        Message::Events => {
            client.request(Request::Subscribe)?;
            loop {
                println!("{}", serde_json::to_string(&client.next_event()?)?);
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
        Message::Key { code } => {
            for pressed in [true, false] {
                client.request(Request::Key { code, pressed })?;
            }
            Ok(())
        }
        Message::Screenshot {
            output,
            window,
            file,
        } => {
            let target = match (output, window) {
                (Some(name), _) => Target::Output(name),
                (None, Some(id)) => Target::Window(id),
                (None, None) => anyhow::bail!("say which: --output NAME or --window ID"),
            };
            let Reply::Screenshot(shot) = client.request(Request::Screenshot { target })? else {
                anyhow::bail!("the compositor did not send a screenshot");
            };
            std::fs::write(&file, shot.png()?)
                .with_context(|| format!("could not write {}", file.display()))?;
            println!("{}x{} → {}", shot.width, shot.height, file.display());
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
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
