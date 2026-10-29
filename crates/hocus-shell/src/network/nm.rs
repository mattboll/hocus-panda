//! NetworkManager on the system bus, on a thread of its own: what it says,
//! followed by its signals, and what the popover asks of it.
//!
//! The thread keeps NetworkManager's objects as last said: the manager
//! (`WirelessEnabled`, `Connectivity`, its check address), the devices
//! (the Wi-Fi one and a wired one, their state), the Wi-Fi device's access
//! points (name, strength, security), the active connections and the saved
//! ones (their SSID, never their secrets). It reads them all when
//! NetworkManager is there, then follows them: every signal NetworkManager
//! sends under its path (`PropertiesChanged` with the new values,
//! `AccessPointAdded` and `…Removed`, devices and connections added and
//! removed, `StateChanged` with its reason), and its owner on the bus, for
//! when it stops or starts. Nothing is polled. After each burst of
//! signals, what the bar shows is made again ([`snapshot`]) and published.
//!
//! What it asks: a scan (`RequestScan`, which NetworkManager limits: its
//! refusal is ignored), the Wi-Fi on or off (`WirelessEnabled`), a saved
//! or open network joined (`ActivateConnection`, or
//! `AddAndActivateConnection` with nothing but its name), a protected one
//! joined with its password (`AddAndActivateConnection` with it in
//! `802-11-wireless-security.psk`; for a saved one, the connection
//! updated with it first), the Wi-Fi device disconnected (`Disconnect`,
//! which keeps it from connecting again by itself). A connection this
//! thread added that fails is deleted, so that a wrong password is not
//! saved.

use std::collections::HashMap;

use iced::futures::StreamExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use zbus::{
    Connection, MatchRule, Message, MessageStream, ObjectPath, OwnedObjectPath, OwnedVariant,
    Variant, fdo::DBusProxy,
};

use super::{
    Ask,
    model::{AccessPoint, Attempt, Failure, Network, Progress, Security, Wifi, listed, name_of},
    publish,
    secret::Secret,
};
use crate::levels::{Presence, RETRY_FIRST, next_retry};

const NM: &str = "org.freedesktop.NetworkManager";
const ROOT: &str = "/org/freedesktop/NetworkManager";
const SETTINGS: &str = "/org/freedesktop/NetworkManager/Settings";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const ACCESS_POINT: &str = "org.freedesktop.NetworkManager.AccessPoint";
const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const SETTINGS_IFACE: &str = "org.freedesktop.NetworkManager.Settings";
const CONNECTION: &str = "org.freedesktop.NetworkManager.Settings.Connection";

/// `NMDeviceType`.
const ETHERNET: u32 = 1;
const WIFI: u32 = 2;
/// `NMDeviceState`: unmanaged, on its way (prepare to secondaries),
/// activated, failed.
const UNMANAGED: u32 = 10;
const PREPARE: u32 = 40;
const SECONDARIES: u32 = 90;
const ACTIVATED: u32 = 100;
/// `NMDeviceStateReason`: no secrets (a wrong password, once the network
/// refused it and no agent gave another).
const NO_SECRETS: u32 = 7;
/// `NMActiveConnectionState` and its reasons: no secrets, login failed.
const AC_ACTIVATED: u32 = 2;
const AC_DEACTIVATED: u32 = 4;
const AC_NO_SECRETS: u32 = 9;
const AC_LOGIN_FAILED: u32 = 10;
/// `NMConnectivityState`: behind a sign-in page.
const PORTAL: u32 = 2;

/// A scan whose end NetworkManager did not tell is taken for done after
/// this.
const SCAN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

type Props = HashMap<String, OwnedVariant>;
type Settings = HashMap<String, HashMap<String, OwnedVariant>>;

