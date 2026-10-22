//! The retained widget tree: nodes laid out like flexbox rows and columns,
//! drawn only where something changed.
//!
//! A frame goes: [`Tree::update`] (animations sampled at a time, layout,
//! damage), then [`Surface::draw`] of the damaged regions. Input goes
//! through [`Tree::input`], which returns what the user activated.

use std::{fmt, sync::Arc, time::Duration};

use vello_cpu::{Pixmap, kurbo::BezPath};

use crate::{
    access::{AccessNode, Role},
    canvas::Canvas,
    geom::{Insets, PixelRect, Point, Rect, Size},
    motion::{Animated, Curve},
    text::{Fonts, TextBox},
    theme::{Color, Paint, TextStyle, Theme},
};

/// A node of a tree; stale ids (of removed nodes) are refused, not reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId {
    index: u32,
    generation: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    Column,
    Row,
}

/// A size along one axis.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Length {
    /// As large as the content.
    #[default]
    Auto,
    Px(f32),
    /// What is left, shared with the other `Fill` children by `grow`.
    Fill,
}

/// Where children go across the main axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Stretch,
    Start,
    Center,
    End,
}

/// Where children go along the main axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    pub direction: Direction,
    pub width: Length,
    pub height: Length,
    pub padding: Insets,
    pub gap: f32,
    pub align: Align,
    pub justify: Justify,
    pub grow: f32,
    /// Children are cut to this box (scrolling, rounded cards).
    pub clip: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            direction: Direction::Column,
            width: Length::Auto,
            height: Length::Auto,
            padding: Insets::default(),
            gap: 0.0,
            align: Align::Stretch,
            justify: Justify::Start,
            grow: 1.0,
            clip: false,
        }
    }
}

/// How a box looks; children draw on top.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Look {
    pub background: Option<Paint>,
    pub radius: f32,
    /// A 1 px line inside the edge.
    pub border: Option<Paint>,
    /// Blur of a shadow under the box, 0 for none.
    pub shadow: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonKind {
    /// On the accent colour: the main action.
    Primary,
    /// A surface with a border.
    #[default]
    Plain,
    /// A tinted surface without a border: icon buttons, status chips.
    Chip,
    /// Nothing until hovered: menu items.
    Ghost,
}

/// A vector icon: SVG path data in a `view`×`view` box, filled or stroked.
#[derive(Debug, Clone)]
pub struct Icon {
    path: BezPath,
    view: f32,
    pub size: f32,
    /// Line width in the box's units, or `None` to fill.
    stroke: Option<f32>,
}

impl Icon {
    /// Filled, from a 24×24 box.
    pub fn filled(svg_path: &str, size: f32) -> Option<Self> {
        Some(Self {
            path: BezPath::from_svg(svg_path).ok()?,
            view: 24.0,
            size,
            stroke: None,
        })
    }

    /// Stroked with round ends, from a 24×24 box.
    pub fn stroked(svg_path: &str, size: f32, width: f32) -> Option<Self> {
        Some(Self {
            stroke: Some(width),
            ..Self::filled(svg_path, size)?
        })
    }

    fn draw(&self, canvas: &mut Canvas, r: Rect, color: Color) {
        match self.stroke {
            Some(width) => canvas.stroke_path(&self.path, self.view, r, width, color),
            None => canvas.path(&self.path, self.view, r, color),
        }
    }
}

/// A button: text, an icon, or both.
#[derive(Debug)]
pub struct Button {
    label: TextBox,
    icon: Option<Icon>,
    /// What it does, for screen readers, when it shows no text.
    name: Option<String>,
    pub kind: ButtonKind,
    /// On, for toggles: tinted with the accent.
    pub selected: bool,
    pub enabled: bool,
    /// Content at the start instead of centred: menu items.
    pub start: bool,
}

impl Button {
    pub fn new(label: impl Into<String>, kind: ButtonKind, theme: &Theme) -> Self {
        let style = match kind {
            ButtonKind::Primary => theme.text.label.with_color(Paint::OnAccent),
            _ => theme.text.label,
        };
        Self {
            label: TextBox::new(label, style),
            icon: None,
            name: None,
            kind,
            selected: false,
            enabled: true,
            start: false,
        }
    }

    /// Content at the start, as in a menu.
    pub fn at_start(mut self) -> Self {
        self.start = true;
        self
    }

    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// The name screen readers say, for a button without text.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn with_style(mut self, style: TextStyle) -> Self {
        self.label = TextBox::new(self.label.text().to_owned(), style);
        self
    }

    fn name(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| self.label.text().to_owned())
    }
}

/// Something drawn by its own code: a gauge, a spinner, a chart.
pub trait Draw {
    /// Draws at the frame's time `now`.
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, now: Duration);
    /// The size it wants, when laid out with [`Length::Auto`].
    fn size(&self) -> Size;
    /// Whether it changes with time at `now`: it is drawn again every frame
    /// while so, and only then (a spinner while work goes on).
    fn is_moving(&self, _now: Duration) -> bool {
        false
    }
}

pub enum Kind {
    Box(Look),
    Text(TextBox),
    Button(Button),
    Image {
        pixmap: Arc<Pixmap>,
        /// What the image shows, for screen readers.
        alt: String,
    },
    Icon {
        icon: Icon,
        color: Paint,
        /// What it means; `None` for a decorative icon.
        label: Option<String>,
    },
    Custom {
        draw: Box<dyn Draw>,
        label: Option<String>,
    },
    TextField(Box<TextField>),
    /// A value from 0 to 1, set by dragging along it or with the arrows.
    Slider {
        value: f32,
        /// What the arrows add or take.
        step: f32,
        name: String,
    },
}

impl fmt::Debug for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Box(look) => f.debug_tuple("Box").field(look).finish(),
            Self::Text(text) => f.debug_tuple("Text").field(text).finish(),
            Self::Button(button) => button.fmt(f),
            Self::Image { alt, .. } => f.debug_struct("Image").field("alt", alt).finish(),
            Self::Icon { label, .. } => f.debug_struct("Icon").field("label", label).finish(),
            Self::Custom { label, .. } => f.debug_struct("Custom").field("label", label).finish(),
            Self::TextField(field) => field.fmt(f),
            Self::Slider { value, name, .. } => f
                .debug_struct("Slider")
                .field("name", name)
                .field("value", value)
                .finish(),
        }
    }
}

impl Kind {
    pub fn text(text: impl Into<String>, style: TextStyle) -> Self {
        Self::Text(TextBox::new(text, style))
    }

    /// A line of text cut with "…" when it does not fit.
    pub fn text_line(text: impl Into<String>, style: TextStyle) -> Self {
        Self::Text(TextBox::new(text, style).single_line())
    }

    pub fn button(label: impl Into<String>, kind: ButtonKind, theme: &Theme) -> Self {
        Self::Button(Button::new(label, kind, theme))
    }

