//! A window's rain as it opens and closes (#432, docs/design/windows.md
//! §5), on the held clock: what it shows on GLES (the GPU
//! `HOCUS_TEST_GPU` names, else Mesa's software device), its fade on
//! pixman, which windows rain, and what it costs: frames on its screen
//! alone, nothing once it ended, allocations on the loop.

mod common;

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    time::Duration,
};

use common::{WINDOW_COLOUR, open_with};
use hocus_compositor::{
    Config, Headless, HeadlessConfig, OutputSpec, RendererChoice, WindowAnimation, model::Rect,
};

/// Counts the allocations of the thread that asks, while it asks: the
/// loop runs on the test's thread, the clients on theirs.
struct Counting;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static COUNTED: Cell<u64> = const { Cell::new(0) };
}

// SAFETY: every call goes to the system's allocator; counting touches
// thread-locals of plain integers only, which never allocate.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = COUNTING.try_with(|on| {
            if on.get() {
                let _ = COUNTED.try_with(|n| n.set(n.get() + 1));
            }
        });
        // SAFETY: as the caller promised us.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as the caller promised us.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The allocations `run` makes on this thread.
fn allocations<T>(run: impl FnOnce() -> T) -> (T, u64) {
    COUNTED.with(|n| n.set(0));
    COUNTING.with(|on| on.set(true));
    let out = run();
    COUNTING.with(|on| on.set(false));
    (out, COUNTED.with(Cell::get))
}

const STEP: Duration = Duration::from_micros(16_667);

fn frosting() -> Config {
    let mut config = Config::default();
    config.windows.animation = WindowAnimation::Frost;
    config
}

fn raining() -> Config {
    let mut config = Config::default();
    config.windows.animation = WindowAnimation::Rain;
    config
}

fn on(renderer: HeadlessConfig, outputs: usize) -> HeadlessConfig {
    HeadlessConfig {
        outputs: (0..outputs).map(|_| OutputSpec::new(1280, 800)).collect(),
        ..renderer
    }
}

fn pixman() -> HeadlessConfig {
    HeadlessConfig {
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    }
}

/// How much of `rect` shows the test window's colour, from 0 to 1, and
/// whether any pixel there is green (a glyph). Under the veil, the
/// window is still bluer than green.
fn shown_as(shot: &(i32, i32, Vec<u8>), rect: Rect, veiled: bool) -> (f64, bool) {
    let [b0, g0, r0] = [
        WINDOW_COLOUR & 0xff,
        (WINDOW_COLOUR >> 8) & 0xff,
        (WINDOW_COLOUR >> 16) & 0xff,
    ]
    .map(|v| v as i32);
    let (mut window, mut all, mut green) = (0u64, 0u64, false);
    // The window's middle, clear of its rim and its rounded corners.
    for y in (rect.y + 12..rect.y + rect.height - 12).step_by(3) {
        for x in (rect.x + 12..rect.x + rect.width - 12).step_by(3) {
            let i = ((y * shot.0 + x) * 4) as usize;
            let p = &shot.2[i..i + 4];
            let (b, g, r) = (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]));
            all += 1;
            let exact = (b - b0).abs() <= 3 && (g - g0).abs() <= 3 && (r - r0).abs() <= 3;
            if exact || (veiled && b > 120 && b > g + 8 && r < 120) {
                window += 1;
            }
            green |= g > 150 && g > r + 60 && g > b + 30;
        }
    }
    (window as f64 / all.max(1) as f64, green)
}

/// As [`shown_as`], the window's colour exact.
fn shown(shot: &(i32, i32, Vec<u8>), rect: Rect) -> (f64, bool) {
    shown_as(shot, rect, false)
}

