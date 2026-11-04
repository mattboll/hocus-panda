//! GNOME Online Accounts (#425's amendment, #567): the Google accounts
//! whose calendar is on are calendar sources, their access tokens asked of
//! Online Accounts each time one expires. Nothing of the account (no
//! refresh token, no secret) is kept by us.
//!
//! On the session bus the compositor names, never another:
//! `org.gnome.OnlineAccounts`, its accounts under the object manager
//! `/org/gnome/OnlineAccounts`, each with `…Account` (`ProviderType`,
//! `Identity`, `CalendarDisabled`, `AttentionNeeded`) and `…OAuth2Based`
//! (`GetAccessToken`).
//!
//! A thread of its own (`day: online accounts`) lists them as it starts,
//! then each time Online Accounts says something changed
//! (`InterfacesAdded`, `InterfacesRemoved`, `PropertiesChanged`, or its
//! name changing hands), and starts or stops their workers. It sleeps
//! meanwhile.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    time::Duration,
};

use zbus::{
    Connection, MatchRule, MessageStream, OwnedObjectPath, OwnedVariant, connection::Builder,
    message::Type,
};

use crate::{
    Launcher,
    config::{self, CalendarEntry},
    google::{AccessToken, GoogleCalendar},
    model::AccountId,
    secrets::Token,
    snapshot::Account,
    source::{Result, SourceError},
    worker::{Plan, Source},
};

pub const NAME: &str = "org.gnome.OnlineAccounts";
pub const ROOT: &str = "/org/gnome/OnlineAccounts";
pub const ACCOUNT: &str = "org.gnome.OnlineAccounts.Account";
pub const OAUTH2: &str = "org.gnome.OnlineAccounts.OAuth2Based";
/// Online Accounts may refresh a token over the network before it answers.
const WAIT: Duration = Duration::from_secs(30);

/// An Online Accounts' Google account, as listed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Listed {
    path: OwnedObjectPath,
    /// Its e-mail, as Online Accounts names it.
    identity: String,
    presentation: String,
    calendar: bool,
    /// It needs signing in again.
    attention: bool,
}

/// What went wrong asking Online Accounts.
enum Failure {
    /// It answered no (the account needs signing in again).
    Refused(String),
    /// It did not answer (no bus, not on it, not in time).
    Silent(String),
}

/// `work` on a connection to `bus` ("session": the session's own), within
/// [`WAIT`], on this thread.
fn block<T>(
    bus: &str,
    work: impl AsyncFnOnce(Connection) -> zbus::Result<T>,
) -> std::result::Result<T, Failure> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| Failure::Silent(err.to_string()))?;
    runtime.block_on(async {
        let asked = async { work(connect(bus).await?).await };
        match tokio::time::timeout(WAIT, asked).await {
            Ok(Ok(answer)) => Ok(answer),
            Ok(Err(err)) => Err(failure(err)),
            Err(_) => Err(Failure::Silent("no answer in time".to_owned())),
        }
    })
}

fn failure(err: zbus::Error) -> Failure {
    match err {
        zbus::Error::MethodError(name, detail, _)
            if !name.starts_with("org.freedesktop.DBus.Error.") =>
        {
            Failure::Refused(detail.unwrap_or_else(|| name.to_string()))
        }
        err => Failure::Silent(err.to_string()),
    }
}

async fn connect(bus: &str) -> zbus::Result<Connection> {
    let builder = if bus == "session" {
        Builder::session()
    } else {
        Builder::address(bus)
    };
    builder.build().await
}

/// An account's access token, asked of Online Accounts on `bus`.
#[derive(Debug)]
pub struct GoaToken {
    bus: String,
    account: OwnedObjectPath,
}

impl GoaToken {
    pub fn new(bus: String, account: OwnedObjectPath) -> Self {
        Self { bus, account }
    }
}

