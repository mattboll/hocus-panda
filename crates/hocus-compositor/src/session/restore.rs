//! The last session's applications (docs/design/windows.md §14, #725):
//! the applications open at log out start again at log in, each window
//! back in its place.
//!
//! - **Saved** in `$XDG_STATE_HOME/hocuspanda/session` (TOML): per window,
//!   its application's desktop entry, its app id, its screen (by identity,
//!   §12), that screen's size and work area, its desktop, where it floats,
//!   its state (floating, a zone, maximised, fullscreen), kept above, on
//!   all desktops, its opacity (#735); the windows of an application in
//!   the order they opened. A snapshot is taken on the loop (cheap: the
//!   model's data) a few seconds after windows changed, and as the
//!   session ends (before log out asks the windows to close); a thread of
//!   its own finds each window's desktop entry (the desktop files are
//!   files) and writes the file when it changed, so that a crash keeps a
//!   recent state. Never saved: dialogs, the Settings, the shell's own
//!   surfaces, windows whose application has no desktop entry.
//! - **Restored** once the bar shows (with the start-up applications,
//!   [`super::autostart`]), when `[session] restore` is on: each saved
//!   application starts once (not those the autostart starts: their
//!   windows still go back), and each window that maps is matched, by its
//!   app id, with the first of that application's saved windows still
//!   waiting, and put back: its screen (a screen not there: the same
//!   share of the screen it opened on, the same-numbered desktop, waiting
//!   for its screen as one unplugged, §12), its desktop, place and state.
//!   After [`PATIENCE`], the places not taken are dropped.
//! - An application that could not start (no entry, its program would not
//!   run, it ended in error before a window) is skipped; once every
//!   application came back or failed (or after [`PATIENCE`]), one
//!   notification names those that did not come back.

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

use hocus_xdg::{
    autostart,
    desktop::{self, Entry, Locale},
};
use serde::{Deserialize, Serialize};
use smithay::{
    desktop::Window,
    reexports::calloop::{
        RegistrationToken, channel,
        timer::{TimeoutAction, Timer},
    },
    wayland::seat::WaylandFocus,
};

use crate::{
    State,
    model::{self, Mode, Rect, WindowId, Zone},
    programs::{self, Env, Outcome, on_a_thread},
    screens::CameFrom,
    windows::{app_id, rect_of},
};

/// The state file the session is kept in.
pub const FILE: &str = "session";
/// How long after windows changed the session is saved.
const SAVE_AFTER: Duration = Duration::from_secs(3);
/// How long the applications started again have to show their windows.
pub const PATIENCE: Duration = Duration::from_secs(30);
/// How long the end of the session waits for its last save to be
/// written: a disk that does not answer never holds the log out.
const EXIT_PATIENCE: Duration = Duration::from_secs(2);
/// Never saved nor started again: the Settings, by their app id and
/// their entry's id.
const NEVER: [&str; 2] = ["hocus-settings", "hocuspanda"];
/// Programs that start another, named after them: an autostart entry
/// through one of them is the same application only with the same
/// arguments.
const WRAPPERS: [&str; 7] = ["env", "flatpak", "snap", "sh", "bash", "dex", "gtk-launch"];

/// The session as saved.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    #[serde(rename = "window")]
    pub windows: Vec<SavedWindow>,
}

/// A window as saved. Places are logical pixels from its screen's
/// top-left corner.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SavedWindow {
    /// Its application's desktop entry (`org.mozilla.firefox`).
    pub entry: String,
    /// The app id its window goes by, which matches it again.
    pub app_id: String,
    /// Its screen's identity (windows.md §12).
    pub screen: String,
    /// That screen's size, and its work area.
    pub screen_size: [i32; 2],
    pub area: [i32; 4],
    /// From 1.
    pub desktop: usize,
    /// Where it floats (where it goes back to, when it is not floating).
    pub place: [i32; 4],
    pub state: Shape,
    pub above: bool,
    pub everywhere: bool,
    /// Its opacity in percent (#735), when not opaque.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<u8>,
}

