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
    sync::{Arc, Condvar, Mutex, PoisonError},
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
    /// The session bus it serves the notifications on; `None`: none.
    pub bus: Option<Bus>,
    /// The socket it reaches the compositor on.
    pub socket: Option<PathBuf>,
    link: Option<Link>,
    /// It draws on the CPU: its bars are opaque.
    pub software: bool,
    /// It is the session's polkit agent (7.5): the compositor is the
    /// session.
    pub auth_agent: bool,
    /// The active keyboard layout as last told; `None`: not told yet.
    pub layout: Option<u32>,
    /// The bars' screens' desktops as last told.
    pub desktops: crate::desktops::Told,
    /// Ends in a row that came quickly.
    quick_ends: u32,
    ended: Option<Sender<Duration>>,
    /// The shells started and not ended yet.
    pub running: Arc<Running>,
    /// The shell's Wayland connection, made by the compositor and handed
    /// to it as it starts (`WAYLAND_SOCKET`): the only client whose lock
    /// surfaces show (7.1).
    pub client: Option<smithay::reexports::wayland_server::backend::ClientId>,
    /// `hocus msg a11y`'s askers, answered with the lines the shell says
    /// once it ends them.
    a11y: A11yAsked,
}

/// The screen readers' tree asked of the shell: who waits, and what it said
/// so far.
#[derive(Debug, Default)]
struct A11yAsked {
    askers: Vec<std::sync::mpsc::Sender<String>>,
    said: String,
}

/// The session bus the shell serves the notifications on (5.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bus {
    /// The bus given (`--session-bus`), by its address.
    Given(String),
    /// The session's own: the compositor is the session.
    Session,
}

impl Bus {
    /// The word the shell takes ([`hocus_link::NOTIFICATIONS_BUS_ENV`]).
    fn word(&self) -> &str {
        match self {
            Self::Given(address) => address,
            Self::Session => "session",
        }
    }
}

/// How many shells run: the in-process harness waits for none to, so that
/// a test's shell does not outlive it (#467).
#[derive(Debug, Default)]
pub struct Running {
    count: Mutex<usize>,
    changed: Condvar,
    /// The shell's process while it runs: its Wayland connection's
    /// credentials are the compositor's own, which made it.
    pid: Mutex<Option<u32>>,
}

/// One shell, from before its thread starts until it ended (or its thread
/// never started).
struct Runs(Arc<Running>);

impl Running {
    fn start(this: &Arc<Self>) -> Runs {
        *this.count.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        Runs(this.clone())
    }

    /// The shell's process, while one runs.
    pub fn pid(&self) -> Option<u32> {
        *self.pid.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set_pid(&self, pid: Option<u32>) {
        *self.pid.lock().unwrap_or_else(PoisonError::into_inner) = pid;
    }

    /// Waits until no shell runs, `timeout` at most: whether none does.
    pub fn wait_none(&self, timeout: Duration) -> bool {
        let count = self.count.lock().unwrap_or_else(PoisonError::into_inner);
        let (count, _) = self
            .changed
            .wait_timeout_while(count, timeout, |n| *n > 0)
            .unwrap_or_else(PoisonError::into_inner);
        *count == 0
    }
}

impl Drop for Runs {
    fn drop(&mut self) {
        let mut count = self.0.count.lock().unwrap_or_else(PoisonError::into_inner);
        *count = count.saturating_sub(1);
        self.0.changed.notify_all();
    }
}

/// The most the shell is sent ahead of what it read: past that, it does
/// not read, and loses its connection.
const UNSENT_MOST: usize = 64 << 20;

/// The shell's connection.
#[derive(Debug)]
struct Link {
    writer: UnixStream,
    token: RegistrationToken,
    /// What the socket did not take yet (a clipboard's image, 5.6), and
    /// what writes it as the socket takes more.
    unsent: Vec<u8>,
    flushing: Option<RegistrationToken>,
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
        let bus = self.shell.bus.clone();
        let auth_agent = self.shell.auth_agent;
        // Its Wayland connection, made here: the compositor knows it is the
        // shell's, which no other client can claim to be (7.1).
        let theirs = match self.shell_connection() {
            Ok(theirs) => theirs,
            Err(err) => {
                tracing::warn!("could not make the shell's connection: {err:#}");
                return;
            }
        };
        let runs = Running::start(&self.shell.running);
        programs::on_a_thread("shell".to_owned(), move || {
            let _runs = runs;
            let started = Instant::now();
            let mut command = env.command(&program);
            hand_socket(&mut command, &theirs);
            command.env(hocus_link::SOCKET_ENV, &socket);
            match &bus {
                Some(bus) => command.env(hocus_link::NOTIFICATIONS_BUS_ENV, bus.word()),
                None => command.env_remove(hocus_link::NOTIFICATIONS_BUS_ENV),
            };
            if auth_agent {
                command.env(hocus_link::AUTH_AGENT_ENV, "1");
            } else {
                command.env_remove(hocus_link::AUTH_AGENT_ENV);
            }
            // Its configuration file (`HOCUS_CONFIG`) is the programs'
            // ([`programs::Env`]).
            for (name, value) in gpu_env(gpu) {
                command.env(name, value);
            }
            let spawned = command.spawn();
            // Its copy is the child's now.
            drop(theirs);
            match spawned {
                Ok(mut child) => {
                    tracing::info!(program = %program.display(), pid = child.id(), "shell started");
                    _runs.0.set_pid(Some(child.id()));
                    match child.wait() {
                        Ok(status) => tracing::warn!(%status, "the shell ended"),
                        Err(err) => tracing::warn!(%err, "could not wait for the shell"),
                    }
                    _runs.0.set_pid(None);
                }
                Err(err) => {
                    tracing::warn!(program = %program.display(), %err, "could not start the shell");
                }
            }
            // The loop is gone when the compositor is: nothing to restart.
            let _ = ended.send(started.elapsed());
        });
    }

