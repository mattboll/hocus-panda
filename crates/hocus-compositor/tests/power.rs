//! The brightness's and the battery's popovers (docs/design/bar.md §9;
//! issue #364): the real shell, against the private fakes of its services
//! (`levels/fakes.rs`: UPower with its keyboard's backlight and its
//! battery's health, power-profiles-daemon, logind), never the machine's.
//!
//! Both rows open their popovers by a click, the battery's in the mini
//! form by hovering too; the brightness's slider, the wheel and the arrows
//! reach logind, its calls coalesced, never below 5 %; the keyboard's
//! backlight by its levels (two, three) or a slider (six), and none; the
//! other screens' line, for a screen plugged in and unplugged; the
//! battery's states, their words, their colours and the bolt on AC; the
//! health's line shown or not; the profiles switched, a missing one greyed
//! with its line; no battery, the modes alone; the mini form's battery sign
//! and its bolt; the keyboard; nothing drawn at rest, and a change drawing
//! only what shows it.
//!
//! Pixman here, so the shell draws on the CPU, without motion: as in CI.

mod common;
#[path = "levels/fakes.rs"]
mod fakes;

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use common::{BTN_LEFT, KEY_ESC, KEY_LEFT, KEY_RIGHT, KEY_TAB, WAIT, wait};
use fakes::{Fakes, Power};
use hocus_compositor::{Config, FormChoice, Headless, HeadlessConfig, RendererChoice, model::Rect};

const KEY_ENTER: u32 = 28;

/// The levels' surface on a 1920 × 1080 screen in the full form, and its
/// rows' middles with all three rows and a one-line status line.
const LEVELS: Rect = Rect::new(1920 - 8 - 1 - 194, 1080 - 8 - 41 - 175, 194, 175);
const LIGHT_ROW: (f64, f64) = (1800.0, (LEVELS.y + 87) as f64);
const BATTERY_ROW: (f64, f64) = (1800.0, (LEVELS.y + 117) as f64);
/// The status line's middle, alone under the rows (no battery).
const STATUS: (f64, f64) = (1850.0, (LEVELS.y + 175 - 20 - 9) as f64);
/// The battery's icon in its row.
const BATTERY_ICON: (i32, i32) = (LEVELS.x + 11, LEVELS.y + 117 - 9);
/// The mini form's levels' surface, its battery's item, and its sign.
const MINI: Rect = Rect::new(1920 - 8 - 1 - 54, 1080 - 8 - 41 - 128, 54, 128);
const MINI_BATTERY: (f64, f64) = ((MINI.x + 27) as f64, (MINI.y + 99) as f64);
const MINI_SIGN: (i32, i32) = (MINI.x + 14, MINI.y + 80);

/// The popovers' parts, from the mockup at ×1. The brightness's: its
/// height without the keyboard and with it, its slider's row and its ends
/// (the knob's centre at 5 % and 100 %), the keyboard's row and levels.
const LIGHT: i32 = 102;
const WITH_KEYS: i32 = 144;
const SLIDER_Y: i32 = 69;
const SLIDER: (f64, f64) = (63.0, 213.0);
const KEYS_Y: i32 = 111;
/// The battery's: the large battery's block, its line, the tiles.
const LINE: f64 = 1.366;
const BLOCK_TOP: f64 = 53.0;
const BLOCK: f64 = 2.0 + 26.0 * 1.1 + 1.0 + 13.5 * LINE + 4.0;
const STATE_TOP: f64 = BLOCK_TOP + 2.0 + 26.0 * 1.1 + 1.0;
const TILES_TOP: f64 = BLOCK_TOP + BLOCK + 8.0 + 2.0 + 1.0 + 2.0 + 8.0 + 4.0 + 11.5 * LINE + 8.0;
const TILE_X: [f64; 3] = [54.67, 140.0, 225.33];
/// Without a battery: the modes alone, under the header.
const MODES: i32 = 13 + 32 + 8 + 58 + 15;

/// The full bar's popover, `height` tall; the mini's.
fn popover(height: i32) -> Rect {
    Rect::new(1920 - 8 - 196 - 8 - 280, 1080 - 8 - height, 280, height)
}

fn mini_popover(height: i32) -> Rect {
    Rect::new(1920 - 8 - 56 - 8 - 280, 1080 - 8 - height, 280, height)
}

