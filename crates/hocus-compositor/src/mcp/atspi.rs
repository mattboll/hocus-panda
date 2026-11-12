//! Applications' accessible trees, read over AT-SPI (docs/design/mcp.md,
//! "In applications"): the accessibility bus screen readers use. The
//! session's bus says where it is (`org.a11y.Bus.GetAddress`); on it, the
//! registry lists the applications, each a tree of
//! `org.a11y.atspi.Accessible` objects with a role, a name, states and,
//! through `org.a11y.atspi.Component`, a place.
//!
//! Ported from v0-first-build's `crates/hocus-platform/src/atspi.rs`, which
//! held up: the matching of a window to its application (its client's
//! process, then its app id) and to a top-level object (its title, then
//! its size), the breadth-first walk within limits, password fields never
//! read. New: zbus 6, a runtime of its own on the reader's thread, and
//! the answers handed to the loop through its channel.
//!
//! Everything here blocks on a thread of its own, never on the loop. The
//! interfaces are those of the AT-SPI specification; no code of its
//! libraries is used.

use std::{
    collections::VecDeque,
    sync::mpsc,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use zbus::{Connection, OwnedObjectPath, OwnedVariant};

/// Elements read at most for one window.
pub const MAX_NODES: usize = 800;
/// Levels below the window read at most.
pub const MAX_DEPTH: u32 = 40;
/// How long one window's tree may take to read.
pub const MAX_TIME: Duration = Duration::from_secs(3);
/// How long one call may take: an application that hangs costs this.
const CALL_TIMEOUT: Duration = Duration::from_millis(1500);
/// Labels and values are cut past this many characters.
pub const MAX_TEXT: usize = 200;

const REGISTRY: &str = "org.a11y.atspi.Registry";
const ROOT: &str = "/org/a11y/atspi/accessible/root";
const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
const COMPONENT: &str = "org.a11y.atspi.Component";
const TEXT: &str = "org.a11y.atspi.Text";
const APPLICATION: &str = "org.a11y.atspi.Application";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
/// `GetExtents` relative to the object's window, not the screen: on
/// Wayland, applications do not know where they are on it.
const WINDOW_COORDS: u32 = 1;

/// Where the accessibility bus is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bus {
    /// At this address (tests: a private one).
    Address(String),
    /// Asked of `org.a11y.Bus` on the session bus at this address.
    Given(String),
    /// Asked of `org.a11y.Bus` on the user's session bus (on a virtual
    /// terminal, the compositor is the session).
    Session,
    /// None: nested beside a live session without `--session-bus`.
    Unknown,
}

/// A window to find the tree of, as the compositor knows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Window {
    /// The process of its Wayland client.
    pub pid: Option<u32>,
    pub app_id: Option<String>,
    pub title: Option<String>,
    /// Its size, logical pixels: tells apart two windows of one title.
    pub size: (i32, i32),
    /// It is the only window of its client: an application with a single
    /// top-level object is its.
    pub alone: bool,
}

/// A role, as AT-SPI numbers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Role(pub u32);

/// The roles' names, by number, as the specification says them.
const ROLES: &[&str] = &[
    "invalid",
    "accelerator label",
    "alert",
    "animation",
    "arrow",
    "calendar",
    "canvas",
    "check box",
    "check menu item",
    "color chooser",
    "column header",
    "combo box",
    "date editor",
    "desktop icon",
    "desktop frame",
    "dial",
    "dialog",
    "directory pane",
    "drawing area",
    "file chooser",
    "filler",
    "focus traversable",
    "font chooser",
    "frame",
    "glass pane",
    "html container",
    "icon",
    "image",
    "internal frame",
    "label",
    "layered pane",
    "list",
    "list item",
    "menu",
    "menu bar",
    "menu item",
    "option pane",
    "page tab",
    "page tab list",
    "panel",
    "password text",
    "popup menu",
    "progress bar",
    "push button",
    "radio button",
    "radio menu item",
    "root pane",
    "row header",
    "scroll bar",
    "scroll pane",
    "separator",
    "slider",
    "spin button",
    "split pane",
    "status bar",
    "table",
    "table cell",
    "table column header",
    "table row header",
    "tearoff menu item",
    "terminal",
    "text",
    "toggle button",
    "tool bar",
    "tool tip",
    "tree",
    "tree table",
    "unknown",
    "viewport",
    "window",
    "extended",
    "header",
    "footer",
    "paragraph",
    "ruler",
    "application",
    "autocomplete",
    "editbar",
    "embedded",
    "entry",
    "chart",
    "caption",
    "document frame",
    "heading",
    "page",
    "section",
    "redundant object",
    "form",
    "link",
    "input method window",
    "table row",
    "tree item",
    "document spreadsheet",
    "document presentation",
    "document text",
    "document web",
    "document email",
    "comment",
    "list box",
    "grouping",
    "image map",
    "notification",
    "info bar",
    "level bar",
    "title bar",
    "block quote",
    "audio",
    "video",
    "definition",
    "article",
    "landmark",
    "log",
    "marquee",
    "math",
    "rating",
    "timer",
    "static",
    "math fraction",
    "math root",
    "subscript",
    "superscript",
    "description list",
    "description term",
    "description value",
    "footnote",
    "content deletion",
    "content insertion",
    "mark",
    "suggestion",
    "push button menu",
    "switch",
];

