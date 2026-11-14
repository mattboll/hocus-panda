//! Project desktops (docs/design/projects.md, #789): a desktop of a screen
//! given a name and a folder. Lot A: the model and its file, the desktop's
//! name everywhere it is said, and what the overview's form and card ask.
//!
//! - **The model.** A project is open on one desktop of one screen (by the
//!   screen's identity, windows.md §12, so that it finds its screen again
//!   on another connector); everything on that desktop belongs to it, a
//!   window on all desktops never does. Switching desktops changes
//!   nothing: its windows go on running.
//! - **Made** by the overview's form (`project-make`, from the shell) or by
//!   the Settings' line (`project-make SCREEN DESKTOP NAME FOLDER`, the
//!   tests): the name 1 to 40 characters and free (case and accents
//!   ignored), the desktop not one already, the folder an absolute path to
//!   a directory, which a thread of its own checks (a stat is I/O, never
//!   done on the loop).
//! - **Kept** in `$XDG_STATE_HOME/hocuspanda/projects` (TOML), written by
//!   that thread through a temporary file, the directory `0700` and the
//!   file `0600`; read once as the compositor starts.
//! - **Its name is the desktop's** while it is open: the bar's tiles and
//!   list (told on the `desktops` line), Alt+Tab's suffix, the overview's
//!   label, the radial menu, the agents' `list_desktops`.

use std::{
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::mpsc,
};

use hocus_link::{DeskProject, NameFault, ProjectRefusal, ToShell, WindowPlace, projects as link};
use serde::{Deserialize, Serialize};
use smithay::{output::Output, reexports::calloop::channel};

use crate::{
    App, State,
    model::{Mode, WindowId, Zone},
};

/// The state file the projects are kept in.
pub const FILE: &str = "projects";

/// The folder of the closed projects' contents (§9): `ID.toml` each.
pub const CONTENTS: &str = "projects.d";

/// The projects as kept.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Kept {
    #[serde(rename = "project")]
    pub projects: Vec<Project>,
}

impl Kept {
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    pub fn text(&self) -> String {
        toml::to_string(self).unwrap_or_default()
    }
}

/// A project (§9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Project {
    /// Its file's name in `projects.d` (§9), kept when the name changes.
    pub id: String,
    pub name: String,
    pub folder: PathBuf,
    /// When it was made, opened and closed: RFC 3339, local time (the
    /// user reads them).
    pub made: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opened: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<String>,
    /// Where it is open; none while it is closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<OpenOn>,
    /// Its screen unplugged, it followed its windows to another (§1): the
    /// screen it goes back to when that one comes back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub away_from: Option<String>,
    /// Closed, what comes back when it reopens: its `projects.d` file,
    /// read as the compositor starts and kept here.
    #[serde(skip)]
    pub contents: Vec<crate::session::restore::SavedWindow>,
}

/// A closed project's contents as its file keeps them (§9).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Contents {
    #[serde(rename = "window")]
    pub windows: Vec<crate::session::restore::SavedWindow>,
}

impl Contents {
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    pub fn text(&self) -> String {
        let body = toml::to_string(self).unwrap_or_default();
        format!(
            "# A closed project's windows, which come back when it reopens\n\
             # (docs/design/projects.md §9). Written by Hocuspanda.\n{body}"
        )
    }
}

/// The desktop a project is: its screen's identity and the desktop's
/// number, from 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenOn {
    pub screen: String,
    pub desktop: usize,
}

/// Who asked for a project: the shell's form, a Settings' line (its
/// session), which is answered, or the launcher's folder row, after which
/// its desktop shows, a terminal opening in its folder when it is empty
/// (§13, B).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asker {
    Shell,
    Line(u64),
    Launcher { empty: bool },
}

/// A project asked for, waiting for its folder to be looked at.
#[derive(Debug, Clone)]
struct Asked {
    serial: u64,
    by: Asker,
    output: String,
    desktop: usize,
    name: String,
    folder: PathBuf,
}

