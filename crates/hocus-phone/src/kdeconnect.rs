//! KDE Connect's daemon on the session bus, on a thread of its own: its
//! paired devices, followed by its own signals, and what the bar asks of
//! them.
//!
//! KDE Connect exports its Qt properties (`Properties.Get`, `GetAll`) but
//! tells their changes by signals of its own only: the thread matches
//! every signal under `/modules/kdeconnect` and reads again what they name
//! (a device's `reachableChanged`, `pairStateChanged`, `nameChanged`,
//! `typeChanged`, `pluginsChanged`; the daemon's `deviceAdded`, `…Removed`,
//! `…VisibilityChanged`, `deviceListChanged`), and takes the battery's
//! `refreshed` as it says it. Its owner is followed (`NameOwnerChanged`):
//! gone, every device goes; back, it is read whole again. Absent, one
//! `info` line says so, never more.
//!
//! The daemon is never started from here: `NameHasOwner` first, and every
//! call with `NO_AUTO_START`. Reads wait 5 s at most; what the bar asks
//! (ring, share, forget, dismiss) runs aside, 5 s at most, and says in
//! the log how it ended.
//!
//! The transfers come from the job view ([`crate::jobs`]): a view of KDE
//! Connect's names its device in its title ("Sending to Pixel 8"); its
//! progress is published 10 times a second at most.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use zbus::{
    Connection, MatchRule, Message, MessageStream, OwnedVariant,
    connection::Builder,
    fdo::DBusProxy,
    names::BusName,
    proxy::{CacheProperties, MethodFlags},
};

use crate::{
    Ask,
    jobs::{self, Job},
    model::{self, Battery, Device, Kind, News, Phones, Props, Transfer, get, number},
};

const KDECONNECT: &str = "org.kde.kdeconnect";
const ROOT: &str = "/modules/kdeconnect";
const DAEMON: &str = "org.kde.kdeconnect.daemon";
const DEVICE: &str = "org.kde.kdeconnect.device";
const BATTERY: &str = "org.kde.kdeconnect.device.battery";
const FIND: &str = "org.kde.kdeconnect.device.findmyphone";
const SHARE: &str = "org.kde.kdeconnect.device.share";
const NOTIFICATIONS: &str = "org.kde.kdeconnect.device.notifications";
const NOTIFICATION: &str = "org.kde.kdeconnect.device.notifications.notification";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const PORTAL: &str = "org.freedesktop.portal.Desktop";

/// The plugins whose objects are used, as `loadedPlugins` names them.
const BATTERY_PLUGIN: &str = "kdeconnect_battery";
const FIND_PLUGIN: &str = "kdeconnect_findmyphone";
const SHARE_PLUGIN: &str = "kdeconnect_share";

/// KDE Connect's `pairState` of a paired device.
const PAIRED: i64 = 3;
/// How long a call may take.
const CALL: Duration = Duration::from_secs(5);
/// A transfer's progress is drawn this often at most.
const DRAW_EVERY: Duration = Duration::from_millis(100);

type Publish = Box<dyn FnMut(News) + Send>;