impl Role {
    pub const CHECK_BOX: Self = Self(7);
    pub const CHECK_MENU_ITEM: Self = Self(8);
    pub const PASSWORD_TEXT: Self = Self(40);
    pub const RADIO_BUTTON: Self = Self(44);
    pub const RADIO_MENU_ITEM: Self = Self(45);
    pub const SPIN_BUTTON: Self = Self(52);
    pub const TEXT: Self = Self(61);
    pub const TOGGLE_BUTTON: Self = Self(62);
    pub const EDITBAR: Self = Self(77);
    pub const ENTRY: Self = Self(79);
    pub const SWITCH: Self = Self(130);

    /// Its name: `push button`, `entry`, `password text`.
    pub fn name(self) -> String {
        ROLES
            .get(self.0 as usize)
            .map_or_else(|| format!("role {}", self.0), |n| (*n).to_owned())
    }

    /// Whether elements of this role are checked or not.
    pub fn checkable(self) -> bool {
        [
            Self::CHECK_BOX,
            Self::CHECK_MENU_ITEM,
            Self::RADIO_BUTTON,
            Self::RADIO_MENU_ITEM,
            Self::TOGGLE_BUTTON,
            Self::SWITCH,
        ]
        .contains(&self)
    }

    /// Whether its text is what the user typed: read as its value.
    fn holds_input(self) -> bool {
        [Self::ENTRY, Self::TEXT, Self::SPIN_BUTTON, Self::EDITBAR].contains(&self)
    }
}

/// An element's states, as AT-SPI's two words of bits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct States(pub u64);

impl States {
    const CHECKED: u32 = 4;
    const ENABLED: u32 = 8;
    const FOCUSED: u32 = 12;
    const SELECTED: u32 = 23;
    const SENSITIVE: u32 = 24;
    const SHOWING: u32 = 25;
    const VISIBLE: u32 = 30;

    /// From `GetState`'s words, the low one first.
    pub fn from_words(words: &[u32]) -> Self {
        let low = u64::from(words.first().copied().unwrap_or_default());
        let high = u64::from(words.get(1).copied().unwrap_or_default());
        Self(low | high << 32)
    }

    fn has(self, bit: u32) -> bool {
        self.0 & (1 << bit) != 0
    }

    pub fn checked(self) -> bool {
        self.has(Self::CHECKED)
    }

    /// Enabled and sensitive: it answers the user.
    pub fn enabled(self) -> bool {
        self.has(Self::ENABLED) && self.has(Self::SENSITIVE)
    }

    pub fn focused(self) -> bool {
        self.has(Self::FOCUSED)
    }

    pub fn selected(self) -> bool {
        self.has(Self::SELECTED)
    }

    pub fn showing(self) -> bool {
        self.has(Self::SHOWING)
    }

    pub fn visible(self) -> bool {
        self.has(Self::VISIBLE)
    }
}

/// A place relative to the window's top-left corner, logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Extents {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// One element of a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// How deep, from the window (0).
    pub depth: u32,
    pub role: Role,
    pub label: Option<String>,
    /// None when the application does not say where it is.
    pub rect: Option<Extents>,
    pub states: States,
    /// What was typed in it; never for a password field.
    pub value: Option<String>,
}

