//! The system tray (#73): the StatusNotifierWatcher and its host on the
//! session bus. Applications register items; the host follows each one's
//! title, status and icon, and reads its dbusmenu menu when asked. Icons
//! come as pixels: the item's own (IconPixmap), or one found by name in the
//! icon themes, PNG or SVG.
//!
//! Everything runs on a thread of its own; the compositor only gets
//! [`TrayEvent`]s and sends [`TrayCommand`]s.

use std::{
    collections::HashMap,
    future::poll_fn,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, Mutex, mpsc},
};

use futures_core::Stream;
use zbus::{Connection, OwnedVariant, Proxy, interface, object_server::SignalEmitter};

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const ITEM: &str = "org.kde.StatusNotifierItem";
const MENU: &str = "com.canonical.dbusmenu";

/// An item's picture: RGBA, straight alpha.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icon {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

/// What the tray shows of an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Its bus name and path, which name it.
    pub key: String,
    pub title: String,
    pub attention: bool,
    pub icon: Option<Icon>,
}

/// One entry of an item's menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuEntry {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub separator: bool,
    /// A check box or radio button, and whether it is on.
    pub toggle: Option<bool>,
    pub children: Vec<MenuEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayEvent {
    /// An item came or changed.
    Item(Item),
    Gone(String),
    /// The menu asked for.
    Menu {
        key: String,
        entries: Vec<MenuEntry>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayCommand {
    /// Left click, with the activation token the application's window
    /// may come forward with.
    Activate {
        key: String,
        x: i32,
        y: i32,
        token: String,
    },
    /// Right click: its menu, or the item's own.
    Menu { key: String, x: i32, y: i32 },
    /// A menu entry chosen.
    Chosen { key: String, id: i32 },
}

type Sink = Arc<dyn Fn(TrayEvent) + Send + Sync>;

/// The tray, served on its thread.
#[derive(Debug)]
pub struct Tray {
    commands: mpsc::Sender<TrayCommand>,
}

impl Tray {
    /// Serves the watcher on `bus`; icons are wanted `size` pixels tall.
    pub fn start(
        bus: String,
        size: u32,
        sink: impl Fn(TrayEvent) + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        let (commands, received) = mpsc::channel();
        let sink: Sink = Arc::new(sink);
        std::thread::Builder::new()
            .name("tray".into())
            .spawn(move || {
                if let Err(err) = zbus::block_on(serve(bus, size, sink, received)) {
                    tracing::info!(%err, "no tray");
                }
            })?;
        Ok(Self { commands })
    }

    pub fn send(&self, command: TrayCommand) {
        let _ = self.commands.send(command);
    }
}

/// The watcher's state, shared with its D-Bus object.
#[derive(Default)]
struct Registered {
    items: Vec<String>,
}

struct Watcher {
    registered: Arc<Mutex<Registered>>,
    found: async_hint::Hint,
}

#[interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &self,
        service: String,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
        // A path names an item of the caller; a name, its usual path.
        let key = if service.starts_with('/') {
            format!("{sender}{service}")
        } else {
            format!("{service}/StatusNotifierItem")
        };
        if let Ok(mut r) = self.registered.lock()
            && !r.items.contains(&key)
        {
            r.items.push(key.clone());
        }
        let _ = Self::status_notifier_item_registered(&emitter, &key).await;
        self.found.ring(key);
    }

    fn register_status_notifier_host(&self, _service: String) {}

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.registered
            .lock()
            .map(|r| r.items.clone())
            .unwrap_or_default()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_registered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// A doorbell between the D-Bus object and the host's loop.
mod async_hint {
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
        task::{Poll, Waker},
    };

    #[derive(Default, Clone)]
    pub struct Hint(Arc<Mutex<(VecDeque<String>, Option<Waker>)>>);

    impl Hint {
        pub fn ring(&self, key: String) {
            if let Ok(mut h) = self.0.lock() {
                h.0.push_back(key);
                if let Some(w) = h.1.take() {
                    w.wake();
                }
            }
        }

        pub async fn next(&self) -> String {
            std::future::poll_fn(|cx| {
                let Ok(mut h) = self.0.lock() else {
                    return Poll::Pending;
                };
                match h.0.pop_front() {
                    Some(key) => Poll::Ready(key),
                    None => {
                        h.1 = Some(cx.waker().clone());
                        Poll::Pending
                    }
                }
            })
            .await
        }
    }
}

