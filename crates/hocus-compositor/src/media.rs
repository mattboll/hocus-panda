//! What a recording takes besides the screen (#131): the microphone and
//! the system's sound, the webcam's bubble and the keys pressed. All of it
//! runs on the recording's worker thread and its encoder processes; the
//! loop only sends what changes.
//!
//! Sound is recorded by the encoder, one segment per stretch between
//! pauses, and put with the video once it is done: the track is cut or
//! padded to the video's length. The webcam is read by an encoder process
//! of its own as small square frames; the worker cuts the latest one into a
//! circle in each frame of the video.

use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use hocus_shell::recording::Overlay;

use crate::capture::Pixels;

/// How long a combination shows, its last part fading.
const KEYS_SHOWN: Duration = Duration::from_millis(1500);
const KEYS_FADE: Duration = Duration::from_millis(300);
/// How long a segment's encoder gets to finish before it is killed.
const FINISH: Duration = Duration::from_secs(5);

/// Where sound comes from.
#[derive(Debug, Clone)]
pub struct Sound {
    /// The encoder's input format: `pulse`, `oss`, `alsa`; `lavfi` in tests.
    pub format: String,
    /// One or two devices: the microphone, the system's sound; mixed.
    pub devices: Vec<String>,
}

/// Where the webcam is, and its bubble.
#[derive(Debug, Clone)]
pub struct Camera {
    pub input: String,
    pub device: String,
    /// The bubble's diameter, video pixels.
    pub diameter: u32,
}

/// The sound being recorded: a segment per stretch between pauses.
pub struct SoundRecorder {
    encoder: String,
    sound: Sound,
    /// Segments are named after this.
    base: PathBuf,
    done: Vec<PathBuf>,
    current: Option<(Child, PathBuf)>,
}

impl SoundRecorder {
    /// Starts the first segment.
    pub fn start(encoder: &str, sound: Sound, base: &Path) -> Result<Self, String> {
        let mut recorder = Self {
            encoder: encoder.to_owned(),
            sound,
            base: base.to_path_buf(),
            done: Vec::new(),
            current: None,
        };
        recorder.resume()?;
        Ok(recorder)
    }

    /// A new segment, after a pause.
    pub fn resume(&mut self) -> Result<(), String> {
        if self.current.is_some() {
            return Ok(());
        }
        let n = self.done.len() + 1;
        let mut name = self.base.as_os_str().to_owned();
        name.push(format!(".sound{n}.wav"));
        let path = PathBuf::from(name);
        let mut command = Command::new(&self.encoder);
        command.args(["-hide_banner", "-loglevel", "error", "-y"]);
        for device in &self.sound.devices {
            // A generated test sound comes at its own pace, not at once.
            if self.sound.format == "lavfi" {
                command.arg("-re");
            }
            command.args(["-f", &self.sound.format, "-i"]).arg(device);
        }
        if self.sound.devices.len() > 1 {
            command.args([
                "-filter_complex",
                "amix=inputs=2:duration=longest:normalize=0",
            ]);
        }
        command
            .args(["-ac", "2", "-ar", "48000"])
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let child = command
            .spawn()
            .map_err(|e| format!("{}: {e}", self.encoder))?;
        self.current = Some((child, path));
        Ok(())
    }

