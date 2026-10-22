//! Drawing on the screen (#86): Super and a left drag leave a felt-tip
//! line over everything, for showing something during a call. Each piece
//! of the line is a node of its own, so a new one redraws only its own
//! small square; the compositor fades the whole when Super is let go.

use std::time::Duration;

use hocus_ui::{
    Color, Kind, Length, Look, NodeId, PixelRect, Point, Rect, Size, Style, Surface, Theme, Tree,
    canvas::Canvas, tree::Draw,
};

use crate::column::View;

/// The line's width, logical.
const WIDTH: f32 = 6.0;

/// A piece of the line, from `a` to `b`, from its node's corner.
struct Piece {
    a: Point,
    b: Point,
    color: Color,
}

impl Draw for Piece {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, _theme: &Theme, _now: Duration) {
        let at = |p: Point| Point::new(rect.x + p.x, rect.y + p.y);
        canvas.polyline(&[at(self.a), at(self.b)], WIDTH, self.color);
    }

    fn size(&self) -> Size {
        Size::default()
    }
}

/// One screen's drawing.
pub struct Ink {
    tree: Tree,
    surface: Surface,
    pieces: Vec<NodeId>,
    color: Color,
}

impl std::fmt::Debug for Ink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ink")
            .field("pieces", &self.pieces.len())
            .finish_non_exhaustive()
    }
}

impl Ink {
    /// A clear sheet the size of a screen.
    pub fn new(theme: Theme, size: Size, scale: f32) -> Self {
        // The accent, as a felt tip: a little see-through.
        let color = theme.colors.accent.with_alpha(230);
        let mut tree = Tree::new(theme, size, scale);
        let root = tree.root();
        tree.set_look(root, Look::default());
        let mut surface = Surface::new(size, scale);
        let damage = tree.update(Duration::ZERO);
        surface.draw(&mut tree, &damage);
        Self {
            tree,
            surface,
            pieces: Vec::new(),
            color,
        }
    }

    /// The line goes on from `a` to `b`, from the screen's corner.
    pub fn line(&mut self, a: Point, b: Point) {
        let pad = WIDTH;
        let x = a.x.min(b.x) - pad;
        let y = a.y.min(b.y) - pad;
        let w = (a.x - b.x).abs() + 2.0 * pad;
        let h = (a.y - b.y).abs() + 2.0 * pad;
        let root = self.tree.root();
        let piece = Piece {
            a: Point::new(a.x - x, a.y - y),
            b: Point::new(b.x - x, b.y - y),
            color: self.color,
        };
        if let Some(id) = self.tree.add(
            root,
            Style {
                at: Some(Point::new(x, y)),
                width: Length::Px(w),
                height: Length::Px(h),
                ..Style::default()
            },
            Kind::Custom {
                draw: Box::new(piece),
                label: None,
            },
        ) {
            self.pieces.push(id);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    /// What changed since the last time, in pixels.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_piece_redraws_only_around_itself() {
        let mut ink = Ink::new(Theme::default(), Size::new(1000.0, 800.0), 1.0);
        ink.draw(Duration::ZERO);
        ink.line(Point::new(100.0, 100.0), Point::new(120.0, 110.0));
        let damage = ink.draw(Duration::ZERO);
        let area: u32 = damage.iter().map(|r| r.w * r.h).sum();
        assert!(area > 0 && area < 2000, "{damage:?}");
        // The line is there.
        let px = ink.view();
        let i = (105 * px.width as usize + 110) * 4;
        assert!(px.pixels[i + 3] > 0, "drawn where the line is");
    }
}
