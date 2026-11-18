//! The Hocuspanda camera (#619, lot 2, docs/design/camera.md): the
//! compositor starts `hocus-camera` (built beside the test, its frames made
//! on the CPU) against a PipeWire of the test's own, in a fresh short
//! runtime directory, with a fake camera of the test's: a `Video/Source`
//! node streaming a picture, never `/dev/video*` nor the session's
//! PipeWire. The reader is the camera window's preview
//! (`hocus_camera::preview`), linked to the source by itself as an
//! application's stream would be by a session manager.
//!
//! One test per point of the spec: a reader of "Hocuspanda" gets frames
//! of the camera, and the fake camera streams only while the source is
//! read; Lumière brings a dark picture's mean to its target, its colour
//! balanced; the screen light's `auto` strength follows the light
//! measured on the face, stronger in the dark, softer in the light. And
//! the window's logic, with the real shell (on the CPU): the camera's tile
//! opens it, its tiles write `[camera]`, and the frames follow. What the
//! window looks like is the maintainer's to see on screenshots
//! (`screenshots_of_the_window`, ignored). The arithmetic itself, on the
//! CPU and the GPU alike: `hocus-camera`'s unit tests.

use crate::common;
use crate::sharing::Pipewire;

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use common::wait;
use hocus_compositor::{
    Authenticator, Config, Headless, HeadlessConfig, LockState, RendererChoice,
};
use hocus_link::camera::AwayWhy;

const SIZE: (u32, u32) = (1280, 720);

/// The fake camera: its picture (B, G, R, x, the frame's size), changed as
/// the test goes, and whether a reader streams from it.
pub(crate) struct FakeCamera {
    picture: Arc<Mutex<Vec<u8>>>,
    streaming: Arc<AtomicBool>,
    quit: pipewire::channel::Sender<()>,
}

impl Drop for FakeCamera {
    fn drop(&mut self) {
        let _ = self.quit.send(());
    }
}

/// A frame of one colour (R, G, B), B, G, R, x.
fn flat([r, g, b]: [u8; 3]) -> Vec<u8> {
    [b, g, r, 255].repeat((SIZE.0 * SIZE.1) as usize)
}

impl FakeCamera {
    fn set(&self, colour: [u8; 3]) {
        *self.picture.lock().unwrap_or_else(PoisonError::into_inner) = flat(colour);
    }

    pub(crate) fn show(&self, picture: Vec<u8>) {
        *self.picture.lock().unwrap_or_else(PoisonError::into_inner) = picture;
    }

    fn streams(&self) -> bool {
        self.streaming.load(Ordering::SeqCst)
    }
}

/// A camera's node on `remote`, as PipeWire's would be: a driver sending a
/// frame of its picture 30 times a second while a reader streams from it.
pub(crate) fn fake_camera(remote: &std::path::Path, colour: [u8; 3]) -> FakeCamera {
    use pipewire as pw;
    use pw::spa::{
        self,
        pod::{Object, Pod, Property, PropertyFlags, Value},
        utils::{Fraction, Rectangle},
    };
    let picture = Arc::new(Mutex::new(flat(colour)));
    let streaming = Arc::new(AtomicBool::new(false));
    let (quit, quitting) = pw::channel::channel::<()>();
    let remote = remote.to_string_lossy().into_owned();
    let (paint, flag) = (picture.clone(), streaming.clone());
    std::thread::spawn(move || -> Result<(), pw::Error> {
        pw::init();
        let mainloop = pw::main_loop::MainLoopRc::new(None)?;
        let context = pw::context::ContextRc::new(&mainloop, None)?;
        let core = context.connect_rc(Some(pw::properties::properties! {
            *pw::keys::REMOTE_NAME => remote,
        }))?;
        let stream = pw::stream::StreamRc::new(
            core,
            "hocus-test-camera",
            pw::properties::properties! {
                *pw::keys::MEDIA_CLASS => "Video/Source",
                *pw::keys::MEDIA_ROLE => "Camera",
                *pw::keys::NODE_NAME => "hocus-test-camera",
                *pw::keys::NODE_DESCRIPTION => "Caméra de test",
            },
        )?;
        let on = flag.clone();
        let _listener = stream
            .add_local_listener::<()>()
            .state_changed(move |_, _, _, new| {
                on.store(new == pw::stream::StreamState::Streaming, Ordering::SeqCst);
            })
            .param_changed(|stream, _, id, param| {
                if id != spa::param::ParamType::Format.as_raw() || param.is_none() {
                    return;
                }
                let stride = SIZE.0 as usize * 4;
                let buffers = hocus_camera::pods::buffers(stride, stride * SIZE.1 as usize);
                if let Some(pod) = Pod::from_bytes(&buffers) {
                    let _ = stream.update_params(&mut [pod]);
                }
            })
            .register()?;
        let prop = |key: spa::param::format::FormatProperties, value: Value| Property {
            key: key.as_raw(),
            flags: PropertyFlags::empty(),
            value,
        };
        let id = |v: u32| Value::Id(spa::utils::Id(v));
        use spa::param::format::{FormatProperties as F, MediaSubtype, MediaType};
        let object = Object {
            type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
            id: spa::param::ParamType::EnumFormat.as_raw(),
            properties: vec![
                prop(F::MediaType, id(MediaType::Video.as_raw())),
                prop(F::MediaSubtype, id(MediaSubtype::Raw.as_raw())),
                prop(
                    F::VideoFormat,
                    id(spa::param::video::VideoFormat::BGRx.as_raw()),
                ),
                prop(
                    F::VideoSize,
                    Value::Rectangle(Rectangle {
                        width: SIZE.0,
                        height: SIZE.1,
                    }),
                ),
                prop(
                    F::VideoFramerate,
                    Value::Fraction(Fraction { num: 30, denom: 1 }),
                ),
            ],
        };
        let bytes = hocus_camera::pods::serialize(object);
        let Some(pod) = Pod::from_bytes(&bytes) else {
            return Err(pw::Error::CreationFailed);
        };
        stream.connect(
            spa::utils::Direction::Output,
            None,
            pw::stream::StreamFlags::DRIVER | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut [pod],
        )?;
        let sending = stream.clone();
        let timer = mainloop.loop_().add_timer(move |_| {
            if sending.state() != pw::stream::StreamState::Streaming {
                return;
            }
            let picture = paint.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(mut buffer) = sending.dequeue_buffer()
                && let Some(data) = buffer.datas_mut().first_mut()
            {
                let mut written = 0;
                if let Some(memory) = data.data() {
                    written = memory.len().min(picture.len());
                    memory[..written].copy_from_slice(&picture[..written]);
                }
                let chunk = data.chunk_mut();
                *chunk.offset_mut() = 0;
                *chunk.stride_mut() = (SIZE.0 * 4) as i32;
                *chunk.size_mut() = written as u32;
            }
            drop(picture);
            let _ = sending.trigger_process();
        });
        let frame = Duration::from_millis(33);
        let _ = timer.update_timer(Some(frame), Some(frame));
        let weak = mainloop.downgrade();
        let _quit = quitting.attach(mainloop.loop_(), move |()| {
            if let Some(mainloop) = weak.upgrade() {
                mainloop.quit();
            }
        });
        mainloop.run();
        flag.store(false, Ordering::SeqCst);
        Ok(())
    });
    FakeCamera {
        picture,
        streaming,
        quit,
    }
}