pub(crate) fn start(bus: Option<String>, asks: UnboundedReceiver<crate::Ask>, publish: Publish) {
    let started = std::thread::Builder::new()
        .name("phone".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(run(bus, asks, publish)),
                Err(err) => tracing::warn!(%err, "no runtime for KDE Connect"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot follow KDE Connect");
    }
}

/// What the thread acts on, in the order it comes.
enum Input {
    Signal(Message),
    /// KDE Connect's daemon got an owner, or lost it.
    Owner(bool),
    /// The desktop portal got an owner, or lost it.
    Portal(bool),
    Ask(Ask),
    Job(Job),
    /// Time to draw a transfer's progress held back.
    Tick,
}

/// Until the program ends, the asks are only answered when it does.
async fn idle(mut asks: UnboundedReceiver<Ask>) {
    while let Some(ask) = asks.recv().await {
        if let Ask::Quit(done) = ask {
            let _ = done.send(());
            return;
        }
    }
}

async fn run(bus: Option<String>, asks: UnboundedReceiver<Ask>, mut publish: Publish) {
    let Some(bus) = bus else {
        tracing::info!("KDE Connect: no session bus given");
        publish(News::Phones(Phones::default()));
        idle(asks).await;
        return;
    };
    let builder = if bus == "session" {
        Builder::session()
    } else {
        Builder::address(bus.as_str())
    };
    let connection = match builder.build().await {
        Ok(connection) => connection,
        Err(err) => {
            tracing::info!(%err, "KDE Connect: the session bus is not there");
            publish(News::Phones(Phones::default()));
            idle(asks).await;
            return;
        }
    };
    let (inputs, mut incoming) = unbounded_channel();
    if let Err(err) = listen(&connection, inputs.clone()).await {
        tracing::warn!(%err, "KDE Connect's signals not followed");
        publish(News::Phones(Phones::default()));
        idle(asks).await;
        return;
    }
    let (jobs, mut views) = unbounded_channel();
    tokio::spawn({
        let inputs = inputs.clone();
        async move {
            while let Some(job) = views.recv().await {
                if inputs.send(Input::Job(job)).is_err() {
                    return;
                }
            }
        }
    });
    jobs::serve(&connection, jobs).await;
    let mut asks = asks;
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
        inputs,
        publish,
        present: false,
        portal: false,
        devices: BTreeMap::new(),
        came: 0,
        jobs: BTreeMap::new(),
        last: None,
        published_at: None,
        held: false,
        said: HashSet::new(),
    };
    client.portal = client.portal_there().await;
    let owned = client.has_owner(KDECONNECT).await;
    client.owner(owned).await;
    client.publish(true);
    while let Some(input) = incoming.recv().await {
        let mut now = matches!(input, Input::Tick);
        if !client.input(input).await {
            return;
        }
        // A burst (a device coming in reach and its plugins) is shown once.
        while let Ok(input) = incoming.try_recv() {
            now |= matches!(input, Input::Tick);
            if !client.input(input).await {
                return;
            }
        }
        client.publish(now);
    }
}

/// KDE Connect's signals, and its owner's and the portal's changes, into
/// `inputs`.
async fn listen(connection: &Connection, inputs: UnboundedSender<Input>) -> zbus::Result<()> {
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path_namespace(ROOT)
        .build()?;
    let mut signals = MessageStream::for_match_rule(rule, connection, None).await?;
    let to = inputs.clone();
    tokio::spawn(async move {
        while let Some(Ok(message)) = signals.next().await {
            if to.send(Input::Signal(message)).is_err() {
                return;
            }
        }
    });
    let bus = DBusProxy::new(connection).await?;
    for (name, portal) in [(KDECONNECT, false), (PORTAL, true)] {
        let mut owners = bus
            .receive_name_owner_changed_with_args(&[(0, name)])
            .await?;
        let to = inputs.clone();
        tokio::spawn(async move {
            while let Some(changed) = owners.next().await {
                let owned = changed
                    .args()
                    .map_or(true, |args| args.new_owner().is_some());
                let input = if portal {
                    Input::Portal(owned)
                } else {
                    Input::Owner(owned)
                };
                if to.send(input).is_err() {
                    return;
                }
            }
        });
    }
    Ok(())
}

/// A device as last read, phone or not.
#[derive(Debug, Clone)]
struct Kept {
    name: String,
    /// `None`: a computer, a TV, anything but a phone or a tablet.
    kind: Option<Kind>,
    reachable: bool,
    battery: Option<Battery>,
    ring: bool,
    share: bool,
    came: u64,
}

/// A view of the job view, as it said.
#[derive(Debug, Default)]
struct JobState {
    /// KDE Connect's: the others are answered and ignored.
    ours: bool,
    /// The device its title names.
    device: Option<String>,
    title: String,
    file: String,
    files: u32,
    processed: u64,
    total: u64,
    /// Ended (0: done) or stopped.
    ended: Option<u32>,
}

struct Client {
    connection: Connection,
    inputs: UnboundedSender<Input>,
    publish: Publish,
    present: bool,
    portal: bool,
    /// Every paired device, by id.
    devices: BTreeMap<String, Kept>,
    /// The last number given to a device coming in reach.
    came: u64,
    /// The job view's views, by number.
    jobs: BTreeMap<u32, JobState>,
    last: Option<Phones>,
    published_at: Option<Instant>,
    /// A transfer's progress held back, a tick on its way.
    held: bool,
    /// What a missing member left out, said once.
    said: HashSet<String>,
}

