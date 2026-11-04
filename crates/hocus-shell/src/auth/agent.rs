//! The session's polkit authentication agent, on a thread of its own: it
//! registers with polkit's authority on the system bus for the session
//! (`RegisterAuthenticationAgent`, the session's logind id as its
//! subject), again whenever polkit starts again, and serves
//! `org.freedesktop.PolicyKit1.AuthenticationAgent`. It follows polkit's
//! documented D-Bus interfaces; no code of polkit's (LGPL) is used.
//!
//! `BeginAuthentication` reads what the dialog needs (the accounts, the
//! asking process, its program) here, hands the request to the dialog and
//! waits for its answer without holding anything else: the call is
//! answered once the dialog is (granted: nothing; cancelled or refused:
//! polkit's errors). `CancelAuthentication` closes the dialog of that
//! cookie. Only polkit's own calls are taken: anyone else's are refused.
//!
//! The shell is the agent only when it is the session's
//! (`HOCUS_AUTH_AGENT`, which the compositor sets on a virtual terminal):
//! a shell nested in another session leaves that session's agent alone.

use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};

use hocus_link::AUTH_AGENT_ENV as ENV;
use iced::futures::StreamExt;
use tokio::sync::oneshot;
use zbus::{Connection, OwnedVariant, Variant, fdo::DBusProxy, message::Header};

use super::{
    Outcome, Request,
    accounts::{self, Identity},
};
use crate::levels::{RETRY_FIRST, next_retry};

const POLKIT: &str = "org.freedesktop.PolicyKit1";
const AUTHORITY_PATH: &str = "/org/freedesktop/PolicyKit1/Authority";
const AUTHORITY: &str = "org.freedesktop.PolicyKit1.Authority";
/// Where the shell's agent answers.
pub const PATH: &str = "/org/hocuspanda/PolicyKit1/AuthenticationAgent";

static REQUESTS: AtomicU64 = AtomicU64::new(1);

pub fn start() {
    if std::env::var_os(ENV).is_none() {
        tracing::info!("not the session's own shell: no authentication agent");
        return;
    }
    let started = std::thread::Builder::new()
        .name("polkit".to_owned())
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(run()),
                Err(err) => tracing::warn!(%err, "no runtime for the authentication agent"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot start the authentication agent");
    }
}

/// polkit's errors, as its own agents answer.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.PolicyKit1.Error")]
enum Refusal {
    #[zbus(error)]
    ZBus(zbus::Error),
    Failed(String),
    Cancelled(String),
    NotAuthorized(String),
}

struct Agent;

impl Agent {
    /// Whether `header`'s call comes from polkit's authority.
    async fn from_polkit(connection: &Connection, header: &Header<'_>) -> bool {
        let Some(sender) = header.sender() else {
            return false;
        };
        let Ok(bus) = DBusProxy::new(connection).await else {
            return false;
        };
        let Ok(name) = POLKIT.try_into() else {
            return false;
        };
        bus.get_name_owner(name)
            .await
            .is_ok_and(|owner| owner.as_str() == sender.as_str())
    }
}

