//! Screen sharing (5.11, issue #443): the desktop's screen cast portal
//! (`org.freedesktop.impl.portal.ScreenCast`), called by the test as
//! xdg-desktop-portal calls it, on a D-Bus daemon of the test's own; the
//! streams on a PipeWire of the test's own (and its WirePlumber, which
//! links a consumer to them), in a fresh short runtime directory; the real
//! shell showing the chooser and the bar's sign. Each daemon is stopped by
//! its own process id; never the session's.
//!
//! One test per point of the approved spec: a request shows the chooser
//! and returns the screen chosen, or the window; cancelling returns the
//! cancel response; the shared screen gets its ring, never in its
//! stream's frames; the bar shows the sign, whose popover stops the share
//! and ends the session; notifications are held while sharing and summed
//! up at its end; a shared window closing ends its share. And the hard
//! rule's budget: handing a 2560 × 1600 frame to the stream costs the loop
//! well under a millisecond (its pixels are lent, never copied there).
//!
//! Pixman here (and GLES on Mesa's software device for the GPU's way of
//! handing frames over), the shell on the CPU (tiny-skia): nothing here
//! draws on a GPU, which may be the live session's. Without the
//! shell built, dbus-daemon or pipewire, the tests skip; without
//! WirePlumber, the frames' part of the ring's test does.

mod common;

use std::{
    collections::HashMap,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, PoisonError, mpsc},
    time::{Duration, Instant},
};

use common::{BTN_LEFT, KEY_ESC, WINDOW_COLOUR};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, ShareInfo};
use hocus_link::ShareSource;
use zbus::{MatchRule, MessageStream, OwnedObjectPath, OwnedVariant, Variant};

const KEY_ENTER: u32 = 28;
const PORTAL: &str = "org.freedesktop.impl.portal.desktop.hocuspanda";
const PATH: &str = "/org/freedesktop/portal/desktop";
const CAST: &str = "org.freedesktop.impl.portal.ScreenCast";
const SCREEN: (u32, u32) = (1920, 1080);

fn has(program: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {program}")])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn ready() -> Option<PathBuf> {
    let Some(shell) = common::shell_program() else {
        eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
        return None;
    };
    if !has("dbus-daemon") || !has("pipewire") {
        eprintln!("dbus-daemon or pipewire missing: skipped");
        return None;
    }
    Some(shell)
}

/// A D-Bus daemon of the test's own in `dir`, and its address.
fn private_bus(dir: &Path) -> anyhow::Result<(Child, String)> {
    let socket = dir.join("bus");
    let config = dir.join("bus.conf");
    std::fs::write(
        &config,
        format!(
            "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
             \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
             <busconfig><type>session</type><listen>unix:path={}</listen>\
             <policy context=\"default\"><allow send_destination=\"*\"/>\
             <allow receive_sender=\"*\"/><allow own=\"*\"/></policy></busconfig>\n",
            socket.display()
        ),
    )?;
    let bus = Command::new("dbus-daemon")
        .arg(format!("--config-file={}", config.display()))
        .arg("--nofork")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    wait_file(&socket)?;
    Ok((bus, format!("unix:path={}", socket.display())))
}