/// A window's state, as saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Shape {
    #[default]
    Floating,
    Maximized,
    Fullscreen,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Shape {
    fn of(mode: Mode) -> Self {
        match mode {
            Mode::Floating => Self::Floating,
            Mode::Fullscreen => Self::Fullscreen,
            Mode::Snapped(zone) => match zone {
                Zone::Maximized => Self::Maximized,
                Zone::Left => Self::Left,
                Zone::Right => Self::Right,
                Zone::TopLeft => Self::TopLeft,
                Zone::TopRight => Self::TopRight,
                Zone::BottomLeft => Self::BottomLeft,
                Zone::BottomRight => Self::BottomRight,
            },
        }
    }

    fn mode(self) -> Mode {
        let zone = match self {
            Self::Floating => return Mode::Floating,
            Self::Fullscreen => return Mode::Fullscreen,
            Self::Maximized => Zone::Maximized,
            Self::Left => Zone::Left,
            Self::Right => Zone::Right,
            Self::TopLeft => Zone::TopLeft,
            Self::TopRight => Zone::TopRight,
            Self::BottomLeft => Zone::BottomLeft,
            Self::BottomRight => Zone::BottomRight,
        };
        Mode::Snapped(zone)
    }
}

impl Saved {
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    /// The file's text.
    pub fn text(&self) -> String {
        let body = toml::to_string(self).unwrap_or_default();
        format!(
            "# The windows open as the session last ended, put back at the next\n\
             # log in ([session] restore in config.toml). Written by Hocuspanda.\n{body}"
        )
    }

    /// The applications to start: each entry once, in the order of their
    /// first window.
    pub fn entries(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for w in &self.windows {
            if !w.entry.is_empty() && !out.contains(&w.entry) {
                out.push(w.entry.clone());
            }
        }
        out
    }
}

fn array(r: Rect) -> [i32; 4] {
    [r.x, r.y, r.width, r.height]
}

fn rect([x, y, w, h]: [i32; 4]) -> Rect {
    Rect::new(x, y, w.max(1), h.max(1))
}

fn shifted(r: Rect, by: Rect, sign: i32) -> Rect {
    Rect::new(r.x + sign * by.x, r.y + sign * by.y, r.width, r.height)
}

fn never(id: &str) -> bool {
    NEVER.iter().any(|n| id.eq_ignore_ascii_case(n))
}

/// The session's saving and restoring, on the loop.
#[derive(Debug, Default)]
pub struct Restore {
    phase: Phase,
    /// The saving thread; none: nothing is saved (not a session).
    saver: Option<mpsc::Sender<Job>>,
    /// The save asked for after a change.
    timer: Option<RegistrationToken>,
    /// Logging out: the windows that close are not the session's.
    held: bool,
    /// What the thread was last given.
    last: Option<Vec<SavedWindow>>,
    /// The applications that did not come back, as last said.
    not_back: Vec<String>,
}

#[derive(Debug, Default)]
enum Phase {
    /// Nothing to restore.
    #[default]
    Off,
    /// Read as the compositor started; the applications start once the
    /// bar shows.
    Waiting(Vec<SavedWindow>),
    Restoring(Restoring),
    Done,
}

#[derive(Debug)]
struct Restoring {
    /// The saved windows not back yet, in their order.
    pending: VecDeque<SavedWindow>,
    apps: Vec<App>,
    /// Those that did not come back were said.
    told: bool,
    timer: Option<RegistrationToken>,
    heard: Option<RegistrationToken>,
}

#[derive(Debug)]
struct App {
    entry: String,
    name: String,
    back: Back,
}

/// Where an application started again is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Back {
    Starting,
    Running,
    /// Ended without a window yet, successfully: it may have handed over
    /// to another process.
    Ended,
    Failed,
    /// One of its windows came.
    Windowed,
    /// The autostart starts it: not this one's to report.
    Autostarted,
}

impl Back {
    fn decided(self) -> bool {
        matches!(self, Self::Windowed | Self::Failed | Self::Autostarted)
    }
}

/// What the saving thread does, in the order asked.
enum Job {
    Write(Vec<SavedWindow>),
    /// Answers once everything asked before is done.
    Flush(mpsc::Sender<()>),
}

/// What the restoring threads say.
#[derive(Debug)]
pub enum Heard {
    /// An application's name, and whether the autostart starts it.
    Named {
        entry: String,
        name: String,
        autostarted: bool,
    },
    Outcome(String, Outcome),
}

