//! Google calendars through GNOME Online Accounts (#137), on the session
//! bus, by its public D-Bus API only:
//!
//! - `org.gnome.OnlineAccounts` owns an `org.freedesktop.DBus.ObjectManager`
//!   at `/org/gnome/OnlineAccounts`; each account is an object below it
//!   with `org.gnome.OnlineAccounts.Account` (properties `Id`,
//!   `ProviderType`, `Identity`, `PresentationIdentity`, `CalendarDisabled`;
//!   method `EnsureCredentials() → (i expires_in)`) and, for Google,
//!   `org.gnome.OnlineAccounts.OAuth2Based` (method
//!   `GetAccessToken() → (s access_token, i expires_in)`).
//! - Accounts come and go with `InterfacesAdded` and `InterfacesRemoved`,
//!   change with `PropertiesChanged`.
//!
//! The accounts whose `ProviderType` is `google` and whose calendar is on
//! are read through Google's CalDAV endpoint with the token GOA hands out,
//! asked afresh at each sync and never kept: GOA refreshes it when it
//! expires. A token Google refuses is renewed once (`EnsureCredentials`,
//! then `GetAccessToken`), and goes to Google's server only.
//!
//! Everything here blocks or runs on threads of its own. Without GOA,
//! nothing is said but at debug level.

use std::{collections::HashMap, future::poll_fn, pin::Pin, sync::Arc, task::Poll};

use futures_core::Stream;
use jiff::Timestamp;
use zbus::{
    Connection, MessageStream, OwnedObjectPath, OwnedVariant, Proxy, fdo::DBusProxy, message::Type,
    names::BusName,
};

use super::caldav::{self, FetchError, Fetched, Source, Token};

const SERVICE: &str = "org.gnome.OnlineAccounts";
const ROOT: &str = "/org/gnome/OnlineAccounts";
const ACCOUNT: &str = "org.gnome.OnlineAccounts.Account";
const OAUTH2: &str = "org.gnome.OnlineAccounts.OAuth2Based";

/// Google's CalDAV, as GOA's Google provider names it: the principal, the
/// calendar home and the calendars are found from there.
pub const GOOGLE_CALDAV: &str = "https://apidata.googleusercontent.com/caldav/v2/";

/// A Google account of GOA's with its calendar on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoogleAccount {
    /// Its object on the bus.
    pub path: String,
    /// GOA's id for it (`account_…`), which stays.
    pub id: String,
    /// Its address, the primary calendar's id.
    pub identity: String,
    /// What GOA shows of it.
    pub name: String,
}

/// What GOA holds, as followed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnlineAccounts {
    /// Running, or there for the bus to start.
    pub installed: bool,
    pub accounts: Vec<GoogleAccount>,
}

type Sink = Arc<dyn Fn(OnlineAccounts) + Send + Sync>;
type Objects = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedVariant>>>;

/// Follows GOA on the session bus at `bus`, on a thread of its own: `sink`
/// hears what it holds first, then each change.
pub fn follow(
    bus: String,
    sink: impl Fn(OnlineAccounts) + Send + Sync + 'static,
) -> std::io::Result<()> {
    let sink: Sink = Arc::new(sink);
    std::thread::Builder::new()
        .name("online accounts".into())
        .spawn(move || {
            if let Err(err) = zbus::block_on(watch(&bus, &sink)) {
                // Not a fault: a bus that went.
                tracing::debug!(%err, "GNOME Online Accounts is not followed");
                sink(OnlineAccounts {
                    installed: false,
                    accounts: Vec::new(),
                });
            }
        })?;
    Ok(())
}

fn get<T>(props: &HashMap<String, OwnedVariant>, key: &str) -> Option<T>
where
    T: TryFrom<OwnedVariant>,
{
    T::try_from(props.get(key)?.try_clone().ok()?).ok()
}

