//! The Matrix theme's size readout (docs/design/matrix-theme.md, §8): while
//! a window is resized, its size beside the pointer, `[1280x720]`, green on
//! black in the terminal's face. The compositor draws it again only when
//! the size changes.

use std::time::Duration;

use hocus_ui::{
    Align, Insets, Justify, Kind, Length, Look, NodeId, Paint, PixelRect, Point, Size, Style,
    Surface, Theme, Tree, theme::Typeface,
};

use crate::{
    column::View,
    matrix::{BLACK, GLOW},
};

/// The readout's logical size: room for `[99999x99999]`.
pub const SIZE: Size = Size::new(132.0, 26.0);

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");

pub struct Readout {
    tree: Tree,
    surface: Surface,
    text: NodeId,
    shown: String,
}

impl std::fmt::Debug for Readout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Readout")
            .field("shown", &self.shown)
            .finish_non_exhaustive()
    }
}

/// What the readout says for a window `w` by `h` logical pixels.
pub fn text(w: i32, h: i32) -> String {
    format!("[{w}x{h}]")
}

impl Readout {
    pub fn new(theme: Theme, scale: f32) -> Self {
        let mut tree = Tree::new(theme.clone(), SIZE, scale);
        let root = tree.root();
        tree.set_style(
            root,
            Style {
                justify: Justify::Center,
                align: Align::Center,
                padding: Insets::all(4.0),
                width: Length::Px(SIZE.w),
                height: Length::Px(SIZE.h),
                ..Style::default()
            },
        );
        tree.set_look(
            root,
            Look {
                background: Some(Paint::Rgba(BLACK.with_alpha(230))),
                radius: 4.0,
                border: Some(Paint::Rgba(GLOW.with_alpha(150))),
                shadow: 0.0,
            },
        );
        let mut style = theme.text.label.with_color(Paint::Rgba(GLOW));
        style.typeface = Typeface::Mono;
        let text = tree
            .add(root, Style::default(), Kind::text("", style))
            .unwrap_or_else(MISSING);
        Self {
            tree,
            surface: Surface::new(SIZE, scale),
            text,
            shown: String::new(),
        }
    }

    /// Shows `text`; whether it changed.
    pub fn set(&mut self, text: &str) -> bool {
        if self.shown == text {
            return false;
        }
        self.shown = text.to_owned();
        self.tree.set_text(self.text, text);
        true
    }

    pub fn shown(&self) -> &str {
        &self.shown
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_reads_the_size_as_a_terminal_would() {
        assert_eq!(text(1280, 720), "[1280x720]");
        let mut readout = Readout::new(Theme::builtin(hocus_ui::ThemeName::Matrix), 1.0);
        assert!(readout.set("[1280x720]"));
        assert!(!readout.set("[1280x720]"), "the same size draws nothing");
        assert!(!readout.draw(Duration::ZERO).is_empty());
        assert_eq!(readout.shown(), "[1280x720]");
    }
}
