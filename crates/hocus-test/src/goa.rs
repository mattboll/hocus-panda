//! A stand-in for GNOME Online Accounts on a private session bus (never the
//! session's), with one Google account, Alex's, its calendar on; and a
//! stand-in for Google's CalDAV on 127.0.0.1, at Google's paths, which
//! takes only the access token the stand-in handed out last. Both write
//! down what they were asked, in one log.
//!
//! GOA's side is its public D-Bus API: an object manager at
//! `/org/gnome/OnlineAccounts`, accounts below it with
//! `org.gnome.OnlineAccounts.Account` and
//! `org.gnome.OnlineAccounts.OAuth2Based`.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::Context;
use zbus::{Connection, interface};

use crate::caldav::{HttpRequest, Server, events, report};

const SERVICE: &str = "org.gnome.OnlineAccounts";
const ROOT: &str = "/org/gnome/OnlineAccounts";
/// GOA's own id of the account, as its path ends.
pub const ACCOUNT_ID: &str = "account_1761055200_0";
const ACCOUNT: &str = "/org/gnome/OnlineAccounts/Accounts/account_1761055200_0";

/// What was asked, in order: `GetAccessToken`, `EnsureCredentials`, and
/// Google's `PROPFIND /caldav/v2/ ok` or `REPORT … refused`.
pub type Log = Arc<Mutex<Vec<String>>>;

fn note(log: &Log, line: String) {
    if let Ok(mut log) = log.lock() {
        log.push(line);
    }
}

/// The access tokens: the one GOA hands out and the one Google takes.
#[derive(Debug, Default)]
struct Tokens {
    handed: u32,
    taken: u32,
    /// Google takes none, whatever GOA hands out.
    broken: bool,
}

type Shared = Arc<Mutex<Tokens>>;

fn token(n: u32) -> String {
    format!("ya29.stand-in-{n}")
}

struct Account {
    identity: String,
    calendar_disabled: bool,
    tokens: Shared,
    log: Log,
}

#[interface(name = "org.gnome.OnlineAccounts.Account")]
impl Account {
    #[zbus(property)]
    fn provider_type(&self) -> String {
        "google".into()
    }

    #[zbus(property)]
    fn provider_name(&self) -> String {
        "Google".into()
    }

    #[zbus(property)]
    fn id(&self) -> String {
        ACCOUNT_ID.into()
    }

    #[zbus(property)]
    fn identity(&self) -> String {
        self.identity.clone()
    }

    #[zbus(property)]
    fn presentation_identity(&self) -> String {
        self.identity.clone()
    }

    #[zbus(property)]
    fn attention_needed(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn calendar_disabled(&self) -> bool {
        self.calendar_disabled
    }

    /// As GOA does, the credentials are checked and the token refreshed:
    /// the new one is the one Google takes.
    fn ensure_credentials(&self) -> i32 {
        note(&self.log, "EnsureCredentials".into());
        if let Ok(mut tokens) = self.tokens.lock() {
            tokens.handed = tokens.taken;
        }
        3600
    }
}

struct OAuth2 {
    tokens: Shared,
    log: Log,
}

#[interface(name = "org.gnome.OnlineAccounts.OAuth2Based")]
impl OAuth2 {
    #[zbus(property)]
    fn client_id(&self) -> String {
        "stand-in.apps.googleusercontent.com".into()
    }

    fn get_access_token(&self) -> (String, i32) {
        note(&self.log, "GetAccessToken".into());
        let handed = self.tokens.lock().map(|t| t.handed).unwrap_or_default();
        (token(handed), 3600)
    }
}

pub struct FakeGoa {
    connection: Connection,
    identity: String,
    tokens: Shared,
    pub log: Log,
    _google: Server,
    google: String,
}

impl std::fmt::Debug for FakeGoa {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeGoa")
            .field("identity", &self.identity)
            .field("log", &self.log)
            .finish_non_exhaustive()
    }
}

impl FakeGoa {
    /// Google's CalDAV, serving the `.ics` files of `dir` as the account's
    /// one calendar; GOA's name on the bus at `address` once `running`,
    /// with the Google account `identity` signed in.
    pub fn start(
        address: &str,
        identity: &str,
        dir: PathBuf,
        running: bool,
    ) -> anyhow::Result<Self> {
        let log = Log::default();
        let tokens = Shared::default();
        let google = google(dir, identity, tokens.clone(), log.clone())?;
        let base = google.url().to_owned();
        let connection = zbus::block_on(
            zbus::connection::Builder::address(address)
                .serve_at(ROOT, zbus::fdo::ObjectManager)
                .build(),
        )
        .context("could not reach the test bus")?;
        let goa = Self {
            connection,
            identity: identity.to_owned(),
            tokens,
            log,
            _google: google,
            google: base,
        };
        goa.sign_in()?;
        if running {
            goa.run()?;
        }
        Ok(goa)
    }

    /// Takes GOA's name, as its daemon starting would.
    pub fn run(&self) -> anyhow::Result<()> {
        zbus::block_on(self.connection.request_name(SERVICE))
            .context("could not take GNOME Online Accounts' name on the bus")?;
        Ok(())
    }

