//! `org.freedesktop.Notifications` on the session bus, on a thread of its
//! own (the Desktop Notifications Specification 1.2): `Notify`,
//! `CloseNotification`, `GetCapabilities`, `GetServerInformation`, and the
//! `NotificationClosed` and `ActionInvoked` signals.
//!
//! A notification is read here (its hints, its icon, the process that sent
//! it and whether it is an agents' tool's) and handed, owned, to the
//! bubbles' stack ([`super::input`]); what the stack says back (closed,
//! acted on) comes as [`Signal`]s, emitted from here. Nothing here waits
//! for the shell's drawing, nor it for the bus.
//!
//! The bus is the one the compositor names ([`NOTIFICATIONS_BUS_ENV`]):
//! an address given with `--session-bus`, or `session`, the session's own.
//! Without it, nothing is served: a nested or headless desktop never takes
//! the live session's notifications.

use std::{
    collections::{BTreeSet, HashMap},
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU32, Ordering},
    },
};

use hocus_link::NOTIFICATIONS_BUS_ENV;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use zbus::{
    Connection, OwnedVariant, Variant, connection::Builder, fdo::RequestNameFlags, message::Header,
    names::BusName,
};

use super::{
    Input,
    note::{self, IconFrom, Note, Sender, Timeout, Urgency},
};
use crate::{
    launcher::{
        entries,
        icons::{self, Themes},
    },
    levels::icons::Icon,
};

const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

/// What the stack says back, for the signals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// Closed: 1 expired, 2 dismissed by the user, 3 closed by
    /// `CloseNotification`.
    Closed { id: u32, reason: u32 },
    /// One of its actions was invoked (`default`: the card).
    Action { id: u32, key: String },
}

/// Where the signals go: the server's thread.
static SIGNALS: Mutex<Option<UnboundedSender<Signal>>> = Mutex::new(None);
/// The notifications not closed yet: a `replaces_id` among them keeps its
/// id.
static LIVE: Mutex<BTreeSet<u32>> = Mutex::new(BTreeSet::new());
static NEXT: AtomicU32 = AtomicU32::new(1);

/// The stack says `signal`.
pub fn signal(signal: Signal) {
    if let Some(sender) = SIGNALS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        let _ = sender.send(signal);
    }
}

/// Serves the notifications on the bus the compositor names, if it names
/// one: whether it does.
pub fn start() -> bool {
    let Some(bus) = std::env::var(NOTIFICATIONS_BUS_ENV)
        .ok()
        .filter(|b| !b.is_empty())
    else {
        tracing::info!("no session bus given: no notifications served");
        return false;
    };
    let (sender, signals) = unbounded_channel();
    *SIGNALS.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);
    let started = std::thread::Builder::new()
        .name("notifications: session bus".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(run(bus, signals)),
                Err(err) => tracing::warn!(%err, "no runtime for the notifications"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot serve the notifications");
        return false;
    }
    true
}

async fn run(bus: String, mut signals: UnboundedReceiver<Signal>) {
    let builder = if bus == "session" {
        Builder::session()
    } else {
        Builder::address(bus.as_str())
    };
    let connection = match builder.serve_at(PATH, Server).build().await {
        Ok(connection) => connection,
        Err(err) => {
            tracing::warn!(%err, "cannot reach the session bus: no notifications");
            return;
        }
    };
    // Its owner then, or in the queue for it, taking it from another that
    // lets it go: the desktop's own server comes first in its session.
    match connection
        .request_name_with_flags(NAME, RequestNameFlags::REPLACE_EXISTING)
        .await
    {
        Ok(reply) => tracing::info!(?reply, "the notifications are served"),
        Err(err) => {
            tracing::warn!(%err, "cannot own {NAME}: no notifications");
            return;
        }
    }
    while let Some(signal) = signals.recv().await {
        let sent = match &signal {
            Signal::Closed { id, reason } => {
                LIVE.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(id);
                tracing::debug!(id, reason, "a notification closed");
                connection
                    .emit_signal(
                        None::<BusName<'_>>,
                        PATH,
                        NAME,
                        "NotificationClosed",
                        &(*id, *reason),
                    )
                    .await
            }
            Signal::Action { id, key } => {
                tracing::debug!(id, key, "a notification's action");
                connection
                    .emit_signal(
                        None::<BusName<'_>>,
                        PATH,
                        NAME,
                        "ActionInvoked",
                        &(*id, key.as_str()),
                    )
                    .await
            }
        };
        if let Err(err) = sent {
            tracing::warn!(%err, ?signal, "a notification's signal not sent");
        }
    }
}