impl Client {
    /// Acts on `input`; `false` once the program ends.
    async fn input(&mut self, input: Input) -> bool {
        match input {
            Input::Signal(message) => self.signal(&message).await,
            Input::Owner(owned) => self.owner(owned).await,
            Input::Portal(owned) => {
                self.portal = owned || self.portal_there().await;
            }
            Input::Ask(Ask::Quit(done)) => {
                let _ = done.send(());
                return false;
            }
            Input::Ask(ask) => self.ask(ask),
            Input::Job(job) => self.job(job),
            Input::Tick => self.held = false,
        }
        true
    }

    async fn has_owner(&self, name: &str) -> bool {
        let asked = async {
            let bus = DBusProxy::new(&self.connection).await?;
            bus.name_has_owner(BusName::try_from(name)?).await
        };
        matches!(tokio::time::timeout(CALL, asked).await, Ok(Ok(true)))
    }

    /// Whether a desktop portal is there, or would start there.
    async fn portal_there(&self) -> bool {
        if self.has_owner(PORTAL).await {
            return true;
        }
        let asked = async {
            let bus = DBusProxy::new(&self.connection).await?;
            bus.list_activatable_names().await
        };
        match tokio::time::timeout(CALL, asked).await {
            Ok(Ok(names)) => names.iter().any(|n| n.as_str() == PORTAL),
            _ => false,
        }
    }

    /// KDE Connect came (read it whole) or went (forget it).
    async fn owner(&mut self, owned: bool) {
        self.devices.clear();
        self.present = owned;
        if !owned {
            tracing::info!("KDE Connect: not on the session bus");
            return;
        }
        self.read_all().await;
        tracing::info!(devices = self.devices.len(), "KDE Connect read");
    }

    /// Says once that a member is missing (an older daemon): its feature
    /// is left out.
    fn missing(&mut self, what: &str, err: &zbus::Error) {
        if self.said.insert(what.to_owned()) {
            tracing::info!(what, %err, "KDE Connect: left out, its daemon lacks it");
        }
    }

    /// The paired devices, read again.
    async fn read_all(&mut self) {
        let ids: Vec<String> =
            match call(&self.connection, ROOT, DAEMON, "devices", &(false, true)).await {
                Ok(ids) => ids,
                Err(err) => {
                    tracing::info!(%err, "KDE Connect: its devices not read");
                    return;
                }
            };
        self.devices.retain(|id, _| ids.contains(id));
        for id in ids {
            self.read_device(&id).await;
        }
    }

