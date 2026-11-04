//! Fakes for the Google tests (#567), never the network nor the live
//! session bus: a private `dbus-daemon` of the test's own, on it a fake
//! GNOME Online Accounts, and a fake Calendar API on 127.0.0.1.
//!
//! Shared with the compositor's tests (`#[path]`): std, zbus, tokio,
//! serde_json and anyhow only.

#![allow(dead_code, reason = "each test uses some of the fakes")]

use std::{
    collections::{BTreeSet, HashMap},
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, PoisonError, mpsc},
    time::Duration,
};

use serde_json::{Value, json};

/// How long a daemon has to listen.
const DAEMON_WAIT: Duration = Duration::from_secs(10);

/// A `dbus-daemon` of the test's own, on a socket in its folder; stopped
/// (its own process, by its handle) when dropped.
pub struct PrivateBus {
    daemon: Child,
    pub address: String,
}

impl PrivateBus {
    pub fn start(dir: &Path) -> anyhow::Result<Self> {
        let socket = dir.join("bus");
        let config = dir.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
                 \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
                 <busconfig><type>session</type><listen>unix:path={}</listen>\
                 <auth>EXTERNAL</auth>\
                 <policy context=\"default\"><allow send_destination=\"*\"/>\
                 <allow receive_sender=\"*\"/><allow own=\"*\"/></policy></busconfig>\n",
                socket.display()
            ),
        )?;
        let mut daemon = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let out = daemon
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("no output from dbus-daemon"))?;
        let (said, heard) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let read = BufReader::new(out).read_line(&mut line);
            let _ = said.send(read.map(|_| line));
        });
        match heard.recv_timeout(DAEMON_WAIT) {
            Ok(Ok(line)) if !line.trim().is_empty() => Ok(Self {
                daemon,
                address: format!("unix:path={}", socket.display()),
            }),
            other => {
                let _ = daemon.kill();
                let _ = daemon.wait();
                anyhow::bail!("dbus-daemon did not listen: {other:?}")
            }
        }
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

// --- Online Accounts ---------------------------------------------------------------

const GOA: &str = "org.gnome.OnlineAccounts";
const ROOT: &str = "/org/gnome/OnlineAccounts";

#[derive(Default)]
struct GoaState {
    /// Every token asked for is refused (the account needs signing in).
    refuse: bool,
    /// Tokens given so far: the next is `tok-{given + 1}`.
    given: u32,
}

struct FakeAccount {
    provider: String,
    identity: String,
    calendar_disabled: bool,
    attention_needed: bool,
}

#[zbus::interface(name = "org.gnome.OnlineAccounts.Account")]
impl FakeAccount {
    #[zbus(property)]
    fn provider_type(&self) -> String {
        self.provider.clone()
    }

    #[zbus(property)]
    fn provider_name(&self) -> String {
        "Google".to_owned()
    }

