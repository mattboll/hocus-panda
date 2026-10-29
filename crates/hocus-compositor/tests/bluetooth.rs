//! Bluetooth's row and popover (docs/design/bar.md §4.3, §5, §9; issue
//! #363): the real shell, against a fake BlueZ (`levels/bluez.rs`) and a
//! fake NetworkManager on the fakes' private bus, never the machine's.
//!
//! The row under the Wi-Fi's, in both forms: the last device connected,
//! its battery, "+N", the low battery's dot, connecting, nothing, off; no
//! adapter, no row, and the surface lets the pointer through. The popover
//! opened by a click and, in the mini form, by hovering; the search started
//! as it opens and stopped as it closes; the connected blocks, the paired
//! devices, the devices around (the strongest first, unnamed ones left out,
//! scrolling past four); a device connected, one that does not answer (the
//! message, and one given up), Disconnect, the switch; a device paired
//! without a code reaching the connected block, one asking a passkey
//! left "in Settings", a pairing that fails; the keyboard; nothing drawn at
//! rest, and a change drawing the connections' surface alone.
//!
//! Pixman here, so the shell draws on the CPU, without motion: as in CI.

#[path = "levels/bluez.rs"]
mod bluez;
mod common;
#[path = "levels/fakes.rs"]
mod fakes;
#[path = "levels/nm.rs"]
mod nm;

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use bluez::{Answer, Bluez, Call, FakeDevice, Pairing, Setup};
use common::{BTN_LEFT, KEY_DOWN, KEY_ESC, KEY_LEFTMETA, KEY_TAB, WAIT, chord, wait};
use fakes::Fakes;
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};

const KEY_B: u32 = 48;
const KEY_ENTER: u32 = 28;

const SONY: &str = "WH-1000XM4";
const MX: &str = "MX Master 3S";
const KEYCHRON: &str = "Keychron K3";
const JBL: &str = "JBL Flip 5";
const BUDS: &str = "Galaxy Buds2 Pro";
const PIXEL: &str = "Pixel 8 d'Alex";
const BOSE: &str = "Bose SoundLink Mini";

/// The mockup's devices: the headphones connected, three paired.
fn mine() -> Vec<FakeDevice> {
    vec![
        FakeDevice::paired(SONY, "audio-headphones", Some(82)).connected(),
        FakeDevice::paired(MX, "input-mouse", Some(12)),
        FakeDevice::paired(KEYCHRON, "input-keyboard", Some(64)),
        FakeDevice::paired(JBL, "audio-card", Some(47)),
    ]
}

/// The mockup's devices around, and one that never said its name.
fn around() -> Vec<FakeDevice> {
    vec![
        FakeDevice::around(BOSE, "audio-card", -70),
        FakeDevice::around(BUDS, "audio-headphones", -55),
        FakeDevice::around(PIXEL, "phone", -60).pairing(Pairing::Passkey),
        FakeDevice {
            name: None,
            ..FakeDevice::around("unnamed", "", -40)
        },
    ]
}

/// The popover's parts, from the mockup at ×1: its padding and header, a
/// connected device's block, a section's divider and label, a note.
const HEADER_END: f64 = 1.0 + 12.0 + 32.0;
const GAP: f64 = 6.0;
const BLOCK: f64 = 48.0;
/// With a battery, its state wraps beside Disconnect (in French): two
/// lines.
const BATTERY: f64 = 14.0 * 1.366 + 1.0 + 2.0 * 12.5 * 1.366;
const LABEL_ROW: f64 = 4.0 + 12.0 * 1.366;
const SECTION: f64 = GAP + 9.0 + GAP + LABEL_ROW;
const NOTE: f64 = 4.0 + 13.5 * 1.366 + 2.0;
const ROW: f64 = 36.0;
const ROW_STEP: f64 = 38.0;
const LIST: f64 = 152.0;
/// A failure under its row, on two lines in French.
const FAILURE: f64 = 2.0 * 12.5 * 1.4 + 6.0;

/// Where the popover's parts are, for the connected blocks (their
/// heights) and the paired rows (each with what is under it).
#[derive(Debug, Clone)]
struct Shape {
    blocks: Vec<f64>,
    mine: Vec<f64>,
}

impl Shape {
    fn new(blocks: &[f64], mine: usize) -> Self {
        Self {
            blocks: blocks.to_vec(),
            mine: vec![0.0; mine],
        }
    }

    fn mine_top(&self) -> f64 {
        HEADER_END + self.blocks.iter().map(|b| GAP + b).sum::<f64>() + SECTION + GAP
    }

