//! The network, for the bar's Wi-Fi row and popover (bar.md §4 group 9,
//! §5) and the Settings' Réseau page alike (settings.md "Réseau"): both
//! read NetworkManager the same way, each on a thread of its own.
//!
//! - [`nm`]: NetworkManager on the system bus, read whole then followed by
//!   its signals (nothing polled), and what is asked of it ([`Ask`]); what
//!   it says goes to a callback, as the bar's row ([`model::Network`]) or
//!   the page's details ([`model::Details`]).
//! - [`rfkill`]: the radios' switches (`/dev/rfkill`), for airplane mode.
//! - [`secret`]: a password as it is typed, wiped when it goes.
//!
//! No smithay, no toolkit.

pub mod model;
pub mod nm;
pub mod rfkill;
pub mod secret;

use std::sync::Arc;

use self::secret::Secret;
pub use self::{
    model::{Details, Network},
    nm::{Heard, Kept, Reply, View},
};

/// What the bar and the Settings ask of NetworkManager.
#[derive(Debug)]
pub enum Ask {
    /// Look for networks (as the popover opens, as the page shows).
    Scan,
    /// The Wi-Fi on or off.
    Enable(bool),
    /// Join a saved or open network.
    Connect { ssid: Vec<u8> },
    /// Join a protected network with this password.
    ConnectWith { ssid: Vec<u8>, secret: Secret },
    /// Disconnect the Wi-Fi device.
    Disconnect,
    /// Give up the connection under way.
    Abandon,
    /// A saved connection joined by itself, or not (`connection.autoconnect`).
    Autoconnect { uuid: String, on: bool },
    /// A saved Wi-Fi connection's password, read for the eye
    /// ([`Reply::Password`]).
    ReadPassword { uuid: String },
    /// A saved Wi-Fi connection's password changed, on disk.
    SetPassword { uuid: String, secret: Secret },
    /// A saved connection deleted (left first if it is in use), its
    /// settings and secrets kept for its undo ([`Reply::Forgotten`]).
    Forget { uuid: String },
    /// A connection forgotten, given back with its UUID, and joined again
    /// if it was in use ([`Reply::Restored`]).
    Restore(Arc<Kept>),
}
