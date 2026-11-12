//! Snow that settles (#795, docs/design/effects.md), on the held clock and
//! on GLES (Mesa's software device, or the GPU `HOCUS_TEST_GPU` names): a
//! window's cap grows, never over what the window shows; a window shaken
//! sheds its snow; off, on the CPU, or still with reduced motion, nothing
//! is drawn frame after frame.

use crate::common;

use std::time::Duration;

use common::{WINDOW_COLOUR, open_with};
use hocus_compositor::{Config, Headless, HeadlessConfig, OutputSpec, RendererChoice, model::Rect};

const STEP: Duration = Duration::from_micros(16_667);

fn snowing() -> Config {
    let mut config = Config::default();
    config.effects.snow = true;
    config
}

fn on(renderer: HeadlessConfig) -> HeadlessConfig {
    HeadlessConfig {
        outputs: vec![OutputSpec::new(1280, 800)],
        ..renderer
    }
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

/// A GLES compositor, a window of the test's colour on it at rest, then
/// the snow turned on (`config`'s, but the switch): its piles grown.
fn bed(config: Config) -> anyhow::Result<(Headless, Rect, common::ClientHandle)> {
    let mut hp = Headless::start_with(on(common::gles()), Config::default())?;
    if hp.renderer() != "gles" {
        common::missing("GLES on Mesa's software device (libegl-mesa0, libgles2)");
    }
    common::at_rest(&mut hp)?;
    let (rect, client) = open_with(&mut hp, "test.snowy", (600, 360), false)?;
    common::at_rest(&mut hp)?;
    hp.hold_clock();
    hp.reconfigure(config);
    run(&mut hp, STEP * 2)?;
    Ok((hp, rect, client))
}

fn id_of(hp: &Headless, app_id: &str) -> anyhow::Result<hocus_compositor::model::WindowId> {
    hp.windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some(app_id))
        .map(|w| w.id)
        .ok_or_else(|| anyhow::anyhow!("no {app_id}"))
}