#[test]
fn a_window_rains_in_then_shows_whole_and_untinted() -> anyhow::Result<()> {
    let mut hp = Headless::start_with(on(common::gles(), 1), raining())?;
    let gles = hp.renderer() == "gles";
    common::at_rest(&mut hp)?;
    hp.hold_clock();
    let (rect, _client) = open_with(&mut hp, "test.rain", (600, 400), false)?;
    assert!(hp.raining("test.rain"));
    // It has the keyboard from its first frame, not at the rain's end.
    assert!(
        hp.windows()
            .iter()
            .any(|w| w.app_id.as_deref() == Some("test.rain") && w.keyboard)
    );
    let mut seen = Vec::new();
    for at in [0u64, 90, 180, 270, 360] {
        let start = hp.now();
        if at > 0 {
            hp.step(Duration::from_millis(90))?;
        } else {
            hp.step(Duration::ZERO)?;
        }
        let shot = hp.screenshot(0)?;
        // Under its veil until 360 ms; whole and untinted then.
        let veiled = at < 360 && gles;
        seen.push((at, shown_as(&shot, rect, veiled), hp.now() - start));
    }
    let fraction = |i: usize| seen[i].1.0;
    eprintln!("{}: {seen:?}", hp.renderer());
    if gles {
        assert!(fraction(0) < 0.01, "hidden at first: {seen:?}");
        assert!(
            fraction(2) > 0.05 && fraction(2) < 0.95,
            "partly shown at 180 ms: {seen:?}"
        );
        assert!(seen[2].1.1, "glyphs fall at 180 ms: {seen:?}");
        assert!(fraction(4) > 0.99, "whole at 360 ms: {seen:?}");
        assert!(!seen[4].1.1, "untinted at 360 ms: {seen:?}");
        // Only ever more of it.
        for pair in seen.windows(2) {
            assert!(pair[1].1.0 + 0.01 >= pair[0].1.0, "{seen:?}");
        }
    } else {
        // Pixman: a fade over the same time, whole at its end.
        assert!(fraction(0) < 0.01, "{seen:?}");
        assert!(fraction(4) > 0.99, "{seen:?}");
    }
    assert!(!hp.raining("test.rain"));
    Ok(())
}

#[test]
fn closing_drains_away_and_a_close_while_opening_goes_back() -> anyhow::Result<()> {
    let mut hp = Headless::start_with(on(common::gles(), 1), raining())?;
    let gles = hp.renderer() == "gles";
    common::at_rest(&mut hp)?;
    let (rect, client) = open_with(&mut hp, "test.drain", (600, 400), false)?;
    common::at_rest(&mut hp)?;
    hp.hold_clock();
    let _ = client.commands.send(common::Command::Close);
    hp.dispatch_until(common::WAIT, |hp| hp.rains() == 1)?;
    assert_eq!(hp.rains(), 1, "its close rains");
    hp.step(Duration::ZERO)?;
    let first = shown(&hp.screenshot(0)?, rect).0;
    let mut last = first;
    for _ in 0..18 {
        hp.step(STEP)?;
        let now = shown(&hp.screenshot(0)?, rect).0;
        assert!(
            now <= last + 0.01,
            "it drains, never back: {last} then {now}"
        );
        last = now;
    }
    assert!(first > 0.95, "whole as it starts: {first}");
    assert!(last < 0.01, "gone at 300 ms: {last}");
    common::at_rest(&mut hp)?;
    hp.release_clock();

    // Closed while it opens: back from where it was, never more of it.
    hp.hold_clock();
    let (rect, client) = open_with(&mut hp, "test.back", (600, 400), false)?;
    for _ in 0..9 {
        hp.step(STEP)?;
    }
    let before = shown_as(&hp.screenshot(0)?, rect, gles).0;
    let _ = client.commands.send(common::Command::Close);
    hp.dispatch_until(common::WAIT, |hp| hp.windows().is_empty())?;
    let mut last = before;
    let mut frames = 0;
    while hp.rains() > 0 && frames < 40 {
        hp.step(STEP)?;
        frames += 1;
        let now = shown_as(&hp.screenshot(0)?, rect, gles).0;
        assert!(now <= last + 0.01, "{before}: {last} then {now}");
        last = now;
    }
    if gles {
        assert!(before > 0.05, "half open: {before}");
    }
    // Back as long as it had come, not a whole closing.
    assert!(frames <= 11, "{frames} frames back");
    assert!(last < 0.01);
    Ok(())
}

