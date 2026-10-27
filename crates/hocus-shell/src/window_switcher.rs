//! The window switcher (#39): a grid of cards, one per window, with room
//! for its live preview (the compositor draws the window there), its app's
//! colour and its title; a ring glides to the card chosen. Alt+Tab shows
//! the windows of one screen, Super+Tab each screen's own. Over the cards,
//! a screen whose desktops have names, or where something goes on
//! elsewhere, has its desktops in a line, the shown one in the accent, the
//! others with their badges (#192).

use std::time::Duration;

use hocus_ui::{
    AccessNode, Align, Direction, Insets, Kind, Length, Look, NodeId, Paint, PixelRect, Point,
    Property, Rect, Size, Style, Surface, Theme, Tree,
    motion::{Curve, Easing, SMALL},
};

use crate::{activity::Badge, column::View, matrix::Variant};

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");

/// A card's preview, logical.
pub const PREVIEW: Size = Size::new(224.0, 140.0);
const CARD_PAD: f32 = 10.0;
const TITLE: f32 = 30.0;
const GAP: f32 = 14.0;
const PAD: f32 = 18.0;
/// Room for the shadow.
const ROOM: f32 = 18.0;
/// The desktops' line over the cards.
const HEADER: f32 = 20.0;
/// A title is typed in at this pace in the Matrix theme
/// (docs/design/matrix-theme.md, §11).
const TYPED: Duration = Duration::from_millis(3);

/// A screen's desktops, as the line over the cards shows them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Desks {
    /// Each one's name, or its number's.
    pub names: Vec<String>,
    /// What screen readers say of each: its name and badge.
    pub said: Vec<String>,
    pub badges: Vec<Badge>,
    /// From 0.
    pub shown: usize,
}

/// A window as the switcher shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub title: String,
    pub color: hocus_ui::Color,
}

pub struct WindowSwitcher {
    tree: Tree,
    surface: Surface,
    cards: Vec<NodeId>,
    previews: Vec<NodeId>,
    ring: NodeId,
    columns: usize,
    selected: Option<usize>,
    /// Each card's title, and what it says in full.
    titles: Vec<(NodeId, String)>,
    /// Titles typed in since then, a character at a time.
    typing: Option<Duration>,
}

impl std::fmt::Debug for WindowSwitcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowSwitcher")
            .field("cards", &self.cards.len())
            .field("selected", &self.selected)
            .finish_non_exhaustive()
    }
}

fn card_size() -> Size {
    Size::new(
        PREVIEW.w + 2.0 * CARD_PAD,
        PREVIEW.h + 2.0 * CARD_PAD + TITLE,
    )
}

impl WindowSwitcher {
    /// How many cards fit across `width`.
    pub fn columns(count: usize, width: f32) -> usize {
        let fit = ((width - 2.0 * (PAD + ROOM) + GAP) / (card_size().w + GAP)).floor();
        (fit.max(1.0) as usize).min(count.max(1))
    }

    /// The size it takes for `count` cards across at most `width`, with
    /// the desktops' line or not.
    pub fn size(count: usize, width: f32, desks: bool) -> Size {
        let columns = Self::columns(count, width);
        let rows = count.max(1).div_ceil(columns);
        let card = card_size();
        let header = if desks { HEADER + GAP } else { 0.0 };
        Size::new(
            columns as f32 * card.w + (columns as f32 - 1.0) * GAP + 2.0 * (PAD + ROOM),
            rows as f32 * card.h + (rows as f32 - 1.0) * GAP + 2.0 * (PAD + ROOM) + header,
        )
    }