fn at(rect: Rect, (x, y): (f64, f64)) -> (f64, f64) {
    (f64::from(rect.x) + x, f64::from(rect.y) + y)
}

/// What the tests run against.
struct Bed {
    fakes: Fakes,
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

/// The compositor on `outputs`, in `form`, the shell against the fakes,
/// UPower and power-profiles-daemon as `power` says from the start.
fn start(
    shell: &Path,
    outputs: &[&str],
    form: FormChoice,
    power: impl FnOnce(&mut Power),
) -> anyhow::Result<Bed> {
    let fakes = Fakes::start_with_power(power)?;
    let log = fakes.dir.path().join("shell.log");
    let env = [("RUST_LOG", "warn,hocuspanda_shell=debug".to_owned())];
    let script = fakes.shell_with(shell, "fr_FR.UTF-8", &env, Some(&log))?;
    let mut config = Config::default();
    config.panel.form = form;
    let hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: outputs
                .iter()
                .map(|o| o.parse())
                .collect::<Result<_, _>>()?,
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
        Some(script),
    )?;
    let mut bed = Bed { fakes, hp, log };
    let want = match form {
        FormChoice::Mini => MINI,
        _ => LEVELS,
    };
    let placed = bed.hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.levels_surfaces().first().map(|(_, r)| *r) == Some(want) && hp.levels_commits() > 0
    })?;
    anyhow::ensure!(placed, "no levels: {:?}", bed.hp.levels_surfaces());
    bed.settle()?;
    Ok(bed)
}