    fn mine_height(&self) -> f64 {
        if self.mine.is_empty() {
            NOTE
        } else {
            self.mine.iter().map(|e| ROW + e).sum::<f64>() + 2.0 * (self.mine.len() as f64 - 1.0)
        }
    }

    fn nearby_top(&self) -> f64 {
        self.mine_top() + self.mine_height() + SECTION + GAP
    }

    /// The popover's height with `nearby` rows around (`extra` under one).
    fn height(&self, nearby: usize, extra: f64) -> i32 {
        let list = if nearby == 0 {
            NOTE
        } else {
            (nearby as f64 * ROW_STEP - 2.0 + extra).min(LIST)
        };
        (self.nearby_top() + list + 14.0 + 1.0).round() as i32
    }

    /// Paired row `i`'s middle in the popover at `rect`.
    fn mine_row(&self, rect: Rect, i: usize) -> (f64, f64) {
        let above: f64 = self.mine[..i].iter().map(|e| ROW + e + 2.0).sum();
        (
            f64::from(rect.x) + 120.0,
            f64::from(rect.y) + self.mine_top() + above + ROW / 2.0,
        )
    }

    /// Nearby row `i`'s middle, the list not scrolled.
    fn nearby_row(&self, rect: Rect, i: usize) -> (f64, f64) {
        (
            f64::from(rect.x) + 120.0,
            f64::from(rect.y) + self.nearby_top() + ROW_STEP * i as f64 + ROW / 2.0,
        )
    }

    /// Connected block `i`'s Disconnect.
    fn disconnect(&self, rect: Rect, i: usize) -> (f64, f64) {
        let above: f64 = self.blocks[..i].iter().map(|b| GAP + b).sum();
        (
            f64::from(rect.x + 280 - 15 - 20),
            f64::from(rect.y) + HEADER_END + above + GAP + self.blocks[i] / 2.0,
        )
    }
}

/// Off: the header and its line.
fn off_height() -> i32 {
    (HEADER_END + GAP + NOTE + 15.0).round() as i32
}

/// The full bar's popover, `height` tall, on a 1920 × 1080 screen.
fn popover(height: i32) -> Rect {
    Rect::new(1920 - 8 - 196 - 8 - 280, 1080 - 8 - height, 280, height)
}

fn mini_popover(height: i32) -> Rect {
    Rect::new(1920 - 8 - 56 - 8 - 280, 1080 - 8 - height, 280, height)
}

/// What the tests run against.
struct Bed {
    fakes: Fakes,
    bluez: Option<Bluez>,
    _nm: Option<nm::Nm>,
    hp: Headless,
    log: PathBuf,
}

fn ready() -> Option<PathBuf> {
    let Some(shell) = common::shell_program() else {
        eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
        return None;
    };
    if !fakes::available() {
        eprintln!("dbus-daemon, pipewire, pw-dump, wpctl or pw-metadata missing: skipped");
        return None;
    }
    Some(shell)
}

/// The compositor, the shell against the fakes: NetworkManager on a
/// network (`wifi`), BlueZ as `setup` says (none: not running).
fn start(shell: &Path, setup: Option<Setup>, wifi: bool) -> anyhow::Result<Bed> {
    let fakes = Fakes::start()?;
    let nm = if wifi {
        let livebox = "Livebox-A3F2";
        let setup = nm::Setup {
            aps: vec![nm::FakeAp::new(livebox, 80, nm::Security::Psk("t"))],
            saved: vec![(livebox, Some("t"))],
            current: Some(livebox),
            ..nm::Setup::default()
        };
        Some(nm::Nm::start(fakes.runtime(), &fakes.address, setup)?)
    } else {
        None
    };
    let bluez = setup
        .map(|setup| Bluez::start(fakes.runtime(), &fakes.address, setup))
        .transpose()?;
    let log = fakes.dir.path().join("shell.log");
    let env = [
        ("RUST_LOG", "warn,hocuspanda_shell=debug".to_owned()),
        ("HOCUS_BLUETOOTH_TIMEOUT", "1".to_owned()),
    ];
    let script = fakes.shell_with(shell, "fr_FR.UTF-8", &env, Some(&log))?;
    let hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        Config::default(),
        Some(script),
    )?;
    let mut bed = Bed {
        fakes,
        bluez,
        _nm: nm,
        hp,
        log,
    };
    let drawn = bed.hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.connections_surfaces().len() == 1
            && hp.connections_commits() > 0
            && hp.levels_commits() > 0
    })?;
    anyhow::ensure!(drawn, "no connections: {:?}", bed.hp.connections_surfaces());
    bed.settle()?;
    Ok(bed)
}