#[test]
fn three_rain_at_once_small_windows_and_reduced_motion_do_not() -> anyhow::Result<()> {
    let mut hp = Headless::start_with(on(pixman(), 1), raining())?;
    common::at_rest(&mut hp)?;
    hp.hold_clock();
    let mut clients = Vec::new();
    for app in ["test.a", "test.b", "test.c", "test.d"] {
        clients.push(open_with(&mut hp, app, (400, 300), false)?.1);
    }
    assert_eq!(hp.rains(), 3, "three rains, the fourth as standard");
    assert!(!hp.raining("test.d"));
    hp.release_clock();
    common::at_rest(&mut hp)?;
    assert_eq!(hp.rains(), 0);
    // A small window opens as standard.
    let (_, _small) = open_with(&mut hp, "test.small", (180, 400), false)?;
    assert!(!hp.raining("test.small"));
    common::at_rest(&mut hp)?;
    // The power saver: as standard.
    hp.set_power_profile(Some("power-saver"));
    let (_, _saver) = open_with(&mut hp, "test.saver", (400, 300), false)?;
    assert!(!hp.raining("test.saver"));
    hp.set_power_profile(Some("balanced"));
    common::at_rest(&mut hp)?;
    // Reduced motion: as standard (its fade).
    let mut reduced = raining();
    reduced.appearance.reduced_motion = true;
    hp.reconfigure(reduced);
    let (_, _reduced) = open_with(&mut hp, "test.reduced", (400, 300), false)?;
    assert!(!hp.raining("test.reduced"));
    common::at_rest(&mut hp)?;
    // The standard animation: no rain.
    hp.reconfigure(Config::default());
    let (_, _plain) = open_with(&mut hp, "test.plain", (400, 300), false)?;
    assert!(!hp.raining("test.plain"));
    // Frost: Givre's frost line, the same rules (#740).
    hp.reconfigure(frosting());
    let (_, _frost) = open_with(&mut hp, "test.frost", (400, 300), false)?;
    assert!(hp.raining("test.frost"));
    Ok(())
}

/// Frames on each screen over `steps` refreshes at 60 Hz of the held
/// clock and 100 ms more (`settle`), and over the 100 ms after those (six
/// refreshes): the motion's end, then the screen at rest.
fn frames_over(hp: &mut Headless, steps: usize) -> anyhow::Result<((u64, u64), u64)> {
    let before = (hp.frames_on(0), hp.frames_on(1));
    for _ in 0..steps + 6 {
        hp.step(STEP)?;
    }
    let moved = (hp.frames_on(0) - before.0, hp.frames_on(1) - before.1);
    let last = hp.frames_on(0);
    for _ in 0..6 {
        hp.step(STEP)?;
    }
    Ok((moved, hp.frames_on(0) - last))
}

#[test]
fn a_rain_costs_its_frames_on_its_screen_alone() -> anyhow::Result<()> {
    costs(raining())
}

#[test]
fn the_thawing_pane_costs_the_rains_frames() -> anyhow::Result<()> {
    costs(frosting())
}

fn costs(config: Config) -> anyhow::Result<()> {
    for renderer in [common::gles(), pixman()] {
        let mut hp = Headless::start_with(on(renderer, 2), config.clone())?;
        let machine = format!("{} {:?}", hp.renderer(), config.windows.animation);
        common::at_rest(&mut hp)?;
        hp.pointer_move(400.0, 300.0);
        common::at_rest(&mut hp)?;
        hp.hold_clock();
        let before = (hp.frames_on(0), hp.frames_on(1));
        let (_, client) = open_with(&mut hp, "test.cost", (600, 400), false)?;
        assert!(hp.raining("test.cost"), "{machine}");
        // 360 ms, then 100 ms more.
        let ((on, _), after) = frames_over(&mut hp, 22)?;
        let open = (hp.frames_on(0) - before.0, hp.frames_on(1) - before.1);
        eprintln!("{machine}: {open:?} frames to open ({on} stepped)");
        // ⌈0.36 × 60⌉ + 1 frames, on its screen alone.
        assert!(open.0 <= 23, "{machine}: {open:?} frames to open");
        assert!(open.0 >= 10, "{machine}: {open:?}: it moved");
        assert_eq!(open.1, 0, "{machine}: none on the other screen");
        assert_eq!(after, 0, "{machine}: at rest 100 ms after");
        let _ = client.commands.send(common::Command::Close);
        hp.dispatch_until(common::WAIT, |hp| hp.rains() == 1)?;
        assert_eq!(hp.rains(), 1, "{machine}: its close rains");
        let before = (hp.frames_on(0), hp.frames_on(1));
        let (_, after) = frames_over(&mut hp, 18)?;
        let close = (hp.frames_on(0) - before.0, hp.frames_on(1) - before.1);
        eprintln!("{machine}: {close:?} frames to close");
        // ⌈0.30 × 60⌉ + 1.
        assert!(close.0 <= 19, "{machine}: {close:?} frames to close");
        assert_eq!(close.1, 0, "{machine}: none on the other screen");
        assert_eq!(after, 0, "{machine}: at rest 100 ms after");
        hp.release_clock();
    }
    Ok(())
}

