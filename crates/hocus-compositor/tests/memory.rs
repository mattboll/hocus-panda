//! What comes and goes all day leaves nothing behind in the compositor
//! (#827): an agent whose process ended before its report was read is
//! forgotten, as one whose end was heard; a surface gone before its next
//! frame leaves no frame callback behind in its client, which the shell's
//! bubbles and pandas did for the session.

use crate::common;

use std::{io::Write as _, os::unix::net::UnixStream};

use anyhow::Context as _;
use common::{Command, Event, WAIT};
use hocus_agents::{Report, Source, State};
use hocus_compositor::Headless;

/// `report` on `hp`'s control socket, a connection of its own (read in
/// the order they come).
fn tell(hp: &Headless, report: &Report) -> anyhow::Result<()> {
    let socket = hp.control_socket().context("no control socket")?;
    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(format!("{report}\n").as_bytes())?;
    Ok(())
}

fn done(key: &str, pid: u32) -> Report {
    Report {
        source: Source::ClaudeCode,
        key: key.to_owned(),
        state: Some(State::Done),
        pid: Some(pid),
        name: key.to_owned(),
        note: None,
        tests: None,
        conversation: hocus_agents::Conversation::default(),
    }
}

/// One-shot runs (`codex exec`, a script's agent) whose last hook's
/// process is gone before the desktop reads it: none stays in the list.
#[test]
fn agents_ended_before_their_report_was_read_are_forgotten() -> anyhow::Result<()> {
    let mut hp = common::compositor()?;
    for n in 0..20 {
        let mut run = std::process::Command::new("true")
            .spawn()
            .unwrap_or_else(|_| common::missing("true"));
        let pid = run.id();
        run.wait()?;
        tell(&hp, &done(&format!("oneshot{n}"), pid))?;
    }
    // The test's own process, still running, told last: once its row
    // shows, the others were read.
    tell(&hp, &done("alive", std::process::id()))?;
    let names =
        |hp: &mut Headless| -> Vec<String> { hp.agents().into_iter().map(|a| a.name).collect() };
    let start = std::time::Instant::now();
    while names(&mut hp) != ["alive"] {
        anyhow::ensure!(start.elapsed() < WAIT, "{:?}", names(&mut hp));
        hp.dispatch(std::time::Duration::from_millis(5))?;
    }
    Ok(())
}

/// Fifty surfaces made and destroyed before a frame, each asking for its
/// frame callback: none of their callbacks stays.
#[test]
fn a_surface_gone_before_its_frame_leaves_no_callback() -> anyhow::Result<()> {
    let mut hp = common::compositor()?;
    let (_, client) = common::open_with(&mut hp, "test.menus", (400, 300), false)?;
    common::wait(&mut hp, std::time::Duration::from_millis(200))?;
    let before = hp.objects("wl_callback");
    client.commands.send(Command::Fleeting(50))?;
    // Its export, asked after them, answered: the compositor read them.
    client.commands.send(Command::Export)?;
    let exported = hp.dispatch_until(WAIT, |_| {
        client
            .events
            .try_iter()
            .any(|event| matches!(event, Event::Exported(_)))
    })?;
    anyhow::ensure!(exported, "the client's export never came");
    let after = hp.objects("wl_callback");
    anyhow::ensure!(after <= before, "{before} frame callbacks, then {after}");
    Ok(())
}
