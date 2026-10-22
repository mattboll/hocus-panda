//! The widget column (decision 0007): on the right of the primary screen,
//! from top to bottom the status row, the clock, the next meeting, the
//! active window with its controls and the line of this screen's desktops,
//! and the account. The desktops line and the account open popovers to the
//! column's left.
//!
//! The compositor gives the column a [`ColumnState`] when something it shows
//! changes, routes the pointer to it when over it, and does the
//! [`Action`]s that come back.

use std::time::Duration;

use hocus_ui::{
    AccessNode, Align, Button, ButtonKind, Canvas, Direction, Draw, Event, Icon, Input, Insets,
    Justify, Kind, Length, Look, NodeId, Paint, PixelRect, Point, Rect, Role, Size, Style, Surface,
    TextStyle, Theme, Tree,
};

use crate::{i18n::Lang, icons};

/// The column's width, from decision 0007.
pub const WIDTH: f32 = 320.0;
/// Between the column and the screen's edges.
pub const MARGIN: f32 = 16.0;
/// Between a popover and the column.
const POPOVER_GAP: f32 = 12.0;
/// Room around a popover's card for its shadow.
const SHADOW_ROOM: f32 = 24.0;
const ICON_STROKE: f32 = 2.1;

/// The time on the wall, as the user reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WallTime {
    pub hour: u8,
    pub minute: u8,
    /// 0 for Monday.
    pub weekday: u8,
    pub day: u8,
    /// From 1.
    pub month: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveWindow {
    pub title: String,
    pub maximized: bool,
}

/// One screen's desktops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenDesktops {
    pub output: String,
    pub count: usize,
    /// From 1.
    pub shown: usize,
}

/// What the column shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ColumnState {
    pub time: WallTime,
    pub active: Option<ActiveWindow>,
    /// Every screen, the column's first.
    pub screens: Vec<ScreenDesktops>,
    /// The keyboard layout, short (`FR`) and long (`fr`).
    pub layout: String,
    pub layout_name: String,
    pub user: String,
    /// The account menu offers hibernating only where it works.
    pub can_hibernate: bool,
    /// The latest notifications, newest first: application and summary.
    pub notifications: Vec<(String, String)>,
    pub status: Status,
}

/// What the status row tells, once known.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Status {
    /// 0 saver, 1 balanced, 2 performance.
    pub profile: Option<usize>,
    /// On, and the network joined.
    pub network: Option<(bool, Option<String>)>,
    /// Volume, and muted.
    pub sound: Option<(u8, bool)>,
    /// Percent.
    pub battery: Option<u8>,
}

/// What the user asked for in the column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Minimize,
    ToggleMaximize,
    Close,
    SwitchLayout,
    /// Shows desktop `desktop` (from 1) of `output`.
    ShowDesktop {
        output: String,
        desktop: usize,
    },
    Lock,
    Settings,
    Suspend,
    Hibernate,
    LogOut,
    Restart,
    ShutDown,
    /// The command field was clicked: the launcher opens.
    OpenLauncher,
}

/// The ring of the meeting card: how close the next meeting is.
struct Ring {
    progress: f32,
}

impl Draw for Ring {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, _now: Duration) {
        let center = Point::new(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        let r = rect.w / 2.0 - 3.0;
        let tau = std::f32::consts::TAU;
        canvas.arc(
            center,
            r,
            0.0,
            tau,
            5.0,
            theme.colors.border.over(theme.colors.surface),
        );
        if self.progress > 0.0 {
            canvas.arc(
                center,
                r,
                -tau / 4.0,
                tau * self.progress,
                5.0,
                theme.colors.accent,
            );
        }
    }

    fn size(&self) -> Size {
        Size::new(44.0, 44.0)
    }
}

/// A screen's desktops as dots, the shown one in the accent.
struct Dots {
    count: usize,
    shown: usize,
}

impl Draw for Dots {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, _now: Duration) {
        let size = 8.0;
        let y = rect.y + (rect.h - size) / 2.0;
        for i in 0..self.count {
            let color = if i + 1 == self.shown {
                theme.colors.accent
            } else {
                theme.colors.muted.with_alpha(0x66)
            };
            let x = rect.x + i as f32 * (size + 5.0);
            canvas.fill_rect(Rect::new(x, y, size, size), size / 2.0, color);
        }
    }

    fn size(&self) -> Size {
        let n = self.count.max(1) as f32;
        Size::new(n * 8.0 + (n - 1.0) * 5.0, 16.0)
    }
}

