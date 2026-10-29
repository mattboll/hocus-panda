//! What a popover's canvas sees: the pointer, the keys, and the order the
//! keyboard goes through what can hold it.

use std::time::{Duration, Instant};

use iced::{Point, Rectangle, keyboard, mouse, window};

use crate::popover::{Event as Seen, Pressed, event};

/// What a popover's canvas saw, as it handles it.
pub(crate) enum Input<'a> {
    /// It is about to draw, at `now`: what it shows is read afresh.
    Redraw(Instant),
    /// The pointer, at a place on it (none: it left).
    Mouse(&'a mouse::Event, Option<Point>),
    Key(Pressed),
    Nothing,
}

/// `event` on the canvas of popover `id` at `bounds`, the pointer where it
/// is as of it: only drawing while the popover fades out.
pub(crate) fn input<'a, T>(
    id: window::Id,
    event: &'a iced::Event,
    bounds: Rectangle,
    cursor: mouse::Cursor,
    pointer: &mut Pointer<T>,
) -> Input<'a> {
    match event {
        iced::Event::Window(window::Event::RedrawRequested(now)) => Input::Redraw(*now),
        _ if !crate::popover::with(|c| c.is_open(id)) => Input::Nothing,
        iced::Event::Mouse(event) => {
            let at = crate::levels::draw::pointer(event, bounds, cursor, pointer.at);
            pointer.at = at;
            Input::Mouse(event, at)
        }
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            text,
            ..
        }) => Input::Key(Pressed {
            key: key.clone(),
            modifiers: *modifiers,
            text: text.as_deref().map(str::to_owned),
        }),
        _ => Input::Nothing,
    }
}

/// The keys for popover `id` that reached another of the shell's surfaces
/// (see `popover::key`), while it is open.
pub(crate) fn keys(id: window::Id) -> Vec<Pressed> {
    let keys = crate::popover::take_keys();
    if !keys.is_empty() && crate::popover::with(|c| c.is_open(id)) {
        keys
    } else {
        Vec::new()
    }
}

/// The pointer on a popover: where it is, what it is over and what it
/// pressed. The controller hears as it comes in and as it leaves.
#[derive(Debug)]
pub(crate) struct Pointer<T> {
    pub at: Option<Point>,
    inside: bool,
    pub hover: Option<T>,
    pub pressed: Option<T>,
}

impl<T> Default for Pointer<T> {
    fn default() -> Self {
        Self {
            at: None,
            inside: false,
            hover: None,
            pressed: None,
        }
    }
}

impl<T: Copy + PartialEq> Pointer<T> {
    /// It moved, or came in: the controller hears that it came in.
    pub(crate) fn moved(&mut self) {
        if !self.inside && self.at.is_some() {
            self.inside = true;
            event(Seen::PopoverEnter);
        }
    }

    /// It left: nothing is under it.
    pub(crate) fn left(&mut self) {
        self.inside = false;
        event(Seen::PopoverLeave);
        self.hover = None;
    }

    /// It is over `target`: whether that changed.
    pub(crate) fn over(&mut self, target: Option<T>) -> bool {
        let changed = self.hover != target;
        self.hover = target;
        changed
    }

    /// The button released over `target`: what it clicked, pressed and
    /// released on it.
    pub(crate) fn release(&mut self, target: Option<T>) -> Option<T> {
        self.pressed.take().filter(|p| Some(*p) == target)
    }

    /// The pointer's look: grabbing while a slider is dragged, a hand over
    /// what can be acted on.
    pub(crate) fn interaction(&self, dragging: bool) -> mouse::Interaction {
        if dragging {
            mouse::Interaction::Grabbing
        } else if self.hover.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

/// What holds the keyboard after `focus` in `order` (`back`: before it),
/// round its ends; from nothing, the first (the last). Nothing when `order`
/// is empty.
pub(crate) fn step<T: Copy + PartialEq>(order: &[T], focus: Option<T>, back: bool) -> Option<T> {
    let n = order.len() as isize;
    if n == 0 {
        return None;
    }
    let at = focus.and_then(|f| order.iter().position(|t| *t == f));
    let by = if back { -1 } else { 1 };
    let next = match at {
        Some(i) => (i as isize + by).rem_euclid(n),
        None if by > 0 => 0,
        None => n - 1,
    };
    order.get(next as usize).copied()
}

/// The row after `current` in `rows` (`forward`; else before it), round
/// its ends, as the arrows go: nothing if it is not one of them.
pub(crate) fn cycle<T: Copy + PartialEq>(rows: &[T], current: T, forward: bool) -> Option<T> {
    let at = rows.iter().position(|r| *r == current)?;
    let n = rows.len();
    let next = if forward {
        (at + 1) % n
    } else {
        (at + n - 1) % n
    };
    rows.get(next).copied()
}

/// What was asked, shown until it is said (`said`) or `long` has passed
/// since it was asked: until when it still shows, if it does.
pub(crate) fn awaited<T>(
    asked: &mut Option<(T, Instant)>,
    said: bool,
    now: Instant,
    long: Duration,
) -> Option<Instant> {
    let until = asked.as_ref()?.1 + long;
    if said || now >= until {
        *asked = None;
        None
    } else {
        Some(until)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keyboard_goes_round_the_order_and_the_arrows_round_the_rows() {
        let order = [1, 2, 3];
        assert_eq!(step(&order, None, false), Some(1));
        assert_eq!(step(&order, None, true), Some(3));
        assert_eq!(step(&order, Some(3), false), Some(1));
        assert_eq!(step(&order, Some(1), true), Some(3));
        // Gone from the order: from the start again.
        assert_eq!(step(&order, Some(7), false), Some(1));
        assert_eq!(step::<u8>(&[], None, false), None);
        assert_eq!(cycle(&order, 3, true), Some(1));
        assert_eq!(cycle(&order, 1, false), Some(3));
        assert_eq!(cycle(&order, 7, true), None);
    }

    #[test]
    fn what_was_asked_shows_until_it_is_said_or_a_while() {
        let t = Instant::now();
        let long = Duration::from_secs(1);
        let mut asked = Some((5, t));
        assert_eq!(awaited(&mut asked, false, t, long), Some(t + long));
        assert!(asked.is_some());
        assert_eq!(awaited(&mut asked, false, t + long, long), None);
        assert!(asked.is_none());
        let mut asked = Some((5, t));
        assert_eq!(awaited(&mut asked, true, t, long), None);
        assert!(asked.is_none());
    }
}
