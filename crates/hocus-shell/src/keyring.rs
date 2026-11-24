//! The session's keyring: the freedesktop Secret Service
//! (`org.freedesktop.secrets`: GNOME Keyring, KWallet, KeePassXC…), on the
//! session bus the compositor names, never another. Items are found,
//! kept and forgotten by their attributes, in the default collection; a
//! locked one is unlocked through the keyring's own prompt.
//!
//! Async, for a thread's own runtime (the clipboard's history's, the
//! NetworkManager agent's), each call within [`WAIT`]: a keyring that
//! hangs, or asks the user to unlock it and is never answered, holds
//! nothing up. The secret goes over the bus as is (the `plain` session),
//! as most clients send theirs: the bus is the user's.

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
/// How long the keyring has, an unlock asked of the user included.
pub const WAIT: Duration = Duration::from_secs(15);

pub type Attributes = HashMap<String, String>;

/// A session with the keyring on a bus.
pub struct Keyring {
    connection: Connection,
    session: OwnedObjectPath,
}

impl Keyring {
    /// The keyring on `bus` (an address, or `session`).
    pub async fn open(bus: &str) -> zbus::Result<Self> {
        let builder = if bus == "session" {
            Builder::session()
        } else {
            Builder::address(bus)
        };
        let connection = builder.build().await?;
        let reply = call(
            &connection,
            PATH,
            IFACE,
            "OpenSession",
            &("plain", Variant::from("")),
        )
        .await?;
        let (_, session): (OwnedVariant, OwnedObjectPath) = reply.body().deserialize()?;
        Ok(Self {
            connection,
            session,
        })
    }

    async fn search(
        &self,
        attributes: &Attributes,
    ) -> zbus::Result<(Vec<OwnedObjectPath>, Vec<OwnedObjectPath>)> {
        let reply = call(&self.connection, PATH, IFACE, "SearchItems", &(attributes,)).await?;
        reply.body().deserialize()
    }

    /// The secret of the first item with `attributes`, unlocked if it must
    /// be; `None` when no item has them.
    pub async fn find(&self, attributes: &Attributes) -> zbus::Result<Option<Vec<u8>>> {
        let (unlocked, locked) = self.search(attributes).await?;
        let item = match (unlocked.first(), locked.first()) {
            (Some(item), _) => item.clone(),
            (None, Some(item)) => {
                let reply = call(
                    &self.connection,
                    PATH,
                    IFACE,
                    "Unlock",
                    &(vec![item.clone()],),
                )
                .await?;
                let (_, prompt): (Vec<OwnedObjectPath>, OwnedObjectPath) =
                    reply.body().deserialize()?;
                prompted(&self.connection, &prompt).await?;
                item.clone()
            }
            (None, None) => return Ok(None),
        };
        let reply = call(
            &self.connection,
            item.as_str(),
            ITEM,
            "GetSecret",
            &(&self.session,),
        )
        .await?;
        let (_, _, value, _): (OwnedObjectPath, Vec<u8>, Vec<u8>, String) =
            reply.body().deserialize()?;
        Ok(Some(value))
    }

    /// Keeps `secret` (of `content_type`) under `label` and `attributes`,
    /// in place of an item that had them.
    pub async fn keep(
        &self,
        label: &str,
        attributes: &Attributes,
        secret: Vec<u8>,
        content_type: &str,
    ) -> zbus::Result<()> {
        let reply = call(&self.connection, PATH, IFACE, "ReadAlias", &("default",)).await?;
        let collection: OwnedObjectPath = reply.body().deserialize()?;
        if collection.as_str() == "/" {
            return Err(zbus::Error::Failure("no default collection".to_owned()));
        }
        let mut properties: HashMap<&str, Variant<'_>> = HashMap::new();
        properties.insert("org.freedesktop.Secret.Item.Label", Variant::from(label));
        properties.insert(
            "org.freedesktop.Secret.Item.Attributes",
            Variant::from(attributes.clone()),
        );
        let secret = (
            ObjectPath::from(self.session.clone()),
            Vec::<u8>::new(),
            secret,
            content_type,
        );
        let reply = call(
            &self.connection,
            collection.as_str(),
            COLLECTION,
            "CreateItem",
            &(properties, secret, true),
        )
        .await?;
        let (_, prompt): (OwnedObjectPath, OwnedObjectPath) = reply.body().deserialize()?;
        prompted(&self.connection, &prompt).await
    }

    /// Deletes every item with `attributes`.
    pub async fn forget(&self, attributes: &Attributes) -> zbus::Result<()> {
        let (unlocked, locked) = self.search(attributes).await?;
        for item in unlocked.iter().chain(&locked) {
            let reply = call(&self.connection, item.as_str(), ITEM, "Delete", &()).await?;
            let prompt: OwnedObjectPath = reply.body().deserialize()?;
            prompted(&self.connection, &prompt).await?;
        }
        Ok(())
    }
}

/// `work` on the keyring on `bus`, within [`WAIT`]; `None` when it does
/// not answer, or not in time (said in the log).
pub async fn within<T>(bus: &str, work: impl AsyncFnOnce(Keyring) -> zbus::Result<T>) -> Option<T> {
    let asked = async { work(Keyring::open(bus).await?).await };
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
}

/// [`within`] on a runtime of its own, on this thread (never the loop's).
pub fn blocking<T>(bus: &str, work: impl AsyncFnOnce(Keyring) -> zbus::Result<T>) -> Option<T> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .inspect_err(|err| tracing::warn!(%err, "no runtime for the keyring"))
        .ok()?;
    runtime.block_on(within(bus, work))
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
