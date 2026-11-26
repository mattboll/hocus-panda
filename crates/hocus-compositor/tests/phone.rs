//! The phone, through KDE Connect (docs/design/bar.md "The phone", issue
//! #957, lot 1): the real shell against a fake KDE Connect
//! (`levels/kdeconnect.rs`), a fake NetworkManager and a fake BlueZ, all on
//! the fakes' private bus, which is the shell's system bus and the session
//! bus the compositor names (`--session-bus`); never the machine's.
//!
//! One test per acceptance criterion of the approved spec: the rows (in
//! reach, charging, low, out of reach, none, two, a laptop none, the
//! daemon gone and back, the mini form, the screen reader, at rest), the
//! popover (ring, send a file through the portal's chooser, the clipboard,
//! forget, a transfer's progress), the phone's notifications (its
//! application, its level, its reply from the bubble and from the bell,
//! ✕ on the phone, removed on the phone, a file received, a pairing), and
//! the daemon never started by the shell. A file dropped on a row is not
//! built yet (bar.md, "The phone").
//!
//! With `HOCUS_KIT_SHOTS=DIR`, the rows, the popover and the bubbles are
//! saved there as PNGs.
//!
//! Pixman here, so the shell draws on the CPU, without motion: as in CI.

use crate::bluez;
use crate::common;
use crate::fakes;
use crate::kdeconnect;
use crate::nm;

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

use anyhow::Context;
use common::{Command, KEY_LEFTMETA, KEY_LEFTSHIFT, Shows, WAIT, chord, wait};
use fakes::Fakes;
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};
use kdeconnect::{Call, Chooser, ENTRY, FakeDevice, KdeConnect};

const KEY_B: u32 = 48;
const KEY_ENTER: u32 = 28;

const PIXEL: FakeDevice = FakeDevice::phone("a1b2c3", "Pixel 8", 82);
const TAB: FakeDevice = FakeDevice::tablet("d4e5f6", "Galaxy Tab S9", 54);

/// The connections' surface with the Wi-Fi's, Bluetooth's and the layout's
/// rows; one more a phone's.
const THREE: (i32, i32) = (194, 104);
const FOUR: (i32, i32) = (194, 136);
const FIVE: (i32, i32) = (194, 168);

struct Bed {
    fakes: Fakes,
    kc: Option<KdeConnect>,
    _nm: nm::Nm,
    _bluez: bluez::Bluez,
    _chooser: Option<zbus::Connection>,
    hp: Headless,
    log: PathBuf,
}

/// How the bed starts.
#[derive(Default)]
struct Setup {
    /// KDE Connect's devices; none: KDE Connect is not running.
    devices: Option<Vec<FakeDevice>>,
    /// KDE Connect activatable on the bus (a service file), its program
    /// writing `started` in the fakes' folder.
    activatable: bool,
    /// A desktop portal whose chooser answers these files.
    chooser: Option<Vec<String>>,
    /// The configuration's lines (`[notifications.apps]`).
    config: String,
    /// One layout, `us`, for the keys typed: no layout's row.
    us: bool,
}

fn ready() -> Option<PathBuf> {
    let shell = common::shell_program();
    if !fakes::available() {
        common::missing("dbus-daemon, pipewire, pw-dump, wpctl and pw-metadata");
    }
    Some(shell)
}