fn stroked(path: &str, size: f32) -> Icon {
    // The icons are constants, tested to parse.
    Icon::stroked(path, size, ICON_STROKE).unwrap_or_else(|| unreachable!("icon {path}"))
}

/// The nodes the state changes.
#[derive(Debug, Clone, Copy)]
struct Nodes {
    layout: NodeId,
    clock: NodeId,
    date: NodeId,
    title: NodeId,
    minimize: NodeId,
    maximize: NodeId,
    close: NodeId,
    desktops: NodeId,
    desktops_text: NodeId,
    dots: NodeId,
    command: NodeId,
    history: NodeId,
    account: NodeId,
    initial: NodeId,
    user: NodeId,
    /// Power profile, network, sound, battery.
    status: [NodeId; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PopoverKind {
    Desktops,
    Account,
}

/// A popover: its own tree and pixels, placed by the column.
struct Popover {
    kind: PopoverKind,
    tree: Tree,
    surface: Surface,
    /// Its corner, relative to the column's.
    at: Point,
    targets: Vec<(NodeId, Action)>,
}

impl std::fmt::Debug for Popover {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Popover")
            .field("kind", &self.kind)
            .field("at", &self.at)
            .finish_non_exhaustive()
    }
}

/// Pixels to show, and where, relative to the column's corner.
#[derive(Debug)]
pub struct View<'a> {
    pub pixels: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub at: Point,
}

/// What changed in a frame, in each surface's physical pixels.
#[derive(Debug, Default)]
pub struct Damage {
    pub column: Vec<PixelRect>,
    pub popover: Vec<PixelRect>,
    /// A popover opened or closed: its old place needs drawing.
    pub popover_changed: bool,
}

#[derive(Debug)]
pub struct Column {
    lang: Lang,
    tree: Tree,
    surface: Surface,
    nodes: Nodes,
    state: ColumnState,
    popover: Option<Popover>,
    popover_changed: bool,
    now: Duration,
}

impl Column {
    /// A column `height` logical pixels tall, drawn at `scale`.
    pub fn new(theme: Theme, lang: Lang, height: f32, scale: f32) -> Self {
        let size = Size::new(WIDTH, height);
        let mut tree = Tree::new(theme.clone(), size, scale);
        let nodes = build(&mut tree, &theme, lang);
        let mut column = Self {
            lang,
            tree,
            surface: Surface::new(size, scale),
            nodes,
            state: ColumnState::default(),
            popover: None,
            popover_changed: false,
            now: Duration::ZERO,
        };
        column.apply(&ColumnState::default(), true);
        column
    }

    pub fn size(&self) -> Size {
        self.tree.size()
    }

