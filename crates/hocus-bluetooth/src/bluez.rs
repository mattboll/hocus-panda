//! BlueZ on the system bus, on a thread of its own: what it says, followed
//! by its signals, and what the bar or the Settings ask of it.
//!
//! The thread keeps BlueZ's objects as last said: the adapters (`Powered`,
//! `PowerState`, `Alias`, `Discoverable`), the devices (their name, what
//! they are, `Paired`, `Trusted`, `Connected`, `RSSI`) and their batteries
//! (`Battery1`'s `Percentage`). It reads them all with `GetManagedObjects`
//! when BlueZ has an owner on the bus, then follows them:
//! `InterfacesAdded` and `…Removed`, `PropertiesChanged` under
//! `/org/bluez`, and BlueZ's owner, for when it stops or starts. Nothing is
//! polled. After each burst of signals, what is shown is made again
//! ([`snapshot`]) and published: a signal strength that changes nothing
//! in the order shown publishes nothing.
//!
//! What it asks: the adapter on or off (`Powered`); a search
//! (`StartDiscovery`) while it is wanted, stopped when it is not
//! (`StopDiscovery`) and as the program ends; the computer visible to
//! others (`Discoverable` and `Pairable`) while it is wanted, made visible
//! again when BlueZ's timeout hides it, hidden when it is not wanted; a
//! paired device connected (`Connect`, 20 s at most, then given up with
//! `Disconnect`) or disconnected; a new one paired (`Pair`, 30 s at most,
//! then `CancelPairing`), trusted (`Trusted`), then connected. Calls that
//! take long run aside, and say how they ended to the thread.
//!
//! Pairing goes through the program's agent, registered with BlueZ with
//! the capability [`Options`] gives (BlueZ uses the agent of whoever asked
//! for the pairing), so that a device that pairs without a code ("Just
//! Works") does. One that asks for a code (a PIN, a passkey to type or to
//! compare) is refused by the agent, and its pairing said to have asked
//! one. A phone, a tablet or a computer is taken to ask for one before it
//! is tried.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use zbus::{
    Connection, MatchRule, Message, MessageStream, ObjectPath, OwnedObjectPath, OwnedVariant,
    Variant, fdo::DBusProxy,
};

use crate::{
    Ask, Options,
    model::{Action, Answer, Asked, Attempt, Bluetooth, Device, Failure, Kind, Request, Step},
};

const BLUEZ: &str = "org.bluez";
const ROOT: &str = "/org/bluez";
const ADAPTER: &str = "org.bluez.Adapter1";
const DEVICE: &str = "org.bluez.Device1";
const BATTERY: &str = "org.bluez.Battery1";
const AGENT_MANAGER: &str = "org.bluez.AgentManager1";
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
/// Where the shell's pairing agent answers.
const AGENT: &str = "/org/hocuspanda/bluetooth/agent";

/// A connection BlueZ has not made by then is given up: a device off or
/// out of range fails sooner (BlueZ pages it about 5 s), one that answers
/// slowly (its profiles) has the time.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// A pairing not done by then is cancelled.
const PAIR_TIMEOUT: Duration = Duration::from_secs(30);
/// "Recherche…" shows this long after the search starts: the search goes
/// on while the popover is open, and what it finds then still comes, but
/// a spinner turning all along would draw at every frame.
const SEARCH: Duration = Duration::from_secs(10);

type Props = HashMap<String, OwnedVariant>;
type Objects = HashMap<OwnedObjectPath, HashMap<String, Props>>;

/// What the thread says each time what it shows changes.
type Publish = Box<dyn FnMut(Bluetooth) + Send>;

/// A service that is away is tried again after this, doubled each time it
/// is still away, up to [`RETRY_LAST`].
const RETRY_FIRST: Duration = Duration::from_secs(2);
const RETRY_LAST: Duration = Duration::from_secs(60);

/// The next wait before trying a missing service again.
fn next_retry(wait: Duration) -> Duration {
    (wait * 2).min(RETRY_LAST)
}

/// Whether BlueZ is there, to say once when it goes and comes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Presence {
    Unknown,
    There,
    Away,
}

impl Presence {
    /// Logs what changed: `what` is there, or away (`why`).
    fn now(&mut self, there: bool, what: &str, why: impl std::fmt::Display) {
        let next = if there { Self::There } else { Self::Away };
        match (*self, next) {
            (Self::Away, Self::There) => tracing::info!("{what}: back"),
            (Self::Unknown | Self::There, Self::Away) => {
                tracing::info!("{what}: not there ({why}); hidden until it comes");
            }
            _ => {}
        }
        *self = next;
    }
}

pub(crate) fn start(asks: UnboundedReceiver<Ask>, options: Options, publish: Publish) {
    let started = std::thread::Builder::new()
        .name("bluetooth".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(run(asks, options, publish)),
                Err(err) => tracing::warn!(%err, "no runtime for BlueZ"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot follow BlueZ");
    }
}

/// How long a pairing takes at most once a code shows (from when it
/// started): the time to read it and type it.
const CODE_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a connection, a pairing and a search's first pass take at
/// most: `HOCUS_BLUETOOTH_TIMEOUT` (seconds) for all three, for tests;
/// a pairing showing a code, `HOCUS_BLUETOOTH_CODE_TIMEOUT`.
#[derive(Debug, Clone, Copy)]
struct Timeouts {
    connect: Duration,
    pair: Duration,
    search: Duration,
    code: Duration,
}

fn timeouts() -> Timeouts {
    let seconds = |name: &str| {
        std::env::var(name)
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|s| *s > 0.0)
            .map(Duration::from_secs_f64)
    };
    let code = seconds("HOCUS_BLUETOOTH_CODE_TIMEOUT").unwrap_or(CODE_TIMEOUT);
    match seconds("HOCUS_BLUETOOTH_TIMEOUT") {
        Some(t) => Timeouts {
            connect: t,
            pair: t,
            search: t,
            code,
        },
        None => Timeouts {
            connect: CONNECT_TIMEOUT,
            pair: PAIR_TIMEOUT,
            search: SEARCH,
            code,
        },
    }
}

/// The agent's questions waiting for the user's answer, by id.
type Waiting = Arc<Mutex<HashMap<u64, tokio::sync::oneshot::Sender<Answer>>>>;

