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
    #[default]
    Plain,
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
    Button {
        label: TextBox,
        kind: ButtonKind,
    },
    Image {
        pixmap: Arc<Pixmap>,
        /// What the image shows, for screen readers.
        alt: String,
    },
    Icon {
        /// SVG path data in a `view`×`view` box.
        path: BezPath,
        view: f32,
        size: f32,
        color: Paint,
        /// What it means; `None` for a decorative icon.
        label: Option<String>,
    },
    Custom {
        draw: Box<dyn Draw>,
        label: Option<String>,
    },
}

impl fmt::Debug for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Box(look) => f.debug_tuple("Box").field(look).finish(),
            Self::Text(text) => f.debug_tuple("Text").field(text).finish(),
            Self::Button { label, kind } => f
                .debug_struct("Button")
                .field("label", label)
                .field("kind", kind)
                .finish(),
            Self::Image { alt, .. } => f.debug_struct("Image").field("alt", alt).finish(),
            Self::Icon { label, .. } => f.debug_struct("Icon").field("label", label).finish(),
            Self::Custom { label, .. } => f.debug_struct("Custom").field("label", label).finish(),
        }
    }
}

impl Kind {
    pub fn text(text: impl Into<String>, style: TextStyle) -> Self {
        Self::Text(TextBox::new(text, style))
    }

    pub fn button(label: impl Into<String>, kind: ButtonKind, theme: &Theme) -> Self {
        let style = match kind {
            ButtonKind::Primary => theme.text.label.with_color(Paint::OnAccent),
            ButtonKind::Plain => theme.text.label,
        };
        Self::Button {
            label: TextBox::new(label, style),
            kind,
        }
    }

    /// An icon from SVG path data in a 24×24 box.
    pub fn icon(svg_path: &str, size: f32, color: Paint, label: Option<&str>) -> Option<Self> {
        Some(Self::Icon {
            path: BezPath::from_svg(svg_path).ok()?,
            view: 24.0,
            size,
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
        matches!(self, Self::Button { .. })
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
}

impl Node {
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
}

/// What came out of input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A button was clicked, or pressed from the keyboard.
    Activated(NodeId),
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
            Some(Kind::Text(t) | Kind::Button { label: t, .. }) => t.set_text(text),
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
                for child in &children {
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
                main += style.gap * children.len().saturating_sub(1) as f32;
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
            Kind::Button { label, .. } => {
                let s = label.measure(fonts, None, scale);
                Some(Size::new(
                    s.w + 2.0 * spacing.l,
                    (s.h + 2.0 * spacing.s).max(32.0),
                ))
            }
            Kind::Image { pixmap, .. } => Some(Size::new(
                f32::from(pixmap.width()) / scale,
                f32::from(pixmap.height()) / scale,
            )),
            Kind::Icon { size, .. } => Some(Size::new(*size, *size)),
            Kind::Custom { draw, .. } => Some(draw.size()),
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
                    (hovered, pressed),
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
        (rect.contains(p) && (!interactive || node.kind.focusable())).then_some(id)
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
                if node.kind.focusable() {
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
            }
            Input::PointerButton { pressed: false } => {
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
            Input::Key(Key::Enter | Key::Space) => {
                events.extend(self.focused.map(Event::Activated));
            }
        }
        events
    }

    // Accessibility.

    pub fn access(&self) -> AccessNode {
        self.access_node(self.root).unwrap_or_else(|| AccessNode {
            role: Role::Group,
            label: None,
            rect: Rect::default(),
            focused: false,
            children: Vec::new(),
        })
    }

    fn access_node(&self, id: NodeId) -> Option<AccessNode> {
        let node = self.node(id)?;
        let (role, label) = match &node.kind {
            Kind::Box(_) => (Role::Group, None),
            Kind::Text(t) => (Role::Text, Some(t.text().to_owned())),
            Kind::Button { label, .. } => (Role::Button, Some(label.text().to_owned())),
            Kind::Image { alt, .. } => (Role::Image, Some(alt.clone())),
            // Decorative icons say nothing.
            Kind::Icon { label: None, .. } => return None,
            Kind::Icon { label, .. } => (Role::Icon, label.clone()),
            Kind::Custom { label, .. } => (Role::Custom, label.clone()),
        };
        Some(AccessNode {
            role,
            label,
            rect: node.rect,
            focused: self.focused == Some(id),
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
    (hovered, pressed): (bool, bool),
    now: Duration,
) {
    match kind {
        Kind::Box(look) => {
            if look.shadow > 0.0 {
                canvas.shadow(
                    rect.translate(0.0, look.shadow / 3.0),
                    look.radius,
                    look.shadow,
                    theme.colors.shadow,
                );
            }
            if let Some(bg) = look.background {
                canvas.fill_rect(rect, look.radius, theme.color(bg));
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
        Kind::Button { label, kind } => {
            let kind = *kind;
            let c = &theme.colors;
            let bg = match (kind, pressed, hovered) {
                (ButtonKind::Primary, true, _) => c.accent.mix(Color::rgb(0, 0, 0), 0.15),
                (ButtonKind::Primary, false, true) => c.accent.mix(Color::rgb(255, 255, 255), 0.12),
                (ButtonKind::Primary, false, false) => c.accent,
                (ButtonKind::Plain, true, _) => c.surface_pressed,
                (ButtonKind::Plain, false, true) => c.surface_hover,
                (ButtonKind::Plain, false, false) => c.surface,
            };
            canvas.fill_rect(rect, theme.radii.small, bg);
            if kind == ButtonKind::Plain {
                canvas.stroke_rect(rect, theme.radii.small, 1.0, c.border);
            }
            let color = theme.color(label.style().color);
            let size = label.measure(fonts, None, scale);
            let at = Point::new(
                rect.x + (rect.w - size.w) / 2.0,
                rect.y + (rect.h - size.h) / 2.0,
            );
            if let Some(layout) = label.laid_out() {
                canvas.text(layout, at, color);
            }
        }
        Kind::Image { pixmap, .. } => canvas.image(pixmap, rect),
        Kind::Icon {
            path,
            view,
            size,
            color,
            ..
        } => {
            let at = Rect::new(
                rect.x + (rect.w - *size) / 2.0,
                rect.y + (rect.h - *size) / 2.0,
                *size,
                *size,
            );
            canvas.path(path, *view, at, theme.color(*color));
        }
        Kind::Custom { draw, .. } => draw.draw(canvas, rect, theme, now),
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