    /// The segment ends, for a pause or the end.
    pub fn pause(&mut self) {
        if let Some((child, path)) = self.current.take() {
            if finish(child) {
                self.done.push(path);
            } else {
                tracing::warn!(path = %path.display(), "a stretch of sound was lost");
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    /// Every segment, in order, once the last has ended.
    pub fn finish(mut self) -> Vec<PathBuf> {
        self.pause();
        std::mem::take(&mut self.done)
    }
}

impl Drop for SoundRecorder {
    fn drop(&mut self) {
        self.pause();
    }
}

/// Asks an encoder to end its file (as Ctrl+C would) and waits for it;
/// killed if it takes too long. Whether it ended well.
fn finish(mut child: Child) -> bool {
    if let Some(pid) = rustix::process::Pid::from_raw(child.id() as i32) {
        let _ = rustix::process::kill_process(pid, rustix::process::Signal::INT);
    }
    let deadline = Instant::now() + FINISH;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    let mut said = String::new();
                    if let Some(mut stderr) = child.stderr.take() {
                        let _ = stderr.read_to_string(&mut said);
                    }
                    // Interrupted is how it is asked to end.
                    return said.trim().is_empty() || status.code() == Some(255);
                }
                return true;
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// Puts the sound's segments with the video into `out`: one track, cut or
/// padded to the video's length.
pub fn mux(encoder: &str, video: &Path, segments: &[PathBuf], out: &Path) -> Result<(), String> {
    let mut command = Command::new(encoder);
    command.args(["-hide_banner", "-loglevel", "error", "-y", "-i"]);
    command.arg(video);
    for segment in segments {
        command.arg("-i").arg(segment);
    }
    let inputs: String = (1..=segments.len()).map(|i| format!("[{i}:a]")).collect();
    let graph = format!("{inputs}concat=n={}:v=0:a=1,apad[a]", segments.len().max(1));
    command
        .args(["-filter_complex", &graph])
        .args(["-map", "0:v", "-map", "[a]"])
        .args(["-c:v", "copy", "-c:a", "aac", "-b:a", "160k", "-shortest"])
        .args(["-movflags", "+faststart"])
        .arg(out)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let output = command.output().map_err(|e| format!("{encoder}: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let said = String::from_utf8_lossy(&output.stderr);
        Err(said.lines().last().map_or_else(
            || format!("{encoder} said {}", output.status),
            str::to_owned,
        ))
    }
}

/// The webcam, read as square frames by an encoder process; the latest
/// one is kept.
pub struct CameraFeed {
    latest: Arc<Mutex<Option<Vec<u8>>>>,
    child: Option<Child>,
    stop: Arc<AtomicBool>,
    pub diameter: u32,
}

impl CameraFeed {
    pub fn start(encoder: &str, camera: &Camera) -> Result<Self, String> {
        let d = camera.diameter.max(16);
        let mut command = Command::new(encoder);
        command.args(["-hide_banner", "-loglevel", "error"]);
        if camera.input == "lavfi" {
            command.arg("-re");
        }
        command
            .args(["-f", &camera.input, "-i"])
            .arg(&camera.device)
            // The middle square, at the bubble's size, mirrored as people
            // expect to see themselves.
            .args([
                "-vf",
                &format!("crop='min(iw,ih)':'min(iw,ih)',scale={d}:{d},hflip,fps=15"),
            ])
            .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().map_err(|e| format!("{encoder}: {e}"))?;
        let mut stdout = child.stdout.take().ok_or("the camera gives nothing")?;
        let latest = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let (kept, stopped) = (latest.clone(), stop.clone());
        let size = (d * d * 4) as usize;
        std::thread::Builder::new()
            .name("camera".into())
            .spawn(move || {
                let mut frame = vec![0; size];
                while !stopped.load(Ordering::Relaxed) && stdout.read_exact(&mut frame).is_ok() {
                    if let Ok(mut slot) = kept.lock() {
                        *slot = Some(frame.clone());
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            latest,
            child: Some(child),
            stop,
            diameter: d,
        })
    }

    pub fn latest(&self) -> Option<Vec<u8>> {
        self.latest.lock().ok().and_then(|f| f.clone())
    }
}

impl Drop for CameraFeed {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// What goes over the screen's picture in the video.
#[derive(Default)]
pub struct Overlays {
    /// The last combination, from when in the video.
    pub keys: Option<(Duration, Overlay)>,
    pub camera: Option<CameraFeed>,
    /// The bubble's corner, video pixels.
    pub bubble: (u32, u32),
}

impl Overlays {
    /// Whether the frame at `at` needs anything drawn over it.
    pub fn active(&self, at: Duration) -> bool {
        self.camera.is_some()
            || self
                .keys
                .as_ref()
                .is_some_and(|(from, _)| at.saturating_sub(*from) < KEYS_SHOWN && at >= *from)
    }

    /// `frame` with the bubble and the keys over it, as at `at`.
    pub fn draw(&self, frame: &[u8], width: u32, height: u32, at: Duration) -> Vec<u8> {
        let mut out = Pixels {
            width,
            height,
            rgba: frame.to_vec(),
        };
        if let Some(camera) = &self.camera
            && let Some(face) = camera.latest()
        {
            paint_bubble(&mut out, &face, camera.diameter, self.bubble);
        }
        if let Some((from, caps)) = &self.keys
            && at >= *from
        {
            let shown = at - *from;
            if shown < KEYS_SHOWN {
                let left = KEYS_SHOWN - shown;
                let alpha = (left.as_secs_f32() / KEYS_FADE.as_secs_f32()).min(1.0);
                // Centred, near the bottom.
                let x = width.saturating_sub(caps.width) / 2;
                let y = height.saturating_sub(caps.height + height / 25);
                blend(&mut out, caps, (x, y), alpha);
            }
        }
        out.rgba
    }
}

/// Draws a premultiplied `overlay` at `(x, y)` over an opaque picture.
pub fn blend(pixels: &mut Pixels, overlay: &Overlay, (x, y): (u32, u32), alpha: f32) {
    let (w, h) = (pixels.width as usize, pixels.height as usize);
    for oy in 0..overlay.height as usize {
        let py = y as usize + oy;
        if py >= h {
            break;
        }
        for ox in 0..overlay.width as usize {
            let px = x as usize + ox;
            if px >= w {
                break;
            }
            let o = (oy * overlay.width as usize + ox) * 4;
            let src = &overlay.rgba[o..o + 4];
            let a = f32::from(src[3]) / 255.0 * alpha;
            if a <= 0.0 {
                continue;
            }
            let p = (py * w + px) * 4;
            for (dst, src) in pixels.rgba[p..p + 3].iter_mut().zip(src) {
                let s = f32::from(*src) * alpha;
                let d = f32::from(*dst);
                *dst = (s + d * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// The webcam's square `face` cut into a circle at `(x, y)`, with a white
/// rim so that it reads on any background.
pub fn paint_bubble(pixels: &mut Pixels, face: &[u8], diameter: u32, (x, y): (u32, u32)) {
    let d = diameter as usize;
    if face.len() < d * d * 4 {
        return;
    }
    let (w, h) = (pixels.width as usize, pixels.height as usize);
    let r = diameter as f32 / 2.0;
    let rim = (diameter as f32 / 60.0).max(2.0);
    for fy in 0..d {
        let py = y as usize + fy;
        if py >= h {
            break;
        }
        for fx in 0..d {
            let px = x as usize + fx;
            if px >= w {
                break;
            }
            let dist = ((fx as f32 + 0.5 - r).powi(2) + (fy as f32 + 0.5 - r).powi(2)).sqrt();
            // Antialiased edge, a pixel wide.
            let cover = (r - dist).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let f = (fy * d + fx) * 4;
            let p = (py * w + px) * 4;
            let on_rim = dist > r - rim;
            for c in 0..3 {
                let source = if on_rim {
                    255.0
                } else {
                    f32::from(face[f + c])
                };
                let d = f32::from(pixels.rgba[p + c]);
                pixels.rgba[p + c] = (source * cover + d * (1.0 - cover)).round() as u8;
            }
        }
    }
}

/// Writes `frame` (or it with what goes over it) for each slot up to
/// `until`.
pub fn write_slots(
    out: &mut impl Write,
    frame: &[u8],
    (width, height): (u32, u32),
    slot: Duration,
    written: &mut u64,
    until: Duration,
    overlays: &Overlays,
) -> std::io::Result<()> {
    while slot * (*written as u32) < until {
        let at = slot * (*written as u32);
        if overlays.active(at) {
            out.write_all(&overlays.draw(frame, width, height, at))?;
        } else {
            out.write_all(frame)?;
        }
        *written += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey(w: u32, h: u32) -> Pixels {
        Pixels {
            width: w,
            height: h,
            rgba: [90, 90, 90, 255].repeat((w * h) as usize),
        }
    }

    #[test]
    fn keys_show_then_fade_then_go() {
        let caps = Overlay {
            width: 4,
            height: 2,
            rgba: [255, 255, 255, 255].repeat(8),
        };
        let o = Overlays {
            keys: Some((Duration::from_secs(1), caps)),
            ..Overlays::default()
        };
        let frame = grey(20, 20).rgba;
        let at = |ms| Duration::from_millis(ms);
        assert!(!o.active(at(500)), "not before");
        let shown = o.draw(&frame, 20, 20, at(1100));
        assert_ne!(shown, frame, "drawn");
        let fading = o.draw(&frame, 20, 20, at(2400));
        let differs = |a: &[u8]| {
            a.iter()
                .zip(&frame)
                .map(|(x, y)| x.abs_diff(*y) as u32)
                .sum::<u32>()
        };
        assert!(differs(&fading) < differs(&shown), "fading");
        assert!(!o.active(at(2600)), "gone");
    }

    #[test]
    fn the_bubble_is_round_with_a_rim() {
        let mut pixels = grey(64, 64);
        let face = [200, 0, 0, 255].repeat(32 * 32);
        paint_bubble(&mut pixels, &face, 32, (16, 16));
        let at = |x: usize, y: usize| &pixels.rgba[(y * 64 + x) * 4..(y * 64 + x) * 4 + 3];
        assert_eq!(at(32, 32), [200, 0, 0], "the face in the middle");
        assert_eq!(at(16, 16), [90, 90, 90], "the corner stays");
        assert_eq!(at(32, 17), [255, 255, 255], "the rim at the top");
    }
}
