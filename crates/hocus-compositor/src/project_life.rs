//! Closing and reopening project desktops (docs/design/projects.md §5, §7,
//! lot B of #789), and what the launcher and the cards say of them.
//!
//! - **Closing** (§5): "Fermer le projet…" asks for its confirmation, which
//!   says what will come back and what closing interrupts (an agent at
//!   work, a terminal's program): the windows' desktop entries and the
//!   terminals' folders and programs are read off the loop first (§8).
//!   Confirmed (or `project-close NAME` on the control socket), its
//!   windows' places are taken on the loop, read off it, then each window
//!   is asked to close as `Super+Q` does; a window that asks something
//!   keeps the project closing until it goes, one still there 30 s after
//!   stays, named. Then its contents are written
//!   (`projects.d/ID.toml`, §9), the desktop is plain again and a toast
//!   says so.
//! - **Reopening** (§7): on the launcher's screen, the current desktop
//!   when it is empty, else the first empty one, else one more (nine at
//!   most, the session's: taken out as it closes); none: the launcher
//!   says the desktops are taken and offers to close a project. Its
//!   windows come back as the last session's do
//!   ([`crate::session::restore`]): each application once, each terminal
//!   in its folder; what did not come back in 30 s is named in one
//!   notification, the project's monogram as its icon. Its file goes once
//!   everything came back or was dropped.
//! - **From the launcher** (§2.2): the projects' rows (`projects` line),
//!   a folder made a project on the desktop §7 chooses, a terminal opening
//!   in it when that desktop is empty (§13, B).
//! - **Log in with `[session] restore = false`** (§9): the projects open as
//!   the session ended are closed, their windows of the session's file
//!   their contents.

use hocus_words::compositor::projects as txt;
use std::{path::PathBuf, time::Duration};

use hocus_link::{
    CardWindow, ComingBack, Interrupted, ProjectCard, ProjectConfirm, ProjectRow, ProjectState,
    ToShell, projects as link,
};
use smithay::{
    output::Output,
    reexports::calloop::{
        RegistrationToken,
        timer::{TimeoutAction, Timer},
    },
};

use crate::{
    State,
    model::{Rect, WindowId},
    projects::{Found, Job, OpenOn, Seen, now_text, place_of, seconds_of},
    session::restore::{Missing, Saved, SavedTerminal, SavedWindow, Shape, Whose},
    windows::rect_of,
};

/// How long a window asked to close has before it is left there (§5).
pub const CLOSE_PATIENCE: Duration = Duration::from_secs(30);

/// What waits for the thread's reading of a project's windows.
#[derive(Debug)]
pub(crate) enum Reading {
    /// The closing's confirmation of project `id`.
    Confirm { id: String },
    /// Project `id`'s card, under `at` on `output`.
    Card {
        id: String,
        output: String,
        at: hocus_link::Rect,
    },
    /// Project `id` closing: its windows' places taken before.
    Close { id: String },
    /// Project `id` closing: its browsers' tabs read again once their
    /// windows are gone (§5 step 3).
    Tabs { id: String },
}

/// A project closing.
#[derive(Debug)]
pub(crate) struct Closing {
    pub id: String,
    /// Its screen's connector, for the toast.
    pub output: String,
    /// Its windows as they were, by their ids, their entries and folders
    /// once read.
    pub windows: Vec<(WindowId, SavedWindow)>,
    pub phase: ClosingPhase,
    /// Its browsers' tabs were read again once their windows went (or
    /// need not be).
    pub tabs_again: bool,
}

/// How long after a project's browser windows closed their files are read
/// again: the browser writes a window closed as it goes (§8.3).
const TABS_AGAIN_AFTER: Duration = Duration::from_secs(1);

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ClosingPhase {
    /// Its windows are being read.
    Reading,
    /// Its windows were asked to close; those still there.
    Waiting {
        left: Vec<WindowId>,
        timer: Option<RegistrationToken>,
    },
}

/// The tool an agent's source names, as the user knows it.
fn tool_of(source: &str, name: &str) -> String {
    let first = source.split('+').next().unwrap_or_default();
    match first {
        "claude-code" => "Claude Code".to_owned(),
        "codex" => "Codex".to_owned(),
        _ => name.to_owned(),
    }
}

/// Where saved window `w` is on its screen, from its top left corner.
fn saved_rect(w: &SavedWindow) -> Rect {
    let [x, y, width, height] = w.place;
    let floating = Rect::new(x, y, width.max(1), height.max(1));
    let [ax, ay, aw, ah] = w.area;
    let area = if aw > 0 && ah > 0 {
        Rect::new(ax, ay, aw, ah)
    } else {
        Rect::new(0, 0, w.screen_size[0].max(1), w.screen_size[1].max(1))
    };
    match w.state {
        Shape::Floating => floating,
        Shape::Fullscreen => Rect::new(0, 0, w.screen_size[0].max(1), w.screen_size[1].max(1)),
        other => match crate::projects::zone_of(other) {
            Some(zone) => zone.rect(area),
            None => floating,
        },
    }
}

/// A saved window as a card and a launcher's row say it.
fn card_window(w: &SavedWindow) -> CardWindow {
    let screen = Rect::new(0, 0, w.screen_size[0].max(1), w.screen_size[1].max(1));
    CardWindow {
        id: 0,
        app: w.app_id.clone(),
        entry: w.entry.clone(),
        title: String::new(),
        place: crate::projects::place_of_shape(w.state),
        rect: crate::desktops::share(saved_rect(w), screen).unwrap_or_default(),
        focused: false,
        terminal: w.terminal.as_ref().map(|t| t.folder.display().to_string()),
        tabs: w.browser.as_ref().and_then(card_tabs),
    }
}

