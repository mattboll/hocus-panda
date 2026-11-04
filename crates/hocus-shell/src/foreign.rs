//! A desktop portal's dialog over a shell surface (#595): the portal's
//! `parent_window` names the surface as `wayland:HANDLE`, an xdg-foreign
//! handle the compositor makes for it ([`ToCompositor::Export`]), since a
//! layer surface cannot be exported through the protocol. The portal's
//! backend makes its dialog a child of that handle, and the compositor
//! brings it in front with the keyboard.

use std::{
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use hocus_link::ToCompositor;
use tokio::sync::oneshot;

/// How long the asking thread waits for the compositor's handle before it
/// asks the portal without a parent. The compositor answers on its next
/// turn.
const WAIT: Duration = Duration::from_millis(500);

type Waiting = Vec<(u64, oneshot::Sender<Option<String>>)>;

/// The asks waiting for their handle, by their number.
static WAITING: Mutex<Waiting> = Mutex::new(Vec::new());
static ASKS: AtomicU64 = AtomicU64::new(0);

fn waiting() -> std::sync::MutexGuard<'static, Waiting> {
    WAITING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The portal's `parent_window` for the shell's surface of `namespace`:
/// `wayland:HANDLE`, or empty (no parent) without a compositor, or if it
/// has no such surface or did not answer. Waited for on the asking thread.
pub async fn parent_window(namespace: &str) -> String {
    let ask = ASKS.fetch_add(1, Ordering::Relaxed) + 1;
    let (sender, answer) = oneshot::channel();
    waiting().push((ask, sender));
    let asked = crate::popover::tell(ToCompositor::Export {
        ask,
        namespace: namespace.to_owned(),
    });
    let handle = if asked {
        tokio::time::timeout(WAIT, answer)
            .await
            .ok()
            .and_then(Result::ok)
            .flatten()
    } else {
        None
    };
    waiting().retain(|(a, _)| *a != ask);
    match handle {
        Some(handle) => format!("wayland:{handle}"),
        None => {
            tracing::info!(namespace, "no handle for the portal's dialog: no parent");
            String::new()
        }
    }
}

/// The compositor's handle for ask `ask`.
pub fn exported(ask: u64, handle: Option<String>) {
    let waiting = {
        let mut all = waiting();
        let at = all.iter().position(|(a, _)| *a == ask);
        at.map(|at| all.swap_remove(at).1)
    };
    if let Some(waiting) = waiting {
        let _ = waiting.send(handle);
    }
}
