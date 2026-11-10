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
//!
//! **Wiped once used** (#768). Her memory's key ([`Key`]) and a provider's
//! API key are overwritten with zeros (`zeroize`) as soon as what held
//! them goes: the key file's bytes as read, the keyring's answer, the
//! line typed on a terminal. A [`Key`] is not `Copy` nor `Clone`, and its
//! bytes are on the heap: moving it copies a pointer, never the key. What
//! is out of reach stays so: the D-Bus library's own message buffers,
//! standard input's, and SQLCipher's copy, which it wipes itself as her
//! memory closes.

use std::{
    collections::HashMap,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    time::Duration,
};

use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

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

/// Her memory's key: 32 bytes, overwritten with zeros when it goes;
/// never printed, never copied by an assignment.
#[derive(PartialEq, Eq)]
pub struct Key(Box<[u8; Key::LEN]>);

impl Key {
    pub const LEN: usize = 32;

    fn blank() -> Self {
        Self(Box::new([0; Self::LEN]))
    }

    /// The key these bytes are, when there are [`Key::LEN`] of them.
    pub fn of(bytes: &[u8]) -> Option<Self> {
        (bytes.len() == Self::LEN).then(|| {
            let mut key = Self::blank();
            key.0.copy_from_slice(bytes);
            key
        })
    }

    pub fn expose(&self) -> &[u8; Self::LEN] {
        &self.0
    }
}

impl std::ops::Deref for Key {
    type Target = [u8; Key::LEN];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key(…)")
    }
}

