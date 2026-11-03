//! The recorder: a thread of its own per recording, off the loop. It names
//! and makes the file, starts ffmpeg (a program of its own: an encoder or
//! a GPU driver that fails ends it, never the session), takes the frames
//! the loop lends it ([`crate::share::lease`], as the screen cast's
//! streams do), copies each into a frame of its own and gives it back at
//! once, and writes ffmpeg [`RECORD_FPS`] frames a second of the
//! recording's time (the last one again while nothing changed, none while
//! paused): the file's pace is exact, whatever the loop's. Its sound comes
//! from [`super::audio`]. Once stopped, ffmpeg ends the file and the
//! thread says what was saved, with a small picture of its first frame
//! for the thumbnail.
//!
//! Which encoder: the GPU's through VA-API when the compositor draws on a
//! GPU and ffmpeg can use it there (tried once, on a second of black),
//! else libx264, else libopenh264 (VP9 for WebM).

use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex, PoisonError, mpsc},
    time::{Duration, Instant, SystemTime},
};

use anyhow::Context;
use hocus_link::{RECORD_FPS, RecordFormat};
use smithay::reexports::calloop::channel::Sender;

use super::{
    audio::{self, Sound, Timeline},
    save::{self, Plan, Video},
};
use crate::share::lease::{Lease, LoopCopies};

/// What the loop tells a recorder.
#[derive(Debug)]
pub enum ToRecorder {
    /// A frame: its pixels lent (rows of `stride` bytes, `size` pixels),
    /// given back once copied.
    Frame {
        lease: Lease,
        size: (u32, u32),
        stride: u32,
    },
    Pause,
    Resume,
    Stop,
}

/// What a recorder tells the loop.
#[derive(Debug)]
pub enum FromRecorder {
    /// ffmpeg runs, with this encoder, writing this file.
    Started { id: u64, encoder: String },
    /// A frame's pixels lent are given back.
    Returned { id: u64 },
    /// It could not record: no file.
    Failed { id: u64, why: String },
    /// Saved.
    Finished { id: u64, saved: Saved },
}

/// A recording saved.
#[derive(Debug, Clone)]
pub struct Saved {
    pub file: PathBuf,
    /// The folder as the user reads it.
    pub folder: String,
    pub size: (u32, u32),
    pub duration: Duration,
    pub format: RecordFormat,
    /// Its first frame, small, as a PNG; none if it could not be written.
    pub poster: Option<PathBuf>,
    pub encoder: String,
}

/// What a recorder is started with.
#[derive(Debug, Clone)]
pub struct Job {
    pub id: u64,
    /// The frames' size, even.
    pub size: (u32, u32),
    pub format: RecordFormat,
    pub sound: bool,
    pub micro: bool,
    /// The render node the compositor draws on: ffmpeg may encode there.
    pub gpu: Option<PathBuf>,
    pub pipewire: Option<PathBuf>,
    pub videos: Option<PathBuf>,
    pub french: bool,
    pub when: SystemTime,
    /// Where its first frame's picture and its sound's pipe go.
    pub runtime: PathBuf,
    /// ffmpeg's program (`ffmpeg`, found on the `PATH`).
    pub ffmpeg: PathBuf,
}

/// The way to a recorder, and back to the loop: the read-back thread holds
/// one too.
#[derive(Clone)]
pub struct RecordSender {
    pub(crate) id: u64,
    to: mpsc::Sender<ToRecorder>,
    back: Sender<FromRecorder>,
    copies: Arc<LoopCopies>,
}

impl std::fmt::Debug for RecordSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecordSender")
            .field("id", &self.id)
            .finish()
    }
}

impl RecordSender {
    pub fn send(&self, said: ToRecorder) {
        if let Err(mpsc::SendError(said)) = self.to.send(said) {
            tracing::debug!(id = self.id, "the recorder is gone");
            // Its pixels back at once: their lender frees them.
            if let ToRecorder::Frame { lease, .. } = said {
                lease.give_back();
                if !lease.is_quiet() {
                    self.tell(FromRecorder::Returned { id: self.id });
                }
            }
        }
    }

    pub fn tell(&self, said: FromRecorder) {
        let _ = self.back.send(said);
    }

    pub fn lend(&self, pixels: &[u8], quiet: bool) -> Lease {
        if quiet {
            Lease::quiet(pixels, self.copies.clone())
        } else {
            Lease::new(pixels, self.copies.clone())
        }
    }

