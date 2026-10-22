//! "Identify": each screen shows its name, maker and size for a few seconds,
//! so that the settings' Screens page can be matched with the desk.

use std::time::Duration;

use hocus_ui::{
    AccessNode, Align, Insets, Justify, Kind, Length, Look, NodeId, Paint, PixelRect, Point, Size,
    Style, Surface, Theme, Tree,
};

use crate::column::View;

/// The card's logical size.
pub const SIZE: Size = Size::new(440.0, 150.0);

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");

pub struct Identify {
    tree: Tree,
    surface: Surface,
}

impl std::fmt::Debug for Identify {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identify").finish_non_exhaustive()
    }
}

impl Identify {
    /// `name` large, then `detail` (maker, model, size).
    pub fn new(theme: Theme, scale: f32, name: &str, detail: &str) -> Self {
        let mut tree = Tree::new(theme.clone(), SIZE, scale);
        let root = tree.root();
        tree.set_style(
            root,
            Style {
                justify: Justify::Center,
                align: Align::Center,
                padding: Insets::all(24.0),
                gap: 8.0,
                width: Length::Px(SIZE.w),
                height: Length::Px(SIZE.h),
                ..Style::default()
            },
        );
        tree.set_look(
            root,
            Look {
                background: Some(Paint::Surface),
                radius: 24.0,
                border: Some(Paint::Accent),
                shadow: 0.0,
            },
        );
        tree.add(
            root,
            Style::default(),
            Kind::text(name, theme.text.headline.with_weight(800)),
        )
        .unwrap_or_else(MISSING);
        tree.add(
            root,
            Style::default(),
            Kind::text(detail, theme.text.body.with_color(Paint::Muted)),
        )
        .unwrap_or_else(MISSING);
        Self {
            tree,
            surface: Surface::new(SIZE, scale),
        }
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