impl Bed {
    fn start(shell: &Path, setup: Setup) -> anyhow::Result<Self> {
        let mut fakes = Fakes::empty()?;
        let root = fakes.dir.path().to_owned();
        if setup.activatable {
            let services = root.join("services");
            std::fs::create_dir_all(&services)?;
            let program = root.join("kdeconnectd");
            std::fs::write(
                &program,
                format!("#!/bin/sh\ntouch '{}'\n", root.join("started").display()),
            )?;
            std::fs::set_permissions(
                &program,
                std::os::unix::fs::PermissionsExt::from_mode(0o755),
            )?;
            std::fs::write(
                services.join("org.kde.kdeconnect.service"),
                format!(
                    "[D-BUS Service]\nName=org.kde.kdeconnect\nExec={}\n",
                    program.display()
                ),
            )?;
        }
        fakes.start_bus()?;
        let wifi = nm::Setup {
            aps: vec![nm::FakeAp::new("Maison", 80, nm::Security::Psk("t"))],
            saved: vec![("Maison", Some("t"))],
            current: Some("Maison"),
            ..nm::Setup::default()
        };
        let nm = nm::Nm::start(fakes.runtime(), &fakes.address, wifi)?;
        let bluez = bluez::Bluez::start(fakes.runtime(), &fakes.address, bluez::Setup::default())?;
        let kc = setup
            .devices
            .map(|devices| KdeConnect::start(fakes.runtime(), &fakes.address, devices))
            .transpose()?;
        let chooser = setup
            .chooser
            .map(|files| Chooser::serve(fakes.runtime(), &fakes.address, files))
            .transpose()?
            .map(|(connection, _)| connection);
        for sub in ["home", "state", "cache", "config"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let config = root.join("config.toml");
        std::fs::write(
            &config,
            format!("[panel]\nform = \"full\"\n{}", setup.config),
        )?;
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin)?;
        let opener = bin.join("xdg-open");
        std::fs::write(
            &opener,
            format!(
                "#!/bin/sh\necho \"$@\" >> '{}'\n",
                root.join("opened").display()
            ),
        )?;
        std::fs::set_permissions(&opener, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
        let log = root.join("shell.log");
        let env = [
            (
                "RUST_LOG",
                "warn,hocuspanda_shell=debug,hocus_phone=debug".to_owned(),
            ),
            ("PATH", format!("{}:/usr/bin:/bin", bin.display())),
            ("HOME", root.join("home").display().to_string()),
            ("XDG_STATE_HOME", root.join("state").display().to_string()),
            ("XDG_CACHE_HOME", root.join("cache").display().to_string()),
            ("XDG_CONFIG_HOME", root.join("config").display().to_string()),
            ("HOCUS_CONFIG", config.display().to_string()),
        ];
        let script = fakes.shell_with(shell, "fr_FR.UTF-8", &env, Some(&log))?;
        let mut settings = Config::default();
        settings.keyboard.layouts = if setup.us {
            vec!["us".to_owned()]
        } else {
            vec!["fr".to_owned(), "us".to_owned()]
        };
        let hp = Headless::start_on_bus(
            HeadlessConfig {
                outputs: vec!["1920x1080".parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            settings,
            Some(script),
            Some(fakes.address.clone()),
        )?;
        let mut bed = Self {
            fakes,
            kc,
            _nm: nm,
            _bluez: bluez,
            _chooser: chooser,
            hp,
            log,
        };
        let drawn = bed.hp.dispatch_until(Duration::from_secs(20), |hp| {
            hp.connections_surfaces().len() == 1 && hp.connections_commits() > 0
        })?;
        anyhow::ensure!(drawn, "no connections: {:?}", bed.hp.connections_surfaces());
        bed.logged("the notifications are served")?;
        Ok(bed)
    }

    fn kc(&self) -> &KdeConnect {
        self.kc.as_ref().expect("KDE Connect")
    }

    fn surface(&self) -> Rect {
        self.hp
            .connections_surfaces()
            .first()
            .map_or(Rect::new(0, 0, 0, 0), |(_, r)| *r)
    }

    /// Waits until the connections' surface is `size`, drawn.
    fn surface_is(&mut self, (width, height): (i32, i32)) -> anyhow::Result<()> {
        let done = self.hp.dispatch_until(WAIT, |hp| {
            hp.connections_surfaces()
                .first()
                .is_some_and(|(_, r)| (r.width, r.height) == (width, height))
                && hp.connections_commits() > 0
        })?;
        anyhow::ensure!(done, "the connections' surface: {:?}", self.surface());
        wait(&mut self.hp, Duration::from_millis(300))
    }

    /// The middle of row `slot` of the full form, from the lowest (0, the
    /// layout's).
    fn row(&self, slot: i32) -> (f64, f64) {
        let s = self.surface();
        (
            f64::from(s.x + s.width / 2),
            f64::from(s.y + s.height) - 5.5 - 15.0 - f64::from(32 * slot),
        )
    }

    fn mark(&self) -> usize {
        common::logged(&self.log).len()
    }

    /// Waits until the shell's log has `text` after `from`: that line.
    fn logged_since(&mut self, from: usize, text: &str) -> anyhow::Result<String> {
        let log = self.log.clone();
        let mut found = None;
        self.hp.dispatch_until(WAIT, |_| {
            found = common::logged(&log)
                .get(from..)
                .and_then(|l| l.lines().rfind(|l| l.contains(text)).map(str::to_owned));
            found.is_some()
        })?;
        found.ok_or_else(|| {
            let said = common::logged(&self.log);
            let tail: Vec<&str> = said
                .lines()
                .filter(|l| l.contains("phone") || l.contains("KDE Connect"))
                .collect();
            anyhow::anyhow!("the shell never logged {text:?}:\n{}", tail.join("\n"))
        })
    }

    fn logged(&mut self, text: &str) -> anyhow::Result<String> {
        self.logged_since(0, text)
    }

    /// The rows as the shell last said them.
    fn rows_said(&self) -> String {
        common::logged(&self.log)
            .lines()
            .rfind(|l| l.contains("the phones' rows"))
            .unwrap_or_default()
            .to_owned()
    }

    /// Waits until the fake has been asked `call`.
    fn asked(&mut self, call: &Call) -> anyhow::Result<()> {
        let kc = self.kc.as_ref().expect("KDE Connect");
        let done = self
            .hp
            .dispatch_until(WAIT, |_| kc.calls().contains(call))?;
        anyhow::ensure!(done, "not asked {call:?}: {:?}", self.kc().calls());
        Ok(())
    }

    /// Opens the popover of the phone's row at `at`: what it shows, as the
    /// kit's dump says it.
    fn open(&mut self, at: (f64, f64)) -> anyhow::Result<serde_json::Value> {
        let mark = self.mark();
        common::click_on(&mut self.hp, at, hocus_link::CONNECTIONS_NAMESPACE)?;
        self.shows_since(mark, "\"phone:Forget\"")
    }

    /// The popover's controls, once its dump after `from` has `text`.
    fn shows_since(&mut self, from: usize, text: &str) -> anyhow::Result<serde_json::Value> {
        let last = |log: &str| {
            log.get(from..)
                .and_then(|l| {
                    l.lines()
                        .rfind(|l| l.contains(" shows {") && l.contains("\"phone:"))
                })
                .map(str::to_owned)
        };
        let log = self.log.clone();
        let done = self.hp.dispatch_until(WAIT, |hp| {
            last(&common::logged(&log)).is_some_and(|l| l.contains(text))
                && common::drawn_popovers(hp).len() == 1
        })?;
        let line = last(&common::logged(&self.log)).unwrap_or_default();
        anyhow::ensure!(done, "the popover never showed {text:?}: {line}");
        let json = line
            .split_once(" shows ")
            .map(|(_, j)| j)
            .unwrap_or_default();
        Ok(serde_json::from_str(json)?)
    }

    fn popover(&self) -> Option<Rect> {
        common::drawn_popovers(&self.hp).first().copied()
    }

    /// A click on the popover's control `id`.
    fn click_id(&mut self, shows: &serde_json::Value, id: &str) -> anyhow::Result<()> {
        let drawn = self
            .hp
            .dispatch_until(WAIT, |hp| !common::drawn_popovers(hp).is_empty())?;
        anyhow::ensure!(drawn, "no popover drawn");
        let pop = self.popover().context("no popover")?;
        let at = shows[id]["at"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("no {id} in {shows}"))?;
        let n = |i: usize| at.get(i).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        let point = (
            f64::from(pop.x) + n(0) + n(2) / 2.0,
            f64::from(pop.y) + n(1) + n(3) / 2.0,
        );
        common::click_on(&mut self.hp, point, hocus_link::POPOVER_NAMESPACE)
    }

    /// Waits until one bubble shows, saying `summary`: its namespace,
    /// where it is, and the kit's dump of it.
    fn bubble(&mut self, summary: &str) -> anyhow::Result<(String, Rect, serde_json::Value)> {
        let log = self.log.clone();
        let mut seen = None;
        self.hp.dispatch_until(WAIT, |hp| {
            let dump = bubble_dump(&log, summary);
            let shown = hp.bubble_surfaces();
            if let (Some(dump), Some((namespace, _, rect))) = (dump, shown.last()) {
                seen = Some((namespace.clone(), *rect, dump));
            }
            seen.is_some()
        })?;
        let seen = seen.ok_or_else(|| {
            let said = common::logged(&self.log);
            let bubbles: Vec<&str> = said
                .lines()
                .filter(|l| {
                    l.contains("bubble") || l.contains("notification") || l.contains("phone")
                })
                .collect();
            anyhow::anyhow!(
                "no bubble saying {summary:?}: {:?}\n{}",
                self.hp.bubble_surfaces(),
                bubbles.join("\n")
            )
        })?;
        common::drawn(&mut self.hp, &seen.0)?;
        Ok(seen)
    }

    /// A click on the bubble's control `id` (`bubble:Action(1)`).
    fn press(
        &mut self,
        bubble: &(String, Rect, serde_json::Value),
        id: &str,
    ) -> anyhow::Result<()> {
        let (namespace, rect, dump) = bubble;
        let at = dump[id]["at"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("no {id} in {dump}"))?;
        let n = |i: usize| at.get(i).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        let point = (
            f64::from(rect.x) + n(0) + n(2) / 2.0,
            f64::from(rect.y) + n(1) + n(3) / 2.0,
        );
        common::click_on(&mut self.hp, point, namespace)
    }

    /// Whether a pixel of the box from `(x, y)`, `w` × `h`, is `like`.
    fn any(
        &mut self,
        (x, y): (i32, i32),
        (w, h): (i32, i32),
        like: fn((u8, u8, u8)) -> bool,
    ) -> anyhow::Result<bool> {
        let (width, _, bytes) = self.hp.screenshot(0)?;
        for y in y..y + h {
            for x in x..x + w {
                let at = usize::try_from(y * width + x)? * 4;
                if let Some([b, g, r, _]) = bytes
                    .get(at..at + 4)
                    .and_then(|p| <[u8; 4]>::try_from(p).ok())
                    && like((r, g, b))
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// The commits of the bar's, the levels', the connections' and the
    /// popovers' surfaces.
    fn counts(&self) -> (u64, u64, u64, u64) {
        (
            self.hp.bar_commits(),
            self.hp.levels_commits(),
            self.hp.connections_commits(),
            self.hp.popover_commits(),
        )
    }

    /// Waits until nothing of the shell's commits for a while.
    fn settle(&mut self) -> anyhow::Result<()> {
        let start = Instant::now();
        loop {
            let counts = self.counts();
            wait(&mut self.hp, Duration::from_millis(600))?;
            if self.counts() == counts || start.elapsed() > WAIT {
                return Ok(());
            }
        }
    }

    fn shot(&mut self, namespace: &str, name: &str) -> anyhow::Result<()> {
        common::layer_shot(&mut self.hp, namespace, name)
    }

    fn opened(&self) -> String {
        std::fs::read_to_string(self.fakes.dir.path().join("opened")).unwrap_or_default()
    }

    /// `hocus msg a11y`'s answer, from a thread while the loop runs.
    fn a11y(&mut self) -> anyhow::Result<String> {
        let path = self
            .hp
            .control_socket()
            .context("no control socket")?
            .to_path_buf();
        let (said, heard) = mpsc::channel();
        std::thread::spawn(move || {
            let asked = (|| -> std::io::Result<String> {
                let mut stream = UnixStream::connect(&path)?;
                stream.set_read_timeout(Some(WAIT))?;
                stream.write_all(b"msg a11y\n")?;
                let mut answer = String::new();
                stream.read_to_string(&mut answer)?;
                Ok(answer)
            })();
            let _ = said.send(asked);
        });
        let mut answer = None;
        self.hp.dispatch_until(WAIT, |_| {
            if answer.is_none() {
                answer = heard.try_recv().ok();
            }
            answer.is_some()
        })?;
        Ok(answer.context("no answer")??)
    }
}

/// The kit's dump of the bubble that shows saying `summary`, once drawn.
fn bubble_dump(log: &Path, summary: &str) -> Option<serde_json::Value> {
    let text = common::logged(log);
    let title = format!("title=\"{summary}\"");
    let lines: Vec<&str> = text.lines().collect();
    let shows = lines
        .iter()
        .rposition(|l| l.contains("a bubble shows") && l.contains(&title))?;
    let line = lines[shows..]
        .iter()
        .rev()
        .find(|l| l.contains(" shows {") && l.contains("bubble:Close"))?;
    let at = line.find(" shows {")? + " shows ".len();
    serde_json::from_str(&line[at..]).ok()
}

fn orange((r, g, b): (u8, u8, u8)) -> bool {
    r > 200 && (100..190).contains(&g) && b < 130
}

/// `#F7B98A`, the low battery's digits, or near it.
fn peach((r, g, b): (u8, u8, u8)) -> bool {
    r > 200 && (150..205).contains(&g) && (100..160).contains(&b)
}

fn with(devices: Vec<FakeDevice>) -> Setup {
    Setup {
        devices: Some(devices),
        ..Setup::default()
    }
}

// --- The rows -------------------------------------------------------------------------

#[test]
fn absent_no_row_and_nothing_said_wrong() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, Setup::default())?;
    bed.logged("KDE Connect: not on the session bus")?;
    bed.surface_is(THREE)?;
    let said = common::logged(&bed.log);
    let wrong: Vec<&str> = said
        .lines()
        .filter(|l| l.contains("WARN") || l.contains("ERROR"))
        .filter(|l| {
            let l = l.to_lowercase();
            l.contains("phone") || l.contains("kde connect") || l.contains("job view")
        })
        .collect();
    assert!(wrong.is_empty(), "{wrong:?}");
    Ok(())
}

#[test]
fn not_started_by_the_shell() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(
        &shell,
        Setup {
            activatable: true,
            ..Setup::default()
        },
    )?;
    bed.logged("KDE Connect: not on the session bus")?;
    wait(&mut bed.hp, Duration::from_secs(2))?;
    assert!(
        !bed.fakes.dir.path().join("started").exists(),
        "the shell started KDE Connect"
    );
    assert_eq!((bed.surface().width, bed.surface().height), THREE);
    Ok(())
}

#[test]
fn a_phone_in_reach_under_bluetooth_with_its_battery() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, with(vec![PIXEL]))?;
    bed.logged("rows=Pixel 8, téléphone, batterie 82 %")?;
    // A row more, between Bluetooth's and the layout's.
    bed.surface_is(FOUR)?;
    let (_, middle) = bed.row(1);
    let s = bed.surface();
    // Its battery on the right, light.
    let lit = bed.any((s.x + 140, middle as i32 - 5), (45, 10), |(r, g, b)| {
        r.min(g).min(b) > 130
    })?;
    assert!(lit, "no battery drawn");
    bed.shot(hocus_link::CONNECTIONS_NAMESPACE, "957-row")?;
    Ok(())
}

#[test]
fn charging_shows_the_bolt_and_draws_the_connections_alone() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, with(vec![PIXEL]))?;
    bed.logged("rows=Pixel 8, téléphone, batterie 82 %")?;
    bed.surface_is(FOUR)?;
    bed.settle()?;
    let (bars, levels) = (bed.hp.bar_commits(), bed.hp.levels_commits());
    let connections = bed.hp.connections_commits();
    bed.kc().set_battery(PIXEL.id, 67, true)?;
    bed.logged("rows=Pixel 8, téléphone, batterie 67 %, en charge")?;
    let drawn = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.connections_commits() > connections)?;
    anyhow::ensure!(drawn, "the connections never drew");
    wait(&mut bed.hp, Duration::from_millis(300))?;
    // The bolt, green, before the digits.
    let (_, middle) = bed.row(1);
    let s = bed.surface();
    let green = bed.any((s.x + 120, middle as i32 - 6), (50, 12), |(r, g, b)| {
        g > 150 && r < 150 && b < 170 && g > r + 40
    })?;
    assert!(green, "no bolt");
    assert_eq!(bed.hp.bar_commits(), bars, "the bar drew");
    assert_eq!(bed.hp.levels_commits(), levels, "the levels drew");
    bed.shot(hocus_link::CONNECTIONS_NAMESPACE, "957-charging")?;
    Ok(())
}

