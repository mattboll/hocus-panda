//! The radial menu at the pointer (#40, decision 0006): a window's actions
//! on a ring around the pointer, as board D01 draws them, clockwise from
//! the top: Maximize, Pin, Next screen, Desktop…, Minimize, Always on top,
//! Snap and, in red at the top left, Close. Desktop… and Snap open a ring
//! of their own: the screen's desktops, the snap zones.
//!
//! Held, a quick flick picks the action it points at on release without
//! waiting for anything to show; a slow move lights the sector up and the
//! release picks it; let go in the centre, nothing. A press let go without
//! moving leaves it open for a click or the keys. Close is never flicked:
//! the pointer rests on it while a ring fills, or it is clicked.
//!
//! The compositor places it, routes the pointer and the keys, and does what
//! comes out; [`Gesture`] tells a flick from a slow move.

use std::{collections::VecDeque, f32::consts::PI, time::Duration};

use hocus_core::Zone;
use hocus_ui::{
    AccessNode, Draw, Icon, Insets, Key, Kind, Length, Look, NodeId, Paint, PixelRect, Point,
    Property, Rect, Size, Style, Surface, Theme, Tree,
    canvas::Canvas,
    motion::{Curve, Easing, SMALL, STANDARD, Spring},
};

use crate::{column::View, i18n::Lang, icons};

/// The panel, logical pixels, square; the pointer at its middle.
pub const SIZE: f32 = 480.0;
/// What it shows reaches this far from its middle: its labels included,
/// kept on the screen.
pub const REACH: f32 = 232.0;
/// The disc behind the ring.
const DISC: f32 = 180.0;
/// From the middle to each action's centre.
const RING: f32 = 165.0;
const BUTTON: f32 = 64.0;
/// An action with its label under it.
const ITEM: f32 = 120.0;
/// The rest ring around Close.
const REST_RING: f32 = 80.0;

/// Let go nearer than this to where it was pressed, nothing (unless it was
/// a flick); a slow move beyond it lights its sector up.
pub const CENTRE: f32 = 56.0;
/// A flick goes at least this far…
pub const FLICK_DISTANCE: f32 = 28.0;
/// …this fast, logical pixels per second, over the last moments.
pub const FLICK_SPEED: f32 = 500.0;
/// The last moments.
pub const SPEED_WINDOW: Duration = Duration::from_millis(100);
/// Moved less than this, a press let go is a click: the menu stays.
pub const CLICK_SLOP: f32 = 6.0;
/// Resting on Close this long, it is armed: let go, the window closes.
pub const CLOSE_REST: Duration = Duration::from_millis(300);

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");

/// The ring's eight places, clockwise from the top, as D01 orders them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Maximize,
    Pin,
    NextScreen,
    Desktop,
    Minimize,
    OnTop,
    Snap,
    Close,
}

impl Slot {
    pub const ALL: [Slot; 8] = [
        Self::Maximize,
        Self::Pin,
        Self::NextScreen,
        Self::Desktop,
        Self::Minimize,
        Self::OnTop,
        Self::Snap,
        Self::Close,
    ];
}

/// The snap zones, where they point.
const ZONES: [(Zone, f32); 6] = [
    (Zone::Right, 0.0),
    (Zone::BottomRight, 45.0),
    (Zone::BottomLeft, 135.0),
    (Zone::Left, 180.0),
    (Zone::TopLeft, 225.0),
    (Zone::TopRight, 315.0),
];

/// The window the menu is for, as the compositor knows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Subject {
    pub maximized: bool,
    pub pinned: bool,
    pub on_top: bool,
    /// Another screen to send it to.
    pub other_screen: bool,
    /// Its screen's desktops, each one's name or its number's.
    pub desktops: Vec<String>,
    /// The one it is on, from 0.
    pub desktop: Option<usize>,
}

/// What the compositor does to the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    ToggleMaximized,
    TogglePinned,
    NextScreen,
    /// Its screen's desktop, from 0.
    ToDesktop(usize),
    Minimize,
    ToggleOnTop,
    Snap(Zone),
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadialEvent {
    Act(Act),
    /// Nothing chosen: the menu goes.
    Close,
}

/// What a release did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The menu stays, for a click or the keys.
    Open,
    Event(RadialEvent),
}

/// Which ring shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ring {
    Main,
    Desktops,
    Zones,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Slot(Slot),
    Desktop(usize),
    Zone(Zone),
    /// The middle of a ring of its own: back to the eight.
    Back,
}

#[derive(Debug)]
struct Item {
    node: NodeId,
    button: NodeId,
    /// What is drawn on the button: an icon, a number.
    marks: Vec<NodeId>,
    label: NodeId,
    rest: Option<NodeId>,
    /// Degrees from 3 o'clock, clockwise; none in the middle.
    angle: Option<f32>,
    choice: Choice,
    enabled: bool,
    /// A toggle that is on, the desktop it is on.
    on: bool,
}

impl Item {
    fn danger(&self) -> bool {
        self.choice == Choice::Slot(Slot::Close)
    }

    /// Dimmed when there is nothing to do.
    fn opacity(&self) -> f32 {
        if self.enabled { 1.0 } else { 0.38 }
    }
}