impl Bed {
    fn bluez(&self) -> &Bluez {
        self.bluez.as_ref().expect("BlueZ")
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

    /// The connections' surface.
    fn surface(&self) -> Rect {
        self.hp
            .connections_surfaces()
            .first()
            .map_or(Rect::new(0, 0, 0, 0), |(_, r)| *r)
    }

    /// Waits until the connections' surface is `width` × `height`.
    fn surface_is(&mut self, width: i32, height: i32) -> anyhow::Result<()> {
        let done = self.hp.dispatch_until(WAIT, |hp| {
            hp.connections_surfaces()
                .first()
                .is_some_and(|(_, r)| (r.width, r.height) == (width, height))
        })?;
        anyhow::ensure!(done, "the connections' surface: {:?}", self.surface());
        Ok(())
    }

    /// Bluetooth's row (full: 30 px) or button (mini: 38 px), the lowest:
    /// its middle.
    fn item(&self) -> (f64, f64) {
        let s = self.surface();
        let half = if s.width == 54 { 19.0 } else { 15.0 };
        (
            f64::from(s.x + s.width / 2),
            f64::from(s.y + s.height) - 5.5 - half,
        )
    }

    fn click(&mut self, (x, y): (f64, f64)) {
        self.hp.pointer_move(x - 1.0, y);
        self.hp.pointer_move(x, y);
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
    }

    fn press(&mut self, key: u32) {
        self.hp.key(key, true);
        self.hp.key(key, false);
    }

    fn popovers(&self) -> Vec<Rect> {
        self.hp
            .popover_surfaces()
            .into_iter()
            .map(|(_, r)| r)
            .collect()
    }

    /// Waits until the popover is at `rect`, or gone.
    fn popover_is(&mut self, rect: Option<Rect>) -> anyhow::Result<()> {
        let want: Vec<Rect> = rect.into_iter().collect();
        let done = self.hp.dispatch_until(WAIT, |hp| {
            hp.popover_surfaces()
                .into_iter()
                .map(|(_, r)| r)
                .collect::<Vec<_>>()
                == want
        })?;
        anyhow::ensure!(done, "popovers {:?}, not {want:?}", self.popovers());
        Ok(())
    }

    /// Waits until the fake has been asked `call`.
    fn asked(&mut self, call: &Call) -> anyhow::Result<()> {
        let bluez = self.bluez.as_ref().expect("BlueZ");
        let done = self
            .hp
            .dispatch_until(WAIT, |_| bluez.calls().contains(call))?;
        anyhow::ensure!(done, "not asked {call:?}: {:?}", self.bluez().calls());
        Ok(())
    }

    /// How many times the fake was asked `call`.
    fn times(&self, call: &Call) -> usize {
        self.bluez().calls().iter().filter(|c| *c == call).count()
    }

    /// Waits until the shell's log has a line with `text`.
    fn logged(&mut self, text: &str) -> anyhow::Result<()> {
        let log = self.log.clone();
        let done = self
            .hp
            .dispatch_until(WAIT, |_| plain(&log).contains(text))?;
        anyhow::ensure!(done, "the shell never logged {text:?}");
        Ok(())
    }

    /// The pixel at `at` (global), as red, green, blue.
    fn rgb(&mut self, (x, y): (i32, i32)) -> anyhow::Result<(u8, u8, u8)> {
        let (width, _, bytes) = self.hp.screenshot(0)?;
        let at = usize::try_from(y * width + x)? * 4;
        let [b, g, r, _] = bytes
            .get(at..at + 4)
            .and_then(|p| <[u8; 4]>::try_from(p).ok())
            .ok_or_else(|| anyhow::anyhow!("no pixel at {x}, {y}"))?;
        Ok((r, g, b))
    }

    /// Waits until the pixel at `at` is `like`.
    fn until(&mut self, at: (i32, i32), like: fn((u8, u8, u8)) -> bool) -> anyhow::Result<()> {
        let mut seen = (0, 0, 0);
        let start = Instant::now();
        while start.elapsed() < WAIT {
            seen = self.rgb(at)?;
            if like(seen) {
                return Ok(());
            }
            wait(&mut self.hp, Duration::from_millis(50))?;
        }
        anyhow::bail!("{at:?} shows {seen:?}")
    }

    /// The light pixels in the box from `(x, y)`, `w` × `h` (global).
    fn ink(&mut self, (x, y): (i32, i32), (w, h): (i32, i32)) -> anyhow::Result<usize> {
        let mut lit = 0;
        for y in y..y + h {
            for x in x..x + w {
                let (r, g, b) = self.rgb((x, y))?;
                if r.min(g).min(b) > 150 {
                    lit += 1;
                }
            }
        }
        Ok(lit)
    }

    /// Opens the popover with a click on the row, at `rect`.
    fn open(&mut self, rect: Rect) -> anyhow::Result<()> {
        let item = self.item();
        self.click(item);
        self.popover_is(Some(rect))?;
        // Drawn, so that a click finds its rows.
        wait(&mut self.hp, Duration::from_millis(300))
    }

    fn mini(&mut self) -> anyhow::Result<()> {
        chord(&mut self.hp, &[KEY_LEFTMETA, KEY_B]);
        let mini = self.hp.dispatch_until(WAIT, |hp| {
            hp.connections_surfaces()
                .first()
                .is_some_and(|(_, r)| r.width == 54)
        })?;
        anyhow::ensure!(mini, "{:?}", self.hp.connections_surfaces());
        self.settle()
    }
}

/// What the shell logged, without its colours.
fn plain(log: &Path) -> String {
    let text = String::from_utf8_lossy(&std::fs::read(log).unwrap_or_default()).into_owned();
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

fn orange((r, g, b): (u8, u8, u8)) -> bool {
    r > 200 && (100..190).contains(&g) && b < 130
}

fn setup(devices: Vec<FakeDevice>, later: Vec<FakeDevice>) -> Setup {
    Setup {
        devices,
        later,
        ..Setup::default()
    }
}

#[test]
fn the_row_shows_the_last_connected_its_battery_the_others_and_a_low_one_in_both_forms()
-> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Some(setup(mine(), Vec::new())), true)?;
    bed.logged("row=Connected { name: \"WH-1000XM4\", battery: Some(82), more: 0, low: false }")?;
    // Under the Wi-Fi's: two rows, the lowest 14 px above the levels.
    bed.surface_is(194, 72)?;
    let s = bed.surface();
    let levels = bed.hp.levels_surfaces()[0].1;
    assert_eq!(s.x, levels.x);
    assert!(s.y + s.height > levels.y, "{s:?} over {levels:?}");
    // Its battery on the right: light, at the row's end.
    let (_, middle) = bed.item();
    let battery = bed.ink((s.x + 150, middle as i32 - 5), (38, 10))?;
    assert!(battery > 10, "no battery drawn: {battery}");
    // The mouse connects too, low: the last one first, "+1", the dot.
    let (bars, levels_drawn) = (bed.hp.bar_commits(), bed.hp.levels_commits());
    bed.bluez().set_connected(MX, true)?;
    bed.logged("row=Connected { name: \"MX Master 3S\", battery: Some(12), more: 1, low: true }")?;
    let dot = (s.x + 28, middle as i32 - 8);
    bed.until(dot, orange)?;
    // Charged, the dot goes: the headphones alone keep no dot.
    bed.bluez().set_battery(MX, 40)?;
    bed.logged("more: 1, low: false")?;
    bed.until(dot, |c| !orange(c))?;
    assert_eq!(bed.hp.levels_commits(), levels_drawn, "the levels drew");
    assert_eq!(bed.hp.bar_commits(), bars, "the bar drew");
    bed.bluez().set_battery(MX, 12)?;
    bed.until(dot, orange)?;
    // The mini form: two 44 × 38 buttons, Bluetooth's the lowest, its dot
    // 10 px from its right and 7 from its top.
    bed.mini()?;
    bed.surface_is(54, 88)?;
    let s = bed.surface();
    let (_, middle) = bed.item();
    bed.until((s.x + 35, middle as i32 - 19 + 10), orange)?;
    // Nothing connected, then off.
    bed.bluez().set_connected(MX, false)?;
    bed.bluez().set_connected(SONY, false)?;
    bed.logged("row=Nothing")?;
    bed.bluez().set_powered(false)?;
    bed.logged("row=Off")?;
    Ok(())
}

