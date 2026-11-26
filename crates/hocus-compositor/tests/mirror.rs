//! Mirroring (#931, docs/design/windows.md §12 "Mirroring"), headless and
//! the compositor alone: a laptop of 2560 × 1600 at 1.25 and a TV of
//! 1920 × 1080 plugged beside it. A set of screens never seen lights
//! extended; mirrored, the TV leaves the desktop as an unplugged screen
//! does and shows the laptop's picture fitted, black bands either side;
//! `Super+Shift+P` and the display key toggle it; each set keeps its
//! choice in the state file; the lid, an unplug, the lock, an agent's
//! list of screens and the frames at rest follow the spec's scenarios.

use crate::common;

use std::{path::Path, sync::mpsc, time::Duration};

use common::{KEY_2, KEY_LEFTMETA, KEY_LEFTSHIFT, WAIT, WINDOW_COLOUR, chord, client, open, wait};
use hocus_compositor::{
    Authenticator, Config, Headless, HeadlessConfig, OutputSpec, Placed, RendererChoice,
    ScreenIdentity, model::Rect,
};
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::wl_registry::WlRegistry,
};

const LAPTOP: &str = "HEADLESS-1";
const TV: &str = "HDMI-A-1";
const KEY_P: u32 = 25;
/// `XF86Display`'s key (Linux's `KEY_SWITCHVIDEOMODE`).
const KEY_DISPLAY: u32 = 227;

fn tv() -> OutputSpec {
    // 55 inches.
    OutputSpec::new(1920, 1080).physical_size(1218, 685)
}

fn tv_identity() -> ScreenIdentity {
    ScreenIdentity::new("SAM", "QN55", "0001")
}

/// The laptop alone, its screen the built-in panel, the sets kept in
/// `dir`.
fn laptop(dir: &Path) -> anyhow::Result<Headless> {
    let mut config = Config::default();
    config.keyboard.layouts = vec!["us".to_owned()];
    config.plain_background = true;
    config.state_dir = Some(dir.join("state"));
    config.lock_auth = Authenticator::test("right");
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec![
                OutputSpec::new(2560, 1600)
                    .physical_size(302, 189)
                    .scale(1.25),
            ],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    hp.set_internal(LAPTOP);
    let drawn = hp.dispatch_until(WAIT, |hp| hp.frames() > 0)?;
    anyhow::ensure!(drawn, "nothing drawn");
    Ok(hp)
}

/// The TV plugged, extended, laid out.
fn plug_tv(hp: &mut Headless) -> anyhow::Result<()> {
    hp.plug_screen(TV, tv(), Some(tv_identity()))?;
    let laid = hp.dispatch_until(WAIT, |hp| hp.outputs().len() == 2)?;
    anyhow::ensure!(laid, "the TV never lit: {:?}", hp.outputs());
    wait(hp, Duration::from_millis(50))
}

fn placements_of(hp: &Headless, app_id: &str) -> Option<Placed> {
    hp.placements()
        .into_iter()
        .find(|p| p.app_id.as_deref() == Some(app_id))
}

fn mirrored(hp: &Headless) -> Option<(String, Vec<String>)> {
    hp.mirroring()
}

/// The index of output `name` in the backend's order (by name).
fn index_of(hp: &Headless, name: &str) -> usize {
    let mut names: Vec<String> = vec![LAPTOP.to_owned()];
    if hp.state().displays_json()["outputs"]
        .as_array()
        .is_some_and(|o| o.iter().any(|o| o["name"] == TV && o["connected"] == true))
    {
        names.push(TV.to_owned());
    }
    names.sort();
    names.iter().position(|n| n == name).unwrap_or(0)
}

/// The colour (R, G, B) of pixel (`x`, `y`) of output `name`.
fn pixel(hp: &mut Headless, name: &str, x: i32, y: i32) -> anyhow::Result<[u8; 3]> {
    let index = index_of(hp, name);
    let (w, _, px) = hp.screenshot(index)?;
    let at = ((y * w + x) * 4) as usize;
    let p = px
        .get(at..at + 4)
        .ok_or_else(|| anyhow::anyhow!("({x}, {y}) is off {name}"))?;
    Ok([p[2], p[1], p[0]])
}

