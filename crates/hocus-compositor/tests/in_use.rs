//! What is in use, under the clock (5.12, issue #445): the real shell
//! against the private fakes of its services (`levels/fakes.rs`): a
//! PipeWire of the test's own, in a short runtime directory, its source
//! a null node, the applications' microphone and camera streams null nodes
//! of their classes linked to it (or to a camera's node) by the test
//! itself, as a session manager would (no WirePlumber); a D-Bus daemon of
//! its own, on which the test calls the screen cast portal as
//! xdg-desktop-portal does (5.11); the cameras' devices files of the
//! test's own (`HOCUS_VIDEO_DEVICES`), never `/dev/video*`. Each daemon is
//! stopped by its own process id; never the session's.
//!
//! One test per point of the approved spec: the sign shows for each kind
//! as it is used; it lingers 3 s after; the microphone grey while muted;
//! the kinds side by side; the full form's row; the popover's lines, their
//! detail and how long; its button muting the default source and back;
//! its buttons stopping a share and a recording; nothing read nor drawn
//! at rest; `/proc` read at most once a second while a camera's node
//! exists. And 5.12b's (issue #561): the recording's red tile with its
//! time, amber and "pause" while paused, before the share's blue one, in
//! the full form's row too, and nothing drawn once it is paused or gone. The timings of the linger and of the pop, on the shell's own
//! clock, are its unit tests (`in_use::tests`, `in_use::draw::tests`):
//! here the shell says, from its monotonic clock, how long a use lingered.
//!
//! Pixman here, so the shell draws on the CPU (tiny-skia): nothing draws
//! on a GPU, which may be the live session's.

use crate::common;
use crate::fakes;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::Context;
use common::{WAIT, shell_program, wait};
use fakes::{FakeSink, Fakes, TWO_SINKS};
use hocus_compositor::{Config, FormChoice, Headless, HeadlessConfig, RendererChoice, model::Rect};
use zbus::{
    OwnedObjectPath,
    zvariant::{OwnedVariant, Variant},
};

const MIC: &str = "hocus-test-mic";
const KEY_ENTER: u32 = 28;
const KEY_PRINT: u32 = 99;
const PORTAL: &str = "org.freedesktop.impl.portal.desktop.hocuspanda";
const PATH: &str = "/org/freedesktop/portal/desktop";
const CAST: &str = "org.freedesktop.impl.portal.ScreenCast";

fn ready() -> Option<PathBuf> {
    let shell = shell_program();
    let tools = ["pw-cli", "pw-link"].iter().all(|p| has(p));
    if !fakes::available() || !tools {
        common::missing("dbus-daemon, pipewire, pw-dump, wpctl, pw-metadata, pw-cli and pw-link");
    }
    Some(shell)
}

fn has(program: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {program}")])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

struct Bed {
    hp: Headless,
    fakes: Fakes,
    log: PathBuf,
    /// The cameras' devices: files of the test's own.
    devices: PathBuf,
    client: zbus::Connection,
    sessions: u32,
    /// What the test started, stopped by their process ids.
    children: Vec<Child>,
    _root: tempfile::TempDir,
}

impl Drop for Bed {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
        let client = self.client.clone();
        self.fakes.runtime().block_on(async move { drop(client) });
    }
}