/// Everything there, the full form on one screen.
fn start_full(shell: &Path, power: impl FnOnce(&mut Power)) -> anyhow::Result<Bed> {
    start(shell, &["1920x1080"], FormChoice::Full, power)
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

    /// The popover, once one shows (drawn, so that a click finds its
    /// parts).
    fn opened(&mut self) -> anyhow::Result<Rect> {
        let done = self
            .hp
            .dispatch_until(WAIT, |hp| hp.popover_surfaces().len() == 1)?;
        anyhow::ensure!(done, "no popover");
        wait(&mut self.hp, Duration::from_millis(300))?;
        Ok(self.popovers()[0])
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

    fn keyboard_on(&mut self, layer: Option<&str>) -> anyhow::Result<()> {
        let done = self
            .hp
            .dispatch_until(WAIT, |hp| hp.keyboard_layer().as_deref() == layer)?;
        anyhow::ensure!(done, "the keyboard is on {:?}", self.hp.keyboard_layer());
        Ok(())
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

    /// Waits until the fake backlight is at `want`.
    fn brightness(&mut self, want: u32) -> anyhow::Result<()> {
        let fakes = &self.fakes;
        let done = self
            .hp
            .dispatch_until(WAIT, |_| fakes.brightness() == Some(want))?;
        anyhow::ensure!(done, "brightness {:?}, not {want}", fakes.brightness());
        Ok(())
    }

    /// Waits until what the fakes were asked is as `done` says.
    fn asked(&mut self, done: impl Fn(&Power) -> bool) -> anyhow::Result<()> {
        let fakes = &self.fakes;
        let ok = self.hp.dispatch_until(WAIT, |_| done(&fakes.power_now()))?;
        anyhow::ensure!(ok, "the fakes were asked {:?}", fakes.power_now());
        Ok(())
    }

    /// The pixels of the box from `(x, y)`, `w` × `h` (global), that are
    /// `like`.
    fn count(
        &mut self,
        (x, y): (i32, i32),
        (w, h): (i32, i32),
        like: fn((u8, u8, u8)) -> bool,
    ) -> anyhow::Result<usize> {
        let (width, _, bytes) = self.hp.screenshot(0)?;
        let mut n = 0;
        for y in y..y + h {
            for x in x..x + w {
                let at = usize::try_from(y * width + x)? * 4;
                if let Some([b, g, r, _]) = bytes.get(at..at + 4)
                    && like((*r, *g, *b))
                {
                    n += 1;
                }
            }
        }
        Ok(n)
    }

    /// Waits until the box holds more than `least` pixels `like`, or no
    /// more than `most`.
    fn until_count(
        &mut self,
        at: (i32, i32),
        size: (i32, i32),
        like: fn((u8, u8, u8)) -> bool,
        between: (usize, usize),
    ) -> anyhow::Result<()> {
        let start = Instant::now();
        let mut seen = 0;
        while start.elapsed() < WAIT {
            seen = self.count(at, size, like)?;
            if (between.0..=between.1).contains(&seen) {
                return Ok(());
            }
            wait(&mut self.hp, Duration::from_millis(50))?;
        }
        anyhow::bail!("{seen} pixels at {at:?}, not within {between:?}")
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

fn green((r, g, b): (u8, u8, u8)) -> bool {
    g > r.saturating_add(40) && g > b.saturating_add(20)
}

/// Amber or orange: the battery's low and critical colours.
fn warm((r, _, b): (u8, u8, u8)) -> bool {
    r > 150 && r > b.saturating_add(60)
}

fn white((r, g, b): (u8, u8, u8)) -> bool {
    r.min(g).min(b) > 200
}

#[test]
fn both_rows_open_their_popovers_and_the_keyboard_goes_through_them() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start_full(&shell, |_| {})?;
    // The brightness's row: its popover, without a keyboard's row nor a
    // screen's line, its bottom on the bar's; it takes the keyboard.
    bed.click(LIGHT_ROW);
    bed.popover_is(Some(popover(LIGHT)))?;
    bed.logged("keyboard=None")?;
    bed.logged("screens=[]")?;
    bed.keyboard_on(Some("hocus-bar-popover"))?;
    // Esc: closed, the row holds the keyboard; Enter opens it again.
    bed.press(KEY_ESC);
    bed.popover_is(None)?;
    bed.keyboard_on(Some("hocus-bar-levels"))?;
    bed.press(KEY_ENTER);
    bed.popover_is(Some(popover(LIGHT)))?;
    bed.press(KEY_ESC);
    bed.popover_is(None)?;
    bed.press(KEY_ESC);
    bed.keyboard_on(None)?;

    // The battery's row: the battery, the modes, the health.
    bed.click(BATTERY_ROW);
    let rect = bed.opened()?;
    assert_eq!(
        (rect.x, rect.y + rect.height),
        (1920 - 8 - 196 - 8 - 280, 1072)
    );
    bed.logged("title=Batterie plugged=Some(\"Sur batterie\") line=Some(\"3 h 40 restantes\")")?;
    bed.logged("health=Some(\"État de la batterie")?;
    bed.logged("91")?;
    bed.logged("de sa capacité d'origine\")")?;
    // The keyboard: Tab to the first tile, Enter chooses it; the arrows
    // choose the next.
    bed.press(KEY_TAB);
    bed.press(KEY_ENTER);
    bed.asked(|p| p.profile == "power-saver")?;
    bed.logged("chosen=Some(PowerSaver)")?;
    bed.press(KEY_RIGHT);
    bed.press(KEY_RIGHT);
    bed.asked(|p| p.profile == "performance")?;
    // Past the last, it stays.
    bed.press(KEY_RIGHT);
    bed.press(KEY_LEFT);
    bed.asked(|p| p.profile == "balanced")?;
    // A click outside closes it; a second click on the row too.
    bed.click((600.0, 500.0));
    bed.popover_is(None)?;
    bed.click(BATTERY_ROW);
    bed.popover_is(Some(rect))?;
    bed.click(BATTERY_ROW);
    bed.popover_is(None)?;
    // A click on one row while the other's is open: the other's.
    bed.click(LIGHT_ROW);
    bed.popover_is(Some(popover(LIGHT)))?;
    bed.click(BATTERY_ROW);
    bed.popover_is(Some(rect))?;
    Ok(())
}

#[test]
fn the_mini_forms_battery_opens_by_hovering_and_by_a_click() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, &["1920x1080"], FormChoice::Mini, |_| {})?;
    // Resting on it: open after 150 ms, without the keyboard.
    bed.hp.pointer_move(MINI_BATTERY.0 - 1.0, MINI_BATTERY.1);
    bed.hp.pointer_move(MINI_BATTERY.0, MINI_BATTERY.1);
    wait(&mut bed.hp, Duration::from_millis(100))?;
    assert_eq!(bed.popovers(), vec![], "open before 150 ms");
    let rect = bed.opened()?;
    assert_eq!(rect, mini_popover(rect.height));
    assert_eq!(bed.hp.keyboard_layer(), None);
    bed.logged("title=Batterie")?;
    // Leaving for the windows closes it.
    bed.hp.pointer_move(800.0, 300.0);
    bed.popover_is(None)?;
    // A click keeps it.
    bed.click(MINI_BATTERY);
    bed.popover_is(Some(rect))?;
    bed.hp.pointer_move(800.0, 300.0);
    wait(&mut bed.hp, Duration::from_millis(700))?;
    assert_eq!(bed.popovers(), vec![rect]);
    bed.keyboard_on(Some("hocus-bar-popover"))?;
    // A tile chosen in it.
    bed.click(at(rect, (TILE_X[2], TILES_TOP + 29.0)));
    bed.asked(|p| p.profile == "performance")?;
    Ok(())
}

#[test]
fn the_slider_the_wheel_and_the_arrows_reach_logind_coalesced_never_below_5() -> anyhow::Result<()>
{
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start_full(&shell, |_| {})?;
    // The wheel over the bar's row: 5 % a notch, without a popover.
    bed.hp.pointer_move(LIGHT_ROW.0 - 1.0, LIGHT_ROW.1);
    bed.hp.pointer_move(LIGHT_ROW.0, LIGHT_ROW.1);
    bed.hp.wheel(-1);
    bed.brightness(750)?;
    bed.hp.wheel(1);
    bed.brightness(700)?;
    assert_eq!(bed.popovers(), vec![]);

    // A slow logind: the slider's moves are asked as it is ready, only the
    // last each time.
    *bed.fakes
        .brightness_delay
        .lock()
        .map_err(|_| anyhow::anyhow!("poisoned"))? = Duration::from_millis(300);
    bed.click(LIGHT_ROW);
    bed.popover_is(Some(popover(LIGHT)))?;
    let rect = popover(LIGHT);
    wait(&mut bed.hp, Duration::from_millis(300))?;
    let calls_before = calls(&bed.fakes).len();
    let x = |share: f64| f64::from(rect.x) + SLIDER.0 + (SLIDER.1 - SLIDER.0) * share;
    let y = f64::from(rect.y + SLIDER_Y);
    // Pressed at its left end: 5 %, never below.
    bed.hp.pointer_move(x(0.0) - 8.0, y);
    bed.hp.pointer_move(x(0.0) - 7.0, y);
    bed.hp.button(BTN_LEFT, true);
    for step in 0..=30 {
        bed.hp.pointer_move(x(f64::from(step) / 30.0), y);
        bed.hp.dispatch(Duration::from_millis(5))?;
    }
    bed.hp.button(BTN_LEFT, false);
    bed.brightness(1000)?;
    let calls = calls(&bed.fakes)[calls_before..].to_vec();
    assert_eq!(calls.first(), Some(&50), "{calls:?}");
    assert_eq!(calls.last(), Some(&1000), "{calls:?}");
    assert!(calls.len() <= 5, "not coalesced: {calls:?}");
    *bed.fakes
        .brightness_delay
        .lock()
        .map_err(|_| anyhow::anyhow!("poisoned"))? = Duration::ZERO;
    // Back down to its left end and past it: 5 %.
    bed.hp.button(BTN_LEFT, true);
    bed.hp.pointer_move(x(0.5), y);
    bed.hp.dispatch(Duration::from_millis(20))?;
    bed.hp.pointer_move(x(0.0) - 40.0, y);
    bed.hp.button(BTN_LEFT, false);
    bed.brightness(50)?;
    // The wheel over its row, and the arrows on the slider: 5 %.
    bed.hp.pointer_move(x(0.5), y);
    bed.hp.wheel(-1);
    bed.brightness(100)?;
    // A fast spin, two notches in one event: 10 %.
    bed.hp.wheel(-2);
    bed.brightness(200)?;
    bed.hp.wheel(2);
    bed.brightness(100)?;
    bed.press(KEY_TAB);
    bed.press(KEY_RIGHT);
    bed.brightness(150)?;
    // logind answers late: a key from the keyboard changes the backlight
    // after logind set it and before it answers (#478).
    let late = Duration::from_millis(400);
    *bed.fakes
        .brightness_answer
        .lock()
        .map_err(|_| anyhow::anyhow!("poisoned"))? = late;
    bed.press(KEY_LEFT);
    bed.press(KEY_LEFT);
    bed.brightness(50)?;
    // A key from the keyboard while it is open: the slider follows, and
    // keeps it once logind answers.
    bed.fakes.backlight(600, 1000)?;
    wait(&mut bed.hp, late * 2)?;
    let knob = (x(55.0 / 95.0) as i32, rect.y + SLIDER_Y);
    bed.until_count(knob, (1, 1), white, (1, 1))?;
    Ok(())
}

/// The values the fake logind was asked.
fn calls(fakes: &Fakes) -> Vec<u32> {
    fakes
        .brightness_calls
        .lock()
        .map(|c| c.iter().map(|(_, _, v)| *v).collect())
        .unwrap_or_default()
}

#[test]
fn the_keyboards_backlight_by_its_levels_or_a_slider() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // Three levels: off, low, high; at low.
    let mut bed = start_full(&shell, |p| p.keyboard = Some((1, 2)))?;
    bed.click(LIGHT_ROW);
    bed.popover_is(Some(popover(WITH_KEYS)))?;
    let rect = popover(WITH_KEYS);
    bed.logged("keyboard=Some(Keyboard { level: 1, max: 2 })")?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    // "Fort": its last third.
    bed.click(at(rect, (229.0, f64::from(KEYS_Y))));
    bed.asked(|p| p.keyboard_set == [2])?;
    bed.logged("keyboard=Some(Keyboard { level: 2, max: 2 })")?;
    // Its keys change it: the popover follows.
    bed.fakes.set_keyboard(0)?;
    bed.logged("keyboard=Some(Keyboard { level: 0, max: 2 })")?;
    // The keyboard: Tab past the slider to the levels, the arrows choose.
    bed.press(KEY_TAB);
    bed.press(KEY_TAB);
    bed.press(KEY_RIGHT);
    bed.asked(|p| p.keyboard_set == [2, 1])?;
    bed.press(KEY_TAB);
    bed.press(KEY_ENTER);
    bed.asked(|p| p.keyboard_set == [2, 1, 2])?;
    drop(bed);

    // Two: off, on.
    let mut bed = start_full(&shell, |p| p.keyboard = Some((0, 1)))?;
    bed.click(LIGHT_ROW);
    bed.popover_is(Some(popover(WITH_KEYS)))?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    bed.click(at(rect, (212.0, f64::from(KEYS_Y))));
    bed.asked(|p| p.keyboard_set == [1])?;
    drop(bed);

    // Six: a slider, a level each sixth of it.
    let mut bed = start_full(&shell, |p| p.keyboard = Some((2, 5)))?;
    bed.click(LIGHT_ROW);
    bed.popover_is(Some(popover(WITH_KEYS)))?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    bed.click(at(rect, (SLIDER.1, f64::from(KEYS_Y))));
    bed.asked(|p| p.keyboard_set.last() == Some(&5))?;
    bed.press(KEY_TAB);
    bed.press(KEY_TAB);
    bed.press(KEY_LEFT);
    bed.asked(|p| p.keyboard_set.last() == Some(&4))?;
    Ok(())
}

