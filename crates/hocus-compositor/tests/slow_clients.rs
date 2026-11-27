//! A client that stops reading for a moment is not cut off (#965): the
//! events the compositor sends it while it is busy (the pointer moving
//! over its window, keys) wait in its outgoing buffer, and reach it once
//! it reads again. One that never reads again is cut off once that buffer
//! is full, and the log says so, with what it was; one that ends is said
//! to have closed its connection, plainly.

use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use crate::common;

use common::{ClientHandle, Command, Event, Pointed, Shows, WAIT, compositor, open_showing};
use hocus_compositor::{Headless, model::Rect};

const APP: &str = "test.slow";
const KEY_A: u32 = 30;

/// The compositor's lines (`INFO` and over), while its guard is held on
/// the test's thread, the compositor's.
#[derive(Clone, Default)]
struct Journal(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Journal {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Journal {
    fn kept() -> (Self, tracing::subscriber::DefaultGuard) {
        let journal = Self::default();
        let writer = journal.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter("info")
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        (journal, tracing::subscriber::set_default(subscriber))
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap_or_else(PoisonError::into_inner)).into_owned()
    }

    /// The lines about clients' connections.
    fn connections(&self) -> Vec<String> {
        self.text()
            .lines()
            .filter(|l| l.contains("connection") || l.contains("cut off"))
            .map(str::to_owned)
            .collect()
    }
}

fn mapped(hp: &Headless) -> bool {
    hp.windows()
        .iter()
        .any(|w| w.app_id.as_deref() == Some(APP))
}

/// A listening window, the pointer over it and the keyboard on it; the
/// middle of its window.
fn listening(hp: &mut Headless) -> anyhow::Result<(ClientHandle, (f64, f64))> {
    let (rect, client) = open_showing(
        hp,
        APP,
        Shows::Listening {
            app_id: APP,
            size: (600, 400),
        },
    )?;
    let Rect {
        x,
        y,
        width,
        height,
    } = rect;
    let middle = (
        f64::from(x) + f64::from(width) / 2.0,
        f64::from(y) + f64::from(height) / 2.0,
    );
    hp.pointer_move(middle.0, middle.1);
    let entered = hp.dispatch_until(WAIT, |_| {
        client
            .events
            .try_iter()
            .any(|e| e == Event::Pointer(Pointed::Enter))
    })?;
    anyhow::ensure!(entered, "the pointer never came into the window");
    let focused = hp
        .windows()
        .into_iter()
        .find(|w| w.focused)
        .and_then(|w| w.app_id);
    anyhow::ensure!(focused.as_deref() == Some(APP), "focused: {focused:?}");
    Ok((client, middle))
}

/// Tells `client` to stop reading, and waits until it has.
fn stall(hp: &mut Headless, client: &ClientHandle) -> anyhow::Result<Arc<AtomicBool>> {
    let stalled = Arc::new(AtomicBool::new(true));
    client.commands.send(Command::Stall(stalled.clone()))?;
    let stopped = hp.dispatch_until(WAIT, |_| {
        client.events.try_iter().any(|e| e == Event::Stalled)
    })?;
    anyhow::ensure!(stopped, "the client never stopped reading");
    Ok(stalled)
}

/// The pointer moved `times` over the window around `middle`, a key
/// pressed and released every tenth time.
fn busy(hp: &mut Headless, middle: (f64, f64), times: u32) {
    for i in 0..times {
        hp.pointer_move(middle.0 + f64::from(i % 50), middle.1 + f64::from(i % 7));
        if i % 10 == 0 {
            hp.key(KEY_A, true);
            hp.key(KEY_A, false);
        }
    }
}

#[test]
fn a_client_that_stops_reading_a_moment_keeps_its_connection() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let (client, middle) = listening(&mut hp)?;
    let stalled = stall(&mut hp, &client)?;
    // A few seconds of the user working over it, the loop turning between
    // each move, as it does: each turn sends what it has.
    for _ in 0..600 {
        busy(&mut hp, middle, 10);
        hp.dispatch(Duration::from_millis(5))?;
    }
    assert!(
        mapped(&hp),
        "the window went while its client was busy for a moment"
    );
    stalled.store(false, Ordering::SeqCst);
    // It reads again: what waited reaches it, and what comes after too.
    hp.wheel(1);
    let mut keys = 0;
    let mut wheeled = false;
    let got = hp.dispatch_until(WAIT, |_| {
        for event in client.events.try_iter() {
            match event {
                Event::Key(KEY_A, true) => keys += 1,
                Event::Pointer(Pointed::Axis) => wheeled = true,
                _ => {}
            }
        }
        keys == 600 && wheeled
    })?;
    assert!(
        got,
        "once it read again, it got {keys} of the 600 keys, the wheel: {wheeled}"
    );
    assert!(mapped(&hp), "the window went");
    Ok(())
}

#[test]
fn a_client_that_never_reads_again_is_cut_off_and_said_so() -> anyhow::Result<()> {
    let (journal, _guard) = Journal::kept();
    let mut hp = compositor()?;
    let (client, middle) = listening(&mut hp)?;
    let stalled = stall(&mut hp, &client)?;
    // Well past what the compositor keeps for a client (a megabyte, some
    // 40,000 moves), in large batches: the moves cost the test more than
    // the turns. It is cut off as an event finds no room.
    let cut_off = |journal: &Journal| journal.connections().iter().any(|l| l.contains("WARN"));
    let mut batches = 0;
    while !cut_off(&journal) && batches < 40 {
        busy(&mut hp, middle, 5_000);
        hp.dispatch(Duration::from_millis(5))?;
        batches += 1;
    }
    // Its windows go at the compositor's next turn through its clients,
    // which another's request brings.
    stalled.store(false, Ordering::SeqCst);
    let _other = common::client(hp.socket(), None)?;
    let gone = hp.dispatch_until(WAIT, |hp| !mapped(hp))?;
    assert!(gone, "a client cut off kept its window");
    let said = journal.connections();
    let cut = said
        .iter()
        .find(|l| l.contains("WARN") && l.contains("stopped reading"))
        .ok_or_else(|| anyhow::anyhow!("nothing said why it was cut off: {said:?}"))?;
    assert!(cut.contains(APP), "{cut}");
    // Not before the room is full: a batch is some 150 kB of events.
    assert!(batches > 4, "cut off after {batches} batches");
    assert!(
        cut.contains(&format!("pid={}", std::process::id())),
        "{cut}"
    );
    Ok(())
}

#[test]
fn a_client_that_ends_is_said_to_have_closed_its_connection() -> anyhow::Result<()> {
    let (journal, _guard) = Journal::kept();
    let mut hp = compositor()?;
    let (client, _) = listening(&mut hp)?;
    client.commands.send(Command::Quit)?;
    let gone = hp.dispatch_until(WAIT, |hp| !mapped(hp))?;
    assert!(gone, "the window of a client that ended stayed");
    let said = journal.connections();
    assert!(
        said.iter()
            .any(|l| l.contains("INFO") && l.contains("closed its connection") && l.contains(APP)),
        "{said:?}"
    );
    assert!(!said.iter().any(|l| l.contains("WARN")), "{said:?}");
    Ok(())
}
