//! The clock's group as an item of the bar (Day.5): a click on the date or
//! the clock (the mini form's clock) opens the calendar
//! ([`crate::popover::calendar`]), a second click closes it. Its group is
//! lit `rgba(255,255,255,0.06)` under the pointer, which turns into a
//! hand, and `0.08` while the calendar is open, radius 10; ringed while it
//! holds the keyboard (after `Esc` in the calendar). Resting on it opens
//! nothing.
//!
//! The clock's surface lets the pointer through to the bar's own, which
//! hears it for the group ([`crate::popover::BarPointer`]) as the clock
//! tells it where the group is. The clock draws every second whatever it
//! shows: its look is read as it draws, and the pointer's comings and
//! goings wake it once more.

use std::{
    collections::HashSet,
    sync::{Mutex, PoisonError},
};

use hocus_link::{CLOCK_NAMESPACE, Form};
use iced::{Point, Rectangle, Size, widget::canvas::Frame};

use super::{
    BIG, CELL_HEIGHT, Clock, DATE, DATE_GAP, DATE_LINE, FULL_TOP, MINI, SURFACE_TOP, State,
};
use crate::{
    levels::draw::{ACCENT, HOVER, OPEN, stroke},
    popover::{self, Item, kit},
};

/// The full form's group: the bar's inside less its 12 px sides, from the
/// digits' cells to 6 px under the date.
const FULL_WIDTH: f32 = 172.0;
/// The mini form's: 48 px, to 6 px under the minutes.
const MINI_WIDTH: f32 = 48.0;
const BELOW: f32 = 6.0;
const RADIUS: f32 = 10.0;
/// The keyboard's ring, inside the group.
const RING: f32 = 2.0;

/// How the group looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Look {
    pub hover: bool,
    pub open: bool,
    pub focused: bool,
}

/// The item's state between the clock's draws.
#[derive(Debug, Default)]
pub struct ItemState {
    look: Look,
}

/// The bars whose clock's group has the pointer on it.
static HOVERED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// The pointer came onto bar `bar`'s group (`on`), or left it: its clock
/// draws it so at once.
pub fn hover(bar: &str, on: bool) {
    let changed = {
        let mut hovered = HOVERED.lock().unwrap_or_else(PoisonError::into_inner);
        let hovered = hovered.get_or_insert_with(HashSet::new);
        if on {
            hovered.insert(bar.to_owned())
        } else {
            hovered.remove(bar)
        }
    };
    if changed {
        popover::send(vec![popover::Effect::Wake(CLOCK_NAMESPACE)]);
    }
}

fn hovered(bar: &str) -> bool {
    HOVERED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_some_and(|h| h.contains(bar))
}

/// The group's height in `form`, from its top.
pub fn height(form: Form) -> f32 {
    match form {
        Form::Full => BIG * CELL_HEIGHT + DATE_GAP + DATE * DATE_LINE + BELOW,
        Form::Mini => 2.0 * MINI * CELL_HEIGHT + BELOW,
    }
}

impl Clock {
    /// The group on the clock's surface, `size` large: while the bar
    /// rests in one form only.
    fn item_rect(&self, size: Size) -> Option<(Form, Rectangle)> {
        let [(form, _)] = self.forms[..] else {
            return None;
        };
        let right = self.surface_width;
        let centre = (right + 2.0 - self.bar_width + right) / 2.0;
        let (width, top) = match form {
            Form::Full => (FULL_WIDTH, FULL_TOP),
            Form::Mini => (MINI_WIDTH, 0.0),
        };
        let rect = Rectangle::new(
            Point::new(centre - width / 2.0, top),
            Size::new(width, height(form)),
        );
        rect.intersection(&Rectangle::with_size(size))
            .map(|_| (form, rect))
    }

    /// `rect` on the surface, in the bar's frame (from its outer right
    /// edge, a pixel right of the surface's, and from its top).
    fn in_bar(&self, rect: Rectangle) -> Rectangle {
        Rectangle::new(
            Point::new(
                rect.x - self.surface_width - 1.0,
                rect.y + SURFACE_TOP as f32,
            ),
            rect.size(),
        )
    }
}

/// The clock's group as screen readers read it: the time and the date
/// `shown`, in `lang`; a click opens the calendar.
pub fn node(lang: crate::clock::time::Lang, shown: &crate::clock::time::Shown) -> hocus_a11y::Node {
    hocus_a11y::Node::new(
        crate::ui::items::key(Item::Calendar),
        hocus_a11y::Role::Button,
        crate::a11y::Words::new(lang).clock(shown),
    )
    .act(hocus_a11y::Act::Click)
}

/// As the clock is about to draw: its look read, and its place told to
/// the popovers (for the pointer on the bar, `Tab`, and where the
/// calendar opens). While the form switches, it has none.
pub fn refresh(clock: &Clock, state: &mut State, size: Size) {
    let place = clock.item_rect(size);
    let in_bar = place.map(|(_, rect)| clock.in_bar(rect));
    let spots = in_bar
        .map(|r| crate::ui::ring::Spot {
            node: Some(node(
                clock.lang,
                &crate::clock::time::Shown::at(&crate::notifications::wall(), clock.lang),
            )),
            ..crate::ui::ring::Spot::at(Item::Calendar, r)
        })
        .into_iter()
        .collect();
    let declared = crate::ui::ring::Declared { spots, group: None };
    let (open, focused) = popover::with(|c| {
        c.declare(&clock.bar, CLOCK_NAMESPACE, declared);
        (
            c.shows(&clock.bar, Item::Calendar),
            c.focused(&clock.bar, Item::Calendar),
        )
    });
    let (Some((form, _)), Some(in_bar)) = (place, in_bar) else {
        state.item.look = Look::default();
        return;
    };
    let look = Look {
        hover: hovered(&clock.bar),
        open,
        focused,
    };
    if look != state.item.look {
        tracing::debug!(
            bar = clock.bar,
            form = form.name(),
            ?look,
            at = ?(in_bar.x, in_bar.y, in_bar.width, in_bar.height),
            "the clock's group is drawn"
        );
        state.item.look = look;
    }
}

/// The group lit as it looks, under the digits.
pub fn draw(clock: &Clock, state: &State, frame: &mut Frame, size: Size) {
    let Some((_, rect)) = clock.item_rect(size) else {
        return;
    };
    let look = state.item.look;
    if look.open {
        frame.fill(&kit::rounded(rect, RADIUS), OPEN.get());
    } else if look.hover {
        crate::theme::hover(frame, &kit::rounded(rect, RADIUS), HOVER.get());
    }
    if look.focused {
        let inset = RING / 2.0;
        let ring = Rectangle::new(
            Point::new(rect.x + inset, rect.y + inset),
            Size::new(rect.width - RING, rect.height - RING),
        );
        frame.stroke(
            &kit::rounded(ring, RADIUS - inset),
            stroke(RING, ACCENT.get()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{FULL_SURFACE, MINI_SURFACE};

    #[test]
    fn the_group_fits_its_surface_in_both_forms() {
        // From the digits' cells to 6 px under the date (or the minutes).
        assert!(FULL_TOP + height(Form::Full) <= FULL_SURFACE.1 as f32);
        assert!(height(Form::Mini) <= MINI_SURFACE.1 as f32);
        assert!(FULL_WIDTH <= FULL_SURFACE.0 as f32);
        assert!(MINI_WIDTH <= MINI_SURFACE.0 as f32);
        // Its top is the calendar's: 22 px from the screen's top in the
        // full form (the bar 8 px down).
        assert_eq!(8.0 + SURFACE_TOP as f32 + FULL_TOP, 22.0);
    }
}