#[test]
fn another_screen_is_said_to_be_set_with_its_own_buttons() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // Two screens: the bar on the first, which the backlight lights.
    let mut bed = start(
        &shell,
        &["1920x1080", "1920x1080"],
        FormChoice::Full,
        |_| {},
    )?;
    bed.click(LIGHT_ROW);
    let rect = bed.opened()?;
    bed.logged("screens=[\"L'écran HEADLESS-2 se règle avec ses propres boutons.\"]")?;
    assert!(rect.height > LIGHT, "{rect:?}");
    assert_eq!(rect.y + rect.height, 1072);
    // Unplugged while it is open: the line goes, the popover shrinks.
    bed.hp.unplug("HEADLESS-2")?;
    bed.popover_is(Some(popover(LIGHT)))?;
    bed.logged("screens=[]")?;
    // Back: the line again.
    bed.hp.plug("HEADLESS-2")?;
    bed.popover_is(Some(rect))?;
    Ok(())
}

#[test]
fn the_batterys_states_their_words_their_colours_and_the_bolt_on_ac() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start_full(&shell, |_| {})?;
    let icon = |bed: &mut Bed| bed.count(BATTERY_ICON, (18, 18), green);
    // On battery: no bolt.
    assert_eq!(icon(&mut bed)?, 0, "a bolt on battery");
    bed.click(BATTERY_ROW);
    let rect = bed.opened()?;
    let line = (rect.x + 69, rect.y + STATE_TOP as i32);
    let line_size = (190, 18);
    assert_eq!(bed.count(line, line_size, warm)?, 0);
    // Charging, on AC.
    bed.fakes.set_power(|p| {
        p.on_battery = false;
        p.state = 1;
        p.percentage = 46.0;
        p.to_full = 70 * 60;
    })?;
    bed.logged("plugged=Some(\"Branché\") line=Some(\"En charge · plein dans 1 h 10\")")?;
    bed.until_count(BATTERY_ICON, (18, 18), green, (4, 400))?;
    // Full.
    bed.fakes.set_power(|p| {
        p.state = 4;
        p.percentage = 100.0;
    })?;
    bed.logged("line=Some(\"Branché, plein\")")?;
    assert!(icon(&mut bed)? > 3);
    // A charge limit: the firmware says discharging, UPower that it is on
    // AC: plugged in, the bolt.
    bed.fakes.set_power(|p| {
        p.state = 2;
        p.percentage = 80.0;
    })?;
    bed.logged("line=Some(\"Branché, ne charge pas\")")?;
    assert!(icon(&mut bed)? > 3);
    // On battery, low: amber, no bolt.
    bed.fakes.set_power(|p| {
        p.on_battery = true;
        p.percentage = 18.0;
        p.to_empty = 50 * 60;
    })?;
    bed.logged("plugged=Some(\"Sur batterie\") line=Some(\"50 min restantes\")")?;
    bed.until_count(BATTERY_ICON, (18, 18), green, (0, 0))?;
    bed.until_count(line, line_size, warm, (10, 10_000))?;
    // Critical: orange, and the charger asked for.
    bed.fakes.set_power(|p| {
        p.percentage = 9.0;
        p.to_empty = 25 * 60;
    })?;
    bed.logged("line=Some(\"25 min restantes · branche le chargeur\")")?;
    bed.until_count(line, line_size, warm, (10, 10_000))?;
    // The health not known: its line goes, the popover shrinks.
    let with = rect.height;
    bed.fakes.set_power(|p| p.capacity = 0.0)?;
    bed.logged("health=None")?;
    let shrunk = bed.hp.dispatch_until(WAIT, |hp| {
        hp.popover_surfaces()
            .first()
            .is_some_and(|(_, r)| r.height < with && r.y + r.height == 1072)
    })?;
    assert!(shrunk, "{:?}", bed.popovers());
    Ok(())
}

