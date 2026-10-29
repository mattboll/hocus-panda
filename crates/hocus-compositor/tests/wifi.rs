//! The Wi-Fi's row and popover (docs/design/bar.md §4 group 9, §5, §9;
//! issue #362): the real shell, against a fake NetworkManager on the
//! fakes' private bus (`levels/nm.rs`), never the machine's.
//!
//! The row's states in both forms (its signal's arcs, connecting, offline,
//! off, a cable, the sign-in page's dot), drawn on a surface of its own;
//! the popover opened by a click and, in the mini form, by hovering; the
//! scan asked as it opens; the networks merged, sorted, scrolled past six;
//! a saved network joined in one click; a protected one's password typed
//! on a French keyboard and handed to NetworkManager exactly; a wrong one
//! said and kept; `Esc` folding, then closing; the switch, Disconnect, the
//! sign-in page opened; the password in no log; nothing drawn at rest.
//! A password pasted (issue #453): copied by a test client with its line
//! break, `Ctrl+V` and `Shift+Insert` in the field hand NetworkManager
//! exactly the password; the eye shows it and hides it, and no log says
//! it even shown; out of the field, from an empty clipboard, one
//! without text, or a client that never writes, nothing is pasted and the
//! field goes on taking keys.
//!
//! Bluetooth's row is there too, under the Wi-Fi's (issue #363): the
//! connections' surface holds both.
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

use common::{
    BTN_LEFT, ClientHandle, Clip, Command, Event, KEY_ESC, KEY_LEFTCTRL, KEY_LEFTMETA,
    KEY_LEFTSHIFT, KEY_TAB, WAIT, chord, wait,
};
use fakes::Fakes;
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};
use nm::{Call, FakeAp, Nm, Security, Setup};

const KEY_B: u32 = 48;
const KEY_ENTER: u32 = 28;
const KEY_BACKSPACE: u32 = 14;
const KEY_DOWN: u32 = 108;
const KEY_RIGHTALT: u32 = 100;
/// On a French (AZERTY) keyboard: a, z, e, r, t, y; `&` (`1` with
/// Shift), `@` (AltGr+`0`), the dead circumflex.
const KEY_Q: u32 = 16;
const KEY_W: u32 = 17;
const KEY_E: u32 = 18;
const KEY_R: u32 = 19;
const KEY_T: u32 = 20;
const KEY_Y: u32 = 21;
const KEY_1: u32 = 2;
const KEY_0: u32 = 11;
const KEY_LEFTBRACE: u32 = 26;
const KEY_V: u32 = 47;
const KEY_INSERT: u32 = 110;

/// The password typed in the tests: an obvious one, with what a French
/// keyboard types through Shift, AltGr and a dead key.
const PASSWORD: &str = "azerty&1@ê";

const LIVEBOX: &str = "Livebox-A3F2";
const FREEBOX: &str = "Freebox-7C21E0";
const SNCF: &str = "_SNCF_WIFI_INOUI";

/// The connections' surface with the Wi-Fi's and Bluetooth's rows.
const TWO_ROWS: i32 = 72;

/// The mockup's networks: seven others around the Livebox.
fn around() -> Vec<FakeAp> {
    vec![
        FakeAp::new(LIVEBOX, 80, Security::Psk("livebox-test")),
        FakeAp::new(SNCF, 62, Security::Open),
        FakeAp::new(FREEBOX, 58, Security::Psk(PASSWORD)),
        FakeAp::new("iPhone de Alex", 45, Security::Psk("iphone-test")),
        FakeAp::new("eduroam", 40, Security::Enterprise),
        FakeAp::new("Bbox-55D1", 22, Security::Psk("bbox-test")),
        FakeAp::new("DIRECT-4B-HP Laser 107w", 15, Security::Psk("printer-test")),
        FakeAp::new("Maison-5G", 12, Security::Psk("maison-test")),
    ]
}

/// The popover's parts, from the mockup at ×1: its padding, its header,
/// the blocks above the list, the divider and the label.
const HEADER_END: f64 = 1.0 + 12.0 + 32.0;
const GAP: f64 = 6.0;
const RULE_BLOCK: f64 = 9.0;
const LABEL_ROW: f64 = 4.0 + 12.0 * 1.366;
const NOTE: f64 = 4.0 + 13.5 * 1.366 + 2.0;
const CURRENT: f64 = 48.0;
/// With a sign-in page waiting: its state on two lines beside Disconnect,
/// in French.
const SIGN_IN_CURRENT: f64 = 14.0 * 1.366 + 1.0 + 2.0 * 12.5 * 1.366;
const PORTAL: f64 = 34.0;
const WIRED: f64 = 32.0;
const ROW: f64 = 36.0;
const ROW_STEP: f64 = 38.0;
/// The unfolded password form, and its error: two lines in French.
const FORM: f64 = 2.0 + 34.0 + 8.0 + 30.0 + 10.0;
const ERROR: f64 = 2.0 * 12.5 * 1.366 + 8.0;

/// The list's top in the popover, under `blocks`.
fn list_top(blocks: &[f64]) -> f64 {
    let mut y = HEADER_END;
    for block in blocks {
        y += GAP + block;
    }
    y + GAP + RULE_BLOCK + GAP + LABEL_ROW + GAP
}