impl AccessToken for GoaToken {
    fn get(&mut self) -> Result<(Token, Duration)> {
        let path = self.account.clone();
        let asked = block(&self.bus, async move |connection| {
            let reply = connection
                .call_method(
                    Some(NAME),
                    path.as_str(),
                    Some(OAUTH2),
                    "GetAccessToken",
                    &(),
                )
                .await?;
            reply.body().deserialize::<(String, i32)>()
        });
        match asked {
            Ok((token, expires)) => Ok((
                Token::new(token),
                Duration::from_secs(u64::try_from(expires).unwrap_or(0)),
            )),
            Err(Failure::Refused(why)) => Err(SourceError::SignIn(format!(
                "Online Accounts gives no token: {why}"
            ))),
            Err(Failure::Silent(why)) => Err(SourceError::Offline(format!(
                "Online Accounts does not answer: {why}"
            ))),
        }
    }
}

/// What the Google accounts are read with.
pub(crate) struct Google {
    /// The configuration's `[[day.calendars]] kind = "google"`, in order.
    pub entries: Vec<CalendarEntry>,
    /// The session bus the compositor names.
    pub bus: String,
    /// The Calendar API's address.
    pub api: String,
}

/// Watches Online Accounts on a thread of its own, starting and stopping
/// the Google accounts' workers through `launcher`.
pub(crate) fn watch(google: Google, launcher: Launcher) {
    let started = std::thread::Builder::new()
        .name("day: online accounts".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(watching(Accounts {
                    google,
                    launcher,
                    running: BTreeMap::new(),
                    missing: BTreeSet::new(),
                })),
                Err(err) => tracing::warn!(%err, "no runtime to watch Online Accounts"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "no thread to watch Online Accounts: no Google calendars");
    }
}

async fn watching(mut accounts: Accounts) {
    let bus = accounts.google.bus.clone();
    let streams = async {
        let connection = connect(&bus).await?;
        let goa = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(NAME)
            .path_namespace(ROOT)
            .build()?;
        let owner = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender("org.freedesktop.DBus")
            .interface("org.freedesktop.DBus")
            .member("NameOwnerChanged")
            .add_arg(NAME)
            .build()?;
        // Before the first listing, so that nothing said meanwhile is missed.
        let goa = MessageStream::for_match_rule(goa, &connection, None).await?;
        let owner = MessageStream::for_match_rule(owner, &connection, None).await?;
        zbus::Result::Ok((connection, goa, owner))
    };
    let (connection, goa, owner) = match streams.await {
        Ok(streams) => streams,
        Err(err) => {
            tracing::info!(%err, "the session bus does not answer: no Google calendars");
            return;
        }
    };
    let (mut goa, mut owner) = (std::pin::pin!(goa), std::pin::pin!(owner));
    loop {
        match list(&connection).await {
            Ok(listed) => accounts.update(&listed),
            Err(err) => {
                tracing::info!(%err, "Online Accounts is not read: its accounts as they were")
            }
        }
        let next = std::future::poll_fn(|cx| {
            use std::task::Poll;
            use zbus::export::futures_core::Stream;
            match goa.as_mut().poll_next(cx) {
                Poll::Pending => owner.as_mut().poll_next(cx),
                ready => ready,
            }
        });
        if next.await.is_none() {
            tracing::info!("the session bus went: Online Accounts no longer watched");
            return;
        }
        tracing::debug!("Online Accounts changed: read again");
    }
}

/// Online Accounts' Google accounts.
async fn list(connection: &Connection) -> zbus::Result<Vec<Listed>> {
    type Objects = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedVariant>>>;
    let reply = connection
        .call_method(
            Some(NAME),
            ROOT,
            Some("org.freedesktop.DBus.ObjectManager"),
            "GetManagedObjects",
            &(),
        )
        .await?;
    let objects: Objects = reply.body().deserialize()?;
    let mut listed: Vec<Listed> = objects
        .into_iter()
        .filter_map(|(path, interfaces)| {
            let account = interfaces.get(ACCOUNT)?;
            interfaces.get(OAUTH2)?;
            let text = |name: &str| {
                account
                    .get(name)
                    .and_then(|v| v.downcast_ref::<&str>().ok().map(str::to_owned))
                    .unwrap_or_default()
            };
            let flag = |name: &str| {
                account
                    .get(name)
                    .and_then(|v| v.downcast_ref::<bool>().ok())
                    .unwrap_or(false)
            };
            (text("ProviderType") == "google").then(|| Listed {
                identity: text("Identity"),
                presentation: text("PresentationIdentity"),
                calendar: !flag("CalendarDisabled"),
                attention: flag("AttentionNeeded"),
                path,
            })
        })
        .collect();
    listed.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
    Ok(listed)
}

