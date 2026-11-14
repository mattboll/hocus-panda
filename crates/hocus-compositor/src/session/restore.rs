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

use hocus_words::compositor::session_restore as txt;
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
    /// The projects open as it ended (docs/design/projects.md §9).
    #[serde(rename = "project", skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<SessionProject>,
}

/// A project open as the session ended: its id, screen and desktop (from
/// 1). With `[session] restore = false`, its windows of this file become
/// its contents, and it is closed.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SessionProject {
    pub id: String,
    pub screen: String,
    pub desktop: usize,
}

/// A window as saved. Places are logical pixels from its screen's
/// top-left corner.
///
/// What a project keeps of a window besides (docs/design/projects.md §6)
/// goes in tables of its own: `terminal` (lot B); a browser's tabs
/// (`browser`, lot C) and an agent's conversation (`agent`, lot D) are
/// added beside it.
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
    /// A terminal's folder (§8.2), which it opens in again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<SavedTerminal>,
    /// Its client's process, for its thread to read a terminal's folder;
    /// never written.
    #[serde(skip)]
    pub pid: Option<u32>,
}

/// A terminal window's folder.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SavedTerminal {
    pub folder: PathBuf,
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

    pub(crate) fn mode(self) -> Mode {
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
}

/// What starts to bring `windows` back: each application once, in the
/// order of their first window, and each terminal with a folder on its own
/// (§8.2).
pub fn starts_of(windows: &[SavedWindow]) -> Vec<Start> {
    let mut out: Vec<Start> = Vec::new();
    for w in windows.iter().filter(|w| !w.entry.is_empty()) {
        match &w.terminal {
            Some(terminal) => out.push(Start {
                entry: w.entry.clone(),
                app_id: w.app_id.clone(),
                folder: Some(terminal.folder.clone()),
            }),
            None => {
                if !out.iter().any(|s| s.entry == w.entry && s.folder.is_none()) {
                    out.push(Start {
                        entry: w.entry.clone(),
                        app_id: w.app_id.clone(),
                        folder: None,
                    });
                }
            }
        }
    }
    out
}

/// A program started to bring windows back: its desktop entry, the app id
/// of its windows, and the folder a terminal opens in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Start {
    pub entry: String,
    pub app_id: String,
    pub folder: Option<PathBuf>,
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

/// The session's saving and restoring, on the loop, and the projects'
/// windows coming back (docs/design/projects.md §7), the same way.
#[derive(Debug, Default)]
pub struct Restore {
    phase: Phase,
    /// The closed projects reopening, each its windows coming back.
    projects: Vec<Comeback>,
    /// The saving thread; none: nothing is saved (not a session).
    saver: Option<mpsc::Sender<Job>>,
    /// The save asked for after a change.
    timer: Option<RegistrationToken>,
    /// Logging out: the windows that close are not the session's.
    held: bool,
    /// What the thread was last given.
    last: Option<(Vec<SavedWindow>, Vec<SessionProject>)>,
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
    Restoring(Comeback),
    Done,
}

/// Whose windows come back: the last session's, or a project's (its id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Whose {
    Session,
    Project(String),
}

/// Windows coming back: the saved places not taken yet, the programs
/// started for them.
#[derive(Debug)]
pub struct Comeback {
    whose: Whose,
    /// The saved windows not back yet, in their order.
    pending: VecDeque<SavedWindow>,
    starts: Vec<Started>,
    /// Those that did not come back were said.
    told: bool,
    /// The project's folder is gone: its terminals opened in `$HOME`.
    folder_gone: Option<PathBuf>,
    timer: Option<RegistrationToken>,
    heard: Option<RegistrationToken>,
}

#[derive(Debug)]
struct Started {
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
    /// The windows, the projects open, and the projects' folders by
    /// their screens and desktops (from 1).
    Write(
        Vec<SavedWindow>,
        Vec<SessionProject>,
        Vec<(String, usize, PathBuf)>,
    ),
    /// Answers once everything asked before is done.
    Flush(mpsc::Sender<()>),
}

