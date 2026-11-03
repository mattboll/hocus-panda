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
    CaptureShown, Config, Headless, HeadlessConfig, OutputSpec, RecordSource, RendererChoice,
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
        Self::start_drawn(outputs, pipewire, change, RendererChoice::Pixman)
    }

    fn start_drawn(
        outputs: &[&str],
        pipewire: bool,
        change: impl FnOnce(&mut Config),
        renderer: RendererChoice,
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
                renderer,
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
fn area(hp: &mut Headless, x: f64, y: f64, w: f64, h: f64) -> anyhow::Result<()> {
    // Drawn with the pointer, as in 5.8.
    hp.pointer_move(x, y);
    hp.button(common::BTN_LEFT, true);
    for i in 1..=4 {
        let t = f64::from(i) / 4.0;
        hp.pointer_move(x + w * t, y + h * t);
    }
    hp.button(common::BTN_LEFT, false);
    wait(hp, Duration::from_millis(30))?;
    let CaptureShown::Choosing { area: Some(_), .. } = hp.capture() else {
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
    area(&mut bed.hp, 201.0, 151.0, 451.0, 301.0)?;
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
        let reds = frame.as_chunks::<3>().0.iter().filter(|p| red(**p)).count();
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
    area(&mut bed.hp, 300.0, 200.0, 400.0, 300.0)?;
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
    area(&mut bed.hp, 100.0, 100.0, 320.0, 240.0)?;
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
    area(&mut bed.hp, 300.0, 200.0, 400.0, 300.0)?;
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
        assert!(!frame.as_chunks::<3>().0.iter().any(|p| red(*p)));
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
    area(&mut bed.hp, 100.0, 100.0, 320.0, 200.0)?;
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

/// With the real shell (on the CPU), in French: its videos' folder, its
/// home and its `xdg-open` (which writes down what it opens) the test's.
struct ShellBed {
    hp: Headless,
    root: tempfile::TempDir,
    log: PathBuf,
}

impl ShellBed {
    fn start(output: &str, form: hocus_compositor::FormChoice) -> anyhow::Result<Option<Self>> {
        let Some(shell) = common::shell_program() else {
            eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
            return Ok(None);
        };
        if !ready() {
            return Ok(None);
        }
        let root = tempfile::tempdir()?;
        let dir = root.path();
        for sub in ["home", "config", "cache", "state", "bin", "launched"] {
            std::fs::create_dir_all(dir.join(sub))?;
        }
        let opener = dir.join("bin/xdg-open");
        std::fs::write(
            &opener,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\n",
                dir.join("launched/xdg-open").display()
            ),
        )?;
        std::fs::set_permissions(&opener, std::fs::Permissions::from_mode(0o755))?;
        let log = dir.join("shell.log");
        let home = |sub: &str| dir.join(sub).display().to_string();
        let script = common::shell_script(
            dir,
            &shell,
            &[
                ("LC_ALL", "fr_FR.UTF-8".to_owned()),
                ("LANG", "fr_FR.UTF-8".to_owned()),
                ("HOME", home("home")),
                ("XDG_CONFIG_HOME", home("config")),
                ("XDG_CACHE_HOME", home("cache")),
                ("XDG_STATE_HOME", home("state")),
                ("XDG_DATA_HOME", home("home")),
                ("XDG_DATA_DIRS", home("home")),
                ("PATH", home("bin")),
                ("ICED_BACKEND", "tiny-skia".to_owned()),
                ("RUST_LOG", "warn,hocuspanda_shell=debug".to_owned()),
            ],
        )?;
        let text = std::fs::read_to_string(&script)?
            .replace("\"$@\"\n", &format!("\"$@\" 2>>'{}'\n", log.display()));
        std::fs::write(&script, text)?;
        let mut config = Config::default();
        config.keyboard.layouts = vec!["us".to_owned()];
        config.plain_background = true;
        config.french = true;
        config.videos = Some(dir.join("Vidéos"));
        config.pipewire_remote = Some(dir.join("no-pipewire"));
        config.panel.form = form;
        let mut hp = Headless::start_with_shell(
            HeadlessConfig {
                outputs: vec![output.parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
            Some(script),
        )?;
        let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.levels_commits() > 0)?;
        anyhow::ensure!(up, "no bar");
        Ok(Some(Self { hp, root, log }))
    }

    fn log(&self) -> String {
        String::from_utf8_lossy(&std::fs::read(&self.log).unwrap_or_default()).into_owned()
    }

    fn click(&mut self, x: i32, y: i32) -> anyhow::Result<()> {
        self.hp.pointer_move(f64::from(x) - 1.0, f64::from(y));
        self.hp.pointer_move(f64::from(x), f64::from(y));
        wait(&mut self.hp, Duration::from_millis(120))?;
        self.hp.button(common::BTN_LEFT, true);
        self.hp.button(common::BTN_LEFT, false);
        wait(&mut self.hp, Duration::from_millis(60))
    }

    /// The bar's item (the sign's surface), once it shows `height` tall.
    fn item(&mut self, height: i32) -> anyhow::Result<hocus_compositor::model::Rect> {
        let shown = self.hp.dispatch_until(WAIT, |hp| {
            hp.layer_rects(hocus_link::IN_USE_NAMESPACE)
                .first()
                .is_some_and(|(_, r)| r.height == height)
        })?;
        let rects = self.hp.layer_rects(hocus_link::IN_USE_NAMESPACE);
        anyhow::ensure!(shown, "the bar's item is not {height} tall: {rects:?}");
        Ok(rects[0].1)
    }

    /// Records the screen from the compositor's side, until it starts.
    fn record_screen(&mut self) -> anyhow::Result<()> {
        chord(&mut self.hp, &[KEY_PRINT]);
        self.hp.capture_ask(CaptureAsk::Video(true));
        self.hp.capture_ask(CaptureAsk::Mode(CaptureMode::Screen));
        self.hp.capture_ask(CaptureAsk::Shoot);
        let started = self.hp.dispatch_until(SLOW, |hp| {
            hp.recording().is_some_and(|r| r.started) && hp.record_frames() > 0
        })?;
        anyhow::ensure!(started, "not started: {:?}", self.hp.record_failures());
        Ok(())
    }

    /// The thumbnail, once the recording is saved.
    fn thumbnail(&mut self) -> anyhow::Result<hocus_compositor::model::Rect> {
        let shown = self.hp.dispatch_until(SLOW, |hp| {
            hp.capture_thumb_surfaces()
                .first()
                .is_some_and(|(_, r)| r.width > 0)
        })?;
        anyhow::ensure!(
            shown,
            "no thumbnail ({:?}, {:?}):\n{}",
            self.hp.record_failures(),
            self.hp.recorded(),
            self.log()
        );
        Ok(self.hp.capture_thumb_surfaces()[0].1)
    }
}

/// The recording's item's height closed (6 + 10 + 4 + 16 + 6) and open
/// (and its buttons), with the surface's 5 px around it.
const ITEM_CLOSED: i32 = 42 + 10;
const ITEM_OPEN: i32 = 112 + 10;

#[test]
fn the_toolbar_in_video_has_the_switch_son_micro_and_demarrer() -> anyhow::Result<()> {
    let Some(mut bed) = ShellBed::start("1920x1080", hocus_compositor::FormChoice::Mini)? else {
        return Ok(());
    };
    bed.hp.pointer_move(400.0, 300.0);
    chord(&mut bed.hp, &[KEY_PRINT]);
    let shown = bed.hp.dispatch_until(WAIT, |hp| {
        hp.capture_toolbar_surfaces()
            .first()
            .is_some_and(|(_, r)| r.height > 0)
    })?;
    anyhow::ensure!(shown, "no toolbar");
    let photo = bed.hp.capture_toolbar_surfaces()[0].1;
    wait(&mut bed.hp, Duration::from_millis(250))?;
    // The switch's second tab, "Vidéo", at its left.
    bed.click(photo.x + 140, photo.y + photo.height / 2)?;
    let video = bed.hp.dispatch_until(WAIT, |hp| {
        matches!(hp.capture(), CaptureShown::Choosing { video: true, .. })
    })?;
    assert!(video, "not in Vidéo: {:?}\n{}", bed.hp.capture(), bed.log());
    // Its own buttons, its own size, still centred.
    let resized = bed.hp.dispatch_until(WAIT, |hp| {
        hp.capture_toolbar_surfaces()
            .first()
            .is_some_and(|(_, r)| r.width != photo.width)
    })?;
    assert!(resized, "the toolbar kept its size");
    let bar = bed.hp.capture_toolbar_surfaces()[0].1;
    assert!((bar.x + bar.width / 2 - 960).abs() <= 1, "{bar:?}");
    wait(&mut bed.hp, Duration::from_millis(250))?;
    // Micro, left of Démarrer: on.
    let y = bar.y + bar.height / 2;
    bed.click(bar.x + bar.width - 230, y)?;
    let micro = bed.hp.dispatch_until(WAIT, |hp| {
        matches!(hp.capture(), CaptureShown::Choosing { micro: true, .. })
    })?;
    assert!(micro, "Micro not on: {:?}\n{}", bed.hp.capture(), bed.log());
    bed.click(bar.x + bar.width - 230, y)?;
    let off = bed.hp.dispatch_until(WAIT, |hp| {
        matches!(
            hp.capture(),
            CaptureShown::Choosing {
                micro: false,
                sound: true,
                ..
            }
        )
    })?;
    assert!(off, "{:?}", bed.hp.capture());
    // Démarrer, at its right end: the countdown, the toolbar gone.
    bed.click(bar.x + bar.width - 40, y)?;
    let counting = bed.hp.dispatch_until(WAIT, |hp| {
        matches!(hp.capture(), CaptureShown::Counting { record: true, .. })
    })?;
    assert!(
        counting,
        "no countdown: {:?}\n{}",
        bed.hp.capture(),
        bed.log()
    );
    let gone = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.capture_toolbar_surfaces().is_empty())?;
    assert!(gone, "the toolbar stayed");
    let started = bed
        .hp
        .dispatch_until(SLOW, |hp| hp.recording().is_some_and(|r| r.started))?;
    assert!(started);
    bed.hp.record_ask(RecordAsk::Stop);
    bed.thumbnail()?;
    Ok(())
}

#[test]
fn the_bar_item_opens_on_hover_pauses_and_stops_then_the_thumbnail() -> anyhow::Result<()> {
    let Some(mut bed) = ShellBed::start("1920x1080", hocus_compositor::FormChoice::Mini)? else {
        return Ok(());
    };
    bed.hp.pointer_move(400.0, 300.0);
    bed.record_screen()?;
    // Under the clock, closed: its dot and its time.
    let item = bed.item(ITEM_CLOSED)?;
    let x = item.x + 27;
    // Hovered: it opens, the agents moving under it.
    bed.hp.pointer_move(f64::from(x), f64::from(item.y + 20));
    let item = bed.item(ITEM_OPEN)?;
    wait(&mut bed.hp, Duration::from_millis(150))?;
    // Pause.
    bed.click(x, item.y + 5 + 42 + 15)?;
    let paused = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.recording().is_some_and(|r| r.paused))?;
    assert!(paused, "not paused:\n{}", bed.log());
    wait(&mut bed.hp, Duration::from_millis(200))?;
    let told = bed.log();
    assert!(told.contains("paused: true"), "{told}");
    // Again: it goes on.
    bed.click(x, item.y + 5 + 42 + 15)?;
    let on = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.recording().is_some_and(|r| !r.paused))?;
    assert!(on, "not on again");
    // Stop: the item goes, the thumbnail comes.
    bed.click(x, item.y + 5 + 76 + 15)?;
    let stopped = bed.hp.dispatch_until(WAIT, |hp| hp.recording().is_none())?;
    assert!(stopped, "not stopped:\n{}", bed.log());
    bed.thumbnail()?;
    let gone = bed.hp.dispatch_until(WAIT, |hp| {
        hp.layer_rects(hocus_link::IN_USE_NAMESPACE).is_empty()
    })?;
    assert!(gone, "the item stayed");
    let said = bed.log();
    let line = said
        .lines()
        .rfind(|l| l.contains("the capture's thumbnail shows"))
        .unwrap_or_default();
    assert!(
        line.contains("Vidéos/Enregistrements d'écran/Enregistrement du ") && line.contains(".mp4"),
        "{line}"
    );
    Ok(())
}