    pub fn resize(&mut self, height: f32, scale: f32) {
        let size = Size::new(WIDTH, height);
        if size != self.tree.size() || scale != self.tree.scale() {
            self.tree.resize(size, scale);
            self.surface = Surface::new(size, scale);
            self.close_popover();
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.tree.set_theme(theme.clone());
        if let Some(p) = &mut self.popover {
            p.tree.set_theme(theme);
        }
    }

    /// Shows `state`; only what differs is drawn again.
    pub fn set_state(&mut self, state: &ColumnState) {
        if *state != self.state {
            self.apply(state, false);
        }
    }

    fn apply(&mut self, state: &ColumnState, all: bool) {
        let lang = self.lang;
        let n = self.nodes;
        let old = std::mem::replace(&mut self.state, state.clone());
        let t = &mut self.tree;
        if all || old.status != state.status {
            let s = &state.status;
            let [profile, network, sound, battery] = n.status;
            let profile_name = s
                .profile
                .map_or_else(|| lang.power_profile().to_owned(), |p| lang.profile_name(p));
            t.set_name(profile, &profile_name);
            let (network_name, on) = match &s.network {
                Some((wifi, name)) => (lang.network_name(*wifi, name.as_deref()), *wifi),
                None => (lang.network().to_owned(), true),
            };
            t.set_name(network, &network_name);
            t.set_icon_paint(network, if on { Paint::Text } else { Paint::Muted });
            let (sound_name, audible) = match s.sound {
                Some((volume, muted)) => (lang.sound_level(volume, muted), !muted),
                None => (lang.sound().to_owned(), true),
            };
            t.set_name(sound, &sound_name);
            t.set_icon_paint(sound, if audible { Paint::Text } else { Paint::Muted });
            let battery_name = s
                .battery
                .map_or_else(|| lang.battery().to_owned(), |p| lang.battery_level(p));
            t.set_name(battery, &battery_name);
            // Low: the waiting colour.
            let low = s.battery.is_some_and(|p| p <= 15);
            t.set_icon_paint(battery, if low { Paint::Waiting } else { Paint::Text });
        }
        if all || old.time != state.time {
            t.set_text(
                n.clock,
                &format!("{:02}:{:02}", state.time.hour, state.time.minute),
            );
            let w = state.time;
            t.set_text(n.date, &lang.date(w.weekday, w.day.max(1), w.month.max(1)));
        }
        if all || old.active != state.active {
            let theme = t.theme().clone();
            match &state.active {
                Some(active) => {
                    t.set_text(n.title, &active.title);
                    t.set_text_style(n.title, theme.text.title.with_weight(600));
                    let (icon, name) = if active.maximized {
                        (icons::RESTORE, lang.restore())
                    } else {
                        (icons::MAXIMIZE, lang.maximize())
                    };
                    t.set_icon(n.maximize, stroked(icon, 16.0));
                    t.set_name(n.maximize, name);
                }
                None => {
                    t.set_text(n.title, lang.no_window());
                    t.set_text_style(n.title, theme.text.title.with_color(Paint::Muted));
                }
            }
            for b in [n.minimize, n.maximize, n.close] {
                t.set_enabled(b, state.active.is_some());
            }
        }
        if all || old.screens.first() != state.screens.first() {
            let (shown, count) = state.screens.first().map_or((1, 1), |s| (s.shown, s.count));
            let line = lang.desktop_of(shown, count);
            t.set_text(n.desktops_text, &line);
            t.set_name(n.desktops, &line);
            t.set_draw(n.dots, Box::new(Dots { count, shown }));
        }
        if all || old.layout != state.layout || old.layout_name != state.layout_name {
            t.set_text(n.layout, &state.layout);
            t.set_name(n.layout, &lang.keyboard_layout(&state.layout_name));
        }
        if all || old.notifications != state.notifications {
            let rows = t.children(n.history);
            for row in rows {
                t.remove(row);
            }
            let theme = t.theme().clone();
            if state.notifications.is_empty() {
                let _ = t.add(
                    n.history,
                    Style::default(),
                    Kind::text(lang.nothing_new(), theme.text.body.with_color(Paint::Muted)),
                );
            }
            for (app, summary) in state.notifications.iter().take(3) {
                let Some(row) = t.add(
                    n.history,
                    Style {
                        direction: Direction::Row,
                        gap: 10.0,
                        align: Align::Center,
                        ..Style::default()
                    },
                    Kind::Box(Look::default()),
                ) else {
                    continue;
                };
                let _ = t.add(
                    row,
                    Style {
                        width: Length::Px(22.0),
                        height: Length::Px(22.0),
                        ..Style::default()
                    },
                    Kind::Box(Look {
                        background: Some(Paint::Rgba(crate::toasts::app_color(app))),
                        radius: 6.0,
                        ..Look::default()
                    }),
                );
                let _ = t.add(
                    row,
                    Style {
                        width: Length::Fill,
                        ..Style::default()
                    },
                    Kind::text_line(format!("{app} · {summary}"), theme.text.body),
                );
            }
        }
        if all || old.user != state.user {
            let initial: String = state
                .user
                .chars()
                .take(1)
                .flat_map(char::to_uppercase)
                .collect();
            t.set_text(n.initial, &initial);
            t.set_text(n.user, &state.user);
            t.set_name(n.account, &lang.account(&state.user));
        }
        // An open desktops popover follows the screens.
        if !all
            && old.screens != state.screens
            && self
                .popover
                .as_ref()
                .is_some_and(|p| p.kind == PopoverKind::Desktops)
        {
            self.open(PopoverKind::Desktops);
        }
    }

    // Popovers.

    fn open(&mut self, kind: PopoverKind) {
        let theme = self.tree.theme().clone();
        let scale = self.tree.scale();
        let anchor = match kind {
            PopoverKind::Desktops => self.nodes.desktops,
            PopoverKind::Account => self.nodes.account,
        };
        let anchor = self.tree.rect(anchor).unwrap_or_default();
        let (mut tree, targets) = match kind {
            PopoverKind::Desktops => {
                desktops_popover(&theme, self.lang, &self.state.screens, scale)
            }
            PopoverKind::Account => account_popover(
                &theme,
                self.lang,
                &self.state.user,
                self.state.can_hibernate,
                scale,
            ),
        };
        let first = tree.update(self.now);
        let size = tree.size();
        // Left of the column; level with the line, or above the account.
        let y = match kind {
            PopoverKind::Desktops => anchor.y - SHADOW_ROOM,
            PopoverKind::Account => anchor.bottom() + SHADOW_ROOM - size.h,
        };
        let y = y.clamp(-MARGIN, (self.tree.size().h + MARGIN - size.h).max(-MARGIN));
        let at = Point::new(-(size.w - SHADOW_ROOM) - POPOVER_GAP, y);
        let mut surface = Surface::new(size, scale);
        surface.draw(&mut tree, &first);
        self.popover = Some(Popover {
            kind,
            tree,
            surface,
            at,
            targets,
        });
        self.popover_changed = true;
    }

    pub fn close_popover(&mut self) {
        if self.popover.take().is_some() {
            self.popover_changed = true;
        }
    }

    fn toggle(&mut self, kind: PopoverKind) {
        if self.popover.as_ref().is_some_and(|p| p.kind == kind) {
            self.close_popover();
        } else {
            self.open(kind);
        }
    }

    // Input.

    fn in_column(&self, at: Point) -> bool {
        let s = self.tree.size();
        Rect::new(0.0, 0.0, s.w, s.h).contains(at)
    }

    fn in_popover(&self, at: Point) -> bool {
        self.popover.as_ref().is_some_and(|p| {
            let s = p.tree.size();
            // The card, not the room for its shadow.
            Rect::new(p.at.x, p.at.y, s.w, s.h)
                .inset(Insets::all(SHADOW_ROOM))
                .contains(at)
        })
    }

    /// Whether the pointer at `at` (relative to the column's corner) is over
    /// the column or its popover.
    pub fn hit(&self, at: Point) -> bool {
        self.in_column(at) || self.in_popover(at)
    }

    /// The pointer moved away from the column and its popover.
    pub fn pointer_left(&mut self) {
        self.tree.input(Input::PointerLeave);
        if let Some(p) = &mut self.popover {
            p.tree.input(Input::PointerLeave);
        }
    }

    /// The primary button pressed outside the column and its popover.
    pub fn clicked_elsewhere(&mut self) {
        self.close_popover();
    }

    /// Pointer input at `at`, relative to the column's corner.
    pub fn input(&mut self, at: Point, input: Input) -> Vec<Action> {
        let mut actions = Vec::new();
        let in_popover = self.in_popover(at);
        if let Some(p) = &mut self.popover {
            let local = Point::new(at.x - p.at.x, at.y - p.at.y);
            let forwarded = match input {
                Input::PointerMove(_) if in_popover => Input::PointerMove(local),
                Input::PointerMove(_) => Input::PointerLeave,
                other => other,
            };
            if in_popover || matches!(forwarded, Input::PointerLeave) {
                for event in p.tree.input(forwarded) {
                    let Event::Activated(id) = event else {
                        continue;
                    };
                    if let Some((_, action)) = p.targets.iter().find(|(t, _)| *t == id) {
                        actions.push(action.clone());
                    }
                }
            }
            if in_popover {
                if !actions.is_empty() {
                    self.close_popover();
                }
                return actions;
            }
            if matches!(input, Input::PointerButton { pressed: true }) {
                self.close_popover();
            }
        }
        let forwarded = match input {
            Input::PointerMove(_) if !self.in_column(at) => Input::PointerLeave,
            Input::PointerMove(_) => Input::PointerMove(at),
            other => other,
        };
        let n = self.nodes;
        for event in self.tree.input(forwarded) {
            let Event::Activated(id) = event else {
                continue;
            };
            if id == n.minimize {
                actions.push(Action::Minimize);
            } else if id == n.maximize {
                actions.push(Action::ToggleMaximize);
            } else if id == n.close {
                actions.push(Action::Close);
            } else if id == n.layout {
                actions.push(Action::SwitchLayout);
            } else if id == n.command {
                actions.push(Action::OpenLauncher);
            } else if id == n.desktops {
                self.toggle(PopoverKind::Desktops);
            } else if id == n.account {
                self.toggle(PopoverKind::Account);
            }
        }
        actions
    }

    // Drawing.

    /// Draws what changed at `now`.
    pub fn draw(&mut self, now: Duration) -> Damage {
        self.now = now;
        let damage = self.tree.update(now);
        let column = self.surface.draw(&mut self.tree, &damage);
        let popover = match &mut self.popover {
            Some(p) => {
                let damage = p.tree.update(now);
                p.surface.draw(&mut p.tree, &damage)
            }
            None => Vec::new(),
        };
        Damage {
            column,
            popover,
            popover_changed: std::mem::take(&mut self.popover_changed),
        }
    }

    pub fn is_animating(&self, now: Duration) -> bool {
        self.tree.is_animating(now)
            || self
                .popover
                .as_ref()
                .is_some_and(|p| p.tree.is_animating(now))
    }

    pub fn view(&self) -> View<'_> {
        View {
            pixels: self.surface.pixels(),
            width: self.surface.width(),
            height: self.surface.height(),
            at: Point::default(),
        }
    }