/// What the projects' thread is asked.
pub(crate) enum Job {
    /// Whether `folder` is a directory, for project `serial`.
    Check { serial: u64, folder: PathBuf },
    /// The file, as it now says.
    Write(String),
    /// What `windows` are (§8): their desktop entries, the terminals'
    /// folders and programs, for `serial`, in the project of folder
    /// `within`; and, `browsers` given, the browsers' tabs (§8.3).
    Read {
        serial: u64,
        windows: Vec<Seen>,
        within: PathBuf,
        browsers: Option<crate::browsers::Places>,
    },
    /// The browsers' windows of a project closed, their tabs read again
    /// once they are gone (§5 step 3), for `serial`.
    Tabs {
        serial: u64,
        windows: Vec<(WindowId, crate::browsers::Asked)>,
        browsers: crate::browsers::Places,
    },
    /// Project `id`'s contents' file written, or taken out (none).
    Contents { id: String, text: Option<String> },
    /// An agent's hook named this folder (§2.3): `folders` says so now.
    Folder(PathBuf),
}

/// The file of the folders the agents' hooks named (§2.3): a line each,
/// the path, a tab, the last time in seconds of the Unix epoch, the newest
/// last.
pub const FOLDERS: &str = "folders";
/// Its lines at most, and how old they may be.
const FOLDERS_MOST: usize = 200;
const FOLDERS_AGE: u64 = 30 * 24 * 3600;

/// `text`, the folders' file, with `folder` named at `now` (seconds): its
/// line last, the others older than 30 days out, 200 at most.
pub fn folders_with(text: &str, folder: &Path, now: u64) -> String {
    let named = folder.display().to_string();
    let mut lines: Vec<String> = text
        .lines()
        .filter(|line| {
            let Some((path, at)) = line.split_once('\t') else {
                return false;
            };
            path != named
                && at
                    .trim()
                    .parse::<u64>()
                    .is_ok_and(|at| now.saturating_sub(at) <= FOLDERS_AGE)
        })
        .map(str::to_owned)
        .collect();
    lines.push(format!("{named}\t{now}"));
    let skip = lines.len().saturating_sub(FOLDERS_MOST);
    let mut out = lines.split_off(skip).join("\n");
    out.push('\n');
    out
}

/// A window handed to the thread to be read: its title and size find a
/// browser's in its file (never logged).
#[derive(Clone)]
pub(crate) struct Seen {
    pub id: WindowId,
    pub app_id: String,
    pub pid: Option<u32>,
    pub title: String,
    pub size: (i32, i32),
}

impl std::fmt::Debug for Seen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Seen")
            .field("id", &self.id)
            .field("app_id", &self.app_id)
            .finish_non_exhaustive()
    }
}

/// A window as the thread read it: its desktop entry and the name it
/// gives the application; a terminal's folder and program; a browser's
/// tabs (none for a private window, or when they are not read).
#[derive(Debug, Clone)]
pub(crate) struct Found {
    pub id: WindowId,
    pub entry: Option<String>,
    pub name: String,
    pub terminal: Option<crate::terminals::Found>,
    pub browser: Option<crate::browsers::SavedBrowser>,
}

/// What it answers.
#[derive(Debug)]
pub enum Done {
    Checked {
        serial: u64,
        directory: bool,
    },
    Read {
        serial: u64,
        found: Vec<Found>,
    },
    Tabs {
        serial: u64,
        found: Vec<(WindowId, Option<crate::browsers::SavedBrowser>)>,
    },
}

/// The projects, and what is on its way for them.
#[derive(Debug, Default)]
pub struct Projects {
    pub list: Vec<Project>,
    pub(crate) worker: Option<mpsc::Sender<Job>>,
    asked: Vec<Asked>,
    pub(crate) serial: u64,
    /// The jobs given to the thread: a folder looked at, the file written
    /// (the tests: none at rest).
    pub jobs: u64,
    /// What waits for the thread's reading of windows.
    pub(crate) reading: Vec<(u64, crate::project_life::Reading)>,
    /// The projects closing (§5).
    pub(crate) closing: Vec<crate::project_life::Closing>,
    /// Desktops the session added for projects reopening (§7), taken out
    /// as they close.
    pub(crate) added: usize,
    /// What the shell was last told of them, for the launcher.
    pub(crate) told: Option<Vec<hocus_link::ProjectRow>>,
    /// The last notification of a project that came back but for some:
    /// its summary and body (the tests).
    pub not_back: Option<(String, String)>,
    /// The last project that found no free desktop (the tests).
    pub no_room: Option<String>,
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Check { serial, .. } => write!(f, "Check({serial})"),
            Self::Write(_) => f.write_str("Write"),
            Self::Read {
                serial, windows, ..
            } => write!(f, "Read({serial}, {} windows)", windows.len()),
            Self::Tabs {
                serial, windows, ..
            } => write!(f, "Tabs({serial}, {} windows)", windows.len()),
            Self::Folder(_) => f.write_str("Folder"),
            Self::Contents { text, .. } => {
                write!(
                    f,
                    "Contents({})",
                    if text.is_some() { "write" } else { "remove" }
                )
            }
        }
    }
}