impl State {
    /// The compositor is the user's session (or a test asks as much),
    /// before its loop runs: the session is saved from now on, and the
    /// last one, read now, is restored once the bar shows unless
    /// `[session] restore` is off.
    pub(crate) fn session_begin(&mut self) {
        let Some(dir) = self.config.state_dir.clone() else {
            return;
        };
        self.session_restore.saver = start_saver(dir.clone(), self.config.applications.clone());
        if !self.config.session.restore {
            tracing::info!("the last session is not restored: [session] restore = false");
            self.session_restore.phase = Phase::Done;
            return;
        }
        let windows: Vec<SavedWindow> = read_saved(&dir)
            .windows
            .into_iter()
            .filter(|w| !w.entry.is_empty() && !never(&w.entry) && !never(&w.app_id))
            .collect();
        self.session_restore.phase = if windows.is_empty() {
            Phase::Done
        } else {
            Phase::Waiting(windows)
        };
    }

    /// Whether the last session waits for the bar to start again.
    pub(crate) fn session_waits(&self) -> bool {
        matches!(self.session_restore.phase, Phase::Waiting(_))
    }

    /// The bar shows: the last session's applications start, those of
    /// `autostart` (the start-up applications' directories) apart.
    pub(crate) fn session_restore_start(&mut self, autostart: Vec<PathBuf>) {
        if !self.session_waits() {
            return;
        }
        let Phase::Waiting(windows) = std::mem::take(&mut self.session_restore.phase) else {
            return;
        };
        let saved = Saved { windows };
        let entries = saved.entries();
        tracing::info!(
            windows = saved.windows.len(),
            applications = ?entries,
            "the last session comes back"
        );
        let (to, from) = channel::channel::<Heard>();
        let heard = self
            .loop_handle
            .insert_source(from, |event, _, app| {
                if let channel::Event::Msg(heard) = event {
                    app.state.session_heard(heard);
                }
            })
            .map_err(|err| tracing::warn!(%err.error, "the restored applications are not heard"))
            .ok();
        let timer = self
            .loop_handle
            .insert_source(Timer::from_duration(PATIENCE), |_, _, app| {
                if let Phase::Restoring(r) = &mut app.state.session_restore.phase {
                    r.timer = None;
                }
                app.state.session_restore_over();
                TimeoutAction::Drop
            })
            .map_err(|err| tracing::warn!(%err.error, "the restore has no end"))
            .ok();
        self.session_restore.phase = Phase::Restoring(Restoring {
            pending: saved.windows.into(),
            apps: entries
                .iter()
                .map(|e| App {
                    entry: e.clone(),
                    name: e.clone(),
                    back: Back::Starting,
                })
                .collect(),
            told: false,
            timer,
            heard,
        });
        relaunch(
            entries,
            self.config.applications.clone(),
            autostart,
            Env::of(self),
            self.config.programs.terminal.clone(),
            to,
        );
    }

    fn session_heard(&mut self, heard: Heard) {
        let Phase::Restoring(r) = &mut self.session_restore.phase else {
            return;
        };
        match heard {
            Heard::Named {
                entry,
                name,
                autostarted,
            } => {
                if let Some(app) = r.apps.iter_mut().find(|a| a.entry == entry) {
                    app.name = name;
                    if autostarted && app.back == Back::Starting {
                        app.back = Back::Autostarted;
                    }
                }
            }
            Heard::Outcome(entry, outcome) => {
                let Some(app) = r.apps.iter_mut().find(|a| a.entry == entry) else {
                    return;
                };
                if app.back.decided() {
                    return;
                }
                let failed = match outcome {
                    Outcome::Started => {
                        app.back = Back::Running;
                        false
                    }
                    Outcome::Ended(true) => {
                        app.back = Back::Ended;
                        false
                    }
                    Outcome::Failed | Outcome::Ended(false) => {
                        app.back = Back::Failed;
                        true
                    }
                };
                if failed {
                    tracing::info!(app = entry, "a restored application did not come back");
                    r.pending.retain(|w| w.entry != entry);
                }
            }
        }
        self.session_check_done();
    }