#[test]
fn without_an_adapter_no_row_and_the_pointer_goes_through() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // BlueZ without an adapter, nor NetworkManager: no row at all, and
    // the bar's surfaces under it take the pointer.
    let empty = Setup {
        adapter: false,
        ..Setup::default()
    };
    let mut bed = start(&shell, Some(empty), false)?;
    bed.logged("Bluetooth: not there (no adapter)")?;
    let s = bed.surface();
    let (x, y) = (f64::from(s.x + 97), f64::from(s.y + 20));
    bed.hp.pointer_move(x - 1.0, y);
    bed.hp.pointer_move(x, y);
    let through = bed.hp.dispatch_until(WAIT, |hp| {
        hp.pointer_layer()
            .is_some_and(|l| l != hocus_link::CONNECTIONS_NAMESPACE)
    })?;
    assert!(through, "the pointer is on {:?}", bed.hp.pointer_layer());
    bed.hp.button(BTN_LEFT, true);
    bed.hp.button(BTN_LEFT, false);
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert!(bed.hp.popover_surfaces().is_empty());
    drop(bed);
    // An adapter there: one row, which takes the pointer.
    let mut bed = start(&shell, Some(setup(Vec::new(), Vec::new())), false)?;
    bed.logged("row=Nothing")?;
    bed.surface_is(194, 40)?;
    let item = bed.item();
    bed.hp.pointer_move(item.0 - 1.0, item.1);
    bed.hp.pointer_move(item.0, item.1);
    let taken = bed.hp.dispatch_until(WAIT, |hp| {
        hp.pointer_layer().as_deref() == Some(hocus_link::CONNECTIONS_NAMESPACE)
    })?;
    assert!(taken, "the pointer is on {:?}", bed.hp.pointer_layer());
    // The adapter unplugged: the row goes, the pointer goes through.
    bed.bluez().remove_adapter()?;
    bed.logged("Bluetooth: not there (no adapter)")?;
    bed.hp.pointer_move(item.0 + 1.0, item.1);
    let through = bed.hp.dispatch_until(WAIT, |hp| {
        hp.pointer_layer()
            .is_some_and(|l| l != hocus_link::CONNECTIONS_NAMESPACE)
    })?;
    assert!(through, "the pointer is on {:?}", bed.hp.pointer_layer());
    Ok(())
}

