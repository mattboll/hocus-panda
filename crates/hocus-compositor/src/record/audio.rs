//! A recording's sound, off the loop: the computer's sound (the default
//! sink's monitor, "Son") and the microphone (the default source,
//! "Micro") captured from PipeWire on a thread of their own, mixed on
//! another into raw 32-bit float samples, 48 kHz, stereo, and written to
//! ffmpeg's pipe at the recording's own pace: exactly as many samples as
//! the recording has lasted, silence where nothing came (nothing playing,
//! no microphone, no PipeWire), nothing while it is paused.
//!
//! The sound is [`LATE`] behind the frames, the time PipeWire takes to
//! hand its buffers over: the frames are put as late in the file
//! (`-itsoffset`), so that both meet.

use std::{
    collections::VecDeque,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use pipewire as pw;
use pw::{properties::properties, spa};

use super::save::{CHANNELS, RATE};

/// How late the sound is put on the frames.
pub const LATE: Duration = Duration::from_millis(100);
/// How often the mixer writes.
const EVERY: Duration = Duration::from_millis(10);
/// Finishing, how long the mixer waits for ffmpeg to read what is left:
/// the pipe is its own to write as to read, so a pipe nobody reads any
/// more is only ever full, never broken.
const STALL: Duration = Duration::from_secs(2);
/// The node names the recording's streams have on PipeWire.
pub const SOUND_NODE: &str = "hocuspanda-recording-sound";
pub const MICRO_NODE: &str = "hocuspanda-recording-micro";

/// How long the recording has lasted, its pauses left out: the clock its
/// frames and its sound both keep.
#[derive(Debug)]
pub struct Timeline {
    inner: Mutex<Times>,
}

#[derive(Debug, Clone, Copy)]
struct Times {
    start: Instant,
    paused: Duration,
    since: Option<Instant>,
    stopped: Option<Duration>,
}

impl Timeline {
    pub fn new(start: Instant) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Times {
                start,
                paused: Duration::ZERO,
                since: None,
                stopped: None,
            }),
        })
    }

    fn times(&self) -> Times {
        *self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// How long it has recorded at `now`.
    pub fn at(&self, now: Instant) -> Duration {
        let t = self.times();
        if let Some(stopped) = t.stopped {
            return stopped;
        }
        let until = t.since.unwrap_or(now);
        until
            .saturating_duration_since(t.start)
            .saturating_sub(t.paused)
    }

    pub fn paused(&self) -> bool {
        self.times().since.is_some()
    }

    pub fn stopped(&self) -> Option<Duration> {
        self.times().stopped
    }

    pub fn pause(&self, now: Instant) {
        let mut t = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if t.since.is_none() && t.stopped.is_none() {
            t.since = Some(now);
        }
    }

    pub fn resume(&self, now: Instant) {
        let mut t = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(since) = t.since.take() {
            t.paused += now.saturating_duration_since(since);
        }
    }

    /// It ends at `now`: its length is kept.
    pub fn stop(&self, now: Instant) -> Duration {
        let length = self.at(now);
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stopped = Some(length);
        length
    }
}

/// Samples come in from one stream, interleaved.
type Queue = Arc<Mutex<VecDeque<f32>>>;

/// The sound's threads, as the recorder holds them.
pub struct Sound {
    quit: Option<pw::channel::Sender<()>>,
    mixer: Option<std::thread::JoinHandle<()>>,
    done: Arc<AtomicBool>,
    abandoned: Arc<AtomicBool>,
}

