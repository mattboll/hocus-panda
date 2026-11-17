//! What comes and goes all day leaves nothing behind in the compositor
//! (#827): an agent whose process ended before its report was read is
//! forgotten, as one whose end was heard.

use crate::common;

use std::{io::Write as _, os::unix::net::UnixStream};

use anyhow::Context as _;
use common::WAIT;
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
