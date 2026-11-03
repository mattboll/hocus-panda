//! Screen recording (5.10, issue #400, docs/design/screenshots.md, "5.10"):
//! one test per point of the approved spec. The compositor records, so
//! most run without a shell: the toolbar's clicks and the bar's buttons
//! are given as the shell sends them (`capture video|sound|micro|shoot`,
//! `record pause|resume|stop`); those of the toolbar, the bar's item and
//! the thumbnail run the real shell (on the CPU).
//!
//! The files are decoded with ffmpeg and ffprobe, the encoder's own
//! programs; without them the tests skip. Pixman draws, the encoder runs
//! on the CPU (libx264): nothing here touches a GPU. The videos' folder,
//! the runtime directory and PipeWire (with Son or Micro) are the test's
//! own, PipeWire stopped by its own process id.

mod common;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::Context;
use common::{KEY_LEFTMETA, KEY_LEFTSHIFT, WAIT, WINDOW_COLOUR, chord, open, wait};
use hocus_compositor::{
    CaptureArea, CaptureShown, Config, Headless, HeadlessConfig, OutputSpec, RecordSource,
    RendererChoice,
};
use hocus_link::{CaptureAsk, CaptureMode, RecordAsk};

const KEY_PRINT: u32 = 99;
const KEY_R: u32 = 19;
/// How long a recording's start and end may take: ffmpeg starting, then
/// ending its file. A wait, never a budget.
const SLOW: Duration = Duration::from_secs(30);

fn has(program: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {program}")])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Whether ffmpeg, ffprobe and the software encoders are here.
fn ready() -> bool {
    if !has("ffmpeg") || !has("ffprobe") {
        eprintln!("ffmpeg or ffprobe missing: skipped");
        return false;
    }
    true
}

/// A PipeWire of the test's own, in a short runtime directory of its own.
struct Pipewire {
    daemon: Child,
    socket: PathBuf,
    _dir: tempfile::TempDir,
}

impl Pipewire {
    fn start() -> anyhow::Result<Option<Self>> {
        if !has("pipewire") {
            return Ok(None);
        }
        let dir = tempfile::Builder::new().prefix("hrw-").tempdir_in("/tmp")?;
        let run = dir.path().join("run");
        let home = dir.path().join("home");
        std::fs::create_dir_all(&run)?;
        std::fs::create_dir_all(&home)?;
        std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700))?;
        let daemon = Command::new("pipewire")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("XDG_RUNTIME_DIR", &run)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("DBUS_SYSTEM_BUS_ADDRESS", "unix:path=/nonexistent")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let socket = run.join("pipewire-0");
        let start = Instant::now();
        while !socket.exists() {
            anyhow::ensure!(start.elapsed() < WAIT, "PipeWire's socket never came");
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(Some(Self {
            daemon,
            socket,
            _dir: dir,
        }))
    }
}

impl Drop for Pipewire {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

/// A compositor on one screen, its videos' folder the test's own.
struct Bed {
    pub hp: Headless,
    dir: tempfile::TempDir,
    _pipewire: Option<Pipewire>,
}

impl Bed {
    fn start() -> anyhow::Result<Self> {
        Self::start_on(&["1280x800"], false)
    }

    fn start_on(outputs: &[&str], pipewire: bool) -> anyhow::Result<Self> {
        Self::start_with(outputs, pipewire, |_| {})
    }