impl Bed {
    fn start(shell: &Path, size: (u32, u32), form: FormChoice) -> anyhow::Result<Self> {
        let root = tempfile::tempdir()?;
        let dir = root.path();
        for sub in [
            "home", "config", "cache", "state", "data", "devices", "videos",
        ] {
            std::fs::create_dir_all(dir.join(sub))?;
        }
        let devices = dir.join("devices");
        std::fs::write(devices.join("video0"), b"")?;
        let fakes =
            Fakes::start_with_sources(&TWO_SINKS, &[FakeSink::new(MIC, "Microphone du portable")])?;
        let log = dir.join("shell.log");
        let at = |sub: &str| dir.join(sub).display().to_string();
        let script = fakes.shell_with(
            shell,
            "fr_FR.UTF-8",
            &[
                ("HOME", at("home")),
                ("XDG_CONFIG_HOME", at("config")),
                ("XDG_CACHE_HOME", at("cache")),
                ("XDG_STATE_HOME", at("state")),
                ("XDG_DATA_HOME", at("data")),
                ("XDG_DATA_DIRS", at("data")),
                ("HOCUS_VIDEO_DEVICES", devices.display().to_string()),
                ("ICED_BACKEND", "tiny-skia".to_owned()),
                ("TZ", "UTC".to_owned()),
                ("RUST_LOG", "warn,hocuspanda_shell=debug".to_owned()),
            ],
            Some(&log),
        )?;
        let mut config = Config {
            plain_background: true,
            french: true,
            pipewire_remote: Some(fakes.pw_socket()),
            videos: Some(dir.join("videos")),
            ..Config::default()
        };
        config.keyboard.layouts = vec!["us".to_owned()];
        config.panel.form = form;
        let mut hp = Headless::start_on_bus(
            HeadlessConfig {
                outputs: vec![format!("{}x{}", size.0, size.1).parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
            Some(script),
            Some(fakes.address.clone()),
        )?;
        let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.levels_commits() > 0)?;
        anyhow::ensure!(up, "no bar");
        let address = fakes.address.clone();
        let client = fakes
            .runtime()
            .block_on(zbus::connection::Builder::address(address.as_str()).build())?;
        let mut bed = Self {
            hp,
            fakes,
            log,
            devices,
            client,
            sessions: 0,
            children: Vec::new(),
            _root: root,
        };
        bed.hp.pointer_move(400.0, 600.0);
        wait(&mut bed.hp, Duration::from_millis(300))?;
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

    /// Lines of the shell's log saying `what`.
    fn lines(&self, what: &str) -> Vec<String> {
        self.log()
            .lines()
            .filter(|l| l.contains(what))
            .map(str::to_owned)
            .collect()
    }

    /// Where the shell's log ends now: what it says next is after it.
    fn mark(&self) -> usize {
        self.log().len()
    }

    /// Waits until the shell logs `what` on a line `pass` takes; that
    /// line.
    fn logged(&mut self, what: &str, pass: impl Fn(&str) -> bool) -> anyhow::Result<String> {
        self.logged_since(0, what, pass)
    }

    /// The same, said after `mark`.
    fn logged_since(
        &mut self,
        mark: usize,
        what: &str,
        pass: impl Fn(&str) -> bool,
    ) -> anyhow::Result<String> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(15) {
            let log = self.log();
            let after = log.get(mark..).unwrap_or_default();
            if let Some(line) = after
                .lines()
                .filter(|l| l.contains(what))
                .rev()
                .find(|l| pass(l))
            {
                return Ok(line.to_owned());
            }
            wait(&mut self.hp, Duration::from_millis(50))?;
        }
        anyhow::bail!("the shell did not log {what:?} as asked:\n{}", self.log())
    }

    /// Waits until the sign shows `kinds` (as the shell writes them) at
    /// rest, then for its surface; the surface.
    fn sign(&mut self, kinds: &str) -> anyhow::Result<Rect> {
        self.sign_since(0, kinds)
    }

    /// The same, said after `mark`.
    fn sign_since(&mut self, mark: usize, kinds: &str) -> anyhow::Result<Rect> {
        let want = format!("kinds=[{kinds}]");
        self.logged_since(mark, "the in-use sign is drawn at rest", |l| {
            l.contains(&want)
        })?;
        Ok(common::drawn(&mut self.hp, hocus_link::IN_USE_NAMESPACE)?[0].1)
    }

    /// A microphone's stream of `app` (its program `binary`, its media
    /// name `media`), linked to the source.
    fn speak(&mut self, node: &str, app: &str, binary: &str, media: &str) -> anyhow::Result<u32> {
        let props = format!(
            "application.name = \"{app}\" application.process.binary = \"{binary}\" \
             media.name = \"{media}\""
        );
        self.fakes.capture(node, "Stream/Input/Audio", &props, MIC)
    }

    /// A camera's node.
    fn camera(&mut self) -> anyhow::Result<u32> {
        self.fakes.node(
            "hocus-test-camera",
            "Video/Source",
            "node.description = \"Caméra\"",
            true,
        )
    }

    /// A camera's stream of `app`, linked to the camera's node.
    fn film(&mut self, node: &str, app: &str) -> anyhow::Result<u32> {
        let props = format!("application.name = \"{app}\"");
        self.fakes
            .capture(node, "Stream/Input/Video", &props, "hocus-test-camera")
    }

    /// A process holding the camera's device open, as a program opening
    /// `/dev/video0` itself: `tail`, following it. Not a shell opening it
    /// for the program it then becomes: the shell reads `/proc` as the
    /// device is opened, which may be before that program replaced the
    /// shell, and nothing tells it of the change.
    fn hold_device(&mut self) -> anyhow::Result<()> {
        let child = Command::new("tail")
            .arg("-f")
            .arg(self.devices.join("video0"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()?;
        self.children.push(child);
        Ok(())
    }

    /// Waits until the source is muted (`muted`) or not, as `wpctl` reads
    /// it.
    fn source_muted(&mut self, muted: bool) -> anyhow::Result<()> {
        let start = Instant::now();
        while start.elapsed() < WAIT {
            if self.fakes.volume(MIC).is_ok_and(|(_, m)| m == muted) {
                return Ok(());
            }
            wait(&mut self.hp, Duration::from_millis(100))?;
        }
        anyhow::bail!("the source is not muted={muted}:\n{}", self.log())
    }

    /// A click at `(x, y)`, once what is there is drawn.
    fn click(&mut self, (x, y): (i32, i32)) -> anyhow::Result<()> {
        common::click(&mut self.hp, (f64::from(x), f64::from(y)))
    }

    /// The popover, opened by hovering the sign at `sign`: its surface.
    fn popover(&mut self, sign: Rect) -> anyhow::Result<Rect> {
        let (x, y) = (sign.x + sign.width / 2, sign.y + sign.height / 2);
        common::over(
            &mut self.hp,
            (f64::from(x), f64::from(y)),
            hocus_link::IN_USE_NAMESPACE,
        )?;
        let open = self
            .hp
            .dispatch_until(WAIT, |hp| !common::drawn_popovers(hp).is_empty())?;
        anyhow::ensure!(open, "no popover:\n{}", self.log());
        self.logged("the in-use popover shows it", |_| true)?;
        // Come to rest: its buttons take the pointer where they are drawn.
        common::at_rest(&mut self.hp)?;
        Ok(common::drawn_popovers(&self.hp)[0])
    }

    fn call(
        &mut self,
        method: &str,
        body: impl serde::Serialize + zbus::zvariant::DynamicType + Send + Sync + 'static,
    ) -> anyhow::Result<u32> {
        let client = self.client.clone();
        let method = method.to_owned();
        self.fakes.runtime().block_on(async move {
            let reply = client
                .call_method(Some(PORTAL), PATH, Some(CAST), method.as_str(), &body)
                .await?;
            let (response, _) = reply
                .body()
                .deserialize::<(u32, HashMap<String, OwnedVariant>)>()?;
            anyhow::Ok(response)
        })
    }

    /// The screen shared through the portal, as 5.11's tests do: the
    /// chooser's Enter.
    fn share_screen(&mut self) -> anyhow::Result<String> {
        self.sessions += 1;
        let n = self.sessions;
        let session = format!("{PATH}/session/1_1/s{n}");
        let path = |p: String| OwnedObjectPath::try_from(p);
        let app = "org.mozilla.firefox".to_owned();
        let none = HashMap::<String, OwnedVariant>::new;
        // The portal may not be on the bus yet.
        let start = Instant::now();
        loop {
            let created = self.call(
                "CreateSession",
                (
                    path(format!("{PATH}/request/1_1/c{n}"))?,
                    path(session.clone())?,
                    app.clone(),
                    none(),
                ),
            );
            match created {
                Ok(0) => break,
                Ok(other) => anyhow::bail!("CreateSession: {other}"),
                Err(_) if start.elapsed() < WAIT => wait(&mut self.hp, Duration::from_millis(100))?,
                Err(err) => return Err(err),
            }
        }
        let mut options = none();
        options.insert(
            "types".into(),
            OwnedVariant::try_from(Variant::U32(hocus_link::SHARE_MONITOR))?,
        );
        let selected = self.call(
            "SelectSources",
            (
                path(format!("{PATH}/request/1_1/l{n}"))?,
                path(session.clone())?,
                app.clone(),
                options,
            ),
        )?;
        anyhow::ensure!(selected == 0, "SelectSources: {selected}");
        let client = self.client.clone();
        let body = (
            path(format!("{PATH}/request/1_1/t{n}"))?,
            path(session.clone())?,
            app,
            String::new(),
            none(),
        );
        let task = self.fakes.runtime().spawn(async move {
            client
                .call_method(Some(PORTAL), PATH, Some(CAST), "Start", &body)
                .await
                .map(|_| ())
        });
        let shown = self.hp.dispatch_until(Duration::from_secs(15), |hp| {
            hp.share_choosing().is_some()
                && hp.keyboard_layer().as_deref() == Some(hocus_link::SHARE_CHOOSER_NAMESPACE)
        })?;
        anyhow::ensure!(shown, "no chooser:\n{}", self.log());
        self.logged("the share's chooser shows it", |_| true)?;
        self.hp.key(KEY_ENTER, true);
        self.hp.key(KEY_ENTER, false);
        let done = self
            .hp
            .dispatch_until(Duration::from_secs(15), |_| task.is_finished())?;
        anyhow::ensure!(done, "Start never answered:\n{}", self.log());
        anyhow::ensure!(self.hp.shares().len() == 1, "{:?}", self.hp.shares());
        Ok(session)
    }

    /// The tiles' colours, as their brightest pixel says, in the sign of
    /// the surface at `surface` with `kinds` tiles in `form`.
    fn tiles(&mut self, surface: Rect, form: FormChoice, kinds: i32) -> anyhow::Result<Vec<Ink>> {
        self.inks(surface, form, &vec![Tile::Kind; kinds as usize])
    }

    /// The same, with the recording's tile among them: its dot's colour,
    /// read at its middle.
    fn inks(
        &mut self,
        surface: Rect,
        form: FormChoice,
        tiles: &[Tile],
    ) -> anyhow::Result<Vec<Ink>> {
        common::at_rest(&mut self.hp)?;
        let (w, _, pixels) = self.hp.screenshot(0)?;
        let size = |t: &Tile| match t {
            Tile::Kind => (30, 24),
            Tile::Recording => (36, 42),
        };
        let mut along = 0;
        let rects: Vec<Rect> = tiles
            .iter()
            .map(|t| {
                let (tw, th) = size(t);
                let r = match form {
                    // Stacked, centred in the 48 px sign 3 px into the
                    // surface, 6 px under its top 5 px into it.
                    FormChoice::Mini => Rect::new(
                        surface.x + 3 + (48 - tw) / 2,
                        surface.y + 11 + along,
                        tw,
                        th,
                    ),
                    // In a row, centred on the bar's middle (97 px into
                    // it), each centred on the row's height.
                    _ => {
                        let width = 18
                            + tiles.iter().map(|t| size(t).0).sum::<i32>()
                            + (tiles.len() as i32 - 1) * 4;
                        let row = tiles.iter().map(|t| size(t).1).max().unwrap_or(24);
                        Rect::new(
                            surface.x + 97 - width / 2 + 9 + along,
                            surface.y + 11 + (row - th) / 2,
                            tw,
                            th,
                        )
                    }
                };
                along += 4 + if form == FormChoice::Mini { th } else { tw };
                r
            })
            .collect();
        let pixel = |x: i32, y: i32| {
            let at = ((y * w + x) * 4) as usize;
            [pixels[at + 2], pixels[at + 1], pixels[at]]
        };
        Ok(tiles
            .iter()
            .zip(rects)
            .map(|(t, r)| match t {
                // Its 10 px dot, 6 px under its top.
                Tile::Recording => Ink::of_dot(pixel(r.x + 18, r.y + 11)),
                Tile::Kind => {
                    let mut best = [0u8; 3];
                    for y in r.y..r.y + r.height {
                        for x in r.x..r.x + r.width {
                            let rgb = pixel(x, y);
                            let sum = |c: [u8; 3]| c.iter().map(|v| u32::from(*v)).sum::<u32>();
                            if sum(rgb) > sum(best) {
                                best = rgb;
                            }
                        }
                    }
                    Ink::of(best)
                }
            })
            .collect())
    }

    /// The screen recorded, as 5.10's tests do: `Print`, Vidéo, the
    /// screen, Démarrer, until it records.
    fn record_screen(&mut self) -> anyhow::Result<()> {
        let pointer = self.hp.pointer();
        self.hp.pointer_move(400.0, 300.0);
        common::chord(&mut self.hp, &[KEY_PRINT]);
        self.hp.capture_ask(hocus_link::CaptureAsk::Video(true));
        self.hp.capture_ask(hocus_link::CaptureAsk::Mode(
            hocus_link::CaptureMode::Screen,
        ));
        self.hp.capture_ask(hocus_link::CaptureAsk::Shoot);
        let started = self.hp.dispatch_until(Duration::from_secs(30), |hp| {
            hp.recording().is_some_and(|r| r.started)
        })?;
        anyhow::ensure!(started, "not recording: {:?}", self.hp.record_failures());
        self.hp.pointer_move(pointer.0, pointer.1);
        Ok(())
    }

    /// The recording paused (`paused`) or going on, as the compositor
    /// says it.
    fn pause(&mut self, paused: bool) -> anyhow::Result<()> {
        self.hp.record_ask(if paused {
            hocus_link::RecordAsk::Pause
        } else {
            hocus_link::RecordAsk::Resume
        });
        let done = self.hp.dispatch_until(WAIT, |hp| {
            hp.recording().is_some_and(|r| r.paused == paused)
        })?;
        anyhow::ensure!(done, "not paused={paused}:\n{}", self.log());
        Ok(())
    }

    /// Waits until the sign is drawn at rest with the recording `paused`
    /// or not, said after `mark`.
    fn rec_at_rest(&mut self, mark: usize, paused: bool) -> anyhow::Result<()> {
        let want = format!("paused=Some({paused})");
        self.logged_since(mark, "the in-use sign is drawn at rest", |l| {
            l.contains(&want)
        })?;
        Ok(())
    }

    /// Waits until the sign has not committed for a second: a tile gone
    /// resizes its surface, and the shell draws again at its new size
    /// after it said what it shows.
    fn settled(&mut self) -> anyhow::Result<u64> {
        common::at_rest(&mut self.hp)?;
        let mut last = self.hp.in_use_commits();
        let mut since = Instant::now();
        let still = self.hp.dispatch_until(WAIT, |hp| {
            let now = hp.in_use_commits();
            if now != last {
                (last, since) = (now, Instant::now());
            }
            since.elapsed() >= Duration::from_secs(1)
        })?;
        anyhow::ensure!(still, "the sign kept drawing:\n{}", self.log());
        Ok(self.hp.in_use_commits())
    }

    /// What the recording's tile reads after `mark` once `pass` takes it:
    /// its time ("0:03"), or "pause".
    fn rec_reads(&mut self, mark: usize, pass: impl Fn(&str) -> bool) -> anyhow::Result<String> {
        let line = self.logged_since(mark, "the in-use sign's recording reads", |l| {
            reads(l).is_some_and(&pass)
        })?;
        Ok(reads(&line).unwrap_or_default().to_owned())
    }
}

/// What a line of the recording's tile says it reads.
fn reads(line: &str) -> Option<&str> {
    line.split("time=").nth(1)?.split_whitespace().next()
}

/// "0:03" in seconds.
fn seconds(time: &str) -> Option<u64> {
    let (m, s) = time.split_once(':')?;
    Some(m.parse::<u64>().ok()? * 60 + s.parse::<u64>().ok()?)
}

/// A tile of the sign: a kind's 30 × 24, or the recording's 36 × 42.
#[derive(Debug, Clone, Copy)]
enum Tile {
    Kind,
    Recording,
}

/// A tile's colour, as its icon's brightest pixel reads (the recording's,
/// its dot).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ink {
    Orange,
    Green,
    Blue,
    Grey,
    Red,
    Amber,
    Other([u8; 3]),
}

impl Ink {
    /// The recording's dot: `#F2545B`, or `#F5B44A` while paused.
    fn of_dot([r, g, b]: [u8; 3]) -> Self {
        let near = |a: u8, b: u8| a.abs_diff(b) <= 12;
        if near(r, 0xF2) && near(g, 0x54) && near(b, 0x5B) {
            Self::Red
        } else if near(r, 0xF5) && near(g, 0xB4) && near(b, 0x4A) {
            Self::Amber
        } else {
            Self::Other([r, g, b])
        }
    }

    fn of([r, g, b]: [u8; 3]) -> Self {
        let (r, g, b) = (i32::from(r), i32::from(g), i32::from(b));
        if r > b + 80 && r > g {
            Self::Orange
        } else if g > r + 50 && g > b + 25 {
            Self::Green
        } else if b > r + 70 {
            Self::Blue
        } else if (r - b).abs() < 45 && (g - b).abs() < 45 && r > 120 {
            Self::Grey
        } else {
            Self::Other([r as u8, g as u8, b as u8])
        }
    }
}

/// The popover's last button's middle, and the one `n` buttons above it.
fn button(popover: Rect, n: i32) -> (i32, i32) {
    (
        popover.x + popover.width / 2,
        popover.y + popover.height - 13 - 15 - n * 38,
    )
}

#[test]
fn the_microphone_shows_its_sign_as_a_stream_takes_a_source() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, (1920, 1080), FormChoice::Mini)?;
    // A stream not linked to the source is not a use.
    bed.fakes.node(
        "hocus-test-idle",
        "Stream/Input/Audio",
        "application.name = \"Idle\"",
        false,
    )?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert!(bed.hp.layer_rects(hocus_link::IN_USE_NAMESPACE).is_empty());
    let started = Instant::now();
    bed.speak("hocus-test-meet", "Firefox", "firefox", "meet.google.com")?;
    let sign = bed.sign("Microphone")?;
    // Printed, never asserted: the shell shows it as PipeWire tells it,
    // no sooner (its unit tests: at once).
    eprintln!("the microphone's sign in {:?}", started.elapsed());
    // Under the clock, the mini bar's: one tile, 36 tall.
    assert_eq!((sign.width, sign.height), (54, 36 + 10), "{sign:?}");
    assert!(sign.y < 200, "{sign:?}");
    assert_eq!(bed.tiles(sign, FormChoice::Mini, 1)?, [Ink::Orange]);
    Ok(())
}

