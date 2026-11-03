//! The StatusNotifierWatcher, and its host, on the session bus the
//! compositor names ([`hocus_link::NOTIFICATIONS_BUS_ENV`], the
//! notifications' bus), on a thread of its own; nothing without one: a
//! nested or headless desktop never takes the live session's icons.
//!
//! It serves `org.kde.StatusNotifierWatcher` and
//! `org.freedesktop.StatusNotifierWatcher` at `/StatusNotifierWatcher`,
//! owns both names and `org.kde.StatusNotifierHost-PID`, and says its host
//! is there. An item registers by its bus name (its object at
//! `/StatusNotifierItem`) or by its object's path (on the bus it calls
//! from); its properties are read (`org.kde.StatusNotifierItem`, else
//! `org.freedesktop.StatusNotifierItem`) and read again on its `New…`
//! signals; it goes when its name's owner does. The items keep the order in
//! which they (by their `Id`) first registered this session.
//!
//! What the bar asks runs aside, so that an item that does not answer
//! holds nothing up.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::Instant,
};

use hocus_link::NOTIFICATIONS_BUS_ENV;
use iced::{Rectangle, futures::StreamExt};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use zbus::{
    Connection, MatchRule, Message, MessageStream, OwnedVariant, Structure, Variant,
    connection::Builder,
    fdo::{DBusProxy, RequestNameFlags},
    message::Header,
    names::BusName,
};

use super::{Ask, ICON, Shown, menu, pixmap};
use crate::popover::{self, Event, Item};

const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const KDE_WATCHER: &str = "org.kde.StatusNotifierWatcher";
const FDO_WATCHER: &str = "org.freedesktop.StatusNotifierWatcher";
const KDE_ITEM: &str = "org.kde.StatusNotifierItem";
const FDO_ITEM: &str = "org.freedesktop.StatusNotifierItem";
const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU: &str = "com.canonical.dbusmenu";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

type Props = HashMap<String, OwnedVariant>;

/// What the thread acts on.
enum Input {
    /// `RegisterStatusNotifierItem(service)` from `sender`.
    Register {
        service: String,
        sender: String,
    },
    /// A bus name has no owner any more.
    Gone(String),
    /// An item's signal, or its menu's.
    Signal(Message),
    Ask(Ask),
    /// An item's properties, read (none: they could not be).
    Read {
        key: u32,
        props: Option<Props>,
    },
    /// An item whose menu could not be read: its own (`ContextMenu`).
    ContextMenu(u32),
}