#[zbus::interface(name = "org.freedesktop.PolicyKit1.AuthenticationAgent")]
impl Agent {
    #[allow(clippy::too_many_arguments)]
    async fn begin_authentication(
        &self,
        action_id: String,
        message: String,
        icon_name: String,
        details: HashMap<String, String>,
        cookie: String,
        identities: Vec<(String, HashMap<String, OwnedVariant>)>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> Result<(), Refusal> {
        if !Self::from_polkit(connection, &header).await {
            tracing::warn!("an authentication asked by another than polkit: refused");
            return Err(Refusal::NotAuthorized("only polkit asks".to_owned()));
        }
        let identities: Vec<Identity> = identities
            .iter()
            .filter_map(|(kind, fields)| {
                let id = |key: &str| fields.get(key).and_then(|v| v.downcast_ref::<u32>().ok());
                match kind.as_str() {
                    "unix-user" => id("uid").map(Identity::User),
                    "unix-group" => id("gid").map(Identity::Group),
                    _ => None,
                }
            })
            .collect();
        let request = request(action_id, message, icon_name, &details, cookie, &identities);
        let Some(request) = request else {
            tracing::warn!("no account polkit offers is known: refused");
            return Err(Refusal::Failed("no account to authenticate".to_owned()));
        };
        let id = request.id;
        tracing::info!(id, action = request.action, "an authentication is asked");
        let (answer, answered) = oneshot::channel();
        super::begin(request, answer);
        match answered.await {
            Ok(Outcome::Granted) => {
                tracing::info!(id, "authorized");
                Ok(())
            }
            Ok(Outcome::Cancelled) => {
                tracing::info!(id, "the authentication was cancelled");
                Err(Refusal::Cancelled("cancelled".to_owned()))
            }
            Ok(Outcome::Refused) | Err(_) => {
                tracing::info!(id, "the authentication failed");
                Err(Refusal::Failed("not authenticated".to_owned()))
            }
        }
    }

    async fn cancel_authentication(
        &self,
        cookie: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> Result<(), Refusal> {
        if !Self::from_polkit(connection, &header).await {
            return Err(Refusal::NotAuthorized("only polkit cancels".to_owned()));
        }
        super::cancel_cookie(&cookie);
        Ok(())
    }
}

/// What the dialog shows of a request, read here off the shell's thread.
fn request(
    action: String,
    message: String,
    icon: String,
    details: &HashMap<String, String>,
    cookie: String,
    identities: &[Identity],
) -> Option<Request> {
    let accounts = accounts::accounts(identities);
    if accounts.is_empty() {
        return None;
    }
    let me = rustix::process::getuid().as_raw();
    let chosen = accounts.iter().position(|a| a.uid == me).unwrap_or(0);
    let pid = details
        .get("polkit.subject-pid")
        .or_else(|| details.get("polkit.caller-pid"))
        .and_then(|p| p.parse::<u32>().ok())
        .filter(|p| *p > 0);
    let pids = pid.map(accounts::ancestry).unwrap_or_default();
    let program = details
        .get("program")
        .filter(|p| !p.is_empty())
        .cloned()
        .or_else(|| pid.and_then(accounts::program))
        .unwrap_or_default();
    let started_by = pid.and_then(accounts::owner).map(accounts::name_of);
    Some(Request {
        id: REQUESTS.fetch_add(1, Ordering::Relaxed),
        cookie,
        action,
        message,
        icon,
        accounts,
        chosen,
        pid,
        pids,
        program,
        started_by,
    })
}

/// The session's id: the one the session's programs are given, else
/// logind's for this process.
async fn session_id(connection: &Connection) -> Option<String> {
    if let Some(id) = std::env::var("XDG_SESSION_ID")
        .ok()
        .filter(|id| !id.is_empty())
    {
        return Some(id);
    }
    let reply = connection
        .call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1/session/auto",
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.login1.Session", "Id"),
        )
        .await
        .ok()?;
    let value: OwnedVariant = reply.body().deserialize().ok()?;
    String::try_from(value).ok().filter(|id| !id.is_empty())
}

/// The messages' locale polkit translates into.
fn locale() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|v| std::env::var(v).ok())
        .find(|v| !v.is_empty())
        .unwrap_or_else(|| "C".to_owned())
}

async fn register(connection: &Connection, session: &str) -> zbus::Result<()> {
    let subject: (&str, HashMap<&str, Variant<'_>>) = (
        "unix-session",
        HashMap::from([("session-id", Variant::from(session))]),
    );
    connection
        .call_method(
            Some(POLKIT),
            AUTHORITY_PATH,
            Some(AUTHORITY),
            "RegisterAuthenticationAgent",
            &(subject, locale(), PATH),
        )
        .await?;
    Ok(())
}

async fn run() {
    let mut wait = RETRY_FIRST;
    let connection = loop {
        match Connection::system().await {
            Ok(connection) => break connection,
            Err(err) => {
                tracing::info!(%err, "no system bus yet: no authentication agent");
                tokio::time::sleep(wait).await;
                wait = next_retry(wait);
            }
        }
    };
    if let Err(err) = connection.object_server().at(PATH, Agent).await {
        tracing::warn!(%err, "cannot serve the authentication agent");
        return;
    }
    let Some(session) = session_id(&connection).await else {
        tracing::warn!("no session id: polkit asks no authentication of this session");
        return;
    };
    let owners = match DBusProxy::new(&connection).await {
        Ok(bus) => {
            bus.receive_name_owner_changed_with_args(&[(0, POLKIT)])
                .await
        }
        Err(err) => Err(err),
    };
    let mut owners = match owners {
        Ok(owners) => Some(owners),
        Err(err) => {
            tracing::warn!(%err, "polkit's restarts are not followed");
            None
        }
    };
    loop {
        match register(&connection, &session).await {
            Ok(()) => tracing::info!(session, "the session's authentication agent"),
            Err(err) => tracing::warn!(
                %err,
                "polkit did not take the authentication agent: it asks no password here"
            ),
        }
        // polkit started again (or for the first time): register again.
        let Some(changes) = owners.as_mut() else {
            std::future::pending::<()>().await;
            return;
        };
        loop {
            let Some(changed) = changes.next().await else {
                std::future::pending::<()>().await;
                return;
            };
            if changed.args().is_ok_and(|a| a.new_owner().is_some()) {
                break;
            }
        }
    }
}
