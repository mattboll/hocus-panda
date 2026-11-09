//! The shell's line to the secretary (docs/design/secretary.md §16), in
//! assistant mode: her own socket, which the compositor names
//! (`secretary ready PATH`). A thread of its own connects, writes and
//! reconnects; another reads what she says. Nothing here runs on iced's
//! loop or waits for her: a question she takes thirty seconds to answer
//! holds no frame (AC3). Her drawer ([`crate::drawer`]) reads the
//! conversation she sends ([`conversation`]) and is woken only while it
//! is open.
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
//!
//! Her speaking up (lot 5): her requests to speak and her restraint go to
//! the shell's speak gate ([`crate::speak`]), whose answers, and what the
//! user does with her peek, go back to her; the moment too. The effects
//! she asks for once a card is accepted are carried out here, on the
//! reader's thread: a task goes to the To do list (Day.4), and she is
//! told whether it went.

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
    Body, Client, Decision, Effect, FromSecretary, Item, Look, Moment, Notification, PeekAnswer,
    Spoken, ToSecretary, VERSION,
    consent::{CONSENTS, ConsentBook},
    knows::{Do, Knowledge, Query},
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
pub fn state() -> State {
    line_state().state
}

/// A word at the bottom of "Ce qu'elle sait" for five seconds, with
/// "Annuler" when it can be taken back.
#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub undo: Option<u64>,
    pub until: std::time::Instant,
}

/// How long a toast shows (§8).
pub const TOAST: Duration = Duration::from_secs(5);

/// What her drawer shows of the conversation: the thread, her answer as
/// it streams, her typing, her face.
#[derive(Debug, Clone, Default)]
pub struct Conversation {
    pub items: Vec<Item>,
    /// Her answer to a request as it comes, until its item replaces it.
    pub streaming: Vec<(u64, String)>,
    /// She composes an answer to a request: what she is doing.
    pub typing: Option<(u64, String)>,
    pub look: Option<Look>,
    /// "Ce qu'elle sait", as she last sent it (lot 6).
    pub knows: Option<Box<Knowledge>>,
    pub toast: Option<Toast>,
    /// Her key given in the drawer: the request waiting for her answer,
    /// and what became of the last one (#752).
    pub key_asked: Option<u64>,
    pub key_said: Option<(String, hocus_secretary_link::KeyOutcome)>,
    /// Bumped at each change: what the drawer lays out again for.
    pub rev: u64,
}

static CONVERSATION: Mutex<Option<Conversation>> = Mutex::new(None);

/// `with` on the conversation.
pub fn conversation<T>(with: impl FnOnce(&Conversation) -> T) -> T {
    let held = CONVERSATION.lock().unwrap_or_else(PoisonError::into_inner);
    match held.as_ref() {
        Some(c) => with(c),
        None => with(&Conversation::default()),
    }
}

fn converse(change: impl FnOnce(&mut Conversation)) {
    {
        let mut held = CONVERSATION.lock().unwrap_or_else(PoisonError::into_inner);
        let c = held.get_or_insert_with(Conversation::default);
        change(c);
        c.rev += 1;
    }
    crate::drawer::changed();
}

/// What she sent, kept for the drawer.
fn heard(message: FromSecretary) {
    match message {
        FromSecretary::Item { item } => converse(|c| {
            // Her words whole replace her answer as it streamed.
            if matches!(item.body, Body::Her { .. })
                && let Some(request) = item.request
            {
                c.streaming.retain(|(r, _)| *r != request);
            }
            match c.items.iter_mut().find(|i| i.id == item.id) {
                Some(kept) => *kept = item,
                None => c.items.push(item),
            }
        }),
        FromSecretary::Thread { items, .. } => converse(|c| {
            c.items = items;
            c.streaming.clear();
        }),
        FromSecretary::Typing { id, doing } => converse(|c| c.typing = Some((id, doing))),
        FromSecretary::Answer { id, text, done } => converse(|c| {
            if !text.is_empty() {
                match c.streaming.iter_mut().find(|(r, _)| *r == id) {
                    Some((_, said)) => said.push_str(&text),
                    None => c.streaming.push((id, text)),
                }
            }
            if done {
                c.streaming.retain(|(r, _)| *r != id);
                if c.typing.as_ref().is_some_and(|(r, _)| *r == id) {
                    c.typing = None;
                }
            }
        }),
        FromSecretary::Look { look } => converse(|c| {
            // The key given elsewhere (the Settings): its field goes.
            if look.key.is_none() {
                c.key_asked = None;
            }
            c.look = Some(look);
        }),
        FromSecretary::Keyed {
            id,
            provider,
            outcome,
        } => {
            tracing::info!(id, provider, ?outcome, "her key");
            converse(|c| {
                if c.key_asked == Some(id) {
                    c.key_asked = None;
                }
                c.key_said = Some((provider, outcome));
            });
        }
        FromSecretary::ClipSecret { digest } => crate::clipboard::forget_secret(digest),
        FromSecretary::Speak { speak } => crate::speak::asked(speak),
        FromSecretary::Restraint { restraint } => crate::speak::restraint(restraint),
        FromSecretary::Effect { effect } => carry_out(effect),
        FromSecretary::Knows { view, .. } => converse(|c| c.knows = Some(view)),
        FromSecretary::Toast { text, undo, .. } => {
            tracing::info!(text, undo = undo.is_some(), "her toast");
            converse(|c| {
                c.toast = Some(Toast {
                    text,
                    undo,
                    until: std::time::Instant::now() + TOAST,
                });
            });
        }
        FromSecretary::Error {
            id: Some(id),
            message,
        } => {
            tracing::info!(id, message, "the secretary said no");
            converse(|c| {
                if c.typing.as_ref().is_some_and(|(r, _)| *r == id) {
                    c.typing = None;
                }
                c.streaming.retain(|(r, _)| *r != id);
            });
        }
        other => tracing::debug!(?other, "the secretary said"),
    }
}