/// A browser window's tabs as a card shows them: their titles and hosts,
/// never their addresses (§10); none when they were not read.
fn card_tabs(b: &crate::browsers::SavedBrowser) -> Option<hocus_link::CardTabs> {
    b.read().then(|| hocus_link::CardTabs {
        count: u32::try_from(b.tabs.len()).unwrap_or(u32::MAX),
        shown: b
            .tabs
            .iter()
            .take(hocus_link::CARD_TABS_MOST)
            .map(|t| hocus_link::CardTab {
                title: t.title.clone(),
                host: crate::browsers::host_of(&t.url),
            })
            .collect(),
    })
}

/// The confirmation's dim, the same piece from one frame to the next.
static DIM: std::sync::OnceLock<smithay::backend::renderer::element::Id> =
    std::sync::OnceLock::new();

impl State {
    /// Where the browsers keep their session files, when a project's tabs
    /// are remembered (`[desktops] remember-tabs`, §13 F) and the home is
    /// known.
    pub(crate) fn browser_places(&self) -> Option<crate::browsers::Places> {
        if !self.config.desktops.remember_tabs {
            return None;
        }
        let home = self.config.home.clone()?;
        let config = self
            .config
            .agent_places
            .as_ref()
            .map_or_else(|| home.join(".config"), |p| p.config_home.clone());
        Some(crate::browsers::Places { home, config })
    }

    /// The dim under a project's closing's confirmation on `output`, as
    /// it fades in and out with it (§5, `rgba(7,9,15,0.45)`).
    pub(crate) fn project_dim(
        &self,
        output: &Output,
        now: Duration,
    ) -> Option<smithay::backend::renderer::element::solid::SolidColorRenderElement> {
        use smithay::{
            backend::renderer::element::{Id, Kind, solid::SolidColorRenderElement},
            desktop::layer_map_for_output,
            utils::{Logical, Rectangle},
        };
        let alpha = {
            let map = layer_map_for_output(output);
            let layer = map
                .layers()
                .find(|l| l.namespace() == hocus_link::PROJECT_CLOSE_NAMESPACE)?;
            self.panel
                .popovers
                .look(layer, now, self.popover_motion())?
                .alpha
        };
        if alpha <= 0.0 {
            return None;
        }
        let size = self.space.output_geometry(output)?.size;
        let scale = output.current_scale().fractional_scale();
        let a = hocus_link::PROJECT_DIM_ALPHA * alpha;
        let channel = |c: u8| f32::from(c) / 255.0 * a;
        let [r, g, b] = hocus_link::PROJECT_DIM;
        Some(SolidColorRenderElement::new(
            DIM.get_or_init(Id::new).clone(),
            Rectangle::<f64, Logical>::from_size(size.to_f64()).to_physical_precise_round(scale),
            (alpha * 1000.0) as usize,
            [channel(r), channel(g), channel(b), a],
            Kind::Unspecified,
        ))
    }

    /// The projects as the launcher lists them, told when they changed.
    pub(crate) fn tell_projects(&mut self) {
        let rows: Vec<ProjectRow> = self
            .projects
            .list
            .iter()
            .map(|p| {
                let (output, state) = match &p.open {
                    Some(open) => (
                        self.connector_of(&open.screen).unwrap_or_default(),
                        ProjectState::Open {
                            desktop: u8::try_from(open.desktop.saturating_sub(1))
                                .unwrap_or(u8::MAX),
                            since: seconds_of(p.opened.as_deref().or(Some(&p.made))),
                        },
                    ),
                    None => (
                        String::new(),
                        ProjectState::Closed {
                            at: seconds_of(p.closed.as_deref().or(Some(&p.made))),
                        },
                    ),
                };
                ProjectRow {
                    name: p.name.clone(),
                    folder: p.folder.display().to_string(),
                    output,
                    state,
                    windows: p.contents.iter().map(card_window).collect(),
                }
            })
            .collect();
        if self.projects.told.as_ref() == Some(&rows) {
            return;
        }
        self.projects.told = Some(rows.clone());
        self.tell_shell(ToShell::Projects(rows));
    }

    /// The project named `name` (case and accents aside), its place.
    fn project_at(&self, name: &str) -> Option<usize> {
        let key = link::name_key(name);
        self.projects
            .list
            .iter()
            .position(|p| link::name_key(&p.name) == key)
    }

    /// The output project `at` is open on.
    fn project_output(&self, at: usize) -> Option<(Output, usize)> {
        let open = self.projects.list.get(at)?.open.as_ref()?;
        let connector = self.connector_of(&open.screen)?;
        let output = self
            .space
            .outputs()
            .find(|o| o.name() == connector)?
            .clone();
        Some((output, open.desktop.checked_sub(1)?))
    }