#[test]
fn a_click_opens_it_and_searches_until_it_closes_listing_the_named_strongest_first()
-> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Some(setup(mine(), around())), true)?;
    let shape = Shape::new(&[BATTERY], 3);
    // As it opens nothing is around yet; then three named devices: the
    // unnamed one is left out.
    let item = bed.item();
    bed.click(item);
    bed.asked(&Call::StartDiscovery)?;
    let rect = popover(shape.height(3, 0.0));
    bed.popover_is(Some(rect))?;
    assert!(bed.bluez().discovering());
    // The strongest first: the Buds. A click pairs them.
    wait(&mut bed.hp, Duration::from_millis(300))?;
    bed.click(shape.nearby_row(rect, 0));
    bed.asked(&Call::Pair(BUDS.to_owned()))?;
    // Paired, trusted, connected: in the connected blocks now, the
    // newest first.
    bed.asked(&Call::Trust(BUDS.to_owned()))?;
    bed.asked(&Call::Connect(BUDS.to_owned()))?;
    let joined = bed.hp.dispatch_until(WAIT, |_| {
        bed.bluez
            .as_ref()
            .is_some_and(|b| b.connected().contains(&BUDS.to_owned()))
    })?;
    assert!(joined);
    assert_eq!(bed.bluez().paired(BUDS), (true, true));
    bed.logged("row=Connected { name: \"Galaxy Buds2 Pro\"")?;
    let shape = Shape::new(&[BLOCK, BATTERY], 3);
    bed.popover_is(Some(popover(shape.height(2, 0.0))))?;
    // A second click on the row closes it: the search stops.
    bed.click(item);
    bed.popover_is(None)?;
    bed.asked(&Call::StopDiscovery)?;
    assert!(!bed.bluez().discovering());
    // Open again, then a click outside: it stops again.
    bed.click(item);
    bed.popover_is(Some(popover(shape.height(2, 0.0))))?;
    let stop = Call::StopDiscovery;
    bed.click((600.0, 400.0));
    bed.popover_is(None)?;
    let stopped = bed.hp.dispatch_until(WAIT, |_| {
        bed.bluez
            .as_ref()
            .is_some_and(|b| b.calls().iter().filter(|c| **c == stop).count() == 2)
    })?;
    assert!(stopped, "{:?}", bed.bluez().calls());
    assert_eq!(bed.times(&Call::StartDiscovery), 2);
    Ok(())
}

