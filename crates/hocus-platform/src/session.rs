//! The session's side of locking (#30): logind asks to lock (`loginctl
//! lock-session`) or unlock, and says when the machine goes to sleep, which
//! a delay inhibitor holds until the screen is locked; the D-Bus
//! ScreenSaver service lets applications keep the screen awake and ask to
//! lock. Each runs on threads of its own; what they hear goes to `sink`.

use std::{
    future::poll_fn,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc,
    },
};

use futures_core::Stream;
use zbus::{Connection, OwnedObjectPath, Proxy, interface, object_server::SignalEmitter};

/// What the session asks of the compositor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Lock,
    Unlock,
    /// Input as far as idle goes (ScreenSaver.SimulateUserActivity).
    Activity,
    /// How many applications hold the screen awake.
    Inhibitors(u32),
    /// One of logind's signals is followed from now on (one per thread
    /// of [`Logind`]): a signal sent before would be missed, which a test
    /// waits for (#235).
    Following,
}

/// logind: lock and unlock requests, and the lock before sleep.
#[derive(Debug)]
pub struct Logind {
    locked: mpsc::Sender<()>,
}

type Sink = Arc<dyn Fn(Request) + Send + Sync>;
/// A thread's work: follow one of logind's signals.
type Follow = Box<dyn FnOnce() -> zbus::Result<()> + Send>;

impl Logind {
    /// Follows logind on `system_bus` (the machine's when `None`).
    pub fn start(
        system_bus: Option<String>,
        sink: impl Fn(Request) + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        let (locked, confirmations) = mpsc::channel::<()>();
        let sink: Sink = Arc::new(sink);
        let threads: [(&str, Follow); 3] = [
            ("logind sleep", {
                let (bus, sink) = (system_bus.clone(), sink.clone());
                Box::new(move || zbus::block_on(sleep(bus, sink, confirmations)))
            }),
            ("logind lock", {
                let (bus, sink) = (system_bus.clone(), sink.clone());
                Box::new(move || zbus::block_on(session_signal(bus, sink, "Lock", Request::Lock)))
            }),
            ("logind unlock", {
                let (bus, sink) = (system_bus, sink);
                Box::new(move || {
                    zbus::block_on(session_signal(bus, sink, "Unlock", Request::Unlock))
                })
            }),
        ];
        for (name, run) in threads {
            std::thread::Builder::new()
                .name(name.into())
                .spawn(move || {
                    if let Err(err) = run() {
                        tracing::debug!(%err, "logind is not there");
                    }
                })?;
        }
        Ok(Self { locked })
    }

    /// The screen is locked: sleep may go on.
    pub fn locked(&self) {
        let _ = self.locked.send(());
    }
}

const LOGIND: &str = "org.freedesktop.login1";

pub(crate) async fn system(bus: &Option<String>) -> zbus::Result<Connection> {
    match bus {
        Some(address) => {
            zbus::connection::Builder::address(address.as_str())
                .build()
                .await
        }
        None => Connection::system().await,
    }
}

pub(crate) async fn manager(conn: &Connection) -> zbus::Result<Proxy<'static>> {
    Proxy::new(
        conn,
        LOGIND,
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await
}

/// Each `name` signal of this process's session is `request`.
async fn session_signal(
    bus: Option<String>,
    sink: Sink,
    name: &'static str,
    request: Request,
) -> zbus::Result<()> {
    let conn = system(&bus).await?;
    let path: OwnedObjectPath = manager(&conn)
        .await?
        .call("GetSessionByPID", &(std::process::id()))
        .await?;
    let session = Proxy::new(
        &conn,
        LOGIND,
        path.as_str().to_owned(),
        "org.freedesktop.login1.Session",
    )
    .await?;
    let mut signals = session.receive_signal(name).await?;
    sink(Request::Following);
    while poll_fn(|cx| Pin::new(&mut signals).poll_next(cx))
        .await
        .is_some()
    {
        sink(request);
    }
    Ok(())
}

