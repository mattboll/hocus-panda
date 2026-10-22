//! The session's polkit agent (#83): when an application asks for a
//! system action that needs the user's password (pkexec, mounting,
//! updates), polkit asks this agent, which asks the compositor to show its
//! dialog. The password is checked by polkit's own helper, as root, which
//! tells polkit; the agent only carries the conversation.
//!
//! The agent serves D-Bus on a thread of its own; each request waits there,
//! never on the compositor's loop. Checking a password runs the helper on a
//! thread too.

use std::{
    collections::HashMap,
    future::poll_fn,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    task::{Poll, Waker},
};

use zbus::{OwnedVariant, Variant, interface};

use crate::session::system;

const AUTHORITY: &str = "org.freedesktop.PolicyKit1";
const PATH: &str = "/org/hocuspanda/PolicyKit1/AuthenticationAgent";
/// polkit 121 and later start the helper per connection on this socket.
const HELPER_SOCKET: &str = "/run/polkit/agent-helper.socket";
/// The setuid helper of older polkit, where distributions put it.
const HELPERS: [&str; 4] = [
    "/usr/lib/polkit-1/polkit-agent-helper-1",
    "/usr/libexec/polkit-agent-helper-1",
    "/usr/local/lib/polkit-1/polkit-agent-helper-1",
    "/usr/local/libexec/polkit-agent-helper-1",
];

/// What the agent tells the compositor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEvent {
    /// A dialog to show.
    Begin(Request),
    /// polkit took a request back (the application went away).
    Cancelled { cookie: String },
    /// A password was checked.
    Checked {
        cookie: String,
        result: Result<(), String>,
    },
}

/// One authentication polkit asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub cookie: String,
    pub action: String,
    /// polkit's sentence, in the session's language.
    pub message: String,
    /// The program run, for pkexec.
    pub command: Option<String>,
    /// Whose password: the user's own, else an administrator's.
    pub user: String,
}

/// How a request ended, for polkit.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Done,
    Cancelled,
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.PolicyKit1.Error")]
enum PolkitError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Cancelled(String),
    Failed(String),
}

/// A request's end, awaited by its D-Bus call.
#[derive(Debug, Default)]
struct Slot {
    outcome: Option<Outcome>,
    waker: Option<Waker>,
}

type Sink = Arc<dyn Fn(AgentEvent) + Send + Sync>;

/// The requests waiting for their end, by cookie.
#[derive(Default, Clone)]
struct Pending(Arc<Mutex<HashMap<String, Arc<Mutex<Slot>>>>>);

impl Pending {
    fn finish(&self, cookie: &str, outcome: Outcome) {
        let slot = self.0.lock().ok().and_then(|mut m| m.remove(cookie));
        if let Some(slot) = slot
            && let Ok(mut slot) = slot.lock()
        {
            slot.outcome = Some(outcome);
            if let Some(waker) = slot.waker.take() {
                waker.wake();
            }
        }
    }
}

struct AgentObject {
    sink: Sink,
    pending: Pending,
}

