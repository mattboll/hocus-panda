//! A closed project's window that a browser brings back by itself at log
//! in (docs/design/projects.md §8.3, #915). A project closed shortly
//! before log out: Firefox, ended by the log out's `SIGTERM` before it
//! wrote that the window closed (it writes every 15 s, Mozilla's bug
//! 2086801), reopens it with the session's own windows. Reopening the
//! project then opens it again: the window would be there twice.
//!
//! While a browser reopening its own windows at log in starts ([`WATCH`]
//! from its start), each of its windows that no saved place took is
//! looked at as it maps and as its title changes: one that shows the tab
//! a closed project's window showed, and that none of the last session's
//! windows of that browser showed, is asked to close, as its ✕ would
//! ([`crate::browsers::ghost_of`]). Unsure, it stays: a window lost is
//! worse than one twice.

use std::time::Duration;

use smithay::reexports::{
    calloop::{
        RegistrationToken,
        timer::{TimeoutAction, Timer},
    },
    wayland_server::protocol::wl_surface::WlSurface,
};

use crate::{
    State,
    browsers::{self, Browser},
    model::WindowId,
};

use super::restore::SavedWindow;

/// How long after a browser reopening its own windows started its windows
/// are looked at: as long as the session's windows are waited for.
const WATCH: Duration = super::restore::PATIENCE;

/// The log in's watch for those windows.
#[derive(Debug, Default)]
pub struct Ghosts {
    phase: Phase,
}

#[derive(Debug, Default)]
enum Phase {
    /// No session coming back with a browser window.
    #[default]
    Off,
    /// The last session's windows come back, a browser's among them; none
    /// of its browsers started yet.
    Armed(Watch),
    /// Browsers reopening their own windows started: each watched until
    /// its timer.
    Watching(Watch),
    Over,
}

#[derive(Debug, Default)]
struct Watch {
    /// The tab each of the last session's browser windows showed (none:
    /// not known), by its browser: a window showing one is the session's.
    session: Vec<(Browser, Option<String>)>,
    /// The browsers reopening their own windows, each with its timer.
    browsers: Vec<(Browser, Option<RegistrationToken>)>,
    /// What became of the windows already decided.
    windows: Vec<(WindowId, Fate)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fate {
    /// A saved place took it: the session's or a project's, never closed.
    Placed,
    /// Asked to close, once.
    Closed,
}

impl Watch {
    fn decided(&self, id: WindowId) -> bool {
        self.windows.iter().any(|(w, _)| *w == id)
    }
}

impl State {
    /// The last session's `windows` come back: its browser windows' tabs
    /// are kept for the watch (none with no browser window).
    pub(crate) fn ghosts_arm(&mut self, windows: &[SavedWindow]) {
        let session: Vec<(Browser, Option<String>)> = windows
            .iter()
            .filter_map(|w| {
                let browser = browsers::of_app(&w.app_id).or_else(|| browsers::of_app(&w.entry))?;
                let shown = w
                    .browser
                    .as_ref()
                    .filter(|b| b.read())
                    .and_then(|b| b.shown_title())
                    .map(str::to_owned);
                Some((browser, shown))
            })
            .collect();
        self.session_restore.ghosts.phase = if session.is_empty() {
            Phase::Off
        } else {
            Phase::Armed(Watch {
                session,
                ..Watch::default()
            })
        };
    }

    /// `browser` starts at log in and reopens its own windows: its windows
    /// are watched for [`WATCH`].
    pub(crate) fn ghosts_watch(&mut self, browser: Browser) {
        let mut watch = match std::mem::take(&mut self.session_restore.ghosts.phase) {
            Phase::Armed(w) | Phase::Watching(w) => w,
            other => {
                self.session_restore.ghosts.phase = other;
                return;
            }
        };
        if !watch.browsers.iter().any(|(b, _)| *b == browser) {
            let token = self
                .loop_handle
                .insert_source(Timer::from_duration(WATCH), move |_, _, app| {
                    app.state.ghosts_watched(browser);
                    TimeoutAction::Drop
                })
                .map_err(|err| tracing::warn!(%err.error, "a browser's windows are not watched"))
                .ok();
            if token.is_some() {
                watch.browsers.push((browser, token));
            }
        }
        self.session_restore.ghosts.phase = if watch.browsers.is_empty() {
            Phase::Armed(watch)
        } else {
            Phase::Watching(watch)
        };
    }

