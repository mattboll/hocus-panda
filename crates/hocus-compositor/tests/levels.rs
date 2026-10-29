//! The bar's levels (docs/design/bar.md §4 group 10, §5; issue #360): the
//! real shell, against private fakes of its services ([`fakes`]): its
//! gauges show them and follow them, the keyboard's level keys change them,
//! a service missing hides its row until it comes, and a change draws only
//! the levels' region.
//!
//! Pixman here, so the shell draws on the CPU, without motion: as in CI.

mod common;
#[path = "levels/fakes.rs"]
mod fakes;

use std::time::Duration;

use common::{KEY_LEFTMETA, WAIT, chord, shell_program, wait};
use fakes::Fakes;
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};

const KEY_B: u32 = 48;
const KEY_MUTE: u32 = 113;
const KEY_VOLUMEDOWN: u32 = 114;
const KEY_VOLUMEUP: u32 = 115;
const KEY_BRIGHTNESSDOWN: u32 = 224;
const KEY_BRIGHTNESSUP: u32 = 225;

/// The levels' surfaces on a 1920×1080 screen: inside the bar's rim, 41 px
/// above its bottom, 194 × 175 in the full form, 54 × 97 in the mini.
const FULL: Rect = Rect::new(1920 - 8 - 1 - 194, 1080 - 8 - 41 - 175, 194, 175);
const MINI: Rect = Rect::new(1920 - 8 - 1 - 54, 1080 - 8 - 41 - 97, 54, 97);

/// Where to look in the full form, a few pixels into each gauge (x), and
/// each row's middle (y), with all three rows and a status line of one
/// line; and the mini form's gauge.
const GAUGE_X: i32 = FULL.x + 40;
const SOUND_Y: i32 = FULL.y + 57;
const LIGHT_Y: i32 = FULL.y + 87;
const BATTERY_Y: i32 = FULL.y + 117;
const MINI_GAUGE: (i32, i32) = (MINI.x + 27, MINI.y + 57);

/// The fakes there, or why the test cannot run.
fn ready() -> Option<std::path::PathBuf> {
    let Some(shell) = shell_program() else {
        eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
        return None;
    };
    if !fakes::available() {
        eprintln!("dbus-daemon, pipewire, pw-dump, wpctl or pw-metadata missing: skipped");
        return None;
    }
    Some(shell)
}

fn start(fakes: &Fakes, shell: &std::path::Path, lang: &str) -> anyhow::Result<Headless> {
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        Config::default(),
        Some(fakes.shell(shell, lang)?),
    )?;
    let placed = hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.levels_surfaces() == vec![("HEADLESS-1".to_owned(), FULL)] && hp.levels_commits() > 0
    })?;
    anyhow::ensure!(placed, "no levels: {:?}", hp.levels_surfaces());
    Ok(hp)
}

/// Red, green and blue at (`x`, `y`) of what the screen shows.
fn rgb(hp: &mut Headless, (x, y): (i32, i32)) -> anyhow::Result<(u8, u8, u8)> {
    let (width, _, bytes) = hp.screenshot(0)?;
    let at = usize::try_from(y * width + x)? * 4;
    let [b, g, r, _] = bytes
        .get(at..at + 4)
        .and_then(|p| <[u8; 4]>::try_from(p).ok())
        .ok_or_else(|| anyhow::anyhow!("no pixel at {x}, {y}"))?;
    Ok((r, g, b))
}

fn blue((r, g, b): (u8, u8, u8)) -> bool {
    b > r.saturating_add(60) && b > g
}
fn yellow((r, g, b): (u8, u8, u8)) -> bool {
    r > b.saturating_add(80) && g > b.saturating_add(60)
}
fn green((r, g, b): (u8, u8, u8)) -> bool {
    g > r.saturating_add(40) && g > b.saturating_add(20)
}
fn amber((r, g, b): (u8, u8, u8)) -> bool {
    r > g && g > b.saturating_add(40) && r > b.saturating_add(120)
}
/// The bar's dark background or a gauge's empty track.
fn dark((r, g, b): (u8, u8, u8)) -> bool {
    r < 70 && g < 70 && b < 90
}

/// Waits until the pixel at `at` is as `like` says.
fn until(hp: &mut Headless, at: (i32, i32), like: fn((u8, u8, u8)) -> bool) -> anyhow::Result<()> {
    let mut seen = (0, 0, 0);
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(15) {
        seen = rgb(hp, at)?;
        if like(seen) {
            return Ok(());
        }
        wait(hp, Duration::from_millis(50))?;
    }
    anyhow::bail!("{at:?} shows {seen:?}")
}