/// Now, as the file says it: RFC 3339, local time.
pub(crate) fn now_text() -> String {
    let now = jiff::Zoned::now();
    now.timestamp()
        .display_with_offset(now.offset())
        .to_string()
}

/// The seconds of the Unix epoch of a time the file says; now when it
/// does not read.
pub(crate) fn seconds_of(text: Option<&str>) -> i64 {
    text.and_then(|t| t.parse::<jiff::Timestamp>().ok())
        .unwrap_or_else(jiff::Timestamp::now)
        .as_second()
}

/// Reads the projects in `dir`, once, as the compositor starts: none when
/// there is no file or it does not read (said in the log).
fn read_kept(dir: &Path) -> Kept {
    let file = dir.join(FILE);
    match std::fs::read_to_string(&file) {
        Ok(text) => Kept::parse(&text).unwrap_or_else(|err| {
            tracing::warn!(file = %file.display(), %err, "the projects do not read");
            Kept::default()
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Kept::default(),
        Err(err) => {
            tracing::warn!(file = %file.display(), %err, "the projects not read");
            Kept::default()
        }
    }
}

/// Writes the projects' file in `dir` through a temporary file, so that it
/// is never read half written: the directory `0700`, the file `0600`.
fn write_kept(dir: &Path, text: &str) -> std::io::Result<()> {
    write_private(dir, FILE, text)
}

/// Writes file `name` in `dir`, for its owner alone, through a temporary
/// file: the directory `0700`, the file `0600`.
fn write_private(dir: &Path, name: &str, text: &str) -> std::io::Result<()> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let partial = dir.join(format!("{name}.new"));
    {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&partial)?;
        file.write_all(text.as_bytes())?;
    }
    std::fs::rename(&partial, dir.join(name))
}

/// Closed project `id`'s contents in `dir` (none when its file is not
/// there or does not read, said in the log).
fn read_contents(dir: &Path, id: &str) -> Vec<crate::session::restore::SavedWindow> {
    let file = dir.join(CONTENTS).join(format!("{id}.toml"));
    match std::fs::read_to_string(&file) {
        Ok(text) => Contents::parse(&text).map_or_else(
            |err| {
                tracing::warn!(file = %file.display(), %err, "a project's contents do not read");
                Vec::new()
            },
            |c| c.windows,
        ),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => {
            tracing::warn!(file = %file.display(), %err, "a project's contents not read");
            Vec::new()
        }
    }
}

/// Reads `windows` (§8): each one's desktop entry, a terminal's folder and
/// program in the project of folder `within`, and, `browsers` given, a
/// browser's tabs.
fn read_windows(
    entries: &mut crate::session::restore::Entries,
    reader: &mut crate::browsers::Reader,
    windows: &[Seen],
    within: &Path,
    browsers: Option<&crate::browsers::Places>,
) -> Vec<Found> {
    let asked: Vec<(WindowId, crate::browsers::Asked)> = windows
        .iter()
        .filter_map(|seen| {
            Some((
                seen.id,
                crate::browsers::Asked {
                    browser: crate::browsers::of_app(&seen.app_id)?,
                    title: seen.title.clone(),
                    size: seen.size,
                },
            ))
        })
        .collect();
    let tabs: Vec<(WindowId, Option<crate::browsers::SavedBrowser>)> = match browsers {
        Some(places) => {
            let only: Vec<crate::browsers::Asked> = asked.iter().map(|(_, a)| a.clone()).collect();
            asked
                .iter()
                .map(|(id, _)| *id)
                .zip(reader.read(places, &only, false))
                .collect()
        }
        None => Vec::new(),
    };
    windows
        .iter()
        .map(|seen| {
            let entry = entries.for_app(&seen.app_id);
            let shared = windows
                .iter()
                .filter(|w| w.pid.is_some() && w.pid == seen.pid)
                .count();
            let terminal = entry.as_ref().and_then(|entry| {
                let window = crate::session::restore::SavedWindow {
                    app_id: seen.app_id.clone(),
                    pid: seen.pid,
                    ..Default::default()
                };
                crate::session::restore::terminal_of(&window, entry, shared, Some(within))
            });
            Found {
                id: seen.id,
                name: entry
                    .as_ref()
                    .map_or_else(|| seen.app_id.clone(), |e| e.name.clone()),
                entry: entry.map(|e| e.id),
                terminal,
                browser: tabs
                    .iter()
                    .find(|(id, _)| *id == seen.id)
                    .and_then(|(_, b)| b.clone()),
            }
        })
        .collect()
}

