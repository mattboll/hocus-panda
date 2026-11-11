//! A day's CPU and memory (#655, docs/performance.md, "A day's view").
//!
//! The loop only increments [`hocus_perf::Counters`]' atomics (a turn, a
//! frame, a scene, an input, an application's commit) and pushes an event
//! now and then (a window opened, a screen plugged, the lock); the
//! recorder's thread reads them every 10 s and never wakes the loop. Off
//! with `[perf] record = false`, or without a state folder (tests, unless
//! they give one).
//!
//! A signal raised sends a low-urgency notification (to the history, no
//! bubble), once a day per kind; clicked there, it opens the Settings'
//! Performances page, the one thing that comes back to the loop.

use std::{sync::Arc, time::Duration};

use hocus_perf::{Counters, EventKind, Recorder, Setup};
use smithay::{
    output::Output,
    reexports::calloop::channel::{self, Event, Sender},
};

use crate::State;

/// How often a sample is taken.
const EVERY: Duration = Duration::from_secs(10);

/// What the loop counts, and the recorder while it runs.
#[derive(Debug)]
pub struct Perf {
    pub counters: Arc<Counters>,
    recorder: Option<Recorder>,
    /// A signal's notification clicked: the Performances page.
    open: Option<Sender<()>>,
}

impl Default for Perf {
    fn default() -> Self {
        Self {
            counters: Counters::new(),
            recorder: None,
            open: None,
        }
    }
}

/// A screen's slot among the counters, kept with it.
struct Slot(Option<usize>);

impl Perf {
    /// Something happened: written with the next sample.
    pub fn event(&self, kind: EventKind, what: impl Into<String>) {
        self.counters.event(kind, what);
    }

    /// `output`'s slot, taken the first time it draws.
    fn slot(&self, output: &Output) -> Option<usize> {
        let data = output.user_data();
        data.insert_if_missing_threadsafe(|| Slot(self.counters.screen(&output.name())));
        data.get::<Slot>().and_then(|s| s.0)
    }

    pub fn recording(&self) -> bool {
        self.recorder.is_some()
    }
}

impl State {
    /// The recorder started or stopped as `[perf] record` says.
    pub(crate) fn perf_follow(&mut self) {
        let dir = self
            .config
            .state_dir
            .as_deref()
            .map(hocus_perf::files::dir_in);
        match (self.config.perf.record, dir, self.perf.recorder.is_some()) {
            (true, Some(dir), false) => self.perf_start(dir),
            (false, _, true) => {
                tracing::info!("the day is no longer recorded");
                self.perf.recorder = None;
            }
            _ => {}
        }
    }

    fn perf_start(&mut self, dir: std::path::PathBuf) {
        let open = match &self.perf.open {
            Some(open) => open.clone(),
            None => {
                let (open, opened) = channel::channel::<()>();
                let inserted = self.loop_handle.insert_source(opened, |event, (), app| {
                    if let Event::Msg(()) = event {
                        app.state.perf_open_page();
                    }
                });
                if let Err(err) = inserted {
                    tracing::warn!(%err.error, "a signal's notification will open nothing");
                }
                self.perf.open = Some(open.clone());
                open
            }
        };
        let running = self.shell.running.clone();
        let bus = self.shell.bus.clone();
        let lang = self.config.lang;
        let supplies = std::env::var_os("HOCUS_POWER_SUPPLY_DIR")
            .map_or_else(|| "/sys/class/power_supply".into(), Into::into);
        let setup = Setup {
            dir,
            interval: self.config.perf.interval.unwrap_or(EVERY),
            counters: self.perf.counters.clone(),
            shell: Box::new(move || running.pid()),
            supplies,
            raised: Box::new(move |signal| {
                let tz = jiff::tz::TimeZone::system();
                let summary = hocus_perf::text::notification(&signal, lang, &tz);
                let Some(bus) = bus.clone() else {
                    tracing::info!(summary, "a signal of the performance, no bus to say it on");
                    return;
                };
                let open = open.clone();
                crate::shell::notify_quietly(bus, "utilities-system-monitor", summary, move || {
                    let _ = open.send(());
                });
            }),
            tz: jiff::tz::TimeZone::system(),
        };
        match Recorder::start(setup) {
            Ok(recorder) => self.perf.recorder = Some(recorder),
            Err(err) => tracing::warn!(%err, "the day is not recorded"),
        }
    }

    /// A signal's notification clicked: the Settings on Performances.
    fn perf_open_page(&mut self) {
        tracing::info!("a signal's notification opens the Performances page");
        self.launch(hocus_link::Launch {
            id: "hocus-settings".to_owned(),
            terminal: false,
            dir: String::new(),
            argv: vec!["hocus-settings".to_owned(), "performance".to_owned()],
        });
    }

    /// A frame of `output` on screen, while a motion ran or not.
    pub(crate) fn perf_frame(&self, output: &Output) {
        self.perf
            .counters
            .frame(self.perf.slot(output), !self.frames.still);
    }

    /// A scene of `output` built and drawn (or found unchanged), in
    /// `took`.
    pub(crate) fn perf_scene(&self, output: &Output, took: Duration) {
        self.perf.counters.scene(self.perf.slot(output), took);
    }
}
