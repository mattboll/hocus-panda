//! Applications' own accessibility trees for agents (#175): read over
//! AT-SPI by [`hocus_platform::atspi`] on a thread of its own, started at
//! the first request, and answered later (the request's handler returns
//! `None`). An agent reads the trees of the windows it may act on alone,
//! and not while the session is locked; reading is not kept in the
//! journal, which keeps actions.
//!
//! `click` and `scroll` may aim at an element of a window's tree by label:
//! the tree is read, the element found, and the action goes on as one at
//! the element's centre, its ring around the element.

use std::collections::HashMap;

use hocus_api::{
    ActTarget, Action, ApiError, AppElement, AppTree, ConnectionId, ErrorCode, JournalEntry, Rect,
    Reply,
};
use hocus_core::WindowId;
use hocus_platform::atspi::{self, Bus, Node, Outcome, Pick, Reader};
use smithay::{
    reexports::{calloop::channel, wayland_server::Resource as _},
    wayland::seat::WaylandFocus as _,
};

use crate::{State, surface::title_and_app_id};

/// Candidates listed when a label is ambiguous.
const LISTED: usize = 8;

/// The reader and what waits on it.
#[derive(Debug)]
pub struct A11y {
    bus: Bus,
    /// Started at the first request.
    reader: Option<Reader>,
    next: u64,
    waiting: HashMap<u64, Waiting>,
}

impl Default for A11y {
    fn default() -> Self {
        Self {
            bus: Bus::Unknown,
            reader: None,
            next: 0,
            waiting: HashMap::new(),
        }
    }
}

#[derive(Debug)]
enum Waiting {
    /// A `window_tree` request.
    Tree {
        connection: ConnectionId,
        id: u64,
        window: u64,
    },
    /// An action aimed at an element, found once the tree is read.
    Element(Box<ElementWait>),
}

#[derive(Debug)]
struct ElementWait {
    connection: ConnectionId,
    id: u64,
    window: u64,
    role: Option<String>,
    label: String,
    action: Action,
    intent: Option<String>,
    /// An agent's: its journal entry, made when it asked.
    entry: Option<(String, (u8, u8, JournalEntry))>,
}

/// The element an action was aimed at, found: its label, and its
/// rectangle from the point the action lands on.
#[derive(Debug, Clone)]
pub(crate) struct Found {
    pub label: String,
    pub around: [i32; 4],
}

/// Where the accessibility bus is: given, or asked of the session's bus
/// when there is one (never the session's bus of a nested compositor).
pub fn start(state: &mut State, a11y_bus: Option<String>, session_bus: Option<&str>) {
    state.a11y.bus = match (a11y_bus, session_bus) {
        (Some(address), _) => Bus::Address(address),
        (None, Some(session)) => Bus::Session(session.to_owned()),
        (None, None) => Bus::Unknown,
    };
}

impl State {
    /// Reads applications' trees on this bus from now on (tests: a
    /// private one).
    pub fn set_a11y_bus(&mut self, address: String) {
        self.a11y.bus = Bus::Address(address);
        self.a11y.reader = None;
    }

    /// A window's own tree, answered once read; `Some` when refused.
    pub(crate) fn window_tree(
        &mut self,
        connection: ConnectionId,
        id: u64,
        window: u64,
    ) -> Option<Result<Reply, ApiError>> {
        if let Err(err) = self.may_read_tree(connection, window) {
            return Some(Err(err));
        }
        self.ask_tree(
            window,
            Waiting::Tree {
                connection,
                id,
                window,
            },
        )
        .err()
        .map(|(err, _)| Err(err))
    }

    /// An agent reads the trees of the windows of its apps (as #42's
    /// rules name them), whatever its access, and none while the session
    /// is locked. Any other client of the session reads them, as it may on
    /// the accessibility bus itself.
    fn may_read_tree(&self, connection: ConnectionId, window: u64) -> Result<(), ApiError> {
        let managed = self.windows.get(&WindowId(window)).ok_or_else(|| {
            ApiError::new(ErrorCode::NotFound, format!("no window with id {window}"))
        })?;
        let Some(name) = self.api.agent(connection) else {
            return Ok(());
        };
        let agent = self.config.agents.mcp_agent(name).ok_or_else(|| {
            ApiError::new(
                ErrorCode::PermissionDenied,
                format!("agent {name:?} is no longer in [[agents.mcp]]"),
            )
        })?;
        if self.locked() {
            return Err(ApiError::new(
                ErrorCode::PermissionDenied,
                "the session is locked: agents do not read applications' trees",
            ));
        }
        let app_id = title_and_app_id(&managed.window).1.unwrap_or_default();
        if !agent.may_read_tree(&app_id) {
            return Err(ApiError::new(
                ErrorCode::PermissionDenied,
                format!(
                    "agent {name:?} may not read windows of {app_id:?}: it reads the trees of \
                     its apps alone, and its apps in [[agents.mcp]] do not match it"
                ),
            ));
        }
        Ok(())
    }