fn wait_file(path: &Path) -> anyhow::Result<()> {
    let start = Instant::now();
    while !path.exists() {
        anyhow::ensure!(
            start.elapsed() < Duration::from_secs(10),
            "{} never came",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

/// A PipeWire of the test's own, and its WirePlumber when there is one,
/// in a short runtime directory of their own (a socket's path is short).
struct Pipewire {
    daemon: Child,
    policy: Option<Child>,
    socket: PathBuf,
    _dir: tempfile::TempDir,
}

impl Pipewire {
    fn start(bus: &str) -> anyhow::Result<Self> {
        let dir = tempfile::Builder::new().prefix("hpw-").tempdir_in("/tmp")?;
        let run = dir.path().join("run");
        let home = dir.path().join("home");
        std::fs::create_dir_all(&run)?;
        std::fs::create_dir_all(&home)?;
        std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700))?;
        let spawn = |program: &str| {
            Command::new(program)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("HOME", &home)
                .env("XDG_RUNTIME_DIR", &run)
                .env("XDG_CONFIG_HOME", home.join(".config"))
                .env("DBUS_SESSION_BUS_ADDRESS", bus)
                .env("DBUS_SYSTEM_BUS_ADDRESS", "unix:path=/nonexistent")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
        };
        let daemon = spawn("pipewire")?;
        let socket = run.join("pipewire-0");
        wait_file(&socket)?;
        let policy = if has("wireplumber") {
            Some(spawn("wireplumber")?)
        } else {
            None
        };
        Ok(Self {
            daemon,
            policy,
            socket,
            _dir: dir,
        })
    }
}

impl Drop for Pipewire {
    fn drop(&mut self) {
        for child in [self.policy.as_mut(), Some(&mut self.daemon)]
            .into_iter()
            .flatten()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// What the test heard on the bus: the sessions closed.
type Heard = Arc<Mutex<Vec<String>>>;

struct Bed {
    hp: Headless,
    log: PathBuf,
    runtime: tokio::runtime::Runtime,
    client: zbus::Connection,
    heard: Heard,
    pipewire: Pipewire,
    sessions: u32,
    /// Last, so stopped once the compositor and its connections are.
    bus: Child,
    _dir: tempfile::TempDir,
}

impl Drop for Bed {
    fn drop(&mut self) {
        let _ = self.bus.kill();
        let _ = self.bus.wait();
    }
}

fn start(shell: &Path) -> anyhow::Result<Bed> {
    start_on(shell, SCREEN, RendererChoice::Pixman)
}

/// The bed on a screen `size` pixels large, at scale 1, drawn by
/// `renderer`: pixman, or GLES on Mesa's software device. Never a GPU:
/// this machine's may be the live session's.
fn start_on(shell: &Path, size: (u32, u32), renderer: RendererChoice) -> anyhow::Result<Bed> {
    let dir = tempfile::tempdir()?;
    let home = dir.path().join("home");
    let state = dir.path().join("state");
    for sub in [&home, &state.join("hocuspanda")] {
        std::fs::create_dir_all(sub)?;
    }
    let (bus, address) = private_bus(dir.path())?;
    let pipewire = Pipewire::start(&address)?;
    let log = dir.path().join("shell.log");
    let script = dir.path().join("shell");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
             export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
             PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
             LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{home}' \
             XDG_CONFIG_HOME='{home}/.config' XDG_CACHE_HOME='{home}/.cache' \
             XDG_DATA_HOME='{home}/.local/share' XDG_DATA_DIRS='{home}/none' \
             XDG_STATE_HOME='{state}' TZ=UTC ICED_BACKEND=tiny-skia \
             RUST_LOG=warn,hocuspanda_shell=debug\n\
             exec '{shell}' \"$@\" 2>>'{log}'\n",
            home = home.display(),
            state = state.display(),
            shell = shell.display(),
            log = log.display()
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let config = Config {
        pipewire_remote: Some(pipewire.socket.clone()),
        plain_background: true,
        ..Config::default()
    };
    let mut hp = Headless::start_on_bus(
        HeadlessConfig {
            outputs: vec![format!("{}x{}", size.0, size.1).parse()?],
            renderer,
            render_node: None,
        },
        config,
        Some(script),
        Some(address.clone()),
    )?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    let client = runtime.block_on(zbus::connection::Builder::address(address.as_str()).build())?;
    let heard = Heard::default();
    runtime.block_on(listen(&client, heard.clone()))?;
    let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| hp.bar_commits() > 0)?;
    anyhow::ensure!(drawn, "no bar");
    let mut bed = Bed {
        hp,
        log,
        runtime,
        client,
        heard,
        pipewire,
        sessions: 0,
        bus,
        _dir: dir,
    };
    bed.served()?;
    bed.hp.pointer_move(400.0, 600.0);
    bed.idle(Duration::from_millis(300))?;
    Ok(bed)
}

/// The sessions' `Closed`, as `closed PATH`.
async fn listen(client: &zbus::Connection, heard: Heard) -> anyhow::Result<()> {
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.impl.portal.Session")
        .build()?;
    let stream = MessageStream::for_match_rule(rule, client, None).await?;
    tokio::spawn(async move {
        use zbus::export::futures_core::Stream;
        let mut stream = Box::pin(stream);
        while let Some(Ok(message)) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await
        {
            let header = message.header();
            if header.member().map(|m| m.as_str()) == Some("Closed")
                && let Some(path) = header.path()
            {
                heard
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(format!("closed {path}"));
            }
        }
    });
    Ok(())
}

type Results = HashMap<String, OwnedVariant>;

/// A stream as `Start` returned it: its node, its source type, its size
/// and its place.
type Stream = (u32, u32, Option<(i32, i32)>, Option<(i32, i32)>);

/// What `Start` answered: the response and its streams.
#[derive(Debug)]
struct Started {
    response: u32,
    streams: Vec<Stream>,
}

impl Bed {
    fn idle(&mut self, time: Duration) -> anyhow::Result<()> {
        self.hp.dispatch_until(time, |_| false)?;
        Ok(())
    }

    /// Waits until the portal is on the bus.
    fn served(&mut self) -> anyhow::Result<()> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            let client = self.client.clone();
            let owned = self.runtime.block_on(async move {
                let dbus = zbus::fdo::DBusProxy::new(&client).await?;
                anyhow::Ok(dbus.name_has_owner(PORTAL.try_into()?).await?)
            })?;
            if owned {
                return Ok(());
            }
            self.idle(Duration::from_millis(50))?;
        }
        anyhow::bail!("the portal is not served")
    }

    fn call(
        &mut self,
        method: &str,
        body: impl serde::Serialize + zbus::zvariant::DynamicType + Send + Sync + 'static,
    ) -> anyhow::Result<(u32, Results)> {
        let client = self.client.clone();
        let method = method.to_owned();
        self.runtime.block_on(async move {
            let reply = client
                .call_method(Some(PORTAL), PATH, Some(CAST), method.as_str(), &body)
                .await?;
            anyhow::Ok(reply.body().deserialize::<(u32, Results)>()?)
        })
    }

    /// A session asking for `types` with `cursor` (none: not said); its
    /// path.
    fn session(&mut self, types: u32, cursor: Option<u32>) -> anyhow::Result<String> {
        self.sessions += 1;
        let n = self.sessions;
        let session = format!("{PATH}/session/1_1/s{n}");
        let created = self.call(
            "CreateSession",
            (
                OwnedObjectPath::try_from(format!("{PATH}/request/1_1/c{n}"))?,
                OwnedObjectPath::try_from(session.as_str())?,
                "org.mozilla.firefox".to_owned(),
                HashMap::<String, OwnedVariant>::new(),
            ),
        )?;
        anyhow::ensure!(created.0 == 0, "CreateSession: {created:?}");
        let mut options: HashMap<String, OwnedVariant> = HashMap::new();
        options.insert("types".into(), OwnedVariant::try_from(Variant::U32(types))?);
        if let Some(cursor) = cursor {
            options.insert(
                "cursor_mode".into(),
                OwnedVariant::try_from(Variant::U32(cursor))?,
            );
        }
        let selected = self.call(
            "SelectSources",
            (
                OwnedObjectPath::try_from(format!("{PATH}/request/1_1/l{n}"))?,
                OwnedObjectPath::try_from(session.as_str())?,
                "org.mozilla.firefox".to_owned(),
                options,
            ),
        )?;
        anyhow::ensure!(selected.0 == 0, "SelectSources: {selected:?}");
        Ok(session)
    }

    /// `Start` on `session`, the chooser answered by `answer` once it shows
    /// and holds the keyboard.
    fn start(&mut self, session: &str, answer: impl FnOnce(&mut Self)) -> anyhow::Result<Started> {
        let n = self.sessions;
        let client = self.client.clone();
        let body = (
            OwnedObjectPath::try_from(format!("{PATH}/request/1_1/t{n}"))?,
            OwnedObjectPath::try_from(session)?,
            "org.mozilla.firefox".to_owned(),
            String::new(),
            HashMap::<String, OwnedVariant>::new(),
        );
        let task = self.runtime.spawn(async move {
            let reply = client
                .call_method(Some(PORTAL), PATH, Some(CAST), "Start", &body)
                .await?;
            anyhow::Ok(reply.body().deserialize::<(u32, Results)>()?)
        });
        let shown = self.hp.dispatch_until(Duration::from_secs(15), |hp| {
            hp.share_choosing().is_some()
                && hp.keyboard_layer().as_deref() == Some(hocus_link::SHARE_CHOOSER_NAMESPACE)
        })?;
        anyhow::ensure!(shown, "no chooser:\n{}", self.log());
        self.logged("the share's chooser shows it")?;
        answer(self);
        let done = self
            .hp
            .dispatch_until(Duration::from_secs(15), |_| task.is_finished())?;
        anyhow::ensure!(done, "Start never answered:\n{}", self.log());
        let (response, results) = self.runtime.block_on(task)??;
        let streams = match results.get("streams") {
            Some(v) => {
                let list = <Vec<(u32, HashMap<String, OwnedVariant>)>>::try_from(v.try_clone()?)?;
                list.into_iter()
                    .map(|(node, props)| {
                        let kind = props
                            .get("source_type")
                            .and_then(|v| u32::try_from(&**v).ok())
                            .unwrap_or(0);
                        let pair = |key: &str| {
                            props
                                .get(key)
                                .and_then(|v| <(i32, i32)>::try_from(v.try_clone().ok()?).ok())
                        };
                        (node, kind, pair("size"), pair("position"))
                    })
                    .collect()
            }
            None => Vec::new(),
        };
        Ok(Started { response, streams })
    }

    fn key(&mut self, code: u32) {
        self.hp.key(code, true);
        self.hp.key(code, false);
    }

    fn shares(&self) -> Vec<ShareInfo> {
        self.hp.shares()
    }

    /// Waits until the test heard session `session` closed.
    fn closed(&mut self, session: &str) -> anyhow::Result<()> {
        let said = format!("closed {session}");
        let heard = self.heard.clone();
        let done = self.hp.dispatch_until(Duration::from_secs(10), |_| {
            heard
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains(&said)
        })?;
        anyhow::ensure!(done, "not {said:?}: {:?}", self.heard);
        Ok(())
    }

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

    fn logged(&mut self, what: &str) -> anyhow::Result<()> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(15) {
            if self.log().contains(what) {
                return Ok(());
            }
            self.idle(Duration::from_millis(50))?;
        }
        anyhow::bail!("the shell did not log {what:?}:\n{}", self.log())
    }

    fn click(&mut self, (x, y): (f64, f64)) {
        self.hp.pointer_move(x, y);
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
    }

    /// A screen shared (Enter on the chooser of a monitors' request).
    fn share_screen(&mut self) -> anyhow::Result<(String, Started)> {
        let session = self.session(hocus_link::SHARE_MONITOR, None)?;
        let started = self.start(&session, |bed| bed.key(KEY_ENTER))?;
        anyhow::ensure!(started.response == 0, "{started:?}");
        Ok((session, started))
    }

    /// The pixel at (`x`, `y`) of the screen, as `0xAARRGGBB`.
    fn pixel(&mut self, x: usize, y: usize) -> anyhow::Result<u32> {
        let (w, _, pixels) = self.hp.screenshot(0)?;
        let at = (y * w as usize + x) * 4;
        let [b, g, r, a] = [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]];
        Ok(u32::from_be_bytes([a, r, g, b]))
    }
}