/// How many `wl_output` globals a client connecting now is told of.
fn outputs_seen(hp: &mut Headless) -> anyhow::Result<usize> {
    struct Counting;
    impl Dispatch<WlRegistry, GlobalListContents> for Counting {
        fn event(
            _: &mut Self,
            _: &WlRegistry,
            _: <WlRegistry as wayland_client::Proxy>::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    let stream = std::os::unix::net::UnixStream::connect(hp.socket())?;
    let (said, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let counted = Connection::from_socket(stream)
            .map_err(anyhow::Error::from)
            .and_then(|conn| Ok(registry_queue_init::<Counting>(&conn)?))
            .map(|(globals, _)| {
                globals
                    .contents()
                    .with_list(|list| list.iter().filter(|g| g.interface == "wl_output").count())
            });
        let _ = said.send(counted.map_err(|e| e.to_string()));
    });
    let mut seen = None;
    hp.dispatch_until(WAIT, |_| {
        seen = heard.try_recv().ok();
        seen.is_some()
    })?;
    seen.ok_or_else(|| anyhow::anyhow!("the client never heard the globals"))?
        .map_err(|e| anyhow::anyhow!(e))
}

/// Acceptance 1: plugged, the TV extends the desktop to the laptop's
/// right, and clients see two screens.
#[test]
fn a_tv_plugged_extends_the_desktop() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = laptop(dir.path())?;
    plug_tv(&mut hp)?;
    assert_eq!(
        hp.outputs(),
        vec![
            (LAPTOP.to_owned(), Rect::new(0, 0, 2048, 1280)),
            (TV.to_owned(), Rect::new(2048, 0, 1920, 1080)),
        ]
    );
    assert_eq!(outputs_seen(&mut hp)?, 2);
    assert_eq!(mirrored(&hp), None);
    Ok(())
}

/// Acceptance 3, the compositor's side: mirroring asked (the bubble's
/// « Recopier »), clients see one `wl_output`, the TV's global removed.
#[test]
fn mirrored_clients_see_one_screen() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = laptop(dir.path())?;
    plug_tv(&mut hp)?;
    hp.mirror(true);
    wait(&mut hp, Duration::from_millis(50))?;
    assert_eq!(
        mirrored(&hp),
        Some((LAPTOP.to_owned(), vec![TV.to_owned()]))
    );
    assert_eq!(hp.outputs().len(), 1, "the TV is no screen for the desktop");
    assert_eq!(outputs_seen(&mut hp)?, 1);
    Ok(())
}