/// The popover's height: `blocks`, then `rows` networks (none: the note)
/// with `extra` under one of them.
fn height(blocks: &[f64], rows: usize, extra: f64) -> i32 {
    let end = if rows == 0 {
        list_top(blocks) + NOTE
    } else {
        let content = rows as f64 * ROW_STEP - 2.0 + extra;
        list_top(blocks) + content.min(236.0)
    };
    (end + 14.0 + 1.0).round() as i32
}

/// The full bar's popover, `height` tall, on a 1920 × 1080 screen.
fn popover(height: i32) -> Rect {
    Rect::new(1920 - 8 - 196 - 8 - 280, 1080 - 8 - height, 280, height)
}

fn mini_popover(height: i32) -> Rect {
    Rect::new(1920 - 8 - 56 - 8 - 280, 1080 - 8 - height, 280, height)
}

/// Row `i`'s middle in the popover at `rect`, under `blocks`.
fn row(rect: Rect, blocks: &[f64], i: usize) -> (f64, f64) {
    (
        f64::from(rect.x) + 120.0,
        f64::from(rect.y) + list_top(blocks) + ROW_STEP * i as f64 + ROW / 2.0,
    )
}

/// What the tests run against.
struct Bed {
    fakes: Fakes,
    nm: Nm,
    _bluez: bluez::Bluez,
    hp: Headless,
    /// What the shell logged, if kept.
    log: Option<PathBuf>,
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

/// The compositor with a French keyboard, the shell against the fakes and
/// NetworkManager as `setup` says; the levels and the row drawn.
fn start(shell: &Path, setup: Setup, keep_log: bool) -> anyhow::Result<Bed> {
    let fakes = Fakes::start()?;
    let nm = Nm::start(fakes.runtime(), &fakes.address, setup)?;
    // An adapter, nothing paired: Bluetooth's row under the Wi-Fi's.
    let bluez = bluez::Bluez::start(fakes.runtime(), &fakes.address, bluez::Setup::default())?;
    let log = keep_log.then(|| fakes.dir.path().join("shell.log"));
    let opener = opener(&fakes)?;
    let env = [("HOCUS_OPENER", opener.display().to_string())];
    let script = fakes.shell_with(shell, "fr_FR.UTF-8", &env, log.as_deref())?;
    let mut config = Config::default();
    config.keyboard.layouts = vec!["fr".to_owned()];
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
        Some(script),
    )?;
    let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.connections_surfaces()
            .first()
            .is_some_and(|(_, r)| r.height == TWO_ROWS)
            && hp.connections_commits() > 0
            && hp.levels_commits() > 0
    })?;
    anyhow::ensure!(drawn, "no connections: {:?}", hp.connections_surfaces());
    let mut bed = Bed {
        fakes,
        nm,
        _bluez: bluez,
        hp,
        log,
    };
    // Where the levels' group ends, once they read their services.
    bed.settle()?;
    Ok(bed)
}

/// A program standing for the browser: it writes what it opens beside it.
fn opener(fakes: &Fakes) -> anyhow::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let script = fakes.dir.path().join("opener");
    let opened = fakes.dir.path().join("opened");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\n",
            opened.display()
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    Ok(script)
}

impl Bed {
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

    /// The Wi-Fi's row (full) or button (mini), the top one: its middle.
    fn item(&self) -> (f64, f64) {
        let s = self.surface();
        let middle = if s.width == 54 { 24.0 } else { 20.0 };
        (f64::from(s.x + s.width / 2), f64::from(s.y) + middle - 0.5)
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
        let nm = &self.nm;
        let done = self
            .hp
            .dispatch_until(WAIT, |_| nm.calls().contains(call))?;
        anyhow::ensure!(done, "not asked {call:?}: {:?}", self.nm.calls());
        Ok(())
    }