#[test]
fn low_shows_the_dot_and_peach_digits() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let low = FakeDevice::phone("a1b2c3", "Pixel 8", 12);
    let mut bed = Bed::start(&shell, with(vec![low]))?;
    bed.logged("rows=Pixel 8, téléphone, batterie 12 %")?;
    bed.surface_is(FOUR)?;
    let (_, middle) = bed.row(1);
    let s = bed.surface();
    let dot = bed.any((s.x + 22, middle as i32 - 13), (12, 10), orange)?;
    assert!(dot, "no dot");
    let digits = bed.any((s.x + 140, middle as i32 - 6), (45, 12), peach)?;
    assert!(digits, "the digits are not peach");
    Ok(())
}

#[test]
fn out_of_reach_says_so_and_its_popover_offers_to_forget_only() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, with(vec![PIXEL]))?;
    bed.logged("rows=Pixel 8, téléphone, batterie 82 %")?;
    bed.surface_is(FOUR)?;
    bed.kc().set_reachable(PIXEL.id, false)?;
    bed.logged("rows=Pixel 8, téléphone, hors de portée")?;
    // Its look changes, not the surface's size.
    assert_eq!((bed.surface().width, bed.surface().height), FOUR);
    bed.shot(hocus_link::CONNECTIONS_NAMESPACE, "957-out-of-reach")?;
    let row = bed.row(1);
    let shows = bed.open(row)?;
    let said = bed.logged("the phone's popover shows it")?;
    assert!(
        said.contains("Pixel 8: Hors de portée. | Oublier ce téléphone"),
        "{said}"
    );
    for absent in [
        "phone:Ring",
        "phone:SendFile",
        "phone:SendClipboard",
        "phone:Battery",
    ] {
        assert!(shows.get(absent).is_none(), "{absent} in {shows}");
    }
    common::popover_shot(&mut bed.hp, "957-popover-out-of-reach")?;
    Ok(())
}