async fn serve(
    bus: String,
    size: u32,
    sink: Sink,
    commands: mpsc::Receiver<TrayCommand>,
) -> zbus::Result<()> {
    let found = async_hint::Hint::default();
    let registered = Arc::new(Mutex::new(Registered::default()));
    let conn = zbus::connection::Builder::address(bus.as_str())
        .serve_at(
            "/StatusNotifierWatcher",
            Watcher {
                registered: registered.clone(),
                found: found.clone(),
            },
        )
        .name(WATCHER)
        .build()
        .await?;
    tracing::info!("serving the tray");
    // Commands come from the compositor on a plain channel: a thread rings
    // them in.
    let asked = async_hint::Hint::default();
    let queue: Arc<Mutex<Vec<TrayCommand>>> = Arc::default();
    {
        let (asked, queue) = (asked.clone(), queue.clone());
        std::thread::Builder::new()
            .name("tray commands".into())
            .spawn(move || {
                while let Ok(command) = commands.recv() {
                    if let Ok(mut q) = queue.lock() {
                        q.push(command);
                    }
                    asked.ring(String::new());
                }
            })
            .map_err(|e| zbus::Error::Failure(e.to_string()))?;
    }
    let mut followed: HashMap<String, ()> = HashMap::new();
    loop {
        // Whichever comes first: an item registered, a command.
        let next = futures_util_select(found.next(), asked.next()).await;
        match next {
            Either::Left(key) => {
                if followed.insert(key.clone(), ()).is_none() {
                    let (conn, sink, registered) = (conn.clone(), sink.clone(), registered.clone());
                    std::thread::Builder::new()
                        .name("tray item".into())
                        .spawn(move || {
                            zbus::block_on(follow(conn, key, size, sink, registered));
                        })
                        .map_err(|e| zbus::Error::Failure(e.to_string()))?;
                }
            }
            Either::Right(_) => {
                let pending: Vec<TrayCommand> = queue
                    .lock()
                    .map(|mut q| q.drain(..).collect())
                    .unwrap_or_default();
                for command in pending {
                    let (conn, sink) = (conn.clone(), sink.clone());
                    std::thread::Builder::new()
                        .name("tray command".into())
                        .spawn(move || {
                            if let Err(err) = zbus::block_on(run(conn, command, sink)) {
                                tracing::debug!(%err, "a tray item did not answer");
                            }
                        })
                        .map_err(|e| zbus::Error::Failure(e.to_string()))?;
                }
            }
        }
    }
}

enum Either<A, B> {
    Left(A),
    Right(B),
}

/// The first of two futures to be ready.
async fn futures_util_select<A, B>(
    a: impl std::future::Future<Output = A>,
    b: impl std::future::Future<Output = B>,
) -> Either<A, B> {
    let mut a = std::pin::pin!(a);
    let mut b = std::pin::pin!(b);
    poll_fn(|cx| {
        if let std::task::Poll::Ready(v) = a.as_mut().poll(cx) {
            return std::task::Poll::Ready(Either::Left(v));
        }
        if let std::task::Poll::Ready(v) = b.as_mut().poll(cx) {
            return std::task::Poll::Ready(Either::Right(v));
        }
        std::task::Poll::Pending
    })
    .await
}

fn split(key: &str) -> (&str, &str) {
    match key.find('/') {
        Some(i) => (&key[..i], &key[i..]),
        None => (key, "/StatusNotifierItem"),
    }
}

async fn item_proxy(conn: &Connection, key: &str) -> zbus::Result<Proxy<'static>> {
    let (name, path) = split(key);
    // Items say a property changed with a signal of their own, not
    // PropertiesChanged: a cache would keep the old values.
    zbus::proxy::Builder::new(conn)
        .destination(name.to_owned())
        .path(path.to_owned())
        .interface(ITEM)
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
}