#[test]
fn in_the_full_form_the_item_shows_its_buttons_always() -> anyhow::Result<()> {
    let Some(mut bed) = ShellBed::start("1920x1080", hocus_compositor::FormChoice::Full)? else {
        return Ok(());
    };
    bed.hp.pointer_move(400.0, 300.0);
    bed.record_screen()?;
    let item = bed.item(ITEM_OPEN)?;
    // Its stop, without hovering first: centred in the 196 px bar.
    let x = item.x + 97;
    bed.hp
        .pointer_move(f64::from(x), f64::from(item.y + 5 + 76 + 15));
    bed.click(x, item.y + 5 + 76 + 15)?;
    let stopped = bed.hp.dispatch_until(WAIT, |hp| hp.recording().is_none())?;
    assert!(stopped, "not stopped:\n{}", bed.log());
    bed.thumbnail()?;
    Ok(())
}

#[test]
fn its_thumbnail_opens_it_in_its_player_and_deletes_it() -> anyhow::Result<()> {
    let Some(mut bed) = ShellBed::start("1920x1080", hocus_compositor::FormChoice::Mini)? else {
        return Ok(());
    };
    bed.hp.pointer_move(400.0, 300.0);
    bed.record_screen()?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    bed.hp.record_ask(RecordAsk::Stop);
    let thumb = bed.thumbnail()?;
    let file = bed.hp.recorded().context("saved")?.file;
    assert_eq!(thumb.width, 330);
    // "Ouvrir": its player (xdg-open) is given the file.
    wait(&mut bed.hp, Duration::from_millis(300))?;
    bed.click(thumb.x + 232, thumb.y + 235)?;
    let launched = bed.root.path().join("launched/xdg-open");
    let opened = bed.hp.dispatch_until(WAIT, |_| launched.exists())?;
    assert!(opened, "not opened:\n{}", bed.log());
    let said = std::fs::read_to_string(&launched)?;
    assert_eq!(said.trim(), file.display().to_string());
    // Then it is in the notifications' history.
    let kept = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.capture_thumb_surfaces().is_empty())?;
    assert!(kept);
    assert!(
        bed.log().contains("the capture goes to the history"),
        "{}",
        bed.log()
    );
    // Another one, deleted from its ✕: its file goes.
    bed.record_screen()?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    bed.hp.record_ask(RecordAsk::Stop);
    let thumb = bed.thumbnail()?;
    let second = bed.hp.recorded().context("saved")?.file;
    assert_ne!(second, file);
    wait(&mut bed.hp, Duration::from_millis(300))?;
    bed.click(thumb.x + 298, thumb.y + 235)?;
    let deleted = bed.hp.dispatch_until(WAIT, |_| !second.exists())?;
    assert!(
        deleted,
        "{} is still there:\n{}",
        second.display(),
        bed.log()
    );
    assert!(file.exists());
    Ok(())
}

