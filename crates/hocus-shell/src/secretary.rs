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
//!
//! And the notifications of the applications the user consented to (lot
//! 7, [`notified`]): her consent book, in her folder, says which; the
//! server's thread reads it again when it changes, and forwards her what
//! a notification says, never what it can do. Nothing of hers comes back
//! to the notifications: she cannot invoke, close or mark one (Slack's
//! would mark the channel read). Forwarded whatever the bubbles do: held
//! while the screen is shared or not to be disturbed, they are still
//! hers to read; when she may speak of them is hers to ask (§6.3).

use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{
        Mutex, PoisonError,
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    time::{Duration, SystemTime},
};

use hocus_link::SecretaryShown;
use hocus_secretary_link::{
    Client, FromSecretary, Notification, ToSecretary, VERSION,
    consent::{CONSENTS, ConsentBook},
    line, read,
};

/// The longest wait between two tries to connect.
const RETRY_MOST: Duration = Duration::from_secs(2);
/// What waits for her while she does not listen yet, at most; the oldest
/// go first.
const PENDING_MOST: usize = 100;

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

/// Her consent book as last read, and its file's time then.
static BOOK: Mutex<Option<(Option<SystemTime>, ConsentBook)>> = Mutex::new(None);

/// Her consent book, read again when its file changed: on the
/// notifications' server's thread, never iced's.
fn book() -> ConsentBook {
    let Some(path) = hocus_secretary_link::data_dir().map(|d| d.join(CONSENTS)) else {
        return ConsentBook::default();
    };
    let changed = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    let mut kept = BOOK.lock().unwrap_or_else(PoisonError::into_inner);
    match kept.as_ref() {
        Some((at, book)) if *at == changed => book.clone(),
        _ => {
            let book = ConsentBook::load(&path);
            *kept = Some((changed, book.clone()));
            book
        }
    }
}

/// A notification the server received: forwarded to her, in assistant
/// mode, when its application is consented to (lot 7). What it says only:
/// its actions stay the bubbles'.
pub fn notified(notification: Notification) {
    if line_state().out.is_none() {
        // Classic, or she is down: nothing is forwarded.
        return;
    }
    if book()
        .notifications(&notification.app, notification.entry.as_deref())
        .is_none()
    {
        return;
    }
    tracing::debug!(
        id = notification.id,
        app = notification.app,
        "a notification forwarded to the secretary"
    );
    send(ToSecretary::Notification(notification));
}

/// Connects (again, while she is wanted), says hello and the lock, then
/// writes what is queued; a reader thread takes her lines.
fn talk(socket: &std::path::Path, queue: &Receiver<ToSecretary>, locked: bool) {
    let mut wait = Duration::from_millis(50);
    let mut pending: VecDeque<ToSecretary> = VecDeque::new();
    loop {
        let mut stream = match UnixStream::connect(socket) {
            Ok(stream) => stream,
            Err(_) => {
                // Waiting for her to listen, or to come back; asked to stop
                // meanwhile when the queue's sender goes.
                match queue.recv_timeout(wait) {
                    Ok(message) => {
                        // Kept for when she listens.
                        if pending.len() >= PENDING_MOST {
                            pending.pop_front();
                        }
                        pending.push_back(message);
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
                wait = (wait * 2).min(RETRY_MOST);
                continue;
            }
        };
        wait = Duration::from_millis(50);
        let hello = ToSecretary::Hello {
            version: VERSION,
            client: Client::Shell,
        };
        let session = ToSecretary::Session {
            locked: locked || line_state().locked,
        };
        let mut first = vec![hello, session];
        first.extend(pending.drain(..));
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
                        pending.push_back(message);
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