/// Acceptance 6: the laptop's picture fitted on the TV, black bands left
/// and right, a window's colour where the window is, scaled 0.675; the
/// TV keeps its mode.
#[test]
fn the_picture_is_fitted_with_black_bands() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = laptop(dir.path())?;
    plug_tv(&mut hp)?;
    hp.pointer_move(1024.0, 640.0);
    open(&mut hp, "test.mirrored", (400, 400))?;
    let id = hp
        .windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some("test.mirrored"))
        .map(|w| w.id)
        .ok_or_else(|| anyhow::anyhow!("no window"))?;
    hp.place_window(id, Rect::new(0, 0, 400, 400));
    let placed = hp.dispatch_until(WAIT, |hp| {
        common::rect_of(hp, "test.mirrored") == Some(Rect::new(0, 0, 400, 400))
    })?;
    assert!(
        placed,
        "the window at {:?}",
        common::rect_of(&hp, "test.mirrored")
    );
    hp.mirror(true);
    // Away from the window, so that the cursor is not over what is read.
    hp.pointer_move(2000.0, 1250.0);
    // The window opened and moved: its motions over, so that both screens'
    // last frames show the same picture.
    common::at_rest(&mut hp)?;
    wait(&mut hp, Duration::from_millis(100))?;
    assert_eq!(
        hp.mirror_fitted(TV),
        Some((LAPTOP.to_owned(), Rect::new(96, 0, 1728, 1080)))
    );
    let shot = |hp: &mut Headless, name: &str| -> anyhow::Result<(i32, Vec<u8>)> {
        let (w, _, px) = hp.screenshot(index_of(hp, name))?;
        Ok((w, px))
    };
    let at = |(w, px): &(i32, Vec<u8>), x: i32, y: i32| -> [u8; 3] {
        let i = ((y * w + x) * 4) as usize;
        px.get(i..i + 3).map_or([255; 3], |p| [p[2], p[1], p[0]])
    };
    let tv = shot(&mut hp, TV)?;
    let lap = shot(&mut hp, LAPTOP)?;
    for y in [0, 300, 540, 1079] {
        for x in [0, 50, 95, 1824, 1900, 1919] {
            assert_eq!(at(&tv, x, y), [0, 0, 0], "a band at ({x}, {y})");
        }
    }
    // The laptop's pixels, scaled 0.675, from the band's edge: the
    // window's top-left 400 × 400 (500 × 500 pixels) fills (96, 0) to
    // (433, 337), within a pixel; linear filtering blends a little.
    let near = |a: [u8; 3], b: [u8; 3]| a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 6);
    let colour = {
        let c = WINDOW_COLOUR.to_be_bytes();
        [c[1], c[2], c[3]]
    };
    for (x, y) in [(250, 250), (60, 120), (480, 480), (490, 260)] {
        let tx = 96 + (f64::from(x) * 0.675).round() as i32;
        let ty = (f64::from(y) * 0.675).round() as i32;
        assert!(
            near(at(&tv, tx, ty), at(&lap, x, y)),
            "the laptop's ({x}, {y}) {:?} at the TV's ({tx}, {ty}) {:?}",
            at(&lap, x, y),
            at(&tv, tx, ty)
        );
    }
    // The window's blue (a shade of the test's own, as drawn), not the
    // background.
    let blue = at(&tv, 96 + 169, 169);
    assert!(
        blue[2] > 150 && near(blue, at(&lap, 250, 250)),
        "the window's colour {blue:?}, not {colour:?}"
    );
    // Its right and bottom edges, a pixel in and a few out.
    for ((tx, ty), (x, y)) in [
        ((431, 169), (497, 250)),
        ((436, 169), (505, 250)),
        ((265, 335), (250, 497)),
        ((265, 340), (250, 505)),
    ] {
        assert!(
            near(at(&tv, tx, ty), at(&lap, x, y)),
            "the laptop's ({x}, {y}) {:?} at the TV's ({tx}, {ty}) {:?}",
            at(&lap, x, y),
            at(&tv, tx, ty)
        );
    }
    let modes = hp.state().displays_json();
    let tv_mode = modes["outputs"]
        .as_array()
        .and_then(|o| o.iter().find(|o| o["name"] == TV))
        .map(|o| o["mode"].clone());
    assert_eq!(tv_mode, Some(serde_json::json!([1920, 1080, 60000])));
    Ok(())
}

/// Acceptance 7: the pointer at the laptop's middle is drawn at the TV's,
/// and it cannot go onto the TV.
#[test]
fn the_pointer_shows_on_the_tv_and_stays_on_the_laptop() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = laptop(dir.path())?;
    plug_tv(&mut hp)?;
    hp.mirror(true);
    hp.pointer_move(1024.0, 640.0);
    wait(&mut hp, Duration::from_millis(100))?;
    // The arrow's hotspot is its tip: drawn there and below to its right,
    // light on the background's dark; nothing of it above or left.
    let index = index_of(&hp, TV);
    let (w, _, px) = hp.screenshot(index)?;
    let at = |x: i32, y: i32| {
        let i = ((y * w + x) * 4) as usize;
        px.get(i..i + 3).map(|p| [p[2], p[1], p[0]])
    };
    let background = at(900, 500);
    let lit = |x0: i32, y0: i32| {
        (0..12)
            .flat_map(|dy| (0..12).map(move |dx| (x0 + dx, y0 + dy)))
            .filter(|(x, y)| at(*x, *y) != background)
            .count()
    };
    assert!(lit(960, 540) > 10, "no cursor at (960, 540)");
    assert_eq!(lit(944, 524), 0, "the cursor drawn above its hotspot");
    // Pushed right past the laptop's edge, it stays on the laptop.
    hp.pointer_by(800.0, 0.0);
    let (x, _) = hp.pointer();
    assert!(x < 2048.0, "the pointer went off the laptop: {x}");
    Ok(())
}