/// What the thread acts on, from wherever it comes.
enum Input {
    Signal(Message),
    /// BlueZ has an owner on the bus now, or not.
    Owner(bool),
    Ask(Ask),
    /// Attempt `id`'s pairing is done: trusted and connected next.
    Paired(u64),
    /// Attempt `id` ended.
    Ended(u64, Option<Failure>),
    /// Search `n`'s first pass is over.
    SearchOver(u64),
    /// The agent was asked a code for this device, and refused.
    CodeAsked(String),
    /// The agent asks the user `asked` for device `path`, and waits.
    Request {
        id: u64,
        path: String,
        asked: Asked,
    },
    /// The agent shows the user `asked` for device `path`.
    Shown {
        path: String,
        asked: Asked,
    },
    /// Question `id` was answered, or given up.
    Answered(u64),
    /// BlueZ cancelled what it asked (the device gave up, a time out).
    Cancelled,
    /// Question `id`'s time is over: refused.
    RequestOver(u64),
}

async fn run(mut asks: UnboundedReceiver<Ask>, options: Options, mut publish: Publish) {
    let mut wait = RETRY_FIRST;
    let mut presence = Presence::Unknown;
    let connection = loop {
        match Connection::system().await {
            Ok(connection) => break connection,
            Err(err) => {
                // Said once: tries that fail again change nothing shown,
                // and must not wake whoever shows it.
                if presence != Presence::Away {
                    publish(Bluetooth::default());
                }
                presence.now(false, "Bluetooth (the system bus)", &err);
                while let Ok(ask) = asks.try_recv() {
                    if let Ask::Quit(done) = ask {
                        let _ = done.send(());
                        return;
                    }
                }
                tokio::time::sleep(wait).await;
                wait = next_retry(wait);
            }
        }
    };
    let (inputs, mut incoming) = unbounded_channel();
    if let Err(err) = listen(&connection, inputs.clone()).await {
        presence.now(false, "Bluetooth", &err);
        publish(Bluetooth::default());
        return;
    }
    let pairing = Arc::new(Mutex::new(None));
    let waiting = Waiting::default();
    let ids = Arc::new(AtomicU64::new(0));
    let agent = Agent {
        inputs: inputs.clone(),
        pairing: pairing.clone(),
        asks_codes: options.asks_codes,
        waiting: waiting.clone(),
        accepted: Arc::default(),
        ids: ids.clone(),
    };
    if let Err(err) = connection.object_server().at(AGENT, agent).await {
        tracing::warn!(%err, "no pairing agent: pairing will fail");
    }
    tokio::spawn({
        let inputs = inputs.clone();
        async move {
            while let Some(ask) = asks.recv().await {
                if inputs.send(Input::Ask(ask)).is_err() {
                    return;
                }
            }
        }
    });
    let mut client = Client {
        connection,
        cache: Cache::default(),
        presence,
        inputs,
        stamps: HashMap::new(),
        stamp: 0,
        codes: HashSet::new(),
        options,
        publish,
        wanted: false,
        visible: false,
        shown: false,
        discovering: None,
        search: None,
        searches: 0,
        attempt: None,
        attempts: 0,
        pairing,
        timeouts: timeouts(),
        request: None,
        waiting,
        ids,
        deadline: Arc::new(Mutex::new(Instant::now())),
        started: None,
        stopped: false,
        cancelled: false,
        registered: false,
    };
    let owned = match DBusProxy::new(&client.connection).await {
        Ok(bus) => bus
            .name_has_owner(BLUEZ.try_into().unwrap_or_else(|_| unreachable!()))
            .await
            .unwrap_or(false),
        Err(_) => false,
    };
    client.owner(owned).await;
    client.publish();
    while let Some(input) = incoming.recv().await {
        if !client.input(input).await {
            return;
        }
        // A burst (a search's strengths) is shown once.
        while let Ok(input) = incoming.try_recv() {
            if !client.input(input).await {
                return;
            }
        }
        client.publish();
    }
}

/// BlueZ's signals and its owner's changes, into `inputs`.
async fn listen(connection: &Connection, inputs: UnboundedSender<Input>) -> zbus::Result<()> {
    // BlueZ's object manager is at `/`: its objects' paths say they are
    // BlueZ's.
    let objects = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(OBJECT_MANAGER)
        .path("/")
        .build()?;
    let properties = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(PROPERTIES)
        .member("PropertiesChanged")
        .path_namespace(ROOT)
        .build()?;
    for rule in [objects, properties] {
        let mut signals = MessageStream::for_match_rule(rule, connection, None).await?;
        let to = inputs.clone();
        tokio::spawn(async move {
            while let Some(Ok(message)) = signals.next().await {
                if to.send(Input::Signal(message)).is_err() {
                    return;
                }
            }
        });
    }
    let bus = DBusProxy::new(connection).await?;
    let mut owners = bus
        .receive_name_owner_changed_with_args(&[(0, BLUEZ)])
        .await?;
    tokio::spawn(async move {
        while let Some(changed) = owners.next().await {
            let owned = changed
                .args()
                .map_or(true, |args| args.new_owner().is_some());
            if inputs.send(Input::Owner(owned)).is_err() {
                return;
            }
        }
    });
    Ok(())
}

/// BlueZ's objects as last said.
#[derive(Debug, Default)]
struct Cache {
    /// The adapters' properties, by path (the first is used).
    adapters: BTreeMap<String, Props>,
    devices: HashMap<String, Props>,
    batteries: HashMap<String, u8>,
}

struct Client {
    connection: Connection,
    cache: Cache,
    presence: Presence,
    inputs: UnboundedSender<Input>,
    /// When each device connected, as seen: the last one first.
    stamps: HashMap<String, u64>,
    stamp: u64,
    /// Devices whose pairing asked for a code.
    codes: HashSet<String>,
    options: Options,
    publish: Publish,
    /// A search is wanted while the adapter is on (the bar's popover is
    /// open, the Settings' page shows).
    wanted: bool,
    /// The computer is wanted visible to others.
    visible: bool,
    /// It was made visible here: hidden here too.
    shown: bool,
    /// The adapter the search was started on.
    discovering: Option<String>,
    /// The search whose first pass runs.
    search: Option<u64>,
    searches: u64,
    attempt: Option<Attempt>,
    attempts: u64,
    /// The device being paired, which the agent lets through.
    pairing: Arc<Mutex<Option<String>>>,
    timeouts: Timeouts,
    /// What the agent asks or shows the user now.
    request: Option<Request>,
    waiting: Waiting,
    /// The questions' ids, the agent's and the shown codes'.
    ids: Arc<AtomicU64>,
    /// When the pairing under way is given up, and when it started.
    deadline: Arc<Mutex<Instant>>,
    started: Option<Instant>,
    /// The pairing under way was cancelled by the user, or by the device.
    stopped: bool,
    cancelled: bool,
    /// The agent is registered with BlueZ.
    registered: bool,
}