/// Reads an item, then again each time it says something changed, until
/// it goes away.
async fn follow(
    conn: Connection,
    key: String,
    size: u32,
    sink: Sink,
    registered: Arc<Mutex<Registered>>,
) {
    let Ok(proxy) = item_proxy(&conn, &key).await else {
        return;
    };
    let read = |proxy: Proxy<'static>, key: String| async move {
        let title: String = proxy.get_property("Title").await.unwrap_or_default();
        let status: String = proxy.get_property("Status").await.unwrap_or_default();
        let attention = status == "NeedsAttention";
        let (pixmap_prop, name_prop) = if attention {
            ("AttentionIconPixmap", "AttentionIconName")
        } else {
            ("IconPixmap", "IconName")
        };
        let pixmaps: Vec<(i32, i32, Vec<u8>)> =
            proxy.get_property(pixmap_prop).await.unwrap_or_default();
        let name: String = proxy.get_property(name_prop).await.unwrap_or_default();
        let theme_path: String = proxy
            .get_property("IconThemePath")
            .await
            .unwrap_or_default();
        let icon = from_pixmaps(&pixmaps, size).or_else(|| {
            let extra = (!theme_path.is_empty()).then(|| PathBuf::from(&theme_path));
            from_theme(&name, size, extra.as_deref())
        });
        TrayEvent::Item(Item {
            key,
            title,
            attention,
            icon,
        })
    };
    sink(read(proxy.clone(), key.clone()).await);
    // Its owner leaving ends it.
    let (name, _) = split(&key);
    let owner_gone = {
        let name = name.to_owned();
        let dbus = zbus::fdo::DBusProxy::new(&conn).await;
        async move {
            let Ok(dbus) = dbus else { return };
            let Ok(mut changes) = dbus.receive_name_owner_changed().await else {
                return;
            };
            while let Some(change) = poll_fn(|cx| Pin::new(&mut changes).poll_next(cx)).await {
                if let Ok(args) = change.args()
                    && args.name().as_str() == name
                    && args.new_owner().is_none()
                {
                    return;
                }
            }
        }
    };
    let changes = async {
        let Ok(mut signals) = proxy.receive_all_signals().await else {
            return;
        };
        while poll_fn(|cx| Pin::new(&mut signals).poll_next(cx))
            .await
            .is_some()
        {
            sink(read(proxy.clone(), key.clone()).await);
        }
    };
    let _ = futures_util_select(owner_gone, changes).await;
    if let Ok(mut r) = registered.lock() {
        r.items.retain(|k| *k != key);
    }
    sink(TrayEvent::Gone(key));
}

async fn run(conn: Connection, command: TrayCommand, sink: Sink) -> zbus::Result<()> {
    match command {
        TrayCommand::Activate { key, x, y, token } => {
            let item = item_proxy(&conn, &key).await?;
            // KDE's addition, which items without it refuse: their window
            // may then still come forward, asking for attention.
            let _: zbus::Result<()> = item.call("ProvideXdgActivationToken", &(token,)).await;
            item.call::<_, _, ()>("Activate", &(x, y)).await
        }
        TrayCommand::Menu { key, x, y } => {
            let item = item_proxy(&conn, &key).await?;
            let path: zbus::OwnedObjectPath = match item.get_property("Menu").await {
                Ok(path) => path,
                // No menu of ours to show: the item shows its own.
                Err(_) => return item.call::<_, _, ()>("ContextMenu", &(x, y)).await,
            };
            let (name, _) = split(&key);
            let menu = Proxy::new(&conn, name.to_owned(), path.to_string(), MENU).await?;
            let _: zbus::Result<bool> = menu.call("AboutToShow", &(0i32,)).await;
            let (_, layout): (u32, Layout) = menu
                .call("GetLayout", &(0i32, -1i32, Vec::<String>::new()))
                .await?;
            sink(TrayEvent::Menu {
                key,
                entries: layout.2.into_iter().filter_map(|v| entry(&v)).collect(),
            });
            Ok(())
        }
        TrayCommand::Chosen { key, id } => {
            let item = item_proxy(&conn, &key).await?;
            let path: zbus::OwnedObjectPath = item.get_property("Menu").await?;
            let (name, _) = split(&key);
            let menu = Proxy::new(&conn, name.to_owned(), path.to_string(), MENU).await?;
            let stamp = 0u32;
            menu.call::<_, _, ()>("Event", &(id, "clicked", zbus::Variant::from(0i32), stamp))
                .await
        }
    }
}

/// dbusmenu's layout: id, properties, children (each a layout, in a
/// variant).
type Layout = (i32, HashMap<String, OwnedVariant>, Vec<OwnedVariant>);

fn entry(value: &OwnedVariant) -> Option<MenuEntry> {
    let (id, props, children): Layout = value.try_clone().ok()?.try_into().ok()?;
    let text = |k: &str| {
        props
            .get(k)
            .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
    };
    let flag = |k: &str| {
        props
            .get(k)
            .and_then(|v| bool::try_from(v.try_clone().ok()?).ok())
    };
    if flag("visible") == Some(false) {
        return None;
    }
    let toggle = text("toggle-type").filter(|t| !t.is_empty()).map(|_| {
        props
            .get("toggle-state")
            .and_then(|v| i32::try_from(v.try_clone().ok()?).ok())
            == Some(1)
    });
    Some(MenuEntry {
        id,
        // Underscores mark access keys in dbusmenu.
        label: text("label").unwrap_or_default().replace('_', ""),
        enabled: flag("enabled").unwrap_or(true),
        separator: text("type").as_deref() == Some("separator"),
        toggle,
        children: children.iter().filter_map(entry).collect(),
    })
}