    fn start_with(
        outputs: &[&str],
        pipewire: bool,
        change: impl FnOnce(&mut Config),
    ) -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let pipewire = if pipewire { Pipewire::start()? } else { None };
        let mut config = Config::default();
        config.keyboard.layouts = vec!["us".to_owned()];
        config.plain_background = true;
        config.videos = Some(dir.path().join("Videos"));
        config.state_dir = Some(dir.path().join("state"));
        // No PipeWire but the test's: a socket that is not there, else.
        config.pipewire_remote = Some(
            pipewire
                .as_ref()
                .map_or_else(|| dir.path().join("no-pipewire"), |p| p.socket.clone()),
        );
        change(&mut config);
        let outputs = outputs
            .iter()
            .map(|o| o.parse())
            .collect::<Result<Vec<OutputSpec>, _>>()?;
        let hp = Headless::start_with(
            HeadlessConfig {
                outputs,
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
        )?;
        let mut bed = Self {
            hp,
            dir,
            _pipewire: pipewire,
        };
        let drawn = bed.hp.dispatch_until(WAIT, |hp| hp.frames() > 0)?;
        anyhow::ensure!(drawn, "nothing drawn");
        Ok(bed)
    }

    fn folder(&self) -> PathBuf {
        self.dir.path().join("Videos/Screen recordings")
    }

    /// `Print`, Vidéo, the mode, and the area if one is given.
    fn choose(&mut self, mode: CaptureMode, sound: bool, micro: bool) -> anyhow::Result<()> {
        chord(&mut self.hp, &[KEY_PRINT]);
        anyhow::ensure!(
            matches!(self.hp.capture(), CaptureShown::Choosing { .. }),
            "the capture did not open"
        );
        self.hp.capture_ask(CaptureAsk::Video(true));
        self.hp.capture_ask(CaptureAsk::Mode(mode));
        let CaptureShown::Choosing {
            sound: was_sound,
            micro: was_micro,
            ..
        } = self.hp.capture()
        else {
            anyhow::bail!("the capture closed");
        };
        if was_sound != sound {
            self.hp.capture_ask(CaptureAsk::Sound);
        }
        if was_micro != micro {
            self.hp.capture_ask(CaptureAsk::Micro);
        }
        wait(&mut self.hp, Duration::from_millis(30))
    }

    /// Démarrer, the countdown, and the recording's first frames.
    fn start_recording(&mut self) -> anyhow::Result<()> {
        self.hp.capture_ask(CaptureAsk::Shoot);
        self.started()
    }

    /// The countdown, then the recording running and its first frames.
    fn started(&mut self) -> anyhow::Result<()> {
        let started = self.hp.dispatch_until(SLOW, |hp| {
            hp.recording().is_some_and(|r| r.started) && hp.record_frames() > 0
        })?;
        anyhow::ensure!(
            started,
            "the recording did not start: {:?}",
            self.hp.record_failures()
        );
        Ok(())
    }

    /// Records for `time`.
    fn record_for(&mut self, time: Duration) -> anyhow::Result<()> {
        wait(&mut self.hp, time)
    }

    /// Stopped from the bar, and its file once ended.
    fn stop(&mut self) -> anyhow::Result<PathBuf> {
        self.hp.record_ask(RecordAsk::Stop);
        self.saved()
    }