    /// [`WATCH`] after `browser` started: its windows are no longer looked
    /// at; none left, the watch is over.
    fn ghosts_watched(&mut self, browser: Browser) {
        let Phase::Watching(watch) = &mut self.session_restore.ghosts.phase else {
            return;
        };
        watch.browsers.retain(|(b, _)| *b != browser);
        if watch.browsers.is_empty() {
            tracing::debug!("the browsers' windows are no longer watched");
            self.session_restore.ghosts.phase = Phase::Over;
        }
    }

    /// The last session is back: armed, no browser reopening its own
    /// windows started, nothing is watched.
    pub(crate) fn ghosts_disarm(&mut self) {
        if matches!(self.session_restore.ghosts.phase, Phase::Armed(_)) {
            self.session_restore.ghosts.phase = Phase::Over;
        }
    }

    /// A saved place took window `id`: it is never closed.
    pub(crate) fn ghosts_placed(&mut self, id: WindowId) {
        if let Phase::Armed(watch) | Phase::Watching(watch) = &mut self.session_restore.ghosts.phase
            && !watch.decided(id)
        {
            watch.windows.push((id, Fate::Placed));
        }
    }

    /// Whether window `id` (mapped, or its title changed) is a closed
    /// project's that its browser reopened by itself: then it is asked to
    /// close.
    pub(crate) fn ghost_closed(&mut self, id: WindowId) -> bool {
        let Phase::Watching(watch) = &self.session_restore.ghosts.phase else {
            return false;
        };
        if watch.decided(id) || self.model.parent(id).is_some() {
            return false;
        }
        let Some(app) = self
            .windows
            .get(&id)
            .and_then(|m| crate::windows::app_id(&m.window))
        else {
            return false;
        };
        let Some(browser) = browsers::of_app(&app) else {
            return false;
        };
        if !watch.browsers.iter().any(|(b, _)| *b == browser) {
            return false;
        }
        let session: Vec<Option<&str>> = watch
            .session
            .iter()
            .filter(|(b, _)| *b == browser)
            .map(|(_, t)| t.as_deref())
            .collect();
        let kept = self
            .projects
            .list
            .iter()
            .filter(|p| p.open.is_none())
            .flat_map(|p| {
                p.contents
                    .iter()
                    .filter(|w| {
                        browsers::of_app(&w.app_id).or_else(|| browsers::of_app(&w.entry))
                            == Some(browser)
                    })
                    .filter_map(move |w| Some((p.id.as_str(), w.browser.as_ref()?)))
            });
        let title = crate::windows::title_of(self, id);
        let Some(project) = browsers::ghost_of(&title, &session, kept).map(str::to_owned) else {
            return false;
        };
        if let Phase::Watching(watch) = &mut self.session_restore.ghosts.phase {
            watch.windows.push((id, Fate::Closed));
        }
        tracing::info!(
            project,
            app,
            "a closed project's window that the browser reopened is closed"
        );
        self.close_window(id);
        true
    }

    /// The title of `surface`'s window changed: while browsers are
    /// watched, it may be a closed project's (a page's title comes after
    /// its window).
    pub(crate) fn ghost_titled(&mut self, surface: &WlSurface) {
        if !matches!(self.session_restore.ghosts.phase, Phase::Watching(_)) {
            return;
        }
        let id = self
            .window_for(surface)
            .and_then(|window| self.id_of(&window));
        if let Some(id) = id {
            self.ghost_closed(id);
        }
    }
}