    #[zbus(property)]
    fn id(&self) -> String {
        self.identity.clone()
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
    fn calendar_disabled(&self) -> bool {
        self.calendar_disabled
    }

    #[zbus(property)]
    fn attention_needed(&self) -> bool {
        self.attention_needed
    }
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.gnome.OnlineAccounts.Error")]
enum GoaError {
    #[zbus(error)]
    ZBus(zbus::Error),
    NotAuthorized(String),
}

struct FakeOAuth2(Arc<Mutex<GoaState>>);

#[zbus::interface(name = "org.gnome.OnlineAccounts.OAuth2Based")]
impl FakeOAuth2 {
    fn get_access_token(&self) -> Result<(String, i32), GoaError> {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if state.refuse {
            return Err(GoaError::NotAuthorized(
                "Credentials not found in keyring".to_owned(),
            ));
        }
        state.given += 1;
        Ok((format!("tok-{}", state.given), 3600))
    }

    #[zbus(property)]
    fn client_id(&self) -> String {
        "fake".to_owned()
    }
}

/// A GNOME Online Accounts on a private bus, its accounts set by the test.
pub struct FakeGoa {
    runtime: tokio::runtime::Runtime,
    connection: zbus::Connection,
    state: Arc<Mutex<GoaState>>,
}

impl FakeGoa {
    pub fn start(bus: &str) -> anyhow::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        let connection = runtime.block_on(async {
            let connection = zbus::connection::Builder::address(bus)
                .serve_at(ROOT, zbus::fdo::ObjectManager)
                .name(GOA)
                .build()
                .await?;
            anyhow::Ok(connection)
        })?;
        Ok(Self {
            runtime,
            connection,
            state: Arc::default(),
        })
    }

    fn path(id: &str) -> String {
        format!("{ROOT}/Accounts/{id}")
    }

    /// An account `id` of `provider` ("google"), its calendar on.
    pub fn add(&self, id: &str, provider: &str, identity: &str) -> anyhow::Result<()> {
        let path = Self::path(id);
        let server = self.connection.object_server();
        self.runtime.block_on(async {
            server
                .at(
                    path.as_str(),
                    FakeAccount {
                        provider: provider.to_owned(),
                        identity: identity.to_owned(),
                        calendar_disabled: false,
                        attention_needed: false,
                    },
                )
                .await?;
            server
                .at(path.as_str(), FakeOAuth2(self.state.clone()))
                .await?;
            anyhow::Ok(())
        })
    }

    pub fn remove(&self, id: &str) -> anyhow::Result<()> {
        let path = Self::path(id);
        let server = self.connection.object_server();
        self.runtime.block_on(async {
            server.remove::<FakeOAuth2, _>(path.as_str()).await?;
            server.remove::<FakeAccount, _>(path.as_str()).await?;
            anyhow::Ok(())
        })
    }

    /// Its calendar switched on or off, as the user does in Settings.
    pub fn set_calendar(&self, id: &str, on: bool) -> anyhow::Result<()> {
        let path = Self::path(id);
        let server = self.connection.object_server();
        self.runtime.block_on(async {
            let account = server.interface::<_, FakeAccount>(path.as_str()).await?;
            account.get_mut().await.calendar_disabled = !on;
            account
                .get()
                .await
                .calendar_disabled_changed(account.signal_emitter())
                .await?;
            anyhow::Ok(())
        })
    }

    /// Whether every token is refused from now on.
    pub fn refuse(&self, refuse: bool) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .refuse = refuse;
    }

    /// How many tokens it gave.
    pub fn given(&self) -> u32 {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .given
    }
}

// --- The Calendar API ----------------------------------------------------------------

/// What the fake answers a sync from a token.
#[derive(Debug, Clone)]
pub enum Sync {
    /// These items changed, then `next`.
    Changes { items: Vec<Value>, next: String },
    /// `410 Gone`: list again.
    Expired,
}

/// A request the fake was sent.
#[derive(Debug, Clone)]
pub struct Asked {
    pub path: String,
    pub query: HashMap<String, String>,
    pub token: String,
}

#[derive(Default)]
pub struct Api {
    /// `calendarList`'s items.
    pub calendars: Vec<Value>,
    /// Each calendar's events, as a listing gives them, and its sync token.
    pub events: HashMap<String, (Vec<Value>, String)>,
    /// What a sync from each token gives.
    pub syncs: HashMap<String, Sync>,
    /// The bearer tokens taken; empty: any.
    pub accept: BTreeSet<String>,
    pub asked: Vec<Asked>,
}

/// A fake Calendar API on 127.0.0.1, pages of 2 items, until dropped.
pub struct FakeApi {
    pub address: String,
    pub api: Arc<Mutex<Api>>,
}

impl FakeApi {
    pub fn start() -> anyhow::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = format!("http://{}", listener.local_addr()?);
        let api: Arc<Mutex<Api>> = Arc::default();
        let served = api.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let api = served.clone();
                std::thread::spawn(move || serve(stream, &api));
            }
        });
        Ok(Self { address, api })
    }

    pub fn with<T>(&self, f: impl FnOnce(&mut Api) -> T) -> T {
        f(&mut self.api.lock().unwrap_or_else(PoisonError::into_inner))
    }

    pub fn asked(&self) -> Vec<Asked> {
        self.with(|api| api.asked.clone())
    }
}