/// The pointer's way since the press: how far it went, how fast at the
/// end.
#[derive(Debug, Clone)]
pub struct Gesture {
    origin: Point,
    at: Point,
    /// The farthest it went from the press.
    far: f32,
    /// Where it was lately, oldest first: the latest at or before the
    /// last moments, then the ones within them.
    trail: VecDeque<(Duration, Point)>,
}

impl Gesture {
    pub fn new(origin: Point, now: Duration) -> Self {
        Self {
            origin,
            at: origin,
            far: 0.0,
            trail: VecDeque::from([(now, origin)]),
        }
    }

    pub fn moved(&mut self, to: Point, now: Duration) {
        self.at = to;
        self.far = self.far.max(self.distance());
        self.trail.push_back((now, to));
        let cut = now.saturating_sub(SPEED_WINDOW);
        while self.trail.len() > 2 && self.trail.get(1).is_some_and(|(t, _)| *t <= cut) {
            self.trail.pop_front();
        }
        // Bounded whatever the device sends within the last moments.
        while self.trail.len() > 64 {
            self.trail.pop_front();
        }
    }

    /// From the press to where it is now.
    pub fn distance(&self) -> f32 {
        (self.at.x - self.origin.x).hypot(self.at.y - self.origin.y)
    }

    /// Where it points, degrees from 3 o'clock, clockwise (y goes down).
    pub fn angle(&self) -> f32 {
        (self.at.y - self.origin.y)
            .atan2(self.at.x - self.origin.x)
            .to_degrees()
    }

    /// It hardly moved: a click.
    pub fn clicked(&self) -> bool {
        self.far < CLICK_SLOP
    }

    /// Logical pixels per second over the last moments, to `now`.
    pub fn speed(&self, now: Duration) -> f32 {
        let cut = now.saturating_sub(SPEED_WINDOW);
        let Some((t, p)) = self
            .trail
            .iter()
            .rev()
            .find(|(t, _)| *t <= cut)
            .or_else(|| self.trail.front())
        else {
            return 0.0;
        };
        let way = (self.at.x - p.x).hypot(self.at.y - p.y);
        let dt = now.saturating_sub(*t).as_secs_f32();
        if way == 0.0 {
            0.0
        } else if dt == 0.0 {
            f32::INFINITY
        } else {
            way / dt
        }
    }

    /// Far and fast enough to be a flick, let go at `now`.
    pub fn flicked(&self, now: Duration) -> bool {
        self.distance() >= FLICK_DISTANCE && self.speed(now) >= FLICK_SPEED
    }
}

/// The dotted line from the middle towards the lit action, as D01 draws
/// it.
struct Guide {
    angle: Option<f32>,
    /// Towards Close: in red.
    danger: bool,
}

impl Draw for Guide {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, _now: Duration) {
        let Some(angle) = self.angle else {
            return;
        };
        let (s, c) = angle.to_radians().sin_cos();
        let (cx, cy) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        let color = if self.danger {
            theme.colors.error
        } else {
            theme.colors.accent
        };
        let mut r = 16.0;
        while r <= 106.0 {
            let p = Point::new(cx + r * c, cy + r * s);
            canvas.polyline(&[p, Point::new(p.x + 0.01, p.y)], 5.0, color);
            r += 12.0;
        }
    }

    fn size(&self) -> Size {
        Size::new(2.0 * DISC, 2.0 * DISC)
    }
}

/// The ring filling around Close while the pointer rests on it.
struct Rest {
    since: Option<Duration>,
}

impl Draw for Rest {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, now: Duration) {
        let Some(since) = self.since else {
            return;
        };
        let done = (now.saturating_sub(since).as_secs_f32() / CLOSE_REST.as_secs_f32()).min(1.0);
        let center = Point::new(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        let radius = REST_RING / 2.0 - 2.5;
        let error = theme.colors.error;
        canvas.arc(center, radius, 0.0, 2.0 * PI, 3.0, error.with_alpha(0x33));
        if done > 0.0 {
            canvas.arc(center, radius, -PI / 2.0, 2.0 * PI * done, 3.0, error);
        }
    }

    fn size(&self) -> Size {
        Size::new(REST_RING, REST_RING)
    }

    fn is_moving(&self, now: Duration) -> bool {
        self.since.is_some_and(|s| now < s + CLOSE_REST)
    }
}

pub struct Radial {
    tree: Tree,
    surface: Surface,
    lang: Lang,
    subject: Subject,
    guide: NodeId,
    ring: Ring,
    items: Vec<Item>,
    lit: Option<usize>,
    /// Since when the held pointer rests on Close.
    resting: Option<Duration>,
    /// The action a press of a click landed on.
    pressed: Option<usize>,
    closing: bool,
}

impl std::fmt::Debug for Radial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Radial")
            .field("ring", &self.ring)
            .field("lit", &self.lit)
            .field("closing", &self.closing)
            .finish_non_exhaustive()
    }
}

fn stroked(path: &str) -> Icon {
    Icon::stroked(path, 26.0, 2.2).unwrap_or_else(|| unreachable!("icon {path}"))
}

