//! The accounts' tokens (a kanban server's access token): opaque strings,
//! kept in the session's Secret Service (`org.freedesktop.secrets`: GNOME
//! Keyring, KWallet, KeePassXC…) on the session bus the compositor names,
//! never another; else, without one or when it does not answer, in a file
//! readable by the user alone, `$XDG_STATE_HOME/hocuspanda/accounts/<id>`
//! (mode 0600, its folder 0700).
//!
//! Every call blocks, a keyring's prompt included, up to [`WAIT`]: for an
//! account's worker, never the shell's loop. The token goes over the bus
//! as is (the `plain` session), as most clients send theirs: the bus is
//! the user's. Its item: the attributes `application=hocuspanda`,
//! `hocuspanda=day` and `account=<id>`, in the default collection.

use std::{
    collections::HashMap,
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

use zbus::{
    Connection, MatchRule, MessageStream, ObjectPath, OwnedObjectPath, OwnedVariant, Variant,
    connection::Builder, message::Type,
};

use crate::model::AccountId;

const SERVICE: &str = "org.freedesktop.secrets";
const PATH: &str = "/org/freedesktop/secrets";
const IFACE: &str = "org.freedesktop.Secret.Service";
const COLLECTION: &str = "org.freedesktop.Secret.Collection";
const ITEM: &str = "org.freedesktop.Secret.Item";
const PROMPT: &str = "org.freedesktop.Secret.Prompt";
/// How long the keyring has, an unlock asked of the user included.
pub const WAIT: Duration = Duration::from_secs(15);

/// A token: never shown in a log.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    /// The token itself, to send to its server.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(…)")
    }
}

/// Where a token was kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kept {
    Keyring,
    File,
}

/// Where the accounts' tokens are kept.
#[derive(Debug, Clone, Default)]
pub struct Secrets {
    /// The session bus the compositor names (`--session-bus`, the
    /// session's own on a virtual terminal); `None`: files only.
    bus: Option<String>,
    /// `$XDG_STATE_HOME/hocuspanda/accounts`; `None`: nowhere.
    dir: Option<PathBuf>,
}

impl Secrets {
    pub fn new(bus: Option<String>, dir: Option<PathBuf>) -> Self {
        Self { bus, dir }
    }

    /// `account`'s token: the keyring's, else its file's; `None` when
    /// neither holds one.
    pub fn get(&self, account: &AccountId) -> Option<Token> {
        if let Some(bus) = &self.bus {
            let account = account.clone();
            let found = run(bus, async move |connection| {
                let session = open_session(&connection).await?;
                find(&connection, &session, &account).await
            });
            if let Some(Some(token)) = found {
                return Some(token);
            }
        }
        let file = self.file(account)?;
        match std::fs::read_to_string(&file) {
            Ok(text) => Some(Token(text.trim_end_matches('\n').to_owned())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                tracing::warn!(file = %file.display(), %err, "an account's token is not read");
                None
            }
        }
    }

    /// Keeps `token` for `account`: in the keyring if it takes it (its
    /// file then removed), else in its file.
    pub fn keep(&self, account: &AccountId, token: &Token) -> std::io::Result<Kept> {
        if let Some(bus) = &self.bus {
            let (id, secret) = (account.clone(), token.0.clone().into_bytes());
            let kept = run(bus, async move |connection| {
                let session = open_session(&connection).await?;
                keep(&connection, &session, &id, secret).await
            });
            if kept.is_some() {
                self.remove_file(account)?;
                return Ok(Kept::Keyring);
            }
        }
        let file = self
            .file(account)
            .ok_or_else(|| std::io::Error::other("no state directory for the accounts' tokens"))?;
        if let Some(dir) = file.parent() {
            create_private_dir(dir)?;
        }
        crate::write_file(&file, token.0.as_bytes())?;
        Ok(Kept::File)
    }

    /// Forgets `account`'s token, wherever it is.
    pub fn forget(&self, account: &AccountId) -> std::io::Result<()> {
        if let Some(bus) = &self.bus {
            let id = account.clone();
            run(bus, async move |connection| forget(&connection, &id).await);
        }
        self.remove_file(account)
    }