/// The projects' thread: it looks at folders and windows, and writes the
/// files.
fn work(
    dir: Option<&Path>,
    applications: Vec<PathBuf>,
    jobs: &mpsc::Receiver<Job>,
    done: &channel::Sender<Done>,
) {
    let mut entries = crate::session::restore::Entries::new(applications);
    let mut reader = crate::browsers::Reader::default();
    while let Ok(job) = jobs.recv() {
        match job {
            Job::Check { serial, folder } => {
                let directory = folder.is_absolute() && folder.is_dir();
                if done.send(Done::Checked { serial, directory }).is_err() {
                    return;
                }
            }
            Job::Write(text) => {
                let Some(dir) = dir else {
                    continue;
                };
                match write_kept(dir, &text) {
                    Ok(()) => tracing::debug!("the projects written"),
                    Err(err) => {
                        tracing::warn!(dir = %dir.display(), %err, "could not write the projects");
                    }
                }
            }
            Job::Read {
                serial,
                windows,
                within,
                browsers,
            } => {
                let found = read_windows(
                    &mut entries,
                    &mut reader,
                    &windows,
                    &within,
                    browsers.as_ref(),
                );
                if done.send(Done::Read { serial, found }).is_err() {
                    return;
                }
            }
            Job::Tabs {
                serial,
                windows,
                browsers,
            } => {
                let only: Vec<crate::browsers::Asked> =
                    windows.iter().map(|(_, a)| a.clone()).collect();
                let found = windows
                    .iter()
                    .map(|(id, _)| *id)
                    .zip(reader.read(&browsers, &only, true))
                    .collect();
                if done.send(Done::Tabs { serial, found }).is_err() {
                    return;
                }
            }
            Job::Folder(folder) => {
                let Some(dir) = dir else {
                    continue;
                };
                // Never a folder with a line break: one line each.
                if folder.to_string_lossy().contains(['\n', '\t']) {
                    continue;
                }
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                let text = std::fs::read_to_string(dir.join(FOLDERS)).unwrap_or_default();
                if let Err(err) = write_private(dir, FOLDERS, &folders_with(&text, &folder, now)) {
                    tracing::warn!(%err, "could not write the agents' folders");
                }
            }
            Job::Contents { id, text } => {
                let Some(dir) = dir else {
                    continue;
                };
                let folder = dir.join(CONTENTS);
                let file = format!("{id}.toml");
                let written = match &text {
                    Some(text) => {
                        if !dir.exists() {
                            let _ = write_private(dir, ".keep", "");
                        }
                        write_private(&folder, &file, text)
                    }
                    None => match std::fs::remove_file(folder.join(&file)) {
                        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                        other => other,
                    },
                };
                match written {
                    Ok(()) => tracing::debug!(written = text.is_some(), "a project's contents"),
                    Err(err) => {
                        tracing::warn!(%err, "could not write a project's contents");
                    }
                }
            }
        }
    }
}

/// Reads the projects kept, and starts the thread that looks at folders
/// and writes them.
pub fn start(handle: &smithay::reexports::calloop::LoopHandle<'static, App>, state: &mut State) {
    let dir = state.config.state_dir.clone();
    if let Some(dir) = &dir {
        state.projects.list = read_kept(dir).projects;
        for project in state.projects.list.iter_mut().filter(|p| p.open.is_none()) {
            project.contents = read_contents(dir, &project.id);
        }
        tracing::info!(
            projects = state.projects.list.len(),
            open = state
                .projects
                .list
                .iter()
                .filter(|p| p.open.is_some())
                .count(),
            "the projects"
        );
    }
    let (done, heard) = channel::channel::<Done>();
    let inserted = handle.insert_source(heard, |event, (), app| {
        if let channel::Event::Msg(done) = event {
            app.state.projects_done(done);
        }
    });
    if let Err(err) = inserted {
        tracing::warn!(%err, "no projects: their thread cannot answer");
        return;
    }
    let (to, jobs) = mpsc::channel();
    let applications = state.config.applications.clone();
    let started = std::thread::Builder::new()
        .name("projects".to_owned())
        .spawn(move || work(dir.as_deref(), applications, &jobs, &done));
    match started {
        Ok(_) => state.projects.worker = Some(to),
        Err(err) => tracing::warn!(%err, "no thread for the projects"),
    }
}