#[test]
fn a_use_lingers_3_s_after_it_ends() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, (1920, 1080), FormChoice::Mini)?;
    bed.speak("hocus-test-short", "Discord", "discord", "")?;
    bed.sign("Microphone")?;
    bed.fakes.stop("hocus-test-short")?;
    let gone = bed.hp.dispatch_until(Duration::from_secs(15), |hp| {
        hp.layer_rects(hocus_link::IN_USE_NAMESPACE).is_empty()
    })?;
    anyhow::ensure!(gone, "the sign stayed:\n{}", bed.log());
    // The shell's monotonic clock: it went no sooner than 3 s after the
    // use ended.
    let line = bed.logged("the in-use sign goes", |_| true)?;
    let lingered: u64 = line
        .split("lingered_ms=")
        .nth(1)
        .and_then(|v| v.split_whitespace().next())
        .context("no linger")?
        .parse()?;
    assert!(lingered >= 3000, "{line}");
    Ok(())
}

#[test]
fn the_camera_shows_through_pipewire_and_through_its_device() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, (1920, 1080), FormChoice::Mini)?;
    bed.camera()?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert!(bed.hp.layer_rects(hocus_link::IN_USE_NAMESPACE).is_empty());
    // A stream linked to the camera's node.
    bed.film("hocus-test-zoom", "Zoom")?;
    let sign = bed.sign("Camera")?;
    assert_eq!(bed.tiles(sign, FormChoice::Mini, 1)?, [Ink::Green]);
    bed.fakes.stop("hocus-test-zoom")?;
    bed.logged("the in-use sign goes", |_| true)?;
    // A program holding the device open itself.
    let started = Instant::now();
    let mark = bed.mark();
    bed.hold_device()?;
    bed.sign_since(mark, "Camera")?;
    eprintln!("the camera's device's sign in {:?}", started.elapsed());
    bed.logged("a camera's device is held open", |l| {
        l.contains("program=\"tail\"")
    })?;
    Ok(())
}

