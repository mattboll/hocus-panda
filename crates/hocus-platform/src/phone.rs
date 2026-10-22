//! The phone (#85), through KDE Connect's daemon on the session bus: the
//! phones it has paired and reaches, their battery, the notifications they
//! post, and what can be asked of them (ring, a reply, the clipboard,
//! files, browsing their files). Only its D-Bus interface is used.
//!
//! The daemon is followed, never started: a session starts it from its
//! autostart entry, with the session's environment (its clipboard needs to
//! reach the compositor), which bus activation would not give it. Without
//! it nothing is shown and nothing is said but at debug level.
//!
//! Everything here runs on threads of its own; the compositor gets
//! [`PhoneEvent`]s and asks through [`Phone::ask`].

use std::{
    future::poll_fn,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    task::Poll,
};

use futures_core::Stream;
use zbus::{Connection, MessageStream, Proxy, fdo::DBusProxy, message::Type, names::BusName};

/// The daemon's name on the session bus.
pub const SERVICE: &str = "org.kde.kdeconnect";
const ROOT: &str = "/modules/kdeconnect";
const DAEMON: &str = "org.kde.kdeconnect.daemon";
const DEVICE: &str = "org.kde.kdeconnect.device";

/// Whether KDE Connect is there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// Neither running nor installed for the bus to start.
    Absent,
    /// Installed, not running.
    Stopped,
    Running,
}

impl Presence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Stopped => "stopped",
            Self::Running => "running",
        }
    }
}

/// A phone paired and within reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// Percent, when its battery is shared.
    pub battery: Option<u8>,
    pub charging: bool,
}

/// A notification a phone posted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub device: String,
    /// KDE Connect's public id for it.
    pub id: String,
    pub app: String,
    pub title: String,
    pub text: String,
    /// What a reply is sent to, when the application takes replies.
    pub reply_id: Option<String>,
}

/// What the compositor can ask of a phone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Ring,
    Reply {
        reply_id: String,
        text: String,
    },
    /// Text for the phone's clipboard; lines that all name files (paths
    /// or `file://` URLs, as file managers copy them) send the files.
    Clipboard(String),
    /// Files to send, by absolute path.
    Share(Vec<PathBuf>),
    /// Mounts its files, to be opened where they are.
    Browse,
}

/// How a request went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Done,
    /// Files were sent: how many.
    Sent(usize),
    /// Its files are there.
    Mounted(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhoneEvent {
    Presence(Presence),
    /// The phones paired and reachable, by name.
    Devices(Vec<Device>),
    Posted(Notification),
    /// A notification shown changed (a progress, a conversation).
    Updated(Notification),
    Removed {
        device: String,
        id: String,
    },
    AllRemoved {
        device: String,
    },
    /// A request was carried out, or why not.
    Answered {
        device: String,
        request: Request,
        outcome: Result<Outcome, String>,
    },
}

type Sink = Arc<dyn Fn(PhoneEvent) + Send + Sync>;

/// KDE Connect, followed on a thread of its own.
pub struct Phone {
    bus: String,
    sink: Sink,
}

impl std::fmt::Debug for Phone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Phone")
    }
}

impl Phone {
    /// Follows KDE Connect on the session bus at `bus`.
    pub fn start(
        bus: String,
        sink: impl Fn(PhoneEvent) + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        let sink: Sink = Arc::new(sink);
        let (address, events) = (bus.clone(), sink.clone());
        std::thread::Builder::new()
            .name("phone".into())
            .spawn(move || {
                if let Err(err) = zbus::block_on(follow(&address, &events)) {
                    // Not a fault: a bus that went, a daemon that did not
                    // answer.
                    tracing::debug!(%err, "the phone is not followed");
                    events(PhoneEvent::Presence(Presence::Absent));
                }
            })?;
        Ok(Self { bus, sink })
    }

    /// Asks `device` for `request`, on a thread of its own: a phone slow to
    /// answer holds only it. The answer comes as [`PhoneEvent::Answered`].
    pub fn ask(&self, device: String, request: Request) {
        let (bus, sink) = (self.bus.clone(), self.sink.clone());
        let spawned = std::thread::Builder::new()
            .name("phone request".into())
            .spawn(move || {
                let outcome =
                    zbus::block_on(carry_out(&bus, &device, &request)).map_err(|e| e.to_string());
                sink(PhoneEvent::Answered {
                    device,
                    request,
                    outcome,
                });
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "could not ask the phone");
        }
    }
}

fn device_path(device: &str) -> String {
    format!("{ROOT}/devices/{device}")
}

/// A proxy that reads properties each time: KDE Connect says they changed
/// with signals of its own, not PropertiesChanged, so a cache would keep
/// old values.
async fn proxy(
    conn: &Connection,
    path: String,
    iface: &'static str,
) -> zbus::Result<Proxy<'static>> {
    zbus::proxy::Builder::new(conn)
        .destination(SERVICE)
        .path(path)
        .interface(iface)
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
}

