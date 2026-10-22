//! Screen tools (#28): wlr-output-management lists every screen with its
//! modes and sets them for the session; a configuration made against an
//! older state is cancelled, one no screen can do fails; output-power turns
//! a screen's light off and on.

use std::{
    thread::JoinHandle,
    time::{Duration, Instant},
};

use hocus_compositor::{FRAME, Headless, HeadlessConfig, OutputSpec, RendererChoice};
use hocus_test::randr::{self, Change, Outcome};

const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

fn two_outputs() -> HeadlessConfig {
    HeadlessConfig {
        outputs: vec![OutputSpec::new(2560, 1600), OutputSpec::new(1920, 1080)],
        renderer: RendererChoice::Pixman,
    }
}

/// Runs a client on its thread while the compositor turns.
fn beside<T: Send + 'static>(
    compositor: &mut Headless,
    client: impl FnOnce(std::path::PathBuf) -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    let socket = compositor.socket_path().to_path_buf();
    let thread: JoinHandle<anyhow::Result<T>> = std::thread::spawn(move || client(socket));
    let deadline = Instant::now() + CLIENT_TIMEOUT;
    while !thread.is_finished() {
        anyhow::ensure!(Instant::now() < deadline, "the client never finished");
        compositor.dispatch(Duration::from_millis(5))?;
    }
    thread
        .join()
        .map_err(|_| anyhow::anyhow!("the client panicked"))?
}

#[test]
fn every_screen_is_listed_with_its_modes() -> anyhow::Result<()> {
    let mut compositor = Headless::start(two_outputs())?;
    let heads = beside(&mut compositor, |s| randr::heads(&s))?;
    let names: Vec<&str> = heads.iter().map(|h| h.name.as_str()).collect();
    assert_eq!(names, ["HEADLESS-1", "HEADLESS-2"]);
    let laptop = &heads[0];
    assert!(laptop.enabled);
    assert_eq!(
        laptop.current_mode.map(|i| laptop.modes[i]),
        Some((2560, 1600, 60_000))
    );
    assert!(laptop.modes.contains(&(1920, 1080, 60_000)));
    assert_eq!(heads[1].position, (2560, 0));
    Ok(())
}

#[test]
fn a_test_changes_nothing_a_stale_one_is_cancelled_an_impossible_one_fails() -> anyhow::Result<()> {
    let mut compositor = Headless::start(two_outputs())?;
    let scale = Change {
        output: "HEADLESS-1".into(),
        scale: Some(2.0),
        ..Default::default()
    };
    let tested = beside(&mut compositor, {
        let scale = scale.clone();
        move |s| randr::apply(&s, &scale, true, false)
    })?;
    assert_eq!(tested, Outcome::Succeeded);
    assert_eq!(
        compositor.outputs()[0].spec.scale,
        1.0,
        "a test changed the screen"
    );

    let stale = beside(&mut compositor, {
        let scale = scale.clone();
        move |s| randr::apply(&s, &scale, false, true)
    })?;
    assert_eq!(stale, Outcome::Cancelled);

    let huge = Change {
        output: "HEADLESS-1".into(),
        scale: Some(20.0),
        ..Default::default()
    };
    let failed = beside(&mut compositor, move |s| {
        randr::apply(&s, &huge, false, false)
    })?;
    assert_eq!(failed, Outcome::Failed);

    let applied = beside(&mut compositor, move |s| {
        randr::apply(&s, &scale, false, false)
    })?;
    assert_eq!(applied, Outcome::Succeeded);
    assert_eq!(compositor.outputs()[0].spec.scale, 2.0);
    Ok(())
}

#[test]
fn the_last_screen_on_stays_on() -> anyhow::Result<()> {
    let mut compositor = Headless::start(two_outputs())?;
    let off = |name: &str| Change {
        output: name.into(),
        enabled: Some(false),
        ..Default::default()
    };
    let first = off("HEADLESS-1");
    let outcome = beside(&mut compositor, move |s| {
        randr::apply(&s, &first, false, false)
    })?;
    assert_eq!(outcome, Outcome::Succeeded);
    let second = off("HEADLESS-2");
    let outcome = beside(&mut compositor, move |s| {
        randr::apply(&s, &second, false, false)
    })?;
    assert_eq!(outcome, Outcome::Failed);
    let heads = beside(&mut compositor, |s| randr::heads(&s))?;
    let on: Vec<&str> = heads
        .iter()
        .filter(|h| h.enabled)
        .map(|h| h.name.as_str())
        .collect();
    assert_eq!(on, ["HEADLESS-2"]);
    Ok(())
}

#[test]
fn output_power_turns_a_screen_dark_and_back() -> anyhow::Result<()> {
    let mut compositor = Headless::start(two_outputs())?;
    compositor.advance(FRAME)?;
    let off = beside(&mut compositor, |s| randr::power(&s, 0, false))?;
    assert_eq!(off, Some(false));
    let frames = compositor.outputs()[0].frames;
    // Something changes on it: nothing is drawn while it is dark.
    compositor.request(hocus_api::Request::PointerMove { x: 300.0, y: 200.0 })?;
    compositor.advance(FRAME * 3)?;
    assert_eq!(compositor.outputs()[0].frames, frames, "a dark screen drew");

    let on = beside(&mut compositor, |s| randr::power(&s, 0, true))?;
    assert_eq!(on, Some(true));
    compositor.request(hocus_api::Request::PointerMove { x: 320.0, y: 200.0 })?;
    compositor.advance(FRAME * 3)?;
    assert!(
        compositor.outputs()[0].frames > frames,
        "the screen stayed dark"
    );
    Ok(())
}