#[test]
fn the_screen_shows_as_it_is_shared() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, (1920, 1080), FormChoice::Mini)?;
    bed.share_screen()?;
    let sign = bed.sign("Screen")?;
    assert_eq!(bed.tiles(sign, FormChoice::Mini, 1)?, [Ink::Blue]);
    Ok(())
}

#[test]
fn muted_the_microphone_is_grey() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, (1920, 1080), FormChoice::Mini)?;
    bed.speak("hocus-test-meet", "Firefox", "firefox", "")?;
    let sign = bed.sign("Microphone")?;
    let mic = bed.fakes.sink(MIC)?.to_string();
    bed.fakes.wpctl(&["set-mute", &mic, "1"])?;
    bed.logged("the in-use sign is drawn at rest", |l| {
        l.contains("muted=true")
    })?;
    assert_eq!(bed.tiles(sign, FormChoice::Mini, 1)?, [Ink::Grey]);
    let mark = bed.mark();
    bed.fakes.wpctl(&["set-mute", &mic, "0"])?;
    bed.logged_since(mark, "the in-use sign is drawn at rest", |l| {
        l.contains("muted=false")
    })?;
    assert_eq!(bed.tiles(sign, FormChoice::Mini, 1)?, [Ink::Orange]);
    Ok(())
}