/// Whether `pixel` (`0xAARRGGBB`) is the ring's blue, or near it.
fn blue(pixel: u32) -> bool {
    let [_, r, g, b] = pixel.to_be_bytes();
    b > 150 && b > r + 60 && g > r
}

#[test]
fn a_request_shows_the_chooser_and_returns_the_screen_chosen() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell)?;
    let (_, started) = bed.share_screen()?;
    // The chooser said who asks, on its screens' tab, the screen chosen.
    let log = bed.log();
    assert!(log.contains("Firefox veut voir ton écran"), "{log}");
    assert!(log.contains("[Écrans]"), "{log}");
    assert!(log.contains("(Écran externe · 1920 × 1080)"), "{log}");
    assert!(log.contains("[x] Montrer le pointeur"), "{log}");
    let [(node, kind, size, position)] = started.streams.as_slice() else {
        anyhow::bail!("{started:?}");
    };
    assert!(*node > 0);
    assert_eq!(*kind, hocus_link::SHARE_MONITOR);
    assert_eq!(*size, Some((1920, 1080)));
    assert_eq!(*position, Some((0, 0)));
    let shares = bed.shares();
    assert_eq!(shares.len(), 1);
    assert_eq!(shares[0].source, ShareSource::Screen("HEADLESS-1".into()));
    assert!(shares[0].pointer);
    assert_eq!(bed.hp.share_choosing(), None);
    assert!(
        bed.hp
            .layer_rects(hocus_link::SHARE_CHOOSER_NAMESPACE)
            .is_empty()
            || {
                bed.idle(Duration::from_millis(300))?;
                bed.hp
                    .layer_rects(hocus_link::SHARE_CHOOSER_NAMESPACE)
                    .is_empty()
            }
    );
    Ok(())
}

