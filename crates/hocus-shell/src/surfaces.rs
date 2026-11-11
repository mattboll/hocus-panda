//! A surface's state, owned by the application (#773).
//!
//! Each surface keeps what it shows in a value of its own, a field of
//! [`crate::Shell`], not in a global lock: what changes it comes to it as
//! messages ([`Surface::update`]), from the compositor's line, its widgets
//! or its workers (owned results through a channel), and its view is a
//! function of it ([`Surface::view`]). So a surface is tried alone: made,
//! told, and its view read, without a compositor.

use std::time::Instant;

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