#[test]
fn a_paired_device_connects_one_that_does_not_answer_says_so_and_disconnect() -> anyhow::Result<()>
{
    let Some(shell) = ready() else {
        return Ok(());
    };
    let devices = vec![
        FakeDevice::paired(SONY, "audio-headphones", Some(82)).connected(),
        FakeDevice::paired(JBL, "audio-card", Some(47)).answering(Answer::Refuses),
        FakeDevice::paired(KEYCHRON, "input-keyboard", None).answering(Answer::Hangs),
        FakeDevice::paired(MX, "input-mouse", Some(12)),
    ];
    let mut bed = start(&shell, Some(setup(devices, Vec::new())), true)?;
    // My devices, by name: JBL, Keychron, MX.
    let shape = Shape::new(&[BATTERY], 3);
    let rect = popover(shape.height(0, 0.0));
    bed.open(rect)?;
    // The mouse connects: its block comes, the newest first.
    bed.click(shape.mine_row(rect, 2));
    bed.asked(&Call::Connect(MX.to_owned()))?;
    let shape = Shape::new(&[BATTERY, BATTERY], 2);
    let rect = popover(shape.height(0, 0.0));
    bed.popover_is(Some(rect))?;
    bed.logged("row=Connected { name: \"MX Master 3S\", battery: Some(12), more: 1, low: true }")?;
    // The speaker refuses: said under its row.
    bed.click(shape.mine_row(rect, 0));
    bed.asked(&Call::Connect(JBL.to_owned()))?;
    let mut failed = shape.clone();
    failed.mine[0] = FAILURE;
    let rect = popover(failed.height(0, 0.0));
    bed.popover_is(Some(rect))?;
    // The keyboard does not answer: given up after the time allowed (1 s
    // here), then said; the speaker's message goes.
    bed.click(failed.mine_row(rect, 1));
    bed.asked(&Call::Connect(KEYCHRON.to_owned()))?;
    bed.logged("action=Connect")?;
    bed.asked(&Call::Disconnect(KEYCHRON.to_owned()))?;
    let mut given_up = shape.clone();
    given_up.mine[1] = FAILURE;
    bed.popover_is(Some(popover(given_up.height(0, 0.0))))?;
    // Disconnect: the mouse, first, leaves.
    let rect = popover(given_up.height(0, 0.0));
    bed.click(given_up.disconnect(rect, 0));
    bed.asked(&Call::Disconnect(MX.to_owned()))?;
    let mut back = Shape::new(&[BATTERY], 3);
    back.mine[1] = FAILURE;
    bed.popover_is(Some(popover(back.height(0, 0.0))))?;
    Ok(())
}

#[test]
fn while_nothing_is_connected_the_row_says_connecting() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let devices =
        vec![FakeDevice::paired(KEYCHRON, "input-keyboard", None).answering(Answer::Hangs)];
    let mut bed = start(&shell, Some(setup(devices, Vec::new())), true)?;
    bed.logged("row=Nothing")?;
    let shape = Shape::new(&[], 1);
    let rect = popover(shape.height(0, 0.0));
    bed.open(rect)?;
    bed.click(shape.mine_row(rect, 0));
    bed.logged("row=Connecting")?;
    // Given up: nothing again.
    bed.asked(&Call::Disconnect(KEYCHRON.to_owned()))?;
    bed.logged("step=Failed(NoAnswer)")?;
    Ok(())
}

#[test]
fn pairing_without_a_code_works_one_asking_a_passkey_goes_to_the_settings_one_fails()
-> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let later = vec![
        FakeDevice::around(BUDS, "audio-headphones", -50).pairing(Pairing::Fails),
        FakeDevice::around(PIXEL, "phone", -55).pairing(Pairing::Passkey),
        // A speaker that asks a passkey all the same.
        FakeDevice::around(BOSE, "audio-card", -60).pairing(Pairing::Passkey),
    ];
    let mut bed = start(&shell, Some(setup(Vec::new(), later)), true)?;
    bed.asked(&Call::RegisterAgent("NoInputNoOutput".to_owned()))?;
    let shape = Shape::new(&[], 0);
    let rect = popover(shape.height(3, 0.0));
    let item = bed.item();
    bed.click(item);
    bed.popover_is(Some(rect))?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    // The phone, second, is inert: "in Settings".
    bed.click(shape.nearby_row(rect, 1));
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert_eq!(bed.times(&Call::Pair(PIXEL.to_owned())), 0);
    // The speaker asks a passkey: the agent refuses, the row is left to
    // the Settings, nothing said under it.
    bed.click(shape.nearby_row(rect, 2));
    bed.asked(&Call::Pair(BOSE.to_owned()))?;
    bed.logged("step=Failed(Code)")?;
    assert_eq!(bed.popovers(), vec![rect]);
    bed.click(shape.nearby_row(rect, 2));
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert_eq!(bed.times(&Call::Pair(BOSE.to_owned())), 1);
    // The buds, not in pairing mode: said under their row.
    bed.click(shape.nearby_row(rect, 0));
    bed.asked(&Call::Pair(BUDS.to_owned()))?;
    // Two or three lines, as the font measures them.
    let grown = bed.hp.dispatch_until(WAIT, |hp| {
        hp.popover_surfaces()
            .first()
            .is_some_and(|(_, r)| r.height > rect.height + 30)
    })?;
    assert!(grown, "{:?}, not taller than {rect:?}", bed.popovers());
    assert_eq!(bed.bluez().paired(BUDS), (false, false));
    Ok(())
}

