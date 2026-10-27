//! The session saved and put back (#31). What is open (each window's
//! application, output, desktop, place and state) is saved now and then and
//! when quitting; at the next start the applications are started again and
//! their windows put back where they were as they come. An application that
//! does not start, or whose window never comes, is reported; the others are
//! put back all the same.
//!
//! The file is read and written on one worker thread, in the order asked
//! (#277): a home directory slow to answer holds back neither the start
//! nor a frame, and a read asked after a save sees it. What is read comes
//! back through a channel; the card waits for it as for the configuration.
//! The one wait on the loop is at the session's very end, for the last
//! save, and it is bounded.
//!
//! Only the session itself saves and restores: nested or headless, the
//! developer's own session file is never touched (tests give a directory).

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

use hocus_core::{
    OutputName,
    session::{SavedWindow, Session},
};
use smithay::{
    reexports::{
        calloop::{LoopHandle, channel},
        wayland_server::Resource,
    },
    utils::{Logical, Rectangle, SERIAL_COUNTER},
    wayland::seat::WaylandFocus,
};

use hocus_shell::boot::Boot;
use hocus_ui::motion::{Animated, Curve, Easing, STANDARD};

use crate::{App, State, desktops::title_and_app_id, window_mode::WindowMode};

/// How long a restored application has to show its window.
const PATIENCE: Duration = Duration::from_secs(15);
/// How long after something happened the session is saved.
const SAVE_AFTER: Duration = Duration::from_secs(30);
/// The boot screen shows at most this long: an application slow to come
/// back is reported, and the desktop is there meanwhile.
const BOOT_AT_MOST: Duration = Duration::from_secs(5);

/// How long the end of the session waits for the last save to be written:
/// a disk that does not answer by then does not keep the machine from
/// shutting down.
const EXIT_PATIENCE: Duration = Duration::from_secs(3);

/// What the session's worker does, in the order asked.
enum Job {
    /// Reads the file; what it holds goes back with this number.
    Read(u64),
    /// Writes the session; the commands of these windows (index, pid) are
    /// their processes' own.
    Write(Session, Vec<(usize, u32)>),
    /// Answers once everything asked before is done.
    Flush(mpsc::Sender<()>),
}

/// The saved session on its way to the card.
#[derive(Debug, Default)]
enum Offer {
    /// Nothing to offer, or offered already.
    #[default]
    Done,
    /// The worker reads the file.
    Reading,
    /// Read: offered once the configuration is read and a screen is there.
    Read(Option<Session>),
}

#[derive(Debug, Default)]
pub struct Sessions {
    /// Where the session file goes; none: nothing is saved nor restored.
    pub dir: Option<PathBuf>,
    worker: Option<mpsc::Sender<Job>>,
    /// The last read asked: an earlier one coming back late is dropped.
    reads: u64,
    /// Saves asked, and saves the worker wrote (or failed to): a test
    /// waits for one to be on disk without the loop being woken.
    saves: u64,
    written: Arc<AtomicU64>,
    /// What was last saved, to write only changes.
    last: Option<Session>,
    /// Windows to put back as they come, by app id.
    pending: VecDeque<SavedWindow>,
    /// Applications started again, and when: those whose window does not
    /// come are reported.
    started: Vec<(String, Duration)>,
    /// The saved session, read off the loop, to offer.
    offer: Offer,
    /// Whether the XDG autostart entries were started.
    autostarted: bool,
    /// The card asking what to put back.
    card: Option<Card>,
    /// The boot screen while windows come back (#48).
    boot: Option<Booting>,
    /// Saved now and then on a real clock; tests save when they ask.
    periodic: bool,
    /// The loop's activity last seen: nothing changes at rest.
    seen_activity: u64,
    /// When the session is saved next, once something happened.
    alarm: crate::alarm::Alarm,
}

#[derive(Debug)]
struct Booting {
    /// Windows to put back, from the start.
    total: usize,
    since: Duration,
    /// By output name.
    panels: std::collections::BTreeMap<String, crate::shell::Panel<Boot>>,
    /// Everything back (or it waited long enough): it fades, or in the
    /// Matrix theme says so and dissolves (#155).
    leaving: Option<crate::lock::Moment>,
    /// The applications put back, in order, for the Matrix theme's
    /// terminal.
    apps: Vec<String>,
}

