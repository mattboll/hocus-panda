//! The clipboard history on disk (#71): kept encrypted, and what a
//! password manager marks secret is never there.

use std::time::{Duration, Instant};

use hocus_compositor::{Headless, HeadlessConfig, RendererChoice};
use hocus_platform::sealed::Sealed;
use hocus_test::client::SolidWindow;

#[test]
fn copies_are_kept_encrypted_and_secrets_never() -> anyhow::Result<()> {
    let mut compositor = Headless::start(HeadlessConfig {
        renderer: RendererChoice::Pixman,
        ..Default::default()
    })?;
    let client = SolidWindow {
        app_id: "editor".into(),
        width: 300,
        height: 200,
        argb: 0xff_33_66_cc,
    }
    .spawn(compositor.socket_path())?;
    compositor.dispatch_until(Duration::from_secs(10), |c| c.window_count() == 1)?;
    client.copy("a note worth keeping")?;
    // Read before the secret replaces it: a copy replaced at once is not.
    compositor.dispatch_until(Duration::from_secs(10), |c| {
        c.clipboard_quiet() == (1, true)
    })?;
    client.copy_secret("hunter2")?;
    let store = compositor.state_dir().join("clipboard");
    let deadline = Instant::now() + Duration::from_secs(10);
    let kept = loop {
        compositor.dispatch(Duration::from_millis(20))?;
        if let Ok(Some(bytes)) = Sealed::new(&store).read() {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            if text.contains("a note worth keeping") {
                break text;
            }
        }
        anyhow::ensure!(Instant::now() < deadline, "the history was never written");
    };
    // A moment for the secret copy to be (not) read.
    let until = Instant::now() + Duration::from_millis(300);
    while Instant::now() < until {
        compositor.dispatch(Duration::from_millis(20))?;
    }
    let kept_now = Sealed::new(&store)
        .read()
        .map_err(anyhow::Error::msg)?
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or(kept);
    assert!(
        !kept_now.contains("hunter2"),
        "a secret was kept: {kept_now}"
    );
    let raw = std::fs::read(&store)?;
    assert!(
        !raw.windows(4).any(|w| w == b"note"),
        "the history is stored in clear"
    );
    Ok(())
}