    /// The cards across at most `width`, `selected` ringed (none: none on
    /// this screen), under the screen's desktops when given; in the Matrix
    /// theme each card has the theme's thin border.
    pub fn new(
        theme: Theme,
        scale: f32,
        cards: &[Card],
        selected: Option<usize>,
        width: f32,
        variant: Variant,
        desks: Option<&Desks>,
    ) -> Self {
        let size = Self::size(cards.len(), width, desks.is_some());
        let columns = Self::columns(cards.len(), width);
        let mut tree = Tree::new(theme.clone(), size, scale);
        let root = tree.root();
        tree.set_look(root, Look::default());
        tree.set_style(
            root,
            Style {
                padding: Insets::all(ROOM),
                ..Style::default()
            },
        );
        let panel = tree
            .add(
                root,
                Style {
                    width: Length::Fill,
                    height: Length::Fill,
                    padding: Insets::all(PAD),
                    gap: GAP,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Glass),
                    border: Some(Paint::Border),
                    radius: 22.0,
                    shadow: 16.0,
                }),
            )
            .unwrap_or_else(MISSING);
        if let Some(desks) = desks {
            desks_line(&mut tree, panel, &theme, desks);
        }
        let below = if desks.is_some() { HEADER + GAP } else { 0.0 };
        let card = card_size();
        let mut nodes = Vec::new();
        let mut previews = Vec::new();
        let mut titles = Vec::new();
        for row in cards.chunks(columns) {
            let line = tree
                .add(
                    panel,
                    Style {
                        direction: Direction::Row,
                        gap: GAP,
                        ..Style::default()
                    },
                    Kind::Box(Look::default()),
                )
                .unwrap_or_else(MISSING);
            for c in row {
                let node = tree
                    .add(
                        line,
                        Style {
                            width: Length::Px(card.w),
                            height: Length::Px(card.h),
                            padding: Insets::all(CARD_PAD),
                            gap: 6.0,
                            ..Style::default()
                        },
                        Kind::Box(Look {
                            background: Some(Paint::SurfaceHover),
                            radius: 14.0,
                            border: (variant == Variant::Matrix).then_some(Paint::Border),
                            ..Look::default()
                        }),
                    )
                    .unwrap_or_else(MISSING);
                tree.set_clickable(node, &c.title);
                // Where the window goes: the compositor draws it there.
                let preview = tree
                    .add(
                        node,
                        Style {
                            width: Length::Px(PREVIEW.w),
                            height: Length::Px(PREVIEW.h),
                            ..Style::default()
                        },
                        Kind::Box(Look {
                            background: Some(Paint::Background),
                            radius: 8.0,
                            ..Look::default()
                        }),
                    )
                    .unwrap_or_else(MISSING);
                let label = tree
                    .add(
                        node,
                        Style {
                            direction: Direction::Row,
                            gap: 8.0,
                            align: Align::Center,
                            ..Style::default()
                        },
                        Kind::Box(Look::default()),
                    )
                    .unwrap_or_else(MISSING);
                tree.add(
                    label,
                    Style {
                        width: Length::Px(12.0),
                        height: Length::Px(12.0),
                        ..Style::default()
                    },
                    Kind::Box(Look {
                        background: Some(Paint::Rgba(c.color)),
                        radius: 6.0,
                        ..Look::default()
                    }),
                )
                .unwrap_or_else(MISSING);
                let title = tree
                    .add(
                        label,
                        Style {
                            width: Length::Px(PREVIEW.w - 20.0),
                            ..Style::default()
                        },
                        Kind::text_line(c.title.clone(), theme.text.label.with_weight(600)),
                    )
                    .unwrap_or_else(MISSING);
                titles.push((title, c.title.clone()));
                nodes.push(node);
                previews.push(preview);
            }
        }
        // The ring, laid over the first card and moved to the chosen one.
        let ring = tree
            .add(
                root,
                Style {
                    // From inside the root's padding.
                    at: Some(Point::new(PAD - 4.0, PAD - 4.0 + below)),
                    width: Length::Px(card.w + 8.0),
                    height: Length::Px(card.h + 8.0),
                    ..Style::default()
                },
                Kind::Box(Look {
                    border: Some(Paint::Accent),
                    radius: 18.0,
                    ..Look::default()
                }),
            )
            .unwrap_or_else(MISSING);
        let mut switcher = Self {
            tree,
            surface: Surface::new(size, scale),
            cards: nodes,
            previews,
            ring,
            columns,
            selected: None,
            titles,
            typing: None,
        };
        switcher.place_ring(selected, false);
        let damage = switcher.tree.update(Duration::ZERO);
        switcher.surface.draw(&mut switcher.tree, &damage);
        switcher
    }

    /// Where card `i` is on the grid's first card, logical.
    fn card_offset(&self, i: usize) -> (f32, f32) {
        let card = card_size();
        let (row, column) = (i / self.columns, i % self.columns);
        (column as f32 * (card.w + GAP), row as f32 * (card.h + GAP))
    }

    fn place_ring(&mut self, selected: Option<usize>, glide: bool) {
        let shown = selected.is_some();
        self.tree
            .jump(self.ring, Property::Opacity, if shown { 1.0 } else { 0.0 });
        if let Some(i) = selected {
            let (x, y) = self.card_offset(i);
            let curve = Curve::Ease(Easing::OutCubic, SMALL);
            if glide && self.selected.is_some() {
                self.tree.animate(self.ring, Property::OffsetX, x, curve);
                self.tree.animate(self.ring, Property::OffsetY, y, curve);
            } else {
                self.tree.jump(self.ring, Property::OffsetX, x);
                self.tree.jump(self.ring, Property::OffsetY, y);
            }
            self.tree.set_selected_box(self.cards[i], true);
        }
        if let Some(old) = self.selected.filter(|o| Some(*o) != selected) {
            self.tree.set_selected_box(self.cards[old], false);
        }
        self.selected = selected;
    }

    /// Rings card `i`, gliding from the one ringed; none: no ring.
    pub fn select(&mut self, selected: Option<usize>) {
        if selected != self.selected {
            self.place_ring(selected, true);
        }
    }

    /// The titles show from nothing, typed in from `since` a character
    /// every 3 ms, each card's at once.
    pub fn type_titles(&mut self, since: Duration) {
        self.typing = Some(since);
        for (node, _) in &self.titles {
            self.tree.set_text(*node, "");
        }
    }

    /// The titles as typed by `now`; whether some are still being typed.
    fn type_in(&mut self, now: Duration) {
        let Some(since) = self.typing else {
            return;
        };
        let typed = (now.saturating_sub(since).as_millis() / TYPED.as_millis()) as usize;
        let mut done = true;
        for (node, title) in &self.titles {
            let count = title.chars().count();
            done &= typed >= count;
            let shown: String = title.chars().take(typed).collect();
            self.tree.set_text(*node, &shown);
        }
        if done {
            self.typing = None;
        }
    }

    /// Card `i`, logical from the corner.
    pub fn card(&self, i: usize) -> Option<Rect> {
        self.tree.rect(*self.cards.get(i)?)
    }

    /// Where card `i`'s preview goes, logical from the corner.
    pub fn preview(&self, i: usize) -> Option<Rect> {
        self.tree.rect(*self.previews.get(i)?)
    }

    /// The card at `at`, from the corner.
    pub fn card_at(&self, at: Point) -> Option<usize> {
        self.cards
            .iter()
            .position(|c| self.tree.rect(*c).is_some_and(|r| r.contains(at)))
    }

    pub fn columns_shown(&self) -> usize {
        self.columns
    }

    pub fn is_animating(&self, now: Duration) -> bool {
        self.tree.is_animating(now) || self.typing.is_some()
    }

    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
        self.type_in(now);
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

