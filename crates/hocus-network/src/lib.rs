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
    Connect {
        ssid: Vec<u8>,
    },
    /// Join a protected network with this password.
    ConnectWith {
        ssid: Vec<u8>,
        secret: Secret,
    },
    /// Disconnect the Wi-Fi device.
    Disconnect,
    /// Give up the connection under way.
    Abandon,
    /// A saved connection joined by itself, or not (`connection.autoconnect`).
    Autoconnect {
        uuid: String,
        on: bool,
    },
    /// A saved Wi-Fi connection's password, read for the eye
    /// ([`Reply::Password`]).
    ReadPassword {
        uuid: String,
    },
    /// A saved Wi-Fi connection's password changed, on disk.
    SetPassword {
        uuid: String,
        secret: Secret,
    },
    /// A saved connection deleted (left first if it is in use), its
    /// settings and secrets kept for its undo ([`Reply::Forgotten`]).
    Forget {
        uuid: String,
    },
    /// A connection forgotten, given back with its UUID, and joined again
    /// if it was in use ([`Reply::Restored`]).
    Restore(Arc<Kept>),
    /// A VPN turned on, or off.
    Activate {
        uuid: String,
    },
    Deactivate {
        uuid: String,
    },
    /// A connection renamed (`connection.id`).
    Rename {
        uuid: String,
        name: String,
    },
    /// A VPN's file imported (`kind`: `wireguard`, `openvpn`), off, with
    /// the user name and password an OpenVPN file asks
    /// ([`Reply::Imported`]).
    Import {
        file: std::path::PathBuf,
        kind: &'static str,
        credentials: Option<(String, Secret)>,
    },
    /// A network joined with what its sheet asked (the Settings'): a
    /// hidden one by its name alone, an 802.1X or WEP one.
    Join {
        ssid: Vec<u8>,
        hidden: bool,
        security: Join,
    },
}

/// What a network joined from the Settings' sheet asks.
#[derive(Debug)]
pub enum Join {
    Open,
    /// WPA or WPA2 personal, or WPA3's SAE (`sae`).
    Psk {
        sae: bool,
        secret: Secret,
    },
    /// A WEP key (5 or 13 characters, 10 or 26 hexadecimal digits) or a
    /// passphrase.
    Wep {
        secret: Secret,
    },
    /// 802.1X.
    Eap(Box<Eap>),
}

/// An 802.1X network's settings, as its service informatique gives them.
#[derive(Debug)]
pub struct Eap {
    /// NetworkManager's word: `peap`, `ttls` or `tls`.
    pub method: &'static str,
    /// Its inner authentication (`phase2-auth`): `mschapv2`, `gtc`, `pap`,
    /// `mschap`, `chap`; none for TLS.
    pub inner: Option<&'static str>,
    pub identity: String,
    /// PEAP's and TTLS's.
    pub password: Option<Secret>,
    pub ca: Ca,
    /// `domain-suffix-match`.
    pub domain: Option<String>,
    /// TLS's: the user's certificate and key, the key's password.
    pub certificate: Option<std::path::PathBuf>,
    pub key: Option<std::path::PathBuf>,
    pub key_password: Option<Secret>,
}

/// How the server's certificate is checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ca {
    /// The system's authorities (`system-ca-certs`).
    System,
    File(std::path::PathBuf),
    /// Not checked.
    Unchecked,
}

/// Whether a WEP secret is a key (`wep-key-type` 1): 5 or 13 characters,
/// 10 or 26 hexadecimal digits; else a passphrase (2).
pub fn wep_is_key(secret: &str) -> bool {
    let n = secret.chars().count();
    let hex = secret.chars().all(|c| c.is_ascii_hexdigit());
    matches!(n, 5 | 13) || (hex && matches!(n, 10 | 26))
}