    /// The windows of open project `at`, front first, as the thread reads
    /// them.
    fn project_seen(&self, at: usize) -> Vec<Seen> {
        let Some((output, desktop)) = self.project_output(at) else {
            return Vec::new();
        };
        self.overview_windows(&output, desktop)
            .into_iter()
            .filter(|id| !self.model.is_everywhere(*id) && self.model.parent(*id).is_none())
            .filter_map(|id| {
                let managed = self.windows.get(&id)?;
                Some(Seen {
                    id,
                    app_id: crate::windows::app_id(&managed.window).unwrap_or_default(),
                    pid: self.pid_of(&managed.window),
                    title: crate::windows::title_of(self, id),
                    size: self.home_rect(id).map_or((0, 0), |r| (r.width, r.height)),
                })
            })
            .collect()
    }

    /// Hands `windows` of project `at` to the thread, for `reading`.
    fn project_read_ask(&mut self, at: usize, windows: Vec<Seen>, reading: Reading) -> bool {
        let Some(project) = self.projects.list.get(at) else {
            return false;
        };
        let within = project.folder.clone();
        let Some(worker) = &self.projects.worker else {
            return false;
        };
        self.projects.serial += 1;
        self.projects.jobs += 1;
        let serial = self.projects.serial;
        let browsers = self.browser_places();
        if worker
            .send(Job::Read {
                serial,
                windows,
                within,
                browsers,
            })
            .is_err()
        {
            tracing::warn!("the projects' thread is gone: nothing read");
            return false;
        }
        self.projects.reading.push((serial, reading));
        true
    }

    /// A project's card under its thumbnail (§4): its terminals' folders
    /// read first.
    pub(crate) fn project_card_ask(
        &mut self,
        output: &Output,
        desktop: usize,
        at: hocus_link::Rect,
    ) {
        let Some(id) = self.project_on(output, desktop).map(|p| p.id.clone()) else {
            return;
        };
        let Some(index) = self.projects.list.iter().position(|p| p.id == id) else {
            return;
        };
        let seen = self.project_seen(index);
        self.project_read_ask(
            index,
            seen,
            Reading::Card {
                id,
                output: output.name(),
                at,
            },
        );
    }

    /// "Fermer le projet…" on project `name`: its confirmation, once its
    /// windows are read.
    pub fn project_close_ask(&mut self, name: &str) {
        let Some(at) = self.project_at(name) else {
            return;
        };
        let Some(id) = self
            .projects
            .list
            .get(at)
            .filter(|p| p.open.is_some())
            .map(|p| p.id.clone())
        else {
            return;
        };
        tracing::info!(project = id, "a project's closing asked");
        let seen = self.project_seen(at);
        self.project_read_ask(at, seen, Reading::Confirm { id });
    }

    /// Project `name` closes (§5): its windows' places taken now, read off
    /// the loop, then each asked to close. Why not, if it cannot.
    pub fn project_close(&mut self, name: &str) -> Result<(), String> {
        let at = self.project_at(name).ok_or("no such project")?;
        let project = self.projects.list.get(at).ok_or("no such project")?;
        if project.open.is_none() {
            return Err("closed already".to_owned());
        }
        let id = project.id.clone();
        if self.projects.closing.iter().any(|c| c.id == id) {
            return Err("closing already".to_owned());
        }
        let (output, _) = self.project_output(at).ok_or("its screen is not there")?;
        let seen = self.project_seen(at);
        let windows: Vec<(WindowId, SavedWindow)> = seen
            .iter()
            .filter_map(|s| Some((s.id, self.session_window(s.id)?)))
            .collect();
        tracing::info!(project = id, windows = windows.len(), "a project closes");
        self.projects.closing.push(Closing {
            id: id.clone(),
            output: output.name(),
            windows,
            phase: ClosingPhase::Reading,
            tabs_again: false,
        });
        if !self.project_read_ask(at, seen, Reading::Close { id: id.clone() }) {
            // Nothing read: closed with what the loop knows.
            self.project_closing_read(&id, Vec::new());
        }
        Ok(())
    }

    /// The thread read windows for `serial`.
    pub(crate) fn project_read(&mut self, serial: u64, found: Vec<Found>) {
        let Some(at) = self.projects.reading.iter().position(|(s, _)| *s == serial) else {
            return;
        };
        let (_, reading) = self.projects.reading.remove(at);
        match reading {
            Reading::Confirm { id } => self.project_confirm(&id, &found),
            Reading::Card { id, output, at } => self.project_card(&id, &output, at, &found),
            Reading::Close { id } => self.project_closing_read(&id, found),
            Reading::Tabs { .. } => {}
        }
    }

    /// The thread read a closing project's browsers' tabs again: those it
    /// found replace what was read before.
    pub(crate) fn project_tabs_again(
        &mut self,
        serial: u64,
        found: Vec<(WindowId, Option<crate::browsers::SavedBrowser>)>,
    ) {
        let Some(at) = self.projects.reading.iter().position(|(s, _)| *s == serial) else {
            return;
        };
        let (_, reading) = self.projects.reading.remove(at);
        let Reading::Tabs { id } = reading else {
            return;
        };
        if let Some(closing) = self.projects.closing.iter_mut().find(|c| c.id == id) {
            for (wid, browser) in found {
                if let Some((_, saved)) = closing.windows.iter_mut().find(|(w, _)| *w == wid)
                    && let Some(browser) = browser.filter(crate::browsers::SavedBrowser::read)
                {
                    saved.browser = Some(browser);
                }
            }
        }
        self.project_closed(&id);
    }