fn filled(path: &str) -> Icon {
    Icon::filled(path, 26.0).unwrap_or_else(|| unreachable!("icon {path}"))
}

const PIN: &str = "M12 17v5M5 9l7-7 7 7-3 3v3H8v-3z";
const NEXT_SCREEN: &str = "M4 12h14M13 6l6 6-6 6";
const DESKTOPS: &str = "M3 5h8v6H3zM13 5h8v6h-8zM3 13h8v6H3zM13 13h8v6h-8z";
const ON_TOP: &str = "M6 15l6-6 6 6M6 20h12";
const SNAP: &str = "M3 4h8v16H3zM15 4h6v16h-6z";
const BACK: &str = "M15 6l-6 6 6 6";
const SCREEN: &str = "M3 5h18v14H3z";

/// A zone drawn filled inside the screen's outline.
fn zone_shape(zone: Zone) -> &'static str {
    match zone {
        Zone::Left => "M3 5h9v14H3z",
        Zone::Right => "M12 5h9v14h-9z",
        Zone::TopLeft => "M3 5h9v7H3z",
        Zone::TopRight => "M12 5h9v7h-9z",
        Zone::BottomLeft => "M3 12h9v7H3z",
        Zone::BottomRight => "M12 12h9v7h-9z",
        Zone::Top => SCREEN,
    }
}

fn zone_label(zone: Zone, lang: Lang) -> &'static str {
    match zone {
        Zone::Left => lang.pick("Left half", "Moitié gauche"),
        Zone::Right => lang.pick("Right half", "Moitié droite"),
        Zone::TopLeft => lang.pick("Top left", "En haut à gauche"),
        Zone::TopRight => lang.pick("Top right", "En haut à droite"),
        Zone::BottomLeft => lang.pick("Bottom left", "En bas à gauche"),
        Zone::BottomRight => lang.pick("Bottom right", "En bas à droite"),
        Zone::Top => lang.pick("Whole screen", "Tout l’écran"),
    }
}