/// Serves the watcher on the bus the compositor names, if it does.
pub fn start(asks: UnboundedReceiver<Ask>) {
    let Some(bus) = std::env::var(NOTIFICATIONS_BUS_ENV)
        .ok()
        .filter(|b| !b.is_empty())
    else {
        tracing::info!("no session bus given: no tray");
        return;
    };
    let started = std::thread::Builder::new()
        .name("tray".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(run(bus, asks)),
                Err(err) => tracing::warn!(%err, "no runtime for the tray"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot serve the tray");
    }
}

/// The items registered, as the watcher's property says them.
type Registered = Arc<Mutex<Vec<String>>>;

struct Watcher {
    inputs: UnboundedSender<Input>,
    registered: Registered,
}

impl Watcher {
    fn register(&self, header: &Header<'_>, service: String) {
        let sender = header
            .sender()
            .map(|s| s.as_str().to_owned())
            .unwrap_or_default();
        let _ = self.inputs.send(Input::Register { service, sender });
    }

    fn items(&self) -> Vec<String> {
        self.registered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    fn register_status_notifier_item(&self, #[zbus(header)] header: Header<'_>, service: String) {
        self.register(&header, service);
    }

    /// Another host: the desktop is the one that shows them.
    fn register_status_notifier_host(&self, service: String) {
        tracing::debug!(service, "another tray host");
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }
}

/// The same, under freedesktop's name.
struct FdoWatcher(Watcher);

#[zbus::interface(name = "org.freedesktop.StatusNotifierWatcher")]
impl FdoWatcher {
    fn register_status_notifier_item(&self, #[zbus(header)] header: Header<'_>, service: String) {
        self.0.register(&header, service);
    }

    fn register_status_notifier_host(&self, service: String) {
        tracing::debug!(service, "another tray host");
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.0.items()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }
}

/// An item registered.
#[derive(Debug, Clone)]
struct Entry {
    key: u32,
    /// What it registered as: its bus name and its path.
    registered: String,
    /// Its connection's unique name, which calls go to.
    owner: String,
    /// Its name on the bus, as it registered (a well-known name, or its
    /// unique one).
    bus: String,
    path: String,
    /// Its interface's name, once read.
    interface: &'static str,
    /// Where it comes in the order: its first registration's.
    order: u64,
    props: Option<Props>,
    shown: Option<Decoded>,
}

/// An item's icons, decoded.
#[derive(Debug, Clone, Default)]
struct Decoded {
    icon: Option<iced::advanced::image::Handle>,
    attention: Option<iced::advanced::image::Handle>,
    overlay: Option<iced::advanced::image::Handle>,
}

struct Tray {
    connection: Connection,
    inputs: UnboundedSender<Input>,
    registered: Registered,
    items: Vec<Entry>,
    next: u32,
    /// When each item (by its `Id`, else what it registered as) first
    /// registered: it keeps its place when it comes back.
    first: HashMap<String, u64>,
    firsts: u64,
}

async fn run(bus: String, mut asks: UnboundedReceiver<Ask>) {
    let (inputs, mut incoming) = unbounded_channel();
    let registered = Registered::default();
    let watcher = || Watcher {
        inputs: inputs.clone(),
        registered: registered.clone(),
    };
    let builder = if bus == "session" {
        Builder::session()
    } else {
        Builder::address(bus.as_str())
    };
    let built = builder
        .serve_at(WATCHER_PATH, watcher())
        .serve_at(WATCHER_PATH, FdoWatcher(watcher()))
        .build()
        .await;
    let connection = match built {
        Ok(connection) => connection,
        Err(err) => {
            tracing::warn!(%err, "cannot reach the session bus: no tray");
            return;
        }
    };
    if let Err(err) = listen(&connection, inputs.clone()).await {
        tracing::warn!(%err, "cannot follow the tray's items");
        return;
    }
    let host = format!("org.kde.StatusNotifierHost-{}", std::process::id());
    for name in [KDE_WATCHER, FDO_WATCHER, host.as_str()] {
        match connection
            .request_name_with_flags(name, RequestNameFlags::REPLACE_EXISTING)
            .await
        {
            Ok(reply) => tracing::debug!(name, ?reply, "a tray name"),
            Err(err) => tracing::warn!(name, %err, "a tray name not owned"),
        }
    }
    for interface in [KDE_WATCHER, FDO_WATCHER] {
        emit(&connection, interface, "StatusNotifierHostRegistered", &()).await;
    }
    tracing::info!("the tray is served");
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
    let mut tray = Tray {
        connection,
        inputs,
        registered,
        items: Vec::new(),
        next: 1,
        first: HashMap::new(),
        firsts: 0,
    };
    while let Some(input) = incoming.recv().await {
        tray.input(input).await;
        while let Ok(input) = incoming.try_recv() {
            tray.input(input).await;
        }
        tray.publish();
    }
}

/// The items' signals and their menus', and names losing their owners,
/// into `inputs`.
async fn listen(connection: &Connection, inputs: UnboundedSender<Input>) -> zbus::Result<()> {
    for interface in [KDE_ITEM, FDO_ITEM, MENU] {
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(interface)
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
    }
    let bus = DBusProxy::new(connection).await?;
    let mut owners = bus.receive_name_owner_changed().await?;
    tokio::spawn(async move {
        while let Some(changed) = owners.next().await {
            let Ok(args) = changed.args() else {
                continue;
            };
            if args.new_owner().is_none() {
                let name = args.name().to_string();
                if inputs.send(Input::Gone(name)).is_err() {
                    return;
                }
            }
        }
    });
    Ok(())
}

async fn emit<B>(connection: &Connection, interface: &str, member: &str, body: &B)
where
    B: serde::Serialize + zbus::wire::DynamicType,
{
    let sent = connection
        .emit_signal(None::<BusName<'_>>, WATCHER_PATH, interface, member, body)
        .await;
    if let Err(err) = sent {
        tracing::debug!(%err, member, "a tray signal not sent");
    }
}

/// Where an item registering as `service` from `sender` is: its bus name
/// and its object's path.
fn place(service: &str, sender: &str) -> Option<(String, String)> {
    let service = service.trim();
    if service.starts_with('/') {
        return (!sender.is_empty()).then(|| (sender.to_owned(), service.to_owned()));
    }
    if service.is_empty() {
        return None;
    }
    match service.split_once('/') {
        Some((bus, path)) => Some((bus.to_owned(), format!("/{path}"))),
        None => Some((service.to_owned(), ITEM_PATH.to_owned())),
    }
}

/// What an item says it is, `Passive` ones not shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Passive,
    Active,
    NeedsAttention,
}

fn status(props: &Props) -> Status {
    match text(props, "Status").as_deref() {
        Some("Passive") => Status::Passive,
        Some("NeedsAttention") => Status::NeedsAttention,
        _ => Status::Active,
    }
}

fn text(props: &Props, name: &str) -> Option<String> {
    match &**props.get(name)? {
        Variant::Str(s) => Some(s.as_str().to_owned()),
        Variant::ObjectPath(p) => Some(p.as_str().to_owned()),
        _ => None,
    }
}

fn flag(props: &Props, name: &str) -> bool {
    matches!(props.get(name).map(|v| &**v), Some(Variant::Bool(true)))
}

/// An `a(iiay)` of pixmaps.
fn pixmaps(value: &Variant<'_>) -> Vec<pixmap::Pixmap> {
    let Variant::Array(array) = value else {
        return Vec::new();
    };
    array
        .inner()
        .iter()
        .filter_map(|p| match p {
            Variant::Structure(s) => one_pixmap(s),
            _ => None,
        })
        .collect()
}

fn one_pixmap(s: &Structure<'_>) -> Option<pixmap::Pixmap> {
    let [Variant::I32(w), Variant::I32(h), Variant::Array(bytes)] = s.fields() else {
        return None;
    };
    let bytes = bytes
        .inner()
        .iter()
        .map(|b| match b {
            Variant::U8(b) => Some(*b),
            _ => None,
        })
        .collect::<Option<Vec<u8>>>()?;
    Some((*w, *h, bytes))
}

/// The tooltip's title and text, `(sa(iiay)ss)`.
fn tooltip(props: &Props) -> (String, String) {
    let Some(Variant::Structure(s)) = props.get("ToolTip").map(|v| &**v) else {
        return (String::new(), String::new());
    };
    let field = |i: usize| match s.fields().get(i) {
        Some(Variant::Str(t)) => t.as_str().trim().to_owned(),
        _ => String::new(),
    };
    (field(2), field(3))
}

/// The name an item's tooltip gives first, and what it says after.
fn words(props: &Props) -> (String, String) {
    let name = text(props, "Title")
        .filter(|t| !t.trim().is_empty())
        .or_else(|| text(props, "Id"))
        .unwrap_or_default()
        .trim()
        .to_owned();
    let (title, body) = tooltip(props);
    let said = if title.is_empty() { body } else { title };
    let said = if said == name { String::new() } else { said };
    (name, said)
}

/// One of an item's icons: its pixmaps (`…Pixmap`), else its name
/// (`…Name`), at `px` pixels.
fn icon(props: &Props, which: &str, px: u32, scale: f32) -> Option<iced::advanced::image::Handle> {
    let pixmaps = props
        .get(&format!("{which}Pixmap"))
        .map(|v| pixmaps(v))
        .unwrap_or_default();
    pixmap::of_pixmaps(&pixmaps, px).or_else(|| {
        let name = text(props, &format!("{which}Name"))?;
        let theme = text(props, "IconThemePath").unwrap_or_default();
        pixmap::of_name(&name, &theme, ICON as u32, scale)
    })
}

fn decode(props: &Props, scale: f32) -> Decoded {
    let px = crate::launcher::icons::pixels(ICON as u32, scale);
    Decoded {
        icon: icon(props, "Icon", px, scale),
        attention: icon(props, "AttentionIcon", px, scale),
        overlay: icon(props, "OverlayIcon", px.div_ceil(2), scale),
    }
}

impl Tray {
    async fn input(&mut self, input: Input) {
        match input {
            Input::Register { service, sender } => self.register(&service, &sender).await,
            Input::Gone(name) => self.gone(&name).await,
            Input::Signal(message) => self.signal(&message),
            Input::Read { key, props } => self.read(key, props),
            Input::Ask(ask) => self.ask(ask),
            Input::ContextMenu(key) => self.call(key, "ContextMenu", (0i32, 0i32)),
        }
    }

    async fn register(&mut self, service: &str, sender: &str) {
        let Some((bus, path)) = place(service, sender) else {
            tracing::debug!(service, "a tray item that says no place");
            return;
        };
        let registered = format!("{bus}{path}");
        if self.items.iter().any(|i| i.registered == registered) {
            return;
        }
        let owner = if bus.starts_with(':') {
            bus.clone()
        } else {
            match self.owner_of(&bus).await {
                Some(owner) => owner,
                None => {
                    tracing::debug!(bus, "a tray item without an owner");
                    return;
                }
            }
        };
        let key = self.next;
        self.next += 1;
        tracing::debug!(key, registered, "a tray item registers");
        self.items.push(Entry {
            key,
            registered: registered.clone(),
            owner,
            bus,
            path,
            interface: KDE_ITEM,
            order: u64::MAX,
            props: None,
            shown: None,
        });
        self.say_registered();
        for interface in [KDE_WATCHER, FDO_WATCHER] {
            emit(
                &self.connection,
                interface,
                "StatusNotifierItemRegistered",
                &(registered.as_str(),),
            )
            .await;
        }
        self.load(key);
    }

    async fn owner_of(&self, bus: &str) -> Option<String> {
        let name = BusName::try_from(bus).ok()?;
        let proxy = DBusProxy::new(&self.connection).await.ok()?;
        proxy.get_name_owner(name).await.ok().map(|o| o.to_string())
    }

    fn say_registered(&self) {
        *self
            .registered
            .lock()
            .unwrap_or_else(PoisonError::into_inner) =
            self.items.iter().map(|i| i.registered.clone()).collect();
    }

    async fn gone(&mut self, name: &str) {
        let gone: Vec<String> = self
            .items
            .iter()
            .filter(|i| i.owner == name || i.bus == name)
            .map(|i| i.registered.clone())
            .collect();
        if gone.is_empty() {
            return;
        }
        self.items.retain(|i| i.owner != name && i.bus != name);
        self.say_registered();
        for registered in gone {
            tracing::debug!(registered, "a tray item went");
            for interface in [KDE_WATCHER, FDO_WATCHER] {
                emit(
                    &self.connection,
                    interface,
                    "StatusNotifierItemUnregistered",
                    &(registered.as_str(),),
                )
                .await;
            }
        }
    }

    /// Reads item `key`'s properties aside; they come back as an input.
    fn load(&self, key: u32) {
        let Some(item) = self.items.iter().find(|i| i.key == key) else {
            return;
        };
        let (connection, owner, path, first) = (
            self.connection.clone(),
            item.owner.clone(),
            item.path.clone(),
            item.interface,
        );
        let inputs = self.inputs.clone();
        tokio::spawn(async move {
            let mut props = None;
            let order = if first == KDE_ITEM {
                [KDE_ITEM, FDO_ITEM]
            } else {
                [FDO_ITEM, KDE_ITEM]
            };
            for interface in order {
                let reply = connection
                    .call_method(
                        Some(owner.as_str()),
                        path.as_str(),
                        Some(PROPERTIES),
                        "GetAll",
                        &(interface,),
                    )
                    .await;
                match reply.and_then(|r| r.body().deserialize::<Props>()) {
                    Ok(mut read) if !read.is_empty() => {
                        if let Ok(name) = OwnedVariant::try_from(Variant::from(interface)) {
                            read.insert("\0interface".to_owned(), name);
                        }
                        props = Some(read);
                        break;
                    }
                    Ok(_) => {}
                    Err(err) => tracing::debug!(%err, owner, interface, "a tray item not read"),
                }
            }
            let _ = inputs.send(Input::Read { key, props });
        });
    }

    fn read(&mut self, key: u32, props: Option<Props>) {
        let scale = super::scale();
        let Some(props) = props else {
            return;
        };
        let id = text(&props, "Id").filter(|i| !i.is_empty());
        let Some(at) = self.items.iter().position(|i| i.key == key) else {
            return;
        };
        if self.items[at].order == u64::MAX {
            let by = id.unwrap_or_else(|| self.items[at].registered.clone());
            let order = *self.first.entry(by).or_insert_with(|| {
                self.firsts += 1;
                self.firsts
            });
            self.items[at].order = order;
        }
        let item = &mut self.items[at];
        item.interface = match text(&props, "\0interface").as_deref() {
            Some(FDO_ITEM) => FDO_ITEM,
            _ => KDE_ITEM,
        };
        item.shown = Some(decode(&props, scale));
        item.props = Some(props);
    }

    fn signal(&mut self, message: &Message) {
        let header = message.header();
        let (Some(sender), Some(path), Some(interface), Some(member)) = (
            header.sender(),
            header.path(),
            header.interface(),
            header.member(),
        ) else {
            return;
        };
        let (sender, path) = (sender.as_str(), path.as_str());
        if interface.as_str() == MENU {
            // The open menu changed: read again.
            if let Some(item) = self.items.iter().find(|i| {
                i.owner == sender
                    && i.props
                        .as_ref()
                        .and_then(|p| text(p, "Menu"))
                        .is_some_and(|m| m == path)
            }) && super::menu_open(item.key)
            {
                tracing::debug!(key = item.key, "the tray's menu changed");
                self.menu(item.key, None);
            }
            return;
        }
        let keys: Vec<u32> = self
            .items
            .iter()
            .filter(|i| i.owner == sender && i.path == path)
            .map(|i| i.key)
            .collect();
        for key in keys {
            tracing::debug!(key, member = member.as_str(), "a tray item changed");
            self.load(key);
        }
    }

    /// What the bar shows, in the items' order.
    fn publish(&self) {
        let mut items: Vec<&Entry> = self.items.iter().filter(|i| i.props.is_some()).collect();
        items.sort_by_key(|i| (i.order, i.key));
        let shown = items
            .into_iter()
            .filter_map(|item| {
                let props = item.props.as_ref()?;
                let decoded = item.shown.clone().unwrap_or_default();
                let icon = match status(props) {
                    Status::Passive => return None,
                    Status::NeedsAttention => decoded.attention.or(decoded.icon),
                    Status::Active => decoded.icon,
                };
                let (name, tip) = words(props);
                Some(Shown {
                    key: item.key,
                    name,
                    tip,
                    icon,
                    overlay: decoded.overlay,
                    since: Instant::now(),
                })
            })
            .collect();
        super::publish(shown);
    }

    fn ask(&mut self, ask: Ask) {
        match ask {
            Ask::Rescale => {
                let scale = super::scale();
                for item in &mut self.items {
                    if let Some(props) = &item.props {
                        item.shown = Some(decode(props, scale));
                    }
                }
            }
            Ask::Activate { key, bar, rect } => {
                let Some(item) = self.items.iter().find(|i| i.key == key) else {
                    return;
                };
                if item.props.as_ref().is_some_and(|p| flag(p, "ItemIsMenu")) {
                    self.menu(key, Some((bar, rect)));
                    return;
                }
                let (connection, owner, path, interface) = (
                    self.connection.clone(),
                    item.owner.clone(),
                    item.path.clone(),
                    item.interface,
                );
                let inputs = self.inputs.clone();
                tokio::spawn(async move {
                    let done = activate(&connection, &owner, &path, interface, "Activate").await;
                    if let Err(err) = done {
                        tracing::debug!(%err, key, "the item does not activate: its menu");
                        let _ = inputs.send(Input::Ask(Ask::Menu { key, bar, rect }));
                    }
                });
            }
            Ask::Secondary { key } => {
                let Some(item) = self.items.iter().find(|i| i.key == key) else {
                    return;
                };
                let (connection, owner, path, interface) = (
                    self.connection.clone(),
                    item.owner.clone(),
                    item.path.clone(),
                    item.interface,
                );
                tokio::spawn(async move {
                    let method = "SecondaryActivate";
                    let done = activate(&connection, &owner, &path, interface, method).await;
                    if let Err(err) = done {
                        tracing::debug!(%err, key, "a tray item's second action failed");
                    }
                });
            }
            Ask::Scroll {
                key,
                delta,
                horizontal,
            } => {
                let orientation = if horizontal { "horizontal" } else { "vertical" };
                self.call(key, "Scroll", (delta, orientation.to_owned()));
            }
            Ask::Menu { key, bar, rect } => self.menu(key, Some((bar, rect))),
            Ask::Event { key, id } => {
                let Some((owner, menu)) = self.menu_of(key) else {
                    return;
                };
                let connection = self.connection.clone();
                tokio::spawn(async move {
                    let stamp = 0u32;
                    let done = connection
                        .call_method(
                            Some(owner.as_str()),
                            menu.as_str(),
                            Some(MENU),
                            "Event",
                            &(id, "clicked", Variant::I32(0), stamp),
                        )
                        .await;
                    if let Err(err) = done {
                        tracing::debug!(%err, key, id, "a menu's row not sent");
                    }
                });
            }
        }
    }

    /// Calls `method` on item `key`, aside.
    fn call<B>(&self, key: u32, method: &'static str, body: B)
    where
        B: serde::Serialize + zbus::wire::DynamicType + Send + Sync + 'static,
    {
        let Some(item) = self.items.iter().find(|i| i.key == key) else {
            return;
        };
        let (connection, owner, path, interface) = (
            self.connection.clone(),
            item.owner.clone(),
            item.path.clone(),
            item.interface,
        );
        tokio::spawn(async move {
            let done = connection
                .call_method(
                    Some(owner.as_str()),
                    path.as_str(),
                    Some(interface),
                    method,
                    &body,
                )
                .await;
            if let Err(err) = done {
                tracing::debug!(%err, key, method, "a tray item's call failed");
            }
        });
    }

    /// Item `key`'s owner and its menu's path, if it has one.
    fn menu_of(&self, key: u32) -> Option<(String, String)> {
        let item = self.items.iter().find(|i| i.key == key)?;
        let menu = text(item.props.as_ref()?, "Menu").filter(|m| m.len() > 1)?;
        Some((item.owner.clone(), menu))
    }

    /// Item `key`'s menu, read aside, then shown: opened beside its icon
    /// (`at`: its bar and where its icon is), else drawn again; an item
    /// without one is asked for its own (`ContextMenu`).
    fn menu(&self, key: u32, at: Option<(String, Rectangle)>) {
        let Some((owner, menu)) = self.menu_of(key) else {
            if at.is_some() {
                self.call(key, "ContextMenu", (0i32, 0i32));
            }
            return;
        };
        let connection = self.connection.clone();
        let inputs = self.inputs.clone();
        tokio::spawn(async move {
            let ask = |method: &'static str| {
                let connection = connection.clone();
                let (owner, menu) = (owner.clone(), menu.clone());
                async move {
                    match method {
                        "AboutToShow" => {
                            connection
                                .call_method(
                                    Some(owner.as_str()),
                                    menu.as_str(),
                                    Some(MENU),
                                    method,
                                    &(0i32,),
                                )
                                .await
                        }
                        _ => {
                            connection
                                .call_method(
                                    Some(owner.as_str()),
                                    menu.as_str(),
                                    Some(MENU),
                                    method,
                                    &(0i32, -1i32, Vec::<String>::new()),
                                )
                                .await
                        }
                    }
                }
            };
            // Some (Electron's) fill their menu as it is about to show.
            let _ = ask("AboutToShow").await;
            let read = match ask("GetLayout").await {
                Ok(reply) => {
                    let body = reply.body();
                    let scale = super::scale();
                    let px = crate::launcher::icons::pixels(menu_icon(), scale);
                    body.deserialize::<Structure<'_>>().ok().and_then(|s| {
                        let root = s.fields().get(1)?;
                        menu::parse(root, &mut |of| match of {
                            menu::IconOf::Png(bytes) => pixmap::of_png(bytes, px),
                            menu::IconOf::Name(name) => {
                                pixmap::of_name(name, "", menu_icon(), scale)
                            }
                        })
                    })
                }
                Err(err) => {
                    tracing::debug!(%err, key, "a tray item's menu not read");
                    None
                }
            };
            match (read, at) {
                (Some(root), at) if !root.children.is_empty() => {
                    tracing::debug!(
                        key,
                        rows = ?root.children.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
                        "the tray's menu is read"
                    );
                    super::set_menu(key, root);
                    if let Some((bar, rect)) = at {
                        popover::event(Event::Open {
                            bar,
                            item: Item::Tray(key),
                            rect,
                        });
                    }
                }
                (_, Some(_)) => {
                    let _ = inputs.send(Input::ContextMenu(key));
                }
                (_, None) => {}
            }
        });
    }
}