/// The allocations of a whole opening, raining and standard, on the same
/// machine: the rain's own are few, and none grow with its frames.
#[test]
fn a_rain_allocates_little_on_the_loop() -> anyhow::Result<()> {
    let mut costs = Vec::new();
    for animation in [WindowAnimation::Standard, WindowAnimation::Rain] {
        let mut config = Config::default();
        config.windows.animation = animation;
        let mut hp = Headless::start_with(on(common::gles(), 1), config)?;
        let machine = hp.renderer();
        common::at_rest(&mut hp)?;
        hp.hold_clock();
        let (_, _client) = open_with(&mut hp, "test.alloc", (600, 400), false)?;
        // The same 22 frames either way, each counted.
        let mut per_frame = Vec::new();
        for _ in 0..22 {
            let ((), n) = allocations(|| {
                let _ = hp.step(STEP);
            });
            per_frame.push(n);
        }
        eprintln!("{machine} {animation:?}: {per_frame:?}");
        costs.push(per_frame);
    }
    let total = |c: &[u64]| c.iter().sum::<u64>();
    let (standard, rain) = (&costs[0], &costs[1]);
    eprintln!(
        "a whole opening: standard {}, rain {} allocations",
        total(standard),
        total(rain)
    );
    // Frame by frame, not their totals: either opening's frames take 7
    // allocations more or fewer as the client's commits fall in them or
    // not, which over 22 frames is as much as the rain's own. Its first
    // two frames make its picture; then each as the standard's most.
    let first = |c: &[u64]| c[..2].iter().sum::<u64>();
    assert!(
        first(rain) <= first(standard) + 100,
        "the rain's picture: {rain:?} against {standard:?}"
    );
    let most = standard[2..].iter().max().copied().unwrap_or(0);
    assert!(
        rain[2..].iter().all(|n| *n <= most + 10),
        "the rain's frames: {rain:?} against {standard:?}"
    );
    // Nothing grows frame after frame: the last frames as the first ones.
    let rain = &costs[1];
    let early = rain[2..6].iter().max().copied().unwrap_or(0);
    let late = rain[16..20].iter().max().copied().unwrap_or(0);
    assert!(late <= early + 2, "{rain:?}");
    Ok(())
}

/// A screenshot of output `index` saved as `name`.png in `out`, cut to
/// `crop` (x, y, width, height) when given.
fn save(
    hp: &mut Headless,
    out: &std::path::Path,
    name: &str,
    crop: Option<(u32, u32, u32, u32)>,
) -> anyhow::Result<()> {
    let (w, h, px) = hp.screenshot(0)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let mut image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
        .ok_or_else(|| anyhow::anyhow!("no image"))?;
    if let Some((x, y, cw, ch)) = crop {
        image = image::imageops::crop_imm(&image, x, y, cw, ch).to_image();
    }
    let file = out.join(format!("{name}.png"));
    image.save(&file)?;
    eprintln!("{}", file.display());
    Ok(())
}

/// The rain's frames for the maintainer, written to `$HOCUS_SHOTS` when it
/// names a folder, on GLES: a window opening at 0, 90, 180, 270 and 360 ms,
/// and closing at 0, 75, 150 and 225 ms. Run by hand (`--run-ignored
/// only`).
#[test]
#[ignore = "screenshots, by hand"]
fn shots_of_the_rain() -> anyhow::Result<()> {
    shots(raining(), "rain")
}

/// The thawing pane's frames (#740), on a window of text.
#[test]
#[ignore = "screenshots, by hand"]
fn shots_of_the_thaw() -> anyhow::Result<()> {
    shots(frosting(), "thaw")
}

