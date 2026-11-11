//! The keyboard's ring across a bar's surfaces (bar.md §11, #785): the
//! controls each surface of the bar declares as it draws, in the bar's
//! frame, and the one holding the ring.
//!
//! A popover's ring stays within its surface ([`super::keys`]); a bar's
//! goes from a surface to the next (the desktops' tiles, the agents, the
//! levels…), each a layer surface of its own that draws alone. So the
//! surfaces declare their controls here ([`super::items`] for those on the
//! kit), and `Tab`, the arrows, `Home` and `End` go through the
//! declarations, from the bar's top to its bottom and along a row; the
//! popovers' controller moves the keyboard from a surface to the next as
//! the ring does. Screen readers read the same declarations
//! ([`crate::a11y`]), and the magnifier is told where the ring is.

use std::collections::HashMap;

use hocus_a11y::Node;
use iced::{
    Color, Point, Rectangle, Size,
    widget::canvas::{Frame, Path, Stroke, Style},
};

use crate::popover::Item;

/// The ring's width and how far out of its control it is drawn.
pub const RING: f32 = 2.0;
pub const RING_OUT: f32 = 3.0;

/// A control a bar's surface declares: what it is to the popovers'
/// controller, where it is (the bar's frame), and its node for screen
/// readers (none from a surface that names its items in `a11y::bar`).
#[derive(Debug, Clone, PartialEq)]
pub struct Spot {
    pub item: Item,
    pub rect: Rectangle,
    pub node: Option<Node>,
    /// A part of the control before it (a window's line's ✕): its node is
    /// that one's child.
    pub part: bool,
}

impl Spot {
    /// `item` at `rect`, its node said elsewhere.
    pub fn at(item: Item, rect: Rectangle) -> Self {
        Self {
            item,
            rect,
            node: None,
            part: false,
        }
    }
}

/// How a surface names its item as screen readers read it, from what is
/// now: for what its drawing does not show, which changes without it
/// drawing (a desktop's windows' titles, how long an agent has waited).
pub type Live = fn(bar: &str, words: crate::a11y::Words, item: Item) -> Option<Node>;

/// What a bar's surface declares: its controls, the group they make for
/// screen readers (a list, a toolbar), if they make one, and how its
/// items are named as they are read, if their nodes as drawn would age.
#[derive(Debug, Clone, Default)]
pub struct Declared {
    pub spots: Vec<Spot>,
    pub group: Option<Node>,
    pub live: Option<Live>,
}

/// The ring across the bars: what each of their surfaces declares, by
/// bar and surface's namespace, and the control holding the ring, on its
/// bar, where it was when it took it.
#[derive(Debug, Default)]
pub struct Ring {
    focused: Option<(String, Item, Rectangle)>,
    declared: HashMap<(String, &'static str), Declared>,
}

impl Ring {
    /// Bar `bar`'s surface of `namespace` shows `declared`. The control
    /// under the ring went (an agent ended, a desktop was removed): the
    /// ring goes to the one now in its place on that surface, else the one
    /// before it. Whether it moved so.
    pub fn declare(&mut self, bar: &str, namespace: &'static str, mut declared: Declared) -> bool {
        declared.spots.sort_by(|a, b| a.rect.y.total_cmp(&b.rect.y));
        let key = (bar.to_owned(), namespace);
        let mut moved = false;
        if let Some((b, item, at)) = &mut self.focused
            && b == bar
            && self
                .declared
                .get(&key)
                .is_some_and(|d| d.spots.iter().any(|s| s.item == *item))
            && !declared.spots.iter().any(|s| s.item == *item)
            && let Some(next) = declared
                .spots
                .iter()
                .find(|s| s.rect.y >= at.y)
                .or_else(|| declared.spots.last())
        {
            tracing::debug!(gone = ?item, item = ?next.item, "the keyboard on an item");
            (*item, *at) = (next.item, next.rect);
            moved = true;
        }
        self.declared.insert(key, declared);
        moved
    }

    /// Bar `bar`'s surface of `namespace` went; the ring with it if it was
    /// on it and `ring` says so.
    pub fn forget(&mut self, bar: &str, namespace: &'static str, ring: bool) {
        let gone = self.declared.remove(&(bar.to_owned(), namespace));
        if ring
            && let (Some(gone), Some((b, item, _))) = (gone, &self.focused)
            && b == bar
            && gone.spots.iter().any(|s| s.item == *item)
        {
            self.focused = None;
        }
    }

    /// Bar `bar` went.
    pub fn forget_bar(&mut self, bar: &str) {
        self.declared.retain(|(b, _), _| b != bar);
    }

    /// The control holding the ring: its bar, its item, where it was.
    pub fn holder(&self) -> Option<&(String, Item, Rectangle)> {
        self.focused.as_ref()
    }