/// Where a window in `mode` is, as the card says it.
pub fn place_of(mode: Mode, minimized: bool) -> WindowPlace {
    if minimized {
        return WindowPlace::Minimized;
    }
    match mode {
        Mode::Floating => WindowPlace::Floating,
        Mode::Fullscreen => WindowPlace::Fullscreen,
        Mode::Snapped(zone) => match zone {
            Zone::Maximized => WindowPlace::Maximized,
            Zone::Left => WindowPlace::Left,
            Zone::Right => WindowPlace::Right,
            Zone::TopLeft => WindowPlace::TopLeft,
            Zone::TopRight => WindowPlace::TopRight,
            Zone::BottomLeft => WindowPlace::BottomLeft,
            Zone::BottomRight => WindowPlace::BottomRight,
        },
    }
}

/// The zone a saved window's state snaps it to, if it does.
pub fn zone_of(shape: crate::session::restore::Shape) -> Option<Zone> {
    match shape.mode() {
        Mode::Snapped(zone) => Some(zone),
        Mode::Floating | Mode::Fullscreen => None,
    }
}

/// Where a saved window is, as the card says it.
pub fn place_of_shape(shape: crate::session::restore::Shape) -> WindowPlace {
    place_of(shape.mode(), false)
}

/// Why a project was refused, for a Settings' line.
fn refusal_said(why: &ProjectRefusal) -> String {
    match why {
        ProjectRefusal::NameTaken(name) => format!("a project is already named {name}"),
        ProjectRefusal::Name(NameFault::Empty) => "a project needs a name".to_owned(),
        ProjectRefusal::Name(NameFault::TooLong) => "40 characters at most".to_owned(),
        ProjectRefusal::NoFolder => "no such folder".to_owned(),
        ProjectRefusal::Desktop => "no such desktop, or a project already".to_owned(),
    }
}

impl State {
    /// The project open on desktop `desktop` (from 0) of `output`.
    pub fn project_on(&self, output: &Output, desktop: usize) -> Option<&Project> {
        let screen = self.screen_key(&output.name());
        self.projects.list.iter().find(|p| {
            p.open
                .as_ref()
                .is_some_and(|o| o.screen == screen && o.desktop == desktop + 1)
        })
    }

    /// `output`'s desktops that are projects, as its bar shows them.
    pub fn projects_view(&self, output: &Output) -> Vec<DeskProject> {
        let screen = self.screen_key(&output.name());
        let count = self.strip(output).count();
        let mut out: Vec<DeskProject> = self
            .projects
            .list
            .iter()
            .filter_map(|p| {
                let open = p.open.as_ref().filter(|o| o.screen == screen)?;
                let desktop = open.desktop.checked_sub(1).filter(|d| *d < count)?;
                Some(DeskProject {
                    desktop: u8::try_from(desktop).ok()?,
                    name: p.name.clone(),
                    folder: p.folder.display().to_string(),
                })
            })
            .collect();
        out.sort_by_key(|p| p.desktop);
        out
    }

    /// Desktop `desktop`'s name on `output` (from 0): its project's, else
    /// its name in `[desktops] names`.
    pub fn desktop_name(&self, output: &Output, desktop: usize) -> Option<String> {
        self.project_on(output, desktop)
            .map(|p| p.name.clone())
            .or_else(|| self.config.desktops.name(desktop).map(str::to_owned))
    }

    /// `output`'s desktops' names, by position: empty for those with none.
    pub fn desktop_names_on(&self, output: &Output) -> Vec<String> {
        (0..self.strip(output).count())
            .map(|d| self.desktop_name(output, d).unwrap_or_default())
            .collect()
    }

    /// The windows that belong to project `name`: those on its desktop,
    /// minimised ones too, never those on all desktops.
    pub fn project_windows(&self, name: &str) -> Vec<WindowId> {
        let key = link::name_key(name);
        let Some(open) = self
            .projects
            .list
            .iter()
            .find(|p| link::name_key(&p.name) == key)
            .and_then(|p| p.open.as_ref())
        else {
            return Vec::new();
        };
        let Some(output) = self
            .connector_of(&open.screen)
            .and_then(|c| self.space.outputs().find(|o| o.name() == c).cloned())
        else {
            return Vec::new();
        };
        self.windows_on(&output)
            .into_iter()
            .filter(|id| {
                !self.model.is_everywhere(*id) && self.model.desktop(*id) + 1 == open.desktop
            })
            .collect()
    }

