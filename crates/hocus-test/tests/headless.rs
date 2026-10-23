//! Acceptance scenarios of the headless backend and its virtual clock (#5).

use std::time::Duration;

use hocus_api::{Event, Permission, Request};
use hocus_compositor::{FRAME, Headless, HeadlessConfig, OutputSpec, RendererChoice, RendererKind};
use hocus_test::client::SolidWindow;

/// Long enough for a client thread on a loaded CI machine.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

/// The two outputs of the scenarios: a laptop panel at scale 2 and a wide
/// screen to its right. Pixman, so that every machine draws the same pixels.
fn two_outputs() -> HeadlessConfig {
    HeadlessConfig {
        outputs: vec![
            OutputSpec::new(2560, 1600).scale(2.0),
            OutputSpec::new(3440, 1440),
        ],
        renderer: RendererChoice::Pixman,
    }
}

fn frames(compositor: &Headless) -> Vec<u64> {
    compositor.outputs().iter().map(|o| o.frames).collect()
}

#[test]
fn starts_with_pixman_and_reports_both_outputs() -> anyhow::Result<()> {
    let compositor = Headless::start(two_outputs())?;

    assert_eq!(compositor.renderer(), Some(RendererKind::Pixman));
    let outputs = compositor.outputs();
    let summary: Vec<_> = outputs
        .iter()
        .map(|o| {
            (
                o.name.as_str(),
                o.spec.width,
                o.spec.height,
                o.spec.scale,
                o.position,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("HEADLESS-1", 2560, 1600, 2.0, (0, 0)),
            ("HEADLESS-2", 3440, 1440, 1.0, (1280, 0)),
        ]
    );
    Ok(())
}

#[test]
fn runs_in_a_private_runtime_directory() -> anyhow::Result<()> {
    let compositor = Headless::start(two_outputs())?;
    let dir = compositor.runtime_dir().to_path_buf();

    assert!(compositor.socket_path().starts_with(&dir));
    assert_ne!(
        Some(dir.as_os_str()),
        std::env::var_os("XDG_RUNTIME_DIR").as_deref()
    );
    drop(compositor);
    assert!(!dir.exists(), "{} was left behind", dir.display());
    Ok(())
}

#[test]
fn draws_nothing_at_rest() -> anyhow::Result<()> {
    let mut compositor = Headless::start(two_outputs())?;
    // The first frame clears both outputs.
    compositor.advance(FRAME)?;
    assert_eq!(frames(&compositor), [1, 1]);

    compositor.advance(Duration::from_secs(10))?;

    assert_eq!(frames(&compositor), [1, 1]);
    Ok(())
}

#[test]
fn a_committed_buffer_draws_one_frame_on_its_output() -> anyhow::Result<()> {
    let mut compositor = Headless::start(two_outputs())?;
    compositor.advance(FRAME)?;
    let start = compositor.now();

    let window = SolidWindow {
        app_id: "hocus-test.solid".into(),
        width: 400,
        height: 300,
        argb: 0xff_33_99_ff,
    };
    let _client = window.spawn(compositor.socket_path())?;
    compositor.dispatch_until(CLIENT_TIMEOUT, |c| c.window_count() == 1)?;
    assert_eq!(
        compositor.now(),
        start,
        "the clock moved without the harness"
    );
    assert_eq!(
        frames(&compositor),
        [1, 1],
        "a frame was drawn before its time"
    );

    compositor.advance(FRAME)?;

    // New windows open centred on the first output.
    assert_eq!(frames(&compositor), [2, 1]);
    Ok(())
}

/// GLES where there is a render node, pixman elsewhere: both must draw.
#[test]
fn the_automatic_renderer_draws() -> anyhow::Result<()> {
    let mut compositor = Headless::start(HeadlessConfig::default())?;
    let window = SolidWindow {
        app_id: "hocus-test.solid".into(),
        width: 200,
        height: 100,
        argb: 0xff_ff_66_00,
    };
    let _client = window.spawn(compositor.socket_path())?;
    compositor.dispatch_until(CLIENT_TIMEOUT, |c| c.window_count() == 1)?;

    compositor.advance(FRAME)?;

    assert_eq!(frames(&compositor), [1]);
    Ok(())
}

/// A stuck output (#27): once a frame is out and never presented, nothing
/// more is drawn there; three looks a second apart later it is recovered,
/// said, and draws again.
#[test]
fn a_stuck_output_is_recovered_after_three_looks() -> anyhow::Result<()> {
    let mut compositor = Headless::start_with(two_outputs(), Permission::Test)?;
    compositor.request(Request::Subscribe)?;
    compositor.advance(FRAME)?;
    compositor.request(Request::StallOutput {
        output: "HEADLESS-1".into(),
    })?;
    // The cursor moves: one frame drawn, never presented.
    compositor.request(Request::PointerMove { x: 100.0, y: 100.0 })?;
    compositor.advance(FRAME * 2)?;
    let stuck = frames(&compositor)[0];

    compositor.request(Request::PointerMove { x: 300.0, y: 100.0 })?;
    compositor.advance(Duration::from_millis(2500))?;
    assert_eq!(frames(&compositor)[0], stuck, "a stuck output drew on");
    let recovered = |events: &[Event]| {
        events
            .iter()
            .filter(
                |e| matches!(e, Event::OutputRecovered { name, missed: 3 } if name == "HEADLESS-1"),
            )
            .count()
    };
    compositor.request(Request::Version)?;
    assert_eq!(
        recovered(&compositor.take_events()),
        0,
        "recovered too soon"
    );

    compositor.advance(Duration::from_millis(1000))?;
    compositor.request(Request::Version)?;
    assert_eq!(recovered(&compositor.take_events()), 1);
    compositor.advance(FRAME * 2)?;
    assert!(
        frames(&compositor)[0] > stuck,
        "the recovered output does not draw"
    );
    Ok(())
}

/// One 1280×800 screen in the Matrix theme, with the shell and motion as in
/// a session, its first frame drawn.
fn matrix_screen() -> anyhow::Result<Headless> {
    let mut compositor = Headless::start_options(hocus_compositor::HarnessOptions {
        headless: HeadlessConfig {
            outputs: vec![OutputSpec::new(1280, 800)],
            renderer: RendererChoice::Pixman,
        },
        api_permission: Permission::Test,
        watchdog: None,
        xwayland: false,
        shell: true,
        session_bus: None,
        system_bus: None,
        data_dirs: None,
        child_env: None,
        polkit_helper: None,
        motion: true,
        first_run: false,
        google_caldav: None,
    })?;
    compositor.write_config("[appearance]\ntheme = \"matrix\"\n")?;
    compositor.dispatch_until(CLIENT_TIMEOUT, Headless::wallpapers_ready)?;
    compositor.advance(FRAME)?;
    Ok(compositor)
}

/// Super+L.
fn lock(compositor: &mut Headless) -> anyhow::Result<()> {
    for (code, pressed) in [(125, true), (38, true), (38, false), (125, false)] {
        compositor.request(Request::Key { code, pressed })?;
    }
    Ok(())
}

/// Something asks for a frame although nothing changed.
fn ask_for_a_frame(compositor: &mut Headless) -> anyhow::Result<()> {
    compositor.on_loop("redraw", |state| state.needs_redraw = true);
    compositor.dispatch(Duration::ZERO)?;
    compositor.advance(FRAME)
}

/// The share of the screen the last frame drawn repainted.
fn damaged(compositor: &Headless) -> f64 {
    let output = &compositor.outputs()[0];
    output.damage as f64 / (f64::from(output.spec.width) * f64::from(output.spec.height))
}

/// Pixels where the screen's buffer, as the frames drawn left it, is not
/// what a full redraw of the same moment draws.
fn stale_pixels(compositor: &mut Headless) -> anyhow::Result<usize> {
    let (shown, redrawn) = compositor.shown_and_redrawn("HEADLESS-1")?;
    anyhow::ensure!(shown.len() == redrawn.len(), "sizes differ");
    Ok(shown
        .chunks(4)
        .zip(redrawn.chunks(4))
        .filter(|(a, b)| a[..3] != b[..3])
        .count())
}

/// The Matrix theme's lock at rest (#221): its rain has stopped, and a
/// frame asked for anyway repaints nothing.
#[test]
fn a_frame_asked_for_under_the_matrix_lock_at_rest_repaints_nothing() -> anyhow::Result<()> {
    let mut compositor = matrix_screen()?;
    lock(&mut compositor)?;
    compositor.advance(Duration::from_secs(2))?;
    let before = frames(&compositor);

    for _ in 0..3 {
        ask_for_a_frame(&mut compositor)?;
    }

    assert_eq!(
        frames(&compositor),
        before,
        "a frame repainted {:.0}% of the screen",
        100.0 * damaged(&compositor)
    );
    assert_eq!(stale_pixels(&mut compositor)?, 0);
    Ok(())
}

/// The share of the screen each frame drawn between `from` and `to`
/// (since `start`) repainted; every frame leaving the screen as a full
/// redraw draws it.
fn repainted(
    compositor: &mut Headless,
    start: Duration,
    (from, to): (u64, u64),
) -> anyhow::Result<Vec<f64>> {
    let (from, to) = (
        start + Duration::from_millis(from),
        start + Duration::from_millis(to),
    );
    compositor.advance(from.saturating_sub(compositor.now()))?;
    let mut shares = Vec::new();
    while compositor.now() + FRAME <= to {
        let before = frames(compositor);
        compositor.advance(FRAME)?;
        if frames(compositor) != before {
            shares.push(damaged(compositor));
            let stale = stale_pixels(compositor)?;
            anyhow::ensure!(
                stale == 0,
                "{stale} pixels left stale at {:?}",
                compositor.now()
            );
        }
    }
    eprintln!("repainted per frame: {shares:.3?}");
    Ok(shares)
}

/// While the lock's rain cascades over the desktop, then slows to a stop
/// (#221), a frame repaints the drops and the black they gain, not the
/// screen: the drops are 8 pixels wide in columns 16 apart and the black
/// grows by a few rows a frame, so they never reach half of it. (In
/// between, the lock itself fades in, repainted whole.)
#[test]
fn the_matrix_lock_rain_repaints_only_its_drops() -> anyhow::Result<()> {
    let mut compositor = matrix_screen()?;
    let start = compositor.now();
    lock(&mut compositor)?;
    // Its first frame brings the lock: all of it is new.
    compositor.advance(FRAME)?;

    let cascade = repainted(&mut compositor, start, (FRAME.as_millis() as u64, 540))?;
    let slowing = repainted(&mut compositor, start, (800, 1000))?;

    assert!(cascade.len() >= 25, "the rain did not move: {cascade:?}");
    assert!(slowing.len() >= 10, "the rain did not move: {slowing:?}");
    let most = cascade.iter().chain(&slowing).copied().fold(0.0, f64::max);
    assert!(
        most < 0.5,
        "a frame repainted {:.0}% of the screen",
        100.0 * most
    );
    Ok(())
}

/// Behind Alt+Tab (#221) a few slow drops fall while it shows: a frame
/// repaints them and not the screen. About one column in three has a
/// drop, its glyphs 8 pixels wide in columns 28 apart, a third of the
/// screen high at most: a tenth of it leaves room.
#[test]
fn the_matrix_switcher_rain_repaints_only_its_drops() -> anyhow::Result<()> {
    let mut compositor = matrix_screen()?;
    let mut clients = Vec::new();
    for (app_id, argb) in [("mail", 0xff_e5_48_4d), ("editor", 0xff_33_cc_66)] {
        let window = SolidWindow {
            app_id: app_id.into(),
            width: 600,
            height: 420,
            argb,
        };
        clients.push(window.spawn(compositor.socket_path())?);
        let n = clients.len();
        compositor.dispatch_until(CLIENT_TIMEOUT, |c| c.window_count() == n)?;
    }
    compositor.advance(Duration::from_millis(600))?;
    compositor.request(Request::Key {
        code: 56,
        pressed: true,
    })?;
    for pressed in [true, false] {
        compositor.request(Request::Key { code: 15, pressed })?;
    }
    let start = compositor.now();
    // From when it has faded in over the dark.
    let shares = repainted(&mut compositor, start, (300, 640))?;

    assert!(shares.len() >= 10, "the rain did not move: {shares:?}");
    let most = shares.iter().copied().fold(0.0, f64::max);
    assert!(
        most < 0.1,
        "a frame repainted {:.0}% of the screen",
        100.0 * most
    );
    Ok(())
}

/// Real time passing changes nothing on the screen (#212): what falls due
/// does on the virtual clock, so a test waiting for a client between two
/// steps sees the frames one that does not wait sees. The calendar's first
/// sync (2 s) used to wake the loop on real time, and a redraw under the
/// Matrix theme's lock is never empty.
#[test]
fn real_time_passing_draws_nothing_at_rest() -> anyhow::Result<()> {
    let mut compositor = Headless::start_options(hocus_compositor::HarnessOptions {
        headless: HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..Default::default()
        },
        api_permission: Permission::Test,
        watchdog: None,
        xwayland: false,
        shell: true,
        session_bus: None,
        system_bus: None,
        data_dirs: None,
        child_env: None,
        polkit_helper: None,
        motion: true,
        first_run: false,
        google_caldav: None,
    })?;
    compositor.write_config("[appearance]\ntheme = \"matrix\"\n")?;
    compositor.dispatch_until(CLIENT_TIMEOUT, Headless::wallpapers_ready)?;
    compositor.advance(FRAME)?;
    // Super+L: locked, the rain comes down and stops.
    for (code, pressed) in [(125, true), (38, true), (38, false), (125, false)] {
        compositor.request(Request::Key { code, pressed })?;
    }
    compositor.advance(Duration::from_secs(3))?;
    let before = frames(&compositor);
    let until = std::time::Instant::now() + Duration::from_millis(2500);
    while std::time::Instant::now() < until {
        compositor.dispatch(Duration::from_millis(20))?;
    }
    compositor.advance(Duration::from_secs(1))?;
    assert_eq!(frames(&compositor), before, "a frame was drawn at rest");
    Ok(())
}