impl Node {
    /// A field for a secret: by its role, or by its label when its
    /// toolkit does not say.
    pub fn secret(&self) -> bool {
        self.role == Role::PASSWORD_TEXT || self.label.as_deref().is_some_and(secret_label)
    }

    /// As the tool says it.
    pub fn json(&self) -> Value {
        let mut out = json!({
            "role": self.role.name(),
            "label": self.label,
            "rect": self.rect.map(|r| json!({ "x": r.x, "y": r.y, "width": r.width, "height": r.height })),
            "depth": self.depth,
            "focused": self.states.focused(),
            "disabled": !self.states.enabled(),
            "selected": self.states.selected(),
        });
        if let Some(fields) = out.as_object_mut() {
            if self.role.checkable() {
                fields.insert("checked".into(), json!(self.states.checked()));
            }
            if let Some(value) = &self.value {
                fields.insert("value".into(), json!(value));
            }
        }
        out
    }

    /// What a refusal lists it as: `push button "Send" (at 284,244 100×40)`.
    fn said(&self) -> String {
        let place = self.rect.map_or_else(String::new, |r| {
            format!(" (at {},{} {}×{})", r.x, r.y, r.width, r.height)
        });
        format!(
            "{} {:?}{place}",
            self.role.name(),
            self.label.as_deref().unwrap_or_default()
        )
    }
}

/// Words that label a field for a secret, in the languages shipped and
/// English.
fn secret_label(label: &str) -> bool {
    let label = label.to_lowercase();
    ["password", "passphrase", "passcode", "mot de passe"]
        .iter()
        .any(|w| label.contains(w))
        || label
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| w == "pin")
}

/// Why the reading stopped before the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    /// [`MAX_NODES`] elements read; at least `left` more were seen.
    Nodes { limit: usize, left: usize },
    /// Elements below [`MAX_DEPTH`] levels were left out.
    Depth { limit: u32 },
    /// [`MAX_TIME`] went by: what was not read by then was left out.
    Time { limit: Duration },
}

impl std::fmt::Display for Cut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nodes { limit, left } => write!(
                f,
                "only the first {limit} elements, breadth first: at least {left} more were left out"
            ),
            Self::Depth { limit } => write!(f, "elements deeper than {limit} levels were left out"),
            Self::Time { limit } => write!(
                f,
                "reading took over {} s: what was not read by then was left out",
                limit.as_secs()
            ),
        }
    }
}

/// A window's tree, in order, depth first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tree {
    /// The application as the bus names it.
    pub application: String,
    pub toolkit: Option<String>,
    pub nodes: Vec<Node>,
    pub cut: Option<Cut>,
}

impl Tree {
    /// As `accessibility_tree` answers it.
    pub fn json(&self) -> Value {
        json!({
            "application": self.application,
            "toolkit": self.toolkit,
            "elements": self.nodes.iter().map(Node::json).collect::<Vec<_>>(),
            "partial": self.cut.map(|c| c.to_string()),
        })
    }
}

/// Why a window has no tree.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum Missing {
    /// The accessibility bus is not reachable.
    NoBus(String),
    /// No application on it is the window's.
    NoApplication,
    /// Its application is there, but none of its windows is this one.
    NoWindow { application: String },
}

impl std::fmt::Display for Missing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoBus(why) => write!(
                f,
                "the accessibility bus is not reachable ({why}): applications' trees cannot be read"
            ),
            Self::NoApplication => write!(
                f,
                "its application is not on the accessibility bus: it exposes no tree (some \
                 toolkits do only once accessibility is turned on, as a screen reader does)"
            ),
            Self::NoWindow { application } => write!(
                f,
                "its application ({application}) is on the accessibility bus, but none of its \
                 windows there is this one"
            ),
        }
    }
}

pub type Outcome = Result<Tree, Missing>;

/// The element `label` names (and `role`, when given): an exact label
/// first, then one that contains it, ignoring case.
#[derive(Debug, PartialEq, Eq)]
pub enum Pick<'a> {
    One(&'a Node),
    Nothing,
    /// Several match: the agent says which, by role or a longer label.
    Many(Vec<&'a Node>),
}

pub fn pick<'a>(nodes: &'a [Node], role: Option<&str>, label: &str) -> Pick<'a> {
    let label = label.trim();
    let of_role = |n: &&Node| role.is_none_or(|r| n.role.name().eq_ignore_ascii_case(r.trim()));
    let exact: Vec<&Node> = nodes
        .iter()
        .filter(of_role)
        .filter(|n| n.label.as_deref().map(str::trim) == Some(label))
        .collect();
    let found = if exact.is_empty() {
        let wanted = label.to_lowercase();
        nodes
            .iter()
            .filter(of_role)
            .filter(|n| {
                n.label
                    .as_deref()
                    .is_some_and(|l| l.to_lowercase().contains(&wanted))
            })
            .collect()
    } else {
        exact
    };
    match found.as_slice() {
        [] => Pick::Nothing,
        [one] => Pick::One(one),
        _ => Pick::Many(found),
    }
}

