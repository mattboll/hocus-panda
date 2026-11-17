//! A transcription's notification, off the loop: one for the whole job,
//! replaced as it goes (its progress, then "Transcription prête" or "La
//! transcription n'a pas pu se faire"), on a thread of its own that holds
//! its connection to the notifications' bus while its actions may be
//! chosen (the server shows them only while their sender holds them), and
//! tells the loop which one was ([`FromWorkers::Acted`]).

use std::collections::HashMap;

use futures_util::StreamExt as _;
use smithay::reexports::calloop::channel::Sender;
use zbus::{MatchRule, MessageStream, message::Type};

use super::FromWorkers;
use crate::shell::Bus;

const NOTIFICATIONS: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

/// What the notification says now.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub summary: String,
    pub body: String,
    /// Its progress bar, 0 to 100.
    pub value: Option<i32>,
    /// Its buttons: key, label (`default` is the click on it).
    pub actions: Vec<(&'static str, String)>,
    /// It stays until it is replaced (the progress), else as long as the
    /// server keeps it.
    pub stays: bool,
}

/// The loop's end of a job's notification.
#[derive(Debug)]
pub struct Notes {
    tx: tokio::sync::mpsc::UnboundedSender<Note>,
}

impl Notes {
    /// The notification says `note` (replacing what it said).
    pub fn say(&self, note: Note) {
        let _ = self.tx.send(note);
    }
}

/// What reaches the notification's thread.
enum Heard {
    Note(Option<Note>),
    Signal(zbus::Message),
}

/// The notification of job `job` on `bus`, its thread started.
pub fn start(bus: Bus, job: u64, back: Sender<FromWorkers>) -> Notes {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let started = std::thread::Builder::new()
        .name("meeting's notification".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(run(bus, job, rx, back)),
                Err(err) => tracing::warn!(%err, "no runtime for a meeting's notification"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "a meeting's notification is not sent");
    }
    Notes { tx }
}

async fn run(
    bus: Bus,
    job: u64,
    rx: tokio::sync::mpsc::UnboundedReceiver<Note>,
    back: Sender<FromWorkers>,
) {
    let builder = match &bus {
        Bus::Given(address) => zbus::connection::Builder::address(address.as_str()),
        Bus::Session => zbus::connection::Builder::session(),
    };
    let connection = match builder.build().await {
        Ok(connection) => connection,
        Err(err) => {
            tracing::warn!(%err, "no session bus: a meeting's notification is not sent");
            return;
        }
    };
    // Listened to before anything is sent, so that no answer is missed.
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(NOTIFICATIONS)
        .path(PATH)
        .build();
    let signals = match rule {
        Ok(rule) => MessageStream::for_match_rule(rule, &connection, None)
            .await
            .ok(),
        Err(_) => None,
    };
    let notes = futures_util::stream::unfold(rx, |mut rx| async move {
        let note = rx.recv().await;
        Some((Heard::Note(note), rx))
    });
    type Heards = std::pin::Pin<Box<dyn futures_util::Stream<Item = Heard> + Send>>;
    let signals: Heards = match signals {
        Some(s) => Box::pin(s.filter_map(|m| async move { m.ok().map(Heard::Signal) })),
        None => Box::pin(futures_util::stream::pending()),
    };
    let notes: Heards = Box::pin(notes);
    let mut heard = futures_util::stream::select(notes, signals);
    // The server's id for it, and its buttons' keys: once it has some
    // (its last word), the thread ends as it is closed or acted on, but
    // for "Réessayer", after which it says the progress again.
    let mut id = 0u32;
    let mut actions: Vec<String> = Vec::new();
    while let Some(next) = heard.next().await {
        match next {
            Heard::Note(Some(note)) => {
                actions = note.actions.iter().map(|(k, _)| (*k).to_owned()).collect();
                match notify(&connection, id, &note).await {
                    Ok(new) => id = new,
                    Err(err) => tracing::warn!(%err, "a meeting's notification is not sent"),
                }
            }
            // The loop let go of it.
            Heard::Note(None) => return,
            Heard::Signal(message) => {
                let header = message.header();
                let member = header.member().map(|m| m.as_str());
                if member == Some("ActionInvoked")
                    && let Ok((of, key)) = message.body().deserialize::<(u32, String)>()
                    && of == id
                    && actions.contains(&key)
                {
                    let again = key == "retry";
                    let _ = back.send(FromWorkers::Acted { id: job, key });
                    if !again {
                        return;
                    }
                } else if member == Some("NotificationClosed")
                    && message
                        .body()
                        .deserialize::<(u32, u32)>()
                        .is_ok_and(|(of, _)| of == id)
                {
                    let _ = back.send(FromWorkers::Closed { id: job });
                    id = 0;
                    if !actions.is_empty() {
                        return;
                    }
                }
            }
        }
    }
}

async fn notify(connection: &zbus::Connection, replaces: u32, note: &Note) -> zbus::Result<u32> {
    let mut hints: HashMap<&str, zbus::Variant<'_>> = HashMap::new();
    hints.insert("urgency", zbus::Variant::from(1u8));
    hints.insert("desktop-entry", zbus::Variant::from("hocus-settings"));
    if let Some(value) = note.value {
        hints.insert("value", zbus::Variant::from(value.clamp(0, 100)));
    }
    let listed: Vec<&str> = note
        .actions
        .iter()
        .flat_map(|(key, label)| [*key, label.as_str()])
        .collect();
    let reply = connection
        .call_method(
            Some(NOTIFICATIONS),
            PATH,
            Some(NOTIFICATIONS),
            "Notify",
            &(
                "Hocuspanda",
                replaces,
                "",
                note.summary.as_str(),
                note.body.as_str(),
                listed,
                hints,
                if note.stays { 0i32 } else { -1i32 },
            ),
        )
        .await?;
    reply.body().deserialize::<u32>()
}