/// What the restoring threads say of start `.0` (its place in the
/// comeback's starts).
#[derive(Debug)]
pub enum Heard {
    /// An application's name, and whether the autostart starts it.
    Named {
        at: usize,
        name: String,
        autostarted: bool,
    },
    Outcome(usize, Outcome),
    /// The project's folder is not there.
    FolderGone(PathBuf),
}

impl State {
    /// The compositor is the user's session (or a test asks as much),
    /// before its loop runs: the session is saved from now on, and the
    /// last one, read now, is restored once the bar shows unless
    /// `[session] restore` is off: then the projects open as it ended
    /// are closed, its windows on their desktops their contents.
    pub(crate) fn session_begin(&mut self) {
        let Some(dir) = self.config.state_dir.clone() else {
            return;
        };
        self.session_restore.saver = start_saver(dir.clone(), self.config.applications.clone());
        let read = read_saved(&dir);
        if !self.config.session.restore {
            tracing::info!("the last session is not restored: [session] restore = false");
            self.session_restore.phase = Phase::Done;
            self.projects_left_open(&read);
            return;
        }
        let windows: Vec<SavedWindow> = read
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
        let comeback = self.comeback_start(Whose::Session, windows, autostart, None);
        self.session_restore.phase = Phase::Restoring(comeback);
    }

    /// `windows` come back for `whose`: each start off the loop, as
    /// session children, the places taken as their windows map, those
    /// not taken dropped after [`PATIENCE`]. A project's `folder` is
    /// looked at: gone, its terminals open in `$HOME`.
    pub(crate) fn comeback_start(
        &mut self,
        whose: Whose,
        windows: Vec<SavedWindow>,
        autostart: Vec<PathBuf>,
        folder: Option<PathBuf>,
    ) -> Comeback {
        let starts = starts_of(&windows);
        tracing::info!(
            ?whose,
            windows = windows.len(),
            starts = ?starts.iter().map(|s| s.entry.as_str()).collect::<Vec<_>>(),
            "windows come back"
        );
        let (to, from) = channel::channel::<Heard>();
        let of = whose.clone();
        let heard = self
            .loop_handle
            .insert_source(from, move |event, _, app| {
                if let channel::Event::Msg(heard) = event {
                    app.state.comeback_heard(&of, heard);
                }
            })
            .map_err(|err| tracing::warn!(%err.error, "the restored applications are not heard"))
            .ok();
        let of = whose.clone();
        let timer = self
            .loop_handle
            .insert_source(Timer::from_duration(PATIENCE), move |_, _, app| {
                if let Some(c) = app.state.comeback_mut(&of) {
                    c.timer = None;
                }
                app.state.comeback_over(&of);
                TimeoutAction::Drop
            })
            .map_err(|err| tracing::warn!(%err.error, "the restore has no end"))
            .ok();
        relaunch(
            Relaunch {
                starts: starts.clone(),
                applications: self.config.applications.clone(),
                autostart,
                env: Env::of(self),
                terminal: self.config.programs.terminal.clone(),
                folder,
                home: self.config.home.clone(),
            },
            to,
        );
        Comeback {
            whose,
            pending: windows.into(),
            starts: starts
                .iter()
                .map(|s| Started {
                    entry: s.entry.clone(),
                    name: s.entry.clone(),
                    back: Back::Starting,
                })
                .collect(),
            told: false,
            folder_gone: None,
            timer,
            heard,
        }
    }

    /// A project's windows come back.
    pub(crate) fn comeback_add(&mut self, comeback: Comeback) {
        self.session_restore.projects.push(comeback);
    }

    fn comeback_mut(&mut self, whose: &Whose) -> Option<&mut Comeback> {
        match whose {
            Whose::Session => match &mut self.session_restore.phase {
                Phase::Restoring(c) => Some(c),
                _ => None,
            },
            Whose::Project(_) => self
                .session_restore
                .projects
                .iter_mut()
                .find(|c| c.whose == *whose),
        }
    }

