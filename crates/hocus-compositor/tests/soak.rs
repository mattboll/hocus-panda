//! The compositor's memory over hours of use, compressed (#827): windows
//! opened and closed by clients that come and go, a browser's drawn again
//! and again with its tab's previews, a screen unplugged and
//! plugged again, desktops switched, the moving wallpapers turned on and
//! off, the overview, the launcher and a popover of the shell, captures,
//! and a call lit by the screen light. Its heap, sampled after each
//! cycle, must not keep growing once warm.
//!
//! The long run is ignored (minutes); run it on purpose:
//!
//! ```sh
//! cargo programs
//! HOCUS_SOAK_CYCLES=120 cargo nextest run -p hocus-compositor \
//!     --run-ignored only soak::a_day_of_use_leaves_the_heap_flat
//! # GLES on Mesa's software device instead of pixman: its textures live
//! # in the process's memory, where a texture kept shows
//! HOCUS_SOAK_RENDERER=gles HOCUS_SOAK_CYCLES=60 cargo nextest run ...
//! ```
//!
//! It prints each cycle's sample and, at the end, what each activity
//! added to the heap over the run.

use crate::common;

use std::{collections::BTreeMap, path::Path, time::Duration};

use anyhow::Context as _;
use common::{
    ClientHandle, Command, KEY_1, KEY_2, KEY_ESC, KEY_LEFTMETA, Shows, WAIT, chord, wait,
};
use hocus_compositor::{
    Config, Headless, HeadlessConfig, LightShape, LightStrength, OutputSpec, RendererChoice,
};
use hocus_link::{CameraUser, CaptureAsk, CaptureMode, ToShell};

const KEY_W: u32 = 17;
const KEY_ENTER: u32 = 28;
const KEY_PRINT: u32 = 99;
const CALL: &str = "org.example.visio";

/// The process's memory: its resident set, and what malloc holds for it
/// (in use, and taken from the system in its arenas, mapped chunks
/// apart), in KiB.
#[derive(Debug, Clone, Copy)]
struct Sample {
    rss: i64,
    in_use: i64,
    arenas: i64,
}

fn sample() -> anyhow::Result<Sample> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    let rss = status
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
        .context("no VmRSS")?;
    // SAFETY: mallinfo2 reads malloc's own counters; it takes nothing.
    #[allow(unsafe_code)]
    let info = unsafe { libc::mallinfo2() };
    let kib = |b: usize| i64::try_from(b / 1024).unwrap_or(i64::MAX);
    Ok(Sample {
        rss,
        in_use: kib(info.uordblks + info.hblkhd),
        arenas: kib(info.arena),
    })
}

/// The soak's compositor: two screens, the shell, captures saved in a
/// folder of its own, the screen light around the call.
struct Soak {
    hp: Headless,
    config: Config,
    _dir: tempfile::TempDir,
    /// What each activity added to the heap in use, KiB, over the run.
    added: BTreeMap<&'static str, i64>,
    captures: u64,
    cycle: u64,
}

