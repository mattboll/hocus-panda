//! Every screen's desktop switched together (docs/design/windows.md §12,
//! issue #947, variant C), headless, the compositor alone: a laptop's
//! panel (2560 × 1600 at 1.25, 2048 × 1280 logical) and a 34-inch screen
//! on its right (3440 × 1440). Linked by default: the keys, the arrows,
//! the swipe, the overview and going to a window put every screen on the
//! same number, sliding together from one start, the dots on each; a
//! window sent stays, one taken goes with every screen; the focus goes
//! to the one focused last on the pointer's screen. Per screen as #420
//! built it. The setting followed live; a screen plugged back joins the
//! link; one unplugged leaves its windows in sight. Nothing draws at
//! rest. The bar's rows (with the real shell), a drop on a tile, the
//! projects and the agents' `switch_desktop` are tested beside theirs
//! (`bar_desktops`, `desktop_drop`, `projects`, `mcp`).

use crate::common;

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    sync::mpsc,
    time::Duration,
};

use common::{
    BTN_LEFT, KEY_1, KEY_2, KEY_3, KEY_LEFT, KEY_LEFTALT, KEY_LEFTCTRL, KEY_LEFTMETA,
    KEY_LEFTSHIFT, KEY_RIGHT, KEY_TAB, WAIT, chord, open, wait, wait_for_rect,
};
use hocus_compositor::{Config, DesktopSwitching, Headless, HeadlessConfig, RendererChoice};
use serde_json::Value;

const LAPTOP: &str = "HEADLESS-1";
const WIDE: &str = "HEADLESS-2";
const ON_LAPTOP: (f64, f64) = (1024.0, 640.0);
const ON_WIDE: (f64, f64) = (2048.0 + 1720.0, 720.0);
const KEY_4: u32 = 5;
const KEY_W: u32 = 17;

fn screens_with(config: Config) -> anyhow::Result<Headless> {
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec!["2560x1600@1.25".parse()?, "3440x1440".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    point(&mut hp, ON_LAPTOP);
    Ok(hp)
}

/// The two screens, linked as by default.
fn linked() -> anyhow::Result<Headless> {
    screens_with(Config::default())
}

fn per_screen_config() -> Config {
    let mut config = Config::default();
    config.desktops.switching = DesktopSwitching::PerScreen;
    config
}

fn point(hp: &mut Headless, (x, y): (f64, f64)) {
    hp.pointer_move(x, y);
}

/// The desktop each screen shows (from 0), the laptop's first.
fn shown(hp: &Headless) -> Vec<usize> {
    hp.strips().into_iter().map(|(_, _, c)| c).collect()
}

/// Runs until every screen is still on desktop `index`.
fn all_on(hp: &mut Headless, index: usize) -> anyhow::Result<()> {
    let still = hp.dispatch_until(WAIT, |hp| {
        hp.strip_looks()
            .iter()
            .all(|s| s.current == index && (s.position - index as f64).abs() < 1e-9)
    })?;
    anyhow::ensure!(still, "never all on {}: {:?}", index + 1, hp.strip_looks());
    Ok(())
}

/// A window's screen, its desktop (from 0), and whether it shows.
fn where_is(hp: &Headless, app_id: &str) -> Option<(String, usize, bool)> {
    hp.placements()
        .into_iter()
        .find(|p| p.app_id.as_deref() == Some(app_id))
        .and_then(|p| Some((p.output?, p.desktop, p.shown)))
}

fn focused(hp: &Headless) -> Option<String> {
    hp.windows()
        .into_iter()
        .find(|w| w.focused && w.keyboard)
        .and_then(|w| w.app_id)
}

/// The control socket's events of `kinds`, as a subscriber hears them.
fn subscribe(hp: &mut Headless, kinds: &str) -> anyhow::Result<mpsc::Receiver<Value>> {
    let socket = hp
        .control_socket()
        .ok_or_else(|| anyhow::anyhow!("no control socket"))?
        .to_path_buf();
    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(format!("msg subscribe {kinds}\n").as_bytes())?;
    let (told, events) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            let Ok(event) = serde_json::from_str::<Value>(&line) else {
                break;
            };
            if told.send(event).is_err() {
                break;
            }
        }
    });
    let there = hp.dispatch_until(WAIT, |hp| hp.subscribers() == 1)?;
    anyhow::ensure!(there, "not subscribed");
    Ok(events)
}

