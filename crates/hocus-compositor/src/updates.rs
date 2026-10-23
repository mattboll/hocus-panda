//! System updates (#84): looked for on a worker thread every few hours
//! (never while a window is fullscreen or on the power saver), shown in the
//! column and the settings, installed through polkit on request. Only the
//! session itself asks the system's package manager; elsewhere (nested,
//! tests) only commands of the configuration's own run.

use std::time::Duration;

use hocus_api::{Event, UpdateInfo, UpdatesInfo};
use hocus_platform::updates::{self as platform, Manager, Update};
use smithay::reexports::calloop::{
    LoopHandle, channel,
    timer::{TimeoutAction, Timer},
};

use crate::{App, State};

/// The first look, once the session has settled.
const FIRST_LOOK: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Phase {
    #[default]
    Idle,
    Checking,
    Installing,
    /// No package manager known.
    Unsupported,
}

#[derive(Debug)]
enum News {
    Checked(Option<Manager>, Result<Vec<Update>, String>),
    Installed(Result<(), String>),
}

#[derive(Debug, Default)]
pub struct Updates {
    phase: Phase,
    list: Vec<Update>,
    error: Option<String>,
    manager: Option<Manager>,
    next_look: Option<Duration>,
    news: Option<channel::Sender<News>>,
}

pub fn start(handle: &LoopHandle<'static, App>, state: &mut State) {
    let (news, received) = channel::channel::<News>();
    let inserted = handle.insert_source(received, |event, _, app| {
        if let channel::Event::Msg(news) = event {
            app.timed("updates", |app| app.state.updates_news(news));
        }
    });
    if let Err(err) = inserted {
        tracing::warn!(%err.error, "no updates");
        return;
    }
    state.updates.news = Some(news);
    state.updates.next_look = Some(state.clock.now() + FIRST_LOOK);
    // Wakes the loop now and then: looks fall due on the clock.
    if state.clock.is_virtual() {
        return;
    }
    let ticking = handle.insert_source(Timer::from_duration(FIRST_LOOK), |_, _, app| {
        app.state.needs_redraw = true;
        TimeoutAction::ToDuration(Duration::from_secs(600))
    });
    if let Err(err) = ticking {
        tracing::warn!(%err.error, "updates will be looked for only when something happens");
    }
}

impl State {
    /// What to ask: the configuration's commands, else (in the session)
    /// the system's package manager, found on the worker.
    fn custom_manager(&self) -> Option<Manager> {
        let u = &self.config.updates;
        (!u.check.is_empty()).then(|| Manager::Custom {
            check: u.check.clone(),
            install: u.install.clone(),
        })
    }

    /// After each dispatch: a look falls due.
    pub fn refresh_updates(&mut self) {
        let now = self.clock.now();
        let due = self.updates.next_look.is_some_and(|t| now >= t);
        if !due || self.updates.phase != Phase::Idle || !self.config.updates.enabled {
            return;
        }
        // Never in the way: later.
        let fullscreen = self.space.outputs().any(|o| self.fullscreen_on(o));
        if fullscreen || self.power_saver() {
            self.updates.next_look = Some(now + Duration::from_secs(600));
            return;
        }
        let every = Duration::from_secs(u64::from(self.config.updates.every_hours) * 3600);
        self.updates.next_look = Some(now + every);
        let custom = self.custom_manager();
        if custom.is_none() && !self.is_session {
            return;
        }
        let Some(news) = self.updates.news.clone() else {
            return;
        };
        self.updates.phase = Phase::Checking;
        spawn("updates check", move || {
            let manager = custom.or_else(platform::detect);
            let result = match &manager {
                Some(m) => platform::check(m),
                None => Ok(Vec::new()),
            };
            let _ = news.send(News::Checked(manager, result));
        });
    }

    /// The column's Install, or the settings'.
    pub fn install_updates(&mut self) {
        if self.updates.phase != Phase::Idle || self.updates.list.is_empty() {
            return;
        }
        let (Some(manager), Some(news)) = (self.updates.manager.clone(), self.updates.news.clone())
        else {
            return;
        };
        self.updates.phase = Phase::Installing;
        self.updates.error = None;
        self.updates_changed();
        spawn("updates install", move || {
            let _ = news.send(News::Installed(platform::install(&manager)));
        });
    }

    fn updates_news(&mut self, news: News) {
        let lang = self.shell_lang();
        match news {
            News::Checked(manager, result) => {
                self.updates.phase = if manager.is_some() {
                    Phase::Idle
                } else {
                    Phase::Unsupported
                };
                self.updates.manager = manager;
                match result {
                    Ok(list) => {
                        tracing::info!(count = list.len(), "updates looked for");
                        self.updates.list = list;
                        self.updates.error = None;
                    }
                    Err(err) => {
                        tracing::warn!(err, "updates could not be looked for");
                        self.updates.error = Some(err);
                    }
                }
            }
            News::Installed(Ok(())) => {
                self.updates.phase = Phase::Idle;
                self.updates.list.clear();
                self.shell_toast(lang.updates_installed(), "");
                // A look again, for what came meanwhile.
                self.updates.next_look = Some(self.clock.now());
            }
            News::Installed(Err(err)) => {
                self.updates.phase = Phase::Idle;
                self.shell_toast(lang.updates_failed(), &err);
                self.updates.error = Some(err);
            }
        }
        self.updates_changed();
    }

    fn updates_changed(&mut self) {
        let (count, state) = (self.updates.list.len(), self.updates_state());
        self.emit(|_| Event::Updates {
            count,
            state: state.into(),
        });
        self.needs_redraw = true;
    }

    fn updates_state(&self) -> &'static str {
        match self.updates.phase {
            Phase::Idle => "idle",
            Phase::Checking => "checking",
            Phase::Installing => "installing",
            Phase::Unsupported => "unsupported",
        }
    }

    pub fn updates_info(&self) -> UpdatesInfo {
        UpdatesInfo {
            updates: self
                .updates
                .list
                .iter()
                .map(|u| UpdateInfo {
                    name: u.name.clone(),
                    from: u.from.clone(),
                    to: u.to.clone(),
                })
                .collect(),
            state: self.updates_state().into(),
            error: self.updates.error.clone(),
        }
    }

    /// The column's line: how many, and whether they are being installed.
    pub fn updates_line(&self) -> Option<(usize, bool)> {
        let installing = self.updates.phase == Phase::Installing;
        // Turned off, the line goes with the looking; an installation
        // under way still shows.
        (installing || (self.config.updates.enabled && !self.updates.list.is_empty()))
            .then_some((self.updates.list.len(), installing))
    }
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(err) = std::thread::Builder::new().name(name.into()).spawn(work) {
        tracing::warn!(%err, "could not start a thread");
    }
}
