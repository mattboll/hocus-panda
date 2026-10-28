//! The shell (`hocuspanda-shell`, crates/hocus-shell): the compositor starts
//! it once ready, restarts it when it ends (sooner or later as it keeps
//! ending), and talks to it over a private socket (`hocus-link`).
//!
//! The socket sits next to the Wayland one, `<socket>.shell`, in the
//! session's runtime directory; its path goes to the shell in
//! `HOCUS_SHELL_SOCKET`. The compositor reads it from the loop without ever
//! waiting: lines come in as they are, and lines out are a few bytes,
//! dropped with the connection should the shell stop reading.
//!
//! The shell is started on the compositor's GPU (tech-choices, "The bar's
//! renderer (3.0)"): wgpu asks for the low-power adapter and the Vulkan
//! loader only loads that GPU's driver, so that a second GPU stays asleep;
//! Mesa is asked for an 8-bit-alpha surface first. Without a GPU, the shell
//! draws with tiny-skia.

use std::{
    io::{ErrorKind, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::Context;
use hocus_link::{ToCompositor, ToShell};
use smithay::reexports::calloop::{
    Interest, LoopHandle, Mode, PostAction, RegistrationToken,
    channel::{self, Event, Sender},
    generic::Generic,
    timer::{TimeoutAction, Timer},
};

use crate::{App, State, backend::DevId, programs};

/// A shell that ran at least this long ended by accident, not in a loop:
/// it is started again at once.
const STEADY: Duration = Duration::from_secs(30);
/// The first restart's delay, doubled at each quick end, up to the last.
const FIRST_DELAY: Duration = Duration::from_millis(500);
const LAST_DELAY: Duration = Duration::from_secs(30);

/// The shell, as the compositor sees it.
#[derive(Debug, Default)]
pub struct Shell {
    /// Its program; `None`: no shell (tests, `--no-shell`).
    pub program: Option<PathBuf>,
    /// The socket it reaches the compositor on.
    pub socket: Option<PathBuf>,
    link: Option<Link>,
    /// It draws on the CPU: its bars are opaque.
    pub software: bool,
    /// Ends in a row that came quickly.
    quick_ends: u32,
    ended: Option<Sender<Duration>>,
}

/// The shell's connection.
#[derive(Debug)]
struct Link {
    writer: UnixStream,
    token: RegistrationToken,
}

/// The shell's socket's path next to the Wayland socket's.
pub fn socket_path(wayland: &Path) -> PathBuf {
    let mut path = wayland.as_os_str().to_owned();
    path.push(".shell");
    PathBuf::from(path)
}

/// Listens on the shell's socket, and learns when the shell ends.
pub fn listen(handle: &LoopHandle<'static, App>, state: &mut State) -> anyhow::Result<()> {
    let path = socket_path(&state.socket_path);
    // A socket left by a compositor that did not end well.
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("could not remove {}", path.display()))?;
    }
    let listener = UnixListener::bind(&path)
        .with_context(|| format!("could not listen on {}", path.display()))?;
    listener
        .set_nonblocking(true)
        .context("could not make the shell's socket non-blocking")?;
    handle
        .insert_source(
            Generic::new(listener, Interest::READ, Mode::Level),
            |_, listener, app| {
                loop {
                    match listener.accept() {
                        Ok((stream, _)) => app.state.shell_connected(stream),
                        Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                        Err(err) => {
                            tracing::warn!(%err, "could not take the shell's connection");
                            break;
                        }
                    }
                }
                Ok(PostAction::Continue)
            },
        )
        .map_err(|err| err.error)
        .context("could not watch the shell's socket")?;
    let (sender, ended) = channel::channel::<Duration>();
    handle
        .insert_source(ended, |event, _, app| {
            if let Event::Msg(ran) = event {
                app.state.shell_ended(ran);
            }
        })
        .map_err(|err| err.error)
        .context("could not watch the shell")?;
    state.shell.socket = Some(path);
    state.shell.ended = Some(sender);
    Ok(())
}