    fn comeback_heard(&mut self, whose: &Whose, heard: Heard) {
        let Some(c) = self.comeback_mut(whose) else {
            return;
        };
        match heard {
            Heard::Named {
                at,
                name,
                autostarted,
            } => {
                if let Some(start) = c.starts.get_mut(at) {
                    start.name = name;
                    if autostarted && start.back == Back::Starting {
                        start.back = Back::Autostarted;
                    }
                }
            }
            Heard::FolderGone(folder) => c.folder_gone = Some(folder),
            Heard::Outcome(at, outcome) => {
                let Some(start) = c.starts.get_mut(at) else {
                    return;
                };
                if start.back.decided() {
                    return;
                }
                let failed = match outcome {
                    Outcome::Started => {
                        start.back = Back::Running;
                        false
                    }
                    Outcome::Ended(true) => {
                        start.back = Back::Ended;
                        false
                    }
                    Outcome::Failed | Outcome::Ended(false) => {
                        start.back = Back::Failed;
                        true
                    }
                };
                if failed {
                    let entry = start.entry.clone();
                    tracing::info!(app = entry, "an application did not come back");
                    // Its windows wait no more, unless another start of
                    // the same entry may bring them.
                    let more = c
                        .starts
                        .iter()
                        .any(|s| s.entry == entry && !s.back.decided());
                    if !more {
                        c.pending.retain(|w| w.entry != entry);
                    } else if let Some(at) = c.pending.iter().position(|w| w.entry == entry) {
                        c.pending.remove(at);
                    }
                }
            }
        }
        self.comeback_check(whose);
    }

    /// Window `id` of `window` mapped (not a dialog): it takes the first
    /// saved place of its application still waiting, the last session's
    /// first, then a reopening project's.
    pub(crate) fn session_window_mapped(&mut self, id: WindowId, window: &Window) {
        if let Some(app) = app_id(window) {
            let restore = &mut self.session_restore;
            let comebacks = match &mut restore.phase {
                Phase::Restoring(c) => Some(c),
                _ => None,
            }
            .into_iter()
            .chain(restore.projects.iter_mut());
            let mut taken = None;
            for c in comebacks {
                if let Some(at) = c.pending.iter().position(|w| w.app_id == app).or_else(|| {
                    c.pending
                        .iter()
                        .position(|w| w.app_id.eq_ignore_ascii_case(&app))
                }) && let Some(saved) = c.pending.remove(at)
                {
                    if let Some(s) = c.starts.iter_mut().find(|s| {
                        s.entry == saved.entry
                            && !matches!(s.back, Back::Windowed | Back::Autostarted)
                    }) {
                        s.back = Back::Windowed;
                    }
                    taken = Some((c.whose.clone(), saved));
                    break;
                }
            }
            if let Some((whose, saved)) = taken {
                self.session_put_back(id, &saved);
                self.comeback_check(&whose);
            }
        }
        self.session_changed();
    }