#[test]
fn nothing_paired_no_row() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let around = FakeDevice::phone("f9e8d7", "Fairphone 5", 50).unpaired();
    let mut bed = Bed::start(&shell, with(vec![around]))?;
    bed.logged("KDE Connect read")?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    bed.surface_is(THREE)?;
    Ok(())
}

#[test]
fn two_devices_the_phone_in_reach_first() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, with(vec![PIXEL, TAB.away()]))?;
    bed.logged("rows=Pixel 8, téléphone, batterie 82 % | Galaxy Tab S9, tablette, hors de portée")?;
    bed.surface_is(FIVE)?;
    bed.shot(hocus_link::CONNECTIONS_NAMESPACE, "957-two")?;
    Ok(())
}

#[test]
fn a_laptop_gets_no_row() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let laptop = FakeDevice::laptop("c0ffee", "ThinkPad");
    let mut bed = Bed::start(&shell, with(vec![laptop]))?;
    bed.logged("KDE Connect read")?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    bed.surface_is(THREE)?;
    assert!(!bed.rows_said().contains("ThinkPad"), "{}", bed.rows_said());
    Ok(())
}

#[test]
fn the_daemon_goes_the_rows_go_back_they_come() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, with(vec![PIXEL]))?;
    bed.surface_is(FOUR)?;
    let mark = bed.mark();
    if let Some(kc) = bed.kc.as_mut() {
        kc.stop();
    }
    bed.logged_since(mark, "KDE Connect: not on the session bus")?;
    bed.surface_is(THREE)?;
    let mark = bed.mark();
    if let Some(kc) = bed.kc.as_mut() {
        kc.restart()?;
    }
    bed.logged_since(mark, "rows=Pixel 8, téléphone, batterie 82 %")?;
    bed.surface_is(FOUR)?;
    Ok(())
}