/// A reader of the Hocuspanda source: the camera window's preview, at
/// the source's own size; its frames' mean colour (R, G, B).
struct Reader {
    _preview: hocus_camera::preview::Preview,
    frames: mpsc::Receiver<[f64; 3]>,
}

fn read(remote: &std::path::Path) -> anyhow::Result<Reader> {
    let (tx, frames) = mpsc::channel();
    let preview = hocus_camera::preview::start(Some(remote.to_owned()), SIZE, move |frame| {
        let mut sum = [0f64; 3];
        for px in frame.rgba.as_chunks::<4>().0 {
            sum[0] += f64::from(px[0]);
            sum[1] += f64::from(px[1]);
            sum[2] += f64::from(px[2]);
        }
        let n = f64::from(frame.size.0 * frame.size.1).max(1.0);
        let _ = tx.send(sum.map(|s| s / n));
    })
    .ok_or_else(|| anyhow::anyhow!("no preview"))?;
    Ok(Reader {
        _preview: preview,
        frames,
    })
}

struct Bed {
    hp: Headless,
    camera: FakeCamera,
    pipewire: Pipewire,
}

fn start(colour: [u8; 3], change: impl FnOnce(&mut Config)) -> anyhow::Result<Bed> {
    let program = common::camera_program();
    if !std::process::Command::new("sh")
        .args(["-c", "command -v pipewire"])
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
    {
        common::missing("pipewire");
    }
    let pipewire = Pipewire::start("unix:path=/nonexistent")?;
    let camera = fake_camera(&pipewire.socket, colour);
    let mut config = Config {
        plain_background: true,
        pipewire_remote: Some(pipewire.socket.clone()),
        camera_program: Some(program),
        camera_device: "cpu".to_owned(),
        // No v4l2loopback device: never one of the machine's.
        camera_video4linux: Some(pipewire.socket.with_file_name("video4linux")),
        ..Config::default()
    };
    config.keyboard.layouts = vec!["us".to_owned()];
    change(&mut config);
    let hp = Headless::start_with(
        HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    Ok(Bed {
        hp,
        camera,
        pipewire,
    })
}

impl Bed {
    /// The next frame's mean colour, within `within`, the loop turning.
    fn frame(&mut self, reader: &Reader, within: Duration) -> anyhow::Result<[f64; 3]> {
        let start = Instant::now();
        while start.elapsed() < within {
            if let Ok(mean) = reader.frames.try_recv() {
                return Ok(mean);
            }
            wait(&mut self.hp, Duration::from_millis(20))?;
        }
        anyhow::bail!("no frame from the Hocuspanda camera")
    }

    /// The last of the frames that came, after `settle` of them.
    fn settled(&mut self, reader: &Reader, settle: usize) -> anyhow::Result<[f64; 3]> {
        let mut mean = self.frame(reader, Duration::from_secs(20))?;
        for _ in 0..settle {
            mean = self.frame(reader, Duration::from_secs(5))?;
        }
        Ok(mean)
    }

    /// Runs until `done`, or fails saying `what`.
    fn until(
        &mut self,
        within: Duration,
        what: &str,
        mut done: impl FnMut(&mut Self) -> bool,
    ) -> anyhow::Result<()> {
        let start = Instant::now();
        while start.elapsed() < within {
            if done(self) {
                return Ok(());
            }
            wait(&mut self.hp, Duration::from_millis(20))?;
        }
        anyhow::bail!("{what}: {:?}", self.hp.camera())
    }
}

fn luma([r, g, b]: [f64; 3]) -> f64 {
    0.299 * r + 0.587 * g + 0.114 * b
}

#[test]
fn a_reader_gets_the_cameras_frames_and_the_camera_is_read_only_meanwhile() -> anyhow::Result<()> {
    let mut bed = start([90, 120, 150], |c| c.camera.lighting = false)?;
    // On the graph, unread: the camera is not read.
    let reader_less = Instant::now();
    while reader_less.elapsed() < Duration::from_secs(2) {
        wait(&mut bed.hp, Duration::from_millis(50))?;
        anyhow::ensure!(!bed.camera.streams(), "the camera is read for nothing");
    }
    let reader = read(&bed.pipewire.socket)?;
    let mean = bed.settled(&reader, 3)?;
    for (got, want) in mean.iter().zip([90.0, 120.0, 150.0]) {
        assert!((got - want).abs() < 2.0, "{mean:?}");
    }
    assert!(bed.camera.streams(), "the camera is not read");
    bed.until(Duration::from_secs(5), "not read", |bed| bed.hp.camera().0)?;
    // The last reader goes: so does the camera.
    drop(reader);
    bed.until(Duration::from_secs(10), "the camera is still read", |bed| {
        !bed.camera.streams() && !bed.hp.camera().0
    })?;
    // And it comes back with the next one.
    let again = read(&bed.pipewire.socket)?;
    bed.frame(&again, Duration::from_secs(20))?;
    assert!(bed.camera.streams());
    Ok(())
}

#[test]
fn lumiere_brings_a_dark_blue_picture_to_its_target_balanced() -> anyhow::Result<()> {
    let dark = [50, 55, 75];
    let mut bed = start(dark, |c| c.camera.lighting = true)?;
    let reader = read(&bed.pipewire.socket)?;
    // The correction follows over a few frames.
    let mean = bed.settled(&reader, 30)?;
    let target = f64::from(hocus_camera::look::TARGET) * 255.0;
    let before = luma(dark.map(f64::from));
    let after = luma(mean);
    assert!(
        (after - target).abs() < 12.0 && after > before + 40.0,
        "{dark:?} → {mean:?}: {before:.0} → {after:.0}, target {target:.0}"
    );
    // Its blue cast mostly gone, a little warm.
    let cast = |[r, _, b]: [f64; 3]| b - r;
    assert!(
        cast(mean) < cast(dark.map(f64::from)) / 2.0,
        "{dark:?} → {mean:?}"
    );
    Ok(())
}

#[test]
fn the_screen_light_follows_the_light_on_the_face() -> anyhow::Result<()> {
    let mut bed = start([25, 25, 25], |c| c.camera.lighting = true)?;
    assert_eq!(bed.hp.camera().2, 0, "soft while nothing measures");
    let reader = read(&bed.pipewire.socket)?;
    bed.frame(&reader, Duration::from_secs(20))?;
    // A dark face: the light grows, step by step, to its strongest.
    let steps = Rc::new(RefCell::new(Vec::new()));
    let seen = steps.clone();
    bed.until(Duration::from_secs(15), "not at its strongest", |bed| {
        let level = bed.hp.camera().2;
        seen.borrow_mut().push(level);
        level == 8
    })?;
    let climbed: Vec<u8> = {
        let mut v = steps.borrow().clone();
        v.dedup();
        v
    };
    assert!(
        climbed.windows(2).all(|w| w[1] == w[0] + 1),
        "one step at a time: {climbed:?}"
    );
    // Daylight on the face: it softens to the softest.
    bed.camera.set([220, 210, 200]);
    bed.until(Duration::from_secs(15), "not back to soft", |bed| {
        bed.hp.camera().2 == 0
    })?;
    // Between the two, it holds.
    bed.camera.set([115, 115, 115]);
    let held = Instant::now();
    wait(&mut bed.hp, Duration::from_secs(2))?;
    let level = bed.hp.camera().2;
    while held.elapsed() < Duration::from_secs(4) {
        wait(&mut bed.hp, Duration::from_millis(100))?;
        assert_eq!(bed.hp.camera().2, level, "it moves in a lit room");
    }
    drop(reader);
    // Nothing read, nothing measured: soft again.
    bed.until(Duration::from_secs(10), "not soft once unread", |bed| {
        bed.hp.camera() == (false, None, 0)
    })?;
    Ok(())
}

// --- The camera's window, with the real shell ----------------------------------------

/// The window's measures, as `popover::camera` lays it out at scale 1:
/// its content 15 px in, the grid's top, a tile's width and step.
const IN: f64 = 15.0;
const GRID_TOP: f64 = IN + 15.7 + 9.0 + 176.0 + 9.0 + 30.0 + 9.0;
const TILE_STEP: f64 = 73.0 + 6.0;
const TILE_HALF: (f64, f64) = (36.5, 31.0);

/// A dim room with someone in it, a little blue, B, G, R, x: what a
/// laptop's camera sees in the evening.
fn someone() -> Vec<u8> {
    let (w, h) = (SIZE.0 as f64, SIZE.1 as f64);
    let mut frame = Vec::with_capacity((SIZE.0 * SIZE.1 * 4) as usize);
    let inside = |x: f64, y: f64, (cx, cy): (f64, f64), (rx, ry): (f64, f64)| {
        ((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2) <= 1.0
    };
    for y in 0..SIZE.1 {
        for x in 0..SIZE.0 {
            let (fx, fy) = (f64::from(x), f64::from(y));
            let t = fy / h;
            // The wall, darker down; a window's light on the left.
            let mut rgb = [70.0 - 15.0 * t, 74.0 - 16.0 * t, 92.0 - 18.0 * t];
            if (60.0..300.0).contains(&fx) && (90.0..330.0).contains(&fy) {
                rgb = [112.0, 120.0, 140.0];
            }
            // The shoulders, the neck, the face, the hair, the eyes.
            if inside(fx, fy, (w / 2.0, h + 60.0), (330.0, 250.0)) {
                rgb = [42.0, 58.0, 92.0];
            }
            if inside(fx, fy, (w / 2.0, 470.0), (52.0, 80.0)) {
                rgb = [130.0, 98.0, 84.0];
            }
            if inside(fx, fy, (w / 2.0, 320.0), (128.0, 160.0)) {
                let lit = 1.0 - ((fx - w / 2.0) / 300.0).abs();
                rgb = [150.0 * lit, 114.0 * lit, 98.0 * lit];
            }
            if inside(fx, fy, (w / 2.0, 210.0), (140.0, 90.0)) && fy < 230.0 {
                rgb = [52.0, 40.0, 34.0];
            }
            for eye in [w / 2.0 - 48.0, w / 2.0 + 48.0] {
                if inside(fx, fy, (eye, 320.0), (11.0, 13.0)) {
                    rgb = [38.0, 30.0, 28.0];
                }
            }
            let [r, g, b] = rgb.map(|v: f64| v.clamp(0.0, 255.0) as u8);
            frame.extend_from_slice(&[b, g, r, 255]);
        }
    }
    frame
}

/// A call's reader of the Hocuspanda source ("Visio", whose stream the
/// in-use sign counts): the mean brightness of its frames' banner's band
/// (their bottom 131 rows), and of the rest.
struct Call {
    _reader: hocus_camera::preview::Preview,
    frames: mpsc::Receiver<(f64, f64)>,
}

fn call(remote: &std::path::Path) -> anyhow::Result<Call> {
    let (tx, frames) = mpsc::channel();
    let band = (SIZE.1 - 131) as usize * SIZE.0 as usize * 4;
    let reader = hocus_camera::preview::start_as(
        Some(remote.to_owned()),
        SIZE,
        ("visio-camera", "Visio"),
        move |frame| {
            let mean = |px: &[u8]| {
                let chunks = px.as_chunks::<4>().0;
                let sum: f64 = chunks
                    .iter()
                    .map(|p| luma([p[0], p[1], p[2]].map(f64::from)))
                    .sum();
                sum / chunks.len().max(1) as f64
            };
            let (top, bottom) = frame.rgba.split_at(band.min(frame.rgba.len()));
            let _ = tx.send((mean(bottom), mean(top)));
        },
    )
    .ok_or_else(|| anyhow::anyhow!("no call"))?;
    Ok(Call {
        _reader: reader,
        frames,
    })
}

/// The real shell (on the CPU, in French) with the fakes of its services
/// (`levels/fakes.rs`: their PipeWire, the test camera on it), the
/// configuration a file of the test's own, a call reading the Hocuspanda
/// source.
struct WindowBed {
    hp: Headless,
    camera: FakeCamera,
    call: Call,
    log: std::path::PathBuf,
    file: std::path::PathBuf,
    fakes: crate::fakes::Fakes,
    root: tempfile::TempDir,
}

/// The tests' password for the lock.
const PASSWORD: &str = "panda";
/// The default source, when the bed has one.
const MIC: &str = "hocus-test-mic";

impl WindowBed {
    fn start(file: &str) -> anyhow::Result<Self> {
        Self::start_on(file, "1600x1000")
    }

    fn start_on(file: &str, screen: &str) -> anyhow::Result<Self> {
        Self::start_full(file, screen, false)
    }

    /// The bed with a microphone, the default source ([`MIC`]), and the
    /// lock opening for [`PASSWORD`].
    fn start_with_mic(file: &str) -> anyhow::Result<Self> {
        Self::start_full(file, "1600x1000", true)
    }

    fn start_full(file: &str, screen: &str, mic: bool) -> anyhow::Result<Self> {
        let shell = common::shell_program();
        let program = common::camera_program();
        if !crate::fakes::available() {
            common::missing("dbus-daemon, pipewire, pw-dump, wpctl and pw-metadata");
        }
        let root = tempfile::tempdir()?;
        let dir = root.path();
        for sub in [
            "home",
            "config/hocuspanda",
            "cache",
            "state",
            "data",
            "devices",
        ] {
            std::fs::create_dir_all(dir.join(sub))?;
        }
        let config_file = dir.join("config/hocuspanda/config.toml");
        std::fs::write(&config_file, file)?;
        let fakes = if mic {
            crate::fakes::Fakes::start_with_sources(
                &crate::fakes::TWO_SINKS,
                &[crate::fakes::FakeSink::new(MIC, "Micro de test")],
            )?
        } else {
            crate::fakes::Fakes::start()?
        };
        let log = dir.join("shell.log");
        let at = |sub: &str| dir.join(sub).display().to_string();
        let script = fakes.shell_with(
            &shell,
            "fr_FR.UTF-8",
            &[
                ("HOME", at("home")),
                ("XDG_CONFIG_HOME", at("config")),
                ("XDG_CACHE_HOME", at("cache")),
                ("XDG_STATE_HOME", at("state")),
                ("XDG_DATA_HOME", at("data")),
                ("XDG_DATA_DIRS", at("data")),
                ("HOCUS_VIDEO_DEVICES", at("devices")),
                ("HOCUS_VIDEO4LINUX", at("video4linux")),
                ("ICED_BACKEND", "tiny-skia".to_owned()),
                ("TZ", "UTC".to_owned()),
                ("RUST_LOG", "warn,hocuspanda_shell=debug".to_owned()),
            ],
            Some(&log),
        )?;
        let mut config = Config::load(&config_file);
        config.file = Some(config_file.clone());
        config.plain_background = true;
        config.lang = hocus_words::Lang::French;
        config.pipewire_remote = Some(fakes.pw_socket());
        config.camera_program = Some(program);
        config.camera_device = "cpu".to_owned();
        config.camera_video4linux = Some(dir.join("video4linux"));
        config.state_dir = Some(dir.join("state/hocuspanda"));
        config.keyboard.layouts = vec!["us".to_owned()];
        config.lock_auth = Authenticator::test(PASSWORD);
        let mut hp = Headless::start_on_bus(
            HeadlessConfig {
                outputs: vec![screen.parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
            Some(script),
            Some(fakes.address.clone()),
        )?;
        let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.levels_commits() > 0)?;
        anyhow::ensure!(up, "no bar");
        hp.pointer_move(400.0, 600.0);
        let camera = fake_camera(&fakes.pw_socket(), [0, 0, 0]);
        camera.show(someone());
        // The source on the graph, then the call reading it.
        let ready = Instant::now();
        while !hp.camera_ready() {
            anyhow::ensure!(
                ready.elapsed() < Duration::from_secs(20),
                "no Hocuspanda camera"
            );
            wait(&mut hp, Duration::from_millis(50))?;
        }
        let call = call(&fakes.pw_socket())?;
        let mut bed = Self {
            hp,
            camera,
            call,
            log,
            file: config_file,
            fakes,
            root,
        };
        bed.band(Duration::from_secs(20))?;
        Ok(bed)
    }

    /// The shell's log, without its colours.
    fn log(&self) -> String {
        let text =
            String::from_utf8_lossy(&std::fs::read(&self.log).unwrap_or_default()).into_owned();
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    /// Waits until the shell logs `what` on a line holding all of `with`.
    fn logged(&mut self, what: &str, with: &[&str]) -> anyhow::Result<String> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(15) {
            if let Some(line) = self
                .log()
                .lines()
                .rev()
                .find(|l| l.contains(what) && with.iter().all(|w| l.contains(w)))
            {
                return Ok(line.to_owned());
            }
            wait(&mut self.hp, Duration::from_millis(50))?;
        }
        anyhow::bail!(
            "the shell did not log {what:?} with {with:?}:\n{}",
            self.log()
        )
    }

    /// The call's next frame's band's and rest's brightness.
    fn band(&mut self, within: Duration) -> anyhow::Result<(f64, f64)> {
        let start = Instant::now();
        let mut last = None;
        while start.elapsed() < within {
            while let Ok(frame) = self.call.frames.try_recv() {
                last = Some(frame);
            }
            if let Some(frame) = last {
                return Ok(frame);
            }
            wait(&mut self.hp, Duration::from_millis(20))?;
        }
        anyhow::bail!("no frame for the call")
    }

    /// The file once `pass` takes it.
    fn written(&mut self, pass: impl Fn(&str) -> bool) -> anyhow::Result<String> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            let text = std::fs::read_to_string(&self.file).unwrap_or_default();
            if pass(&text) {
                return Ok(text);
            }
            wait(&mut self.hp, Duration::from_millis(50))?;
        }
        anyhow::bail!(
            "not written:\n{}",
            std::fs::read_to_string(&self.file).unwrap_or_default()
        )
    }

    /// The window, opened by a click on the camera's tile of the in-use
    /// sign: where it is.
    fn open(&mut self) -> anyhow::Result<hocus_compositor::model::Rect> {
        self.logged("the in-use sign is drawn at rest", &["kinds=[Camera]"])?;
        let sign = common::drawn(&mut self.hp, hocus_link::IN_USE_NAMESPACE)?[0].1;
        let at = (
            f64::from(sign.x) + f64::from(sign.width) / 2.0,
            f64::from(sign.y) + f64::from(sign.height) / 2.0,
        );
        common::click_on(&mut self.hp, at, hocus_link::IN_USE_NAMESPACE)?;
        let open = self
            .hp
            .dispatch_until(WAIT, |hp| !common::drawn_popovers(hp).is_empty())?;
        anyhow::ensure!(open, "no window:\n{}", self.log());
        self.logged("the camera's window shows it", &["CAMÉRA ET MICRO"])?;
        common::at_rest(&mut self.hp)?;
        Ok(common::drawn_popovers(&self.hp)[0])
    }

    /// A click on tile `n` (from 0) of the window at `window`.
    fn tile(&mut self, window: hocus_compositor::model::Rect, n: usize) -> anyhow::Result<()> {
        let at = (
            f64::from(window.x) + IN + TILE_STEP * n as f64 + TILE_HALF.0,
            f64::from(window.y) + GRID_TOP + TILE_HALF.1,
        );
        common::click_on(&mut self.hp, at, hocus_link::POPOVER_NAMESPACE)
    }
}

const WAIT: Duration = common::WAIT;

/// The camera's tile opens the window; its Bandeau's tile writes `banner =
/// true`, the window shows it chosen and its tab's dot, and the call's
/// frames get the banner; Lumière's writes `lighting = false`. Who reads
/// the camera: the call, as the in-use sign says it.
#[test]
fn the_window_s_tiles_write_the_effects_and_the_frames_follow() -> anyhow::Result<()> {
    let mut bed = WindowBed::start("[camera]\nbanner-name = \"Alex Martin\"\n")?;
    let window = bed.open()?;
    assert!(bed.camera.streams(), "the camera is not read");
    bed.logged(
        "the camera's window shows it",
        &[
            "Utilisée par Visio",
            "[x] Lumière",
            "[~] Cadrage",
            "[ ] Bandeau",
        ],
    )?;
    let (plain, _) = bed.band(Duration::from_secs(5))?;
    bed.tile(window, 2)?;
    bed.written(|t| t.contains("banner = true"))?;
    bed.logged(
        "the camera's window shows it",
        &["[x] Bandeau", "(dots: Image, Réactions)"],
    )?;
    // The compositor follows the file and tells the camera: the band
    // darkens in the call's frames.
    let start = Instant::now();
    loop {
        let (band, _) = bed.band(Duration::from_secs(5))?;
        if band < plain - 15.0 {
            break;
        }
        anyhow::ensure!(
            start.elapsed() < Duration::from_secs(10),
            "no banner in the frames: {band:.0} (was {plain:.0})"
        );
    }
    bed.tile(window, 0)?;
    bed.written(|t| t.contains("lighting = false"))?;
    bed.logged(
        "the camera's window shows it",
        &["[ ] Lumière", "[x] Bandeau"],
    )?;
    Ok(())
}

impl WindowBed {
    /// Desktop 1 made project `name`, its folder the test's own, as the
    /// Settings' line makes one (`project-make`).
    fn project(&mut self, name: &str) -> anyhow::Result<()> {
        use std::io::{BufRead, Write};
        let folder = self.root.path().join(name);
        std::fs::create_dir_all(&folder)?;
        let path = self
            .hp
            .control_socket()
            .ok_or_else(|| anyhow::anyhow!("no control socket"))?
            .to_owned();
        let mut stream = std::os::unix::net::UnixStream::connect(path)?;
        stream.write_all(b"msg screens\n")?;
        let reader = stream.try_clone()?;
        let (said, heard) = mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(reader)
                .lines()
                .map_while(Result::ok)
            {
                if said.send(line).is_err() {
                    return;
                }
            }
        });
        let mut lines = 0;
        self.hp.dispatch_until(WAIT, |_| {
            lines += heard.try_iter().count();
            lines > 0
        })?;
        stream.write_all(
            format!(
                "project-make\tHEADLESS-1\t1\t{name}\t{}\n",
                folder.display()
            )
            .as_bytes(),
        )?;
        let mut made = false;
        self.hp.dispatch_until(WAIT, |_| {
            made |= heard.try_iter().any(|l| l.contains("project-made"));
            made
        })?;
        anyhow::ensure!(made, "project {name} not made");
        Ok(())
    }

    /// What the first screen shows around `rect` (logical pixels, `more`
    /// around it), at its `scale`, saved in `out` as `name`.
    fn shoot(
        &mut self,
        out: &std::path::Path,
        name: &str,
        rect: (i32, i32, i32, i32),
        scale: i32,
    ) -> anyhow::Result<()> {
        use anyhow::Context;
        common::at_rest(&mut self.hp)?;
        wait(&mut self.hp, Duration::from_millis(300))?;
        let (w, h, px) = self.hp.screenshot(0)?;
        let rgba: Vec<u8> = px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
        let (x, y, cw, ch) = rect;
        let clamp = |v: i32, most: i32| v.clamp(0, most) as u32;
        let (x0, y0) = (clamp(x * scale, w), clamp(y * scale, h));
        let (x1, y1) = (clamp((x + cw) * scale, w), clamp((y + ch) * scale, h));
        let image = image::imageops::crop_imm(&image, x0, y0, x1 - x0, y1 - y0).to_image();
        let file = out.join(name);
        image.save(&file)?;
        eprintln!("{}", file.display());
        Ok(())
    }
}

