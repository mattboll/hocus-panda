//! The shell's description of a surface, and its AccessKit tree.

use std::{collections::HashSet, fmt::Write as _};

use accesskit::{Live, NodeId, Toggled, TreeId, TreeInfo, TreeUpdate};

/// What a node is, in the words screen readers use (AT-SPI's roles).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// A dialog: a popover, the authentication dialog.
    Dialog,
    /// A notification's bubble: Orca reads it as it comes.
    Notification,
    Button,
    /// A button that stays pressed or not ("Couper le son"): AT-SPI's
    /// toggle button.
    ToggleButton,
    CheckBox,
    RadioButton,
    /// Radio buttons together: the desktops.
    RadioGroup,
    Slider,
    ProgressBar,
    List,
    ListItem,
    Group,
    Toolbar,
    PasswordField,
    TextField,
    Label,
    /// What is said on its own, politely (an agent that waits): AT-SPI's
    /// status bar, which announces its name as it comes.
    Status,
}

impl Role {
    fn kit(self) -> accesskit::Role {
        use accesskit::Role as R;
        match self {
            Self::Dialog => R::Dialog,
            // AccessKit's alert is AT-SPI's notification.
            Self::Notification => R::Alert,
            // A button with a toggled state is AT-SPI's toggle button.
            Self::Button | Self::ToggleButton => R::Button,
            Self::CheckBox => R::CheckBox,
            Self::RadioButton => R::RadioButton,
            Self::RadioGroup => R::RadioGroup,
            Self::Slider => R::Slider,
            Self::ProgressBar => R::ProgressIndicator,
            Self::List => R::List,
            Self::ListItem => R::ListItem,
            Self::Group => R::Group,
            Self::Toolbar => R::Toolbar,
            Self::PasswordField => R::PasswordInput,
            Self::TextField => R::TextInput,
            Self::Label => R::Label,
            Self::Status => R::Status,
        }
    }

    /// Its name in the description's text ([`Window::dump`]).
    pub fn word(self) -> &'static str {
        match self {
            Self::Dialog => "dialog",
            Self::Notification => "notification",
            Self::Button => "button",
            Self::ToggleButton => "toggle button",
            Self::CheckBox => "check box",
            Self::RadioButton => "radio button",
            Self::RadioGroup => "radio group",
            Self::Slider => "slider",
            Self::ProgressBar => "progress bar",
            Self::List => "list",
            Self::ListItem => "list item",
            Self::Group => "group",
            Self::Toolbar => "toolbar",
            Self::PasswordField => "password text",
            Self::TextField => "text",
            Self::Label => "label",
            Self::Status => "status",
        }
    }
}

/// What a screen reader can do on a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Act {
    /// Its default action: what a click does.
    Click,
    Focus,
    Increment,
    Decrement,
    /// Its menu (a tray icon's, a window's line).
    Menu,
}

impl Act {
    fn kit(self) -> accesskit::Action {
        match self {
            Self::Click => accesskit::Action::Click,
            Self::Focus => accesskit::Action::Focus,
            Self::Increment => accesskit::Action::Increment,
            Self::Decrement => accesskit::Action::Decrement,
            Self::Menu => accesskit::Action::ShowContextMenu,
        }
    }

    pub(crate) fn of(action: accesskit::Action) -> Option<Self> {
        Some(match action {
            accesskit::Action::Click => Self::Click,
            accesskit::Action::Focus => Self::Focus,
            accesskit::Action::Increment => Self::Increment,
            accesskit::Action::Decrement => Self::Decrement,
            accesskit::Action::ShowContextMenu => Self::Menu,
            _ => return None,
        })
    }

    fn word(self) -> &'static str {
        match self {
            Self::Click => "click",
            Self::Focus => "focus",
            Self::Increment => "increment",
            Self::Decrement => "decrement",
            Self::Menu => "menu",
        }
    }
}

/// A rectangle in its surface's logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn kit(self) -> accesskit::Rect {
        accesskit::Rect {
            x0: f64::from(self.x),
            y0: f64::from(self.y),
            x1: f64::from(self.x + self.width),
            y1: f64::from(self.y + self.height),
        }
    }
}

/// A slider's or a gauge's number: AT-SPI's value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
    pub value: f64,
    pub min: f64,
    pub max: f64,
    /// What one step of the arrows moves it by.
    pub step: Option<f64>,
}