    /// Where Google's stand-in is, as Google's CalDAV base.
    pub fn google(&self) -> &str {
        &self.google
    }

    /// The account is added, as signing in to Google in GOA's settings
    /// would: InterfacesAdded.
    pub fn sign_in(&self) -> anyhow::Result<()> {
        let server = self.connection.object_server();
        zbus::block_on(async {
            server
                .at(
                    ACCOUNT,
                    Account {
                        identity: self.identity.clone(),
                        calendar_disabled: false,
                        tokens: self.tokens.clone(),
                        log: self.log.clone(),
                    },
                )
                .await?;
            server
                .at(
                    ACCOUNT,
                    OAuth2 {
                        tokens: self.tokens.clone(),
                        log: self.log.clone(),
                    },
                )
                .await
        })?;
        Ok(())
    }

    /// The account is removed in GOA: InterfacesRemoved.
    pub fn remove(&self) -> anyhow::Result<()> {
        let server = self.connection.object_server();
        zbus::block_on(async {
            server.remove::<OAuth2, _>(ACCOUNT).await?;
            server.remove::<Account, _>(ACCOUNT).await
        })?;
        Ok(())
    }

    /// The account's Calendar switch in GOA: PropertiesChanged.
    pub fn set_calendar(&self, on: bool) -> anyhow::Result<()> {
        zbus::block_on(async {
            let iface = self
                .connection
                .object_server()
                .interface::<_, Account>(ACCOUNT)
                .await?;
            iface.get_mut().await.calendar_disabled = !on;
            iface
                .get()
                .await
                .calendar_disabled_changed(iface.signal_emitter())
                .await
        })?;
        Ok(())
    }

    /// Google no longer takes the token GOA holds (it expired, or was
    /// revoked); GOA's next EnsureCredentials gets a new one.
    pub fn revoke(&self) {
        if let Ok(mut tokens) = self.tokens.lock() {
            tokens.taken = tokens.handed + 1;
        }
    }

    /// From now on Google takes no token at all.
    pub fn break_tokens(&self) {
        if let Ok(mut tokens) = self.tokens.lock() {
            tokens.broken = true;
        }
    }

    /// What was asked so far.
    pub fn asked(&self) -> Vec<String> {
        self.log.lock().map(|l| l.clone()).unwrap_or_default()
    }
}

/// Google's CalDAV as far as discovery goes: the base names the user's
/// principal, the principal its calendar home, the home the calendars;
/// each calendar is `<calendar id>/events/`, the primary one's id the
/// account's address.
fn google(dir: PathBuf, identity: &str, tokens: Shared, log: Log) -> anyhow::Result<Server> {
    const BASE: &str = "/caldav/v2/";
    let user = identity.replace('@', "%40");
    let principal = format!("{BASE}{user}/user");
    let home = format!("{BASE}{user}/");
    let calendar = format!("{BASE}{user}/events/");
    let name = identity.to_owned();
    let server = Server::serve("google caldav", BASE, move |request: &HttpRequest| {
        let taken = tokens
            .lock()
            .ok()
            .filter(|t| !t.broken)
            .map(|t| format!("Bearer {}", token(t.taken)));
        let path = request.path.as_str();
        if taken.is_none() || request.authorization != taken {
            note(&log, format!("{} {path} refused", request.method));
            return ("401 Unauthorized", String::new());
        }
        note(&log, format!("{} {path} ok", request.method));
        let found = |href: &str, props: &str| {
            (
                "207 Multi-Status",
                format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:caldav="urn:ietf:params:xml:ns:caldav">
<D:response><D:href>{href}</D:href><D:propstat><D:status>HTTP/1.1 200 OK</D:status><D:prop>{props}</D:prop></D:propstat></D:response>
</D:multistatus>"#
                ),
            )
        };
        let calendar_props = format!(
            "<D:resourcetype><D:collection/><caldav:calendar/></D:resourcetype><D:displayname>{name}</D:displayname>"
        );
        match (request.method.as_str(), path) {
            ("PROPFIND", p) if p == BASE => found(
                BASE,
                &format!(
                    "<D:resourcetype><D:collection/></D:resourcetype><D:current-user-principal><D:href>{principal}</D:href></D:current-user-principal>"
                ),
            ),
            ("PROPFIND", p) if p == principal => found(
                &principal,
                &format!(
                    "<D:resourcetype><D:principal/></D:resourcetype><caldav:calendar-home-set><D:href>{home}</D:href></caldav:calendar-home-set>"
                ),
            ),
            ("PROPFIND", p) if p == home && request.depth == "1" => {
                found(&calendar, &calendar_props)
            }
            ("PROPFIND", p) if p == calendar => found(&calendar, &calendar_props),
            ("REPORT", p) if p == calendar => {
                ("207 Multi-Status", report(&calendar, &events(&dir)))
            }
            _ => ("404 Not Found", String::new()),
        }
    })?;
    Ok(server)
}