    /// Waits until the shell's log has a line with `text` (debug).
    fn logged(&mut self, text: &str) -> anyhow::Result<()> {
        let Some(log) = self.log.clone() else {
            anyhow::bail!("no log kept");
        };
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

    /// The brightest of the pixels from `(x, y - 1)` to `(x, y + 1)`.
    fn brightest(&mut self, (x, y): (i32, i32)) -> anyhow::Result<u8> {
        let mut most = 0;
        for y in y - 1..=y + 1 {
            let (r, g, b) = self.rgb((x, y))?;
            most = most.max(r.min(g).min(b));
        }
        Ok(most)
    }

    /// The full row's icon's parts, lit or not: the dot and the three
    /// arcs, from the inner one.
    fn arcs(&mut self) -> anyhow::Result<[bool; 4]> {
        let s = self.surface();
        // The icon, 18 px, 6 px into the row, centred on it: the row's
        // middle 20 px into the surface, give or take a pixel (the levels'
        // group's top is not whole).
        let (x, top) = (s.x + 11 + 9, s.y + 20 - 9);
        // The SVG's tops of the arcs and the dot's middle, ×0.75.
        let mut lit = [false; 4];
        for (part, y) in [(0, 14.25), (1, 10.55), (2, 7.24), (3, 3.82)] {
            let y = top + (y - 0.5_f64).round() as i32;
            lit[part] = self.brightest((x, y))? > 150;
        }
        Ok(lit)
    }

    /// Waits until `count` parts of the row's icon are lit.
    fn bars(&mut self, count: usize) -> anyhow::Result<()> {
        let start = Instant::now();
        let mut seen = [false; 4];
        while start.elapsed() < WAIT {
            seen = self.arcs()?;
            let want: Vec<bool> = (0..4).map(|i| i < count).collect();
            if seen.as_slice() == want.as_slice() {
                return Ok(());
            }
            wait(&mut self.hp, Duration::from_millis(50))?;
        }
        anyhow::bail!("the icon's parts lit: {seen:?}, not {count}")
    }

    /// A window that takes the keyboard puts `clip` on the clipboard; it
    /// keeps offering it while it runs.
    fn copy(&mut self, clip: Clip) -> anyhow::Result<ClientHandle> {
        let (rect, client) = common::open_with(&mut self.hp, "test.copy", (300, 200), false)?;
        self.copy_again(&client, rect, clip)?;
        Ok(client)
    }

    /// The window `client` at `rect` takes the keyboard and copies `clip`.
    fn copy_again(&mut self, client: &ClientHandle, rect: Rect, clip: Clip) -> anyhow::Result<()> {
        self.click((
            f64::from(rect.x + rect.width / 2),
            f64::from(rect.y + rect.height / 2),
        ));
        wait(&mut self.hp, Duration::from_millis(100))?;
        client.commands.send(Command::Copy(clip))?;
        let copied = self.hp.dispatch_until(WAIT, |_| {
            client.events.try_iter().any(|e| e == Event::Copied)
        })?;
        anyhow::ensure!(copied, "the clipboard was never taken");
        Ok(())
    }

    /// The light pixels on line `y` from `x` to `x + width` (global).
    fn ink(&mut self, (x, y): (i32, i32), width: i32) -> anyhow::Result<usize> {
        let mut lit = 0;
        for x in x..x + width {
            let (r, g, b) = self.rgb((x, y))?;
            if r.min(g).min(b) > 150 {
                lit += 1;
            }
        }
        Ok(lit)
    }

    /// Opens the popover, `rows` networks, and unfolds row `i`'s field.
    fn unfold(&mut self, rows: usize, i: usize) -> anyhow::Result<Rect> {
        let item = self.item();
        self.click(item);
        let rect = popover(height(&[], rows, 0.0));
        self.popover_is(Some(rect))?;
        self.click(row(rect, &[], i));
        self.popover_is(Some(popover(height(&[], rows, FORM))))?;
        // Drawn unfolded, so that the keys find the field.
        wait(&mut self.hp, Duration::from_millis(300))?;
        Ok(rect)
    }

    /// Types `PASSWORD` on the French keyboard: `azerty`, `&`, `1` (Shift),
    /// `@` (AltGr), `ê` (the dead circumflex, then e).
    fn type_password(&mut self) {
        for key in [KEY_Q, KEY_W, KEY_E, KEY_R, KEY_T, KEY_Y, KEY_1] {
            self.press(key);
        }
        chord(&mut self.hp, &[KEY_LEFTSHIFT, KEY_1]);
        chord(&mut self.hp, &[KEY_RIGHTALT, KEY_0]);
        self.press(KEY_LEFTBRACE);
        self.press(KEY_E);
    }
}

/// What the shell logged, without its colours.
fn plain(log: &Path) -> String {
    let text = String::from_utf8_lossy(&std::fs::read(log).unwrap_or_default()).into_owned();
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // `ESC [ … m`
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

#[test]
fn the_row_shows_the_network_and_its_signal_in_both_forms() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let setup = Setup {
        aps: around(),
        saved: vec![(LIVEBOX, Some("livebox-test"))],
        current: Some(LIVEBOX),
        ..Setup::default()
    };
    let mut bed = start(&shell, setup, true)?;
    bed.logged("Connected { name: \"Livebox-A3F2\", bars: 4 }")?;
    // 14 px above the levels' group: the sound's row's top.
    let levels = bed.hp.levels_surfaces()[0].1;
    let s = bed.surface();
    assert_eq!((s.x, s.width, s.height), (levels.x, 194, TWO_ROWS));
    assert!(s.y + s.height > levels.y, "{s:?} over {levels:?}");
    bed.bars(4)?;
    // Each strength lights its parts; the levels and the bar do not draw.
    let (bars, levels_drawn) = (bed.hp.bar_commits(), bed.hp.levels_commits());
    for (strength, count) in [(55, 3), (35, 2), (10, 1), (90, 4)] {
        bed.nm.set_strength(LIVEBOX, strength)?;
        bed.bars(count)?;
    }
    assert_eq!(bed.hp.levels_commits(), levels_drawn, "the levels drew");
    assert_eq!(bed.hp.bar_commits(), bars, "the bar drew");
    // A strength that lights the same parts draws nothing.
    bed.settle()?;
    let drawn = bed.hp.connections_commits();
    bed.nm.set_strength(LIVEBOX, 85)?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert_eq!(bed.hp.connections_commits(), drawn, "drawn for nothing");

    // The mini form: a 44 × 38 button above the gauge.
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_B]);
    let mini = bed.hp.dispatch_until(WAIT, |hp| {
        hp.connections_surfaces()
            .first()
            .is_some_and(|(_, r)| r.width == 54)
    })?;
    assert!(mini, "{:?}", bed.hp.connections_surfaces());
    bed.settle()?;
    let s = bed.surface();
    let gauge = bed.hp.levels_surfaces()[0].1;
    assert_eq!((s.width, s.height), (54, 88));
    assert!(s.y + s.height <= gauge.y + 10, "{s:?} over {gauge:?}");
    // Its icon, 20 px in the button's middle: the outer arc lit.
    let top = (s.x + 5 + 22, s.y + 5 + 9 + 4);
    assert!(bed.brightest(top)? > 150, "the mini icon's outer arc");
    Ok(())
}