/// The phones paired and within reach, by name.
async fn devices(conn: &Connection) -> zbus::Result<Vec<Device>> {
    let daemon = proxy(conn, ROOT.into(), DAEMON).await?;
    let ids: Vec<String> = daemon.call("devices", &(true, true)).await?;
    let mut out = Vec::new();
    for id in ids {
        let device = proxy(conn, device_path(&id), DEVICE).await?;
        let name: String = device.get_property("name").await.unwrap_or_default();
        // Without the battery plugin, no battery.
        let battery = proxy(
            conn,
            format!("{}/battery", device_path(&id)),
            "org.kde.kdeconnect.device.battery",
        )
        .await?;
        let charge: Option<i32> = battery.get_property("charge").await.ok();
        let charging: bool = battery.get_property("isCharging").await.unwrap_or(false);
        out.push(Device {
            name: if name.is_empty() { id.clone() } else { name },
            id,
            battery: charge.filter(|c| *c >= 0).map(|c| c.clamp(0, 100) as u8),
            charging,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    Ok(out)
}

/// A notification by its public id; `None` for a silent one (music,
/// downloads), which the phone does not ring for either.
async fn notification(
    conn: &Connection,
    device: &str,
    id: &str,
) -> zbus::Result<Option<Notification>> {
    let n = proxy(
        conn,
        format!("{}/notifications/{id}", device_path(device)),
        "org.kde.kdeconnect.device.notifications.notification",
    )
    .await?;
    if n.get_property::<bool>("silent").await.unwrap_or(false) {
        return Ok(None);
    }
    let text = |name: &'static str| {
        let n = n.clone();
        async move { n.get_property::<String>(name).await.unwrap_or_default() }
    };
    let title = text("title").await;
    let reply_id = text("replyId").await;
    Ok(Some(Notification {
        device: device.to_owned(),
        id: id.to_owned(),
        app: text("appName").await,
        title: if title.is_empty() {
            text("ticker").await
        } else {
            title
        },
        text: text("text").await,
        reply_id: (!reply_id.is_empty()).then_some(reply_id),
    }))
}

enum Either<A, B> {
    Left(A),
    Right(B),
}

/// Follows the daemon: whether it runs, its phones, their notifications.
async fn follow(bus: &str, sink: &Sink) -> zbus::Result<()> {
    let conn = zbus::connection::Builder::address(bus).build().await?;
    let dbus = DBusProxy::new(&conn).await?;
    let mut owners = dbus
        .receive_name_owner_changed_with_args(&[(0, SERVICE)])
        .await?;
    let rule = zbus::MatchRule::builder()
        .msg_type(Type::Signal)
        .sender(SERVICE)
        .path_namespace(ROOT)
        .build()?;
    let mut signals = MessageStream::for_match_rule(rule, &conn, None).await?;
    let name = BusName::try_from(SERVICE)?;
    let running = dbus.name_has_owner(name.clone()).await?;
    let installed = running
        || dbus
            .list_activatable_names()
            .await?
            .iter()
            .any(|n| n.as_str() == SERVICE);
    let mut shown: Vec<Device> = Vec::new();
    let mut presence = match (running, installed) {
        (true, _) => Presence::Running,
        (false, true) => Presence::Stopped,
        (false, false) => Presence::Absent,
    };
    tracing::debug!(presence = presence.as_str(), "KDE Connect");
    sink(PhoneEvent::Presence(presence));
    let refresh = |shown: &mut Vec<Device>, now: Vec<Device>| {
        if *shown != now {
            shown.clone_from(&now);
            sink(PhoneEvent::Devices(now));
        }
    };
    if running {
        match devices(&conn).await {
            Ok(now) => refresh(&mut shown, now),
            Err(err) => tracing::debug!(%err, "KDE Connect did not list its phones"),
        }
    }
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
                let came = change.args().is_ok_and(|a| a.new_owner().is_some());
                presence = if came {
                    Presence::Running
                } else if presence == Presence::Absent {
                    Presence::Absent
                } else {
                    Presence::Stopped
                };
                sink(PhoneEvent::Presence(presence));
                let now = if came {
                    devices(&conn).await.unwrap_or_default()
                } else {
                    Vec::new()
                };
                refresh(&mut shown, now);
            }
            Some(Either::Right(Ok(message))) => {
                let header = message.header();
                let (Some(path), Some(member)) = (header.path(), header.member()) else {
                    continue;
                };
                let path = path.as_str().to_owned();
                let member = member.as_str().to_owned();
                // `/modules/kdeconnect/devices/ID/notifications`.
                let device = path
                    .strip_prefix(&format!("{ROOT}/devices/"))
                    .and_then(|rest| rest.strip_suffix("/notifications"))
                    .map(str::to_owned);
                match (device, member.as_str()) {
                    (Some(device), "notificationPosted" | "notificationUpdated") => {
                        let Ok(id) = message.body().deserialize::<String>() else {
                            continue;
                        };
                        match notification(&conn, &device, &id).await {
                            Ok(Some(n)) if member == "notificationPosted" => {
                                sink(PhoneEvent::Posted(n));
                            }
                            Ok(Some(n)) => sink(PhoneEvent::Updated(n)),
                            Ok(None) => {}
                            Err(err) => tracing::debug!(%err, "a phone notification did not read"),
                        }
                    }
                    (Some(device), "notificationRemoved") => {
                        if let Ok(id) = message.body().deserialize::<String>() {
                            sink(PhoneEvent::Removed { device, id });
                        }
                    }
                    (Some(device), "allNotificationsRemoved") => {
                        sink(PhoneEvent::AllRemoved { device });
                    }
                    // Anything else (a phone came or went, paired, its
                    // battery, its name): the phones are read again.
                    _ => match devices(&conn).await {
                        Ok(now) => refresh(&mut shown, now),
                        Err(err) => tracing::debug!(%err, "KDE Connect did not list its phones"),
                    },
                }
            }
            Some(Either::Right(Err(err))) => tracing::debug!(%err, "a KDE Connect signal"),
        }
    }
}