#[test]
fn past_four_devices_around_the_list_scrolls() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let later = vec![
        FakeDevice::around("Enceinte 1", "audio-card", -40),
        FakeDevice::around("Enceinte 2", "audio-card", -45),
        FakeDevice::around("Enceinte 3", "audio-card", -50),
        FakeDevice::around("Enceinte 4", "audio-card", -55),
        FakeDevice::around("Enceinte 5", "audio-card", -60),
        FakeDevice::around("Enceinte 6", "audio-card", -65),
    ];
    let mut bed = start(&shell, Some(setup(mine(), later)), true)?;
    let shape = Shape::new(&[BATTERY], 3);
    let item = bed.item();
    bed.click(item);
    let rect = popover(shape.height(6, 0.0));
    bed.popover_is(Some(rect))?;
    assert_eq!(rect.height, shape.height(4, 2.0), "four rows and a peek");
    wait(&mut bed.hp, Duration::from_millis(300))?;
    // The wheel brings the last one; a click pairs it.
    let first = shape.nearby_row(rect, 0);
    bed.hp.pointer_move(first.0, first.1);
    bed.hp.wheel(1);
    bed.hp.wheel(1);
    wait(&mut bed.hp, Duration::from_millis(300))?;
    let bottom = f64::from(rect.y) + shape.nearby_top() + LIST;
    bed.click((first.0, bottom - ROW / 2.0));
    bed.asked(&Call::Pair("Enceinte 6".to_owned()))?;
    Ok(())
}

#[test]
fn the_switch_turns_it_off_and_on_and_a_blocked_adapter_is_off() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Some(setup(mine(), Vec::new())), true)?;
    let shape = Shape::new(&[BATTERY], 3);
    let rect = popover(shape.height(0, 0.0));
    bed.open(rect)?;
    let switch = (f64::from(rect.x + 245), f64::from(rect.y + 29));
    bed.click(switch);
    bed.asked(&Call::Power(false))?;
    let off = popover(off_height());
    bed.popover_is(Some(off))?;
    bed.logged("row=Off")?;
    // The search stopped with the adapter; on again, it starts again.
    let switch = (switch.0, f64::from(off.y + 1 + 12 + 16));
    bed.click(switch);
    bed.asked(&Call::Power(true))?;
    let back = bed.hp.dispatch_until(WAIT, |_| {
        bed.bluez.as_ref().is_some_and(|b| {
            b.calls()
                .iter()
                .filter(|c| **c == Call::StartDiscovery)
                .count()
                == 2
        })
    })?;
    assert!(back, "{:?}", bed.bluez().calls());
    drop(bed);
    // Blocked by rfkill: off, and the switch cannot turn it on.
    let blocked = Setup {
        blocked: true,
        devices: mine(),
        ..Setup::default()
    };
    let mut bed = start(&shell, Some(blocked), true)?;
    bed.logged("row=Off")?;
    let off = popover(off_height());
    bed.open(off)?;
    bed.click((f64::from(off.x + 245), f64::from(off.y + 29)));
    bed.asked(&Call::Power(true))?;
    wait(&mut bed.hp, Duration::from_secs(4))?;
    assert!(!bed.bluez().powered());
    bed.popover_is(Some(off))?;
    Ok(())
}

#[test]
fn the_keyboard_goes_through_it_as_through_the_wifis() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Some(setup(mine(), Vec::new())), true)?;
    let shape = Shape::new(&[BATTERY], 3);
    let rect = popover(shape.height(0, 0.0));
    bed.open(rect)?;
    // Tab: the switch, Disconnect, then the first paired row (JBL);
    // Down: the Keychron; Enter connects it.
    bed.press(KEY_TAB);
    bed.press(KEY_TAB);
    bed.press(KEY_TAB);
    bed.press(KEY_DOWN);
    bed.press(KEY_ENTER);
    bed.asked(&Call::Connect(KEYCHRON.to_owned()))?;
    bed.popover_is(Some(popover(
        Shape::new(&[BATTERY, BATTERY], 2).height(0, 0.0),
    )))?;
    // Esc closes it; the row keeps the keyboard; Enter opens it again.
    bed.press(KEY_ESC);
    bed.popover_is(None)?;
    let held = bed.hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() == Some(hocus_link::CONNECTIONS_NAMESPACE)
    })?;
    assert!(held, "the keyboard is on {:?}", bed.hp.keyboard_layer());
    bed.press(KEY_ENTER);
    bed.popover_is(Some(popover(
        Shape::new(&[BATTERY, BATTERY], 2).height(0, 0.0),
    )))?;
    Ok(())
}

