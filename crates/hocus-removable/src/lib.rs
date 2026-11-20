//! Removable drives, from UDisks2 on the system bus, for the bar (bar.md
//! "Removable drives", #898): USB sticks, external disks and SD cards,
//! their filesystems mounted, opened and ejected.
//!
//! - [`model`]: the drives as UDisks2 last said them, and which count.
//! - [`start`]: UDisks2 followed and asked on a thread of its own, never on
//!   a loop that draws: it says what changes ([`News`]) through the
//!   callback it is given, and takes what is asked ([`Ask`]) through a
//!   channel. Calls that wait (a mount, an unmount) run aside.
//!
//! No smithay, no toolkit.

pub mod model;
mod udisks;

pub use model::{Doing, Drive, Kind, News};

/// What is asked of UDisks2, by the drive's serial ([`Drive::serial`]).
#[derive(Debug)]
pub enum Ask {
    /// Mounted if it is not, then said where ([`News::Opened`]); a locked
    /// encrypted one is said locked ([`News::Locked`]).
    Open(u32),
    /// Every filesystem of its drive unmounted (an encrypted one locked
    /// again), then the drive powered off when it can be
    /// ([`News::Ejected`], or [`News::Busy`]).
    Eject(u32),
    /// The program ends.
    Quit(std::sync::mpsc::Sender<()>),
}

/// The channel what is asked goes through, to [`start`].
pub type Asks = tokio::sync::mpsc::UnboundedSender<Ask>;
pub type Incoming = tokio::sync::mpsc::UnboundedReceiver<Ask>;

/// A channel for [`start`].
pub fn channel() -> (Asks, Incoming) {
    tokio::sync::mpsc::unbounded_channel()
}

/// Follows UDisks2 on a thread of its own: `publish` is called with what
/// changed (the drives once UDisks2 is read, or found away, then each
/// time they change); what comes on `asks` is asked.
pub fn start(asks: Incoming, publish: impl FnMut(News) + Send + 'static) {
    udisks::start(asks, Box::new(publish));
}
