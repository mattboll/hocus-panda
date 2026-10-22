//! Calendar passwords: in the keyring (the Secret Service on the session
//! bus: GNOME Keyring, KWallet, KeePassXC…) when there is one, else in a
//! file sealed with the session's key (`crate::sealed`). Blocking: called
//! on worker threads.

use std::{collections::HashMap, path::PathBuf};

use zbus::{Connection, OwnedObjectPath, OwnedVariant, Proxy, Variant};

use crate::sealed::Sealed;

const SERVICE: &str = "org.freedesktop.secrets";
const PATH: &str = "/org/freedesktop/secrets";
const DEFAULT: &str = "/org/freedesktop/secrets/aliases/default";

/// Where secrets go.
#[derive(Debug, Clone)]
pub struct Secrets {
    /// The session bus, when the keyring may be there.
    pub bus: Option<String>,
    /// The sealed file used without a keyring.
    pub fallback: PathBuf,
}

/// What the Secret Service calls a secret: session, parameters, value,
/// content type.
type Secret = (OwnedObjectPath, Vec<u8>, Vec<u8>, String);

impl Secrets {
    /// Stores `secret` for calendar `id`, over what was there.
    pub fn store(&self, id: &str, label: &str, secret: &str) -> Result<(), String> {
        if let Some(keyring) = self.keyring() {
            return zbus::block_on(keyring_store(&keyring, id, label, secret));
        }
        let mut all = self.sealed()?;
        all.insert(id.to_owned(), secret.to_owned());
        self.seal(&all)
    }

    pub fn lookup(&self, id: &str) -> Result<Option<String>, String> {
        if let Some(keyring) = self.keyring() {
            return zbus::block_on(keyring_lookup(&keyring, id));
        }
        Ok(self.sealed()?.remove(id))
    }

    /// Forgets every secret but those of `keep`.
    pub fn keep_only(&self, keep: &[String]) -> Result<(), String> {
        if let Some(keyring) = self.keyring() {
            return zbus::block_on(keyring_keep_only(&keyring, keep));
        }
        let mut all = self.sealed()?;
        let before = all.len();
        all.retain(|id, _| keep.contains(id));
        if all.len() != before {
            self.seal(&all)?;
        }
        Ok(())
    }

    /// How many calendar secrets are kept.
    pub fn count(&self) -> Result<usize, String> {
        if let Some(keyring) = self.keyring() {
            // Counted locked too: no unlocking prompt for a count.
            return zbus::block_on(search(&keyring, None)).map(|(a, b)| a.len() + b.len());
        }
        Ok(self.sealed()?.len())
    }

    /// The keyring, when the session bus has one.
    fn keyring(&self) -> Option<Connection> {
        let bus = self.bus.as_deref()?;
        zbus::block_on(async {
            let conn = zbus::connection::Builder::address(bus).build().await.ok()?;
            let dbus = zbus::fdo::DBusProxy::new(&conn).await.ok()?;
            let name = zbus::names::BusName::try_from(SERVICE).ok()?;
            dbus.name_has_owner(name).await.ok()?.then_some(conn)
        })
    }

    fn sealed(&self) -> Result<HashMap<String, String>, String> {
        match Sealed::new(&self.fallback).read()? {
            Some(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
            None => Ok(HashMap::new()),
        }
    }

    fn seal(&self, all: &HashMap<String, String>) -> Result<(), String> {
        let bytes = serde_json::to_vec(all).map_err(|e| e.to_string())?;
        Sealed::new(&self.fallback).write(&bytes)
    }
}

fn attributes(id: Option<&str>) -> HashMap<&str, &str> {
    let mut a = HashMap::from([("application", "hocuspanda")]);
    if let Some(id) = id {
        a.insert("calendar", id);
    }
    a
}

async fn proxy<'a>(conn: &Connection, path: &str, interface: &'a str) -> zbus::Result<Proxy<'a>> {
    Proxy::new(conn, SERVICE, path.to_owned(), interface).await
}

/// A plain session: the bus is the user's own.
async fn session(conn: &Connection) -> Result<OwnedObjectPath, String> {
    let service = proxy(conn, PATH, "org.freedesktop.Secret.Service")
        .await
        .map_err(|e| e.to_string())?;
    let (_, path): (OwnedVariant, OwnedObjectPath) = service
        .call("OpenSession", &("plain", Variant::from("")))
        .await
        .map_err(|e| e.to_string())?;
    Ok(path)
}

/// Waits for a prompt the keyring shows (to unlock it), if any.
async fn prompted(conn: &Connection, prompt: &OwnedObjectPath) -> Result<(), String> {
    if prompt.as_str() == "/" {
        return Ok(());
    }
    let p = proxy(conn, prompt.as_str(), "org.freedesktop.Secret.Prompt")
        .await
        .map_err(|e| e.to_string())?;
    let mut completed = p
        .receive_signal("Completed")
        .await
        .map_err(|e| e.to_string())?;
    p.call::<_, _, ()>("Prompt", &("",))
        .await
        .map_err(|e| e.to_string())?;
    let signal = std::future::poll_fn(|cx| {
        use futures_core::Stream;
        std::pin::Pin::new(&mut completed).poll_next(cx)
    })
    .await
    .ok_or("the keyring did not answer")?;
    let (dismissed, _): (bool, OwnedVariant) =
        signal.body().deserialize().map_err(|e| e.to_string())?;
    if dismissed {
        return Err("the keyring stayed locked".into());
    }
    Ok(())
}

