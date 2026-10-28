//! Programs started with the compositor (`--spawn`), once it is ready to
//! take them: after XWayland when it runs, as they may be X11 ones. Each
//! starts and is waited for on a thread of its own, so that neither a slow
//! fork nor its exit ever holds the loop.

use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

/// What a started program is told about the compositor.
#[derive(Debug, Clone)]
pub struct Env {
    pub runtime_dir: PathBuf,
    pub socket: PathBuf,
    pub xdisplay: Option<u32>,
}

impl Env {
    pub fn of(state: &crate::State) -> Self {
        Self {
            runtime_dir: state.runtime_dir.clone(),
            socket: state.socket_path.clone(),
            xdisplay: state.xdisplay,
        }
    }
}

/// The commands waiting for the compositor to be ready.
#[derive(Debug, Default)]
pub enum Programs {
    #[default]
    Started,
    Waiting(Vec<String>),
}

impl Programs {
    pub fn wait(&mut self, commands: Vec<String>) {
        *self = Self::Waiting(commands);
    }

    /// The compositor is ready: the commands waiting start.
    pub fn ready(&mut self, env: &Env) {
        if let Self::Waiting(commands) = std::mem::take(self) {
            for command in commands {
                spawn(command, env.clone());
            }
        }
    }
}

/// Starts `command` through the shell, connected to this compositor only.
pub fn spawn(command: String, env: Env) {
    let name = format!("spawn {command}");
    let started = std::thread::Builder::new().name(name).spawn(move || {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c")
            .arg(&command)
            .stdin(Stdio::null())
            .env("XDG_RUNTIME_DIR", &env.runtime_dir)
            .env("WAYLAND_DISPLAY", &env.socket)
            .env("XDG_SESSION_TYPE", "wayland")
            .env("XDG_CURRENT_DESKTOP", "Hocuspanda")
            .env_remove("WAYLAND_SOCKET")
            .env_remove("DISPLAY");
        if let Some(n) = env.xdisplay {
            cmd.env("DISPLAY", format!(":{n}"));
        }
        match cmd.spawn() {
            Ok(mut child) => {
                tracing::info!(command, pid = child.id(), "started");
                match child.wait() {
                    Ok(status) => tracing::info!(command, %status, "ended"),
                    Err(err) => tracing::warn!(command, %err, "could not wait for it"),
                }
            }
            Err(err) => tracing::warn!(command, %err, "could not start"),
        }
    });
    if let Err(err) = started {
        tracing::warn!(%err, "could not start a thread for a program");
    }
}