#[test]
fn the_kinds_side_by_side() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, (1920, 1080), FormChoice::Mini)?;
    bed.speak("hocus-test-meet", "Firefox", "firefox", "")?;
    bed.camera()?;
    bed.film("hocus-test-cam", "Firefox")?;
    bed.share_screen()?;
    let sign = bed.sign("Microphone, Camera, Screen")?;
    // Stacked: 6 + 3 × 24 + 2 × 4 + 6, and the surface's 5 px around.
    let grown = bed.hp.dispatch_until(WAIT, |hp| {
        hp.layer_rects(hocus_link::IN_USE_NAMESPACE)
            .first()
            .is_some_and(|(_, r)| r.height == 92 + 10)
    })?;
    assert!(
        grown,
        "{:?}",
        bed.hp.layer_rects(hocus_link::IN_USE_NAMESPACE)
    );
    assert_eq!(
        bed.tiles(sign, FormChoice::Mini, 3)?,
        [Ink::Orange, Ink::Green, Ink::Blue]
    );
    Ok(())
}

#[test]
fn in_the_full_form_the_tiles_are_in_a_row() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, (1920, 1080), FormChoice::Full)?;
    bed.speak("hocus-test-meet", "Firefox", "firefox", "")?;
    bed.camera()?;
    bed.film("hocus-test-cam", "Firefox")?;
    let sign = bed.sign("Microphone, Camera")?;
    // The full bar's width, one row's height.
    assert_eq!((sign.width, sign.height), (194, 36 + 10), "{sign:?}");
    assert_eq!(
        bed.tiles(sign, FormChoice::Full, 2)?,
        [Ink::Orange, Ink::Green]
    );
    Ok(())
}

#[test]
fn the_popover_says_who_uses_what_and_for_how_long() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, (1920, 1080), FormChoice::Mini)?;
    // Firefox's window, its title the line's detail.
    let (_, firefox) = common::open_with(&mut bed.hp, "firefox", (600, 400), false)?;
    firefox
        .commands
        .send(common::Command::Title("Réunion d'équipe — Meet"))?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    bed.speak("hocus-test-meet", "Firefox", "firefox", "AudioStream")?;
    // Discord without a window: its stream's media name.
    bed.speak("hocus-test-call", "Discord", "discord", "Appel vocal")?;
    let sign = bed.sign("Microphone")?;
    bed.hp.pointer_move(400.0, 300.0);
    let popover = bed.popover(sign)?;
    assert_eq!(popover.width, 290, "{popover:?}");
    let said = bed.logged("the in-use popover shows it", |_| true)?;
    assert!(said.contains("\"En ce moment\""), "{said}");
    assert!(said.contains("\"Firefox\""), "{said}");
    assert!(
        said.contains("\"Micro · Réunion d'équipe — Meet · depuis "),
        "{said}"
    );
    assert!(said.contains("\"Discord\""), "{said}");
    assert!(said.contains("\"Micro · Appel vocal · depuis "), "{said}");
    assert!(said.contains("\"Couper le micro\""), "{said}");
    drop(firefox);
    Ok(())
}

#[test]
fn its_button_mutes_the_default_source_and_back() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, (1920, 1080), FormChoice::Mini)?;
    bed.speak("hocus-test-meet", "Firefox", "firefox", "")?;
    let sign = bed.sign("Microphone")?;
    let popover = bed.popover(sign)?;
    bed.click(button(popover, 0))?;
    bed.source_muted(true)?;
    bed.logged("the in-use popover shows it", |l| {
        l.contains("\"Rétablir le micro\"") && l.contains("\"Micro coupé · depuis ")
    })?;
    bed.click(button(popover, 0))?;
    bed.source_muted(false)?;
    Ok(())
}

