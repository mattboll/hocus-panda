//! Screen recording (5.10, docs/design/screenshots.md, "5.10"): an area, a
//! window or a screen, with the computer's sound and the microphone, to an
//! MP4 file (H.264 and AAC) at 30 frames a second.
//!
//! - **Starting**: the capture's toolbar in Vidéo (5.8's, with its Photo /
//!   Vidéo switch, Son and Micro) and its red "Démarrer", or
//!   `Super+Shift+R` with the last settings: a countdown of 3 (the
//!   capture's, with a red ring), then the recording.
//! - **While recording**: the frames are the compositor's own rendering,
//!   drawn as the screen shows them but for what only the screen shows
//!   (the capture's UI, the recording's own frame, the bar's item), lent
//!   to the recorder's thread without the loop copying them ([`frames`],
//!   as the screen cast's are); the recorder makes the file with ffmpeg
//!   ([`encode`]) and the sound from PipeWire ([`audio`]). The recorded
//!   area keeps a red dashed frame on the screen ([`draw`]); the shell
//!   shows the bar's item (`recording`), whose buttons pause, go on and
//!   stop (`record pause|resume|stop`).
//! - **Stopping**: from the bar, `Super+Shift+R`, the screen locking (the
//!   session leaving its terminal), the screen unplugged or the window
//!   closed: the file is ended, and the shell shows its thumbnail
//!   (`recorded`), whose ✕ deletes it and whose drag drops it.
//! - One at a time.

pub mod audio;
mod draw;
pub mod encode;
pub mod frames;
pub mod save;

use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, SystemTime},
};

use hocus_link::{RecordAsk, Recorded, RecordingShown, ToShell};
use smithay::reexports::calloop::{
    RegistrationToken,
    channel::{self, Event, Sender},
    timer::{TimeoutAction, Timer},
};

use crate::{State, capture::Area, model::WindowId, programs};
use encode::{FromRecorder, Job, RecordSender, Saved, ToRecorder};

/// The frames' buffers of a recording are keyed apart from the shares'.
const KEY: u64 = 1 << 62;

/// The key a recording's frames are drawn under.
pub fn frame_key(id: u64) -> u64 {
    KEY | id
}

/// What is recorded.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// An area of screen `output`, logical pixels of it.
    Area { output: String, area: Area },
    /// A window, by the compositor's id.
    Window(u64),
    /// A screen, by name.
    Screen(String),
}

/// A recording chosen: what, with the sound and the microphone or not.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub source: Source,
    pub sound: bool,
    pub micro: bool,
}

/// The recording under way.
#[derive(Debug)]
pub struct Live {
    pub id: u64,
    pub source: Source,
    /// Its frames' size, pixels, even.
    pub size: (u32, u32),
    pub sender: RecordSender,
    pub paused: bool,
    /// ffmpeg runs: frames are drawn.
    pub started: bool,
    /// When its last frame was drawn; a frame is owed whatever changed.
    pub last: Option<Duration>,
    pub fresh: bool,
    /// The screen it is on, where its thumbnail shows.
    output: String,
    /// How long it recorded before it last went on, and since when it
    /// goes on (none while paused).
    before: Duration,
    since: Option<Duration>,
}

impl Live {
    /// How long it has recorded at `now`, the pauses left out.
    fn elapsed(&self, now: Duration) -> Duration {
        self.before + self.since.map_or(Duration::ZERO, |s| now.saturating_sub(s))
    }
}

/// A recording stopped, its file being ended.
#[derive(Debug)]
struct Ending {
    id: u64,
    output: String,
    copies: u64,
}

/// What a test can know of a recording.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordInfo {
    pub id: u64,
    pub source: Source,
    pub size: (u32, u32),
    pub paused: bool,
    pub started: bool,
    pub sound: bool,
    pub micro: bool,
}

