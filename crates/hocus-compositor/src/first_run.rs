//! The first session (#82): a moment after the desktop is up, the
//! settings open on their welcome, once. A mark in the state directory
//! says it was offered, whether the user went through it or closed it.

use std::{path::PathBuf, time::Duration};

use smithay::reexports::calloop::{
    LoopHandle,
    timer::{TimeoutAction, Timer},
};

use crate::{App, State};

/// After the desktop shows, so that it opens on a session that is there.
const AFTER: Duration = Duration::from_millis(1500);
const MARK: &str = "welcomed";

/// When to open the welcome, and the mark to leave.
#[derive(Debug, Default)]
pub struct FirstRun {
    due: Option<(Duration, PathBuf)>,
}

/// At start: the first session, when the state directory has no mark.
pub fn start(handle: &LoopHandle<'static, App>, state: &mut State, dir: Option<PathBuf>) {
    let Some(dir) = dir else {
        return;
    };
    let mark = dir.join(MARK);
    // At start, before the loop runs: a look at one file.
    if mark.exists() {
        return;
    }
    state.first_run.due = Some((state.clock.now() + AFTER, mark));
    let woken = handle.insert_source(Timer::from_duration(AFTER), |_, _, app| {
        app.state.needs_redraw = true;
        TimeoutAction::Drop
    });
    if let Err(err) = woken {
        tracing::debug!(%err.error, "the welcome will open on the next event");
    }
}

impl State {
    /// After each dispatch: the welcome opens when it is time, and the
    /// mark is left off the loop.
    pub fn refresh_first_run(&mut self) {
        let now = self.clock.now();
        let Some((at, _)) = &self.first_run.due else {
            return;
        };
        if now < *at {
            return;
        }
        let Some((_, mark)) = self.first_run.due.take() else {
            return;
        };
        tracing::info!("first session: opening the welcome");
        self.open_settings(Some("welcome".into()));
        let written = std::thread::Builder::new()
            .name("first-run".into())
            .spawn(move || {
                let done = mark
                    .parent()
                    .map_or(Ok(()), std::fs::create_dir_all)
                    .and_then(|()| std::fs::write(&mark, b"1\n"));
                if let Err(err) = done {
                    tracing::warn!(%err, path = %mark.display(), "the welcome may open again");
                }
            });
        if let Err(err) = written {
            tracing::warn!(%err, "the welcome may open again");
        }
    }
}