    /// The shell's Wayland connection: the compositor's end taken as a
    /// client known to be the shell, the other end to hand it.
    fn shell_connection(&mut self) -> anyhow::Result<UnixStream> {
        let (ours, theirs) = UnixStream::pair().context("a socket pair")?;
        let client = self
            .display_handle
            .insert_client(
                ours,
                std::sync::Arc::new(crate::state::ClientState {
                    shell: true,
                    ..Default::default()
                }),
            )
            .context("the shell's client")?;
        // One shell at a time: a former one's connection is no longer it.
        if let Some(old) = self.shell.client.replace(client.id()) {
            self.display_handle.backend_handle().kill_client(
                old,
                smithay::reexports::wayland_server::backend::DisconnectReason::ConnectionClosed,
            );
        }
        Ok(theirs)
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

    /// The shell connected (again): it is told how bars and their clock
    /// look, where bars are, the other screens, the keyboard's layouts and
    /// the bars' screens' desktops.
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
                // A clipboard's image comes in one long line (5.6).
                let mut buffer = [0u8; 16 * 1024];
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
        self.shell.link = Some(Link {
            writer,
            token,
            unsent: Vec::new(),
            flushing: None,
        });
        self.tell_shell(self.bar_look());
        self.wallpaper_accent_now();
        self.tell_shell(ToShell::Theme(self.theme_choice()));
        self.tell_shell(ToShell::Wallpaper(self.wallpapers()));
        self.tell_shell(ToShell::Locale(self.config.locale));
        self.tell_shell(ToShell::Clock(self.config.panel.clock.look()));
        self.tell_shell(ToShell::BarWindows(self.config.panel.windows));
        self.tell_shell(ToShell::ClipboardRules(self.config.clipboard.rules()));
        self.tell_shell(ToShell::AgentPandas(self.config.agents.pandas));
        self.tell_shell(ToShell::DesktopNames(self.config.desktops.shown_names()));
        self.tell_shell(ToShell::BarGroups(self.config.panel.groups()));
        self.tell_shell(ToShell::BarSide(self.bar_side()));
        let bars: Vec<ToShell> = self
            .panel
            .bars
            .iter()
            .map(|(output, bar)| ToShell::Bar {
                output: output.clone(),
                form: bar.form,
                since: 0,
                scale: self.bar_scale(output),
            })
            .collect();
        for bar in bars {
            self.tell_shell(bar);
        }
        self.tell_screens(true);
        self.tell_layouts();
        self.tell_desktops();
        self.tell_window_states(true);
        self.tell_agents();
        self.tell_bubbles_screen();
        self.tell_fullscreen_again();
        self.hand_kept_levels();
        self.tell_sharing_again();
        self.auth_tell_caps();
        if self.record.recording() {
            self.tell_recording();
        }
        self.lock_shell_connected();
        self.away_shell_connected();
        self.leave_shell_connected();
    }