/// Screen recording, owned by the loop.
#[derive(Debug, Default)]
pub struct Recording {
    live: Option<Live>,
    plan: Option<Plan>,
    ending: Vec<Ending>,
    next: u64,
    back: Option<Sender<FromRecorder>>,
    /// A frame is owed: drawn at the backend's next turn.
    pub(crate) due: bool,
    pace: Option<RegistrationToken>,
    /// The files saved, by recording.
    saved: HashMap<u64, PathBuf>,
    pub(crate) drawn: draw::Drawn,
    /// Frames handed over, the longest a hand-over took the loop, the
    /// bytes of frames read on the loop (recordings ended), and what was
    /// saved last: for tests.
    frames_sent: u64,
    lend_most: Duration,
    copied: u64,
    last: Option<Saved>,
    failed: Vec<String>,
}

impl Recording {
    pub fn live(&self) -> Option<&Live> {
        self.live.as_ref()
    }

    /// Whether one records (or is being started).
    pub fn recording(&self) -> bool {
        self.live.is_some()
    }

    /// The key its frames are drawn under, while one records.
    pub fn frames_key(&self) -> Option<u64> {
        self.live.as_ref().map(|l| frame_key(l.id))
    }

    pub(crate) fn owe(&mut self) {
        if let Some(live) = &mut self.live {
            live.fresh = true;
        }
    }

    pub fn info(&self) -> Option<RecordInfo> {
        let live = self.live.as_ref()?;
        let plan = self.plan.as_ref();
        Some(RecordInfo {
            id: live.id,
            source: live.source.clone(),
            size: live.size,
            paused: live.paused,
            started: live.started,
            sound: plan.is_some_and(|p| p.sound),
            micro: plan.is_some_and(|p| p.micro),
        })
    }

    pub fn frames_sent(&self) -> u64 {
        self.frames_sent
    }

    pub fn lend_most(&self) -> Duration {
        self.lend_most
    }

    /// The bytes of recorded frames read on the loop's thread so far: a
    /// budget of 0.
    pub fn copied_on_loop(&self) -> u64 {
        self.copied
            + self.ending.iter().map(|e| e.copies).sum::<u64>()
            + self.live.as_ref().map_or(0, |l| l.sender.copied_on_loop())
    }

    /// The last recording saved, and why the last ones failed.
    pub fn last_saved(&self) -> Option<&Saved> {
        self.last.as_ref()
    }

    pub fn failures(&self) -> &[String] {
        &self.failed
    }

    /// Whether recordings are being ended.
    pub fn ending(&self) -> bool {
        !self.ending.is_empty()
    }
}

impl State {
    /// The screen `source` is on.
    pub(crate) fn record_output(&self, source: &Source) -> Option<String> {
        match source {
            Source::Area { output, .. } | Source::Screen(output) => Some(output.clone()),
            Source::Window(id) => self.window_output(WindowId(*id)).map(|o| o.name()),
        }
    }

    /// The size of what `source` records, pixels, even.
    fn record_size(&self, source: &Source) -> Option<(u32, u32)> {
        let scale_of = |name: &str| {
            self.space
                .outputs()
                .find(|o| o.name() == name)
                .map(|o| o.current_scale().fractional_scale())
        };
        let size = match source {
            Source::Screen(name) => {
                let o = self.space.outputs().find(|o| o.name() == *name)?;
                let mode = o.current_mode()?;
                let size = o.current_transform().transform_size(mode.size);
                (size.w.unsigned_abs(), size.h.unsigned_abs())
            }
            Source::Area { output, area } => {
                let scale = scale_of(output)?;
                (
                    (area.w * scale).round().max(1.0) as u32,
                    (area.h * scale).round().max(1.0) as u32,
                )
            }
            Source::Window(id) => {
                let rect = self.rect_of(WindowId(*id))?;
                let output = self.window_output(WindowId(*id))?;
                let scale = output.current_scale().fractional_scale();
                (
                    (f64::from(rect.width) * scale).round().max(1.0) as u32,
                    (f64::from(rect.height) * scale).round().max(1.0) as u32,
                )
            }
        };
        Some(save::even(size))
    }

