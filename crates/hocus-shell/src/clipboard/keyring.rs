//! The clipboard's history's key in the session's keyring (5.6): the
//! freedesktop Secret Service (`org.freedesktop.secrets`: GNOME Keyring,
//! KWallet, KeePassXC…), on the session bus the compositor names, never
//! another. One item, its attributes `application=hocuspanda` and
//! `hocuspanda=clipboard`, holding the key's 32 bytes, in the default
//! collection.
//!
//! Asked on the history's own thread, with a timeout: a keyring that hangs
//! or asks the user to unlock it and is never answered holds nothing up.
//! The secret goes over the bus as is (the `plain` session), as most
//! clients send theirs: the bus is the user's.

use std::{collections::HashMap, time::Duration};

use iced::futures::StreamExt;
use zbus::{
    Connection, MatchRule, MessageStream, ObjectPath, OwnedObjectPath, OwnedVariant, Variant,
    connection::Builder, message::Type,
};

const SERVICE: &str = "org.freedesktop.secrets";
const PATH: &str = "/org/freedesktop/secrets";
const IFACE: &str = "org.freedesktop.Secret.Service";
const COLLECTION: &str = "org.freedesktop.Secret.Collection";
const ITEM: &str = "org.freedesktop.Secret.Item";
const PROMPT: &str = "org.freedesktop.Secret.Prompt";
const ATTRIBUTES: [(&str, &str); 2] = [("application", "hocuspanda"), ("hocuspanda", "clipboard")];
/// How long the keyring has, an unlock asked of the user included.
const WAIT: Duration = Duration::from_secs(15);

/// What the keyring said.
#[derive(Debug)]
pub enum Found {
    Key(Vec<u8>),
    /// It answered, and holds no key of the history's.
    Absent,
    /// It did not answer, or not in time.
    NoAnswer,
}

/// The history's key, as the keyring on `bus` holds it.
pub fn find(bus: &str) -> Found {
    let found = run(bus, async |connection| {
        let session = open_session(&connection).await?;
        find_in(&connection, &session).await
    });
    match found {
        Some(Some(key)) if key.len() == 32 => Found::Key(key),
        Some(Some(_)) => {
            tracing::warn!("the keyring's key for the clipboard is not one");
            Found::NoAnswer
        }
        Some(None) => Found::Absent,
        None => Found::NoAnswer,
    }
}

/// Keeps `key` in the keyring on `bus`: whether it did.
pub fn keep(bus: &str, key: &[u8]) -> bool {
    let key = key.to_vec();
    run(bus, async move |connection| {
        let session = open_session(&connection).await?;
        keep_in(&connection, &session, key).await
    })
    .is_some()
}

/// `work` on a connection to `bus`, within [`WAIT`], on this thread.
fn run<T>(bus: &str, work: impl AsyncFnOnce(Connection) -> zbus::Result<T>) -> Option<T> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .inspect_err(|err| tracing::warn!(%err, "no runtime for the keyring"))
        .ok()?;
    runtime.block_on(async {
        let asked = async {
            let builder = if bus == "session" {
                Builder::session()
            } else {
                Builder::address(bus)
            };
            work(builder.build().await?).await
        };
        match tokio::time::timeout(WAIT, asked).await {
            Ok(Ok(found)) => Some(found),
            Ok(Err(err)) => {
                tracing::info!(%err, "the keyring does not answer");
                None
            }
            Err(_) => {
                tracing::info!("the keyring did not answer in time");
                None
            }
        }
    })
}

async fn call<B>(
    connection: &Connection,
    path: &str,
    interface: &str,
    method: &str,
    body: &B,
) -> zbus::Result<zbus::Message>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    connection
        .call_method(Some(SERVICE), path, Some(interface), method, body)
        .await
}

async fn open_session(connection: &Connection) -> zbus::Result<OwnedObjectPath> {
    let reply = call(
        connection,
        PATH,
        IFACE,
        "OpenSession",
        &("plain", Variant::from("")),
    )
    .await?;
    let (_, session): (OwnedVariant, OwnedObjectPath) = reply.body().deserialize()?;
    Ok(session)
}

fn attributes() -> HashMap<&'static str, &'static str> {
    ATTRIBUTES.into_iter().collect()
}

async fn find_in(
    connection: &Connection,
    session: &OwnedObjectPath,
) -> zbus::Result<Option<Vec<u8>>> {
    let reply = call(connection, PATH, IFACE, "SearchItems", &(attributes(),)).await?;
    let (unlocked, locked): (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) =
        reply.body().deserialize()?;
    let item = match (unlocked.first(), locked.first()) {
        (Some(item), _) => item.clone(),
        (None, Some(item)) => {
            let reply = call(connection, PATH, IFACE, "Unlock", &(vec![item.clone()],)).await?;
            let (_, prompt): (Vec<OwnedObjectPath>, OwnedObjectPath) =
                reply.body().deserialize()?;
            prompted(connection, &prompt).await?;
            item.clone()
        }
        (None, None) => return Ok(None),
    };
    let reply = call(connection, item.as_str(), ITEM, "GetSecret", &(session,)).await?;
    let (_, _, value, _): (OwnedObjectPath, Vec<u8>, Vec<u8>, String) =
        reply.body().deserialize()?;
    Ok(Some(value))
}

async fn keep_in(
    connection: &Connection,
    session: &OwnedObjectPath,
    key: Vec<u8>,
) -> zbus::Result<()> {
    let reply = call(connection, PATH, IFACE, "ReadAlias", &("default",)).await?;
    let collection: OwnedObjectPath = reply.body().deserialize()?;
    if collection.as_str() == "/" {
        return Err(zbus::Error::Failure("no default collection".to_owned()));
    }
    let mut properties: HashMap<&str, Variant<'_>> = HashMap::new();
    properties.insert(
        "org.freedesktop.Secret.Item.Label",
        Variant::from("Hocuspanda: the clipboard's history"),
    );
    properties.insert(
        "org.freedesktop.Secret.Item.Attributes",
        Variant::from(attributes()),
    );
    let secret = (
        ObjectPath::from(session.clone()),
        Vec::<u8>::new(),
        key,
        "application/octet-stream",
    );
    let reply = call(
        connection,
        collection.as_str(),
        COLLECTION,
        "CreateItem",
        &(properties, secret, true),
    )
    .await?;
    let (_, prompt): (OwnedObjectPath, OwnedObjectPath) = reply.body().deserialize()?;
    prompted(connection, &prompt).await
}

/// What `prompt` asks the user, if anything ("/": nothing), answered.
async fn prompted(connection: &Connection, prompt: &OwnedObjectPath) -> zbus::Result<()> {
    if prompt.as_str() == "/" {
        return Ok(());
    }
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(PROMPT)
        .member("Completed")
        .path(prompt.as_str())
        .build()?;
    let mut completed = MessageStream::for_match_rule(rule, connection, None).await?;
    call(connection, prompt.as_str(), PROMPT, "Prompt", &("",)).await?;
    let Some(signal) = completed.next().await else {
        return Err(zbus::Error::Failure("the prompt went".to_owned()));
    };
    let (dismissed, _): (bool, OwnedVariant) = signal?.body().deserialize()?;
    if dismissed {
        Err(zbus::Error::Failure(
            "the user dismissed the prompt".to_owned(),
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bus_that_is_not_there_is_no_answer() {
        let dir = tempfile::tempdir().expect("a directory");
        let bus = format!("unix:path={}", dir.path().join("none").display());
        assert!(matches!(find(&bus), Found::NoAnswer));
        assert!(!keep(&bus, &[0; 32]));
    }
}