impl Client {
    /// Acts on `input`; `false` once the shell ends.
    async fn input(&mut self, input: Input) -> bool {
        match input {
            Input::Signal(message) => self.signal(&message).await,
            Input::Owner(owned) => self.owner(owned).await,
            Input::Ask(Ask::Quit(done)) => {
                self.wanted = false;
                self.discover().await;
                self.visible = false;
                self.show().await;
                self.agent().await;
                let _ = done.send(());
                return false;
            }
            Input::Ask(ask) => self.ask(ask).await,
            Input::Paired(id) => self.paired(id).await,
            Input::Ended(id, failure) => self.ended(id, failure),
            Input::SearchOver(n) => {
                if self.search == Some(n) {
                    self.search = None;
                }
            }
            Input::CodeAsked(device) => {
                tracing::info!(device, "the device asks for a code: left to the Settings");
                self.codes.insert(device);
            }
            Input::Request { id, path, asked } => self.asked(id, path, asked),
            Input::Shown { path, asked } => {
                let id = match &self.request {
                    Some(r) if r.path == path && matches!(r.asked, Asked::Display { .. }) => r.id,
                    _ => self.ids.fetch_add(1, Ordering::Relaxed) + 1,
                };
                self.asked(id, path, asked);
            }
            Input::Answered(id) => {
                if self.request.as_ref().is_some_and(|r| r.id == id) {
                    self.request = None;
                }
            }
            Input::Cancelled => {
                tracing::info!("BlueZ cancelled what the agent asked");
                self.request = None;
                if self.running_pair().is_some() && !self.stopped {
                    self.cancelled = true;
                }
            }
            Input::RequestOver(id) => self.answer(id, Answer::No),
        }
        true
    }

    /// The device paired here now, if one is.
    fn running_pair(&self) -> Option<&str> {
        self.attempt
            .as_ref()
            .filter(|a| a.step == Step::Running(Action::Pair))
            .map(|a| a.path.as_str())
    }

    /// The agent asks the user `asked` for `path`: shown until answered.
    /// A pairing asked here then has more time; one the device asked
    /// for is refused once its time is over.
    fn asked(&mut self, id: u64, path: String, asked: Asked) {
        let ours = self.running_pair() == Some(path.as_str());
        let until = if ours {
            let until = self.started.unwrap_or_else(Instant::now) + self.timeouts.code;
            let mut deadline = self.deadline.lock().unwrap_or_else(PoisonError::into_inner);
            *deadline = (*deadline).max(until);
            *deadline
        } else {
            let until = Instant::now() + self.timeouts.code;
            let inputs = self.inputs.clone();
            let wait = self.timeouts.code;
            tokio::spawn(async move {
                tokio::time::sleep(wait).await;
                let _ = inputs.send(Input::RequestOver(id));
            });
            until
        };
        let (name, kind) = self
            .cache
            .devices
            .get(&path)
            .map(|p| {
                let name = get::<String>(p, "Alias")
                    .or_else(|| get::<String>(p, "Name"))
                    .unwrap_or_default();
                (name, kind(p))
            })
            .unwrap_or_default();
        tracing::info!(?asked, ours, "the agent asks the user");
        self.request = Some(Request {
            id,
            path,
            name,
            kind,
            asked,
            until: Some(until),
            ours,
        });
    }

    /// The user answers question `id` (a code shown has none waiting).
    fn answer(&mut self, id: u64, answer: Answer) {
        let shown = self.request.as_ref().filter(|r| r.id == id).cloned();
        if answer == Answer::No
            && shown
                .as_ref()
                .is_some_and(|r| self.running_pair() == Some(r.path.as_str()))
        {
            self.stopped = true;
        }
        if shown.is_some() {
            self.request = None;
        }
        let waiting = self
            .waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
        if let Some(waiting) = waiting {
            let _ = waiting.send(answer);
        }
    }

    fn publish(&mut self) {
        let bluetooth = snapshot(
            &self.cache,
            &self.stamps,
            &self.codes,
            self.search.is_some(),
            self.attempt.clone(),
        );
        let bluetooth = Bluetooth {
            request: self.request.clone(),
            ..bluetooth
        };
        (self.publish)(bluetooth);
    }

    /// BlueZ came (read it all) or went (forget it).
    async fn owner(&mut self, owned: bool) {
        self.cache = Cache::default();
        self.discovering = None;
        self.search = None;
        self.registered = false;
        if !owned {
            self.presence
                .now(false, "Bluetooth", "BlueZ is not running");
            return;
        }
        if let Err(err) = self.load().await {
            self.cache = Cache::default();
            self.presence.now(false, "Bluetooth", &err);
            return;
        }
        // Those connected already, in the order of their paths.
        let mut connected: Vec<String> = self
            .cache
            .devices
            .iter()
            .filter(|(_, d)| get::<bool>(d, "Connected") == Some(true))
            .map(|(p, _)| p.clone())
            .collect();
        connected.sort();
        for path in connected {
            self.connected(&path);
        }
        if self.adapter().is_some() {
            self.presence.now(true, "Bluetooth", "");
        } else {
            self.presence.now(false, "Bluetooth", "no adapter");
        }
        self.agent().await;
        self.discover().await;
        self.show().await;
    }