    /// `Super+Shift+R`: a recording stops; a countdown to one is
    /// cancelled; else one starts with the last settings (the capture's
    /// UI goes if it shows), once counted down.
    pub fn record_shortcut(&mut self) {
        if self.record.recording() {
            tracing::info!("the recording is stopped by its shortcut");
            self.record_stop();
            return;
        }
        if self.counting_to_record() {
            tracing::info!("the countdown to a recording is cancelled");
            self.capture_away();
            return;
        }
        let Some(plan) = self.record_plan(None) else {
            tracing::info!("nothing to record");
            return;
        };
        if self.capture_open() {
            self.capture_away();
        }
        self.record_count_down(plan);
        self.redraw = true;
    }

    /// The countdown ended: `plan` records.
    pub(crate) fn record_start(&mut self, plan: Plan) {
        if self.record.live.is_some() {
            tracing::info!("a recording is under way: one at a time");
            return;
        }
        let (Some(size), Some(output)) = (
            self.record_size(&plan.source),
            self.record_output(&plan.source),
        ) else {
            tracing::info!(?plan.source, "what was chosen to record is gone");
            return;
        };
        let Some(back) = self.recorder_back() else {
            return;
        };
        self.record.next += 1;
        let id = self.record.next;
        let job = Job {
            id,
            size,
            format: self.config.recording.format,
            sound: plan.sound,
            micro: plan.micro,
            // The GPU the compositor draws on, if it does: ffmpeg may
            // encode there (VA-API).
            gpu: self.gpu.and_then(|dev| {
                smithay::backend::drm::DrmNode::from_dev_id(dev)
                    .ok()?
                    .node_with_type(smithay::backend::drm::NodeType::Render)?
                    .ok()?
                    .dev_path()
            }),
            pipewire: self.config.pipewire_remote.clone(),
            videos: self.config.videos.clone(),
            french: self.config.french,
            when: SystemTime::now(),
            runtime: self.runtime_dir.clone(),
            ffmpeg: if self.config.ffmpeg.as_os_str().is_empty() {
                PathBuf::from("ffmpeg")
            } else {
                self.config.ffmpeg.clone()
            },
        };
        let sender = match encode::start(job, back) {
            Ok(sender) => sender,
            Err(err) => {
                tracing::warn!(%err, "the recorder does not start");
                self.record.failed.push(format!("{err:#}"));
                return;
            }
        };
        tracing::info!(
            id,
            source = ?plan.source,
            width = size.0,
            height = size.1,
            sound = plan.sound,
            micro = plan.micro,
            "a recording starts"
        );
        self.record.live = Some(Live {
            id,
            source: plan.source.clone(),
            size,
            sender,
            paused: false,
            started: false,
            last: None,
            fresh: true,
            output,
            before: Duration::ZERO,
            since: Some(self.now()),
        });
        self.record.plan = Some(plan);
        self.tell_recording();
        self.redraw = true;
    }

    /// Where the recorders say what happens, made once.
    fn recorder_back(&mut self) -> Option<Sender<FromRecorder>> {
        if let Some(back) = &self.record.back {
            return Some(back.clone());
        }
        let (sender, receiver) = channel::channel::<FromRecorder>();
        let inserted = self.loop_handle.insert_source(receiver, |event, _, app| {
            if let Event::Msg(said) = event {
                app.state.recorder_says(said);
            }
        });
        match inserted {
            Ok(_) => {
                self.record.back = Some(sender.clone());
                Some(sender)
            }
            Err(err) => {
                tracing::warn!(%err.error, "could not listen to the recorders");
                None
            }
        }
    }