pub fn start(asks: UnboundedReceiver<Ask>) {
    let started = std::thread::Builder::new()
        .name("network".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(run(asks)),
                Err(err) => tracing::warn!(%err, "no runtime for NetworkManager"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot follow NetworkManager");
    }
}

/// What the thread acts on, from wherever it comes.
enum Input {
    Signal(Message),
    /// NetworkManager has an owner on the bus now, or not.
    Owner(bool),
    Ask(Ask),
    /// The scan asked as `n` is taken for done.
    ScanTimeout(u64),
}

async fn run(mut asks: UnboundedReceiver<Ask>) {
    let mut wait = RETRY_FIRST;
    let mut presence = Presence::Unknown;
    let connection = loop {
        match Connection::system().await {
            Ok(connection) => break connection,
            Err(err) => {
                presence.now(false, "NetworkManager (the system bus)", &err);
                publish(Network::default());
                while asks.try_recv().is_ok() {}
                tokio::time::sleep(wait).await;
                wait = next_retry(wait);
            }
        }
    };
    let (inputs, mut incoming) = unbounded_channel();
    if let Err(err) = listen(&connection, inputs.clone()).await {
        presence.now(false, "NetworkManager", &err);
        publish(Network::default());
        return;
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
        scan: Scan::Idle,
        scans: 0,
        attempt: None,
        tracking: Tracking::default(),
        attempts: 0,
        inputs,
    };
    // Its owner now: the stream says only what changes.
    let owned = match DBusProxy::new(&client.connection).await {
        Ok(bus) => bus
            .name_has_owner(NM.try_into().unwrap_or_else(|_| unreachable!()))
            .await
            .unwrap_or(false),
        Err(_) => false,
    };
    client.owner(owned).await;
    client.publish();
    while let Some(input) = incoming.recv().await {
        client.input(input).await;
        // A burst (a scan's strengths) is shown once.
        while let Ok(input) = incoming.try_recv() {
            client.input(input).await;
        }
        client.publish();
    }
}

/// NetworkManager's signals and its owner's changes, into `inputs`.
async fn listen(connection: &Connection, inputs: UnboundedSender<Input>) -> zbus::Result<()> {
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path_namespace(ROOT)
        .build()?;
    let mut signals = MessageStream::for_match_rule(rule, connection, None).await?;
    let bus = DBusProxy::new(connection).await?;
    let mut owners = bus.receive_name_owner_changed_with_args(&[(0, NM)]).await?;
    let to = inputs.clone();
    tokio::spawn(async move {
        while let Some(Ok(message)) = signals.next().await {
            if to.send(Input::Signal(message)).is_err() {
                return;
            }
        }
    });
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

/// NetworkManager's objects as last said.
#[derive(Debug, Default)]
struct Cache {
    /// The manager's properties; empty: NetworkManager is not there.
    root: Props,
    devices: HashMap<String, Device>,
    aps: HashMap<String, Props>,
    actives: HashMap<String, Props>,
    /// The saved Wi-Fi connections' SSIDs, by path.
    saved: HashMap<String, Vec<u8>>,
}

#[derive(Debug, Default)]
struct Device {
    props: Props,
    wireless: Props,
    /// Its last state change's reason.
    reason: u32,
}

impl Device {
    fn kind(&self) -> u32 {
        get(&self.props, "DeviceType").unwrap_or(0)
    }

    fn state(&self) -> u32 {
        get(&self.props, "State").unwrap_or(0)
    }
}

/// The scan asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Scan {
    Idle,
    /// Asked as `n`, NetworkManager's `LastScan` then `last`.
    Running {
        n: u64,
        last: i64,
    },
}

/// The connection asked for, as NetworkManager runs it.
#[derive(Debug, Default)]
struct Tracking {
    /// Its active connection.
    active: Option<String>,
    /// The connection added for it, deleted if it fails.
    added: Option<String>,
    /// The Wi-Fi device's reason as it failed, if it said one.
    reason: Option<u32>,
    /// Its active connection was seen among NetworkManager's: its going
    /// away now means it ended.
    seen: bool,
}

struct Client {
    connection: Connection,
    cache: Cache,
    presence: Presence,
    scan: Scan,
    scans: u64,
    attempt: Option<Attempt>,
    tracking: Tracking,
    attempts: u64,
    inputs: UnboundedSender<Input>,
}

impl Client {
    async fn input(&mut self, input: Input) {
        match input {
            Input::Signal(message) => self.signal(&message).await,
            Input::Owner(owned) => self.owner(owned).await,
            Input::Ask(ask) => self.ask(ask).await,
            Input::ScanTimeout(n) => {
                if matches!(self.scan, Scan::Running { n: m, .. } if m == n) {
                    self.scan = Scan::Idle;
                }
            }
        }
    }

    fn publish(&self) {
        let scanning = matches!(self.scan, Scan::Running { .. });
        publish(snapshot(&self.cache, scanning, self.attempt.clone()));
    }

    /// NetworkManager came (read it all) or went (forget it).
    async fn owner(&mut self, owned: bool) {
        self.cache = Cache::default();
        self.scan = Scan::Idle;
        if !owned {
            self.presence
                .now(false, "NetworkManager", "it is not running");
            return;
        }
        match self.load().await {
            Ok(()) => self.presence.now(true, "NetworkManager", ""),
            Err(err) => {
                self.cache = Cache::default();
                self.presence.now(false, "NetworkManager", &err);
            }
        }
    }

    async fn load(&mut self) -> zbus::Result<()> {
        self.cache.root = self.get_all(ROOT, NM).await?;
        for path in paths(&self.cache.root, "Devices") {
            self.load_device(&path).await;
        }
        for path in paths(&self.cache.root, "ActiveConnections") {
            self.load_active(&path).await;
        }
        let reply = self
            .call(SETTINGS, SETTINGS_IFACE, "ListConnections", &())
            .await?;
        let saved: Vec<OwnedObjectPath> = reply.body().deserialize()?;
        for path in saved {
            self.load_saved(path.as_str()).await;
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
            .call_method(Some(NM), path, Some(interface), method, body)
            .await
    }

    async fn get_all(&self, path: &str, interface: &str) -> zbus::Result<Props> {
        let reply = self.call(path, PROPERTIES, "GetAll", &(interface,)).await?;
        reply.body().deserialize()
    }

    async fn load_device(&mut self, path: &str) {
        let Ok(props) = self.get_all(path, DEVICE).await else {
            return;
        };
        let mut device = Device {
            props,
            ..Device::default()
        };
        if device.kind() == WIFI {
            device.wireless = self.get_all(path, WIRELESS).await.unwrap_or_default();
            for ap in paths(&device.wireless, "AccessPoints") {
                self.load_ap(&ap).await;
            }
        }
        self.cache.devices.insert(path.to_owned(), device);
    }

    async fn load_ap(&mut self, path: &str) {
        if let Ok(props) = self.get_all(path, ACCESS_POINT).await {
            self.cache.aps.insert(path.to_owned(), props);
        }
    }

    async fn load_active(&mut self, path: &str) {
        if let Ok(props) = self.get_all(path, ACTIVE).await {
            self.cache.actives.insert(path.to_owned(), props);
        }
    }

    async fn load_saved(&mut self, path: &str) {
        match self.settings(path).await {
            Ok(settings) => match wifi_ssid(&settings) {
                Some(ssid) => {
                    self.cache.saved.insert(path.to_owned(), ssid);
                }
                None => {
                    self.cache.saved.remove(path);
                }
            },
            Err(err) => tracing::debug!(%err, path, "a saved connection that does not read"),
        }
    }

    /// A saved connection's settings, without its secrets.
    async fn settings(&self, path: &str) -> zbus::Result<Settings> {
        let reply = self.call(path, CONNECTION, "GetSettings", &()).await?;
        reply.body().deserialize()
    }

    /// A signal from NetworkManager's objects.
    async fn signal(&mut self, message: &Message) {
        let header = message.header();
        let (Some(path), Some(interface), Some(member)) =
            (header.path(), header.interface(), header.member())
        else {
            return;
        };
        let (path, interface, member) = (
            path.as_str().to_owned(),
            interface.as_str().to_owned(),
            member.as_str().to_owned(),
        );
        let body = message.body();
        match (interface.as_str(), member.as_str()) {
            (PROPERTIES, "PropertiesChanged") => {
                if let Ok((iface, changed, _)) = body.deserialize::<(String, Props, Vec<String>)>()
                {
                    self.changed(&path, &iface, changed).await;
                }
            }
            // NetworkManager before 1.32 also sends its own, per interface.
            (iface, "PropertiesChanged") => {
                if let Ok(changed) = body.deserialize::<Props>() {
                    self.changed(&path, iface, changed).await;
                }
            }
            (WIRELESS, "AccessPointAdded") => {
                if let Ok(ap) = body.deserialize::<OwnedObjectPath>() {
                    self.load_ap(ap.as_str()).await;
                }
            }
            (WIRELESS, "AccessPointRemoved") => {
                if let Ok(ap) = body.deserialize::<OwnedObjectPath>() {
                    self.cache.aps.remove(ap.as_str());
                }
            }
            (NM, "DeviceAdded") => {
                if let Ok(device) = body.deserialize::<OwnedObjectPath>() {
                    self.load_device(device.as_str()).await;
                }
            }
            (NM, "DeviceRemoved") => {
                if let Ok(device) = body.deserialize::<OwnedObjectPath>() {
                    self.cache.devices.remove(device.as_str());
                }
            }
            (DEVICE, "StateChanged") => {
                if let Ok((new, _old, reason)) = body.deserialize::<(u32, u32, u32)>() {
                    self.device_state(&path, new, reason);
                }
            }
            (ACTIVE, "StateChanged") => {
                if let Ok((state, reason)) = body.deserialize::<(u32, u32)>() {
                    self.active_state(&path, state, reason).await;
                }
            }
            (SETTINGS_IFACE, "NewConnection") | (CONNECTION, "Updated") => {
                let path = match body.deserialize::<OwnedObjectPath>() {
                    Ok(added) => added.as_str().to_owned(),
                    Err(_) => path,
                };
                self.load_saved(&path).await;
            }
            (SETTINGS_IFACE, "ConnectionRemoved") => {
                if let Ok(gone) = body.deserialize::<OwnedObjectPath>() {
                    self.cache.saved.remove(gone.as_str());
                }
            }
            (CONNECTION, "Removed") => {
                self.cache.saved.remove(&path);
            }
            _ => {}
        }
    }

    /// Properties of `interface` changed at `path`; lists that changed
    /// bring their new objects in.
    async fn changed(&mut self, path: &str, interface: &str, changed: Props) {
        if path == ROOT && interface == NM {
            if self.cache.root.is_empty() {
                return;
            }
            let devices = changed.contains_key("Devices");
            let actives = changed.contains_key("ActiveConnections");
            self.cache.root.extend(changed);
            if devices {
                let listed = paths(&self.cache.root, "Devices");
                self.cache.devices.retain(|p, _| listed.contains(p));
                for path in listed {
                    if !self.cache.devices.contains_key(&path) {
                        self.load_device(&path).await;
                    }
                }
            }
            if actives {
                let listed = paths(&self.cache.root, "ActiveConnections");
                self.cache.actives.retain(|p, _| listed.contains(p));
                for path in listed {
                    if !self.cache.actives.contains_key(&path) {
                        self.load_active(&path).await;
                    }
                }
                self.active_gone();
            }
            return;
        }
        if let Some(device) = self.cache.devices.get_mut(path) {
            match interface {
                DEVICE => device.props.extend(changed),
                WIRELESS => {
                    let aps = changed.contains_key("AccessPoints");
                    let scanned: Option<i64> = get(&changed, "LastScan");
                    device.wireless.extend(changed);
                    if let (Some(at), Scan::Running { last, .. }) = (scanned, self.scan)
                        && at != last
                    {
                        self.scan = Scan::Idle;
                    }
                    if aps {
                        let listed = paths(&device.wireless, "AccessPoints");
                        for ap in &listed {
                            if !self.cache.aps.contains_key(ap) {
                                self.load_ap(ap).await;
                            }
                        }
                        let all: Vec<String> = self.wifi_aps();
                        self.cache.aps.retain(|p, _| all.contains(p));
                    }
                }
                _ => {}
            }
            return;
        }
        if interface == ACCESS_POINT {
            if let Some(ap) = self.cache.aps.get_mut(path) {
                ap.extend(changed);
            }
            return;
        }
        if interface == ACTIVE
            && let Some(active) = self.cache.actives.get_mut(path)
        {
            active.extend(changed);
        }
    }

    /// The access points of every Wi-Fi device.
    fn wifi_aps(&self) -> Vec<String> {
        self.cache
            .devices
            .values()
            .filter(|d| d.kind() == WIFI)
            .flat_map(|d| paths(&d.wireless, "AccessPoints"))
            .collect()
    }

    fn device_state(&mut self, path: &str, state: u32, reason: u32) {
        let wifi = match self.cache.devices.get_mut(path) {
            Some(device) => {
                device.reason = reason;
                device
                    .props
                    .insert("State".to_owned(), OwnedVariant::from(state));
                device.kind() == WIFI
            }
            None => false,
        };
        if wifi && self.tracking.active.is_some() && reason != 0 {
            self.tracking.reason = Some(reason);
        }
    }

    async fn active_state(&mut self, path: &str, state: u32, reason: u32) {
        if let Some(active) = self.cache.actives.get_mut(path) {
            active.insert("State".to_owned(), OwnedVariant::from(state));
        }
        if self.tracking.active.as_deref() != Some(path) {
            return;
        }
        match state {
            AC_ACTIVATED => self.ended(Progress::Connected).await,
            AC_DEACTIVATED => {
                let secret = matches!(reason, AC_NO_SECRETS | AC_LOGIN_FAILED)
                    || self.tracking.reason == Some(NO_SECRETS);
                let failure = if secret {
                    Failure::Secret
                } else {
                    Failure::Other
                };
                tracing::info!(reason, device = ?self.tracking.reason, ?failure, "the connection failed");
                self.ended(Progress::Failed(failure)).await;
            }
            _ => {}
        }
    }

    /// The active connection asked for went without saying how it ended.
    fn active_gone(&mut self) {
        let Some(active) = &self.tracking.active else {
            return;
        };
        if self.cache.actives.contains_key(active) {
            self.tracking.seen = true;
            return;
        }
        if !self.tracking.seen {
            return;
        }
        let failure = if self.tracking.reason == Some(NO_SECRETS) {
            Failure::Secret
        } else {
            Failure::Other
        };
        let added = self.tracking.added.take();
        self.finish(Progress::Failed(failure), added);
    }

    async fn ended(&mut self, progress: Progress) {
        let added = self.tracking.added.take();
        if let (Progress::Failed(_), Some(added)) = (progress, &added) {
            // Not saved with a wrong password, nor at all if it failed.
            if let Err(err) = self.call(added, CONNECTION, "Delete", &()).await {
                tracing::debug!(%err, "could not delete the connection that failed");
            }
        }
        self.finish(progress, None);
    }

    fn finish(&mut self, progress: Progress, added: Option<String>) {
        if let Some(added) = added {
            // Deleted later, off this call.
            let connection = self.connection.clone();
            tokio::spawn(async move {
                let _ = connection
                    .call_method(Some(NM), added.as_str(), Some(CONNECTION), "Delete", &())
                    .await;
            });
        }
        self.tracking = Tracking::default();
        if let Some(attempt) = &mut self.attempt {
            attempt.state = progress;
        }
    }

    /// The Wi-Fi device, if there is one.
    fn wifi(&self) -> Option<(&str, &Device)> {
        wifi_device(&self.cache)
    }

    async fn ask(&mut self, ask: Ask) {
        let Some(device) = self.wifi().map(|(p, _)| p.to_owned()) else {
            return;
        };
        match ask {
            Ask::Scan => self.scan(&device).await,
            Ask::Enable(on) => {
                let set = self
                    .call(
                        ROOT,
                        PROPERTIES,
                        "Set",
                        &(NM, "WirelessEnabled", Variant::from(on)),
                    )
                    .await;
                match set {
                    // The radio comes up and NetworkManager scans by
                    // itself: searching until it says it did.
                    Ok(_) if on => self.searching(&device),
                    Ok(_) => {}
                    Err(err) => tracing::warn!(%err, on, "NetworkManager did not switch the Wi-Fi"),
                }
            }
            Ask::Disconnect => {
                if let Err(err) = self.call(&device, DEVICE, "Disconnect", &()).await {
                    tracing::warn!(%err, "NetworkManager did not disconnect the Wi-Fi");
                }
            }
            Ask::Connect { ssid } => self.join(&device, ssid, None).await,
            Ask::ConnectWith { ssid, secret } => self.join(&device, ssid, Some(secret)).await,
            Ask::Abandon => {
                if let Some(active) = self.tracking.active.clone() {
                    let path = ObjectPath::try_from(active.as_str());
                    if let Ok(path) = path {
                        let _ = self.call(ROOT, NM, "DeactivateConnection", &(path,)).await;
                    }
                }
            }
        }
    }

    async fn scan(&mut self, device: &str) {
        let options: HashMap<&str, Variant<'_>> = HashMap::new();
        let last = self
            .wifi()
            .and_then(|(_, d)| get::<i64>(&d.wireless, "LastScan"))
            .unwrap_or(0);
        match self
            .call(device, WIRELESS, "RequestScan", &(options,))
            .await
        {
            Ok(_) => self.searching_since(last),
            // It scanned a moment ago: what it found is fresh.
            Err(err) => tracing::debug!(%err, "no scan now"),
        }
    }

    /// A scan runs on `device`, until its `LastScan` changes.
    fn searching(&mut self, device: &str) {
        let last = self
            .cache
            .devices
            .get(device)
            .and_then(|d| get::<i64>(&d.wireless, "LastScan"))
            .unwrap_or(0);
        self.searching_since(last);
    }

    /// A scan runs, until `LastScan` is no longer `last`, or a while.
    fn searching_since(&mut self, last: i64) {
        self.scans += 1;
        let n = self.scans;
        self.scan = Scan::Running { n, last };
        let inputs = self.inputs.clone();
        tokio::spawn(async move {
            tokio::time::sleep(SCAN_TIMEOUT).await;
            let _ = inputs.send(Input::ScanTimeout(n));
        });
    }

    /// Joins `ssid`: its saved connection, else a new one, with `secret`
    /// if given.
    async fn join(&mut self, device: &str, ssid: Vec<u8>, secret: Option<Secret>) {
        let network = snapshot(&self.cache, false, None)
            .networks
            .into_iter()
            .find(|n| n.ssid == ssid);
        let Some(network) = network else {
            tracing::info!("the network asked for is not around any more");
            return;
        };
        self.attempts += 1;
        self.attempt = Some(Attempt {
            id: self.attempts,
            ssid: ssid.clone(),
            state: Progress::Connecting,
        });
        self.tracking = Tracking::default();
        let saved = self
            .cache
            .saved
            .iter()
            .find(|(_, s)| **s == ssid)
            .map(|(p, _)| p.clone());
        let started = match (saved, secret) {
            (Some(saved), secret) => self.activate_saved(device, &network, &saved, secret).await,
            (None, secret) => {
                self.add_and_activate(device, &network, secret.as_ref())
                    .await
            }
        };
        match started {
            Ok((active, added)) => {
                tracing::info!(added = added.is_some(), "a connection is on its way");
                self.tracking.active = Some(active);
                self.tracking.added = added;
            }
            Err(err) => {
                tracing::warn!(%err, "NetworkManager did not start the connection");
                self.finish(Progress::Failed(Failure::Other), None);
            }
        }
    }

    /// The saved connection `saved`, its password updated first if one is
    /// given: its active connection.
    async fn activate_saved(
        &self,
        device: &str,
        network: &Wifi,
        saved: &str,
        secret: Option<Secret>,
    ) -> zbus::Result<(String, Option<String>)> {
        if let Some(secret) = secret {
            let settings = self.settings(saved).await?;
            let mut borrowed: HashMap<&str, HashMap<&str, Variant<'_>>> = HashMap::new();
            for (group, values) in &settings {
                let mut copy = HashMap::new();
                for (key, value) in values {
                    copy.insert(key.as_str(), Variant::try_from(value)?);
                }
                borrowed.insert(group.as_str(), copy);
            }
            let security = borrowed.entry("802-11-wireless-security").or_default();
            security
                .entry("key-mgmt")
                .or_insert_with(|| Variant::from(key_mgmt(network.security)));
            security.insert("psk", Variant::from(secret.as_str()));
            self.call(saved, CONNECTION, "Update", &(borrowed,)).await?;
        }
        let (saved, device, ap) = (
            ObjectPath::try_from(saved)?,
            ObjectPath::try_from(device)?,
            ObjectPath::try_from(network.ap.as_str())?,
        );
        let reply = self
            .call(ROOT, NM, "ActivateConnection", &(saved, device, ap))
            .await?;
        let active: OwnedObjectPath = reply.body().deserialize()?;
        Ok((active.as_str().to_owned(), None))
    }

    /// A new connection to `network`, with `secret` for a protected one:
    /// its active connection and itself.
    async fn add_and_activate(
        &self,
        device: &str,
        network: &Wifi,
        secret: Option<&Secret>,
    ) -> zbus::Result<(String, Option<String>)> {
        let mut settings: HashMap<&str, HashMap<&str, Variant<'_>>> = HashMap::new();
        settings
            .entry("802-11-wireless")
            .or_default()
            .insert("ssid", Variant::from(network.ssid.as_slice()));
        if let Some(secret) = secret {
            let security = settings.entry("802-11-wireless-security").or_default();
            security.insert("key-mgmt", Variant::from(key_mgmt(network.security)));
            security.insert("psk", Variant::from(secret.as_str()));
        }
        let (device, ap) = (
            ObjectPath::try_from(device)?,
            ObjectPath::try_from(network.ap.as_str())?,
        );
        let reply = self
            .call(
                ROOT,
                NM,
                "AddAndActivateConnection",
                &(settings, device, ap),
            )
            .await?;
        let (added, active): (OwnedObjectPath, OwnedObjectPath) = reply.body().deserialize()?;
        Ok((active.as_str().to_owned(), Some(added.as_str().to_owned())))
    }
}

/// The key management NetworkManager is given with a password.
fn key_mgmt(security: Security) -> &'static str {
    match security {
        Security::Psk { sae: true } => "sae",
        _ => "wpa-psk",
    }
}

fn get<T>(props: &Props, name: &str) -> Option<T>
where
    T: for<'a> TryFrom<&'a Variant<'a>>,
    for<'a> <T as TryFrom<&'a Variant<'a>>>::Error: Into<zbus::Error>,
{
    props.get(name)?.downcast_ref::<T>().ok()
}

fn bytes(props: &Props, name: &str) -> Option<Vec<u8>> {
    let value = props.get(name)?.try_clone().ok()?;
    Vec::<u8>::try_from(value).ok()
}

/// The object paths of a list property (`ao`), without NetworkManager's
/// "none" (`/`).
fn paths(props: &Props, name: &str) -> Vec<String> {
    props
        .get(name)
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| Vec::<OwnedObjectPath>::try_from(v).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.as_str().to_owned())
        .filter(|p| p != "/")
        .collect()
}

