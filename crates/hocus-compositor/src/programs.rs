//! Programs the compositor starts: those given with `--spawn`, once it is
//! ready to take them (after XWayland when it runs, as they may be X11
//! ones), and the terminal of `Super+Return`. Each starts and is waited for
//! on a thread of its own, so that neither a slow fork, a look through
//! `PATH` nor a program's exit ever holds the loop.

use std::{
    ffi::OsStr,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// The terminals `Super+Return` looks for in `PATH`, in this order, when the
/// configuration names none.
pub const TERMINALS: [&str; 4] = ["kitty", "foot", "alacritty", "xterm"];

/// What a started program is told about the compositor.
#[derive(Debug, Clone)]
pub struct Env {
    pub runtime_dir: PathBuf,
    pub socket: PathBuf,
    pub xdisplay: Option<u32>,
    /// An activation token the program's first window may take the focus
    /// with (`XDG_ACTIVATION_TOKEN`, `DESKTOP_STARTUP_ID`).
    pub activation: Option<String>,
    /// The cursor's theme and size, for the program's to match.
    pub cursor: crate::cursor::Choice,
}

impl Env {
    pub fn of(state: &crate::State) -> Self {
        Self {
            runtime_dir: state.runtime_dir.clone(),
            socket: state.socket_path.clone(),
            xdisplay: state.xdisplay,
            activation: None,
            cursor: state.cursor_images.choice().clone(),
        }
    }

    pub fn with_activation(mut self, token: String) -> Self {
        self.activation = Some(token);
        self
    }

    /// `WAYLAND_DISPLAY`: the socket's name when it is in the runtime
    /// directory, as some programs expect, its path otherwise.
    pub fn wayland_display(&self) -> &OsStr {
        match self.socket.strip_prefix(&self.runtime_dir) {
            Ok(name) if name.parent() == Some(Path::new("")) => name.as_os_str(),
            _ => self.socket.as_os_str(),
        }
    }

    /// `DISPLAY`, when XWayland runs.
    pub fn display(&self) -> Option<String> {
        self.xdisplay.map(|n| format!(":{n}"))
    }

    /// A command connected to this compositor only.
    pub fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let mut cmd = Command::new(program);
        cmd.stdin(Stdio::null())
            .env("XDG_RUNTIME_DIR", &self.runtime_dir)
            .env("WAYLAND_DISPLAY", self.wayland_display())
            .env("XDG_SESSION_TYPE", "wayland")
            .env("XDG_CURRENT_DESKTOP", "Hocuspanda")
            .env("XCURSOR_THEME", &self.cursor.theme)
            .env("XCURSOR_SIZE", self.cursor.size.to_string())
            .env_remove("WAYLAND_SOCKET")
            .env_remove("DISPLAY");
        if let Some(display) = self.display() {
            cmd.env("DISPLAY", display);
        }
        match &self.activation {
            Some(token) => {
                cmd.env("XDG_ACTIVATION_TOKEN", token)
                    .env("DESKTOP_STARTUP_ID", token);
            }
            None => {
                cmd.env_remove("XDG_ACTIVATION_TOKEN")
                    .env_remove("DESKTOP_STARTUP_ID");
            }
        }
        cmd
    }
}

/// What waits for the compositor to be ready.
#[derive(Debug, Default)]
pub enum Programs {
    #[default]
    Started,
    Waiting {
        commands: Vec<String>,
        /// Tell the user's service manager and D-Bus where the session is.
        announce: bool,
    },
}

impl Programs {
    pub fn wait(&mut self, commands: Vec<String>, announce: bool) {
        *self = Self::Waiting { commands, announce };
    }

    /// The compositor is ready: the session is announced, and the commands
    /// waiting start.
    pub fn ready(&mut self, env: &Env, mut token: impl FnMut() -> String) {
        if let Self::Waiting { commands, announce } = std::mem::take(self) {
            if announce {
                crate::session::announce(env.clone());
            }
            for command in commands {
                // Started with the session: its window may take the focus.
                spawn(command, env.clone().with_activation(token()));
            }
        }
    }
}