    /// The agent registered as wanted: always for the bar's; while the
    /// computer is visible for one that asks the user codes, then the
    /// default one, so that a device pairing by itself reaches it.
    async fn agent(&mut self) {
        let want = !self.options.asks_codes || self.visible;
        let agent = ObjectPath::from_static_str_unchecked(AGENT);
        if want && !self.registered {
            let registered = self
                .call(
                    ROOT,
                    AGENT_MANAGER,
                    "RegisterAgent",
                    &(&agent, self.options.capability),
                )
                .await;
            match registered {
                Ok(_) => self.registered = true,
                Err(err) => tracing::warn!(%err, "BlueZ did not take the pairing agent"),
            }
            if self.registered && self.options.asks_codes {
                let default = self
                    .call(ROOT, AGENT_MANAGER, "RequestDefaultAgent", &(&agent,))
                    .await;
                if let Err(err) = default {
                    tracing::warn!(%err, "BlueZ did not make the agent its default");
                }
            }
        } else if !want && self.registered {
            self.registered = false;
            let gone = self
                .call(ROOT, AGENT_MANAGER, "UnregisterAgent", &(&agent,))
                .await;
            if let Err(err) = gone {
                tracing::debug!(%err, "the agent was not registered");
            }
        }
    }

    async fn load(&mut self) -> zbus::Result<()> {
        let reply = self
            .call("/", OBJECT_MANAGER, "GetManagedObjects", &())
            .await?;
        let objects: Objects = reply.body().deserialize()?;
        for (path, interfaces) in objects {
            self.added(path.as_str(), interfaces);
        }
        Ok(())
    }

    async fn call<B>(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        body: &B,
    ) -> zbus::Result<Message>
    where
        B: serde::Serialize + zbus::wire::DynamicType,
    {
        self.connection
            .call_method(Some(BLUEZ), path, Some(interface), method, body)
            .await
    }

    /// The adapter used: the first, by path.
    fn adapter(&self) -> Option<(&str, &Props)> {
        self.cache
            .adapters
            .iter()
            .next()
            .map(|(p, a)| (p.as_str(), a))
    }

    fn powered(&self) -> bool {
        self.adapter()
            .is_some_and(|(_, a)| get::<bool>(a, "Powered") == Some(true))
    }

    /// Device `path` is connected now: the last one.
    fn connected(&mut self, path: &str) {
        self.stamp += 1;
        self.stamps.insert(path.to_owned(), self.stamp);
    }

    fn added(&mut self, path: &str, interfaces: HashMap<String, Props>) {
        if !path.starts_with(ROOT) {
            return;
        }
        for (interface, props) in interfaces {
            match interface.as_str() {
                ADAPTER => {
                    self.cache.adapters.insert(path.to_owned(), props);
                }
                DEVICE => {
                    if get::<bool>(&props, "Connected") == Some(true)
                        && !self.cache.devices.contains_key(path)
                    {
                        self.connected(path);
                    }
                    self.cache.devices.insert(path.to_owned(), props);
                }
                BATTERY => {
                    if let Some(percent) = get::<u8>(&props, "Percentage") {
                        self.cache.batteries.insert(path.to_owned(), percent);
                    }
                }
                _ => {}
            }
        }
    }

    /// A signal from BlueZ's objects.
    async fn signal(&mut self, message: &Message) {
        let header = message.header();
        let (Some(at), Some(interface), Some(member)) =
            (header.path(), header.interface(), header.member())
        else {
            return;
        };
        let (at, interface, member) = (
            at.as_str().to_owned(),
            interface.as_str().to_owned(),
            member.as_str().to_owned(),
        );
        let body = message.body();
        let powered = self.powered();
        let adapter = self.adapter().map(|(p, _)| p.to_owned());
        match (interface.as_str(), member.as_str()) {
            (OBJECT_MANAGER, "InterfacesAdded") => {
                if let Ok((path, interfaces)) =
                    body.deserialize::<(OwnedObjectPath, HashMap<String, Props>)>()
                {
                    self.added(path.as_str(), interfaces);
                }
            }
            (OBJECT_MANAGER, "InterfacesRemoved") => {
                if let Ok((path, interfaces)) = body.deserialize::<(OwnedObjectPath, Vec<String>)>()
                {
                    self.removed(path.as_str(), &interfaces);
                }
            }
            (PROPERTIES, "PropertiesChanged") => {
                if let Ok((iface, changed, gone)) =
                    body.deserialize::<(String, Props, Vec<String>)>()
                {
                    self.changed(&at, &iface, changed, &gone);
                }
            }
            _ => {}
        }
        let now = self.adapter().map(|(p, _)| p.to_owned());
        if now != adapter {
            match &now {
                Some(_) => self.presence.now(true, "Bluetooth", ""),
                None => self.presence.now(false, "Bluetooth", "no adapter"),
            }
            if now.is_none() || self.discovering != now {
                self.discovering = None;
                self.search = None;
            }
        }
        if now != adapter || self.powered() != powered {
            if !self.powered() {
                // BlueZ ends a search on an adapter going off.
                self.discovering = None;
                self.search = None;
            }
            self.discover().await;
        }
        // Visible again once on, or when BlueZ's timeout hid it.
        self.show().await;
    }

    fn removed(&mut self, path: &str, interfaces: &[String]) {
        for interface in interfaces {
            match interface.as_str() {
                ADAPTER => {
                    self.cache.adapters.remove(path);
                }
                DEVICE => {
                    self.cache.devices.remove(path);
                    self.stamps.remove(path);
                }
                BATTERY => {
                    self.cache.batteries.remove(path);
                }
                _ => {}
            }
        }
    }

    fn changed(&mut self, path: &str, interface: &str, changed: Props, gone: &[String]) {
        match interface {
            ADAPTER => {
                if let Some(adapter) = self.cache.adapters.get_mut(path) {
                    adapter.extend(changed);
                    for name in gone {
                        adapter.remove(name);
                    }
                }
            }
            DEVICE => {
                let Some(device) = self.cache.devices.get_mut(path) else {
                    return;
                };
                let was = get::<bool>(device, "Connected") == Some(true);
                device.extend(changed);
                for name in gone {
                    device.remove(name);
                }
                if !was && get::<bool>(device, "Connected") == Some(true) {
                    self.connected(path);
                }
            }
            BATTERY => match get::<u8>(&changed, "Percentage") {
                Some(percent) => {
                    self.cache.batteries.insert(path.to_owned(), percent);
                }
                None if gone.iter().any(|g| g == "Percentage") => {
                    self.cache.batteries.remove(path);
                }
                None => {}
            },
            _ => {}
        }
    }