/// What the compositor says of her process.
pub fn told(shown: SecretaryShown) {
    told_line(shown.clone());
    match shown {
        // Classic again: no drawer, nothing of her (§21).
        SecretaryShown::Off => {
            crate::drawer::close();
            crate::speak::off();
            *CONVERSATION.lock().unwrap_or_else(PoisonError::into_inner) = None;
        }
        SecretaryShown::Unavailable | SecretaryShown::Ready(_) => crate::drawer::changed(),
    }
}

fn told_line(shown: SecretaryShown) {
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

fn next_id() -> u64 {
    let mut line = line_state();
    let id = line.next_id;
    line.next_id += 1;
    id
}

/// A sentence to her: the drawer's field, or a test's line.
pub fn ask(text: String) {
    let id = next_id();
    tracing::info!(id, "a sentence to the secretary");
    send(ToSecretary::Ask { id, text });
}

/// `key`, pasted in her drawer, for `provider`: she checks it and keeps
/// it in her keyring. The text is overwritten once written to her.
pub fn give_key(provider: String, key: String) {
    let id = next_id();
    tracing::info!(id, provider, "her key, given in the drawer");
    converse(|c| {
        c.key_asked = Some(id);
        c.key_said = None;
    });
    send(ToSecretary::Key {
        id,
        provider,
        key: Some(hocus_secretary_link::Secret::new(key)),
    });
}

/// Quick reply `choice` of her question `item`.
pub fn choose(item: u64, choice: u32) {
    let id = next_id();
    tracing::info!(id, item, choice, "a quick reply to the secretary");
    send(ToSecretary::Choose { id, item, choice });
}

/// A card's button; a consent card's "Autoriser" with the notebook
/// chosen on it.
pub fn decide(item: u64, decision: Decision, notebook: Option<hocus_secretary_link::Notebook>) {
    let id = next_id();
    tracing::info!(id, item, ?decision, ?notebook, "a card's button");
    send(ToSecretary::Decide {
        id,
        item,
        decision,
        notebook,
    });
}

/// "Annuler" under what she kept.
pub fn undo(item: u64) {
    let id = next_id();
    send(ToSecretary::Undo { id, item });
}

/// "Ce qu'elle sait" for `query`, sent again while it changes; none: no
/// more (the tab left, the drawer closed).
pub fn knows(query: Option<Query>) {
    let id = next_id();
    send(ToSecretary::Knows { id, query });
}

/// Something done in "Ce qu'elle sait".
pub fn memory(act: Do) {
    let id = next_id();
    tracing::info!(id, ?act, "an act on her memory");
    send(ToSecretary::Memory { id, act });
}

/// The toast's time is over, or its "Annuler" clicked: it goes.
pub fn toast_gone() {
    converse(|c| c.toast = None);
}

/// The drawer opened or closed: she is told; opened, the thread is asked
/// for once (the shell may have started again since she began it).
pub fn drawer(open: bool) {
    send(ToSecretary::Drawer { open });
    if !open {
        send(ToSecretary::Knows {
            id: next_id(),
            query: None,
        });
    }
    if open && conversation(|c| c.items.is_empty()) {
        let id = next_id();
        send(ToSecretary::Thread { id });
    }
}

/// What the gate did with her request `speak`.
pub fn spoken(speak: u64, outcome: Spoken) {
    send(ToSecretary::Spoken { speak, outcome });
}

/// What the user did with her peek of `speak`.
pub fn peeked(speak: u64, answer: PeekAnswer) {
    let id = next_id();
    send(ToSecretary::Peeked { id, speak, answer });
}

/// The moment the screen is in.
pub fn moment(moment: Moment) {
    send(ToSecretary::Moment { moment });
}

/// An effect she asks for, a card accepted: carried out, and she is told.
fn carry_out(effect: Effect) {
    let id = effect.id();
    let (ok, detail) = match effect {
        Effect::Task { title, .. } => match crate::day::add_task(title) {
            Ok(()) => (true, String::new()),
            Err(why) => (false, why),
        },
    };
    tracing::info!(id, ok, "an effect for the secretary");
    send(ToSecretary::Effected {
        effect: id,
        ok,
        detail,
    });
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
        first.extend(pending.drain(..));
        if first.iter().any(|m| !write_to(&mut stream, m)) {
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
                    if !write_to(&mut stream, &message) {
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

/// `message` written to her: whether it went. A key's line is overwritten
/// once written.
fn write_to(stream: &mut UnixStream, message: &ToSecretary) -> bool {
    let mut text = line(message);
    let wrote = stream.write_all(text.as_bytes()).is_ok();
    if matches!(message, ToSecretary::Key { .. }) {
        hocus_secretary_link::wipe(&mut text);
    }
    wrote
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
                crate::drawer::changed();
                // Her watch on "Ce qu'elle sait" went with her last run.
                crate::drawer::knows_again();
            }
            Ok(FromSecretary::Refused { reason }) => {
                tracing::warn!(reason, "the secretary refused the shell");
            }
            Ok(FromSecretary::Answer { id, text, done }) => {
                if done {
                    tracing::info!(id, "the secretary answered");
                }
                heard(FromSecretary::Answer { id, text, done });
            }
            Ok(FromSecretary::Error { id: None, message }) => {
                tracing::info!(message, "the secretary said no");
            }
            Ok(message) => heard(message),
            Err(err) => tracing::warn!(%err, "a line from the secretary that does not read"),
        }
    }
    let mut line = line_state();
    if line.state == State::Ready {
        line.state = State::Unavailable;
        drop(line);
        crate::drawer::changed();
    }
}
