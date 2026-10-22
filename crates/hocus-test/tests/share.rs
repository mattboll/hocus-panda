//! Choosing what to share (#116): the portal's chooser asks over the
//! control API, as `hocus share-pick` does, and waits; the shell shows the
//! screens and the windows; the answer is what was clicked, or nothing.

use std::time::{Duration, Instant};

use hocus_api::{Button, Client, Reply, Request};
use hocus_compositor::{FRAME, Headless, HeadlessConfig, RendererChoice};
use hocus_test::client::SolidWindow;

const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

/// Asks on a thread of its own, as the chooser program would.
fn ask(compositor: &Headless) -> std::thread::JoinHandle<anyhow::Result<Option<String>>> {
    let socket = compositor.api_socket().to_path_buf();
    std::thread::spawn(move || {
        let mut client = Client::connect(&socket)?;
        client.set_timeout(None)?;
        match client.request(Request::PickShare)? {
            Reply::Shared(answer) => Ok(answer),
            other => anyhow::bail!("unexpected {other:?}"),
        }
    })
}

/// The shell's element with this label, and where its centre is.
fn find(compositor: &mut Headless, label: &str) -> anyhow::Result<Option<(f64, f64)>> {
    let Reply::Ui(nodes) = compositor.request(Request::Ui)? else {
        anyhow::bail!("expected the interface");
    };
    Ok(nodes
        .into_iter()
        .find(|n| n.label.as_deref() == Some(label))
        .map(|n| {
            let r = n.rect;
            (
                f64::from(r.x) + f64::from(r.width) / 2.0,
                f64::from(r.y) + f64::from(r.height) / 2.0,
            )
        }))
}

fn wait_for(compositor: &mut Headless, label: &str) -> anyhow::Result<(f64, f64)> {
    let deadline = Instant::now() + CLIENT_TIMEOUT;
    loop {
        if let Some(at) = find(compositor, label)? {
            return Ok(at);
        }
        anyhow::ensure!(Instant::now() < deadline, "{label:?} never showed");
        compositor.dispatch(Duration::from_millis(10))?;
    }
}

fn click(compositor: &mut Headless, (x, y): (f64, f64)) -> anyhow::Result<()> {
    compositor.request(Request::PointerMove { x, y })?;
    for pressed in [true, false] {
        compositor.request(Request::PointerButton {
            button: Button::Left,
            pressed,
        })?;
    }
    Ok(())
}

fn answer(
    compositor: &mut Headless,
    asking: std::thread::JoinHandle<anyhow::Result<Option<String>>>,
) -> anyhow::Result<Option<String>> {
    let deadline = Instant::now() + CLIENT_TIMEOUT;
    while !asking.is_finished() {
        anyhow::ensure!(Instant::now() < deadline, "never answered");
        compositor.dispatch(Duration::from_millis(10))?;
    }
    asking
        .join()
        .map_err(|_| anyhow::anyhow!("the asking thread panicked"))?
}

#[test]
fn the_chooser_gets_the_screen_or_window_clicked_or_nothing() -> anyhow::Result<()> {
    let mut compositor = Headless::start(HeadlessConfig {
        renderer: RendererChoice::Pixman,
        ..Default::default()
    })?;
    let window = SolidWindow {
        app_id: "slides".into(),
        width: 400,
        height: 300,
        argb: 0xff_e8_e4_da,
    };
    let _client = window.spawn(compositor.socket_path())?;
    compositor.dispatch_until(CLIENT_TIMEOUT, |c| c.window_count() == 1)?;
    compositor.advance(FRAME * 2)?;

    // A window, by its foreign-toplevel identifier.
    let asking = ask(&compositor);
    let at = wait_for(&mut compositor, "slides · slides")?;
    click(&mut compositor, at)?;
    let chosen = answer(&mut compositor, asking)?;
    assert!(
        chosen
            .as_deref()
            .is_some_and(|a| a.starts_with("Window: ") && a.len() > 8),
        "{chosen:?}"
    );
    assert!(
        find(&mut compositor, "slides · slides")?.is_none(),
        "the picker closed"
    );

    // A screen.
    let asking = ask(&compositor);
    let at = wait_for(&mut compositor, "HEADLESS-1 · 1920×1080")?;
    click(&mut compositor, at)?;
    assert_eq!(
        answer(&mut compositor, asking)?.as_deref(),
        Some("Monitor: HEADLESS-1")
    );

    // Nothing.
    let asking = ask(&compositor);
    let at = wait_for(&mut compositor, "Share nothing")?;
    click(&mut compositor, at)?;
    assert_eq!(answer(&mut compositor, asking)?, None);
    Ok(())
}