    /// The size a window about to open is first told: its saved place's,
    /// when it will take one floating.
    pub(crate) fn session_first_size(&self, window: &Window) -> Option<(i32, i32)> {
        let app = app_id(window)?;
        let restore = &self.session_restore;
        let saved = match &restore.phase {
            Phase::Restoring(c) => Some(c),
            _ => None,
        }
        .into_iter()
        .chain(restore.projects.iter())
        .find_map(|c| c.pending.iter().find(|w| w.app_id == app))?;
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

    /// Once every start came back or failed, those that did not are said;
    /// once every window is back too, the comeback is over.
    fn comeback_check(&mut self, whose: &Whose) {
        let Some(c) = self.comeback_mut(whose) else {
            return;
        };
        let mut say = None;
        if !c.told && c.starts.iter().all(|a| a.back.decided()) {
            c.told = true;
            say = Some(failed_names(c, |b| b == Back::Failed));
        }
        let over = c.told && c.pending.is_empty();
        let gone = c.folder_gone.clone();
        if let Some(names) = say {
            self.comeback_report(whose, names, gone);
        }
        if over {
            self.comeback_finish(whose);
        }
    }

    /// [`PATIENCE`] after the starts: the places not taken are dropped;
    /// the applications without a window did not come back.
    fn comeback_over(&mut self, whose: &Whose) {
        let Some(c) = self.comeback_mut(whose) else {
            return;
        };
        if !c.pending.is_empty() {
            tracing::info!(
                left = c.pending.len(),
                ?whose,
                "places not taken are dropped"
            );
        }
        c.pending.clear();
        let say = (!c.told)
            .then(|| failed_names(c, |b| !matches!(b, Back::Windowed | Back::Autostarted)));
        c.told = true;
        let gone = c.folder_gone.clone();
        if let Some(names) = say {
            self.comeback_report(whose, names, gone);
        }
        self.comeback_finish(whose);
    }

    fn comeback_finish(&mut self, whose: &Whose) {
        let comeback = match whose {
            Whose::Session => {
                match std::mem::replace(&mut self.session_restore.phase, Phase::Done) {
                    Phase::Restoring(c) => Some(c),
                    other => {
                        self.session_restore.phase = other;
                        None
                    }
                }
            }
            Whose::Project(_) => {
                let at = self
                    .session_restore
                    .projects
                    .iter()
                    .position(|c| c.whose == *whose);
                at.map(|at| self.session_restore.projects.remove(at))
            }
        };
        let Some(comeback) = comeback else {
            return;
        };
        for token in [comeback.timer, comeback.heard].into_iter().flatten() {
            self.loop_handle.remove(token);
        }
        match whose {
            Whose::Session => tracing::info!("the last session is back"),
            Whose::Project(id) => self.project_back(id),
        }
        self.session_changed();
    }

    /// What did not come back for `whose`, `names`; `gone`, a project's
    /// folder that is not there: one notification.
    fn comeback_report(&mut self, whose: &Whose, names: Vec<String>, gone: Option<PathBuf>) {
        match whose {
            Whose::Session => self.session_report(names),
            Whose::Project(id) => self.project_not_back(id, names, gone),
        }
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
            not_back_said(&names, self.config.lang),
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

    /// Whether project `id`'s windows are still coming back.
    pub(crate) fn project_coming_back(&self, id: &str) -> bool {
        self.session_restore
            .projects
            .iter()
            .any(|c| c.whose == Whose::Project(id.to_owned()))
    }

    /// The windows still waiting to come back on `screen`'s desktop
    /// `desktop` (from 1): a project's desktop reopening is not empty.
    pub(crate) fn coming_back_to(&self, screen: &str, desktop: usize) -> bool {
        let restore = &self.session_restore;
        let session = match &restore.phase {
            Phase::Waiting(w) => w.iter().any(|w| w.screen == screen && w.desktop == desktop),
            Phase::Restoring(c) => c
                .pending
                .iter()
                .any(|w| w.screen == screen && w.desktop == desktop),
            Phase::Off | Phase::Done => false,
        };
        session
            || restore.projects.iter().any(|c| {
                c.pending
                    .iter()
                    .any(|w| w.screen == screen && w.desktop == desktop)
            })
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
        self.session_save_as(false);
    }

    /// Saves the session now; `again`, even when its windows did not
    /// change: the terminals' folders, which the thread reads, may have.
    fn session_save_as(&mut self, again: bool) {
        let Some(saver) = &self.session_restore.saver else {
            return;
        };
        let snapshot = (self.session_snapshot(), self.session_projects());
        if !again && self.session_restore.last.as_ref() == Some(&snapshot) {
            return;
        }
        let folders = self
            .projects
            .list
            .iter()
            .filter_map(|p| {
                let open = p.open.as_ref()?;
                Some((open.screen.clone(), open.desktop, p.folder.clone()))
            })
            .collect();
        if saver
            .send(Job::Write(snapshot.0.clone(), snapshot.1.clone(), folders))
            .is_err()
        {
            tracing::warn!("the session's thread is gone: not saved");
            return;
        }
        self.session_restore.last = Some(snapshot);
    }

    /// The projects open now, as the session's file keeps them.
    fn session_projects(&self) -> Vec<SessionProject> {
        self.projects
            .list
            .iter()
            .filter_map(|p| {
                let open = p.open.as_ref()?;
                Some(SessionProject {
                    id: p.id.clone(),
                    screen: open.screen.clone(),
                    desktop: open.desktop,
                })
            })
            .collect()
    }

    /// Log out asks the windows to close: the session as it is now is
    /// saved, its terminals' folders read again, and nothing after.
    pub(crate) fn session_leaving(&mut self) {
        self.session_save_as(true);
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
            self.session_save_as(true);
        }
        if !self.session_flush(EXIT_PATIENCE) {
            tracing::warn!("the session's last save was not written in time");
        }
    }

    /// The compositor ends on a panic of its loop (#778): the last save
    /// asked is written, and no new one is taken of a state the panic
    /// left half changed.
    pub(crate) fn session_end_as_saved(&self) {
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
        // A project's windows still coming back are its desktop's.
        for c in &self.session_restore.projects {
            out.extend(c.pending.iter().cloned());
        }
        out
    }

    /// Window `id` as saved, its entry left to the thread: none for a
    /// dialog, the Settings and the shell's.
    pub(crate) fn session_window(&self, id: WindowId) -> Option<SavedWindow> {
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
                terminal: None,
                pid: self.pid_of(&managed.window),
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
            terminal: None,
            pid: self.pid_of(&managed.window),
        })
    }
}