    /// The search as wanted: on while the popover is open and the adapter
    /// is on, else off.
    async fn discover(&mut self) {
        let adapter = self.adapter().map(|(p, _)| p.to_owned());
        let want = self.wanted && self.powered();
        match (want, adapter, self.discovering.clone()) {
            (true, Some(adapter), None) => {
                match self.call(&adapter, ADAPTER, "StartDiscovery", &()).await {
                    Ok(_) => {
                        tracing::debug!("the search starts");
                        self.discovering = Some(adapter);
                        self.searches += 1;
                        let n = self.searches;
                        self.search = Some(n);
                        let (inputs, search) = (self.inputs.clone(), self.timeouts.search);
                        tokio::spawn(async move {
                            tokio::time::sleep(search).await;
                            let _ = inputs.send(Input::SearchOver(n));
                        });
                    }
                    Err(err) => tracing::warn!(%err, "BlueZ did not start a search"),
                }
            }
            (false, _, Some(adapter)) => {
                self.discovering = None;
                self.search = None;
                match self.call(&adapter, ADAPTER, "StopDiscovery", &()).await {
                    Ok(_) => tracing::debug!("the search stops"),
                    // Already over: the adapter went off, or away.
                    Err(err) => tracing::debug!(%err, "no search to stop"),
                }
            }
            _ => {}
        }
    }

    /// The computer visible to others as wanted: while it is wanted and
    /// the adapter is on, `Pairable` and `Discoverable` set whenever BlueZ
    /// says they are not (its timeout ends `Discoverable`); once it is no
    /// longer wanted, hidden if it was made visible here.
    async fn show(&mut self) {
        let Some((adapter, discoverable, pairable)) = self.adapter().map(|(p, a)| {
            (
                p.to_owned(),
                get::<bool>(a, "Discoverable") == Some(true),
                get::<bool>(a, "Pairable") == Some(true),
            )
        }) else {
            self.shown = false;
            return;
        };
        let powered = self.powered();
        if self.visible && powered {
            if !pairable && self.set_adapter(&adapter, "Pairable", true).await {
                tracing::debug!("the computer can be paired with");
            }
            if !discoverable && self.set_adapter(&adapter, "Discoverable", true).await {
                tracing::info!("the computer is visible");
                self.shown = true;
            }
        } else if !self.visible && self.shown {
            self.shown = false;
            if discoverable && powered && self.set_adapter(&adapter, "Discoverable", false).await {
                tracing::info!("the computer is hidden");
            }
        }
    }

    /// The adapter's `name` set to `on`, kept as set until BlueZ says it:
    /// whether BlueZ took it.
    async fn set_adapter(&mut self, adapter: &str, name: &str, on: bool) -> bool {
        let set = self
            .call(
                adapter,
                PROPERTIES,
                "Set",
                &(ADAPTER, name, Variant::from(on)),
            )
            .await;
        match set {
            Ok(_) => {
                if let Some(props) = self.cache.adapters.get_mut(adapter)
                    && let Ok(value) = OwnedVariant::try_from(Variant::from(on))
                {
                    props.insert(name.to_owned(), value);
                }
                true
            }
            Err(err) => {
                tracing::warn!(%err, name, on, "BlueZ did not set the adapter");
                false
            }
        }
    }

    async fn ask(&mut self, ask: Ask) {
        match ask {
            Ask::Power(on) => {
                let Some(adapter) = self.adapter().map(|(p, _)| p.to_owned()) else {
                    return;
                };
                let set = self
                    .call(
                        &adapter,
                        PROPERTIES,
                        "Set",
                        &(ADAPTER, "Powered", Variant::from(on)),
                    )
                    .await;
                // Blocked by rfkill, it stays off.
                if let Err(err) = set {
                    tracing::warn!(%err, on, "BlueZ did not switch the adapter");
                }
            }
            Ask::Discover(on) => {
                self.wanted = on;
                self.discover().await;
            }
            Ask::Visible(on) => {
                self.visible = on;
                self.show().await;
                self.agent().await;
            }
            Ask::Answer(id, answer) => self.answer(id, answer),
            Ask::CancelPairing => {
                if let Some(id) = self.request.as_ref().map(|r| r.id) {
                    self.answer(id, Answer::No);
                }
                if let Some(path) = self.running_pair().map(str::to_owned) {
                    self.stopped = true;
                    self.request = None;
                    let connection = self.connection.clone();
                    tokio::spawn(async move {
                        let done = connection
                            .call_method(
                                Some(BLUEZ),
                                path.as_str(),
                                Some(DEVICE),
                                "CancelPairing",
                                &(),
                            )
                            .await;
                        if let Err(err) = done {
                            tracing::debug!(%err, "no pairing to cancel");
                        }
                    });
                }
            }
            Ask::Rename(path, alias) => self.set_device(path, "Alias", Variant::from(alias)),
            Ask::Trust(path, on) => self.set_device(path, "Trusted", Variant::from(on)),
            Ask::Forget(path) => {
                let Some(adapter) = self.adapter().map(|(p, _)| p.to_owned()) else {
                    return;
                };
                let connection = self.connection.clone();
                tokio::spawn(async move {
                    let Ok(device) = ObjectPath::try_from(path.as_str()) else {
                        return;
                    };
                    let done = connection
                        .call_method(
                            Some(BLUEZ),
                            adapter.as_str(),
                            Some(ADAPTER),
                            "RemoveDevice",
                            &(device,),
                        )
                        .await;
                    match done {
                        Ok(_) => tracing::info!("a device is forgotten"),
                        Err(err) => tracing::warn!(%err, "BlueZ did not forget the device"),
                    }
                });
            }
            Ask::Connect(path) => {
                if self.start(&path, Action::Connect) {
                    self.connect(path);
                }
            }
            Ask::Disconnect(path) => {
                let connection = self.connection.clone();
                tokio::spawn(async move {
                    let done = connection
                        .call_method(Some(BLUEZ), path.as_str(), Some(DEVICE), "Disconnect", &())
                        .await;
                    if let Err(err) = done {
                        tracing::warn!(%err, "BlueZ did not disconnect the device");
                    }
                });
            }
            Ask::Pair(path) => {
                if self.start(&path, Action::Pair) {
                    self.pair(path);
                }
            }
            Ask::Quit(_) => {}
        }
    }

    /// Device `path`'s property `name` set to `value`, aside: an empty
    /// `Alias` gives it back its own name (BlueZ's).
    fn set_device(&self, path: String, name: &'static str, value: Variant<'static>) {
        let connection = self.connection.clone();
        tokio::spawn(async move {
            let done = connection
                .call_method(
                    Some(BLUEZ),
                    path.as_str(),
                    Some(PROPERTIES),
                    "Set",
                    &(DEVICE, name, value),
                )
                .await;
            match done {
                Ok(_) => tracing::info!(name, "a device's property is set"),
                Err(err) => tracing::warn!(%err, name, "BlueZ did not set the device's property"),
            }
        });
    }

