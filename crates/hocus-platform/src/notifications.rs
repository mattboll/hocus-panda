//! The freedesktop notification server (`org.freedesktop.Notifications`,
//! version 1.2), on zbus's threads and one of its own, never on the
//! compositor's loop. What arrives goes to the loop through `sink`; what the
//! loop decides (a notification closed, an action chosen) comes back through
//! [`Notifications`] and is signalled from here.
//!
//! Each application gets a few notifications at once, then one a second: a
//! flood is dropped here, before it costs the loop anything.

use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use zbus::{OwnedVariant, interface, object_server::SignalEmitter};

const PATH: &str = "/org/freedesktop/Notifications";
const NAME: &str = "org.freedesktop.Notifications";
/// Notifications an application may send at once…
const BURST: f64 = 5.0;
/// …then this many a second.
const PER_SECOND: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Urgency {
    Low,
    #[default]
    Normal,
    Critical,
}

/// How long a notification stays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Expiry {
    /// The server's choice, by urgency.
    #[default]
    Default,
    Never,
    After(Duration),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub id: u32,
    pub app: String,
    pub summary: String,
    pub body: String,
    /// Action keys and their labels; `default` is the notification itself.
    pub actions: Vec<(String, String)>,
    pub urgency: Urgency,
    pub expiry: Expiry,
    /// It replaces the notification of the same id.
    pub replaces: bool,
    /// The application and event that KDE's notifications name in their
    /// hints (`x-kde-appname`, `x-kde-eventId`): KDE Connect's copies of
    /// phone notifications, which the phone module shows itself.
    pub kde_event: Option<(String, String)>,
}

/// What clients ask, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    Notify(Notification),
    Close(u32),
}

/// Why a notification went away, as the specification numbers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    Expired = 1,
    Dismissed = 2,
    /// Closed by `CloseNotification`.
    Closed = 3,
}

enum Signal {
    Closed(u32, CloseReason),
    /// An action, with the activation token what it opens may use.
    Action(u32, String, Option<String>),
}

/// A token bucket per application.
#[derive(Debug, Default)]
struct Limiter {
    apps: HashMap<String, (f64, Instant)>,
}

