//! The garland under the active window (#799, docs/design/effects.md), on
//! the held clock and on GLES (Mesa's software device, or the GPU
//! `HOCUS_TEST_GPU` names): it hangs below the focused window and never
//! on it, moving; it follows the focus to another window; none on a
//! maximised window; off, nothing; still with reduced motion, nothing
//! drawn frame after frame.

use crate::common;

use std::{path::Path, time::Duration};

use common::{KEY_LEFTMETA, KEY_UP, WAIT, open_with};
use hocus_compositor::{
    Config, Headless, HeadlessConfig, OutputSpec,
    model::{Mode, Rect, WindowId, Zone},
};

const STEP: Duration = Duration::from_micros(16_667);
/// The two windows: side by side, far enough apart that one's garland
/// never lights below the other.
const LEFT: Rect = Rect {
    x: 100,
    y: 150,
    width: 400,
    height: 260,
};
const RIGHT: Rect = Rect {
    x: 720,
    y: 150,
    width: 400,
    height: 260,
};

fn garland(on: bool, reduced_motion: bool) -> Config {
    let mut config = Config::default();
    config.effects.garland = on;
    config.appearance.reduced_motion = reduced_motion;
    config
}

/// Steps the held clock by `by`, a frame at a time.
fn run(hp: &mut Headless, by: Duration) -> anyhow::Result<()> {
    let mut left = by;
    while left > Duration::ZERO {
        let step = left.min(STEP);
        hp.step(step)?;
        left -= step;
    }
    Ok(())
}

fn id_of(hp: &Headless, app_id: &str) -> anyhow::Result<WindowId> {
    hp.windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some(app_id))
        .map(|w| w.id)
        .ok_or_else(|| anyhow::anyhow!("no {app_id}"))
}

/// A GLES compositor, two windows of the test's colour at [`LEFT`] and
/// [`RIGHT`] (the right one focused), at rest, the garland off; the
/// clock held.
fn bed() -> anyhow::Result<(Headless, [WindowId; 2], Vec<common::ClientHandle>)> {
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1280, 800)],
            ..common::gles()
        },
        Config::default(),
    )?;
    if hp.renderer() != "gles" {
        common::missing("GLES on Mesa's software device (libegl-mesa0, libgles2)");
    }
    common::at_rest(&mut hp)?;
    let mut clients = Vec::new();
    let mut ids = Vec::new();
    for (app_id, rect) in [("test.left", LEFT), ("test.right", RIGHT)] {
        let (_, client) = open_with(
            &mut hp,
            app_id,
            (rect.width as u32, rect.height as u32),
            false,
        )?;
        clients.push(client);
        let id = id_of(&hp, app_id)?;
        hp.place_window(id, rect);
        ids.push(id);
    }
    common::at_rest(&mut hp)?;
    hp.hold_clock();
    run(&mut hp, STEP * 2)?;
    Ok((hp, [ids[0], ids[1]], clients))
}

/// Over `rect`, the pixels the same in `a` and `b`, of how many.
fn same(a: &(i32, i32, Vec<u8>), b: &(i32, i32, Vec<u8>), rect: Rect) -> (usize, usize) {
    let (mut same, mut all) = (0, 0);
    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width {
            let i = ((y * a.0 + x) * 4) as usize;
            all += 1;
            same += usize::from(a.2.get(i..i + 3) == b.2.get(i..i + 3));
        }
    }
    (same, all)
}

/// Under `rect`: where its garland hangs.
fn below(rect: Rect) -> Rect {
    Rect {
        x: rect.x,
        y: rect.y + rect.height + 2,
        width: rect.width,
        height: 70,
    }
}

/// What `rect` shows but its bottom corners (round: the wallpaper, lit,
/// shows there).
fn inside(rect: Rect) -> [Rect; 2] {
    [
        Rect {
            height: rect.height - 12,
            ..rect
        },
        Rect {
            x: rect.x + 12,
            y: rect.y + rect.height - 12,
            width: rect.width - 24,
            height: 12,
        },
    ]
}

/// Its bulbs below `rect`'s bottom edge, within its sides.
fn hangs_under(bulbs: &[(f64, f64)], rect: Rect) -> bool {
    let bottom = f64::from(rect.y + rect.height);
    bulbs.len() == 15
        && bulbs.iter().all(|(x, y)| {
            *y > bottom && *x > f64::from(rect.x) && *x < f64::from(rect.x + rect.width)
        })
}