#[test]
fn the_levels_show_their_services_and_follow_them() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let fakes = Fakes::start()?;
    let mut hp = start(&fakes, &shell, "fr_FR.UTF-8")?;
    // Sound 62 %, brightness 70 %, battery 82 %: their gauges' colours.
    until(&mut hp, (GAUGE_X, SOUND_Y), blue)?;
    until(&mut hp, (GAUGE_X, LIGHT_Y), yellow)?;
    until(&mut hp, (GAUGE_X, BATTERY_Y), green)?;
    // 62 % of the 102 px gauge reaches past 55 px; 30 % does not.
    let past_a_third = (FULL.x + 37 + 55, SOUND_Y);
    until(&mut hp, past_a_third, blue)?;

    // At rest: nothing drawn but the clock, once every service has
    // answered (the profile, which no pixel above shows, may come last).
    let start = std::time::Instant::now();
    loop {
        let levels = hp.levels_commits();
        wait(&mut hp, Duration::from_millis(700))?;
        if hp.levels_commits() == levels || start.elapsed() > WAIT {
            break;
        }
    }
    let (levels, bars) = (hp.levels_commits(), hp.bar_commits());
    wait(&mut hp, Duration::from_secs(2))?;
    assert_eq!(hp.levels_commits(), levels, "the levels drew at rest");
    assert_eq!(hp.bar_commits(), bars);

    // The battery runs low: amber, and only the levels' region is drawn.
    only_the_levels_drawn(&mut hp, |_, attempt| {
        fakes.set_power(|p| {
            p.percentage = 14.0 + f64::from(attempt);
            p.to_empty = 35 * 60;
        })
    })?;
    until(&mut hp, (GAUGE_X, BATTERY_Y), amber)?;
    // The volume set elsewhere, then another default sink (at 100 %).
    let sink = fakes.sink("hocus-test-sink")?.to_string();
    fakes.wpctl(&["set-volume", &sink, "0.30"])?;
    until(&mut hp, past_a_third, dark)?;
    let other = fakes.sink("hocus-test-other")?.to_string();
    fakes.wpctl(&["set-volume", &other, "1.0"])?;
    fakes.default_sink("hocus-test-other")?;
    until(&mut hp, (FULL.x + 37 + 98, SOUND_Y), blue).map_err(|err| {
        let status = fakes.wpctl(&["status"]).unwrap_or_default();
        let volume = fakes.volume("hocus-test-other");
        anyhow::anyhow!("{err}; other at {volume:?}; {status}")
    })?;
    // The bar itself was never drawn again.
    assert_eq!(hp.bar_commits(), bars);

    // The mini form: the sound's vertical gauge.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_B]);
    let placed = hp.dispatch_until(WAIT, |hp| {
        hp.levels_surfaces() == vec![("HEADLESS-1".to_owned(), MINI)]
    })?;
    assert!(placed, "{:?}", hp.levels_surfaces());
    until(&mut hp, MINI_GAUGE, blue)?;
    // At half, filled from the bottom only.
    fakes.wpctl(&["set-volume", &other, "0.5"])?;
    until(&mut hp, (MINI_GAUGE.0, MINI.y + 18), dark)?;
    assert!(blue(rgb(&mut hp, MINI_GAUGE)?));
    // Its whole width, just under its level (from 25 px down).
    for x in [MINI.x + 24, MINI.x + 29] {
        assert!(blue(rgb(&mut hp, (x, MINI.y + 40))?), "{x}");
    }
    Ok(())
}

/// Runs `change`, then checks that what the compositor drew for it is the
/// levels' surface alone. The clock ticks once a second: a try that
/// crossed a tick is tried again, with another change (its number given).
fn only_the_levels_drawn(
    hp: &mut Headless,
    mut change: impl FnMut(&mut Headless, u8) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    for attempt in 0..5 {
        wait(hp, Duration::from_millis(100))?;
        hp.take_damage(0);
        let (clocks, levels, bars) = (hp.clock_commits(), hp.levels_commits(), hp.bar_commits());
        change(hp, attempt)?;
        let drawn = hp.dispatch_until(WAIT, |hp| hp.levels_commits() > levels)?;
        anyhow::ensure!(drawn, "the levels did not draw the change");
        wait(hp, Duration::from_millis(100))?;
        assert_eq!(hp.bar_commits(), bars, "the bar was drawn again");
        if hp.clock_commits() == clocks {
            assert_eq!(hp.take_damage(0), Some(FULL));
            return Ok(());
        }
    }
    anyhow::bail!("the clock ticked through every try")
}