#[test]
fn a_request_for_windows_returns_the_window_chosen_with_its_ring() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell)?;
    let rect = common::open(&mut bed.hp, "test.shared", (600, 400))?;
    bed.idle(Duration::from_millis(200))?;
    let session = bed.session(hocus_link::SHARE_WINDOW, Some(1))?;
    let started = bed.start(&session, |bed| bed.key(KEY_ENTER))?;
    assert_eq!(started.response, 0, "{started:?}");
    let log = bed.log();
    assert!(log.contains("[Fenêtres]"), "{log}");
    assert!(log.contains("· ce bureau)"), "{log}");
    // The application asked for the pointer hidden: off by default.
    assert!(log.contains("[ ] Montrer le pointeur"), "{log}");
    let [(_, kind, size, position)] = started.streams.as_slice() else {
        anyhow::bail!("{started:?}");
    };
    assert_eq!(*kind, hocus_link::SHARE_WINDOW);
    assert_eq!(*size, Some((rect.width, rect.height)));
    assert_eq!(*position, None);
    let shares = bed.shares();
    assert!(matches!(shares[0].source, ShareSource::Window(_)));
    assert!(!shares[0].pointer);
    // Its ring, 2 px outside its edge, on the screen.
    bed.idle(Duration::from_millis(300))?;
    let mid = (rect.y + rect.height / 2) as usize;
    let outside = bed.pixel(rect.x as usize - 1, mid)?;
    assert!(blue(outside), "{outside:08x}");
    let inside = bed.pixel(rect.x as usize + 20, mid)?;
    assert_eq!(inside, WINDOW_COLOUR);
    Ok(())
}