/// The pull request's screenshots of the camera's window (#619, lot 2),
/// at 2560 × 1600 at scale 2, the shell on the CPU, the test camera's
/// evening picture through hocus-camera: the Image tab with Lumière and
/// Bandeau on (the project desktop's name under the user's), then without
/// Lumière; the camera's tile and its violet dot; a tab to come; the help.
/// Written to `$HOCUS_SHOTS` when it names a folder. Run by hand
/// (`--run-ignored only`).
#[test]
#[ignore = "writes the pull request's screenshots"]
fn screenshots_of_the_window() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(std::path::PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let mut bed = WindowBed::start_on(
        "[camera]\nbanner = true\nbanner-name = \"Alex Martin\"\n",
        "2560x1600@2",
    )?;
    bed.project("hocus-panda")?;
    let window = bed.open()?;
    bed.logged("the camera's preview shows a frame", &[])?;
    // Lumière's correction settles over a few frames.
    wait(&mut bed.hp, Duration::from_secs(2))?;
    let around =
        |r: hocus_compositor::model::Rect| (r.x - 16, r.y - 16, r.width + 32, r.height + 32);
    bed.shoot(&out, "camera-window-image.png", around(window), 2)?;
    let sign = common::drawn(&mut bed.hp, hocus_link::IN_USE_NAMESPACE)?[0].1;
    bed.shoot(
        &out,
        "camera-tile-dot.png",
        (sign.x - 40, sign.y - 30, sign.width + 80, sign.height + 60),
        2,
    )?;
    bed.tile(window, 0)?;
    bed.logged("the camera's window shows it", &["[ ] Lumière"])?;
    wait(&mut bed.hp, Duration::from_secs(1))?;
    bed.shoot(&out, "camera-window-without-lumiere.png", around(window), 2)?;
    bed.tile(window, 0)?;
    bed.logged("the camera's window shows it", &["[x] Lumière"])?;
    // Visage: its tiles to come.
    let tab = |i: f64| {
        (
            f64::from(window.x) + IN + 2.0 + i * 61.6 + 29.8,
            f64::from(window.y) + IN + 15.7 + 9.0 + 176.0 + 9.0 + 15.0,
        )
    };
    common::click_on(&mut bed.hp, tab(2.0), hocus_link::POPOVER_NAMESPACE)?;
    bed.logged("the camera's window shows it", &["| Visage (dots"])?;
    let window = common::drawn_popovers(&bed.hp)[0];
    bed.shoot(&out, "camera-window-visage.png", around(window), 2)?;
    common::click_on(&mut bed.hp, tab(0.0), hocus_link::POPOVER_NAMESPACE)?;
    bed.logged("the camera's window shows it", &["| Image (dots"])?;
    let window = common::drawn_popovers(&bed.hp)[0];
    let link = (
        f64::from(window.x) + IN + 40.0,
        f64::from(window.y) + GRID_TOP + 62.0 + 9.0 + 15.0 + 9.0 + 1.0 + 9.0 + 22.0 + 9.0 + 8.0,
    );
    common::click_on(&mut bed.hp, link, hocus_link::POPOVER_NAMESPACE)?;
    bed.logged("the camera's window shows it", &["Dans l’appel"])?;
    let window = common::drawn_popovers(&bed.hp)[0];
    bed.shoot(&out, "camera-window-help.png", around(window), 2)?;
    Ok(())
}

