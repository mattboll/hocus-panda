//! polkit's helper, which checks a password: the shell never does. It is
//! polkit's documented protocol, read from its documentation and its
//! agent's behaviour, no code of polkit's (LGPL) copied:
//!
//! - Where `/run/polkit/agent-helper.socket` is (polkit 126 and later, a
//!   systemd socket starting the helper as root for each connection), the
//!   shell connects to it and writes the account's login, then the
//!   request's cookie, a line each.
//! - Else it starts the setuid `polkit-agent-helper-1` with the login as
//!   its argument, and writes the cookie on its input (never in its
//!   arguments, which every process can read).
//!
//! Then the helper runs PAM and says, a line each, `PAM_PROMPT_ECHO_OFF`
//! or `PAM_PROMPT_ECHO_ON` and a prompt (each answered with a line),
//! `PAM_ERROR_MSG` or `PAM_TEXT_INFO` and a message (C escapes in them),
//! and at last `SUCCESS` (it has told polkit itself) or `FAILURE`.
//!
//! Each conversation runs on a thread of its own. What is typed reaches it
//! as a [`Secret`], is written to the helper as it is, and is wiped as the
//! secret goes. Ending a conversation closes the helper's input (or the
//! socket) and stops the helper by its own handle; its thread then reaps
//! it. `HOCUS_POLKIT_HELPER` (a program) and `HOCUS_POLKIT_SOCKET` name
//! others, for tests: with the first set, the system's socket is never
//! used.

use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

use crate::network::secret::Secret;

/// Where polkit puts its helper, by distribution.
const PROGRAMS: [&str; 4] = [
    "/usr/lib/polkit-1/polkit-agent-helper-1",
    "/usr/libexec/polkit-agent-helper-1",
    "/usr/libexec/polkit-1/polkit-agent-helper-1",
    "/usr/lib/policykit-1/polkit-agent-helper-1",
];
const SOCKET: &str = "/run/polkit/agent-helper.socket";

/// What the helper said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    /// It asks something: hidden (a password) or not.
    Prompt {
        echo: bool,
        text: String,
    },
    Info(String),
    Error(String),
    /// It is done: granted or not (a helper gone counts as not).
    Done(bool),
}

/// How the helper is reached.
enum Way {
    Socket(PathBuf),
    Program(PathBuf),
}

fn way() -> Option<Way> {
    if let Some(program) = std::env::var_os("HOCUS_POLKIT_HELPER") {
        return Some(match std::env::var_os("HOCUS_POLKIT_SOCKET") {
            Some(socket) => Way::Socket(socket.into()),
            None => Way::Program(program.into()),
        });
    }
    if let Some(socket) = std::env::var_os("HOCUS_POLKIT_SOCKET") {
        return Some(Way::Socket(socket.into()));
    }
    let socket = PathBuf::from(SOCKET);
    if socket.exists() {
        return Some(Way::Socket(socket));
    }
    PROGRAMS
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
        .map(Way::Program)
}

/// What stops a conversation from outside its thread.
enum Stop {
    Socket(UnixStream),
    Child(Arc<Mutex<Option<ChildStdin>>>, Arc<Mutex<Option<Child>>>),
}

/// A conversation with the helper under way.
pub struct Conversation {
    answers: mpsc::Sender<Secret>,
    stop: Arc<Mutex<Option<Stop>>>,
    ended: Arc<AtomicBool>,
}

impl std::fmt::Debug for Conversation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Conversation")
    }
}

impl Conversation {
    /// A conversation for `login` and the request's `cookie`, on a thread
    /// of its own; what the helper says goes to `tell`, never once it was
    /// ended.
    pub fn start(login: String, cookie: String, tell: impl Fn(Said) + Send + 'static) -> Self {
        let (answers, asked) = mpsc::channel::<Secret>();
        let stop = Arc::new(Mutex::new(None));
        let ended = Arc::new(AtomicBool::new(false));
        let conversation = Self {
            answers,
            stop: stop.clone(),
            ended: ended.clone(),
        };
        let started = std::thread::Builder::new()
            .name("polkit-helper".to_owned())
            .spawn(move || {
                let tell = |said: Said| {
                    if !ended.load(Ordering::SeqCst) {
                        tell(said);
                    }
                };
                match run(&login, &cookie, &asked, &stop, &ended, &tell) {
                    Ok(()) => {}
                    Err(err) => {
                        tracing::warn!(%err, "polkit's helper failed");
                        tell(Said::Done(false));
                    }
                }
            });
        if let Err(err) = started {
            tracing::warn!(%err, "cannot start polkit's helper's thread");
        }
        conversation
    }

    /// The answer to the prompt (now, or the next one).
    pub fn answer(&self, secret: Secret) {
        let _ = self.answers.send(secret);
    }

