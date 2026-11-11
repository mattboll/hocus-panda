//! A surface's state, owned by the application (#773).
//!
//! Each surface keeps what it shows in a value of its own, a field of
//! [`crate::Shell`], not in a global lock: what changes it comes to it as
//! messages ([`Surface::update`]), from the compositor's line, its widgets
//! or its workers (owned results through a channel), and its view is a
//! function of it ([`Surface::view`]). So a surface is tried alone: made,
//! told, and its view read, without a compositor.
//!
//! The widget kit's surfaces ([`crate::ui::Around`]) handle what their
//! widgets ask at once, while the view lives, without a message (which
//! would build every surface's view again): their state is [`Lent`] to the
//! view. Another thread (the screen readers') asks through [`Asked`], a
//! message for the loop.

use std::{cell::RefCell, time::Instant};

use iced::{Element, window};

use crate::popover::Effect;

/// How the shell's surfaces look, as the compositor says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    /// The compositor blurs behind: the surfaces are a tint over it.
    pub glass: bool,
    /// Things move (the bar's form switch, a panda's breath).
    pub motion: bool,
}

/// One of the shell's surfaces, or a family of them (a bar's on every
/// screen), and what it shows.
pub trait Surface {
    /// What changes it.
    type Message;

    /// `message` taken in at `now`: what the shell does for it (a surface
    /// opened, removed, drawn again).
    fn update(&mut self, message: Self::Message, now: Instant) -> Vec<Effect>;

    /// What it shows on surface `id`, if `id` is one of its own.
    fn view(&self, id: window::Id, look: Look) -> Option<Element<'_, crate::Message>>;
}

/// A surface's state, lent to its view's widgets, which change it as they
/// handle what they ask: on the loop's thread alone, no lock. Taken twice
/// at once (a bug), it is said in the log and nothing is done.
#[derive(Debug, Default)]
pub struct Lent<T>(RefCell<T>);

impl<T> Lent<T> {
    pub fn get_mut(&mut self) -> &mut T {
        self.0.get_mut()
    }

    /// `read` on the state, unless it is being changed.
    pub fn read<R>(&self, read: impl FnOnce(&T) -> R) -> Option<R> {
        match self.0.try_borrow() {
            Ok(state) => Some(read(&state)),
            Err(err) => {
                tracing::error!(%err, "a surface's state read while it changes");
                None
            }
        }
    }

    /// `change` on the state, unless it is being read or changed.
    pub fn change<R>(&self, change: impl FnOnce(&mut T) -> R) -> Option<R> {
        match self.0.try_borrow_mut() {
            Ok(mut state) => Some(change(&mut state)),
            Err(err) => {
                tracing::error!(%err, "a surface's state changed while in use");
                None
            }
        }
    }
}

/// What another thread asks of a surface, as a message for the loop (a
/// screen reader's click).
#[derive(Debug, Clone, PartialEq)]
pub enum Asked {
    Share(crate::share::Act),
}
