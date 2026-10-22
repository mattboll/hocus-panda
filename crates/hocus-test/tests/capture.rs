//! Screen capture through the protocols (#114): grim gets each output
//! pixel for pixel as the compositor's own screenshot.

use std::{
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

use hocus_api::{Reply, Request, Target};
use hocus_compositor::{FRAME, Headless, HeadlessConfig, OutputSpec, RendererChoice};
use hocus_test::{client::SolidWindow, image::Image};

const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

/// RGB pixels of a PNG, whatever its colour type.
fn rgb(png: &[u8]) -> anyhow::Result<(u32, u32, Vec<[u8; 3]>)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buf)?;
    let channels = info.color_type.samples();
    let pixels = buf[..info.buffer_size()]
        .chunks_exact(channels)
        .map(|p| [p[0], p[1], p[2]])
        .collect();
    Ok((info.width, info.height, pixels))
}

/// Runs grim on the compositor's socket only, the loop turning meanwhile.
fn grim(compositor: &mut Headless, output: &str, file: &Path) -> anyhow::Result<()> {
    let socket = compositor.socket_path().to_path_buf();
    let mut child = Command::new("grim")
        .args(["-o", output])
        .arg(file)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("XDG_RUNTIME_DIR", socket.parent().unwrap_or(Path::new("/")))
        .env("WAYLAND_DISPLAY", socket.file_name().unwrap_or_default())
        .spawn()?;
    let deadline = Instant::now() + CLIENT_TIMEOUT;
    loop {
        if let Some(status) = child.try_wait()? {
            anyhow::ensure!(status.success(), "grim failed: {status}");
            return Ok(());
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            anyhow::bail!("grim never finished");
        }
        compositor.dispatch(Duration::from_millis(5))?;
    }
}

#[test]
fn grim_captures_each_output_as_the_compositor_does() -> anyhow::Result<()> {
    if Command::new("grim").arg("-h").output().is_err() {
        eprintln!("skipped: no grim");
        return Ok(());
    }
    let outputs: Vec<OutputSpec> = ["640x400", "320x200@2+640+0"]
        .iter()
        .map(|s| s.parse())
        .collect::<Result<_, _>>()?;
    let mut compositor = Headless::start(HeadlessConfig {
        outputs,
        renderer: RendererChoice::Pixman,
    })?;
    let window = SolidWindow {
        app_id: "editor".into(),
        width: 200,
        height: 120,
        argb: 0xff_33_99_ff,
    };
    let _client = window.spawn(compositor.socket_path())?;
    compositor.dispatch_until(CLIENT_TIMEOUT, |c| c.window_count() == 1)?;
    compositor.advance(FRAME * 2)?;
    let dir = tempfile::tempdir()?;
    for name in ["HEADLESS-1", "HEADLESS-2"] {
        let Reply::Screenshot(shot) = compositor.request(Request::Screenshot {
            target: Target::Output(name.into()),
            cursor: false,
        })?
        else {
            anyhow::bail!("expected a screenshot");
        };
        let own = Image::from_png(&shot.png()?)?;
        let file = dir.path().join(format!("{name}.png"));
        grim(&mut compositor, name, &file)?;
        let (w, h, pixels) = rgb(&std::fs::read(&file)?)?;
        assert_eq!((w, h), (own.width, own.height), "{name}: size");
        let differ = own
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .zip(&pixels)
            .filter(|(a, b)| a[..3] != b[..])
            .count();
        assert_eq!(differ, 0, "{name}: {differ} pixels differ");
    }
    Ok(())
}

#[test]
fn screencopy_copies_a_region_in_the_output_s_pixels() -> anyhow::Result<()> {
    let outputs: Vec<OutputSpec> = ["320x200@2"]
        .iter()
        .map(|s| s.parse())
        .collect::<Result<_, _>>()?;
    let mut compositor = Headless::start(HeadlessConfig {
        outputs,
        renderer: RendererChoice::Pixman,
    })?;
    let window = SolidWindow {
        app_id: "editor".into(),
        width: 100,
        height: 60,
        argb: 0xff_33_99_ff,
    };
    let _client = window.spawn(compositor.socket_path())?;
    compositor.dispatch_until(CLIENT_TIMEOUT, |c| c.window_count() == 1)?;
    compositor.advance(FRAME * 2)?;
    let Reply::Screenshot(shot) = compositor.request(Request::Screenshot {
        target: Target::Output("HEADLESS-1".into()),
        cursor: false,
    })?
    else {
        anyhow::bail!("expected a screenshot");
    };
    let own = Image::from_png(&shot.png()?)?;
    let socket = compositor.socket_path().to_path_buf();
    let (done, result) = std::sync::mpsc::channel();
    // A logical region: twice as many pixels at scale 2.
    std::thread::spawn(move || {
        let _ = done.send(hocus_test::screencopy::region(
            &socket,
            0,
            (100, 60, 50, 40),
        ));
    });
    let mut got = None;
    compositor.dispatch_until(CLIENT_TIMEOUT, |_| {
        got = got.take().or_else(|| result.try_recv().ok());
        got.is_some()
    })?;
    let (w, h, bytes) = got.expect("the copy finished")?;
    assert_eq!((w, h), (100, 80));
    for row in 0..h {
        for col in 0..w {
            let i = ((row * w + col) * 4) as usize;
            let got = [bytes[i + 2], bytes[i + 1], bytes[i]];
            let want = own.pixel(200 + col, 120 + row).expect("inside");
            assert_eq!(got, want[..3], "{col},{row}");
        }
    }
    Ok(())
}