/// Acceptance 8: a window on the TV's desktop 2 goes to the laptop's
/// desktop 2 as mirroring starts, and back to the TV at its place as it
/// stops.
#[test]
fn windows_leave_the_tv_and_come_back() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = laptop(dir.path())?;
    plug_tv(&mut hp)?;
    hp.pointer_move(2048.0 + 960.0, 540.0);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    wait(&mut hp, Duration::from_millis(50))?;
    let before = open(&mut hp, "test.on.tv", (600, 400))?;
    let placed =
        |hp: &Headless| placements_of(hp, "test.on.tv").map(|p| (p.output, p.desktop, p.rect));
    assert_eq!(placed(&hp), Some((Some(TV.to_owned()), 1, before)));
    hp.mirror(true);
    wait(&mut hp, Duration::from_millis(50))?;
    let on_laptop = placed(&hp);
    assert_eq!(
        on_laptop.as_ref().map(|p| (p.0.clone(), p.1)),
        Some((Some(LAPTOP.to_owned()), 1))
    );
    hp.mirror(false);
    let back = hp.dispatch_until(WAIT, |hp| {
        placed(hp) == Some((Some(TV.to_owned()), 1, before))
    })?;
    assert!(back, "not back on the TV: {:?}", placed(&hp));
    Ok(())
}

/// Acceptance 9 and 12: `Super+Shift+P` mirrors, the laptop shown
/// whichever screen the pointer is on; again, extended, the TV right of
/// the laptop, showing the laptop's desktop.
#[test]
fn super_shift_p_toggles_mirroring() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = laptop(dir.path())?;
    plug_tv(&mut hp)?;
    hp.pointer_move(2048.0 + 960.0, 540.0);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_P]);
    wait(&mut hp, Duration::from_millis(50))?;
    assert_eq!(
        mirrored(&hp),
        Some((LAPTOP.to_owned(), vec![TV.to_owned()]))
    );
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_P]);
    wait(&mut hp, Duration::from_millis(50))?;
    assert_eq!(mirrored(&hp), None);
    assert_eq!(
        hp.outputs(),
        vec![
            (LAPTOP.to_owned(), Rect::new(0, 0, 2048, 1280)),
            (TV.to_owned(), Rect::new(2048, 0, 1920, 1080)),
        ]
    );
    let desktops = |hp: &Headless, name: &str| hp.desktops_view(name).map(|d| d.current);
    assert!(desktops(&hp, LAPTOP).is_some());
    assert_eq!(desktops(&hp, TV), desktops(&hp, LAPTOP));
    Ok(())
}

/// Acceptance 10: the display key mirrors.
#[test]
fn the_display_key_mirrors() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = laptop(dir.path())?;
    plug_tv(&mut hp)?;
    chord(&mut hp, &[KEY_DISPLAY]);
    wait(&mut hp, Duration::from_millis(50))?;
    assert_eq!(
        mirrored(&hp),
        Some((LAPTOP.to_owned(), vec![TV.to_owned()]))
    );
    Ok(())
}

/// Acceptance 11: with one screen, the shortcut does nothing.
#[test]
fn one_screen_mirrors_nothing() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = laptop(dir.path())?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_P]);
    wait(&mut hp, Duration::from_millis(50))?;
    assert_eq!(mirrored(&hp), None);
    assert_eq!(hp.outputs().len(), 1);
    Ok(())
}

/// The state file's line of the set of the laptop and the TV.
fn kept(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("state").join("screen-sets")).unwrap_or_default()
}