/// Scenario 1: without `per-screen` in the file, `Super+2` with the
/// pointer on the laptop puts both screens on desktop 2, and the
/// subscribers are told once per screen.
#[test]
fn by_default_super_2_puts_every_screen_on_desktop_2_told_once_each() -> anyhow::Result<()> {
    let mut hp = linked()?;
    assert_eq!(
        hp.state().config.desktops.switching,
        DesktopSwitching::Together
    );
    let events = subscribe(&mut hp, "desktop")?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    assert_eq!(shown(&hp), [1, 1]);
    let switched = |e: Value| {
        (e["event"] == "desktop-switched").then(|| {
            (
                e["screen"]["name"].as_str().unwrap_or_default().to_owned(),
                e["desktop"]["number"].as_u64().unwrap_or_default(),
            )
        })
    };
    let mut told: Vec<(String, u64)> = Vec::new();
    hp.dispatch_until(WAIT, |_| {
        told.extend(events.try_iter().filter_map(switched));
        told.len() >= 2
    })?;
    wait(&mut hp, Duration::from_millis(100))?;
    told.extend(events.try_iter().filter_map(switched));
    told.sort();
    assert_eq!(told, [(LAPTOP.to_owned(), 2), (WIDE.to_owned(), 2)]);
    Ok(())
}

/// Scenario 2: per screen, `Super+2` switches the screen under the
/// pointer alone.
#[test]
fn per_screen_super_2_switches_the_screen_under_the_pointer_alone() -> anyhow::Result<()> {
    let mut hp = screens_with(per_screen_config())?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    assert_eq!(shown(&hp), [1, 0]);
    Ok(())
}

/// Scenario 3: on desktop 4, `Super+Ctrl+→` switches nothing;
/// `Super+Ctrl+←` puts both on 3.
#[test]
fn the_arrows_move_every_screen_and_stop_at_the_last() -> anyhow::Result<()> {
    let mut hp = linked()?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_4]);
    all_on(&mut hp, 3)?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTCTRL, KEY_RIGHT]);
    assert_eq!(shown(&hp), [3, 3]);
    point(&mut hp, ON_WIDE);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTCTRL, KEY_LEFT]);
    assert_eq!(shown(&hp), [2, 2]);
    Ok(())
}

/// Scenario 4: three fingers left by a third of a desktop, slowly, then
/// lifted: both strips go on to desktop 2; by a tenth, both spring back
/// to 1. No swipe reached a client.
#[test]
fn a_swipe_moves_every_strip_and_they_complete_or_spring_back_together() -> anyhow::Result<()> {
    let mut hp = linked()?;
    open(&mut hp, "test.laptop", (500, 400))?;
    // A third of a desktop's 360 units, slowly: no flick.
    hp.swipe(3, &[(-12.0, 0.0); 10], 100);
    all_on(&mut hp, 1)?;
    assert_eq!(
        where_is(&hp, "test.laptop"),
        Some((LAPTOP.to_owned(), 0, false))
    );
    // A tenth, back toward 1: both spring back to 2.
    hp.swipe(3, &[(9.0, 0.0); 4], 100);
    all_on(&mut hp, 1)?;
    // While the fingers move, both strips follow them, at the same place.
    hp.swipe_begin(3);
    for _ in 0..5 {
        hp.swipe_move(12.0, 0.0, 100);
    }
    let looks = hp.strip_looks();
    assert!(
        looks[0].position < 1.0 && (looks[0].position - looks[1].position).abs() < 1e-9,
        "{looks:?}"
    );
    hp.swipe_end(100, true);
    all_on(&mut hp, 1)?;
    assert_eq!(hp.swipes_given(), (0, 0));
    Ok(())
}

