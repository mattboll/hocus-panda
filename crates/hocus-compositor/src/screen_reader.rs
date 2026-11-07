//! The screen reader with the session (docs/design/screen-reader.md, #734):
//! Orca, started as the session's program when `[accessibility]
//! screen-reader` says so, and started or stopped by `Super+Alt+S`, on the
//! lock screen too. Only when the compositor is the session (a virtual
//! terminal): nested, Orca would read the session it runs in.
//!
//! Orca runs as any program the compositor starts (its environment, an
//! activation token); a thread waits for it and says when it ended. Stopped,
//! it is sent `SIGTERM` by its own process id, then `SIGKILL` if it is
//! still there a moment later.

use std::{path::PathBuf, process::Stdio, time::Duration};

use smithay::reexports::calloop::{
    LoopHandle,
    channel::{self, Event, Sender},
    timer::{TimeoutAction, Timer},
};

use crate::{App, State, programs};

/// How long Orca has to end after `SIGTERM`.
const GRACE: Duration = Duration::from_secs(2);

/// The screen reader, as the compositor keeps it.
#[derive(Debug, Default)]
pub struct ScreenReader {
    /// The compositor is the session: Orca may be started.
    pub allowed: bool,
    /// Its program (`orca`, or a test's).
    pub program: Option<PathBuf>,
    /// Its process while it runs.
    running: Option<u32>,
    /// Where its thread says it ended.
    ended: Option<Sender<u32>>,
}

impl ScreenReader {
    pub fn running(&self) -> Option<u32> {
        self.running
    }
}

/// Hears Orca end; starts it if the configuration says so.
pub fn listen(handle: &LoopHandle<'static, App>, state: &mut State) -> anyhow::Result<()> {
    let (ended, heard) = channel::channel::<u32>();
    handle
        .insert_source(heard, |event, _, app| {
            if let Event::Msg(pid) = event {
                app.state.screen_reader_ended(pid);
            }
        })
        .map_err(|err| anyhow::anyhow!("could not hear the screen reader: {}", err.error))?;
    state.screen_reader.ended = Some(ended);
    if state.config.accessibility.screen_reader {
        state.screen_reader_on();
    }
    Ok(())
}

impl State {
    /// `Super+Alt+S`: the screen reader started, or stopped.
    pub(crate) fn toggle_screen_reader(&mut self) {
        if self.screen_reader.running.is_some() {
            self.screen_reader_off();
        } else {
            self.screen_reader_on();
        }
    }

    pub(crate) fn screen_reader_on(&mut self) {
        if !self.screen_reader.allowed {
            tracing::info!("the screen reader starts only with the session, not nested");
            return;
        }
        if self.screen_reader.running.is_some() {
            return;
        }
        let program = self
            .screen_reader
            .program
            .clone()
            .unwrap_or_else(|| PathBuf::from("orca"));
        let env = programs::Env::of(self).with_activation(self.activation_token());
        let mut command = env.command(&program);
        command
            .arg("--replace")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                tracing::warn!(%err, program = %program.display(), "the screen reader did not start");
                return;
            }
        };
        let pid = child.id();
        tracing::info!(pid, program = %program.display(), "the screen reader started");
        self.screen_reader.running = Some(pid);
        let ended = self.screen_reader.ended.clone();
        programs::on_a_thread("screen reader".to_owned(), move || {
            let _ = child.wait();
            if let Some(ended) = ended {
                let _ = ended.send(pid);
            }
        });
    }

    pub(crate) fn screen_reader_off(&mut self) {
        let Some(pid) = self.screen_reader.running else {
            return;
        };
        tracing::info!(pid, "the screen reader is stopped");
        signal(pid, rustix::process::Signal::TERM);
        // Still there after a while (SIGTERM held back): ended for good.
        let timer = Timer::from_duration(GRACE);
        let kept = self.loop_handle.insert_source(timer, move |_, _, app| {
            if app.state.screen_reader.running == Some(pid) {
                tracing::warn!(pid, "the screen reader did not end: killed");
                signal(pid, rustix::process::Signal::KILL);
            }
            TimeoutAction::Drop
        });
        if let Err(err) = kept {
            tracing::warn!(%err, "no timer for the screen reader's end");
        }
    }

    fn screen_reader_ended(&mut self, pid: u32) {
        if self.screen_reader.running == Some(pid) {
            tracing::info!(pid, "the screen reader ended");
            self.screen_reader.running = None;
        }
    }

    /// `[accessibility]` changed: the screen reader started or stopped as
    /// it now says. Whether it changed.
    pub(crate) fn accessibility_changed(&mut self, new: &crate::config::Accessibility) -> bool {
        if *new == self.config.accessibility {
            return false;
        }
        self.config.accessibility = new.clone();
        if new.screen_reader {
            self.screen_reader_on();
        } else {
            self.screen_reader_off();
        }
        true
    }
}

/// Signal `signal` to process `pid`, one the compositor started.
fn signal(pid: u32, signal: rustix::process::Signal) {
    let Some(pid) = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    else {
        return;
    };
    if let Err(err) = rustix::process::kill_process(pid, signal) {
        tracing::debug!(%err, "the screen reader is gone already");
    }
}
