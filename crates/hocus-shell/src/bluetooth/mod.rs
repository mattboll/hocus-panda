//! Bluetooth in the connections (bar.md §4.3, §5, §9): its row under the
//! Wi-Fi's, its popover's data, and what the popover asks of BlueZ.
//!
//! BlueZ is read on a thread of its own ([`bluez`]), never on the loop
//! that draws: it pushes a [`Bluetooth`] each time what it shows changes,
//! and takes the popover's asks ([`Ask`]) through a channel. The row draws
//! on the connections' surface ([`draw`]), woken through the compositor
//! only when what it shows changes, so that neither the levels nor the
//! clock draw for Bluetooth, nor the row for a level. The popover, when it
//! shows Bluetooth, is told of every change.

pub mod bluez;
pub mod draw;
pub mod model;

use std::sync::{Mutex, OnceLock, PoisonError};

pub use self::model::Bluetooth;

/// What the popover asks of BlueZ, by device path.
#[derive(Debug)]
pub enum Ask {
    /// The adapter on or off.
    Power(bool),
    /// Look for devices around (while the popover is open), or stop.
    Discover(bool),
    Connect(String),
    Disconnect(String),
    /// Pair, trust, then connect.
    Pair(String),
    /// The shell ends: stop looking, then say so.
    Quit(std::sync::mpsc::Sender<()>),
}

/// What changed, for whoever shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    /// What the row says.
    pub row: bool,
    /// Whether there is a row at all.
    pub shown: bool,
}

type Waker = Box<dyn FnMut(Change) -> bool + Send>;

struct Service {
    bluetooth: Mutex<Bluetooth>,
    waker: Mutex<Option<Waker>>,
    asks: tokio::sync::mpsc::UnboundedSender<Ask>,
}

static SERVICE: OnceLock<Service> = OnceLock::new();

/// Starts BlueZ's thread, once.
pub fn start() {
    let (asks, incoming) = tokio::sync::mpsc::unbounded_channel();
    let service = Service {
        bluetooth: Mutex::new(Bluetooth::default()),
        waker: Mutex::new(None),
        asks,
    };
    if SERVICE.set(service).is_ok() {
        bluez::start(incoming);
    }
}

/// Bluetooth as last read.
pub fn snapshot() -> Bluetooth {
    SERVICE.get().map_or_else(Bluetooth::default, |s| {
        s.bluetooth
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    })
}

/// BlueZ's thread says `bluetooth`: kept, and shown if it changed.
fn publish(bluetooth: Bluetooth) {
    let Some(service) = SERVICE.get() else {
        return;
    };
    let change = {
        let mut kept = service
            .bluetooth
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if *kept == bluetooth {
            return;
        }
        let (before, after) = (kept.row(), bluetooth.row());
        let change = Change {
            row: before != after,
            shown: (before == model::Row::Hidden) != (after == model::Row::Hidden),
        };
        *kept = bluetooth;
        change
    };
    tracing::debug!(row = change.row, "Bluetooth changed");
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
        let alive = wake(Change {
            row: true,
            shown: true,
        });
        *service.waker.lock().unwrap_or_else(PoisonError::into_inner) = alive.then_some(wake);
    }
}

pub fn ask(ask: Ask) {
    let sent = SERVICE
        .get()
        .is_some_and(|service| service.asks.send(ask).is_ok());
    if !sent {
        tracing::warn!("BlueZ's thread has stopped: dropped");
    }
}

/// The shell ends: the search it started stops (BlueZ would stop it when
/// the shell's connection goes, but not before). Waits a little for it,
/// never long.
pub fn stop() {
    let (done, said) = std::sync::mpsc::channel();
    ask(Ask::Quit(done));
    let _ = said.recv_timeout(std::time::Duration::from_millis(500));
}