/// Calls the item's `method` (`Activate`, `SecondaryActivate`) for the
/// user's click: the compositor told first, naming the item's process,
/// and its token for the click given to the item, so that its window may
/// come forward (5.14b).
async fn activate(
    connection: &Connection,
    owner: &str,
    path: &str,
    interface: &'static str,
    method: &'static str,
) -> zbus::Result<()> {
    let pid = pid_of(connection, owner).await.unwrap_or(0);
    if let Some(token) = super::clicked(pid).await {
        provide_token(connection, owner, path, interface, &token).await;
    }
    connection
        .call_method(Some(owner), path, Some(interface), method, &(0i32, 0i32))
        .await
        .map(|_| ())
}

/// The process of the connection `owner`, as the bus knows it.
async fn pid_of(connection: &Connection, owner: &str) -> Option<u32> {
    let name = BusName::try_from(owner).ok()?;
    let bus = DBusProxy::new(connection).await.ok()?;
    match bus.get_connection_unix_process_id(name).await {
        Ok(pid) => Some(pid),
        Err(err) => {
            tracing::debug!(%err, owner, "a tray item's process not known");
            None
        }
    }
}

/// How long an item may take to take the click's token before it is
/// activated all the same.
const TOKEN_TAKEN: std::time::Duration = std::time::Duration::from_millis(250);