/// The pixmap closest to `size`, ARGB in network order made RGBA.
pub fn from_pixmaps(pixmaps: &[(i32, i32, Vec<u8>)], size: u32) -> Option<Icon> {
    let (w, h, argb) = pixmaps
        .iter()
        .filter(|(w, h, d)| *w > 0 && *h > 0 && d.len() == (*w * *h * 4) as usize)
        .min_by_key(|(w, _, _)| (*w as i64 - i64::from(size)).abs())?;
    let rgba: Vec<u8> = argb
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[1], p[2], p[3], p[0]])
        .collect();
    Some(Icon {
        width: *w as u32,
        height: *h as u32,
        rgba: rgba.into(),
    })
}

/// An icon by name: the item's own folder, then the icon themes (the
/// user's, hicolor, then pixmaps), PNG at the nearest size or SVG drawn at
/// `size`.
pub fn from_theme(name: &str, size: u32, extra: Option<&Path>) -> Option<Icon> {
    if name.is_empty() {
        return None;
    }
    if name.starts_with('/') {
        return load(Path::new(name), size);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let data =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    let mut roots: Vec<PathBuf> = extra.into_iter().map(Path::to_path_buf).collect();
    roots.push(home.join(".local/share/icons"));
    roots.push(home.join(".icons"));
    roots.extend(data.split(':').map(|d| Path::new(d).join("icons")));
    let themes = ["hicolor", "Adwaita", "breeze"];
    let mut best: Option<(u32, PathBuf)> = None;
    for root in &roots {
        // An item's own folder may hold the files directly.
        for direct in [
            root.join(format!("{name}.png")),
            root.join(format!("{name}.svg")),
        ] {
            if direct.is_file() {
                return load(&direct, size);
            }
        }
        for theme in themes {
            let Ok(sizes) = std::fs::read_dir(root.join(theme)) else {
                continue;
            };
            for dir in sizes.flatten() {
                let dir_name = dir.file_name().to_string_lossy().into_owned();
                let px: u32 = dir_name
                    .split('x')
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(if dir_name == "scalable" { 1024 } else { 0 });
                for category in ["status", "apps", "devices", "actions", "panel", "places"] {
                    for ext in ["png", "svg"] {
                        let path = dir.path().join(category).join(format!("{name}.{ext}"));
                        if !path.is_file() {
                            continue;
                        }
                        let score = px.abs_diff(size);
                        if best.as_ref().is_none_or(|(s, _)| score < *s) {
                            best = Some((score, path));
                        }
                    }
                }
            }
        }
    }
    if let Some((_, path)) = best {
        return load(&path, size);
    }
    ["/usr/share/pixmaps", "/usr/local/share/pixmaps"]
        .iter()
        .flat_map(|d| ["png", "svg"].map(|e| Path::new(d).join(format!("{name}.{e}"))))
        .find(|p| p.is_file())
        .and_then(|p| load(&p, size))
}

pub(crate) fn load(path: &Path, size: u32) -> Option<Icon> {
    if path.extension().is_some_and(|e| e == "svg") {
        let data = std::fs::read(path).ok()?;
        let tree = resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default()).ok()?;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)?;
        let s = tree.size();
        let scale = size as f32 / s.width().max(s.height());
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        // tiny-skia is premultiplied; the tray hands out straight alpha.
        let rgba: Vec<u8> = pixmap
            .pixels()
            .iter()
            .flat_map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            })
            .collect();
        return Some(Icon {
            width: size,
            height: size,
            rgba: rgba.into(),
        });
    }
    let file = std::fs::File::open(path).ok()?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::GrayscaleAlpha => buf[..info.buffer_size()]
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        _ => return None,
    };
    Some(Icon {
        width: info.width,
        height: info.height,
        rgba: rgba.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_pixmap_is_made_rgba() {
        let red = [255u8, 255, 0, 0];
        let small = (16, 16, red.repeat(256));
        let large = (64, 64, red.repeat(4096));
        let icon = from_pixmaps(&[small, large], 24).expect("an icon");
        assert_eq!((icon.width, icon.height), (16, 16));
        assert_eq!(&icon.rgba[..4], &[255, 0, 0, 255]);
        assert!(
            from_pixmaps(&[(4, 4, vec![0; 3])], 24).is_none(),
            "a broken one"
        );
    }

    #[test]
    fn an_svg_is_drawn_at_the_size_asked() {
        let dir = tempfile::tempdir().expect("a directory");
        let path = dir.path().join("dot.svg");
        std::fs::write(
            &path,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#00ff00"/></svg>"##,
        )
        .expect("written");
        let icon = from_theme("dot", 32, Some(dir.path())).expect("an icon");
        assert_eq!((icon.width, icon.height), (32, 32));
        assert_eq!(&icon.rgba[..4], &[0, 255, 0, 255]);
    }
}