    /// Window `id` of `window` mapped (not a dialog): it takes the first
    /// saved place of its application still waiting.
    pub(crate) fn session_window_mapped(&mut self, id: WindowId, window: &Window) {
        if let Phase::Restoring(r) = &mut self.session_restore.phase
            && let Some(app) = app_id(window)
            && let Some(at) = r.pending.iter().position(|w| w.app_id == app).or_else(|| {
                r.pending
                    .iter()
                    .position(|w| w.app_id.eq_ignore_ascii_case(&app))
            })
            && let Some(saved) = r.pending.remove(at)
        {
            if let Some(a) = r.apps.iter_mut().find(|a| a.entry == saved.entry)
                && a.back != Back::Autostarted
            {
                a.back = Back::Windowed;
            }
            self.session_put_back(id, &saved);
            self.session_check_done();
        }
        self.session_changed();
    }

    /// The size a window about to open is first told: its saved place's,
    /// when it will take one floating.
    pub(crate) fn session_first_size(&self, window: &Window) -> Option<(i32, i32)> {
        let Phase::Restoring(r) = &self.session_restore.phase else {
            return None;
        };
        let app = app_id(window)?;
        let saved = r.pending.iter().find(|w| w.app_id == app)?;
        (saved.state == Shape::Floating).then(|| (saved.place[2].max(1), saved.place[3].max(1)))
    }

    /// Puts window `id` where `saved` says.
    fn session_put_back(&mut self, id: WindowId, saved: &SavedWindow) {
        let here = self
            .connector_of(&saved.screen)
            .and_then(|name| self.space.outputs().find(|o| o.name() == name).cloned());
        let gone = here.is_none();
        let Some(output) = here.or_else(|| self.output_of(id)) else {
            return;
        };
        let (Some(geometry), Some(area)) = (
            self.space.output_geometry(&output).map(rect_of),
            self.work_area(&output),
        ) else {
            return;
        };
        let mode = saved.state.mode();
        let desktop = saved.desktop.saturating_sub(1);
        let place = rect(saved.place);
        // A file without its screen's work area (written by hand): the
        // screen as it is.
        let known = saved.area[2] > 0 && saved.area[3] > 0;
        let saved_area = if known {
            rect(saved.area)
        } else {
            Rect::new(0, 0, geometry.width, geometry.height)
        };
        // The same place on a screen of the same size, the same share of
        // another (windows.md §12).
        let same = saved.screen_size == [geometry.width, geometry.height] || !known;
        let floating = if !gone && same {
            shifted(place, geometry, 1)
        } else {
            model::carried(place, saved_area, area)
        };
        let placed = match mode {
            Mode::Floating => floating,
            Mode::Snapped(zone) => zone.rect(area),
            Mode::Fullscreen => geometry,
        };
        let restore = (mode != Mode::Floating).then_some(floating);
        tracing::info!(
            window = id.0,
            app = saved.app_id,
            screen = saved.screen,
            gone,
            desktop = desktop + 1,
            ?mode,
            "a window of the last session back in its place"
        );
        if self.model.mode(id) == Mode::Floating && mode != Mode::Floating {
            self.model.set_mode(id, mode, floating);
        } else if mode == Mode::Floating {
            self.model.unsnap(id);
        }
        if let Some(managed) = self.windows.get_mut(&id) {
            // Coming back, not asking for anything.
            managed.pulse = None;
            managed.before_fullscreen = Mode::Floating;
        }
        self.relocate(id, &output, desktop, placed, restore);
        if saved.above {
            self.model.set_above(id, true);
        }
        if saved.everywhere {
            self.set_everywhere(id, true);
        }
        if let Some(percent) = saved.opacity {
            self.set_opacity(id, percent);
        }
        if gone && let Some(put) = self.put_now(id) {
            let came = CameFrom {
                output: saved.screen.clone(),
                desktop,
                size: (saved.screen_size[0], saved.screen_size[1]),
                area: saved_area,
                rect: place,
                restore: (mode != Mode::Floating).then_some(place),
                put,
            };
            if let Some(managed) = self.windows.get_mut(&id) {
                managed.came_from = Some(came);
            }
        }
        self.restack();
        self.refocus();
        self.tell_window_states(false);
        self.redraw = true;
    }

    /// Once every application came back or failed, those that did not are
    /// said; once every window is back too, the restore is over.
    fn session_check_done(&mut self) {
        let Phase::Restoring(r) = &mut self.session_restore.phase else {
            return;
        };
        let mut say = None;
        if !r.told && r.apps.iter().all(|a| a.back.decided()) {
            r.told = true;
            say = Some(
                r.apps
                    .iter()
                    .filter(|a| a.back == Back::Failed)
                    .map(|a| a.name.clone())
                    .collect::<Vec<String>>(),
            );
        }
        let over = r.told && r.pending.is_empty();
        if let Some(names) = say {
            self.session_report(names);
        }
        if over {
            self.session_restore_finish();
        }
    }