impl Sound {
    /// The streams asked for (`sound`, `micro`) on PipeWire's `remote`
    /// (its default otherwise), mixed into `pipe` along `timeline`.
    pub fn start(
        remote: Option<PathBuf>,
        sound: bool,
        micro: bool,
        pipe: &Path,
        timeline: Arc<Timeline>,
    ) -> anyhow::Result<Self> {
        let file = open_pipe(pipe)?;
        let queues: Vec<(&'static str, Queue)> = [(sound, SOUND_NODE), (micro, MICRO_NODE)]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, node)| (node, Queue::default()))
            .collect();
        let (quit, quit_rx) = pw::channel::channel::<()>();
        let capture_queues = queues.clone();
        std::thread::Builder::new()
            .name("recording's sound".to_owned())
            .spawn(move || {
                if let Err(err) = capture(remote, &capture_queues, quit_rx) {
                    tracing::warn!(%err, "no sound from PipeWire: the recording's is silent");
                }
            })?;
        let done = Arc::new(AtomicBool::new(false));
        let abandoned = Arc::new(AtomicBool::new(false));
        let (mixer_done, mixer_abandoned) = (done.clone(), abandoned.clone());
        let mixer = std::thread::Builder::new()
            .name("recording's mixer".to_owned())
            .spawn(move || mix(file, &queues, &timeline, &mixer_done, &mixer_abandoned))?;
        Ok(Self {
            quit: Some(quit),
            mixer: Some(mixer),
            done,
            abandoned,
        })
    }

    /// The recording stopped (its timeline says when): the mixer writes
    /// what is left, closes the pipe, and the streams go. Waits for the
    /// mixer (the recorder's thread, never the loop).
    pub fn finish(mut self) {
        self.done.store(true, Ordering::Release);
        self.end();
    }

    /// The recording ended with nothing for ffmpeg (it is gone): the mixer
    /// stops at once, writing nothing more.
    pub fn abandon(mut self) {
        self.abandoned.store(true, Ordering::Release);
        self.done.store(true, Ordering::Release);
        self.end();
    }

    fn end(&mut self) {
        if let Some(mixer) = self.mixer.take() {
            let _ = mixer.join();
        }
        if let Some(quit) = self.quit.take() {
            let _ = quit.send(());
        }
    }
}

impl std::fmt::Debug for Sound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Sound")
    }
}

impl Drop for Sound {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(quit) = self.quit.take() {
            let _ = quit.send(());
        }
    }
}

/// The pipe, opened to read and write, without waiting: a pipe opened to
/// write alone waits for its reader, which may never come (ffmpeg gone).
/// Its writes do not wait either: a full pipe is written again later.
fn open_pipe(pipe: &Path) -> anyhow::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let flags = rustix::fs::OFlags::NONBLOCK.bits();
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(i32::try_from(flags).unwrap_or(0))
        .open(pipe)?;
    Ok(file)
}

/// Makes the pipe ffmpeg reads the sound from.
pub fn make_pipe(path: &Path) -> anyhow::Result<()> {
    rustix::fs::mknodat(
        rustix::fs::CWD,
        path,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::from_raw_mode(0o600),
        0,
    )?;
    Ok(())
}

/// The streams on PipeWire, their samples into `queues`, until `quit`.
fn capture(
    remote: Option<PathBuf>,
    queues: &[(&'static str, Queue)],
    quit: pw::channel::Receiver<()>,
) -> Result<(), pw::Error> {
    if queues.is_empty() {
        return Ok(());
    }
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let props = remote.map(|r| {
        properties! {
            *pw::keys::REMOTE_NAME => r.to_string_lossy().into_owned(),
        }
    });
    let core = context.connect_rc(props)?;
    let mut streams = Vec::new();
    for (node, queue) in queues {
        let props = stream_props(node);
        let stream = pw::stream::StreamRc::new(core.clone(), node, props)?;
        let queue = queue.clone();
        let listener = stream
            .add_local_listener::<()>()
            .process(move |stream, _| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let Some(data) = buffer.datas_mut().first_mut() else {
                    return;
                };
                let size = data.chunk().size() as usize;
                let offset = data.chunk().offset() as usize;
                if let Some(bytes) = data.data() {
                    let end = (offset + size).min(bytes.len());
                    let bytes = bytes.get(offset..end).unwrap_or_default();
                    let mut queue = queue.lock().unwrap_or_else(PoisonError::into_inner);
                    queue.extend(
                        bytes
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .map(|b| f32::from_le_bytes(*b)),
                    );
                    // A second at most: a mixer that stalled does not keep
                    // what it will never write.
                    let most = (RATE * CHANNELS) as usize;
                    let extra = queue.len().saturating_sub(most);
                    queue.drain(..extra);
                }
            })
            .register()?;
        let pod = format_pod();
        let pod = spa::pod::Pod::from_bytes(&pod).ok_or(pw::Error::CreationFailed)?;
        let mut params = [pod];
        stream.connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )?;
        tracing::info!(node, "the recording's sound stream");
        streams.push((stream, listener));
    }
    let weak = mainloop.downgrade();
    let _quit = quit.attach(mainloop.loop_(), move |()| {
        if let Some(mainloop) = weak.upgrade() {
            mainloop.quit();
        }
    });
    mainloop.run();
    for (stream, _) in &streams {
        let _ = stream.disconnect();
    }
    Ok(())
}