    /// A new attempt for `path`, unless one runs: whether it starts.
    fn start(&mut self, path: &str, action: Action) -> bool {
        let running = self
            .attempt
            .as_ref()
            .is_some_and(|a| matches!(a.step, Step::Running(_)));
        if running || !self.cache.devices.contains_key(path) {
            return false;
        }
        self.attempts += 1;
        self.stopped = false;
        self.cancelled = false;
        self.attempt = Some(Attempt {
            id: self.attempts,
            path: path.to_owned(),
            step: Step::Running(action),
        });
        tracing::info!(?action, "a device is asked for");
        true
    }

    /// Connects the attempt's device, aside.
    fn connect(&self, path: String) {
        let Some(id) = self.attempt.as_ref().map(|a| a.id) else {
            return;
        };
        let (connection, inputs, timeout) = (
            self.connection.clone(),
            self.inputs.clone(),
            self.timeouts.connect,
        );
        tokio::spawn(async move {
            let call =
                connection.call_method(Some(BLUEZ), path.as_str(), Some(DEVICE), "Connect", &());
            let failure = match tokio::time::timeout(timeout, call).await {
                Ok(Ok(_)) => None,
                Ok(Err(err)) => {
                    tracing::info!(%err, "the device did not connect");
                    Some(Failure::NoAnswer)
                }
                Err(_) => {
                    tracing::info!("the device did not connect in time: given up");
                    // Disconnect cancels a connection under way.
                    let _ = connection
                        .call_method(Some(BLUEZ), path.as_str(), Some(DEVICE), "Disconnect", &())
                        .await;
                    Some(Failure::NoAnswer)
                }
            };
            let _ = inputs.send(Input::Ended(id, failure));
        });
    }

    /// Pairs the attempt's device, aside, through the agent.
    /// Its time is `timeouts.pair`, longer once a code shows ([`Self::asked`]
    /// moves the deadline).
    fn pair(&mut self, path: String) {
        let Some(id) = self.attempt.as_ref().map(|a| a.id) else {
            return;
        };
        *self.pairing.lock().unwrap_or_else(PoisonError::into_inner) = Some(path.clone());
        let now = Instant::now();
        self.started = Some(now);
        *self.deadline.lock().unwrap_or_else(PoisonError::into_inner) = now + self.timeouts.pair;
        let (connection, inputs, deadline) = (
            self.connection.clone(),
            self.inputs.clone(),
            self.deadline.clone(),
        );
        tokio::spawn(async move {
            let call =
                connection.call_method(Some(BLUEZ), path.as_str(), Some(DEVICE), "Pair", &());
            let mut call = std::pin::pin!(call);
            let done = loop {
                let until = *deadline.lock().unwrap_or_else(PoisonError::into_inner);
                let at = tokio::time::Instant::from_std(until);
                match tokio::time::timeout_at(at, &mut call).await {
                    Ok(done) => break Some(done),
                    // Moved while it waited: a code shows.
                    Err(_) if *deadline.lock().unwrap_or_else(PoisonError::into_inner) > until => {}
                    Err(_) => break None,
                }
            };
            let input = match done {
                Some(Ok(_)) => Input::Paired(id),
                Some(Err(err)) => {
                    tracing::info!(%err, "the device did not pair");
                    Input::Ended(id, Some(Failure::Pairing))
                }
                None => {
                    tracing::info!("the device did not pair in time: cancelled");
                    let _ = connection
                        .call_method(
                            Some(BLUEZ),
                            path.as_str(),
                            Some(DEVICE),
                            "CancelPairing",
                            &(),
                        )
                        .await;
                    Input::Ended(id, Some(Failure::Pairing))
                }
            };
            let _ = inputs.send(input);
        });
    }

    /// Attempt `id` paired its device: trusted, so that it comes back by
    /// itself, then connected.
    async fn paired(&mut self, id: u64) {
        *self.pairing.lock().unwrap_or_else(PoisonError::into_inner) = None;
        let Some(attempt) = self.attempt.as_mut().filter(|a| a.id == id) else {
            return;
        };
        attempt.step = Step::Running(Action::Connect);
        let path = attempt.path.clone();
        let trusted = self
            .call(
                &path,
                PROPERTIES,
                "Set",
                &(DEVICE, "Trusted", Variant::from(true)),
            )
            .await;
        if let Err(err) = trusted {
            tracing::warn!(%err, "BlueZ did not trust the device paired");
        }
        self.connect(path);
    }

    fn ended(&mut self, id: u64, failure: Option<Failure>) {
        let Some(attempt) = self.attempt.as_mut().filter(|a| a.id == id) else {
            return;
        };
        let pairing = attempt.step == Step::Running(Action::Pair);
        if pairing {
            *self.pairing.lock().unwrap_or_else(PoisonError::into_inner) = None;
            if self.request.as_ref().is_some_and(|r| r.ours) {
                self.request = None;
            }
        }
        attempt.step = match failure {
            None => Step::Done,
            Some(_) if pairing && self.stopped => Step::Failed(Failure::Stopped),
            Some(_) if pairing && self.cancelled => Step::Failed(Failure::Cancelled),
            // Refused by the agent: a code was asked.
            Some(Failure::Pairing) if self.codes.contains(&attempt.path) => {
                Step::Failed(Failure::Code)
            }
            Some(failure) => Step::Failed(failure),
        };
        tracing::info!(step = ?attempt.step, "the device asked for is done");
    }
}

/// The program's pairing agent (`org.bluez.Agent1`). The bar's
/// (`NoInputNoOutput`) lets BlueZ pair "Just Works" devices and refuses a
/// code, said to the thread. The Settings' (`KeyboardDisplay`) asks the
/// user: a code to compare, to type on the device, a PIN to give, a
/// device pairing by itself to let in; it waits for the answer without
/// holding the thread. Either lets through only the device paired here,
/// or one the user accepted.
struct Agent {
    inputs: UnboundedSender<Input>,
    /// The device paired here, if one.
    pairing: Arc<Mutex<Option<String>>>,
    asks_codes: bool,
    waiting: Waiting,
    /// Devices the user let pair.
    accepted: Arc<Mutex<HashSet<String>>>,
    ids: Arc<AtomicU64>,
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
enum Refusal {
    #[zbus(error)]
    ZBus(zbus::Error),
    Rejected(String),
    Canceled(String),
}

impl Agent {
    /// A code is asked for `device` of an agent that gives none: refused.
    fn code(&self, device: &OwnedObjectPath) -> Refusal {
        let _ = self
            .inputs
            .send(Input::CodeAsked(device.as_str().to_owned()));
        Refusal::Rejected("a code is entered in the Settings".to_owned())
    }