/// Locks before sleep: a delay inhibitor holds sleep until the lock shows
/// (or two seconds pass), and is taken again on waking.
async fn sleep(
    bus: Option<String>,
    sink: Sink,
    confirmations: mpsc::Receiver<()>,
) -> zbus::Result<()> {
    let conn = system(&bus).await?;
    let manager = manager(&conn).await?;
    let inhibit = || async {
        manager
            .call::<_, _, zbus::OwnedFd>(
                "Inhibit",
                &(
                    "sleep",
                    "Hocuspanda",
                    "Lock the screen before sleep",
                    "delay",
                ),
            )
            .await
            .inspect_err(|err| tracing::debug!(%err, "no sleep inhibitor"))
            .ok()
    };
    let mut delay = inhibit().await;
    let mut signals = manager.receive_signal("PrepareForSleep").await?;
    sink(Request::Following);
    while let Some(signal) = poll_fn(|cx| Pin::new(&mut signals).poll_next(cx)).await {
        let going: bool = signal.body().deserialize()?;
        if going {
            sink(Request::Lock);
            // This thread does nothing else: waiting here is fine.
            let _ = confirmations.recv_timeout(std::time::Duration::from_secs(2));
            delay = None;
        } else if delay.is_none() {
            delay = inhibit().await;
        }
    }
    drop(delay);
    Ok(())
}

/// org.freedesktop.ScreenSaver: inhibitions, lock, activity.
struct ScreenSaver {
    sink: Arc<dyn Fn(Request) + Send + Sync>,
    cookies: Mutex<Vec<u32>>,
    next: AtomicU32,
    active: Arc<AtomicBool>,
}

#[interface(name = "org.freedesktop.ScreenSaver")]
impl ScreenSaver {
    fn inhibit(&self, application: &str, reason: &str) -> u32 {
        let cookie = self.next.fetch_add(1, Ordering::Relaxed);
        tracing::info!(application, reason, "the screen is kept awake");
        let count = self.cookies.lock().map_or(0, |mut c| {
            c.push(cookie);
            c.len()
        });
        (self.sink)(Request::Inhibitors(count as u32));
        cookie
    }

    fn un_inhibit(&self, cookie: u32) {
        let count = self.cookies.lock().map_or(0, |mut c| {
            c.retain(|k| *k != cookie);
            c.len()
        });
        (self.sink)(Request::Inhibitors(count as u32));
    }

    fn lock(&self) {
        (self.sink)(Request::Lock);
    }

    fn simulate_user_activity(&self) {
        (self.sink)(Request::Activity);
    }

    fn get_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    fn get_active_time(&self) -> u32 {
        0
    }

    #[zbus(signal)]
    async fn active_changed(emitter: &SignalEmitter<'_>, active: bool) -> zbus::Result<()>;
}

/// The ScreenSaver service on a session bus.
#[derive(Debug)]
pub struct ScreenSaverService {
    active: Arc<AtomicBool>,
    changes: mpsc::Sender<bool>,
}

impl ScreenSaverService {
    pub fn start(
        session_bus: &str,
        sink: impl Fn(Request) + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let active = Arc::new(AtomicBool::new(false));
        let (changes, received) = mpsc::channel::<bool>();
        let (ready, started) = mpsc::channel();
        let address = session_bus.to_owned();
        let sink: Arc<dyn Fn(Request) + Send + Sync> = Arc::new(sink);
        let shared = active.clone();
        std::thread::Builder::new()
            .name("screensaver".into())
            .spawn(move || {
                let service = || ScreenSaver {
                    sink: sink.clone(),
                    cookies: Mutex::new(Vec::new()),
                    next: AtomicU32::new(1),
                    active: shared.clone(),
                };
                // Under both paths applications use.
                let connection = zbus::block_on(
                    zbus::connection::Builder::address(address.as_str())
                        .serve_at("/org/freedesktop/ScreenSaver", service())
                        .serve_at("/ScreenSaver", service())
                        .name("org.freedesktop.ScreenSaver")
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
                for now in received {
                    let _ = zbus::block_on(async {
                        let iface = connection
                            .object_server()
                            .interface::<_, ScreenSaver>("/org/freedesktop/ScreenSaver")
                            .await?;
                        ScreenSaver::active_changed(iface.signal_emitter(), now).await
                    });
                }
            })
            .map_err(|e| e.to_string())?;
        match started.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self { active, changes }),
            Ok(Err(err)) => Err(err),
            Err(_) => Err("the bus did not answer".into()),
        }
    }

    /// The curtain or the lock shows, or not.
    pub fn set_active(&self, active: bool) {
        if self.active.swap(active, Ordering::Relaxed) != active {
            let _ = self.changes.send(active);
        }
    }
}
