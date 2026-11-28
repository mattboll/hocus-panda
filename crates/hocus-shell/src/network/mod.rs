//! The connections above the levels (bar.md §4 group 9, §5): the Wi-Fi's
//! row, its popover's data, and what the popover asks of NetworkManager.
//!
//! NetworkManager is read on a thread of its own (`hocus_network::nm`,
//! shared with the Settings' Réseau page), never on the loop that draws:
//! it pushes a [`Network`] each time what it says changes, and takes the
//! popover's asks ([`Ask`]) through a channel. The row draws
//! on a surface of its own ([`draw`]), woken through the compositor only
//! when what it shows changes (`wake hocus-bar-connections`), so that
//! neither the levels nor the clock draw for a network, nor the row for a
//! level. The popover, when it shows the Wi-Fi, is told of every change.

pub mod draw;
pub mod model;

use std::sync::{Mutex, OnceLock, PoisonError, mpsc};

use hocus_network::rfkill::{self, Block, Radios};

pub use hocus_network::{Ask, secret};

pub use self::model::Network;

/// What changed, for whoever shows it: the row, or only the popover's
/// contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    pub row: bool,
    /// Whether there is a row at all.
    pub shown: bool,
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
        hocus_network::nm::start(incoming, |heard| {
            if let hocus_network::Heard::Now(view) = heard {
                publish(view.network());
            }
        });
        // The radios, for airplane mode (6.6): the bar only reads them,
        // and unblocks the Wi-Fi's when its switch turns it on.
        let (radio_asks, receive) = mpsc::channel();
        let _ = RADIO_ASKS.set(Mutex::new(radio_asks));
        rfkill::start(rfkill::device(), None, receive, |radios| {
            *RADIOS.lock().unwrap_or_else(PoisonError::into_inner) = radios;
            radios_changed();
            true
        });
    }
}

/// The radios as rfkill last said them.
static RADIOS: Mutex<Option<Radios>> = Mutex::new(None);
static RADIO_ASKS: OnceLock<Mutex<mpsc::Sender<rfkill::Ask>>> = OnceLock::new();

/// The Wi-Fi's radio unblocked: the switch turns it on in airplane mode,
/// alone.
pub fn unblock_wifi() {
    if let Some(asks) = RADIO_ASKS.get() {
        let sent = asks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send(rfkill::Ask::Unblock(rfkill::WLAN));
        if sent.is_err() {
            tracing::warn!("the radios' thread has stopped: dropped");
        }
    }
}

/// The radios changed: the row and the popover say it.
fn radios_changed() {
    let Some(service) = SERVICE.get() else {
        return;
    };
    let mut waker = service.waker.lock().unwrap_or_else(PoisonError::into_inner);
    let change = Change {
        row: true,
        shown: false,
    };
    if waker.as_mut().is_some_and(|wake| !wake(change)) {
        *waker = None;
    }
}

/// Airplane mode, and the Wi-Fi's radio blocked by software (it is then
/// off whatever NetworkManager says).
fn radios(network: &mut Network) {
    let radios = RADIOS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(radios) = radios.as_ref() {
        network.airplane = radios.airplane();
        if radios.block(rfkill::WLAN) != Block::No {
            network.enabled = false;
        }
    }
}

/// The network as last read.
pub fn snapshot() -> Network {
    let mut network = SERVICE.get().map_or_else(Network::default, |s| {
        s.network
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    });
    radios(&mut network);
    network
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
        let shown = (kept.link() == model::Link::Hidden) != (network.link() == model::Link::Hidden);
        *kept = network;
        Change { row, shown }
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
        // Under the waker's lock: a change published meanwhile waits for
        // it and wakes it, instead of falling between the two.
        let mut waker = service.waker.lock().unwrap_or_else(PoisonError::into_inner);
        let mut wake: Waker = Box::new(wake);
        let alive = wake(Change {
            row: true,
            shown: true,
        });
        *waker = alive.then_some(wake);
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
/// `xdg-open` in `PATH`.
fn opener() -> Option<String> {
    std::env::var("HOCUS_OPENER")
        .ok()
        .or_else(|| crate::launcher::program("xdg-open"))
}

/// Opens `uri` in the user's browser: a sign-in page shows itself there
/// when NetworkManager's check address is asked for behind it. Started by
/// the compositor, as the launcher's addresses are: off its loop, with an
/// activation token, so that the browser comes forward (#966).
pub fn open_in_browser(uri: String) -> Option<hocus_link::Launch> {
    let Some(program) = opener() else {
        tracing::warn!("no xdg-open in PATH: the sign-in page does not open");
        return None;
    };
    let launch = hocus_link::Launch {
        id: "xdg-open".to_owned(),
        terminal: false,
        dir: String::new(),
        argv: vec![program, uri],
    };
    if !launch.fits() {
        tracing::warn!("a sign-in address that cannot be sent: not opened");
        return None;
    }
    Some(launch)
}