impl Soak {
    fn start(dir: tempfile::TempDir) -> anyhow::Result<Self> {
        let root = dir.path();
        let at = |p: &str| root.join(p).display().to_string();
        let script = common::shell_script(
            root,
            &common::shell_program(),
            &[
                ("HOME", at("home")),
                ("XDG_CONFIG_HOME", at("config")),
                ("XDG_STATE_HOME", at("state")),
                ("XDG_CACHE_HOME", at("cache")),
                ("ICED_BACKEND", "tiny-skia".to_owned()),
            ],
        )?;
        let mut config = Config {
            pictures: Some(root.join("Pictures")),
            ..Config::default()
        };
        config.keyboard.layouts = vec!["us".to_owned()];
        config.camera.screen_light = true;
        config.camera.screen_light_shape = LightShape::AroundCall;
        config.camera.screen_light_strength = LightStrength::Strong;
        let outputs = vec![OutputSpec::new(1920, 1080), OutputSpec::new(2560, 1440)];
        let headless = match std::env::var("HOCUS_SOAK_RENDERER").as_deref() {
            Ok("gles") => HeadlessConfig {
                outputs,
                ..common::gles()
            },
            _ => HeadlessConfig {
                outputs,
                renderer: RendererChoice::Pixman,
                render_node: None,
            },
        };
        let mut hp = Headless::start_with_shell(headless, config.clone(), Some(script))?;
        eprintln!("renderer: {}", hp.renderer());
        let up = hp.dispatch_until(WAIT, |hp| hp.bar_commits() > 0)?;
        anyhow::ensure!(up, "no bar");
        wait(&mut hp, Duration::from_secs(1))?;
        Ok(Self {
            hp,
            config,
            _dir: dir,
            added: BTreeMap::new(),
            captures: 0,
            cycle: 0,
        })
    }

    /// Runs `activity`, counting what it added to the heap in use.
    fn run(
        &mut self,
        name: &'static str,
        activity: impl FnOnce(&mut Self) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let before = sample()?;
        activity(self).with_context(|| format!("{name}, cycle {}", self.cycle))?;
        let after = sample()?;
        *self.added.entry(name).or_default() += after.in_use - before.in_use;
        Ok(())
    }

    /// One cycle of everything.
    fn cycle(&mut self) -> anyhow::Result<()> {
        self.run("windows", Self::windows)?;
        self.run("browser", Self::browser)?;
        self.run("screens", Self::screens)?;
        self.run("desktops", Self::desktops)?;
        self.run("effects", Self::effects)?;
        self.run("overview", Self::overview)?;
        self.run("launcher", Self::launcher)?;
        self.run("popover", Self::popover)?;
        self.run("capture", Self::capture)?;
        self.run("call", Self::call)?;
        self.run("rest", |soak| {
            wait(&mut soak.hp, Duration::from_millis(300))
        })?;
        self.cycle += 1;
        Ok(())
    }

    /// A client showing a window `app_id`, mapped.
    fn open(&mut self, app_id: &'static str, size: (u32, u32)) -> anyhow::Result<ClientHandle> {
        let before = self.hp.windows().len();
        let client = common::client(
            self.hp.socket(),
            Some(Shows::Window {
                app_id,
                size,
                popup: false,
            }),
        )?;
        let mapped = self
            .hp
            .dispatch_until(WAIT, |hp| hp.windows().len() > before)?;
        anyhow::ensure!(mapped, "{app_id} was never mapped");
        Ok(client)
    }

    /// Its window closed and its process gone, then its window too.
    fn quit(&mut self, client: &ClientHandle, app_id: &str) -> anyhow::Result<()> {
        client.commands.send(Command::Close)?;
        client.commands.send(Command::Quit)?;
        let gone = self.hp.dispatch_until(WAIT, |hp| {
            hp.windows()
                .iter()
                .all(|w| w.app_id.as_deref() != Some(app_id))
        })?;
        anyhow::ensure!(gone, "{app_id} stays");
        Ok(())
    }

    fn windows(&mut self) -> anyhow::Result<()> {
        const APPS: [&str; 3] = ["test.editor", "test.terminal", "test.browser"];
        let mut clients = Vec::new();
        for (i, app) in APPS.into_iter().enumerate() {
            let side = 500 + 150 * u32::try_from(i)?;
            clients.push((self.open(app, (side, side * 2 / 3))?, app));
        }
        wait(&mut self.hp, Duration::from_millis(200))?;
        for (client, app) in &clients {
            self.quit(client, app)?;
        }
        // Their closing, drawn to its end.
        wait(&mut self.hp, Duration::from_millis(400))
    }

