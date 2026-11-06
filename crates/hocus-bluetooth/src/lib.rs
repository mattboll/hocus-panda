//! Bluetooth, from BlueZ on the system bus, for the bar's popover (bar.md
//! §9) and the Settings' Bluetooth page (settings.md "Bluetooth") alike.
//!
//! - [`model`]: the adapter and the devices as BlueZ last said them.
//! - [`start`]: BlueZ followed and asked on a thread of its own, never on
//!   a loop that draws: it says each change of what is shown through the
//!   callback it is given, and takes what is asked ([`Ask`]) through a
//!   channel. Calls that wait on a device run aside.
//!
//! No smithay, no toolkit.

mod bluez;
pub mod model;

pub use model::{Action, Attempt, Bluetooth, Device, Failure, Kind, Row, Step};

/// What is asked of BlueZ, by device path.
#[derive(Debug)]
pub enum Ask {
    /// The adapter on or off.
    Power(bool),
    /// Look for devices around (while the bar's popover is open, the
    /// Settings' page shows), or stop.
    Discover(bool),
    /// The computer visible to other devices and pairable (while the
    /// Settings' page shows), or hidden again.
    Visible(bool),
    Connect(String),
    Disconnect(String),
    /// Pair, trust, then connect.
    Pair(String),
    /// The program ends: stop looking, hide the computer, then say so.
    Quit(std::sync::mpsc::Sender<()>),
}

/// How the program asking pairs.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Its agent's capability, as BlueZ's `RegisterAgent` takes it.
    pub capability: &'static str,
}

impl Default for Options {
    /// The bar's: pairing without a code only.
    fn default() -> Self {
        Self {
            capability: "NoInputNoOutput",
        }
    }
}

/// The channel what is asked goes through, to [`start`].
pub type Asks = tokio::sync::mpsc::UnboundedSender<Ask>;
pub type Incoming = tokio::sync::mpsc::UnboundedReceiver<Ask>;

/// A channel for [`start`].
pub fn channel() -> (Asks, Incoming) {
    tokio::sync::mpsc::unbounded_channel()
}

/// Follows BlueZ on a thread of its own: `publish` is called with what is
/// shown each time it changes (and once BlueZ is read, or found away);
/// what comes on `asks` is asked.
pub fn start(asks: Incoming, options: Options, publish: impl FnMut(Bluetooth) + Send + 'static) {
    bluez::start(asks, options, Box::new(publish));
}