#[test]
fn the_profiles_switch_and_a_missing_one_is_greyed_with_its_line() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start_full(&shell, |p| {
        p.profiles = vec!["power-saver", "balanced"];
    })?;
    bed.click(BATTERY_ROW);
    let rect = bed.opened()?;
    bed.logged("notes=[\"Le mode performance n'existe pas sur cet ordinateur.\"]")?;
    // The active tile's border, green.
    let edge = |i: usize| (rect.x + TILE_X[i] as i32, rect.y + (TILES_TOP + 0.5) as i32);
    bed.until_count(edge(1), (1, 1), green, (1, 1))?;
    // The missing one does nothing.
    bed.click(at(rect, (TILE_X[2], TILES_TOP + 29.0)));
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert!(bed.fakes.power_now().profiles_set.is_empty());
    // The saver: asked, chosen, lit.
    bed.click(at(rect, (TILE_X[0], TILES_TOP + 29.0)));
    bed.asked(|p| p.profile == "power-saver")?;
    bed.until_count(edge(0), (1, 1), green, (1, 1))?;
    bed.until_count(edge(1), (1, 1), green, (0, 0))?;
    // Tab goes through the two there only.
    bed.press(KEY_TAB);
    bed.press(KEY_TAB);
    bed.press(KEY_ENTER);
    bed.asked(|p| p.profile == "balanced")?;
    bed.press(KEY_TAB);
    bed.press(KEY_ENTER);
    bed.asked(|p| p.profile == "power-saver" && p.profiles_set.len() == 3)?;
    drop(bed);

    // Performance held back, while chosen: why, under the tiles.
    let mut bed = start_full(&shell, |p| {
        p.profile = "performance".to_owned();
        p.degraded = "high-operating-temperature".to_owned();
    })?;
    bed.click(BATTERY_ROW);
    bed.opened()?;
    bed.logged("notes=[\"Performance réduite")?;
    bed.logged("l'ordinateur est trop chaud.\"]")?;
    Ok(())
}