    /// The conversation ends: the helper is stopped.
    pub fn end(self) {
        self.ended.store(true, Ordering::SeqCst);
        let stop = self
            .stop
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        match stop {
            Some(Stop::Socket(stream)) => {
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
            Some(Stop::Child(input, child)) => {
                drop(input.lock().unwrap_or_else(PoisonError::into_inner).take());
                // A setuid helper cannot be signalled: its input closed,
                // it ends by itself.
                if let Some(child) = child
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .as_mut()
                {
                    let _ = child.kill();
                }
            }
            None => {}
        }
        // Dropping `answers` wakes a thread waiting for one.
    }
}

/// Where the conversation writes.
enum Out {
    Socket(UnixStream),
    Child(Arc<Mutex<Option<ChildStdin>>>),
}

impl Out {
    fn write(&self, bytes: &[u8]) -> std::io::Result<()> {
        match self {
            Self::Socket(stream) => (&*stream).write_all(bytes),
            Self::Child(input) => match input
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_mut()
            {
                Some(input) => input.write_all(bytes),
                None => Err(std::io::ErrorKind::BrokenPipe.into()),
            },
        }
    }
}

fn run(
    login: &str,
    cookie: &str,
    asked: &mpsc::Receiver<Secret>,
    stop: &Mutex<Option<Stop>>,
    ended: &AtomicBool,
    tell: &dyn Fn(Said),
) -> std::io::Result<()> {
    let Some(way) = way() else {
        tracing::warn!("polkit's helper is not installed: no authentication");
        tell(Said::Done(false));
        return Ok(());
    };
    let (out, input, child): (Out, Box<dyn Read + Send>, _) = match way {
        Way::Socket(path) => {
            let stream = UnixStream::connect(&path)?;
            *stop.lock().unwrap_or_else(PoisonError::into_inner) =
                Some(Stop::Socket(stream.try_clone()?));
            let reader = stream.try_clone()?;
            let out = Out::Socket(stream);
            out.write(format!("{login}\n").as_bytes())?;
            (out, Box::new(reader), None)
        }
        Way::Program(program) => {
            let mut child = Command::new(&program)
                .arg(login)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()?;
            let stdout = child.stdout.take().ok_or(std::io::ErrorKind::BrokenPipe)?;
            let input = Arc::new(Mutex::new(child.stdin.take()));
            let child = Arc::new(Mutex::new(Some(child)));
            *stop.lock().unwrap_or_else(PoisonError::into_inner) =
                Some(Stop::Child(input.clone(), child.clone()));
            (Out::Child(input), Box::new(stdout), Some(child))
        }
    };
    if ended.load(Ordering::SeqCst) {
        reap(child.as_ref());
        return Ok(());
    }
    let talked = talk(cookie, &out, input, asked, tell);
    drop(out);
    reap(child.as_ref());
    talked
}

/// The conversation itself, once the helper is reached.
fn talk(
    cookie: &str,
    out: &Out,
    input: Box<dyn Read + Send>,
    asked: &mpsc::Receiver<Secret>,
    tell: &dyn Fn(Said),
) -> std::io::Result<()> {
    out.write(format!("{cookie}\n").as_bytes())?;
    let mut lines = BufReader::new(input);
    let mut line = String::new();
    loop {
        line.clear();
        if lines.read_line(&mut line)? == 0 {
            tell(Said::Done(false));
            return Ok(());
        }
        let said = read_line(line.trim_end_matches('\n'));
        let prompt = matches!(said, Said::Prompt { .. });
        let done = matches!(said, Said::Done(_));
        tell(said);
        if done {
            return Ok(());
        }
        if prompt {
            // Ended meanwhile: the sender is gone.
            let Ok(secret) = asked.recv() else {
                return Ok(());
            };
            out.write(secret.as_str().as_bytes())?;
            out.write(b"\n")?;
        }
    }
}

fn reap(child: Option<&Arc<Mutex<Option<Child>>>>) {
    if let Some(child) = child
        && let Some(mut child) = child.lock().unwrap_or_else(PoisonError::into_inner).take()
    {
        let _ = child.wait();
    }
}

/// A line of the helper's.
fn read_line(line: &str) -> Said {
    let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
    let text = || unescape(rest);
    match word {
        "PAM_PROMPT_ECHO_OFF" => Said::Prompt {
            echo: false,
            text: text(),
        },
        "PAM_PROMPT_ECHO_ON" => Said::Prompt {
            echo: true,
            text: text(),
        },
        "PAM_ERROR_MSG" => Said::Error(text()),
        "PAM_TEXT_INFO" => Said::Info(text()),
        w if w.starts_with("SUCCESS") => Said::Done(true),
        w if w.starts_with("FAILURE") => Said::Done(false),
        _ => {
            tracing::warn!("polkit's helper said something unknown");
            Said::Done(false)
        }
    }
}

/// C's escapes undone: `\n`, `\t`, `\\`, `\"`, octal `\NNN`, and so on.
fn unescape(text: &str) -> String {
    let mut out = Vec::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' || i + 1 == bytes.len() {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        i += 1;
        let c = bytes[i];
        i += 1;
        out.push(match c {
            b'b' => 8,
            b'f' => 12,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 11,
            b'0'..=b'7' => {
                let mut value = u32::from(c - b'0');
                for _ in 0..2 {
                    match bytes.get(i) {
                        Some(d @ b'0'..=b'7') => {
                            value = value * 8 + u32::from(d - b'0');
                            i += 1;
                        }
                        _ => break,
                    }
                }
                (value & 0xFF) as u8
            }
            other => other,
        });
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_helpers_lines() {
        assert_eq!(
            read_line("PAM_PROMPT_ECHO_OFF Password: "),
            Said::Prompt {
                echo: false,
                text: "Password: ".into()
            }
        );
        assert_eq!(
            read_line("PAM_TEXT_INFO Place your finger\\n on the reader"),
            Said::Info("Place your finger\n on the reader".into())
        );
        assert_eq!(
            read_line("PAM_ERROR_MSG Le compte est verrouill\\303\\251."),
            Said::Error("Le compte est verrouillé.".into())
        );
        assert_eq!(read_line("SUCCESS"), Said::Done(true));
        assert_eq!(read_line("FAILURE"), Said::Done(false));
        assert_eq!(read_line("WHAT"), Said::Done(false));
        assert_eq!(unescape("a\\\\b\\\"c\\"), "a\\b\"c\\");
    }
}