    /// Project `id`'s browsers' windows closed: their files are read again
    /// a moment later (§5 step 3). Whether it was asked.
    fn project_tabs_again_ask(&mut self, id: &str, left: &[WindowId]) -> bool {
        let Some(browsers) = self.browser_places() else {
            return false;
        };
        let Some(closing) = self.projects.closing.iter_mut().find(|c| c.id == id) else {
            return false;
        };
        closing.tabs_again = true;
        let windows: Vec<(WindowId, crate::browsers::Asked)> = closing
            .windows
            .iter()
            .filter(|(w, s)| !left.contains(w) && s.browser.is_some())
            .filter_map(|(w, s)| {
                Some((
                    *w,
                    crate::browsers::Asked {
                        browser: crate::browsers::of_app(&s.app_id)?,
                        title: s.title.clone(),
                        size: s.size,
                    },
                ))
            })
            .collect();
        if windows.is_empty() || self.projects.worker.is_none() {
            return false;
        }
        self.projects.serial += 1;
        let serial = self.projects.serial;
        self.projects
            .reading
            .push((serial, Reading::Tabs { id: id.to_owned() }));
        let job = Job::Tabs {
            serial,
            windows,
            browsers,
        };
        let mut job = Some(job);
        let sent = self.loop_handle.insert_source(
            Timer::from_duration(TABS_AGAIN_AFTER),
            move |_, _, app| {
                if let Some(job) = job.take() {
                    app.state.projects_job(job);
                }
                TimeoutAction::Drop
            },
        );
        if let Err(err) = sent {
            tracing::warn!(%err.error, "a project's tabs are not read again");
            self.projects.reading.retain(|(s, _)| *s != serial);
            return false;
        }
        true
    }

    /// The confirmation (§5): what comes back, what closing interrupts.
    fn project_confirm(&mut self, id: &str, found: &[Found]) {
        let Some(at) = self.projects.list.iter().position(|p| p.id == id) else {
            return;
        };
        let Some((output, _)) = self.project_output(at) else {
            return;
        };
        let Some(geometry) = self.space.output_geometry(&output) else {
            return;
        };
        let mut back: Vec<ComingBack> = Vec::new();
        let mut interrupted: Vec<Interrupted> = Vec::new();
        for f in found {
            let app = self
                .windows
                .get(&f.id)
                .and_then(|m| crate::windows::app_id(&m.window))
                .unwrap_or_default();
            match (&f.entry, &f.terminal) {
                (None, _) => back.push(ComingBack::NotBack {
                    app: f.name.clone(),
                }),
                (Some(entry), None) if f.browser.as_ref().is_some_and(|b| b.read()) => {
                    back.push(ComingBack::Browser {
                        app,
                        entry: entry.clone(),
                        tabs: f
                            .browser
                            .as_ref()
                            .map_or(0, |b| u32::try_from(b.tabs.len()).unwrap_or(u32::MAX)),
                    });
                }
                (Some(entry), Some(terminal)) => {
                    back.push(ComingBack::Terminal {
                        app,
                        entry: entry.clone(),
                        folder: terminal.folder.display().to_string(),
                    });
                    if let Some(program) = &terminal.program {
                        interrupted.push(Interrupted::Program(program.clone()));
                    }
                }
                (Some(entry), None) => {
                    if let Some(ComingBack::App { count, .. }) = back
                        .iter_mut()
                        .find(|b| matches!(b, ComingBack::App { entry: e, .. } if e == entry))
                    {
                        *count += 1;
                    } else {
                        back.push(ComingBack::App {
                            app,
                            entry: entry.clone(),
                            count: 1,
                        });
                    }
                }
            }
        }
        let ids: Vec<u64> = found.iter().map(|f| f.id.0).collect();
        for agent in self.agents.told.iter().flatten() {
            let waiting = agent.state == "waiting";
            if ids.contains(&agent.window) && (waiting || agent.state == "working") {
                interrupted.insert(
                    0,
                    Interrupted::Agent {
                        tool: tool_of(&agent.source, &agent.name),
                        waiting,
                    },
                );
            }
        }
        let Some(project) = self.projects.list.get(at) else {
            return;
        };
        let confirm = ProjectConfirm {
            output: output.name(),
            screen: (geometry.size.w, geometry.size.h),
            name: project.name.clone(),
            folder: project.folder.display().to_string(),
            back,
            interrupted,
        };
        tracing::info!(
            project = id,
            rows = confirm.back.len(),
            interrupted = confirm.interrupted.len(),
            "a project's closing to confirm"
        );
        // From its card in the overview: the overview closes first.
        self.close_overview(None);
        self.tell_shell(ToShell::ProjectConfirm(confirm));
    }

