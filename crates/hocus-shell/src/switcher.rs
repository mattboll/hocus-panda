//! The desktop switcher (#36): while a screen switches desktops, a strip of
//! its desktops in that screen's proportions, each with its windows as
//! small blocks of their colour, the one shown ringed. Named desktops have
//! their names under them, as board D03 draws them; one where something
//! goes on carries its badge in its corner (#192). It only draws; the
//! compositor shows it during the switch and a moment after.

use std::time::Duration;

use hocus_ui::{
    AccessNode, Color, Direction, Insets, Kind, Length, Look, NodeId, Paint, PixelRect, Point,
    Size, Style, Surface, Theme, Tree,
};

use crate::{activity::Badge, column::View, i18n::Lang};

/// A desktop's height in the strip, logical.
const HEIGHT: f32 = 72.0;
const GAP: f32 = 10.0;
const PAD: f32 = 12.0;
/// Room for the shadow around the card.
const ROOM: f32 = 18.0;
/// A name's line under its desktop, and the gap above it.
const LABEL: f32 = 16.0;
const LABEL_GAP: f32 = 6.0;
/// A badge's side.
const BADGE: f32 = 10.0;

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");

/// A window in a desktop: where, as a share of the screen (0 to 1), and
/// its colour.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: Color,
}

/// A desktop in the strip: its windows, its name (empty: none) and its
/// badge.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Desk {
    pub blocks: Vec<Block>,
    pub name: String,
    pub badge: Badge,
}

pub struct Switcher {
    tree: Tree,
    surface: Surface,
}

impl std::fmt::Debug for Switcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Switcher").finish_non_exhaustive()
    }
}

impl Switcher {
    /// The size it takes for `count` desktops of a screen `aspect` wide
    /// (width over height), `named` when they have their names under them.
    pub fn size(count: usize, aspect: f32, named: bool) -> Size {
        let w = HEIGHT * aspect.clamp(0.5, 4.0);
        let n = count.max(1) as f32;
        let label = if named { LABEL_GAP + LABEL } else { 0.0 };
        Size::new(
            n * w + (n - 1.0) * GAP + 2.0 * PAD + 2.0 * ROOM,
            HEIGHT + label + 2.0 * PAD + 2.0 * ROOM,
        )
    }

    /// Whether these desktops show their names: when one has a name.
    pub fn named(desktops: &[Desk]) -> bool {
        desktops.iter().any(|d| !d.name.is_empty())
    }