    /// A button showing only an icon, named for screen readers.
    pub fn icon_button(icon: Icon, name: &str, kind: ButtonKind, theme: &Theme) -> Self {
        Self::Button(Button::new("", kind, theme).with_icon(icon).with_name(name))
    }

    /// A filled icon from SVG path data in a 24×24 box.
    pub fn icon(svg_path: &str, size: f32, color: Paint, label: Option<&str>) -> Option<Self> {
        Some(Self::Icon {
            icon: Icon::filled(svg_path, size)?,
            color,
            label: label.map(str::to_owned),
        })
    }

    /// An image from premultiplied RGBA pixels.
    pub fn image(width: u16, height: u16, premultiplied: &[u8], alt: &str) -> Option<Self> {
        let mut pixmap = Pixmap::new(width, height);
        let data = pixmap.data_as_u8_slice_mut();
        if data.len() != premultiplied.len() {
            return None;
        }
        data.copy_from_slice(premultiplied);
        Some(Self::Image {
            pixmap: Arc::new(pixmap),
            alt: alt.to_owned(),
        })
    }

    fn focusable(&self) -> bool {
        matches!(self, Self::Button(b) if b.enabled)
            || matches!(self, Self::TextField(_) | Self::Slider { .. })
    }
}

/// A property that can move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Property {
    Opacity,
    /// Horizontal offset from the laid out place, logical pixels.
    OffsetX,
    OffsetY,
}

#[derive(Debug)]
struct Slot {
    generation: u32,
    node: Option<Node>,
}

#[derive(Debug)]
struct Node {
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    style: Style,
    kind: Kind,
    /// Laid out, absolute, without animated offsets.
    rect: Rect,
    /// Where it was last drawn, offsets and decorations included.
    painted: Option<Rect>,
    /// Its looks changed since it was drawn.
    changed: bool,
    opacity: Animated,
    offset_x: Animated,
    offset_y: Animated,
    hovered: bool,
    pressed: bool,
    /// It moved or faded at the last frame: the next one draws where it
    /// stopped.
    moving: bool,
    /// A box that takes clicks, with its name for screen readers.
    clickable: Option<String>,
    /// A clickable box that is the current choice (a result in a list).
    chosen: bool,
}

impl Node {
    /// It takes clicks and the keyboard focus.
    fn interactive(&self) -> bool {
        self.kind.focusable() || self.clickable.is_some()
    }

    fn is_moving(&self, now: Duration) -> bool {
        self.opacity.is_running(now)
            || self.offset_x.is_running(now)
            || self.offset_y.is_running(now)
            || matches!(&self.kind, Kind::Custom { draw, .. } if draw.is_moving(now))
    }
}

/// What the user did, as the tree understands it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    PointerMove(Point),
    PointerLeave,
    /// The primary button.
    PointerButton {
        pressed: bool,
    },
    Key(Key),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Tab,
    BackTab,
    Enter,
    Space,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Backspace,
    Delete,
}

/// What came out of input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A button was clicked, or pressed from the keyboard; Enter in a text
    /// field.
    Activated(NodeId),
    /// The text of a field changed.
    Changed(NodeId),
}

/// A line of editable text.
#[derive(Debug)]
pub struct TextField {
    value: TextBox,
    placeholder: TextBox,
    /// Byte index of the caret in the value.
    cursor: usize,
    /// What it is for, for screen readers.
    name: String,
}

impl TextField {
    pub fn new(name: &str, placeholder: &str, style: TextStyle) -> Self {
        Self {
            value: TextBox::new("", style),
            placeholder: TextBox::new(placeholder, style.with_color(Paint::Muted)),
            cursor: 0,
            name: name.to_owned(),
        }
    }

    pub fn value(&self) -> &str {
        self.value.text()
    }

    fn set(&mut self, text: &str, cursor: usize) {
        self.value.set_text(text);
        self.cursor = cursor.min(text.len());
    }

    /// Edits with `key`; whether the value changed.
    fn key(&mut self, key: Key) -> (bool, bool) {
        let text = self.value.text().to_owned();
        let c = self.cursor;
        let prev = text[..c].char_indices().next_back().map_or(0, |(i, _)| i);
        let next = text[c..].chars().next().map_or(c, |ch| c + ch.len_utf8());
        let (value, cursor) = match key {
            Key::Backspace if c > 0 => (format!("{}{}", &text[..prev], &text[c..]), prev),
            Key::Delete if c < text.len() => (format!("{}{}", &text[..c], &text[next..]), c),
            Key::Left => (text.clone(), prev),
            Key::Right => (text.clone(), next),
            Key::Home => (text.clone(), 0),
            Key::End => (text.clone(), text.len()),
            _ => return (false, false),
        };
        let changed = value != text;
        let moved = cursor != c;
        self.set(&value, cursor);
        (changed, changed || moved)
    }

    fn insert(&mut self, s: &str) {
        let text = self.value.text();
        let value = format!("{}{s}{}", &text[..self.cursor], &text[self.cursor..]);
        let cursor = self.cursor + s.len();
        self.set(&value, cursor);
    }
}

/// A tree of widgets drawn on one surface.
pub struct Tree {
    nodes: Vec<Slot>,
    free: Vec<u32>,
    root: NodeId,
    fonts: Fonts,
    theme: Theme,
    size: Size,
    scale: f32,
    now: Duration,
    relayout: bool,
    /// Regions to draw again for nodes that went away.
    removed: Vec<Rect>,
    hovered: Option<NodeId>,
    pressed: Option<NodeId>,
    focused: Option<NodeId>,
    /// The ring shows once the keyboard is used, not after a click.
    focus_visible: bool,
    pointer: Option<Point>,
    /// The slider the pointer drags.
    dragging: Option<NodeId>,
}

impl fmt::Debug for Tree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tree")
            .field(
                "nodes",
                &self.nodes.iter().filter(|s| s.node.is_some()).count(),
            )
            .field("size", &self.size)
            .field("scale", &self.scale)
            .finish_non_exhaustive()
    }
}

