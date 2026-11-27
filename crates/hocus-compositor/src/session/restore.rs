//! The last session's applications (docs/design/windows.md §14, #725):
//! the applications open at log out start again at log in, each window
//! back in its place.
//!
//! - **Saved** in `$XDG_STATE_HOME/hocuspanda/session` (TOML): per window,
//!   its application's desktop entry, its app id, its screen (by identity,
//!   §12), that screen's size and work area, its desktop, where it floats,
//!   its state (floating, a zone, maximised, fullscreen), kept above, on
//!   all desktops, its opacity (#735); a window waiting for its screen
//!   gone, its place and state there, and where it waits (#952); the
//!   windows of an application in the order they opened. A snapshot is taken on the loop (cheap: the
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
//!   waiting, and put back: its screen (a screen not there: where it
//!   waited for it, that screen here, else the same share of the screen
//!   it opened on, the same-numbered desktop; waiting for its screen as
//!   one unplugged, §12), its desktop, place and state.
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
/// How long after a browser reopening its own windows showed its first,
/// those of its saved windows not back are opened from their saved tabs:
/// a browser shows the windows it restores together.
const BROWSER_GRACE: Duration = Duration::from_secs(10);
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
/// goes in tables of its own: `terminal` (lot B), a browser's tabs
/// (`browser`, lot C), an agent's conversation (`agent`, lot D).
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
    /// Where it waited for its screen, gone (§12), as the session ended:
    /// shown there again at log in while its screen is still away, then
    /// back to its screen, in the state it had there (#952).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meanwhile: Option<Shown>,
    /// A terminal's folder (§8.2), which it opens in again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<SavedTerminal>,
    /// A browser window's tabs (§8.3), which it opens again: read as the
    /// session ends for every browser window (one its browser's own
    /// restore leaves out comes back with them), as a project closes for
    /// its windows; only with `[desktops] remember-tabs`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser: Option<crate::browsers::SavedBrowser>,
    /// The conversation of the agent that ran in this terminal (§8.1),
    /// which it resumes: on a project's desktop only. Its terminal is the
    /// agent's then, not saved as a plain terminal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<crate::resume::SavedAgent>,
    /// The herdr session this terminal showed (#887), on a project's
    /// desktop only: it attaches to it again, or, the session gone, its
    /// agents' conversations resume each in a terminal of its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub herdr: Option<crate::resume::SavedHerdr>,
    /// Its client's process, for its thread to read a terminal's folder;
    /// never written.
    #[serde(skip)]
    pub pid: Option<u32>,
    /// Its title and size as it was, for a browser's window to be found
    /// in its file by (§8.3); never written.
    #[serde(skip)]
    pub title: String,
    #[serde(skip)]
    pub size: (i32, i32),
}

/// Where a window is shown: its screen (by identity), that screen's size
/// and work area, its desktop (from 1), where it floats and its state.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Shown {
    pub screen: String,
    pub screen_size: [i32; 2],
    pub area: [i32; 4],
    pub desktop: usize,
    pub place: [i32; 4],
    pub state: Shape,
}

impl Shown {
    /// Where `w` goes on its own screen.
    fn of(w: &SavedWindow) -> Self {
        Self {
            screen: w.screen.clone(),
            screen_size: w.screen_size,
            area: w.area,
            desktop: w.desktop,
            place: w.place,
            state: w.state,
        }
    }
}

/// Where a window of the last session is put back: on its own screen,
/// where it waited for it as the session ended, or elsewhere, its screen
/// and that one away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum At<'a> {
    Home,
    Meanwhile(&'a Shown),
    Elsewhere,
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
/// order of their first window, each terminal with a folder on its own
/// (§8.2), and each browser window whose tabs were read on its own, with
/// its addresses (§8.3).
pub fn starts_of(windows: &[SavedWindow]) -> Vec<Start> {
    let mut out: Vec<Start> = Vec::new();
    for w in windows.iter().filter(|w| !w.entry.is_empty()) {
        let browser =
            crate::browsers::of_app(&w.app_id).or_else(|| crate::browsers::of_app(&w.entry));
        let tabs = w
            .browser
            .as_ref()
            .zip(browser)
            .map(|(saved, browser)| (browser, saved.addresses()))
            .filter(|(_, addresses)| !addresses.is_empty());
        let start = |folder, tabs| Start {
            entry: w.entry.clone(),
            app_id: w.app_id.clone(),
            folder,
            tabs,
            unread: 0,
            agent: None,
            herdr: None,
        };
        if let Some(herdr) = &w.herdr {
            // Its terminal on its own, attaching to its session again.
            let folder = w
                .terminal
                .as_ref()
                .map(|t| t.folder.clone())
                .or_else(|| herdr.agents.first().map(|a| a.folder.clone()));
            out.push(Start {
                herdr: Some(herdr.clone()),
                ..start(folder, None)
            });
            continue;
        }
        if let Some(agent) = &w.agent {
            // An agent's terminal on its own, in its folder, resuming it.
            out.push(Start {
                agent: Some(agent.clone()),
                ..start(Some(agent.folder.clone()), None)
            });
            continue;
        }
        match (&w.terminal, tabs) {
            (Some(terminal), _) => out.push(start(Some(terminal.folder.clone()), None)),
            (None, Some(tabs)) => out.push(start(None, Some(tabs))),
            (None, None) => {
                let at = match out
                    .iter()
                    .position(|s| s.entry == w.entry && s.folder.is_none() && s.tabs.is_none())
                {
                    Some(at) => at,
                    None => {
                        out.push(start(None, None));
                        out.len() - 1
                    }
                };
                // A browser window whose tabs were not read comes back
                // as a new window, said.
                if w.browser.as_ref().is_some_and(|b| !b.read())
                    && let Some(s) = out.get_mut(at)
                {
                    s.unread += 1;
                }
            }
        }
    }
    out
}

/// A program started to bring windows back: its desktop entry, the app id
/// of its windows, the folder a terminal opens in, a browser window's
/// addresses, and how many of its browser's windows come back without
/// their tabs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Start {
    pub entry: String,
    pub app_id: String,
    pub folder: Option<PathBuf>,
    pub tabs: Option<(crate::browsers::Browser, Vec<String>)>,
    pub unread: u32,
    /// The agent whose conversation its terminal resumes (§8.1).
    pub agent: Option<crate::resume::SavedAgent>,
    /// The herdr session its terminal attaches to again (#887).
    pub herdr: Option<crate::resume::SavedHerdr>,
}