fn path(props: &Props, name: &str) -> Option<String> {
    let value = props.get(name)?.try_clone().ok()?;
    let path = OwnedObjectPath::try_from(value).ok()?;
    (path.as_str() != "/").then(|| path.as_str().to_owned())
}

/// A saved connection's SSID, when it is a Wi-Fi one.
fn wifi_ssid(settings: &Settings) -> Option<Vec<u8>> {
    let wifi = settings.get("802-11-wireless")?;
    let value = wifi.get("ssid")?.try_clone().ok()?;
    Vec::<u8>::try_from(value).ok().filter(|s| !s.is_empty())
}

/// The first Wi-Fi device NetworkManager manages.
fn wifi_device(cache: &Cache) -> Option<(&str, &Device)> {
    let mut wifi: Vec<(&String, &Device)> = cache
        .devices
        .iter()
        .filter(|(_, d)| d.kind() == WIFI && d.state() != UNMANAGED)
        .collect();
    wifi.sort_by_key(|(p, _)| p.as_str());
    wifi.first().map(|(p, d)| (p.as_str(), *d))
}

fn access_point(path: &str, props: &Props) -> Option<AccessPoint> {
    Some(AccessPoint {
        path: path.to_owned(),
        ssid: bytes(props, "Ssid")?,
        strength: get(props, "Strength").unwrap_or(0),
        security: Security::from_flags(
            get(props, "Flags").unwrap_or(0),
            get(props, "WpaFlags").unwrap_or(0),
            get(props, "RsnFlags").unwrap_or(0),
        ),
    })
}