    /// An action aimed at an element of a window's tree: checked as one
    /// aimed at the window, then carried out once the element is found.
    pub(crate) fn act_on_element(
        &mut self,
        connection: ConnectionId,
        id: u64,
        target: ActTarget,
        action: Action,
        intent: Option<String>,
    ) -> Option<Result<Reply, ApiError>> {
        let ActTarget::AppElement {
            window,
            role,
            label,
        } = target.clone()
        else {
            return Some(Err(ApiError::new(ErrorCode::BadRequest, "not an element")));
        };
        let agent = self.api.agent(connection).map(str::to_owned);
        let entry = agent
            .as_ref()
            .map(|name| (name.clone(), self.journal_entry(name, &target, &action)));
        let checked = if matches!(action, Action::Type { .. } | Action::Keys { .. }) {
            Err(ApiError::new(
                ErrorCode::BadRequest,
                "an element is clicked or scrolled: type into its window once it has it",
            ))
        } else if self.element_busy(connection) {
            Err(ApiError::new(
                ErrorCode::BadRequest,
                "one action at a time: the last one is not done yet",
            ))
        } else {
            match &agent {
                Some(name) => self
                    .agent_may(connection, name, window, &action)
                    .and_then(|()| self.may_read_tree(connection, window)),
                None => self.may_read_tree(connection, window),
            }
        };
        let waiting = Waiting::Element(Box::new(ElementWait {
            connection,
            id,
            window,
            role,
            label,
            action,
            intent,
            entry,
        }));
        let result = match checked {
            Err(err) => Err((err, waiting)),
            Ok(()) => self.ask_tree(window, waiting),
        };
        result.err().map(|(err, waiting)| {
            if let Waiting::Element(wait) = waiting
                && let Some((_, entry)) = wait.entry
            {
                self.journal_keep(entry, Some(&err));
            }
            Err(err)
        })
    }

    /// Whether this connection waits on an element or on an action.
    fn element_busy(&self, connection: ConnectionId) -> bool {
        self.resolving(connection) || self.seat_busy(connection)
    }

    /// Whether an element this connection aims at is being looked for.
    pub(crate) fn resolving(&self, connection: ConnectionId) -> bool {
        self.a11y
            .waiting
            .values()
            .any(|w| matches!(w, Waiting::Element(e) if e.connection == connection))
    }

    /// Hands the window's description to the reader, started now if it
    /// is not yet.
    fn ask_tree(&mut self, window: u64, waiting: Waiting) -> Result<(), (ApiError, Waiting)> {
        let query = self.tree_query(window);
        if self.a11y.reader.is_none() {
            match self.start_reader() {
                Ok(reader) => self.a11y.reader = Some(reader),
                Err(err) => return Err((err, waiting)),
            }
        }
        let token = self.a11y.next;
        self.a11y.next += 1;
        let sent = self
            .a11y
            .reader
            .as_ref()
            .is_some_and(|r| r.read(token, query));
        if !sent {
            self.a11y.reader = None;
            return Err((
                ApiError::new(ErrorCode::Failed, "the accessibility reader stopped"),
                waiting,
            ));
        }
        self.a11y.waiting.insert(token, waiting);
        Ok(())
    }

    fn start_reader(&mut self) -> Result<Reader, ApiError> {
        let failed = |why: String| ApiError::new(ErrorCode::Failed, why);
        let handle = self
            .loop_handle
            .clone()
            .ok_or_else(|| failed("the compositor is not running".into()))?;
        let (tx, rx) = channel::channel::<(u64, Outcome)>();
        handle
            .insert_source(rx, |event, _, app| {
                if let channel::Event::Msg((token, outcome)) = event {
                    app.timed("accessibility tree", |app| {
                        app.state.tree_read(token, outcome)
                    });
                }
            })
            .map_err(|err| failed(format!("could not wait for trees: {}", err.error)))?;
        let tx = std::sync::Mutex::new(tx);
        Reader::start(self.a11y.bus.clone(), move |token, outcome| {
            if let Ok(tx) = tx.lock() {
                // The loop is gone if this fails: nobody waits.
                let _ = tx.send((token, outcome));
            }
        })
        .map_err(|err| failed(format!("could not start the accessibility reader: {err}")))
    }