#[interface(name = "org.freedesktop.PolicyKit1.AuthenticationAgent")]
impl AgentObject {
    async fn begin_authentication(
        &self,
        action_id: String,
        message: String,
        _icon_name: String,
        details: HashMap<String, String>,
        cookie: String,
        identities: Vec<(String, HashMap<String, OwnedVariant>)>,
    ) -> Result<(), PolkitError> {
        let slot = Arc::new(Mutex::new(Slot::default()));
        if let Ok(mut map) = self.pending.0.lock() {
            map.insert(cookie.clone(), slot.clone());
        }
        let user = whose(&identities)
            .ok_or_else(|| PolkitError::Failed("no user to authenticate".into()))?;
        (self.sink)(AgentEvent::Begin(Request {
            cookie,
            action: action_id,
            message,
            command: details.get("command_line").cloned(),
            user,
        }));
        let outcome = poll_fn(|cx| {
            let Ok(mut slot) = slot.lock() else {
                return Poll::Ready(Outcome::Cancelled);
            };
            match slot.outcome.clone() {
                Some(outcome) => Poll::Ready(outcome),
                None => {
                    slot.waker = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        })
        .await;
        match outcome {
            Outcome::Done => Ok(()),
            Outcome::Cancelled => Err(PolkitError::Cancelled("cancelled by the user".into())),
        }
    }

    fn cancel_authentication(&self, cookie: String) {
        (self.sink)(AgentEvent::Cancelled {
            cookie: cookie.clone(),
        });
        self.pending.finish(&cookie, Outcome::Cancelled);
    }
}

/// The agent, registered with polkit for this session.
#[derive(Clone)]
pub struct Agent {
    sink: Sink,
    pending: Pending,
    helper: Option<PathBuf>,
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agent")
            .field("helper", &self.helper)
            .finish_non_exhaustive()
    }
}

impl Agent {
    /// Registers with polkit on `system_bus` (the machine's when `None`),
    /// on a thread; `helper` replaces polkit's helper (tests).
    pub fn start(
        system_bus: Option<String>,
        helper: Option<PathBuf>,
        sink: impl Fn(AgentEvent) + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        let agent = Self {
            sink: Arc::new(sink),
            pending: Pending::default(),
            helper,
        };
        let (sink, pending) = (agent.sink.clone(), agent.pending.clone());
        std::thread::Builder::new()
            .name("polkit agent".into())
            .spawn(move || {
                let served = zbus::block_on(async {
                    let conn = system(&system_bus).await?;
                    conn.object_server()
                        .at(PATH, AgentObject { sink, pending })
                        .await?;
                    let authority = zbus::Proxy::new(
                        &conn,
                        AUTHORITY,
                        "/org/freedesktop/PolicyKit1/Authority",
                        "org.freedesktop.PolicyKit1.Authority",
                    )
                    .await?;
                    let locale = std::env::var("LANG").unwrap_or_else(|_| "C".into());
                    authority
                        .call::<_, _, ()>(
                            "RegisterAuthenticationAgent",
                            &(subject(), locale.as_str(), PATH),
                        )
                        .await?;
                    tracing::info!("polkit agent registered");
                    // Requests are served by the connection's executor.
                    std::future::pending::<()>().await;
                    zbus::Result::Ok(())
                });
                if let Err(err) = served {
                    tracing::info!(%err, "no polkit agent");
                }
            })?;
        Ok(agent)
    }

    /// Checks `password` for `request`'s user through polkit's helper, on a
    /// thread; the answer comes as [`AgentEvent::Checked`].
    pub fn check(&self, request: &Request, password: String) {
        let (sink, helper) = (self.sink.clone(), self.helper.clone());
        let (cookie, user) = (request.cookie.clone(), request.user.clone());
        let spawned = std::thread::Builder::new()
            .name("polkit helper".into())
            .spawn(move || {
                let result = converse(helper.as_deref(), &user, &cookie, &password);
                sink(AgentEvent::Checked { cookie, result });
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "could not check the password");
        }
    }

    /// The dialog is done: polkit was told by its helper, the request ends.
    pub fn done(&self, cookie: &str) {
        self.pending.finish(cookie, Outcome::Done);
    }

    /// The user cancelled.
    pub fn cancel(&self, cookie: &str) {
        self.pending.finish(cookie, Outcome::Cancelled);
    }
}

/// This session, as polkit knows it: by its logind id, else by this
/// process.
fn subject() -> (&'static str, HashMap<&'static str, Variant<'static>>) {
    if let Ok(id) = std::env::var("XDG_SESSION_ID") {
        return (
            "unix-session",
            HashMap::from([("session-id", Variant::from(id))]),
        );
    }
    let start = std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|s| {
            // After the command's closing parenthesis, the 20th field.
            let rest = s.rsplit_once(')')?.1;
            rest.split_whitespace().nth(19)?.parse::<u64>().ok()
        })
        .unwrap_or(0);
    (
        "unix-process",
        HashMap::from([
            ("pid", Variant::from(std::process::id())),
            ("start-time", Variant::from(start)),
        ]),
    )
}