    /// An open project's card, its windows' terminals read.
    fn project_card(&mut self, id: &str, output: &str, at: hocus_link::Rect, found: &[Found]) {
        let Some(index) = self.projects.list.iter().position(|p| p.id == id) else {
            return;
        };
        let Some((on, desktop)) = self.project_output(index) else {
            return;
        };
        if on.name() != output {
            return;
        }
        let Some(geometry) = self.space.output_geometry(&on) else {
            return;
        };
        let screen = rect_of(geometry);
        let focused = self.model.focused();
        let windows = found
            .iter()
            .filter(|f| self.windows.contains_key(&f.id))
            .map(|f| {
                let rect = self
                    .home_rect(f.id)
                    .and_then(|r| crate::desktops::share(r, screen))
                    .unwrap_or_default();
                CardWindow {
                    id: f.id.0,
                    app: self
                        .windows
                        .get(&f.id)
                        .and_then(|m| crate::windows::app_id(&m.window))
                        .unwrap_or_default(),
                    entry: f.entry.clone().unwrap_or_default(),
                    title: crate::windows::title_of(self, f.id),
                    place: place_of(self.model.mode(f.id), self.model.is_minimized(f.id)),
                    rect,
                    focused: focused == Some(f.id),
                    terminal: f.terminal.as_ref().map(|t| t.folder.display().to_string()),
                    tabs: f.browser.as_ref().and_then(card_tabs),
                }
            })
            .collect();
        let Some(project) = self.projects.list.get(index) else {
            return;
        };
        let card = ProjectCard {
            output: output.to_owned(),
            at: Some(at),
            screen: (geometry.size.w, geometry.size.h),
            name: project.name.clone(),
            folder: project.folder.display().to_string(),
            state: ProjectState::Open {
                desktop: u8::try_from(desktop).unwrap_or(u8::MAX),
                since: seconds_of(project.opened.as_deref().or(Some(&project.made))),
            },
            windows,
        };
        self.tell_shell(ToShell::ProjectCard(card));
    }

    /// Project `id`'s windows were read as it closes: each is asked to
    /// close, and the project waits for them, [`CLOSE_PATIENCE`] at most.
    fn project_closing_read(&mut self, id: &str, found: Vec<Found>) {
        let Some(closing) = self.projects.closing.iter_mut().find(|c| c.id == id) else {
            return;
        };
        for (wid, saved) in &mut closing.windows {
            if let Some(f) = found.iter().find(|f| f.id == *wid) {
                saved.entry = f.entry.clone().unwrap_or_default();
                saved.terminal = f.terminal.as_ref().map(|t| SavedTerminal {
                    folder: t.folder.clone(),
                });
                saved.browser.clone_from(&f.browser);
            }
        }
        let left: Vec<WindowId> = closing
            .windows
            .iter()
            .map(|(w, _)| *w)
            .filter(|w| self.windows.contains_key(w))
            .collect();
        let of = id.to_owned();
        let timer = self
            .loop_handle
            .insert_source(Timer::from_duration(CLOSE_PATIENCE), move |_, _, app| {
                app.state.project_closing_over(&of);
                TimeoutAction::Drop
            })
            .map_err(|err| tracing::warn!(%err.error, "a project's closing has no end"))
            .ok();
        if let Some(closing) = self.projects.closing.iter_mut().find(|c| c.id == id) {
            closing.phase = ClosingPhase::Waiting {
                left: left.clone(),
                timer,
            };
        }
        for window in &left {
            self.close_window(*window);
        }
        if left.is_empty() {
            self.project_closed(id);
        }
    }

    /// Window `id` went: a project closing waits for it no more.
    pub(crate) fn projects_window_gone(&mut self, id: WindowId) {
        let mut done = Vec::new();
        for closing in &mut self.projects.closing {
            if let ClosingPhase::Waiting { left, .. } = &mut closing.phase
                && let Some(at) = left.iter().position(|w| *w == id)
            {
                left.remove(at);
                if left.is_empty() {
                    done.push(closing.id.clone());
                }
            }
        }
        for id in done {
            self.project_closed(&id);
        }
    }

    /// [`CLOSE_PATIENCE`] after the windows were asked to close.
    pub(crate) fn project_closing_over(&mut self, id: &str) {
        if let Some(closing) = self.projects.closing.iter_mut().find(|c| c.id == id)
            && let ClosingPhase::Waiting { timer, .. } = &mut closing.phase
        {
            *timer = None;
        }
        self.project_closed(id);
    }

    /// Project `id` is closed: what closed is saved, what stayed is named;
    /// its desktop is plain again.
    fn project_closed(&mut self, id: &str) {
        let Some(at) = self.projects.closing.iter().position(|c| c.id == id) else {
            return;
        };
        // Its browsers' windows gone, their tabs are read again first: no
        // more waiting for its windows meanwhile.
        if let Some(closing) = self.projects.closing.get_mut(at)
            && !closing.tabs_again
        {
            closing.tabs_again = true;
            let left = match &mut closing.phase {
                ClosingPhase::Waiting { left, timer } => {
                    if let Some(timer) = timer.take() {
                        self.loop_handle.remove(timer);
                    }
                    left.clone()
                }
                ClosingPhase::Reading => Vec::new(),
            };
            if self.project_tabs_again_ask(id, &left) {
                return;
            }
        }
        let Some(at) = self.projects.closing.iter().position(|c| c.id == id) else {
            return;
        };
        let closing = self.projects.closing.remove(at);
        let left = match closing.phase {
            ClosingPhase::Waiting { left, timer } => {
                if let Some(timer) = timer {
                    self.loop_handle.remove(timer);
                }
                left
            }
            ClosingPhase::Reading => Vec::new(),
        };
        let stayed: Vec<String> = left
            .iter()
            .filter(|w| self.windows.contains_key(w))
            .map(|w| crate::windows::title_of(self, *w))
            .collect();
        let contents: Vec<SavedWindow> = closing
            .windows
            .into_iter()
            .filter(|(w, s)| !left.contains(w) && !s.entry.is_empty())
            .map(|(_, mut s)| {
                s.pid = None;
                s.title.clear();
                s
            })
            .collect();
        let Some(index) = self.projects.list.iter().position(|p| p.id == id) else {
            return;
        };
        let place = self.project_output(index);
        let text = crate::projects::Contents {
            windows: contents.clone(),
        }
        .text();
        self.projects_job(Job::Contents {
            id: id.to_owned(),
            text: Some(text),
        });
        let Some(project) = self.projects.list.get_mut(index) else {
            return;
        };
        project.open = None;
        project.away_from = None;
        project.closed = Some(now_text());
        project.contents = contents;
        let name = project.name.clone();
        let kept = project.contents.len();
        tracing::info!(
            project = id,
            kept,
            stayed = stayed.len(),
            "a project is closed"
        );
        self.projects_changed();
        if let Some((output, desktop)) = place {
            self.desktop_added_gone(&output, desktop);
        }
        self.tell_shell(ToShell::ProjectClosed {
            output: closing.output,
            name,
            kept: u32::try_from(kept).unwrap_or(u32::MAX),
            stayed,
        });
    }