    /// A window as a browser's: drawn again and again (a video), retitled,
    /// a tab's preview shown, then quit.
    fn browser(&mut self) -> anyhow::Result<()> {
        const TITLES: [&str; 3] = ["Meet", "Meet (1)", "Meet - presenting"];
        let browser = self.open("test.web", (1200, 800))?;
        for (i, colour) in (0u32..30).zip((0x10..).step_by(7)) {
            browser
                .commands
                .send(Command::Header(0xff00_0000 | colour << 8))?;
            if i % 10 == 0 {
                browser
                    .commands
                    .send(Command::Title(TITLES[(i / 10) as usize]))?;
                browser.commands.send(Command::Popup {
                    anchor: (40, 0, 200, 30),
                    size: (240, 140),
                })?;
            }
            wait(&mut self.hp, Duration::from_millis(30))?;
        }
        self.quit(&browser, "test.web")
    }

    fn screens(&mut self) -> anyhow::Result<()> {
        let window = self.open("test.kept", (700, 500))?;
        self.hp.unplug("HEADLESS-2")?;
        wait(&mut self.hp, Duration::from_millis(300))?;
        self.hp.plug("HEADLESS-2")?;
        let back = self.hp.dispatch_until(WAIT, |hp| hp.outputs().len() == 2)?;
        anyhow::ensure!(back, "the second screen did not come back");
        wait(&mut self.hp, Duration::from_millis(300))?;
        self.quit(&window, "test.kept")
    }

    fn desktops(&mut self) -> anyhow::Result<()> {
        let window = self.open("test.desk", (600, 400))?;
        chord(&mut self.hp, &[KEY_LEFTMETA, KEY_2]);
        wait(&mut self.hp, Duration::from_millis(400))?;
        chord(&mut self.hp, &[KEY_LEFTMETA, KEY_1]);
        wait(&mut self.hp, Duration::from_millis(400))?;
        self.quit(&window, "test.desk")
    }

    /// One of the moving wallpapers a cycle, on for a while, then off.
    fn effects(&mut self) -> anyhow::Result<()> {
        let mut config = self.config.clone();
        match self.cycle % 3 {
            0 => config.effects.aurora = true,
            1 => config.effects.koi = true,
            _ => config.effects.depth = true,
        }
        self.hp.reconfigure(config);
        wait(&mut self.hp, Duration::from_millis(600))?;
        self.hp.reconfigure(self.config.clone());
        wait(&mut self.hp, Duration::from_millis(200))
    }

    fn overview(&mut self) -> anyhow::Result<()> {
        let window = self.open("test.seen", (800, 500))?;
        chord(&mut self.hp, &[KEY_LEFTMETA, KEY_W]);
        let open = self
            .hp
            .dispatch_until(WAIT, |hp| hp.overview() == Some(true))?;
        anyhow::ensure!(open, "the overview did not open");
        wait(&mut self.hp, Duration::from_millis(400))?;
        chord(&mut self.hp, &[KEY_ESC]);
        let shut = self.hp.dispatch_until(WAIT, |hp| hp.overview().is_none())?;
        anyhow::ensure!(shut, "the overview stays");
        self.quit(&window, "test.seen")
    }

    fn launcher(&mut self) -> anyhow::Result<()> {
        let toggle = |hp: &mut Headless| {
            hp.tell_shell(ToShell::Launcher {
                output: "HEADLESS-1".to_owned(),
                scale: 120,
            });
        };
        toggle(&mut self.hp);
        let open = self
            .hp
            .dispatch_until(WAIT, |hp| !hp.launcher_surfaces().is_empty())?;
        anyhow::ensure!(open, "the launcher never opened");
        wait(&mut self.hp, Duration::from_millis(300))?;
        toggle(&mut self.hp);
        let shut = self
            .hp
            .dispatch_until(WAIT, |hp| hp.launcher_surfaces().is_empty())?;
        anyhow::ensure!(shut, "the launcher never closed");
        Ok(())
    }