/// A Google account's worker running.
struct Running {
    path: OwnedObjectPath,
    attention: bool,
}

/// The Google accounts' workers, as Online Accounts lists them.
struct Accounts {
    google: Google,
    launcher: Launcher,
    running: BTreeMap<AccountId, Running>,
    /// The accounts the configuration names that Online Accounts does not
    /// have (said once).
    missing: BTreeSet<String>,
}

impl Accounts {
    /// The workers as `listed` says: an account with its calendar on
    /// that an entry takes runs, the others stop (their cache forgotten:
    /// removed, or its calendar switched off). One signed in again is
    /// read again.
    fn update(&mut self, listed: &[Listed]) {
        let mut wanted: BTreeMap<AccountId, (Account, &CalendarEntry, &Listed)> = BTreeMap::new();
        for account in listed.iter().filter(|l| l.calendar) {
            let Some(entry) = entry_for(&self.google.entries, account) else {
                continue;
            };
            let name = if account.presentation.is_empty() {
                account.identity.clone()
            } else {
                account.presentation.clone()
            };
            let made = config::google_account(&account.identity, name);
            wanted
                .entry(made.id.clone())
                .or_insert((made, entry, account));
        }
        let gone: Vec<AccountId> = self
            .running
            .iter()
            .filter(|(id, running)| {
                wanted
                    .get(*id)
                    .is_none_or(|(_, _, listed)| listed.path != running.path)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in gone {
            tracing::info!(account = %id, "a Google account stops: removed from Online Accounts, or its calendar off");
            self.running.remove(&id);
            self.launcher.stop(&id, true);
        }
        for (id, (account, entry, listed)) in wanted {
            if let Some(running) = self.running.get_mut(&id) {
                if running.attention && !listed.attention {
                    tracing::info!(account = %id, "a Google account signed in again: read again");
                    self.launcher.refresh(&id);
                }
                running.attention = listed.attention;
                continue;
            }
            tracing::info!(account = %id, "a Google account of Online Accounts starts");
            let plan = plan(account, entry, listed, &self.google);
            if self.launcher.launch(plan) {
                self.running.insert(
                    id,
                    Running {
                        path: listed.path.clone(),
                        attention: listed.attention,
                    },
                );
            }
        }
        let missing: BTreeSet<String> = self
            .google
            .entries
            .iter()
            .map(|e| e.account.trim().to_owned())
            .filter(|named| {
                !named.is_empty()
                    && !listed.iter().any(|l| {
                        l.identity.eq_ignore_ascii_case(named)
                            || l.presentation.eq_ignore_ascii_case(named)
                    })
            })
            .collect();
        for named in missing.difference(&self.missing) {
            tracing::info!(account = named, "no such Google account in Online Accounts");
        }
        self.missing = missing;
    }
}

/// The entry that takes `account`: the first that names it, else the
/// first that names none.
fn entry_for<'a>(entries: &'a [CalendarEntry], account: &Listed) -> Option<&'a CalendarEntry> {
    let names = |e: &CalendarEntry| {
        let named = e.account.trim();
        !named.is_empty()
            && (named.eq_ignore_ascii_case(&account.identity)
                || named.eq_ignore_ascii_case(&account.presentation))
    };
    entries
        .iter()
        .find(|e| names(e))
        .or_else(|| entries.iter().find(|e| e.account.trim().is_empty()))
}

fn plan(account: Account, entry: &CalendarEntry, listed: &Listed, google: &Google) -> Plan {
    let (shown, colours) = (entry.shown.clone(), entry.colors.clone());
    let (id, bus, api, path) = (
        account.id.clone(),
        google.bus.clone(),
        google.api.clone(),
        listed.path.clone(),
    );
    Plan {
        account,
        make: Box::new(move || Source::Calendars {
            source: Box::new(GoogleCalendar::new(
                id,
                Box::new(GoaToken::new(bus, path)),
                api,
            )),
            shown,
            colours,
        }),
        every: None,
        cache: None,
        watch: None,
    }
}