    /// Each desktop, and the one shown.
    pub fn new(
        theme: Theme,
        lang: Lang,
        scale: f32,
        aspect: f32,
        desktops: &[Desk],
        shown: usize,
    ) -> Self {
        let named = Self::named(desktops);
        let size = Self::size(desktops.len(), aspect, named);
        let mut tree = Tree::new(theme, size, scale);
        let root = tree.root();
        tree.set_look(root, Look::default());
        tree.set_style(
            root,
            Style {
                padding: Insets::all(ROOM),
                ..Style::default()
            },
        );
        let card = tree
            .add(
                root,
                Style {
                    direction: Direction::Row,
                    padding: Insets::all(PAD),
                    gap: GAP,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Glass),
                    border: Some(Paint::Border),
                    radius: 18.0,
                    shadow: 14.0,
                }),
            )
            .unwrap_or_else(MISSING);
        let w = HEIGHT * aspect.clamp(0.5, 4.0);
        for (
            i,
            Desk {
                blocks,
                name,
                badge,
            },
        ) in desktops.iter().enumerate()
        {
            let (here, badge) = (i == shown, *badge);
            // A named strip has each name under its desktop.
            let place = if named {
                tree.add(
                    card,
                    Style {
                        gap: LABEL_GAP,
                        align: hocus_ui::Align::Center,
                        ..Style::default()
                    },
                    Kind::Box(Look::default()),
                )
                .unwrap_or_else(MISSING)
            } else {
                card
            };
            let desk = tree
                .add(
                    place,
                    Style {
                        width: Length::Px(w),
                        height: Length::Px(HEIGHT),
                        ..Style::default()
                    },
                    Kind::Box(Look {
                        background: Some(Paint::Background),
                        border: Some(if here { Paint::Accent } else { Paint::Border }),
                        radius: 8.0,
                        ..Look::default()
                    }),
                )
                .unwrap_or_else(MISSING);
            tree.set_name(desk, &lang.desktop_of(i + 1, desktops.len()));
            if here {
                tree.set_selected_box(desk, true);
            }
            if named {
                let label = theme_label(&tree, here);
                // Centred under it, cut to its width.
                let line = tree
                    .add(
                        place,
                        Style {
                            direction: Direction::Row,
                            justify: hocus_ui::Justify::Center,
                            width: Length::Px(w),
                            height: Length::Px(LABEL),
                            clip: true,
                            ..Style::default()
                        },
                        Kind::Box(Look::default()),
                    )
                    .unwrap_or_else(MISSING);
                let text = tree
                    .add(
                        line,
                        Style {
                            grow: 0.0,
                            ..Style::default()
                        },
                        Kind::text_line(lang.desktop_label(name, i + 1), label),
                    )
                    .unwrap_or_else(MISSING);
                tree.set_selected_box(text, here);
            }
            for b in blocks {
                tree.add(
                    desk,
                    Style {
                        at: Some(Point::new(b.x * w, b.y * HEIGHT)),
                        width: Length::Px((b.w * w).max(3.0)),
                        height: Length::Px((b.h * HEIGHT).max(3.0)),
                        ..Style::default()
                    },
                    Kind::Box(Look {
                        background: Some(Paint::Rgba(b.color)),
                        radius: 3.0,
                        ..Look::default()
                    }),
                )
                .unwrap_or_else(MISSING);
            }
            // Its badge, in its corner, over its windows; said to screen
            // readers with its desktop.
            if badge.is_some() && !here {
                let dot = tree
                    .add(
                        desk,
                        Style {
                            at: Some(Point::new(w - BADGE - 5.0, 5.0)),
                            width: Length::Px(BADGE),
                            height: Length::Px(BADGE),
                            ..Style::default()
                        },
                        Kind::Box(Look {
                            background: Some(Paint::Background),
                            radius: BADGE / 2.0,
                            ..Look::default()
                        }),
                    )
                    .unwrap_or_else(MISSING);
                tree.add(
                    dot,
                    Style {
                        width: Length::Fill,
                        height: Length::Fill,
                        ..Style::default()
                    },
                    Kind::Custom {
                        draw: Box::new(crate::activity::BadgeDot(badge)),
                        label: lang.desktop_badge(&lang.desktop_label(name, i + 1), badge),
                    },
                )
                .unwrap_or_else(MISSING);
            }
        }
        let mut surface = Surface::new(size, scale);
        let damage = tree.update(Duration::ZERO);
        surface.draw(&mut tree, &damage);
        Self { tree, surface }
    }

    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
        let damage = self.tree.update(now);
        self.surface.draw(&mut self.tree, &damage)
    }

    /// Its see-through panels, from its corner, for the compositor to blur
    /// what is behind them (#37, #162).
    pub fn glass(&self) -> Vec<hocus_ui::Glass> {
        self.tree.glass()
    }

    /// Its panels' shadows, from its corner, for the compositor to draw
    /// under it (#281).
    pub fn shadows(&self) -> Vec<hocus_ui::Cast> {
        self.tree.shadows()
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

/// A name under its desktop: the shown one's in the accent.
fn theme_label(tree: &Tree, here: bool) -> hocus_ui::TextStyle {
    let style = tree.theme().text.label;
    hocus_ui::TextStyle {
        size: 12.0,
        weight: 700,
        ..style
    }
    .with_color(if here { Paint::Accent } else { Paint::Muted })
}