// --- Lot 3: the person, « Je reviens » --------------------------------------------

/// The test picture of a person (`hocus-camera/data/person.jpg`, NASA's,
/// public domain), B, G, R, x.
pub(crate) fn person() -> Vec<u8> {
    hocus_camera::segment::test_person()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, _]| [b, g, r, 255])
        .collect()
}

impl Bed {
    /// A frame made after now: what came before let go.
    fn fresh(&mut self, reader: &Reader, settle: usize) -> anyhow::Result<[f64; 3]> {
        while reader.frames.try_recv().is_ok() {}
        self.settled(reader, settle)
    }
}

/// Nobody in the picture for 10 s: the call sees « Je reviens »'s card on a
/// living wallpaper (dark, where the room was bright), and someone coming
/// back takes it away.
#[test]
fn je_reviens_shows_on_an_empty_picture_and_goes_as_someone_comes_back() -> anyhow::Result<()> {
    let mut bed = start([205, 200, 192], |c| c.camera.lighting = false)?;
    let reader = read(&bed.pipewire.socket)?;
    let room = bed.settled(&reader, 3)?;
    assert!(luma(room) > 150.0, "{room:?}");
    let empty = Instant::now();
    bed.until(Duration::from_secs(25), "no « Je reviens »", |bed| {
        bed.hp.camera_lot3().0 == Some(AwayWhy::Gone)
    })?;
    assert!(
        empty.elapsed() >= Duration::from_secs(9),
        "it came after {:?}",
        empty.elapsed()
    );
    let away = bed.fresh(&reader, 3)?;
    assert!(luma(away) < 70.0, "the card's picture: {away:?}");
    // Someone in the picture again: theirs.
    bed.camera.show(person());
    bed.until(Duration::from_secs(10), "still away", |bed| {
        bed.hp.camera_lot3().0.is_none()
    })?;
    let back = bed.fresh(&reader, 3)?;
    assert!(luma(back) > 90.0, "the person's picture: {back:?}");
    Ok(())
}

