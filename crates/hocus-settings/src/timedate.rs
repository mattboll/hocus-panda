//! systemd's timedated (`org.freedesktop.timedate1`) on the system bus,
//! followed and asked on a thread of its own, never the window's (6.11):
//! the zone, whether the time is set automatically and is right, whether
//! the computer can set it so.
//!
//! What it says reaches the window only when it changes: the window draws
//! on every message, and draws nothing at rest. Its properties are read
//! again whenever it says one changed; `NTPSynchronized`, which it never
//! says, is read again every few seconds only while the automatic time is
//! on and not right yet. Without timedated (or the bus), the window is
//! told so once and the thread tries again, slower and slower, silently.
//!
//! The calls are interactive: polkit may ask for an administrator's
//! password (the system's authentication dialog, 7.5), which each call's
//! answer waits for, aside, while the following goes on.

use std::{collections::HashMap, time::Duration};

use iced::futures::{
    StreamExt as _,
    channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded},
    stream,
};
use zbus::{
    Connection, MatchRule, MessageStream, OwnedVariant, connection::Builder, message::Type,
};

const DEST: &str = "org.freedesktop.timedate1";
const PATH: &str = "/org/freedesktop/timedate1";
const IFACE: &str = "org.freedesktop.timedate1";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// How often `NTPSynchronized` is read while the time is not right yet.
const NOT_RIGHT_YET: Duration = Duration::from_secs(5);
/// Tried again after this long without timedated, up to a minute.
const RETRY: Duration = Duration::from_secs(2);
const RETRY_MOST: Duration = Duration::from_secs(60);

/// What timedated says.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TimeDate {
    /// `Europe/Paris`.
    pub zone: String,
    /// The time is set automatically (`NTP`).
    pub ntp: bool,
    /// And is right (`NTPSynchronized`).
    pub synced: bool,
    /// A time service is there to set it (`CanNTP`).
    pub can_ntp: bool,
}

/// What the window asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    Ntp(bool),
    Zone(String),
    /// The time, in microseconds since the epoch.
    Time(i64),
}

/// What the thread says to the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// timedated as it is now; `None`: not there.
    State(Option<TimeDate>),
    /// A call answered: `Err` holds why it was refused (polkit's
    /// refusal, a cancel, a zone the system does not know).
    Done(Ask, Result<(), String>),
}

/// Where the window's asks go.
pub type Asks = UnboundedSender<Ask>;

/// timedated followed on a thread of its own: what it says goes to `say`
/// (which says `false` once the window is gone, and the thread ends).
pub fn start(say: impl FnMut(Heard) -> bool + Send + 'static) -> Asks {
    let (asks, incoming) = unbounded();
    let started = std::thread::Builder::new()
        .name("timedated".to_owned())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    tracing::warn!(%err, "no runtime for timedated");
                    return;
                }
            };
            runtime.block_on(follow(incoming, say));
        });
    if let Err(err) = started {
        tracing::warn!(%err, "timedated is not followed");
    }
    asks
}

/// What wakes the thread.
enum Event {
    Ask(Ask),
    Changed,
    Answered(Ask, Result<(), String>),
    /// The window's asks are gone: so is the window.
    Gone,
    /// The bus went away.
    Lost,
}

