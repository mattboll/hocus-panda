//! Secrets: calendar passwords, AI providers' keys. In the keyring (the
//! Secret Service on the session bus: GNOME Keyring, KWallet, KeePassXC…)
//! when there is one, else in a file sealed with the session's key
//! (`crate::sealed`). Each kind has its own attribute in the keyring and
//! its own sealed file. Blocking: called on worker threads.

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
    /// The keyring attribute naming what a secret is for: `calendar` (the
    /// account), `ai-provider` (the provider).
    pub attribute: &'static str,
}

/// What the Secret Service calls a secret: session, parameters, value,
/// content type.
type Secret = (OwnedObjectPath, Vec<u8>, Vec<u8>, String);

impl Secrets {
    /// Stores `secret` for `id`, over what was there.
    pub fn store(&self, id: &str, label: &str, secret: &str) -> Result<(), String> {
        if let Some(keyring) = self.keyring() {
            return zbus::block_on(keyring_store(&keyring, self.attribute, id, label, secret));
        }
        let mut all = self.sealed()?;
        all.insert(id.to_owned(), secret.to_owned());
        self.seal(&all)
    }

    pub fn lookup(&self, id: &str) -> Result<Option<String>, String> {
        if let Some(keyring) = self.keyring() {
            return zbus::block_on(keyring_lookup(&keyring, self.attribute, id));
        }
        Ok(self.sealed()?.remove(id))
    }

    /// Forgets every secret but those of `keep`.
    pub fn keep_only(&self, keep: &[String]) -> Result<(), String> {
        if let Some(keyring) = self.keyring() {
            return zbus::block_on(keyring_keep_only(&keyring, self.attribute, keep));
        }
        let mut all = self.sealed()?;
        let before = all.len();
        all.retain(|id, _| keep.contains(id));
        if all.len() != before {
            self.seal(&all)?;
        }
        Ok(())
    }

    /// How many secrets of this kind are kept.
    pub fn count(&self) -> Result<usize, String> {
        if let Some(keyring) = self.keyring() {
            // Counted locked too: no unlocking prompt for a count.
            return zbus::block_on(keyring_count(&keyring, self.attribute));
        }
        Ok(self.sealed()?.len())
    }

    /// The ids of the secrets of this kind kept (which providers have a
    /// key). Read locked too, without asking to unlock, and without the
    /// secrets themselves.
    pub fn ids(&self) -> Result<Vec<String>, String> {
        let mut ids = match self.keyring() {
            Some(keyring) => zbus::block_on(keyring_ids(&keyring, self.attribute))?,
            None => self.sealed()?.into_keys().collect(),
        };
        ids.sort();
        ids.dedup();
        Ok(ids)
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

fn attributes<'a>(attribute: &'a str, id: Option<&'a str>) -> HashMap<&'a str, &'a str> {
    let mut a = HashMap::from([("application", "hocuspanda")]);
    if let Some(id) = id {
        a.insert(attribute, id);
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
    attribute: &str,
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
        Variant::from(attributes(attribute, Some(id))),
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

/// The items of `id` (or all of the application's): unlocked, and locked.
async fn search(
    conn: &Connection,
    attribute: &str,
    id: Option<&str>,
) -> Result<(Vec<OwnedObjectPath>, Vec<OwnedObjectPath>), String> {
    let service = proxy(conn, PATH, "org.freedesktop.Secret.Service")
        .await
        .map_err(|e| e.to_string())?;
    service
        .call("SearchItems", &(attributes(attribute, id),))
        .await
        .map_err(|e| e.to_string())
}

/// The items of `id` (or all of the application's), unlocked if they
/// were not.
async fn find(
    conn: &Connection,
    attribute: &str,
    id: Option<&str>,
) -> Result<Vec<OwnedObjectPath>, String> {
    let (mut unlocked, locked) = search(conn, attribute, id).await?;
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

async fn keyring_lookup(
    conn: &Connection,
    attribute: &str,
    id: &str,
) -> Result<Option<String>, String> {
    let items = find(conn, attribute, Some(id)).await?;
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

/// The attributes of an item, which the keyring shows locked too.
async fn item_attributes(
    conn: &Connection,
    item: &OwnedObjectPath,
) -> Result<HashMap<String, String>, String> {
    let p = proxy(conn, item.as_str(), "org.freedesktop.Secret.Item")
        .await
        .map_err(|e| e.to_string())?;
    p.get_property("Attributes")
        .await
        .map_err(|e| e.to_string())
}

/// The application's items of this kind, locked or not.
async fn keyring_count(conn: &Connection, attribute: &str) -> Result<usize, String> {
    Ok(keyring_ids(conn, attribute).await?.len())
}

/// The ids of the application's items of this kind, locked or not.
async fn keyring_ids(conn: &Connection, attribute: &str) -> Result<Vec<String>, String> {
    let (unlocked, locked) = search(conn, attribute, None).await?;
    let mut ids = Vec::new();
    for item in unlocked.iter().chain(&locked) {
        if let Some(id) = item_attributes(conn, item).await?.remove(attribute) {
            ids.push(id);
        }
    }
    Ok(ids)
}

async fn keyring_keep_only(
    conn: &Connection,
    attribute: &str,
    keep: &[String],
) -> Result<(), String> {
    for item in find(conn, attribute, None).await? {
        let attributes = item_attributes(conn, &item).await?;
        // Another kind's secret is not this one's to forget.
        let Some(id) = attributes.get(attribute) else {
            continue;
        };
        let p = proxy(conn, item.as_str(), "org.freedesktop.Secret.Item")
            .await
            .map_err(|e| e.to_string())?;
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
            attribute: "calendar",
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
        assert_eq!(s.ids(), Ok(vec!["home".to_owned()]));
    }
}