    /// The file of the recording stopped, once ended.
    fn saved(&mut self) -> anyhow::Result<PathBuf> {
        let done = self
            .hp
            .dispatch_until(SLOW, |hp| hp.recorded().is_some() && !hp.recording_ending())?;
        anyhow::ensure!(
            done,
            "the recording was not saved: {:?}",
            self.hp.record_failures()
        );
        Ok(self.hp.recorded().context("saved")?.file)
    }
}

/// What ffprobe says of a file's streams: each one's codec type, codec,
/// size and frame rate.
#[derive(Debug)]
struct Probed {
    streams: Vec<(String, String, u32, u32, String)>,
    duration: f64,
    format: String,
}

fn probe(file: &Path) -> anyhow::Result<Probed> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-of",
            "json",
            "-show_streams",
            "-show_format",
        ])
        .arg(file)
        .output()?;
    anyhow::ensure!(out.status.success(), "ffprobe: {}", out.status);
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let streams = json["streams"]
        .as_array()
        .context("streams")?
        .iter()
        .map(|s| {
            (
                s["codec_type"].as_str().unwrap_or_default().to_owned(),
                s["codec_name"].as_str().unwrap_or_default().to_owned(),
                s["width"].as_u64().unwrap_or(0) as u32,
                s["height"].as_u64().unwrap_or(0) as u32,
                s["r_frame_rate"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let duration = json["format"]["duration"]
        .as_str()
        .and_then(|d| d.parse().ok())
        .unwrap_or(0.0);
    let format = json["format"]["format_name"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    Ok(Probed {
        streams,
        duration,
        format,
    })
}

/// The file's frames, decoded: `count` of them, every `every` frames from
/// the first, as RGB rows of `size`.
fn frames(file: &Path, size: (u32, u32)) -> anyhow::Result<Vec<Vec<u8>>> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(file)
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()?;
    anyhow::ensure!(out.status.success(), "ffmpeg decoding: {}", out.status);
    let len = (size.0 * size.1 * 3) as usize;
    Ok(out.stdout.chunks_exact(len).map(<[u8]>::to_vec).collect())
}

/// A frame's pixel at (`x`, `y`).
fn at(frame: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * width + x) * 3) as usize;
    [frame[i], frame[i + 1], frame[i + 2]]
}

/// Whether `rgb` is near the recording frame's red.
fn red(rgb: [u8; 3]) -> bool {
    rgb[0] > 170 && rgb[1] < 120 && rgb[2] < 130
}

/// Whether `a` and `b` are near (the encoder's losses).
fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 24)
}

const WINDOW: [u8; 3] = [
    (WINDOW_COLOUR >> 16) as u8,
    (WINDOW_COLOUR >> 8) as u8,
    WINDOW_COLOUR as u8,
];

/// An area of the first screen, kept for the capture: the next capture
/// opens on it.
fn area(bed: &mut Bed, x: f64, y: f64, w: f64, h: f64) -> anyhow::Result<()> {
    // Drawn with the pointer, as in 5.8.
    bed.hp.pointer_move(x, y);
    bed.hp.button(common::BTN_LEFT, true);
    for i in 1..=4 {
        let t = f64::from(i) / 4.0;
        bed.hp.pointer_move(x + w * t, y + h * t);
    }
    bed.hp.button(common::BTN_LEFT, false);
    wait(&mut bed.hp, Duration::from_millis(30))?;
    let CaptureShown::Choosing { area: Some(_), .. } = bed.hp.capture() else {
        anyhow::bail!("no area drawn");
    };
    Ok(())
}

#[test]
fn an_area_is_recorded_to_an_mp4_at_30_fps_its_size_even_without_its_frame() -> anyhow::Result<()> {
    if !ready() {
        return Ok(());
    }
    let mut bed = Bed::start()?;
    open(&mut bed.hp, "page", (900, 600))?;
    bed.choose(CaptureMode::Area, false, false)?;
    // 451 × 301 logical pixels at scale 1: recorded at 450 × 300.
    area(&mut bed, 201.0, 151.0, 451.0, 301.0)?;
    bed.start_recording()?;
    let info = bed.hp.recording().context("recording")?;
    assert_eq!(info.size, (450, 300));
    assert!(matches!(info.source, RecordSource::Area { .. }), "{info:?}");
    // The frame shows on the screen, red, just outside the area.
    let (width, _, pixels) = bed.hp.screenshot(0)?;
    let p = |x: i32, y: i32| {
        let i = ((y * width + x) * 4) as usize;
        [pixels[i + 2], pixels[i + 1], pixels[i]]
    };
    assert!(red(p(201 + 2, 151 - 4)), "{:?}", p(201 + 2, 151 - 4));
    bed.record_for(Duration::from_secs(2))?;
    let file = bed.stop()?;
    // Its place and its name.
    assert_eq!(file.parent(), Some(bed.folder().as_path()));
    let name = file.file_name().context("name")?.to_string_lossy();
    assert!(
        name.starts_with("Screen recording from ") && name.ends_with(".mp4"),
        "{name}"
    );
    let probed = probe(&file)?;
    assert!(probed.format.contains("mp4"), "{probed:?}");
    assert_eq!(probed.streams.len(), 1, "{probed:?}");
    let (kind, codec, w, h, rate) = &probed.streams[0];
    assert_eq!((kind.as_str(), codec.as_str()), ("video", "h264"));
    assert_eq!((*w, *h), (450, 300));
    assert_eq!(rate, "30/1");
    assert!(probed.duration >= 1.5, "{probed:?}");
    // No frame of it shows the red frame, at its edges or anywhere; and
    // the window shows in it.
    let decoded = frames(&file, (450, 300))?;
    assert!(decoded.len() >= 45, "{} frames", decoded.len());
    for frame in &decoded {
        let reds = frame
            .chunks_exact(3)
            .filter(|p| red([p[0], p[1], p[2]]))
            .count();
        assert_eq!(reds, 0, "red in the video");
    }
    let middle = at(&decoded[decoded.len() / 2], 450, 225, 150);
    assert!(near(middle, WINDOW), "{middle:?}");
    // The hard rule's budget: no byte of its frames copied on the loop.
    assert_eq!(bed.hp.record_copied_on_loop(), 0);
    eprintln!(
        "a recorded frame's hand-over took the loop {:?} at most",
        bed.hp.record_lend_most()
    );
    Ok(())
}