#[test]
fn the_mini_form_has_its_button_and_hovering_it_opens_its_popover() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, with(vec![PIXEL]))?;
    bed.surface_is(FOUR)?;
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_B]);
    // The Wi-Fi's, Bluetooth's and the phone's buttons (the layout's is
    // the levels').
    bed.surface_is((54, 128))?;
    let s = bed.surface();
    let button = (
        f64::from(s.x + s.width / 2),
        f64::from(s.y + s.height) - 5.5 - 19.0,
    );
    let mark = bed.mark();
    common::over(&mut bed.hp, button, hocus_link::CONNECTIONS_NAMESPACE)?;
    bed.shows_since(mark, "\"phone:Ring\"")?;
    Ok(())
}

#[test]
fn a_screen_reader_reads_its_name_kind_and_battery() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, with(vec![PIXEL]))?;
    bed.surface_is(FOUR)?;
    let mut said = String::new();
    let start = Instant::now();
    while start.elapsed() < WAIT {
        said = bed.a11y()?;
        if said.contains("Pixel 8, téléphone, batterie 82 %") {
            return Ok(());
        }
        wait(&mut bed.hp, Duration::from_millis(200))?;
    }
    anyhow::bail!("screen readers read:\n{said}")
}

#[test]
fn at_rest_nothing_draws() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, with(vec![PIXEL]))?;
    bed.surface_is(FOUR)?;
    bed.settle()?;
    let before = bed.counts();
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert_eq!(bed.counts(), before, "a surface committed at rest");
    Ok(())
}

// --- The popover -----------------------------------------------------------------------

#[test]
fn ring_rings_once_and_the_popover_stays() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, with(vec![PIXEL]))?;
    bed.surface_is(FOUR)?;
    let row = bed.row(1);
    let shows = bed.open(row)?;
    common::popover_shot(&mut bed.hp, "957-popover")?;
    bed.click_id(&shows, "phone:Ring")?;
    bed.asked(&Call::Ring(PIXEL.id.to_owned()))?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    let rings = bed
        .kc()
        .calls()
        .iter()
        .filter(|c| matches!(c, Call::Ring(_)))
        .count();
    assert_eq!(rings, 1);
    assert!(bed.popover().is_some(), "the popover closed");
    Ok(())
}