    /// Device `id`, read again: gone from the list unless paired.
    async fn read_device(&mut self, id: &str) {
        let path = format!("{ROOT}/devices/{id}");
        let props: Props =
            match call(&self.connection, &path, PROPERTIES, "GetAll", &(DEVICE,)).await {
                Ok(props) => props,
                Err(err) => {
                    tracing::debug!(id, %err, "a device not read");
                    self.devices.remove(id);
                    return;
                }
            };
        let paired = match number(&props, "pairState") {
            Some(state) => state == PAIRED,
            None => get::<bool>(&props, "isPaired").unwrap_or(false),
        };
        if !paired {
            self.devices.remove(id);
            return;
        }
        let name = get::<String>(&props, "name").unwrap_or_default();
        let kind = Kind::of(&get::<String>(&props, "type").unwrap_or_default());
        let reachable = get::<bool>(&props, "isReachable").unwrap_or(false);
        let plugins: Vec<String> = if reachable && kind.is_some() {
            match call(&self.connection, &path, DEVICE, "loadedPlugins", &()).await {
                Ok(plugins) => plugins,
                Err(err) => {
                    self.missing("loadedPlugins", &err);
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };
        let has = |plugin: &str| plugins.iter().any(|p| p == plugin);
        let battery = if has(BATTERY_PLUGIN) {
            self.battery(&path).await
        } else {
            None
        };
        let before = self.devices.get(id);
        let came = match before {
            Some(kept) if kept.reachable || !reachable => kept.came,
            _ if reachable => {
                self.came += 1;
                self.came
            }
            _ => 0,
        };
        self.devices.insert(
            id.to_owned(),
            Kept {
                name,
                kind,
                reachable,
                battery,
                ring: has(FIND_PLUGIN),
                share: has(SHARE_PLUGIN),
                came,
            },
        );
    }

    async fn battery(&mut self, device: &str) -> Option<Battery> {
        let path = format!("{device}/battery");
        let props: Props =
            match call(&self.connection, &path, PROPERTIES, "GetAll", &(BATTERY,)).await {
                Ok(props) => props,
                Err(err) => {
                    tracing::debug!(%err, "a battery not read");
                    return None;
                }
            };
        if get::<bool>(&props, "hasBattery") == Some(false) {
            return None;
        }
        let charge = number(&props, "charge")?;
        (charge >= 0).then(|| Battery {
            charge: charge.clamp(0, 100) as u8,
            charging: get::<bool>(&props, "isCharging").unwrap_or(false),
        })
    }

    async fn signal(&mut self, message: &Message) {
        let header = message.header();
        let (Some(at), Some(interface), Some(member)) =
            (header.path(), header.interface(), header.member())
        else {
            return;
        };
        let at = at.as_str().to_owned();
        let interface = interface.as_str().to_owned();
        let member = member.as_str().to_owned();
        if !self.present {
            return;
        }
        let device = at
            .strip_prefix(ROOT)
            .and_then(|rest| rest.strip_prefix("/devices/"))
            .map(|rest| rest.split('/').next().unwrap_or(rest).to_owned());
        let body = message.body();
        match (interface.as_str(), member.as_str()) {
            (DAEMON, "deviceAdded" | "deviceRemoved" | "deviceListChanged") => {
                self.read_all().await;
            }
            (DAEMON, "deviceVisibilityChanged") => {
                if let Ok((id, _)) = body.deserialize::<(String, bool)>()
                    && self.devices.contains_key(&id)
                {
                    self.read_device(&id).await;
                }
            }
            (DEVICE, "pairStateChanged") => {
                // Paired now, or forgotten: the list changes.
                self.read_all().await;
            }
            (DEVICE, "reachableChanged" | "nameChanged" | "typeChanged" | "pluginsChanged") => {
                if let Some(id) = device
                    && self.devices.contains_key(&id)
                {
                    self.read_device(&id).await;
                }
            }
            (BATTERY, "refreshed") => {
                let (Some(id), Ok((charging, charge))) =
                    (device, body.deserialize::<(bool, i32)>())
                else {
                    return;
                };
                if let Some(kept) = self.devices.get_mut(&id)
                    && kept.reachable
                {
                    kept.battery = (charge >= 0).then(|| Battery {
                        charge: charge.clamp(0, 100) as u8,
                        charging,
                    });
                }
            }
            (SHARE, "shareReceived") => {
                let (Some(id), Ok(url)) = (device, body.deserialize::<String>()) else {
                    return;
                };
                let Some((name, kind)) = self
                    .devices
                    .get(&id)
                    .and_then(|k| k.kind.map(|kind| (k.name.clone(), kind)))
                else {
                    return;
                };
                tracing::debug!(device = id, "something received from a phone");
                (self.publish)(News::Received {
                    device: name,
                    kind,
                    url,
                });
            }
            _ => {}
        }
    }

    /// What the bar asks: run aside.
    fn ask(&mut self, ask: Ask) {
        let connection = self.connection.clone();
        let device = |id: &str| format!("{ROOT}/devices/{id}");
        match ask {
            Ask::Ring(id) => {
                let path = format!("{}/findmyphone", device(&id));
                aside("ring", async move {
                    call::<_, ()>(&connection, &path, FIND, "ring", &()).await
                });
            }
            Ask::Share { device: id, urls } => {
                let path = format!("{}/share", device(&id));
                aside("shareUrls", async move {
                    call::<_, ()>(&connection, &path, SHARE, "shareUrls", &(urls,)).await
                });
            }
            Ask::Text { device: id, text } => {
                let path = format!("{}/share", device(&id));
                aside("shareText", async move {
                    call::<_, ()>(&connection, &path, SHARE, "shareText", &(text,)).await
                });
            }
            Ask::Forget(id) => {
                let path = device(&id);
                aside("unpair", async move {
                    call::<_, ()>(&connection, &path, DEVICE, "unpair", &()).await
                });
            }
            Ask::Dismiss {
                device: name,
                app,
                title,
                text,
            } => {
                let Some(id) = self
                    .devices
                    .iter()
                    .find(|(_, k)| k.name == name)
                    .map(|(id, _)| id.clone())
                else {
                    tracing::debug!("a phone's notification closed, its device unknown");
                    return;
                };
                let path = format!("{}/notifications", device(&id));
                aside("dismiss", async move {
                    dismiss(&connection, &path, &app, &title, &text).await
                });
            }
            Ask::Cancel(view) => {
                let path = jobs::view_path(view);
                aside("cancelRequested", async move {
                    connection
                        .emit_signal(
                            None::<BusName<'_>>,
                            path.as_str(),
                            jobs::VIEW,
                            "cancelRequested",
                            &(),
                        )
                        .await
                });
            }
            Ask::Settle(id) => {
                self.jobs.retain(|_, job| {
                    !(job.device.as_deref() == Some(id.as_str()) && job.ended.is_some())
                });
            }
            Ask::Quit(_) => {}
        }
    }

    fn job(&mut self, job: Job) {
        match job {
            Job::Requested { view, entry, hints } => {
                let ours = entry == jobs::KDECONNECT_ENTRY;
                if !ours {
                    tracing::debug!(view, entry, "another application's job: not shown");
                }
                let mut state = JobState {
                    ours,
                    ..JobState::default()
                };
                self.merge(&mut state, &hints);
                self.jobs.insert(view, state);
            }
            Job::Update { view, props } => {
                if let Some(mut state) = self.jobs.remove(&view) {
                    self.merge(&mut state, &props);
                    self.jobs.insert(view, state);
                }
            }
            Job::Terminated { view, code } => {
                if code == 0 {
                    self.jobs.remove(&view);
                } else if let Some(state) = self.jobs.get_mut(&view) {
                    tracing::info!(view, code, "a transfer to a phone stopped");
                    state.ended = Some(code);
                }
            }
        }
        // Ended and never shown: nothing to keep.
        self.jobs.retain(|_, job| job.ours || job.ended.is_none());
    }

    /// What a view says, kept: its title names its device.
    fn merge(&self, state: &mut JobState, props: &HashMap<String, OwnedVariant>) {
        let text = |key: &str| get::<String>(props, key);
        let count = |key: &str| number(props, key).and_then(|n| u64::try_from(n).ok());
        if let Some(title) = text("title") {
            state.title = title;
        }
        if let Some(file) = text("descriptionValue1") {
            state.file = file;
        }
        if let Some(files) = count("totalFiles") {
            state.files = u32::try_from(files).unwrap_or(u32::MAX);
        }
        if let Some(total) = count("totalBytes") {
            state.total = total;
        }
        if let Some(processed) = count("processedBytes") {
            state.processed = processed;
        }
        if state.device.is_none() && !state.title.is_empty() {
            state.device = self
                .devices
                .iter()
                .filter(|(_, k)| !k.name.is_empty() && state.title.contains(&k.name))
                .max_by_key(|(_, k)| k.name.len())
                .map(|(id, _)| id.clone());
        }
    }

    /// The transfer of device `id` shown: its last.
    fn transfer(&self, id: &str) -> Option<Transfer> {
        self.jobs
            .iter()
            .rev()
            .find(|(_, job)| job.ours && job.device.as_deref() == Some(id))
            .map(|(view, job)| Transfer {
                view: *view,
                file: job.file.clone(),
                files: job.files.max(1),
                processed: job.processed,
                total: job.total,
                failed: job.ended.is_some(),
            })
    }

    fn phones(&self) -> Phones {
        let devices = self
            .devices
            .iter()
            .filter_map(|(id, kept)| {
                Some(Device {
                    id: id.clone(),
                    name: kept.name.clone(),
                    kind: kept.kind?,
                    reachable: kept.reachable,
                    battery: kept.battery.filter(|_| kept.reachable),
                    ring: kept.ring && kept.reachable,
                    share: kept.share && kept.reachable,
                    came: kept.came,
                    transfer: self.transfer(id),
                })
            })
            .collect();
        Phones {
            present: self.present,
            portal: self.portal,
            devices: model::rows(devices),
        }
    }

    /// Publishes what changed; a transfer's progress alone, 10 times a
    /// second at most, unless `now`.
    fn publish(&mut self, now: bool) {
        let phones = self.phones();
        if self.last.as_ref() == Some(&phones) {
            return;
        }
        let progress_only = self
            .last
            .as_ref()
            .is_some_and(|last| without_progress(last) == without_progress(&phones));
        if progress_only && !now {
            let since = self.published_at.map(|at| at.elapsed());
            if let Some(since) = since.filter(|s| *s < DRAW_EVERY) {
                if !self.held {
                    self.held = true;
                    let inputs = self.inputs.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(DRAW_EVERY - since).await;
                        let _ = inputs.send(Input::Tick);
                    });
                }
                return;
            }
        }
        self.published_at = Some(Instant::now());
        self.last = Some(phones.clone());
        tracing::debug!(
            present = phones.present,
            rows = phones.devices.len(),
            "KDE Connect's devices"
        );
        (self.publish)(News::Phones(phones));
    }
}

