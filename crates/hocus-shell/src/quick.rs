//! Quick settings (Super+A): a sheet from the right edge of the screen
//! under the pointer, after the design. Tiles, the power profile, sound,
//! brightness per screen, keyboard layouts and notifications. Escape or a
//! click outside closes it.

use std::time::Duration;

use hocus_ui::{
    AccessNode, Align, Button, ButtonKind, Direction, Event, Icon, Input, Insets, Justify, Key,
    Kind, Length, Look, NodeId, Paint, PixelRect, Point, Rect, Size, Style, Surface, TextStyle,
    Theme, Tree,
};

use crate::{column::View, i18n::Lang, icons};

/// The sheet's width, from the design.
pub const WIDTH: f32 = 480.0;
/// Room for its shadow on the left.
pub const ROOM: f32 = 32.0;
/// Between the sheet and the screen's edges.
pub const MARGIN: f32 = 16.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tile {
    Wifi,
    Bluetooth,
    DoNotDisturb,
    NightLight,
    Focus,
    Record,
    Airplane,
    Pandas,
}

impl Tile {
    const ALL: [Tile; 8] = [
        Self::Wifi,
        Self::Bluetooth,
        Self::DoNotDisturb,
        Self::NightLight,
        Self::Focus,
        Self::Record,
        Self::Airplane,
        Self::Pandas,
    ];

    fn icon(self) -> &'static str {
        match self {
            Self::Wifi => icons::WIFI,
            Self::Bluetooth => "M7 7l10 10-5 5V2l5 5L7 17",
            Self::DoNotDisturb => "M20 14A8 8 0 1 1 10 4a6 6 0 0 0 10 10z",
            Self::NightLight => {
                "M12 3v2M12 19v2M5 12H3M21 12h-2M6 6l1.5 1.5M16.5 16.5L18 18M18 6l-1.5 1.5M7.5 16.5L6 18M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8z"
            }
            Self::Focus => "M9 12a3 3 0 1 0 6 0 3 3 0 1 0-6 0M2 12C5 6 19 6 22 12C19 18 5 18 2 12",
            Self::Record => "M8 12a4 4 0 1 0 8 0 4 4 0 1 0-8 0M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0",
            Self::Airplane => "M10 14L3 11l1-2 7 1 4-6h2l-2 7 5 2-1 2-5-1-2 6h-2z",
            Self::Pandas => {
                "M4 6a3 3 0 1 0 6 0 3 3 0 1 0-6 0M14 6a3 3 0 1 0 6 0 3 3 0 1 0-6 0M5 15a7 7 0 1 0 14 0 7 7 0 1 0-14 0"
            }
        }
    }
}

/// What the sheet shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickState {
    /// Each tile: on, what it says under its name, whether it works yet.
    pub tiles: Vec<(Tile, bool, String, bool)>,
    /// 0 saver, 1 balanced, 2 performance.
    pub profile: usize,
    pub volume: u8,
    pub output_device: String,
    /// Each screen: its name and brightness.
    pub brightness: Vec<(String, u8)>,
    /// `Battery 64 % · about 3 h 50`, or nothing.
    pub battery: Option<String>,
    /// Short names, and the one in use.
    pub layouts: Vec<String>,
    pub layout: usize,
    pub notifications: usize,
}

/// What the user did in the sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickAction {
    Toggle(Tile, bool),
    Profile(usize),
    Volume(u8),
    Brightness(String, u8),
    Layout(usize),
    ClearNotifications,
    Closed,
}

/// A tile's nodes: the card, its icon, name and line under it.
#[derive(Debug, Clone, Copy)]
struct TileNodes {
    tile: Tile,
    card: NodeId,
    icon: NodeId,
    name: NodeId,
    sub: NodeId,
}

#[derive(Debug, Default)]
struct Nodes {
    tiles: Vec<TileNodes>,
    profiles: Vec<NodeId>,
    volume: Option<(NodeId, NodeId)>,
    output: Option<NodeId>,
    brightness: Vec<(String, NodeId, NodeId)>,
    layouts: Vec<NodeId>,
    notifications: Option<NodeId>,
    clear: Option<NodeId>,
    battery: Option<NodeId>,
}

