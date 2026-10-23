//! Applications' accessibility trees for tests (#175): a stand-in for the
//! AT-SPI registry and applications exposing trees on a private
//! accessibility bus, never the session's. An application's elements are
//! where its test client draws them ([`FakeElement::drawn`]), and it says
//! which elements' text was read, so that a test sees a password's never
//! is.

use std::sync::{Arc, Mutex};

use anyhow::Context;
use hocus_platform::atspi::{Role, States};
use zbus::{Connection, OwnedObjectPath, interface};

use crate::client::Drawn;

const REGISTRY: &str = "org.a11y.atspi.Registry";
const ROOT: &str = "/org/a11y/atspi/accessible/root";

type Refs = Arc<Mutex<Vec<(String, OwnedObjectPath)>>>;

fn path(p: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(p).unwrap_or_default()
}

/// The registry: the applications that embedded themselves, as the
/// desktop's children.
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
        self.apps.lock().map(|a| a.clone()).unwrap_or_default()
    }

    fn get_role(&self) -> u32 {
        14
    }

    fn get_state(&self) -> Vec<u32> {
        vec![0, 0]
    }
}

struct Socket {
    apps: Refs,
    me: String,
}

#[interface(name = "org.a11y.atspi.Socket")]
impl Socket {
    /// An application embeds its root in the desktop.
    fn embed(&self, plug: (String, OwnedObjectPath)) -> (String, OwnedObjectPath) {
        if let Ok(mut apps) = self.apps.lock() {
            apps.push(plug);
        }
        (self.me.clone(), path(ROOT))
    }
}

/// The registry, on the bus at `address`.
pub struct FakeRegistry {
    _connection: Connection,
}

impl std::fmt::Debug for FakeRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeRegistry").finish_non_exhaustive()
    }
}

impl FakeRegistry {
    pub fn start(address: &str) -> anyhow::Result<Self> {
        let apps = Refs::default();
        let connection = zbus::block_on(async {
            let connection = zbus::connection::Builder::address(address)
                .name(REGISTRY)
                .build()
                .await?;
            let me = connection
                .unique_name()
                .map(|n| n.to_string())
                .unwrap_or_default();
            connection
                .object_server()
                .at(ROOT, Registry { apps: apps.clone() })
                .await?;
            connection
                .object_server()
                .at(ROOT, Socket { apps, me })
                .await?;
            zbus::Result::Ok(connection)
        })
        .context("could not take the registry's name on the accessibility bus")?;
        Ok(Self {
            _connection: connection,
        })
    }
}

/// An element of a test application's tree.
#[derive(Debug, Clone)]
pub struct FakeElement {
    pub role: Role,
    pub label: String,
    /// From the window's top-left corner: `[x, y, width, height]`.
    pub rect: Option<[i32; 4]>,
    pub states: States,
    /// What it holds, for text elements.
    pub text: Option<String>,
    /// Drawn by the test client in this colour (ARGB), where it is.
    pub color: Option<u32>,
    pub children: Vec<FakeElement>,
}

impl FakeElement {
    pub fn new(role: Role, label: &str, rect: [i32; 4]) -> Self {
        Self {
            role,
            label: label.to_owned(),
            rect: Some(rect),
            states: States::default().with(&States::SHOWN),
            text: None,
            color: None,
            children: Vec::new(),
        }
    }

    fn with(mut self, children: Vec<FakeElement>) -> Self {
        self.children = children;
        self
    }

    fn drawn_in(mut self, color: u32) -> Self {
        self.color = Some(color);
        self
    }

    fn holding(mut self, text: &str) -> Self {
        self.text = Some(text.to_owned());
        self.states = self.states.with(&[States::EDIT]);
        self
    }

    /// What its test client draws: the elements with a colour, in order.
    pub fn drawn(&self) -> Vec<Drawn> {
        let mut out = Vec::new();
        let mut stack = vec![self];
        while let Some(e) = stack.pop() {
            if let (Some(color), Some([x, y, w, h])) = (e.color, e.rect) {
                out.push(Drawn {
                    label: e.label.clone(),
                    rect: [x as u32, y as u32, w as u32, h as u32],
                    argb: color,
                });
            }
            stack.extend(e.children.iter().rev());
        }
        out
    }