/// The angular distance between two directions, degrees.
fn apart(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

impl Radial {
    /// The eight actions for `subject`, unfolding from the middle from
    /// `now` (at once with reduced motion).
    pub fn new(theme: Theme, lang: Lang, subject: Subject, scale: f32, now: Duration) -> Self {
        let size = Size::new(SIZE, SIZE);
        let mut tree = Tree::new(theme, size, scale);
        let root = tree.root();
        tree.set_look(root, Look::default());
        let background = tree.theme().colors.background;
        let corner = SIZE / 2.0 - DISC;
        let disc = tree
            .add(
                root,
                Style {
                    at: Some(Point::new(corner, corner)),
                    width: Length::Px(2.0 * DISC),
                    height: Length::Px(2.0 * DISC),
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Rgba(background.with_alpha(0x8C))),
                    border: Some(Paint::Border),
                    radius: DISC,
                    shadow: 0.0,
                }),
            )
            .unwrap_or_else(MISSING);
        let guide = tree
            .add(
                disc,
                Style {
                    at: Some(Point::new(0.0, 0.0)),
                    ..Style::default()
                },
                Kind::Custom {
                    draw: Box::new(Guide {
                        angle: None,
                        danger: false,
                    }),
                    label: None,
                },
            )
            .unwrap_or_else(MISSING);
        // Laid out and drawn once, at `now`, before anything moves.
        tree.update(now);
        let mut radial = Self {
            tree,
            surface: Surface::new(size, scale),
            lang,
            subject,
            guide,
            ring: Ring::Main,
            items: Vec::new(),
            lit: None,
            resting: None,
            pressed: None,
            closing: false,
        };
        radial.show_ring(Ring::Main);
        radial.tree.jump(disc, Property::Opacity, 0.0);
        let fade = Curve::Ease(Easing::OutCubic, SMALL);
        radial.tree.animate(disc, Property::Opacity, 1.0, fade);
        for i in 0..radial.items.len() {
            let item = &radial.items[i];
            let (node, (dx, dy), seen) = (item.node, offset_of(item.angle), item.opacity());
            radial.tree.jump(node, Property::OffsetX, -dx);
            radial.tree.jump(node, Property::OffsetY, -dy);
            radial.tree.jump(node, Property::Opacity, 0.0);
            let spring = Curve::Spring(Spring::SMOOTH);
            radial.tree.animate(node, Property::OffsetX, 0.0, spring);
            radial.tree.animate(node, Property::OffsetY, 0.0, spring);
            radial.tree.animate(node, Property::Opacity, seen, fade);
        }
        let damage = radial.tree.update(now);
        radial.surface.draw(&mut radial.tree, &damage);
        radial
    }

    /// Puts the actions of `ring` in place of those shown.
    fn show_ring(&mut self, ring: Ring) {
        for item in std::mem::take(&mut self.items) {
            self.tree.remove(item.node);
        }
        self.ring = ring;
        self.lit = None;
        self.resting = None;
        self.pressed = None;
        let lang = self.lang;
        let s = self.subject.clone();
        match ring {
            Ring::Main => {
                for (i, slot) in Slot::ALL.into_iter().enumerate() {
                    let angle = -90.0 + 45.0 * i as f32;
                    let (label, icon, enabled, on): (&str, &str, bool, bool) = match slot {
                        Slot::Maximize if s.maximized => {
                            (lang.restore(), icons::RESTORE, true, false)
                        }
                        Slot::Maximize => (lang.maximize(), icons::MAXIMIZE, true, false),
                        Slot::Pin if s.pinned => {
                            (lang.pick("Unpin", "Désépingler"), PIN, true, true)
                        }
                        Slot::Pin => (lang.pick("Pin", "Épingler"), PIN, true, false),
                        Slot::NextScreen => (
                            lang.pick("Next screen", "Écran suivant"),
                            NEXT_SCREEN,
                            s.other_screen,
                            false,
                        ),
                        Slot::Desktop => (
                            lang.pick("Desktop…", "Bureau…"),
                            DESKTOPS,
                            s.desktops.len() > 1,
                            false,
                        ),
                        Slot::Minimize => (lang.minimize(), icons::MINIMIZE, true, false),
                        Slot::OnTop if s.on_top => {
                            (lang.pick("Not on top", "Pas au-dessus"), ON_TOP, true, true)
                        }
                        Slot::OnTop => {
                            (lang.pick("Always on top", "Au-dessus"), ON_TOP, true, false)
                        }
                        Slot::Snap => (lang.pick("Snap", "Ancrer"), SNAP, true, false),
                        Slot::Close => (lang.close(), icons::CLOSE, true, false),
                    };
                    self.add_item(
                        Some(angle),
                        Choice::Slot(slot),
                        label,
                        &[stroked(icon)],
                        None,
                        enabled,
                        on,
                    );
                }
            }
            Ring::Desktops => {
                let n = s.desktops.len().max(1);
                for (i, name) in s.desktops.iter().enumerate() {
                    let angle = -90.0 + 360.0 * i as f32 / n as f32;
                    let number = (i + 1).to_string();
                    self.add_item(
                        Some(angle),
                        Choice::Desktop(i),
                        name,
                        &[],
                        Some(&number),
                        true,
                        s.desktop == Some(i),
                    );
                }
                self.add_back();
            }
            Ring::Zones => {
                for (zone, angle) in ZONES {
                    self.add_item(
                        Some(angle),
                        Choice::Zone(zone),
                        zone_label(zone, lang),
                        &[stroked(SCREEN), filled(zone_shape(zone))],
                        None,
                        true,
                        false,
                    );
                }
                self.add_back();
            }
        }
        for i in 0..self.items.len() {
            self.paint(i);
        }
        self.set_guide();
    }

    fn add_back(&mut self) {
        let back = self.lang.pick("Back", "Retour");
        self.add_item(
            None,
            Choice::Back,
            back,
            &[stroked(BACK)],
            None,
            true,
            false,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn add_item(
        &mut self,
        angle: Option<f32>,
        choice: Choice,
        label: &str,
        icons: &[Icon],
        number: Option<&str>,
        enabled: bool,
        on: bool,
    ) {
        let (dx, dy) = offset_of(angle);
        let root = self.tree.root();
        let at = Point::new(SIZE / 2.0 + dx - ITEM / 2.0, SIZE / 2.0 + dy - BUTTON / 2.0);
        let node = self
            .tree
            .add(
                root,
                Style {
                    at: Some(at),
                    width: Length::Px(ITEM),
                    align: hocus_ui::Align::Center,
                    gap: 6.0,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(MISSING);
        if enabled {
            self.tree.set_clickable(node, label);
        }
        let button = self
            .tree
            .add(
                node,
                Style {
                    width: Length::Px(BUTTON),
                    height: Length::Px(BUTTON),
                    justify: hocus_ui::Justify::Center,
                    align: hocus_ui::Align::Center,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(MISSING);
        let mut marks = Vec::new();
        let inset = (BUTTON - 26.0) / 2.0;
        for icon in icons {
            let mark = self
                .tree
                .add(
                    button,
                    Style {
                        at: Some(Point::new(inset, inset)),
                        ..Style::default()
                    },
                    Kind::Icon {
                        icon: icon.clone(),
                        color: Paint::Text,
                        label: None,
                    },
                )
                .unwrap_or_else(MISSING);
            marks.push(mark);
        }
        if let Some(number) = number {
            let style = self.tree.theme().text.title.with_weight(700);
            let mark = self
                .tree
                .add(button, Style::default(), Kind::text_line(number, style))
                .unwrap_or_else(MISSING);
            marks.push(mark);
        }
        let rest = (choice == Choice::Slot(Slot::Close)).then(|| {
            let out = (REST_RING - BUTTON) / 2.0;
            self.tree
                .add(
                    button,
                    Style {
                        at: Some(Point::new(-out, -out)),
                        ..Style::default()
                    },
                    Kind::Custom {
                        draw: Box::new(Rest { since: None }),
                        label: None,
                    },
                )
                .unwrap_or_else(MISSING)
        });
        let background = self.tree.theme().colors.background;
        let tag = self
            .tree
            .add(
                node,
                Style {
                    padding: Insets::symmetric(3.0, 9.0),
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Rgba(background.with_alpha(0xD9))),
                    radius: 8.0,
                    ..Look::default()
                }),
            )
            .unwrap_or_else(MISSING);
        let style = self.tree.theme().text.body.with_weight(600);
        let text = self
            .tree
            .add(tag, Style::default(), Kind::text_line(label, style))
            .unwrap_or_else(MISSING);
        let item = Item {
            node,
            button,
            marks,
            label: text,
            rest,
            angle,
            choice,
            enabled,
            on,
        };
        self.tree.jump(node, Property::Opacity, item.opacity());
        self.items.push(item);
    }

    /// Item `i`'s looks: lit, on, in red, or plain.
    fn paint(&mut self, i: usize) {
        let Some(item) = self.items.get(i) else {
            return;
        };
        let lit = self.lit == Some(i);
        let error = self.tree.theme().colors.error;
        let (background, border, fg, label) = match (lit, item.danger(), item.on) {
            (true, true, _) => (Paint::Error, None, Paint::Background, Paint::Error),
            (true, false, _) => (Paint::Accent, None, Paint::OnAccent, Paint::Accent),
            (false, true, _) => (
                Paint::Rgba(error.with_alpha(0x2E)),
                None,
                Paint::Error,
                Paint::Text,
            ),
            (false, false, true) => (
                Paint::Surface,
                Some(Paint::Accent),
                Paint::Accent,
                Paint::Text,
            ),
            (false, false, false) => (
                Paint::Surface,
                Some(Paint::Border),
                Paint::Text,
                Paint::Text,
            ),
        };
        let (node, button, label_node) = (item.node, item.button, item.label);
        let marks = item.marks.clone();
        self.tree.set_look(
            button,
            Look {
                background: Some(background),
                border,
                radius: BUTTON / 2.0,
                shadow: 1.0,
            },
        );
        for mark in marks {
            self.tree.set_icon_paint(mark, fg);
            let style = self.tree.theme().text.title.with_weight(700).with_color(fg);
            self.tree.set_text_style(mark, style);
        }
        let style = self
            .tree
            .theme()
            .text
            .body
            .with_weight(600)
            .with_color(label);
        self.tree.set_text_style(label_node, style);
        self.tree.set_selected_box(node, lit);
    }

    fn set_guide(&mut self) {
        let lit = self.lit.and_then(|i| self.items.get(i));
        let guide = Guide {
            angle: lit.and_then(|item| item.angle),
            danger: lit.is_some_and(Item::danger),
        };
        self.tree.set_draw(self.guide, Box::new(guide));
    }

    /// Lights item `i` up, or none.
    fn light(&mut self, lit: Option<usize>, now: Duration) {
        let lit = lit.filter(|i| self.items.get(*i).is_some_and(|it| it.enabled));
        if lit == self.lit {
            return;
        }
        let old = self.lit;
        self.lit = lit;
        for i in [old, lit].into_iter().flatten() {
            self.paint(i);
        }
        self.set_guide();
        // Resting on Close starts again each time it is reached.
        let close = lit.is_some_and(|i| self.items[i].danger());
        self.set_resting(close.then_some(now));
    }

    fn set_resting(&mut self, since: Option<Duration>) {
        self.resting = since;
        if let Some(rest) = self.items.iter().find_map(|i| i.rest) {
            self.tree.set_draw(rest, Box::new(Rest { since }));
        }
    }

    /// The action under `p`, from the panel's corner.
    fn item_at(&self, p: Point) -> Option<usize> {
        let hit = self.tree.hit(p, true)?;
        self.items.iter().position(|i| i.node == hit)
    }

    /// The action of the ring nearest the direction `angle`.
    fn toward(&self, angle: f32) -> Option<usize> {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| Some((i, apart(item.angle?, angle))))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// The action held over or pointed at by `gesture`.
    fn aimed(&self, gesture: &Gesture, hover: Option<Point>, beyond: f32) -> Option<usize> {
        hover
            .and_then(|p| self.item_at(p))
            .or_else(|| (gesture.distance() >= beyond).then(|| self.toward(gesture.angle()))?)
    }

    /// The button held, the pointer moved: the sector it points at lights
    /// up past the centre (or the action it is over).
    pub fn drag(&mut self, gesture: &Gesture, hover: Option<Point>, now: Duration) {
        if self.ring != Ring::Main || self.closing {
            return;
        }
        let aimed = self.aimed(gesture, hover, CENTRE);
        self.light(aimed, now);
    }

    /// The button let go at `now`.
    pub fn let_go(&mut self, gesture: &Gesture, hover: Option<Point>, now: Duration) -> Outcome {
        if gesture.clicked() {
            return Outcome::Open;
        }
        let flick = gesture.flicked(now);
        let slow = gesture.distance() >= CENTRE || hover.and_then(|p| self.item_at(p)).is_some();
        let Some(i) = self.aimed(gesture, hover, FLICK_DISTANCE) else {
            return Outcome::Event(RadialEvent::Close);
        };
        if self.items[i].danger() {
            // Never flicked: only once rested on.
            let armed = self.resting.is_some_and(|s| now >= s + CLOSE_REST) && self.lit == Some(i);
            return if armed {
                self.choose(i)
            } else {
                Outcome::Event(RadialEvent::Close)
            };
        }
        if flick || slow {
            self.choose(i)
        } else {
            Outcome::Event(RadialEvent::Close)
        }
    }

    /// Item `i` chosen: its action, or its own ring.
    fn choose(&mut self, i: usize) -> Outcome {
        let Some(item) = self.items.get(i).filter(|i| i.enabled) else {
            return Outcome::Event(RadialEvent::Close);
        };
        let act = match item.choice {
            Choice::Slot(Slot::Maximize) => Act::ToggleMaximized,
            Choice::Slot(Slot::Pin) => Act::TogglePinned,
            Choice::Slot(Slot::NextScreen) => Act::NextScreen,
            Choice::Slot(Slot::Minimize) => Act::Minimize,
            Choice::Slot(Slot::OnTop) => Act::ToggleOnTop,
            Choice::Slot(Slot::Close) => Act::Close,
            Choice::Desktop(n) => Act::ToDesktop(n),
            Choice::Zone(zone) => Act::Snap(zone),
            Choice::Slot(Slot::Desktop) => return self.open_ring(Ring::Desktops),
            Choice::Slot(Slot::Snap) => return self.open_ring(Ring::Zones),
            Choice::Back => return self.open_ring(Ring::Main),
        };
        Outcome::Event(RadialEvent::Act(act))
    }

    /// A ring of its own slides in, in the reading direction.
    fn open_ring(&mut self, ring: Ring) -> Outcome {
        self.show_ring(ring);
        let curve = Curve::Ease(Easing::OutCubic, STANDARD);
        for i in 0..self.items.len() {
            let (node, seen) = (self.items[i].node, self.items[i].opacity());
            self.tree.jump(node, Property::OffsetX, -24.0);
            self.tree.jump(node, Property::Opacity, 0.0);
            self.tree.animate(node, Property::OffsetX, 0.0, curve);
            self.tree.animate(node, Property::Opacity, seen, curve);
        }
        Outcome::Open
    }

    /// The pointer over the open menu, `pressed` for a button: an action
    /// lights up under it; a click on one chooses it, a press elsewhere
    /// closes the menu.
    pub fn pointer(
        &mut self,
        p: Point,
        pressed: Option<bool>,
        now: Duration,
    ) -> Option<RadialEvent> {
        if self.closing {
            return None;
        }
        let at = self.item_at(p).filter(|i| self.items[*i].enabled);
        match pressed {
            None => {
                // Off the actions, what the keys lit stays lit.
                if at.is_some() {
                    self.light(at, now);
                    // A click needs no rest: no ring then.
                    self.set_resting(None);
                }
                None
            }
            Some(true) => {
                self.pressed = at;
                at.is_none().then_some(RadialEvent::Close)
            }
            Some(false) => {
                let pressed = self.pressed.take();
                match pressed.filter(|p| at == Some(*p)) {
                    Some(i) => match self.choose(i) {
                        Outcome::Event(event) => Some(event),
                        Outcome::Open => None,
                    },
                    None => None,
                }
            }
        }
    }

    /// A key while it is open: the arrows go round, Enter chooses, Escape
    /// goes back to the eight or closes it.
    pub fn key(&mut self, key: Key, now: Duration) -> Option<RadialEvent> {
        if self.closing {
            return None;
        }
        let direction = match key {
            Key::Up => Some((0.0, -1.0)),
            Key::Down => Some((0.0, 1.0)),
            Key::Left => Some((-1.0, 0.0)),
            Key::Right => Some((1.0, 0.0)),
            _ => None,
        };
        if let Some((x, y)) = direction {
            // From the lit one, a step that way.
            let (lx, ly) = self
                .lit
                .and_then(|i| self.items[i].angle)
                .map_or((0.0, 0.0), |a| {
                    let (s, c) = a.to_radians().sin_cos();
                    (c * 0.5, s * 0.5)
                });
            let angle = (ly + y).atan2(lx + x).to_degrees();
            let to = self.toward_enabled(angle);
            self.light(to, now);
            self.set_resting(None);
            return None;
        }
        match key {
            Key::Tab | Key::BackTab => {
                let order: Vec<usize> = (0..self.items.len())
                    .filter(|i| self.items[*i].enabled)
                    .collect();
                let at = self.lit.and_then(|l| order.iter().position(|i| *i == l));
                let n = order.len();
                if n > 0 {
                    let next = match (at, key == Key::Tab) {
                        (None, _) => 0,
                        (Some(p), true) => (p + 1) % n,
                        (Some(p), false) => (p + n - 1) % n,
                    };
                    self.light(Some(order[next]), now);
                    self.set_resting(None);
                }
                None
            }
            Key::Enter | Key::Space => match self.lit.map(|i| self.choose(i))? {
                Outcome::Event(event) => Some(event),
                Outcome::Open => None,
            },
            Key::Escape if self.ring != Ring::Main => {
                self.open_ring(Ring::Main);
                None
            }
            Key::Escape => Some(RadialEvent::Close),
            _ => None,
        }
    }

    /// The enabled action nearest `angle`.
    fn toward_enabled(&self, angle: f32) -> Option<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.enabled)
            .filter_map(|(i, item)| Some((i, apart(item.angle?, angle))))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// It goes: the actions fold back into the middle as it fades, faster
    /// than it came.
    pub fn close(&mut self) {
        if self.closing {
            return;
        }
        self.closing = true;
        self.set_resting(None);
        let curve = Curve::Ease(Easing::InCubic, SMALL);
        let root = self.tree.root();
        self.tree.animate(root, Property::Opacity, 0.0, curve);
        for i in 0..self.items.len() {
            let (node, (dx, dy)) = (self.items[i].node, offset_of(self.items[i].angle));
            self.tree.animate(node, Property::OffsetX, -dx * 0.6, curve);
            self.tree.animate(node, Property::OffsetY, -dy * 0.6, curve);
        }
    }

    /// The labels of the actions shown, in order: for tests.
    pub fn labels(&self) -> Vec<String> {
        self.access()
            .children
            .iter()
            .filter(|n| n.role == hocus_ui::Role::Button)
            .filter_map(|n| n.label.clone())
            .collect()
    }

    pub fn is_animating(&self, now: Duration) -> bool {
        self.tree.is_animating(now)
    }

    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
        let damage = self.tree.update(now);
        self.surface.draw(&mut self.tree, &damage)
    }

    pub fn view(&self) -> View<'_> {
        View {
            pixels: self.surface.pixels(),
            width: self.surface.width(),
            height: self.surface.height(),
            at: Point::default(),
        }
    }

    pub fn access(&self) -> AccessNode {
        self.tree.access()
    }
}

