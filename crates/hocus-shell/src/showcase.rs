//! Over a whole screen: words as large as they fit, or a QR code to scan
//! with a phone. A click or Escape closes it.

use std::time::Duration;

use hocus_ui::{
    AccessNode, Align, Canvas, Color, Draw, Insets, Justify, Kind, Length, Look, Paint, PixelRect,
    Point, Rect, Size, Style, Surface, TextStyle, Theme, Tree, Typeface,
};

use crate::{column::View, i18n::Lang, launcher::Showcase};

/// A QR code: dark modules on a white card with its quiet zone.
struct QrCode {
    modules: Vec<bool>,
    width: usize,
}

impl Draw for QrCode {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, _theme: &Theme, _now: Duration) {
        canvas.fill_rect(rect, 16.0, Color::WHITE);
        // Four modules of quiet zone around, as the standard asks.
        let cells = self.width as f32 + 8.0;
        let m = rect.w / cells;
        for (i, dark) in self.modules.iter().enumerate() {
            if *dark {
                let (x, y) = ((i % self.width) as f32, (i / self.width) as f32);
                canvas.fill_rect(
                    Rect::new(
                        rect.x + (x + 4.0) * m,
                        rect.y + (y + 4.0) * m,
                        m + 0.25,
                        m + 0.25,
                    ),
                    0.0,
                    Color::BLACK,
                );
            }
        }
    }

    fn size(&self) -> Size {
        Size::new(0.0, 0.0)
    }
}

pub struct Showcased {
    tree: Tree,
    surface: Surface,
    what: Showcase,
}

impl std::fmt::Debug for Showcased {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Showcased")
            .field("what", &self.what)
            .finish_non_exhaustive()
    }
}

impl Showcased {
    /// `what` over a screen of `size` logical pixels.
    pub fn new(what: Showcase, theme: Theme, lang: Lang, size: Size, scale: f32) -> Self {
        let mut tree = Tree::new(theme.clone(), size, scale);
        let root = tree.root();
        tree.set_look(
            root,
            Look {
                background: Some(Paint::Rgba(theme.colors.background.with_alpha(0xeb))),
                ..Look::default()
            },
        );
        tree.set_style(
            root,
            Style {
                align: Align::Center,
                justify: Justify::Center,
                gap: 32.0,
                padding: Insets::all(48.0),
                ..Style::default()
            },
        );
        match &what {
            Showcase::LargeText(text) => {
                // As large as it fits on one line, within reason.
                let fits = (size.w - 96.0) / (text.chars().count().max(1) as f32 * 0.6);
                let style = TextStyle {
                    typeface: Typeface::Display,
                    size: fits.clamp(32.0, 280.0),
                    weight: 600,
                    line_height: 1.1,
                    color: Paint::Text,
                };
                let _ = tree.add(root, Style::default(), Kind::text(text.as_str(), style));
            }
            Showcase::Qr(text) => {
                let code = qrcode::QrCode::new(text.as_bytes()).ok();
                let side = (size.w.min(size.h) * 0.6).round();
                if let Some(code) = code {
                    let _ = tree.add(
                        root,
                        Style {
                            width: Length::Px(side),
                            height: Length::Px(side),
                            ..Style::default()
                        },
                        Kind::Custom {
                            draw: Box::new(QrCode {
                                modules: code
                                    .to_colors()
                                    .into_iter()
                                    .map(|c| c == qrcode::Color::Dark)
                                    .collect(),
                                width: code.width(),
                            }),
                            label: Some(format!("QR: {text}")),
                        },
                    );
                }
                let _ = tree.add(
                    root,
                    Style::default(),
                    Kind::text(text.as_str(), theme.text.headline.with_weight(600)),
                );
            }
        }
        let _ = tree.add(
            root,
            Style::default(),
            Kind::text(
                lang.close_showcase(),
                theme.text.body.with_color(Paint::Muted),
            ),
        );
        Self {
            surface: Surface::new(size, scale),
            tree,
            what,
        }
    }

    pub fn what(&self) -> &Showcase {
        &self.what
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
    fn a_qr_code_is_drawn_for_its_text() {
        let mut s = Showcased::new(
            Showcase::Qr("hello".into()),
            Theme::default(),
            Lang::En,
            Size::new(800.0, 600.0),
            1.0,
        );
        s.draw(Duration::ZERO);
        assert!(s.access().dump().contains("custom \"QR: hello\""));
    }
}