/// `phones` without its transfers' progress.
fn without_progress(phones: &Phones) -> Phones {
    let mut phones = phones.clone();
    for device in &mut phones.devices {
        if let Some(transfer) = &mut device.transfer {
            transfer.processed = 0;
        }
    }
    phones
}

/// `call` run aside, 5 s at most; how it ended said in the log.
fn aside<F, T>(what: &'static str, call: F)
where
    F: std::future::Future<Output = zbus::Result<T>> + Send + 'static,
    T: Send + 'static,
{
    tokio::spawn(async move {
        match tokio::time::timeout(CALL, call).await {
            Ok(Ok(_)) => tracing::debug!(what, "asked of KDE Connect"),
            Ok(Err(err)) => tracing::info!(what, %err, "KDE Connect refused"),
            Err(_) => tracing::info!(what, "KDE Connect did not answer in time"),
        }
    });
}

/// The phone's notification matching the one the user closed, the newest
/// first, dismissed there if it lets it go.
async fn dismiss(
    connection: &Connection,
    plugin: &str,
    app: &str,
    title: &str,
    text: &str,
) -> zbus::Result<()> {
    let mut ids: Vec<String> = call(
        connection,
        plugin,
        NOTIFICATIONS,
        "activeNotifications",
        &(),
    )
    .await?;
    // The newest first: KDE Connect numbers them as they come.
    ids.sort_by_key(|id| std::cmp::Reverse(id.parse::<u64>().unwrap_or(0)));
    let (title, text) = (model::plain(title), model::plain(text));
    let mut loose = None;
    for id in ids {
        let path = format!("{plugin}/{id}");
        let props: Props = call(connection, &path, PROPERTIES, "GetAll", &(NOTIFICATION,)).await?;
        let said = |key: &str| model::plain(&get::<String>(&props, key).unwrap_or_default());
        if said("appName") != model::plain(app) || said("title") != title {
            continue;
        }
        if said("text") == text {
            return dismiss_one(connection, &path, &props).await;
        }
        loose.get_or_insert((path, props));
    }
    match loose {
        Some((path, props)) => dismiss_one(connection, &path, &props).await,
        None => {
            tracing::debug!("a phone's notification closed, not found on the phone");
            Ok(())
        }
    }
}

async fn dismiss_one(connection: &Connection, path: &str, props: &Props) -> zbus::Result<()> {
    if get::<bool>(props, "dismissable") != Some(true) {
        tracing::debug!("a phone's notification it keeps: closed here only");
        return Ok(());
    }
    call::<_, ()>(connection, path, NOTIFICATION, "dismiss", &()).await
}

/// `method` of `interface` at `path`, on KDE Connect's daemon, never
/// starting it, 5 s at most.
async fn call<B, R>(
    connection: &Connection,
    path: &str,
    interface: &str,
    method: &str,
    body: &B,
) -> zbus::Result<R>
where
    B: serde::Serialize + zbus::DynamicType,
    R: for<'d> zbus::wire::DynamicDeserialize<'d>,
{
    let asked = async {
        let proxy: zbus::Proxy<'_> = zbus::proxy::Builder::new(connection)
            .destination(KDECONNECT)
            .path(path)
            .interface(interface)
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        proxy
            .call_with_flags::<_, _, R>(method, MethodFlags::NO_AUTO_START, body)
            .await?
            .ok_or_else(|| zbus::Error::Failure("no reply".to_owned()))
    };
    match tokio::time::timeout(CALL, asked).await {
        Ok(result) => result,
        Err(_) => Err(zbus::Error::Failure(format!("{method}: no answer in time"))),
    }
}