/// One thing shown: an item of the bar, a popover's row, a field.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Unique in its window, and the same from one description to the
    /// next: what a screen reader's action comes back by.
    pub key: String,
    pub role: Role,
    /// The words drawn, whole ("62 %", not "62").
    pub name: String,
    pub description: Option<String>,
    /// What it holds, as words ("62 %", "en charge").
    pub value: Option<String>,
    pub range: Option<Range>,
    /// Checked, ticked or pressed; `None` when it cannot be.
    pub checked: Option<bool>,
    pub selected: Option<bool>,
    pub disabled: bool,
    /// Its place among its kind, from 1: "2 sur 4".
    pub set: Option<(usize, usize)>,
    pub bounds: Option<Rect>,
    pub acts: Vec<Act>,
    /// Said as it comes or changes, politely.
    pub polite: bool,
    pub children: Vec<Node>,
}

impl Node {
    pub fn new(key: impl Into<String>, role: Role, name: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            role,
            name: name.into(),
            description: None,
            value: None,
            range: None,
            checked: None,
            selected: None,
            disabled: false,
            set: None,
            bounds: None,
            acts: Vec::new(),
            polite: false,
            children: Vec::new(),
        }
    }

    pub fn describe(mut self, description: impl Into<String>) -> Self {
        let description = description.into();
        self.description = (!description.is_empty()).then_some(description);
        self
    }

    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    pub fn range(mut self, value: f64, min: f64, max: f64, step: Option<f64>) -> Self {
        self.range = Some(Range {
            value,
            min,
            max,
            step,
        });
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = Some(selected);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn set(mut self, position: usize, size: usize) -> Self {
        self.set = Some((position, size));
        self
    }

    pub fn bounds(mut self, bounds: Option<Rect>) -> Self {
        self.bounds = bounds;
        self
    }

    pub fn act(mut self, act: Act) -> Self {
        if !self.acts.contains(&act) {
            self.acts.push(act);
        }
        self
    }

    pub fn polite(mut self) -> Self {
        self.polite = true;
        self
    }

    pub fn child(mut self, child: Node) -> Self {
        self.children.push(child);
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = Node>) -> Self {
        self.children.extend(children);
        self
    }

    /// The node of `key` here or under it.
    pub fn find(&self, key: &str) -> Option<&Node> {
        if self.key == key {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(key))
    }

    fn kit(&self, children: Vec<NodeId>) -> accesskit::Node {
        let mut node = accesskit::Node::new(self.role.kit());
        node.set_label(self.name.clone());
        if let Some(description) = &self.description {
            node.set_description(description.clone());
        }
        if let Some(value) = &self.value {
            node.set_value(value.clone());
        }
        if let Some(range) = self.range {
            node.set_numeric_value(range.value);
            node.set_min_numeric_value(range.min);
            node.set_max_numeric_value(range.max);
            if let Some(step) = range.step {
                node.set_numeric_value_step(step);
            }
        }
        match (self.role, self.checked) {
            (_, Some(checked)) => node.set_toggled(Toggled::from(checked)),
            // A toggle button is one, pressed or not.
            (Role::ToggleButton, None) => node.set_toggled(Toggled::False),
            _ => {}
        }
        if let Some(selected) = self.selected {
            node.set_selected(selected);
        }
        if self.disabled {
            node.set_disabled();
        }
        if let Some((position, size)) = self.set {
            node.set_position_in_set(position);
            node.set_size_of_set(size);
        }
        if let Some(bounds) = self.bounds {
            node.set_bounds(bounds.kit());
        }
        for act in &self.acts {
            node.add_action(act.kit());
        }
        if self.polite {
            node.set_live(Live::Polite);
        }
        if self.role == Role::Dialog {
            node.set_modal();
        }
        node.set_children(children);
        node
    }

    fn dump(&self, depth: usize, out: &mut String) {
        let _ = write!(
            out,
            "{:indent$}{} {:?}",
            "",
            self.role.word(),
            self.name,
            indent = depth * 2
        );
        if let Some(value) = &self.value {
            let _ = write!(out, " = {value:?}");
        }
        if let Some(range) = self.range {
            let _ = write!(out, " ({} in {}..{}", range.value, range.min, range.max);
            if let Some(step) = range.step {
                let _ = write!(out, " by {step}");
            }
            out.push(')');
        }
        if let Some(description) = &self.description {
            let _ = write!(out, " — {description:?}");
        }
        let mut states = Vec::new();
        match self.checked {
            Some(true) if self.role == Role::ToggleButton => states.push("pressed"),
            Some(true) => states.push("checked"),
            Some(false) if self.role == Role::ToggleButton => states.push("not pressed"),
            Some(false) => states.push("not checked"),
            None => {}
        }
        if self.selected == Some(true) {
            states.push("selected");
        }
        if self.disabled {
            states.push("disabled");
        }
        if self.polite {
            states.push("polite");
        }
        if !states.is_empty() {
            let _ = write!(out, " [{}]", states.join(", "));
        }
        if let Some((position, size)) = self.set {
            let _ = write!(out, " {position}/{size}");
        }
        if let Some(b) = self.bounds {
            let _ = write!(out, " @{},{} {}x{}", b.x, b.y, b.width, b.height);
        }
        if !self.acts.is_empty() {
            let acts: Vec<&str> = self.acts.iter().map(|a| a.word()).collect();
            let _ = write!(out, " <{}>", acts.join(", "));
        }
        out.push('\n');
        for child in &self.children {
            child.dump(depth + 1, out);
        }
    }
}