#[test]
fn in_the_mini_form_hovering_the_button_opens_it() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Some(setup(mine(), Vec::new())), true)?;
    bed.mini()?;
    let (x, y) = bed.item();
    bed.hp.pointer_move(x - 1.0, y);
    bed.hp.pointer_move(x, y);
    let rect = mini_popover(Shape::new(&[BATTERY], 3).height(0, 0.0));
    bed.popover_is(Some(rect))?;
    assert_eq!(bed.hp.keyboard_layer(), None, "a hover took the keyboard");
    bed.asked(&Call::StartDiscovery)?;
    // Away: it closes, and the search stops.
    bed.hp.pointer_move(800.0, 300.0);
    bed.popover_is(None)?;
    bed.asked(&Call::StopDiscovery)?;
    // The Wi-Fi's button, above, opens the Wi-Fi's.
    bed.hp.pointer_move(x, y - 40.0);
    let wifi = bed.hp.dispatch_until(WAIT, |hp| {
        hp.popover_surfaces()
            .first()
            .is_some_and(|(_, r)| r.height != rect.height)
    })?;
    assert!(wifi, "{:?}", bed.popovers());
    Ok(())
}

#[test]
fn nothing_draws_at_rest_and_a_change_draws_the_connections_alone() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Some(setup(mine(), Vec::new())), true)?;
    let rest = |bed: &mut Bed| -> anyhow::Result<(u64, u64, u64, u64)> {
        bed.settle()?;
        let before = bed.counts();
        wait(&mut bed.hp, Duration::from_secs(2))?;
        let after = bed.counts();
        Ok((
            after.0 - before.0,
            after.1 - before.1,
            after.2 - before.2,
            after.3 - before.3,
        ))
    };
    bed.hp.pointer_move(600.0, 500.0);
    assert_eq!(rest(&mut bed)?, (0, 0, 0, 0), "closed");
    // A battery's change draws the connections alone, there alone.
    for attempt in 0..5 {
        let clocks = bed.hp.clock_commits();
        wait(&mut bed.hp, Duration::from_millis(100))?;
        bed.hp.take_damage(0);
        let before = bed.counts();
        bed.bluez()
            .set_battery(SONY, if attempt % 2 == 0 { 81 } else { 80 })?;
        let drawn = bed
            .hp
            .dispatch_until(WAIT, |hp| hp.connections_commits() > before.2)?;
        assert!(drawn, "the row did not draw");
        wait(&mut bed.hp, Duration::from_millis(100))?;
        let after = bed.counts();
        assert_eq!((after.0, after.1, after.3), (before.0, before.1, before.3));
        if bed.hp.clock_commits() == clocks {
            let damage = bed.hp.take_damage(0).expect("damage");
            let s = bed.surface();
            assert!(
                damage.x >= s.x
                    && damage.y >= s.y
                    && damage.x + damage.width <= s.x + s.width
                    && damage.y + damage.height <= s.y + s.height,
                "{damage:?} out of {s:?}"
            );
            break;
        }
        assert!(attempt < 4, "the clock ticked through every try");
    }
    // A level's change does not draw the connections.
    let rows_drawn = bed.hp.connections_commits();
    let speakers = bed.fakes.sink("hocus-test-sink")?.to_string();
    bed.fakes.wpctl(&["set-volume", &speakers, "0.30"])?;
    let levels = bed.hp.levels_commits();
    let drawn = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.levels_commits() > levels)?;
    assert!(drawn);
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(bed.hp.connections_commits(), rows_drawn, "drew for a level");
    // Open, the pointer in it and still, the search under way: nothing
    // draws.
    let rect = popover(Shape::new(&[BATTERY], 3).height(0, 0.0));
    bed.open(rect)?;
    bed.asked(&Call::StartDiscovery)?;
    bed.hp
        .pointer_move(f64::from(rect.x + 140), f64::from(rect.y + 20));
    // Its first pass's "Recherche…" over (1 s here).
    wait(&mut bed.hp, Duration::from_millis(1500))?;
    assert_eq!(rest(&mut bed)?, (0, 0, 0, 0), "open");
    Ok(())
}
