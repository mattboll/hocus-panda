//! A tray application for tests: a StatusNotifierItem with a dbusmenu
//! menu, on the private bus, that says what it was asked.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Context;
use zbus::{
    Connection, OwnedObjectPath, OwnedVariant as OwnedValue, Variant as Value, interface,
    object_server::SignalEmitter,
};

use crate::gate::Gate;

const MENU_PATH: &str = "/MenuBar";

/// What it shows and how it answers.
#[derive(Debug, Clone)]
struct Shown {
    title: String,
    attention: bool,
    /// Its icon's colour, ARGB.
    color: [u8; 4],
    /// Menu entries' labels, from id 1.
    menu: Vec<String>,
    /// How long it takes to answer anything.
    delay: Duration,
    /// What it holds its answers back on, until it opens.
    gate: Option<Gate>,
}

/// What it was asked, in order: `token` (an activation token given),
/// `activate`, `clicked N`.
pub type Log = Arc<Mutex<Vec<String>>>;

struct Item {
    shown: Arc<Mutex<Shown>>,
    log: Log,
}

impl Item {
    fn shown(&self) -> Shown {
        let shown = self
            .shown
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|e| e.into_inner().clone());
        std::thread::sleep(shown.delay);
        if let Some(gate) = &shown.gate {
            gate.wait();
        }
        shown
    }
}

#[interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    #[zbus(property)]
    fn id(&self) -> String {
        self.shown().title.to_lowercase()
    }

    #[zbus(property)]
    fn category(&self) -> &str {
        "Communications"
    }

    #[zbus(property)]
    fn title(&self) -> String {
        self.shown().title
    }

    #[zbus(property)]
    fn status(&self) -> &str {
        if self.shown().attention {
            "NeedsAttention"
        } else {
            "Active"
        }
    }

    #[zbus(property)]
    fn icon_name(&self) -> &str {
        ""
    }

    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
        let [a, r, g, b] = self.shown().color;
        // A disc, so it reads as an icon in the screenshots.
        let size = 22;
        let mut data = Vec::with_capacity(size * size * 4);
        for y in 0..size {
            for x in 0..size {
                let (dx, dy) = (x as f32 - 10.5, y as f32 - 10.5);
                let inside = dx * dx + dy * dy <= 100.0;
                data.extend(if inside { [a, r, g, b] } else { [0, 0, 0, 0] });
            }
        }
        vec![(size as i32, size as i32, data)]
    }

    #[zbus(property)]
    fn attention_icon_name(&self) -> &str {
        ""
    }

    #[zbus(property)]
    fn attention_icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
        self.icon_pixmap()
    }

    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(MENU_PATH).unwrap_or_default()
    }

    fn activate(&self, _x: i32, _y: i32) {
        if let Ok(mut log) = self.log.lock() {
            log.push("activate".into());
        }
    }

    fn provide_xdg_activation_token(&self, token: String) {
        if let Ok(mut log) = self.log.lock() {
            log.push(if token.is_empty() {
                "empty token".into()
            } else {
                "token".into()
            });
        }
    }

    fn context_menu(&self, _x: i32, _y: i32) {
        if let Ok(mut log) = self.log.lock() {
            log.push("context menu".into());
        }
    }

    fn secondary_activate(&self, _x: i32, _y: i32) {}

    #[zbus(signal)]
    async fn new_status(emitter: &SignalEmitter<'_>, status: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_title(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

struct Menu {
    shown: Arc<Mutex<Shown>>,
    log: Log,
}

type Layout = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

#[interface(name = "com.canonical.dbusmenu")]
impl Menu {
    fn get_layout(
        &self,
        _parent: i32,
        _depth: i32,
        _properties: Vec<String>,
    ) -> zbus::fdo::Result<(u32, Layout)> {
        let shown = self
            .shown
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|e| e.into_inner().clone());
        std::thread::sleep(shown.delay);
        if let Some(gate) = &shown.gate {
            gate.wait();
        }
        let value = |v: Value<'_>| {
            OwnedValue::try_from(v).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
        };
        let mut children = Vec::new();
        for (i, label) in shown.menu.iter().enumerate() {
            let mut props = HashMap::new();
            if label == "-" {
                props.insert("type".to_owned(), value(Value::from("separator"))?);
            } else {
                props.insert("label".to_owned(), value(Value::from(label.as_str()))?);
            }
            let child: Layout = (i as i32 + 1, props, Vec::new());
            children.push(value(Value::from(child))?);
        }
        let mut root = HashMap::new();
        root.insert(
            "children-display".to_owned(),
            value(Value::from("submenu"))?,
        );
        Ok((1, (0, root, children)))
    }

    fn event(&self, id: i32, event_id: String, _data: OwnedValue, _timestamp: u32) {
        if let Ok(mut log) = self.log.lock() {
            log.push(format!("{event_id} {id}"));
        }
    }

    fn about_to_show(&self, _id: i32) -> bool {
        false
    }

    #[zbus(property)]
    fn version(&self) -> u32 {
        3
    }
}