#[test]
fn it_hangs_below_the_focused_window_and_never_on_it() -> anyhow::Result<()> {
    let (mut hp, [_, right], _clients) = bed()?;
    let before = hp.screenshot(0)?;
    assert!(hp.garland().is_none());
    hp.reconfigure(garland(true, false));
    run(&mut hp, Duration::from_millis(300))?;
    let (window, bulbs) = hp.garland().ok_or_else(|| anyhow::anyhow!("no garland"))?;
    assert_eq!(window, right);
    assert!(hangs_under(&bulbs, RIGHT), "{bulbs:?}");
    let shot = hp.screenshot(0)?;
    let (kept, all) = same(&before, &shot, below(RIGHT));
    assert!(
        all - kept > all / 10,
        "a garland below: {kept} of {all} kept"
    );
    for rect in inside(RIGHT) {
        let (kept, all) = same(&before, &shot, rect);
        assert_eq!(kept, all, "the window's pixels untouched");
    }
    // It moves, drawn over its own box alone.
    let frames = hp.frames_on(0);
    hp.take_damage(0);
    run(&mut hp, Duration::from_millis(300))?;
    assert!(hp.frames_on(0) > frames + 5, "it moves");
    let damage = hp
        .take_damage(0)
        .ok_or_else(|| anyhow::anyhow!("nothing drawn"))?;
    assert!(
        damage.x > RIGHT.x - 200
            && damage.x + damage.width < RIGHT.x + RIGHT.width + 200
            && damage.y > RIGHT.y + RIGHT.height - 200,
        "{damage:?}"
    );
    Ok(())
}

#[test]
fn it_follows_the_focus_to_another_window() -> anyhow::Result<()> {
    let (mut hp, [left, right], _clients) = bed()?;
    let on_left = (f64::from(LEFT.x + 200), f64::from(LEFT.y + 130));
    let on_right = (f64::from(RIGHT.x + 200), f64::from(RIGHT.y + 130));
    // Without it, the left window focused: what the screen shows then.
    common::click(&mut hp, on_left)?;
    run(&mut hp, Duration::from_millis(1000))?;
    let before = hp.screenshot(0)?;
    common::click(&mut hp, on_right)?;
    run(&mut hp, Duration::from_millis(1000))?;
    hp.reconfigure(garland(true, false));
    run(&mut hp, Duration::from_millis(200))?;
    assert_eq!(hp.garland().map(|(w, _)| w), Some(right));
    common::click(&mut hp, on_left)?;
    run(&mut hp, Duration::from_millis(1500))?;
    let (window, bulbs) = hp.garland().ok_or_else(|| anyhow::anyhow!("no garland"))?;
    assert_eq!(window, left);
    assert!(hangs_under(&bulbs, LEFT), "{bulbs:?}");
    let shot = hp.screenshot(0)?;
    let (kept, all) = same(&before, &shot, below(LEFT));
    assert!(
        all - kept > all / 10,
        "a garland below: {kept} of {all} kept"
    );
    let (kept, all) = same(&before, &shot, below(RIGHT));
    assert_eq!(kept, all, "none left below the other");
    Ok(())
}

#[test]
fn none_on_a_maximized_window() -> anyhow::Result<()> {
    let (mut hp, [_, right], _clients) = bed()?;
    hp.reconfigure(garland(true, false));
    run(&mut hp, Duration::from_millis(100))?;
    assert!(hp.garland().is_some());
    common::chord(&mut hp, &[KEY_LEFTMETA, KEY_UP]);
    let maximized =
        hp.dispatch_until(WAIT, |hp| hp.mode(right) == Mode::Snapped(Zone::Maximized))?;
    anyhow::ensure!(maximized, "{:?}", hp.mode(right));
    run(&mut hp, Duration::from_millis(600))?;
    assert!(hp.garland().is_none());
    let last = hp.frames_on(0);
    run(&mut hp, Duration::from_millis(300))?;
    assert_eq!(hp.frames_on(0), last, "nothing drawn per frame");
    Ok(())
}