async fn keyring_store(
    conn: &Connection,
    id: &str,
    label: &str,
    secret: &str,
) -> Result<(), String> {
    let session = session(conn).await?;
    let collection = proxy(conn, DEFAULT, "org.freedesktop.Secret.Collection")
        .await
        .map_err(|e| e.to_string())?;
    let mut properties: HashMap<&str, Variant<'_>> = HashMap::new();
    properties.insert("org.freedesktop.Secret.Item.Label", Variant::from(label));
    properties.insert(
        "org.freedesktop.Secret.Item.Attributes",
        Variant::from(attributes(Some(id))),
    );
    let value: Secret = (
        session,
        Vec::new(),
        secret.as_bytes().to_vec(),
        "text/plain".into(),
    );
    let (_, prompt): (OwnedObjectPath, OwnedObjectPath) = collection
        .call("CreateItem", &(properties, value, true))
        .await
        .map_err(|e| e.to_string())?;
    prompted(conn, &prompt).await
}

/// The items of calendar `id` (or all): unlocked, and locked.
async fn search(
    conn: &Connection,
    id: Option<&str>,
) -> Result<(Vec<OwnedObjectPath>, Vec<OwnedObjectPath>), String> {
    let service = proxy(conn, PATH, "org.freedesktop.Secret.Service")
        .await
        .map_err(|e| e.to_string())?;
    service
        .call("SearchItems", &(attributes(id),))
        .await
        .map_err(|e| e.to_string())
}

/// The items of calendar `id` (or all), unlocked if they were not.
async fn find(conn: &Connection, id: Option<&str>) -> Result<Vec<OwnedObjectPath>, String> {
    let (mut unlocked, locked) = search(conn, id).await?;
    if !locked.is_empty() {
        let service = proxy(conn, PATH, "org.freedesktop.Secret.Service")
            .await
            .map_err(|e| e.to_string())?;
        let (opened, prompt): (Vec<OwnedObjectPath>, OwnedObjectPath) = service
            .call("Unlock", &(locked.clone(),))
            .await
            .map_err(|e| e.to_string())?;
        prompted(conn, &prompt).await?;
        unlocked.extend(if prompt.as_str() == "/" {
            opened
        } else {
            locked
        });
    }
    Ok(unlocked)
}

async fn keyring_lookup(conn: &Connection, id: &str) -> Result<Option<String>, String> {
    let items = find(conn, Some(id)).await?;
    let Some(item) = items.first() else {
        return Ok(None);
    };
    let session = session(conn).await?;
    let service = proxy(conn, PATH, "org.freedesktop.Secret.Service")
        .await
        .map_err(|e| e.to_string())?;
    let secrets: HashMap<OwnedObjectPath, Secret> = service
        .call("GetSecrets", &(vec![item.clone()], session))
        .await
        .map_err(|e| e.to_string())?;
    Ok(secrets
        .get(item)
        .map(|(_, _, value, _)| String::from_utf8_lossy(value).into_owned()))
}

async fn keyring_keep_only(conn: &Connection, keep: &[String]) -> Result<(), String> {
    for item in find(conn, None).await? {
        let p = proxy(conn, item.as_str(), "org.freedesktop.Secret.Item")
            .await
            .map_err(|e| e.to_string())?;
        let attributes: HashMap<String, String> = p
            .get_property("Attributes")
            .await
            .map_err(|e| e.to_string())?;
        let Some(id) = attributes.get("calendar") else {
            continue;
        };
        if keep.contains(id) {
            continue;
        }
        let prompt: OwnedObjectPath = p.call("Delete", &()).await.map_err(|e| e.to_string())?;
        prompted(conn, &prompt).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_keyring_secrets_are_sealed_and_forgotten() {
        let dir = tempfile::tempdir().expect("a directory");
        let s = Secrets {
            bus: None,
            fallback: dir.path().join("calendar-secrets"),
        };
        s.store("work", "Work", "hunter2").expect("stored");
        s.store("home", "Home", "panda").expect("stored");
        assert_eq!(s.lookup("work").expect("read").as_deref(), Some("hunter2"));
        let raw = std::fs::read(dir.path().join("calendar-secrets")).expect("a file");
        assert!(!String::from_utf8_lossy(&raw).contains("hunter2"), "sealed");
        s.keep_only(&["home".into()]).expect("forgotten");
        assert_eq!(s.lookup("work").expect("read"), None);
        assert_eq!(s.lookup("home").expect("read").as_deref(), Some("panda"));
        assert_eq!(s.count(), Ok(1));
    }
}