    /// The bytes of its frames read (so copied) on the loop's thread.
    pub fn copied_on_loop(&self) -> u64 {
        self.copies.bytes()
    }
}

/// Starts the recorder of `job`; called on the loop, whose thread the
/// leases' reads are counted on.
pub fn start(job: Job, back: Sender<FromRecorder>) -> anyhow::Result<RecordSender> {
    let (to, from_loop) = mpsc::channel::<ToRecorder>();
    let sender = RecordSender {
        id: job.id,
        to,
        back: back.clone(),
        copies: LoopCopies::here(),
    };
    let id = job.id;
    std::thread::Builder::new()
        .name("screen recording".to_owned())
        .spawn(move || {
            let said = match record(&job, from_loop, &back) {
                Ok(saved) => FromRecorder::Finished { id, saved },
                Err(err) => {
                    tracing::warn!("the recording failed: {err:#}");
                    FromRecorder::Failed {
                        id,
                        why: format!("{err:#}"),
                    }
                }
            };
            let _ = back.send(said);
        })?;
    Ok(sender)
}

/// What ffmpeg can do here, found once.
#[derive(Debug, Clone, Default)]
struct Probed {
    encoders: Vec<String>,
    /// VA-API tried on these nodes, and whether it encoded.
    vaapi: Vec<(PathBuf, bool)>,
}

static PROBED: Mutex<Option<Probed>> = Mutex::new(None);

/// The video encoder for `job`: VA-API on its GPU when ffmpeg encodes
/// there, else the first software one ffmpeg has.
fn encoder(job: &Job) -> anyhow::Result<Video> {
    let mut probed = PROBED.lock().unwrap_or_else(PoisonError::into_inner);
    let probed = match &mut *probed {
        Some(p) => p,
        empty => {
            let out = Command::new(&job.ffmpeg)
                .args(["-hide_banner", "-encoders"])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .context("ffmpeg is not there: install it to record the screen")?;
            empty.insert(Probed {
                encoders: save::listed_encoders(&String::from_utf8_lossy(&out.stdout)),
                vaapi: Vec::new(),
            })
        }
    };
    let gpu_codec = match job.format {
        RecordFormat::Mp4 => "h264_vaapi",
        RecordFormat::WebM => "vp9_vaapi",
    };
    if let Some(node) = &job.gpu
        && probed.encoders.iter().any(|e| e == gpu_codec)
    {
        let works = match probed.vaapi.iter().find(|(n, _)| n == node) {
            Some((_, works)) => *works,
            None => {
                let works = vaapi_works(&job.ffmpeg, node, gpu_codec);
                tracing::info!(node = %node.display(), works, "VA-API tried for recording");
                probed.vaapi.push((node.clone(), works));
                works
            }
        };
        if works {
            return Ok(Video::Vaapi(node.clone()));
        }
    }
    save::software(job.format)
        .iter()
        .find(|name| probed.encoders.iter().any(|e| e == *name))
        .map(|name| Video::Software(name))
        .with_context(|| {
            format!(
                "ffmpeg has none of {:?} to encode the video",
                save::software(job.format)
            )
        })
}