/// One of the shell's surfaces, as a top-level window: a bar, a popover, a
/// bubble, the lock on a screen, the authentication dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    /// Unique among the shell's windows, and the same while it shows.
    pub key: String,
    pub name: String,
    pub nodes: Vec<Node>,
    /// The node holding the keyboard, by its key.
    pub focus: Option<String>,
    /// The window holds the keyboard.
    pub active: bool,
    /// Its surface's size.
    pub size: Option<(f32, f32)>,
}

impl Window {
    pub fn new(key: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            name: name.into(),
            nodes: Vec::new(),
            focus: None,
            active: false,
            size: None,
        }
    }

    /// The node of `key`, wherever it is.
    pub fn find(&self, key: &str) -> Option<&Node> {
        self.nodes.iter().find_map(|n| n.find(key))
    }

    /// Every node, depth first.
    pub fn all(&self) -> Vec<&Node> {
        fn walk<'a>(node: &'a Node, out: &mut Vec<&'a Node>) {
            out.push(node);
            for child in &node.children {
                walk(child, out);
            }
        }
        let mut out = Vec::new();
        for node in &self.nodes {
            walk(node, &mut out);
        }
        out
    }

    /// Its whole tree for AccessKit, every node with an id made from its
    /// key: a key that comes twice gets a second one, so that the tree is
    /// always one AccessKit takes.
    pub fn update(&self) -> TreeUpdate {
        let root = id_of("");
        let mut taken = HashSet::from([root]);
        let mut nodes = Vec::new();
        let mut focus = None;
        let children: Vec<NodeId> = self
            .nodes
            .iter()
            .map(|n| self.add(n, &mut taken, &mut nodes, &mut focus))
            .collect();
        let mut window = accesskit::Node::new(accesskit::Role::Window);
        window.set_label(self.name.clone());
        if let Some((width, height)) = self.size {
            window.set_bounds(Rect::new(0.0, 0.0, width, height).kit());
        }
        window.set_children(children);
        nodes.push((root, window));
        TreeUpdate {
            nodes,
            tree: Some(TreeInfo {
                root,
                toolkit_name: Some("Hocuspanda".to_owned()),
                toolkit_version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }),
            tree_id: TreeId::ROOT,
            focus: focus.unwrap_or(root),
        }
    }

    fn add(
        &self,
        node: &Node,
        taken: &mut HashSet<NodeId>,
        nodes: &mut Vec<(NodeId, accesskit::Node)>,
        focus: &mut Option<NodeId>,
    ) -> NodeId {
        let mut id = id_of(&node.key);
        let mut again = 1u32;
        while !taken.insert(id) {
            id = id_of(&format!("{}#{again}", node.key));
            again += 1;
        }
        if self.focus.as_deref() == Some(node.key.as_str()) && focus.is_none() {
            *focus = Some(id);
        }
        let children: Vec<NodeId> = node
            .children
            .iter()
            .map(|c| self.add(c, taken, nodes, focus))
            .collect();
        nodes.push((id, node.kit(children)));
        id
    }

    /// The keys of its nodes by their AccessKit ids, as [`Self::update`]
    /// gives them: what a screen reader's action names.
    pub(crate) fn keys(&self) -> Vec<(NodeId, String)> {
        let mut taken = HashSet::from([id_of("")]);
        let mut out = Vec::new();
        for node in self.all() {
            let mut id = id_of(&node.key);
            let mut again = 1u32;
            while !taken.insert(id) {
                id = id_of(&format!("{}#{again}", node.key));
                again += 1;
            }
            out.push((id, node.key.clone()));
        }
        out
    }

    /// The tree as text, one node a line: role, name, value, states, place
    /// among its kind, bounds and actions; the focused node marked.
    pub fn dump(&self) -> String {
        let mut out = format!("window {:?}", self.name);
        if self.active {
            out.push_str(" [active]");
        }
        if let Some(focus) = &self.focus {
            let _ = write!(out, " focus={focus}");
        }
        out.push('\n');
        for node in &self.nodes {
            node.dump(1, &mut out);
        }
        out
    }
}