impl State {
    /// Starts the shell, on a thread that waits for it to end.
    pub fn start_shell(&mut self) {
        let (Some(program), Some(socket), Some(ended)) = (
            self.shell.program.clone(),
            self.shell.socket.clone(),
            self.shell.ended.clone(),
        ) else {
            return;
        };
        let env = programs::Env::of(self);
        let gpu = self.gpu;
        programs::on_a_thread("shell".to_owned(), move || {
            let started = Instant::now();
            let mut command = env.command(&program);
            command.env(hocus_link::SOCKET_ENV, &socket);
            for (name, value) in gpu_env(gpu) {
                command.env(name, value);
            }
            match command.spawn() {
                Ok(mut child) => {
                    tracing::info!(program = %program.display(), pid = child.id(), "shell started");
                    match child.wait() {
                        Ok(status) => tracing::warn!(%status, "the shell ended"),
                        Err(err) => tracing::warn!(%err, "could not wait for the shell"),
                    }
                }
                Err(err) => {
                    tracing::warn!(program = %program.display(), %err, "could not start the shell");
                }
            }
            // The loop is gone when the compositor is: nothing to restart.
            let _ = ended.send(started.elapsed());
        });
    }

    /// The shell ended after running `ran`: started again, later each time
    /// it ends quickly. Its bars' space stays kept meanwhile.
    fn shell_ended(&mut self, ran: Duration) {
        if self.ending.is_some() {
            return;
        }
        if ran >= STEADY {
            self.shell.quick_ends = 0;
        }
        let delay = FIRST_DELAY
            .saturating_mul(1 << self.shell.quick_ends.min(8))
            .min(LAST_DELAY);
        self.shell.quick_ends += 1;
        tracing::info!(?delay, "the shell starts again");
        let restart = self
            .loop_handle
            .insert_source(Timer::from_duration(delay), |_, _, app| {
                app.state.start_shell();
                TimeoutAction::Drop
            });
        if let Err(err) = restart {
            tracing::warn!(%err.error, "could not schedule the shell's restart");
        }
    }

    /// The shell connected (again): it is told how bars look and where
    /// they are.
    fn shell_connected(&mut self, stream: UnixStream) {
        self.shell_gone();
        if let Err(err) = stream.set_nonblocking(true) {
            tracing::warn!(%err, "could not make the shell's connection non-blocking");
            return;
        }
        let writer = match stream.try_clone() {
            Ok(writer) => writer,
            Err(err) => {
                tracing::warn!(%err, "could not keep the shell's connection");
                return;
            }
        };
        let mut pending = Vec::new();
        let token = self.loop_handle.insert_source(
            Generic::new(stream, Interest::READ, Mode::Level),
            move |_, stream, app| {
                let mut buffer = [0u8; 1024];
                loop {
                    match (&**stream).read(&mut buffer) {
                        Ok(0) => {
                            app.state.shell_gone();
                            return Ok(PostAction::Remove);
                        }
                        Ok(n) => pending.extend_from_slice(&buffer[..n]),
                        Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                        Err(err) if err.kind() == ErrorKind::Interrupted => {}
                        Err(err) => {
                            tracing::warn!(%err, "could not read from the shell");
                            app.state.shell_gone();
                            return Ok(PostAction::Remove);
                        }
                    }
                }
                while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                    let line: Vec<u8> = pending.drain(..=end).collect();
                    let line = String::from_utf8_lossy(&line);
                    match line.trim().parse::<ToCompositor>() {
                        Ok(message) => app.state.shell_says(message),
                        Err(err) => {
                            tracing::warn!(%err, "a line from the shell that does not read")
                        }
                    }
                }
                Ok(PostAction::Continue)
            },
        );
        let token = match token {
            Ok(token) => token,
            Err(err) => {
                tracing::warn!(%err.error, "could not read the shell's connection");
                return;
            }
        };
        tracing::info!("the shell is connected");
        self.shell.link = Some(Link { writer, token });
        self.tell_shell(self.bar_look());
        let bars: Vec<ToShell> = self
            .panel
            .bars
            .iter()
            .map(|(output, bar)| ToShell::Bar {
                output: output.clone(),
                form: bar.form,
                since: 0,
            })
            .collect();
        for bar in bars {
            self.tell_shell(bar);
        }
    }

    /// The shell's connection ended.
    fn shell_gone(&mut self) {
        if let Some(link) = self.shell.link.take() {
            self.loop_handle.remove(link.token);
            tracing::info!("the shell is gone; its bars' space stays");
        }
        if self.shell.software {
            self.shell.software = false;
            self.redraw = true;
        }
    }

    fn shell_says(&mut self, message: ToCompositor) {
        match message {
            ToCompositor::Software => {
                tracing::info!("the shell draws on the CPU: no glass behind its bars");
                self.shell.software = true;
                self.redraw = true;
                self.tell_shell(self.bar_look());
            }
        }
    }

    /// Whether the shell is connected.
    pub fn shell_linked(&self) -> bool {
        self.shell.link.is_some()
    }

    /// How bars look now.
    pub fn bar_look(&self) -> ToShell {
        ToShell::Look {
            glass: self.glass_on(),
            motion: !self.reduced_motion(),
        }
    }

    /// Whether the compositor blurs behind the bars: it can, transparency
    /// is not reduced, and the shell draws on a GPU.
    pub fn glass_on(&self) -> bool {
        self.blur && !self.config.appearance.reduce_transparency && !self.shell.software
    }

    /// Tells the shell, if it is there; a shell that does not read loses
    /// its connection rather than holding the loop.
    pub fn tell_shell(&mut self, message: ToShell) {
        let Some(link) = &mut self.shell.link else {
            return;
        };
        let line = format!("{message}\n");
        if let Err(err) = link.writer.write_all(line.as_bytes()) {
            tracing::warn!(%err, "could not write to the shell: dropping its connection");
            self.shell_gone();
        }
    }
}

