//! The shell's line to the secretary (docs/design/secretary.md §16), in
//! assistant mode: her own socket, which the compositor names
//! (`secretary ready PATH`). A thread of its own connects, writes and
//! reconnects; another reads what she says. Nothing here runs on iced's
//! loop or waits for her: a question she takes thirty seconds to answer
//! holds no frame (AC3). Her drawer (lot 2) will read [`state`].
//!
//! The shell tells her what she cannot see from her process: the session
//! locked or not (she keeps working, and never speaks while locked) and
//! the machine resumed (her sources catch up).

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{
        Mutex, PoisonError,
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    time::Duration,
};

use hocus_link::SecretaryShown;
use hocus_secretary_link::{Client, FromSecretary, ToSecretary, VERSION, line, read};

/// The longest wait between two tries to connect.
const RETRY_MOST: Duration = Duration::from_secs(2);

/// Where she is, as her drawer will say it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    /// Classic mode: the 4.6 secretary.
    #[default]
    Off,
    /// Down, or not connected yet: "indisponible".
    Unavailable,
    Ready,
}

#[derive(Debug, Default)]
struct Line {
    /// What writes to her, while one is wanted.
    out: Option<Sender<ToSecretary>>,
    socket: Option<PathBuf>,
    state: State,
    locked: bool,
    next_id: u64,
}

static LINE: Mutex<Line> = Mutex::new(Line {
    out: None,
    socket: None,
    state: State::Off,
    locked: false,
    next_id: 1,
});

fn line_state() -> std::sync::MutexGuard<'static, Line> {
    LINE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Where she is.
#[allow(dead_code)]
pub fn state() -> State {
    line_state().state
}

/// What the compositor says of her process.
pub fn told(shown: SecretaryShown) {
    let mut line = line_state();
    match shown {
        SecretaryShown::Off => {
            line.out = None;
            line.socket = None;
            line.state = State::Off;
        }
        SecretaryShown::Unavailable => {
            // Her next start is told `ready` again: a new connection then.
            line.out = None;
            line.state = State::Unavailable;
        }
        SecretaryShown::Ready(socket) => {
            if line.socket.as_ref() == Some(&socket) && line.out.is_some() {
                return;
            }
            let (out, queue) = mpsc::channel();
            let locked = line.locked;
            line.out = Some(out);
            line.socket = Some(socket.clone());
            line.state = State::Unavailable;
            let started = std::thread::Builder::new()
                .name("secretary".to_owned())
                .spawn(move || talk(&socket, &queue, locked));
            if let Err(err) = started {
                tracing::warn!(%err, "no thread to talk to the secretary");
                line.out = None;
            }
        }
    }
}

fn send(message: ToSecretary) {
    if let Some(out) = &line_state().out {
        let _ = out.send(message);
    }
}

/// A question to her (the drawer's field, lot 2; tests until then).
pub fn ask(text: String) {
    let id = {
        let mut line = line_state();
        let id = line.next_id;
        line.next_id += 1;
        id
    };
    tracing::info!(id, "a question to the secretary");
    send(ToSecretary::Ask { id, text });
}

/// The session locked or unlocked.
pub fn locked(locked: bool) {
    {
        let mut line = line_state();
        if line.locked == locked {
            return;
        }
        line.locked = locked;
    }
    send(ToSecretary::Session { locked });
}

/// The machine resumed after `slept` seconds (unknown: she measures it).
pub fn resumed(slept: Option<u64>) {
    send(ToSecretary::Resumed { slept });
}

/// Connects (again, while she is wanted), says hello and the lock, then
/// writes what is queued; a reader thread takes her lines.
fn talk(socket: &std::path::Path, queue: &Receiver<ToSecretary>, locked: bool) {
    let mut wait = Duration::from_millis(50);
    let mut pending: Option<ToSecretary> = None;
    loop {
        let mut stream = match UnixStream::connect(socket) {
            Ok(stream) => stream,
            Err(_) => {
                // Waiting for her to listen, or to come back; asked to stop
                // meanwhile when the queue's sender goes.
                match queue.recv_timeout(wait) {
                    Ok(message) => {
                        // Kept for when she listens; the newest first.
                        pending = Some(message);
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
                wait = (wait * 2).min(RETRY_MOST);
                continue;
            }
        };
        wait = Duration::from_millis(50);
        let lang = match crate::clock::time::Lang::of_messages() {
            crate::clock::time::Lang::French => "fr",
            crate::clock::time::Lang::English => "en",
        };
        let hello = ToSecretary::Hello {
            version: VERSION,
            client: Client::Shell,
            lang: Some(lang.to_owned()),
        };
        let session = ToSecretary::Session {
            locked: locked || line_state().locked,
        };
        let mut first = vec![hello, session];
        first.extend(pending.take());
        if first
            .iter()
            .any(|m| stream.write_all(line(m).as_bytes()).is_err())
        {
            continue;
        }
        let reader = match stream.try_clone() {
            Ok(reader) => reader,
            Err(err) => {
                tracing::warn!(%err, "could not read the secretary");
                return;
            }
        };
        let reading = std::thread::Builder::new()
            .name("secretary: read".to_owned())
            .spawn(move || listen(reader));
        if let Err(err) = reading {
            tracing::warn!(%err, "no thread to read the secretary");
            return;
        }
        loop {
            match queue.recv() {
                Ok(message) => {
                    if stream.write_all(line(&message).as_bytes()).is_err() {
                        pending = Some(message);
                        break;
                    }
                }
                Err(_) => {
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                    return;
                }
            }
        }
    }
}

/// Her lines, until the connection ends.
fn listen(stream: UnixStream) {
    for text in BufReader::new(stream).lines() {
        let Ok(text) = text else {
            break;
        };
        match read::<FromSecretary>(&text) {
            Ok(FromSecretary::Hello { version }) => {
                tracing::info!(version, "the secretary is connected");
                line_state().state = State::Ready;
            }
            Ok(FromSecretary::Refused { reason }) => {
                tracing::warn!(reason, "the secretary refused the shell");
            }
            Ok(FromSecretary::Answer { id, done, .. }) => {
                tracing::info!(id, done, "the secretary answered");
            }
            Ok(FromSecretary::Error { id, message }) => {
                tracing::info!(?id, message, "the secretary said no");
            }
            Ok(other) => tracing::debug!(?other, "the secretary said"),
            Err(err) => tracing::warn!(%err, "a line from the secretary that does not read"),
        }
    }
    let mut line = line_state();
    if line.state == State::Ready {
        line.state = State::Unavailable;
    }
}