    /// Gives the thread `job`.
    pub(crate) fn projects_job(&mut self, job: Job) {
        if let Some(worker) = &self.projects.worker {
            self.projects.jobs += 1;
            if worker.send(job).is_err() {
                tracing::warn!("the projects' thread is gone");
            }
        }
    }

    /// A desktop the session added for a project, now its last and empty
    /// on every screen: taken out.
    fn desktop_added_gone(&mut self, output: &Output, desktop: usize) {
        let count = self.config.desktops.count;
        if self.projects.added == 0 || desktop + 1 != count || count <= 1 {
            return;
        }
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        let empty = outputs.iter().all(|o| self.desktop_free(o, desktop));
        if !empty {
            return;
        }
        tracing::info!(
            desktop = desktop + 1,
            "the desktop added for a project is taken out"
        );
        self.projects.added -= 1;
        let last = count - 2;
        let current = self.strip(output).current();
        self.renumber_desktops_for_projects(count - 1, move |d| d.min(last));
        if current == desktop {
            self.switch_desktop(output, last, None);
        }
    }

    /// Whether desktop `desktop` of `output` is free: no window of its own
    /// (those on every desktop aside), not a project, nothing coming back
    /// to it.
    pub(crate) fn desktop_free(&self, output: &Output, desktop: usize) -> bool {
        let screen = self.screen_key(&output.name());
        self.project_on(output, desktop).is_none()
            && !self.coming_back_to(&screen, desktop + 1)
            && !self
                .windows_on(output)
                .into_iter()
                .any(|id| !self.model.is_everywhere(id) && self.model.desktop(id) == desktop)
    }

    /// The desktop of `output` a project opens on (§7): the current one
    /// when free, else the first free one, else one more (nine at most).
    /// Whether it was added; none when nine are taken.
    pub(crate) fn desktop_for_project(&mut self, output: &Output) -> Option<(usize, bool)> {
        let strip = self.strip(output);
        if self.desktop_free(output, strip.current()) {
            return Some((strip.current(), false));
        }
        if let Some(free) = (0..strip.count()).find(|d| self.desktop_free(output, *d)) {
            return Some((free, false));
        }
        if strip.count() >= crate::desktops::MAX_COUNT {
            return None;
        }
        let count = strip.count() + 1;
        tracing::info!(count, "a desktop added for a project");
        self.projects.added += 1;
        self.renumber_desktops_for_projects(count, |d| d);
        Some((count - 1, true))
    }

    /// Closed project `name` reopens on `output` (§7). The desktop it
    /// opens on (from 0), or why not.
    pub fn project_open(&mut self, name: &str, output: &str) -> Result<usize, String> {
        let at = self.project_at(name).ok_or("no such project")?;
        let project = self.projects.list.get(at).ok_or("no such project")?;
        if project.open.is_some() {
            return Err("open already".to_owned());
        }
        let id = project.id.clone();
        let project_name = project.name.clone();
        if self.project_coming_back(&id) {
            return Err("coming back already".to_owned());
        }
        let output = self
            .space
            .outputs()
            .find(|o| o.name() == output)
            .or_else(|| self.space.outputs().next())
            .cloned()
            .ok_or("no screen")?;
        let Some((desktop, _)) = self.desktop_for_project(&output) else {
            tracing::info!(project = id, "no free desktop: the project does not open");
            let name = project_name.clone();
            self.projects.no_room = Some(name.clone());
            self.tell_shell(ToShell::ProjectNoRoom {
                output: output.name(),
                name,
            });
            return Err("no free desktop".to_owned());
        };
        let screen = self.screen_key(&output.name());
        let project = self.projects.list.get_mut(at).ok_or("no such project")?;
        project.open = Some(OpenOn {
            screen: screen.clone(),
            desktop: desktop + 1,
        });
        project.opened = Some(now_text());
        let folder = project.folder.clone();
        let windows: Vec<SavedWindow> = std::mem::take(&mut project.contents)
            .into_iter()
            .map(|mut w| {
                w.screen.clone_from(&screen);
                w.desktop = desktop + 1;
                w.everywhere = false;
                w
            })
            .collect();
        tracing::info!(
            project = id,
            desktop = desktop + 1,
            windows = windows.len(),
            "a project reopens"
        );
        self.switch_desktop(&output, desktop, None);
        self.projects_changed();
        if windows.is_empty() {
            self.project_back(&id);
        } else {
            let comeback =
                self.comeback_start(Whose::Project(id), windows, Vec::new(), Some(folder));
            self.comeback_add(comeback);
        }
        Ok(desktop)
    }

