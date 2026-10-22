//! The session saved and put back (#31). What is open (each window's
//! application, output, desktop, place and state) is saved now and then and
//! when quitting, the file written on a worker thread; at the next start
//! the applications are started again and their windows put back where
//! they were as they come. An application that does not start, or whose
//! window never comes, is reported; the others are put back all the same.
//!
//! Only the session itself saves and restores: nested or headless, the
//! developer's own session file is never touched (tests give a directory).

use std::{collections::VecDeque, path::PathBuf, sync::mpsc, time::Duration};

use hocus_core::{
    OutputName,
    session::{SavedWindow, Session},
};
use smithay::{
    reexports::wayland_server::Resource,
    utils::{Logical, Rectangle, SERIAL_COUNTER},
    wayland::seat::WaylandFocus,
};

use crate::{State, desktops::title_and_app_id, window_mode::WindowMode};

/// How long a restored application has to show its window.
const PATIENCE: Duration = Duration::from_secs(15);

/// What the writer gets: the file, the session, and the windows whose
/// command comes from their process (index, pid).
type Job = (PathBuf, Session, Vec<(usize, u32)>);

#[derive(Debug, Default)]
pub struct Sessions {
    /// Where the session file goes; none: nothing is saved nor restored.
    pub dir: Option<PathBuf>,
    writer: Option<mpsc::Sender<Job>>,
    /// What was last saved, to write only changes.
    last: Option<Session>,
    /// Windows to put back as they come, by app id.
    pending: VecDeque<SavedWindow>,
    /// Applications started again, and when: those whose window does not
    /// come are reported.
    started: Vec<(String, Duration)>,
    /// Whether the saved session was offered at start.
    offered: bool,
    /// The card asking what to put back.
    card: Option<Card>,
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

impl State {
    /// Starts the writer; called once the directory is known.
    pub fn start_sessions(&mut self, dir: Option<PathBuf>) {
        self.sessions.dir = dir;
        if self.sessions.dir.is_none() {
            return;
        }
        let (tx, rx) = mpsc::channel::<Job>();
        let spawned = std::thread::Builder::new()
            .name("session".into())
            .spawn(move || {
                for (path, mut session, pids) in rx {
                    // Commands not known from desktop entries: the
                    // processes' own.
                    for (i, pid) in pids {
                        if let Some(w) = session.windows.get_mut(i)
                            && w.command.is_empty()
                        {
                            w.command = command_of(pid);
                        }
                    }
                    let write = (|| -> std::io::Result<()> {
                        if let Some(parent) = path.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        let tmp = path.with_extension("toml.new");
                        std::fs::write(&tmp, session.to_toml())?;
                        std::fs::rename(&tmp, &path)
                    })();
                    if let Err(err) = write {
                        tracing::warn!(%err, "could not save the session");
                    }
                }
            });
        match spawned {
            Ok(_) => self.sessions.writer = Some(tx),
            Err(err) => tracing::warn!(%err, "the session will not be saved"),
        }
        // Saved now and then; tests save when they ask.
        if matches!(self.clock, crate::clock::Clock::Monotonic { .. })
            && let Some(handle) = &self.loop_handle
        {
            use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
            let every = Duration::from_secs(30);
            let inserted = handle.insert_source(Timer::from_duration(every), move |_, _, app| {
                app.timed("session", |app| app.state.save_session());
                TimeoutAction::ToDuration(every)
            });
            if let Err(err) = inserted {
                tracing::warn!(%err, "the session will only be saved when quitting");
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
        let (Some(dir), Some(writer)) = (&self.sessions.dir, &self.sessions.writer) else {
            return;
        };
        let (session, pids) = self.snapshot();
        if self.sessions.last.as_ref() == Some(&session) {
            return;
        }
        let _ = writer.send((file(dir), session.clone(), pids));
        self.sessions.last = Some(session);
    }

    /// The saved session, if there is one to put back.
    pub fn saved_session(&self) -> Option<Session> {
        let dir = self.sessions.dir.as_ref()?;
        let text = std::fs::read_to_string(file(dir)).ok()?;
        Session::parse(&text)
            .inspect_err(|err| tracing::warn!(err, "the saved session is unreadable"))
            .ok()
            .filter(|s| !s.windows.is_empty())
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
            match self.try_launch(program, args) {
                Ok(()) => self.sessions.started.push((app_id, now)),
                Err(err) => {
                    tracing::warn!(app_id, %err, "could not restore");
                    self.sessions.pending.retain(|w| w.app_id != app_id);
                    self.report_restore(&app_id, &err.to_string());
                }
            }
        }
    }

    /// A window just managed: put back where it was, if it was saved.
    pub fn place_restored(&mut self, id: hocus_core::WindowId, app_id: Option<&str>) {
        let Some(app_id) = app_id else { return };
        self.sessions.started.retain(|(a, _)| a != app_id);
        let Some(i) = self
            .sessions
            .pending
            .iter()
            .position(|w| w.app_id == app_id)
        else {
            return;
        };
        let Some(saved) = self.sessions.pending.remove(i) else {
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
        self.offer_when_ready();
        self.refresh_card();
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
    /// Once the configuration is read: put the session back, ask, or not.
    fn offer_when_ready(&mut self) {
        if self.sessions.offered
            || self.sessions.dir.is_none()
            || (self.config_path.is_some() && self.config_generation == 0)
            || self.space.outputs().next().is_none()
        {
            return;
        }
        self.sessions.offered = true;
        self.offer_session();
        self.autostart();
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
                            app.state.launch(program, args);
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
    pub fn offer_session(&mut self) {
        let Some(session) = self.saved_session() else {
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

    /// Saves at once, on the loop: when quitting, and for tests.
    pub fn save_session_now(&mut self) {
        let Some(dir) = self.sessions.dir.clone() else {
            return;
        };
        let (mut session, pids) = self.snapshot();
        for (i, pid) in pids {
            if let Some(w) = session.windows.get_mut(i)
                && w.command.is_empty()
            {
                w.command = command_of(pid);
            }
        }
        let path = file(&dir);
        let written =
            std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, session.to_toml()));
        if let Err(err) = written {
            tracing::warn!(%err, "could not save the session");
        }
        self.sessions.last = Some(session);
    }
}
