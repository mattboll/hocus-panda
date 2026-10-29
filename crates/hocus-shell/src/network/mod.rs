//! The connections above the levels (bar.md §4 group 9, §5): the Wi-Fi's
//! row, its popover's data, and what the popover asks of NetworkManager.
//!
//! NetworkManager is read on a thread of its own ([`nm`]), never on the
//! loop that draws: it pushes a [`Network`] each time what it says changes,
//! and takes the popover's asks ([`Ask`]) through a channel. The row draws
//! on a surface of its own ([`draw`]), woken through the compositor only
//! when what it shows changes (`wake hocus-bar-connections`), so that
//! neither the levels nor the clock draw for a network, nor the row for a
//! level. The popover, when it shows the Wi-Fi, is told of every change.

pub mod draw;
pub mod model;
pub mod nm;
pub mod secret;

use std::sync::{Mutex, OnceLock, PoisonError};

pub use self::model::Network;
use self::secret::Secret;

/// What the popover asks of NetworkManager.
#[derive(Debug)]
pub enum Ask {
    /// Look for networks (as the popover opens).
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
}

/// What changed, for whoever shows it: the row, or only the popover's
/// contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    pub row: bool,
}

type Waker = Box<dyn FnMut(Change) -> bool + Send>;

struct Service {
    network: Mutex<Network>,
    waker: Mutex<Option<Waker>>,
    asks: tokio::sync::mpsc::UnboundedSender<Ask>,
}

static SERVICE: OnceLock<Service> = OnceLock::new();

/// Starts NetworkManager's thread, once.
pub fn start() {
    let (asks, incoming) = tokio::sync::mpsc::unbounded_channel();
    let service = Service {
        network: Mutex::new(Network::default()),
        waker: Mutex::new(None),
        asks,
    };
    if SERVICE.set(service).is_ok() {
        nm::start(incoming);
    }
}

/// The network as last read.
pub fn snapshot() -> Network {
    SERVICE.get().map_or_else(Network::default, |s| {
        s.network
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    })
}

/// NetworkManager's thread says `network`: kept, and shown if it changed.
fn publish(network: Network) {
    let Some(service) = SERVICE.get() else {
        return;
    };
    let change = {
        let mut kept = service
            .network
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if *kept == network {
            return;
        }
        let row = kept.link() != network.link() || kept.warns() != network.warns();
        *kept = network;
        Change { row }
    };
    tracing::debug!(row = change.row, "the network changed");
    let mut waker = service.waker.lock().unwrap_or_else(PoisonError::into_inner);
    if waker.as_mut().is_some_and(|wake| !wake(change)) {
        *waker = None;
    }
}

/// From now on, a change calls `wake` (until it returns `false`); it is
/// called once now, for what changed before.
pub fn on_change(wake: impl FnMut(Change) -> bool + Send + 'static) {
    if let Some(service) = SERVICE.get() {
        let mut wake: Waker = Box::new(wake);
        let alive = wake(Change { row: true });
        *service.waker.lock().unwrap_or_else(PoisonError::into_inner) = alive.then_some(wake);
    }
}

pub fn ask(ask: Ask) {
    let sent = SERVICE
        .get()
        .is_some_and(|service| service.asks.send(ask).is_ok());
    if !sent {
        tracing::warn!("NetworkManager's thread has stopped: dropped");
    }
}

/// The program that opens a web page: `HOCUS_OPENER` for tests, else
/// `xdg-open`.
fn opener() -> String {
    std::env::var("HOCUS_OPENER").unwrap_or_else(|_| "xdg-open".to_owned())
}

/// Opens `uri` in the user's browser: a sign-in page shows itself there
/// when NetworkManager's check address is asked for behind it. The program
/// runs on a thread of its own, which waits for it: never the loop that
/// draws.
pub fn open_in_browser(uri: String) {
    let program = opener();
    let started = std::thread::Builder::new()
        .name("opener".to_owned())
        .spawn(move || {
            let child = std::process::Command::new(&program)
                .arg(&uri)
                .stdin(std::process::Stdio::null())
                .spawn();
            match child.and_then(|mut child| child.wait()) {
                Ok(status) if status.success() => {}
                Ok(status) => tracing::warn!(program, %status, "the browser did not open"),
                Err(err) => tracing::warn!(program, %err, "the browser did not open"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot open the browser");
    }
}