    fn file(&self, account: &AccountId) -> Option<PathBuf> {
        let name = account.as_str();
        let safe = !name.is_empty()
            && !name.starts_with('.')
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "@._-".contains(c));
        if !safe {
            tracing::warn!(account = name, "an account id unfit for a file's name");
            return None;
        }
        Some(self.dir.as_ref()?.join(name))
    }

    fn remove_file(&self, account: &AccountId) -> std::io::Result<()> {
        let Some(file) = self.file(account) else {
            return Ok(());
        };
        match std::fs::remove_file(file) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        }
    }
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
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

fn attributes(account: &AccountId) -> HashMap<&'static str, String> {
    HashMap::from([
        ("application", "hocuspanda".to_owned()),
        ("hocuspanda", "day".to_owned()),
        ("account", account.as_str().to_owned()),
    ])
}

async fn search(
    connection: &Connection,
    account: &AccountId,
) -> zbus::Result<(Vec<OwnedObjectPath>, Vec<OwnedObjectPath>)> {
    let reply = call(
        connection,
        PATH,
        IFACE,
        "SearchItems",
        &(attributes(account),),
    )
    .await?;
    reply.body().deserialize()
}

async fn find(
    connection: &Connection,
    session: &OwnedObjectPath,
    account: &AccountId,
) -> zbus::Result<Option<Token>> {
    let (unlocked, locked) = search(connection, account).await?;
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
    String::from_utf8(value)
        .map(|t| Some(Token(t)))
        .map_err(|_| zbus::Error::Failure("a token that is not text".to_owned()))
}

async fn keep(
    connection: &Connection,
    session: &OwnedObjectPath,
    account: &AccountId,
    secret: Vec<u8>,
) -> zbus::Result<()> {
    let reply = call(connection, PATH, IFACE, "ReadAlias", &("default",)).await?;
    let collection: OwnedObjectPath = reply.body().deserialize()?;
    if collection.as_str() == "/" {
        return Err(zbus::Error::Failure("no default collection".to_owned()));
    }
    let label = format!("Hocuspanda: {account}");
    let mut properties: HashMap<&str, Variant<'_>> = HashMap::new();
    properties.insert("org.freedesktop.Secret.Item.Label", Variant::from(label));
    properties.insert(
        "org.freedesktop.Secret.Item.Attributes",
        Variant::from(attributes(account)),
    );
    let secret = (
        ObjectPath::from(session.clone()),
        Vec::<u8>::new(),
        secret,
        "text/plain",
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

async fn forget(connection: &Connection, account: &AccountId) -> zbus::Result<()> {
    let (unlocked, locked) = search(connection, account).await?;
    for item in unlocked.iter().chain(&locked) {
        let reply = call(connection, item.as_str(), ITEM, "Delete", &()).await?;
        let prompt: OwnedObjectPath = reply.body().deserialize()?;
        prompted(connection, &prompt).await?;
    }
    Ok(())
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
    fn without_a_keyring_a_token_is_kept_in_a_file_of_the_users_alone() -> anyhow::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;
        let accounts = dir.path().join("hocuspanda/accounts");
        // A bus that is not there: no answer, the file.
        let bus = format!("unix:path={}", dir.path().join("none").display());
        let secrets = Secrets::new(Some(bus), Some(accounts.clone()));
        let id = AccountId::new("planka-tasks-planka.example.org-1234");
        assert_eq!(secrets.get(&id), None);
        assert_eq!(secrets.keep(&id, &Token::new("abc.def"))?, Kept::File);
        assert_eq!(secrets.get(&id), Some(Token::new("abc.def")));
        let file = accounts.join(id.as_str());
        assert_eq!(
            std::fs::metadata(&file)?.permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&accounts)?.permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(format!("{:?}", Token::new("abc.def")), "Token(…)");
        secrets.forget(&id)?;
        assert_eq!(secrets.get(&id), None);
        assert!(!file.exists());
        // No file for an id that could leave the folder.
        assert!(
            secrets
                .keep(&AccountId::new("../x"), &Token::new("t"))
                .is_err()
        );
        Ok(())
    }
}