/// The pull request's screenshots, at 2560 × 1600 at scale 1, the shell on
/// the CPU: written to `$HOCUS_SHOTS` when it names a folder. Run by hand
/// (`--run-ignored only`).
#[test]
#[ignore = "writes the pull request's screenshots"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    for form in [
        hocus_compositor::FormChoice::Mini,
        hocus_compositor::FormChoice::Full,
    ] {
        let Some(mut bed) = ShellBed::start("2560x1600", form)? else {
            return Ok(());
        };
        let name = |what: &str| {
            let form = match form {
                hocus_compositor::FormChoice::Full => "full",
                _ => "mini",
            };
            out.join(format!("{what}-{form}.png"))
        };
        let shoot = |bed: &mut ShellBed,
                     file: PathBuf,
                     crop: Option<(i32, i32, i32, i32)>|
         -> anyhow::Result<()> {
            wait(&mut bed.hp, Duration::from_millis(400))?;
            let (w, h, px) = bed.hp.screenshot(0)?;
            let rgba: Vec<u8> = px
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[2], p[1], p[0], 255])
                .collect();
            let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
            let image = match crop {
                Some((x, y, cw, ch)) => {
                    image::imageops::crop_imm(&image, x as u32, y as u32, cw as u32, ch as u32)
                        .to_image()
                }
                None => image,
            };
            image.save(&file)?;
            eprintln!("{}", file.display());
            Ok(())
        };
        open(&mut bed.hp, "page", (1400, 900))?;
        bed.hp.pointer_move(900.0, 700.0);
        if form == hocus_compositor::FormChoice::Mini {
            // The toolbar in Vidéo, over an area.
            chord(&mut bed.hp, &[KEY_PRINT]);
            let shown = bed
                .hp
                .dispatch_until(WAIT, |hp| !hp.capture_toolbar_surfaces().is_empty())?;
            anyhow::ensure!(shown, "no toolbar");
            bed.hp.capture_ask(CaptureAsk::Video(true));
            wait(&mut bed.hp, Duration::from_millis(100))?;
            area(&mut bed.hp, 700.0, 450.0, 900.0, 600.0)?;
            bed.hp.pointer_move(1200.0, 1350.0);
            shoot(&mut bed, out.join("toolbar-video.png"), None)?;
            bed.hp.capture_ask(CaptureAsk::Shoot);
            wait(&mut bed.hp, Duration::from_millis(500))?;
            shoot(
                &mut bed,
                out.join("countdown.png"),
                Some((980, 500, 600, 600)),
            )?;
            let started = bed
                .hp
                .dispatch_until(SLOW, |hp| hp.recording().is_some_and(|r| r.started))?;
            anyhow::ensure!(started, "not started");
            wait(&mut bed.hp, Duration::from_millis(2200))?;
            shoot(&mut bed, out.join("recording-frame.png"), None)?;
        } else {
            bed.record_screen()?;
            wait(&mut bed.hp, Duration::from_millis(2200))?;
        }
        let bar = (2560 - 260, 0, 260, 520);
        let closed = if form == hocus_compositor::FormChoice::Full {
            ITEM_OPEN
        } else {
            ITEM_CLOSED
        };
        let item = bed.item(closed)?;
        shoot(&mut bed, name("bar-recording"), Some(bar))?;
        let x = if form == hocus_compositor::FormChoice::Full {
            item.x + 97
        } else {
            item.x + 27
        };
        bed.hp.pointer_move(f64::from(x), f64::from(item.y + 20));
        let item = bed.item(ITEM_OPEN)?;
        bed.hp
            .pointer_move(f64::from(x), f64::from(item.y + 5 + 42 + 15));
        shoot(&mut bed, name("bar-hovered"), Some(bar))?;
        bed.click(x, item.y + 5 + 42 + 15)?;
        let paused = bed
            .hp
            .dispatch_until(WAIT, |hp| hp.recording().is_some_and(|r| r.paused))?;
        anyhow::ensure!(paused, "not paused");
        shoot(&mut bed, name("bar-paused"), Some(bar))?;
        if form == hocus_compositor::FormChoice::Mini {
            shoot(&mut bed, out.join("recording-paused.png"), None)?;
        }
        bed.click(x, item.y + 5 + 42 + 15)?;
        wait(&mut bed.hp, Duration::from_millis(1500))?;
        bed.click(x, item.y + 5 + 76 + 15)?;
        let thumb = bed.thumbnail()?;
        let file = bed.hp.recorded().context("saved")?.file;
        if form == hocus_compositor::FormChoice::Mini {
            bed.hp
                .pointer_move(f64::from(thumb.x + 100), f64::from(thumb.y + 100));
            shoot(
                &mut bed,
                out.join("thumbnail.png"),
                Some((
                    thumb.x - 200,
                    thumb.y - 200,
                    2560 - thumb.x + 200,
                    1600 - thumb.y + 200,
                )),
            )?;
            std::fs::copy(&file, out.join("recording.mp4"))?;
        }
    }
    Ok(())
}