    /// [`PATIENCE`] after the applications started: the places not taken
    /// are dropped; the applications without a window did not come back.
    fn session_restore_over(&mut self) {
        let Phase::Restoring(r) = &mut self.session_restore.phase else {
            return;
        };
        if !r.pending.is_empty() {
            tracing::info!(
                left = r.pending.len(),
                "the last session's places not taken are dropped"
            );
        }
        r.pending.clear();
        let say = (!r.told).then(|| {
            r.apps
                .iter()
                .filter(|a| !matches!(a.back, Back::Windowed | Back::Autostarted))
                .map(|a| a.name.clone())
                .collect::<Vec<String>>()
        });
        r.told = true;
        if let Some(names) = say {
            self.session_report(names);
        }
        self.session_restore_finish();
    }

    fn session_restore_finish(&mut self) {
        let Phase::Restoring(r) = std::mem::replace(&mut self.session_restore.phase, Phase::Done)
        else {
            return;
        };
        for token in [r.timer, r.heard].into_iter().flatten() {
            self.loop_handle.remove(token);
        }
        tracing::info!("the last session is back");
        self.session_changed();
    }

    /// The applications that did not come back, `names`: one notification.
    fn session_report(&mut self, names: Vec<String>) {
        self.session_restore.not_back.clone_from(&names);
        if names.is_empty() {
            return;
        }
        tracing::info!(?names, "applications of the last session did not come back");
        let Some(bus) = self.shell.bus.clone() else {
            return;
        };
        crate::shell::notify(
            bus,
            "dialog-warning",
            not_back_said(&names, self.config.french),
        );
    }

    /// The applications that did not come back, as last said.
    pub(crate) fn session_not_back(&self) -> &[String] {
        &self.session_restore.not_back
    }

    /// Whether the last session is still coming back.
    pub(crate) fn session_restoring(&self) -> bool {
        matches!(
            self.session_restore.phase,
            Phase::Waiting(_) | Phase::Restoring(_)
        )
    }

    // --- Saving ------------------------------------------------------------------------

    /// Windows changed: the session is saved a moment later.
    pub(crate) fn session_changed(&mut self) {
        let restore = &self.session_restore;
        if restore.saver.is_none() || restore.held || restore.timer.is_some() {
            return;
        }
        let inserted =
            self.loop_handle
                .insert_source(Timer::from_duration(SAVE_AFTER), |_, _, app| {
                    app.state.session_restore.timer = None;
                    app.state.session_save();
                    TimeoutAction::Drop
                });
        match inserted {
            Ok(token) => self.session_restore.timer = Some(token),
            Err(err) => tracing::warn!(%err.error, "the session is not saved"),
        }
    }

    /// Saves the session now, if it changed since it last was.
    pub(crate) fn session_save(&mut self) {
        let Some(saver) = &self.session_restore.saver else {
            return;
        };
        let snapshot = self.session_snapshot();
        if self.session_restore.last.as_ref() == Some(&snapshot) {
            return;
        }
        if saver.send(Job::Write(snapshot.clone())).is_err() {
            tracing::warn!("the session's thread is gone: not saved");
            return;
        }
        self.session_restore.last = Some(snapshot);
    }

    /// Log out asks the windows to close: the session as it is now is
    /// saved, and nothing after.
    pub(crate) fn session_leaving(&mut self) {
        self.session_save();
        self.session_restore.held = true;
        if let Some(token) = self.session_restore.timer.take() {
            self.loop_handle.remove(token);
        }
    }

    /// Log out was cancelled: saved again as windows change.
    pub(crate) fn session_stays(&mut self) {
        self.session_restore.held = false;
    }

    /// The compositor ends: the session is saved (unless log out did),
    /// and written before it goes, [`EXIT_PATIENCE`] at most.
    pub fn session_end(&mut self) {
        if !self.session_restore.held {
            self.session_save();
        }
        if !self.session_flush(EXIT_PATIENCE) {
            tracing::warn!("the session's last save was not written in time");
        }
    }

