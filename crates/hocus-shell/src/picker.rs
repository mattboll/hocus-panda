//! Choosing what a screenshot takes (#69): a region drawn with the pointer,
//! its size in pixels beside it, or the window under the pointer. The rest
//! of the screen dims. The view only draws; the compositor follows the
//! pointer and the keys.

use std::time::Duration;

use hocus_ui::{
    AccessNode, Align, Color, Insets, Justify, Kind, Length, Look, NodeId, Paint, PixelRect, Point,
    Rect, Size, Style, Surface, Theme, Tree,
};

use crate::{column::View, i18n::Lang};

/// What is being chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickMode {
    Region,
    Window,
}

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");
const DIM: Color = Color::rgba(0, 0, 0, 120);

pub struct Picker {
    tree: Tree,
    surface: Surface,
    lang: Lang,
    mode: PickMode,
    /// What is chosen on this screen, in its logical pixels.
    chosen: Option<Rect>,
}

impl std::fmt::Debug for Picker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Picker")
            .field("mode", &self.mode)
            .field("chosen", &self.chosen)
            .finish_non_exhaustive()
    }
}

impl Picker {
    pub fn new(theme: Theme, lang: Lang, size: Size, scale: f32, mode: PickMode) -> Self {
        let mut picker = Self {
            tree: Tree::new(theme, size, scale),
            surface: Surface::new(size, scale),
            lang,
            mode,
            chosen: None,
        };
        picker.build();
        picker
    }

    /// The region drawn, or the window under the pointer, on this screen.
    pub fn set(&mut self, chosen: Option<Rect>) {
        if chosen != self.chosen {
            self.chosen = chosen;
            self.build();
        }
    }

    fn build(&mut self) {
        let theme = self.tree.theme().clone();
        let size = self.tree.size();
        let scale = self.tree.scale();
        let t = &mut self.tree;
        let root = t.root();
        for child in t.children(root) {
            t.remove(child);
        }
        // Only the dimming and the marks: what is chosen shows through.
        t.set_look(root, Look::default());
        let dim = |t: &mut Tree, r: Rect| {
            if r.w > 0.0 && r.h > 0.0 {
                t.add(
                    root,
                    Style {
                        at: Some(Point::new(r.x, r.y)),
                        width: Length::Px(r.w),
                        height: Length::Px(r.h),
                        ..Style::default()
                    },
                    Kind::Box(Look {
                        background: Some(Paint::Rgba(DIM)),
                        ..Look::default()
                    }),
                )
                .unwrap_or_else(MISSING);
            }
        };
        let Some(c) = self.chosen else {
            dim(t, Rect::new(0.0, 0.0, size.w, size.h));
            let hint = match self.mode {
                PickMode::Region => self.lang.pick_region(),
                PickMode::Window => self.lang.pick_window(),
            };
            let pill = t
                .add(
                    root,
                    Style {
                        at: Some(Point::new(0.0, size.h / 2.0 - 24.0)),
                        width: Length::Px(size.w),
                        justify: Justify::Center,
                        align: Align::Center,
                        ..Style::default()
                    },
                    Kind::Box(Look::default()),
                )
                .unwrap_or_else(MISSING);
            let label = t
                .add(
                    pill,
                    Style {
                        padding: Insets::symmetric(12.0, 18.0),
                        ..Style::default()
                    },
                    Kind::Box(Look {
                        background: Some(Paint::Surface),
                        radius: 14.0,
                        border: Some(Paint::Border),
                        ..Look::default()
                    }),
                )
                .unwrap_or_else(MISSING);
            t.add(
                label,
                Style::default(),
                Kind::text_line(hint, theme.text.body),
            )
            .unwrap_or_else(MISSING);
            return;
        };
        // Dim around what is chosen.
        dim(t, Rect::new(0.0, 0.0, size.w, c.y));
        dim(t, Rect::new(0.0, c.y + c.h, size.w, size.h - c.y - c.h));
        dim(t, Rect::new(0.0, c.y, c.x, c.h));
        dim(t, Rect::new(c.x + c.w, c.y, size.w - c.x - c.w, c.h));
        t.add(
            root,
            Style {
                at: Some(Point::new(c.x - 1.0, c.y - 1.0)),
                width: Length::Px(c.w + 2.0),
                height: Length::Px(c.h + 2.0),
                ..Style::default()
            },
            Kind::Box(Look {
                border: Some(Paint::Accent),
                ..Look::default()
            }),
        )
        .unwrap_or_else(MISSING);
        if self.mode == PickMode::Region {
            // Its size in pixels, under it, or inside when there is no room.
            let text = format!(
                "{} × {}",
                (c.w * scale).round() as i32,
                (c.h * scale).round() as i32
            );
            let below = c.y + c.h + 8.0;
            let y = if below + 30.0 < size.h {
                below
            } else {
                c.y + c.h - 38.0
            };
            let pill = t
                .add(
                    root,
                    Style {
                        at: Some(Point::new(c.x.max(4.0), y.max(4.0))),
                        padding: Insets::symmetric(4.0, 10.0),
                        ..Style::default()
                    },
                    Kind::Box(Look {
                        background: Some(Paint::Surface),
                        radius: 8.0,
                        border: Some(Paint::Border),
                        ..Look::default()
                    }),
                )
                .unwrap_or_else(MISSING);
            t.add(
                pill,
                Style::default(),
                Kind::text_line(text, theme.text.label.with_weight(600)),
            )
            .unwrap_or_else(MISSING);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_region_says_its_size_in_pixels() {
        let mut picker = Picker::new(
            Theme::default(),
            Lang::En,
            Size::new(1280.0, 800.0),
            2.0,
            PickMode::Region,
        );
        picker.draw(Duration::ZERO);
        assert!(picker.access().dump().contains("Drag over a region"));
        picker.set(Some(Rect::new(100.0, 100.0, 320.0, 240.0)));
        picker.draw(Duration::ZERO);
        assert!(picker.access().dump().contains("640 × 480"));
    }
}