/// Where element `label` (and `role`) of `tree` is: its centre from the
/// window's top left; or why it cannot be aimed at.
pub fn centre_of(tree: &Tree, role: Option<&str>, label: &str) -> Result<(f64, f64), String> {
    let node = match pick(&tree.nodes, role, label) {
        Pick::One(node) => node,
        Pick::Nothing => {
            return Err(format!(
                "no element labelled {label:?} in the window's tree"
            ));
        }
        Pick::Many(many) => {
            let listed: Vec<String> = many.iter().map(|n| n.said()).collect();
            return Err(format!(
                "{} elements match {label:?}: {}; say which with role or a longer label",
                many.len(),
                listed.join("; ")
            ));
        }
    };
    if !node.states.enabled() {
        return Err(format!("{} is disabled", node.said()));
    }
    let rect = node
        .rect
        .ok_or_else(|| format!("{} has no place its application says", node.said()))?;
    Ok((
        f64::from(rect.x) + f64::from(rect.width) / 2.0,
        f64::from(rect.y) + f64::from(rect.height) / 2.0,
    ))
}

/// Reads windows' trees on a thread of its own, one at a time, and hands
/// each to `done` with the token it was asked with.
#[derive(Debug)]
pub struct Reader {
    jobs: mpsc::Sender<(u64, Window)>,
}

impl Reader {
    pub fn start(bus: Bus, mut done: impl FnMut(u64, Outcome) + Send + 'static) -> Option<Self> {
        let (jobs, to_do) = mpsc::channel::<(u64, Window)>();
        let started = std::thread::Builder::new()
            .name("mcp atspi".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(err) => {
                        tracing::warn!(%err, "no runtime to read applications' trees");
                        return;
                    }
                };
                let mut connection = None;
                while let Ok((token, window)) = to_do.recv() {
                    let outcome = runtime.block_on(read(&bus, &mut connection, &window));
                    done(token, outcome);
                }
            });
        match started {
            Ok(_) => Some(Self { jobs }),
            Err(err) => {
                tracing::warn!(%err, "no thread to read applications' trees");
                None
            }
        }
    }

    /// Asks for `window`'s tree; false when the thread is gone.
    pub fn read(&self, token: u64, window: Window) -> bool {
        self.jobs.send((token, window)).is_ok()
    }
}

/// An object on the bus: its connection's name and its path.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Ref {
    bus: String,
    path: String,
}

impl Ref {
    fn from_wire((bus, path): (String, OwnedObjectPath)) -> Self {
        Self {
            bus,
            path: path.as_str().to_owned(),
        }
    }

    /// An object of no application: a child that went, or a "null" one.
    fn is_null(&self) -> bool {
        self.bus.is_empty() || self.path == "/org/a11y/atspi/null"
    }
}

async fn connect(bus: &Bus) -> Result<Connection, String> {
    let session = match bus {
        Bus::Address(address) => {
            return zbus::connection::Builder::address(address.as_str())
                .method_timeout(CALL_TIMEOUT)
                .build()
                .await
                .map_err(|e| e.to_string());
        }
        Bus::Given(address) => zbus::connection::Builder::address(address.as_str()),
        Bus::Session => zbus::connection::Builder::session(),
        Bus::Unknown => {
            return Err(
                "no session bus to ask it of: the desktop was started nested \
                        without --session-bus"
                    .into(),
            );
        }
    };
    let session = session
        .method_timeout(CALL_TIMEOUT)
        .build()
        .await
        .map_err(|e| e.to_string())?;
    let reply = session
        .call_method(
            Some("org.a11y.Bus"),
            "/org/a11y/bus",
            Some("org.a11y.Bus"),
            "GetAddress",
            &(),
        )
        .await
        .map_err(|e| format!("org.a11y.Bus: {e}"))?;
    let address: String = reply.body().deserialize().map_err(|e| e.to_string())?;
    zbus::connection::Builder::address(address.as_str())
        .method_timeout(CALL_TIMEOUT)
        .build()
        .await
        .map_err(|e| e.to_string())
}