/// Scenario 5 (headless, on the held clock: both screens' frames are
/// drawn at the same times here, the curve is what they share): halfway,
/// both strips are at the same share of their travel; both end at 320 ms.
#[test]
fn every_strip_slides_from_the_same_start_and_ends_at_320_ms() -> anyhow::Result<()> {
    let mut hp = linked()?;
    hp.hold_clock();
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    hp.step(Duration::from_millis(160))?;
    let looks = hp.strip_looks();
    assert!(
        looks[0].position > 0.0 && looks[0].position < 1.0,
        "{looks:?}"
    );
    assert!(
        (looks[0].position - looks[1].position).abs() < 1e-9,
        "{looks:?}"
    );
    hp.step(Duration::from_millis(160))?;
    let looks = hp.strip_looks();
    assert!(
        looks
            .iter()
            .all(|s| (s.position - 1.0).abs() < 1e-9 && !s.moving),
        "{looks:?}"
    );
    Ok(())
}

/// Scenario 6: the dots show on both screens and fade together.
#[test]
fn the_dots_show_on_every_screen_and_fade_together() -> anyhow::Result<()> {
    let mut hp = linked()?;
    hp.hold_clock();
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    hp.step(Duration::from_millis(100))?;
    let looks = hp.strip_looks();
    assert!(looks[0].dots > 0.0, "{looks:?}");
    assert_eq!(looks[0].dots, looks[1].dots, "{looks:?}");
    // The slide's 320 ms over at the next frame; then the dots' stay and
    // half their fade.
    hp.step(Duration::from_millis(300))?;
    hp.step(Duration::from_millis(40 + 150))?;
    let looks = hp.strip_looks();
    assert!(looks[0].dots > 0.0 && looks[0].dots < 1.0, "{looks:?}");
    assert_eq!(looks[0].dots, looks[1].dots, "{looks:?}");
    hp.step(Duration::from_millis(200))?;
    assert!(
        hp.strip_looks().iter().all(|s| s.dots == 0.0),
        "{:?}",
        hp.strip_looks()
    );
    Ok(())
}

/// Scenario 7: `Super+Shift+3` sends the laptop's focused window to its
/// desktop 3; both screens still show 1.
#[test]
fn a_window_sent_goes_alone_and_nothing_switches() -> anyhow::Result<()> {
    let mut hp = linked()?;
    open(&mut hp, "test.sent", (500, 400))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_3]);
    assert_eq!(
        where_is(&hp, "test.sent"),
        Some((LAPTOP.to_owned(), 2, false))
    );
    assert_eq!(shown(&hp), [0, 0]);
    Ok(())
}

/// Scenario 8: `Super+Ctrl+Shift+3` takes it to the laptop's desktop 3,
/// and both screens go there with it.
#[test]
fn a_window_taken_goes_and_every_screen_with_it() -> anyhow::Result<()> {
    let mut hp = linked()?;
    open(&mut hp, "test.taken", (500, 400))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTCTRL, KEY_LEFTSHIFT, KEY_3]);
    assert_eq!(
        where_is(&hp, "test.taken"),
        Some((LAPTOP.to_owned(), 2, true))
    );
    assert_eq!(shown(&hp), [2, 2]);
    assert_eq!(focused(&hp).as_deref(), Some("test.taken"));
    Ok(())
}