impl Tree {
    /// An empty tree of `size` logical pixels: a root box with the theme's
    /// background.
    pub fn new(theme: Theme, size: Size, scale: f32) -> Self {
        let mut tree = Self {
            nodes: Vec::new(),
            free: Vec::new(),
            root: NodeId {
                index: 0,
                generation: 0,
            },
            fonts: Fonts::new(theme.faces),
            theme,
            size,
            scale,
            now: Duration::ZERO,
            relayout: true,
            removed: Vec::new(),
            hovered: None,
            pressed: None,
            focused: None,
            focus_visible: false,
            pointer: None,
            dragging: None,
        };
        let look = Look {
            background: Some(Paint::Background),
            ..Look::default()
        };
        tree.root = tree.alloc(None, Style::default(), Kind::Box(look));
        tree
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Draws everything again in another theme or accent.
    pub fn set_theme(&mut self, theme: Theme) {
        if theme == self.theme {
            return;
        }
        self.fonts.set_faces(theme.faces);
        self.theme = theme;
        self.relayout = true;
        for node in self.nodes.iter_mut().filter_map(|s| s.node.as_mut()) {
            node.changed = true;
        }
    }

    pub fn size(&self) -> Size {
        self.size
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    fn alloc(&mut self, parent: Option<NodeId>, style: Style, kind: Kind) -> NodeId {
        let node = Node {
            parent,
            children: Vec::new(),
            style,
            kind,
            rect: Rect::default(),
            painted: None,
            changed: true,
            opacity: Animated::new(1.0),
            offset_x: Animated::new(0.0),
            offset_y: Animated::new(0.0),
            hovered: false,
            pressed: false,
            moving: false,
            clickable: None,
            chosen: false,
        };
        if let Some(index) = self.free.pop() {
            let slot = &mut self.nodes[index as usize];
            slot.generation += 1;
            slot.node = Some(node);
            NodeId {
                index,
                generation: slot.generation,
            }
        } else {
            self.nodes.push(Slot {
                generation: 0,
                node: Some(node),
            });
            NodeId {
                index: (self.nodes.len() - 1) as u32,
                generation: 0,
            }
        }
    }

    fn node(&self, id: NodeId) -> Option<&Node> {
        let slot = self.nodes.get(id.index as usize)?;
        slot.node
            .as_ref()
            .filter(|_| slot.generation == id.generation)
    }

    fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        let slot = self.nodes.get_mut(id.index as usize)?;
        let generation = slot.generation;
        slot.node.as_mut().filter(|_| generation == id.generation)
    }

    /// Adds a child at the end of `parent`'s children.
    pub fn add(&mut self, parent: NodeId, style: Style, kind: Kind) -> Option<NodeId> {
        self.node(parent)?;
        let id = self.alloc(Some(parent), style, kind);
        self.node_mut(parent)?.children.push(id);
        self.relayout = true;
        Some(id)
    }

    /// Adds a child at `index` among `parent`'s children (the end if past
    /// it).
    pub fn insert(
        &mut self,
        parent: NodeId,
        index: usize,
        style: Style,
        kind: Kind,
    ) -> Option<NodeId> {
        self.node(parent)?;
        let id = self.alloc(Some(parent), style, kind);
        let children = &mut self.node_mut(parent)?.children;
        children.insert(index.min(children.len()), id);
        self.relayout = true;
        Some(id)
    }

    /// Removes a node and everything below it.
    pub fn remove(&mut self, id: NodeId) {
        if id == self.root {
            return;
        }
        let Some(node) = self.node(id) else {
            return;
        };
        let (parent, children) = (node.parent, node.children.clone());
        for child in children {
            self.remove(child);
        }
        if let Some(parent) = parent.and_then(|p| self.node_mut(p)) {
            parent.children.retain(|c| *c != id);
        }
        if let Some(node) = self
            .nodes
            .get_mut(id.index as usize)
            .and_then(|slot| slot.node.take())
        {
            if let Some(painted) = node.painted {
                self.removed.push(painted);
            }
            self.free.push(id.index);
        }
        for slot in [&mut self.hovered, &mut self.pressed, &mut self.focused] {
            if *slot == Some(id) {
                *slot = None;
            }
        }
        self.relayout = true;
    }

    /// Changes the text of a text node or the label of a button.
    pub fn set_text(&mut self, id: NodeId, text: &str) {
        let changed = match self.node_mut(id).map(|n| &mut n.kind) {
            Some(Kind::Text(t) | Kind::Button(Button { label: t, .. })) => t.set_text(text),
            _ => false,
        };
        if changed {
            self.mark(id);
            self.relayout = true;
        }
    }

    pub fn set_text_style(&mut self, id: NodeId, style: TextStyle) {
        let changed = match self.node_mut(id).map(|n| &mut n.kind) {
            Some(Kind::Text(t)) => t.set_style(style),
            _ => false,
        };
        if changed {
            self.mark(id);
            self.relayout = true;
        }
    }

    pub fn set_look(&mut self, id: NodeId, look: Look) {
        if let Some(node) = self.node_mut(id)
            && let Kind::Box(current) = &mut node.kind
            && *current != look
        {
            *current = look;
            node.changed = true;
        }
    }

    pub fn set_style(&mut self, id: NodeId, style: Style) {
        if let Some(node) = self.node_mut(id)
            && node.style != style
        {
            node.style = style;
            node.changed = true;
            self.relayout = true;
        }
    }

    /// Makes a box take clicks, as a button named `name` would: its whole
    /// content is the button (a line with dots, an avatar and a name).
    pub fn set_clickable(&mut self, id: NodeId, name: &str) {
        if let Some(node) = self.node_mut(id)
            && node.clickable.as_deref() != Some(name)
        {
            node.clickable = Some(name.to_owned());
        }
    }

    /// Changes a button's or an icon's icon.
    pub fn set_icon(&mut self, id: NodeId, new: Icon) {
        if let Some(node) = self.node_mut(id) {
            match &mut node.kind {
                Kind::Button(b) => b.icon = Some(new),
                Kind::Icon { icon, .. } => *icon = new,
                _ => return,
            }
            node.changed = true;
        }
    }

    /// Changes the colour of an icon.
    pub fn set_icon_paint(&mut self, id: NodeId, paint: Paint) {
        if let Some(node) = self.node_mut(id)
            && let Kind::Icon { color, .. } = &mut node.kind
            && *color != paint
        {
            *color = paint;
            node.changed = true;
        }
    }

    /// Changes what screen readers call a button or a clickable box.
    pub fn set_name(&mut self, id: NodeId, name: &str) {
        if let Some(node) = self.node_mut(id) {
            if let Kind::Button(b) = &mut node.kind {
                b.name = Some(name.to_owned());
            } else if node.clickable.is_some() {
                node.clickable = Some(name.to_owned());
            }
        }
    }

    /// Marks a clickable box as the current choice, for screen readers.
    pub fn set_selected_box(&mut self, id: NodeId, chosen: bool) {
        if let Some(node) = self.node_mut(id) {
            node.chosen = chosen;
        }
    }

    /// Turns a toggle button on or off.
    pub fn set_selected(&mut self, id: NodeId, selected: bool) {
        if let Some(node) = self.node_mut(id)
            && let Kind::Button(b) = &mut node.kind
            && b.selected != selected
        {
            b.selected = selected;
            node.changed = true;
        }
    }

    /// A disabled button is shown, but neither takes clicks nor focus.
    pub fn set_enabled(&mut self, id: NodeId, enabled: bool) {
        let changed = if let Some(node) = self.node_mut(id)
            && let Kind::Button(b) = &mut node.kind
            && b.enabled != enabled
        {
            b.enabled = enabled;
            node.changed = true;
            true
        } else {
            false
        };
        if changed && !enabled {
            for slot in [&mut self.hovered, &mut self.pressed, &mut self.focused] {
                if *slot == Some(id) {
                    *slot = None;
                }
            }
        }
    }

    /// Replaces what a [`Kind::Custom`] node draws.
    pub fn set_draw(&mut self, id: NodeId, new: Box<dyn Draw>) {
        if let Some(node) = self.node_mut(id)
            && let Kind::Custom { draw, .. } = &mut node.kind
        {
            *draw = new;
            node.changed = true;
            self.relayout = true;
        }
    }

    /// Something a [`Kind::Custom`] draws changed: draw it again.
    pub fn mark(&mut self, id: NodeId) {
        if let Some(node) = self.node_mut(id) {
            node.changed = true;
        }
    }

    /// Moves a property to `to` from now, along `curve`.
    pub fn animate(&mut self, id: NodeId, property: Property, to: f32, curve: Curve) {
        if self.theme.reduced_motion {
            self.jump(id, property, to);
            return;
        }
        let now = self.now;
        if let Some(node) = self.node_mut(id) {
            let value = match property {
                Property::Opacity => &mut node.opacity,
                Property::OffsetX => &mut node.offset_x,
                Property::OffsetY => &mut node.offset_y,
            };
            value.set(to, curve, now);
        }
    }

    /// Lays out what changed now, without drawing: to know where nodes go
    /// before animating them there.
    pub fn layout(&mut self) {
        if self.relayout {
            let root = self.root;
            let size = self.size;
            if let Some(root) = self.node_mut(root) {
                root.style.width = Length::Px(size.w);
                root.style.height = Length::Px(size.h);
            }
            self.arrange(root, Rect::new(0.0, 0.0, size.w, size.h));
            self.relayout = false;
        }
    }

    /// Puts a property at `value` at once (reduced motion, or the start of a
    /// move from where a node was).
    pub fn jump(&mut self, id: NodeId, property: Property, value: f32) {
        if let Some(node) = self.node_mut(id) {
            let v = match property {
                Property::Opacity => &mut node.opacity,
                Property::OffsetX => &mut node.offset_x,
                Property::OffsetY => &mut node.offset_y,
            };
            v.jump(value);
        }
    }

    /// Whether something moves at `now`: the caller schedules a frame then,
    /// and only then.
    pub fn is_animating(&self, now: Duration) -> bool {
        self.nodes
            .iter()
            .filter_map(|s| s.node.as_ref())
            .any(|n| n.is_moving(now))
    }

    pub fn resize(&mut self, size: Size, scale: f32) {
        if size != self.size || scale != self.scale {
            self.size = size;
            self.scale = scale;
            self.relayout = true;
            for node in self.nodes.iter_mut().filter_map(|s| s.node.as_mut()) {
                node.changed = true;
            }
        }
    }

    /// A node's children, in order.
    pub fn children(&self, id: NodeId) -> Vec<NodeId> {
        self.node(id)
            .map(|n| n.children.clone())
            .unwrap_or_default()
    }

    /// Where a node was laid out, without animated offsets.
    pub fn rect(&self, id: NodeId) -> Option<Rect> {
        self.node(id).map(|n| n.rect)
    }

    pub fn focused(&self) -> Option<NodeId> {
        self.focused
    }

    // Layout.

    /// The size a node wants within `avail`.
    fn measure(&mut self, id: NodeId, avail: Size) -> Size {
        let scale = self.scale;
        let Some(node) = self.node(id) else {
            return Size::default();
        };
        let style = node.style;
        let children = node.children.clone();
        let pad = style.padding;
        let fixed = |len: Length, avail: f32| match len {
            Length::Px(v) => Some(v),
            Length::Fill => Some(avail),
            Length::Auto => None,
        };
        let (fw, fh) = (fixed(style.width, avail.w), fixed(style.height, avail.h));
        let inner_w = fw.unwrap_or(avail.w) - pad.horizontal();
        let content = match self.content_size(id, Size::new(inner_w.max(0.0), avail.h), scale) {
            Some(size) => size,
            None => {
                // A container: its children, along its direction.
                let mut main = 0.0_f32;
                let mut cross = 0.0_f32;
                let inner = Size::new(
                    inner_w.max(0.0),
                    (fh.unwrap_or(avail.h) - pad.vertical()).max(0.0),
                );
                main += style.gap * children.len().saturating_sub(1) as f32;
                let mut fills = Vec::new();
                for child in &children {
                    let child_style = self.node(*child).map(|n| n.style).unwrap_or_default();
                    if style.direction == Direction::Row && child_style.width == Length::Fill {
                        fills.push((*child, child_style.grow.max(0.0)));
                        continue;
                    }
                    let s = self.measure(*child, inner);
                    match style.direction {
                        Direction::Column => {
                            main += s.h;
                            cross = cross.max(s.w);
                        }
                        Direction::Row => {
                            main += s.w;
                            cross = cross.max(s.h);
                        }
                    }
                }
                // In a row, filling children share what is left, and their
                // height is the one at that width (text wraps).
                let grow: f32 = fills.iter().map(|(_, g)| g).sum();
                let free = (inner.w - main).max(0.0);
                for (child, g) in fills {
                    let w = if grow > 0.0 { free * g / grow } else { 0.0 };
                    let s = self.measure(child, Size::new(w, inner.h));
                    main += w;
                    cross = cross.max(s.h);
                }
                match style.direction {
                    Direction::Column => Size::new(cross, main),
                    Direction::Row => Size::new(main, cross),
                }
            }
        };
        Size::new(
            fw.unwrap_or(content.w + pad.horizontal()),
            fh.unwrap_or(content.h + pad.vertical()),
        )
    }

    /// The size of a leaf's content, `None` for a container.
    fn content_size(&mut self, id: NodeId, avail: Size, scale: f32) -> Option<Size> {
        let spacing = self.theme.spacing;
        let Tree { nodes, fonts, .. } = self;
        let node = nodes.get_mut(id.index as usize)?.node.as_mut()?;
        match &mut node.kind {
            Kind::Box(_) => None,
            Kind::Text(text) => Some(text.measure(fonts, Some(avail.w), scale)),
            Kind::Button(b) => {
                let text = if b.label.text().is_empty() {
                    Size::default()
                } else {
                    b.label.measure(fonts, None, scale)
                };
                let icon = b.icon.as_ref().map_or(0.0, |i| i.size);
                let gap = if icon > 0.0 && text.w > 0.0 {
                    spacing.xs * 1.5
                } else {
                    0.0
                };
                let pad = if text.w > 0.0 { spacing.l } else { spacing.s };
                Some(Size::new(
                    icon + gap + text.w + 2.0 * pad,
                    (text.h.max(icon) + 2.0 * spacing.s).max(32.0),
                ))
            }
            Kind::Image { pixmap, .. } => Some(Size::new(
                f32::from(pixmap.width()) / scale,
                f32::from(pixmap.height()) / scale,
            )),
            Kind::Icon { icon, .. } => Some(Size::new(icon.size, icon.size)),
            Kind::Custom { draw, .. } => Some(draw.size()),
            Kind::TextField(field) => {
                let v = field.value.measure(fonts, None, scale);
                let p = field.placeholder.measure(fonts, None, scale);
                Some(Size::new(v.w.max(p.w), v.h.max(p.h)))
            }
            Kind::Slider { .. } => Some(Size::new(120.0, 24.0)),
        }
    }

    /// Places a node at `rect` and its children inside.
    fn arrange(&mut self, id: NodeId, rect: Rect) {
        let Some(node) = self.node_mut(id) else {
            return;
        };
        node.rect = rect;
        let (style, children) = (node.style, node.children.clone());
        if children.is_empty() {
            return;
        }
        let inner = rect.inset(style.padding);
        let column = style.direction == Direction::Column;
        let (main_len, cross_len) = if column {
            (inner.h, inner.w)
        } else {
            (inner.w, inner.h)
        };
        // Sizes of the children that are not `Fill` along the main axis.
        let mut sizes = Vec::with_capacity(children.len());
        let mut used = style.gap * (children.len() - 1) as f32;
        let mut grow = 0.0;
        for child in &children {
            let child_style = self.node(*child).map(|n| n.style).unwrap_or_default();
            let main_fill = if column {
                child_style.height == Length::Fill
            } else {
                child_style.width == Length::Fill
            };
            let s = self.measure(*child, Size::new(inner.w, inner.h));
            if main_fill {
                grow += child_style.grow.max(0.0);
                sizes.push((s, Some(child_style.grow.max(0.0))));
            } else {
                used += if column { s.h } else { s.w };
                sizes.push((s, None));
            }
        }
        let free = (main_len - used).max(0.0);
        let mut pos = 0.0;
        let mut spacing = style.gap;
        if grow == 0.0 {
            match style.justify {
                Justify::Start => {}
                Justify::Center => pos = free / 2.0,
                Justify::End => pos = free,
                Justify::SpaceBetween if children.len() > 1 => {
                    spacing += free / (children.len() - 1) as f32;
                }
                Justify::SpaceBetween => {}
            }
        }
        for (child, (size, fill)) in children.iter().zip(sizes) {
            let main = match fill {
                Some(g) if grow > 0.0 => free * g / grow,
                Some(_) => 0.0,
                None if column => size.h,
                None => size.w,
            };
            let child_style = self.node(*child).map(|n| n.style).unwrap_or_default();
            // A child filling a row is as tall as its content at its width.
            let size = if !column && fill.is_some() {
                self.measure(*child, Size::new(main, inner.h))
            } else {
                size
            };
            let cross_fill = if column {
                child_style.width == Length::Fill
            } else {
                child_style.height == Length::Fill
            };
            let wanted_cross = if column { size.w } else { size.h };
            let (cross, offset) = if cross_fill || style.align == Align::Stretch {
                (cross_len, 0.0)
            } else {
                let c = wanted_cross.min(cross_len);
                let offset = match style.align {
                    Align::Start | Align::Stretch => 0.0,
                    Align::Center => (cross_len - c) / 2.0,
                    Align::End => cross_len - c,
                };
                (c, offset)
            };
            let r = if column {
                Rect::new(inner.x + offset, inner.y + pos, cross, main)
            } else {
                Rect::new(inner.x + pos, inner.y + offset, main, cross)
            };
            self.arrange(*child, r);
            pos += main + spacing;
        }
    }

    // Frames.

    /// Samples animations at `now`, lays out what changed, and returns the
    /// regions to draw again, in logical pixels.
    pub fn update(&mut self, now: Duration) -> Vec<Rect> {
        self.now = now;
        // The system's fonts arrived: every text again.
        if self.fonts.sync() {
            self.relayout = true;
            for node in self.nodes.iter_mut().filter_map(|s| s.node.as_mut()) {
                node.changed = true;
            }
        }
        self.layout();
        let mut damage = std::mem::take(&mut self.removed);
        let root = self.root;
        self.collect_damage(root, (0.0, 0.0), false, &mut damage);
        merge(damage)
    }

    /// Where a node draws: its box moved by `offset`, with room for its
    /// shadow and focus ring.
    fn visual_rect(&self, id: NodeId, node: &Node, offset: (f32, f32)) -> Rect {
        let mut r = node.rect.translate(offset.0, offset.1);
        if let Kind::Box(look) = &node.kind
            && look.shadow > 0.0
        {
            r = r.union(&r.translate(0.0, look.shadow / 3.0).grow(look.shadow * 1.25));
        }
        if self.focus_visible && self.focused == Some(id) {
            r = r.grow(self.theme.focus_ring * 2.0);
        }
        r
    }

    fn collect_damage(
        &mut self,
        id: NodeId,
        offset: (f32, f32),
        parent_moved: bool,
        out: &mut Vec<Rect>,
    ) {
        let now = self.now;
        let Some(node) = self.node(id) else {
            return;
        };
        let offset = (
            offset.0 + node.offset_x.value(now),
            offset.1 + node.offset_y.value(now),
        );
        let running = node.is_moving(now);
        // The frame after the last step draws where it stopped.
        let animating = running || node.moving;
        let visual = self.visual_rect(id, node, offset);
        let moved = node.painted != Some(visual);
        let dirty = node.changed || moved || animating || parent_moved;
        if dirty {
            if let Some(painted) = node.painted {
                out.push(painted);
            }
            out.push(visual);
        }
        let children = node.children.clone();
        if let Some(node) = self.node_mut(id) {
            node.painted = Some(visual);
            node.changed = false;
            node.moving = running;
        }
        // A moving subtree moves its children with it.
        let carries = animating && dirty;
        for child in children {
            self.collect_damage(child, offset, carries, out);
        }
    }

    // Drawing.

    /// Draws the parts of the tree that touch `region`.
    fn draw(&mut self, canvas: &mut Canvas, region: Rect) {
        let root = self.root;
        let theme = self.theme.clone();
        self.draw_node(canvas, root, region, (0.0, 0.0), &theme);
    }

    fn draw_node(
        &mut self,
        canvas: &mut Canvas,
        id: NodeId,
        region: Rect,
        offset: (f32, f32),
        theme: &Theme,
    ) {
        let now = self.now;
        let focused = self.focus_visible && self.focused == Some(id);
        let scale = self.scale;
        let Some(node) = self.node(id) else {
            return;
        };
        let (dx, dy) = (node.offset_x.value(now), node.offset_y.value(now));
        let offset = (offset.0 + dx, offset.1 + dy);
        let visual = self.visual_rect(id, node, offset);
        let opacity = node.opacity.value(now);
        if opacity <= 0.0 {
            return;
        }
        let touches = visual.intersects(&region);
        let has_children = !node.children.is_empty();
        if !touches && !has_children {
            return;
        }
        canvas.push_offset(dx, dy);
        if opacity < 1.0 {
            canvas.push_opacity(opacity);
        }
        let rect = node.rect;
        let (style, hovered, pressed) = (node.style, node.hovered, node.pressed);
        if touches {
            let Tree { nodes, fonts, .. } = self;
            if let Some(node) = nodes
                .get_mut(id.index as usize)
                .and_then(|slot| slot.node.as_mut())
            {
                draw_kind(
                    canvas,
                    &mut node.kind,
                    rect,
                    theme,
                    fonts,
                    scale,
                    (hovered, pressed, self.focused == Some(id)),
                    now,
                );
            }
            if focused {
                let ring = theme.focus_ring;
                canvas.stroke_rect(
                    rect.grow(ring * 2.0),
                    theme.radii.small + ring * 2.0,
                    ring,
                    theme.colors.accent,
                );
            }
        }
        let children = self
            .node(id)
            .map(|n| n.children.clone())
            .unwrap_or_default();
        if !children.is_empty() {
            let radius = match self.node(id).map(|n| &n.kind) {
                Some(Kind::Box(look)) => look.radius,
                _ => 0.0,
            };
            if style.clip {
                canvas.push_clip(rect, radius);
            }
            for child in children {
                self.draw_node(canvas, child, region, offset, theme);
            }
            if style.clip {
                canvas.pop();
            }
        }
        if opacity < 1.0 {
            canvas.pop();
        }
        canvas.pop_offset();
    }

    // Input.

    /// The topmost node under `p`, offsets included; with `interactive`, only
    /// the ones that take clicks.
    pub fn hit(&self, p: Point, interactive: bool) -> Option<NodeId> {
        self.hit_node(self.root, p, (0.0, 0.0), interactive)
    }

    fn hit_node(
        &self,
        id: NodeId,
        p: Point,
        offset: (f32, f32),
        interactive: bool,
    ) -> Option<NodeId> {
        let node = self.node(id)?;
        let offset = (
            offset.0 + node.offset_x.value(self.now),
            offset.1 + node.offset_y.value(self.now),
        );
        let rect = node.rect.translate(offset.0, offset.1);
        if node.opacity.value(self.now) <= 0.0 || (node.style.clip && !rect.contains(p)) {
            return None;
        }
        for child in node.children.iter().rev() {
            if let Some(hit) = self.hit_node(*child, p, offset, interactive) {
                return Some(hit);
            }
        }
        (rect.contains(p) && (!interactive || node.interactive())).then_some(id)
    }

    fn set_flag(&mut self, id: Option<NodeId>, hovered: Option<bool>, pressed: Option<bool>) {
        if let Some(node) = id.and_then(|id| self.node_mut(id)) {
            if let Some(h) = hovered
                && node.hovered != h
            {
                node.hovered = h;
                node.changed = true;
            }
            if let Some(p) = pressed
                && node.pressed != p
            {
                node.pressed = p;
                node.changed = true;
            }
        }
    }

    fn focus(&mut self, id: Option<NodeId>) {
        if self.focused != id {
            let old = self.focused;
            self.focused = id;
            for n in [old, id].into_iter().flatten() {
                self.mark(n);
            }
        }
    }

    fn set_focus_visible(&mut self, visible: bool) {
        if self.focus_visible != visible {
            self.focus_visible = visible;
            if let Some(f) = self.focused {
                self.mark(f);
            }
        }
    }

    /// Focusable nodes in reading order.
    fn focus_order(&self) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            if let Some(node) = self.node(id) {
                if node.interactive() {
                    out.push(id);
                }
                stack.extend(node.children.iter().rev());
            }
        }
        out
    }

    pub fn input(&mut self, input: Input) -> Vec<Event> {
        let mut events = Vec::new();
        match input {
            Input::PointerMove(p) => {
                self.pointer = Some(p);
                if let Some(id) = self.dragging {
                    events.extend(self.slide_to(id, p));
                }
                let hovered = self.hit(p, true);
                if hovered != self.hovered {
                    let old = self.hovered;
                    self.set_flag(old, Some(false), None);
                    self.set_flag(hovered, Some(true), None);
                    self.hovered = hovered;
                }
            }
            Input::PointerLeave => {
                self.pointer = None;
                let old = self.hovered.take();
                self.set_flag(old, Some(false), Some(false));
            }
            Input::PointerButton { pressed: true } => {
                self.pressed = self.hovered;
                let pressed = self.pressed;
                self.set_flag(pressed, None, Some(true));
                if pressed.is_some() {
                    // Clicking a button focuses it; the ring is for the
                    // keyboard.
                    self.focus(pressed);
                    self.set_focus_visible(false);
                }
                // A slider follows the pointer from the press on.
                if let (Some(id), Some(p)) = (pressed, self.pointer)
                    && matches!(self.node(id).map(|n| &n.kind), Some(Kind::Slider { .. }))
                {
                    self.dragging = Some(id);
                    events.extend(self.slide_to(id, p));
                }
            }
            Input::PointerButton { pressed: false } => {
                self.dragging = None;
                let pressed = self.pressed.take();
                self.set_flag(pressed, None, Some(false));
                if pressed.is_some() && pressed == self.hovered {
                    events.extend(pressed.map(Event::Activated));
                }
            }
            Input::Key(key @ (Key::Tab | Key::BackTab)) => {
                let order = self.focus_order();
                if !order.is_empty() {
                    let at = self
                        .focused
                        .and_then(|f| order.iter().position(|n| *n == f));
                    let next = match (key, at) {
                        (Key::Tab, Some(i)) => (i + 1) % order.len(),
                        (Key::Tab, None) => 0,
                        (_, Some(i)) => (i + order.len() - 1) % order.len(),
                        (_, None) => order.len() - 1,
                    };
                    self.focus(Some(order[next]));
                    self.set_focus_visible(true);
                }
            }
            Input::Key(Key::Enter) => {
                events.extend(self.focused.map(Event::Activated));
            }
            Input::Key(Key::Space) => {
                // In a field, a space is text.
                if self.focused_field().is_some() {
                    events.extend(self.text_input(" "));
                } else {
                    events.extend(self.focused.map(Event::Activated));
                }
            }
            Input::Key(key @ (Key::Left | Key::Right | Key::Home | Key::End))
                if self.focused_slider().is_some() =>
            {
                if let Some(id) = self.focused_slider()
                    && let Some(Kind::Slider { value, step, .. }) = self.node(id).map(|n| &n.kind)
                {
                    let to = match key {
                        Key::Left => value - step,
                        Key::Right => value + step,
                        Key::Home => 0.0,
                        _ => 1.0,
                    };
                    if self.set_slider(id, to) {
                        events.push(Event::Changed(id));
                    }
                }
            }
            Input::Key(key) => {
                if let Some(id) = self.focused_field() {
                    let mut edited = (false, false);
                    if let Some(Kind::TextField(field)) = self.node_mut(id).map(|n| &mut n.kind) {
                        edited = field.key(key);
                    }
                    if edited.1 {
                        self.mark(id);
                        self.relayout = true;
                    }
                    if edited.0 {
                        events.push(Event::Changed(id));
                    }
                }
            }
        }
        events
    }

    fn focused_slider(&self) -> Option<NodeId> {
        let id = self.focused?;
        matches!(self.node(id)?.kind, Kind::Slider { .. }).then_some(id)
    }

    /// Moves a slider to the pointer at `p`.
    fn slide_to(&mut self, id: NodeId, p: Point) -> Option<Event> {
        let rect = self.node(id)?.rect;
        let to = (p.x - rect.x) / rect.w.max(1.0);
        self.set_slider(id, to).then_some(Event::Changed(id))
    }

    /// Whether the value changed.
    fn set_slider(&mut self, id: NodeId, to: f32) -> bool {
        let to = to.clamp(0.0, 1.0);
        match self.node_mut(id).map(|n| &mut n.kind) {
            Some(Kind::Slider { value, .. }) if (*value - to).abs() > f32::EPSILON => {
                *value = to;
                self.mark(id);
                true
            }
            _ => false,
        }
    }

    /// A slider's value, from 0 to 1.
    pub fn slider_value(&self, id: NodeId) -> Option<f32> {
        match self.node(id)?.kind {
            Kind::Slider { value, .. } => Some(value),
            _ => None,
        }
    }

    /// Sets a slider from outside (the system changed the volume).
    pub fn set_slider_value(&mut self, id: NodeId, value: f32) {
        if self.dragging != Some(id) {
            self.set_slider(id, value);
        }
    }

    fn focused_field(&self) -> Option<NodeId> {
        let id = self.focused?;
        matches!(self.node(id)?.kind, Kind::TextField(_)).then_some(id)
    }

    /// Types `text` into the focused field.
    pub fn text_input(&mut self, text: &str) -> Vec<Event> {
        let Some(id) = self.focused_field() else {
            return Vec::new();
        };
        if let Some(Kind::TextField(field)) = self.node_mut(id).map(|n| &mut n.kind) {
            field.insert(text);
        }
        self.mark(id);
        self.relayout = true;
        vec![Event::Changed(id)]
    }

    /// Gives a node the keyboard, without the ring (a field shows its caret).
    pub fn set_focus(&mut self, id: Option<NodeId>) {
        self.focus(id);
        self.set_focus_visible(false);
    }

    /// The text of a field.
    pub fn field_value(&self, id: NodeId) -> Option<&str> {
        match &self.node(id)?.kind {
            Kind::TextField(field) => Some(field.value()),
            _ => None,
        }
    }

    /// Replaces the text of a field, the caret at its end.
    pub fn set_field_value(&mut self, id: NodeId, text: &str) {
        if let Some(Kind::TextField(field)) = self.node_mut(id).map(|n| &mut n.kind)
            && field.value() != text
        {
            field.set(text, text.len());
            self.mark(id);
            self.relayout = true;
        }
    }

    // Accessibility.

    pub fn access(&self) -> AccessNode {
        self.access_node(self.root).unwrap_or_else(|| AccessNode {
            role: Role::Group,
            label: None,
            rect: Rect::default(),
            focused: false,
            disabled: false,
            selected: false,
            children: Vec::new(),
        })
    }

    fn access_node(&self, id: NodeId) -> Option<AccessNode> {
        let node = self.node(id)?;
        let (role, label) = match &node.kind {
            Kind::Box(_) if node.clickable.is_some() => (Role::Button, node.clickable.clone()),
            Kind::Box(_) => (Role::Group, None),
            Kind::Text(t) => (Role::Text, Some(t.text().to_owned())),
            Kind::Button(b) => (Role::Button, Some(b.name())),
            Kind::Image { alt, .. } => (Role::Image, Some(alt.clone())),
            // Decorative icons say nothing.
            Kind::Icon { label: None, .. } => return None,
            Kind::Icon { label, .. } => (Role::Icon, label.clone()),
            Kind::Custom { label, .. } => (Role::Custom, label.clone()),
            Kind::TextField(field) => (Role::Field, Some(field.name.clone())),
            Kind::Slider { name, value, .. } => (
                Role::Slider,
                Some(format!("{name} {}%", (value * 100.0).round())),
            ),
        };
        Some(AccessNode {
            role,
            label,
            rect: node.rect,
            focused: self.focused == Some(id),
            disabled: matches!(&node.kind, Kind::Button(b) if !b.enabled),
            selected: matches!(&node.kind, Kind::Button(b) if b.selected) || node.chosen,
            children: node
                .children
                .iter()
                .filter_map(|c| self.access_node(*c))
                .collect(),
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_kind(
    canvas: &mut Canvas,
    kind: &mut Kind,
    rect: Rect,
    theme: &Theme,
    fonts: &mut Fonts,
    scale: f32,
    (hovered, pressed, caret): (bool, bool, bool),
    now: Duration,
) {
    match kind {
        Kind::Box(look) => {
            // A clickable box shows the pointer over it.
            let tint = match (pressed, hovered) {
                (true, _) => Some(theme.colors.surface_pressed),
                (false, true) => Some(theme.colors.surface_hover),
                _ => None,
            };
            if look.shadow > 0.0 {
                canvas.shadow(
                    rect.translate(0.0, look.shadow / 3.0),
                    look.radius,
                    look.shadow,
                    theme.colors.shadow,
                );
            }
            if let Some(bg) = tint.or(look.background.map(|bg| theme.color(bg))) {
                canvas.fill_rect(rect, look.radius, bg);
            }
            if let Some(border) = look.border {
                canvas.stroke_rect(rect, look.radius, 1.0, theme.color(border));
            }
        }
        Kind::Text(text) => {
            let color = theme.color(text.style().color);
            let layout = text.layout(fonts, Some(rect.w), scale);
            canvas.text(layout, Point::new(rect.x, rect.y), color);
        }
        Kind::Button(b) => {
            let c = &theme.colors;
            let (hovered, pressed) = (hovered && b.enabled, pressed && b.enabled);
            let tint = c.accent.with_alpha(0x29);
            let (bg, border, fg) = match (b.kind, b.selected) {
                (ButtonKind::Primary, _) => {
                    let bg = if pressed {
                        c.accent.mix(Color::BLACK, 0.15)
                    } else if hovered {
                        c.accent.mix(Color::WHITE, 0.12)
                    } else {
                        c.accent
                    };
                    (Some(bg), None, c.on_accent)
                }
                (_, true) => (Some(tint), Some(c.accent), c.accent),
                (kind, false) => {
                    let rest = match kind {
                        ButtonKind::Plain => Some(c.surface),
                        ButtonKind::Chip => Some(c.surface_hover),
                        _ => None,
                    };
                    let bg = if pressed {
                        Some(c.surface_pressed)
                    } else if hovered {
                        Some(if kind == ButtonKind::Chip {
                            c.surface_pressed
                        } else {
                            c.surface_hover
                        })
                    } else {
                        rest
                    };
                    let border = (kind == ButtonKind::Plain).then_some(c.border);
                    (bg, border, theme.color(b.label.style().color))
                }
            };
            let fg = if b.enabled { fg } else { c.muted };
            let radius = match b.kind {
                ButtonKind::Primary | ButtonKind::Plain => theme.radii.small,
                ButtonKind::Chip | ButtonKind::Ghost => theme.radii.medium,
            };
            if let Some(bg) = bg {
                canvas.fill_rect(rect, radius, bg);
            }
            if let Some(border) = border {
                canvas.stroke_rect(rect, radius, 1.0, border);
            }
            let has_text = !b.label.text().is_empty();
            let text = if has_text {
                b.label.measure(fonts, None, scale)
            } else {
                Size::default()
            };
            let icon = b.icon.as_ref().map_or(0.0, |i| i.size);
            let gap = if icon > 0.0 && has_text {
                theme.spacing.xs * 1.5
            } else {
                0.0
            };
            let mut x = if b.start {
                rect.x + theme.spacing.m
            } else {
                rect.x + (rect.w - (icon + gap + text.w)) / 2.0
            };
            if let Some(i) = &b.icon {
                let at = Rect::new(x, rect.y + (rect.h - i.size) / 2.0, i.size, i.size);
                i.draw(canvas, at, fg);
                x += i.size + gap;
            }
            if has_text && let Some(layout) = b.label.laid_out() {
                canvas.text(layout, Point::new(x, rect.y + (rect.h - text.h) / 2.0), fg);
            }
        }
        Kind::Image { pixmap, .. } => canvas.image(pixmap, rect),
        Kind::Icon { icon, color, .. } => {
            let at = Rect::new(
                rect.x + (rect.w - icon.size) / 2.0,
                rect.y + (rect.h - icon.size) / 2.0,
                icon.size,
                icon.size,
            );
            icon.draw(canvas, at, theme.color(*color));
        }
        Kind::Custom { draw, .. } => draw.draw(canvas, rect, theme, now),
        Kind::Slider { value, .. } => {
            let c = &theme.colors;
            let y = rect.y + rect.h / 2.0;
            let track = Rect::new(rect.x, y - 2.0, rect.w, 4.0);
            canvas.fill_rect(track, 2.0, c.muted.with_alpha(0x55));
            let x = rect.x + rect.w * *value;
            canvas.fill_rect(Rect::new(rect.x, y - 2.0, x - rect.x, 4.0), 2.0, c.accent);
            let r = if hovered || pressed { 9.0 } else { 8.0 };
            canvas.fill_rect(Rect::new(x - r, y - r, 2.0 * r, 2.0 * r), r, c.text);
        }
        Kind::TextField(field) => {
            let empty = field.value.text().is_empty();
            let shown = if empty {
                &mut field.placeholder
            } else {
                &mut field.value
            };
            let color = theme.color(shown.style().color);
            let size = shown.measure(fonts, None, scale);
            let y = rect.y + (rect.h - size.h) / 2.0;
            if let Some(layout) = shown.laid_out() {
                canvas.push_clip(rect, 0.0);
                canvas.text(layout, Point::new(rect.x, y), color);
                canvas.pop();
            }
            if caret {
                let before = &field.value.text()[..field.cursor];
                let x = TextBox::new(before, *field.value.style())
                    .measure(fonts, None, scale)
                    .w;
                let caret = Rect::new((rect.x + x).min(rect.right() - 2.0), y, 2.0, size.h);
                canvas.fill_rect(caret, 1.0, theme.colors.accent);
            }
        }
    }
}

/// Merges overlapping regions, so that no pixel is drawn twice.
fn merge(mut rects: Vec<Rect>) -> Vec<Rect> {
    rects.retain(|r| !r.is_empty());
    let mut merged: Vec<Rect> = Vec::new();
    while let Some(mut r) = rects.pop() {
        while let Some(i) = merged.iter().position(|m| m.intersects(&r)) {
            r = r.union(&merged.swap_remove(i));
        }
        merged.push(r);
    }
    merged.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
    merged
}

/// The pixels of a tree, kept between frames: only damaged regions are drawn
/// again.
pub struct Surface {
    pixmap: Pixmap,
    scale: f32,
    canvas: Option<Canvas>,
    scratch: Pixmap,
}

impl fmt::Debug for Surface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Surface")
            .field("width", &self.pixmap.width())
            .field("height", &self.pixmap.height())
            .field("scale", &self.scale)
            .finish_non_exhaustive()
    }
}

impl Surface {
    /// A transparent surface for a tree of `size` at `scale`.
    pub fn new(size: Size, scale: f32) -> Self {
        let w = (size.w * scale).ceil() as u16;
        let h = (size.h * scale).ceil() as u16;
        Self {
            pixmap: Pixmap::new(w, h),
            scale,
            canvas: None,
            scratch: Pixmap::new(1, 1),
        }
    }

    pub fn width(&self) -> u32 {
        u32::from(self.pixmap.width())
    }

    pub fn height(&self) -> u32 {
        u32::from(self.pixmap.height())
    }

    /// Premultiplied RGBA, row by row.
    pub fn pixels(&self) -> &[u8] {
        self.pixmap.data_as_u8_slice()
    }

    /// Draws the damaged regions of `tree`; returns them in physical pixels,
    /// for the compositor's own damage.
    pub fn draw(&mut self, tree: &mut Tree, damage: &[Rect]) -> Vec<PixelRect> {
        let (w, h) = (self.width(), self.height());
        let mut drawn = Vec::new();
        for region in damage {
            let px = region.to_physical(self.scale).clamp(w, h);
            if px.is_empty() {
                continue;
            }
            let reused = self
                .canvas
                .as_mut()
                .is_some_and(|c| c.reuse(px, self.scale));
            if !reused {
                self.canvas = Some(Canvas::new(px, self.scale));
            }
            let Some(canvas) = self.canvas.as_mut() else {
                continue;
            };
            // The region in logical pixels, as drawn: whole physical pixels.
            let logical = Rect::new(
                px.x as f32 / self.scale,
                px.y as f32 / self.scale,
                px.w as f32 / self.scale,
                px.h as f32 / self.scale,
            );
            tree.draw(canvas, logical);
            if u32::from(self.scratch.width()) != px.w || u32::from(self.scratch.height()) != px.h {
                self.scratch = Pixmap::new(px.w as u16, px.h as u16);
            } else {
                self.scratch.data_as_u8_slice_mut().fill(0);
            }
            canvas.finish(&mut self.scratch);
            let stride = w as usize * 4;
            let row = px.w as usize * 4;
            let src = self.scratch.data_as_u8_slice();
            let dst = self.pixmap.data_as_u8_slice_mut();
            for y in 0..px.h as usize {
                let d = (px.y as usize + y) * stride + px.x as usize * 4;
                dst[d..d + row].copy_from_slice(&src[y * row..(y + 1) * row]);
            }
            drawn.push(px);
        }
        drawn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_regions_merge() {
        let merged = merge(vec![
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Rect::new(5.0, 5.0, 10.0, 10.0),
            Rect::new(100.0, 0.0, 10.0, 10.0),
        ]);
        assert_eq!(
            merged,
            vec![
                Rect::new(0.0, 0.0, 15.0, 15.0),
                Rect::new(100.0, 0.0, 10.0, 10.0)
            ]
        );
    }
}