#[test]
fn without_a_battery_the_status_line_opens_the_modes_alone() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start_full(&shell, |p| p.present = false)?;
    bed.click(STATUS);
    bed.popover_is(Some(popover(MODES)))?;
    bed.logged("title=Énergie plugged=None line=None")?;
    bed.click(at(popover(MODES), (TILE_X[0], 53.0 + 29.0)));
    bed.asked(|p| p.profile == "power-saver")?;
    Ok(())
}

#[test]
fn the_mini_battery_sign_wears_a_bolt_on_ac() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, &["1920x1080"], FormChoice::Mini, |p| {
        p.percentage = 60.0;
    })?;
    // Its outline, filled green to its level (60 %: 10 of its 18 units).
    assert!(bed.count(MINI_SIGN, (26, 15), white)? > 20);
    let fill = ((MINI_SIGN.0 + 3, MINI_SIGN.1 + 4), (10, 7));
    let green_on_battery = bed.count(fill.0, fill.1, green)?;
    assert!(green_on_battery > 50, "{green_on_battery}");
    // On AC: the bolt over the fill.
    bed.fakes.set_power(|p| {
        p.on_battery = false;
        p.state = 1;
    })?;
    bed.until_count(fill.0, fill.1, green, (0, green_on_battery - 6))?;
    // Back on battery: no bolt; low: amber.
    bed.fakes.set_power(|p| {
        p.on_battery = true;
        p.state = 2;
        p.percentage = 15.0;
    })?;
    bed.until_count(MINI_SIGN, (26, 15), green, (0, 0))?;
    assert!(bed.count(MINI_SIGN, (26, 15), warm)? > 3);
    Ok(())
}

