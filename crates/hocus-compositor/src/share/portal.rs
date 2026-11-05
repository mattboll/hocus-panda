//! The desktop portal's screen cast, as the desktop serves it
//! (`org.freedesktop.impl.portal.ScreenCast`, version 5): xdg-desktop-portal
//! finds it by `hocuspanda.portal` and `hocuspanda-portals.conf`, and calls
//! it for the applications.
//!
//! It runs on a thread of its own with its own runtime, on the session bus
//! the compositor serves the notifications on (the bus given, else the
//! session's own on a virtual terminal; never a live session's beside a
//! nested one). A session's options are kept here; its `Start` is handed,
//! owned, to the loop ([`FromPortal`]), which shows the chooser and makes
//! the stream, and answers through a channel of its own: the call waits
//! here, never on the loop. A share the user stops from the bar closes
//! its session, which xdg-desktop-portal tells the application.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
};

use smithay::reexports::calloop::channel::Sender;
use tokio::sync::{mpsc, oneshot};
use zbus::{
    Connection, ObjectServer, OwnedObjectPath, OwnedVariant, Variant, connection::Builder,
    fdo::RequestNameFlags, object_server::SignalEmitter,
};

/// The name xdg-desktop-portal looks for (`hocuspanda.portal`).
pub const NAME: &str = "org.freedesktop.impl.portal.desktop.hocuspanda";
const PATH: &str = "/org/freedesktop/portal/desktop";
/// The portal's answers: done, cancelled by the user, failed.
pub const SUCCESS: u32 = 0;
pub const CANCELLED: u32 = 1;
pub const FAILED: u32 = 2;
/// The cursor modes served: hidden and embedded (no metadata).
const HIDDEN: u32 = 1;
const EMBEDDED: u32 = 2;

/// A stream as `Start` returns it: its PipeWire node, where its source is
/// and how large (logical pixels), and what it is ([`hocus_link::SHARE_MONITOR`]
/// or [`hocus_link::SHARE_WINDOW`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamOut {
    pub node: u32,
    pub position: Option<(i32, i32)>,
    pub size: (i32, i32),
    pub source_type: u32,
}

/// What a `Start` comes to.
#[derive(Debug)]
pub enum Answer {
    Streams(Vec<StreamOut>),
    Cancelled,
    Failed,
}

/// A `Start` to answer.
#[derive(Debug)]
pub struct StartAsk {
    /// The session's object path, which names it.
    pub session: String,
    pub app_id: String,
    /// The application's window, as the portal says it (`x11:XID`,
    /// `wayland:HANDLE`; empty: none).
    pub parent_window: String,
    /// What it may share ([`hocus_link::SHARE_MONITOR`] and
    /// [`hocus_link::SHARE_WINDOW`]); 0: not said, screens.
    pub types: u32,
    /// The cursor mode it asked for, if it did.
    pub cursor_mode: Option<u32>,
    pub reply: oneshot::Sender<Answer>,
}

/// What the portal hands the loop.
#[derive(Debug)]
pub enum FromPortal {
    Start(StartAsk),
    /// The pending `Start` of this session was cancelled (its request
    /// closed).
    Cancel {
        session: String,
    },
    /// The application, or xdg-desktop-portal, closed the session: its
    /// share ends.
    Closed {
        session: String,
    },
}

/// What the loop tells the portal: the session ended on the desktop's
/// side (stopped from the bar, its window closed): `Closed` is emitted.
#[derive(Debug)]
pub enum ToPortal {
    Close {
        session: String,
    },
    /// `[appearance]` changed: the settings portal says it
    /// ([`crate::appearance::portal`]).
    Appearance(crate::appearance::portal::Values),
}

/// The portal's thread, as the loop holds it.
#[derive(Debug)]
pub struct Portal {
    to: mpsc::UnboundedSender<ToPortal>,
}

impl Portal {
    pub fn close(&self, session: &str) {
        let _ = self.to.send(ToPortal::Close {
            session: session.to_owned(),
        });
    }

    pub fn appearance(&self, values: crate::appearance::portal::Values) {
        let _ = self.to.send(ToPortal::Appearance(values));
    }
}