#[test]
fn its_buttons_stop_the_share_and_the_recording() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, (1920, 1080), FormChoice::Mini)?;
    bed.share_screen()?;
    let sign = bed.sign("Screen")?;
    let popover = bed.popover(sign)?;
    let said = bed.logged("the in-use popover shows it", |_| true)?;
    assert!(
        said.contains("\"Écran · partage l'écran « HEADLESS-1 » · depuis "),
        "{said}"
    );
    assert!(
        said.contains("\"Les notifications attendent la fin\""),
        "{said}"
    );
    bed.click(button(popover, 0))?;
    let stopped = bed.hp.dispatch_until(WAIT, |hp| hp.shares().is_empty())?;
    assert!(stopped, "still shared:\n{}", bed.log());
    bed.logged("the in-use sign goes", |_| true)?;
    if !has("ffmpeg") {
        common::missing("ffmpeg");
    }
    let mark = bed.mark();
    // A recording of the screen: its line and its buttons.
    bed.hp.pointer_move(400.0, 300.0);
    common::chord(&mut bed.hp, &[KEY_PRINT]);
    bed.hp.capture_ask(hocus_link::CaptureAsk::Video(true));
    bed.hp.capture_ask(hocus_link::CaptureAsk::Mode(
        hocus_link::CaptureMode::Screen,
    ));
    bed.hp.capture_ask(hocus_link::CaptureAsk::Shoot);
    let started = bed.hp.dispatch_until(Duration::from_secs(30), |hp| {
        hp.recording().is_some_and(|r| r.started)
    })?;
    anyhow::ensure!(started, "not recording: {:?}", bed.hp.record_failures());
    let sign = bed.sign_since(mark, "Recording")?;
    let popover = bed.popover(sign)?;
    bed.logged_since(mark, "the in-use popover shows it", |l| {
        l.contains("\"Enregistrement d'écran\"")
            && l.contains("\"Écran · enregistrement · ")
            && l.contains("\"Arrêter l'enregistrement\"")
    })?;
    // Its pause, at the left of its row.
    let (_, y) = button(popover, 0);
    bed.click((popover.x + 13 + 18, y))?;
    let paused = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.recording().is_some_and(|r| r.paused))?;
    assert!(paused, "not paused:\n{}", bed.log());
    bed.logged("the in-use popover shows it", |l| {
        l.contains("\"Écran · en pause · ")
    })?;
    // Its stop.
    bed.click(button(popover, 0))?;
    let stopped = bed
        .hp
        .dispatch_until(Duration::from_secs(30), |hp| hp.recording().is_none())?;
    assert!(stopped, "not stopped:\n{}", bed.log());
    Ok(())
}

#[test]
fn nothing_is_read_nor_drawn_at_rest() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, (1920, 1080), FormChoice::Mini)?;
    bed.speak("hocus-test-meet", "Firefox", "firefox", "")?;
    bed.sign("Microphone")?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    // In use, the popover closed: the sign draws nothing more, and no
    // camera's node, so `/proc` is never read.
    let commits = bed.hp.in_use_commits();
    let shown = bed.lines("the in-use sign").len();
    wait(&mut bed.hp, Duration::from_secs(3))?;
    assert_eq!(bed.hp.in_use_commits(), commits, "the sign drew at rest");
    assert_eq!(bed.lines("the in-use sign").len(), shown);
    assert!(bed.lines("the cameras' holders read").is_empty());
    // A camera's node, nobody using it: one reading, then none.
    bed.camera()?;
    bed.logged("the cameras' holders read", |_| true)?;
    wait(&mut bed.hp, Duration::from_secs(3))?;
    assert_eq!(
        bed.lines("the cameras' holders read").len(),
        1,
        "{}",
        bed.log()
    );
    Ok(())
}

#[test]
fn proc_is_read_at_most_once_a_second_while_a_cameras_node_exists() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, (1920, 1080), FormChoice::Mini)?;
    // No camera's node: the device opened, nothing read.
    let device = bed.devices.join("video0");
    for _ in 0..5 {
        drop(std::fs::File::open(&device)?);
        wait(&mut bed.hp, Duration::from_millis(100))?;
    }
    assert!(bed.lines("the cameras' holders read").is_empty());
    bed.camera()?;
    bed.logged("the cameras' holders read", |_| true)?;
    // Opened and closed twenty times over two seconds: a reading a second
    // at most, and the last touch read.
    let before = bed.lines("the cameras' holders read").len();
    let started = Instant::now();
    for _ in 0..20 {
        drop(std::fs::File::open(&device)?);
        wait(&mut bed.hp, Duration::from_millis(100))?;
    }
    wait(&mut bed.hp, Duration::from_millis(1200))?;
    let readings = bed.lines("the cameras' holders read").len() - before;
    eprintln!("{readings} readings in {:?}", started.elapsed());
    let bound = started.elapsed().as_secs() as usize + 1;
    assert!(readings >= 1, "the touches never read");
    assert!(
        readings <= bound,
        "{readings} readings in {:?}",
        started.elapsed()
    );
    // The camera's node gone: the device opened, nothing read.
    bed.fakes.stop("hocus-test-camera")?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    let after = bed.lines("the cameras' holders read").len();
    for _ in 0..5 {
        drop(std::fs::File::open(&device)?);
        wait(&mut bed.hp, Duration::from_millis(100))?;
    }
    wait(&mut bed.hp, Duration::from_millis(1200))?;
    assert_eq!(bed.lines("the cameras' holders read").len(), after);
    Ok(())
}