#[test]
fn on_gles_the_frames_read_back_off_the_loop_come_upright() -> anyhow::Result<()> {
    if !ready() {
        return Ok(());
    }
    // Mesa's software device, never a GPU; pixman without it.
    let renderer = common::gles().renderer;
    let mut bed = Bed::start_drawn(&["1280x800"], false, |_| {}, renderer)?;
    eprintln!("drawn with {}", bed.hp.renderer());
    let rect = open(&mut bed.hp, "page", (600, 400))?;
    bed.choose(CaptureMode::Area, false, false)?;
    // An area over the window's top left corner: the background at its
    // top left, the window at its bottom right.
    let (x, y) = (f64::from(rect.x) - 100.0, f64::from(rect.y) - 80.0);
    area(&mut bed.hp, x, y, 300.0, 240.0)?;
    bed.hp.pointer_move(5.0, 5.0);
    bed.start_recording()?;
    bed.record_for(Duration::from_secs(1))?;
    let file = bed.stop()?;
    let decoded = frames(&file, (300, 240))?;
    let last = decoded.last().context("no frame")?;
    let corner = at(last, 300, 10, 10);
    let window = at(last, 300, 280, 220);
    assert!(near(window, WINDOW), "{window:?}");
    assert!(corner.iter().all(|c| *c < 60), "{corner:?}");
    assert_eq!(bed.hp.record_copied_on_loop(), 0);
    Ok(())
}