/// Scenario 9: on desktop 1, Alt+Tab picks a window on the 34-inch
/// screen's desktop 3: both screens show 3 and it has the keyboard.
#[test]
fn alt_tab_to_a_window_of_another_desktop_switches_every_screen() -> anyhow::Result<()> {
    let mut hp = linked()?;
    point(&mut hp, ON_WIDE);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_3]);
    open(&mut hp, "test.far", (900, 600))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_1]);
    all_on(&mut hp, 0)?;
    point(&mut hp, ON_LAPTOP);
    open(&mut hp, "test.near", (500, 400))?;
    hp.key(KEY_LEFTALT, true);
    hp.key(KEY_TAB, true);
    hp.key(KEY_TAB, false);
    wait(&mut hp, Duration::from_millis(50))?;
    hp.key(KEY_LEFTALT, false);
    wait(&mut hp, Duration::from_millis(100))?;
    assert_eq!(shown(&hp), [2, 2]);
    assert_eq!(focused(&hp).as_deref(), Some("test.far"));
    Ok(())
}

/// Scenario 10: with the pointer on the 34-inch screen, `Super+2` gives
/// the keyboard to the window focused last on its desktop 2, though the
/// laptop's desktop 2 has one too, focused after it.
#[test]
fn the_focus_goes_to_the_window_focused_last_on_the_pointers_screen() -> anyhow::Result<()> {
    let mut hp = linked()?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    point(&mut hp, ON_WIDE);
    open(&mut hp, "test.wide", (900, 600))?;
    point(&mut hp, ON_LAPTOP);
    open(&mut hp, "test.laptop", (500, 400))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_1]);
    all_on(&mut hp, 0)?;
    point(&mut hp, ON_WIDE);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    assert_eq!(shown(&hp), [1, 1]);
    assert_eq!(focused(&hp).as_deref(), Some("test.wide"));
    // The pointer did not move.
    assert_eq!(hp.pointer(), ON_WIDE);
    Ok(())
}

/// Scenario 11: per screen, the laptop on 1 and the 34-inch on 3, the
/// pointer on the laptop: the file says `per-screen = false`, the 34-inch
/// slides to 1, the laptop does not move.
#[test]
fn linked_from_the_file_the_other_screens_slide_to_the_pointers() -> anyhow::Result<()> {
    let mut hp = screens_with(per_screen_config())?;
    point(&mut hp, ON_WIDE);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_3]);
    point(&mut hp, ON_LAPTOP);
    let wide = hp.dispatch_until(WAIT, |hp| hp.strip_looks().iter().all(|s| !s.moving))?;
    anyhow::ensure!(wide, "{:?}", hp.strip_looks());
    assert_eq!(shown(&hp), [0, 2]);
    hp.hold_clock();
    hp.reconfigure(Config::default());
    hp.step(Duration::from_millis(100))?;
    let looks = hp.strip_looks();
    assert_eq!(looks[0].position, 0.0, "{looks:?}");
    assert!(!looks[0].moving, "{looks:?}");
    assert!(
        looks[1].moving && looks[1].position > 0.0 && looks[1].position < 2.0,
        "{looks:?}"
    );
    hp.step(Duration::from_millis(300))?;
    assert_eq!(shown(&hp), [0, 0]);
    // From now on, linked.
    hp.release_clock();
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    assert_eq!(shown(&hp), [1, 1]);
    // Unlinked again: nothing moves.
    hp.reconfigure(per_screen_config());
    assert_eq!(shown(&hp), [1, 1]);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_1]);
    assert_eq!(shown(&hp), [0, 1]);
    Ok(())
}

/// Scenario 17: in the overview, a click on the laptop's thumbnail 3
/// shows desktop 3 on both screens, the overview staying open.
#[test]
fn a_thumbnail_clicked_in_the_overview_shows_its_number_on_every_screen() -> anyhow::Result<()> {
    let mut hp = linked()?;
    open(&mut hp, "test.laptop", (500, 400))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_W]);
    anyhow::ensure!(hp.overview() == Some(true), "not open");
    wait(&mut hp, Duration::from_millis(300))?;
    let at = hp
        .overview_thumbs()
        .into_iter()
        .find(|(o, d, _)| o == LAPTOP && *d == 2)
        .map(|(.., at)| at)
        .ok_or_else(|| anyhow::anyhow!("no thumbnail: {:?}", hp.overview_thumbs()))?;
    hp.pointer_move(at.0, at.1);
    hp.button(BTN_LEFT, true);
    hp.button(BTN_LEFT, false);
    wait(&mut hp, Duration::from_millis(50))?;
    assert_eq!(hp.overview(), Some(true), "it stays open");
    assert_eq!(shown(&hp), [2, 2]);
    Ok(())
}