/// `ProvideXdgActivationToken(token)`, its answer waited for before
/// `Activate`: an item may serve its calls side by side (zbus's do), and
/// must have the token first. An item without the method (Electron's)
/// says so at once.
async fn provide_token(
    connection: &Connection,
    owner: &str,
    path: &str,
    interface: &'static str,
    token: &str,
) {
    let body = (token,);
    let call = connection.call_method(
        Some(owner),
        path,
        Some(interface),
        "ProvideXdgActivationToken",
        &body,
    );
    match tokio::time::timeout(TOKEN_TAKEN, call).await {
        Ok(Ok(_)) => tracing::debug!(owner, "a tray item took its click's token"),
        Ok(Err(err)) => tracing::debug!(%err, owner, "a tray item without a click's token"),
        Err(_) => tracing::debug!(owner, "a tray item slow to take its click's token"),
    }
}

/// The rows' icons, logical pixels.
fn menu_icon() -> u32 {
    crate::popover::tray::ICON as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_registers_by_its_name_or_by_its_path() {
        assert_eq!(
            place("org.kde.StatusNotifierItem-42-1", ":1.7"),
            Some((
                "org.kde.StatusNotifierItem-42-1".to_owned(),
                ITEM_PATH.to_owned()
            ))
        );
        assert_eq!(
            place("/org/ayatana/NotificationItem/slack", ":1.9"),
            Some((
                ":1.9".to_owned(),
                "/org/ayatana/NotificationItem/slack".to_owned()
            ))
        );
        assert_eq!(
            place(":1.3/StatusNotifierItem", ":1.3"),
            Some((":1.3".to_owned(), ITEM_PATH.to_owned()))
        );
        assert_eq!(place("", ":1.3"), None);
        assert_eq!(place("/path", ""), None);
    }

    fn props(pairs: &[(&str, Variant<'static>)]) -> Props {
        pairs
            .iter()
            .map(|(k, v)| {
                (
                    (*k).to_owned(),
                    OwnedVariant::try_from(v.try_clone().expect("a value")).expect("owned"),
                )
            })
            .collect()
    }

    #[test]
    fn its_tooltip_says_its_name_then_what_it_says() {
        let tip = Structure::builder()
            .append_field(Variant::from(""))
            .append_field(Variant::Array(zbus::Array::new(
                &zbus::Signature::from_bytes(b"(iiay)").expect("a signature"),
            )))
            .append_field(Variant::from("Vous avez 2 notifications"))
            .append_field(Variant::from(""))
            .build()
            .expect("a tooltip");
        let said = props(&[
            ("Title", Variant::from("Slack")),
            ("Id", Variant::from("slack")),
            ("ToolTip", Variant::Structure(tip)),
        ]);
        assert_eq!(
            words(&said),
            ("Slack".to_owned(), "Vous avez 2 notifications".to_owned())
        );
        // No title: its id; the same words twice: once.
        let bare = props(&[("Id", Variant::from("nextcloud"))]);
        assert_eq!(words(&bare), ("nextcloud".to_owned(), String::new()));
        assert_eq!(status(&bare), Status::Active);
        let passive = props(&[("Status", Variant::from("Passive"))]);
        assert_eq!(status(&passive), Status::Passive);
        let wants = props(&[("Status", Variant::from("NeedsAttention"))]);
        assert_eq!(status(&wants), Status::NeedsAttention);
    }
}