/// The screen's desktops in a line: each one's name, the shown one in the
/// accent, the others with their badge before it.
fn desks_line(tree: &mut Tree, panel: NodeId, theme: &Theme, desks: &Desks) {
    let line = tree
        .add(
            panel,
            Style {
                direction: Direction::Row,
                gap: 18.0,
                align: Align::Center,
                height: Length::Px(HEADER),
                padding: Insets {
                    left: 4.0,
                    ..Insets::default()
                },
                ..Style::default()
            },
            Kind::Box(Look::default()),
        )
        .unwrap_or_else(MISSING);
    for (i, name) in desks.names.iter().enumerate() {
        let here = i == desks.shown;
        let badge = desks
            .badges
            .get(i)
            .copied()
            .filter(|_| !here)
            .unwrap_or_default();
        let desk = tree
            .add(
                line,
                Style {
                    direction: Direction::Row,
                    gap: 6.0,
                    align: Align::Center,
                    grow: 0.0,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(MISSING);
        // The badge says what goes on, to screen readers too.
        if badge.is_some() {
            tree.add(
                desk,
                Style {
                    width: Length::Px(8.0),
                    height: Length::Px(8.0),
                    grow: 0.0,
                    ..Style::default()
                },
                Kind::Custom {
                    draw: Box::new(crate::activity::BadgeDot(badge)),
                    label: desks.said.get(i).cloned(),
                },
            )
            .unwrap_or_else(MISSING);
        }
        let style = theme.text.label.with_weight(700).with_color(if here {
            Paint::Accent
        } else {
            Paint::Muted
        });
        let text = tree
            .add(
                desk,
                Style {
                    grow: 0.0,
                    ..Style::default()
                },
                Kind::text_line(name.clone(), style),
            )
            .unwrap_or_else(MISSING);
        tree.set_selected_box(text, here);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards(n: usize) -> Vec<Card> {
        (0..n)
            .map(|i| Card {
                title: format!("Window {i}"),
                color: hocus_ui::Color::rgb(200, 80, 80),
            })
            .collect()
    }

    #[test]
    fn the_grid_wraps_and_the_ring_follows() {
        let mut s = WindowSwitcher::new(
            Theme::default(),
            1.0,
            &cards(5),
            Some(1),
            900.0,
            Variant::Plain,
            None,
        );
        assert_eq!(s.columns_shown(), 3);
        let first = s.preview(0).expect("a preview");
        let fourth = s.preview(3).expect("a preview");
        assert!(fourth.y > first.y, "the fourth card is on the second row");
        s.select(Some(4));
        assert_eq!(s.selected, Some(4));
        assert!(s.access().dump().contains("Window 4"));
    }

    #[test]
    fn titles_are_typed_in_three_ms_a_character() {
        let mut s = WindowSwitcher::new(
            Theme::default(),
            1.0,
            &cards(2),
            Some(0),
            900.0,
            Variant::Matrix,
            None,
        );
        let since = Duration::from_secs(1);
        s.type_titles(since);
        s.draw(since + Duration::from_millis(9));
        assert!(
            s.access().dump().contains("\"Win\""),
            "{}",
            s.access().dump()
        );
        assert!(s.is_animating(since + Duration::from_millis(9)));
        s.draw(since + Duration::from_millis(40));
        assert!(s.access().dump().contains("Window 1"));
        assert!(!s.is_animating(since + Duration::from_millis(40)));
    }
}
