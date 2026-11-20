//! UDisks2 on the system bus, on a thread of its own: its objects,
//! followed by its signals, and what the bar asks of it.
//!
//! The thread keeps UDisks2's objects as last said (every interface of
//! every object under `/org/freedesktop/UDisks2`): it reads them all with
//! `GetManagedObjects` as it starts and whenever UDisks2 gets an owner on
//! the bus, then follows `InterfacesAdded` and `…Removed`,
//! `PropertiesChanged` under its path, and its owner. After each burst of
//! signals, the drives that count ([`crate::model::counts`]) are made
//! again and published when they changed; one that was not there before
//! (and not when UDisks2 was read) is said plugged. A mounted drive's free
//! space is read with statvfs then, and every 30 s while one is mounted.
//!
//! What it asks: `Filesystem.Mount` (polkit lets the active session's
//! user); `Filesystem.Unmount` for every filesystem of the drive, which
//! writes what is pending, `Encrypted.Lock` for an unlocked container, then
//! `Drive.PowerOff` when the drive can. Each runs aside, two minutes at
//! most (a polkit dialog may wait for the user meanwhile), and says how it
//! ended; one action at a time a drive.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};

use futures_util::StreamExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use zbus::{
    Connection, MatchRule, Message, MessageStream, OwnedObjectPath, Variant, fdo::DBusProxy,
};

use crate::{
    Ask,
    model::{
        BLOCK, DRIVE, Doing, Drive, ENCRYPTED, FILESYSTEM, Interfaces, News, Props, counts, get,
        kind, mount_point, name, path,
    },
};

const UDISKS: &str = "org.freedesktop.UDisks2";
const ROOT: &str = "/org/freedesktop/UDisks2";
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
/// UDisks2's error when a filesystem is in use.
const BUSY: &str = "org.freedesktop.UDisks2.Error.DeviceBusy";

/// How long a mount, an unmount or a power off may take: a polkit dialog
/// may wait for the user meanwhile.
const CALL: Duration = Duration::from_secs(120);
/// How often a mounted drive's free space is read again.
const FREE_AGAIN: Duration = Duration::from_secs(30);

/// For tests: what statvfs would say a mounted drive has free (bytes).
const FREE_ENV: &str = "HOCUS_DRIVES_FREE";

type Publish = Box<dyn FnMut(News) + Send>;
type Objects = HashMap<OwnedObjectPath, Interfaces>;

/// The system bus, when away, is tried again after this, doubled each
/// time, up to [`RETRY_LAST`].
const RETRY_FIRST: Duration = Duration::from_secs(2);
const RETRY_LAST: Duration = Duration::from_secs(60);