pub struct Sheet {
    lang: Lang,
    tree: Tree,
    surface: Surface,
    nodes: Nodes,
    state: Option<QuickState>,
    open: bool,
    now: Duration,
}

impl std::fmt::Debug for Sheet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sheet")
            .field("open", &self.open)
            .finish_non_exhaustive()
    }
}

fn stroked(path: &str, size: f32) -> Icon {
    Icon::stroked(path, size, 2.1).unwrap_or_else(|| unreachable!("icon {path}"))
}

impl Sheet {
    /// A sheet for a screen `height` logical pixels tall.
    pub fn new(theme: Theme, lang: Lang, height: f32, scale: f32) -> Self {
        let size = Size::new(WIDTH + ROOM, height);
        Self {
            lang,
            tree: Tree::new(theme, size, scale),
            surface: Surface::new(size, scale),
            nodes: Nodes::default(),
            state: None,
            open: false,
            now: Duration::ZERO,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn resize(&mut self, height: f32, scale: f32) {
        let size = Size::new(WIDTH + ROOM, height);
        if size != self.tree.size() || scale != self.tree.scale() {
            self.tree.resize(size, scale);
            self.surface = Surface::new(size, scale);
        }
    }

    pub fn open(&mut self) {
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// Shows `state`: the sheet is built again when its shape changes
    /// (screens, layouts), its values follow otherwise.
    pub fn set_state(&mut self, state: &QuickState) {
        let shape = |s: &QuickState| {
            (
                s.tiles.iter().map(|t| (t.0, t.3)).collect::<Vec<_>>(),
                s.brightness.iter().map(|b| b.0.clone()).collect::<Vec<_>>(),
                s.layouts.clone(),
                s.battery.is_some(),
            )
        };
        if self.state.as_ref().map(shape) != Some(shape(state)) {
            self.build(state);
        }
        self.apply(state);
        self.state = Some(state.clone());
    }

    fn build(&mut self, state: &QuickState) {
        let theme = self.tree.theme().clone();
        let lang = self.lang;
        let t = &mut self.tree;
        let root = t.root();
        for child in t.children(root) {
            t.remove(child);
        }
        self.nodes = Nodes::default();
        let n = &mut self.nodes;
        let missing = || unreachable!("adding to a node of this tree");
        t.set_look(root, Look::default());
        t.set_style(
            root,
            Style {
                padding: Insets {
                    left: ROOM,
                    ..Insets::default()
                },
                ..Style::default()
            },
        );
        let sheet = t
            .add(
                root,
                Style {
                    width: Length::Fill,
                    height: Length::Fill,
                    padding: Insets::all(22.0),
                    gap: 18.0,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Surface),
                    radius: 26.0,
                    border: Some(Paint::Border),
                    shadow: 30.0,
                }),
            )
            .unwrap_or_else(missing);
        let caption = theme.text.caption;
        let row = |gap: f32| Style {
            direction: Direction::Row,
            gap,
            align: Align::Center,
            ..Style::default()
        };

        // Title, battery, settings.
        let head = t
            .add(
                sheet,
                Style {
                    justify: Justify::SpaceBetween,
                    ..row(8.0)
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(missing);
        let titles = t
            .add(head, Style::default(), Kind::Box(Look::default()))
            .unwrap_or_else(missing);
        t.add(
            titles,
            Style::default(),
            Kind::text(lang.quick_settings(), theme.text.headline.with_weight(700)),
        )
        .unwrap_or_else(missing);
        if state.battery.is_some() {
            n.battery = t.add(
                titles,
                Style::default(),
                Kind::text(
                    "",
                    theme.text.label.with_weight(500).with_color(Paint::Muted),
                ),
            );
        }
        let mut settings = Button::new("", ButtonKind::Plain, &theme)
            .with_icon(stroked(icons::SETTINGS, 18.0))
            .with_name(lang.settings());
        // The settings application comes with #66.
        settings.enabled = false;
        t.add(
            head,
            Style {
                width: Length::Px(44.0),
                height: Length::Px(44.0),
                ..Style::default()
            },
            Kind::Button(settings),
        )
        .unwrap_or_else(missing);

        // Tiles, two by two.
        let grid = t
            .add(
                sheet,
                Style {
                    gap: 10.0,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(missing);
        let mut line = None;
        for (i, (tile, _, _, works)) in state.tiles.iter().enumerate() {
            if i % 2 == 0 {
                line = t.add(grid, row(10.0), Kind::Box(Look::default()));
            }
            let Some(line) = line else { continue };
            let card = t
                .add(
                    line,
                    Style {
                        width: Length::Fill,
                        height: Length::Px(64.0),
                        padding: Insets::symmetric(0.0, 14.0),
                        ..row(12.0)
                    },
                    Kind::Box(Look {
                        background: Some(Paint::SurfaceHover),
                        radius: 16.0,
                        ..Look::default()
                    }),
                )
                .unwrap_or_else(missing);
            if *works {
                t.set_clickable(card, lang.tile(*tile));
            }
            let color = if *works { Paint::Text } else { Paint::Muted };
            let icon = t
                .add(
                    card,
                    Style::default(),
                    Kind::Icon {
                        icon: stroked(tile.icon(), 22.0),
                        color,
                        label: None,
                    },
                )
                .unwrap_or_else(missing);
            let words = t
                .add(
                    card,
                    Style {
                        width: Length::Fill,
                        ..Style::default()
                    },
                    Kind::Box(Look::default()),
                )
                .unwrap_or_else(missing);
            let name = t
                .add(
                    words,
                    Style::default(),
                    Kind::text_line(
                        lang.tile(*tile),
                        theme.text.title.with_weight(700).with_color(color),
                    ),
                )
                .unwrap_or_else(missing);
            let sub = t
                .add(
                    words,
                    Style::default(),
                    Kind::text_line(
                        "",
                        theme.text.caption.with_weight(500).with_color(Paint::Muted),
                    ),
                )
                .unwrap_or_else(missing);
            n.tiles.push(TileNodes {
                tile: *tile,
                card,
                icon,
                name,
                sub,
            });
        }

        // Power profile.
        let energy = t
            .add(
                sheet,
                Style {
                    gap: 8.0,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(missing);
        t.add(energy, Style::default(), Kind::text(lang.energy(), caption))
            .unwrap_or_else(missing);
        let seg = t
            .add(
                energy,
                Style {
                    padding: Insets::all(4.0),
                    ..row(4.0)
                },
                Kind::Box(Look {
                    background: Some(Paint::SurfaceHover),
                    radius: 14.0,
                    ..Look::default()
                }),
            )
            .unwrap_or_else(missing);
        for label in lang.profiles() {
            let id = t
                .add(
                    seg,
                    Style {
                        width: Length::Fill,
                        height: Length::Px(44.0),
                        ..Style::default()
                    },
                    Kind::Button(
                        Button::new(label, ButtonKind::Ghost, &theme)
                            .with_style(theme.text.label.with_weight(700)),
                    ),
                )
                .unwrap_or_else(missing);
            n.profiles.push(id);
        }

        // Sound.
        let sound = t
            .add(
                sheet,
                Style {
                    gap: 12.0,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(missing);
        t.add(
            sound,
            Style::default(),
            Kind::text(lang.sound_caption(), caption),
        )
        .unwrap_or_else(missing);
        let volume_row = t
            .add(sound, row(12.0), Kind::Box(Look::default()))
            .unwrap_or_else(missing);
        t.add(
            volume_row,
            Style::default(),
            Kind::Icon {
                icon: stroked(icons::SOUND, 20.0),
                color: Paint::Text,
                label: None,
            },
        )
        .unwrap_or_else(missing);
        let volume = t
            .add(
                volume_row,
                Style {
                    width: Length::Fill,
                    ..Style::default()
                },
                Kind::Slider {
                    value: 0.0,
                    step: 0.05,
                    name: lang.volume().into(),
                },
            )
            .unwrap_or_else(missing);
        let volume_text = t
            .add(
                volume_row,
                Style {
                    width: Length::Px(44.0),
                    ..Style::default()
                },
                Kind::text(
                    "",
                    theme.text.label.with_weight(500).with_color(Paint::Muted),
                ),
            )
            .unwrap_or_else(missing);
        n.volume = Some((volume, volume_text));
        n.output = t.add(
            sound,
            Style::default(),
            Kind::text_line(
                "",
                theme.text.label.with_weight(500).with_color(Paint::Muted),
            ),
        );

        // Brightness, per screen.
        if !state.brightness.is_empty() {
            let light = t
                .add(
                    sheet,
                    Style {
                        gap: 12.0,
                        ..Style::default()
                    },
                    Kind::Box(Look::default()),
                )
                .unwrap_or_else(missing);
            t.add(
                light,
                Style::default(),
                Kind::text(lang.brightness_caption(), caption),
            )
            .unwrap_or_else(missing);
            for (screen, _) in &state.brightness {
                let r = t
                    .add(light, row(12.0), Kind::Box(Look::default()))
                    .unwrap_or_else(missing);
                t.add(
                    r,
                    Style {
                        width: Length::Px(88.0),
                        ..Style::default()
                    },
                    Kind::text_line(screen.as_str(), theme.text.label.with_weight(500)),
                )
                .unwrap_or_else(missing);
                let slider = t
                    .add(
                        r,
                        Style {
                            width: Length::Fill,
                            ..Style::default()
                        },
                        Kind::Slider {
                            value: 0.0,
                            step: 0.05,
                            name: lang.brightness_of(screen),
                        },
                    )
                    .unwrap_or_else(missing);
                let text = t
                    .add(
                        r,
                        Style {
                            width: Length::Px(44.0),
                            ..Style::default()
                        },
                        Kind::text(
                            "",
                            theme.text.label.with_weight(500).with_color(Paint::Muted),
                        ),
                    )
                    .unwrap_or_else(missing);
                n.brightness.push((screen.clone(), slider, text));
            }
        }

        // Keyboard layouts.
        if state.layouts.len() > 1 {
            let keys = t
                .add(sheet, row(10.0), Kind::Box(Look::default()))
                .unwrap_or_else(missing);
            t.add(
                keys,
                Style::default(),
                Kind::text(
                    lang.keyboard(),
                    theme.text.label.with_weight(500).with_color(Paint::Muted),
                ),
            )
            .unwrap_or_else(missing);
            for layout in &state.layouts {
                let id = t
                    .add(
                        keys,
                        Style {
                            height: Length::Px(40.0),
                            ..Style::default()
                        },
                        Kind::Button(
                            Button::new(layout.as_str(), ButtonKind::Chip, &theme)
                                .with_name(lang.keyboard_layout(layout)),
                        ),
                    )
                    .unwrap_or_else(missing);
                n.layouts.push(id);
            }
        }

        // What is left, then the notifications.
        t.add(
            sheet,
            Style {
                height: Length::Fill,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        )
        .unwrap_or_else(missing);
        let foot = t
            .add(
                sheet,
                Style {
                    justify: Justify::SpaceBetween,
                    ..row(8.0)
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(missing);
        n.notifications = t.add(foot, Style::default(), Kind::text("", theme.text.body));
        n.clear = t.add(
            foot,
            Style {
                height: Length::Px(40.0),
                ..Style::default()
            },
            Kind::button(lang.clear_all(), ButtonKind::Plain, &theme),
        );
    }

    fn apply(&mut self, state: &QuickState) {
        let lang = self.lang;
        let theme = self.tree.theme().clone();
        let t = &mut self.tree;
        let n = &self.nodes;
        for (nodes, (_, on, detail, works)) in n.tiles.iter().zip(&state.tiles) {
            let on = *on && *works;
            t.set_look(
                nodes.card,
                Look {
                    background: Some(if on {
                        Paint::Accent
                    } else {
                        Paint::SurfaceHover
                    }),
                    radius: 16.0,
                    ..Look::default()
                },
            );
            t.set_selected_box(nodes.card, on);
            let (ink, sub_ink) = match (on, *works) {
                (true, _) => (Paint::OnAccent, Paint::OnAccent),
                (false, true) => (Paint::Text, Paint::Muted),
                (false, false) => (Paint::Muted, Paint::Muted),
            };
            t.set_icon_paint(nodes.icon, ink);
            t.set_text_style(
                nodes.name,
                theme.text.title.with_weight(700).with_color(ink),
            );
            t.set_text(nodes.sub, detail);
            t.set_text_style(
                nodes.sub,
                TextStyle {
                    color: sub_ink,
                    ..theme.text.caption.with_weight(500)
                },
            );
        }
        for (i, id) in n.profiles.iter().enumerate() {
            t.set_selected(*id, i == state.profile);
        }
        if let Some((slider, text)) = n.volume {
            t.set_slider_value(slider, f32::from(state.volume) / 100.0);
            t.set_text(text, &format!("{}\u{202f}%", state.volume));
        }
        if let Some(out) = n.output {
            t.set_text(out, &lang.output_device(&state.output_device));
        }
        for (screen, slider, text) in &n.brightness {
            if let Some((_, b)) = state.brightness.iter().find(|(s, _)| s == screen) {
                t.set_slider_value(*slider, f32::from(*b) / 100.0);
                t.set_text(*text, &format!("{b}\u{202f}%"));
            }
        }
        for (i, id) in n.layouts.iter().enumerate() {
            t.set_selected(*id, i == state.layout);
        }
        if let (Some(battery), Some(text)) = (n.battery, &state.battery) {
            t.set_text(battery, text);
        }
        if let Some(id) = n.notifications {
            t.set_text(id, &lang.notifications_count(state.notifications));
        }
        if let Some(id) = n.clear {
            t.set_enabled(id, state.notifications > 0);
        }
    }

    /// The sheet, without the room for its shadow, relative to the surface.
    fn card(&self) -> Rect {
        let s = self.tree.size();
        Rect::new(ROOM, 0.0, s.w - ROOM, s.h)
    }

    pub fn hit(&self, at: Point) -> bool {
        self.open && self.card().contains(at)
    }

    fn actions(&mut self, events: Vec<Event>) -> Vec<QuickAction> {
        let Some(state) = self.state.clone() else {
            return Vec::new();
        };
        let n = &self.nodes;
        let mut out = Vec::new();
        for event in events {
            match event {
                Event::Activated(id) => {
                    if let Some((nodes, (_, on, _, _))) = n
                        .tiles
                        .iter()
                        .zip(&state.tiles)
                        .find(|(nodes, _)| nodes.card == id)
                    {
                        out.push(QuickAction::Toggle(nodes.tile, !on));
                    } else if let Some(i) = n.profiles.iter().position(|p| *p == id) {
                        out.push(QuickAction::Profile(i));
                    } else if let Some(i) = n.layouts.iter().position(|l| *l == id) {
                        out.push(QuickAction::Layout(i));
                    } else if Some(id) == n.clear {
                        out.push(QuickAction::ClearNotifications);
                    }
                }
                Event::Changed(id) => {
                    let percent = |t: &Tree| t.slider_value(id).map(|v| (v * 100.0).round() as u8);
                    if n.volume.is_some_and(|(s, _)| s == id) {
                        out.extend(percent(&self.tree).map(QuickAction::Volume));
                    } else if let Some((screen, ..)) =
                        n.brightness.iter().find(|(_, s, _)| *s == id)
                    {
                        out.extend(
                            percent(&self.tree).map(|v| QuickAction::Brightness(screen.clone(), v)),
                        );
                    }
                }
            }
        }
        out
    }

    /// Pointer input relative to the surface; a press outside closes it.
    pub fn input(&mut self, at: Point, input: Input) -> Vec<QuickAction> {
        if !self.open {
            return Vec::new();
        }
        if matches!(input, Input::PointerButton { pressed: true }) && !self.hit(at) {
            self.close();
            return vec![QuickAction::Closed];
        }
        let events = self.tree.input(input);
        self.actions(events)
    }

    /// A key while open: Escape closes, the arrows move a focused slider,
    /// Tab goes round.
    pub fn key(&mut self, key: Key) -> Vec<QuickAction> {
        if !self.open {
            return Vec::new();
        }
        if key == Key::Escape {
            self.close();
            return vec![QuickAction::Closed];
        }
        let events = self.tree.input(Input::Key(key));
        self.actions(events)
    }

    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
        self.now = now;
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

/// The tiles as the stub and the shell know them today.
pub fn tiles_in_order() -> [Tile; 8] {
    Tile::ALL
}
