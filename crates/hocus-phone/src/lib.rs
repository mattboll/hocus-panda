//! The phone, through KDE Connect, for the bar (bar.md "The phone", #957):
//! the paired phones and tablets, their battery, what is asked of them,
//! the files sent to them and from them.
//!
//! - [`model`]: the devices as KDE Connect last said them, and which get a
//!   row.
//! - [`start`]: KDE Connect's daemon (`kdeconnectd`) followed and asked on
//!   the session bus the compositor names, on a thread of its own, never
//!   on a loop that draws: it says what changes ([`News`]) through the
//!   callback it is given, and takes what is asked ([`Ask`]) through a
//!   channel. Its calls run aside, 5 s at most each; nothing is polled.
//!   The thread also serves KDE's job view (`org.kde.JobViewServerV2`),
//!   which KDE Connect reports its transfers to.
//!
//! The daemon is never started from here: its name is D-Bus activatable,
//! so every call goes with `NO_AUTO_START`, and only once it has an owner.
//!
//! No smithay, no toolkit: the Settings may share it.

mod jobs;
mod kdeconnect;
pub mod model;

pub use model::{Battery, Device, Kind, News, Phones, Transfer};

/// What is asked of KDE Connect, by the device's id ([`Device::id`]).
#[derive(Debug)]
pub enum Ask {
    /// "Faire sonner": the phone rings until stopped on it.
    Ring(String),
    /// Files sent to it, as `file://` URLs.
    Share { device: String, urls: Vec<String> },
    /// A text put in its clipboard.
    Text { device: String, text: String },
    /// "Oublier": unpaired.
    Forget(String),
    /// The user closed a notification of the device named `device` (its
    /// `x-kdeconnect-source-device`): the phone's own, of the same
    /// application, title and text, the newest first, is dismissed there
    /// when it lets it go.
    Dismiss {
        device: String,
        app: String,
        title: String,
        text: String,
    },
    /// A transfer's ✕: its view asks KDE Connect to stop it.
    Cancel(u32),
    /// A stopped transfer of the device was said: let go.
    Settle(String),
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

/// Follows KDE Connect on the bus at `bus` (an address, or `session`) on a
/// thread of its own: `publish` is called with what changed (KDE Connect
/// once read, or found away, then each time it changes); what comes on
/// `asks` is asked. Without a bus, it says once that there is nothing.
pub fn start(bus: Option<String>, asks: Incoming, publish: impl FnMut(News) + Send + 'static) {
    kdeconnect::start(bus, asks, Box::new(publish));
}
