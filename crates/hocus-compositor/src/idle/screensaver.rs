//! `org.freedesktop.ScreenSaver`'s inhibitions, as browsers use it to keep
//! the screen on through a video or a call (Chromium, Firefox, VLC): an
//! application's `Inhibit(name, reason)` holds idleness off until its
//! `UnInhibit(cookie)`, or until it leaves the bus, crashed or not.
//! `SimulateUserActivity` counts as the user's activity.
//!
//! Served on a thread of its own with its runtime, on the session bus the
//! compositor serves the notifications on (the bus given, else the
//! session's own on a virtual terminal; never a live session's beside a
//! nested one), at both paths applications call
//! (`/org/freedesktop/ScreenSaver` and `/ScreenSaver`). The calls are
//! answered here: the loop is only handed the list of holds, owned, each
//! time it changes. The screen saver's other methods (`GetActive`,
//! `Lock`...) are not served: nothing locks yet (7.2).

use std::sync::{Arc, Mutex, PoisonError};

use smithay::reexports::calloop::channel::Sender;
use zbus::{
    Connection, connection::Builder, fdo::DBusProxy, fdo::RequestNameFlags, message::Header,
};

pub const NAME: &str = "org.freedesktop.ScreenSaver";
const PATHS: [&str; 2] = ["/org/freedesktop/ScreenSaver", "/ScreenSaver"];

/// An application's hold, as the loop has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hold {
    pub cookie: u32,
    /// The name it gave, and why.
    pub app: String,
    pub reason: String,
}

/// What the bus's thread hands the loop.
#[derive(Debug)]
pub enum FromBus {
    /// The holds now, after one came or went.
    Holds(Vec<Hold>),
    /// An application says the user is active.
    Activity,
}

/// The holds, and whose: the caller's unique bus name.
#[derive(Debug, Default)]
struct Holds {
    next: u32,
    list: Vec<(Hold, String)>,
}

#[derive(Debug, Clone)]
struct Shared {
    holds: Arc<Mutex<Holds>>,
    loop_: Sender<FromBus>,
}

impl Shared {
    fn told(&self, holds: &Holds) {
        let list = holds.list.iter().map(|(h, _)| h.clone()).collect();
        let _ = self.loop_.send(FromBus::Holds(list));
    }

    /// Every hold of `caller`, which left the bus.
    fn release_all(&self, caller: &str) {
        let mut holds = self.holds.lock().unwrap_or_else(PoisonError::into_inner);
        let before = holds.list.len();
        holds.list.retain(|(hold, owner)| {
            let kept = owner != caller;
            if !kept {
                tracing::info!(
                    cookie = hold.cookie,
                    app = hold.app,
                    "a screen saver inhibition ends: its application left the bus"
                );
            }
            kept
        });
        if holds.list.len() != before {
            self.told(&holds);
        }
    }
}

/// Serves it on `bus` (an address, or `session`), its holds going to
/// `loop_`.
pub fn start(bus: String, loop_: Sender<FromBus>) {
    let started = std::thread::Builder::new()
        .name("screen saver".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(run(bus, loop_)),
                Err(err) => tracing::warn!(%err, "no runtime for org.freedesktop.ScreenSaver"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot serve org.freedesktop.ScreenSaver");
    }
}

async fn run(bus: String, loop_: Sender<FromBus>) {
    let shared = Shared {
        holds: Arc::default(),
        loop_,
    };
    let mut builder = if bus == "session" {
        Builder::session()
    } else {
        Builder::address(bus.as_str())
    };
    for path in PATHS {
        builder = builder.serve_at(
            path,
            ScreenSaver {
                shared: shared.clone(),
            },
        );
    }
    let connection = match builder.build().await {
        Ok(connection) => connection,
        Err(err) => {
            tracing::warn!(%err, "cannot reach the session bus: no org.freedesktop.ScreenSaver");
            return;
        }
    };
    // Departures watched before the name is owned: no caller can leave
    // unseen.
    let changes = match DBusProxy::new(&connection).await {
        Ok(proxy) => proxy.receive_name_owner_changed().await,
        Err(err) => Err(err),
    };
    let changes = match changes {
        Ok(changes) => changes,
        Err(err) => {
            tracing::warn!(%err, "cannot watch the bus: no org.freedesktop.ScreenSaver");
            return;
        }
    };
    match connection
        .request_name_with_flags(NAME, RequestNameFlags::REPLACE_EXISTING)
        .await
    {
        Ok(reply) => tracing::info!(?reply, "org.freedesktop.ScreenSaver is served"),
        Err(err) => {
            tracing::warn!(%err, "cannot own {NAME}");
            return;
        }
    }
    use zbus::export::futures_core::Stream;
    let mut changes = std::pin::pin!(changes);
    while let Some(changed) = std::future::poll_fn(|cx| changes.as_mut().poll_next(cx)).await {
        let Ok(args) = changed.args() else {
            continue;
        };
        let name = args.name().as_str();
        if args.new_owner().is_none() && name.starts_with(':') {
            shared.release_all(name);
        }
    }
}

struct ScreenSaver {
    shared: Shared,
}

#[zbus::interface(name = "org.freedesktop.ScreenSaver")]
impl ScreenSaver {
    async fn inhibit(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
        application_name: String,
        reason_for_inhibit: String,
    ) -> u32 {
        let Some(caller) = header.sender().map(|s| s.as_str().to_owned()) else {
            return 0;
        };
        let cookie = {
            let mut holds = self
                .shared
                .holds
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            // Never 0, which some applications take for a failure.
            holds.next = holds.next.wrapping_add(1).max(1);
            let cookie = holds.next;
            holds.list.push((
                Hold {
                    cookie,
                    app: application_name.clone(),
                    reason: reason_for_inhibit.clone(),
                },
                caller.clone(),
            ));
            self.shared.told(&holds);
            cookie
        };
        tracing::info!(
            cookie,
            app = application_name,
            reason = reason_for_inhibit,
            "a screen saver inhibition"
        );
        // A caller gone before its call was handled left no departure to
        // see after it.
        if let Ok(proxy) = DBusProxy::new(connection).await
            && let Ok(name) = zbus::names::BusName::try_from(caller.as_str())
            && proxy.name_has_owner(name).await.is_ok_and(|has| !has)
        {
            self.shared.release_all(&caller);
        }
        cookie
    }

    async fn un_inhibit(&self, #[zbus(header)] header: Header<'_>, cookie: u32) {
        let caller = header.sender().map(|s| s.as_str().to_owned());
        let mut holds = self
            .shared
            .holds
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let at = holds
            .list
            .iter()
            .position(|(h, owner)| h.cookie == cookie && Some(owner) == caller.as_ref());
        match at {
            Some(at) => {
                let (hold, _) = holds.list.remove(at);
                tracing::info!(cookie, app = hold.app, "a screen saver inhibition ends");
                self.shared.told(&holds);
            }
            None => tracing::debug!(cookie, "UnInhibit of no inhibition of this caller's"),
        }
    }

    async fn simulate_user_activity(&self) {
        let _ = self.shared.loop_.send(FromBus::Activity);
    }
}