    fn recorder_says(&mut self, said: FromRecorder) {
        let now = self.now();
        match said {
            FromRecorder::Started { id, encoder } => {
                if let Some(live) = self.record.live.as_mut().filter(|l| l.id == id) {
                    tracing::info!(id, encoder, "recording");
                    live.started = true;
                    live.fresh = true;
                    // Its time counts from ffmpeg's start, as the file's.
                    if !live.paused {
                        live.since = Some(now);
                    }
                    self.record.due = true;
                    self.tell_recording();
                    self.redraw = true;
                }
            }
            FromRecorder::Returned { id } => {
                if self.record.live.as_ref().is_some_and(|l| l.id == id) {
                    self.record.due = true;
                }
            }
            FromRecorder::Failed { id, why } => {
                tracing::warn!(id, why, "a recording failed");
                self.record.failed.push(why);
                let output = self.record_forget_live(id);
                if let Some(output) = output {
                    self.tell_shell(ToShell::Recorded(Recorded {
                        id,
                        output,
                        width: 0,
                        height: 0,
                        duration: 0,
                        format: self.config.recording.format,
                        folder: String::new(),
                        file: String::new(),
                        poster: String::new(),
                    }));
                }
            }
            FromRecorder::Finished { id, saved } => {
                let output = self
                    .record_forget_live(id)
                    .unwrap_or_else(|| saved_output(self));
                tracing::info!(id, file = %saved.file.display(), "a recording saved");
                self.record.saved.insert(id, saved.file.clone());
                let path = |p: &std::path::Path| p.to_str().map(str::to_owned).unwrap_or_default();
                self.tell_shell(ToShell::Recorded(Recorded {
                    id,
                    output,
                    width: saved.size.0,
                    height: saved.size.1,
                    duration: u64::try_from(saved.duration.as_millis()).unwrap_or(u64::MAX),
                    format: saved.format,
                    folder: saved.folder.clone(),
                    file: path(&saved.file),
                    poster: saved.poster.as_deref().map(path).unwrap_or_default(),
                }));
                self.record.last = Some(saved);
            }
        }
    }

    /// Recording `id` is over (ended, or failed): the screen its thumbnail
    /// shows on.
    fn record_forget_live(&mut self, id: u64) -> Option<String> {
        if self.record.live.as_ref().is_some_and(|l| l.id == id) {
            let live = self.record.live.take()?;
            self.record.copied += live.sender.copied_on_loop();
            self.tell_recording();
            self.redraw = true;
            return Some(live.output);
        }
        let at = self.record.ending.iter().position(|e| e.id == id)?;
        let ending = self.record.ending.remove(at);
        self.record.copied += ending.copies;
        Some(ending.output)
    }

    /// The shell asks: from the bar's item, or a recording's thumbnail.
    pub fn record_asked(&mut self, ask: RecordAsk) {
        match ask {
            RecordAsk::Pause => self.record_pause(true),
            RecordAsk::Resume => self.record_pause(false),
            RecordAsk::Stop => self.record_stop(),
            RecordAsk::Forget(id) => self.record_delete(id),
            RecordAsk::Drag(id) => {
                if let Some(file) = self.record.saved.get(&id).cloned() {
                    self.drag_file(file);
                }
            }
        }
    }

    /// The recording pauses (`paused`), or goes on.
    pub fn record_pause(&mut self, paused: bool) {
        let now = self.now();
        let Some(live) = &mut self.record.live else {
            return;
        };
        if live.paused == paused {
            return;
        }
        live.paused = paused;
        if paused {
            live.before = live.elapsed(now);
            live.since = None;
            live.sender.send(ToRecorder::Pause);
        } else {
            live.since = Some(now);
            live.fresh = true;
            live.sender.send(ToRecorder::Resume);
            self.record.due = true;
        }
        tracing::info!(id = live.id, paused, "the recording");
        self.tell_recording();
        self.redraw = true;
    }