/// Whose password is asked: this user if polkit accepts them, else the
/// first user it names (an administrator).
fn whose(identities: &[(String, HashMap<String, OwnedVariant>)]) -> Option<String> {
    let uids: Vec<u32> = identities
        .iter()
        .filter(|(kind, _)| kind == "unix-user")
        .filter_map(|(_, d)| d.get("uid").and_then(|v| u32::try_from(v).ok()))
        .collect();
    let me = rustix::process::getuid().as_raw();
    let uid = uids
        .iter()
        .copied()
        .find(|u| *u == me)
        .or(uids.first().copied())?;
    user_name(uid)
}

/// A user's name, from the passwd database.
fn user_name(uid: u32) -> Option<String> {
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    passwd.lines().find_map(|line| {
        let mut f = line.split(':');
        let name = f.next()?;
        let id: u32 = f.nth(1)?.parse().ok()?;
        (id == uid).then(|| name.to_string())
    })
}

/// polkit's helper conversation: the user and the cookie, then each PAM
/// prompt answered with the password, until SUCCESS or FAILURE. The
/// helper tells polkit itself.
fn converse(
    helper: Option<&std::path::Path>,
    user: &str,
    cookie: &str,
    password: &str,
) -> Result<(), String> {
    let socket = std::path::Path::new(HELPER_SOCKET);
    if helper.is_none() && socket.exists() {
        let stream = UnixStream::connect(socket).map_err(|e| format!("polkit's helper: {e}"))?;
        let reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
        let mut writer = stream;
        writeln!(writer, "{user}\n{cookie}").map_err(|e| e.to_string())?;
        return talk(reader, writer, password);
    }
    let program = helper
        .map(std::path::Path::to_path_buf)
        .or_else(|| HELPERS.iter().map(PathBuf::from).find(|p| p.exists()))
        .ok_or("polkit's helper is not installed")?;
    let mut child = Command::new(program)
        .arg(user)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("polkit's helper: {e}"))?;
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return Err("polkit's helper has no pipes".into());
    };
    writeln!(stdin, "{cookie}").map_err(|e| e.to_string())?;
    let result = talk(BufReader::new(stdout), stdin, password);
    let _ = child.wait();
    result
}

fn talk(reader: impl BufRead, mut writer: impl Write, password: &str) -> Result<(), String> {
    let mut said = None;
    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let (word, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        match word {
            "PAM_PROMPT_ECHO_OFF" | "PAM_PROMPT_ECHO_ON" => {
                writeln!(writer, "{password}").map_err(|e| e.to_string())?;
                writer.flush().map_err(|e| e.to_string())?;
            }
            "PAM_ERROR_MSG" => said = Some(rest.trim().to_string()),
            "SUCCESS" => return Ok(()),
            // Empty when the helper said nothing: the dialog says it its way.
            "FAILURE" => return Err(said.unwrap_or_default()),
            _ => {}
        }
    }
    Err("polkit's helper stopped".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_helper_is_answered_until_it_says() {
        let ok = talk(
            "PAM_PROMPT_ECHO_OFF Password: \nSUCCESS\n".as_bytes(),
            Vec::new(),
            "panda",
        );
        assert_eq!(ok, Ok(()));
        let mut written = Vec::new();
        let refused = talk(
            "PAM_PROMPT_ECHO_OFF Password: \nPAM_ERROR_MSG Authentication failure\nFAILURE\n"
                .as_bytes(),
            &mut written,
            "bamboo",
        );
        assert_eq!(refused, Err("Authentication failure".into()));
        assert_eq!(written, b"bamboo\n");
    }

    #[test]
    fn root_is_named_from_the_passwd_database() {
        assert_eq!(user_name(0).as_deref(), Some("root"));
    }
}
