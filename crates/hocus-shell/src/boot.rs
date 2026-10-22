//! The boot screen (#48): while a saved session is put back, the pandas
//! set the desktop up: the logo, the name, how far it has got, and two
//! pandas at work in a corner. It goes as soon as everything is back.

use std::time::Duration;

use hocus_ui::{
    AccessNode, Align, Direction, Justify, Kind, Length, Look, NodeId, Paint, PixelRect, Point,
    Size, Style, Surface, Theme, Tree,
};

use crate::{
    column::View,
    i18n::Lang,
    panda::{Character, Mood, Panda},
};

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");

/// The progress bar's width, logical pixels.
const BAR: f32 = 260.0;

pub struct Boot {
    tree: Tree,
    surface: Surface,
    lang: Lang,
    filled: NodeId,
    line: NodeId,
    progress: (usize, usize),
}

impl std::fmt::Debug for Boot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Boot")
            .field("progress", &self.progress)
            .finish_non_exhaustive()
    }
}

impl Boot {
    /// For `total` windows to put back; `pandas` false leaves the two at
    /// work out (the logo stays: it is the name's).
    pub fn new(
        theme: Theme,
        lang: Lang,
        size: Size,
        scale: f32,
        total: usize,
        pandas: bool,
    ) -> Self {
        let mut tree = Tree::new(theme.clone(), size, scale);
        let root = tree.root();
        tree.set_look(
            root,
            Look {
                background: Some(Paint::Background),
                ..Look::default()
            },
        );
        tree.set_style(
            root,
            Style {
                justify: Justify::Center,
                align: Align::Center,
                gap: 22.0,
                ..Style::default()
            },
        );
        let side = |px: f32| Style {
            width: Length::Px(px),
            height: Length::Px(px),
            ..Style::default()
        };
        tree.add(
            root,
            side(200.0),
            Kind::Custom {
                draw: Box::new(Panda::logo(&theme).looping()),
                label: Some(lang.logo().into()),
            },
        )
        .unwrap_or_else(MISSING);
        let name = tree
            .add(
                root,
                Style {
                    direction: Direction::Row,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(MISSING);
        let big = hocus_ui::TextStyle {
            size: 44.0,
            line_height: 1.1,
            ..theme.text.display.with_weight(600)
        };
        tree.add(name, Style::default(), Kind::text("Hocus", big))
            .unwrap_or_else(MISSING);
        tree.add(
            name,
            Style::default(),
            Kind::text("panda", big.with_weight(500).with_color(Paint::Accent)),
        )
        .unwrap_or_else(MISSING);
        let bar = tree
            .add(
                root,
                Style {
                    width: Length::Px(BAR),
                    height: Length::Px(6.0),
                    align: Align::Start,
                    clip: true,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::SurfaceHover),
                    radius: 3.0,
                    ..Look::default()
                }),
            )
            .unwrap_or_else(MISSING);
        let filled = tree
            .add(
                bar,
                Style {
                    width: Length::Px(0.0),
                    height: Length::Px(6.0),
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Accent),
                    radius: 3.0,
                    ..Look::default()
                }),
            )
            .unwrap_or_else(MISSING);
        let line = tree
            .add(
                root,
                Style::default(),
                Kind::text("", theme.text.body.with_color(Paint::Muted)),
            )
            .unwrap_or_else(MISSING);
        if pandas {
            // Two at work in the corner, as the board has them.
            for (i, (character, mood)) in [
                (Character::Watcher, Mood::Work),
                (Character::Apprentice, Mood::Wave),
            ]
            .into_iter()
            .enumerate()
            {
                tree.add(
                    root,
                    Style {
                        at: Some(Point::new(24.0 + 84.0 * i as f32, size.h - 100.0)),
                        ..side(80.0)
                    },
                    Kind::Custom {
                        draw: Box::new(Panda::themed(character, mood, &theme).looping()),
                        label: Some(mood.label(lang).into()),
                    },
                )
                .unwrap_or_else(MISSING);
            }
        }
        let mut boot = Self {
            surface: Surface::new(size, scale),
            tree,
            lang,
            filled,
            line,
            progress: (usize::MAX, total),
        };
        boot.set_progress(0, total);
        boot
    }

    /// `done` of `total` windows back.
    pub fn set_progress(&mut self, done: usize, total: usize) {
        if self.progress == (done, total) {
            return;
        }
        self.progress = (done, total);
        let part = if total == 0 {
            1.0
        } else {
            done.min(total) as f32 / total as f32
        };
        self.tree.set_style(
            self.filled,
            Style {
                width: Length::Px(BAR * part),
                height: Length::Px(6.0),
                ..Style::default()
            },
        );
        self.tree
            .set_text(self.line, &self.lang.putting_back(total));
    }

    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
        let damage = self.tree.update(now);
        self.surface.draw(&mut self.tree, &damage)
    }

    /// Whether it moves: frames are wanted while it shows.
    pub fn is_moving(&self, now: Duration) -> bool {
        self.tree.is_animating(now)
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
    fn it_says_how_many_windows_come_back_and_fills_as_they_do() {
        let mut boot = Boot::new(
            Theme::default(),
            Lang::En,
            Size::new(1280.0, 800.0),
            1.0,
            7,
            true,
        );
        boot.draw(Duration::ZERO);
        let labels = |boot: &Boot| {
            boot.access()
                .walk()
                .into_iter()
                .filter_map(|n| n.label.clone())
                .collect::<Vec<_>>()
        };
        let shown = labels(&boot);
        assert!(
            shown
                .iter()
                .any(|l| l == "The pandas are putting your 7 windows back"),
            "{shown:?}"
        );
        assert!(shown.iter().any(|l| l == "Panda waving"), "{shown:?}");
        boot.set_progress(7, 7);
        assert!(!boot.draw(Duration::from_millis(16)).is_empty());
        let without = Boot::new(
            Theme::default(),
            Lang::En,
            Size::new(1280.0, 800.0),
            1.0,
            7,
            false,
        );
        assert!(!labels(&without).iter().any(|l| l == "Panda waving"));
    }
}