    /// The project of window `id`, if it belongs to one.
    pub fn project_of(&self, id: WindowId) -> Option<&Project> {
        if self.model.is_everywhere(id) {
            return None;
        }
        let output = self.output_of(id)?;
        self.project_on(&output, self.model.desktop(id))
    }

    /// The pids of the terminals on desktop `desktop` of `output`, whose
    /// folders the form proposes first.
    pub fn project_terminals(&self, output: &Output, desktop: usize) -> Vec<u32> {
        let mut pids: Vec<u32> = self
            .windows_on(output)
            .into_iter()
            .filter(|id| self.model.desktop(*id) == desktop && !self.model.is_everywhere(*id))
            .filter_map(|id| {
                let managed = self.windows.get(&id)?;
                let app = crate::windows::app_id(&managed.window)?;
                crate::terminals::is_terminal_app(&app).then(|| self.pid_of(&managed.window))?
            })
            .collect();
        pids.sort_unstable();
        pids.dedup();
        pids
    }

    /// The form's or a line's project asked: refused at once when the name
    /// or the desktop will not do, else made once its folder is looked at.
    pub fn project_make(
        &mut self,
        by: Asker,
        output: &str,
        desktop: usize,
        name: &str,
        folder: &str,
    ) {
        let name = name.trim().to_owned();
        let folder = PathBuf::from(folder.trim());
        if let Err(why) = self.project_may(output, desktop, &name) {
            self.project_refused(by, &why);
            return;
        }
        if !folder.is_absolute() {
            self.project_refused(by, &ProjectRefusal::NoFolder);
            return;
        }
        let Some(worker) = &self.projects.worker else {
            tracing::warn!("no projects' thread: no project made");
            self.project_refused(by, &ProjectRefusal::NoFolder);
            return;
        };
        self.projects.serial += 1;
        self.projects.jobs += 1;
        let serial = self.projects.serial;
        if worker
            .send(Job::Check {
                serial,
                folder: folder.clone(),
            })
            .is_err()
        {
            self.project_refused(by, &ProjectRefusal::NoFolder);
            return;
        }
        self.projects.asked.push(Asked {
            serial,
            by,
            output: output.to_owned(),
            desktop,
            name,
            folder,
        });
    }

    /// Whether desktop `desktop` of `output` may become project `name`.
    fn project_may(&self, output: &str, desktop: usize, name: &str) -> Result<(), ProjectRefusal> {
        if let Some(fault) = link::name_fault(name) {
            return Err(ProjectRefusal::Name(fault));
        }
        let key = link::name_key(name);
        if let Some(taken) = self
            .projects
            .list
            .iter()
            .find(|p| link::name_key(&p.name) == key)
        {
            return Err(ProjectRefusal::NameTaken(taken.name.clone()));
        }
        let Some(output) = self.space.outputs().find(|o| o.name() == output) else {
            return Err(ProjectRefusal::Desktop);
        };
        if desktop >= self.strip(output).count() || self.project_on(output, desktop).is_some() {
            return Err(ProjectRefusal::Desktop);
        }
        Ok(())
    }

    fn project_refused(&mut self, by: Asker, why: &ProjectRefusal) {
        tracing::info!(?why, "a project refused");
        match by {
            Asker::Shell => self.tell_shell(ToShell::ProjectRefused(why.clone())),
            Asker::Launcher { .. } => {}
            Asker::Line(session) => {
                let said = serde_json::json!({ "refused": refusal_said(why) }).to_string();
                self.displays_send(Some(session), &said);
            }
        }
    }