/// Acceptance 13: the TV unplugged while mirrored moves nothing; plugged
/// again it mirrors in the same turn, no bubble, the set kept mirrored
/// on the laptop in the state file.
#[test]
fn a_set_mirrored_is_remembered() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = laptop(dir.path())?;
    plug_tv(&mut hp)?;
    let window = open(&mut hp, "test.stays", (500, 300))?;
    hp.mirror(true);
    wait(&mut hp, Duration::from_millis(50))?;
    let key = hp.screen_key(LAPTOP);
    let written = hp.dispatch_until(WAIT, |_| {
        kept(dir.path()).contains(&format!("mirror = \"{key}\""))
    })?;
    assert!(written, "the state file: {}", kept(dir.path()));
    hp.unplug(TV)?;
    wait(&mut hp, Duration::from_millis(50))?;
    assert_eq!(common::rect_of(&hp, "test.stays"), Some(window));
    assert!(!hp.screens_peek_out());
    hp.plug_screen(TV, tv(), Some(tv_identity()))?;
    hp.dispatch(Duration::ZERO)?;
    assert_eq!(
        mirrored(&hp),
        Some((LAPTOP.to_owned(), vec![TV.to_owned()])),
        "mirrored in the turn it lit"
    );
    assert_eq!(hp.outputs().len(), 1);
    assert!(!hp.screens_new_out());
    Ok(())
}

/// Acceptance 14: a set seen and left extended lights extended again.
#[test]
fn a_set_seen_extended_stays_extended() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = laptop(dir.path())?;
    plug_tv(&mut hp)?;
    hp.unplug(TV)?;
    wait(&mut hp, Duration::from_millis(50))?;
    plug_tv(&mut hp)?;
    assert_eq!(mirrored(&hp), None);
    assert_eq!(hp.outputs().len(), 2);
    assert!(!hp.screens_new_out());
    Ok(())
}

/// Acceptance 15: the lid closed over the laptop shown stops mirroring
/// for now, the TV takes the windows; opened, mirrored again.
#[test]
fn the_lid_stops_and_brings_back_mirroring() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = laptop(dir.path())?;
    plug_tv(&mut hp)?;
    open(&mut hp, "test.lid", (500, 300))?;
    hp.mirror(true);
    wait(&mut hp, Duration::from_millis(50))?;
    hp.lid(true);
    wait(&mut hp, Duration::from_millis(50))?;
    assert_eq!(mirrored(&hp), None);
    assert_eq!(
        hp.outputs().into_iter().map(|(n, _)| n).collect::<Vec<_>>(),
        vec![TV.to_owned()]
    );
    assert_eq!(
        placements_of(&hp, "test.lid").and_then(|p| p.output),
        Some(TV.to_owned())
    );
    hp.lid(false);
    wait(&mut hp, Duration::from_millis(50))?;
    assert_eq!(
        mirrored(&hp),
        Some((LAPTOP.to_owned(), vec![TV.to_owned()]))
    );
    assert_eq!(
        placements_of(&hp, "test.lid").and_then(|p| p.output),
        Some(LAPTOP.to_owned())
    );
    Ok(())
}

/// The Settings' "Montrer" (acceptance 18, the compositor's side): the
/// TV shown, the laptop shows its picture with 64 px bands above and
/// below (80 pixels at 1.25).
#[test]
fn the_tv_shown_on_the_laptop_has_bands_above_and_below() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = laptop(dir.path())?;
    plug_tv(&mut hp)?;
    hp.mirror(true);
    wait(&mut hp, Duration::from_millis(50))?;
    let tv_key = hp.screen_key(TV);
    hp.mirror_from_settings(Some(&tv_key))
        .map_err(|e| anyhow::anyhow!(e))?;
    hp.pointer_move(1900.0, 1000.0);
    wait(&mut hp, Duration::from_millis(100))?;
    assert_eq!(
        mirrored(&hp),
        Some((TV.to_owned(), vec![LAPTOP.to_owned()]))
    );
    assert_eq!(
        hp.mirror_fitted(LAPTOP),
        Some((TV.to_owned(), Rect::new(0, 80, 2560, 1440)))
    );
    for x in [0, 1280, 2559] {
        for y in [0, 79, 1520, 1599] {
            assert_eq!(
                pixel(&mut hp, LAPTOP, x, y)?,
                [0, 0, 0],
                "a band at ({x}, {y})"
            );
        }
    }
    hp.mirror_from_settings(None)
        .map_err(|e| anyhow::anyhow!(e))?;
    wait(&mut hp, Duration::from_millis(50))?;
    assert_eq!(mirrored(&hp), None);
    assert_eq!(hp.outputs().len(), 2);
    Ok(())
}

