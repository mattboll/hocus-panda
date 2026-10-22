//! Acceptance scenarios of frame timing, handler budgets and the watchdog (#8).

use std::{
    io::Write,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use hocus_api::{Reply, Request, Stats};
use hocus_compositor::{HarnessOptions, Headless, HeadlessConfig, RendererChoice, WatchdogConfig};
use hocus_test::client::SolidWindow;

fn compositor(dump_dir: &std::path::Path, limit: Duration) -> anyhow::Result<Headless> {
    Headless::start_options(HarnessOptions {
        headless: HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..Default::default()
        },
        api_permission: hocus_api::Permission::Test,
        watchdog: Some(WatchdogConfig {
            limit,
            interval: Duration::from_millis(20),
            dump_dir: dump_dir.to_path_buf(),
        }),
        xwayland: false,
    })
}

fn stats(compositor: &mut Headless) -> anyhow::Result<Stats> {
    match compositor.request(Request::Stats)? {
        Reply::Stats(stats) => Ok(stats),
        other => anyhow::bail!("expected stats, got {other:?}"),
    }
}

/// Runs the loop for `time` of real time.
fn keep_running(compositor: &mut Headless, time: Duration) -> anyhow::Result<()> {
    let end = Instant::now() + time;
    while Instant::now() < end {
        compositor.dispatch(Duration::from_millis(5))?;
    }
    Ok(())
}

#[test]
fn a_quiet_screen_is_not_a_stuck_one() -> anyhow::Result<()> {
    let dumps = tempfile::tempdir()?;
    let mut compositor = compositor(dumps.path(), Duration::from_millis(300))?;

    // A minute with nothing to draw, then a moment for the watchdog to look.
    compositor.advance(Duration::from_secs(60))?;
    keep_running(&mut compositor, Duration::from_millis(600))?;

    let stats = stats(&mut compositor)?;
    assert_eq!(stats.stalls, []);
    assert_eq!(std::fs::read_dir(dumps.path())?.count(), 0, "no dump");
    Ok(())
}

#[test]
fn frames_drawn_are_timed_per_output() -> anyhow::Result<()> {
    let dumps = tempfile::tempdir()?;
    let mut compositor = compositor(dumps.path(), Duration::from_secs(10))?;
    compositor.advance(hocus_compositor::FRAME)?;
    let window = SolidWindow {
        app_id: "timed".into(),
        width: 100,
        height: 100,
        argb: 0xff00_ff00,
    };
    let _client = window.spawn(compositor.socket_path())?;
    compositor.dispatch_until(Duration::from_secs(10), |c| c.window_count() == 1)?;
    compositor.advance(hocus_compositor::FRAME * 2)?;

    let stats = stats(&mut compositor)?;
    let output = &stats.outputs[0];
    assert_eq!(output.name, "HEADLESS-1");
    // The first frame, then the window's.
    assert_eq!(output.frames, 2);
    assert_eq!(output.histogram.iter().map(|b| b.count).sum::<u64>(), 2);
    assert!(output.max_ms > 0.0);
    Ok(())
}

/// Collects what is logged on this thread.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Ok(mut log) = self.0.lock() {
            log.extend_from_slice(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_slow_handler_is_named_with_its_duration() -> anyhow::Result<()> {
    // A global subscriber: with `cargo test`, tests share the process, and a
    // thread-local one misses events whose callsite another test cached.
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;
    let dumps = tempfile::tempdir()?;
    let mut compositor = compositor(dumps.path(), Duration::from_secs(10))?;

    compositor.on_loop("test sleeper", |_| {
        std::thread::sleep(Duration::from_secs(3))
    });
    compositor.dispatch(Duration::ZERO)?;

    let log = String::from_utf8_lossy(&captured.0.lock().map(|l| l.clone()).unwrap_or_default())
        .into_owned();
    let line = log
        .lines()
        .find(|l| l.contains("loop handler over budget") && l.contains("test sleeper"))
        .unwrap_or_default();
    assert!(line.contains("WARN"), "{log}");
    assert!(line.contains(r#"handler="test sleeper""#), "{log}");
    let ms: u64 = line
        .split("ms=")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|ms| ms.parse().ok())
        .unwrap_or_default();
    assert!((3000..4000).contains(&ms), "{line}");

    let stats = stats(&mut compositor)?;
    assert!(
        stats
            .slow_handlers
            .iter()
            .any(|h| h.name == "test sleeper" && h.ms >= 3000.0),
        "{stats:?}"
    );
    Ok(())
}

#[test]
fn a_blocked_loop_leaves_a_dump_of_every_thread() -> anyhow::Result<()> {
    let dumps = tempfile::tempdir()?;
    let mut compositor = compositor(dumps.path(), Duration::from_secs(1))?;
    keep_running(&mut compositor, Duration::from_millis(100))?;

    compositor.on_loop("stuck", |_| std::thread::sleep(Duration::from_secs(3)));
    compositor.dispatch(Duration::ZERO)?;
    // The watchdog's report comes back once the loop runs again.
    compositor.dispatch_until(Duration::from_secs(30), |_| {
        std::fs::read_dir(dumps.path()).is_ok_and(|d| d.count() > 0)
    })?;
    keep_running(&mut compositor, Duration::from_millis(200))?;

    let stats = stats(&mut compositor)?;
    let [stall] = stats.stalls.as_slice() else {
        anyhow::bail!("expected one stall, got {:?}", stats.stalls);
    };
    assert!(stall.seconds > 1.0, "{stall:?}");
    let dump = stall.dump.clone().unwrap_or_default();
    let text = std::fs::read_to_string(&dump)?;
    assert!(text.contains("the event loop has not answered"), "{text}");
    // Every thread is listed: this one (the loop's), the watchdog, and more.
    assert!(text.contains("watchdog"), "{text}");
    let threads = text
        .split("== threads")
        .nth(1)
        .and_then(|rest| rest.split("== backtraces").next())
        .unwrap_or_default();
    assert!(
        threads.lines().filter(|l| !l.trim().is_empty()).count() >= 2,
        "{text}"
    );
    // User-space backtraces come from gdb, when it is installed (it is in CI).
    let has_gdb = std::process::Command::new("gdb")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if has_gdb {
        let backtraces = text.split("== backtraces").nth(1).unwrap_or_default();
        assert!(backtraces.contains("Thread"), "{text}");
        assert!(
            backtraces.contains("sleep"),
            "the stuck handler shows: {text}"
        );
    }
    Ok(())
}
