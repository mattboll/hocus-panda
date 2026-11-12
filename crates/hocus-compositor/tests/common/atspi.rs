//! Applications' accessible trees for tests (docs/design/mcp.md, "In
//! applications"): a stand-in for the AT-SPI registry and an application
//! exposing a tree, on a private bus of the test's own, never the
//! session's accessibility bus. It says which elements' text was read, so
//! that a test sees a password's never is.
//!
//! Ported from v0-first-build's `crates/hocus-test/src/atspi.rs`, on zbus 6
//! and a runtime of its own.

use std::sync::{Arc, Mutex, PoisonError};

use anyhow::Context;
use zbus::{Connection, OwnedObjectPath, interface};

const REGISTRY: &str = "org.a11y.atspi.Registry";
const ROOT: &str = "/org/a11y/atspi/accessible/root";

/// AT-SPI's roles and states this uses, by number.
pub const FRAME: u32 = 23;
pub const PANEL: u32 = 39;
pub const PASSWORD_TEXT: u32 = 40;
pub const PUSH_BUTTON: u32 = 43;
pub const ENTRY: u32 = 79;
const APPLICATION: u32 = 75;
/// Enabled, sensitive, showing, visible.
pub const SHOWN: u64 = 1 << 8 | 1 << 24 | 1 << 25 | 1 << 30;

type Refs = Arc<Mutex<Vec<(String, OwnedObjectPath)>>>;

fn path(p: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(p).unwrap_or_default()
}

struct Registry {
    apps: Refs,
}

#[interface(name = "org.a11y.atspi.Accessible")]
impl Registry {
    #[zbus(property)]
    fn name(&self) -> &str {
        "main"
    }

    fn get_children(&self) -> Vec<(String, OwnedObjectPath)> {
        self.apps
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn get_role(&self) -> u32 {
        14
    }

    fn get_state(&self) -> Vec<u32> {
        vec![0, 0]
    }
}

/// An element of a test application's tree.
#[derive(Debug, Clone)]
pub struct Element {
    pub role: u32,
    pub label: String,
    /// From the window's top left: `[x, y, width, height]`.
    pub rect: Option<[i32; 4]>,
    pub states: u64,
    pub text: Option<String>,
    pub children: Vec<Element>,
}

impl Element {
    pub fn new(role: u32, label: &str, rect: [i32; 4]) -> Self {
        Self {
            role,
            label: label.to_owned(),
            rect: Some(rect),
            states: SHOWN,
            text: None,
            children: Vec::new(),
        }
    }

    pub fn with(mut self, children: Vec<Element>) -> Self {
        self.children = children;
        self
    }

    pub fn holding(mut self, text: &str) -> Self {
        self.text = Some(text.to_owned());
        self
    }

    /// A mail's compose window of 400×300: a subject, a password, and two
    /// buttons.
    pub fn compose(title: &str) -> Self {
        Element::new(FRAME, title, [0, 0, 400, 300]).with(vec![
            Element::new(PANEL, "", [0, 0, 400, 300]).with(vec![
                Element::new(ENTRY, "Sujet", [16, 40, 368, 32]).holding("Déjeuner vendredi"),
                Element::new(PASSWORD_TEXT, "Mot de passe", [16, 96, 368, 32]).holding("hunter2"),
                Element::new(PUSH_BUTTON, "Plus tard", [16, 244, 140, 40]),
                Element::new(PUSH_BUTTON, "Envoyer", [284, 244, 100, 40]),
            ]),
        ])
    }
}

/// The labels of the elements whose text was read.
pub type Reads = Arc<Mutex<Vec<String>>>;

struct Node {
    element: Element,
    children: Vec<(String, OwnedObjectPath)>,
}

#[interface(name = "org.a11y.atspi.Accessible")]
impl Node {
    #[zbus(property)]
    fn name(&self) -> String {
        self.element.label.clone()
    }

    fn get_children(&self) -> Vec<(String, OwnedObjectPath)> {
        self.children.clone()
    }

    fn get_role(&self) -> u32 {
        self.element.role
    }