/// `$XDG_STATE_HOME/hocuspanda`, or `~/.local/state/hocuspanda`.
pub fn default_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .map(|d| d.join("hocuspanda"))
}

fn file(dir: &std::path::Path) -> PathBuf {
    dir.join("session.toml")
}

/// The command line of a process, from /proc (Linux).
fn command_of(pid: u32) -> Vec<String> {
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .map(|bytes| {
            bytes
                .split(|b| *b == 0)
                .filter(|a| !a.is_empty())
                .map(|a| String::from_utf8_lossy(a).into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// What the saved session file holds, if anything is to be put back.
fn read(path: &Path) -> Option<Session> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => {
            tracing::warn!(%err, "the saved session does not read");
            return None;
        }
    };
    Session::parse(&text)
        .inspect_err(|err| tracing::warn!(err, "the saved session is unreadable"))
        .ok()
        .filter(|s| !s.windows.is_empty())
}

/// Writes `session` beside the file, then over it: a crash halfway leaves
/// the last one whole.
fn write(path: &Path, mut session: Session, pids: Vec<(usize, u32)>) {
    // Commands not known from desktop entries: the processes' own.
    for (i, pid) in pids {
        if let Some(w) = session.windows.get_mut(i)
            && w.command.is_empty()
        {
            w.command = command_of(pid);
        }
    }
    let written = (|| -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("toml.new");
        std::fs::write(&tmp, session.to_toml())?;
        std::fs::rename(&tmp, path)
    })();
    if let Err(err) = written {
        tracing::warn!(%err, "could not save the session");
    }
}

impl State {
    /// Starts the session's worker, which reads the saved session at once;
    /// called once the directory is known.
    pub(crate) fn start_sessions(
        &mut self,
        handle: &LoopHandle<'static, App>,
        dir: Option<PathBuf>,
    ) {
        self.sessions.dir = dir;
        let Some(dir) = &self.sessions.dir else {
            return;
        };
        let path = file(dir);
        // Nothing comes back without a worker: the card does not wait.
        self.sessions.offer = Offer::Read(None);
        let (back, came) = channel::channel::<(u64, Option<Session>)>();
        let inserted = handle.insert_source(came, |event, _, app| {
            // Quietly: reading is not something happening on the desktop,
            // which would have the session saved again (#88).
            app.timed_quietly("session read", |app| match event {
                channel::Event::Msg((read, session)) => app.state.session_read(read, session),
                channel::Event::Closed => {
                    let read = app.state.sessions.reads;
                    app.state.session_read(read, None);
                }
            });
        });
        if let Err(err) = inserted {
            tracing::warn!(err = %err.error, "the saved session will not be offered");
            return;
        }
        let (worker, jobs) = mpsc::channel::<Job>();
        let written = self.sessions.written.clone();
        let spawned = std::thread::Builder::new()
            .name("session".into())
            .spawn(move || {
                for job in jobs {
                    match job {
                        Job::Read(n) => {
                            let _ = back.send((n, read(&path)));
                        }
                        Job::Write(session, pids) => {
                            write(&path, session, pids);
                            written.fetch_add(1, Ordering::Release);
                        }
                        Job::Flush(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            });
        match spawned {
            Ok(_) => {
                self.sessions.worker = Some(worker);
                self.read_session();
            }
            Err(err) => tracing::warn!(%err, "the session will not be saved nor offered"),
        }
        // Saved now and then; tests save when they ask.
        self.sessions.periodic = self.waker().is_some();
    }

    /// Reads the saved session again, off the loop, to offer it once read:
    /// at start, and when a test asks.
    pub fn read_session(&mut self) {
        let Some(worker) = &self.sessions.worker else {
            return;
        };
        self.sessions.reads += 1;
        if worker.send(Job::Read(self.sessions.reads)).is_ok() {
            self.sessions.offer = Offer::Reading;
        }
    }

    /// The worker read the saved session: the last read asked is offered.
    fn session_read(&mut self, read: u64, session: Option<Session>) {
        if read == self.sessions.reads && matches!(self.sessions.offer, Offer::Reading) {
            self.sessions.offer = Offer::Read(session);
        }
    }

    /// Whether the saved session was read and offered (or there was none).
    pub fn session_offered(&self) -> bool {
        matches!(self.sessions.offer, Offer::Done)
    }

    /// Whether every save asked is written.
    pub fn session_saved(&self) -> bool {
        self.sessions.written.load(Ordering::Acquire) >= self.sessions.saves
    }

    /// Hands what is open to the worker, to write as it comes.
    fn send_save(&mut self, session: Session, pids: Vec<(usize, u32)>) {
        let Some(worker) = &self.sessions.worker else {
            return;
        };
        if worker.send(Job::Write(session.clone(), pids)).is_ok() {
            self.sessions.saves += 1;
        }
        self.sessions.last = Some(session);
    }

    /// At the session's end (quitting, logging out, shutting down): waits
    /// for the last save to be on disk, for at most [`EXIT_PATIENCE`]. The
    /// one wait for the file on the loop's thread, once, as the session
    /// ends: the loop has stopped, or the machine is about to go.
    pub fn finish_sessions(&mut self) {
        let Some(worker) = &self.sessions.worker else {
            return;
        };
        let (done, flushed) = mpsc::channel();
        if worker.send(Job::Flush(done)).is_err() {
            return;
        }
        if flushed.recv_timeout(EXIT_PATIENCE).is_err() {
            tracing::warn!("the session may not have been saved");
        }
    }

    /// Saves the session thirty seconds after something happened (a
    /// client, an input, a worker's result), at most so often: at rest,
    /// nothing changes and the loop is not woken to look (#88).
    fn save_after_activity(&mut self) {
        if !self.sessions.periodic {
            return;
        }
        let now = self.clock.now();
        let handle = self.waker().cloned();
        if self.sessions.alarm.due(now) {
            self.sessions
                .alarm
                .set(handle.as_ref(), "session save", None, now);
            self.save_session();
        }
        let activity = self.measures.activity();
        if activity != self.sessions.seen_activity {
            self.sessions.seen_activity = activity;
            if self.sessions.alarm.at().is_none() {
                let at = Some(now + SAVE_AFTER);
                self.sessions
                    .alarm
                    .set(handle.as_ref(), "session save", at, now);
            }
        }
    }

    /// What is open now.
    fn snapshot(&self) -> (Session, Vec<(usize, u32)>) {
        let mut windows = Vec::new();
        let mut pids = Vec::new();
        for (id, managed) in &self.windows {
            let (title, app_id) = title_and_app_id(&managed.window);
            let Some(app_id) = app_id else { continue };
            let desktop = self.model.desktop_of(*id);
            let host = desktop.and_then(|d| self.model.host_of(d));
            let output = host.map(|o| o.name.0.clone()).unwrap_or_default();
            let index = host
                .zip(desktop)
                .and_then(|(o, d)| o.desktops.iter().position(|x| *x == d))
                .map_or(1, |i| i as u32 + 1);
            let frame = self.current_rect(*id).unwrap_or_else(|| {
                Rectangle::new(managed.location, managed.window.geometry().size)
            });
            let (restore, state) = match managed.mode {
                WindowMode::Maximized { restore } => (Some(restore), "maximized"),
                WindowMode::Fullscreen { restore, .. } => (Some(restore), "fullscreen"),
                _ => (None, "normal"),
            };
            let place = restore.unwrap_or(frame);
            let command = self.command_for(&app_id);
            if command.is_empty()
                && let Some(pid) = managed
                    .window
                    .wl_surface()
                    .and_then(|s| self.display_handle.get_client(s.id()).ok())
                    .and_then(|c| c.get_credentials(&self.display_handle).ok())
                    .map(|c| c.pid as u32)
            {
                pids.push((windows.len(), pid));
            }
            windows.push(SavedWindow {
                app_id,
                title: title.unwrap_or_default(),
                command,
                output,
                desktop: index,
                geometry: [place.loc.x, place.loc.y, place.size.w, place.size.h],
                state: state.into(),
                pinned: self.model.is_pinned(*id),
            });
        }
        (Session { windows }, pids)
    }

    /// The desktop entry's command for an app id, if one matches.
    fn command_for(&self, app_id: &str) -> Vec<String> {
        self.launcher
            .as_ref()
            .and_then(|l| l.app_for(app_id))
            .map(|app| app.exec.clone())
            .unwrap_or_default()
    }

    /// Saves what is open, if it changed.
    pub fn save_session(&mut self) {
        if self.sessions.worker.is_none() {
            return;
        }
        let (session, pids) = self.snapshot();
        if self.sessions.last.as_ref() == Some(&session) {
            return;
        }
        self.send_save(session, pids);
    }

    /// Starts the applications of `session` again, `only` these when
    /// given; their windows are put back as they come.
    pub fn restore_session(&mut self, session: &Session, only: Option<&[String]>) {
        let now = self.clock.now();
        let wanted = |app: &str| only.is_none_or(|o| o.iter().any(|a| a == app));
        for window in &session.windows {
            if wanted(&window.app_id) {
                self.sessions.pending.push_back(window.clone());
            }
        }
        for (app_id, command) in session.commands() {
            if !wanted(&app_id) {
                continue;
            }
            let Some((program, args)) = command.split_first() else {
                continue;
            };
            match self.start_program(program, args, crate::actions::Asked::BySession) {
                Ok(()) => self.sessions.started.push((app_id, now)),
                Err(err) => {
                    tracing::warn!(app_id, %err, "could not restore");
                    self.sessions.pending.retain(|w| w.app_id != app_id);
                    self.report_restore(&app_id, &err.to_string());
                }
            }
        }
        if !self.sessions.pending.is_empty() {
            self.sessions.boot = Some(Booting {
                total: self.sessions.pending.len(),
                since: now,
                panels: Default::default(),
                leaving: None,
                apps: self
                    .sessions
                    .pending
                    .iter()
                    .map(|w| w.app_id.clone())
                    .collect(),
            });
            self.needs_redraw = true;
        }
    }

    /// The boot screen follows the windows coming back, and goes as soon
    /// as they are all there, without waiting for its own time.
    fn refresh_boot(&mut self) {
        let now = self.clock.now();
        let moves = !self.instant_motion && !self.config.appearance.reduced_motion;
        let matrix = self.matrix_moves();
        let variant = if self.matrix_look() {
            hocus_shell::matrix::Variant::Matrix
        } else {
            hocus_shell::matrix::Variant::Plain
        };
        let left = self.sessions.pending.len();
        let back: Vec<(String, bool)> = self
            .sessions
            .boot
            .as_ref()
            .map(|b| {
                b.apps
                    .iter()
                    .map(|a| {
                        (
                            a.clone(),
                            !self.sessions.pending.iter().any(|w| &w.app_id == a),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let Some(boot) = self.sessions.boot.as_mut() else {
            return;
        };
        let done = left == 0 || now.saturating_sub(boot.since) >= BOOT_AT_MOST;
        if done && boot.leaving.is_none() {
            if !moves {
                self.sessions.boot = None;
                self.needs_redraw = true;
                return;
            }
            boot.leaving = Some(if matrix {
                crate::lock::Moment::Matrix(now)
            } else {
                let mut fade = Animated::new(1.0);
                fade.set(0.0, Curve::Ease(Easing::InCubic, STANDARD), now);
                crate::lock::Moment::Plain(fade)
            });
        }
        if boot
            .leaving
            .as_ref()
            .is_some_and(|f| !f.is_running(now, crate::matrix_screens::BOOT_OUT))
        {
            self.sessions.boot = None;
            self.needs_redraw = true;
            return;
        }
        let theme = self.theme();
        let lang = self.shell_lang();
        let pandas = self.pandas_on();
        let outputs: Vec<(String, hocus_ui::Size, f32)> = self
            .space
            .outputs()
            .filter_map(|o| {
                let g = self.space.output_geometry(o)?;
                let scale = o.current_scale().fractional_scale() as f32;
                Some((
                    o.name(),
                    hocus_ui::Size::new(g.size.w as f32, g.size.h as f32),
                    scale,
                ))
            })
            .collect();
        let Some(boot) = self.sessions.boot.as_mut() else {
            return;
        };
        let total = boot.total;
        boot.panels
            .retain(|name, _| outputs.iter().any(|(o, _, _)| o == name));
        // The Matrix theme's rain falls while it shows.
        let mut redraw = boot.leaving.is_some() || matrix;
        for (name, size, scale) in outputs {
            let panel = boot.panels.entry(name).or_insert_with(|| {
                crate::shell::Panel::new(Boot::new(
                    theme.clone(),
                    lang,
                    size,
                    scale,
                    total,
                    pandas,
                    variant,
                ))
            });
            panel.view.set_lines(&back);
            panel.view.set_progress(total - left.min(total), total);
            let drawn = panel.view.draw(now);
            redraw |= panel.upload(&drawn) || panel.view.is_moving(now);
        }
        if redraw {
            self.needs_redraw = true;
        }
    }

    pub fn boot_elements<R>(
        &self,
        renderer: &mut R,
        output: &smithay::output::Output,
    ) -> Vec<smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement<R>>
    where
        R: smithay::backend::renderer::Renderer + smithay::backend::renderer::ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let Some(boot) = &self.sessions.boot else {
            return Vec::new();
        };
        // The Matrix theme's boot draws it with its rain (#155).
        if self.matrix_look() {
            return Vec::new();
        }
        let alpha = boot.leaving.as_ref().map(|m| match m {
            crate::lock::Moment::Plain(fade) => fade.value(self.clock.now()).clamp(0.0, 1.0),
            crate::lock::Moment::Matrix(_) => 0.0,
        });
        boot.panels
            .get(&output.name())
            .and_then(|p| p.element_with(renderer, output, Default::default(), alpha))
            .into_iter()
            .collect()
    }

    /// The boot screen on `output` in the Matrix theme: since when it
    /// shows, since when it goes, and its picture.
    pub(crate) fn boot_matrix_on(
        &self,
        output: &smithay::output::Output,
    ) -> Option<(Duration, Option<Duration>, &crate::shell::Panel<Boot>)> {
        let boot = self.sessions.boot.as_ref()?;
        let leaving = match boot.leaving {
            Some(crate::lock::Moment::Matrix(since)) => Some(since),
            Some(crate::lock::Moment::Plain(_)) | None => None,
        };
        Some((boot.since, leaving, boot.panels.get(&output.name())?))
    }

    pub fn boot_ui(&self, out: &mut Vec<hocus_api::UiNode>) {
        let Some(boot) = &self.sessions.boot else {
            return;
        };
        for (name, panel) in &boot.panels {
            if let Some(g) = self
                .find_output(name)
                .and_then(|o| self.space.output_geometry(&o))
            {
                crate::shell::flatten(&panel.view.access(), g.loc, 0, out);
            }
        }
    }

    /// A window just managed: put back where it was, if it was saved;
    /// among several of its application, the one saved with its title,
    /// else the first saved.
    pub fn place_restored(
        &mut self,
        id: hocus_core::WindowId,
        app_id: Option<&str>,
        title: Option<&str>,
    ) {
        let Some(app_id) = app_id else { return };
        // One start of the application came: others it had may still.
        if let Some(i) = self.sessions.started.iter().position(|(a, _)| a == app_id) {
            self.sessions.started.remove(i);
        }
        let Some(saved) = hocus_core::session::take_match(
            &mut self.sessions.pending,
            app_id,
            title.unwrap_or_default(),
        ) else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        if let Some(output) = self.find_output(&saved.output) {
            let n = saved.desktop.saturating_sub(1) as usize;
            let has = self
                .model
                .output(&OutputName(saved.output.clone()))
                .is_some_and(|o| n < o.desktops.len());
            let desktop = has.then_some(n);
            if let Err(err) = self.send_window(id, &output, desktop, serial) {
                tracing::warn!(%err, "could not put a window back on its desktop");
            }
        }
        let [x, y, w, h] = saved.geometry;
        let frame = Rectangle::<i32, Logical>::new((x, y).into(), (w, h).into());
        self.resize_to(id, frame, false);
        if saved.pinned {
            let _ = self.model.set_pinned(id, true);
        }
        match saved.state.as_str() {
            "maximized" => self.set_maximized(id, true, serial),
            "fullscreen" => self.set_fullscreen(id, true, None, serial),
            _ => {}
        }
        self.sync_desktops();
    }

    /// After each dispatch: restored applications whose window never came
    /// are reported.
    pub fn refresh_sessions(&mut self) {
        self.save_after_activity();
        self.offer_when_ready();
        self.refresh_card();
        self.refresh_boot();
        let now = self.clock.now();
        let late: Vec<String> = self
            .sessions
            .started
            .iter()
            .filter(|(_, t)| now.saturating_sub(*t) > PATIENCE)
            .map(|(a, _)| a.clone())
            .collect();
        for app_id in late {
            self.sessions.started.retain(|(a, _)| *a != app_id);
            self.sessions.pending.retain(|w| w.app_id != app_id);
            self.report_restore(&app_id, "its window never came");
        }
    }

    fn report_restore(&mut self, app_id: &str, why: &str) {
        let lang = self.shell_lang();
        self.shell_toast(&lang.not_restored(app_id), why);
    }
}

/// The card at start, on one output.
#[derive(Debug)]
pub struct Card {
    output: String,
    panel: crate::shell::Panel<hocus_shell::welcome::Welcome>,
    session: Session,
}

impl State {
    /// Once the configuration and the saved session are read: put the
    /// session back, ask, or not; then the autostart entries, once.
    fn offer_when_ready(&mut self) {
        if !matches!(self.sessions.offer, Offer::Read(_))
            || (self.config_path.is_some() && self.config_generation == 0)
            || self.space.outputs().next().is_none()
        {
            return;
        }
        if let Offer::Read(session) = std::mem::take(&mut self.sessions.offer) {
            self.offer_session(session);
        }
        if !self.sessions.autostarted {
            self.sessions.autostarted = true;
            self.autostart();
        }
    }

    /// The XDG autostart entries, read on a worker thread, started on the
    /// loop. Only the session itself: nested, they are the developer's.
    fn autostart(&mut self) {
        use smithay::reexports::calloop::channel;
        if !self.is_session || !self.config.session.autostart {
            return;
        }
        let Some(handle) = self.loop_handle.clone() else {
            return;
        };
        let (found, entries) = channel::channel::<Vec<hocus_platform::apps::Entry>>();
        let inserted = handle.insert_source(entries, |event, _, app| {
            if let channel::Event::Msg(entries) = event {
                app.timed("autostart", |app| {
                    for entry in entries {
                        tracing::info!(id = entry.id, "autostart");
                        if let Some((program, args)) = entry.exec.split_first() {
                            let started = app.state.start_program(
                                program,
                                args,
                                crate::actions::Asked::BySession,
                            );
                            if let Err(err) = started {
                                tracing::warn!(%err, program, "could not start");
                            }
                        }
                    }
                });
            }
        });
        if let Err(err) = inserted {
            tracing::warn!(err = %err.error, "no autostart");
            return;
        }
        let spawned = std::thread::Builder::new()
            .name("autostart".into())
            .spawn(move || {
                let dirs = hocus_platform::apps::config_dirs();
                let _ = found.send(hocus_platform::apps::autostart(&dirs));
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "no autostart");
        }
    }

    /// As the configuration says: put the saved session back, show the
    /// card, or nothing.
    fn offer_session(&mut self, session: Option<Session>) {
        let Some(session) = session else {
            return;
        };
        match self.config.session.restore.as_str() {
            "never" => {}
            "always" => self.restore_session(&session, None),
            _ => self.show_card(session),
        }
    }

    fn show_card(&mut self, session: Session) {
        let Some(output) = self
            .model
            .primary()
            .and_then(|o| self.find_output(&o.name.0))
            .or_else(|| self.output_under_pointer())
        else {
            return;
        };
        let Some(g) = self.space.output_geometry(&output) else {
            return;
        };
        let mut apps: Vec<(String, String)> = Vec::new();
        for w in &session.windows {
            if !apps.iter().any(|(a, _)| *a == w.app_id) {
                let name = self
                    .launcher
                    .as_ref()
                    .and_then(|l| l.app_for(&w.app_id))
                    .map_or_else(|| w.app_id.clone(), |a| a.name.clone());
                apps.push((w.app_id.clone(), name));
            }
        }
        let welcome = hocus_shell::welcome::Welcome::new(
            self.theme(),
            self.shell_lang(),
            hocus_ui::Size::new(g.size.w as f32, g.size.h as f32),
            output.current_scale().fractional_scale() as f32,
            apps,
            session.windows.len(),
        );
        self.sessions.card = Some(Card {
            output: output.name(),
            panel: crate::shell::Panel::new(welcome),
            session,
        });
        self.needs_redraw = true;
    }

    pub fn card_open(&self) -> bool {
        self.sessions.card.is_some()
    }

    /// Whether the card shows on `output`.
    pub(crate) fn card_on(&self, output: &smithay::output::Output) -> bool {
        self.sessions
            .card
            .as_ref()
            .is_some_and(|card| card.output == output.name())
    }

    fn card_chose(&mut self, choice: Option<hocus_shell::welcome::WelcomeChoice>) {
        use hocus_shell::welcome::WelcomeChoice;
        let Some(choice) = choice else { return };
        let Some(card) = self.sessions.card.take() else {
            return;
        };
        match choice {
            WelcomeChoice::PutBack => self.restore_session(&card.session, None),
            WelcomeChoice::Only(apps) => self.restore_session(&card.session, Some(&apps)),
            WelcomeChoice::StartFresh => {}
        }
        self.needs_redraw = true;
    }

    /// The card takes the pointer while it shows.
    pub fn card_pointer(
        &mut self,
        at: smithay::utils::Point<f64, Logical>,
        pressed: Option<bool>,
    ) -> bool {
        let Some(card) = self.sessions.card.as_ref() else {
            return false;
        };
        let Some(g) = self
            .find_output(&card.output)
            .and_then(|o| self.space.output_geometry(&o))
        else {
            return false;
        };
        let local = hocus_ui::Point::new(
            (at.x - f64::from(g.loc.x)) as f32,
            (at.y - f64::from(g.loc.y)) as f32,
        );
        let Some(card) = self.sessions.card.as_mut() else {
            return false;
        };
        card.panel.view.pointer_move(local);
        let choice = pressed.and_then(|p| card.panel.view.pointer_button(p));
        self.card_chose(choice);
        self.needs_redraw = true;
        true
    }

    /// The card takes the keys while it shows.
    pub fn card_key(&mut self, sym: smithay::input::keyboard::Keysym) -> bool {
        let Some(card) = self.sessions.card.as_mut() else {
            return false;
        };
        let choice = crate::launcher::key_of(sym).and_then(|k| card.panel.view.key(k));
        self.card_chose(choice);
        self.needs_redraw = true;
        true
    }

    fn refresh_card(&mut self) {
        let now = self.clock.now();
        let Some(card) = self.sessions.card.as_mut() else {
            return;
        };
        let drawn = card.panel.view.draw(now);
        if card.panel.upload(&drawn) {
            self.needs_redraw = true;
        }
    }

    pub fn card_elements<R>(
        &self,
        renderer: &mut R,
        output: &smithay::output::Output,
    ) -> Option<smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement<R>>
    where
        R: smithay::backend::renderer::Renderer + smithay::backend::renderer::ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let card = self.sessions.card.as_ref()?;
        if card.output != output.name() {
            return None;
        }
        card.panel
            .element(renderer, output, smithay::utils::Point::default())
    }

    /// Its panel on glass, on the output it shows on.
    pub(crate) fn card_glass(&self, output: &smithay::output::Output) -> Vec<crate::blur::Spot> {
        match self.sessions.card.as_ref() {
            Some(card) if card.output == output.name() => crate::blur::spots(
                crate::blur::Surface::Welcome,
                Default::default(),
                &card.panel.view.glass(),
            ),
            _ => Vec::new(),
        }
    }

    pub fn card_ui(&self, out: &mut Vec<hocus_api::UiNode>) {
        let Some(card) = &self.sessions.card else {
            return;
        };
        if let Some(g) = self
            .find_output(&card.output)
            .and_then(|o| self.space.output_geometry(&o))
        {
            crate::shell::flatten(&card.panel.view.access(), g.loc, 0, out);
        }
    }

    /// Saves what is open now, changed or not, off the loop: when
    /// quitting or leaving (then [`State::finish_sessions`] waits for it),
    /// and for tests.
    pub fn save_session_now(&mut self) {
        let (session, pids) = self.snapshot();
        self.send_save(session, pids);
    }
}