impl Limiter {
    fn allow(&mut self, app: &str, now: Instant) -> bool {
        let (tokens, last) = self.apps.entry(app.to_owned()).or_insert((BURST, now));
        *tokens = (*tokens + now.duration_since(*last).as_secs_f64() * PER_SECOND).min(BURST);
        *last = now;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

type Sink = Box<dyn Fn(Incoming) + Send + Sync>;

struct Server {
    next: AtomicU32,
    sink: Sink,
    limiter: Mutex<Limiter>,
    dropped: AtomicU32,
}

#[interface(name = "org.freedesktop.Notifications")]
impl Server {
    fn get_capabilities(&self) -> Vec<String> {
        vec!["body".into(), "actions".into()]
    }

    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        _app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedVariant>,
        expire_timeout: i32,
    ) -> u32 {
        let replaces = replaces_id != 0;
        let id = if replaces {
            replaces_id
        } else {
            self.next.fetch_add(1, Ordering::Relaxed)
        };
        let allowed = self
            .limiter
            .lock()
            .map(|mut l| l.allow(&app_name, Instant::now()))
            .unwrap_or(true);
        if !allowed {
            let n = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
            if n.is_power_of_two() {
                tracing::info!(
                    app = app_name,
                    dropped = n,
                    "notifications dropped by the rate limit"
                );
            }
            return id;
        }
        let urgency = match hints.get("urgency").and_then(|v| u8::try_from(v).ok()) {
            Some(0) => Urgency::Low,
            Some(2) => Urgency::Critical,
            _ => Urgency::Normal,
        };
        let expiry = match expire_timeout {
            0 => Expiry::Never,
            t if t > 0 => Expiry::After(Duration::from_millis(t as u64)),
            _ => Expiry::Default,
        };
        let text = |key: &str| {
            hints
                .get(key)
                .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
        };
        let kde_event = text("x-kde-appname").zip(text("x-kde-eventId"));
        (self.sink)(Incoming::Notify(Notification {
            id,
            app: app_name,
            summary,
            body,
            actions: actions
                .as_chunks::<2>()
                .0
                .iter()
                .map(|[key, label]| (key.clone(), label.clone()))
                .collect(),
            urgency,
            expiry,
            replaces,
            kde_event,
        }));
        id
    }

    fn close_notification(&self, id: u32) {
        (self.sink)(Incoming::Close(id));
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        (
            "Hocuspanda".into(),
            "Hocuspanda".into(),
            env!("CARGO_PKG_VERSION").into(),
            "1.2".into(),
        )
    }

    #[zbus(signal)]
    async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    /// Sent before an action, as specification 1.2 says: the token the
    /// application brings its window forward with.
    #[zbus(signal)]
    async fn activation_token(
        emitter: &SignalEmitter<'_>,
        id: u32,
        activation_token: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
}

/// The running server; dropping it ends its thread, which releases the bus
/// name (the thread is not waited for: nothing depends on it).
pub struct Notifications {
    signals: mpsc::Sender<Signal>,
}

impl std::fmt::Debug for Notifications {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Notifications")
    }
}

impl Notifications {
    /// Serves on the bus at `address` (a session bus). Waits at most
    /// `timeout` for the bus, at start only; fails when another server owns
    /// the name.
    pub fn start(
        address: &str,
        sink: impl Fn(Incoming) + Send + Sync + 'static,
        timeout: Duration,
    ) -> Result<Self, String> {
        let (signals, received) = mpsc::channel();
        let (ready, started) = mpsc::channel();
        let address = address.to_owned();
        let sink: Sink = Box::new(sink);
        std::thread::Builder::new()
            .name("notifications".into())
            .spawn(move || {
                let server = Server {
                    next: AtomicU32::new(1),
                    sink,
                    limiter: Mutex::new(Limiter::default()),
                    dropped: AtomicU32::new(0),
                };
                // zbus's runtime serves the connection from its own threads;
                // this one only starts it and sends the signals.
                let connection = zbus::block_on(
                    zbus::connection::Builder::address(address.as_str())
                        .serve_at(PATH, server)
                        .name(NAME)
                        .build(),
                );
                let connection = match connection {
                    Ok(c) => {
                        let _ = ready.send(Ok(()));
                        c
                    }
                    Err(err) => {
                        let _ = ready.send(Err(err.to_string()));
                        return;
                    }
                };
                // Signals until the compositor drops its end.
                for signal in received {
                    let emitted = zbus::block_on(async {
                        let iface = connection
                            .object_server()
                            .interface::<_, Server>(PATH)
                            .await?;
                        let emitter = iface.signal_emitter();
                        match signal {
                            Signal::Closed(id, reason) => {
                                Server::notification_closed(emitter, id, reason as u32).await
                            }
                            Signal::Action(id, key, token) => {
                                if let Some(token) = token {
                                    Server::activation_token(emitter, id, &token).await?;
                                }
                                Server::action_invoked(emitter, id, &key).await
                            }
                        }
                    });
                    if let Err(err) = emitted {
                        tracing::warn!(%err, "could not signal a notification");
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        match started.recv_timeout(timeout) {
            Ok(Ok(())) => Ok(Self { signals }),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(format!("the bus did not answer within {timeout:?}")),
        }
    }

    /// Tells the client its notification went away.
    pub fn closed(&self, id: u32, reason: CloseReason) {
        let _ = self.signals.send(Signal::Closed(id, reason));
    }

    /// Tells the client its action was chosen, with the activation token
    /// its window may come forward with.
    pub fn action_invoked(&self, id: u32, key: &str, token: Option<String>) {
        let _ = self.signals.send(Signal::Action(id, key.to_owned(), token));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flood_gets_a_burst_then_one_a_second() {
        let mut limiter = Limiter::default();
        let start = Instant::now();
        let allowed = (0..100)
            .filter(|i| limiter.allow("chat", start + Duration::from_millis(i * 10)))
            .count();
        // 5 at once; the next one a second after the burst.
        assert_eq!(allowed, 5);
        assert!(limiter.allow("chat", start + Duration::from_millis(1200)));
        assert!(!limiter.allow("chat", start + Duration::from_millis(1300)));
        // Another application is not held back.
        assert!(limiter.allow("mail", start + Duration::from_millis(1300)));
    }
}