/// Acceptance 25: locked while mirrored, the TV shows the lock, fitted.
#[test]
fn the_lock_shows_on_the_tv() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = laptop(dir.path())?;
    plug_tv(&mut hp)?;
    hp.mirror(true);
    wait(&mut hp, Duration::from_millis(50))?;
    hp.lock();
    let drawn = hp.dispatch_until(WAIT, Headless::lock_drawn)?;
    assert!(drawn, "the lock never drew");
    common::at_rest(&mut hp)?;
    wait(&mut hp, Duration::from_millis(100))?;
    // The same picture: a few of the laptop's pixels where the TV shows
    // them.
    let mut same = 0;
    for (x, y) in [(400, 300), (1280, 200), (2000, 1400), (300, 1500)] {
        let tx = 96 + (f64::from(x) * 0.675) as i32;
        let ty = (f64::from(y) * 0.675) as i32;
        if pixel(&mut hp, TV, tx, ty)? == pixel(&mut hp, LAPTOP, x, y)? {
            same += 1;
        }
    }
    assert!(
        same >= 3,
        "the TV does not show the lock: {same} of 4 alike"
    );
    assert_eq!(pixel(&mut hp, TV, 10, 540)?, [0, 0, 0]);
    Ok(())
}

/// Acceptance 26, as `hocus msg outputs` and an agent's list of screens
/// read it: the TV mirrors the laptop.
#[test]
fn the_screens_said_name_the_mirror() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = laptop(dir.path())?;
    plug_tv(&mut hp)?;
    hp.mirror(true);
    wait(&mut hp, Duration::from_millis(50))?;
    let said = hp.state().displays_json();
    assert_eq!(said["shown"], LAPTOP);
    let row = |name: &str| {
        said["outputs"]
            .as_array()
            .and_then(|o| o.iter().find(|o| o["name"] == name))
            .cloned()
            .unwrap_or_default()
    };
    assert_eq!(row(TV)["mirrors"], LAPTOP);
    assert!(row(LAPTOP).get("mirrors").is_none());
    let listed = hp.mcp_outputs();
    let tv = listed
        .iter()
        .find(|o| o["name"] == TV)
        .cloned()
        .unwrap_or_default();
    assert_eq!(tv["mirror_of"], LAPTOP);
    assert!(tv.get("geometry").is_none());
    Ok(())
}

/// Acceptance 27: at rest, neither screen draws; a window's commit on
/// the laptop draws both, once.
#[test]
fn at_rest_neither_screen_draws() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = laptop(dir.path())?;
    plug_tv(&mut hp)?;
    hp.pointer_move(2000.0, 1250.0);
    let handle = client(
        hp.socket(),
        Some(common::Shows::Window {
            app_id: "test.commits",
            size: (400, 300),
            popup: false,
        }),
    )?;
    let mapped = hp.dispatch_until(WAIT, |hp| common::rect_of(hp, "test.commits").is_some())?;
    assert!(mapped);
    hp.mirror(true);
    // The window's opening over, the screens come to rest.
    common::at_rest(&mut hp)?;
    wait(&mut hp, Duration::from_millis(300))?;
    let (laptop_i, tv_i) = (index_of(&hp, LAPTOP), index_of(&hp, TV));
    let before = (hp.frames_on(laptop_i), hp.frames_on(tv_i));
    wait(&mut hp, Duration::from_millis(500))?;
    assert_eq!(
        (hp.frames_on(laptop_i), hp.frames_on(tv_i)),
        before,
        "frames drawn at rest"
    );
    handle
        .commands
        .send(common::Command::Redraw(10, 10, 50, 50))?;
    let both = hp.dispatch_until(WAIT, |hp| {
        hp.frames_on(laptop_i) > before.0 && hp.frames_on(tv_i) > before.1
    })?;
    assert!(both, "not both drawn");
    wait(&mut hp, Duration::from_millis(300))?;
    assert_eq!(
        (hp.frames_on(laptop_i), hp.frames_on(tv_i)),
        (before.0 + 1, before.1 + 1),
        "drawn once each"
    );
    Ok(())
}