/// `window`'s tree, on the bus `bus` (connected to once, and again after
/// it failed).
async fn read(bus: &Bus, connection: &mut Option<Connection>, window: &Window) -> Outcome {
    if connection.is_none() {
        *connection = Some(connect(bus).await.map_err(Missing::NoBus)?);
    }
    let Some(conn) = connection.clone() else {
        return Err(Missing::NoBus("not connected".into()));
    };
    let registry = Ref {
        bus: REGISTRY.into(),
        path: ROOT.into(),
    };
    let apps = match children(&conn, &registry).await {
        Ok(apps) => apps,
        Err(err) => {
            // Connected again at the next reading.
            *connection = None;
            return Err(Missing::NoBus(format!("its registry: {err}")));
        }
    };
    let started = Instant::now();
    let (app, toplevel) = find_window(&conn, &apps, window).await?;
    let toolkit = property_at::<String>(&conn, &app.0, APPLICATION, "ToolkitName")
        .await
        .ok()
        .filter(|t| !t.is_empty());
    let (nodes, cut) = walk(&conn, &toplevel, started).await;
    Ok(Tree {
        application: app.1,
        toolkit,
        nodes,
        cut,
    })
}

/// An application seen on the bus: its root, name and process.
#[derive(Debug, Clone)]
struct App {
    root: Ref,
    name: String,
    pid: Option<u32>,
}

/// The application of `window`, and its top-level object that is the
/// window.
async fn find_window(
    conn: &Connection,
    apps: &[Ref],
    window: &Window,
) -> Result<((Ref, String), Ref), Missing> {
    let mut seen = Vec::new();
    for root in apps.iter().filter(|r| !r.is_null()) {
        let pid = pid_of(conn, &root.bus).await;
        seen.push(App {
            root: root.clone(),
            name: String::new(),
            pid,
        });
    }
    let mut candidates: Vec<App> = seen
        .iter()
        .filter(|a| window.pid.is_some() && a.pid == window.pid)
        .cloned()
        .collect();
    if candidates.is_empty() {
        // A sandboxed application talks through a proxy of another
        // process: by its name then.
        for app in &mut seen {
            app.name = name(conn, &app.root).await.unwrap_or_default();
        }
        candidates = seen
            .into_iter()
            .filter(|a| {
                window
                    .app_id
                    .as_deref()
                    .is_some_and(|id| app_named(&a.name, id))
            })
            .collect();
    }
    if candidates.is_empty() {
        return Err(Missing::NoApplication);
    }
    let mut toplevels = Vec::new();
    for app in &mut candidates {
        if app.name.is_empty() {
            app.name = name(conn, &app.root).await.unwrap_or_default();
        }
        for top in children(conn, &app.root).await.unwrap_or_default() {
            let title = name(conn, &top).await.unwrap_or_default();
            let states = states(conn, &top).await.unwrap_or_default();
            toplevels.push((app.clone(), top, title, states));
        }
    }
    let application = candidates
        .iter()
        .map(|a| a.name.clone())
        .find(|n| !n.is_empty())
        .unwrap_or_else(|| window.app_id.clone().unwrap_or_default());
    let titled: Vec<&(App, Ref, String, States)> = toplevels
        .iter()
        .filter(|(_, _, t, _)| {
            window
                .title
                .as_deref()
                .is_some_and(|w| !w.is_empty() && t == w)
        })
        .collect();
    let chosen = match titled.as_slice() {
        [] => {
            let shown: Vec<_> = toplevels.iter().filter(|(.., s)| s.visible()).collect();
            match shown.as_slice() {
                [one] if window.alone => Some(*one),
                _ => None,
            }
        }
        [one] => Some(*one),
        // Two windows of one title: the one of the window's size.
        several => {
            let mut sized = None;
            for t in several {
                if let Some(e) = extents(conn, &t.1).await
                    && (e.width, e.height) == window.size
                {
                    sized = Some(*t);
                    break;
                }
            }
            sized.or(several.first().copied())
        }
    };
    match chosen {
        Some((app, top, ..)) => Ok(((app.root.clone(), app.name.clone()), top.clone())),
        None => Err(Missing::NoWindow { application }),
    }
}