    /// Waits until what was asked of the saving thread is done, `patience`
    /// at most; whether it was.
    pub(crate) fn session_flush(&self, patience: Duration) -> bool {
        let Some(saver) = &self.session_restore.saver else {
            return true;
        };
        let (done, wait) = mpsc::channel();
        if saver.send(Job::Flush(done)).is_err() {
            return false;
        }
        wait.recv_timeout(patience).is_ok()
    }

    /// The windows as they are, then the saved ones still waiting to come
    /// back (a crash or a log out meanwhile loses none): what the session
    /// is now.
    fn session_snapshot(&self) -> Vec<SavedWindow> {
        let mut out = Vec::new();
        for id in self.windows.keys() {
            if let Some(saved) = self.session_window(*id) {
                out.push(saved);
            }
        }
        match &self.session_restore.phase {
            Phase::Waiting(windows) => out.extend(windows.iter().cloned()),
            Phase::Restoring(r) => out.extend(r.pending.iter().cloned()),
            Phase::Off | Phase::Done => {}
        }
        out
    }

    /// Window `id` as saved, its entry left to the thread: none for a
    /// dialog, the Settings and the shell's.
    fn session_window(&self, id: WindowId) -> Option<SavedWindow> {
        if self.model.parent(id).is_some() {
            return None;
        }
        let managed = self.windows.get(&id)?;
        let app = app_id(&managed.window).filter(|a| !a.trim().is_empty())?;
        if never(&app)
            || managed
                .window
                .wl_surface()
                .is_some_and(|s| self.is_shell_surface(&s))
        {
            return None;
        }
        let mode = self.model.mode(id);
        let above = self.model.is_above(id);
        let everywhere = self.model.is_everywhere(id);
        let opacity = Some(managed.opacity).filter(|p| *p < crate::opacity::FULL);
        // Waiting for its screen gone: where it was there.
        if let Some(came) = managed
            .came_from
            .as_ref()
            .filter(|c| self.put_now(id).as_ref() == Some(&c.put))
        {
            let place = match mode {
                Mode::Floating => came.rect,
                _ => came.restore.unwrap_or(came.rect),
            };
            return Some(SavedWindow {
                entry: String::new(),
                app_id: app,
                screen: came.output.clone(),
                screen_size: [came.size.0, came.size.1],
                area: array(came.area),
                desktop: came.desktop + 1,
                place: array(place),
                state: Shape::of(mode),
                above,
                everywhere,
                opacity,
            });
        }
        let output = self.output_of(id)?;
        let geometry = rect_of(self.space.output_geometry(&output)?);
        let area = self.work_area(&output)?;
        let place = match mode {
            Mode::Floating => self.target_rect(id)?,
            _ => self
                .model
                .placement(id)
                .restore
                .or_else(|| self.home_rect(id))?,
        };
        Some(SavedWindow {
            entry: String::new(),
            app_id: app,
            screen: self.screen_key(&output.name()),
            screen_size: [geometry.width, geometry.height],
            area: array(shifted(area, geometry, -1)),
            desktop: self.model.desktop(id) + 1,
            place: array(shifted(place, geometry, -1)),
            state: Shape::of(mode),
            above,
            everywhere,
            opacity,
        })
    }
}

/// "Slack et Signal n'ont pas pu être rouvertes." / "Slack and Signal
/// could not be reopened."
pub fn not_back_said(names: &[String], french: bool) -> String {
    let and = if french { " et " } else { " and " };
    let listed = match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{}{and}{last}", rest.join(", ")),
    };
    match (french, names.len()) {
        (true, 1) => format!("{listed} n’a pas pu être rouverte."),
        (true, _) => format!("{listed} n’ont pas pu être rouvertes."),
        (false, _) => format!("{listed} could not be reopened."),
    }
}