async fn follow(mut incoming: UnboundedReceiver<Ask>, mut say: impl FnMut(Heard) -> bool) {
    // The last state said, so that only a change is said again.
    let mut said: Option<Option<TimeDate>> = None;
    let mut tell = |state: Option<TimeDate>, say: &mut dyn FnMut(Heard) -> bool| -> bool {
        if said.as_ref() == Some(&state) {
            return true;
        }
        said = Some(state.clone());
        say(Heard::State(state))
    };
    let mut wait = RETRY;
    loop {
        let (connection, changes) = match connect().await {
            Ok(connected) => connected,
            Err(err) => {
                tracing::info!(%err, "no timedated");
                if !tell(None, &mut say) {
                    return;
                }
                // Asks meanwhile are refused at once.
                let until = tokio::time::Instant::now() + wait;
                loop {
                    match tokio::time::timeout_at(until, incoming.next()).await {
                        Err(_) => break,
                        Ok(None) => return,
                        Ok(Some(ask)) => {
                            if !say(Heard::Done(ask, Err("timedated".to_owned()))) {
                                return;
                            }
                        }
                    }
                }
                wait = (wait * 2).min(RETRY_MOST);
                continue;
            }
        };
        wait = RETRY;
        let (answers, answered) = unbounded::<(Ask, Result<(), String>)>();
        let mut state = read(&connection).await;
        if !tell(state.clone(), &mut say) {
            return;
        }
        let mut events = stream::select_all([
            incoming
                .by_ref()
                .map(Event::Ask)
                .chain(stream::once(async { Event::Gone }))
                .boxed(),
            changes
                .map(|_| Event::Changed)
                .chain(stream::once(async { Event::Lost }))
                .boxed(),
            answered
                .map(|(ask, result)| Event::Answered(ask, result))
                .boxed(),
        ]);
        loop {
            let polling = state.as_ref().is_some_and(|s| s.ntp && !s.synced);
            let event = if polling {
                match tokio::time::timeout(NOT_RIGHT_YET, events.next()).await {
                    Ok(event) => event,
                    Err(_) => Some(Event::Changed),
                }
            } else {
                events.next().await
            };
            match event {
                None | Some(Event::Gone) => return,
                Some(Event::Lost) => break,
                Some(Event::Ask(ask)) => {
                    let connection = connection.clone();
                    let answers = answers.clone();
                    tokio::spawn(async move {
                        let result = call(&connection, &ask).await;
                        let _ = answers.unbounded_send((ask, result));
                    });
                }
                Some(Event::Answered(ask, result)) => {
                    if let Err(why) = &result {
                        tracing::info!(?ask, why, "timedated refused");
                    }
                    // What it holds now, before the answer is said.
                    state = read(&connection).await;
                    if !tell(state.clone(), &mut say) || !say(Heard::Done(ask, result)) {
                        return;
                    }
                }
                Some(Event::Changed) => {
                    state = read(&connection).await;
                    if !tell(state.clone(), &mut say) {
                        return;
                    }
                }
            }
        }
        tracing::info!("timedated's bus went away");
    }
}

/// The system bus, and timedated's `PropertiesChanged` listened to.
async fn connect() -> zbus::Result<(Connection, MessageStream)> {
    let connection = Builder::system().build().await?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(PROPERTIES)
        .member("PropertiesChanged")
        .path(PATH)
        .arg(0, IFACE)
        .build()?;
    let changes = MessageStream::for_match_rule(rule, &connection, None).await?;
    Ok((connection, changes))
}

/// timedated's properties now; `None` when it does not answer.
async fn read(connection: &Connection) -> Option<TimeDate> {
    let reply = connection
        .call_method(Some(DEST), PATH, Some(PROPERTIES), "GetAll", &(IFACE,))
        .await;
    let all: HashMap<String, OwnedVariant> = match reply {
        Ok(reply) => match reply.body().deserialize() {
            Ok(all) => all,
            Err(err) => {
                tracing::warn!(%err, "timedated's properties do not read");
                return None;
            }
        },
        Err(err) => {
            tracing::info!(%err, "timedated does not answer");
            return None;
        }
    };
    let flag = |name: &str| {
        all.get(name)
            .and_then(|v| bool::try_from(v.clone()).ok())
            .unwrap_or(false)
    };
    let zone = all
        .get("Timezone")
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_default();
    Some(TimeDate {
        zone,
        ntp: flag("NTP"),
        synced: flag("NTPSynchronized"),
        can_ntp: flag("CanNTP"),
    })
}

/// `ask` made of timedated, interactively; why it was refused.
async fn call(connection: &Connection, ask: &Ask) -> Result<(), String> {
    let called = match ask {
        Ask::Ntp(on) => {
            connection
                .call_method(Some(DEST), PATH, Some(IFACE), "SetNTP", &(*on, true))
                .await
        }
        Ask::Zone(zone) => {
            connection
                .call_method(
                    Some(DEST),
                    PATH,
                    Some(IFACE),
                    "SetTimezone",
                    &(zone.as_str(), true),
                )
                .await
        }
        Ask::Time(usec) => {
            connection
                .call_method(
                    Some(DEST),
                    PATH,
                    Some(IFACE),
                    "SetTime",
                    &(*usec, false, true),
                )
                .await
        }
    };
    called.map(|_| ()).map_err(|err| match err {
        zbus::Error::MethodError(name, message, _) => {
            message.unwrap_or_else(|| name.as_str().to_owned())
        }
        other => other.to_string(),
    })
}