/// Pixels of snow (bright, faintly blue) in `rect`, clipped to the screen.
fn snow_pixels(shot: &(i32, i32, Vec<u8>), rect: Rect) -> usize {
    let (w, h) = (shot.0, shot.2.len() as i32 / 4 / shot.0);
    let mut n = 0;
    for y in rect.y.max(0)..(rect.y + rect.height).min(h) {
        for x in rect.x.max(0)..(rect.x + rect.width).min(w) {
            let i = ((y * w + x) * 4) as usize;
            let (b, g, r) = (shot.2[i], shot.2[i + 1], shot.2[i + 2]);
            n += usize::from(r > 180 && g > 190 && b > 210 && b >= r);
        }
    }
    n
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

#[test]
fn a_cap_grows_on_a_window_and_never_over_what_it_shows() -> anyhow::Result<()> {
    let mut hp = Headless::start_with(on(common::gles()), Config::default())?;
    if hp.renderer() != "gles" {
        common::missing("GLES on Mesa's software device (libegl-mesa0, libgles2)");
    }
    common::at_rest(&mut hp)?;
    let (rect, _client) = open_with(&mut hp, "test.snowy", (600, 360), false)?;
    common::at_rest(&mut hp)?;
    let id = id_of(&hp, "test.snowy")?;
    hp.hold_clock();
    hp.step(STEP)?;
    let before = hp.screenshot(0)?;
    assert_eq!(hp.snow_cap(id), None, "no snow yet");
    // Turned on: its piles already hold snow.
    hp.reconfigure(snowing());
    run(&mut hp, STEP * 2)?;
    let warm = hp.snow_cap(id).unwrap_or_default();
    assert!(warm > 1000.0, "warmed up: {warm}");
    run(&mut hp, Duration::from_secs(3))?;
    let grown = hp.snow_cap(id).unwrap_or_default();
    eprintln!("the cap: {warm} warm, {grown} 3 s later");
    assert!(grown > warm, "it grows: {warm} then {grown}");
    let shot = hp.screenshot(0)?;
    // White above its top edge.
    let above = Rect {
        x: rect.x,
        y: rect.y - 26,
        width: rect.width,
        height: 26,
    };
    let white = snow_pixels(&shot, above);
    assert!(white > rect.width as usize * 3, "snow on its edge: {white}");
    // What it shows, but the top pixels its snow rests on, as before.
    let inside = Rect {
        x: rect.x + 2,
        y: rect.y + 9,
        width: rect.width - 4,
        height: rect.height - 11,
    };
    let (kept, all) = same(&before, &shot, inside);
    assert_eq!(kept, all, "the window's pixels untouched");
    // Its colour there still, to be sure what was compared is the window.
    let i = (((rect.y + 40) * shot.0 + rect.x + 40) * 4) as usize;
    let colour =
        u32::from(shot.2[i]) | u32::from(shot.2[i + 1]) << 8 | u32::from(shot.2[i + 2]) << 16;
    assert_eq!(colour, WINDOW_COLOUR & 0xff_ffff);
    Ok(())
}

#[test]
fn a_window_shaken_sharply_sheds_its_snow_in_clumps() -> anyhow::Result<()> {
    let (mut hp, rect, _client) = bed(snowing())?;
    let id = id_of(&hp, "test.snowy")?;
    run(&mut hp, Duration::from_millis(200))?;
    let held = hp.snow_cap(id).unwrap_or_default();
    assert!(held > 1000.0, "{held}");
    // The board's shake: 22 px each way at 38 rad/s, dying out.
    let mut clumps = 0;
    for k in 1..=12 {
        let t = k as f64 * STEP.as_secs_f64();
        let dx = (t * 38.0).sin() * 22.0 * (-t * 5.0).exp();
        hp.place_window(
            id,
            Rect {
                x: rect.x + dx.round() as i32,
                ..rect
            },
        );
        hp.step(STEP)?;
        clumps = clumps.max(hp.snow_clumps());
    }
    let left = hp.snow_cap(id).unwrap_or_default();
    eprintln!("the cap: {held} before, {left} after; {clumps} clumps");
    assert!(left < held * 0.1, "shed: {held} then {left}");
    assert!(clumps > 10, "in clumps: {clumps}");
    // They fall, and are gone into the ground within a few seconds.
    hp.place_window(id, rect);
    run(&mut hp, Duration::from_secs(3))?;
    assert_eq!(hp.snow_clumps(), 0);
    // A slow move sheds nothing.
    run(&mut hp, Duration::from_secs(2))?;
    let held = hp.snow_cap(id).unwrap_or_default();
    for k in 1..=30 {
        hp.place_window(
            id,
            Rect {
                y: rect.y + k / 3,
                ..rect
            },
        );
        hp.step(STEP)?;
    }
    let kept = hp.snow_cap(id).unwrap_or_default();
    assert!(
        kept >= held * 0.95,
        "a slow move keeps it: {held} then {kept}"
    );
    Ok(())
}

#[test]
fn off_on_the_cpu_or_still_nothing_is_drawn_frame_after_frame() -> anyhow::Result<()> {
    let pixman = HeadlessConfig {
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    };
    let mut still = snowing();
    still.appearance.reduced_motion = true;
    let cases = [
        (on(pixman), snowing(), "pixman", false),
        (on(common::gles()), Config::default(), "off", false),
        (on(common::gles()), still, "reduced motion", true),
    ];
    for (renderer, config, case, piles) in cases {
        let mut hp = Headless::start_with(renderer, Config::default())?;
        common::at_rest(&mut hp)?;
        let (rect, _client) = open_with(&mut hp, "test.snowy", (600, 360), false)?;
        common::at_rest(&mut hp)?;
        hp.hold_clock();
        hp.reconfigure(config);
        run(&mut hp, Duration::from_millis(100))?;
        assert_eq!(hp.snowing(), piles, "{case}");
        let white = snow_pixels(
            &hp.screenshot(0)?,
            Rect {
                x: rect.x,
                y: rect.y - 26,
                width: rect.width,
                height: 26,
            },
        );
        // Still, its piles show; otherwise none.
        assert_eq!(white > 0, piles, "{case}: {white} pixels of snow");
        let last = hp.frames_on(0);
        run(&mut hp, Duration::from_millis(300))?;
        assert_eq!(hp.frames_on(0), last, "{case}: nothing drawn per frame");
    }
    Ok(())
}

/// A screenshot of output 0 saved as `name`.png in `out`.
fn save(hp: &mut Headless, out: &std::path::Path, name: &str) -> anyhow::Result<()> {
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

/// The snow for the maintainer, written to `$HOCUS_SHOTS` when it names a
/// folder, on GLES, with the shell's bar: two windows and the bar under
/// settled snow, flakes falling; then a window just shaken, its clumps
/// falling. Run by hand (`--run-ignored only`).
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
    let left = common::Shows::Document {
        app_id: "test.notes",
        size: (460, 340),
    };
    let (_, _left) = common::open_showing(&mut hp, "test.notes", left)?;
    let right = common::Shows::Document {
        app_id: "test.page",
        size: (520, 400),
    };
    let (_, _right) = common::open_showing(&mut hp, "test.page", right)?;
    let notes = id_of(&hp, "test.notes")?;
    let page = id_of(&hp, "test.page")?;
    hp.place_window(notes, Rect::new(110, 330, 460, 340));
    hp.place_window(page, Rect::new(620, 210, 520, 400));
    common::wait(&mut hp, Duration::from_secs(2))?;
    hp.hold_clock();
    config.effects.snow = true;
    hp.reconfigure(config);
    run(&mut hp, Duration::from_secs(4))?;
    save(&mut hp, &out, "snow-settled")?;
    let rect = Rect::new(620, 210, 520, 400);
    for k in 1..=7 {
        let t = k as f64 * STEP.as_secs_f64();
        let dx = (t * 38.0).sin() * 22.0 * (-t * 5.0).exp();
        hp.place_window(
            page,
            Rect {
                x: rect.x + dx.round() as i32,
                ..rect
            },
        );
        hp.step(STEP)?;
    }
    save(&mut hp, &out, "snow-shaken")?;
    run(&mut hp, Duration::from_millis(150))?;
    save(&mut hp, &out, "snow-shaken-later")?;
    Ok(())
}