struct Server;

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedVariant>,
        expire_timeout: i32,
    ) -> u32 {
        let id = {
            let mut live = LIVE.lock().unwrap_or_else(PoisonError::into_inner);
            let id = if replaces_id != 0 && live.contains(&replaces_id) {
                replaces_id
            } else {
                NEXT.fetch_add(1, Ordering::Relaxed)
            };
            live.insert(id);
            id
        };
        let pid = match header.sender() {
            Some(sender) => sender_pid(connection, sender.as_str()).await,
            None => 0,
        };
        let urgency = Urgency::of(hints.get("urgency").and_then(byte));
        let entry = hints
            .get("desktop-entry")
            .and_then(|v| String::try_from(&**v).ok())
            .map(|e| e.trim_end_matches(".desktop").to_owned())
            .filter(|e| !e.is_empty());
        let (buttons, default) = note::actions(&actions);
        let body = note::plain(&body);
        let image = hints
            .get("image-data")
            .or_else(|| hints.get("image_data"))
            .or_else(|| hints.get("icon_data"))
            .and_then(image_data);
        let image_path = hints
            .get("image-path")
            .or_else(|| hints.get("image_path"))
            .and_then(|v| String::try_from(&**v).ok());
        let icon_from = IconFrom {
            app_icon: app_icon.clone(),
            image_path: image_path.clone(),
        };
        let looked = Looked {
            app_icon,
            image_path,
            image,
            entry: entry.clone(),
            app: app_name.clone(),
            scale: screens_scale(),
        };
        let found = tokio::task::spawn_blocking(move || {
            let icon = icon_of(looked);
            (icon, note::programs_above(pid))
        })
        .await;
        let (icon, programs) = found.unwrap_or_default();
        let said = crate::agents::said();
        let agent = note::agent_state(
            &Sender {
                app: &app_name,
                entry: entry.as_deref(),
                summary: &summary,
                body: &body,
                programs: &programs,
            },
            |tool| said.state(tool.word()).followed(),
        );
        let progress = hints
            .get("value")
            .and_then(number)
            .map(|v| v.clamp(0, 100) as u8);
        let resident = hints
            .get("resident")
            .and_then(|v| bool::try_from(&**v).ok())
            .unwrap_or(false);
        let note = Note {
            id,
            app: app_name,
            entry,
            icon,
            summary,
            body,
            actions: buttons,
            default,
            urgency,
            timeout: Timeout::of(expire_timeout, urgency),
            progress,
            resident,
            pid,
            agent,
            icon_from,
            at: super::wall_ms(),
        };
        tracing::debug!(
            id,
            replaces = replaces_id,
            app = note.app,
            urgency = ?note.urgency,
            agent,
            "a notification"
        );
        super::input(Input::Notify(Box::new(note)));
        id
    }

    async fn close_notification(&self, id: u32) {
        super::input(Input::Close(id));
    }

    fn get_capabilities(&self) -> Vec<String> {
        [
            "body",
            "body-markup",
            "actions",
            "persistence",
            "icon-static",
        ]
        .map(str::to_owned)
        .into()
    }

    #[zbus(out_args("name", "vendor", "version", "spec_version"))]
    fn get_server_information(&self) -> (String, String, String, String) {
        (
            "Hocuspanda".to_owned(),
            "Hocuspanda".to_owned(),
            env!("CARGO_PKG_VERSION").to_owned(),
            "1.2".to_owned(),
        )
    }
}

/// The process of the bus's connection `sender` (0: not known).
async fn sender_pid(connection: &Connection, sender: &str) -> u32 {
    let Ok(name) = BusName::try_from(sender) else {
        return 0;
    };
    match zbus::fdo::DBusProxy::new(connection).await {
        Ok(bus) => bus
            .get_connection_unix_process_id(name)
            .await
            .unwrap_or_default(),
        Err(_) => 0,
    }
}

/// A byte hint, as some applications send it in a wider integer.
fn byte(value: &OwnedVariant) -> Option<u8> {
    number(value).and_then(|n| u8::try_from(n).ok())
}

/// An integer hint, whatever its width.
fn number(value: &OwnedVariant) -> Option<i64> {
    match &**value {
        Variant::U8(n) => Some(i64::from(*n)),
        Variant::I16(n) => Some(i64::from(*n)),
        Variant::U16(n) => Some(i64::from(*n)),
        Variant::I32(n) => Some(i64::from(*n)),
        Variant::U32(n) => Some(i64::from(*n)),
        Variant::I64(n) => Some(*n),
        Variant::U64(n) => i64::try_from(*n).ok(),
        Variant::Variant(inner) => OwnedVariant::try_from(&**inner)
            .ok()
            .and_then(|v| number(&v)),
        _ => None,
    }
}