#[test]
fn the_row_says_offline_off_connecting_a_cable_and_a_sign_in_page() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let setup = Setup {
        aps: around(),
        saved: vec![(SNCF, None)],
        portals: vec![SNCF],
        ..Setup::default()
    };
    let mut bed = start(&shell, setup, true)?;
    bed.logged("link=Offline")?;
    bed.bars(0)?;
    // The sign-in page: joined, its dot on the icon, orange.
    let (x, y) = bed.item();
    bed.click((x, y));
    let rect = popover(height(&[], 8, 0.0));
    bed.popover_is(Some(rect))?;
    // The Livebox first, the SNCF's second.
    bed.click(row(rect, &[], 1));
    bed.asked(&Call::Activate {
        ssid: SNCF.to_owned(),
    })?;
    bed.logged("link=Connecting")?;
    bed.logged("Connected { name: \"_SNCF_WIFI_INOUI\"")?;
    let s = bed.surface();
    bed.until((s.x + 11 + 17, s.y + 20 - 9 + 1), orange)?;
    // Its popover: the orange button under the network.
    let blocks = [SIGN_IN_CURRENT, PORTAL];
    bed.popover_is(Some(popover(height(&blocks, 7, 0.0))))?;
    // A cable: "Câble", whatever the Wi-Fi does.
    bed.nm.set_wired(true)?;
    bed.logged("link=Wired")?;
    // The popover says it above the network.
    let blocks = [WIRED, SIGN_IN_CURRENT, PORTAL];
    bed.popover_is(Some(popover(height(&blocks, 7, 0.0))))?;
    bed.nm.set_wired(false)?;
    // The Wi-Fi off, from the switch.
    bed.press(KEY_TAB);
    bed.press(KEY_ENTER);
    bed.asked(&Call::Enable(false))?;
    bed.logged("link=Off")?;
    bed.popover_is(Some(popover(
        (HEADER_END + GAP + NOTE + 15.0).round() as i32
    )))?;
    Ok(())
}

#[test]
fn a_click_opens_it_asks_for_a_scan_and_lists_the_networks_once_each() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // Two access points of the Livebox, a hidden network, and one that
    // the scan finds.
    let setup = Setup {
        aps: vec![
            FakeAp::new(LIVEBOX, 40, Security::Psk("livebox-test")),
            FakeAp::new(SNCF, 62, Security::Open),
            FakeAp::new(LIVEBOX, 75, Security::Psk("livebox-test")),
            FakeAp::new("", 90, Security::Psk("hidden-test")),
        ],
        later: vec![FakeAp::new(FREEBOX, 30, Security::Psk(PASSWORD))],
        ..Setup::default()
    };
    let mut bed = start(&shell, setup, false)?;
    let item = bed.item();
    bed.click(item);
    // Two rows, then three once the scan asked as it opened is done.
    bed.popover_is(Some(popover(height(&[], 2, 0.0))))?;
    bed.asked(&Call::Scan)?;
    bed.popover_is(Some(popover(height(&[], 3, 0.0))))?;
    // A second click on the row closes it; a click opens it again.
    bed.click(item);
    bed.popover_is(None)?;
    bed.click(item);
    bed.popover_is(Some(popover(height(&[], 3, 0.0))))?;
    // A click outside closes it.
    bed.click((600.0, 400.0));
    bed.popover_is(None)?;
    Ok(())
}

#[test]
fn past_six_networks_the_list_scrolls_and_a_saved_one_joins_in_a_click() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let setup = Setup {
        aps: around(),
        saved: vec![
            (LIVEBOX, Some("livebox-test")),
            ("Maison-5G", Some("maison-test")),
        ],
        current: Some(LIVEBOX),
        ..Setup::default()
    };
    let mut bed = start(&shell, setup, false)?;
    let item = bed.item();
    bed.click(item);
    let blocks = [CURRENT];
    let rect = popover(height(&blocks, 7, 0.0));
    bed.popover_is(Some(rect))?;
    // The last row, under the list's end: the wheel brings it.
    let first = row(rect, &blocks, 0);
    bed.hp.pointer_move(first.0, first.1);
    bed.hp.wheel(1);
    bed.hp.wheel(1);
    wait(&mut bed.hp, Duration::from_millis(200))?;
    // Scrolled to its end: the last row's bottom on the list's.
    let bottom = f64::from(rect.y) + list_top(&blocks) + 236.0;
    bed.click((first.0, bottom - ROW / 2.0));
    bed.asked(&Call::Activate {
        ssid: "Maison-5G".to_owned(),
    })?;
    let joined = bed
        .hp
        .dispatch_until(WAIT, |_| bed_connected(&bed.nm, "Maison-5G"))?;
    assert!(joined, "connected to {:?}", bed.nm.connected());
    // Off the list, its rows sort by strength again.
    bed.hp
        .pointer_move(f64::from(rect.x + 100), f64::from(rect.y + 20));
    wait(&mut bed.hp, Duration::from_millis(300))?;
    // The keyboard reaches the rows too: Tab to the switch, Disconnect,
    // the first row, then down.
    bed.press(KEY_TAB);
    bed.press(KEY_TAB);
    bed.press(KEY_TAB);
    bed.press(KEY_DOWN);
    bed.press(KEY_ENTER);
    // The second row now is the SNCF's (the Livebox first): open, it
    // joins.
    bed.asked(&Call::AddAndActivate {
        ssid: SNCF.to_owned(),
        psk: None,
    })?;
    Ok(())
}