/// The names of `c`'s starts whose state `failed` says did not come back,
/// each once.
fn failed_names(c: &Comeback, failed: impl Fn(Back) -> bool) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for s in c.starts.iter().filter(|s| failed(s.back)) {
        if !names.contains(&s.name) {
            names.push(s.name.clone());
        }
    }
    names
}

/// " et " / " and ": the last of a list's joint.
pub fn and_word(lang: hocus_words::Lang) -> &'static str {
    txt::AND.get(lang)
}

/// "Slack et Signal n'ont pas pu être rouvertes." / "Slack and Signal
/// could not be reopened."
pub fn not_back_said(names: &[String], lang: hocus_words::Lang) -> String {
    let and = txt::AND.get(lang);
    let listed = match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{}{and}{last}", rest.join(", ")),
    };
    txt::not_reopened(
        lang,
        txt::NotReopened {
            n: names.len(),
            listed: &listed,
        },
    )
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
pub(crate) struct Entries {
    applications: Vec<PathBuf>,
    read: Option<(Vec<Entry>, Instant)>,
}

impl Entries {
    const AGAIN_AFTER: Duration = Duration::from_secs(30);

    pub(crate) fn new(applications: Vec<PathBuf>) -> Self {
        Self {
            applications,
            read: None,
        }
    }

    fn scan(&self) -> Vec<Entry> {
        desktop::scan_applications(
            &self.applications,
            &Locale::from_env(),
            &[super::DESKTOP.to_owned()],
        )
    }