#[test]
fn cancelling_the_chooser_returns_the_cancel_response() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell)?;
    let session = bed.session(hocus_link::SHARE_MONITOR | hocus_link::SHARE_WINDOW, None)?;
    let started = bed.start(&session, |bed| bed.key(KEY_ESC))?;
    assert_eq!(started.response, 1, "{started:?}");
    assert!(started.streams.is_empty());
    assert!(bed.shares().is_empty());
    assert_eq!(bed.hp.share_choosing(), None);
    Ok(())
}

/// A consumer of a PipeWire node, on a thread of its own: its frames'
/// size and pixels (B, G, R, x).
struct Consumer {
    frames: mpsc::Receiver<((u32, u32), Vec<u8>)>,
    quit: pipewire::channel::Sender<()>,
}

impl Drop for Consumer {
    fn drop(&mut self) {
        let _ = self.quit.send(());
    }
}

fn consume(remote: &Path, node: u32) -> anyhow::Result<Consumer> {
    use pipewire as pw;
    use pw::spa::{
        self,
        pod::{ChoiceValue, Object, Pod, Property, PropertyFlags, Value},
        utils::{Choice, ChoiceEnum, ChoiceFlags, Fraction, Rectangle},
    };
    let (tx, frames) = mpsc::channel();
    let (quit, quitting) = pw::channel::channel::<()>();
    let remote = remote.to_string_lossy().into_owned();
    std::thread::spawn(move || -> Result<(), pw::Error> {
        pw::init();
        let mainloop = pw::main_loop::MainLoopRc::new(None)?;
        let context = pw::context::ContextRc::new(&mainloop, None)?;
        let core = context.connect_rc(Some(pw::properties::properties! {
            *pw::keys::REMOTE_NAME => remote,
        }))?;
        let stream = pw::stream::StreamRc::new(
            core,
            "test-consumer",
            pw::properties::properties! {
                *pw::keys::MEDIA_TYPE => "Video",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::MEDIA_ROLE => "Screen",
                *pw::keys::TARGET_OBJECT => node.to_string(),
            },
        )?;
        let size = Arc::new(Mutex::new((0u32, 0u32)));
        let seen = size.clone();
        let _listener = stream
            .add_local_listener::<()>()
            .param_changed(move |_, _, id, param| {
                let Some(param) = param else { return };
                if id != spa::param::ParamType::Format.as_raw() {
                    return;
                }
                let mut info = spa::param::video::VideoInfoRaw::default();
                if info.parse(param).is_ok() {
                    *seen.lock().unwrap_or_else(PoisonError::into_inner) =
                        (info.size().width, info.size().height);
                }
            })
            .process(move |stream, _| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let (w, h) = *size.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(data) = buffer.datas_mut().first_mut() {
                    let len = data.chunk().size() as usize;
                    if let Some(bytes) = data.data()
                        && len > 0
                    {
                        let _ = tx.send(((w, h), bytes[..len.min(bytes.len())].to_vec()));
                    }
                }
            })
            .register()?;
        let id = |v: u32| Value::Id(spa::utils::Id(v));
        let prop = |key: spa::param::format::FormatProperties, value: Value| Property {
            key: key.as_raw(),
            flags: PropertyFlags::empty(),
            value,
        };
        use spa::param::format::{FormatProperties as F, MediaSubtype, MediaType};
        use spa::param::video::VideoFormat;
        let object = Object {
            type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
            id: spa::param::ParamType::EnumFormat.as_raw(),
            properties: vec![
                prop(F::MediaType, id(MediaType::Video.as_raw())),
                prop(F::MediaSubtype, id(MediaSubtype::Raw.as_raw())),
                prop(
                    F::VideoFormat,
                    Value::Choice(ChoiceValue::Id(Choice(
                        ChoiceFlags::empty(),
                        ChoiceEnum::Enum {
                            default: spa::utils::Id(VideoFormat::BGRx.as_raw()),
                            alternatives: vec![
                                spa::utils::Id(VideoFormat::BGRx.as_raw()),
                                spa::utils::Id(VideoFormat::BGRA.as_raw()),
                            ],
                        },
                    ))),
                ),
                prop(
                    F::VideoSize,
                    Value::Choice(ChoiceValue::Rectangle(Choice(
                        ChoiceFlags::empty(),
                        ChoiceEnum::Range {
                            default: Rectangle {
                                width: 1920,
                                height: 1080,
                            },
                            min: Rectangle {
                                width: 1,
                                height: 1,
                            },
                            max: Rectangle {
                                width: 8192,
                                height: 8192,
                            },
                        },
                    ))),
                ),
                prop(
                    F::VideoFramerate,
                    Value::Choice(ChoiceValue::Fraction(Choice(
                        ChoiceFlags::empty(),
                        ChoiceEnum::Range {
                            default: Fraction { num: 30, denom: 1 },
                            min: Fraction { num: 0, denom: 1 },
                            max: Fraction { num: 120, denom: 1 },
                        },
                    ))),
                ),
            ],
        };
        let bytes = spa::pod::serialize::PodSerializer::serialize(
            std::io::Cursor::new(Vec::new()),
            &Value::Object(object),
        )
        .map(|(c, _)| c.into_inner())
        .unwrap_or_default();
        let Some(pod) = Pod::from_bytes(&bytes) else {
            return Err(pw::Error::CreationFailed);
        };
        stream.connect(
            spa::utils::Direction::Input,
            Some(node),
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::DONT_RECONNECT,
            &mut [pod],
        )?;
        let weak = mainloop.downgrade();
        let _quit = quitting.attach(mainloop.loop_(), move |()| {
            if let Some(mainloop) = weak.upgrade() {
                mainloop.quit();
            }
        });
        mainloop.run();
        Ok(())
    });
    Ok(Consumer { frames, quit })
}