#[test]
fn while_recording_its_red_tile_says_its_time_amber_and_pause_while_paused() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    if !has("ffmpeg") {
        common::missing("ffmpeg");
    }
    let mut bed = Bed::start(&shell, (1920, 1080), FormChoice::Mini)?;
    let mark = bed.mark();
    bed.record_screen()?;
    // Its own tile, 5.10's item: 36 × 42 in the 48 px sign, 6 px around.
    let sign = bed.sign_since(mark, "Recording")?;
    assert_eq!((sign.width, sign.height), (54, 54 + 10), "{sign:?}");
    assert_eq!(
        bed.inks(sign, FormChoice::Mini, &[Tile::Recording])?,
        [Ink::Red]
    );
    // Its time, "0:0N", and it moves on.
    let first = bed.rec_reads(mark, |t| t.starts_with("0:0"))?;
    let then = seconds(&first).context("a time")?;
    let later = bed.rec_reads(mark, |t| seconds(t).is_some_and(|s| s > then))?;
    eprintln!("the recording's tile read {first}, then {later}");
    // Drawn again as its second changes, on the CPU without a beat: about
    // once a second, never four times (5.10's 250 ms).
    common::at_rest(&mut bed.hp)?;
    let commits = bed.hp.in_use_commits();
    let started = Instant::now();
    wait(&mut bed.hp, Duration::from_secs(3))?;
    let drawn = bed.hp.in_use_commits() - commits;
    let bound = started.elapsed().as_secs() + 2;
    eprintln!("{drawn} commits of the sign in {:?}", started.elapsed());
    assert!(
        (1..=bound).contains(&drawn),
        "{drawn} commits in {:?}",
        started.elapsed()
    );
    // Paused: the dot amber and still, "pause".
    let mark = bed.mark();
    bed.pause(true)?;
    assert_eq!(bed.rec_reads(mark, |t| t == "pause")?, "pause");
    bed.rec_at_rest(mark, true)?;
    assert_eq!(
        bed.inks(sign, FormChoice::Mini, &[Tile::Recording])?,
        [Ink::Amber]
    );
    // Going on: red again, its time again.
    let mark = bed.mark();
    bed.pause(false)?;
    bed.rec_reads(mark, |t| seconds(t).is_some())?;
    bed.rec_at_rest(mark, false)?;
    assert_eq!(
        bed.inks(sign, FormChoice::Mini, &[Tile::Recording])?,
        [Ink::Red]
    );
    // Stopped: the tile goes once it has lingered 5.12's 3 s.
    let mark = bed.mark();
    bed.hp.record_ask(hocus_link::RecordAsk::Stop);
    let gone = bed.hp.dispatch_until(Duration::from_secs(30), |hp| {
        hp.layer_rects(hocus_link::IN_USE_NAMESPACE).is_empty()
    })?;
    anyhow::ensure!(gone, "the sign stayed:\n{}", bed.log());
    let line = bed.logged_since(mark, "the in-use sign goes", |_| true)?;
    let lingered: u64 = line
        .split("lingered_ms=")
        .nth(1)
        .and_then(|v| v.split_whitespace().next())
        .context("no linger")?
        .parse()?;
    assert!(lingered >= 3000, "{line}");
    Ok(())
}

#[test]
fn with_a_share_and_a_recording_both_tiles_the_recordings_first() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    if !has("ffmpeg") {
        common::missing("ffmpeg");
    }
    let mut bed = Bed::start(&shell, (1920, 1080), FormChoice::Mini)?;
    bed.share_screen()?;
    bed.sign("Screen")?;
    let mark = bed.mark();
    bed.record_screen()?;
    let sign = bed.sign_since(mark, "Recording, Screen")?;
    // Stacked: 6 + 42 + 4 + 24 + 6, and the surface's 5 px around.
    let grown = bed.hp.dispatch_until(WAIT, |hp| {
        hp.layer_rects(hocus_link::IN_USE_NAMESPACE)
            .first()
            .is_some_and(|(_, r)| r.height == 82 + 10)
    })?;
    assert!(
        grown,
        "{:?}",
        bed.hp.layer_rects(hocus_link::IN_USE_NAMESPACE)
    );
    assert_eq!(
        bed.inks(sign, FormChoice::Mini, &[Tile::Recording, Tile::Kind])?,
        [Ink::Red, Ink::Blue]
    );
    Ok(())
}

#[test]
fn in_the_full_form_the_recordings_tile_is_in_the_row() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    if !has("ffmpeg") {
        common::missing("ffmpeg");
    }
    let mut bed = Bed::start(&shell, (1920, 1080), FormChoice::Full)?;
    bed.speak("hocus-test-meet", "Firefox", "firefox", "")?;
    bed.sign("Microphone")?;
    let mark = bed.mark();
    bed.record_screen()?;
    // After the microphone's, the row as tall as its tile.
    let sign = bed.sign_since(mark, "Microphone, Recording")?;
    let grown = bed.hp.dispatch_until(WAIT, |hp| {
        hp.layer_rects(hocus_link::IN_USE_NAMESPACE)
            .first()
            .is_some_and(|(_, r)| (r.width, r.height) == (194, 54 + 10))
    })?;
    assert!(
        grown,
        "{:?}",
        bed.hp.layer_rects(hocus_link::IN_USE_NAMESPACE)
    );
    assert_eq!(
        bed.inks(sign, FormChoice::Full, &[Tile::Kind, Tile::Recording])?,
        [Ink::Orange, Ink::Red]
    );
    Ok(())
}