/// Whether an application named `name` on the bus is that of `app_id`:
/// the same, or its last part (`org.gnome.TextEditor`, `TextEditor`).
fn app_named(name: &str, app_id: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && (name.eq_ignore_ascii_case(app_id)
            || app_id
                .rsplit('.')
                .next()
                .is_some_and(|last| name.eq_ignore_ascii_case(last)))
}

/// Reads the tree under `top`, breadth first within the limits, and
/// gives it depth first.
async fn walk(conn: &Connection, top: &Ref, started: Instant) -> (Vec<Node>, Option<Cut>) {
    struct Seen {
        node: Node,
        children: Vec<usize>,
    }
    let Some(root) = node(conn, top, 0).await else {
        return (Vec::new(), None);
    };
    let mut seen = vec![Seen {
        node: root,
        children: Vec::new(),
    }];
    let mut queue: VecDeque<(usize, Ref)> = VecDeque::from([(0, top.clone())]);
    let mut cut = None;
    'walk: while let Some((i, at)) = queue.pop_front() {
        if started.elapsed() > MAX_TIME {
            cut = Some(Cut::Time { limit: MAX_TIME });
            break;
        }
        let depth = seen.get(i).map_or(0, |s| s.node.depth) + 1;
        let kids = children(conn, &at).await.unwrap_or_default();
        if kids.is_empty() {
            continue;
        }
        if depth > MAX_DEPTH {
            cut.get_or_insert(Cut::Depth { limit: MAX_DEPTH });
            continue;
        }
        for (k, kid) in kids.iter().enumerate() {
            if seen.len() >= MAX_NODES {
                cut = Some(Cut::Nodes {
                    limit: MAX_NODES,
                    left: kids.len() - k,
                });
                break 'walk;
            }
            if kid.is_null() {
                continue;
            }
            let Some(node) = node(conn, kid, depth).await else {
                continue;
            };
            // Not showing: neither it nor what it holds.
            if !node.states.showing() {
                continue;
            }
            seen.push(Seen {
                node,
                children: Vec::new(),
            });
            let j = seen.len() - 1;
            if let Some(parent) = seen.get_mut(i) {
                parent.children.push(j);
            }
            queue.push_back((j, kid.clone()));
        }
    }
    let mut out = Vec::with_capacity(seen.len());
    let mut stack = vec![0];
    while let Some(i) = stack.pop() {
        if let Some(s) = seen.get(i) {
            stack.extend(s.children.iter().rev());
            out.push(s.node.clone());
        }
    }
    (out, cut)
}

/// One element: its role, label, states, place, and its value when it
/// holds what the user typed, unless it is for a secret.
async fn node(conn: &Connection, at: &Ref, depth: u32) -> Option<Node> {
    let role = Role(
        call::<u32>(conn, at, ACCESSIBLE, "GetRole", &())
            .await
            .ok()?,
    );
    let label = name(conn, at).await.ok().filter(|n| !n.trim().is_empty());
    let states = states(conn, at).await.unwrap_or_default();
    let rect = extents(conn, at).await;
    let mut node = Node {
        depth,
        role,
        label: label.map(|l| cut_text(&l)),
        rect,
        states,
        value: None,
    };
    if role.holds_input() && !node.secret() {
        node.value = text(conn, at).await;
    }
    Some(node)
}

/// What a text element holds, its first characters only.
async fn text(conn: &Connection, at: &Ref) -> Option<String> {
    let count = property_at::<i32>(conn, at, TEXT, "CharacterCount")
        .await
        .unwrap_or(-1);
    let end = if count < 0 {
        -1
    } else {
        count.min(MAX_TEXT as i32 + 1)
    };
    let body = call::<String>(conn, at, TEXT, "GetText", &(0_i32, end))
        .await
        .ok()?;
    Some(cut_text(&body))
}