    /// Screen `key` went, its windows to `target`, each on the
    /// same-numbered desktop: each of its projects goes with them, unless
    /// that desktop is a project already (it then waits for its screen,
    /// with its windows, §1).
    pub(crate) fn projects_screen_gone(&mut self, key: &str, target: &Output) {
        let to = self.screen_key(&target.name());
        let mut changed = false;
        let taken: Vec<usize> = self
            .projects
            .list
            .iter()
            .filter_map(|p| {
                p.open
                    .as_ref()
                    .filter(|o| o.screen == to)
                    .map(|o| o.desktop)
            })
            .collect();
        for project in &mut self.projects.list {
            let Some(open) = project.open.as_mut().filter(|o| o.screen == key) else {
                continue;
            };
            if taken.contains(&open.desktop) {
                tracing::info!(desktop = open.desktop, "a project waits for its screen");
                continue;
            }
            tracing::info!(
                desktop = open.desktop,
                "a project follows its windows to another screen"
            );
            open.screen.clone_from(&to);
            project.away_from = Some(key.to_owned());
            changed = true;
        }
        if changed {
            self.projects_changed();
        }
    }

    /// Screen `key` came back: the projects that followed their windows
    /// away go back with them.
    pub(crate) fn projects_screen_back(&mut self, key: &str) {
        let mut changed = false;
        for project in &mut self.projects.list {
            if project.away_from.as_deref() != Some(key) {
                continue;
            }
            project.away_from = None;
            if let Some(open) = project.open.as_mut() {
                tracing::info!(desktop = open.desktop, "a project goes back to its screen");
                open.screen = key.to_owned();
                changed = true;
            }
        }
        if changed {
            self.projects_changed();
        }
    }

    /// Open project `name`: its desktop shows, as a tile's click.
    pub(crate) fn project_go(&mut self, name: &str) {
        let Some(at) = self.project_at(name) else {
            return;
        };
        if let Some((output, desktop)) = self.project_output(at) {
            self.switch_desktop(&output, desktop, None);
        }
    }

    /// The launcher's folder row (§2.2): `folder` made a project on the
    /// desktop of `output` §7 chooses, named after its last part.
    pub(crate) fn project_new(&mut self, output: &str, folder: &str) {
        let Some(out) = self.space.outputs().find(|o| o.name() == output).cloned() else {
            return;
        };
        let path = PathBuf::from(folder);
        let base = path
            .file_name()
            .map_or_else(|| "projet".to_owned(), |n| n.to_string_lossy().into_owned());
        let base: String = base.chars().take(hocus_link::NAME_MOST - 3).collect();
        let taken = |n: &str| {
            let key = link::name_key(n);
            self.projects
                .list
                .iter()
                .any(|p| link::name_key(&p.name) == key)
        };
        let name = if taken(&base) {
            (2..)
                .map(|n| format!("{base} {n}"))
                .find(|n| !taken(n))
                .unwrap_or(base)
        } else {
            base
        };
        let Some((desktop, added)) = self.desktop_for_project(&out) else {
            self.tell_shell(ToShell::ProjectNoRoom {
                output: out.name(),
                name,
            });
            return;
        };
        let empty = added || self.desktop_free(&out, desktop);
        self.project_make(
            crate::projects::Asker::Launcher { empty },
            &out.name(),
            desktop,
            &name,
            folder,
        );
    }

    /// The launcher's project is made on desktop `desktop` of `output`: it
    /// shows, and, empty, a terminal opens in its folder (§13, B).
    pub(crate) fn project_made_from_launcher(
        &mut self,
        output: &str,
        desktop: usize,
        folder: &std::path::Path,
        empty: bool,
    ) {
        let out = self.space.outputs().find(|o| o.name() == output).cloned();
        if let Some(out) = out {
            self.switch_desktop(&out, desktop, None);
        }
        if empty {
            crate::programs::spawn_terminal_in(
                self.config.programs.terminal.clone(),
                folder.to_path_buf(),
                crate::programs::Env::of(self),
            );
        }
    }

    /// Everything of project `id` came back or was dropped: its file goes.
    pub(crate) fn project_back(&mut self, id: &str) {
        tracing::info!(project = id, "a project is back");
        self.projects_job(Job::Contents {
            id: id.to_owned(),
            text: None,
        });
        self.tell_projects();
    }

    /// What of project `id` did not come back (`missing`), and its folder
    /// if it is gone: one notification, its monogram as its icon.
    pub(crate) fn project_not_back(
        &mut self,
        id: &str,
        missing: Vec<Missing>,
        gone: Option<PathBuf>,
    ) {
        if missing.is_empty() && gone.is_none() {
            return;
        }
        let Some(project) = self.projects.list.iter().find(|p| p.id == id) else {
            return;
        };
        let lang = self.config.lang;
        let home = self
            .config
            .home
            .clone()
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_default();
        let (summary, body) = not_back_said(
            &project.name,
            &missing,
            gone.map(|g| link::tilde(&g.display().to_string(), &home.display().to_string()))
                .as_deref(),
            lang,
        );
        tracing::info!(
            project = id,
            apps = ?Missing::apps(&missing),
            missing = missing.len(),
            "a project came back but for some"
        );
        self.projects.not_back = Some((summary.clone(), body.clone()));
        let Some(bus) = self.shell.bus.clone() else {
            return;
        };
        crate::shell::notify_project(bus, link::monogram(&project.name), summary, body);
    }