    /// What the reader is told of `window`: its process, app id, title,
    /// size, and whether it is its process's only window.
    fn tree_query(&self, window: u64) -> atspi::Window {
        let Some(managed) = self.windows.get(&WindowId(window)) else {
            return atspi::Window::default();
        };
        let pid_of = |w: &smithay::desktop::Window| {
            w.wl_surface()
                .and_then(|s| s.client())
                .and_then(|c| c.get_credentials(&self.display_handle).ok())
                .and_then(|c| u32::try_from(c.pid).ok())
        };
        let (title, app_id) = title_and_app_id(&managed.window);
        let pid = pid_of(&managed.window);
        let alone = self
            .windows
            .values()
            .filter(|m| match pid {
                Some(pid) => pid_of(&m.window) == Some(pid),
                None => title_and_app_id(&m.window).1 == app_id,
            })
            .count()
            == 1;
        let size = managed.window.geometry().size;
        atspi::Window {
            pid,
            app_id,
            title,
            size: (size.w, size.h),
            alone,
        }
    }

    /// A tree read: the request answered, or the action aimed.
    fn tree_read(&mut self, token: u64, outcome: Outcome) {
        let Some(waiting) = self.a11y.waiting.remove(&token) else {
            return;
        };
        match waiting {
            Waiting::Tree {
                connection,
                id,
                window,
            } => {
                let tree = app_tree(window, outcome);
                self.api.reply(connection, id, Ok(Reply::AppTree(tree)));
            }
            Waiting::Element(wait) => {
                let ElementWait {
                    connection,
                    id,
                    window,
                    role,
                    label,
                    action,
                    intent,
                    entry,
                } = *wait;
                let found = outcome
                    .map_err(|missing| {
                        ApiError::new(
                            ErrorCode::NotFound,
                            format!("window {window} has no accessibility tree: {missing}"),
                        )
                    })
                    .and_then(|tree| find_element(&tree.nodes, role.as_deref(), &label));
                let result = found.and_then(|(at, found)| {
                    // Checked again: a while went by.
                    if entry.is_some() && self.agents_stopped() {
                        return Err(crate::journal::stopped());
                    }
                    Ok((at, found))
                });
                let target = |at: [f64; 2]| ActTarget::Window {
                    window,
                    at: Some(hocus_api::Point { x: at[0], y: at[1] }),
                };
                let reply = match (result, entry) {
                    (Err(err), Some((_, entry))) => {
                        self.journal_keep(entry, Some(&err));
                        Some(Err(err))
                    }
                    (Err(err), None) => Some(Err(err)),
                    (Ok((at, found)), Some((name, entry))) => {
                        if self.api.agent(connection).is_none() {
                            let gone =
                                ApiError::new(ErrorCode::Failed, "the agent went away first");
                            self.journal_keep(entry, Some(&gone));
                            return;
                        }
                        self.agent_act_aimed(
                            connection,
                            id,
                            &name,
                            target(at),
                            action,
                            intent,
                            entry,
                            Some(found),
                        )
                    }
                    (Ok((at, _)), None) => Some(self.carry_out(target(at), action)),
                };
                if let Some(result) = reply {
                    self.api.reply(connection, id, result);
                }
                self.needs_redraw = true;
            }
        }
    }
}

/// The element `label` (and `role`) names in a tree, and the point to
/// aim at: its centre, from the window's top-left corner.
fn find_element(
    nodes: &[Node],
    role: Option<&str>,
    label: &str,
) -> Result<([f64; 2], Found), ApiError> {
    let of = || match role {
        Some(role) => format!("no {role} labelled {label:?}"),
        None => format!("no element labelled {label:?}"),
    };
    let node = match atspi::pick(nodes, role, label) {
        Pick::One(node) => node,
        Pick::Nothing => {
            return Err(ApiError::new(
                ErrorCode::NotFound,
                format!(
                    "the window's tree has {}: read it with accessibility_tree",
                    of()
                ),
            ));
        }
        Pick::Many(nodes) => {
            let listed: Vec<String> = nodes
                .iter()
                .take(LISTED)
                .map(|n| {
                    let place = n.rect.map_or_else(
                        || "no place".to_owned(),
                        |r| format!("at {},{} {}×{}", r.x, r.y, r.width, r.height),
                    );
                    format!(
                        "{} {:?} ({place})",
                        n.role.name(),
                        n.label.as_deref().unwrap_or_default()
                    )
                })
                .collect();
            let more = nodes.len().saturating_sub(LISTED);
            let more = if more > 0 {
                format!(", and {more} more")
            } else {
                String::new()
            };
            return Err(ApiError::new(
                ErrorCode::BadRequest,
                format!(
                    "{} elements match {label:?}: {}{more}; say which with role, or the whole \
                     label",
                    nodes.len(),
                    listed.join("; ")
                ),
            ));
        }
    };
    let name = node.label.clone().unwrap_or_else(|| label.to_owned());
    if !node.states.enabled() {
        return Err(ApiError::new(
            ErrorCode::Failed,
            format!("{name:?} is disabled"),
        ));
    }
    let Some(r) = node.rect else {
        return Err(ApiError::new(
            ErrorCode::Failed,
            format!(
                "its application does not say where {name:?} is: capture the window and aim at a \
                 point of it"
            ),
        ));
    };
    let (cx, cy) = (
        f64::from(r.x) + f64::from(r.width) / 2.0,
        f64::from(r.y) + f64::from(r.height) / 2.0,
    );
    let around = [
        r.x - cx.floor() as i32,
        r.y - cy.floor() as i32,
        r.width,
        r.height,
    ];
    Ok((
        [cx.floor(), cy.floor()],
        Found {
            label: name,
            around,
        },
    ))
}