#[test]
fn the_shared_screen_has_its_ring_never_in_its_stream() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell)?;
    let (_, started) = bed.share_screen()?;
    bed.idle(Duration::from_millis(300))?;
    // The ring, 2 px around the screen's edge, on the screen.
    let edge = bed.pixel(1, 540)?;
    assert!(blue(edge), "{edge:08x}");
    if bed.pipewire.policy.is_none() {
        eprintln!("no WirePlumber to link a consumer: the stream's frames are not looked at");
        return Ok(());
    }
    let node = started.streams[0].0;
    let consumer = consume(&bed.pipewire.socket, node)?;
    let start = Instant::now();
    let frame = loop {
        anyhow::ensure!(
            start.elapsed() < Duration::from_secs(15),
            "no frame came (frames sent: {}, {:?})",
            bed.hp.share_frames(),
            bed.shares()
        );
        bed.idle(Duration::from_millis(50))?;
        if let Ok(frame) = consumer.frames.try_recv() {
            break frame;
        }
    };
    let ((w, h), pixels) = frame;
    assert_eq!((w, h), SCREEN);
    // No ring in it: the edge is the desktop's.
    let at = (540 * w as usize + 1) * 4;
    let px = u32::from_be_bytes([0xff, pixels[at + 2], pixels[at + 1], pixels[at]]);
    assert!(!blue(px), "{px:08x}");
    // And the rest is the screen's.
    let screen = bed.pixel(300, 540)?;
    let at = (540 * w as usize + 300) * 4;
    let same = u32::from_be_bytes([0xff, pixels[at + 2], pixels[at + 1], pixels[at]]);
    assert_eq!(same & 0xffffff, screen & 0xffffff);
    Ok(())
}