fn serve(mut stream: TcpStream, api: &Mutex<Api>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let target = line.split_whitespace().nth(1).unwrap_or("").to_owned();
        let mut token = String::new();
        let mut length = 0usize;
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).unwrap_or(0) == 0 {
                return;
            }
            let header = header.trim_end();
            if header.is_empty() {
                break;
            }
            let (name, value) = header.split_once(':').unwrap_or((header, ""));
            match name.to_ascii_lowercase().as_str() {
                "authorization" => {
                    token = value.trim().trim_start_matches("Bearer ").to_owned();
                }
                "content-length" => length = value.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
        let mut body = vec![0; length];
        let _ = reader.read_exact(&mut body);
        let (status, answer) = answer(&target, token, api);
        let body = answer.to_string();
        let reply = format!(
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        if stream.write_all(reply.as_bytes()).is_err() {
            return;
        }
    }
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A page of `items` from `page` (its number, "0" first), 2 per page.
fn page(items: &[Value], page: Option<&String>, last: Value) -> Value {
    let at: usize = page.and_then(|p| p.parse().ok()).unwrap_or(0);
    let shown: Vec<Value> = items.iter().skip(at * 2).take(2).cloned().collect();
    let mut page =
        json!({ "items": shown, "defaultReminders": [{"method": "popup", "minutes": 10}] });
    if (at + 1) * 2 < items.len() {
        page["nextPageToken"] = json!((at + 1).to_string());
    } else if let (Some(page), Some(last)) = (page.as_object_mut(), last.as_object()) {
        page.extend(last.clone());
    }
    page
}

fn answer(target: &str, token: String, api: &Mutex<Api>) -> (u16, Value) {
    let mut api = api.lock().unwrap_or_else(PoisonError::into_inner);
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let query: HashMap<String, String> = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect();
    let path = decode(path);
    api.asked.push(Asked {
        path: path.clone(),
        query: query.clone(),
        token: token.clone(),
    });
    if !api.accept.is_empty() && !api.accept.contains(&token) {
        return (
            401,
            json!({"error": {"code": 401, "message": "Invalid Credentials"}}),
        );
    }
    let page_token = query.get("pageToken");
    if path == "/users/me/calendarList" {
        return (200, page(&api.calendars, page_token, json!({})));
    }
    let Some(calendar) = path
        .strip_prefix("/calendars/")
        .and_then(|p| p.strip_suffix("/events"))
    else {
        return (404, json!({"error": {"code": 404}}));
    };
    if let Some(sync) = query.get("syncToken") {
        return match api.syncs.get(sync) {
            Some(Sync::Changes { items, next }) => (
                200,
                page(items, page_token, json!({ "nextSyncToken": next })),
            ),
            Some(Sync::Expired) | None => (410, json!({"error": {"code": 410}})),
        };
    }
    match api.events.get(calendar) {
        Some((items, next)) => (
            200,
            page(items, page_token, json!({ "nextSyncToken": next })),
        ),
        None => (404, json!({"error": {"code": 404}})),
    }
}

/// A timed event of 4 November 2026, `from` to `to` (`HH:MM`, UTC).
pub fn event(id: &str, title: &str, from: &str, to: &str) -> Value {
    json!({
        "id": id,
        "status": "confirmed",
        "summary": title,
        "htmlLink": format!("https://calendar.google.com/event?eid={id}"),
        "start": { "dateTime": format!("2026-11-04T{from}:00Z") },
        "end": { "dateTime": format!("2026-11-04T{to}:00Z") },
        "reminders": { "useDefault": true },
    })
}

/// A calendar of `calendarList`.
pub fn calendar(id: &str, summary: &str, colour: &str, role: &str, selected: bool) -> Value {
    json!({
        "id": id,
        "summary": summary,
        "backgroundColor": colour,
        "accessRole": role,
        "selected": selected,
        "defaultReminders": [{"method": "popup", "minutes": 10}],
    })
}

// --- Planka ----------------------------------------------------------------------------

/// A fake Planka board's state, set by the test.
pub struct Planka {
    /// The token it takes.
    pub token: String,
    /// The boards the token's user is shared: their ids.
    pub shared: Vec<String>,
    /// Its lists: id, name, position.
    pub lists: Vec<(String, String, f64)>,
    /// Its cards: id, list, name, position.
    pub cards: Vec<(String, String, String, f64)>,
    /// What it was asked: `METHOD path`.
    pub asked: Vec<String>,
}

/// A fake Planka server on 127.0.0.1, with Planka's answers' shapes: one
/// board, "1", named Sprint, its lists Backlog, TODO and Done; until
/// dropped.
pub struct FakePlanka {
    pub address: String,
    pub state: Arc<Mutex<Planka>>,
}

impl FakePlanka {
    pub fn start(token: &str) -> anyhow::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = format!("http://{}", listener.local_addr()?);
        let list = |id: &str, name: &str, position: f64| (id.to_owned(), name.to_owned(), position);
        let card = |id: &str, list: &str, name: &str, position: f64| {
            (id.to_owned(), list.to_owned(), name.to_owned(), position)
        };
        let state = Arc::new(Mutex::new(Planka {
            token: token.to_owned(),
            shared: vec!["1".to_owned()],
            lists: vec![
                list("10", "Backlog", 65536.0),
                list("11", "TODO", 131072.0),
                list("12", "Done", 196608.0),
            ],
            cards: vec![
                card("c1", "11", "Relire la spec", 65536.0),
                card("c2", "11", "Écrire le ticket", 131072.0),
                card("c3", "10", "Un jour", 65536.0),
                card("c4", "12", "Fait", 65536.0),
            ],
            asked: Vec::new(),
        }));
        let served = state.clone();
        std::thread::Builder::new()
            .name("fake planka".to_owned())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let state = served.clone();
                    std::thread::spawn(move || planka_serve(stream, &state));
                }
            })?;
        Ok(Self { address, state })
    }

    pub fn with<T>(&self, f: impl FnOnce(&mut Planka) -> T) -> T {
        f(&mut self.state.lock().unwrap_or_else(PoisonError::into_inner))
    }

    pub fn asked(&self) -> Vec<String> {
        self.with(|p| p.asked.clone())
    }
}