    pub fn popover_view(&self) -> Option<View<'_>> {
        self.popover.as_ref().map(|p| View {
            pixels: p.surface.pixels(),
            width: p.surface.width(),
            height: p.surface.height(),
            at: p.at,
        })
    }

    /// The column and its popover for screen readers, rectangles relative to
    /// the column's corner.
    pub fn access(&self) -> AccessNode {
        let mut children = vec![self.tree.access()];
        if let Some(p) = &self.popover {
            let mut popover = p.tree.access();
            translate(&mut popover, p.at);
            children.push(popover);
        }
        AccessNode {
            role: Role::Group,
            label: None,
            rect: Rect::new(0.0, 0.0, self.tree.size().w, self.tree.size().h),
            focused: false,
            disabled: false,
            selected: false,
            children,
        }
    }
}

fn translate(node: &mut AccessNode, by: Point) {
    node.rect = node.rect.translate(by.x, by.y);
    for child in &mut node.children {
        translate(child, by);
    }
}

fn caption(theme: &Theme) -> TextStyle {
    theme.text.caption
}

/// The column's nodes, empty: [`Column::apply`] fills them.
fn build(tree: &mut Tree, theme: &Theme, lang: Lang) -> Nodes {
    let root = tree.root();
    let missing = || unreachable!("adding to a node of this tree");
    tree.set_look(root, Look::default());
    let card = tree
        .add(
            root,
            Style {
                width: Length::Fill,
                height: Length::Fill,
                padding: Insets::symmetric(16.0, 18.0),
                gap: 16.0,
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::Surface),
                radius: 24.0,
                border: Some(Paint::Border),
                shadow: 0.0,
            }),
        )
        .unwrap_or_else(missing);
    let row = |gap: f32| Style {
        direction: Direction::Row,
        gap,
        align: Align::Center,
        ..Style::default()
    };
    let add = |tree: &mut Tree, parent, style, kind| {
        tree.add(parent, style, kind).unwrap_or_else(missing)
    };

    // Status row: the keyboard layout works; the others are placeholders
    // until their modules come (sound, network, power, battery).
    let status = add(
        tree,
        card,
        Style {
            justify: Justify::SpaceBetween,
            ..row(6.0)
        },
        Kind::Box(Look::default()),
    );
    let chip = Style {
        height: Length::Px(44.0),
        ..Style::default()
    };
    let layout = add(
        tree,
        status,
        Style {
            width: Length::Px(64.0),
            ..chip
        },
        Kind::Button(
            Button::new("", ButtonKind::Chip, theme)
                .with_icon(stroked(icons::KEYBOARD, 18.0))
                .with_style(theme.text.label.with_weight(700)),
        ),
    );
    let mut status_icons = Vec::new();
    for (path, name) in [
        (icons::LEAF, lang.power_profile()),
        (icons::WIFI, lang.network()),
        (icons::SOUND, lang.sound()),
        (icons::BATTERY, lang.battery()),
    ] {
        let holder = add(
            tree,
            status,
            Style {
                width: Length::Px(44.0),
                height: Length::Px(44.0),
                align: Align::Center,
                justify: Justify::Center,
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                radius: theme.radii.medium,
                ..Look::default()
            }),
        );
        status_icons.push(add(
            tree,
            holder,
            Style::default(),
            Kind::Icon {
                icon: stroked(path, 18.0),
                color: Paint::Text,
                label: Some(name.to_owned()),
            },
        ));
    }
    let status_icons: [NodeId; 4] = status_icons
        .try_into()
        .unwrap_or_else(|_| unreachable!("four status icons"));

    // Clock and date.
    let time = add(
        tree,
        card,
        Style {
            gap: 2.0,
            ..Style::default()
        },
        Kind::Box(Look::default()),
    );
    let clock = add(
        tree,
        time,
        Style::default(),
        Kind::text("", theme.text.display.with_weight(600)),
    );
    let date = add(
        tree,
        time,
        Style::default(),
        Kind::text("", theme.text.body.with_color(Paint::Muted)),
    );

    // Next meeting: a placeholder until the calendar (#75).
    let meeting = add(
        tree,
        card,
        Style {
            padding: Insets::all(14.0),
            ..row(14.0)
        },
        Kind::Box(Look {
            background: Some(Paint::SurfaceHover),
            radius: theme.radii.large,
            ..Look::default()
        }),
    );
    add(
        tree,
        meeting,
        Style::default(),
        Kind::Custom {
            draw: Box::new(Ring { progress: 0.0 }),
            label: None,
        },
    );
    let meeting_text = add(
        tree,
        meeting,
        Style {
            width: Length::Fill,
            gap: 2.0,
            ..Style::default()
        },
        Kind::Box(Look::default()),
    );
    add(
        tree,
        meeting_text,
        Style::default(),
        Kind::text(lang.no_meeting(), theme.text.title.with_weight(700)),
    );
    add(
        tree,
        meeting_text,
        Style::default(),
        Kind::text(
            lang.calendar_not_set(),
            theme.text.body.with_color(Paint::Muted),
        ),
    );

    // The active window and this screen's desktops.
    let active = add(
        tree,
        card,
        Style {
            gap: 10.0,
            ..Style::default()
        },
        Kind::Box(Look::default()),
    );
    add(
        tree,
        active,
        Style::default(),
        Kind::text(lang.active_window(), caption(theme)),
    );
    let controls = add(tree, active, row(8.0), Kind::Box(Look::default()));
    let title = add(
        tree,
        controls,
        Style {
            width: Length::Fill,
            ..Style::default()
        },
        Kind::text_line("", theme.text.title.with_weight(600)),
    );
    let square = Style {
        width: Length::Px(44.0),
        height: Length::Px(44.0),
        ..Style::default()
    };
    let control = |tree: &mut Tree, path: &str, name: &str| {
        add(
            tree,
            controls,
            square,
            Kind::icon_button(stroked(path, 16.0), name, ButtonKind::Chip, theme),
        )
    };
    let minimize = control(tree, icons::MINIMIZE, lang.minimize());
    let maximize = control(tree, icons::MAXIMIZE, lang.maximize());
    let close = control(tree, icons::CLOSE, lang.close());
    let desktops = add(
        tree,
        active,
        Style {
            justify: Justify::SpaceBetween,
            padding: Insets::symmetric(6.0, 8.0),
            ..row(8.0)
        },
        Kind::Box(Look {
            radius: theme.radii.small,
            ..Look::default()
        }),
    );
    tree.set_clickable(desktops, "");
    let desktops_text = add(
        tree,
        desktops,
        Style::default(),
        Kind::text(
            "",
            theme.text.label.with_weight(500).with_color(Paint::Muted),
        ),
    );
    let dots = add(
        tree,
        desktops,
        Style::default(),
        Kind::Custom {
            draw: Box::new(Dots { count: 1, shown: 1 }),
            label: None,
        },
    );

    // The command field: it opens the launcher.
    let command = add(
        tree,
        card,
        Style {
            height: Length::Px(52.0),
            padding: Insets::symmetric(0.0, 14.0),
            ..row(10.0)
        },
        Kind::Box(Look {
            background: Some(Paint::SurfaceHover),
            radius: 14.0,
            border: Some(Paint::Border),
            shadow: 0.0,
        }),
    );
    tree.set_clickable(command, lang.command_field());
    add(
        tree,
        command,
        Style::default(),
        Kind::Icon {
            icon: stroked(icons::SEARCH, 18.0),
            color: Paint::Muted,
            label: None,
        },
    );
    add(
        tree,
        command,
        Style {
            width: Length::Fill,
            ..Style::default()
        },
        Kind::text_line(
            lang.command_placeholder(),
            theme.text.title.with_weight(500).with_color(Paint::Muted),
        ),
    );

    // The latest notifications.
    let notes = add(
        tree,
        card,
        Style {
            gap: 8.0,
            ..Style::default()
        },
        Kind::Box(Look::default()),
    );
    add(
        tree,
        notes,
        Style::default(),
        Kind::text(lang.notifications(), caption(theme)),
    );
    let history = add(
        tree,
        notes,
        Style {
            gap: 8.0,
            ..Style::default()
        },
        Kind::Box(Look::default()),
    );

    // What is left, then the account at the bottom.
    add(
        tree,
        card,
        Style {
            height: Length::Fill,
            ..Style::default()
        },
        Kind::Box(Look::default()),
    );
    let account = add(
        tree,
        card,
        Style {
            height: Length::Px(56.0),
            padding: Insets::symmetric(0.0, 10.0),
            ..row(12.0)
        },
        Kind::Box(Look {
            background: Some(Paint::SurfaceHover),
            radius: theme.radii.large,
            ..Look::default()
        }),
    );
    tree.set_clickable(account, "");
    let avatar = add(
        tree,
        account,
        Style {
            width: Length::Px(36.0),
            height: Length::Px(36.0),
            align: Align::Center,
            justify: Justify::Center,
            ..Style::default()
        },
        Kind::Box(Look {
            background: Some(Paint::Accent),
            radius: 18.0,
            ..Look::default()
        }),
    );
    let initial = add(
        tree,
        avatar,
        Style::default(),
        Kind::text(
            "",
            theme
                .text
                .title
                .with_weight(700)
                .with_color(Paint::OnAccent),
        ),
    );
    let user = add(
        tree,
        account,
        Style {
            width: Length::Fill,
            ..Style::default()
        },
        Kind::text_line("", theme.text.title),
    );
    add(
        tree,
        account,
        Style::default(),
        Kind::Icon {
            icon: stroked(icons::CHEVRON_UP, 16.0),
            color: Paint::Muted,
            label: None,
        },
    );
    Nodes {
        layout,
        clock,
        date,
        title,
        minimize,
        maximize,
        close,
        desktops,
        desktops_text,
        dots,
        command,
        history,
        account,
        initial,
        user,
        status: status_icons,
    }
}