impl crate::State {
    /// The compositor (and XWayland, when it runs) is ready: the programs
    /// waiting start.
    pub fn programs_ready(&mut self) {
        let env = Env::of(self);
        let mut programs = std::mem::take(&mut self.programs);
        programs.ready(&env, || self.activation_token());
    }
}

/// Starts `command` through the shell, connected to this compositor only.
pub fn spawn(command: String, env: Env) {
    on_a_thread(format!("spawn {command}"), move || run(&command, &env));
}

/// Starts the terminal: `configured`, or the first of [`TERMINALS`] in
/// `PATH`.
pub fn spawn_terminal(configured: String, env: Env) {
    on_a_thread("terminal".to_owned(), move || {
        let path = std::env::var_os("PATH");
        match terminal(&configured, path.as_deref()) {
            Some(command) => run(&command, &env),
            None => tracing::warn!(
                looked_for = ?TERMINALS,
                "no terminal in PATH; set [programs] terminal in config.toml"
            ),
        }
    });
}

/// The terminal's command: the configured one, or the first of
/// [`TERMINALS`] that is an executable file in `path`.
pub fn terminal(configured: &str, path: Option<&OsStr>) -> Option<String> {
    if !configured.trim().is_empty() {
        return Some(configured.to_owned());
    }
    let dirs: Vec<PathBuf> = std::env::split_paths(path?).collect();
    TERMINALS
        .into_iter()
        .find(|name| dirs.iter().any(|dir| executable(&dir.join(name))))
        .map(str::to_owned)
}

fn executable(file: &Path) -> bool {
    std::fs::metadata(file).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

pub fn on_a_thread(name: String, work: impl FnOnce() + Send + 'static) {
    if let Err(err) = std::thread::Builder::new().name(name).spawn(work) {
        tracing::warn!(%err, "could not start a thread for a program");
    }
}

/// Runs `command` through the shell and waits for it, on the calling
/// thread.
fn run(command: &str, env: &Env) {
    let mut cmd = env.command("/bin/sh");
    cmd.arg("-c").arg(command);
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(dir: &Path, name: &str, mode: u32) -> anyhow::Result<()> {
        let file = dir.join(name);
        std::fs::write(&file, "#!/bin/sh\n")?;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode))?;
        Ok(())
    }

    #[test]
    fn the_first_terminal_found_in_path_in_the_list_order() -> anyhow::Result<()> {
        let a = tempfile::tempdir()?;
        let b = tempfile::tempdir()?;
        program(a.path(), "xterm", 0o755)?;
        program(b.path(), "foot", 0o755)?;
        // Not executable, or not a file: passed over.
        program(a.path(), "kitty", 0o644)?;
        std::fs::create_dir(b.path().join("alacritty"))?;
        let path = std::env::join_paths([a.path(), b.path()])?;
        assert_eq!(terminal("", Some(&path)).as_deref(), Some("foot"));
        program(b.path(), "kitty", 0o700)?;
        assert_eq!(terminal("", Some(&path)).as_deref(), Some("kitty"));
        Ok(())
    }

    #[test]
    fn the_configured_terminal_wins_and_none_is_none() -> anyhow::Result<()> {
        let empty = tempfile::tempdir()?;
        let path = std::env::join_paths([empty.path()])?;
        assert_eq!(terminal("", Some(&path)), None);
        assert_eq!(terminal("", None), None);
        assert_eq!(
            terminal("wezterm start", Some(&path)).as_deref(),
            Some("wezterm start")
        );
        Ok(())
    }

    #[test]
    fn wayland_display_is_the_sockets_name_in_the_runtime_dir() {
        let env = |socket: &str| Env {
            runtime_dir: PathBuf::from("/run/user/1000"),
            socket: PathBuf::from(socket),
            xdisplay: Some(1),
            activation: None,
            cursor: crate::cursor::Choice::of(None, None, &crate::CursorEnv::default()),
        };
        assert_eq!(env("/run/user/1000/hocus-0").wayland_display(), "hocus-0");
        assert_eq!(env("/tmp/hp/hocus-0").wayland_display(), "/tmp/hp/hocus-0");
        assert_eq!(env("/tmp/hp/hocus-0").display().as_deref(), Some(":1"));
    }
}