    /// The projects' thread answered.
    fn projects_done(&mut self, done: Done) {
        let (serial, directory) = match done {
            Done::Checked { serial, directory } => (serial, directory),
            Done::Read { serial, found } => {
                self.project_read(serial, found);
                return;
            }
            Done::Tabs { serial, found } => {
                self.project_tabs_again(serial, found);
                return;
            }
        };
        let Some(at) = self.projects.asked.iter().position(|a| a.serial == serial) else {
            return;
        };
        let asked = self.projects.asked.remove(at);
        if !directory {
            self.project_refused(asked.by, &ProjectRefusal::NoFolder);
            return;
        }
        // What was free may be taken by now.
        if let Err(why) = self.project_may(&asked.output, asked.desktop, &asked.name) {
            self.project_refused(asked.by, &why);
            return;
        }
        let screen = self.screen_key(&asked.output);
        let id = link::id_of(&asked.name, |id| {
            self.projects.list.iter().any(|p| p.id == id)
        });
        let now = now_text();
        tracing::info!(
            output = asked.output,
            desktop = asked.desktop + 1,
            "a desktop made a project"
        );
        self.projects.list.push(Project {
            id: id.clone(),
            name: asked.name.clone(),
            folder: asked.folder.clone(),
            made: now.clone(),
            opened: Some(now),
            closed: None,
            open: Some(OpenOn {
                screen: screen.clone(),
                desktop: asked.desktop + 1,
            }),
            away_from: None,
            contents: Vec::new(),
        });
        self.projects_changed();
        match asked.by {
            Asker::Launcher { empty } => {
                self.project_made_from_launcher(&asked.output, asked.desktop, &asked.folder, empty);
                self.tell_shell(ToShell::ProjectMade {
                    output: asked.output.clone(),
                    desktop: u8::try_from(asked.desktop).unwrap_or(u8::MAX),
                    name: asked.name.clone(),
                });
            }
            Asker::Shell => self.tell_shell(ToShell::ProjectMade {
                output: asked.output.clone(),
                desktop: u8::try_from(asked.desktop).unwrap_or(u8::MAX),
                name: asked.name.clone(),
            }),
            Asker::Line(session) => {
                let said = serde_json::json!({
                    "project-made": {
                        "id": id,
                        "name": asked.name,
                        "screen": screen,
                        "desktop": asked.desktop + 1,
                    }
                })
                .to_string();
                self.displays_send(Some(session), &said);
            }
        }
    }

    /// The projects changed: written, and said wherever a desktop's name
    /// shows.
    pub(crate) fn projects_changed(&mut self) {
        if let Some(worker) = &self.projects.worker {
            self.projects.jobs += 1;
            let text = Kept {
                projects: self.projects.list.clone(),
            }
            .text();
            if worker.send(Job::Write(text)).is_err() {
                tracing::warn!("the projects' thread is gone: not written");
            }
        }
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        for output in &outputs {
            self.desktops_drawn(output);
        }
        self.tell_projects();
        self.redraw = true;
    }

    /// The desktops are renumbered on every screen (a desktop taken out,
    /// fewer in the file): each project goes with its desktop to `map(d)`,
    /// unless a project is there already, or it would land on a desktop
    /// another one goes to: then it is no longer open.
    pub(crate) fn projects_renumbered(&mut self, map: impl Fn(usize) -> usize) {
        let mut changed = false;
        let mut taken: Vec<(String, usize)> = Vec::new();
        let now = now_text();
        for project in &mut self.projects.list {
            let Some(open) = &mut project.open else {
                continue;
            };
            let to = map(open.desktop - 1) + 1;
            if taken.contains(&(open.screen.clone(), to)) {
                tracing::info!(
                    desktop = open.desktop,
                    "a project's desktop went onto another's: it closes"
                );
                project.open = None;
                project.closed = Some(now.clone());
                changed = true;
                continue;
            }
            taken.push((open.screen.clone(), to));
            if to != open.desktop {
                open.desktop = to;
                changed = true;
            }
        }
        if changed {
            self.projects_changed();
        }
    }