/// The saved session in `dir`; none when there is none or it does not
/// read (said in the log). Read once, before the loop runs.
fn read_saved(dir: &Path) -> Saved {
    let file = dir.join(FILE);
    match std::fs::read_to_string(&file) {
        Ok(text) => Saved::parse(&text).unwrap_or_else(|err| {
            tracing::warn!(file = %file.display(), %err, "the last session does not read");
            Saved::default()
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Saved::default(),
        Err(err) => {
            tracing::warn!(file = %file.display(), %err, "the last session not read");
            Saved::default()
        }
    }
}

/// The thread that writes the session into `dir`, finding each window's
/// desktop entry in `applications`.
fn start_saver(dir: PathBuf, applications: Vec<PathBuf>) -> Option<mpsc::Sender<Job>> {
    let (to, jobs) = mpsc::channel();
    let started = std::thread::Builder::new()
        .name("session save".to_owned())
        .spawn(move || save_jobs(&dir, &applications, &jobs));
    match started {
        Ok(_) => Some(to),
        Err(err) => {
            tracing::warn!(%err, "no thread to save the session: not saved");
            None
        }
    }
}

/// The desktop entries, read again when an app id finds none and they
/// were read a while ago (an application installed meanwhile).
struct Entries {
    applications: Vec<PathBuf>,
    read: Option<(Vec<Entry>, Instant)>,
}

impl Entries {
    const AGAIN_AFTER: Duration = Duration::from_secs(30);

    fn scan(&self) -> Vec<Entry> {
        desktop::scan_applications(
            &self.applications,
            &Locale::from_env(),
            &[super::DESKTOP.to_owned()],
        )
    }

    fn for_app(&mut self, app_id: &str) -> Option<String> {
        if self.read.is_none() {
            self.read = Some((self.scan(), Instant::now()));
        }
        let found = |read: &Option<(Vec<Entry>, Instant)>| {
            read.as_ref()
                .and_then(|(e, _)| desktop::entry_for_app(e, app_id))
                .map(|e| e.id.clone())
        };
        if let Some(id) = found(&self.read) {
            return Some(id);
        }
        if self
            .read
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() > Self::AGAIN_AFTER)
        {
            self.read = Some((self.scan(), Instant::now()));
            return found(&self.read);
        }
        None
    }
}

fn save_jobs(dir: &Path, applications: &[PathBuf], jobs: &mpsc::Receiver<Job>) {
    let mut entries = Entries {
        applications: applications.to_vec(),
        read: None,
    };
    let mut written: Option<String> = None;
    while let Ok(job) = jobs.recv() {
        let windows = match job {
            Job::Flush(done) => {
                let _ = done.send(());
                continue;
            }
            Job::Write(windows) => windows,
        };
        let mut saved = Saved::default();
        for mut window in windows {
            if window.entry.is_empty() {
                match entries.for_app(&window.app_id) {
                    Some(id) if !never(&id) => window.entry = id,
                    Some(_) => continue,
                    None => {
                        tracing::debug!(
                            app = window.app_id,
                            "a window without a desktop entry: not saved"
                        );
                        continue;
                    }
                }
            }
            saved.windows.push(window);
        }
        let text = saved.text();
        if written.as_ref() == Some(&text) {
            continue;
        }
        match write_state(dir, &text) {
            Ok(()) => {
                tracing::debug!(windows = saved.windows.len(), "the session saved");
                written = Some(text);
            }
            Err(err) => tracing::warn!(dir = %dir.display(), %err, "could not save the session"),
        }
    }
}

/// Writes the session's file in `dir`, through a temporary file so that
/// it is never read half written.
fn write_state(dir: &Path, text: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let partial = dir.join(format!("{FILE}.new"));
    std::fs::write(&partial, text)?;
    std::fs::rename(&partial, dir.join(FILE))
}

/// The program a command runs, unless it is a wrapper.
fn program(argv: &[String]) -> Option<&str> {
    let first = argv.first()?;
    let name = first.rsplit('/').next().unwrap_or(first);
    (!WRAPPERS.contains(&name)).then_some(name)
}

/// Whether the autostart `entry` starts the application `app`.
fn starts_app(entry: &autostart::Entry, app: Option<&Entry>, id: &str) -> bool {
    if entry.id == id {
        return true;
    }
    let Some(app) = app else {
        return false;
    };
    (!app.argv.is_empty() && app.argv == entry.argv)
        || program(&app.argv).is_some_and(|p| program(&entry.argv) == Some(p))
}

/// Starts each of `apps` (desktop entries of `applications`) again, on
/// threads: those that `autostart`'s entries start are only named.
fn relaunch(
    apps: Vec<String>,
    applications: Vec<PathBuf>,
    autostart_dirs: Vec<PathBuf>,
    env: Env,
    terminal: String,
    heard: channel::Sender<Heard>,
) {
    on_a_thread("restore the session".to_owned(), move || {
        let locale = Locale::from_env();
        let desktops = [super::DESKTOP.to_owned()];
        let entries = desktop::scan_applications(&applications, &locale, &desktops);
        let starting: Vec<autostart::Entry> = autostart::scan(&autostart_dirs, &locale, &desktops)
            .into_iter()
            .filter(autostart::Entry::starts)
            .collect();
        for id in apps {
            let found = entries.iter().find(|e| e.id == id);
            let autostarted = starting.iter().any(|a| starts_app(a, found, &id));
            let name = found.map_or_else(|| id.clone(), |e| e.name.clone());
            let _ = heard.send(Heard::Named {
                entry: id.clone(),
                name,
                autostarted,
            });
            if autostarted {
                tracing::info!(app = id, "of the last session, the autostart starts it");
                continue;
            }
            let Some(entry) = found else {
                tracing::warn!(app = id, "of the last session, no longer installed");
                let _ = heard.send(Heard::Outcome(id, Outcome::Failed));
                continue;
            };
            let launch = hocus_link::Launch {
                id: entry.id.clone(),
                terminal: entry.terminal,
                dir: entry.dir.clone().unwrap_or_default(),
                argv: entry.argv.clone(),
            };
            let term = launch.terminal.then(|| terminal.clone());
            let to = heard.clone();
            programs::launch_watched(launch, term, env.clone(), move |outcome| {
                let _ = to.send(Heard::Outcome(id.clone(), outcome));
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_reads_back_and_starts_each_application_once() -> Result<(), String> {
        let window = |entry: &str, desktop, state| SavedWindow {
            entry: entry.into(),
            app_id: entry.into(),
            screen: "DEL U3423WE 7XY1234".into(),
            screen_size: [3440, 1440],
            area: [8, 8, 3424, 1424],
            desktop,
            place: [100, 80, 1200, 800],
            state,
            above: desktop == 2,
            everywhere: false,
            opacity: (desktop == 2).then_some(60),
        };
        let saved = Saved {
            windows: vec![
                window("firefox", 1, Shape::Maximized),
                window("slack", 2, Shape::Floating),
                window("firefox", 3, Shape::TopLeft),
            ],
        };
        assert_eq!(Saved::parse(&saved.text())?, saved);
        assert_eq!(saved.entries(), ["firefox", "slack"]);
        for shape in [Shape::Floating, Shape::Fullscreen, Shape::BottomRight] {
            assert_eq!(Shape::of(shape.mode()), shape);
        }
        Ok(())
    }

    #[test]
    fn those_not_back_are_named_in_one_line() {
        let names = |n: &[&str]| n.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            not_back_said(&names(&["Slack"]), true),
            "Slack n’a pas pu être rouverte."
        );
        assert_eq!(
            not_back_said(&names(&["Slack", "Signal", "Zoom"]), true),
            "Slack, Signal et Zoom n’ont pas pu être rouvertes."
        );
        assert_eq!(
            not_back_said(&names(&["Slack", "Signal"]), false),
            "Slack and Signal could not be reopened."
        );
    }

    #[test]
    fn an_autostart_entry_starts_the_same_program_not_a_wrapper() {
        let app = |argv: &[&str]| Entry {
            id: "app".into(),
            argv: argv.iter().map(|s| (*s).to_owned()).collect(),
            ..Entry::default()
        };
        let auto = |id: &str, argv: &[&str]| autostart::Entry {
            id: id.into(),
            file: PathBuf::new(),
            user: true,
            installed: None,
            name: id.into(),
            comment: None,
            icon: None,
            exec: String::new(),
            argv: argv.iter().map(|s| (*s).to_owned()).collect(),
            terminal: false,
            dir: None,
            hidden: false,
            here: autostart::Here::Starts,
            only_in: Vec::new(),
        };
        let slack = app(&["/usr/bin/slack"]);
        assert!(starts_app(&auto("slack", &["x"]), None, "slack"));
        assert!(starts_app(
            &auto("other", &["slack", "-u"]),
            Some(&slack),
            "app"
        ));
        let flatpak = app(&["flatpak", "run", "com.slack.Slack"]);
        assert!(!starts_app(
            &auto("signal", &["flatpak", "run", "org.signal.Signal"]),
            Some(&flatpak),
            "app"
        ));
        assert!(starts_app(
            &auto("slack", &["flatpak", "run", "com.slack.Slack"]),
            Some(&flatpak),
            "app"
        ));
    }
}