fn bed_connected(nm: &Nm, ssid: &str) -> bool {
    nm.connected().as_deref() == Some(ssid)
}

#[test]
fn a_password_typed_on_a_french_keyboard_reaches_networkmanager_exactly() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let setup = Setup {
        aps: around(),
        ..Setup::default()
    };
    let mut bed = start(&shell, setup, true)?;
    let item = bed.item();
    bed.click(item);
    let rect = popover(height(&[], 8, 0.0));
    bed.popover_is(Some(rect))?;
    // The Freebox, third: protected, not saved: its field unfolds.
    bed.click(row(rect, &[], 2));
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(
        bed.popovers(),
        vec![rect],
        "the list is already at its most"
    );
    // Seven characters: Enter does nothing.
    for key in [KEY_Q, KEY_W, KEY_E, KEY_R, KEY_T, KEY_Y, KEY_1] {
        bed.press(key);
    }
    bed.press(KEY_ENTER);
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert!(
        !bed.nm
            .calls()
            .iter()
            .any(|c| matches!(c, Call::AddAndActivate { .. })),
        "{:?}",
        bed.nm.calls()
    );
    // A typo taken back, then the rest.
    bed.press(KEY_Q);
    bed.press(KEY_BACKSPACE);
    chord(&mut bed.hp, &[KEY_LEFTSHIFT, KEY_1]);
    chord(&mut bed.hp, &[KEY_RIGHTALT, KEY_0]);
    bed.press(KEY_LEFTBRACE);
    bed.press(KEY_E);
    bed.press(KEY_ENTER);
    bed.asked(&Call::AddAndActivate {
        ssid: FREEBOX.to_owned(),
        psk: Some(PASSWORD.to_owned()),
    })?;
    let joined = bed
        .hp
        .dispatch_until(WAIT, |_| bed_connected(&bed.nm, FREEBOX))?;
    assert!(joined, "connected to {:?}", bed.nm.connected());
    // Saved with it, by NetworkManager.
    assert!(
        bed.nm
            .saved()
            .contains(&(FREEBOX.to_owned(), Some(PASSWORD.to_owned()))),
        "saved {:?}, asked {:?}",
        bed.nm.saved(),
        bed.nm.calls()
    );
    bed.logged("Connected { name: \"Freebox-7C21E0\"")?;
    // The row folded: the Freebox is the current network now.
    bed.popover_is(Some(popover(height(&[CURRENT], 7, 0.0))))?;
    // Nothing the shell logged, every crate at trace, says it.
    let log = plain(bed.log.as_ref().expect("a log"));
    for part in [PASSWORD, "azerty&", "rty&1@"] {
        assert!(!log.contains(part), "the log says {part:?}");
    }
    Ok(())
}

#[test]
fn a_wrong_password_is_said_and_kept_and_esc_folds_then_closes() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let setup = Setup {
        aps: vec![
            FakeAp::new(SNCF, 62, Security::Open),
            FakeAp::new(FREEBOX, 58, Security::Psk("the-right-one")),
        ],
        ..Setup::default()
    };
    let mut bed = start(&shell, setup, false)?;
    let item = bed.item();
    bed.click(item);
    let rect = popover(height(&[], 2, 0.0));
    bed.popover_is(Some(rect))?;
    bed.click(row(rect, &[], 1));
    let unfolded = popover(height(&[], 2, FORM));
    bed.popover_is(Some(unfolded))?;
    // Esc folds it; a second Esc closes the popover, the row keeps the
    // keyboard.
    bed.press(KEY_ESC);
    bed.popover_is(Some(rect))?;
    bed.press(KEY_ESC);
    bed.popover_is(None)?;
    let row_held = bed.hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() == Some("hocus-bar-connections")
    })?;
    assert!(row_held, "the keyboard is on {:?}", bed.hp.keyboard_layer());
    // Enter opens it again; the field again; a wrong password.
    bed.press(KEY_ENTER);
    bed.popover_is(Some(rect))?;
    bed.click(row(rect, &[], 1));
    bed.popover_is(Some(unfolded))?;
    bed.type_password();
    bed.press(KEY_ENTER);
    bed.asked(&Call::AddAndActivate {
        ssid: FREEBOX.to_owned(),
        psk: Some(PASSWORD.to_owned()),
    })?;
    // Said under the field, which stays, and the connection it made is
    // not kept.
    bed.popover_is(Some(popover(height(&[], 2, FORM + ERROR))))?;
    bed.asked(&Call::Delete {
        ssid: FREEBOX.to_owned(),
    })?;
    assert!(bed.nm.saved().is_empty(), "{:?}", bed.nm.saved());
    // The field still has the keyboard: the right one, typed over it.
    for _ in 0..PASSWORD.chars().count() {
        bed.press(KEY_BACKSPACE);
    }
    for key in [KEY_T, KEY_Y, KEY_E, KEY_R, KEY_T, KEY_Y, KEY_Q, KEY_W] {
        bed.press(key);
    }
    bed.press(KEY_ENTER);
    // "tyertyaz": not the right one either; NetworkManager is asked again.
    bed.asked(&Call::AddAndActivate {
        ssid: FREEBOX.to_owned(),
        psk: Some("tyertyaz".to_owned()),
    })?;
    Ok(())
}