/// The pixel at (`x`, `y`) of screen `index` as it shows: red, green, blue.
fn shown(bed: &mut Bed, index: usize, x: i32, y: i32) -> anyhow::Result<[u8; 3]> {
    let (width, _, pixels) = bed.hp.screenshot(index)?;
    let i = usize::try_from(y * width + x)? * 4;
    let p = pixels.get(i..i + 4).context("off the screen")?;
    Ok([p[2], p[1], p[0]])
}

#[test]
fn video_turns_the_frame_red_and_offers_area_window_and_screen_with_sound_on() -> anyhow::Result<()>
{
    let mut bed = Bed::start()?;
    chord(&mut bed.hp, &[KEY_PRINT]);
    let CaptureShown::Choosing {
        video,
        sound,
        micro,
        ..
    } = bed.hp.capture()
    else {
        anyhow::bail!("the capture did not open");
    };
    // Photo first; Son on and Micro off by default.
    assert_eq!((video, sound, micro), (false, true, false));
    area(&mut bed, 300.0, 200.0, 400.0, 300.0)?;
    // The area's border: white in Photo.
    let photo = shown(&mut bed, 0, 300, 300)?;
    assert!(photo.iter().all(|c| *c > 200), "{photo:?}");
    bed.hp.capture_ask(CaptureAsk::Video(true));
    wait(&mut bed.hp, Duration::from_millis(30))?;
    let video = shown(&mut bed, 0, 300, 300)?;
    assert!(red(video), "{video:?}");
    // No "all screens" for a video: the screen instead.
    bed.hp.capture_ask(CaptureAsk::Mode(CaptureMode::All));
    let CaptureShown::Choosing { mode, .. } = bed.hp.capture() else {
        anyhow::bail!("closed");
    };
    assert_eq!(mode, CaptureMode::Area);
    for mode in [CaptureMode::Window, CaptureMode::Screen, CaptureMode::Area] {
        bed.hp.capture_ask(CaptureAsk::Mode(mode));
        let CaptureShown::Choosing { mode: now, .. } = bed.hp.capture() else {
            anyhow::bail!("closed");
        };
        assert_eq!(now, mode);
    }
    // Photo, all screens, then Vidéo: its screen.
    bed.hp.capture_ask(CaptureAsk::Video(false));
    bed.hp.capture_ask(CaptureAsk::Mode(CaptureMode::All));
    bed.hp.capture_ask(CaptureAsk::Video(true));
    let CaptureShown::Choosing { mode, .. } = bed.hp.capture() else {
        anyhow::bail!("closed");
    };
    assert_eq!(mode, CaptureMode::Screen);
    // The toggles.
    bed.hp.capture_ask(CaptureAsk::Sound);
    bed.hp.capture_ask(CaptureAsk::Micro);
    let CaptureShown::Choosing { sound, micro, .. } = bed.hp.capture() else {
        anyhow::bail!("closed");
    };
    assert_eq!((sound, micro), (false, true));
    Ok(())
}