    /// Whether `item` of `bar` holds the ring.
    pub fn holds(&self, bar: &str, item: Item) -> bool {
        self.focused
            .as_ref()
            .is_some_and(|(b, i, _)| b == bar && *i == item)
    }

    /// The ring onto `item` of `bar`, at `rect`.
    pub fn put(&mut self, bar: &str, item: Item, rect: Rectangle) {
        tracing::debug!(?item, "the keyboard on an item");
        self.focused = Some((bar.to_owned(), item, rect));
    }

    /// The ring goes, from where it was.
    pub fn take(&mut self) -> Option<(String, Item, Rectangle)> {
        self.focused.take()
    }

    /// Where `item` of `bar` was declared, on the surface of `namespace`.
    pub fn find(&self, bar: &str, namespace: &'static str, item: Item) -> Option<Rectangle> {
        self.declared
            .get(&(bar.to_owned(), namespace))?
            .spots
            .iter()
            .find(|s| s.item == item)
            .map(|s| s.rect)
    }

    /// The top of what bar `bar`'s surface of `namespace` declares.
    pub fn top_of(&self, bar: &str, namespace: &'static str) -> Option<f32> {
        self.declared
            .get(&(bar.to_owned(), namespace))?
            .spots
            .iter()
            .map(|s| s.rect.y)
            .reduce(f32::min)
    }

    /// Bar `bar`'s controls, each with its surface's namespace, in the
    /// drawing's order: top to bottom, and along a row left to right.
    pub fn spots(&self, bar: &str) -> Vec<(&'static str, &Spot)> {
        let mut spots: Vec<(&'static str, &Spot)> = self
            .declared
            .iter()
            .filter(|((b, _), _)| b == bar)
            .flat_map(|((_, namespace), d)| d.spots.iter().map(|s| (*namespace, s)))
            .collect();
        spots.sort_by(|a, b| {
            a.1.rect
                .y
                .total_cmp(&b.1.rect.y)
                .then(a.1.rect.x.total_cmp(&b.1.rect.x))
        });
        spots
    }

    /// How bar `bar`'s surface of `namespace` names its items as they are
    /// read, if it does.
    pub fn live(&self, bar: &str, namespace: &'static str) -> Option<Live> {
        self.declared.get(&(bar.to_owned(), namespace))?.live
    }

    /// The group bar `bar`'s surface of `namespace` declared, if any.
    #[allow(
        dead_code,
        reason = "read by screen readers once a surface declares one"
    )]
    pub fn group(&self, bar: &str, namespace: &'static str) -> Option<&Node> {
        self.declared
            .get(&(bar.to_owned(), namespace))?
            .group
            .as_ref()
    }

    /// Bar `bar`'s items and where they are, in the drawing's order.
    pub fn items(&self, bar: &str) -> Vec<(Item, Rectangle)> {
        self.spots(bar)
            .into_iter()
            .map(|(_, s)| (s.item, s.rect))
            .collect()
    }

    /// Where `Tab` takes the ring (`Shift+Tab`, `back`): the next control
    /// of its bar, from its top to its bottom and round. One gone
    /// meanwhile (an agent that ended) steps from where it was.
    pub fn step(&self, back: bool) -> Option<(String, Item, Rectangle)> {
        let (bar, item, at) = self.focused.clone()?;
        let items = self.items(&bar);
        let order: Vec<Item> = items.iter().map(|(i, _)| *i).collect();
        let next = if order.contains(&item) {
            crate::popover::kit::step(&order, Some(item), back)
        } else {
            // From where it was: the first under it, or the last over it,
            // round the ends.
            let after = items.iter().position(|(_, r)| r.y >= at.y);
            match (after, back) {
                (Some(i), false) => order.get(i).copied(),
                (Some(i), true) => i
                    .checked_sub(1)
                    .and_then(|i| order.get(i))
                    .or(order.last())
                    .copied(),
                (None, false) => order.first().copied(),
                (None, true) => order.last().copied(),
            }
        }?;
        let rect = items.iter().find(|(i, _)| *i == next)?.1;
        Some((bar, next, rect))
    }

    /// `Home` or `End` (`last`): its bar's first control, or its last.
    pub fn edge(&self, last: bool) -> Option<(String, Item, Rectangle)> {
        let (bar, _, _) = self.focused.clone()?;
        let items = self.items(&bar);
        let &(item, rect) = if last { items.last() } else { items.first() }?;
        Some((bar, item, rect))
    }

    /// `←` or `→` (`right`): the control beside along the same row (the
    /// desktops' tiles, the footer's buttons, the tray's icons), stopping
    /// at its ends.
    pub fn side(&self, right: bool) -> Option<(String, Item, Rectangle)> {
        let (bar, item, at) = self.focused.clone()?;
        let items = self.items(&bar);
        let middle = at.center_y();
        let mut row: Vec<&(Item, Rectangle)> = items
            .iter()
            .filter(|(_, r)| r.y <= middle && middle <= r.y + r.height)
            .collect();
        row.sort_by(|a, b| a.1.x.total_cmp(&b.1.x));
        let here = row.iter().position(|(i, _)| *i == item)?;
        let next = if right {
            row.get(here + 1)
        } else {
            here.checked_sub(1).and_then(|i| row.get(i))
        };
        next.map(|&&(item, rect)| (bar, item, rect))
    }