#[test]
fn send_a_file_through_the_portals_chooser() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let file = std::env::temp_dir().join("hp-957").join("rapport.pdf");
    let url = format!("file://{}", file.display());
    let mut bed = Bed::start(
        &shell,
        Setup {
            devices: Some(vec![PIXEL]),
            chooser: Some(vec![url.clone()]),
            ..Setup::default()
        },
    )?;
    bed.surface_is(FOUR)?;
    let row = bed.row(1);
    let shows = bed.open(row)?;
    bed.click_id(&shows, "phone:SendFile")?;
    bed.asked(&Call::ShareUrls(PIXEL.id.to_owned(), vec![url]))?;
    Ok(())
}

#[test]
fn a_transfers_progress_on_the_row_and_the_popover() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, with(vec![PIXEL]))?;
    bed.surface_is(FOUR)?;
    bed.logged("the job view is served")?;
    let view = bed
        .kc()
        .transfer(ENTRY, "Envoi vers Pixel 8", "rapport.pdf", 10_000_000)?;
    bed.kc().progress(&view, 4_200_000)?;
    bed.logged("[rapport.pdf 42 %]")?;
    bed.shot(hocus_link::CONNECTIONS_NAMESPACE, "957-sending")?;
    let row = bed.row(1);
    let shows = bed.open(row)?;
    bed.logged("rapport.pdf 42 %")?;
    common::popover_shot(&mut bed.hp, "957-popover-sending")?;
    // Its ✕: KDE Connect is asked to stop it.
    bed.click_id(&shows, "phone:Stop")?;
    bed.asked(&Call::Cancel(view.clone()))?;
    // Done: it goes.
    bed.kc().terminate(&view, 0)?;
    let log = bed.log.clone();
    let gone = bed.hp.dispatch_until(WAIT, |_| {
        common::logged(&log)
            .lines()
            .rfind(|l| l.contains("the phones' rows"))
            .is_some_and(|l| !l.contains("rapport.pdf"))
    })?;
    anyhow::ensure!(gone, "the transfer stayed: {}", bed.rows_said());
    // Another application's view draws nothing.
    let other = bed
        .kc()
        .transfer("org.kde.dolphin", "Copie vers Pixel 8", "photo.jpg", 1000)?;
    bed.kc().progress(&other, 500)?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    let rows = bed.rows_said();
    assert!(!rows.contains("photo.jpg"), "{rows}");
    Ok(())
}

#[test]
fn the_clipboard_to_the_phone_never_a_secret() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, with(vec![PIXEL]))?;
    bed.surface_is(FOUR)?;
    let (rect, editor) = common::open_showing(
        &mut bed.hp,
        "test.editor",
        Shows::Typing {
            app_id: "test.editor",
            size: (640, 420),
        },
    )?;
    common::click(
        &mut bed.hp,
        (f64::from(rect.x + 20), f64::from(rect.y + 20)),
    )?;
    let holds = bed.hp.dispatch_until(WAIT, |hp| {
        hp.windows()
            .iter()
            .any(|w| w.app_id.as_deref() == Some("test.editor") && w.keyboard)
    })?;
    anyhow::ensure!(holds, "the editor never held the keyboard");
    let mark = bed.mark();
    editor.commands.send(Command::Copy(common::Clip {
        types: &["text/plain;charset=utf-8", "text/plain"],
        text: "Rendez-vous à 15 h",
        hang: false,
    }))?;
    bed.logged_since(mark, "a copy for the history")?;
    // A secret after it: never handed to the shell.
    editor.commands.send(Command::Copy(common::Clip {
        types: &["text/plain;charset=utf-8", "x-kde-passwordManagerHint"],
        text: "hunter2-mot-de-passe",
        hang: false,
    }))?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    let row = bed.row(1);
    let shows = bed.open(row)?;
    bed.click_id(&shows, "phone:SendClipboard")?;
    bed.asked(&Call::ShareText(
        PIXEL.id.to_owned(),
        "Rendez-vous à 15 h".to_owned(),
    ))?;
    bed.logged("Envoyé")?;
    Ok(())
}

#[test]
fn forget_unpairs_and_the_row_goes() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, with(vec![PIXEL]))?;
    bed.surface_is(FOUR)?;
    let row = bed.row(1);
    let shows = bed.open(row)?;
    let mark = bed.mark();
    bed.click_id(&shows, "phone:Forget")?;
    let shows = bed.shows_since(mark, "\"phone:ForgetYes\"")?;
    bed.logged("Oublier Pixel 8 ?")?;
    common::popover_shot(&mut bed.hp, "957-forget")?;
    bed.click_id(&shows, "phone:ForgetYes")?;
    bed.asked(&Call::Unpair(PIXEL.id.to_owned()))?;
    bed.surface_is(THREE)?;
    let closed = bed
        .hp
        .dispatch_until(WAIT, |hp| common::drawn_popovers(hp).is_empty())?;
    anyhow::ensure!(closed, "the popover stayed");
    Ok(())
}

// --- The phone's notifications ---------------------------------------------------------