impl WindowBed {
    /// Whether the microphone is muted.
    fn muted(&self) -> bool {
        self.fakes.volume(MIC).is_ok_and(|(_, muted)| muted)
    }

    /// Runs until `done`, or fails saying `what`.
    fn until(
        &mut self,
        within: Duration,
        what: &str,
        mut done: impl FnMut(&mut Self) -> bool,
    ) -> anyhow::Result<()> {
        let start = Instant::now();
        while start.elapsed() < within {
            if done(self) {
                return Ok(());
            }
            wait(&mut self.hp, Duration::from_millis(50))?;
        }
        anyhow::bail!("{what}: {:?}", self.hp.camera_lot3())
    }

    /// The lock opened with its password.
    fn unlock(&mut self) -> anyhow::Result<()> {
        // A US keyboard's p, a, n, d, a, then Enter.
        for key in [25, 30, 49, 32, 30, 28] {
            common::chord(&mut self.hp, &[key]);
        }
        self.until(Duration::from_secs(15), "the lock did not open", |bed| {
            bed.hp.lock_state() == LockState::Unlocked
        })
    }

    /// The mean brightness of the call's next frames, the band left out.
    fn call_seen(&mut self) -> anyhow::Result<f64> {
        while self.call.frames.try_recv().is_ok() {}
        let start = Instant::now();
        let mut seen = Vec::new();
        while seen.len() < 3 {
            anyhow::ensure!(start.elapsed() < Duration::from_secs(15), "no frame");
            if let Ok((_, rest)) = self.call.frames.try_recv() {
                seen.push(rest);
            }
            wait(&mut self.hp, Duration::from_millis(20))?;
        }
        Ok(seen.last().copied().unwrap_or_default())
    }
}