pub(crate) fn start(asks: UnboundedReceiver<Ask>, publish: Publish) {
    let started = std::thread::Builder::new()
        .name("removable".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(run(asks, publish)),
                Err(err) => tracing::warn!(%err, "no runtime for UDisks2"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot follow UDisks2");
    }
}

/// What the thread acts on, in the order it comes.
enum Input {
    Signal(Message),
    Owner(bool),
    Ask(Ask),
    /// An action that ran aside on drive `.0` ended, saying `.1`.
    Ended(u32, News),
    /// Time to read the free space again.
    Tick,
}

async fn run(mut asks: UnboundedReceiver<Ask>, mut publish: Publish) {
    let mut wait = RETRY_FIRST;
    let mut said = false;
    let connection = loop {
        match Connection::system().await {
            Ok(connection) => break connection,
            Err(err) => {
                if !said {
                    tracing::info!(%err, "removable drives: no system bus; tried again later");
                    publish(News::Drives(Vec::new()));
                    said = true;
                }
                while let Ok(ask) = asks.try_recv() {
                    if let Ask::Quit(done) = ask {
                        let _ = done.send(());
                        return;
                    }
                }
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(RETRY_LAST);
            }
        }
    };
    let (inputs, mut incoming) = unbounded_channel();
    if let Err(err) = listen(&connection, inputs.clone()).await {
        tracing::warn!(%err, "removable drives: UDisks2's signals not followed");
        publish(News::Drives(Vec::new()));
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
    tokio::spawn({
        let inputs = inputs.clone();
        async move {
            let mut ticks = tokio::time::interval(FREE_AGAIN);
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticks.tick().await;
                if inputs.send(Input::Tick).is_err() {
                    return;
                }
            }
        }
    });
    let mut client = Client {
        connection,
        objects: BTreeMap::new(),
        inputs,
        publish,
        serials: HashMap::new(),
        next: 1,
        present: HashSet::new(),
        fresh: true,
        last: None,
        running: HashSet::new(),
        free: std::env::var(FREE_ENV).ok().and_then(|v| v.parse().ok()),
    };
    // UDisks2 starts on demand: reading it starts it.
    client.owner(true).await;
    client.publish();
    while let Some(input) = incoming.recv().await {
        if !client.input(input).await {
            return;
        }
        // A burst (a drive's partitions coming) is shown once.
        while let Ok(input) = incoming.try_recv() {
            if !client.input(input).await {
                return;
            }
        }
        client.publish();
    }
}

/// UDisks2's signals and its owner's changes, into `inputs`.
async fn listen(connection: &Connection, inputs: UnboundedSender<Input>) -> zbus::Result<()> {
    let objects = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(OBJECT_MANAGER)
        .path(ROOT)
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
        .receive_name_owner_changed_with_args(&[(0, UDISKS)])
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

/// What an action on a drive needs of UDisks2's objects.
#[derive(Debug, Clone)]
struct Facts {
    drive: Drive,
    /// The block whose filesystem is mounted: the counted block, or its
    /// container's cleartext; none while locked.
    filesystem: Option<String>,
    /// The encrypted container, while unlocked (locked again on eject).
    container: Option<String>,
    /// The drive's object, and whether it can be powered off.
    holder: String,
    can_power_off: bool,
}

struct Client {
    connection: Connection,
    /// Every object under UDisks2's path: its interfaces and properties.
    objects: BTreeMap<String, Interfaces>,
    inputs: UnboundedSender<Input>,
    publish: Publish,
    /// Each counted block's serial, kept while it is there.
    serials: HashMap<String, u32>,
    next: u32,
    /// The serials counted as last published: one that comes is plugged.
    present: HashSet<u32>,
    /// UDisks2 was just read: what it has is not plugged now.
    fresh: bool,
    last: Option<Vec<Drive>>,
    /// The drives an action runs for: another is not started meanwhile.
    running: HashSet<u32>,
    /// Tests': the free space of every mounted drive.
    free: Option<u64>,
}

impl Client {
    /// Acts on `input`; `false` once the program ends.
    async fn input(&mut self, input: Input) -> bool {
        match input {
            Input::Signal(message) => self.signal(&message),
            Input::Owner(owned) => self.owner(owned).await,
            Input::Ask(Ask::Quit(done)) => {
                let _ = done.send(());
                return false;
            }
            Input::Ask(ask) => self.ask(ask),
            Input::Ended(serial, news) => {
                self.running.remove(&serial);
                tracing::debug!(serial, ?news, "a drive's action ended");
                (self.publish)(news);
            }
            Input::Tick => {}
        }
        true
    }

    /// UDisks2 came (read it all) or went (forget it).
    async fn owner(&mut self, owned: bool) {
        self.objects.clear();
        self.fresh = true;
        if !owned {
            tracing::info!("removable drives: UDisks2 stopped");
            return;
        }
        let reply = self
            .connection
            .call_method(
                Some(UDISKS),
                ROOT,
                Some(OBJECT_MANAGER),
                "GetManagedObjects",
                &(),
            )
            .await;
        let objects = reply.and_then(|r| r.body().deserialize::<Objects>());
        match objects {
            Ok(objects) => {
                for (path, interfaces) in objects {
                    self.added(path.as_str(), interfaces);
                }
                tracing::info!(
                    objects = self.objects.len(),
                    "removable drives: UDisks2 read"
                );
            }
            Err(err) => {
                tracing::info!(%err, "removable drives: no UDisks2; followed when it comes");
            }
        }
    }

    fn added(&mut self, path: &str, interfaces: Interfaces) {
        if !path.starts_with(ROOT) {
            return;
        }
        self.objects
            .entry(path.to_owned())
            .or_default()
            .extend(interfaces);
    }

    fn signal(&mut self, message: &Message) {
        let header = message.header();
        let (Some(at), Some(interface), Some(member)) =
            (header.path(), header.interface(), header.member())
        else {
            return;
        };
        let at = at.as_str().to_owned();
        let body = message.body();
        match (interface.as_str(), member.as_str()) {
            (OBJECT_MANAGER, "InterfacesAdded") => {
                if let Ok((path, interfaces)) = body.deserialize::<(OwnedObjectPath, Interfaces)>()
                {
                    self.added(path.as_str(), interfaces);
                }
            }
            (OBJECT_MANAGER, "InterfacesRemoved") => {
                if let Ok((path, gone)) = body.deserialize::<(OwnedObjectPath, Vec<String>)>()
                    && let Some(object) = self.objects.get_mut(path.as_str())
                {
                    for interface in gone {
                        object.remove(&interface);
                    }
                    if object.is_empty() {
                        self.objects.remove(path.as_str());
                    }
                }
            }
            (PROPERTIES, "PropertiesChanged") => {
                if let Ok((iface, changed, gone)) =
                    body.deserialize::<(String, Props, Vec<String>)>()
                    && let Some(props) = self
                        .objects
                        .get_mut(&at)
                        .and_then(|object| object.get_mut(&iface))
                {
                    props.extend(changed);
                    for name in gone {
                        props.remove(&name);
                    }
                }
            }
            _ => {}
        }
    }

    /// The drives that count now, in the order they came, and what acting
    /// on each needs.
    fn facts(&mut self) -> Vec<Facts> {
        let mut found = Vec::new();
        for (at, block) in &self.objects {
            let Some(props) = block.get(BLOCK) else {
                continue;
            };
            let holder = path(props, "Drive");
            let drive = holder
                .as_deref()
                .and_then(|h| self.objects.get(h))
                .and_then(|d| d.get(DRIVE));
            if !counts(block, drive) {
                continue;
            }
            let (Some(holder), Some(drive)) = (holder, drive) else {
                continue;
            };
            // An encrypted container's filesystem is its cleartext's.
            let encrypted = block.contains_key(ENCRYPTED);
            let cleartext = block
                .get(ENCRYPTED)
                .and_then(|e| path(e, "CleartextDevice"))
                .filter(|c| self.objects.contains_key(c));
            let filesystem = match &cleartext {
                Some(clear) => Some(clear.clone()),
                None if block.contains_key(FILESYSTEM) => Some(at.clone()),
                None => None,
            };
            let fs = filesystem.as_ref().and_then(|f| self.objects.get(f));
            let label = fs
                .and_then(|o| o.get(BLOCK))
                .and_then(|b| get::<String>(b, "IdLabel"))
                .filter(|l| !l.trim().is_empty())
                .or_else(|| get::<String>(props, "IdLabel"))
                .unwrap_or_default();
            let mount = fs.and_then(|o| o.get(FILESYSTEM)).and_then(mount_point);
            found.push((
                at.clone(),
                Facts {
                    drive: Drive {
                        serial: 0,
                        name: name(&label, drive),
                        kind: kind(drive),
                        size: get::<u64>(props, "Size").unwrap_or(0),
                        free: None,
                        mount,
                        encrypted,
                    },
                    filesystem,
                    container: cleartext.is_some().then(|| at.clone()),
                    holder,
                    can_power_off: get::<bool>(drive, "CanPowerOff").unwrap_or(false),
                },
            ));
        }
        // Serials: kept for those still there, the next for newcomers.
        let here: HashSet<&String> = found.iter().map(|(p, _)| p).collect();
        self.serials.retain(|p, _| here.contains(p));
        let mut facts: Vec<Facts> = found
            .into_iter()
            .map(|(path, mut facts)| {
                let next = &mut self.next;
                let serial = *self.serials.entry(path).or_insert_with(|| {
                    *next += 1;
                    *next - 1
                });
                facts.drive.serial = serial;
                facts.drive.free = facts
                    .drive
                    .mount
                    .as_deref()
                    .and_then(|m| self.free.or_else(|| free_in(m)));
                facts
            })
            .collect();
        facts.sort_by_key(|f| f.drive.serial);
        facts
    }

    /// The drives as they are now, if they changed, and those plugged in.
    fn publish(&mut self) {
        let drives: Vec<Drive> = self.facts().into_iter().map(|f| f.drive).collect();
        let now: HashSet<u32> = drives.iter().map(|d| d.serial).collect();
        if self.last.as_ref() != Some(&drives) {
            tracing::debug!(?drives, "removable drives");
            (self.publish)(News::Drives(drives.clone()));
        }
        if !self.fresh {
            for drive in drives.iter().filter(|d| !self.present.contains(&d.serial)) {
                tracing::info!(
                    serial = drive.serial,
                    name = drive.name,
                    "a drive plugged in"
                );
                (self.publish)(News::Plugged(drive.clone()));
            }
        }
        self.fresh = false;
        self.present = now;
        self.last = Some(drives);
    }

    fn ask(&mut self, ask: Ask) {
        let (serial, doing) = match ask {
            Ask::Open(serial) => (serial, Doing::Open),
            Ask::Eject(serial) => (serial, Doing::Eject),
            Ask::Quit(_) => return,
        };
        let facts = self.facts();
        let Some(this) = facts.iter().find(|f| f.drive.serial == serial).cloned() else {
            tracing::debug!(serial, "a drive no longer there: nothing asked");
            return;
        };
        if !self.running.insert(serial) {
            tracing::debug!(serial, "a drive's action runs already");
            return;
        }
        let connection = self.connection.clone();
        let inputs = self.inputs.clone();
        // Ejecting: every filesystem of the same drive.
        let siblings: Vec<Facts> = facts
            .into_iter()
            .filter(|f| f.holder == this.holder)
            .collect();
        tracing::info!(serial, ?doing, name = this.drive.name, "a drive's action");
        tokio::spawn(async move {
            let news = match doing {
                Doing::Open => open(&connection, this).await,
                Doing::Eject => eject(&connection, this, siblings).await,
            };
            let _ = inputs.send(Input::Ended(serial, news));
        });
    }
}

/// Bytes free on the filesystem mounted at `mount`.
fn free_in(mount: &Path) -> Option<u64> {
    let stat = rustix::fs::statvfs(mount).ok()?;
    Some(stat.f_bavail.saturating_mul(stat.f_frsize))
}

/// No options: `a{sv}` empty.
fn none() -> HashMap<&'static str, Variant<'static>> {
    HashMap::new()
}

async fn call(
    connection: &Connection,
    path: &str,
    interface: &str,
    method: &str,
) -> zbus::Result<Message> {
    let options = (none(),);
    let call = connection.call_method(Some(UDISKS), path, Some(interface), method, &options);
    match tokio::time::timeout(CALL, call).await {
        Ok(reply) => reply,
        Err(_) => Err(zbus::Error::Failure(format!("{method}: no answer"))),
    }
}

fn busy(err: &zbus::Error) -> bool {
    matches!(err, zbus::Error::MethodError(name, ..) if name.as_str() == BUSY)
}

/// Mounted if it is not, then where.
async fn open(connection: &Connection, facts: Facts) -> News {
    if let Some(at) = facts.drive.mount.clone() {
        return News::Opened {
            drive: facts.drive,
            at,
        };
    }
    let Some(filesystem) = facts.filesystem.as_deref() else {
        return News::Locked(facts.drive);
    };
    let mounted = call(connection, filesystem, FILESYSTEM, "Mount")
        .await
        .and_then(|reply| reply.body().deserialize::<String>());
    match mounted {
        Ok(at) => News::Opened {
            drive: facts.drive,
            at: PathBuf::from(at),
        },
        Err(err) => {
            tracing::warn!(%err, name = facts.drive.name, "a drive not mounted");
            News::Failed {
                drive: facts.drive,
                doing: Doing::Open,
            }
        }
    }
}

/// Every filesystem of its drive unmounted, an unlocked container locked,
/// then the drive powered off.
async fn eject(connection: &Connection, facts: Facts, siblings: Vec<Facts>) -> News {
    for sibling in &siblings {
        let mut steps: Vec<(&str, &str, &str)> = Vec::new();
        if sibling.drive.mounted()
            && let Some(filesystem) = sibling.filesystem.as_deref()
        {
            steps.push((filesystem, FILESYSTEM, "Unmount"));
        }
        if let Some(container) = sibling.container.as_deref() {
            steps.push((container, ENCRYPTED, "Lock"));
        }
        for (path, interface, method) in steps {
            if let Err(err) = call(connection, path, interface, method).await {
                tracing::info!(%err, name = sibling.drive.name, method, "a drive not ejected");
                return if busy(&err) {
                    News::Busy(facts.drive)
                } else {
                    News::Failed {
                        drive: facts.drive,
                        doing: Doing::Eject,
                    }
                };
            }
        }
    }
    // Unmounted, it can be pulled out already: a power off refused (by
    // polkit, by the drive) changes nothing of that.
    if facts.can_power_off
        && let Err(err) = call(connection, &facts.holder, DRIVE, "PowerOff").await
    {
        tracing::info!(%err, name = facts.drive.name, "a drive not powered off");
    }
    News::Ejected(facts.drive)
}
