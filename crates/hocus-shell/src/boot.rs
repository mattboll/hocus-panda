//! The boot screen (#48): while a saved session is put back, the pandas
//! set the desktop up: the logo, the name, how far it has got, and two
//! pandas at work in a corner. It goes as soon as everything is back.
//!
//! In the Matrix theme it leaves its background and its progress bar to
//! the compositor, which draws the rain there and fills the bar with
//! glyphs, and lists the windows as a terminal would, each `OK` once
//! back (docs/design/matrix-theme.md, §16).

use std::time::Duration;

use hocus_ui::{
    AccessNode, Align, Direction, Justify, Kind, Length, Look, NodeId, Paint, PixelRect, Point,
    Rect, Size, Style, Surface, Theme, Tree, Typeface,
};

use crate::{
    column::View,
    i18n::Lang,
    matrix::{HEAD, Variant},
    panda::{Character, Mood, Panda},
};

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");

/// The progress bar's width, logical pixels.
const BAR: f32 = 260.0;
/// The terminal's lines, at most, the latest.
const LINES: usize = 6;

pub struct Boot {
    tree: Tree,
    surface: Surface,
    lang: Lang,
    bar: NodeId,
    filled: NodeId,
    line: NodeId,
    progress: (usize, usize),
    /// The Matrix theme's terminal, and what it lists.
    terminal: Option<NodeId>,
    lines: Vec<(String, bool)>,
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
    /// work out (the logo stays: it is the name's). The Matrix variant has
    /// no background, an empty bar and a terminal.
    pub fn new(
        theme: Theme,
        lang: Lang,
        size: Size,
        scale: f32,
        total: usize,
        pandas: bool,
        variant: Variant,
    ) -> Self {
        let matrix = variant == Variant::Matrix;
        let mut tree = Tree::new(theme.clone(), size, scale);
        let root = tree.root();
        tree.set_look(
            root,
            Look {
                background: (!matrix).then_some(Paint::Background),
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
                    background: (!matrix).then_some(Paint::SurfaceHover),
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
                    background: (!matrix).then_some(Paint::Accent),
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
        let terminal = matrix.then(|| {
            tree.add(
                root,
                Style {
                    width: Length::Px(BAR + 60.0),
                    height: Length::Px(LINES as f32 * 22.0),
                    gap: 4.0,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(MISSING)
        });
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
            bar,
            filled,
            line,
            progress: (usize::MAX, total),
            terminal,
            lines: Vec::new(),
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

    /// The terminal's lines in the Matrix theme: each window put back, by
    /// its application, and whether it is back.
    pub fn set_lines(&mut self, lines: &[(String, bool)]) {
        let Some(terminal) = self.terminal else {
            return;
        };
        let shown = &lines[lines.len().saturating_sub(LINES)..];
        if self.lines == shown {
            return;
        }
        self.lines = shown.to_vec();
        for child in self.tree.children(terminal) {
            self.tree.remove(child);
        }
        let theme = self.tree.theme().clone();
        let mono = hocus_ui::TextStyle {
            typeface: Typeface::Mono,
            ..theme.text.body
        };
        for (app, back) in shown {
            let row = self
                .tree
                .add(
                    terminal,
                    Style {
                        direction: Direction::Row,
                        width: Length::Fill,
                        justify: Justify::SpaceBetween,
                        ..Style::default()
                    },
                    Kind::Box(Look::default()),
                )
                .unwrap_or_else(MISSING);
            self.tree
                .add(
                    row,
                    Style::default(),
                    Kind::text_line(format!("> {app}"), mono.with_color(Paint::Muted)),
                )
                .unwrap_or_else(MISSING);
            // Back: its OK bright, as the boot's status lines are.
            let (status, color) = if *back {
                ("OK", Paint::Rgba(HEAD))
            } else {
                ("..", Paint::Muted)
            };
            self.tree
                .add(
                    row,
                    Style::default(),
                    Kind::text_line(status, mono.with_weight(700).with_color(color)),
                )
                .unwrap_or_else(MISSING);
        }
    }

    /// The progress bar, logical from the corner: the Matrix theme's
    /// compositor fills it with glyphs.
    pub fn bar(&self) -> Option<Rect> {
        self.tree.rect(self.bar)
    }

    /// How much of the bar is filled, 0 to 1.
    pub fn progress(&self) -> f32 {
        let (done, total) = self.progress;
        if total == 0 {
            1.0
        } else {
            done.min(total) as f32 / total as f32
        }
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
            Variant::Plain,
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
            Variant::Plain,
        );
        assert!(!labels(&without).iter().any(|l| l == "Panda waving"));
    }

    #[test]
    fn the_matrix_terminal_lists_the_windows_and_says_ok_once_back() {
        let mut boot = Boot::new(
            Theme::default(),
            Lang::En,
            Size::new(1280.0, 800.0),
            1.0,
            2,
            false,
            Variant::Matrix,
        );
        boot.set_lines(&[("editor".into(), true), ("mail".into(), false)]);
        boot.set_progress(1, 2);
        boot.draw(Duration::ZERO);
        let dump = boot.access().dump();
        assert!(
            dump.contains("> editor") && dump.contains("\"OK\""),
            "{dump}"
        );
        assert!(dump.contains("> mail") && dump.contains("\"..\""), "{dump}");
        assert_eq!(boot.progress(), 0.5);
        assert_eq!(boot.bar().map(|r| r.w), Some(BAR));
    }
}