    /// A mail's compose window of 400×300: a subject, a password, and
    /// buttons, one of them disabled, and a page not shown.
    pub fn compose(title: &str) -> Self {
        let field = 0xff_2a_2f_38;
        let mut delete = FakeElement::new(Role::PUSH_BUTTON, "Delete", [172, 244, 96, 40])
            .drawn_in(0xff_33_36_3d);
        // Shown, not enabled.
        delete.states = States::default().with(&States::SHOWN[2..]);
        let mut hidden =
            FakeElement::new(Role::PANEL, "Drafts", [0, 0, 400, 300]).with(vec![FakeElement::new(
                Role::PUSH_BUTTON,
                "Hidden",
                [16, 16, 80, 32],
            )]);
        hidden.states = States::default();
        FakeElement::new(Role::FRAME, title, [0, 0, 400, 300]).with(vec![
            FakeElement::new(Role::PANEL, "", [0, 0, 400, 300]).with(vec![
                FakeElement::new(Role::LABEL, "Subject", [16, 16, 80, 20]),
                FakeElement::new(Role::ENTRY, "Subject", [16, 40, 368, 32])
                    .holding("Lunch on Friday")
                    .drawn_in(field),
                FakeElement::new(Role::PASSWORD_TEXT, "Password", [16, 96, 368, 32])
                    .holding("hunter2")
                    .drawn_in(field),
                FakeElement::new(Role::PUSH_BUTTON, "Send later", [16, 244, 140, 40])
                    .drawn_in(0xff_4a_52_60),
                delete,
                FakeElement::new(Role::PUSH_BUTTON, "Send", [284, 244, 100, 40])
                    .drawn_in(0xff_6b_72_80),
            ]),
            hidden,
        ])
    }

    /// A window holding a list of `items`: more than a tree is read.
    pub fn long_list(title: &str, items: usize) -> Self {
        FakeElement::new(Role::FRAME, title, [0, 0, 400, 300]).with(vec![
            FakeElement::new(Role::LIST, "Messages", [0, 0, 400, 300]).with(
                (0..items)
                    .map(|i| {
                        FakeElement::new(
                            Role::LIST_ITEM,
                            &format!("Message {i}"),
                            [0, (i as i32 * 20) % 300, 400, 20],
                        )
                    })
                    .collect(),
            ),
        ])
    }
}

/// The labels of the elements whose text was read.
pub type Reads = Arc<Mutex<Vec<String>>>;

struct Node {
    element: FakeElement,
    children: Vec<(String, OwnedObjectPath)>,
}

#[interface(name = "org.a11y.atspi.Accessible")]
impl Node {
    #[zbus(property)]
    fn name(&self) -> String {
        self.element.label.clone()
    }

    #[zbus(property)]
    fn child_count(&self) -> i32 {
        self.children.len() as i32
    }

    fn get_children(&self) -> Vec<(String, OwnedObjectPath)> {
        self.children.clone()
    }

    fn get_role(&self) -> u32 {
        self.element.role.0
    }

    fn get_state(&self) -> Vec<u32> {
        let s = self.element.states.0;
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
        if let Ok(mut reads) = self.reads.lock() {
            reads.push(self.label.clone());
        }
        let end = if end < 0 { i32::MAX } else { end };
        self.text
            .chars()
            .skip(start.max(0) as usize)
            .take((end - start.max(0)).max(0) as usize)
            .collect()
    }
}

struct Application;

#[interface(name = "org.a11y.atspi.Application")]
impl Application {
    #[zbus(property)]
    fn toolkit_name(&self) -> &str {
        "hocus-test"
    }
}

/// A test application on the accessibility bus, embedded in the
/// registry, its windows' trees served.
pub struct FakeApp {
    _connection: Connection,
    pub reads: Reads,
}

impl std::fmt::Debug for FakeApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeApp")
            .field("reads", &self.reads)
            .finish_non_exhaustive()
    }
}

impl FakeApp {
    /// Application `name` with these windows, on the bus at `address`.
    pub fn start(address: &str, name: &str, windows: Vec<FakeElement>) -> anyhow::Result<Self> {
        let reads = Reads::default();
        let connection = zbus::block_on(async {
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
            let mut root = FakeElement::new(Role::APPLICATION, name, [0, 0, 0, 0]);
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
            server.at(ROOT, Application).await?;
            connection
                .call_method(
                    Some(REGISTRY),
                    ROOT,
                    Some("org.a11y.atspi.Socket"),
                    "Embed",
                    &((me.as_str(), path(ROOT)),),
                )
                .await?;
            zbus::Result::Ok(connection)
        })
        .context("could not put the application on the accessibility bus")?;
        Ok(Self {
            _connection: connection,
            reads,
        })
    }

    /// The labels of the elements whose text was read, in order.
    pub fn read(&self) -> Vec<String> {
        self.reads.lock().map(|r| r.clone()).unwrap_or_default()
    }
}

/// Serves `element` and what it holds; its path.
async fn serve(
    server: &zbus::ObjectServer,
    me: &str,
    element: FakeElement,
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
    let element = FakeElement {
        children: Vec::new(),
        ..element
    };
    server.at(at.as_str(), Node { element, children }).await?;
    Ok(at)
}