    /// The form that makes desktop `desktop` of `output` a project, as it
    /// opens under `at`.
    pub(crate) fn project_form(
        &self,
        output: &Output,
        desktop: usize,
        at: hocus_link::Rect,
    ) -> Option<hocus_link::ProjectForm> {
        let geometry = self.space.output_geometry(output)?;
        Some(hocus_link::ProjectForm {
            output: output.name(),
            desktop: u8::try_from(desktop).ok()?,
            at,
            screen: (geometry.size.w, geometry.size.h),
            terminals: self.project_terminals(output, desktop),
            taken: self.projects.list.iter().map(|p| p.name.clone()).collect(),
            folders: self
                .projects
                .list
                .iter()
                .map(|p| p.folder.display().to_string())
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_reads_back_as_written() {
        let kept = Kept {
            projects: vec![
                Project {
                    id: "hocus-site".into(),
                    name: "hocus-site".into(),
                    folder: "/home/alex/github/hocus-site".into(),
                    made: "2026-11-13T09:12:00+01:00".into(),
                    opened: Some("2026-11-13T09:12:00+01:00".into()),
                    closed: None,
                    open: Some(OpenOn {
                        screen: "BOE NE160QDM 0".into(),
                        desktop: 2,
                    }),
                    away_from: None,
                    contents: Vec::new(),
                },
                Project {
                    id: "blog".into(),
                    name: "Blog".into(),
                    folder: "/home/alex/code/blog".into(),
                    made: "2026-11-01T18:40:00+01:00".into(),
                    opened: None,
                    closed: Some("2026-11-12T18:40:00+01:00".into()),
                    open: None,
                    away_from: None,
                    contents: Vec::new(),
                },
            ],
        };
        let text = kept.text();
        assert!(text.contains("[[project]]"), "{text}");
        assert!(text.contains("[project.open]"), "{text}");
        assert_eq!(Kept::parse(&text), Ok(kept));
        assert_eq!(Kept::parse(""), Ok(Kept::default()));
    }

    #[test]
    fn it_is_written_for_its_owner_alone() {
        let dir = tempfile::tempdir().expect("a directory");
        let state = dir.path().join("state/hocuspanda");
        write_kept(&state, "x = 1\n").expect("written");
        let mode = |p: &Path| std::fs::metadata(p).expect("there").permissions().mode() & 0o777;
        assert_eq!(mode(&state), 0o700);
        assert_eq!(mode(&state.join(FILE)), 0o600);
        assert!(!state.join("projects.new").exists());
        assert_eq!(read_kept(&state), Kept::default(), "x = 1 holds no project");
    }

    #[test]
    fn a_closed_projects_contents_read_back_as_written() {
        use crate::session::restore::{SavedTerminal, SavedWindow, Shape};
        let dir = tempfile::tempdir().expect("a directory");
        let contents = Contents {
            windows: vec![
                SavedWindow {
                    entry: "foot".into(),
                    app_id: "foot".into(),
                    screen: "BOE NE160QDM 0".into(),
                    screen_size: [2560, 1600],
                    area: [0, 0, 2364, 1600],
                    desktop: 2,
                    place: [8, 8, 1180, 1584],
                    state: Shape::Left,
                    terminal: Some(SavedTerminal {
                        folder: "/home/alex/github/hocus-site".into(),
                    }),
                    ..SavedWindow::default()
                },
                SavedWindow {
                    entry: "org.mozilla.firefox".into(),
                    app_id: "org.mozilla.firefox".into(),
                    browser: Some(crate::browsers::SavedBrowser {
                        kind: crate::browsers::Kind::Firefox,
                        selected: 1,
                        tabs: vec![crate::browsers::Tab {
                            url: "http://localhost:4321/".into(),
                            title: "hocus-site · Accueil".into(),
                            pinned: true,
                        }],
                    }),
                    ..SavedWindow::default()
                },
            ],
        };
        let text = contents.text();
        assert!(text.contains("[window.terminal]"), "{text}");
        assert!(
            text.contains("[window.browser]\nkind = \"firefox\""),
            "{text}"
        );
        assert!(text.contains("[[window.browser.tab]]"), "{text}");
        let folder = dir.path().join(CONTENTS);
        write_private(&folder, "hocus-site.toml", &text).expect("written");
        assert_eq!(read_contents(dir.path(), "hocus-site"), contents.windows);
        assert!(read_contents(dir.path(), "blog").is_empty());
    }

    #[test]
    fn the_folders_file_keeps_each_folder_once_newest_last() {
        let now = 40 * 24 * 3600;
        let text = format!("/old\t{}\n/a\t{}\n/b\t{}\n", 1, now - 60, now - 30);
        assert_eq!(
            folders_with(&text, Path::new("/a"), now),
            format!("/b\t{}\n/a\t{now}\n", now - 30)
        );
        let many: String = (0..250).map(|i| format!("/f{i}\t{now}\n")).collect();
        let kept = folders_with(&many, Path::new("/new"), now);
        assert_eq!(kept.lines().count(), 200);
        assert!(kept.ends_with(&format!("/new\t{now}\n")));
    }

    #[test]
    fn a_seconds_time_reads_from_the_file() {
        assert_eq!(seconds_of(Some("1970-01-01T01:00:10+01:00")), 10);
        assert!(seconds_of(Some("not a time")) > 1_700_000_000);
    }

    #[test]
    fn a_windows_place_is_its_mode() {
        assert_eq!(place_of(Mode::Floating, false), WindowPlace::Floating);
        assert_eq!(
            place_of(Mode::Snapped(Zone::Left), false),
            WindowPlace::Left
        );
        assert_eq!(place_of(Mode::Fullscreen, true), WindowPlace::Minimized);
    }
}