async fn proxy(conn: &Connection, path: &str, iface: &'static str) -> zbus::Result<Proxy<'static>> {
    zbus::proxy::Builder::new(conn)
        .destination(SERVICE)
        .path(path.to_owned())
        .interface(iface)
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
}

/// The Google accounts with their calendar on, by name; asking starts GOA
/// when the bus can.
async fn accounts(conn: &Connection) -> zbus::Result<Vec<GoogleAccount>> {
    let manager = proxy(conn, ROOT, "org.freedesktop.DBus.ObjectManager").await?;
    let all: Objects = manager.call("GetManagedObjects", &()).await?;
    let mut out: Vec<GoogleAccount> = all
        .into_iter()
        .filter(|(_, interfaces)| interfaces.contains_key(OAUTH2))
        .filter_map(|(path, interfaces)| {
            let account = interfaces.get(ACCOUNT)?;
            let google = get::<String>(account, "ProviderType").as_deref() == Some("google");
            let calendar = !get::<bool>(account, "CalendarDisabled").unwrap_or(true);
            let id = get::<String>(account, "Id").filter(|id| !id.is_empty())?;
            let identity = get::<String>(account, "Identity").unwrap_or_default();
            let name = get::<String>(account, "PresentationIdentity")
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| identity.clone());
            (google && calendar && !identity.is_empty()).then(|| GoogleAccount {
                path: path.as_str().to_owned(),
                id,
                identity,
                name,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    Ok(out)
}

enum Either<A, B> {
    Left(A),
    Right(B),
}

async fn watch(bus: &str, sink: &Sink) -> zbus::Result<()> {
    let conn = zbus::connection::Builder::address(bus).build().await?;
    let dbus = DBusProxy::new(&conn).await?;
    let mut owners = dbus
        .receive_name_owner_changed_with_args(&[(0, SERVICE)])
        .await?;
    // The object manager's signals and the accounts' PropertiesChanged.
    let rule = zbus::MatchRule::builder()
        .msg_type(Type::Signal)
        .sender(SERVICE)
        .path_namespace(ROOT)
        .build()?;
    let mut signals = MessageStream::for_match_rule(rule, &conn, None).await?;
    let name = BusName::try_from(SERVICE)?;
    let running = dbus.name_has_owner(name).await?;
    let activatable = dbus
        .list_activatable_names()
        .await?
        .iter()
        .any(|n| n.as_str() == SERVICE);
    let mut shown: Option<OnlineAccounts> = None;
    let read = |installed: bool| {
        let conn = conn.clone();
        async move {
            if !installed {
                return Vec::new();
            }
            accounts(&conn).await.unwrap_or_else(|err| {
                tracing::debug!(%err, "GNOME Online Accounts did not list its accounts");
                Vec::new()
            })
        }
    };
    let tell = |shown: &mut Option<OnlineAccounts>, now: OnlineAccounts| {
        if shown.as_ref() != Some(&now) {
            *shown = Some(now.clone());
            sink(now);
        }
    };
    let installed = running || activatable;
    tracing::debug!(installed, running, "GNOME Online Accounts");
    let now = read(installed).await;
    tell(
        &mut shown,
        OnlineAccounts {
            installed,
            accounts: now,
        },
    );
    loop {
        let next = poll_fn(|cx| {
            if let Poll::Ready(change) = Pin::new(&mut owners).poll_next(cx) {
                return Poll::Ready(change.map(Either::Left));
            }
            if let Poll::Ready(message) = Pin::new(&mut signals).poll_next(cx) {
                return Poll::Ready(message.map(Either::Right));
            }
            Poll::Pending
        })
        .await;
        match next {
            None => return Ok(()),
            Some(Either::Left(change)) => {
                if change.args().is_ok_and(|a| a.new_owner().is_some()) {
                    let accounts = read(true).await;
                    tell(
                        &mut shown,
                        OnlineAccounts {
                            installed: true,
                            accounts,
                        },
                    );
                } else if !activatable {
                    // Gone for good; when the bus can start it again, its
                    // accounts stay until it says otherwise.
                    tell(
                        &mut shown,
                        OnlineAccounts {
                            installed: false,
                            accounts: Vec::new(),
                        },
                    );
                }
            }
            // An account came, went or changed (its calendar switched).
            Some(Either::Right(Ok(_))) => {
                let accounts = read(true).await;
                tell(
                    &mut shown,
                    OnlineAccounts {
                        installed: true,
                        accounts,
                    },
                );
            }
            Some(Either::Right(Err(err))) => {
                tracing::debug!(%err, "a GNOME Online Accounts signal");
            }
        }
    }
}

/// What GOA answered, as a fetch's error: a refusal when it would not hand
/// out a token (the account needs signing in again).
fn refused(err: zbus::Error) -> FetchError {
    match err {
        zbus::Error::MethodError(name, message, _) => FetchError::Refused(format!(
            "GNOME Online Accounts: {}",
            message.unwrap_or_else(|| name.as_str().to_owned())
        )),
        other => FetchError::Failed(format!("GNOME Online Accounts: {other}")),
    }
}

async fn access_token(conn: &Connection, path: &str) -> zbus::Result<Token> {
    let oauth = proxy(conn, path, OAUTH2).await?;
    let (token, _expires_in): (String, i32) = oauth.call("GetAccessToken", &()).await?;
    Ok(Token(token))
}

async fn ensure_credentials(conn: &Connection, path: &str) -> zbus::Result<()> {
    let account = proxy(conn, path, ACCOUNT).await?;
    let _expires_in: i32 = account.call("EnsureCredentials", &()).await?;
    Ok(())
}

/// The primary calendar's id in a path: an address, kept readable.
fn calendar_id(identity: &str) -> String {
    identity
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-._~@+".contains(c) {
                c.to_string()
            } else {
                let mut buf = [0; 4];
                c.encode_utf8(&mut buf)
                    .bytes()
                    .map(|b| format!("%{b:02X}"))
                    .collect()
            }
        })
        .collect()
}

/// The account's calendars at `base` (Google's CalDAV, or a test's
/// stand-in): found from the base, else the primary one alone.
fn google(
    base: &str,
    identity: &str,
    token: Token,
    from: Timestamp,
    to: Timestamp,
) -> Result<Vec<Fetched>, FetchError> {
    let found = caldav::fetch(
        &Source::Bearer {
            url: base.to_owned(),
            token: token.clone(),
        },
        from,
        to,
    );
    match found {
        Err(FetchError::Failed(_)) => caldav::fetch(
            &Source::Bearer {
                url: format!("{base}{}/events/", calendar_id(identity)),
                token,
            },
            from,
            to,
        ),
        other => other,
    }
}

/// Every calendar of `account`, with the events of `from`..`to`, read with
/// GOA's token from the bus at `bus`; blocking, for a worker thread. A
/// token refused is renewed once.
pub fn fetch(
    bus: &str,
    account: &GoogleAccount,
    base: &str,
    from: Timestamp,
    to: Timestamp,
) -> Result<Vec<Fetched>, FetchError> {
    let conn = zbus::block_on(zbus::connection::Builder::address(bus).build())
        .map_err(|e| FetchError::Failed(format!("the session bus: {e}")))?;
    let token = zbus::block_on(access_token(&conn, &account.path)).map_err(refused)?;
    match google(base, &account.identity, token, from, to) {
        Err(FetchError::Refused(_)) => {
            zbus::block_on(ensure_credentials(&conn, &account.path)).map_err(refused)?;
            let token = zbus::block_on(access_token(&conn, &account.path)).map_err(refused)?;
            google(base, &account.identity, token, from, to)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_stays_readable_in_a_path() {
        assert_eq!(
            calendar_id("alex.panda@example.com"),
            "alex.panda@example.com"
        );
        assert_eq!(calendar_id("a b/é@x.org"), "a%20b%2F%C3%A9@x.org");
    }

    #[test]
    fn a_token_is_never_printed() {
        let source = Source::Bearer {
            url: GOOGLE_CALDAV.into(),
            token: Token("ya29.secret".into()),
        };
        assert!(!format!("{source:?}").contains("secret"));
    }
}