/// A stream's properties: the computer's sound is the default sink's
/// monitor (`stream.capture.sink`), the microphone the default source;
/// the session manager links them.
pub fn stream_props(node: &str) -> pw::properties::PropertiesBox {
    let mut props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => if node == SOUND_NODE { "Screen" } else { "Communication" },
        *pw::keys::NODE_NAME => node.to_owned(),
        *pw::keys::APP_NAME => "Hocuspanda",
        *pw::keys::NODE_DESCRIPTION => if node == SOUND_NODE {
            "Hocuspanda: recording the sound"
        } else {
            "Hocuspanda: recording the microphone"
        },
    };
    if node == SOUND_NODE {
        props.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");
    }
    props
}

/// Raw audio, 32-bit floats, 48 kHz, stereo: PipeWire converts to it.
fn format_pod() -> Vec<u8> {
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_rate(RATE);
    info.set_channels(CHANNELS);
    let mut position = [0; spa::param::audio::MAX_CHANNELS];
    position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
    position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
    info.set_position(position);
    let object = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    };
    spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )
    .map(|(cursor, _)| cursor.into_inner())
    .unwrap_or_default()
}

/// The mixer: what the recording owes in samples, from the queues (silence
/// where they are dry), written to `pipe` until `done` and the samples
/// owed up to its end and [`LATE`] past it are.
fn mix(
    mut pipe: std::fs::File,
    queues: &[(&'static str, Queue)],
    timeline: &Timeline,
    done: &AtomicBool,
    abandoned: &AtomicBool,
) {
    let late = frames_in(LATE);
    // Kept at most: twice the lateness and a fifth of a second, past
    // which the streams' clock ran ahead of the recording's.
    let most = 2 * late + u64::from(RATE / 5);
    let mut mixer = Mixer::new(queues.iter().map(|(_, q)| q.clone()).collect(), late, most);
    let mut pending: Vec<u8> = Vec::new();
    // Since when nothing could be written while finishing.
    let mut stalled: Option<Instant> = None;
    loop {
        if abandoned.load(Ordering::Acquire) {
            return;
        }
        let finishing = done.load(Ordering::Acquire);
        let now = Instant::now();
        // Stopped, its last samples at once: ffmpeg holds the last frames
        // until the sound reaches them (they are `LATE` in the file).
        let owed = match timeline.stopped() {
            Some(length) => frames_in(length) + late,
            None => frames_in(timeline.at(now)),
        };
        if timeline.paused() {
            mixer.trim();
        }
        let samples = mixer.take(owed);
        pending.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
        // A full pipe is written again at the next turn.
        let before = pending.len();
        while !pending.is_empty() {
            match pipe.write(&pending) {
                Ok(0) => break,
                Ok(n) => {
                    pending.drain(..n);
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                Err(err) => {
                    tracing::warn!(%err, "the recording's sound cannot be written");
                    return;
                }
            }
        }
        if finishing && pending.is_empty() && mixer.written >= owed {
            return;
        }
        // Finishing, and ffmpeg reads no more of it (it ended, or never
        // got a frame): what is left is dropped.
        if finishing && pending.len() == before && !pending.is_empty() {
            let since = *stalled.get_or_insert(now);
            if now.saturating_duration_since(since) >= STALL {
                tracing::warn!(
                    left = pending.len(),
                    "ffmpeg reads no more of the recording's sound: the rest is dropped"
                );
                return;
            }
        } else {
            stalled = None;
        }
        if finishing && timeline.stopped().is_none() {
            return;
        }
        std::thread::sleep(EVERY);
    }
}

/// Frames (a sample per channel) in `time`.
fn frames_in(time: Duration) -> u64 {
    (time.as_secs_f64() * f64::from(RATE)).round() as u64
}

/// The sum of the streams, frame by frame.
struct Mixer {
    queues: Vec<Queue>,
    /// Frames written so far, the first `late` silent.
    written: u64,
    late: u64,
    /// The most frames a queue keeps.
    most: u64,
}

impl Mixer {
    fn new(queues: Vec<Queue>, late: u64, most: u64) -> Self {
        Self {
            queues,
            written: 0,
            late,
            most,
        }
    }

    /// The samples up to frame `owed`.
    fn take(&mut self, owed: u64) -> Vec<f32> {
        let n = owed.saturating_sub(self.written) as usize;
        if n == 0 {
            return Vec::new();
        }
        let channels = CHANNELS as usize;
        let mut out = vec![0.0f32; n * channels];
        // The first frames: silence, the sound's lateness.
        let silent = self.late.saturating_sub(self.written).min(n as u64) as usize;
        for queue in &self.queues {
            let mut queue = queue.lock().unwrap_or_else(PoisonError::into_inner);
            // Later than it should be by far: the oldest go.
            let most = self.most as usize * channels;
            let extra = queue.len().saturating_sub(most);
            queue.drain(..extra - extra % channels);
            let wanted = (n - silent) * channels;
            let have = queue.len().min(wanted);
            for (slot, sample) in out[silent * channels..].iter_mut().zip(queue.drain(..have)) {
                *slot += sample;
            }
        }
        for sample in &mut out {
            *sample = sample.clamp(-1.0, 1.0);
        }
        self.written += n as u64;
        out
    }

    /// Paused: what comes is kept to its lateness alone.
    fn trim(&mut self) {
        let channels = CHANNELS as usize;
        let keep = self.late as usize * channels;
        for queue in &self.queues {
            let mut queue = queue.lock().unwrap_or_else(PoisonError::into_inner);
            let extra = queue.len().saturating_sub(keep);
            queue.drain(..extra - extra % channels);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_timeline_leaves_its_pauses_out_and_stops() {
        let t0 = Instant::now();
        let timeline = Timeline::new(t0);
        assert_eq!(
            timeline.at(t0 + Duration::from_secs(2)),
            Duration::from_secs(2)
        );
        timeline.pause(t0 + Duration::from_secs(2));
        assert!(timeline.paused());
        assert_eq!(
            timeline.at(t0 + Duration::from_secs(9)),
            Duration::from_secs(2)
        );
        timeline.resume(t0 + Duration::from_secs(5));
        assert_eq!(
            timeline.at(t0 + Duration::from_secs(6)),
            Duration::from_secs(3)
        );
        assert_eq!(
            timeline.stop(t0 + Duration::from_secs(7)),
            Duration::from_secs(4)
        );
        assert_eq!(
            timeline.at(t0 + Duration::from_secs(60)),
            Duration::from_secs(4)
        );
    }

    #[test]
    fn the_mixer_sums_the_streams_late_and_silent_where_dry() {
        let a = Queue::default();
        let b = Queue::default();
        a.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend([0.25f32; 8]);
        b.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend([0.5f32, 0.5, 0.9, 0.9]);
        let mut mixer = Mixer::new(vec![a, b], 1, 100);
        let out = mixer.take(4);
        // One frame of lateness, then both, then the first alone, then dry.
        assert_eq!(out, vec![0.0, 0.0, 0.75, 0.75, 1.0, 1.0, 0.25, 0.25]);
        assert_eq!(mixer.take(4), Vec::<f32>::new());
        assert_eq!(mixer.take(5), vec![0.25, 0.25]);
        assert_eq!(mixer.take(6), vec![0.0, 0.0]);
    }

    #[test]
    fn the_sound_is_the_default_sinks_monitor_and_the_micro_the_default_source() {
        let sound = stream_props(SOUND_NODE);
        assert_eq!(sound.get("stream.capture.sink"), Some("true"));
        assert_eq!(sound.get("media.category"), Some("Capture"));
        let micro = stream_props(MICRO_NODE);
        assert_eq!(micro.get("stream.capture.sink"), None);
        assert_eq!(micro.get("node.name"), Some(MICRO_NODE));
    }
}