/// The screen locked during a call: the call sees « Je reviens » at once,
/// and the default microphone is muted; unlocked, both go. A microphone
/// muted before stays muted: only what it muted is unmuted.
#[test]
fn locked_during_a_call_je_reviens_shows_and_the_microphone_is_held() -> anyhow::Result<()> {
    let mut bed = WindowBed::start_with_mic("")?;
    bed.camera.show(person());
    let present = bed.call_seen()?;
    assert!(!bed.muted(), "muted before anything");
    bed.hp.lock();
    bed.until(Duration::from_secs(10), "no « Je reviens »", |bed| {
        bed.hp.camera_lot3().0 == Some(AwayWhy::Locked)
    })?;
    bed.until(Duration::from_secs(15), "the microphone not muted", |bed| {
        bed.muted()
    })?;
    let locked = bed.call_seen()?;
    assert!(locked < present - 20.0, "{present} → {locked}");
    bed.unlock()?;
    bed.until(Duration::from_secs(10), "still « Je reviens »", |bed| {
        bed.hp.camera_lot3().0.is_none()
    })?;
    bed.until(
        Duration::from_secs(15),
        "the microphone still muted",
        |bed| !bed.muted(),
    )?;
    // Muted by its user before: locked and unlocked, still muted.
    bed.fakes
        .wpctl(&["set-mute", &bed.fakes.sink(MIC)?.to_string(), "1"])?;
    bed.until(Duration::from_secs(10), "not muted by hand", |bed| {
        bed.muted()
    })?;
    bed.hp.lock();
    bed.until(Duration::from_secs(10), "no « Je reviens »", |bed| {
        bed.hp.camera_lot3().0 == Some(AwayWhy::Locked)
    })?;
    bed.unlock()?;
    bed.until(Duration::from_secs(10), "still « Je reviens »", |bed| {
        bed.hp.camera_lot3().0.is_none()
    })?;
    let after = Instant::now();
    while after.elapsed() < Duration::from_secs(2) {
        wait(&mut bed.hp, Duration::from_millis(100))?;
        assert!(bed.muted(), "unmuted what its user muted");
    }
    Ok(())
}