/// From the middle to an action at `angle`; nothing for the middle.
fn offset_of(angle: Option<f32>) -> (f32, f32) {
    angle.map_or((0.0, 0.0), |a| {
        let (s, c) = a.to_radians().sin_cos();
        (RING * c, RING * s)
    })
}

/// Where the panel's corner goes so that the menu, around `pointer`, stays
/// on a screen of `size` (both from the screen's corner, logical).
pub fn place(pointer: Point, size: Size) -> Point {
    let keep = |p: f32, length: f32| {
        if length < 2.0 * REACH {
            length / 2.0
        } else {
            p.clamp(REACH, length - REACH)
        }
    };
    Point::new(
        keep(pointer.x, size.w) - SIZE / 2.0,
        keep(pointer.y, size.h) - SIZE / 2.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: Point = Point::new(640.0, 400.0);
    /// The panel's middle, from its corner.
    const MIDDLE: Point = Point::new(SIZE / 2.0, SIZE / 2.0);

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn subject() -> Subject {
        Subject {
            other_screen: true,
            desktops: vec!["Desktop 1".into(), "Mail".into(), "Desktop 3".into()],
            desktop: Some(0),
            ..Subject::default()
        }
    }

    fn menu(lang: Lang) -> Radial {
        let theme = Theme::default().with_reduced_motion(true);
        Radial::new(theme, lang, subject(), 1.0, ms(1000))
    }

    /// Pressed at `ORIGIN` at 1 s, moved by `by` at once, let go `after`
    /// that.
    fn gesture(menu: &mut Radial, by: (f32, f32), after: u64) -> Outcome {
        let mut g = Gesture::new(ORIGIN, ms(1000));
        g.moved(Point::new(ORIGIN.x + by.0, ORIGIN.y + by.1), ms(1000));
        menu.drag(&g, None, ms(1000));
        menu.let_go(&g, None, ms(1000 + after))
    }

    fn act(a: Act) -> Outcome {
        Outcome::Event(RadialEvent::Act(a))
    }

    #[test]
    fn eight_actions_clockwise_from_the_top_in_both_languages() {
        let en = menu(Lang::En).labels();
        assert_eq!(
            en,
            [
                "Maximize",
                "Pin",
                "Next screen",
                "Desktop…",
                "Minimize",
                "Always on top",
                "Snap",
                "Close"
            ]
        );
        let fr = menu(Lang::Fr).labels();
        assert_eq!(
            fr,
            [
                "Agrandir",
                "Épingler",
                "Écran suivant",
                "Bureau…",
                "Réduire",
                "Au-dessus",
                "Ancrer",
                "Fermer"
            ]
        );
    }

    #[test]
    fn a_flick_picks_at_once_and_a_slow_move_once_past_the_centre() {
        // 40 px up in 50 ms: a flick, short as it is.
        assert_eq!(
            gesture(&mut menu(Lang::En), (0.0, -40.0), 50),
            act(Act::ToggleMaximized)
        );
        // The same, slowly: still in the centre, nothing.
        assert_eq!(
            gesture(&mut menu(Lang::En), (0.0, -40.0), 400),
            Outcome::Event(RadialEvent::Close)
        );
        // Slowly past the centre: down, it minimizes.
        assert_eq!(
            gesture(&mut menu(Lang::En), (0.0, 80.0), 400),
            act(Act::Minimize)
        );
        // Barely moved: a click, it stays.
        assert_eq!(gesture(&mut menu(Lang::En), (2.0, 1.0), 50), Outcome::Open);
        // Back in the middle after a move: nothing.
        let mut m = menu(Lang::En);
        let mut g = Gesture::new(ORIGIN, ms(1000));
        g.moved(Point::new(ORIGIN.x, ORIGIN.y - 90.0), ms(1050));
        g.moved(Point::new(ORIGIN.x + 3.0, ORIGIN.y), ms(1300));
        m.drag(&g, None, ms(1300));
        assert_eq!(
            m.let_go(&g, None, ms(1500)),
            Outcome::Event(RadialEvent::Close)
        );
    }

    #[test]
    fn close_is_never_flicked_only_rested_on() {
        let up_left = (-80.0, -80.0);
        assert_eq!(
            gesture(&mut menu(Lang::En), up_left, 50),
            Outcome::Event(RadialEvent::Close)
        );
        assert_eq!(
            gesture(&mut menu(Lang::En), up_left, 250),
            Outcome::Event(RadialEvent::Close)
        );
        let mut m = menu(Lang::En);
        let mut g = Gesture::new(ORIGIN, ms(1000));
        g.moved(Point::new(ORIGIN.x - 80.0, ORIGIN.y - 80.0), ms(1000));
        m.drag(&g, None, ms(1000));
        assert!(m.is_animating(ms(1150)), "the ring fills");
        assert!(!m.is_animating(ms(1300)), "then holds still");
        assert_eq!(m.let_go(&g, None, ms(1300)), act(Act::Close));
    }

    #[test]
    fn the_speed_is_that_of_the_last_moments() {
        let mut g = Gesture::new(ORIGIN, ms(0));
        g.moved(Point::new(ORIGIN.x, ORIGIN.y - 60.0), ms(500));
        g.moved(Point::new(ORIGIN.x, ORIGIN.y - 120.0), ms(560));
        assert!(
            (g.speed(ms(600)) - 600.0).abs() < 1.0,
            "{}",
            g.speed(ms(600))
        );
        assert!(g.flicked(ms(600)));
        // Held still a moment before letting go: no flick.
        assert_eq!(g.speed(ms(800)), 0.0);
        assert!(!g.flicked(ms(800)));
    }

    #[test]
    fn a_click_leaves_it_to_the_pointer_and_the_keys() {
        let mut m = menu(Lang::En);
        // Down lights Minimize, Right from there Desktop… (bottom right),
        // Enter opens the desktops, Escape comes back.
        m.key(Key::Down, ms(1000));
        assert_eq!(m.lit, Some(4));
        m.key(Key::Right, ms(1000));
        assert_eq!(m.lit, Some(3));
        assert_eq!(m.key(Key::Enter, ms(1000)), None);
        assert_eq!(m.labels(), ["Desktop 1", "Mail", "Desktop 3", "Back"]);
        assert_eq!(m.key(Key::Escape, ms(1000)), None);
        assert_eq!(m.labels().len(), 8);
        assert_eq!(m.key(Key::Escape, ms(1000)), Some(RadialEvent::Close));
        // Up, Enter: maximized; a click on Close closes, a press elsewhere
        // closes the menu.
        let mut m = menu(Lang::En);
        m.key(Key::Up, ms(1000));
        assert_eq!(
            m.key(Key::Enter, ms(1000)),
            Some(RadialEvent::Act(Act::ToggleMaximized))
        );
        let mut m = menu(Lang::En);
        let close = Point::new(MIDDLE.x - 117.0, MIDDLE.y - 117.0);
        assert_eq!(m.pointer(close, Some(true), ms(1000)), None);
        assert_eq!(
            m.pointer(close, Some(false), ms(1000)),
            Some(RadialEvent::Act(Act::Close))
        );
        assert_eq!(
            m.pointer(Point::new(4.0, 470.0), Some(true), ms(1000)),
            Some(RadialEvent::Close)
        );
    }

    #[test]
    fn snap_offers_the_zones_where_they_are() {
        let mut m = menu(Lang::En);
        assert_eq!(gesture(&mut m, (-90.0, 0.0), 400), Outcome::Open);
        m.key(Key::Left, ms(1000));
        assert_eq!(
            m.key(Key::Enter, ms(1000)),
            Some(RadialEvent::Act(Act::Snap(Zone::Left)))
        );
    }

    #[test]
    fn one_screen_and_one_desktop_leave_their_actions_out() {
        let theme = Theme::default().with_reduced_motion(true);
        let alone = Subject {
            desktops: vec!["Desktop 1".into()],
            ..Subject::default()
        };
        let mut m = Radial::new(theme, Lang::En, alone, 1.0, ms(0));
        assert!(!m.labels().iter().any(|l| l == "Next screen"));
        assert_eq!(
            gesture(&mut m, (90.0, 0.0), 50),
            Outcome::Event(RadialEvent::Close)
        );
    }

    #[test]
    fn it_stays_on_the_screen_near_its_edges() {
        let screen = Size::new(1280.0, 800.0);
        let corner = place(Point::new(1270.0, 790.0), screen);
        assert_eq!(corner.x + SIZE / 2.0 + REACH, 1280.0);
        assert_eq!(corner.y + SIZE / 2.0 + REACH, 800.0);
        let corner = place(Point::new(600.0, 400.0), screen);
        assert_eq!(
            (corner.x, corner.y),
            (600.0 - SIZE / 2.0, 400.0 - SIZE / 2.0)
        );
    }
}