#[test]
fn a_saved_network_whose_password_is_refused_unfolds_its_field() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // Saved with a password the network no longer takes.
    let setup = Setup {
        aps: vec![
            FakeAp::new(SNCF, 62, Security::Open),
            FakeAp::new(FREEBOX, 58, Security::Psk(PASSWORD)),
        ],
        saved: vec![(FREEBOX, Some("an-old-password"))],
        ..Setup::default()
    };
    let mut bed = start(&shell, setup, false)?;
    let item = bed.item();
    bed.click(item);
    let rect = popover(height(&[], 2, 0.0));
    bed.popover_is(Some(rect))?;
    // One click: NetworkManager tries the saved one, which fails.
    bed.click(row(rect, &[], 1));
    bed.asked(&Call::Activate {
        ssid: FREEBOX.to_owned(),
    })?;
    // The field unfolds with the error, holding the keyboard; the saved
    // connection stays.
    bed.popover_is(Some(popover(height(&[], 2, FORM + ERROR))))?;
    assert_eq!(bed.nm.saved().len(), 1);
    bed.type_password();
    bed.press(KEY_ENTER);
    // The saved connection takes the new password, then joins.
    bed.asked(&Call::Update {
        ssid: FREEBOX.to_owned(),
        psk: Some(PASSWORD.to_owned()),
    })?;
    let joined = bed
        .hp
        .dispatch_until(WAIT, |_| bed_connected(&bed.nm, FREEBOX))?;
    assert!(
        joined,
        "connected to {:?}: {:?}",
        bed.nm.connected(),
        bed.nm.calls()
    );
    bed.popover_is(Some(popover(height(&[CURRENT], 1, 0.0))))?;
    Ok(())
}

#[test]
fn the_switch_disconnect_and_the_sign_in_page() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let setup = Setup {
        aps: around(),
        saved: vec![(SNCF, None)],
        current: Some(SNCF),
        portals: vec![SNCF],
        ..Setup::default()
    };
    let mut bed = start(&shell, setup, false)?;
    let item = bed.item();
    bed.click(item);
    let blocks = [SIGN_IN_CURRENT, PORTAL];
    let rect = popover(height(&blocks, 7, 0.0));
    bed.popover_is(Some(rect))?;
    // The sign-in page's button: the check address, in the browser.
    let button = (
        f64::from(rect.x) + 140.0,
        f64::from(rect.y) + HEADER_END + GAP + SIGN_IN_CURRENT + GAP + PORTAL / 2.0,
    );
    bed.click(button);
    let opened = bed.fakes.dir.path().join("opened");
    let done = bed.hp.dispatch_until(WAIT, |_| {
        std::fs::read_to_string(&opened).is_ok_and(|o| o.trim() == "http://check.hocus.test/portal")
    })?;
    assert!(done, "opened {:?}", std::fs::read_to_string(&opened).ok());
    // Disconnect: the device leaves its network.
    let disconnect = (
        f64::from(rect.x + 280 - 15 - 20),
        f64::from(rect.y) + HEADER_END + GAP + CURRENT / 2.0,
    );
    bed.click(disconnect);
    bed.asked(&Call::Disconnect)?;
    bed.popover_is(Some(popover(height(&[], 8, 0.0))))?;
    // The switch: off, then on again.
    let rect = popover(height(&[], 8, 0.0));
    let switch = (f64::from(rect.x + 245), f64::from(rect.y + 29));
    bed.click(switch);
    bed.asked(&Call::Enable(false))?;
    let off = popover((HEADER_END + GAP + NOTE + 15.0).round() as i32);
    bed.popover_is(Some(off))?;
    let switch = (switch.0, f64::from(off.y + 1 + 12 + 16));
    bed.click(switch);
    bed.asked(&Call::Enable(true))?;
    let back = bed.hp.dispatch_until(WAIT, |_| bed.nm.enabled())?;
    assert!(back);
    Ok(())
}

#[test]
fn in_the_mini_form_hovering_the_button_opens_it() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let setup = Setup {
        aps: around(),
        ..Setup::default()
    };
    let mut bed = start(&shell, setup, false)?;
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_B]);
    let mini = bed.hp.dispatch_until(WAIT, |hp| {
        hp.connections_surfaces()
            .first()
            .is_some_and(|(_, r)| r.width == 54)
    })?;
    assert!(mini);
    bed.settle()?;
    let (x, y) = bed.item();
    bed.hp.pointer_move(x - 1.0, y);
    bed.hp.pointer_move(x, y);
    let rect = mini_popover(height(&[], 8, 0.0));
    bed.popover_is(Some(rect))?;
    assert_eq!(bed.hp.keyboard_layer(), None, "a hover took the keyboard");
    bed.asked(&Call::Scan)?;
    // Away: it closes.
    bed.hp.pointer_move(800.0, 300.0);
    bed.popover_is(None)?;
    // A click opens it as in the full form; a protected row's field takes
    // the keyboard.
    bed.click((x, y));
    bed.popover_is(Some(rect))?;
    bed.click(row(rect, &[], 2));
    let held = bed.hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() == Some("hocus-bar-popover")
    })?;
    assert!(held);
    Ok(())
}