/// The `image-data` hint, `(iiibiiay)`: width, height, row stride, alpha,
/// bits per sample, channels, the pixels; made RGBA.
fn image_data(value: &OwnedVariant) -> Option<Icon> {
    let Variant::Structure(image) = &**value else {
        return None;
    };
    let fields = image.fields();
    let int = |i: usize| match fields.get(i) {
        Some(Variant::I32(n)) => usize::try_from(*n).ok(),
        _ => None,
    };
    let (width, height, stride, channels) = (int(0)?, int(1)?, int(2)?, int(5)?);
    if int(4)? != 8 || !(3..=4).contains(&channels) || width == 0 || height == 0 {
        return None;
    }
    if width > 1024 || height > 1024 {
        return None;
    }
    let Some(Variant::Array(pixels)) = fields.get(6) else {
        return None;
    };
    let bytes: Vec<u8> = pixels
        .inner()
        .iter()
        .map(|b| match b {
            Variant::U8(b) => Some(*b),
            _ => None,
        })
        .collect::<Option<_>>()?;
    let mut rgba = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        let line = bytes.get(row * stride..row * stride + width * channels)?;
        for pixel in line.chunks(channels) {
            rgba.extend_from_slice(&pixel[..3]);
            rgba.push(if channels == 4 { pixel[3] } else { 255 });
        }
    }
    Some(Icon::Raster(iced::advanced::image::Handle::from_rgba(
        u32::try_from(width).ok()?,
        u32::try_from(height).ok()?,
        rgba,
    )))
}

/// What a notification's icon is looked for by.
pub(super) struct Looked {
    pub app_icon: String,
    pub image_path: Option<String>,
    pub image: Option<Icon>,
    pub entry: Option<String>,
    pub app: String,
    /// The bubbles' screens' largest scale.
    pub scale: u32,
}

/// The largest scale of the bars' screens: the icon is drawn for it.
pub(super) fn screens_scale() -> u32 {
    let bars = crate::popover::with(|c| c.bar_forms());
    bars.iter()
        .map(|(bar, _)| crate::popover::kit::scale(bar))
        .fold(1.0_f32, f32::max)
        .ceil() as u32
}

/// The icon themes, as the launcher reads them, once.
static THEMES: Mutex<Option<Themes>> = Mutex::new(None);

/// Icon `text` (a path, a `file://` address or an icon's name) at the
/// card's size, found as the launcher finds its icons (5.1).
fn icon_named(text: &str, scale: u32) -> Option<Icon> {
    let text = text.trim();
    let text = text
        .strip_prefix("file://")
        .unwrap_or(text)
        .replace("%20", " ");
    if text.is_empty() {
        return None;
    }
    let px = icons::pixels(super::draw::ICON as u32, scale as f32);
    let file = if text.starts_with('/') {
        std::path::PathBuf::from(&text)
    } else {
        let mut themes = THEMES.lock().unwrap_or_else(PoisonError::into_inner);
        let themes = themes.get_or_insert_with(|| Themes::load(&entries::data_dirs()));
        themes.find(&text, super::draw::ICON as u32, scale)?
    };
    icons::load(&file, px).map(Icon::Raster)
}

/// A notification's icon: its application's icon (`app_icon`), else its
/// image (`image-path`, `image-data`), else its desktop entry's icon (or
/// the icon named as the entry), else
/// the icon of the application whose name is its application's, among
/// those the launcher lists. Read off the drawing thread.
pub(super) fn icon_of(looked: Looked) -> Option<Icon> {
    let scale = looked.scale.max(1);
    if let Some(icon) = icon_named(&looked.app_icon, scale) {
        return Some(icon);
    }
    if let Some(icon) = looked
        .image_path
        .as_deref()
        .and_then(|p| icon_named(p, scale))
    {
        return Some(icon);
    }
    if looked.image.is_some() {
        return looked.image;
    }
    let catalog = crate::launcher::apps::catalog();
    let entries = || catalog.apps.iter().map(|a| &a.entry);
    if let Some(wanted) = looked.entry.as_deref() {
        // Its entry's icon; an entry the launcher does not list (shown in
        // another desktop only) is often its icon's name too.
        let found = entries()
            .find(|e| e.id.eq_ignore_ascii_case(wanted))
            .and_then(|e| e.icon.clone())
            .and_then(|name| icon_named(&name, scale))
            .or_else(|| icon_named(wanted, scale));
        if found.is_some() {
            return found;
        }
    }
    let app = looked.app.trim();
    if app.is_empty() {
        return None;
    }
    entries()
        .find(|e| e.name.eq_ignore_ascii_case(app))
        .and_then(|e| e.icon.clone())
        .and_then(|name| icon_named(&name, scale))
}