impl PartialEq<Key> for Vec<u8> {
    fn eq(&self, other: &Key) -> bool {
        self.as_slice() == other.0.as_slice()
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl ZeroizeOnDrop for Key {}

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
            let bytes = Zeroizing::new(bytes);
            return Key::of(&bytes)
                .map(|key| (key, Place::File))
                .ok_or_else(|| format!("{} is not a key", file.display()));
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
    write_private(&file, key.expose()).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok((key, Place::File))
}

fn fresh() -> Result<Key, String> {
    let mut key = Key::blank();
    getrandom::fill(key.0.as_mut_slice()).map_err(|e| format!("no random bytes: {e}"))?;
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
        find_in(&connection, &session, attributes()).await
    });
    match found {
        Some(Some(bytes)) => match Key::of(&bytes) {
            Some(key) => Found::Key(key),
            None => {
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
    let key = Zeroizing::new(key.to_vec());
    run(bus, async move |connection| {
        let session = open_session(&connection).await?;
        keep_in(
            &connection,
            &session,
            attributes(),
            "Hocuspanda: the secretary's memory",
            key,
        )
        .await
    })
    .is_some()
}

/// Said when a key follows `hocus-secretary key PROVIDER` on its line
/// (or takes PROVIDER's place): never the argument itself.
pub const ON_THE_LINE: &str = "La clé ne se met pas sur la ligne de commande : lance « hocus-secretary key anthropic », Entrée, puis colle-la quand elle te la demande. Ta clé est peut-être restée dans l’historique de ton terminal : tu peux l’en retirer (sous bash : history -d, sous zsh : en éditant ~/.zsh_history).\n\
The key never goes on the command line: run “hocus-secretary key anthropic”, Enter, then paste it when asked. Your key may have stayed in your shell’s history: you may want to remove it from there (bash: history -d; zsh: edit ~/.zsh_history).";

/// Whether `word` looks like an API key rather than a provider's name
/// (`hocus-secretary key sk-ant-…`).
pub fn looks_like_a_key(word: &str) -> bool {
    word.starts_with("sk-") || word.len() > 40
}

/// A provider's name as the user reads it.
pub fn provider_name(provider: &str) -> String {
    match provider {
        "anthropic" => "Anthropic".to_owned(),
        "openai" => "OpenAI".to_owned(),
        other => other.to_owned(),
    }
}

/// What the terminal says before the key is pasted.
pub fn prompt(provider: &str, fr: bool) -> String {
    let name = provider_name(provider);
    if fr {
        format!("Colle ta clé {name} puis Entrée : ")
    } else {
        format!("Paste your {name} key, then Enter: ")
    }
}

/// A line of standard input: on a terminal, after `prompt` (on standard
/// error) and with the echo off, back on after, whatever happened; else
/// read as it comes, silently. The line is wiped when it goes.
pub fn read_secret(prompt: &str) -> std::io::Result<Zeroizing<String>> {
    use rustix::termios::{LocalModes, OptionalActions, tcgetattr, tcsetattr};
    use std::io::{BufRead as _, IsTerminal as _};
    let stdin = std::io::stdin();
    // Room for any key at once: a line that grows leaves its shorter
    // copies behind, unwiped.
    let mut line = Zeroizing::new(String::with_capacity(4096));
    if !stdin.is_terminal() {
        stdin.lock().read_line(&mut line)?;
        return Ok(line);
    }
    let before = tcgetattr(&stdin)?;
    let mut quiet = before.clone();
    quiet.local_modes.remove(LocalModes::ECHO);
    // The Enter typed still moves to the next line.
    quiet.local_modes.insert(LocalModes::ECHONL);
    tcsetattr(&stdin, OptionalActions::Flush, &quiet)?;
    let mut err = std::io::stderr();
    let read = err
        .write_all(prompt.as_bytes())
        .and_then(|()| err.flush())
        .and_then(|()| stdin.lock().read_line(&mut line));
    let restored = tcsetattr(&stdin, OptionalActions::Now, &before);
    read?;
    restored?;
    Ok(line)
}

/// A provider's API key's attributes in the keyring (§11.3).
pub fn provider_attributes(provider: &str) -> HashMap<&str, &str> {
    HashMap::from([
        ("application", "hocuspanda"),
        ("hocuspanda", "secretary-provider"),
        ("provider", provider),
    ])
}

/// `provider`'s API key in the keyring on `bus`: `Ok(None)` when it holds
/// none, an error when it does not answer. Read on a model call's worker
/// thread, never her loop. Wiped when it goes.
pub fn provider_key(bus: &str, provider: &str) -> Result<Option<Zeroizing<String>>, String> {
    let provider = provider.to_owned();
    let found = run(bus, async move |connection| {
        let session = open_session(&connection).await?;
        find_in(&connection, &session, provider_attributes(&provider)).await
    })
    .ok_or("the keyring does not answer")?;
    found
        .map(|bytes| {
            std::str::from_utf8(&bytes)
                .map(|k| Zeroizing::new(k.trim().to_owned()))
                .map_err(|_| "the keyring's key is not text".to_owned())
        })
        .transpose()
}

/// Keeps `secret` as `provider`'s API key in the keyring on `bus`:
/// whether it did (`hocus-secretary key PROVIDER`, Settings).
pub fn keep_provider_key(bus: &str, provider: &str, secret: &str) -> bool {
    let (provider, secret) = (
        provider.to_owned(),
        Zeroizing::new(secret.as_bytes().to_vec()),
    );
    run(bus, async move |connection| {
        let session = open_session(&connection).await?;
        let label = format!("Hocuspanda: the secretary's {provider} key");
        keep_in(
            &connection,
            &session,
            provider_attributes(&provider),
            &label,
            secret,
        )
        .await
    })
    .is_some()
}

/// Takes `provider`'s API key out of the keyring on `bus`: whether it
/// answered (a keyring holding none answers too).
pub fn erase_provider_key(bus: &str, provider: &str) -> bool {
    let provider = provider.to_owned();
    run(bus, async move |connection| {
        let reply = call(
            &connection,
            PATH,
            IFACE,
            "SearchItems",
            &(provider_attributes(&provider),),
        )
        .await?;
        let (unlocked, locked): (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) =
            reply.body().deserialize()?;
        for item in unlocked.iter().chain(&locked) {
            let reply = call(&connection, item.as_str(), ITEM, "Delete", &()).await?;
            let prompt: OwnedObjectPath = reply.body().deserialize()?;
            prompted(&connection, &prompt).await?;
        }
        Ok(())
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
    attributes: HashMap<&str, &str>,
) -> zbus::Result<Option<Zeroizing<Vec<u8>>>> {
    let reply = call(connection, PATH, IFACE, "SearchItems", &(attributes,)).await?;
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
    Ok(Some(Zeroizing::new(value)))
}

async fn keep_in(
    connection: &Connection,
    session: &OwnedObjectPath,
    attributes: HashMap<&str, &str>,
    label: &str,
    key: Zeroizing<Vec<u8>>,
) -> zbus::Result<()> {
    let reply = call(connection, PATH, IFACE, "ReadAlias", &("default",)).await?;
    let collection: OwnedObjectPath = reply.body().deserialize()?;
    if collection.as_str() == "/" {
        return Err(zbus::Error::Failure("no default collection".to_owned()));
    }
    let mut properties: HashMap<&str, Variant<'_>> = HashMap::new();
    properties.insert("org.freedesktop.Secret.Item.Label", Variant::from(label));
    properties.insert(
        "org.freedesktop.Secret.Item.Attributes",
        Variant::from(attributes),
    );
    let secret = (
        ObjectPath::from(session.clone()),
        Vec::<u8>::new(),
        // Borrowed: the bytes are wiped when this returns.
        &*key,
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