#[test]
fn nothing_draws_at_rest_and_each_change_draws_its_own_surface() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let setup = Setup {
        aps: around(),
        saved: vec![(LIVEBOX, Some("livebox-test"))],
        current: Some(LIVEBOX),
        ..Setup::default()
    };
    let mut bed = start(&shell, setup, false)?;
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
    // A network's change draws the row alone, there alone.
    for attempt in 0..5 {
        let clocks = bed.hp.clock_commits();
        wait(&mut bed.hp, Duration::from_millis(100))?;
        bed.hp.take_damage(0);
        let before = bed.counts();
        bed.nm
            .set_strength(LIVEBOX, if attempt % 2 == 0 { 40 } else { 90 })?;
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
    // A level's change does not draw the row.
    let rows_drawn = bed.hp.connections_commits();
    let speakers = bed.fakes.sink("hocus-test-sink")?.to_string();
    bed.fakes.wpctl(&["set-volume", &speakers, "0.30"])?;
    let levels = bed.hp.levels_commits();
    let drawn = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.levels_commits() > levels)?;
    assert!(drawn);
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(
        bed.hp.connections_commits(),
        rows_drawn,
        "the row drew for a level"
    );
    // Open, the pointer in it and still: nothing draws.
    let item = bed.item();
    bed.click(item);
    let rect = popover(height(&[CURRENT], 7, 0.0));
    bed.popover_is(Some(rect))?;
    bed.hp
        .pointer_move(f64::from(rect.x + 140), f64::from(rect.y + 20));
    // Once the scan asked as it opened is done (and said).
    bed.asked(&Call::Scan)?;
    wait(&mut bed.hp, Duration::from_secs(2))?;
    assert_eq!(rest(&mut bed)?, (0, 0, 0, 0), "open");
    // A row under the pointer: the popover alone draws.
    let (x, y) = row(rect, &[CURRENT], 1);
    let before = bed.counts();
    bed.hp.pointer_move(x, y);
    let drawn = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.popover_commits() > before.3)?;
    assert!(drawn);
    wait(&mut bed.hp, Duration::from_millis(100))?;
    let after = bed.counts();
    assert_eq!((after.0, after.1, after.2), (before.0, before.1, before.2));
    Ok(())
}

#[test]
fn without_networkmanager_no_row_and_without_wifi_no_list() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // No Wi-Fi adapter: "Hors ligne", and the popover says so.
    let setup = Setup {
        hardware: false,
        ..Setup::default()
    };
    let mut bed = start(&shell, setup, true)?;
    bed.logged("link=Offline")?;
    let item = bed.item();
    bed.click(item);
    bed.popover_is(Some(popover(
        (HEADER_END + GAP + NOTE + 15.0).round() as i32
    )))?;
    drop(bed);
    // No NetworkManager at all: the row is not drawn, nothing opens.
    let fakes = Fakes::start()?;
    let log = fakes.dir.path().join("shell.log");
    let script = fakes.shell_with(&shell, "fr_FR.UTF-8", &[], Some(&log))?;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        Config::default(),
        Some(script),
    )?;
    let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| hp.connections_commits() > 0)?;
    assert!(drawn);
    let said = hp.dispatch_until(WAIT, |_| plain(&log).contains("NetworkManager: not there"))?;
    assert!(said, "the shell never said NetworkManager is away");
    let s = hp.connections_surfaces()[0].1;
    let (x, y) = (f64::from(s.x + 97), f64::from(s.y + 20));
    hp.pointer_move(x - 1.0, y);
    hp.pointer_move(x, y);
    // Its surface draws no row, and lets the pointer through to the bar's
    // surfaces under it.
    let through = hp.dispatch_until(WAIT, |hp| {
        hp.pointer_layer()
            .is_some_and(|l| l != hocus_link::CONNECTIONS_NAMESPACE)
    })?;
    assert!(through, "the pointer is on {:?}", hp.pointer_layer());
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    wait(&mut hp, Duration::from_millis(500))?;
    assert!(hp.popover_surfaces().is_empty());
    Ok(())
}

/// What the copying client offers: text, with the line break a password
/// copied from a file ends with.
const COPIED: Clip = Clip {
    types: &["text/plain;charset=utf-8", "text/plain"],
    text: "azerty&1@ê\n",
    hang: false,
};