#[test]
fn a_frame_costs_the_loop_well_under_a_millisecond() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // The maintainer's screen: a frame is 16 MB.
    let mut bed = start_on(&shell, (2560, 1600), RendererChoice::Pixman)?;
    if bed.pipewire.policy.is_none() {
        eprintln!("no WirePlumber to link a consumer: skipped");
        return Ok(());
    }
    let (_, started) = bed.share_screen()?;
    let consumer = consume(&bed.pipewire.socket, started.streams[0].0)?;
    let start = Instant::now();
    let mut got = 0;
    let mut x = 300.0;
    while got < 5 {
        anyhow::ensure!(
            start.elapsed() < Duration::from_secs(20),
            "{got} frames came (sent: {})",
            bed.hp.share_frames()
        );
        // The pointer moves: each frame is new.
        x += 40.0;
        bed.hp.pointer_move(x, 800.0);
        bed.idle(Duration::from_millis(60))?;
        while let Ok(((w, h), pixels)) = consumer.frames.try_recv() {
            assert_eq!((w, h), (2560, 1600));
            assert!(pixels.len() >= 2560 * 1600 * 4);
            got += 1;
        }
    }
    // Handing a frame over is lending its pixels, never copying them.
    let most = bed.hp.share_lend_most();
    assert!(most < Duration::from_millis(1), "{most:?}");
    Ok(())
}

/// The GPU's way (GLES): the frame read back into a pixel buffer, which a
/// thread of its own maps and lends; on Mesa's software device, never a
/// GPU. Its frames come whole and upright, in the screen's colours.
#[test]
fn on_gles_the_read_back_thread_hands_the_frames_over() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = match start_on(&shell, SCREEN, RendererChoice::GlesSoftware) {
        Ok(bed) => bed,
        Err(err) => {
            eprintln!("no GLES on Mesa's software device ({err:#}): skipped");
            return Ok(());
        }
    };
    if bed.pipewire.policy.is_none() {
        eprintln!("no WirePlumber to link a consumer: skipped");
        return Ok(());
    }
    let (_, started) = bed.share_screen()?;
    let consumer = consume(&bed.pipewire.socket, started.streams[0].0)?;
    let start = Instant::now();
    let mut x = 300.0;
    let frame = loop {
        anyhow::ensure!(
            start.elapsed() < Duration::from_secs(20),
            "no frame came (sent: {})",
            bed.hp.share_frames()
        );
        x += 20.0;
        bed.hp.pointer_move(x, 800.0);
        bed.idle(Duration::from_millis(80))?;
        if let Ok(frame) = consumer.frames.try_recv() {
            break frame;
        }
    };
    let ((w, h), pixels) = frame;
    assert_eq!((w, h), SCREEN);
    // Upright and in its colours: a pixel of the screen's top and one of
    // its bottom, as the screen shows them.
    bed.hp.pointer_move(1500.0, 300.0);
    bed.idle(Duration::from_millis(200))?;
    for (px, py) in [(300usize, 40usize), (300, 1040)] {
        let screen = bed.pixel(px, py)? & 0x00ff_ffff;
        let at = (py * w as usize + px) * 4;
        let streamed = u32::from_be_bytes([0, pixels[at + 2], pixels[at + 1], pixels[at]]);
        let close = screen
            .to_be_bytes()
            .iter()
            .zip(streamed.to_be_bytes())
            .all(|(a, b)| a.abs_diff(b) <= 8);
        assert!(
            close,
            "({px}, {py}): screen {screen:06x}, stream {streamed:06x}"
        );
    }
    Ok(())
}