/// Scenario 18: linked on 1, the 34-inch unplugged with its window on its
/// desktop 1; the laptop goes to 3; plugged back, it shows 3 too, its
/// window back on its desktop 1 at its place.
#[test]
fn a_screen_plugged_back_shows_the_number_the_others_show() -> anyhow::Result<()> {
    let mut hp = linked()?;
    point(&mut hp, ON_WIDE);
    let place = open(&mut hp, "test.wide", (900, 600))?;
    point(&mut hp, ON_LAPTOP);
    hp.unplug(WIDE)?;
    wait(&mut hp, Duration::from_millis(100))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_3]);
    assert_eq!(shown(&hp), [2]);
    hp.plug(WIDE)?;
    let back = hp.dispatch_until(WAIT, |hp| {
        where_is(hp, "test.wide") == Some((WIDE.to_owned(), 0, false))
    })?;
    anyhow::ensure!(back, "{:?}", where_is(&hp, "test.wide"));
    assert_eq!(shown(&hp), [2, 2]);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_1]);
    wait_for_rect(&mut hp, "test.wide", place)?;
    Ok(())
}

/// Scenario 19: linked on 2, the 34-inch unplugged: its desktop 2's window
/// is on the laptop's desktop 2, in sight.
#[test]
fn a_screen_unplugged_leaves_its_shown_windows_in_sight() -> anyhow::Result<()> {
    let mut hp = linked()?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    point(&mut hp, ON_WIDE);
    open(&mut hp, "test.wide", (900, 600))?;
    hp.unplug(WIDE)?;
    wait(&mut hp, Duration::from_millis(100))?;
    assert_eq!(
        where_is(&hp, "test.wide"),
        Some((LAPTOP.to_owned(), 1, true))
    );
    Ok(())
}

/// Scenario 26: with reduced motion, `Super+2` cross-fades both screens
/// in 120 ms, nothing sliding.
#[test]
fn with_reduced_motion_every_screen_cross_fades_together() -> anyhow::Result<()> {
    let mut config = Config::default();
    config.appearance.reduced_motion = true;
    let mut hp = screens_with(config)?;
    hp.hold_clock();
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    hp.step(Duration::from_millis(60))?;
    let looks = hp.strip_looks();
    assert!(
        looks.iter().all(|s| s.moving && s.position == 1.0),
        "{looks:?}"
    );
    hp.step(Duration::from_millis(70))?;
    assert!(
        hp.strip_looks().iter().all(|s| !s.moving),
        "{:?}",
        hp.strip_looks()
    );
    Ok(())
}

/// Scenario 27: linked and nothing switching, no screen draws a frame in
/// half a second.
#[test]
fn at_rest_no_screen_draws() -> anyhow::Result<()> {
    let mut hp = linked()?;
    open(&mut hp, "test.laptop", (500, 400))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    all_on(&mut hp, 1)?;
    wait(&mut hp, Duration::from_millis(800))?;
    let before = (hp.frames_on(0), hp.frames_on(1));
    wait(&mut hp, Duration::from_millis(500))?;
    assert_eq!((hp.frames_on(0), hp.frames_on(1)), before);
    Ok(())
}

// --- The bar, with the real shell ------------------------------------------------------