/// What the bar shows of `cache`.
fn snapshot(cache: &Cache, scanning: bool, attempt: Option<Attempt>) -> Network {
    if cache.root.is_empty() {
        return Network::default();
    }
    let saved = |ssid: &[u8]| cache.saved.values().any(|s| s == ssid);
    let wifi = wifi_device(cache);
    let enabled = get::<bool>(&cache.root, "WirelessEnabled").unwrap_or(false)
        && get::<bool>(&cache.root, "WirelessHardwareEnabled").unwrap_or(true);
    let wired = cache
        .devices
        .values()
        .any(|d| d.kind() == ETHERNET && d.state() == ACTIVATED);
    let aps: Vec<AccessPoint> = wifi
        .map(|(_, d)| paths(&d.wireless, "AccessPoints"))
        .unwrap_or_default()
        .iter()
        .filter_map(|p| access_point(p, cache.aps.get(p)?))
        .collect();
    let current = wifi
        .filter(|(_, d)| d.state() == ACTIVATED)
        .and_then(|(_, d)| path(&d.wireless, "ActiveAccessPoint"))
        .and_then(|p| access_point(&p, cache.aps.get(&p)?))
        .filter(|ap| !ap.ssid.is_empty())
        .map(|ap| Wifi {
            name: name_of(&ap.ssid),
            saved: saved(&ap.ssid),
            ssid: ap.ssid,
            strength: ap.strength,
            security: ap.security,
            ap: ap.path,
        });
    let connecting = wifi.is_some_and(|(_, d)| (PREPARE..=SECONDARIES).contains(&d.state()));
    let check_uri = get::<String>(&cache.root, "ConnectivityCheckUri").filter(|u| !u.is_empty());
    Network {
        manager: true,
        hardware: wifi.is_some(),
        enabled,
        wired,
        portal: get::<u32>(&cache.root, "Connectivity") == Some(PORTAL) && current.is_some(),
        current,
        connecting,
        check_uri,
        networks: listed(&aps, saved),
        scanning: scanning && enabled,
        attempt,
    }
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

    fn object_path(path: &str) -> ObjectPath<'_> {
        ObjectPath::try_from(path).expect("a path")
    }

    fn cache(state: u32, connectivity: u32) -> Cache {
        let mut cache = Cache {
            root: props(vec![
                ("WirelessEnabled", owned(true)),
                ("Connectivity", owned(connectivity)),
                ("ConnectivityCheckUri", owned("http://check.test/")),
            ]),
            ..Cache::default()
        };
        let wifi = "/org/freedesktop/NetworkManager/Devices/1";
        let ap1 = "/org/freedesktop/NetworkManager/AccessPoint/1";
        let ap2 = "/org/freedesktop/NetworkManager/AccessPoint/2";
        let active = if state == ACTIVATED { ap1 } else { "/" };
        let list = vec![object_path(ap1), object_path(ap2)];
        cache.devices.insert(
            wifi.to_owned(),
            Device {
                props: props(vec![("DeviceType", owned(WIFI)), ("State", owned(state))]),
                wireless: props(vec![
                    ("AccessPoints", owned(list)),
                    ("ActiveAccessPoint", owned(object_path(active))),
                ]),
                reason: 0,
            },
        );
        for (path, name, strength, rsn) in
            [(ap1, "Livebox-A3F2", 80u8, 0x188u32), (ap2, "Bbox", 40, 0)]
        {
            cache.aps.insert(
                path.to_owned(),
                props(vec![
                    ("Ssid", owned(name.as_bytes())),
                    ("Strength", owned(strength)),
                    ("RsnFlags", owned(rsn)),
                ]),
            );
        }
        cache.saved.insert(
            "/org/freedesktop/NetworkManager/Settings/1".to_owned(),
            b"Livebox-A3F2".to_vec(),
        );
        cache
    }

    #[test]
    fn what_the_bar_shows_of_networkmanagers_objects() {
        assert!(!snapshot(&Cache::default(), false, None).manager);
        let connected = snapshot(&cache(ACTIVATED, 4), true, None);
        assert!(connected.manager && connected.hardware && connected.enabled);
        let current = connected.current.as_ref().expect("connected");
        assert_eq!(current.name, "Livebox-A3F2");
        assert!(current.saved);
        assert!(!connected.portal && connected.scanning);
        assert_eq!(connected.check_uri.as_deref(), Some("http://check.test/"));
        let others: Vec<&str> = connected.others().map(|n| n.name.as_str()).collect();
        assert_eq!(others, ["Bbox"]);
        // Behind a sign-in page.
        assert!(snapshot(&cache(ACTIVATED, PORTAL), false, None).portal);
        // On its way: no current network, connecting.
        let joining = snapshot(&cache(50, 1), false, None);
        assert!(joining.current.is_none() && joining.connecting);
        assert!(!joining.portal);
    }
}
