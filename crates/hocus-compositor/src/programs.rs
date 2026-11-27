//! Programs the compositor starts: those given with `--spawn`, once it is
//! ready to take them (after XWayland when it runs, as they may be X11
//! ones), and the terminal of `Super+Return`. Each starts and is waited for
//! on a thread of its own, so that neither a slow fork, a look through
//! `PATH` nor a program's exit ever holds the loop.
//!
//! Every program starts with no signal blocked ([`default_signals`]): the
//! loop blocks SIGTERM and SIGINT to read them itself, and a mask survives
//! `exec`, so without it `kill` would do nothing to them (#733).

use std::{
    ffi::OsStr,
    io::Write,
    os::{
        fd::OwnedFd,
        unix::{fs::PermissionsExt, net::UnixStream},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// The terminals `Super+Return` looks for in `PATH`, in this order, when the
/// configuration names none.
pub const TERMINALS: [&str; 4] = ["kitty", "foot", "alacritty", "xterm"];

/// The signals whose disposition a program gets back at their default.
const DEFAULT_SIGNALS: [i32; 4] = [libc::SIGTERM, libc::SIGINT, libc::SIGHUP, libc::SIGQUIT];

/// `cmd`'s program starts as a program should: no signal blocked, and
/// SIGTERM, SIGINT, SIGHUP and SIGQUIT at their default. The compositor's
/// threads block SIGTERM and SIGINT (the loop reads them from a signalfd,
/// `calloop::signals`), the mask is inherited by a child and kept across
/// `exec`, and std leaves it so. Every program the compositor starts goes
/// through here.
#[allow(unsafe_code)]
pub fn default_signals(cmd: &mut Command) -> &mut Command {
    use std::os::unix::process::CommandExt;
    // SAFETY: the closure runs in the child between fork and exec, where
    // only async-signal-safe calls are allowed: sigemptyset, sigprocmask
    // and sigaction are, and it allocates nothing. The structures are
    // plain data, zeroed then filled by those calls.
    unsafe {
        cmd.pre_exec(|| {
            let mut none: libc::sigset_t = std::mem::zeroed();
            if libc::sigemptyset(&mut none) != 0
                || libc::sigprocmask(libc::SIG_SETMASK, &none, std::ptr::null_mut()) != 0
            {
                return Err(std::io::Error::last_os_error());
            }
            for signal in DEFAULT_SIGNALS {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = libc::SIG_DFL;
                if libc::sigemptyset(&mut action.sa_mask) != 0
                    || libc::sigaction(signal, &action, std::ptr::null_mut()) != 0
                {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    cmd
}

/// The journal's socket for programs' output, as systemd-cat uses it.
pub const JOURNAL_STREAMS: &str = "/run/systemd/journal/stdout";

/// The name the desktop's own programs write under in the journal, the
/// compositor's (`packaging/hocuspanda-session`): `journalctl --user -t
/// hocuspanda` shows them all.
pub const DESKTOP_LOG: &str = "hocuspanda";

/// Where the programs the compositor starts write their stdout and stderr
/// (#965).
///
/// In a session, the compositor's stderr is a pipe to the journal: a
/// program writing there in turn with the others, and the compositor
/// with them, waits when the journal is slow, and an application waiting
/// on its stderr stops reading its Wayland connection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Output {
    /// Where the compositor writes: nested or headless, a developer's
    /// terminal or a test's output.
    #[default]
    Shared,
    /// The journal, through its socket at this path ([`JOURNAL_STREAMS`]):
    /// a stream of its own for each program, under its name. The
    /// session's.
    Journal(PathBuf),
}

/// A program's stdout and stderr.
#[derive(Debug)]
pub struct Streams {
    pub out: Stdio,
    pub err: Stdio,
}

impl Streams {
    fn inherited() -> Self {
        Self {
            out: Stdio::inherit(),
            err: Stdio::inherit(),
        }
    }

    /// `cmd` writes there.
    pub fn on(self, cmd: &mut Command) -> &mut Command {
        cmd.stdout(self.out).stderr(self.err)
    }
}

impl Output {
    /// A program of the desktop's own (the shell, the camera, the
    /// microphone, the transcriber): a stream of its own under
    /// [`DESKTOP_LOG`]; the compositor's when the journal takes none.
    pub fn desktop(&self) -> Streams {
        self.journal(DESKTOP_LOG).unwrap_or_else(Streams::inherited)
    }

    /// An application, `name` its desktop entry's id or its program's
    /// name: a stream of its own under that name (`journalctl --user -t
    /// NAME`); nowhere when the journal takes none, never the
    /// compositor's.
    pub fn application(&self, name: &str) -> Streams {
        match self {
            Self::Shared => Streams::inherited(),
            Self::Journal(_) => self.journal(name).unwrap_or_else(|| Streams {
                out: Stdio::null(),
                err: Stdio::null(),
            }),
        }
    }

    /// A journal stream named `name`, when there is a journal and it
    /// takes it.
    fn journal(&self, name: &str) -> Option<Streams> {
        let Self::Journal(socket) = self else {
            return None;
        };
        match journal_stream(socket, name) {
            Ok(fd) => {
                let out = fd.try_clone().ok()?;
                Some(Streams {
                    out: Stdio::from(out),
                    err: Stdio::from(fd),
                })
            }
            Err(err) => {
                tracing::debug!(%err, name, "the journal took no stream for a program");
                None
            }
        }
    }
}

/// A stream into the journal at `socket`, its lines under `name`, at the
/// info level unless a line says its own (`<3>`), as systemd-cat's
/// (`sd_journal_stream_fd`). Never waits: a journal too busy to take it
/// now gives none.
fn journal_stream(socket: &Path, name: &str) -> std::io::Result<OwnedFd> {
    use rustix::net::{AddressFamily, SocketAddrUnix, SocketFlags, SocketType};
    let fd = rustix::net::socket_with(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
        None,
    )?;
    rustix::net::connect(&fd, &SocketAddrUnix::new(socket)?)?;
    let mut stream = UnixStream::from(fd);
    // One line each: the name, no unit, the priority (info), the level
    // prefix read, and nothing forwarded to syslog, the kernel's log or
    // the console. A name holds no line's end.
    let name: String = name
        .chars()
        .map(|c| if c.is_control() { '_' } else { c })
        .collect();
    stream.write_all(format!("{name}\n\n6\n1\n0\n0\n0\n").as_bytes())?;
    stream.shutdown(std::net::Shutdown::Read)?;
    // The program writes as it would to a terminal: waiting when full.
    stream.set_nonblocking(false)?;
    Ok(OwnedFd::from(stream))
}

/// The name a shell command's program writes under: its first word's
/// file name.
fn program_name(command: &str) -> &str {
    let first = command.split_whitespace().next().unwrap_or(command);
    first.rsplit('/').next().unwrap_or(first)
}

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
    /// The session bus given (`--session-bus`), which the programs use.
    pub session_bus: Option<String>,
    /// The configuration file read (`HOCUS_CONFIG`): the Settings started
    /// from the desktop write into this one.
    pub config: Option<PathBuf>,
    /// `LANGUAGE`, when `[locale] language` is not the system's (#402).
    pub language: Option<&'static str>,
    /// Where it writes its output (#965).
    pub output: Output,
}

impl Env {
    pub fn of(state: &crate::State) -> Self {
        Self {
            runtime_dir: state.runtime_dir.clone(),
            socket: state.socket_path.clone(),
            xdisplay: state.xwayland.display,
            activation: None,
            cursor: state.pointer.cursor_images.choice().clone(),
            session_bus: match &state.shell.bus {
                Some(crate::shell::Bus::Given(address)) => Some(address.clone()),
                _ => None,
            },
            config: state.config.file.clone(),
            language: state.config.locale.language.env(),
            output: state.config.program_output.clone(),
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
        default_signals(&mut cmd)
            .stdin(Stdio::null())
            .env("XDG_RUNTIME_DIR", &self.runtime_dir)
            .env("WAYLAND_DISPLAY", self.wayland_display())
            .env("XDG_SESSION_TYPE", "wayland")
            .env(
                hocus_agents::SOCKET_ENV,
                hocus_agents::socket_path(&self.socket),
            )
            .env("XDG_CURRENT_DESKTOP", crate::session::DESKTOP)
            .env("XCURSOR_THEME", &self.cursor.theme)
            .env("XCURSOR_SIZE", self.cursor.size.to_string())
            .env_remove("WAYLAND_SOCKET")
            .env_remove("DISPLAY");
        if let Some(display) = self.display() {
            cmd.env("DISPLAY", display);
        }
        if let Some(bus) = &self.session_bus {
            cmd.env("DBUS_SESSION_BUS_ADDRESS", bus);
        }
        if let Some(language) = self.language {
            cmd.env("LANGUAGE", language);
        }
        // Empty: no file, never the one a program would look for.
        cmd.env(
            hocus_link::CONFIG_ENV,
            self.config
                .as_deref()
                .map(Path::as_os_str)
                .unwrap_or_default(),
        );
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
        self.start_shell();
    }
}

/// Starts `command` through the shell, connected to this compositor only.
pub fn spawn(command: String, env: Env) {
    on_a_thread(format!("spawn {command}"), move || run(&command, &env));
}

/// Starts an application the launcher asks for (5.1): its arguments as
/// they are, without a shell, in its working directory when it names one
/// that is there; one that runs in a terminal (`terminal`: the configured
/// one, empty for the first of [`TERMINALS`] in `PATH`) after its `-e`.
pub fn launch(launch: hocus_link::Launch, terminal: Option<String>, env: Env) {
    launch_watched(launch, terminal, env, |_| {});
}

/// What became of an application started by [`launch_watched`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It runs: its process.
    Started(u32),
    /// It could not start: no terminal for it, or its program would not
    /// run.
    Failed,
    /// It ended; successfully or not.
    Ended(bool),
}

/// [`launch`], saying what becomes of it to `said`, from its thread (the
/// session's applications started again, windows.md §14).
pub fn launch_watched(
    launch: hocus_link::Launch,
    terminal: Option<String>,
    env: Env,
    said: impl Fn(Outcome) + Send + 'static,
) {
    on_a_thread(format!("launch {}", launch.id), move || {
        let Some(mut cmd) = command_for(&launch, terminal, &env) else {
            said(Outcome::Failed);
            return;
        };
        match cmd.spawn() {
            Ok(mut child) => {
                tracing::info!(app = launch.id, pid = child.id(), "started");
                said(Outcome::Started(child.id()));
                match child.wait() {
                    Ok(status) => {
                        tracing::info!(app = launch.id, %status, "ended");
                        said(Outcome::Ended(status.success()));
                    }
                    Err(err) => tracing::warn!(app = launch.id, %err, "could not wait for it"),
                }
            }
            Err(err) => {
                tracing::warn!(app = launch.id, %err, "could not start");
                said(Outcome::Failed);
            }
        }
    });
}

/// The command that starts `launch`, as [`launch`] says; `None` when it
/// needs a terminal and none is found (said in the log).
fn command_for(
    launch: &hocus_link::Launch,
    terminal: Option<String>,
    env: &Env,
) -> Option<Command> {
    let mut cmd = match terminal {
        Some(configured) => {
            let path = std::env::var_os("PATH");
            let Some(terminal) = self::terminal(&configured, path.as_deref()) else {
                tracing::warn!(
                    app = launch.id,
                    looked_for = ?TERMINALS,
                    "no terminal in PATH for an application that needs one"
                );
                return None;
            };
            // The terminal as configured may hold its own arguments.
            let mut cmd = env.command("/bin/sh");
            cmd.arg("-c")
                .arg(format!("{terminal} -e \"$@\""))
                .arg("sh")
                .args(&launch.argv);
            cmd
        }
        None => {
            let (program, args) = launch.argv.split_first()?;
            let mut cmd = env.command(program);
            cmd.args(args);
            cmd
        }
    };
    if !launch.dir.is_empty() && Path::new(&launch.dir).is_dir() {
        cmd.current_dir(&launch.dir);
    }
    env.output.application(&launch.id).on(&mut cmd);
    Some(cmd)
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

/// The terminal (`configured`, else the first found) opens in `folder`
/// (docs/design/projects.md §13, B): its working directory, and its own
/// option where §8.2's table has one.
pub fn spawn_terminal_in(configured: String, folder: PathBuf, env: Env) {
    on_a_thread("terminal".to_owned(), move || {
        let path = std::env::var_os("PATH");
        let Some(command) = terminal(&configured, path.as_deref()) else {
            tracing::warn!("no terminal in PATH; set [programs] terminal in config.toml");
            return;
        };
        let program = program_name(&command).to_owned();
        let extra: Vec<String> = crate::terminals::in_folder(&program, &[], &folder)
            .into_iter()
            .map(|a| format!("'{}'", a.replace('\'', "'\\''")))
            .collect();
        let command = if extra.is_empty() {
            command
        } else {
            format!("{command} {}", extra.join(" "))
        };
        let mut cmd = env.command("/bin/sh");
        cmd.arg("-c").arg(&command).current_dir(&folder);
        env.output.application(&program).on(&mut cmd);
        match cmd.spawn() {
            Ok(mut child) => {
                tracing::info!(pid = child.id(), "a terminal in a project's folder");
                let _ = child.wait();
            }
            Err(err) => tracing::warn!(%err, "could not start the terminal"),
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
    env.output.application(program_name(command)).on(&mut cmd);
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

    fn env(socket: &str) -> Env {
        Env {
            runtime_dir: PathBuf::from("/run/user/1000"),
            socket: PathBuf::from(socket),
            xdisplay: Some(1),
            activation: None,
            cursor: crate::cursor::Choice::of(None, None, &crate::CursorEnv::default()),
            session_bus: None,
            config: Some(PathBuf::from("/home/alex/.config/hocuspanda/config.toml")),
            language: None,
            output: Output::Shared,
        }
    }

    /// What the journal at `socket` read on its first stream: what was
    /// said before its lines, and its lines.
    fn journal_read(listener: &std::os::unix::net::UnixListener) -> anyhow::Result<String> {
        use std::io::Read;
        let (mut stream, _) = listener.accept()?;
        let mut text = String::new();
        stream.read_to_string(&mut text)?;
        Ok(text)
    }

    #[test]
    fn an_application_writes_into_a_journal_stream_of_its_own() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let socket = dir.path().join("stdout");
        let listener = std::os::unix::net::UnixListener::bind(&socket)?;
        let output = Output::Journal(socket);
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg("echo out; echo err >&2");
        output.application("org.example.App").on(&mut cmd);
        let status = cmd.status()?;
        assert!(status.success());
        // The command keeps its copy of the stream until it goes.
        drop(cmd);
        assert_eq!(
            journal_read(&listener)?,
            "org.example.App\n\n6\n1\n0\n0\n0\nout\nerr\n"
        );
        // The desktop's own under the compositor's name.
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg("echo said");
        output.desktop().on(&mut cmd);
        assert!(cmd.status()?.success());
        drop(cmd);
        assert_eq!(
            journal_read(&listener)?,
            "hocuspanda\n\n6\n1\n0\n0\n0\nsaid\n"
        );
        Ok(())
    }

    #[test]
    fn without_a_journal_an_application_writes_nowhere_and_still_runs() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let output = Output::Journal(dir.path().join("none"));
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg("echo out; echo err >&2");
        output.application("org.example.App").on(&mut cmd);
        let out = cmd.output()?;
        assert!(out.status.success());
        assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
        Ok(())
    }

    #[test]
    fn a_command_writes_under_its_program_s_name() {
        assert_eq!(program_name("/usr/bin/kitty -1 --title x"), "kitty");
        assert_eq!(program_name("foot"), "foot");
    }

    /// The signals `/proc` says process `pid` has blocked, once it has
    /// exec'd `sleep`.
    fn blocked(pid: u32) -> anyhow::Result<String> {
        let status = format!("/proc/{pid}/status");
        for _ in 0..500 {
            let text = std::fs::read_to_string(&status)?;
            if text.lines().any(|l| l == "Name:\tsleep") {
                let line = text.lines().find(|l| l.starts_with("SigBlk:"));
                return Ok(line.unwrap_or_default().to_owned());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        anyhow::bail!("{pid} never ran sleep")
    }

    /// A thread blocking SIGTERM and SIGINT, as the loop's do: a program
    /// started as std does has them blocked; started through
    /// [`default_signals`], as [`Env::command`] does for every program,
    /// none. Not through `sh`: dash clears its mask as it starts.
    #[test]
    #[allow(unsafe_code)]
    fn a_program_starts_with_no_signal_blocked() -> anyhow::Result<()> {
        let masks = std::thread::spawn(|| -> anyhow::Result<(String, String, String)> {
            // SAFETY: the mask of this thread alone, from plain data.
            unsafe {
                let mut set: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&mut set);
                libc::sigaddset(&mut set, libc::SIGTERM);
                libc::sigaddset(&mut set, libc::SIGINT);
                let blocked = libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
                anyhow::ensure!(blocked == 0, "could not block");
            }
            let mask = |mut cmd: Command| -> anyhow::Result<String> {
                let mut child = cmd.arg("600").stdin(Stdio::null()).spawn()?;
                let mask = blocked(child.id());
                let _ = child.kill();
                let _ = child.wait();
                mask
            };
            let plain = mask(Command::new("sleep"))?;
            let mut reset = Command::new("sleep");
            default_signals(&mut reset);
            let reset = mask(reset)?;
            let started = mask(env("/run/user/1000/hocus-0").command("sleep"))?;
            Ok((plain, reset, started))
        })
        .join()
        .map_err(|_| anyhow::anyhow!("the thread panicked"))??;
        // SIGTERM (15) and SIGINT (2): bits 14 and 1.
        assert_eq!(masks.0, "SigBlk:\t0000000000004002");
        assert_eq!(masks.1, "SigBlk:\t0000000000000000");
        assert_eq!(masks.2, "SigBlk:\t0000000000000000");
        Ok(())
    }

    #[test]
    fn wayland_display_is_the_sockets_name_in_the_runtime_dir() {
        // The Settings it starts write the file it read.
        let command = env("/run/user/1000/hocus-0").command("hocus-settings");
        let config = command
            .get_envs()
            .find(|(name, _)| *name == hocus_link::CONFIG_ENV)
            .and_then(|(_, value)| value);
        assert_eq!(
            config,
            Some(std::ffi::OsStr::new(
                "/home/alex/.config/hocuspanda/config.toml"
            ))
        );
        assert_eq!(env("/run/user/1000/hocus-0").wayland_display(), "hocus-0");
        assert_eq!(env("/tmp/hp/hocus-0").wayland_display(), "/tmp/hp/hocus-0");
        assert_eq!(env("/tmp/hp/hocus-0").display().as_deref(), Some(":1"));
    }
}