    fn get_state(&self) -> Vec<u32> {
        let s = self.element.states;
        vec![s as u32, (s >> 32) as u32]
    }
}

struct Component {
    rect: [i32; 4],
}

#[interface(name = "org.a11y.atspi.Component")]
impl Component {
    fn get_extents(&self, coord_type: u32) -> (i32, i32, i32, i32) {
        let [x, y, w, h] = self.rect;
        // Window coordinates only: on Wayland the screen's are unknown.
        if coord_type == 1 {
            (x, y, w, h)
        } else {
            (-1, -1, -1, -1)
        }
    }
}

struct Text {
    text: String,
    label: String,
    reads: Reads,
}

#[interface(name = "org.a11y.atspi.Text")]
impl Text {
    #[zbus(property)]
    fn character_count(&self) -> i32 {
        self.text.chars().count() as i32
    }

    fn get_text(&self, start: i32, end: i32) -> String {
        self.reads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(self.label.clone());
        let end = if end < 0 { i32::MAX } else { end };
        self.text
            .chars()
            .skip(start.max(0) as usize)
            .take((end - start.max(0)).max(0) as usize)
            .collect()
    }
}

/// The registry and an application named so, with these windows, on the
/// bus at `address`, served by a runtime of their own.
pub struct FakeTree {
    _runtime: tokio::runtime::Runtime,
    _connections: Vec<Connection>,
    pub reads: Reads,
}

impl std::fmt::Debug for FakeTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeTree")
            .field("reads", &self.reads)
            .finish_non_exhaustive()
    }
}

impl FakeTree {
    pub fn start(address: &str, name: &str, windows: Vec<Element>) -> anyhow::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        let reads = Reads::default();
        let apps = Refs::default();
        let registry = runtime
            .block_on(async {
                let connection = zbus::connection::Builder::address(address)
                    .name(REGISTRY)
                    .build()
                    .await?;
                connection
                    .object_server()
                    .at(ROOT, Registry { apps: apps.clone() })
                    .await?;
                zbus::Result::Ok(connection)
            })
            .context("the registry on the bus")?;
        let app = runtime
            .block_on(async {
                let connection = zbus::connection::Builder::address(address).build().await?;
                let me = connection
                    .unique_name()
                    .map(|n| n.to_string())
                    .unwrap_or_default();
                let server = connection.object_server();
                let mut next = 0;
                let mut tops = Vec::new();
                for window in windows {
                    let at = serve(server, &me, window, &mut next, &reads).await?;
                    tops.push((me.clone(), at));
                }
                let mut root = Element::new(APPLICATION, name, [0, 0, 0, 0]);
                root.rect = None;
                server
                    .at(
                        ROOT,
                        Node {
                            element: root,
                            children: tops,
                        },
                    )
                    .await?;
                apps.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push((me, path(ROOT)));
                zbus::Result::Ok(connection)
            })
            .context("the application on the bus")?;
        Ok(Self {
            _runtime: runtime,
            _connections: vec![registry, app],
            reads,
        })
    }

    /// The labels of the elements whose text was read, in order.
    pub fn read(&self) -> Vec<String> {
        self.reads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Serves `element` and what it holds; its path.
async fn serve(
    server: &zbus::ObjectServer,
    me: &str,
    element: Element,
    next: &mut usize,
    reads: &Reads,
) -> zbus::Result<OwnedObjectPath> {
    let at = path(&format!("/org/a11y/atspi/accessible/{next}"));
    *next += 1;
    let mut children = Vec::new();
    for child in element.children.clone() {
        let child_at = Box::pin(serve(server, me, child, next, reads)).await?;
        children.push((me.to_owned(), child_at));
    }
    if let Some(rect) = element.rect {
        server.at(at.as_str(), Component { rect }).await?;
    }
    if let Some(text) = &element.text {
        server
            .at(
                at.as_str(),
                Text {
                    text: text.clone(),
                    label: element.label.clone(),
                    reads: reads.clone(),
                },
            )
            .await?;
    }
    let element = Element {
        children: Vec::new(),
        ..element
    };
    server.at(at.as_str(), Node { element, children }).await?;
    Ok(at)
}