#[test]
fn the_bars_sign_shows_and_stopping_ends_the_session() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell)?;
    let (session, _) = bed.share_screen()?;
    let shown = bed.hp.dispatch_until(Duration::from_secs(10), |hp| {
        !hp.layer_rects(hocus_link::IN_USE_NAMESPACE).is_empty()
    })?;
    anyhow::ensure!(shown, "no sign");
    bed.idle(Duration::from_millis(300))?;
    let (_, sign) = bed.hp.layer_rects(hocus_link::IN_USE_NAMESPACE)[0].clone();
    // Under the clock, in the full bar.
    assert!(sign.y < 200, "{sign:?}");
    // A click on it opens its popover.
    bed.click((
        f64::from(sign.x + sign.width / 2),
        f64::from(sign.y + sign.height / 2),
    ));
    let open = bed.hp.dispatch_until(Duration::from_secs(10), |hp| {
        !hp.popover_surfaces().is_empty()
    })?;
    anyhow::ensure!(open, "no popover:\n{}", bed.log());
    bed.logged("the sharing popover shows it")?;
    let log = bed.log();
    assert!(log.contains("Firefox voit ton écran"), "{log}");
    assert!(log.contains("Tu partages l'écran « HEADLESS-1 »"), "{log}");
    assert!(log.contains("Les notifications attendent la fin"), "{log}");
    bed.idle(Duration::from_millis(300))?;
    let (_, popover) = bed.hp.popover_surfaces()[0].clone();
    // Its button, the last of it.
    bed.click((
        f64::from(popover.x + popover.width / 2),
        f64::from(popover.y + popover.height - 13 - 16),
    ));
    bed.closed(&session)?;
    let gone = bed.hp.dispatch_until(Duration::from_secs(10), |hp| {
        hp.shares().is_empty() && hp.layer_rects(hocus_link::IN_USE_NAMESPACE).is_empty()
    })?;
    anyhow::ensure!(gone, "still {:?}", bed.shares());
    Ok(())
}

#[test]
fn notifications_are_held_while_sharing_and_summed_up_at_its_end() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell)?;
    bed.logged("the notifications are served")?;
    let (session, _) = bed.share_screen()?;
    bed.logged("the screen is shared: bubbles are held")?;
    for app in ["Slack", "Slack", "Signal"] {
        let client = bed.client.clone();
        bed.runtime.block_on(async move {
            client
                .call_method(
                    Some("org.freedesktop.Notifications"),
                    "/org/freedesktop/Notifications",
                    Some("org.freedesktop.Notifications"),
                    "Notify",
                    &(
                        app,
                        0u32,
                        "",
                        "Camille",
                        "Un message",
                        Vec::<String>::new(),
                        HashMap::<&str, OwnedVariant>::new(),
                        -1i32,
                    ),
                )
                .await
        })?;
    }
    bed.idle(Duration::from_millis(500))?;
    assert!(bed.hp.bubble_surfaces().is_empty(), "held: no bubble");
    // The application ends the session.
    let client = bed.client.clone();
    let path = session.clone();
    bed.runtime.block_on(async move {
        client
            .call_method(
                Some(PORTAL),
                path.as_str(),
                Some("org.freedesktop.impl.portal.Session"),
                "Close",
                &(),
            )
            .await
    })?;
    let summed = bed.hp.dispatch_until(Duration::from_secs(10), |hp| {
        hp.bubble_surfaces().len() == 1
    })?;
    anyhow::ensure!(summed, "no summary:\n{}", bed.log());
    bed.logged("the summary of what was held")?;
    let log = bed.log();
    assert!(
        log.contains("title=\"3 notifications pendant le partage\""),
        "{log}"
    );
    assert!(
        log.contains("body=\"Slack (2), Signal · dans l'historique\""),
        "{log}"
    );
    assert!(bed.shares().is_empty());
    Ok(())
}

#[test]
fn a_shared_window_closing_ends_its_share() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell)?;
    let (_, handle) = common::open_with(&mut bed.hp, "test.closing", (500, 300), false)?;
    bed.idle(Duration::from_millis(200))?;
    let session = bed.session(hocus_link::SHARE_WINDOW, None)?;
    let started = bed.start(&session, |bed| bed.key(KEY_ENTER))?;
    assert_eq!(started.response, 0, "{started:?}");
    assert_eq!(bed.shares().len(), 1);
    handle.commands.send(common::Command::Close)?;
    bed.closed(&session)?;
    assert!(bed.shares().is_empty());
    Ok(())
}