/// Whether ffmpeg encodes a short black video with `codec` on `node`.
fn vaapi_works(ffmpeg: &Path, node: &Path, codec: &str) -> bool {
    let child = Command::new(ffmpeg)
        .args(["-hide_banner", "-nostdin", "-loglevel", "error"])
        .arg("-init_hw_device")
        .arg(format!("vaapi=va:{}", node.display()))
        .args([
            "-filter_hw_device",
            "va",
            "-f",
            "lavfi",
            "-i",
            "color=black:s=256x256:d=0.2",
            "-vf",
            "format=nv12,hwupload",
            "-c:v",
            codec,
            "-f",
            "null",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return false;
    };
    wait(&mut child, Duration::from_secs(10)).is_some_and(|s| s.success())
}

/// Waits for `child` `most` at most (the recorder's thread): its status, or
/// none and it is killed (it is the recorder's own).
fn wait(child: &mut Child, most: Duration) -> Option<std::process::ExitStatus> {
    let until = Instant::now() + most;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// The recording, from its start to its file.
fn record(
    job: &Job,
    from_loop: mpsc::Receiver<ToRecorder>,
    back: &Sender<FromRecorder>,
) -> anyhow::Result<Saved> {
    let video = encoder(job)?;
    let videos = job
        .videos
        .clone()
        .context("no videos' folder to save the recording in")?;
    let dir = videos.join(save::folder_name(job.french));
    std::fs::create_dir_all(&dir).with_context(|| format!("could not make {}", dir.display()))?;
    let file = save::unused(&dir, &save::file_name(job.when, job.french, job.format));
    // The sound's pipe, in a directory of its own in the runtime one.
    let pipes = tempfile::Builder::new()
        .prefix("hocuspanda-recording-")
        .tempdir_in(&job.runtime)
        .context("no directory for the recording's sound")?;
    let audio_pipe = (job.sound || job.micro).then(|| pipes.path().join("sound"));
    if let Some(pipe) = &audio_pipe {
        audio::make_pipe(pipe).context("no pipe for the recording's sound")?;
    }
    let plan = Plan {
        size: job.size,
        video: video.clone(),
        format: job.format,
        audio: audio_pipe.clone(),
        audio_late: audio::LATE,
        file: file.clone(),
    };
    let mut child = Command::new(&job.ffmpeg)
        .args(save::ffmpeg_args(&plan))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("ffmpeg does not start: install it to record the screen")?;
    let said = collect_errors(&mut child);
    let stdin = child.stdin.take().context("ffmpeg's input")?;
    tracing::info!(
        id = job.id,
        encoder = video.name(),
        file = %file.display(),
        width = job.size.0,
        height = job.size.1,
        sound = job.sound,
        micro = job.micro,
        "recording"
    );
    let _ = back.send(FromRecorder::Started {
        id: job.id,
        encoder: video.name(),
    });
    let timeline = Timeline::new(Instant::now());
    let sound = match &audio_pipe {
        Some(pipe) => Some(Sound::start(
            job.pipewire.clone(),
            job.sound,
            job.micro,
            pipe,
            timeline.clone(),
        )?),
        None => None,
    };
    let mut frames = Frames {
        stdin: Some(stdin),
        size: job.size,
        latest: None,
        first: None,
        written: 0,
    };
    let went = frames.run(job.id, &from_loop, back, &timeline);
    let length = timeline.stop(Instant::now());
    // Frames lent since are given back, and those to come at once: their
    // lenders need not wait for them while the file is ended.
    while let Ok(said) = from_loop.try_recv() {
        if let ToRecorder::Frame { lease, .. } = said {
            lease.give_back();
            if !lease.is_quiet() {
                let _ = back.send(FromRecorder::Returned { id: job.id });
            }
        }
    }
    drop(from_loop);
    // The last frames, then the end of the input: ffmpeg ends the file.
    let ended = went.and_then(|()| frames.until(length));
    frames.stdin = None;
    tracing::debug!(
        id = job.id,
        frames = frames.written,
        "the recording's frames are all written"
    );
    if let Some(sound) = sound {
        sound.finish();
        tracing::debug!(id = job.id, "the recording's sound is all written");
    }
    let status = wait(&mut child, Duration::from_secs(120));
    drop(pipes);
    let errors = said.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let ok = ended.is_ok() && status.is_some_and(|s| s.success());
    if !ok {
        let _ = std::fs::remove_file(&file);
        let why = ended.err().map(|e| format!("{e:#}")).unwrap_or_default();
        anyhow::bail!(
            "ffmpeg could not record ({status:?}) {why} {}",
            errors.trim()
        );
    }
    let duration = Duration::from_secs_f64(frames.written as f64 / f64::from(RECORD_FPS));
    let poster = frames.first.as_ref().and_then(|first| {
        poster(job, first)
            .map_err(|err| tracing::warn!("{err:#}"))
            .ok()
    });
    tracing::info!(
        id = job.id,
        file = %file.display(),
        frames = frames.written,
        seconds = duration.as_secs_f64(),
        "recorded"
    );
    Ok(Saved {
        file,
        folder: save::shown_folder(&videos, job.french),
        size: job.size,
        duration,
        format: job.format,
        poster,
        encoder: video.name(),
    })
}

/// What ffmpeg says on its errors' output, kept (the last lines) for the
/// log, read on a thread of its own so that it never fills.
fn collect_errors(child: &mut Child) -> Arc<Mutex<String>> {
    let said = Arc::new(Mutex::new(String::new()));
    if let Some(mut err) = child.stderr.take() {
        let kept = said.clone();
        let _ = std::thread::Builder::new()
            .name("ffmpeg's errors".to_owned())
            .spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = err.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let mut kept = kept.lock().unwrap_or_else(PoisonError::into_inner);
                    kept.push_str(&String::from_utf8_lossy(&buf[..n]));
                    let extra = kept.len().saturating_sub(2000);
                    if extra > 0 {
                        let at = kept.ceil_char_boundary(extra);
                        kept.drain(..at);
                    }
                }
            });
    }
    said
}