#[test]
fn a_phones_notification_is_its_applications() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, with(vec![PIXEL]))?;
    bed.surface_is(FOUR)?;
    bed.kc().post(
        PIXEL.id,
        "Signal",
        "Camille",
        "On se voit à 15 h ?",
        true,
        true,
    )?;
    let bubble = bed.bubble("Camille")?;
    let said = bed.logged("a notification")?;
    assert!(said.contains("Signal · Pixel 8"), "{said}");
    bed.shot(&bubble.0, "957-bubble")?;
    // The bell lists it by that application.
    let (_, at) = bed.hp.bell_surfaces()[0];
    let bell = (f64::from(at.x + 11 + 44 + 20), f64::from(at.y + 6 + 17));
    let mark = bed.mark();
    common::click(&mut bed.hp, bell)?;
    let shows = bed.logged_since(mark, "the history shows")?;
    assert!(shows.contains("Signal · Pixel 8"), "{shows}");
    Ok(())
}

#[test]
fn its_level_applies_by_its_id() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(
        &shell,
        Setup {
            devices: Some(vec![PIXEL]),
            config: "[notifications.apps]\n\"kdeconnect:Pixel 8:Signal\" = \"history\"\n"
                .to_owned(),
            ..Setup::default()
        },
    )?;
    bed.surface_is(FOUR)?;
    bed.kc().post(
        PIXEL.id,
        "Signal",
        "Camille",
        "On se voit à 15 h ?",
        true,
        false,
    )?;
    bed.logged("the bell unread=1")?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert!(bed.hp.bubble_surfaces().is_empty(), "a bubble showed");
    Ok(())
}

#[test]
fn its_cross_dismisses_it_on_the_phone_when_it_lets_go() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, with(vec![PIXEL]))?;
    bed.surface_is(FOUR)?;
    let (public, _) = bed.kc().post(
        PIXEL.id,
        "Signal",
        "Camille",
        "On se voit à 15 h ?",
        true,
        false,
    )?;
    let bubble = bed.bubble("Camille")?;
    bed.press(&bubble, "bubble:Close")?;
    bed.asked(&Call::Dismiss(PIXEL.id.to_owned(), public))?;
    // One the phone keeps: nothing asked of it.
    let (kept, _) = bed.kc().post(
        PIXEL.id,
        "Téléphone",
        "Appel en cours",
        "Maman",
        false,
        false,
    )?;
    let bubble = bed.bubble("Appel en cours")?;
    bed.press(&bubble, "bubble:Close")?;
    wait(&mut bed.hp, Duration::from_secs(1))?;
    let calls = bed.kc().calls();
    assert!(
        !calls.contains(&Call::Dismiss(PIXEL.id.to_owned(), kept)),
        "{calls:?}"
    );
    Ok(())
}

#[test]
fn removed_on_the_phone_it_leaves_the_bell() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, with(vec![PIXEL]))?;
    bed.surface_is(FOUR)?;
    let (public, _) = bed.kc().post(
        PIXEL.id,
        "Signal",
        "Camille",
        "On se voit à 15 h ?",
        true,
        false,
    )?;
    bed.bubble("Camille")?;
    bed.logged("the bell unread=1")?;
    let mark = bed.mark();
    bed.kc().remove(PIXEL.id, &public)?;
    bed.logged_since(mark, "the bell unread=0")?;
    let gone = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.bubble_surfaces().is_empty())?;
    anyhow::ensure!(gone, "the bubble stayed");
    Ok(())
}

#[test]
fn a_file_from_the_phone_makes_one_bubble() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, with(vec![PIXEL]))?;
    bed.surface_is(FOUR)?;
    let downloads = bed.fakes.dir.path().join("home/Téléchargements");
    std::fs::create_dir_all(&downloads)?;
    let photo = downloads.join("IMG_2041.jpg");
    std::fs::write(&photo, b"jpeg")?;
    bed.kc()
        .share_received(PIXEL.id, &format!("file://{}", photo.display()))?;
    let bubble = bed.bubble("IMG_2041.jpg")?;
    bed.shot(&bubble.0, "957-received")?;
    bed.press(&bubble, "bubble:Action(open)")?;
    let opened = {
        let dir = bed.fakes.dir.path().to_owned();
        bed.hp.dispatch_until(WAIT, |_| {
            std::fs::read_to_string(dir.join("opened")).is_ok_and(|o| o.contains("IMG_2041.jpg"))
        })?
    };
    assert!(opened, "xdg-open was given {:?}", bed.opened());
    // Three in a row: one bubble.
    for name in ["IMG_2042.jpg", "IMG_2043.jpg", "IMG_2044.jpg"] {
        let file = downloads.join(name);
        std::fs::write(&file, b"jpeg")?;
        bed.kc()
            .share_received(PIXEL.id, &format!("file://{}", file.display()))?;
    }
    bed.logged("a bubble updated in place")?;
    bed.logged("title=\"3 fichiers\"")?;
    // The first one gone, one bubble.
    let one = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.bubble_surfaces().len() == 1)?;
    anyhow::ensure!(one, "bubbles: {:?}", bed.hp.bubble_surfaces());
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(bed.hp.bubble_surfaces().len(), 1, "more than one bubble");
    Ok(())
}

#[test]
fn paired_from_the_phone_its_row_comes() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell, with(Vec::new()))?;
    bed.logged("KDE Connect read")?;
    bed.surface_is(THREE)?;
    let asked = bed.kc().ask_to_pair(PIXEL.unpaired())?;
    let bubble = bed.bubble("Demande d’association de Pixel 8")?;
    bed.shot(&bubble.0, "957-pairing")?;
    bed.press(&bubble, "bubble:Action(1)")?;
    bed.asked(&Call::Action(asked, "1".to_owned()))?;
    assert!(bed.kc().paired(PIXEL.id));
    bed.logged("rows=Pixel 8, téléphone, batterie 82 %")?;
    bed.surface_is(FOUR)?;
    Ok(())
}

