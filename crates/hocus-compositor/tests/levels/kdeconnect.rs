//! KDE Connect's daemon (`kdeconnectd`), faked on the fakes' private bus
//! (`fakes.rs`), never the machine's: its daemon object, its paired
//! devices and their battery, findmyphone, share and notifications
//! plugins, as kdeconnect-kde exports them (`core/daemon.h`,
//! `core/device.h`, `plugins/*/…plugin.h`), and what it does around them:
//!
//! - its Qt properties read with `Properties.Get`/`GetAll`, their changes
//!   told by its own signals only (`reachableChanged`, `pairStateChanged`,
//!   the battery's `refreshed`, the daemon's `deviceVisibilityChanged` and
//!   `deviceListChanged`), never `PropertiesChanged`;
//! - a phone's notification sent to the desktop's server as its
//!   `KNotification` sends it (the application "KDE Connect", the
//!   `desktop-entry` `org.kde.kdeconnect.daemon`, `resident`, the phone's
//!   application in `x-kde-display-appname`, the device in
//!   `x-kde-origin-name` and `x-kdeconnect-source-device`, the
//!   `inline-reply` action and its placeholder), closed there
//!   (`CloseNotification`) when the phone removes it, as the
//!   `KNotification`'s destructor does; its notification object kept under
//!   the plugin's path, `dismiss()` recorded;
//! - a pairing asked from the phone: its notification, "Accepter" pairing
//!   it (`pairStateChanged(3)`);
//! - a transfer reported to the job view as KIO's tracker does
//!   (`requiresJobTracker`, then `requestView` on `org.kde.JobViewServer`,
//!   `update`, `terminate`), its `cancelRequested` heard.
//!
//! What the shell asked is recorded, for the tests to compare.

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, PoisonError},
};

use futures_util::StreamExt;
use zbus::{
    Connection, MatchRule, MessageStream, OwnedObjectPath, OwnedVariant, Variant,
    connection::Builder, names::BusName,
};

pub const KDECONNECT: &str = "org.kde.kdeconnect";
const ROOT: &str = "/modules/kdeconnect";
const DAEMON: &str = "org.kde.kdeconnect.daemon";
const DEVICE: &str = "org.kde.kdeconnect.device";
const BATTERY: &str = "org.kde.kdeconnect.device.battery";
const SHARE: &str = "org.kde.kdeconnect.device.share";
const NOTIFICATIONS: &str = "org.kde.kdeconnect.device.notifications";
const SERVER: &str = "org.freedesktop.Notifications";
const SERVER_PATH: &str = "/org/freedesktop/Notifications";
pub const ENTRY: &str = "org.kde.kdeconnect.daemon";

/// A device KDE Connect knows.
#[derive(Debug, Clone)]
pub struct FakeDevice {
    pub id: &'static str,
    pub name: &'static str,
    /// KDE Connect's `type`: `phone`, `tablet`, `laptop`…
    pub kind: &'static str,
    pub reachable: bool,
    /// KDE Connect's `pairState`: 3 paired, 0 not.
    pub pair_state: i32,
    /// Its charge and whether it charges, while its plugin says one.
    pub battery: Option<(i32, bool)>,
}

impl FakeDevice {
    /// A phone paired and in reach, at `charge` %.
    pub const fn phone(id: &'static str, name: &'static str, charge: i32) -> Self {
        Self {
            id,
            name,
            kind: "phone",
            reachable: true,
            pair_state: 3,
            battery: Some((charge, false)),
        }
    }

    pub const fn tablet(id: &'static str, name: &'static str, charge: i32) -> Self {
        Self {
            kind: "tablet",
            ..Self::phone(id, name, charge)
        }
    }

    pub const fn laptop(id: &'static str, name: &'static str) -> Self {
        Self {
            kind: "laptop",
            battery: None,
            ..Self::phone(id, name, 0)
        }
    }

    pub const fn away(self) -> Self {
        Self {
            reachable: false,
            ..self
        }
    }

    pub const fn unpaired(self) -> Self {
        Self {
            pair_state: 0,
            ..self
        }
    }
}