fn shots(mut config: Config, name: &str) -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(std::path::PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    config.wallpaper.picture = hocus_link::wallpaper::Picture::of("night").unwrap_or_default();
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1280, 800)],
            ..common::gles()
        },
        config,
    )?;
    anyhow::ensure!(hp.renderer() == "gles", "no GLES here");
    common::at_rest(&mut hp)?;
    hp.hold_clock();
    let shows = common::Shows::Document {
        app_id: "test.shot",
        size: (640, 420),
    };
    let (rect, client) = common::open_showing(&mut hp, "test.shot", shows)?;
    let crop = Some((
        (rect.x - 30).max(0) as u32,
        (rect.y - 30).max(0) as u32,
        (rect.width + 60) as u32,
        (rect.height + 60) as u32,
    ));
    hp.step(Duration::ZERO)?;
    save(&mut hp, &out, &format!("{name}-open-000ms"), crop)?;
    for at in [90, 180, 270, 360] {
        hp.step(Duration::from_millis(90))?;
        save(&mut hp, &out, &format!("{name}-open-{at:03}ms"), crop)?;
    }
    for _ in 0..12 {
        hp.step(STEP)?;
    }
    let _ = client.commands.send(common::Command::Close);
    hp.dispatch_until(common::WAIT, |hp| hp.rains() == 1)?;
    hp.step(Duration::ZERO)?;
    save(&mut hp, &out, &format!("{name}-close-000ms"), crop)?;
    for at in [75, 150, 225] {
        hp.step(Duration::from_millis(75))?;
        save(&mut hp, &out, &format!("{name}-close-{at:03}ms"), crop)?;
    }
    Ok(())
}

/// Over `rect`'s inside, the share of pixels whose brightness against
/// their right neighbour is ordered as in the checkered window (white and
/// black squares of 3 px): 1 when nothing is displaced, whatever the veil;
/// and the same within 3 px of its edges alone.
fn steady(shot: &(i32, i32, Vec<u8>), rect: Rect) -> (f64, f64) {
    let lum = |x: i32, y: i32| {
        let i = ((y * shot.0 + x) * 4) as usize;
        u32::from(shot.2[i]) + u32::from(shot.2[i + 1]) + u32::from(shot.2[i + 2])
    };
    let white = |dx: i32, dy: i32| (dx / 3 + dy / 3) % 2 == 0;
    let (mut all, mut kept, mut edge, mut edge_kept) = (0u64, 0u64, 0u64, 0u64);
    for dy in (1..rect.height - 1).step_by(2) {
        for dx in 1..rect.width - 2 {
            if white(dx, dy) == white(dx + 1, dy) {
                continue;
            }
            let (x, y) = (rect.x + dx, rect.y + dy);
            let ordered = (lum(x, y) > lum(x + 1, y)) == white(dx, dy);
            let near = dx < 3 || dy < 3 || rect.width - dx < 4 || rect.height - dy < 4;
            all += 1;
            kept += u64::from(ordered);
            if near {
                edge += 1;
                edge_kept += u64::from(ordered);
            }
        }
    }
    (
        kept as f64 / all.max(1) as f64,
        edge_kept as f64 / edge.max(1) as f64,
    )
}

/// Pixels of `rect` brighter, in a channel, than both the test window's
/// and the ice's (`#DBF0FF`), by more than rounding.
fn brighter_than_window_and_ice(shot: &(i32, i32, Vec<u8>), rect: Rect) -> usize {
    let window = [
        WINDOW_COLOUR & 0xff,
        (WINDOW_COLOUR >> 8) & 0xff,
        (WINDOW_COLOUR >> 16) & 0xff,
    ]
    .map(|v| v as i32);
    let ice = [0xFF, 0xF0, 0xDB];
    let mut over = 0;
    for y in (rect.y + 2..rect.y + rect.height - 2).step_by(2) {
        for x in (rect.x + 2..rect.x + rect.width - 2).step_by(2) {
            let i = ((y * shot.0 + x) * 4) as usize;
            let p = &shot.2[i..i + 3];
            over += usize::from((0..3).any(|c| i32::from(p[c]) > window[c].max(ice[c]) + 3));
        }
    }
    over
}

