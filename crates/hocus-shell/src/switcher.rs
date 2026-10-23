//! The desktop switcher (#36): while a screen switches desktops, a strip of
//! its desktops in that screen's proportions, each with its windows as
//! small blocks of their colour, the one shown ringed. It only draws; the
//! compositor shows it during the switch and a moment after.

use std::time::Duration;

use hocus_ui::{
    AccessNode, Color, Direction, Insets, Kind, Length, Look, NodeId, Paint, PixelRect, Point,
    Size, Style, Surface, Theme, Tree,
};

use crate::{column::View, i18n::Lang};

/// A desktop's height in the strip, logical.
const HEIGHT: f32 = 72.0;
const GAP: f32 = 10.0;
const PAD: f32 = 12.0;
/// Room for the shadow around the card.
const ROOM: f32 = 18.0;

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
    /// (width over height).
    pub fn size(count: usize, aspect: f32) -> Size {
        let w = HEIGHT * aspect.clamp(0.5, 4.0);
        let n = count.max(1) as f32;
        Size::new(
            n * w + (n - 1.0) * GAP + 2.0 * PAD + 2.0 * ROOM,
            HEIGHT + 2.0 * PAD + 2.0 * ROOM,
        )
    }

    /// Each desktop's windows, and the one shown.
    pub fn new(
        theme: Theme,
        lang: Lang,
        scale: f32,
        aspect: f32,
        desktops: &[Vec<Block>],
        shown: usize,
    ) -> Self {
        let size = Self::size(desktops.len(), aspect);
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
        for (i, blocks) in desktops.iter().enumerate() {
            let here = i == shown;
            let desk = tree
                .add(
                    card,
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