    /// Whether `device` is the one paired here, or one the user let in.
    fn ours(&self, device: &OwnedObjectPath) -> bool {
        let pairing = self.pairing.lock().unwrap_or_else(PoisonError::into_inner);
        pairing.as_deref() == Some(device.as_str())
            || self
                .accepted
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains(device.as_str())
    }

    /// Asks the user `asked` for `device`, and waits for the answer.
    async fn ask(&self, device: &OwnedObjectPath, asked: Asked) -> Result<Answer, Refusal> {
        if !self.asks_codes {
            return Err(self.code(device));
        }
        let id = self.ids.fetch_add(1, Ordering::Relaxed) + 1;
        let (answer, answered) = tokio::sync::oneshot::channel();
        self.waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, answer);
        let path = device.as_str().to_owned();
        let _ = self.inputs.send(Input::Request {
            id,
            path: path.clone(),
            asked,
        });
        let answer = answered.await;
        self.waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
        let _ = self.inputs.send(Input::Answered(id));
        match answer {
            Ok(Answer::No) => Err(Refusal::Rejected("refused".to_owned())),
            Err(_) => Err(Refusal::Canceled("not answered".to_owned())),
            Ok(answer) => {
                self.accepted
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(path);
                Ok(answer)
            }
        }
    }

    /// Shows the user `asked` for `device`, which waits for nothing.
    fn show(&self, device: &OwnedObjectPath, asked: Asked) -> Result<(), Refusal> {
        if !self.asks_codes {
            return Err(self.code(device));
        }
        let _ = self.inputs.send(Input::Shown {
            path: device.as_str().to_owned(),
            asked,
        });
        Ok(())
    }
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {}

    async fn request_pin_code(&self, device: OwnedObjectPath) -> Result<String, Refusal> {
        match self.ask(&device, Asked::Pin).await? {
            Answer::Pin(pin) => Ok(pin),
            _ => Err(Refusal::Rejected("no PIN".to_owned())),
        }
    }

    fn display_pin_code(&self, device: OwnedObjectPath, code: String) -> Result<(), Refusal> {
        self.show(&device, Asked::Display { code, typed: None })
    }

    async fn request_passkey(&self, device: OwnedObjectPath) -> Result<u32, Refusal> {
        match self.ask(&device, Asked::Passkey).await? {
            Answer::Passkey(key) => Ok(key),
            _ => Err(Refusal::Rejected("no passkey".to_owned())),
        }
    }

    fn display_passkey(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
        entered: u16,
    ) -> Result<(), Refusal> {
        self.show(
            &device,
            Asked::Display {
                code: format!("{passkey:06}"),
                typed: Some(entered),
            },
        )
    }

    async fn request_confirmation(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
    ) -> Result<(), Refusal> {
        self.ask(&device, Asked::Confirm(passkey)).await.map(|_| ())
    }

    async fn request_authorization(&self, device: OwnedObjectPath) -> Result<(), Refusal> {
        if self.ours(&device) {
            return Ok(());
        }
        if !self.asks_codes {
            return Err(Refusal::Rejected("not asked for here".to_owned()));
        }
        self.ask(&device, Asked::Authorize).await.map(|_| ())
    }

    fn authorize_service(&self, device: OwnedObjectPath, _uuid: String) -> Result<(), Refusal> {
        if self.ours(&device) {
            Ok(())
        } else {
            Err(Refusal::Rejected("not asked for here".to_owned()))
        }
    }

    /// BlueZ gives up what it asked: the questions waiting are dropped.
    fn cancel(&self) {
        self.waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        let _ = self.inputs.send(Input::Cancelled);
    }
}

fn get<T>(props: &Props, name: &str) -> Option<T>
where
    T: for<'a> TryFrom<&'a Variant<'a>>,
    for<'a> <T as TryFrom<&'a Variant<'a>>>::Error: Into<zbus::Error>,
{
    props.get(name)?.downcast_ref::<T>().ok()
}

/// A device's kind: its `Icon`, made finer by its `Class`, else its
/// `Appearance`.
fn kind(props: &Props) -> Kind {
    let icon = get::<String>(props, "Icon").and_then(|i| Kind::from_icon(&i));
    let fine = get::<u32>(props, "Class")
        .and_then(Kind::from_class)
        .or_else(|| get::<u16>(props, "Appearance").and_then(Kind::from_appearance));
    Kind::of(icon, fine)
}