    /// Log in with `[session] restore = false` (§9): the projects open as
    /// the session ended are closed, their windows of `read` (on their
    /// screens and desktops) their contents.
    pub(crate) fn projects_left_open(&mut self, read: &Saved) {
        let mut changed = false;
        let mut written = Vec::new();
        for project in &mut self.projects.list {
            let Some(on) = project.open.take() else {
                continue;
            };
            let contents: Vec<SavedWindow> = read
                .windows
                .iter()
                .filter(|w| w.screen == on.screen && w.desktop == on.desktop && !w.everywhere)
                .cloned()
                .collect();
            project.closed = Some(now_text());
            project.contents = contents.clone();
            let id = project.id.clone();
            tracing::info!(
                project = id,
                kept = contents.len(),
                "a project open at log out is closed"
            );
            let text = crate::projects::Contents { windows: contents }.text();
            written.push(Job::Contents {
                id,
                text: Some(text),
            });
            changed = true;
        }
        for job in written {
            self.projects_job(job);
        }
        if changed {
            self.projects_changed();
        }
    }
}

/// The notification of a project that came back but for some (§7): its
/// summary, "hocus-site est revenu, sauf Slack et 2 autres", and its body,
/// a sentence for what did not come back and one for its folder gone.
pub fn not_back_said(
    name: &str,
    missing: &[Missing],
    gone: Option<&str>,
    lang: hocus_words::Lang,
) -> (String, String) {
    const SHOWN: usize = 3;
    let and = crate::session::restore::and_word(lang);
    let apps = Missing::apps(missing);
    // What did not come back, each once, as the summary names it.
    let mut names: Vec<String> = apps.clone();
    for m in missing {
        let said = match m {
            Missing::App(_) => continue,
            Missing::Tabs(browser) => txt::their_tabs(lang, browser),
        };
        if !names.contains(&said) {
            names.push(said);
        }
    }
    let names = names.as_slice();
    let what = if names.is_empty() {
        txt::ITS_FOLDER.get(lang).to_owned()
    } else if names.len() > SHOWN {
        let shown = names
            .iter()
            .take(SHOWN)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        txt::and_others(
            lang,
            txt::AndOthers {
                shown: &shown,
                n: names.len() - SHOWN,
            },
        )
    } else {
        match names {
            [one] => one.clone(),
            [rest @ .., last] => format!("{}{and}{last}", rest.join(", ")),
            [] => String::new(),
        }
    };
    let summary = txt::back_but(lang, txt::BackBut { name, what: &what });
    let mut body = Vec::new();
    if !apps.is_empty() {
        body.push(crate::session::restore::not_back_said(&apps, lang));
    }
    body.extend(crate::session::restore::missing_said(missing, None, lang));
    if let Some(folder) = gone {
        body.push(txt::folder_gone(lang, folder));
    }
    (summary, body.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_did_not_come_back_is_named_three_at_most() {
        let names = |n: &[&str]| {
            n.iter()
                .map(|s| Missing::App((*s).to_owned()))
                .collect::<Vec<_>>()
        };
        let fr = hocus_words::Lang::French;
        let (summary, body) = not_back_said("hocus-site", &names(&["Slack"]), None, fr);
        assert_eq!(summary, "hocus-site est revenu, sauf Slack");
        assert_eq!(body, "Slack n’a pas pu être rouverte.");
        let (summary, _) = not_back_said(
            "hocus-site",
            &names(&["Slack", "Signal", "Zoom", "Gimp", "Inkscape"]),
            None,
            fr,
        );
        assert_eq!(
            summary,
            "hocus-site est revenu, sauf Slack, Signal, Zoom et 2 autres"
        );
        let (summary, body) =
            not_back_said("blog", &[], Some("~/code/blog"), hocus_words::Lang::English);
        assert_eq!(summary, "blog came back, but for its folder");
        assert_eq!(body, "The folder ~/code/blog no longer exists.");
        // A browser window without its tabs.
        let (summary, body) = not_back_said("site", &[Missing::Tabs("Firefox".into())], None, fr);
        assert_eq!(summary, "site est revenu, sauf les onglets de Firefox");
        assert_eq!(
            body,
            "Les onglets d’une fenêtre de Firefox n’ont pas pu être lus."
        );
    }

    #[test]
    fn a_saved_windows_place_is_its_state_on_its_screen() {
        let w = SavedWindow {
            screen_size: [2000, 1000],
            area: [0, 0, 2000, 1000],
            place: [100, 100, 400, 300],
            state: Shape::Left,
            ..SavedWindow::default()
        };
        let left = saved_rect(&w);
        assert!(
            left.x == 0 && left.width <= 1000 && left.width > 900,
            "{left:?}"
        );
        let floating = SavedWindow {
            state: Shape::Floating,
            ..w.clone()
        };
        assert_eq!(saved_rect(&floating), Rect::new(100, 100, 400, 300));
        let share = card_window(&w).rect[2];
        assert!(
            share <= hocus_link::SHARE / 2 && share > hocus_link::SHARE / 3,
            "{share}"
        );
        assert_eq!(tool_of("claude-code+herdr", "site"), "Claude Code");
        assert_eq!(tool_of("command", "build"), "build");
    }
}