#[test]
fn the_pane_thaws_its_text_steady_from_200_ms_its_edges_never_wavering() -> anyhow::Result<()> {
    let mut hp = Headless::start_with(on(common::gles(), 1), frosting())?;
    if hp.renderer() != "gles" {
        eprintln!("no GLES here: the pane does not thaw");
        return Ok(());
    }
    common::at_rest(&mut hp)?;
    hp.hold_clock();
    let shows = common::Shows::Checkered {
        app_id: "test.thaw",
        size: (600, 400),
    };
    let (rect, _client) = common::open_showing(&mut hp, "test.thaw", shows)?;
    assert!(hp.raining("test.thaw"));
    let mut seen = Vec::new();
    let mut t = 0;
    hp.step(Duration::ZERO)?;
    while t <= 360 {
        let shot = hp.screenshot(0)?;
        seen.push((t, steady(&shot, rect)));
        hp.step(Duration::from_millis(30))?;
        t += 30;
    }
    eprintln!("{seen:?}");
    for (t, (inside, edges)) in &seen {
        if *t == 0 {
            continue;
        }
        // The frame never wavers.
        assert!(*edges > 0.99, "{t} ms: edges {edges}");
        // The text is steady from 200 ms on.
        if *t >= 210 {
            assert!(*inside > 0.99, "{t} ms: {inside}");
        }
    }
    // It did refract early on.
    assert!(
        seen.iter()
            .any(|(t, (inside, _))| *t <= 120 && *inside < 0.97),
        "{seen:?}"
    );
    Ok(())
}

#[test]
fn the_pane_thaws_to_the_window_and_freezes_away_adding_nothing_brighter() -> anyhow::Result<()> {
    let mut hp = Headless::start_with(on(common::gles(), 1), frosting())?;
    let gles = hp.renderer() == "gles";
    common::at_rest(&mut hp)?;
    hp.hold_clock();
    let (rect, client) = open_with(&mut hp, "test.frost", (600, 400), false)?;
    assert!(hp.raining("test.frost"));
    let mut seen = Vec::new();
    for at in [0u64, 90, 180, 270, 360] {
        if at > 0 {
            hp.step(Duration::from_millis(90))?;
        } else {
            hp.step(Duration::ZERO)?;
        }
        let shot = hp.screenshot(0)?;
        seen.push((
            at,
            shown(&shot, rect).0,
            shown_as(&shot, rect, true).0,
            brighter_than_window_and_ice(&shot, rect),
        ));
    }
    eprintln!("{}: {seen:?}", hp.renderer());
    // Nothing at first, whole and untinted at 360 ms.
    assert!(seen[0].1 < 0.01 && seen[0].2 < 0.01, "{seen:?}");
    assert!(seen[4].1 > 0.99, "{seen:?}");
    if gles {
        // Seen through the ice at 90 ms: there, but not itself yet.
        assert!(seen[1].1 < 0.5 && seen[1].2 > 0.5, "{seen:?}");
        // No pattern brighter than the window and the ice.
        assert!(seen.iter().all(|s| s.3 == 0), "{seen:?}");
    }
    // Closing: whole at 0, gone at 300 ms, nothing brighter meanwhile.
    let _ = client.commands.send(common::Command::Close);
    hp.dispatch_until(common::WAIT, |hp| hp.rains() == 1)?;
    hp.step(Duration::ZERO)?;
    let first = shown(&hp.screenshot(0)?, rect).0;
    let mut brighter = 0;
    for _ in 0..18 {
        hp.step(STEP)?;
        brighter += brighter_than_window_and_ice(&hp.screenshot(0)?, rect);
    }
    let last = shown_as(&hp.screenshot(0)?, rect, true).0;
    assert!(first > 0.95, "{first}");
    assert!(last < 0.01, "{last}");
    if gles {
        assert_eq!(brighter, 0);
    }
    Ok(())
}

#[test]
fn the_glyphs_are_8_by_12_at_least() {
    let ((x0, y0), (x1, y1)) = {
        let (lo, hi) = hocus_link::moving::GLYPH_BOX;
        ((lo[0], lo[1]), (hi[0], hi[1]))
    };
    let (w, h) = hocus_link::moving::WINDOW_CELL;
    assert!((x1 - x0) * w >= 8.0 && (y1 - y0) * h >= 12.0);
}