/// The frames' side: the last frame lent, copied, and how many went to
/// ffmpeg.
struct Frames {
    stdin: Option<ChildStdin>,
    size: (u32, u32),
    latest: Option<Arc<Vec<u8>>>,
    first: Option<Arc<Vec<u8>>>,
    written: u64,
}

impl Frames {
    /// Until the loop says stop (or goes): the frames it lends taken, and
    /// what the recording's time owes written.
    fn run(
        &mut self,
        id: u64,
        from_loop: &mpsc::Receiver<ToRecorder>,
        back: &Sender<FromRecorder>,
        timeline: &Timeline,
    ) -> anyhow::Result<()> {
        let slot = Duration::from_secs(1) / RECORD_FPS;
        loop {
            let said = from_loop.recv_timeout(slot / 2);
            match said {
                Ok(ToRecorder::Frame {
                    lease,
                    size,
                    stride,
                }) => {
                    let copied = lease.with(|pixels| self.copy(pixels, size, stride));
                    lease.give_back();
                    if !lease.is_quiet() {
                        let _ = back.send(FromRecorder::Returned { id });
                    }
                    if let Some(frame) = copied.flatten() {
                        let frame = Arc::new(frame);
                        self.first.get_or_insert_with(|| frame.clone());
                        self.latest = Some(frame);
                    }
                }
                Ok(ToRecorder::Pause) => timeline.pause(Instant::now()),
                Ok(ToRecorder::Resume) => timeline.resume(Instant::now()),
                Ok(ToRecorder::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Ok(());
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if !timeline.paused() {
                self.until(timeline.at(Instant::now()))?;
            }
        }
    }

    /// `pixels` (rows of `stride` bytes, `size` pixels) as a frame of the
    /// recording's size: cut, or black where it is smaller.
    fn copy(&self, pixels: &[u8], size: (u32, u32), stride: u32) -> Option<Vec<u8>> {
        let (w, h) = self.size;
        let row = w as usize * 4;
        let mut frame = vec![0u8; row * h as usize];
        let take = (size.0.min(w) as usize) * 4;
        for y in 0..(size.1.min(h) as usize) {
            let from = y * stride as usize;
            let src = pixels.get(from..from + take)?;
            frame[y * row..y * row + take].copy_from_slice(src);
        }
        Some(frame)
    }

    /// The frames owed up to `time` of the recording written (the last one
    /// again while nothing changed); none before the first.
    fn until(&mut self, time: Duration) -> anyhow::Result<()> {
        let Some(latest) = self.latest.clone() else {
            return Ok(());
        };
        let owed = (time.as_secs_f64() * f64::from(RECORD_FPS)).floor() as u64 + 1;
        while self.written < owed {
            let stdin = self.stdin.as_mut().context("ffmpeg's input is closed")?;
            stdin
                .write_all(&latest)
                .context("ffmpeg stopped reading the frames")?;
            self.written += 1;
        }
        Ok(())
    }
}

/// The first frame, at most 640 pixels wide, as a PNG in the runtime
/// directory: the thumbnail's picture.
fn poster(job: &Job, first: &[u8]) -> anyhow::Result<PathBuf> {
    let (w, h) = job.size;
    let mut rgba = Vec::with_capacity(first.len());
    for &[b, g, r, _] in first.as_chunks::<4>().0 {
        rgba.extend_from_slice(&[r, g, b, 255]);
    }
    let image = image::RgbaImage::from_raw(w, h, rgba).context("the first frame")?;
    let k = (640.0 / f64::from(w)).min(1.0);
    let small = if k < 1.0 {
        image::imageops::resize(
            &image,
            ((f64::from(w) * k).round() as u32).max(1),
            ((f64::from(h) * k).round() as u32).max(1),
            image::imageops::FilterType::Triangle,
        )
    } else {
        image
    };
    let path = job
        .runtime
        .join(format!("hocuspanda-recording-{}.png", job.id));
    small
        .save_with_format(&path, image::ImageFormat::Png)
        .with_context(|| format!("could not write {}", path.display()))?;
    Ok(path)
}
