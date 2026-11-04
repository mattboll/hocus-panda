//! The clock's group as an item of the bar (Day.5): a click on the date or
//! the clock (the mini form's clock) opens the calendar
//! ([`crate::popover::calendar`]), a second click closes it. Its group is
//! lit `rgba(255,255,255,0.06)` under the pointer, which turns into a
//! hand, and `0.08` while the calendar is open, radius 10; ringed while it
//! holds the keyboard (after `Esc` in the calendar). Resting on it opens
//! nothing.
//!
//! The clock draws every second whatever it shows: its look is read as it
//! draws, and the pointer's comings and goings draw it once more.

use iced::{
    Point, Rectangle, Size, keyboard, mouse,
    widget::canvas::{self, Frame},
};

use super::{
    BIG, CELL_HEIGHT, Clock, DATE, DATE_GAP, DATE_LINE, FULL_TOP, MINI, SURFACE_TOP, State,
};
use crate::{
    levels::draw::{ACCENT, HOVER, OPEN, stroke},
    popover::{self, Event, Item, kit},
};
use hocus_link::{CLOCK_NAMESPACE, Form};

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
    at: Option<Point>,
    hover: bool,
    look: Look,
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

/// What the pointer and the keys do on the clock's surface.
pub fn update<Message>(
    clock: &Clock,
    state: &mut State,
    event: &iced::Event,
    bounds: Rectangle,
    cursor: mouse::Cursor,
) -> Option<canvas::Action<Message>> {
    match event {
        iced::Event::Mouse(seen) => {
            let item = &mut state.item;
            let at = crate::levels::draw::pointer(seen, bounds, cursor, item.at);
            item.at = at;
            let rect = clock.item_rect(bounds.size()).map(|(_, r)| r);
            let over = matches!((at, rect), (Some(at), Some(rect)) if rect.contains(at));
            match seen {
                mouse::Event::CursorEntered
                | mouse::Event::CursorMoved { .. }
                | mouse::Event::CursorLeft => {
                    if over != item.hover {
                        item.hover = over;
                        match rect.filter(|_| over) {
                            Some(rect) => popover::event(Event::ItemEnter {
                                bar: clock.bar.clone(),
                                item: Item::Calendar,
                                rect: clock.in_bar(rect),
                            }),
                            None => {
                                let at = at.unwrap_or(Point::new(bounds.width / 2.0, bounds.height));
                                popover::event(Event::ItemLeave {
                                    at: clock.in_bar(Rectangle::new(at, Size::ZERO)).position(),
                                });
                            }
                        }
                        return Some(canvas::Action::request_redraw());
                    }
                    if let Some(at) = at.filter(|_| !over) {
                        let at = clock.in_bar(Rectangle::new(at, Size::ZERO)).position();
                        popover::event(Event::Pointer { at });
                    }
                    None
                }
                mouse::Event::ButtonPressed(mouse::Button::Left) if over => {
                    if let Some(rect) = rect {
                        popover::event(Event::ItemPress {
                            bar: clock.bar.clone(),
                            item: Item::Calendar,
                            rect: clock.in_bar(rect),
                        });
                    }
                    Some(canvas::Action::request_redraw().and_capture())
                }
                mouse::Event::ButtonPressed(_) => {
                    popover::event(Event::PressElsewhere);
                    None
                }
                _ => None,
            }
        }
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            text,
            ..
        }) => popover::key(key, *modifiers, text.as_deref())
            .then(|| canvas::Action::request_redraw().and_capture()),
        _ => None,
    }
}

/// As the clock is about to draw: its look read, and its place told to
/// the popovers (for `Tab`, and where the calendar opens).
pub fn refresh(clock: &Clock, state: &mut State, size: Size) {
    let Some((form, rect)) = clock.item_rect(size) else {
        state.item.look = Look::default();
        return;
    };
    let in_bar = clock.in_bar(rect);
    let (open, focused) = popover::with(|c| {
        c.set_items(&clock.bar, CLOCK_NAMESPACE, vec![(Item::Calendar, in_bar)]);
        (
            c.shows(&clock.bar, Item::Calendar),
            c.focused(&clock.bar, Item::Calendar),
        )
    });
    let look = Look {
        hover: state.item.hover,
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
    let fill = if look.open {
        Some(OPEN)
    } else if look.hover {
        Some(HOVER)
    } else {
        None
    };
    if let Some(fill) = fill {
        frame.fill(&kit::rounded(rect, RADIUS), fill);
    }
    if look.focused {
        let inset = RING / 2.0;
        let ring = Rectangle::new(
            Point::new(rect.x + inset, rect.y + inset),
            Size::new(rect.width - RING, rect.height - RING),
        );
        frame.stroke(&kit::rounded(ring, RADIUS - inset), stroke(RING, ACCENT));
    }
}

/// The pointer's look: a hand over the group.
pub fn interaction(state: &State) -> mouse::Interaction {
    if state.item.hover {
        mouse::Interaction::Pointer
    } else {
        mouse::Interaction::default()
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
