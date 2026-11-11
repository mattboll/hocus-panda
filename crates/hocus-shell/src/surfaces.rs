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

use std::{
    cell::RefCell,
    sync::{Mutex, PoisonError},
    time::Instant,
};

use iced::{Element, futures::channel::mpsc::UnboundedSender, window};

use crate::popover::Effect;

/// What every surface's view is built with: how they look, the clock and
/// the language.
#[derive(Debug, Clone)]
pub struct Context {
    pub look: Look,
    pub wall: crate::clock::time::Wall,
    pub lang: crate::clock::time::Lang,
}

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
    fn view(&self, id: window::Id, context: &Context) -> Option<Element<'_, crate::Message>>;
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

/// What another thread asks of a surface, or tells it (a screen reader's
/// click, polkit's request, the helper's answer), as a message for the
/// loop ([`ask`], [`inbox`]).
#[derive(Debug, Clone)]
pub enum Asked {
    Share(crate::share::Act),
    Auth(crate::auth::Come),
}

/// The loop's end of [`ask`]: a channel's end, made once, not state.
static ASKS: Mutex<Option<UnboundedSender<Asked>>> = Mutex::new(None);

/// `asked` for the loop, from any thread.
pub fn ask(asked: Asked) {
    let sent = ASKS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_some_and(|asks| asks.unbounded_send(asked).is_ok());
    if !sent {
        tracing::debug!("asked of a surface before the shell runs: dropped");
    }
}

/// What other threads ask of the surfaces, as messages.
pub fn inbox() -> impl iced::futures::Stream<Item = crate::Message> {
    let (asks, asked) = iced::futures::channel::mpsc::unbounded();
    *ASKS.lock().unwrap_or_else(PoisonError::into_inner) = Some(asks);
    iced::futures::StreamExt::map(asked, crate::Message::Surface)
}