/// An address where nothing answers: a port bound, then let go.
pub fn nothing_there() -> anyhow::Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = format!("http://{}", listener.local_addr()?);
    drop(listener);
    Ok(address)
}

fn planka_serve(mut stream: TcpStream, state: &Mutex<Planka>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let Ok(reader) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut words = line.split_whitespace();
    let (method, path) = (
        words.next().unwrap_or("").to_owned(),
        words.next().unwrap_or("").to_owned(),
    );
    let (mut bearer, mut length) = (String::new(), 0usize);
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "authorization" => {
                    bearer = value.trim().trim_start_matches("Bearer ").to_owned();
                }
                "content-length" => length = value.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
    }
    let mut body = vec![0; length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    let (status, answer) = planka_answer(state, &method, &path, &bearer);
    let text = answer.to_string();
    let _ = write!(
        stream,
        "HTTP/1.1 {status} X\r\nContent-Type: application/json; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    );
}

fn planka_answer(state: &Mutex<Planka>, method: &str, path: &str, bearer: &str) -> (u16, Value) {
    let mut planka = state.lock().unwrap_or_else(PoisonError::into_inner);
    planka.asked.push(format!("{method} {path}"));
    let not_found = || (404, json!({"code": "E_NOT_FOUND"}));
    if method == "GET" && path == "/api/bootstrap" {
        return (200, json!({"item": {"oidc": null, "version": "2.0.0"}}));
    }
    if bearer != planka.token {
        return (401, json!({"code": "E_UNAUTHORIZED"}));
    }
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match (method, segments.as_slice()) {
        ("GET", ["api", "users", "me"]) => (
            200,
            json!({"item": {"id": "100", "name": "Alex", "username": "alex"}}),
        ),
        ("GET", ["api", "projects"]) => {
            let boards: Vec<Value> = planka
                .shared
                .iter()
                .map(|id| json!({"id": id, "projectId": "2", "position": 65536, "name": "Sprint"}))
                .collect();
            (
                200,
                json!({"items": [{"id": "2", "name": "Équipe"}], "included": {"boards": boards}}),
            )
        }
        ("GET", ["api", "boards", id]) if planka.shared.iter().any(|s| s == id) => {
            let lists: Vec<Value> = planka
                .lists
                .iter()
                .map(|(id, name, position)| {
                    json!({"id": id, "boardId": "1", "type": "active", "position": position, "name": name})
                })
                .collect();
            let cards: Vec<Value> = planka
                .cards
                .iter()
                .map(|(id, list, name, position)| {
                    json!({"id": id, "boardId": "1", "listId": list, "type": "project",
                           "position": position, "name": name, "dueDate": null})
                })
                .collect();
            (
                200,
                json!({
                    "item": {"id": id, "projectId": "2", "name": "Sprint", "defaultCardType": "project"},
                    "included": {"lists": lists, "cards": cards},
                }),
            )
        }
        _ => not_found(),
    }
}