/// Carries out `request` on `device`, with a connection of its own.
async fn carry_out(bus: &str, device: &str, request: &Request) -> zbus::Result<Outcome> {
    let conn = zbus::connection::Builder::address(bus).build().await?;
    let path = device_path(device);
    let plugin = |name: &'static str, iface: &'static str| {
        let conn = conn.clone();
        let path = format!("{path}/{name}");
        async move { proxy(&conn, path, iface).await }
    };
    match request {
        Request::Ring => {
            plugin("findmyphone", "org.kde.kdeconnect.device.findmyphone")
                .await?
                .call::<_, _, ()>("ring", &())
                .await?;
            Ok(Outcome::Done)
        }
        Request::Reply { reply_id, text } => {
            plugin("notifications", "org.kde.kdeconnect.device.notifications")
                .await?
                .call::<_, _, ()>("sendReply", &(reply_id.as_str(), text.as_str()))
                .await?;
            Ok(Outcome::Done)
        }
        Request::Clipboard(text) => match files_in(text) {
            Some(files) => share(plugin("share", SHARE).await?, &files).await,
            None => {
                let clipboard = plugin("clipboard", "org.kde.kdeconnect.device.clipboard").await?;
                // Older daemons send their own clipboard only.
                match clipboard
                    .call::<_, _, ()>("sendClipboard", &(text.as_str(),))
                    .await
                {
                    Err(zbus::Error::MethodError(name, _, _))
                        if name.as_str() == "org.freedesktop.DBus.Error.UnknownMethod" =>
                    {
                        clipboard.call::<_, _, ()>("sendClipboard", &()).await?;
                    }
                    other => other?,
                }
                Ok(Outcome::Done)
            }
        },
        Request::Share(files) => share(plugin("share", SHARE).await?, files).await,
        Request::Browse => {
            let sftp = plugin("sftp", "org.kde.kdeconnect.device.sftp").await?;
            let mounted: bool = sftp.call("mountAndWait", &()).await?;
            let at: String = sftp.call("mountPoint", &()).await?;
            if !mounted || at.is_empty() {
                return Err(zbus::Error::Failure(
                    "its files could not be mounted".into(),
                ));
            }
            Ok(Outcome::Mounted(PathBuf::from(at)))
        }
    }
}

const SHARE: &str = "org.kde.kdeconnect.device.share";

async fn share(share: Proxy<'static>, files: &[PathBuf]) -> zbus::Result<Outcome> {
    for file in files {
        share
            .call::<_, _, ()>("shareUrl", &(file_url(file).as_str(),))
            .await?;
    }
    Ok(Outcome::Sent(files.len()))
}

/// The files `text` names, when every line names one that exists.
pub fn files_in(text: &str) -> Option<Vec<PathBuf>> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }
    lines
        .iter()
        .map(|line| {
            let path = match line.strip_prefix("file://") {
                Some(rest) => PathBuf::from(percent_decoded(rest)?),
                None => PathBuf::from(line),
            };
            (path.is_absolute() && path.is_file()).then_some(path)
        })
        .collect()
}

fn percent_decoded(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// `file://` and the path, with what URLs do not hold escaped.
pub fn file_url(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut url = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            url.push(char::from(b));
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_url_escapes_what_it_must() {
        assert_eq!(
            file_url(Path::new("/home/alex/Été 1.png")),
            "file:///home/alex/%C3%89t%C3%A9%201.png"
        );
        assert_eq!(
            percent_decoded("/home/alex/%C3%89t%C3%A9%201.png").as_deref(),
            Some("/home/alex/Été 1.png")
        );
    }

    #[test]
    fn copied_files_are_sent_as_files_and_text_as_text() {
        let dir = tempfile::tempdir().expect("a directory");
        let a = dir.path().join("a photo.jpg");
        let b = dir.path().join("b.pdf");
        std::fs::write(&a, b"jpeg").expect("written");
        std::fs::write(&b, b"pdf").expect("written");
        let copied = format!("{}\n{}\n", file_url(&a), b.display());
        assert_eq!(files_in(&copied), Some(vec![a.clone(), b]));
        assert_eq!(files_in("See you at 6"), None);
        assert_eq!(files_in(&format!("{}\nand some text", a.display())), None);
        assert_eq!(files_in("/no/such/file"), None);
        assert_eq!(files_in(""), None);
    }
}