    /// The shell's connection ended.
    fn shell_gone(&mut self) {
        if let Some(link) = self.shell.link.take() {
            self.loop_handle.remove(link.token);
            if let Some(flushing) = link.flushing {
                self.loop_handle.remove(flushing);
            }
            tracing::info!("the shell is gone; its bars' space stays");
        }
        self.answer_a11y(Some("the shell went\n"));
        self.peek_shell_gone();
        self.auth_shell_gone();
        // The meeting's halo is the shell's to say: its next one says it
        // again.
        self.meeting_near(None);
        if self.shell.software {
            self.shell.software = false;
            self.redraw = true;
        }
        self.lock_shell_gone();
        self.leave_shell_gone();
    }

    fn shell_says(&mut self, message: ToCompositor) {
        // Locked (7.1): nothing that shows or moves a window, starts or
        // copies anything; the lock's own asks.
        if self.locked() && !Self::lock_lets(&message) {
            tracing::info!("the session is locked: the shell's ask is refused");
            return;
        }
        match message {
            ToCompositor::Lock(ask) => self.lock_asked(ask),
            ToCompositor::Software => {
                tracing::info!("the shell draws on the CPU: no glass behind its bars");
                self.shell.software = true;
                self.redraw = true;
                self.tell_shell(self.bar_look());
            }
            ToCompositor::Wake(namespace) => self.wake_shell_surfaces(&namespace),
            ToCompositor::A11yLine(line) => {
                if !self.shell.a11y.askers.is_empty() {
                    self.shell.a11y.said.push_str(&line);
                    self.shell.a11y.said.push('\n');
                }
            }
            ToCompositor::A11yEnd => self.answer_a11y(None),
            ToCompositor::Closing(namespace) if hocus_link::slides_in(&namespace) => {
                self.bubble_closing(&namespace);
            }
            ToCompositor::Closing(namespace) => {
                self.popovers_closing(&namespace);
                self.launcher_closing(&namespace);
            }
            ToCompositor::NextLayout => self.next_layout(),
            ToCompositor::Desktop { output, to } => self.desktop_asked(&output, to),
            ToCompositor::Bring(id) => self.window_asked(id),
            ToCompositor::DeskTiles { bar, tiles } => self.desk_tiles_said(bar, tiles),
            ToCompositor::Overview => {
                tracing::debug!("the bar asks for the overview");
                self.toggle_overview();
            }
            ToCompositor::Close(id) => {
                tracing::debug!(window = id, "the shell asks a window to close");
                self.close_window(crate::model::WindowId(id));
            }
            ToCompositor::WindowAction { id, action } => {
                tracing::debug!(window = id, action = action.name(), "the bar's menu acts");
                self.run_window_action(crate::model::WindowId(id), action);
            }
            ToCompositor::SendTo {
                id,
                output,
                desktop,
            } => self.send_to_asked(id, &output, desktop),
            ToCompositor::WindowMenu(id) => self.window_menu_asked(id),
            ToCompositor::Action(run) => self.action_asked(run),
            ToCompositor::GoToAgent { id, row } => self.go_to_agent(id, row),
            ToCompositor::Ring(row) => self.ring_moved(&row),
            ToCompositor::Plug(tool) => match crate::agents::tool(&tool) {
                Some(tool) => self.agent_tools_asked(hocus_agents::watch::Ask::Plug(tool)),
                None => tracing::warn!(tool, "the shell plugs in a tool that is not one"),
            },
            ToCompositor::Unplug(tool) => match crate::agents::tool(&tool) {
                Some(tool) => self.agent_tools_asked(hocus_agents::watch::Ask::Unplug(tool)),
                None => tracing::warn!(tool, "the shell unplugs a tool that is not one"),
            },
            ToCompositor::Decline => self.agent_tools_asked(hocus_agents::watch::Ask::Decline),
            ToCompositor::Follow(tool) if tool == "herdr" => {
                self.agent_tools_asked(hocus_agents::watch::Ask::FollowHerdr);
            }
            ToCompositor::Follow(tool) => {
                tracing::warn!(tool, "the shell follows a tool that is not herdr");
            }
            ToCompositor::Briefing { open } => self.peek_briefing(open),
            ToCompositor::PeekClose => self.peek_closed(),
            ToCompositor::Meeting(near) => {
                tracing::debug!(?near, "the next meeting, for the halo");
                self.meeting_near(near.map(|near| match near {
                    hocus_link::MeetingNear::Ten => crate::halo::Meeting::InTen,
                    hocus_link::MeetingNear::Two => crate::halo::Meeting::InTwo,
                }));
            }
            ToCompositor::Copy(text) => self.copy_text(text),
            ToCompositor::Clip { paste, clip } => self.clip_asked(paste, clip),
            ToCompositor::ClipboardCount { items, pinned } => {
                self.clipboard_counted(items, pinned);
            }
            ToCompositor::Emoji { keep, text } => self.emoji_asked(keep, text),
            ToCompositor::Capture(ask) => self.capture_asked(ask),
            ToCompositor::Share(ask) => self.share_asked(ask),
            ToCompositor::Auth(ask) => self.auth_asked(ask),
            ToCompositor::Record(ask) => self.record_asked(ask),
            ToCompositor::Launch(launch) => self.launch(launch),
            ToCompositor::Raise { pid, app } => self.raise_app(pid, &app),
            ToCompositor::TrayClick { ask, pid } => self.tray_clicked(ask, pid),
            ToCompositor::Export { ask, namespace } => {
                let handle = self.export_for_shell(&namespace);
                self.tell_shell(ToShell::Exported { ask, handle });
            }
            ToCompositor::LogOut => {
                tracing::info!("the shell logs out");
                self.end(crate::Ending::LoggedOut);
            }
            ToCompositor::Leave(ask) => self.leave_asked(ask),
        }
    }

