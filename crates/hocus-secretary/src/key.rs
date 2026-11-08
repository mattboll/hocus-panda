//! Her memory's key: 32 random bytes, in the session's keyring (the
//! freedesktop Secret Service, `org.freedesktop.secrets`) on the session
//! bus the compositor names ([`hocus_secretary_link::BUS_ENV`]), never
//! another; else, without a bus or when the keyring does not answer as
//! her memory is first made, in a key file beside it, `memory.key`
//! (`0600`), made once and kept, as the clipboard's history does. Once
//! there, a key file is used from then on.
//!
//! The keyring is asked on her own thread, with a timeout: one that hangs,
//! or asks the user to unlock it and is never answered, leaves her memory
//! closed (she exits, and the compositor starts her again later), never a
//! new key over an old memory.

use std::{
    collections::HashMap,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    time::Duration,
};

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
pub const ATTRIBUTES: [(&str, &str); 2] =
    [("application", "hocuspanda"), ("hocuspanda", "secretary")];
/// How long the keyring has, an unlock asked of the user included.
const WAIT: Duration = Duration::from_secs(15);

/// The key file's name, beside her memory.
pub const FILE: &str = "memory.key";

pub type Key = [u8; 32];

/// Where the key is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Keyring,
    File,
}

impl Place {
    pub const fn word(self) -> &'static str {
        match self {
            Self::Keyring => "keyring",
            Self::File => "file",
        }
    }
}

/// Her memory's key for the folder `dir`, the keyring on `bus` asked when
/// one is given; `made`: her memory is there already (a new key would not
/// open it).
pub fn key(dir: &Path, bus: Option<&str>, made: bool) -> Result<(Key, Place), String> {
    let file = dir.join(FILE);
    match std::fs::read(&file) {
        Ok(bytes) => {
            return Key::try_from(bytes.as_slice())
                .map(|key| (key, Place::File))
                .map_err(|_| format!("{} is not a key", file.display()));
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(format!("{}: {err}", file.display())),
    }
    if let Some(bus) = bus {
        match find(bus) {
            Found::Key(key) => return Ok((key, Place::Keyring)),
            Found::Absent if made => {
                return Err("the keyring holds no key for her memory".to_owned());
            }
            Found::Absent => {
                let key = fresh()?;
                if keep(bus, &key) {
                    return Ok((key, Place::Keyring));
                }
            }
            Found::NoAnswer if made => {
                return Err("the keyring does not answer: her memory stays closed".to_owned());
            }
            Found::NoAnswer => {}
        }
    } else if made {
        return Err(format!(
            "{} is gone: her memory cannot be opened",
            file.display()
        ));
    }
    let key = fresh()?;
    write_private(&file, &key).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok((key, Place::File))
}

fn fresh() -> Result<Key, String> {
    let mut key = [0u8; 32];
    getrandom::fill(&mut key).map_err(|e| format!("no random bytes: {e}"))?;
    Ok(key)
}

/// Writes `bytes` to a new file only the user reads.
pub fn write_private(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(file)?;
    out.write_all(bytes)?;
    out.sync_all()?;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600))
}

/// What the keyring said.
#[derive(Debug)]
pub enum Found {
    Key(Key),
    /// It answered, and holds no key of hers.
    Absent,
    /// It did not answer, or not in time.
    NoAnswer,
}

pub fn find(bus: &str) -> Found {
    let found = run(bus, async |connection| {
        let session = open_session(&connection).await?;
        find_in(&connection, &session).await
    });
    match found {
        Some(Some(bytes)) => match Key::try_from(bytes.as_slice()) {
            Ok(key) => Found::Key(key),
            Err(_) => {
                tracing::warn!("the keyring's key for her memory is not one");
                Found::NoAnswer
            }
        },
        Some(None) => Found::Absent,
        None => Found::NoAnswer,
    }
}

/// Keeps `key` in the keyring on `bus`: whether it did.
pub fn keep(bus: &str, key: &Key) -> bool {
    let key = key.to_vec();
    run(bus, async move |connection| {
        let session = open_session(&connection).await?;
        keep_in(&connection, &session, key).await
    })
    .is_some()
}

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
        Variant::from("Hocuspanda: the secretary's memory"),
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
    let completed = MessageStream::for_match_rule(rule, connection, None).await?;
    call(connection, prompt.as_str(), PROMPT, "Prompt", &("",)).await?;
    let mut completed = std::pin::pin!(completed);
    let next = std::future::poll_fn(|cx| {
        use zbus::export::futures_core::Stream;
        completed.as_mut().poll_next(cx)
    });
    let Some(signal) = next.await else {
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
    fn without_a_bus_the_key_is_a_private_file_made_once() {
        let dir = tempfile::tempdir().expect("a directory");
        let (key, place) = key(dir.path(), None, false).expect("a key");
        assert_eq!(place, Place::File);
        let mode = std::fs::metadata(dir.path().join(FILE))
            .expect("the file")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(super::key(dir.path(), None, true).expect("again").0, key);
        std::fs::remove_file(dir.path().join(FILE)).expect("removed");
        assert!(
            super::key(dir.path(), None, true).is_err(),
            "no new key over a memory"
        );
    }

    #[test]
    fn a_bus_that_is_not_there_is_no_answer() {
        let dir = tempfile::tempdir().expect("a directory");
        let bus = format!("unix:path={}", dir.path().join("none").display());
        assert!(matches!(find(&bus), Found::NoAnswer));
        assert!(key(dir.path(), Some(&bus), true).is_err());
        assert_eq!(
            key(dir.path(), Some(&bus), false).expect("a file").1,
            Place::File
        );
    }
}