#[test]
fn a_password_pasted_with_ctrl_v_reaches_networkmanager_without_its_line_break()
-> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let setup = Setup {
        aps: around(),
        ..Setup::default()
    };
    let mut bed = start(&shell, setup, true)?;
    let _copier = bed.copy(COPIED)?;
    let item = bed.item();
    bed.click(item);
    let rect = popover(height(&[], 8, 0.0));
    bed.popover_is(Some(rect))?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    // No field yet: nothing to paste into.
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_V]);
    wait(&mut bed.hp, Duration::from_millis(300))?;
    // The Freebox's field; then the eye holds the keyboard: nothing either.
    bed.click(row(rect, &[], 2));
    bed.popover_is(Some(popover(height(&[], 8, FORM))))?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    bed.press(KEY_TAB);
    wait(&mut bed.hp, Duration::from_millis(300))?;
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_V]);
    wait(&mut bed.hp, Duration::from_millis(300))?;
    // Back in the field: pasted, without its line break.
    chord(&mut bed.hp, &[KEY_LEFTSHIFT, KEY_TAB]);
    wait(&mut bed.hp, Duration::from_millis(300))?;
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_V]);
    bed.logged("the clipboard's text pasted into the field")?;
    // The eye shows it, then hides it again; the field keeps the keyboard.
    let field = popover(height(&[], 8, FORM));
    let middle = f64::from(field.y) + list_top(&[]) + 2.0 * ROW_STEP + ROW + 2.0 + 17.0;
    let eye = (f64::from(field.x) + 240.0, middle);
    // Above the dots, under the letters' tops.
    let band = (field.x + 53, middle as i32 - 5);
    wait(&mut bed.hp, Duration::from_millis(300))?;
    let dots = bed.ink(band, 150)?;
    bed.click(eye);
    wait(&mut bed.hp, Duration::from_millis(500))?;
    let letters = bed.ink(band, 150)?;
    bed.click(eye);
    wait(&mut bed.hp, Duration::from_millis(500))?;
    let hidden = bed.ink(band, 150)?;
    assert!(
        dots <= 2 && letters >= 5 && hidden <= 2,
        "light pixels above the dots: {dots}, shown {letters}, hidden {hidden}"
    );
    bed.press(KEY_ENTER);
    bed.asked(&Call::AddAndActivate {
        ssid: FREEBOX.to_owned(),
        psk: Some(PASSWORD.to_owned()),
    })?;
    let joined = bed
        .hp
        .dispatch_until(WAIT, |_| bed_connected(&bed.nm, FREEBOX))?;
    assert!(joined, "connected to {:?}", bed.nm.connected());
    // Pasted once only: the one call.
    let tries = bed
        .nm
        .calls()
        .into_iter()
        .filter(|c| matches!(c, Call::AddAndActivate { .. }))
        .count();
    assert_eq!(tries, 1, "{:?}", bed.nm.calls());
    // Nothing the shell logged, every crate at trace, says it, though it
    // was shown.
    let log = plain(bed.log.as_ref().expect("a log"));
    for part in [PASSWORD, "azerty&", "rty&1@"] {
        assert!(!log.contains(part), "the log says {part:?}");
    }
    Ok(())
}

#[test]
fn shift_insert_pastes_too_and_clears_a_wrong_password() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let setup = Setup {
        aps: vec![
            FakeAp::new(SNCF, 62, Security::Open),
            FakeAp::new(FREEBOX, 58, Security::Psk("the-right-one")),
        ],
        ..Setup::default()
    };
    let mut bed = start(&shell, setup, false)?;
    let _copier = bed.copy(Clip {
        text: "azerty&1@ê\r\n",
        ..COPIED
    })?;
    bed.unfold(2, 1)?;
    chord(&mut bed.hp, &[KEY_LEFTSHIFT, KEY_INSERT]);
    // Enter as soon as it is there.
    let mut tried = false;
    let start = Instant::now();
    while !tried && start.elapsed() < WAIT {
        bed.press(KEY_ENTER);
        wait(&mut bed.hp, Duration::from_millis(100))?;
        tried = bed
            .nm
            .calls()
            .iter()
            .any(|c| matches!(c, Call::AddAndActivate { .. }));
    }
    bed.asked(&Call::AddAndActivate {
        ssid: FREEBOX.to_owned(),
        psk: Some(PASSWORD.to_owned()),
    })?;
    // Refused: said under the field; a paste clears it, as typing does,
    // and adds to what the field holds.
    let refused = popover(height(&[], 2, FORM + ERROR));
    bed.popover_is(Some(refused))?;
    chord(&mut bed.hp, &[KEY_LEFTSHIFT, KEY_INSERT]);
    bed.popover_is(Some(popover(height(&[], 2, FORM))))?;
    bed.press(KEY_ENTER);
    bed.asked(&Call::AddAndActivate {
        ssid: FREEBOX.to_owned(),
        psk: Some(format!("{PASSWORD}{PASSWORD}")),
    })?;
    Ok(())
}

#[test]
fn nothing_is_pasted_from_an_empty_clipboard_one_without_text_or_a_client_that_hangs()
-> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let setup = Setup {
        aps: vec![
            FakeAp::new(SNCF, 62, Security::Open),
            FakeAp::new(FREEBOX, 58, Security::Psk(PASSWORD)),
        ],
        ..Setup::default()
    };
    let mut bed = start(&shell, setup, true)?;
    // Nothing was ever copied.
    bed.unfold(2, 1)?;
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_V]);
    bed.logged("the clipboard is empty")?;
    bed.press(KEY_ESC);
    bed.press(KEY_ESC);
    bed.popover_is(None)?;
    // An image only.
    let image = Clip {
        types: &["image/png"],
        text: "not an image",
        hang: false,
    };
    let copier = bed.copy(image)?;
    bed.unfold(2, 1)?;
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_V]);
    bed.logged("the clipboard holds no text")?;
    bed.press(KEY_ESC);
    bed.press(KEY_ESC);
    bed.popover_is(None)?;
    // A client that never writes: the field goes on taking keys while
    // the paste waits, and the paste gives up.
    let window = common::rect_of(&bed.hp, "test.copy").expect("the copying window");
    bed.copy_again(
        &copier,
        window,
        Clip {
            hang: true,
            ..COPIED
        },
    )?;
    bed.unfold(2, 1)?;
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_V]);
    bed.logged("the clipboard's text asked for")?;
    bed.type_password();
    bed.press(KEY_ENTER);
    bed.asked(&Call::AddAndActivate {
        ssid: FREEBOX.to_owned(),
        psk: Some(PASSWORD.to_owned()),
    })?;
    bed.logged("did not write it in time")?;
    let log = plain(bed.log.as_ref().expect("a log"));
    assert!(
        !log.contains("pasted into the field"),
        "something was pasted"
    );
    Ok(())
}