/// The shell's environment for the compositor's GPU, `gpu` (its render
/// node's device number); `None`: no GPU, the CPU renderer.
fn gpu_env(gpu: Option<DevId>) -> Vec<(&'static str, String)> {
    let Some(dev) = gpu else {
        return vec![("ICED_BACKEND", "tiny-skia".to_owned())];
    };
    let (major, minor) = (
        smithay::reexports::rustix::fs::major(dev),
        smithay::reexports::rustix::fs::minor(dev),
    );
    let driver = std::fs::read_link(format!("/sys/dev/char/{major}:{minor}/device/driver"))
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
    let mut env = vec![
        ("WGPU_POWER_PREF", "low".to_owned()),
        // Mesa's surfaces offer 10-bit colour with 2-bit alpha first, which
        // iced takes: translucent parts would blend in thirds.
        ("vk_wsi_force_bgra8_unorm_first", "true".to_owned()),
    ];
    match driver.as_deref().and_then(vulkan_driver) {
        Some(manifest) => env.push(("VK_LOADER_DRIVERS_SELECT", manifest.to_owned())),
        None => tracing::info!(
            ?driver,
            "no Vulkan driver known for the GPU: the loader picks"
        ),
    }
    let mesa_egl = "/usr/share/glvnd/egl_vendor.d/50_mesa.json";
    if driver.as_deref() != Some("nvidia") && Path::new(mesa_egl).exists() {
        env.push(("__EGL_VENDOR_LIBRARY_FILENAMES", mesa_egl.to_owned()));
    }
    env
}

/// The Vulkan loader's manifest names for a kernel driver, as
/// `VK_LOADER_DRIVERS_SELECT` takes them.
fn vulkan_driver(kernel: &str) -> Option<&'static str> {
    Some(match kernel {
        "i915" | "xe" => "*intel*",
        "amdgpu" | "radeon" => "*radeon*",
        "nouveau" => "*nouveau*",
        "nvidia" => "*nvidia*",
        "virtio_gpu" | "virtio-gpu" => "*virtio*",
        "msm" => "*freedreno*",
        "panfrost" | "panthor" => "*panfrost*",
        "v3d" => "*broadcom*",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_gpu_is_the_cpu_renderer() {
        assert_eq!(
            gpu_env(None),
            vec![("ICED_BACKEND", "tiny-skia".to_owned())]
        );
        assert_eq!(vulkan_driver("i915"), Some("*intel*"));
        assert_eq!(vulkan_driver("simpledrm"), None);
    }

    #[test]
    fn the_shells_socket_is_beside_the_wayland_one() {
        assert_eq!(
            socket_path(Path::new("/run/user/1000/hocus-0")),
            PathBuf::from("/run/user/1000/hocus-0.shell")
        );
    }
}