/// Serves the screen cast on `bus` (an address, or `session`), what comes
/// in going to `loop_`, and the settings, `appearance` first.
pub fn start(
    bus: String,
    loop_: Sender<FromPortal>,
    appearance: crate::appearance::portal::Values,
) -> Option<Portal> {
    let (to, from_loop) = mpsc::unbounded_channel();
    let started = std::thread::Builder::new()
        .name("screen cast portal".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(run(bus, loop_, from_loop, appearance)),
                Err(err) => tracing::warn!(%err, "no runtime for the screen cast portal"),
            }
        });
    match started {
        Ok(_) => Some(Portal { to }),
        Err(err) => {
            tracing::warn!(%err, "cannot serve the screen cast portal");
            None
        }
    }
}

/// A session's options, from `SelectSources`.
#[derive(Debug, Clone, Default)]
struct Options {
    types: u32,
    cursor_mode: Option<u32>,
}

type Sessions = Arc<Mutex<HashMap<String, Options>>>;

async fn run(
    bus: String,
    loop_: Sender<FromPortal>,
    mut from_loop: mpsc::UnboundedReceiver<ToPortal>,
    appearance: crate::appearance::portal::Values,
) {
    use crate::appearance::portal as settings;
    let sessions = Sessions::default();
    let values = settings::Shared::new(Mutex::new(appearance));
    let served = settings::Settings {
        values: values.clone(),
    };
    let cast = ScreenCast {
        loop_: loop_.clone(),
        sessions: sessions.clone(),
    };
    let builder = if bus == "session" {
        Builder::session()
    } else {
        Builder::address(bus.as_str())
    };
    let built = builder
        .serve_at(PATH, cast)
        .serve_at(PATH, served)
        .build()
        .await;
    let connection = match built {
        Ok(connection) => connection,
        Err(err) => {
            tracing::warn!(%err, "cannot reach the session bus: no screen cast portal");
            return;
        }
    };
    match connection
        .request_name_with_flags(NAME, RequestNameFlags::REPLACE_EXISTING)
        .await
    {
        Ok(reply) => tracing::info!(?reply, "the screen cast portal is served"),
        Err(err) => {
            tracing::warn!(%err, "cannot own {NAME}: no screen cast portal");
            return;
        }
    }
    while let Some(said) = from_loop.recv().await {
        match said {
            ToPortal::Close { session } => {
                sessions
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&session);
                close_session(&connection, &session).await;
            }
            ToPortal::Appearance(new) => settings::changed(&connection, &values, new).await,
        }
    }
}

/// Session `session` is closed from the desktop's side: `Closed`, then its
/// object goes.
async fn close_session(connection: &Connection, session: &str) {
    let server = connection.object_server();
    let Ok(path) = zbus::ObjectPath::try_from(session) else {
        return;
    };
    let emitted = match SignalEmitter::new(connection, path.clone()) {
        Ok(emitter) => Session::closed(&emitter).await,
        Err(err) => Err(err),
    };
    if let Err(err) = emitted {
        tracing::warn!(%err, session, "the session's Closed not sent");
    }
    if let Err(err) = server.remove::<Session, _>(path).await {
        tracing::debug!(%err, session, "the session's object was gone already");
    }
    tracing::info!(session, "a screen cast session closed by the desktop");
}

fn uint(options: &HashMap<String, OwnedVariant>, key: &str) -> Option<u32> {
    options.get(key).and_then(|v| u32::try_from(&**v).ok())
}

type Results = HashMap<String, OwnedVariant>;

fn owned(value: Variant<'_>) -> Option<OwnedVariant> {
    OwnedVariant::try_from(value).ok()
}

struct ScreenCast {
    loop_: Sender<FromPortal>,
    sessions: Sessions,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.ScreenCast")]
impl ScreenCast {
    async fn create_session(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        _handle: OwnedObjectPath,
        session_handle: OwnedObjectPath,
        app_id: String,
        _options: HashMap<String, OwnedVariant>,
    ) -> (u32, Results) {
        let session = session_handle.as_str().to_owned();
        let object = Session {
            loop_: self.loop_.clone(),
            sessions: self.sessions.clone(),
            name: session.clone(),
        };
        if let Err(err) = server.at(&session_handle, object).await {
            tracing::warn!(%err, session, "a screen cast session not made");
            return (FAILED, Results::new());
        }
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(session.clone(), Options::default());
        tracing::info!(session, app_id, "a screen cast session");
        let mut results = Results::new();
        if let Some(id) = owned(Variant::from(session.as_str())) {
            results.insert("session_id".to_owned(), id);
        }
        (SUCCESS, results)
    }