/// The pull request's screenshots of lot 3 in the camera's window (its
/// preview live, the test picture of a person): Fond with Aurore, then
/// Koï, then Flou; Réactions with « Je reviens »; Image with the
/// presenter. Written to `$HOCUS_SHOTS` when it names a folder. Run by
/// hand (`--run-ignored only`).
#[test]
#[ignore = "writes the pull request's screenshots"]
fn screenshots_of_the_window_s_lot_3() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(std::path::PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let mut bed = WindowBed::start_on("[camera]\nbackground = \"aurora\"\n", "2560x1600@2")?;
    bed.camera.show(person());
    let window = bed.open()?;
    bed.logged("the camera's preview shows a frame", &[])?;
    let around =
        |r: hocus_compositor::model::Rect| (r.x - 16, r.y - 16, r.width + 32, r.height + 32);
    let tab = |window: hocus_compositor::model::Rect, i: f64| {
        (
            f64::from(window.x) + IN + 2.0 + i * 61.6 + 29.8,
            f64::from(window.y) + IN + 15.7 + 9.0 + 176.0 + 9.0 + 15.0,
        )
    };
    common::click_on(&mut bed.hp, tab(window, 1.0), hocus_link::POPOVER_NAMESPACE)?;
    bed.logged("the camera's window shows it", &["| Fond (dots"])?;
    // The wallpaper's first pictures, and the mask settled.
    wait(&mut bed.hp, Duration::from_secs(4))?;
    let window = common::drawn_popovers(&bed.hp)[0];
    bed.shoot(&out, "camera-window-fond-aurore.png", around(window), 2)?;
    for (n, name, word) in [(3, "koi", "[x] Koï"), (1, "flou", "[x] Flou")] {
        bed.tile(window, n)?;
        bed.logged("the camera's window shows it", &[word])?;
        wait(&mut bed.hp, Duration::from_secs(4))?;
        let window = common::drawn_popovers(&bed.hp)[0];
        bed.shoot(
            &out,
            &format!("camera-window-fond-{name}.png"),
            around(window),
            2,
        )?;
    }
    common::click_on(&mut bed.hp, tab(window, 4.0), hocus_link::POPOVER_NAMESPACE)?;
    bed.logged("the camera's window shows it", &["| Réactions (dots"])?;
    let window = common::drawn_popovers(&bed.hp)[0];
    bed.shoot(&out, "camera-window-reactions.png", around(window), 2)?;
    common::click_on(&mut bed.hp, tab(window, 0.0), hocus_link::POPOVER_NAMESPACE)?;
    bed.logged("the camera's window shows it", &["| Image (dots"])?;
    let window = common::drawn_popovers(&bed.hp)[0];
    bed.shoot(&out, "camera-window-image-lot3.png", around(window), 2)?;
    Ok(())
}