/// A popover's tree: a card with room around it for its shadow.
fn popover_tree(theme: &Theme, width: f32, height: f32, scale: f32) -> (Tree, NodeId) {
    let size = Size::new(width + 2.0 * SHADOW_ROOM, height + 2.0 * SHADOW_ROOM);
    let mut tree = Tree::new(theme.clone(), size, scale);
    let root = tree.root();
    tree.set_look(root, Look::default());
    tree.set_style(
        root,
        Style {
            padding: Insets::all(SHADOW_ROOM),
            ..Style::default()
        },
    );
    let card = tree
        .add(
            root,
            Style {
                height: Length::Fill,
                padding: Insets::all(16.0),
                gap: 12.0,
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::Surface),
                radius: 20.0,
                border: Some(Paint::Border),
                shadow: 18.0,
            }),
        )
        .unwrap_or_else(|| unreachable!("the root takes children"));
    (tree, card)
}

fn desktops_popover(
    theme: &Theme,
    lang: Lang,
    screens: &[ScreenDesktops],
    scale: f32,
) -> (Tree, Vec<(NodeId, Action)>) {
    let height = 16.0 * 2.0 + 24.0 + screens.len() as f32 * (12.0 + 20.0 + 6.0 + 40.0);
    let (mut tree, card) = popover_tree(theme, 300.0, height, scale);
    let missing = || unreachable!("adding to a node of this tree");
    tree.add(
        card,
        Style::default(),
        Kind::text(lang.desktops_title(), theme.text.title.with_weight(700)),
    )
    .unwrap_or_else(missing);
    let mut targets = Vec::new();
    for screen in screens {
        let block = tree
            .add(
                card,
                Style {
                    gap: 6.0,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(missing);
        let head = tree
            .add(
                block,
                Style {
                    direction: Direction::Row,
                    justify: Justify::SpaceBetween,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(missing);
        tree.add(
            head,
            Style::default(),
            Kind::text(screen.output.as_str(), theme.text.label),
        )
        .unwrap_or_else(missing);
        tree.add(
            head,
            Style::default(),
            Kind::text(
                lang.desktops_count(screen.count),
                theme.text.label.with_color(Paint::Muted),
            ),
        )
        .unwrap_or_else(missing);
        let chips = tree
            .add(
                block,
                Style {
                    direction: Direction::Row,
                    gap: 6.0,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(missing);
        for d in 1..=screen.count {
            let mut button = Button::new(d.to_string(), ButtonKind::Chip, theme)
                .with_name(format!("{} · {}", screen.output, lang.desktop(d)));
            button.selected = d == screen.shown;
            let id = tree
                .add(
                    chips,
                    Style {
                        width: Length::Px(58.0),
                        height: Length::Px(40.0),
                        ..Style::default()
                    },
                    Kind::Button(button),
                )
                .unwrap_or_else(missing);
            targets.push((
                id,
                Action::ShowDesktop {
                    output: screen.output.clone(),
                    desktop: d,
                },
            ));
        }
    }
    (tree, targets)
}

fn account_popover(
    theme: &Theme,
    lang: Lang,
    user: &str,
    can_hibernate: bool,
    scale: f32,
) -> (Tree, Vec<(NodeId, Action)>) {
    let mut items = vec![
        (icons::LOCK, lang.lock(), Action::Lock),
        (icons::MOON, lang.suspend(), Action::Suspend),
    ];
    if can_hibernate {
        items.push((icons::MOON, lang.hibernate(), Action::Hibernate));
    }
    items.extend([
        (icons::SETTINGS, lang.settings(), Action::Settings),
        (icons::LOG_OUT, lang.log_out(), Action::LogOut),
        (icons::RESTART, lang.restart(), Action::Restart),
        (icons::POWER, lang.shut_down(), Action::ShutDown),
    ]);
    let height = 16.0 * 2.0 + 24.0 + 12.0 + items.len() as f32 * 44.0 + 12.0;
    let (mut tree, card) = popover_tree(theme, 260.0, height, scale);
    let missing = || unreachable!("adding to a node of this tree");
    tree.set_style(
        card,
        Style {
            height: Length::Fill,
            padding: Insets::all(16.0),
            gap: 2.0,
            ..Style::default()
        },
    );
    tree.add(
        card,
        Style {
            padding: Insets {
                bottom: 12.0,
                left: 12.0,
                ..Insets::default()
            },
            ..Style::default()
        },
        Kind::text(user, theme.text.title.with_weight(700)),
    )
    .unwrap_or_else(missing);
    let mut targets = Vec::new();
    for (path, label, action) in items {
        // Shutting down in the error colour, as the design has it.
        let ink = if action == Action::ShutDown {
            theme.text.body.with_color(Paint::Error)
        } else {
            theme.text.body
        };
        let button = Button::new(label, ButtonKind::Ghost, theme)
            .with_icon(stroked(path, 18.0))
            .with_style(ink)
            .at_start();
        let id = tree
            .add(
                card,
                Style {
                    height: Length::Px(44.0),
                    ..Style::default()
                },
                Kind::Button(button),
            )
            .unwrap_or_else(missing);
        targets.push((id, action));
    }
    (tree, targets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_parses() {
        for path in [
            icons::MINIMIZE,
            icons::MAXIMIZE,
            icons::RESTORE,
            icons::CLOSE,
            icons::KEYBOARD,
            icons::LEAF,
            icons::WIFI,
            icons::SOUND,
            icons::BATTERY,
            icons::CHEVRON_UP,
            icons::LOCK,
            icons::SETTINGS,
            icons::LOG_OUT,
            icons::RESTART,
            icons::POWER,
            icons::MOON,
        ] {
            assert!(Icon::stroked(path, 16.0, 2.0).is_some(), "{path}");
        }
    }
}