/// A tray application on the bus, until dropped.
pub struct TrayApp {
    connection: Connection,
    shown: Arc<Mutex<Shown>>,
    pub log: Log,
}

impl std::fmt::Debug for TrayApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrayApp")
            .field("log", &self.log)
            .finish_non_exhaustive()
    }
}

impl TrayApp {
    /// Registers an application titled `title` with the watcher on the bus
    /// at `address`; it takes `delay` to answer anything.
    pub fn start(
        address: &str,
        title: &str,
        color: [u8; 3],
        menu: &[String],
        delay: Duration,
    ) -> anyhow::Result<Self> {
        Self::start_with(address, title, color, menu, delay, None)
    }

    /// As [`TrayApp::start`], answering nothing until `gate` opens.
    pub fn start_held(
        address: &str,
        title: &str,
        color: [u8; 3],
        gate: Gate,
    ) -> anyhow::Result<Self> {
        Self::start_with(address, title, color, &[], Duration::ZERO, Some(gate))
    }

    fn start_with(
        address: &str,
        title: &str,
        color: [u8; 3],
        menu: &[String],
        delay: Duration,
        gate: Option<Gate>,
    ) -> anyhow::Result<Self> {
        let shown = Arc::new(Mutex::new(Shown {
            title: title.to_owned(),
            attention: false,
            color: [255, color[0], color[1], color[2]],
            menu: menu.to_vec(),
            delay,
            gate,
        }));
        let log = Log::default();
        let connection = zbus::block_on(async {
            zbus::connection::Builder::address(address)
                .serve_at(
                    "/StatusNotifierItem",
                    Item {
                        shown: shown.clone(),
                        log: log.clone(),
                    },
                )
                .serve_at(
                    MENU_PATH,
                    Menu {
                        shown: shown.clone(),
                        log: log.clone(),
                    },
                )
                .build()
                .await
        })
        .context("could not reach the bus")?;
        let name = connection
            .unique_name()
            .map(|n| n.to_string())
            .context("no name on the bus")?;
        zbus::block_on(connection.call_method(
            Some("org.kde.StatusNotifierWatcher"),
            "/StatusNotifierWatcher",
            Some("org.kde.StatusNotifierWatcher"),
            "RegisterStatusNotifierItem",
            &(name.as_str(),),
        ))
        .context("no tray on the bus")?;
        Ok(Self {
            connection,
            shown,
            log,
        })
    }

    /// Asks for attention, or stops; says so with NewStatus.
    pub fn set_attention(&self, attention: bool) -> anyhow::Result<()> {
        if let Ok(mut s) = self.shown.lock() {
            s.attention = attention;
        }
        let status = if attention {
            "NeedsAttention"
        } else {
            "Active"
        };
        zbus::block_on(self.connection.emit_signal(
            None::<&str>,
            "/StatusNotifierItem",
            "org.kde.StatusNotifierItem",
            "NewStatus",
            &(status,),
        ))?;
        Ok(())
    }

    pub fn asked(&self) -> Vec<String> {
        self.log.lock().map(|l| l.clone()).unwrap_or_default()
    }
}