    async fn select_sources(
        &self,
        _handle: OwnedObjectPath,
        session_handle: OwnedObjectPath,
        _app_id: String,
        options: HashMap<String, OwnedVariant>,
    ) -> (u32, Results) {
        let types = uint(&options, "types").unwrap_or(hocus_link::SHARE_MONITOR);
        let cursor_mode = uint(&options, "cursor_mode");
        let mut sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(session) = sessions.get_mut(session_handle.as_str()) else {
            return (FAILED, Results::new());
        };
        if cursor_mode.is_some_and(|m| m & !(HIDDEN | EMBEDDED) != 0) {
            tracing::info!(?cursor_mode, "a cursor mode not offered");
            return (FAILED, Results::new());
        }
        session.types = types;
        session.cursor_mode = cursor_mode;
        tracing::debug!(types, ?cursor_mode, "the screen cast's sources");
        (SUCCESS, Results::new())
    }

    async fn start(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        handle: OwnedObjectPath,
        session_handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        _options: HashMap<String, OwnedVariant>,
    ) -> (u32, Results) {
        let session = session_handle.as_str().to_owned();
        let options = self
            .sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&session)
            .cloned();
        let Some(options) = options else {
            return (FAILED, Results::new());
        };
        // The request: closing it cancels the chooser.
        let request = Request {
            loop_: self.loop_.clone(),
            session: session.clone(),
        };
        let exported = server.at(&handle, request).await.is_ok();
        let (reply, answer) = oneshot::channel();
        let asked = self.loop_.send(FromPortal::Start(StartAsk {
            session: session.clone(),
            app_id,
            parent_window,
            types: options.types,
            cursor_mode: options.cursor_mode,
            reply,
        }));
        let answer = match asked {
            Ok(()) => answer.await.unwrap_or(Answer::Failed),
            Err(_) => Answer::Failed,
        };
        if exported {
            let _ = server.remove::<Request, _>(&handle).await;
        }
        match answer {
            Answer::Streams(streams) => {
                let mut results = Results::new();
                let streams: Vec<(u32, HashMap<String, OwnedVariant>)> = streams
                    .into_iter()
                    .map(|s| {
                        let mut props = HashMap::new();
                        if let Some(at) = s.position.and_then(|p| owned(Variant::from(p))) {
                            props.insert("position".to_owned(), at);
                        }
                        if let Some(size) = owned(Variant::from(s.size)) {
                            props.insert("size".to_owned(), size);
                        }
                        if let Some(kind) = owned(Variant::from(s.source_type)) {
                            props.insert("source_type".to_owned(), kind);
                        }
                        (s.node, props)
                    })
                    .collect();
                if let Some(streams) = owned(Variant::from(streams)) {
                    results.insert("streams".to_owned(), streams);
                }
                (SUCCESS, results)
            }
            Answer::Cancelled => (CANCELLED, Results::new()),
            Answer::Failed => (FAILED, Results::new()),
        }
    }

    #[zbus(property, name = "AvailableSourceTypes")]
    fn available_source_types(&self) -> u32 {
        hocus_link::SHARE_MONITOR | hocus_link::SHARE_WINDOW
    }

    #[zbus(property, name = "AvailableCursorModes")]
    fn available_cursor_modes(&self) -> u32 {
        HIDDEN | EMBEDDED
    }

    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        5
    }
}

/// A screen cast session's object.
struct Session {
    loop_: Sender<FromPortal>,
    sessions: Sessions,
    name: String,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Session")]
impl Session {
    async fn close(&self, #[zbus(object_server)] server: &ObjectServer) {
        tracing::info!(session = self.name, "a screen cast session closed");
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.name);
        let _ = self.loop_.send(FromPortal::Closed {
            session: self.name.clone(),
        });
        if let Ok(path) = zbus::ObjectPath::try_from(self.name.as_str()) {
            let _ = server.remove::<Session, _>(path).await;
        }
    }

    #[zbus(signal)]
    async fn closed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        1
    }
}

/// A `Start`'s request's object.
struct Request {
    loop_: Sender<FromPortal>,
    session: String,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Request")]
impl Request {
    async fn close(&self) {
        tracing::info!(session = self.session, "a screen cast's request closed");
        let _ = self.loop_.send(FromPortal::Cancel {
            session: self.session.clone(),
        });
    }
}
