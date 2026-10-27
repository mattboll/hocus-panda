//! Budgets of the desktop at rest (#88), counted rather than timed: frames
//! drawn, scenes built, what wakes the loop, what it allocates and holds.
//! Counts are the same on every machine however loaded, so they gate CI;
//! times are measured apart (`timings`), and only reported. Twenty windows
//! and desktops switching are the `budget-twenty-windows` scenario.

use std::time::{Duration, Instant};

use hocus_api::{HandlerRuns, Permission, Stats};
use hocus_compositor::{
    HarnessOptions, Headless, HeadlessConfig, OutputSpec, RendererChoice, WatchdogConfig,
    allocations::{self, Counting},
};

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Long enough for a worker on a loaded CI machine.
const TIMEOUT: Duration = Duration::from_secs(10);

/// What the loop may allocate in a minute at rest on a virtual clock,
/// where the harness turns it twice a frame (7,200 turns): measured at
/// 209,500, 29 a turn (the column's state is worked out again after each
/// turn), with a margin of 10 %.
const REST_ALLOCATIONS: u64 = 230_000;

/// What the loop may hold more after that minute. The minute's new digits
/// are laid out and kept (about 316 kB measured), and whether a buffer of
/// the start is let go before the minute or during it depends on when its
/// release came, so `held` is either that or 4 MB less (#269). A leak at
/// rest grows minute after minute: over 90 minutes the loop held 190 kB
/// more in all, the wallpapers' light aside.
const REST_HELD: i64 = 512 * 1024;

/// The desktop of the scenarios: a laptop panel at scale 2 and a wide
/// screen, the shell, windows moving as in a session.
fn options(watchdog: Option<WatchdogConfig>) -> HarnessOptions {
    HarnessOptions {
        headless: HeadlessConfig {
            outputs: vec![
                OutputSpec::new(2560, 1600).scale(2.0),
                OutputSpec::new(3440, 1440),
            ],
            renderer: RendererChoice::Pixman,
        },
        api_permission: Permission::Test,
        watchdog,
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
    }
}

/// Each loop handler that ran between `before` and `after`, and how many
/// times.
fn ran(before: &Stats, after: &Stats) -> Vec<HandlerRuns> {
    after
        .handlers
        .iter()
        .filter_map(|h| {
            let was = before
                .handlers
                .iter()
                .find(|b| b.name == h.name)
                .map_or(0, |b| b.runs);
            (h.runs > was).then(|| HandlerRuns {
                name: h.name.clone(),
                runs: h.runs - was,
            })
        })
        .collect()
}

fn per_output(
    before: &Stats,
    after: &Stats,
    count: fn(&hocus_api::OutputStats) -> u64,
) -> Vec<u64> {
    after
        .outputs
        .iter()
        .zip(&before.outputs)
        .map(|(a, b)| count(a) - count(b))
        .collect()
}

/// Given the desktop at rest with two outputs, when measured for 60 s on
/// a virtual clock, then nothing wakes the loop, nothing is drawn but the
/// column's clock turning a minute, and nothing more is held.
#[test]
fn at_rest_two_screens_draw_only_the_minute_turning() -> anyhow::Result<()> {
    assert!(allocations::installed(), "nothing would be counted");
    let mut desktop = Headless::start_options(options(None))?;
    desktop.dispatch_until(TIMEOUT, Headless::wallpapers_ready)?;
    // The column comes in, the first frames are drawn.
    desktop.advance(Duration::from_secs(2))?;
    let (before, counted) = (desktop.stats(), allocations::counted());

    // 14:37:02 to 14:38:02 on the column's clock.
    desktop.advance(Duration::from_secs(60))?;

    let after = desktop.stats();
    let allocated = allocations::counted().since(&counted);
    let frames = per_output(&before, &after, |o| o.frames);
    let renders = per_output(&before, &after, |o| o.renders);
    eprintln!("frames {frames:?}, scenes built {renders:?}, {allocated:?}");
    assert_eq!(frames, [1, 0], "frames drawn at rest");
    // One turn of the minute asks both screens again (follow-up: only the
    // column's).
    assert_eq!(renders, [1, 1], "scenes built at rest");
    assert_eq!(ran(&before, &after), [], "handlers ran at rest");
    assert!(
        allocated.count <= REST_ALLOCATIONS,
        "{} allocations at rest, budget {REST_ALLOCATIONS}",
        allocated.count
    );
    assert!(
        allocated.held <= REST_HELD,
        "{} bytes more held, budget {REST_HELD}",
        allocated.held
    );
    Ok(())
}

/// Given the desktop at rest with two outputs, when measured for 60 s on
/// real time, then the loop wakes for the watchdog, and otherwise only for
/// what the clock shows: the column's minute, the wallpaper's light every
/// ten minutes, and the session saved after something happened.
#[test]
fn at_rest_the_loop_wakes_only_for_the_watchdog() -> anyhow::Result<()> {
    let dumps = tempfile::tempdir()?;
    let mut desktop = Headless::start_real_time(options(Some(WatchdogConfig::new(
        dumps.path().to_path_buf(),
    ))))?;
    // What comes once at the start: the wallpapers, the system's fonts.
    desktop.dispatch_until(TIMEOUT, |d| {
        d.wallpapers_ready() && d.stats().handlers.iter().any(|h| h.name == "system fonts")
    })?;
    let settled = Instant::now() + Duration::from_secs(1);
    while Instant::now() < settled {
        desktop.dispatch(Duration::from_millis(50))?;
    }
    let before = desktop.stats();

    let end = Instant::now() + Duration::from_secs(60);
    while Instant::now() < end {
        desktop.dispatch(Duration::from_millis(100))?;
    }

    let after = desktop.stats();
    let woken = ran(&before, &after);
    eprintln!("woken by {woken:?}");
    // What the clock shows, at most: a minute turning once or twice, a
    // ten minutes' light on each screen, a frame for each, and the
    // session saved after the start's activity and after the light.
    let allowed: [(&str, u64); 5] = [
        ("column clock", 2),
        ("wallpaper", 2),
        ("headless frame", 4),
        ("session save", 2),
        ("watchdog", u64::MAX),
    ];
    for handler in &woken {
        let most = allowed
            .iter()
            .find(|(name, _)| *name == handler.name)
            .map_or(0, |(_, most)| *most);
        assert!(
            handler.runs <= most,
            "the loop woke {} times for {} at rest; all: {woken:?}",
            handler.runs,
            handler.name
        );
    }
    let watchdog = woken.iter().find(|h| h.name == "watchdog");
    assert!(watchdog.is_some(), "the watchdog never woke the loop");
    Ok(())
}