/// The two screens with the real shell on the CPU, in French, away from
/// the machine's services, its tiles told.
fn with_shell(dir: &std::path::Path) -> anyhow::Result<Headless> {
    let at = |sub: &str| -> anyhow::Result<String> {
        let path = dir.join(sub);
        std::fs::create_dir_all(&path)?;
        Ok(path.display().to_string())
    };
    let env = [
        ("HOME", at("home")?),
        ("XDG_CONFIG_HOME", at("config")?),
        ("XDG_CACHE_HOME", at("cache")?),
        ("XDG_STATE_HOME", at("state")?),
        ("XDG_DATA_HOME", at("data")?),
        ("LC_ALL", "fr_FR.UTF-8".to_owned()),
        ("LANG", "fr_FR.UTF-8".to_owned()),
    ];
    let script = common::shell_script(dir, &common::shell_program(), &env)?;
    let mut config = Config::default();
    config.panel.form = hocus_compositor::FormChoice::Full;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["2560x1600@1.25".parse()?, "3440x1440".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
        Some(script),
    )?;
    let told = hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.desk_tile_rects().len() == 8
    })?;
    anyhow::ensure!(told, "tiles: {:?}", hp.desk_tile_rects());
    point(&mut hp, ON_LAPTOP);
    Ok(hp)
}

/// The middle of `output`'s tile `desktop` (from 0) in the laptop's bar.
fn tile(hp: &Headless, output: &str, desktop: usize) -> anyhow::Result<(f64, f64)> {
    hp.desk_tile_rects()
        .into_iter()
        .find(|(bar, o, d, _)| bar == LAPTOP && o == output && *d == desktop)
        .map(|(.., (x, y, w, h))| (x + w / 2.0, y + h / 2.0))
        .ok_or_else(|| anyhow::anyhow!("no tile {desktop} of {output}"))
}

/// The desktop each screen's row rings, as the bar is told it.
fn ringed(hp: &Headless) -> Vec<Option<u8>> {
    [LAPTOP, WIDE]
        .iter()
        .map(|o| hp.desktops_view(o).map(|d| d.current))
        .collect()
}

/// Scenarios 13 and 14: linked, the laptop's bar keeps a row per screen,
/// its own first, its tiles taller; a click on the 34-inch's tile 3 puts
/// both screens on 3, both rows ringing tile 3.
#[test]
fn the_bar_keeps_a_row_per_screen_and_a_tile_of_either_switches_every_screen() -> anyhow::Result<()>
{
    let dir = tempfile::tempdir()?;
    let mut hp = with_shell(dir.path())?;
    let rows: Vec<String> = hp
        .state()
        .desk_rows(LAPTOP)
        .iter()
        .map(smithay::output::Output::name)
        .collect();
    assert_eq!(rows, [LAPTOP, WIDE]);
    let height = |output: &str| {
        hp.desk_tile_rects()
            .into_iter()
            .find(|(_, o, ..)| o == output)
            .map(|(.., (_, _, _, h))| h)
            .unwrap_or_default()
    };
    let (own, other) = (height(LAPTOP), height(WIDE));
    assert!(own > other, "{own} {other}");
    let at = tile(&hp, WIDE, 2)?;
    common::click_on(&mut hp, at, hocus_link::DESKTOPS_NAMESPACE)?;
    let both = hp.dispatch_until(WAIT, |hp| shown(hp) == [2, 2])?;
    anyhow::ensure!(both, "{:?}", shown(&hp));
    let rung = hp.dispatch_until(WAIT, |hp| ringed(hp) == [Some(2), Some(2)])?;
    anyhow::ensure!(rung, "{:?}", ringed(&hp));
    Ok(())
}

/// Scenario 15: linked on 1, the wheel turned a notch down over the
/// laptop's row puts both screens on 2.
#[test]
fn the_wheel_over_a_row_switches_every_screen() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = with_shell(dir.path())?;
    let at = tile(&hp, LAPTOP, 0)?;
    common::over(&mut hp, at, hocus_link::DESKTOPS_NAMESPACE)?;
    hp.wheel(1);
    let both = hp.dispatch_until(WAIT, |hp| shown(hp) == [1, 1])?;
    anyhow::ensure!(both, "{:?}", shown(&hp));
    Ok(())
}