#[test]
fn nothing_draws_at_rest_and_a_change_draws_only_what_shows_it() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start_full(&shell, |p| p.keyboard = Some((1, 2)))?;
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
    assert_eq!(rest(&mut bed)?, (0, 0, 0, 0));
    let bars = bed.hp.bar_commits();
    for row in [LIGHT_ROW, BATTERY_ROW] {
        bed.click(row);
        let rect = bed.opened()?;
        let inside = at(rect, (140.0, 20.0));
        bed.hp.pointer_move(inside.0 - 1.0, inside.1);
        bed.hp.pointer_move(inside.0, inside.1);
        // Open and still: nothing draws.
        assert_eq!(rest(&mut bed)?, (0, 0, 0, 0));
        assert_eq!(bed.hp.bar_commits(), bars, "the bar was drawn again");
        bed.press(KEY_ESC);
        bed.popover_is(None)?;
        bed.press(KEY_ESC);
    }

    // A tile under the pointer: the popover alone draws, its rectangle
    // only.
    bed.click(BATTERY_ROW);
    let rect = bed.opened()?;
    // Off the row first: the row's light goes with it.
    let inside = at(rect, (140.0, 20.0));
    bed.hp.pointer_move(inside.0, inside.1);
    bed.settle()?;
    for attempt in 0..5 {
        let clocks = bed.hp.clock_commits();
        wait(&mut bed.hp, Duration::from_millis(100))?;
        bed.hp.take_damage(0);
        let (popover, levels) = (bed.hp.popover_commits(), bed.hp.levels_commits());
        let tile = at(rect, (TILE_X[attempt % 2 * 2], TILES_TOP + 29.0));
        bed.hp.pointer_move(tile.0, tile.1);
        let drawn = bed
            .hp
            .dispatch_until(WAIT, |hp| hp.popover_commits() > popover)?;
        anyhow::ensure!(drawn, "the popover did not draw the hover");
        wait(&mut bed.hp, Duration::from_millis(100))?;
        assert_eq!(bed.hp.levels_commits(), levels, "the levels were drawn");
        assert_eq!(bed.hp.bar_commits(), bars, "the bar was drawn again");
        if bed.hp.clock_commits() == clocks {
            assert_eq!(bed.hp.take_damage(0), Some(rect));
            // The battery changes: the levels and the popover draw it,
            // nothing else.
            let connections = bed.hp.connections_commits();
            bed.fakes.set_power(|p| p.percentage = 77.0)?;
            let both = bed.hp.dispatch_until(WAIT, |hp| {
                hp.popover_commits() > popover + 1 && hp.levels_commits() > levels
            })?;
            assert!(both, "the change was not drawn");
            assert_eq!(bed.hp.bar_commits(), bars, "the bar was drawn again");
            assert_eq!(bed.hp.connections_commits(), connections);
            return Ok(());
        }
    }
    anyhow::bail!("the clock ticked through every try")
}
