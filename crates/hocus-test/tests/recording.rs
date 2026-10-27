//! Screen recording (#70), through the encoder for real: a region recorded
//! five seconds plays at its size, and a click rings in the video and not
//! on the screen. What held the loop meanwhile is reported, not failed: its
//! times are the machine's load as much as the code's (#298). Needs ffmpeg
//! and ffprobe; skipped without them.

use std::{
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

use hocus_api::{Button, Event, Reply, Request, Target};
use hocus_compositor::{HarnessOptions, Headless, HeadlessConfig, RendererChoice};
use hocus_test::bus::installed;

const LEFT_META: u32 = 125;
const LEFT_SHIFT: u32 = 42;
const KEY_R: u32 = 19;

fn keys(compositor: &mut Headless, codes: &[u32]) -> anyhow::Result<()> {
    for code in codes {
        compositor.request(Request::Key {
            code: *code,
            pressed: true,
        })?;
    }
    for code in codes.iter().rev() {
        compositor.request(Request::Key {
            code: *code,
            pressed: false,
        })?;
    }
    Ok(())
}

fn click(compositor: &mut Headless, x: f64, y: f64) -> anyhow::Result<()> {
    compositor.request(Request::PointerMove { x, y })?;
    for pressed in [true, false] {
        compositor.request(Request::PointerButton {
            button: Button::Left,
            pressed,
        })?;
    }
    Ok(())
}

/// Moves the virtual clock by `by`, a frame at a time, giving the encoder
/// real time to keep up as it would have.
fn run(compositor: &mut Headless, by: Duration) -> anyhow::Result<()> {
    let step = Duration::from_millis(100);
    let mut gone = Duration::ZERO;
    while gone < by {
        compositor.advance(step.min(by - gone))?;
        compositor.dispatch(Duration::from_millis(15))?;
        gone += step;
    }
    Ok(())
}

/// `width`, `height` and the duration ffprobe reads.
fn probe(path: &Path) -> anyhow::Result<(u32, u32, f64)> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0"])
        .args(["-show_entries", "stream=width,height:format=duration"])
        .args(["-of", "default=noprint_wrappers=1"])
        .arg(path)
        .output()?;
    anyhow::ensure!(
        out.status.success(),
        "ffprobe: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout)?;
    let value = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{key}=")))
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("no {key} in {text}"))
    };
    Ok((
        value("width")?.parse()?,
        value("height")?.parse()?,
        value("duration")?.parse()?,
    ))
}

/// The video's frame at `at` seconds, as RGB rows.
fn frame_at(path: &Path, at: f64) -> anyhow::Result<Vec<u8>> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-ss", &format!("{at:.3}"), "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()?;
    anyhow::ensure!(
        out.status.success(),
        "ffmpeg: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(out.stdout)
}

#[test]
fn a_region_recorded_plays_at_its_size_with_clicks_ringed_in_it_only() -> anyhow::Result<()> {
    if !installed("ffmpeg") || !installed("ffprobe") {
        eprintln!("skipped: no ffmpeg");
        return Ok(());
    }
    let folder = tempfile::tempdir()?;
    let mut compositor = Headless::start_options(HarnessOptions {
        headless: HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..Default::default()
        },
        api_permission: hocus_api::Permission::Test,
        watchdog: None,
        xwayland: false,
        shell: true,
        session_bus: None,
        system_bus: None,
        data_dirs: None,
        child_env: None,
        polkit_helper: None,
        motion: false,
        first_run: false,
        google_caldav: None,
        google_tasks: None,
    })?;
    compositor.request(Request::Subscribe)?;
    compositor.write_config(&format!(
        "[recording]\nfolder = {:?}\nhardware = false\ncountdown = 1\n",
        folder.path().display().to_string()
    ))?;

    // Super+Shift+R, then a region drawn: 320×240 at (100, 100).
    keys(&mut compositor, &[LEFT_META, LEFT_SHIFT, KEY_R])?;
    compositor.request(Request::PointerMove { x: 100.0, y: 100.0 })?;
    compositor.request(Request::PointerButton {
        button: Button::Left,
        pressed: true,
    })?;
    compositor.request(Request::PointerMove { x: 420.0, y: 340.0 })?;
    compositor.request(Request::PointerButton {
        button: Button::Left,
        pressed: false,
    })?;
    // The countdown, then two seconds, a click, three seconds.
    run(&mut compositor, Duration::from_millis(1000))?;
    run(&mut compositor, Duration::from_secs(2))?;
    click(&mut compositor, 260.0, 220.0)?;
    compositor.dispatch(Duration::from_millis(15))?;
    // The screen has no ring.
    let Reply::Screenshot(live) = compositor.request(Request::Screenshot {
        target: Target::Output("HEADLESS-1".into()),
        cursor: false,
    })?
    else {
        anyhow::bail!("expected a screenshot");
    };
    let live = hocus_test::image::Image::from_png(&live.png()?)?;
    let around = (270..278)
        .filter_map(|x| live.pixel(x, 220))
        .map(|p| p[0])
        .max()
        .unwrap_or(0);
    assert!(around < 200, "a ring on the live screen: {around}");
    run(&mut compositor, Duration::from_millis(3000))?;
    keys(&mut compositor, &[LEFT_META, LEFT_SHIFT, KEY_R])?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let path = loop {
        let saved = compositor.take_events().into_iter().find_map(|e| match e {
            Event::Recording { path, .. } => path,
            _ => None,
        });
        if let Some(path) = saved {
            break path;
        }
        anyhow::ensure!(Instant::now() < deadline, "the recording was never saved");
        compositor.dispatch(Duration::from_millis(20))?;
        // Events come in with a reply.
        compositor.request(Request::Stats)?;
    };
    let path = Path::new(&path);

    let (w, h, duration) = probe(path)?;
    assert_eq!((w, h), (320, 240));
    assert!((4.8..=5.3).contains(&duration), "it lasts {duration} s");

    // The ring, just after the click: its right side at about 12 pixels
    // from where it was, white, in the region's own pixels (160, 120).
    let rgb = frame_at(path, 2.05)?;
    let at = |x: usize, y: usize| rgb[(y * 320 + x) * 3];
    let ring = (170..180).map(|x| at(x, 120)).max().unwrap_or(0);
    assert!(ring > 180, "no ring in the video: {ring}");

    hocus_test::report_held(&compositor.stats());
    Ok(())
}