/// A node's id, from its key: FNV-1a, never 0's.
fn id_of(key: &str) -> NodeId {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    NodeId(hash.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar() -> Window {
        let mut window = Window::new("bar:eDP-1", "Barre — eDP-1");
        window.nodes = vec![
            Node::new("clock", Role::Button, "14 h 32, jeudi 6 novembre").act(Act::Click),
            Node::new("volume", Role::Slider, "Volume")
                .value("62 %")
                .range(62.0, 0.0, 100.0, Some(5.0))
                .act(Act::Increment)
                .act(Act::Decrement),
            Node::new("desktops", Role::RadioGroup, "Bureaux").children([
                Node::new("desktop:0", Role::RadioButton, "Bureau 1, actuel")
                    .checked(true)
                    .set(1, 2),
                Node::new("desktop:1", Role::RadioButton, "Bureau 2").set(2, 2),
            ]),
        ];
        window.focus = Some("volume".to_owned());
        window
    }

    #[test]
    fn the_tree_holds_every_node_under_its_window() {
        let update = bar().update();
        assert_eq!(update.nodes.len(), 6);
        let root = update.tree.as_ref().map(|t| t.root);
        let (_, window) = update
            .nodes
            .iter()
            .find(|(id, _)| Some(*id) == root)
            .expect("the root");
        assert_eq!(window.role(), accesskit::Role::Window);
        assert_eq!(window.label(), Some("Barre — eDP-1"));
        assert_eq!(window.children().len(), 3);
        let volume = update
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some("Volume"))
            .map(|(id, n)| (*id, n))
            .expect("the volume");
        assert_eq!(volume.1.role(), accesskit::Role::Slider);
        assert_eq!(volume.1.numeric_value(), Some(62.0));
        assert_eq!(volume.1.value(), Some("62 %"));
        assert!(volume.1.supports_action(accesskit::Action::Increment));
        assert_eq!(update.focus, volume.0);
        let current = update
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some("Bureau 1, actuel"))
            .expect("the desktop");
        assert_eq!(current.1.toggled(), Some(Toggled::True));
        assert_eq!(current.1.position_in_set(), Some(1));
        assert_eq!(current.1.size_of_set(), Some(2));
    }

    #[test]
    fn a_key_given_twice_still_makes_a_tree() {
        let mut window = Window::new("w", "W");
        window.nodes = vec![
            Node::new("same", Role::Button, "A"),
            Node::new("same", Role::Button, "B"),
        ];
        let update = window.update();
        let ids: HashSet<NodeId> = update.nodes.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids.len(), 3);
        let keys = window.keys();
        assert_eq!(keys.len(), 2);
        assert_ne!(keys[0].0, keys[1].0);
        // The same ids as the tree's.
        assert!(keys.iter().all(|(id, _)| ids.contains(id)));
    }

    #[test]
    fn no_focus_is_the_window_itself() {
        let mut window = bar();
        window.focus = Some("gone".to_owned());
        let update = window.update();
        assert_eq!(Some(update.focus), update.tree.map(|t| t.root));
    }

    #[test]
    fn the_dump_says_roles_names_values_and_states() {
        let dump = bar().dump();
        assert!(
            dump.contains("button \"14 h 32, jeudi 6 novembre\" <click>"),
            "{dump}"
        );
        assert!(
            dump.contains(
                "slider \"Volume\" = \"62 %\" (62 in 0..100 by 5) <increment, decrement>"
            ),
            "{dump}"
        );
        assert!(
            dump.contains("radio button \"Bureau 1, actuel\" [checked] 1/2"),
            "{dump}"
        );
        assert!(dump.contains("focus=volume"), "{dump}");
    }
}