#[test]
fn demarrer_counts_down_3_2_1_with_a_red_ring_the_capture_gone() -> anyhow::Result<()> {
    if !ready() {
        return Ok(());
    }
    let mut bed = Bed::start()?;
    let before = shown(&mut bed, 0, 20, 20)?;
    bed.choose(CaptureMode::Screen, false, false)?;
    bed.hp.capture_ask(CaptureAsk::Shoot);
    assert_eq!(
        bed.hp.capture(),
        CaptureShown::Counting {
            left: 3,
            record: true
        }
    );
    assert!(bed.hp.recording().is_none());
    wait(&mut bed.hp, Duration::from_millis(400))?;
    // The capture's darkening is gone; the disc's red ring shows, 55 px
    // from its middle, zoomed (82 px).
    let outputs = bed.hp.outputs();
    let (_, screen) = outputs.first().context("a screen")?;
    let (cx, cy) = (screen.width / 2, screen.height / 2);
    let ring = shown(&mut bed, 0, cx + 81, cy)?;
    assert!(red(ring) || ring[0] > ring[1] + 60, "{ring:?}");
    assert_eq!(shown(&mut bed, 0, 20, 20)?, before);
    for left in [2, 1] {
        let counted = bed.hp.dispatch_until(WAIT, |hp| {
            hp.capture() == CaptureShown::Counting { left, record: true }
        })?;
        anyhow::ensure!(counted, "not {left}");
        assert!(bed.hp.recording().is_none());
    }
    bed.started()?;
    assert_eq!(bed.hp.capture(), CaptureShown::Closed);
    bed.stop()?;
    Ok(())
}

#[test]
fn super_shift_r_starts_with_the_last_settings_and_stops() -> anyhow::Result<()> {
    if !ready() {
        return Ok(());
    }
    let mut bed = Bed::start()?;
    open(&mut bed.hp, "page", (900, 600))?;
    bed.choose(CaptureMode::Area, false, false)?;
    area(&mut bed, 100.0, 100.0, 320.0, 240.0)?;
    bed.start_recording()?;
    let first = bed.hp.recording().context("recording")?;
    bed.record_for(Duration::from_millis(500))?;
    // The shortcut stops it.
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_R]);
    let file = bed.saved()?;
    assert!(file.exists());
    assert!(bed.hp.recording().is_none());
    // Again: the same area, counted down, recorded.
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_R]);
    assert_eq!(
        bed.hp.capture(),
        CaptureShown::Counting {
            left: 3,
            record: true
        }
    );
    bed.started()?;
    let second = bed.hp.recording().context("recording")?;
    assert_ne!(second.id, first.id);
    assert_eq!(second.source, first.source);
    assert_eq!(second.size, (320, 240));
    assert_eq!((second.sound, second.micro), (false, false));
    bed.record_for(Duration::from_millis(500))?;
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_R]);
    let again = bed.saved()?;
    assert_ne!(again, file);
    assert_eq!(probe(&again)?.streams[0].2, 320);
    Ok(())
}