/// The keys of `text` on a US layout, each with Shift or not.
fn keys_of(text: &str) -> Vec<(u32, bool)> {
    text.chars()
        .filter_map(|c| {
            Some(match c {
                'O' => (24, true),
                'o' => (24, false),
                'u' => (22, false),
                'i' => (23, false),
                ',' => (51, false),
                ' ' => (57, false),
                '1' => (2, false),
                '5' => (6, false),
                'h' => (35, false),
                _ => return None,
            })
        })
        .collect()
}

#[test]
fn a_reply_typed_in_the_bubble_goes_to_the_phone() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(
        &shell,
        Setup {
            devices: Some(vec![PIXEL]),
            us: true,
            ..Setup::default()
        },
    )?;
    bed.logged("rows=Pixel 8")?;
    let (_, server) = bed.kc().post(
        PIXEL.id,
        "Signal",
        "Camille",
        "On se voit à 15 h ?",
        true,
        true,
    )?;
    let bubble = bed.bubble("Camille")?;
    anyhow::ensure!(
        bubble.2["bubble:Reply"].is_object(),
        "no reply field: {}",
        bubble.2
    );
    // Its `inline-reply` is the field, not a button.
    assert!(bubble.2.get("bubble:Action(inline-reply)").is_none());
    bed.shot(&bubble.0, "957-reply")?;
    bed.press(&bubble, "bubble:Reply")?;
    let held = bed.hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() == Some(bubble.0.as_str())
    })?;
    anyhow::ensure!(held, "the bubble never took the keyboard");
    for (key, shift) in keys_of("Oui, 15 h") {
        if shift {
            bed.hp.key(KEY_LEFTSHIFT, true);
        }
        bed.hp.key(key, true);
        bed.hp.key(key, false);
        if shift {
            bed.hp.key(KEY_LEFTSHIFT, false);
        }
    }
    wait(&mut bed.hp, Duration::from_millis(200))?;
    bed.hp.key(KEY_ENTER, true);
    bed.hp.key(KEY_ENTER, false);
    bed.asked(&Call::Replied(server, "Oui, 15 h".to_owned()))?;
    let closed = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.bubble_surfaces().is_empty())?;
    anyhow::ensure!(closed, "the bubble stayed");
    Ok(())
}

/// The history's controls in the last dump that has `text`, after `from`:
/// id → its rect on the popover.
fn history_dump(log: &Path, from: usize, text: &str) -> Option<serde_json::Value> {
    let said = common::logged(log);
    let line = said
        .get(from..)?
        .lines()
        .rfind(|l| l.contains(" shows {") && l.contains("\"history:") && l.contains(text))?;
    let json = line.split_once(" shows ").map(|(_, j)| j)?;
    serde_json::from_str(json).ok()
}

#[test]
fn a_reply_from_the_bells_line_goes_to_the_phone() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(
        &shell,
        Setup {
            devices: Some(vec![PIXEL]),
            us: true,
            ..Setup::default()
        },
    )?;
    bed.logged("rows=Pixel 8")?;
    let (_, server) = bed.kc().post(
        PIXEL.id,
        "Signal",
        "Camille",
        "On se voit à 15 h ?",
        true,
        true,
    )?;
    bed.bubble("Camille")?;
    let (_, at) = bed.hp.bell_surfaces()[0];
    let bell = (f64::from(at.x + 11 + 44 + 20), f64::from(at.y + 6 + 17));
    let mark = bed.mark();
    common::click(&mut bed.hp, bell)?;
    bed.logged_since(mark, "the history shows")?;
    // Its "Répondre": the line's chip.
    let log = bed.log.clone();
    let mut dump = None;
    bed.hp.dispatch_until(WAIT, |hp| {
        dump = history_dump(&log, mark, "\"history:Action(");
        dump.is_some() && common::drawn_popovers(hp).len() == 1
    })?;
    let dump = dump.context("no line's action in the history")?;
    let reply = dump
        .as_object()
        .and_then(|o| o.keys().find(|k| k.starts_with("history:Action(")).cloned())
        .context("no Répondre")?;
    let mark = bed.mark();
    bed.click_id(&dump, &reply)?;
    let mut unfolded = None;
    bed.hp.dispatch_until(WAIT, |_| {
        unfolded = history_dump(&log, mark, "\"history:ReplyField(");
        unfolded.is_some()
    })?;
    anyhow::ensure!(unfolded.is_some(), "the field never unfolded");
    common::popover_shot(&mut bed.hp, "957-bell-reply")?;
    for (key, shift) in keys_of("Oui, 15 h") {
        if shift {
            bed.hp.key(KEY_LEFTSHIFT, true);
        }
        bed.hp.key(key, true);
        bed.hp.key(key, false);
        if shift {
            bed.hp.key(KEY_LEFTSHIFT, false);
        }
    }
    wait(&mut bed.hp, Duration::from_millis(200))?;
    bed.hp.key(KEY_ENTER, true);
    bed.hp.key(KEY_ENTER, false);
    bed.asked(&Call::Replied(server, "Oui, 15 h".to_owned()))?;
    Ok(())
}