/// The tree as the control API gives it.
fn app_tree(window: u64, outcome: Outcome) -> AppTree {
    match outcome {
        Ok(tree) => AppTree {
            window,
            application: Some(tree.application),
            toolkit: tree.toolkit,
            elements: tree
                .nodes
                .iter()
                .map(|n| AppElement {
                    role: n.role.name(),
                    label: n.label.clone(),
                    rect: n.rect.map(|r| Rect {
                        x: r.x,
                        y: r.y,
                        width: r.width,
                        height: r.height,
                    }),
                    depth: n.depth,
                    focused: n.states.focused(),
                    disabled: !n.states.enabled(),
                    selected: n.states.selected(),
                    checked: n.role.checkable().then(|| n.states.checked()),
                    // Never a secret's, whatever the reader brought.
                    value: n.value.clone().filter(|_| !n.secret()),
                })
                .collect(),
            partial: tree.cut.map(|c| c.to_string()),
            missing: None,
        },
        Err(missing) => AppTree {
            window,
            missing: Some(missing.to_string()),
            ..AppTree::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use hocus_platform::atspi::{Extents, Role, States};

    use super::*;

    fn button(label: &str, rect: Option<Extents>, enabled: bool) -> Node {
        let states = if enabled {
            States::default().with(&States::SHOWN)
        } else {
            States::default()
        };
        Node {
            depth: 2,
            role: Role::PUSH_BUTTON,
            label: Some(label.into()),
            rect,
            states,
            value: None,
        }
    }

    #[test]
    fn an_element_is_aimed_at_its_centre_and_its_ring_surrounds_it() -> Result<(), ApiError> {
        let rect = Extents {
            x: 284,
            y: 236,
            width: 100,
            height: 40,
        };
        let nodes = [button("Send", Some(rect), true)];
        let (at, found) = find_element(&nodes, None, "Send")?;
        assert_eq!(at, [334.0, 256.0]);
        assert_eq!(found.around, [-50, -20, 100, 40]);
        Ok(())
    }

    #[test]
    fn ambiguous_disabled_and_placeless_elements_are_refused() {
        let rect = Some(Extents {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        });
        let nodes = [
            button("Send", rect, true),
            button("Send later", rect, true),
            button("Delete", rect, false),
            button("Archive", None, true),
        ];
        let err = find_element(&nodes, None, "send").err().map(|e| e.message);
        assert_eq!(
            err.as_deref(),
            Some(
                "2 elements match \"send\": push button \"Send\" (at 0,0 10×10); push button \
                 \"Send later\" (at 0,0 10×10); say which with role, or the whole label"
            )
        );
        let err = find_element(&nodes, None, "Delete")
            .err()
            .map(|e| e.message);
        assert_eq!(err.as_deref(), Some("\"Delete\" is disabled"));
        let err = find_element(&nodes, None, "Archive")
            .err()
            .map(|e| e.message);
        assert!(err.is_some_and(|e| e.contains("does not say where")));
        let err = find_element(&nodes, Some("entry"), "Send")
            .err()
            .map(|e| e.message);
        assert_eq!(
            err.as_deref(),
            Some(
                "the window's tree has no entry labelled \"Send\": read it with accessibility_tree"
            )
        );
    }

    #[test]
    fn a_secrets_value_never_leaves() {
        let tree = atspi::Tree {
            application: "mail".into(),
            toolkit: None,
            nodes: vec![Node {
                depth: 1,
                role: Role::PASSWORD_TEXT,
                label: Some("Code".into()),
                rect: None,
                states: States::default(),
                value: Some("hunter2".into()),
            }],
            cut: None,
        };
        let tree = app_tree(1, Ok(tree));
        assert_eq!(tree.elements[0].value, None);
        assert_eq!(tree.elements[0].role, "password text");
    }
}