#[test]
fn the_keys_change_the_sound_and_the_brightness() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let fakes = Fakes::start()?;
    let mut hp = start(&fakes, &shell, "en_US.UTF-8")?;
    until(&mut hp, (GAUGE_X, SOUND_Y), blue)?;
    let volume = |hp: &mut Headless, want: (f32, bool)| -> anyhow::Result<()> {
        let mut seen = None;
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            let now = fakes.volume("hocus-test-sink")?;
            if (now.0 - want.0).abs() < 0.005 && now.1 == want.1 {
                return Ok(());
            }
            seen = Some(now);
            wait(hp, Duration::from_millis(50))?;
        }
        anyhow::bail!("volume {seen:?}, not {want:?}")
    };
    let press = |hp: &mut Headless, key: u32| {
        hp.key(key, true);
        hp.key(key, false);
    };
    press(&mut hp, KEY_VOLUMEUP);
    volume(&mut hp, (0.67, false))?;
    press(&mut hp, KEY_MUTE);
    volume(&mut hp, (0.67, true))?;
    until(&mut hp, (GAUGE_X, SOUND_Y), |(r, g, b)| {
        r.abs_diff(b) < 30 && g.abs_diff(b) < 30 && b > 60
    })?;
    // A volume key unmutes.
    press(&mut hp, KEY_VOLUMEDOWN);
    volume(&mut hp, (0.62, false))?;
    until(&mut hp, (GAUGE_X, SOUND_Y), blue)?;
    // Held, it steps on at the keyboard's rate, and stops at 100 %.
    hp.key(KEY_VOLUMEUP, true);
    wait(&mut hp, Duration::from_millis(800))?;
    hp.key(KEY_VOLUMEUP, false);
    volume(&mut hp, (1.0, false))?;
    let still = fakes.volume("hocus-test-sink")?;
    wait(&mut hp, Duration::from_millis(500))?;
    assert_eq!(
        fakes.volume("hocus-test-sink")?,
        still,
        "it went on after the key was let go"
    );

    // Brightness: 5 % of 1000 a press, through logind.
    let brightness = |hp: &mut Headless, want: u32| -> anyhow::Result<()> {
        let done = hp.dispatch_until(WAIT, |_| fakes.brightness() == Some(want))?;
        anyhow::ensure!(done, "brightness {:?}, not {want}", fakes.brightness());
        Ok(())
    };
    press(&mut hp, KEY_BRIGHTNESSUP);
    brightness(&mut hp, 750)?;
    press(&mut hp, KEY_BRIGHTNESSDOWN);
    brightness(&mut hp, 700)?;
    // Never below 5 %: the screen never goes black.
    fakes.backlight(80, 1000)?;
    wait(&mut hp, Duration::from_millis(300))?;
    press(&mut hp, KEY_BRIGHTNESSDOWN);
    brightness(&mut hp, 50)?;
    press(&mut hp, KEY_BRIGHTNESSDOWN);
    wait(&mut hp, Duration::from_millis(500))?;
    assert_eq!(fakes.brightness(), Some(50));
    let calls = fakes
        .brightness_calls
        .lock()
        .map(|c| c.clone())
        .unwrap_or_default();
    assert_eq!(
        calls.first().map(|c| (c.0.as_str(), c.1.as_str())),
        Some(("backlight", "intel_backlight"))
    );
    Ok(())
}

#[test]
fn a_missing_service_hides_its_row_until_it_comes() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // Nothing there: no bus, no PipeWire, no backlight.
    let mut fakes = Fakes::empty()?;
    let mut hp = start(&fakes, &shell, "fr_FR.UTF-8")?;
    wait(&mut hp, Duration::from_millis(800))?;
    for y in [SOUND_Y, LIGHT_Y, BATTERY_Y] {
        assert!(dark(rgb(&mut hp, (GAUGE_X, y))?), "something drawn at {y}");
    }
    // The bus and UPower come: the battery's row, alone, at the bottom.
    fakes.start_bus()?;
    fakes.start_upower()?;
    until(&mut hp, (GAUGE_X, BATTERY_Y), green)?;
    // Then the backlight, and PipeWire: their rows above it.
    fakes.backlight(700, 1000)?;
    fakes.start_pipewire()?;
    until(&mut hp, (GAUGE_X, LIGHT_Y), yellow)?;
    until(&mut hp, (GAUGE_X, SOUND_Y), blue)?;
    // UPower goes: its row and the status line go, the others close up
    // to the bottom: the brightness's row last, the sound's above it.
    fakes.stop_upower();
    let lowest = (GAUGE_X, FULL.y + 145);
    until(&mut hp, lowest, yellow)?;
    until(&mut hp, (GAUGE_X, BATTERY_Y), blue)?;
    // PipeWire goes: the sound's row goes.
    fakes.stop_pipewire();
    until(&mut hp, (GAUGE_X, BATTERY_Y), dark)?;
    assert!(yellow(rgb(&mut hp, lowest)?));
    Ok(())
}