#[test]
fn a_pause_is_left_out_of_the_file_and_dims_the_frame() -> anyhow::Result<()> {
    if !ready() {
        return Ok(());
    }
    let mut bed = Bed::start()?;
    bed.choose(CaptureMode::Area, false, false)?;
    area(&mut bed, 300.0, 200.0, 400.0, 300.0)?;
    bed.start_recording()?;
    let lit = shown(&mut bed, 0, 302, 200 - 4)?;
    assert!(red(lit), "{lit:?}");
    bed.record_for(Duration::from_secs(1))?;
    bed.hp.record_ask(RecordAsk::Pause);
    assert!(bed.hp.recording().context("recording")?.paused);
    wait(&mut bed.hp, Duration::from_millis(100))?;
    let sent = bed.hp.record_frames();
    let dimmed = shown(&mut bed, 0, 302, 200 - 4)?;
    assert!(
        dimmed[0] < lit[0] && dimmed[0] > 40,
        "{dimmed:?} after {lit:?}"
    );
    // Something changes while paused: no frame goes.
    open(&mut bed.hp, "page", (900, 600))?;
    bed.record_for(Duration::from_secs(2))?;
    assert_eq!(bed.hp.record_frames(), sent);
    bed.hp.record_ask(RecordAsk::Resume);
    assert!(!bed.hp.recording().context("recording")?.paused);
    bed.record_for(Duration::from_secs(1))?;
    let file = bed.stop()?;
    let probed = probe(&file)?;
    // About 2 s recorded, the 2 s paused left out.
    assert!(probed.duration > 1.5 && probed.duration < 3.2, "{probed:?}");
    Ok(())
}

#[test]
fn a_window_is_recorded_alone_at_its_size_its_frame_never_in_it() -> anyhow::Result<()> {
    if !ready() {
        return Ok(());
    }
    let mut bed = Bed::start()?;
    let rect = open(&mut bed.hp, "page", (641, 401))?;
    bed.choose(CaptureMode::Window, false, false)?;
    bed.hp.pointer_move(
        f64::from(rect.x + rect.width / 2),
        f64::from(rect.y + rect.height / 2),
    );
    wait(&mut bed.hp, Duration::from_millis(30))?;
    bed.start_recording()?;
    let info = bed.hp.recording().context("recording")?;
    assert!(matches!(info.source, RecordSource::Window(_)), "{info:?}");
    assert_eq!(info.size, (640, 400));
    // Its frame, on the screen, around it.
    let frame = shown(&mut bed, 0, rect.x + 2, rect.y - 4)?;
    assert!(red(frame), "{frame:?}");
    bed.record_for(Duration::from_secs(1))?;
    let file = bed.stop()?;
    let decoded = frames(&file, (640, 400))?;
    assert!(!decoded.is_empty());
    for frame in &decoded {
        assert!(!frame.chunks_exact(3).any(|p| red([p[0], p[1], p[2]])));
    }
    // The window's colour (the pointer, in its middle, is in it too).
    let inside = at(&decoded[0], 640, 600, 380);
    assert!(near(inside, WINDOW), "{inside:?}");
    Ok(())
}

#[test]
fn with_son_or_micro_the_file_has_a_sound_track() -> anyhow::Result<()> {
    if !ready() || !has("pipewire") {
        eprintln!("no pipewire: skipped");
        return Ok(());
    }
    for (sound, micro) in [(true, false), (false, true), (true, true)] {
        let mut bed = Bed::start_on(&["1280x800"], true)?;
        bed.choose(CaptureMode::Screen, sound, micro)?;
        bed.start_recording()?;
        let info = bed.hp.recording().context("recording")?;
        assert_eq!((info.sound, info.micro), (sound, micro));
        assert_eq!(info.size, (1280, 800));
        bed.record_for(Duration::from_secs(1))?;
        let file = bed.stop()?;
        let probed = probe(&file)?;
        let kinds: Vec<(&str, &str)> = probed
            .streams
            .iter()
            .map(|s| (s.0.as_str(), s.1.as_str()))
            .collect();
        assert_eq!(kinds, [("video", "h264"), ("audio", "aac")], "{probed:?}");
    }
    Ok(())
}