/// What the shell asked of the fake, or said to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Ring(String),
    ShareUrls(String, Vec<String>),
    ShareText(String, String),
    Unpair(String),
    /// A notification object of device `.0`, its public id `.1`.
    Dismiss(String, String),
    /// The server's `NotificationReplied`: its id, the text.
    Replied(u32, String),
    /// The server's `ActionInvoked`: its id, the action.
    Action(u32, String),
    /// A view's `cancelRequested`, by its path.
    Cancel(String),
}

/// A phone's notification, as its plugin keeps it.
#[derive(Debug, Clone)]
struct Note {
    app: String,
    title: String,
    text: String,
    dismissable: bool,
    /// The desktop server's id for its `KNotification`.
    server: u32,
}

#[derive(Debug, Default)]
struct State {
    devices: BTreeMap<String, FakeDevice>,
    calls: Vec<Call>,
    /// Each device's notifications, by public id.
    notes: BTreeMap<String, BTreeMap<String, Note>>,
    next_note: u32,
    /// The pairing asked from a device: the server's id for its
    /// notification, the device.
    pairing: Option<(u32, String)>,
}

type Shared = Arc<Mutex<State>>;

fn state<T>(shared: &Shared, with: impl FnOnce(&mut State) -> T) -> T {
    with(&mut shared.lock().unwrap_or_else(PoisonError::into_inner))
}

fn device_path(id: &str) -> String {
    format!("{ROOT}/devices/{id}")
}

pub struct KdeConnect {
    shared: Shared,
    handle: tokio::runtime::Handle,
    address: String,
    connection: Option<Connection>,
}

impl KdeConnect {
    /// Serves KDE Connect on the bus at `address`, knowing `devices`.
    pub fn start(
        runtime: &tokio::runtime::Runtime,
        address: &str,
        devices: Vec<FakeDevice>,
    ) -> anyhow::Result<Self> {
        let shared: Shared = Arc::default();
        state(&shared, |s| {
            for device in devices {
                s.devices.insert(device.id.to_owned(), device);
            }
        });
        let mut fake = Self {
            shared,
            handle: runtime.handle().clone(),
            address: address.to_owned(),
            connection: None,
        };
        fake.serve()?;
        Ok(fake)
    }

    /// Its objects served, its name taken, the server's signals heard.
    fn serve(&mut self) -> anyhow::Result<()> {
        let shared = self.shared.clone();
        let address = self.address.clone();
        let connection = self.handle.block_on(async move {
            let connection = Builder::address(address.as_str())
                .serve_at(ROOT, Daemon(shared.clone()))
                .build()
                .await?;
            let ids: Vec<String> = state(&shared, |s| s.devices.keys().cloned().collect());
            for id in ids {
                serve_device(&connection, &shared, &id).await?;
            }
            let rule = MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface(SERVER)
                .build()?;
            let mut heard = MessageStream::for_match_rule(rule, &connection, None).await?;
            let to = shared.clone();
            let replying = connection.clone();
            tokio::spawn(async move {
                while let Some(Ok(message)) = heard.next().await {
                    let header = message.header();
                    let member = header.member().map(|m| m.as_str().to_owned());
                    match member.as_deref() {
                        Some("NotificationReplied") => {
                            if let Ok((id, text)) = message.body().deserialize::<(u32, String)>() {
                                state(&to, |s| s.calls.push(Call::Replied(id, text)));
                            }
                        }
                        Some("ActionInvoked") => {
                            if let Ok((id, key)) = message.body().deserialize::<(u32, String)>() {
                                state(&to, |s| s.calls.push(Call::Action(id, key.clone())));
                                accepted(&replying, &to, id, &key).await;
                            }
                        }
                        _ => {}
                    }
                }
            });
            connection.request_name(KDECONNECT).await?;
            Ok::<_, anyhow::Error>(connection)
        })?;
        self.connection = Some(connection);
        Ok(())
    }

    /// The daemon ends: its name loses its owner.
    pub fn stop(&mut self) {
        if let Some(connection) = self.connection.take() {
            self.handle.block_on(async move {
                let _ = connection.release_name(KDECONNECT).await;
                drop(connection);
            });
        }
    }

    /// It starts again, with the devices it knew.
    pub fn restart(&mut self) -> anyhow::Result<()> {
        self.stop();
        self.serve()
    }

    fn connection(&self) -> anyhow::Result<Connection> {
        self.connection
            .clone()
            .ok_or_else(|| anyhow::anyhow!("KDE Connect is not served"))
    }

