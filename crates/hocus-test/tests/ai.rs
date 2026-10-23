//! A slow provider streams a long answer into the command field (#44):
//! the loop never waits for it: none of its handlers (the pieces that
//! came, the card laid out again, the frame drawn) holds it while the
//! answer comes.

use std::time::{Duration, Instant};

use hocus_api::{Reply, Request};
use hocus_compositor::{FRAME, HarnessOptions, Headless, HeadlessConfig, RendererChoice};
use hocus_test::ai::{Answers, Server};

fn press(compositor: &mut Headless, codes: &[u32]) -> anyhow::Result<()> {
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

fn labels(compositor: &mut Headless) -> anyhow::Result<Vec<String>> {
    let Reply::Ui(nodes) = compositor.request(Request::Ui)? else {
        anyhow::bail!("no interface");
    };
    Ok(nodes.into_iter().filter_map(|n| n.label).collect())
}

#[test]
fn a_slow_provider_streams_while_the_loop_keeps_its_budget() -> anyhow::Result<()> {
    // Eighty pieces, 25 ms apart, after a model "loading" for 300 ms.
    let pieces: Vec<String> = (1..=80).map(|i| format!("piece {i} ")).collect();
    let whole = pieces.concat().trim_end().to_owned();
    let server = Server::start(Answers {
        pieces,
        delay: Duration::from_millis(25),
        first: Duration::from_millis(300),
    })?;
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
    })?;
    compositor.write_config(&format!(
        "[ai]\nenabled = true\nurl = \"{}\"\n",
        server.url()
    ))?;
    compositor.advance(FRAME * 4)?;
    // Super+Space, "why?", Enter on "Ask “why?”".
    press(&mut compositor, &[125, 57])?;
    compositor.advance(FRAME * 2)?;
    for codes in [&[17][..], &[35], &[21], &[42, 53]] {
        press(&mut compositor, codes)?;
    }
    compositor.advance(FRAME * 2)?;
    assert!(
        labels(&mut compositor)?.contains(&"Ask “why?”".to_owned()),
        "a question is offered to ask first"
    );
    press(&mut compositor, &[28])?;

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut turns = 0;
    loop {
        compositor.dispatch(Duration::ZERO)?;
        compositor.advance(FRAME)?;
        turns += 1;
        if turns % 20 == 0
            && labels(&mut compositor)?
                .iter()
                .any(|l| l.starts_with("ANSWER · LOCAL AI · "))
        {
            break;
        }
        anyhow::ensure!(Instant::now() < deadline, "the answer never ended");
        // The provider takes its time; the test does not spin.
        std::thread::sleep(Duration::from_millis(4));
    }
    let shown = labels(&mut compositor)?;
    assert!(shown.contains(&whole), "the whole answer shows: {shown:?}");
    assert!(
        server.asked().iter().any(|a| a.path == "/api/chat"),
        "ollama was asked"
    );
    let Reply::Stats(stats) = compositor.request(Request::Stats)? else {
        anyhow::bail!("expected stats");
    };
    // What the loop's own handlers took, not the wall clock of a loaded
    // machine.
    let held = hocus_test::held_the_loop(&stats);
    assert!(held.is_empty(), "the loop was held: {held:?}");
    Ok(())
}