/// What is shown of `cache`: the first adapter's devices, connected (the
/// last connected first, as `stamps` say), paired (those too while it is
/// off), and around (named only, the strongest first).
fn snapshot(
    cache: &Cache,
    stamps: &HashMap<String, u64>,
    codes: &HashSet<String>,
    searching: bool,
    attempt: Option<Attempt>,
) -> Bluetooth {
    let Some((adapter, props)) = cache.adapters.iter().next() else {
        return Bluetooth::default();
    };
    let powered = get::<bool>(props, "Powered") == Some(true);
    let mut bluetooth = Bluetooth {
        adapter: true,
        powered,
        blocked: !powered && get::<String>(props, "PowerState").as_deref() == Some("off-blocked"),
        alias: get::<String>(props, "Alias").unwrap_or_default(),
        discoverable: powered && get::<bool>(props, "Discoverable") == Some(true),
        searching: searching && powered,
        attempt,
        ..Bluetooth::default()
    };
    let prefix = format!("{adapter}/");
    let mut nearby = Vec::new();
    for (path, props) in cache.devices.iter().filter(|(p, _)| p.starts_with(&prefix)) {
        let named = get::<String>(props, "Name").filter(|n| !n.trim().is_empty());
        let alias = get::<String>(props, "Alias").filter(|n| !n.trim().is_empty());
        let paired = get::<bool>(props, "Paired") == Some(true);
        let connected = get::<bool>(props, "Connected") == Some(true);
        // Off, only the paired ones.
        if !powered && !paired {
            continue;
        }
        // A device that never said its name (an address) is not offered.
        let name = match (named.is_some() || paired || connected, alias.or(named)) {
            (true, Some(name)) => name,
            _ => continue,
        };
        let kind = kind(props);
        let device = Device {
            path: path.clone(),
            address: get::<String>(props, "Address").unwrap_or_default(),
            name,
            kind,
            paired,
            connected,
            trusted: get::<bool>(props, "Trusted") == Some(true),
            battery: cache.batteries.get(path).copied().filter(|_| connected),
            code: !paired && (kind.asks_a_code() || codes.contains(path)),
        };
        if connected {
            bluetooth.connected.push(device);
        } else if paired {
            bluetooth.paired.push(device);
        } else {
            nearby.push((get::<i16>(props, "RSSI"), device));
        }
    }
    bluetooth
        .connected
        .sort_by_key(|d| std::cmp::Reverse(stamps.get(&d.path).copied().unwrap_or(0)));
    bluetooth.paired.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.path.cmp(&b.path))
    });
    // The strongest first; those not heard in this search last.
    nearby.sort_by(|(a, x), (b, y)| {
        b.map_or(i32::MIN, i32::from)
            .cmp(&a.map_or(i32::MIN, i32::from))
            .then_with(|| x.name.to_lowercase().cmp(&y.name.to_lowercase()))
            .then_with(|| x.path.cmp(&y.path))
    });
    bluetooth.nearby = nearby.into_iter().map(|(_, d)| d).collect();
    bluetooth
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned<'a>(value: impl Into<Variant<'a>>) -> OwnedVariant {
        OwnedVariant::try_from(value.into()).expect("an owned variant")
    }

    fn props(values: Vec<(&str, OwnedVariant)>) -> Props {
        values.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
    }

    const HCI0: &str = "/org/bluez/hci0";

    fn device(cache: &mut Cache, id: &str, values: Vec<(&str, OwnedVariant)>) -> String {
        let path = format!("{HCI0}/dev_{id}");
        cache.devices.insert(path.clone(), props(values));
        path
    }

    fn cache() -> Cache {
        let mut cache = Cache::default();
        cache.adapters.insert(
            HCI0.to_owned(),
            props(vec![
                ("Powered", owned(true)),
                ("Alias", owned("portable-alex")),
            ]),
        );
        cache
    }

    #[test]
    fn what_the_bar_shows_of_bluezs_objects() {
        assert!(
            !snapshot(
                &Cache::default(),
                &HashMap::new(),
                &HashSet::new(),
                true,
                None
            )
            .adapter
        );
        let mut cache = cache();
        let sony = device(
            &mut cache,
            "1",
            vec![
                ("Alias", owned("WH-1000XM4")),
                ("Name", owned("WH-1000XM4")),
                ("Icon", owned("audio-headphones")),
                ("Paired", owned(true)),
                ("Connected", owned(true)),
            ],
        );
        cache.batteries.insert(sony.clone(), 82);
        let mx = device(
            &mut cache,
            "2",
            vec![
                ("Alias", owned("MX Master 3S")),
                ("Class", owned(0x00_2580_u32)),
                ("Paired", owned(true)),
                ("Connected", owned(true)),
            ],
        );
        device(
            &mut cache,
            "3",
            vec![
                ("Alias", owned("Keychron K3")),
                ("Paired", owned(true)),
                ("Connected", owned(false)),
            ],
        );
        device(
            &mut cache,
            "4",
            vec![
                ("Alias", owned("JBL Flip 5")),
                ("Name", owned("JBL Flip 5")),
                ("Icon", owned("audio-card")),
                ("RSSI", owned(-70_i16)),
            ],
        );
        let pixel = device(
            &mut cache,
            "5",
            vec![
                ("Alias", owned("Pixel 8")),
                ("Name", owned("Pixel 8")),
                ("Icon", owned("phone")),
                ("RSSI", owned(-50_i16)),
            ],
        );
        // An address alone: not offered.
        device(
            &mut cache,
            "6",
            vec![
                ("Alias", owned("5C-12-AA-01-02-03")),
                ("RSSI", owned(-40_i16)),
            ],
        );
        // Another adapter's: not this one's.
        cache.devices.insert(
            "/org/bluez/hci1/dev_7".to_owned(),
            props(vec![("Alias", owned("Elsewhere")), ("Paired", owned(true))]),
        );
        let stamps = HashMap::from([(sony.clone(), 1), (mx.clone(), 2)]);
        let shown = snapshot(&cache, &stamps, &HashSet::new(), true, None);
        assert!(shown.adapter && shown.powered && shown.searching);
        assert!(!shown.blocked && !shown.discoverable);
        assert_eq!(shown.alias, "portable-alex");
        let names = |list: &[Device]| list.iter().map(|d| d.name.clone()).collect::<Vec<_>>();
        // The last connected first.
        assert_eq!(names(&shown.connected), ["MX Master 3S", "WH-1000XM4"]);
        assert_eq!(shown.connected[0].kind, Kind::Mouse);
        assert_eq!(shown.connected[1].battery, Some(82));
        assert_eq!(names(&shown.paired), ["Keychron K3"]);
        // The strongest first; the phone asks a code.
        assert_eq!(names(&shown.nearby), ["Pixel 8", "JBL Flip 5"]);
        assert!(shown.nearby[0].code && !shown.nearby[1].code);
        // A device whose pairing asked a code is left to the Settings too.
        let codes = HashSet::from([format!("{HCI0}/dev_4")]);
        assert!(snapshot(&cache, &stamps, &codes, false, None).nearby[1].code);
        let _ = pixel;
        // Off, blocked by rfkill: the paired devices only.
        cache.adapters.insert(
            HCI0.to_owned(),
            props(vec![
                ("Powered", owned(false)),
                ("PowerState", owned("off-blocked")),
                ("Discoverable", owned(true)),
            ]),
        );
        let off = snapshot(&cache, &stamps, &HashSet::new(), true, None);
        assert!(off.adapter && !off.powered && off.blocked && !off.searching);
        assert!(!off.discoverable && off.nearby.is_empty());
        assert_eq!(names(&off.paired), ["Keychron K3"]);
    }
}