    /// The clipboard's history, a popover beside the bar.
    fn popover(&mut self) -> anyhow::Result<()> {
        self.hp.tell_shell(ToShell::Clipboard {
            output: "HEADLESS-1".to_owned(),
        });
        let open = self.hp.dispatch_until(WAIT, |hp| {
            common::drawn_popovers(hp).len() == 1
                && hp.keyboard_layer().as_deref() == Some(hocus_link::POPOVER_NAMESPACE)
        })?;
        anyhow::ensure!(open, "the clipboard's history never opened");
        wait(&mut self.hp, Duration::from_millis(200))?;
        self.hp.key(KEY_ESC, true);
        self.hp.key(KEY_ESC, false);
        let shut = self
            .hp
            .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
        anyhow::ensure!(shut, "the clipboard's history never closed");
        Ok(())
    }

    /// A capture of a screen, saved.
    fn capture(&mut self) -> anyhow::Result<()> {
        shoot(&mut self.hp, &mut self.captures)?;
        wait(&mut self.hp, Duration::from_millis(300))
    }

    /// A call's window lit around while it uses the camera, then gone.
    fn call(&mut self) -> anyhow::Result<()> {
        let window = self.open(CALL, (900, 600))?;
        self.hp.camera_users(vec![CameraUser {
            app: "Visio".to_owned(),
            ids: vec!["visio".to_owned()],
        }]);
        let lit = self
            .hp
            .dispatch_until(WAIT, |hp| !hp.screen_light().is_empty())?;
        anyhow::ensure!(lit, "the screen light never came");
        wait(&mut self.hp, Duration::from_millis(800))?;
        self.hp.camera_users(Vec::new());
        let dark = self
            .hp
            .dispatch_until(WAIT, |hp| hp.screen_light().is_empty())?;
        anyhow::ensure!(dark, "the screen light stays");
        self.quit(&window, CALL)
    }
}

/// `Print`, the screen under the pointer, `Enter`: a capture saved, the
/// `taken`th; its file's size (its PNG, which the clipboard was given),
/// the file then removed so that the folder does not grow.
fn shoot(hp: &mut Headless, taken: &mut u64) -> anyhow::Result<u64> {
    chord(hp, &[KEY_PRINT]);
    wait(hp, Duration::from_millis(100))?;
    hp.capture_ask(CaptureAsk::Mode(CaptureMode::Screen));
    hp.pointer_move(900.0, 500.0);
    chord(hp, &[KEY_ENTER]);
    let want = *taken + 1;
    let saved = hp.dispatch_until(WAIT, |hp| hp.captures().0 >= want)?;
    anyhow::ensure!(saved, "the capture was never saved");
    *taken = want;
    let path = hp.captures().1.context("no capture's file")?;
    let size = std::fs::metadata(&path)?.len();
    remove(&path)?;
    Ok(size)
}

/// A saved capture, removed.
fn remove(path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(path) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
        _ => Ok(()),
    }
}

/// Captures taken one after another: the heap does not keep their PNGs.
/// Each stayed, held by the weak reference the capture's record keeps to
/// what the clipboard was given, up to the last 64: 225 KiB a capture of
/// this screen, megabytes of a real one (#827). The bound is half of
/// what they would have kept: the rest of the heap moves by a few dozen
/// KiB meanwhile.
#[test]
fn captures_leave_the_heap_flat() -> anyhow::Result<()> {
    const WARM: usize = 2;
    const TAKEN: usize = 12;
    let dir = tempfile::tempdir()?;
    let mut config = Config {
        pictures: Some(dir.path().join("Pictures")),
        ..Config::default()
    };
    config.keyboard.layouts = vec!["us".to_owned()];
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1920, 1080)],
            renderer: RendererChoice::Pixman,
            render_node: None,
        },
        config,
    )?;
    let drawn = hp.dispatch_until(WAIT, |hp| hp.frames() > 0)?;
    anyhow::ensure!(drawn, "nothing drawn");
    common::open(&mut hp, "test.editor", (1000, 700))?;
    let mut taken = 0;
    for _ in 0..WARM {
        shoot(&mut hp, &mut taken)?;
        wait(&mut hp, Duration::from_millis(100))?;
    }
    let before = sample()?;
    let mut pngs = 0;
    for _ in 0..TAKEN {
        pngs += shoot(&mut hp, &mut taken)?;
        wait(&mut hp, Duration::from_millis(100))?;
    }
    let after = sample()?;
    let grown = after.in_use - before.in_use;
    let pngs = i64::try_from(pngs / 1024)?;
    eprintln!("{TAKEN} captures, {pngs} KiB of PNG: the heap in use grew {grown} KiB");
    anyhow::ensure!(
        grown < pngs / 2,
        "{TAKEN} captures ({pngs} KiB of PNG) grew the heap in use by {grown} KiB"
    );
    Ok(())
}