    /// The recording stops: its recorder ends the file, off the loop.
    pub fn record_stop(&mut self) {
        let Some(live) = self.record.live.take() else {
            return;
        };
        tracing::info!(id = live.id, "the recording stops");
        live.sender.send(ToRecorder::Stop);
        self.record.ending.push(Ending {
            id: live.id,
            output: live.output.clone(),
            copies: live.sender.copied_on_loop(),
        });
        if let Some(token) = self.record.pace.take() {
            self.loop_handle.remove(token);
        }
        self.tell_recording();
        self.redraw = true;
    }

    /// A recording's frame could not be drawn: it stops, its file saved.
    pub(crate) fn record_failed(&mut self, why: &str) {
        tracing::warn!(why, "the recording's frame could not be drawn: it stops");
        self.record_stop();
    }

    /// A frame of the recording's went, from `since`.
    pub(crate) fn record_frame_went(&mut self, now: Duration, since: std::time::Instant) {
        if let Some(live) = &mut self.record.live {
            live.last = Some(now);
            live.fresh = false;
        }
        self.record.frames_sent += 1;
        self.record.lend_most = self.record.lend_most.max(since.elapsed());
    }

    /// A frame waits for its pace: a timer owes it, once.
    pub(crate) fn record_pace(&mut self, at: Duration) {
        if self.record.pace.is_some() {
            return;
        }
        let inserted = self
            .loop_handle
            .insert_source(Timer::from_duration(at), |_, _, app| {
                app.state.record.pace = None;
                app.state.record.due = true;
                TimeoutAction::Drop
            });
        match inserted {
            Ok(token) => self.record.pace = Some(token),
            Err(err) => tracing::warn!(%err.error, "a recording's frame not paced"),
        }
    }

    /// The bar's item: the recording, or none.
    pub fn tell_recording(&mut self) {
        let now = self.now();
        let line = match &self.record.live {
            Some(live) => ToShell::Recording(RecordingShown {
                id: live.id,
                paused: live.paused,
                elapsed: u64::try_from(live.elapsed(now).as_millis()).unwrap_or(u64::MAX),
            }),
            None => ToShell::RecordingEnd,
        };
        self.tell_shell(line);
    }

    /// Screen `name` went: a recording on it stops.
    pub fn record_output_gone(&mut self, name: &str) {
        let on_it = self.record.live.as_ref().is_some_and(|l| match &l.source {
            Source::Screen(n) | Source::Area { output: n, .. } => n == name,
            Source::Window(_) => false,
        });
        if on_it {
            tracing::info!(output = name, "the recorded screen is unplugged");
            self.record_stop();
        }
    }

    /// Window `id` went: its recording stops.
    pub fn record_window_gone(&mut self, id: WindowId) {
        if self
            .record
            .live
            .as_ref()
            .is_some_and(|l| l.source == Source::Window(id.0))
        {
            tracing::info!(window = id.0, "the recorded window closed");
            self.record_stop();
        }
    }

    /// The screen locks (the session leaves its terminal, for the display
    /// manager's lock or another user): a recording stops, saved; a
    /// countdown to one is cancelled.
    pub fn record_screen_locks(&mut self) {
        if self.counting_to_record() {
            self.capture_away();
        }
        if self.record.recording() {
            tracing::info!("the screen locks: the recording stops");
            self.record_stop();
        }
    }

    /// `record forget ID`: the recording's file goes (on a thread).
    fn record_delete(&mut self, id: u64) {
        let Some(file) = self.record.saved.remove(&id) else {
            return;
        };
        tracing::info!(id, file = %file.display(), "the recording is deleted");
        programs::on_a_thread("delete the recording".to_owned(), move || {
            if let Err(err) = std::fs::remove_file(&file) {
                tracing::warn!(file = %file.display(), %err, "could not delete the recording");
            }
        });
    }
}

/// A screen for a thumbnail when the recording's is not known.
fn saved_output(state: &State) -> String {
    state
        .output_under_pointer()
        .map(|o| o.name())
        .unwrap_or_default()
}