#[test]
fn webm_is_vp9_and_opus_as_its_setting_says() -> anyhow::Result<()> {
    if !ready() || !has("pipewire") {
        return Ok(());
    }
    let mut bed = Bed::start_with(&["1280x800"], true, |config| {
        config.recording.format = hocus_link::RecordFormat::WebM;
    })?;
    bed.choose(CaptureMode::Area, true, false)?;
    area(&mut bed, 100.0, 100.0, 320.0, 200.0)?;
    bed.start_recording()?;
    bed.record_for(Duration::from_secs(1))?;
    let file = bed.stop()?;
    assert!(file.to_string_lossy().ends_with(".webm"));
    let probed = probe(&file)?;
    let kinds: Vec<&str> = probed.streams.iter().map(|s| s.1.as_str()).collect();
    assert_eq!(kinds, ["vp9", "opus"], "{probed:?}");
    Ok(())
}

#[test]
fn one_recording_at_a_time() -> anyhow::Result<()> {
    if !ready() {
        return Ok(());
    }
    let mut bed = Bed::start()?;
    bed.choose(CaptureMode::Screen, false, false)?;
    bed.start_recording()?;
    let first = bed.hp.recording().context("recording")?;
    // The capture opens for photos; Démarrer records nothing more.
    bed.choose(CaptureMode::Area, false, false)?;
    bed.hp.capture_ask(CaptureAsk::Shoot);
    wait(&mut bed.hp, Duration::from_millis(100))?;
    assert!(matches!(bed.hp.capture(), CaptureShown::Choosing { .. }));
    assert_eq!(bed.hp.recording().map(|r| r.id), Some(first.id));
    chord(&mut bed.hp, &[common::KEY_ESC]);
    bed.stop()?;
    Ok(())
}

#[test]
fn the_screen_locking_stops_and_saves_it() -> anyhow::Result<()> {
    if !ready() {
        return Ok(());
    }
    let mut bed = Bed::start()?;
    bed.choose(CaptureMode::Screen, false, false)?;
    bed.start_recording()?;
    bed.record_for(Duration::from_millis(500))?;
    bed.hp.session_paused();
    assert!(bed.hp.recording().is_none());
    let file = bed.saved()?;
    assert!(probe(&file)?.duration > 0.3);
    Ok(())
}

#[test]
fn its_screen_unplugged_stops_and_saves_it() -> anyhow::Result<()> {
    if !ready() {
        return Ok(());
    }
    let mut bed = Bed::start_on(&["1280x800", "1024x768"], false)?;
    let outputs = bed.hp.outputs();
    let (name, second) = outputs.get(1).context("two screens")?.clone();
    bed.hp.pointer_move(
        f64::from(second.x + second.width / 2),
        f64::from(second.y + second.height / 2),
    );
    bed.choose(CaptureMode::Screen, false, false)?;
    bed.start_recording()?;
    let info = bed.hp.recording().context("recording")?;
    assert_eq!(info.source, RecordSource::Screen(name.clone()));
    assert_eq!(info.size, (1024, 768));
    bed.record_for(Duration::from_millis(500))?;
    bed.hp.unplug(&name)?;
    assert!(bed.hp.recording().is_none());
    let file = bed.saved()?;
    assert_eq!(probe(&file)?.streams[0].2, 1024);
    Ok(())
}

#[test]
fn deleting_it_from_its_thumbnail_removes_its_file() -> anyhow::Result<()> {
    if !ready() {
        return Ok(());
    }
    let mut bed = Bed::start()?;
    bed.choose(CaptureMode::Screen, false, false)?;
    bed.start_recording()?;
    bed.record_for(Duration::from_millis(300))?;
    let file = bed.stop()?;
    let id = bed.hp.recorded().context("saved")?;
    assert!(file.exists());
    assert!(id.poster.as_ref().is_some_and(|p| p.exists()), "{id:?}");
    bed.hp.record_ask(RecordAsk::Forget(1));
    let gone = bed.hp.dispatch_until(WAIT, |_| !file.exists())?;
    assert!(gone, "{} is still there", file.display());
    Ok(())
}