    /// Where the ring lands coming into bar `bar` (`Super+B`): on the
    /// first of `wanted` it shows, else on `then`, else on its first
    /// control.
    pub fn landing(&self, bar: &str, wanted: &[Item], then: Item) -> Option<(Item, Rectangle)> {
        let items = self.items(bar);
        let find = |want: Item| items.iter().find(|(i, _)| *i == want);
        wanted
            .iter()
            .find_map(|w| find(*w))
            .or_else(|| find(then))
            .or_else(|| items.first())
            .copied()
    }
}

/// The ring around `rect` (bar.md §11), drawn on a canvas: 2 px of the
/// accent as text, 3 px out, its radius the control's and as much again.
/// Under high contrast, 3 px, 2 px out, the background's colour between:
/// the same room.
pub fn draw(f: &mut Frame, rect: Rectangle, radius: f32) {
    let colour = crate::theme::ACCENT_TEXT.get();
    if crate::theme::contrast() {
        let gap = RING_OUT - 1.0;
        let back = crate::theme::Ink::Token(|t| t.popover_opaque_bottom).get();
        stroke_around(f, rect, radius, 0.0, gap, back);
        stroke_around(f, rect, radius, gap, RING + 1.0, colour);
    } else {
        stroke_around(f, rect, radius, RING_OUT, RING, colour);
    }
}

/// A stroke `width` wide around `rect`, `out` px from it.
fn stroke_around(f: &mut Frame, rect: Rectangle, radius: f32, out: f32, width: f32, colour: Color) {
    let middle = out + width / 2.0;
    let path = Path::rounded_rectangle(
        Point::new(rect.x - middle, rect.y - middle),
        Size::new(rect.width + 2.0 * middle, rect.height + 2.0 * middle),
        (radius + middle).into(),
    );
    f.stroke(
        &path,
        Stroke {
            style: Style::Solid(colour),
            width,
            ..Stroke::default()
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: &str = "eDP-1";

    fn at(x: f32, y: f32) -> Rectangle {
        Rectangle::new(Point::new(x, y), Size::new(30.0, 20.0))
    }

    fn declared(spots: &[(Item, Rectangle)]) -> Declared {
        Declared {
            spots: spots.iter().map(|(i, r)| Spot::at(*i, *r)).collect(),
            ..Declared::default()
        }
    }

    #[test]
    fn the_ring_goes_through_every_surfaces_controls_top_to_bottom_and_round() {
        let mut ring = Ring::default();
        let tiles = [
            (Item::Desktop { row: 0, index: 0 }, at(-180.0, 300.0)),
            (Item::Desktop { row: 0, index: 1 }, at(-140.0, 300.0)),
        ];
        ring.declare(BAR, "desktops", declared(&tiles));
        ring.declare(
            BAR,
            "clock",
            declared(&[(Item::Calendar, at(-180.0, 10.0))]),
        );
        ring.declare(BAR, "levels", declared(&[(Item::Sound, at(-180.0, 900.0))]));
        ring.put(BAR, Item::Calendar, at(-180.0, 10.0));
        let mut seen = Vec::new();
        for _ in 0..4 {
            let (bar, item, rect) = ring.step(false).expect("a next control");
            ring.put(&bar, item, rect);
            seen.push(item);
        }
        assert_eq!(seen, [tiles[0].0, tiles[1].0, Item::Sound, Item::Calendar]);
        // Along the row, stopping at its end.
        ring.put(BAR, tiles[0].0, tiles[0].1);
        assert_eq!(ring.side(true).map(|s| s.1), Some(tiles[1].0));
        ring.put(BAR, tiles[1].0, tiles[1].1);
        assert_eq!(ring.side(true), None);
        assert_eq!(ring.edge(true).map(|s| s.1), Some(Item::Sound));
    }

    #[test]
    fn the_control_under_the_ring_gone_the_ring_takes_the_one_in_its_place() {
        let mut ring = Ring::default();
        let rows = [
            (Item::Agent(1), at(-180.0, 100.0)),
            (Item::Agent(2), at(-180.0, 130.0)),
        ];
        ring.declare(BAR, "agents", declared(&rows));
        ring.put(BAR, Item::Agent(1), rows[0].1);
        assert!(ring.declare(
            BAR,
            "agents",
            declared(&[(Item::Agent(2), at(-180.0, 100.0))])
        ));
        assert!(ring.holds(BAR, Item::Agent(2)));
        // Its surface gone, the ring with it.
        ring.forget(BAR, "agents", true);
        assert!(ring.holder().is_none());
    }
}