#[test]
fn off_nothing_and_still_with_reduced_motion() -> anyhow::Result<()> {
    for (config, case) in [
        (garland(false, false), "off"),
        (garland(true, true), "reduced motion"),
    ] {
        let (mut hp, _, _clients) = bed()?;
        let before = hp.screenshot(0)?;
        hp.reconfigure(config);
        run(&mut hp, Duration::from_millis(100))?;
        let still = case == "reduced motion";
        assert_eq!(hp.garland().is_some(), still, "{case}");
        let shot = hp.screenshot(0)?;
        let (kept, all) = same(&before, &shot, below(RIGHT));
        assert_eq!(
            kept < all,
            still,
            "{case}: drawn below: {kept} of {all} kept"
        );
        let last = hp.frames_on(0);
        run(&mut hp, Duration::from_millis(300))?;
        assert_eq!(hp.frames_on(0), last, "{case}: nothing drawn per frame");
    }
    Ok(())
}

// --- The screenshots ------------------------------------------------------------

/// A screenshot of output 0 saved as `name`.png in `out`.
fn save(hp: &mut Headless, out: &Path, name: &str) -> anyhow::Result<()> {
    let (w, h, px) = hp.screenshot(0)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
        .ok_or_else(|| anyhow::anyhow!("no image"))?;
    let file = out.join(format!("{name}.png"));
    image.save(&file)?;
    eprintln!("{}", file.display());
    Ok(())
}

/// The garland for the maintainer, written to `$HOCUS_SHOTS` when it
/// names a folder, on GLES, with the shell's bar: under the active window
/// of two; with the snow on too; then after the other window took the
/// focus. Run by hand (`--run-ignored only`).
#[test]
#[ignore = "screenshots, by hand"]
fn shots() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(std::path::PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let home = tempfile::tempdir()?;
    let at = |sub: &str| home.path().join(sub).display().to_string();
    for sub in ["home", "config", "cache", "state", "data"] {
        std::fs::create_dir_all(home.path().join(sub))?;
    }
    let script = common::shell_script(
        home.path(),
        &common::shell_program(),
        &[
            ("LC_ALL", "fr_FR.UTF-8".to_owned()),
            ("LANG", "fr_FR.UTF-8".to_owned()),
            ("HOME", at("home")),
            ("XDG_CONFIG_HOME", at("config")),
            ("XDG_CACHE_HOME", at("cache")),
            ("XDG_STATE_HOME", at("state")),
            ("XDG_DATA_HOME", at("data")),
        ],
    )?;
    let mut config = Config::default();
    config.wallpaper.picture = hocus_link::wallpaper::Picture::of("night").unwrap_or_default();
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1440, 900)],
            ..common::gles()
        },
        config.clone(),
        Some(script),
    )?;
    anyhow::ensure!(hp.renderer() == "gles", "no GLES here");
    let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.levels_commits() > 0)?;
    anyhow::ensure!(up, "no bar");
    let notes = Rect::new(110, 280, 420, 320);
    let page = Rect::new(500, 190, 520, 380);
    let (_, _notes) = common::open_showing(
        &mut hp,
        "test.notes",
        common::Shows::Document {
            app_id: "test.notes",
            size: (notes.width as u32, notes.height as u32),
        },
    )?;
    let (_, _page) = common::open_showing(
        &mut hp,
        "test.page",
        common::Shows::Document {
            app_id: "test.page",
            size: (page.width as u32, page.height as u32),
        },
    )?;
    hp.place_window(id_of(&hp, "test.notes")?, notes);
    hp.place_window(id_of(&hp, "test.page")?, page);
    common::wait(&mut hp, Duration::from_secs(2))?;
    hp.hold_clock();
    config.effects.garland = true;
    hp.reconfigure(config.clone());
    run(&mut hp, Duration::from_millis(2300))?;
    save(&mut hp, &out, "garland")?;
    // The snow on too: its piles grown, flakes falling, the garland under.
    config.effects.snow = true;
    hp.reconfigure(config.clone());
    run(&mut hp, Duration::from_secs(4))?;
    save(&mut hp, &out, "garland-snow")?;
    config.effects.snow = false;
    hp.reconfigure(config);
    run(&mut hp, Duration::from_millis(100))?;
    // The other window takes the focus: the garland jumps there.
    common::click(&mut hp, (f64::from(notes.x + 60), f64::from(notes.y + 200)))?;
    run(&mut hp, Duration::from_secs(2))?;
    save(&mut hp, &out, "garland-other-window")?;
    Ok(())
}