/// The heap in use may grow this much over the second half of the run,
/// KiB: once warm it is flat but for what is freed later than it is
/// sampled (a closing's picture; on GLES, textures at its next cleanup),
/// a few hundred KiB either way on pixman, 2 MiB on GLES. A capture's PNG
/// kept (#827) grew it by 225 KiB a cycle: 13 MiB over the second half
/// of 120 cycles.
const BOUND_KIB: i64 = 4096;

/// Runs `warm` cycles, then `cycles` more, printing a sample after each;
/// the samples after the warm-up, half way and at the end.
fn soak(warm: u64, cycles: u64) -> anyhow::Result<[Sample; 3]> {
    let mut soak = Soak::start(tempfile::tempdir()?)?;
    for _ in 0..warm {
        soak.cycle()?;
    }
    wait(&mut soak.hp, Duration::from_millis(500))?;
    soak.added.clear();
    let first = sample()?;
    eprintln!("warm: {first:?}");
    let mut half = first;
    for n in 1..=cycles {
        soak.cycle()?;
        let now = sample()?;
        eprintln!(
            "cycle {n}: rss {} KiB ({:+}), in use {} KiB ({:+}), arenas {} KiB",
            now.rss,
            now.rss - first.rss,
            now.in_use,
            now.in_use - first.in_use,
            now.arenas,
        );
        if n == cycles / 2 {
            half = now;
        }
    }
    wait(&mut soak.hp, Duration::from_millis(500))?;
    let end = sample()?;
    eprintln!("end: {end:?}");
    eprintln!(
        "added to the heap in use, KiB, by activity: {:?}",
        soak.added
    );
    // What malloc holds freed and could give back.
    // SAFETY: malloc_trim only returns free memory to the system.
    #[allow(unsafe_code)]
    unsafe {
        libc::malloc_trim(0);
    }
    eprintln!("trimmed: {:?}", sample()?);
    Ok([first, half, end])
}

/// Hours of use, compressed (`HOCUS_SOAK_CYCLES`, 120 by default: about 18
/// minutes): over the second half, the heap in use grows by less than
/// [`BOUND_KIB`].
#[test]
#[ignore = "long: minutes (HOCUS_SOAK_CYCLES)"]
fn a_day_of_use_leaves_the_heap_flat() -> anyhow::Result<()> {
    let cycles = std::env::var("HOCUS_SOAK_CYCLES")
        .ok()
        .and_then(|c| c.parse().ok())
        .unwrap_or(120);
    let [warm, half, end] = soak(3, cycles)?;
    let grown = end.in_use - half.in_use;
    eprintln!(
        "{cycles} cycles: the heap in use grew {} KiB, {grown} KiB over the second half",
        end.in_use - warm.in_use
    );
    anyhow::ensure!(
        grown < BOUND_KIB,
        "the heap in use grew {grown} KiB over the last {} cycles",
        cycles - cycles / 2
    );
    Ok(())
}