#[test]
fn paused_or_gone_the_recording_draws_nothing_at_rest() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    if !has("ffmpeg") {
        common::missing("ffmpeg");
    }
    let mut bed = Bed::start(&shell, (1920, 1080), FormChoice::Mini)?;
    bed.share_screen()?;
    bed.sign("Screen")?;
    let mark = bed.mark();
    bed.record_screen()?;
    bed.sign_since(mark, "Recording, Screen")?;
    // Paused: its time still, nothing drawn.
    let mark = bed.mark();
    bed.pause(true)?;
    bed.rec_at_rest(mark, true)?;
    let commits = bed.settled()?;
    wait(&mut bed.hp, Duration::from_secs(2))?;
    assert_eq!(bed.hp.in_use_commits(), commits, "the sign drew, paused");
    // Stopped, the share still on: once the recording's tile has gone,
    // nothing wakes.
    let mark = bed.mark();
    bed.hp.record_ask(hocus_link::RecordAsk::Stop);
    bed.sign_since(mark, "Screen")?;
    let commits = bed.settled()?;
    let reads = bed.lines("the in-use sign's recording reads").len();
    wait(&mut bed.hp, Duration::from_secs(3))?;
    assert_eq!(bed.hp.in_use_commits(), commits, "the sign drew at rest");
    assert_eq!(bed.lines("the in-use sign's recording reads").len(), reads);
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
    let Some(shell) = ready() else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let shoot = |bed: &mut Bed, name: &str, crop: (i32, i32, i32, i32)| -> anyhow::Result<()> {
        common::at_rest(&mut bed.hp)?;
        let (w, h, px) = bed.hp.screenshot(0)?;
        let rgba: Vec<u8> = px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
        let (x, y, cw, ch) = crop;
        let image =
            image::imageops::crop_imm(&image, x as u32, y as u32, cw as u32, ch as u32).to_image();
        let file = out.join(name);
        image.save(&file)?;
        eprintln!("{}", file.display());
        Ok(())
    };
    let bar = (2560 - 420, 0, 420, 560);
    for form in [FormChoice::Mini, FormChoice::Full] {
        let suffix = if form == FormChoice::Full {
            "full"
        } else {
            "mini"
        };
        let mut bed = Bed::start(&shell, (2560, 1600), form)?;
        let (_, window) = common::open_with(&mut bed.hp, "firefox", (1400, 900), false)?;
        window
            .commands
            .send(common::Command::Title("Réunion d'équipe — Meet"))?;
        bed.hp.pointer_move(900.0, 700.0);
        bed.speak("hocus-test-meet", "Firefox", "firefox", "")?;
        bed.sign("Microphone")?;
        shoot(&mut bed, &format!("sign-one-{suffix}.png"), bar)?;
        if form == FormChoice::Mini {
            let mic = bed.fakes.sink(MIC)?.to_string();
            bed.fakes.wpctl(&["set-mute", &mic, "1"])?;
            bed.logged("the in-use sign is drawn at rest", |l| {
                l.contains("muted=true")
            })?;
            shoot(&mut bed, "sign-muted-mini.png", bar)?;
            let mark = bed.mark();
            bed.fakes.wpctl(&["set-mute", &mic, "0"])?;
            bed.logged_since(mark, "the in-use sign is drawn at rest", |l| {
                l.contains("muted=false")
            })?;
        }
        bed.camera()?;
        bed.film("hocus-test-cam", "Firefox")?;
        bed.share_screen()?;
        let sign = bed.sign("Microphone, Camera, Screen")?;
        bed.hp.pointer_move(900.0, 700.0);
        shoot(&mut bed, &format!("sign-three-{suffix}.png"), bar)?;
        bed.popover(sign)?;
        shoot(
            &mut bed,
            &format!("popover-{suffix}.png"),
            (2560 - 760, 0, 760, 760),
        )?;
        drop(window);
    }
    Ok(())
}

/// 5.12b's screenshots (issue #561), at 2560 × 1600 at scale 1, the shell
/// on the CPU: the bar, and the sign 3 × larger, while recording, paused,
/// with a share, and with the four kinds; written to `$HOCUS_SHOTS` when
/// it names a folder. Run by hand (`--run-ignored only`).
#[test]
#[ignore = "writes the pull request's screenshots"]
fn screenshots_of_the_recording_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    let Some(shell) = ready() else {
        return Ok(());
    };
    if !has("ffmpeg") {
        return Ok(());
    }
    std::fs::create_dir_all(&out)?;
    let shoot = |bed: &mut Bed, name: &str| -> anyhow::Result<()> {
        common::at_rest(&mut bed.hp)?;
        let sign = common::drawn(&mut bed.hp, hocus_link::IN_USE_NAMESPACE)?[0].1;
        let (w, h, px) = bed.hp.screenshot(0)?;
        let rgba: Vec<u8> = px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
        let bar = image::imageops::crop_imm(&image, 2560 - 420, 0, 420, 560).to_image();
        let file = out.join(format!("{name}.png"));
        bar.save(&file)?;
        eprintln!("{}", file.display());
        let (x, y) = ((sign.x - 16).max(0) as u32, (sign.y - 16).max(0) as u32);
        let (cw, ch) = ((sign.width + 32) as u32, (sign.height + 32) as u32);
        let zoomed = image::imageops::resize(
            &image::imageops::crop_imm(&image, x, y, cw, ch).to_image(),
            cw * 3,
            ch * 3,
            image::imageops::FilterType::Nearest,
        );
        let file = out.join(format!("{name}-zoom.png"));
        zoomed.save(&file)?;
        eprintln!("{}", file.display());
        Ok(())
    };
    for form in [FormChoice::Mini, FormChoice::Full] {
        let suffix = if form == FormChoice::Full {
            "full"
        } else {
            "mini"
        };
        let mut bed = Bed::start(&shell, (2560, 1600), form)?;
        let (_, window) = common::open_with(&mut bed.hp, "firefox", (1400, 900), false)?;
        window
            .commands
            .send(common::Command::Title("Réunion d'équipe — Meet"))?;
        bed.hp.pointer_move(900.0, 700.0);
        let mark = bed.mark();
        bed.record_screen()?;
        bed.sign_since(mark, "Recording")?;
        bed.rec_reads(mark, |t| seconds(t).is_some_and(|s| s >= 3))?;
        shoot(&mut bed, &format!("rec-{suffix}"))?;
        let mark = bed.mark();
        bed.pause(true)?;
        bed.rec_at_rest(mark, true)?;
        shoot(&mut bed, &format!("rec-paused-{suffix}"))?;
        bed.pause(false)?;
        let mark = bed.mark();
        bed.share_screen()?;
        bed.sign_since(mark, "Recording, Screen")?;
        bed.hp.pointer_move(900.0, 700.0);
        shoot(&mut bed, &format!("rec-share-{suffix}"))?;
        let mark = bed.mark();
        bed.speak("hocus-test-meet", "Firefox", "firefox", "")?;
        bed.camera()?;
        bed.film("hocus-test-cam", "Firefox")?;
        bed.sign_since(mark, "Microphone, Camera, Recording, Screen")?;
        shoot(&mut bed, &format!("rec-all-{suffix}"))?;
        bed.hp.record_ask(hocus_link::RecordAsk::Stop);
        let stopped = bed
            .hp
            .dispatch_until(Duration::from_secs(30), |hp| hp.recording().is_none())?;
        anyhow::ensure!(stopped, "not stopped");
        drop(window);
    }
    Ok(())
}