    /// What the shell asked, in order.
    pub fn calls(&self) -> Vec<Call> {
        state(&self.shared, |s| s.calls.clone())
    }

    fn emit<B>(&self, path: &str, interface: &str, member: &str, body: &B) -> anyhow::Result<()>
    where
        B: serde::Serialize + zbus::DynamicType + Sync,
    {
        let connection = self.connection()?;
        self.handle.block_on(async {
            connection
                .emit_signal(None::<BusName<'_>>, path, interface, member, body)
                .await
        })?;
        Ok(())
    }

    /// Device `id` comes in reach, or goes out of it.
    pub fn set_reachable(&self, id: &str, reachable: bool) -> anyhow::Result<()> {
        state(&self.shared, |s| {
            if let Some(d) = s.devices.get_mut(id) {
                d.reachable = reachable;
            }
        });
        self.emit(&device_path(id), DEVICE, "reachableChanged", &(reachable,))?;
        self.emit(
            ROOT,
            DAEMON,
            "deviceVisibilityChanged",
            &(id.to_owned(), reachable),
        )?;
        self.emit(ROOT, DAEMON, "deviceListChanged", &())
    }

    /// Device `id`'s battery says `refreshed(charging, charge)`.
    pub fn set_battery(&self, id: &str, charge: i32, charging: bool) -> anyhow::Result<()> {
        state(&self.shared, |s| {
            if let Some(d) = s.devices.get_mut(id) {
                d.battery = Some((charge, charging));
            }
        });
        self.emit(
            &format!("{}/battery", device_path(id)),
            BATTERY,
            "refreshed",
            &(charging, charge),
        )
    }

    /// A device not known yet asks to pair, from the phone: its
    /// notification, as KDE Connect posts it, "Accepter" pairing it.
    pub fn ask_to_pair(&self, device: FakeDevice) -> anyhow::Result<u32> {
        let id = device.id.to_owned();
        let name = device.name.to_owned();
        state(&self.shared, |s| {
            s.devices.insert(id.clone(), device);
        });
        let connection = self.connection()?;
        let shared = self.shared.clone();
        let served = self.handle.block_on(async {
            serve_device(&connection, &shared, &id).await?;
            let mut hints: HashMap<&str, Variant<'_>> = HashMap::new();
            hints.insert("desktop-entry", Variant::from(ENTRY));
            hints.insert("x-kde-appname", Variant::from("kdeconnect"));
            let actions = ["1", "Accepter", "2", "Refuser"];
            let reply = connection
                .call_method(
                    Some(SERVER),
                    SERVER_PATH,
                    Some(SERVER),
                    "Notify",
                    &(
                        "KDE Connect",
                        0u32,
                        "kdeconnect",
                        format!("Demande d’association de {name}"),
                        "Clé : 4F 2A 91 0C".to_owned(),
                        actions.to_vec(),
                        hints,
                        -1i32,
                    ),
                )
                .await?;
            let server: u32 = reply.body().deserialize()?;
            connection
                .emit_signal(
                    None::<BusName<'_>>,
                    ROOT,
                    DAEMON,
                    "deviceAdded",
                    &(id.clone(),),
                )
                .await?;
            Ok::<_, anyhow::Error>(server)
        })?;
        state(&self.shared, |s| s.pairing = Some((served, id)));
        Ok(served)
    }

    /// Whether device `id` is paired now.
    pub fn paired(&self, id: &str) -> bool {
        state(&self.shared, |s| {
            s.devices.get(id).is_some_and(|d| d.pair_state == 3)
        })
    }

    /// Device `id`'s phone posts a notification of `app`: its public id,
    /// and the server's id for its `KNotification`.
    pub fn post(
        &self,
        id: &str,
        app: &str,
        title: &str,
        text: &str,
        dismissable: bool,
        reply: bool,
    ) -> anyhow::Result<(String, u32)> {
        let name = state(&self.shared, |s| {
            s.devices.get(id).map(|d| d.name.to_owned())
        })
        .ok_or_else(|| anyhow::anyhow!("no device {id}"))?;
        let connection = self.connection()?;
        let shared = self.shared.clone();
        let (app_, title_, text_) = (app.to_owned(), title.to_owned(), text.to_owned());
        let (public, server) = self.handle.block_on(async move {
            let mut hints: HashMap<&str, Variant<'_>> = HashMap::new();
            hints.insert("desktop-entry", Variant::from(ENTRY));
            hints.insert("resident", Variant::from(true));
            hints.insert("x-kde-appname", Variant::from("kdeconnect"));
            hints.insert("x-kde-display-appname", Variant::from(app_.as_str()));
            hints.insert("x-kde-origin-name", Variant::from(name.as_str()));
            hints.insert("x-kdeconnect-source-device", Variant::from(name.as_str()));
            let placeholder = format!("Répondre à {title_}…");
            let mut actions: Vec<&str> = Vec::new();
            if reply {
                hints.insert(
                    "x-kde-reply-placeholder-text",
                    Variant::from(placeholder.as_str()),
                );
                actions.extend(["inline-reply", "Répondre"]);
            }
            let answer = connection
                .call_method(
                    Some(SERVER),
                    SERVER_PATH,
                    Some(SERVER),
                    "Notify",
                    &(
                        "KDE Connect",
                        0u32,
                        "kdeconnect",
                        title_.as_str(),
                        text_.as_str(),
                        actions,
                        hints,
                        -1i32,
                    ),
                )
                .await?;
            let server: u32 = answer.body().deserialize()?;
            let public = state(&shared, |s| {
                s.next_note += 1;
                let public = s.next_note.to_string();
                s.notes.entry(id.to_owned()).or_default().insert(
                    public.clone(),
                    Note {
                        app: app_.clone(),
                        title: title_.clone(),
                        text: text_.clone(),
                        dismissable,
                        server,
                    },
                );
                public
            });
            let path = format!("{}/notifications/{public}", device_path(id));
            connection
                .object_server()
                .at(
                    path.as_str(),
                    NoteIface {
                        shared: shared.clone(),
                        device: id.to_owned(),
                        public: public.clone(),
                    },
                )
                .await?;
            connection
                .emit_signal(
                    None::<BusName<'_>>,
                    format!("{}/notifications", device_path(id)).as_str(),
                    NOTIFICATIONS,
                    "notificationPosted",
                    &(public.clone(),),
                )
                .await?;
            Ok::<_, anyhow::Error>((public, server))
        })?;
        Ok((public, server))
    }

    /// The phone removes notification `public` of device `id`: closed on
    /// the desktop as the `KNotification`'s destructor closes it.
    pub fn remove(&self, id: &str, public: &str) -> anyhow::Result<()> {
        let note = state(&self.shared, |s| {
            s.notes.get_mut(id).and_then(|n| n.remove(public))
        });
        let Some(note) = note else {
            anyhow::bail!("no notification {public} of {id}");
        };
        let connection = self.connection()?;
        let path = format!("{}/notifications/{public}", device_path(id));
        self.handle.block_on(async {
            let _ = connection
                .object_server()
                .remove::<NoteIface, _>(path.as_str())
                .await;
            connection
                .emit_signal(
                    None::<BusName<'_>>,
                    format!("{}/notifications", device_path(id)).as_str(),
                    NOTIFICATIONS,
                    "notificationRemoved",
                    &(public.to_owned(),),
                )
                .await?;
            connection
                .call_method(
                    Some(SERVER),
                    SERVER_PATH,
                    Some(SERVER),
                    "CloseNotification",
                    &(note.server,),
                )
                .await?;
            Ok::<_, anyhow::Error>(())
        })
    }

    /// Device `id` sent `url`: its share plugin's `shareReceived`.
    pub fn share_received(&self, id: &str, url: &str) -> anyhow::Result<()> {
        self.emit(
            &format!("{}/share", device_path(id)),
            SHARE,
            "shareReceived",
            &(url.to_owned(),),
        )
    }

    /// A transfer of `entry`'s (KDE Connect's own: [`ENTRY`]) reported to
    /// the job view as KIO's tracker does: its view's path. Its
    /// `cancelRequested` is recorded.
    pub fn transfer(
        &self,
        entry: &str,
        title: &str,
        file: &str,
        total: u64,
    ) -> anyhow::Result<String> {
        let connection = self.connection()?;
        let shared = self.shared.clone();
        let mut state_: HashMap<&str, Variant<'_>> = HashMap::new();
        state_.insert("title", Variant::from(title));
        state_.insert("descriptionLabel1", Variant::from("Fichier"));
        state_.insert("descriptionValue1", Variant::from(file));
        state_.insert("totalBytes", Variant::from(total));
        state_.insert("totalFiles", Variant::from(1u64));
        state_.insert("immediate", Variant::from(true));
        self.handle.block_on(async move {
            let needs: bool = connection
                .call_method(
                    Some("org.kde.kuiserver"),
                    "/JobViewServer",
                    Some("org.kde.kuiserver"),
                    "requiresJobTracker",
                    &(),
                )
                .await?
                .body()
                .deserialize()?;
            anyhow::ensure!(!needs, "the job view asks for a tracker of KDE Connect's");
            let reply = connection
                .call_method(
                    Some("org.kde.JobViewServer"),
                    "/JobViewServer",
                    Some("org.kde.JobViewServerV2"),
                    "requestView",
                    &(entry, 1i32, state_),
                )
                .await?;
            let view: OwnedObjectPath = reply.body().deserialize()?;
            let rule = MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface("org.kde.JobViewV3")
                .member("cancelRequested")
                .path(view.as_str())
                .build()?;
            let mut heard = MessageStream::for_match_rule(rule, &connection, None).await?;
            let path = view.as_str().to_owned();
            tokio::spawn(async move {
                while let Some(Ok(_)) = heard.next().await {
                    state(&shared, |s| s.calls.push(Call::Cancel(path.clone())));
                }
            });
            Ok::<_, anyhow::Error>(view.as_str().to_owned())
        })
    }

    /// The transfer of `view` at `processed` bytes.
    pub fn progress(&self, view: &str, processed: u64) -> anyhow::Result<()> {
        let connection = self.connection()?;
        let mut props: HashMap<&str, Variant<'_>> = HashMap::new();
        props.insert("processedBytes", Variant::from(processed));
        self.handle.block_on(async {
            connection
                .call_method(
                    Some("org.kde.JobViewServer"),
                    view,
                    Some("org.kde.JobViewV3"),
                    "update",
                    &(props,),
                )
                .await
        })?;
        Ok(())
    }

    /// The transfer of `view` ends: 0 done, else stopped.
    pub fn terminate(&self, view: &str, code: u32) -> anyhow::Result<()> {
        let connection = self.connection()?;
        let hints: HashMap<&str, Variant<'_>> = HashMap::new();
        self.handle.block_on(async {
            connection
                .call_method(
                    Some("org.kde.JobViewServer"),
                    view,
                    Some("org.kde.JobViewV3"),
                    "terminate",
                    &(code, "", hints),
                )
                .await
        })?;
        Ok(())
    }
}

impl Drop for KdeConnect {
    fn drop(&mut self) {
        let connection = self.connection.take();
        self.handle.block_on(async move { drop(connection) });
    }
}

/// "Accepter" on the pairing's notification: paired, said as KDE Connect
/// says it.
async fn accepted(connection: &Connection, shared: &Shared, server: u32, key: &str) {
    let id = state(shared, |s| match &s.pairing {
        Some((asked, id)) if *asked == server && key == "1" => {
            let id = id.clone();
            if let Some(d) = s.devices.get_mut(&id) {
                d.pair_state = 3;
            }
            s.pairing = None;
            Some(id)
        }
        _ => None,
    });
    let Some(id) = id else {
        return;
    };
    let _ = connection
        .emit_signal(
            None::<BusName<'_>>,
            device_path(&id).as_str(),
            DEVICE,
            "pairStateChanged",
            &(3i32,),
        )
        .await;
    let _ = connection
        .emit_signal(None::<BusName<'_>>, ROOT, DAEMON, "deviceListChanged", &())
        .await;
}

/// Device `id`'s objects: itself and its plugins.
async fn serve_device(connection: &Connection, shared: &Shared, id: &str) -> zbus::Result<()> {
    let path = device_path(id);
    let server = connection.object_server();
    let it = |s: &Shared| Plugin {
        shared: s.clone(),
        id: id.to_owned(),
    };
    server.at(path.as_str(), DeviceIface(it(shared))).await?;
    server
        .at(format!("{path}/battery").as_str(), BatteryIface(it(shared)))
        .await?;
    server
        .at(
            format!("{path}/findmyphone").as_str(),
            FindIface(it(shared)),
        )
        .await?;
    server
        .at(format!("{path}/share").as_str(), ShareIface(it(shared)))
        .await?;
    server
        .at(
            format!("{path}/notifications").as_str(),
            NotificationsIface(it(shared)),
        )
        .await?;
    Ok(())
}

struct Daemon(Shared);

#[zbus::interface(name = "org.kde.kdeconnect.daemon")]
impl Daemon {
    #[zbus(name = "devices")]
    fn devices(&self, only_reachable: bool, only_paired: bool) -> Vec<String> {
        state(&self.0, |s| {
            s.devices
                .values()
                .filter(|d| !only_reachable || d.reachable)
                .filter(|d| !only_paired || d.pair_state == 3)
                .map(|d| d.id.to_owned())
                .collect()
        })
    }
}

#[derive(Clone)]
struct Plugin {
    shared: Shared,
    id: String,
}

impl Plugin {
    fn device(&self) -> Option<FakeDevice> {
        state(&self.shared, |s| s.devices.get(&self.id).cloned())
    }

    fn call(&self, call: Call) {
        state(&self.shared, |s| s.calls.push(call));
    }
}

struct DeviceIface(Plugin);

#[zbus::interface(name = "org.kde.kdeconnect.device")]
impl DeviceIface {
    #[zbus(property, name = "name")]
    fn name(&self) -> String {
        self.0
            .device()
            .map(|d| d.name.to_owned())
            .unwrap_or_default()
    }

    #[zbus(property, name = "type")]
    fn kind(&self) -> String {
        self.0
            .device()
            .map(|d| d.kind.to_owned())
            .unwrap_or_default()
    }

    #[zbus(property, name = "isReachable")]
    fn is_reachable(&self) -> bool {
        self.0.device().is_some_and(|d| d.reachable)
    }

    #[zbus(property, name = "isPaired")]
    fn is_paired(&self) -> bool {
        self.0.device().is_some_and(|d| d.pair_state == 3)
    }

    #[zbus(property, name = "pairState")]
    fn pair_state(&self) -> i32 {
        self.0.device().map_or(0, |d| d.pair_state)
    }

    /// The plugins loaded: only while in reach.
    #[zbus(name = "loadedPlugins")]
    fn loaded_plugins(&self) -> Vec<String> {
        match self.0.device() {
            Some(d) if d.reachable => {
                let mut plugins = vec![
                    "kdeconnect_findmyphone",
                    "kdeconnect_share",
                    "kdeconnect_notifications",
                    "kdeconnect_clipboard",
                ];
                if d.battery.is_some() {
                    plugins.push("kdeconnect_battery");
                }
                plugins.into_iter().map(str::to_owned).collect()
            }
            _ => Vec::new(),
        }
    }

    #[zbus(name = "hasPlugin")]
    fn has_plugin(&self, name: String) -> bool {
        self.loaded_plugins().contains(&name)
    }

    #[zbus(name = "unpair")]
    async fn unpair(
        &self,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) {
        self.0.call(Call::Unpair(self.0.id.clone()));
        state(&self.0.shared, |s| {
            if let Some(d) = s.devices.get_mut(&self.0.id) {
                d.pair_state = 0;
            }
        });
        if let Some(path) = header.path() {
            let _ = connection
                .emit_signal(
                    None::<BusName<'_>>,
                    path.as_str(),
                    DEVICE,
                    "pairStateChanged",
                    &(0i32,),
                )
                .await;
        }
    }
}

struct BatteryIface(Plugin);

#[zbus::interface(name = "org.kde.kdeconnect.device.battery")]
impl BatteryIface {
    #[zbus(property, name = "charge")]
    fn charge(&self) -> i32 {
        self.0.device().and_then(|d| d.battery).map_or(-1, |b| b.0)
    }

    #[zbus(property, name = "isCharging")]
    fn is_charging(&self) -> bool {
        self.0.device().and_then(|d| d.battery).is_some_and(|b| b.1)
    }

    #[zbus(property, name = "hasBattery")]
    fn has_battery(&self) -> bool {
        self.0.device().is_some_and(|d| d.battery.is_some())
    }
}

struct FindIface(Plugin);

#[zbus::interface(name = "org.kde.kdeconnect.device.findmyphone")]
impl FindIface {
    #[zbus(name = "ring")]
    fn ring(&self) {
        self.0.call(Call::Ring(self.0.id.clone()));
    }
}

struct ShareIface(Plugin);

#[zbus::interface(name = "org.kde.kdeconnect.device.share")]
impl ShareIface {
    #[zbus(name = "shareUrls")]
    fn share_urls(&self, urls: Vec<String>) {
        self.0.call(Call::ShareUrls(self.0.id.clone(), urls));
    }

    #[zbus(name = "shareText")]
    fn share_text(&self, text: String) {
        self.0.call(Call::ShareText(self.0.id.clone(), text));
    }
}

struct NotificationsIface(Plugin);

#[zbus::interface(name = "org.kde.kdeconnect.device.notifications")]
impl NotificationsIface {
    #[zbus(name = "activeNotifications")]
    fn active_notifications(&self) -> Vec<String> {
        state(&self.0.shared, |s| {
            s.notes
                .get(&self.0.id)
                .map(|n| n.keys().cloned().collect())
                .unwrap_or_default()
        })
    }
}

/// One of a phone's notifications, at its public id.
struct NoteIface {
    shared: Shared,
    device: String,
    public: String,
}

impl NoteIface {
    fn note(&self) -> Option<Note> {
        state(&self.shared, |s| {
            s.notes.get(&self.device)?.get(&self.public).cloned()
        })
    }
}

#[zbus::interface(name = "org.kde.kdeconnect.device.notifications.notification")]
impl NoteIface {
    #[zbus(property, name = "appName")]
    fn app_name(&self) -> String {
        self.note().map(|n| n.app).unwrap_or_default()
    }

    #[zbus(property, name = "title")]
    fn title(&self) -> String {
        self.note().map(|n| n.title).unwrap_or_default()
    }

    #[zbus(property, name = "text")]
    fn text(&self) -> String {
        self.note().map(|n| n.text).unwrap_or_default()
    }

    #[zbus(property, name = "dismissable")]
    fn dismissable(&self) -> bool {
        self.note().is_some_and(|n| n.dismissable)
    }

    /// Recorded whether the phone lets it go or not: KDE Connect's own
    /// checks `dismissable` again, the shell should not have asked.
    #[zbus(name = "dismiss")]
    fn dismiss(&self) {
        state(&self.shared, |s| {
            s.calls
                .push(Call::Dismiss(self.device.clone(), self.public.clone()));
        });
    }
}

// --- The desktop portal's file chooser -------------------------------------------------

/// What the chooser was asked: the parent, the title.
pub type Asked = Arc<Mutex<Vec<(String, String)>>>;

/// The desktop portal's file chooser, on the fakes' bus: it answers
/// `OpenFile` at once with `files` (as `file://` URLs), and says what it
/// was asked (the parent, the title).
pub struct Chooser {
    files: Vec<String>,
    pub asked: Asked,
}

impl Chooser {
    pub fn serve(
        runtime: &tokio::runtime::Runtime,
        address: &str,
        files: Vec<String>,
    ) -> anyhow::Result<(Connection, Asked)> {
        let asked: Asked = Arc::default();
        let chooser = Self {
            files,
            asked: asked.clone(),
        };
        let connection = runtime.block_on(
            Builder::address(address)
                .name("org.freedesktop.portal.Desktop")
                .serve_at("/org/freedesktop/portal/desktop", chooser)
                .build(),
        )?;
        Ok((connection, asked))
    }
}

#[zbus::interface(name = "org.freedesktop.portal.FileChooser")]
impl Chooser {
    async fn open_file(
        &self,
        parent_window: String,
        title: String,
        options: HashMap<String, OwnedVariant>,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
        let token = options
            .get("handle_token")
            .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
            .unwrap_or_default();
        let path = format!(
            "/org/freedesktop/portal/desktop/request/{}/{token}",
            sender.trim_start_matches(':').replace('.', "_")
        );
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((parent_window, title));
        let files = self.files.clone();
        let (answering, at) = (connection.clone(), path.clone());
        // The answer after the call's: the shell listens before it asks.
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let mut results: HashMap<&str, Variant<'_>> = HashMap::new();
            results.insert("uris", Variant::from(files));
            let _ = answering
                .emit_signal(
                    None::<BusName<'_>>,
                    at.as_str(),
                    "org.freedesktop.portal.Request",
                    "Response",
                    &(0u32, results),
                )
                .await;
        });
        OwnedObjectPath::try_from(path).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
}