/// What did not come back, as the notification says it (projects.md §7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    /// An application that could not start again: its name.
    App(String),
    /// A browser window whose tabs were not read: its browser's name.
    Tabs(String),
    /// An agent's conversation that did not resume: its tool, the folder
    /// its terminal opened in (its last part), why, its program's name
    /// and the command that resumes it (`claude --resume ID`).
    Conversation {
        tool: String,
        folder: String,
        why: crate::resume::NotResumed,
        program: String,
        command: String,
    },
}

impl Missing {
    /// The applications' names alone.
    pub fn apps(missing: &[Self]) -> Vec<String> {
        missing
            .iter()
            .filter_map(|m| match m {
                Self::App(name) => Some(name.clone()),
                Self::Tabs(_) | Self::Conversation { .. } => None,
            })
            .collect()
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
    /// A closed project's windows a browser reopens by itself (#915).
    pub(super) ghosts: super::ghosts::Ghosts,
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
    /// The agents' conversations resuming (§8.1).
    resumes: Vec<Resume>,
    /// The browsers reopening their own windows that showed one, by their
    /// entries: [`BROWSER_GRACE`] later, those not back are opened.
    graces: Vec<(String, Option<RegistrationToken>)>,
    /// Where the restoring threads say what they heard.
    to: Option<channel::Sender<Heard>>,
}

/// An agent's conversation resuming: its session's id, its tool, its
/// folder's last part; whether its hook was heard since.
#[derive(Debug)]
struct Resume {
    session: String,
    tool: String,
    folder: String,
    program: String,
    command: String,
    state: Resuming,
}

impl Resume {
    /// `a`'s conversation, its hook waited for.
    fn of(a: &crate::resume::SavedAgent) -> Self {
        Self {
            session: a.session.clone(),
            tool: a.title().to_owned(),
            program: a.program().unwrap_or_default().to_owned(),
            command: a.command_line().unwrap_or_default(),
            folder: a
                .folder
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| a.folder.display().to_string()),
            state: Resuming::Waiting,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resuming {
    /// Its agent's hook is waited for, [`PATIENCE`] at most.
    Waiting,
    Back,
    /// Not started: its transcript or its program is not there.
    Failed(crate::resume::NotResumed),
}

#[derive(Debug)]
struct Started {
    entry: String,
    name: String,
    back: Back,
    /// Its browser's windows that come back without their tabs.
    unread: u32,
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
    /// Not started: its browser, started once, reopens its own windows.
    ByItself,
}

impl Back {
    fn decided(self) -> bool {
        matches!(self, Self::Windowed | Self::Failed | Self::Autostarted)
    }
}

/// What the saving thread does, in the order asked.
enum Job {
    /// The windows, the projects open, the projects' folders by their
    /// screens and desktops (from 1), and where the browsers' tabs of the
    /// windows on a project's desktop are read (log out only).
    Write(
        Vec<SavedWindow>,
        Vec<SessionProject>,
        Vec<(String, usize, PathBuf)>,
        Option<crate::browsers::Places>,
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
    /// The conversation of session `.0` cannot resume, for `.1`: its
    /// terminal opens in its folder alone.
    NotResumed(String, crate::resume::NotResumed),
    /// Start `.0` is a window its browser reopens itself (§13 D).
    ByItself(usize),
    /// This browser starts once and reopens its own windows (§13 D): a
    /// closed project's among them is closed (#915).
    RestoresItself(crate::browsers::Browser),
    /// Herdr session `.0` is gone (#887): its window's agents resume each
    /// in a terminal of its own.
    HerdrGone(String),
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
        self.ghosts_arm(&windows);
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
        let timer = self.comeback_patience(&whose);
        // At log in, a browser that reopens its own windows does (§13 D);
        // a project reopening always gives the addresses.
        let browsers = (whose == Whose::Session)
            .then(|| self.browser_places())
            .flatten();
        relaunch(
            self.relaunch_of(
                starts.iter().cloned().enumerate().collect(),
                autostart,
                folder,
                browsers,
            ),
            to.clone(),
        );
        let resumes = windows
            .iter()
            .filter_map(|w| w.agent.as_ref())
            .map(Resume::of)
            .collect();
        Comeback {
            whose,
            resumes,
            pending: windows.into(),
            starts: starts
                .iter()
                .map(|s| Started {
                    entry: s.entry.clone(),
                    name: s.entry.clone(),
                    back: Back::Starting,
                    unread: s.unread,
                })
                .collect(),
            told: false,
            folder_gone: None,
            timer,
            heard,
            graces: Vec::new(),
            to: Some(to),
        }
    }

    /// [`PATIENCE`] from now, the end of `whose`'s comeback.
    fn comeback_patience(&self, whose: &Whose) -> Option<RegistrationToken> {
        let of = whose.clone();
        self.loop_handle
            .insert_source(Timer::from_duration(PATIENCE), move |_, _, app| {
                if let Some(c) = app.state.comeback_mut(&of) {
                    c.timer = None;
                }
                app.state.comeback_over(&of);
                TimeoutAction::Drop
            })
            .map_err(|err| tracing::warn!(%err.error, "the restore has no end"))
            .ok()
    }

    /// What [`relaunch`] needs to start `starts` (each with its place in
    /// the comeback's starts).
    fn relaunch_of(
        &self,
        starts: Vec<(usize, Start)>,
        autostart: Vec<PathBuf>,
        folder: Option<PathBuf>,
        browsers: Option<crate::browsers::Places>,
    ) -> Relaunch {
        Relaunch {
            starts,
            applications: self.config.applications.clone(),
            autostart,
            env: Env::of(self),
            terminal: self.config.programs.terminal.clone(),
            folder,
            home: self.config.home.clone(),
            browsers,
            path: self.config.agent_places.as_ref().map_or_else(
                || crate::resume::path_of(std::env::var_os("PATH").as_deref()),
                |p| p.path.clone(),
            ),
            shell: std::env::var("SHELL")
                .ok()
                .filter(|s| s.starts_with('/'))
                .unwrap_or_else(|| "/bin/sh".to_owned()),
            herdr: self.config.agent_places.clone(),
        }
    }

    /// A window of browser `entry`, which reopens its own windows, came
    /// back (§13 D): [`BROWSER_GRACE`] later, its saved windows not back
    /// are opened from their saved tabs. The last session's only.
    fn comeback_browser_showed(&mut self, entry: &str) {
        let Some(c) = self.comeback_mut(&Whose::Session) else {
            return;
        };
        if c.graces.iter().any(|(e, _)| e == entry)
            || !c
                .starts
                .iter()
                .any(|s| s.entry == entry && s.back == Back::ByItself)
        {
            return;
        }
        let of = entry.to_owned();
        let token = self
            .loop_handle
            .insert_source(Timer::from_duration(BROWSER_GRACE), move |_, _, app| {
                app.state.comeback_browser_grace_over(&of);
                TimeoutAction::Drop
            })
            .map_err(|err| tracing::warn!(%err.error, "a browser's windows are not looked for"))
            .ok();
        if let Some(c) = self.comeback_mut(&Whose::Session) {
            c.graces.push((entry.to_owned(), token));
        }
    }

    /// [`BROWSER_GRACE`] after browser `entry` showed a window: its saved
    /// windows still waiting whose tabs were saved open with them, each a
    /// new window, a browser's lost by its own restore (a window it was
    /// asked to close by itself, kept as closed).
    pub(crate) fn comeback_browser_grace_over(&mut self, entry: &str) {
        let Some(c) = self.comeback_mut(&Whose::Session) else {
            return;
        };
        let Some((_, token)) = c.graces.iter_mut().find(|(e, _)| e == entry) else {
            return;
        };
        let token = token.take();
        let first = c.starts.len();
        let missing: Vec<(usize, Start)> = starts_of(
            &c.pending
                .iter()
                .filter(|w| w.entry == entry)
                .cloned()
                .collect::<Vec<_>>(),
        )
        .into_iter()
        .filter(|s| s.tabs.is_some())
        .enumerate()
        .map(|(i, s)| (first + i, s))
        .collect();
        let to = c.to.clone();
        if let Some(token) = token {
            self.loop_handle.remove(token);
        }
        let Some(to) = to.filter(|_| !missing.is_empty()) else {
            return;
        };
        tracing::info!(
            app = entry,
            windows = missing.len(),
            "windows a browser did not reopen come back with their saved tabs"
        );
        let timer = self.comeback_patience(&Whose::Session);
        let asked = self.relaunch_of(missing.clone(), Vec::new(), None, None);
        let Some(c) = self.comeback_mut(&Whose::Session) else {
            return;
        };
        c.starts.extend(missing.iter().map(|(_, s)| Started {
            entry: s.entry.clone(),
            name: s.entry.clone(),
            back: Back::Starting,
            unread: 0,
        }));
        // Their patience from now.
        let old = std::mem::replace(&mut c.timer, timer);
        if let Some(old) = old {
            self.loop_handle.remove(old);
        }
        relaunch(asked, to);
    }

    /// The browsers' graces are over now (the tests').
    pub(crate) fn comeback_browser_graces_over(&mut self) {
        let entries: Vec<String> = self
            .comeback_mut(&Whose::Session)
            .map(|c| c.graces.iter().map(|(e, _)| e.clone()).collect())
            .unwrap_or_default();
        for entry in entries {
            self.comeback_browser_grace_over(&entry);
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
        if let Heard::RestoresItself(browser) = heard {
            if *whose == Whose::Session {
                self.ghosts_watch(browser);
            }
            return;
        }
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
            Heard::NotResumed(session, why) => {
                tracing::info!(?why, "an agent's conversation cannot resume");
                for r in c.resumes.iter_mut().filter(|r| r.session == session) {
                    r.state = Resuming::Failed(why);
                }
            }
            Heard::HerdrGone(session) => {
                // Its window's place, once for each of its agents, each
                // with its conversation; without any, a plain terminal.
                let at = c
                    .pending
                    .iter()
                    .position(|w| w.herdr.as_ref().is_some_and(|h| h.session == session));
                if let Some(window) = at.and_then(|at| c.pending.remove(at)) {
                    let agents = window
                        .herdr
                        .as_ref()
                        .map(|h| h.agents.clone())
                        .unwrap_or_default();
                    tracing::info!(
                        agents = agents.len(),
                        "a herdr session is gone: its agents resume each in a terminal"
                    );
                    let at = at.unwrap_or(0);
                    if agents.is_empty() {
                        c.pending.insert(
                            at,
                            SavedWindow {
                                herdr: None,
                                ..window.clone()
                            },
                        );
                    }
                    for (k, agent) in agents.iter().enumerate() {
                        c.resumes.push(Resume::of(agent));
                        c.pending.insert(
                            at + k,
                            SavedWindow {
                                herdr: None,
                                terminal: None,
                                agent: Some(agent.clone()),
                                ..window.clone()
                            },
                        );
                    }
                }
            }
            Heard::ByItself(at) => {
                if let Some(start) = c.starts.get_mut(at)
                    && !start.back.decided()
                {
                    start.back = Back::ByItself;
                }
            }
            Heard::RestoresItself(_) => {}
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
        // A closed project's window its browser reopened by itself takes
        // no place: it goes (#915).
        if self.ghost_closed(id) {
            self.session_changed();
            return;
        }
        if let Some(app) = app_id(window) {
            let mut taken = None;
            let mut waiting = false;
            // A browser's window takes the place whose shown tab it shows,
            // when it says one already (a browser reopening its own, §13
            // D); else the first of its application's.
            let title = match crate::browsers::shown(&crate::windows::title_of(self, id)) {
                crate::browsers::Shown::Page(t) if !t.is_empty() => Some(t),
                _ => None,
            };
            let restore = &mut self.session_restore;
            let comebacks = match &mut restore.phase {
                Phase::Restoring(c) => Some(c),
                _ => None,
            }
            .into_iter()
            .chain(restore.projects.iter_mut());
            for c in comebacks {
                waiting = true;
                let titled = title.as_deref().and_then(|t| {
                    c.pending.iter().position(|w| {
                        w.app_id == app
                            && w.browser.as_ref().and_then(|b| b.shown_title()) == Some(t)
                    })
                });
                let by = if titled.is_some() {
                    "its tab"
                } else {
                    "its app"
                };
                if let Some(at) = titled
                    .or_else(|| c.pending.iter().position(|w| w.app_id == app))
                    .or_else(|| {
                        c.pending
                            .iter()
                            .position(|w| w.app_id.eq_ignore_ascii_case(&app))
                    })
                    && let Some(saved) = c.pending.remove(at)
                {
                    // The places of its application it could have taken
                    // as well: more than none, and which is a guess.
                    let alike = c
                        .pending
                        .iter()
                        .filter(|w| w.app_id.eq_ignore_ascii_case(&app))
                        .count();
                    tracing::info!(
                        window = id.0,
                        app,
                        by,
                        alike,
                        "a window takes a saved place"
                    );
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
            if taken.is_none() && waiting {
                tracing::info!(
                    window = id.0,
                    app,
                    "a window takes no saved place: none of its app"
                );
            }
            if let Some((whose, saved)) = taken {
                self.ghosts_placed(id);
                self.session_put_back(id, &saved);
                if whose == Whose::Session {
                    self.comeback_browser_showed(&saved.entry);
                }
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

    /// Puts window `id` where `saved` says: on its screen; that screen
    /// away, where it waited for it as the session ended when that screen
    /// is here (#952), else the same share of the screen it opened on,
    /// waiting for its own all the same.
    fn session_put_back(&mut self, id: WindowId, saved: &SavedWindow) {
        let screen = |key: &str| {
            self.connector_of(key)
                .and_then(|name| self.space.outputs().find(|o| o.name() == name).cloned())
        };
        let home = Shown::of(saved);
        let (output, at) = match screen(&saved.screen) {
            Some(output) => (output, At::Home),
            None => match saved
                .meanwhile
                .as_ref()
                .and_then(|m| Some((screen(&m.screen)?, m)))
            {
                Some((output, meanwhile)) => (output, At::Meanwhile(meanwhile)),
                None => {
                    let Some(output) = self.output_of(id) else {
                        return;
                    };
                    (output, At::Elsewhere)
                }
            },
        };
        let (spot, found) = match at {
            At::Home => (&home, true),
            At::Meanwhile(m) => (m, true),
            At::Elsewhere => (&home, false),
        };
        let gone = at != At::Home;
        let (Some(geometry), Some(area)) = (
            self.space.output_geometry(&output).map(rect_of),
            self.work_area(&output),
        ) else {
            return;
        };
        let mode = spot.state.mode();
        let desktop = spot.desktop.saturating_sub(1);
        let place = rect(spot.place);
        // A file without its screen's work area (written by hand): the
        // screen as it is.
        let known = spot.area[2] > 0 && spot.area[3] > 0;
        let saved_area = if known {
            rect(spot.area)
        } else {
            Rect::new(0, 0, geometry.width, geometry.height)
        };
        // The same place on a screen of the same size, the same share of
        // another (windows.md §12).
        let same = spot.screen_size == [geometry.width, geometry.height] || !known;
        let floating = if found && same {
            shifted(place, geometry, 1)
        } else {
            model::carried(place, saved_area, area)
        };
        let placed = match mode {
            Mode::Floating => floating,
            Mode::Snapped(zone) => zone.rect(area, self.gap()),
            Mode::Fullscreen => geometry,
        };
        let restore = (mode != Mode::Floating).then_some(floating);
        tracing::info!(
            window = id.0,
            app = saved.app_id,
            screen = saved.screen,
            gone,
            meanwhile = matches!(at, At::Meanwhile(_)),
            desktop = desktop + 1,
            ?mode,
            ?placed,
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
            // Its own screen's place, to go back to.
            let place = rect(home.place);
            let mode = home.state.mode();
            let came = CameFrom {
                output: saved.screen.clone(),
                desktop: home.desktop.saturating_sub(1),
                size: (home.screen_size[0], home.screen_size[1]),
                area: if home.area[2] > 0 && home.area[3] > 0 {
                    rect(home.area)
                } else {
                    Rect::new(0, 0, geometry.width, geometry.height)
                },
                rect: place,
                restore: (mode != Mode::Floating).then_some(place),
                mode,
                put: Some(put),
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
        let waiting = waiting_entries(c);
        if !c.told
            && c.starts
                .iter()
                .all(|a| a.back.decided() || all_back(c, &a.entry, &waiting))
            && c.resumes.iter().all(|r| r.state != Resuming::Waiting)
        {
            c.told = true;
            say = Some(failed_names(c, &waiting, |b| b == Back::Failed));
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
            for w in &c.pending {
                tracing::info!(
                    app = w.app_id,
                    screen = w.screen,
                    desktop = w.desktop,
                    state = ?w.state,
                    tabs = w.browser.is_some(),
                    "a place not taken"
                );
            }
        }
        let waiting = waiting_entries(c);
        c.pending.clear();
        for r in c
            .resumes
            .iter_mut()
            .filter(|r| r.state == Resuming::Waiting)
        {
            tracing::info!("an agent's conversation was not heard resuming");
            r.state = Resuming::Failed(crate::resume::NotResumed::Other);
        }
        let say = (!c.told).then(|| {
            failed_names(c, &waiting, |b| {
                !matches!(b, Back::Windowed | Back::Autostarted)
            })
        });
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
        let graces = comeback.graces.into_iter().filter_map(|(_, t)| t);
        for token in [comeback.timer, comeback.heard]
            .into_iter()
            .flatten()
            .chain(graces)
        {
            self.loop_handle.remove(token);
        }
        match whose {
            Whose::Session => {
                tracing::info!("the last session is back");
                self.ghosts_disarm();
            }
            Whose::Project(id) => self.project_back(id),
        }
        self.session_changed();
    }

    /// What did not come back for `whose`, `missing`; `gone`, a project's
    /// folder that is not there: one notification.
    fn comeback_report(&mut self, whose: &Whose, missing: Vec<Missing>, gone: Option<PathBuf>) {
        match whose {
            Whose::Session => self.session_report(&missing),
            Whose::Project(id) => self.project_not_back(id, missing, gone),
        }
    }

    /// What did not come back of the last session: one notification.
    fn session_report(&mut self, missing: &[Missing]) {
        let names = Missing::apps(missing);
        self.session_restore.not_back.clone_from(&names);
        if missing.is_empty() {
            return;
        }
        tracing::info!(
            ?names,
            others = missing.len() - names.len(),
            "applications of the last session did not come back"
        );
        let Some(bus) = self.shell.bus.clone() else {
            return;
        };
        let lang = self.config.lang;
        let mut said = Vec::new();
        if !names.is_empty() {
            said.push(not_back_said(&names, lang));
        }
        said.extend(missing_said(missing, lang));
        crate::shell::notify(bus, "dialog-warning", said.join(" "));
    }

    /// The applications that did not come back, as last said.
    pub(crate) fn session_not_back(&self) -> &[String] {
        &self.session_restore.not_back
    }

    /// An agent of session `session` was heard (§8.1): its conversation,
    /// if one was resuming, came back.
    pub(crate) fn comeback_agent_heard(&mut self, session: &str) {
        let restore = &mut self.session_restore;
        let mut heard = Vec::new();
        let comebacks = match &mut restore.phase {
            Phase::Restoring(c) => Some(c),
            _ => None,
        }
        .into_iter()
        .chain(restore.projects.iter_mut());
        for c in comebacks {
            for r in c
                .resumes
                .iter_mut()
                .filter(|r| r.session == session && r.state == Resuming::Waiting)
            {
                r.state = Resuming::Back;
                heard.push(c.whose.clone());
            }
        }
        for whose in heard {
            tracing::info!(?whose, "an agent's conversation resumed");
            self.comeback_check(&whose);
        }
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
        // At the session's end, the agents on a project's desktop with
        // their conversations (§8, §9).
        let snapshot = (self.session_snapshot(again), self.session_projects());
        if !again && self.session_restore.last.as_ref() == Some(&snapshot) {
            return;
        }
        if again {
            // What the next log in puts back, to compare with what it did.
            for w in &snapshot.0 {
                tracing::info!(
                    app = w.app_id,
                    screen = w.screen,
                    desktop = w.desktop,
                    state = ?w.state,
                    place = ?w.place,
                    "a window saved as the session ends"
                );
            }
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
        // The browsers' tabs at the session's end only (§8).
        let browsers = again.then(|| self.browser_places()).flatten();
        if saver
            .send(Job::Write(
                snapshot.0.clone(),
                snapshot.1.clone(),
                folders,
                browsers,
            ))
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
    fn session_snapshot(&self, agents: bool) -> Vec<SavedWindow> {
        let mut out = Vec::new();
        let (conversations, herdr) = if agents {
            (self.agent_conversations(), self.herdr_session_windows())
        } else {
            (Vec::new(), Vec::new())
        };
        for id in self.windows.keys() {
            if let Some(saved) = self.session_window(*id) {
                if self.project_of(*id).is_some() {
                    out.extend(with_agents(*id, saved, &conversations, &herdr));
                } else {
                    out.push(saved);
                }
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
        let above = self.model.is_above(id);
        let everywhere = self.model.is_everywhere(id);
        let opacity = Some(managed.opacity).filter(|p| *p < crate::opacity::FULL);
        let shown = self.shown(id);
        // Waiting for its screen gone: where it was there, in the state it
        // had there, which it goes back to (#952, decided 2026-11-25); and
        // where it waits, which it is shown in again meanwhile.
        let (home, meanwhile) = match managed
            .came_from
            .as_ref()
            .filter(|c| self.still_put_where(id, c))
        {
            Some(came) => {
                let place = match came.mode {
                    Mode::Floating => came.rect,
                    _ => came.restore.unwrap_or(came.rect),
                };
                let home = Shown {
                    screen: came.output.clone(),
                    screen_size: [came.size.0, came.size.1],
                    area: array(came.area),
                    desktop: came.desktop + 1,
                    place: array(place),
                    state: Shape::of(came.mode),
                };
                (home, shown)
            }
            None => (shown?, None),
        };
        Some(SavedWindow {
            entry: String::new(),
            app_id: app,
            screen: home.screen,
            screen_size: home.screen_size,
            area: home.area,
            desktop: home.desktop,
            place: home.place,
            state: home.state,
            above,
            everywhere,
            opacity,
            meanwhile,
            terminal: None,
            browser: None,
            agent: None,
            herdr: None,
            pid: self.pid_of(&managed.window),
            title: crate::windows::title_of(self, id),
            size: self.home_rect(id).map_or((0, 0), |r| (r.width, r.height)),
        })
    }

    /// Where window `id` is shown now, as saved.
    fn shown(&self, id: WindowId) -> Option<Shown> {
        let output = self.output_of(id)?;
        let geometry = rect_of(self.space.output_geometry(&output)?);
        let area = self.work_area(&output)?;
        let mode = self.model.mode(id);
        let place = match mode {
            Mode::Floating => self.target_rect(id)?,
            _ => self
                .model
                .placement(id)
                .restore
                .or_else(|| self.home_rect(id))?,
        };
        Some(Shown {
            screen: self.screen_key(&output.name()),
            screen_size: [geometry.width, geometry.height],
            area: array(shifted(area, geometry, -1)),
            desktop: self.model.desktop(id) + 1,
            place: array(shifted(place, geometry, -1)),
            state: Shape::of(mode),
        })
    }
}

/// Window `id` as saved, `saved`: a herdr session's terminal (`herdr`, the
/// sessions' windows) once, with that session and the conversations of
/// its panes' agents (#887); else as many times as agents ran in it
/// (`conversations`), each with its conversation: the agent's terminal,
/// not a plain one.
pub(crate) fn with_agents(
    id: WindowId,
    saved: SavedWindow,
    conversations: &[crate::agents::Conversation],
    herdr: &[(WindowId, String)],
) -> Vec<SavedWindow> {
    if let Some((_, session)) = herdr.iter().find(|(w, _)| *w == id) {
        let agents = conversations
            .iter()
            .filter(|c| c.window == id && c.herdr.as_ref() == Some(session))
            .map(|c| c.agent.clone())
            .collect();
        return vec![SavedWindow {
            herdr: Some(crate::resume::SavedHerdr {
                session: session.clone(),
                agents,
            }),
            agent: None,
            ..saved
        }];
    }
    let theirs: Vec<&crate::resume::SavedAgent> = conversations
        .iter()
        .filter(|c| c.window == id)
        .map(|c| &c.agent)
        .collect();
    if theirs.is_empty() {
        return vec![saved];
    }
    theirs
        .into_iter()
        .map(|agent| SavedWindow {
            agent: Some(agent.clone()),
            terminal: None,
            ..saved.clone()
        })
        .collect()
}

/// The entries of `c`'s windows still waiting.
fn waiting_entries(c: &Comeback) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in &c.pending {
        if !out.contains(&w.entry) {
            out.push(w.entry.clone());
        }
    }
    out
}

/// Whether every window of application `entry` came back (`waiting`, the
/// entries of the windows still waiting): one did, none waits. A start of
/// it whose own window never came (a browser's reopening its own, one
/// started for a window it did not reopen) is not one that failed then.
fn all_back(c: &Comeback, entry: &str, waiting: &[String]) -> bool {
    !waiting.iter().any(|e| e == entry)
        && c.starts
            .iter()
            .any(|s| s.entry == entry && s.back == Back::Windowed)
}

/// What of `c` did not come back: its starts whose state `failed` says
/// did not, unless all their application's windows came back (`waiting`,
/// the entries still waiting), each once; its browsers' windows whose
/// tabs were not read.
fn failed_names(c: &Comeback, waiting: &[String], failed: impl Fn(Back) -> bool) -> Vec<Missing> {
    let mut out: Vec<Missing> = Vec::new();
    for s in c
        .starts
        .iter()
        .filter(|s| failed(s.back) && !all_back(c, &s.entry, waiting))
    {
        let app = Missing::App(s.name.clone());
        if !out.contains(&app) {
            out.push(app);
        }
    }
    for s in c
        .starts
        .iter()
        .filter(|s| !failed(s.back) || all_back(c, &s.entry, waiting))
    {
        for _ in 0..s.unread {
            out.push(Missing::Tabs(s.name.clone()));
        }
    }
    for r in &c.resumes {
        if let Resuming::Failed(why) = r.state {
            out.push(Missing::Conversation {
                tool: r.tool.clone(),
                folder: r.folder.clone(),
                why,
                program: r.program.clone(),
                command: r.command.clone(),
            });
        }
    }
    out
}

/// The sentences of what came back but not whole: a browser window
/// without its tabs ("Les onglets d'une fenêtre de Firefox n'ont pas pu
/// être lus."), an agent's conversation that did not resume ("La
/// conversation de Claude Code n'a pas pu reprendre. Son terminal est
/// ouvert dans hocus-site.").
pub fn missing_said(missing: &[Missing], lang: hocus_words::Lang) -> Vec<String> {
    missing
        .iter()
        .filter_map(|m| match m {
            Missing::App(_) => None,
            Missing::Tabs(browser) => Some(txt::tabs_unread(lang, browser)),
            Missing::Conversation {
                tool,
                folder,
                why,
                program,
                command,
            } => Some(match why {
                crate::resume::NotResumed::Transcript => {
                    txt::conversation_gone(lang, txt::ConversationGone { tool, folder })
                }
                crate::resume::NotResumed::Program if !command.is_empty() => {
                    txt::conversation_no_program(
                        lang,
                        txt::ConversationNoProgram {
                            tool,
                            program,
                            folder,
                            command,
                        },
                    )
                }
                _ => txt::conversation_not_resumed(
                    lang,
                    txt::ConversationNotResumed { tool, folder },
                ),
            }),
        })
        .collect()
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
    let mut reader = crate::browsers::Reader::default();
    let mut written: Option<String> = None;
    while let Ok(job) = jobs.recv() {
        let (mut windows, projects, folders, browsers) = match job {
            Job::Flush(done) => {
                let _ = done.send(());
                continue;
            }
            Job::Write(windows, projects, folders, browsers) => {
                (windows, projects, folders, browsers)
            }
        };
        // The session ends: the tabs of every browser window (§8.3), those
        // still coming back keeping theirs. A browser that reopens its
        // own windows may not reopen them all: those it does not come
        // back with these.
        if let Some(places) = &browsers {
            let asked: Vec<(usize, crate::browsers::Asked)> = windows
                .iter()
                .enumerate()
                .filter(|(_, w)| w.browser.is_none())
                .filter_map(|(i, w)| {
                    let browser = crate::browsers::of_app(&w.app_id)?;
                    Some((
                        i,
                        crate::browsers::Asked {
                            browser,
                            title: w.title.clone(),
                            size: w.size,
                            order: u64::try_from(i).unwrap_or(u64::MAX),
                        },
                    ))
                })
                .collect();
            let only: Vec<crate::browsers::Asked> = asked.iter().map(|(_, a)| a.clone()).collect();
            let read = reader.read(places, &only, &[]);
            for ((i, _), found) in asked.iter().zip(read) {
                if let Some(w) = windows.get_mut(*i) {
                    w.browser = found;
                }
            }
        }
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
            // Every terminal comes back in its folder (§13, C); an agent's
            // in its agent's.
            if window.terminal.is_none()
                && window.agent.is_none()
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
    use std::{io::Write as _, os::unix::fs::OpenOptionsExt as _};
    std::fs::create_dir_all(dir)?;
    let partial = dir.join(format!("{FILE}.new"));
    // The user's alone (`0600`): it holds the browsers' addresses.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&partial)?;
    file.write_all(text.as_bytes())?;
    drop(file);
    std::fs::set_permissions(
        &partial,
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    )?;
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
    /// Each with its place in its comeback's starts.
    starts: Vec<(usize, Start)>,
    applications: Vec<PathBuf>,
    /// The start-up applications' directories: those they start are only
    /// named (the last session's only).
    autostart: Vec<PathBuf>,
    env: Env,
    terminal: String,
    /// A project's folder, looked at: gone, its terminals open in `home`.
    folder: Option<PathBuf>,
    home: Option<PathBuf>,
    /// At log in, where a browser's preferences say whether it reopens
    /// its own windows (§13 D): then it starts once, without addresses.
    browsers: Option<crate::browsers::Places>,
    /// The `PATH`'s folders, where an agent's program is looked for, and
    /// the user's shell its command runs through (§8.1).
    path: Vec<PathBuf>,
    shell: String,
    /// Where herdr's sessions' sockets are (#887): a session whose server
    /// answers is attached to again.
    herdr: Option<hocus_agents::tools::Places>,
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
        // The entries started without addresses: a browser reopening its
        // own windows starts once.
        let mut plain: Vec<String> = asked
            .starts
            .iter()
            .filter(|(_, s)| s.folder.is_none() && s.tabs.is_none())
            .map(|(_, s)| s.entry.clone())
            .collect();
        for (at, start) in asked.starts {
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
            let term = launch.terminal.then(|| asked.terminal.clone());
            let watched = |launch: hocus_link::Launch| {
                let to = heard.clone();
                programs::launch_watched(launch, term.clone(), asked.env.clone(), move |outcome| {
                    let _ = to.send(Heard::Outcome(at, outcome));
                });
            };
            let app = if crate::terminals::is_terminal_app(&start.app_id) {
                start.app_id.as_str()
            } else {
                entry.id.as_str()
            };
            if let Some(herdr) = &start.herdr {
                // A herdr session's terminal (#887): attached to it again
                // while its server runs, else its agents each in a
                // terminal of its own resuming its conversation.
                let in_folder = |folder: &Path| {
                    let folder = if folder.is_dir() { folder } else { &home };
                    hocus_link::Launch {
                        argv: crate::terminals::in_folder(app, &entry.argv, folder),
                        dir: folder.display().to_string(),
                        ..launch.clone()
                    }
                };
                let folder = start.folder.clone().unwrap_or_else(|| home.clone());
                let attach = asked
                    .herdr
                    .as_ref()
                    .filter(|places| {
                        std::os::unix::net::UnixStream::connect(
                            places.herdr_socket_of(&herdr.session),
                        )
                        .is_ok()
                    })
                    .and_then(|_| crate::resume::program_found("herdr", &asked.path, Some(&home)))
                    .and_then(|found| herdr.command(&asked.shell, &found))
                    .and_then(|command| crate::terminals::with_command(app, &command));
                if let Some(command) = attach {
                    tracing::info!(app = id, "a herdr session's terminal attaches to it again");
                    let mut launch = in_folder(&folder);
                    launch.argv.extend(command);
                    watched(launch);
                    continue;
                }
                let _ = heard.send(Heard::HerdrGone(herdr.session.clone()));
                if herdr.agents.is_empty() {
                    watched(in_folder(&folder));
                }
                for agent in &herdr.agents {
                    let mut launch = in_folder(&agent.folder);
                    match agent_command(
                        agent,
                        &agent.folder,
                        app,
                        (&asked.path, &asked.shell),
                        &home,
                    ) {
                        Ok(command) => {
                            tracing::info!(
                                app = id,
                                "an agent's terminal resumes its conversation"
                            );
                            launch.argv.extend(command);
                        }
                        Err(why) => {
                            let _ = heard.send(Heard::NotResumed(agent.session.clone(), why));
                        }
                    }
                    watched(launch);
                }
                continue;
            }
            if let Some(folder) = &start.folder {
                // A folder that went (the project's, or one under it):
                // the home.
                let folder = if folder.is_dir() { folder } else { &home };
                launch.argv = crate::terminals::in_folder(app, &entry.argv, folder);
                launch.dir = folder.display().to_string();
                if let Some(agent) = &start.agent {
                    match agent_command(agent, folder, app, (&asked.path, &asked.shell), &home) {
                        Ok(command) => {
                            tracing::info!(
                                app = id,
                                "an agent's terminal resumes its conversation"
                            );
                            launch.argv.extend(command);
                        }
                        Err(why) => {
                            let _ = heard.send(Heard::NotResumed(agent.session.clone(), why));
                        }
                    }
                }
            }
            if let Some((browser, addresses)) = &start.tabs {
                let itself = asked
                    .browsers
                    .as_ref()
                    .is_some_and(|places| crate::browsers::restores_itself(*browser, places));
                if itself {
                    if plain.contains(&id) {
                        // Started already: its windows come with it.
                        let _ = heard.send(Heard::ByItself(at));
                        continue;
                    }
                    tracing::info!(app = id, "a browser reopens its own windows");
                    let _ = heard.send(Heard::RestoresItself(*browser));
                    plain.push(id.clone());
                } else {
                    tracing::info!(
                        app = id,
                        tabs = addresses.len(),
                        "a browser window comes back with its tabs"
                    );
                    launch
                        .argv
                        .extend(crate::browsers::new_window_args(browser.kind, addresses));
                }
            }
            watched(launch);
        }
    });
}

/// What `agent`'s terminal in `folder` (a terminal of `app`) runs after its
/// own arguments to resume its conversation, or why it cannot: its folder,
/// its transcript or its program gone, or its terminal not one that can be
/// given a command. Its program is looked for where the user's shell finds
/// it too: a display manager's `PATH` may lack it.
fn agent_command(
    agent: &crate::resume::SavedAgent,
    folder: &Path,
    app: &str,
    (path, shell): (&[PathBuf], &str),
    home: &Path,
) -> Result<Vec<String>, crate::resume::NotResumed> {
    if folder != agent.folder {
        return Err(crate::resume::NotResumed::Other);
    }
    agent.resumable(path, Some(home)).and_then(|found| {
        agent
            .command(shell, &found)
            .and_then(|c| crate::terminals::with_command(app, &c))
            .ok_or(crate::resume::NotResumed::Other)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A herdr session's terminal (#887) reads back with its session and
    /// its agents, and starts once, in its folder, attaching to it.
    #[test]
    fn a_herdr_sessions_terminal_reads_back_and_starts_once() -> Result<(), String> {
        let agent = |id: &str| crate::resume::SavedAgent {
            tool: "claude-code".into(),
            session: id.into(),
            folder: "/home/alex/site".into(),
            transcript: format!("/home/alex/.claude/projects/x/{id}.jsonl").into(),
            ..crate::resume::SavedAgent::default()
        };
        let saved = Saved {
            windows: vec![SavedWindow {
                entry: "foot".into(),
                app_id: "foot".into(),
                desktop: 1,
                terminal: Some(SavedTerminal {
                    folder: "/home/alex/site".into(),
                }),
                herdr: Some(crate::resume::SavedHerdr {
                    session: "site".into(),
                    agents: vec![agent("a-1"), agent("b-2")],
                }),
                ..SavedWindow::default()
            }],
            projects: Vec::new(),
        };
        let text = saved.text();
        assert!(text.contains("[window.herdr]"), "{text}");
        assert!(text.contains("[[window.herdr.agents]]"), "{text}");
        let read = Saved::parse(&text)?;
        assert_eq!(read.windows, saved.windows);
        let starts = starts_of(&read.windows);
        assert_eq!(starts.len(), 1, "{starts:?}");
        assert_eq!(starts[0].folder, Some("/home/alex/site".into()));
        assert_eq!(
            starts[0].herdr.as_ref().map(|h| h.session.as_str()),
            Some("site")
        );
        assert_eq!(
            starts[0]
                .herdr
                .as_ref()
                .and_then(crate::resume::SavedHerdr::attach_line),
            Some("herdr session attach site".to_owned())
        );
        Ok(())
    }

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
            // Waiting for its screen on the laptop's, maximised there.
            meanwhile: (desktop == 2).then(|| Shown {
                screen: "BOE NE160QDM 0".into(),
                screen_size: [2560, 1600],
                area: [0, 0, 2560, 1600],
                desktop: 2,
                place: [10, 10, 800, 600],
                state: Shape::Maximized,
            }),
            terminal: None,
            browser: None,
            agent: None,
            herdr: None,
            pid: None,
            title: String::new(),
            size: (0, 0),
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
        assert!(text.contains("[window.meanwhile]"), "{text}");
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
    fn a_browser_window_and_an_agents_terminal_each_start_on_their_own() {
        let firefox = |tabs: &[&str]| SavedWindow {
            entry: "firefox".into(),
            app_id: "firefox".into(),
            browser: Some(crate::browsers::SavedBrowser {
                kind: crate::browsers::Kind::Firefox,
                selected: 1,
                tabs: tabs
                    .iter()
                    .map(|u| crate::browsers::Tab {
                        url: (*u).into(),
                        ..crate::browsers::Tab::default()
                    })
                    .collect(),
            }),
            ..SavedWindow::default()
        };
        let agent = SavedWindow {
            entry: "kitty".into(),
            app_id: "kitty".into(),
            agent: Some(crate::resume::SavedAgent {
                tool: "claude-code".into(),
                session: "3f6c".into(),
                folder: "/home/alex/site".into(),
                ..crate::resume::SavedAgent::default()
            }),
            ..SavedWindow::default()
        };
        let windows = [
            firefox(&["https://a/", "https://b/"]),
            firefox(&[]),
            firefox(&["https://c/"]),
            agent,
        ];
        let starts = starts_of(&windows);
        let said: Vec<(&str, Option<Vec<String>>, u32, bool)> = starts
            .iter()
            .map(|s| {
                (
                    s.entry.as_str(),
                    s.tabs.as_ref().map(|(_, a)| a.clone()),
                    s.unread,
                    s.agent.is_some(),
                )
            })
            .collect();
        let urls = |u: &[&str]| Some(u.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
        assert_eq!(
            said,
            [
                ("firefox", urls(&["https://a/", "https://b/"]), 0, false),
                ("firefox", None, 1, false),
                ("firefox", urls(&["https://c/"]), 0, false),
                ("kitty", None, 0, true),
            ]
        );
        assert_eq!(
            starts[3].folder.as_deref(),
            Some(Path::new("/home/alex/site"))
        );
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