    /// The entry of the windows of `app_id`.
    pub(crate) fn for_app(&mut self, app_id: &str) -> Option<Entry> {
        if self.read.is_none() {
            self.read = Some((self.scan(), Instant::now()));
        }
        let found = |read: &Option<(Vec<Entry>, Instant)>| {
            read.as_ref()
                .and_then(|(e, _)| desktop::entry_for_app(e, app_id))
                .cloned()
        };
        if let Some(entry) = found(&self.read) {
            return Some(entry);
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

    /// The entry of id `id`.
    pub(crate) fn by_id(&mut self, id: &str) -> Option<Entry> {
        if self.read.is_none() {
            self.read = Some((self.scan(), Instant::now()));
        }
        self.read
            .as_ref()
            .and_then(|(e, _)| e.iter().find(|e| e.id == id))
            .cloned()
    }
}

/// The folder of terminal window `window` of `entry` (§8.2), its process
/// serving `windows` windows, in the project of folder `within` if it is
/// in one: read from `/proc`, off the loop.
pub(crate) fn terminal_of(
    window: &SavedWindow,
    entry: &Entry,
    windows: usize,
    within: Option<&Path>,
) -> Option<crate::terminals::Found> {
    if !crate::terminals::is_terminal_entry(entry)
        && !crate::terminals::is_terminal_app(&window.app_id)
    {
        return None;
    }
    crate::terminals::read(window.pid?, windows, within)
}

fn save_jobs(dir: &Path, applications: &[PathBuf], jobs: &mpsc::Receiver<Job>) {
    let mut entries = Entries::new(applications.to_vec());
    let mut written: Option<String> = None;
    while let Ok(job) = jobs.recv() {
        let (windows, projects, folders) = match job {
            Job::Flush(done) => {
                let _ = done.send(());
                continue;
            }
            Job::Write(windows, projects, folders) => (windows, projects, folders),
        };
        let mut saved = Saved {
            windows: Vec::new(),
            projects,
        };
        for mut window in windows.iter().cloned() {
            let entry = if window.entry.is_empty() {
                match entries.for_app(&window.app_id) {
                    Some(e) if !never(&e.id) => {
                        window.entry.clone_from(&e.id);
                        Some(e)
                    }
                    Some(_) => continue,
                    None => {
                        tracing::debug!(
                            app = window.app_id,
                            "a window without a desktop entry: not saved"
                        );
                        continue;
                    }
                }
            } else {
                entries.by_id(&window.entry)
            };
            // Every terminal comes back in its folder (§13, C).
            if window.terminal.is_none()
                && let Some(entry) = &entry
            {
                let shared = windows
                    .iter()
                    .filter(|w| w.pid.is_some() && w.pid == window.pid)
                    .count();
                let within = folders
                    .iter()
                    .find(|(screen, desktop, _)| {
                        *screen == window.screen && *desktop == window.desktop
                    })
                    .map(|(.., f)| f.as_path());
                window.terminal =
                    terminal_of(&window, entry, shared, within).map(|found| SavedTerminal {
                        folder: found.folder,
                    });
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

/// What [`relaunch`] starts, and with what.
struct Relaunch {
    starts: Vec<Start>,
    applications: Vec<PathBuf>,
    /// The start-up applications' directories: those they start are only
    /// named (the last session's only).
    autostart: Vec<PathBuf>,
    env: Env,
    terminal: String,
    /// A project's folder, looked at: gone, its terminals open in `home`.
    folder: Option<PathBuf>,
    home: Option<PathBuf>,
}

/// Starts each of `starts` (desktop entries of `applications`) again, on
/// threads, each terminal in its folder (§8.2): those that the autostart
/// starts are only named.
fn relaunch(asked: Relaunch, heard: channel::Sender<Heard>) {
    on_a_thread("bring windows back".to_owned(), move || {
        let locale = Locale::from_env();
        let desktops = [super::DESKTOP.to_owned()];
        let entries = desktop::scan_applications(&asked.applications, &locale, &desktops);
        let starting: Vec<autostart::Entry> = autostart::scan(&asked.autostart, &locale, &desktops)
            .into_iter()
            .filter(autostart::Entry::starts)
            .collect();
        let home = asked
            .home
            .clone()
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/"));
        let gone = asked.folder.as_ref().filter(|f| !f.is_dir()).cloned();
        if let Some(folder) = &gone {
            tracing::info!("a project's folder is gone: its terminals open in the home");
            let _ = heard.send(Heard::FolderGone(folder.clone()));
        }
        for (at, start) in asked.starts.into_iter().enumerate() {
            let id = start.entry.clone();
            let found = entries.iter().find(|e| e.id == id);
            let autostarted =
                start.folder.is_none() && starting.iter().any(|a| starts_app(a, found, &id));
            let name = found.map_or_else(|| id.clone(), |e| e.name.clone());
            let _ = heard.send(Heard::Named {
                at,
                name,
                autostarted,
            });
            if autostarted {
                tracing::info!(app = id, "the autostart starts it");
                continue;
            }
            let Some(entry) = found else {
                tracing::warn!(app = id, "no longer installed");
                let _ = heard.send(Heard::Outcome(at, Outcome::Failed));
                continue;
            };
            let mut launch = hocus_link::Launch {
                id: entry.id.clone(),
                terminal: entry.terminal,
                dir: entry.dir.clone().unwrap_or_default(),
                argv: entry.argv.clone(),
            };
            if let Some(folder) = &start.folder {
                // A folder that went (the project's, or one under it):
                // the home.
                let folder = if folder.is_dir() { folder } else { &home };
                let app = if crate::terminals::is_terminal_app(&start.app_id) {
                    &start.app_id
                } else {
                    &entry.id
                };
                launch.argv = crate::terminals::in_folder(app, &entry.argv, folder);
                launch.dir = folder.display().to_string();
            }
            let term = launch.terminal.then(|| asked.terminal.clone());
            let to = heard.clone();
            programs::launch_watched(launch, term, asked.env.clone(), move |outcome| {
                let _ = to.send(Heard::Outcome(at, outcome));
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
            terminal: None,
            pid: None,
        };
        let terminal = |folder: &str| SavedWindow {
            terminal: Some(SavedTerminal {
                folder: folder.into(),
            }),
            ..window("foot", 1, Shape::Left)
        };
        let saved = Saved {
            windows: vec![
                window("firefox", 1, Shape::Maximized),
                terminal("/home/alex/site"),
                window("slack", 2, Shape::Floating),
                window("firefox", 3, Shape::TopLeft),
                terminal("/home/alex/site/src"),
            ],
            projects: vec![SessionProject {
                id: "hocus-site".into(),
                screen: "DEL U3423WE 7XY1234".into(),
                desktop: 1,
            }],
        };
        let text = saved.text();
        assert!(text.contains("[window.terminal]"), "{text}");
        assert!(text.contains("[[project]]"), "{text}");
        assert_eq!(Saved::parse(&text)?, saved);
        // Each application once, each terminal on its own in its folder.
        let starts: Vec<(String, Option<PathBuf>)> = starts_of(&saved.windows)
            .into_iter()
            .map(|s| (s.entry, s.folder))
            .collect();
        assert_eq!(
            starts,
            [
                ("firefox".to_owned(), None),
                ("foot".to_owned(), Some("/home/alex/site".into())),
                ("slack".to_owned(), None),
                ("foot".to_owned(), Some("/home/alex/site/src".into())),
            ]
        );
        for shape in [Shape::Floating, Shape::Fullscreen, Shape::BottomRight] {
            assert_eq!(Shape::of(shape.mode()), shape);
        }
        Ok(())
    }

    #[test]
    fn those_not_back_are_named_in_one_line() {
        let names = |n: &[&str]| n.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            not_back_said(&names(&["Slack"]), hocus_words::Lang::French),
            "Slack n’a pas pu être rouverte."
        );
        assert_eq!(
            not_back_said(
                &names(&["Slack", "Signal", "Zoom"]),
                hocus_words::Lang::French
            ),
            "Slack, Signal et Zoom n’ont pas pu être rouvertes."
        );
        assert_eq!(
            not_back_said(&names(&["Slack", "Signal"]), hocus_words::Lang::English),
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