fn cut_text(text: &str) -> String {
    if text.chars().count() <= MAX_TEXT {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(MAX_TEXT).collect();
    cut.push('…');
    cut
}

async fn extents(conn: &Connection, at: &Ref) -> Option<Extents> {
    let (x, y, width, height) =
        call::<(i32, i32, i32, i32)>(conn, at, COMPONENT, "GetExtents", &(WINDOW_COORDS,))
            .await
            .ok()?;
    // Unknown places come as -1 or empty.
    (width > 0 && height > 0 && x > i32::MIN / 2 && y > i32::MIN / 2).then_some(Extents {
        x,
        y,
        width,
        height,
    })
}

async fn states(conn: &Connection, at: &Ref) -> Result<States, String> {
    let words = call::<Vec<u32>>(conn, at, ACCESSIBLE, "GetState", &()).await?;
    Ok(States::from_words(&words))
}

async fn name(conn: &Connection, at: &Ref) -> Result<String, String> {
    property_at::<String>(conn, at, ACCESSIBLE, "Name").await
}

async fn children(conn: &Connection, at: &Ref) -> Result<Vec<Ref>, String> {
    let kids =
        call::<Vec<(String, OwnedObjectPath)>>(conn, at, ACCESSIBLE, "GetChildren", &()).await?;
    Ok(kids.into_iter().map(Ref::from_wire).collect())
}

async fn pid_of(conn: &Connection, bus: &str) -> Option<u32> {
    let reply = conn
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetConnectionUnixProcessID",
            &(bus,),
        )
        .await
        .ok()?;
    reply.body().deserialize::<u32>().ok()
}

async fn property_at<T>(
    conn: &Connection,
    at: &Ref,
    interface: &str,
    name: &str,
) -> Result<T, String>
where
    T: TryFrom<OwnedVariant>,
    T::Error: std::fmt::Display,
{
    let value: OwnedVariant = call(conn, at, PROPERTIES, "Get", &(interface, name)).await?;
    T::try_from(value).map_err(|e| e.to_string())
}

async fn call<T>(
    conn: &Connection,
    at: &Ref,
    interface: &str,
    method: &str,
    body: &(impl serde::Serialize + zbus::DynamicType),
) -> Result<T, String>
where
    T: for<'d> serde::Deserialize<'d> + zbus::Type,
{
    let reply = conn
        .call_method(
            Some(at.bus.as_str()),
            at.path.as_str(),
            Some(interface),
            method,
            body,
        )
        .await
        .map_err(|e| e.to_string())?;
    reply.body().deserialize::<T>().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOWN: u64 = 1 << 8 | 1 << 24 | 1 << 25 | 1 << 30;

    fn node(role: u32, label: &str) -> Node {
        Node {
            depth: 1,
            role: Role(role),
            label: Some(label.into()),
            rect: Some(Extents {
                x: 10,
                y: 20,
                width: 100,
                height: 40,
            }),
            states: States(SHOWN),
            value: None,
        }
    }

    #[test]
    fn an_exact_label_first_then_one_that_contains_it() {
        let nodes = [
            node(43, "Send"),
            node(43, "Send later"),
            node(29, "Subject"),
            node(79, "Subject"),
        ];
        assert_eq!(pick(&nodes, None, "Send"), Pick::One(&nodes[0]));
        assert_eq!(pick(&nodes, None, "later"), Pick::One(&nodes[1]));
        assert!(matches!(pick(&nodes, None, "send"), Pick::Many(m) if m.len() == 2));
        assert_eq!(pick(&nodes, Some("Entry"), "subj"), Pick::One(&nodes[3]));
        assert_eq!(pick(&nodes, None, "Archive"), Pick::Nothing);
        let tree = Tree {
            application: "mail".into(),
            toolkit: None,
            nodes: nodes.to_vec(),
            cut: None,
        };
        assert_eq!(centre_of(&tree, None, "Send"), Ok((60.0, 40.0)));
        assert!(centre_of(&tree, None, "Subject").is_err_and(|e| e.contains("2 elements")));
    }

    #[test]
    fn fields_for_secrets_are_known_by_role_or_label_and_never_hold_a_value() {
        assert!(node(40, "Code").secret());
        assert!(node(79, "Mot de passe :").secret());
        assert!(node(79, "PIN").secret());
        assert!(!node(79, "Spinach").secret());
        assert_eq!(Role(43).name(), "push button");
        assert!(node(79, "Subject").json().get("value").is_none());
    }

    #[test]
    fn applications_are_named_by_app_id_or_its_last_part() {
        assert!(app_named("texteditor", "org.gnome.TextEditor"));
        assert!(!app_named("", "foot"));
        assert!(!app_named("gedit", "org.gnome.TextEditor"));
    }
}