    /// A configure, as they are, to the shell's layer surfaces of
    /// `namespace`: iced then draws them alone ([`ToCompositor::Wake`]).
    fn wake_shell_surfaces(&mut self, namespace: &str) {
        for output in self.space.outputs() {
            let map = smithay::desktop::layer_map_for_output(output);
            for layer in map.layers().filter(|l| l.namespace() == namespace) {
                layer.layer_surface().send_configure();
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
    /// is not reduced (high contrast reduces it too, #423), and the shell
    /// draws on a GPU.
    pub fn glass_on(&self) -> bool {
        self.blur
            && !self.config.appearance.reduce_transparency
            && !self.config.appearance.high_contrast
            && !self.shell.software
    }

    /// `hocus msg a11y`: the shell is asked what it shows for screen
    /// readers; `reply` hears it once the shell said it all (or went).
    pub(crate) fn ask_a11y(&mut self, reply: std::sync::mpsc::Sender<String>) {
        if self.shell.link.is_none() {
            let _ = reply.send("no shell\n".to_owned());
            return;
        }
        self.shell.a11y.askers.push(reply);
        if self.shell.a11y.askers.len() == 1 {
            self.shell.a11y.said.clear();
            self.tell_shell(ToShell::A11y);
        }
    }

    /// Answers whoever asked for the screen readers' tree: with what the
    /// shell said, or `instead`.
    fn answer_a11y(&mut self, instead: Option<&str>) {
        let said = std::mem::take(&mut self.shell.a11y.said);
        let answer = instead.map_or(said, str::to_owned);
        for asker in self.shell.a11y.askers.drain(..) {
            let _ = asker.send(answer.clone());
        }
    }

    /// Tells the shell, if it is there; a shell that does not read loses
    /// its connection rather than holding the loop.
    pub fn tell_shell(&mut self, message: ToShell) {
        self.tell_shell_line(message.to_string());
    }

    /// Tells the shell a line made already (off the loop: a clipboard's
    /// copy, 5.6). What the socket does not take at once is written as it
    /// takes more, never waited for.
    pub fn tell_shell_line(&mut self, mut line: String) {
        let Some(link) = &mut self.shell.link else {
            return;
        };
        line.push('\n');
        if !link.unsent.is_empty() {
            if link.unsent.len() + line.len() > UNSENT_MOST {
                tracing::warn!("the shell does not read: dropping its connection");
                self.shell_gone();
            } else {
                link.unsent.extend_from_slice(line.as_bytes());
            }
            return;
        }
        let written = match write_some(&link.writer, line.as_bytes()) {
            Ok(written) => written,
            Err(err) => {
                tracing::warn!(%err, "could not write to the shell: dropping its connection");
                self.shell_gone();
                return;
            }
        };
        if written == line.len() {
            return;
        }
        link.unsent.extend_from_slice(&line.as_bytes()[written..]);
        let flushing = link.writer.try_clone().map(|writer| {
            self.loop_handle.insert_source(
                Generic::new(writer, Interest::WRITE, Mode::Level),
                |_, _, app| Ok(app.state.flush_shell()),
            )
        });
        match flushing {
            Ok(Ok(token)) => {
                if let Some(link) = &mut self.shell.link {
                    link.flushing = Some(token);
                }
            }
            Ok(Err(err)) => {
                tracing::warn!(%err.error, "could not write to the shell later: dropping its connection");
                self.shell_gone();
            }
            Err(err) => {
                tracing::warn!(%err, "could not write to the shell later: dropping its connection");
                self.shell_gone();
            }
        }
    }

    /// The shell's socket takes more: what was left is written.
    fn flush_shell(&mut self) -> PostAction {
        let Some(link) = &mut self.shell.link else {
            return PostAction::Remove;
        };
        match write_some(&link.writer, &link.unsent) {
            Ok(written) => {
                link.unsent.drain(..written);
                if link.unsent.is_empty() {
                    link.flushing = None;
                    PostAction::Remove
                } else {
                    PostAction::Continue
                }
            }
            Err(err) => {
                tracing::warn!(%err, "could not write to the shell: dropping its connection");
                // This source goes as it returns.
                link.flushing = None;
                self.shell_gone();
                PostAction::Remove
            }
        }
    }
}

/// The shell's Wayland connection handed to it: `WAYLAND_SOCKET` names
/// the descriptor, which stays open across `exec` in the child alone (the
/// compositor's copy closes on exec, so no other program gets it).
#[allow(unsafe_code)]
fn hand_socket(command: &mut std::process::Command, socket: &UnixStream) {
    use std::os::{fd::AsRawFd, unix::process::CommandExt};
    let fd = socket.as_raw_fd();
    command.env("WAYLAND_SOCKET", fd.to_string());
    // SAFETY: the closure runs in the child between fork and exec, and only
    // clears the descriptor's close-on-exec flag with fcntl, which is
    // async-signal-safe and allocates nothing. `fd` is open in the child:
    // the parent holds `socket` until `spawn` returns.
    unsafe {
        command.pre_exec(move || {
            let fd = rustix::fd::BorrowedFd::borrow_raw(fd);
            rustix::io::fcntl_setfd(fd, rustix::io::FdFlags::empty())?;
            Ok(())
        });
    }
}

/// Writes what `writer` takes of `bytes` now: how much.
fn write_some(writer: &UnixStream, bytes: &[u8]) -> std::io::Result<usize> {
    let mut written = 0;
    while written < bytes.len() {
        match (&*writer).write(&bytes[written..]) {
            Ok(0) => return Err(ErrorKind::WriteZero.into()),
            Ok(n) => written += n,
            Err(err) if err.kind() == ErrorKind::WouldBlock => break,
            Err(err) if err.kind() == ErrorKind::Interrupted => {}
            Err(err) => return Err(err),
        }
    }
    Ok(written)
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

/// Sends one notification, `org.freedesktop.Notifications.Notify` on
/// `bus` (the shell serves it, 5.3), from `Hocuspanda` with `icon`, on a
/// thread of its own: a call to another process never holds the loop.
pub fn notify(bus: Bus, icon: &'static str, summary: String) {
    let started = std::thread::Builder::new()
        .name("notification".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(notify_on(bus, icon, summary)),
                Err(err) => tracing::warn!(%err, "no runtime for a notification"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "a notification is not sent");
    }
}

async fn notify_on(bus: Bus, icon: &str, summary: String) {
    let builder = match &bus {
        Bus::Given(address) => zbus::connection::Builder::address(address.as_str()),
        Bus::Session => zbus::connection::Builder::session(),
    };
    let connection = match builder.build().await {
        Ok(connection) => connection,
        Err(err) => {
            tracing::warn!(%err, summary, "no session bus: a notification is not sent");
            return;
        }
    };
    let hints: std::collections::HashMap<&str, zbus::Variant<'_>> =
        std::collections::HashMap::new();
    let sent = connection
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                "Hocuspanda",
                0u32,
                icon,
                summary.as_str(),
                "",
                Vec::<&str>::new(),
                hints,
                -1i32,
            ),
        )
        .await;
    if let Err(err) = sent {
        tracing::warn!(%err, summary, "a notification is not sent");
    }
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
