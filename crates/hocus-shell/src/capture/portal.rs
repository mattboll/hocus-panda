//! "Enregistrer sous…" (5.9): the desktop portal's file chooser
//! (`org.freedesktop.portal.FileChooser.SaveFile`), on the session bus the
//! compositor names ([`hocus_link::NOTIFICATIONS_BUS_ENV`]), never another.
//!
//! Asked on the editor's thread for it: whether a portal is there (owned,
//! or started on demand) is asked within [`ASK`], and so is the call;
//! then the user chooses for as long as they like. The editor is the
//! chooser's parent (`parent_window`, [`crate::foreign`]): the chooser
//! comes in front of the windows with the keyboard (#595). Without a bus, without
//! a portal, or a portal without a file chooser, the caller saves beside
//! the capture instead ([`Chosen::NoPortal`]).

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

use hocus_link::NOTIFICATIONS_BUS_ENV;
use iced::futures::StreamExt;
use zbus::{
    Connection, MatchRule, MessageStream, OwnedObjectPath, OwnedVariant, Variant,
    connection::Builder, message::Type,
};

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const CHOOSER: &str = "org.freedesktop.portal.FileChooser";
const REQUEST: &str = "org.freedesktop.portal.Request";
/// How long the bus and the portal have to answer, before the chooser
/// shows.
const ASK: Duration = Duration::from_secs(5);

/// What the file chooser said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chosen {
    Path(PathBuf),
    Cancelled,
    /// No portal answers, or none chooses files: save beside instead.
    NoPortal,
}

/// What the chooser is asked: where to save a file, or a folder (the
/// project's form's "Choisir un autre dossier…", docs/design/projects.md
/// §2.1), over the surface of `parent`'s namespace.
#[derive(Debug, Clone, Copy)]
enum Pick<'a> {
    Save { name: &'a str, folder: &'a Path },
    Folder { parent: &'static str },
}

/// The user chooses where to save `name`, from `folder`, in a dialog
/// titled `title`; `showing` runs once the chooser shows. Blocks this
/// thread until they do.
pub fn save_file(title: &str, name: &str, folder: &Path, showing: impl FnOnce()) -> Chosen {
    run(title, Pick::Save { name, folder }, showing)
}

/// The user chooses a folder in a dialog titled `title`, over the shell's
/// surface of `parent`'s namespace. Blocks this thread until they do.
pub fn choose_folder(title: &str, parent: &'static str) -> Chosen {
    run(title, Pick::Folder { parent }, || {})
}

fn run(title: &str, pick: Pick<'_>, showing: impl FnOnce()) -> Chosen {
    let Some(bus) = std::env::var(NOTIFICATIONS_BUS_ENV)
        .ok()
        .filter(|b| !b.is_empty())
    else {
        tracing::info!("no session bus given: no file chooser");
        return Chosen::NoPortal;
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            tracing::warn!(%err, "no runtime for the file chooser");
            return Chosen::NoPortal;
        }
    };
    runtime.block_on(choose(&bus, title, pick, showing))
}

async fn choose(bus: &str, title: &str, pick: Pick<'_>, showing: impl FnOnce()) -> Chosen {
    let asked = async {
        let builder = if bus == "session" {
            Builder::session()
        } else {
            Builder::address(bus)
        };
        let connection = builder.build().await?;
        if !there(&connection).await? {
            return Ok(None);
        }
        let namespace = match pick {
            Pick::Save { .. } => hocus_link::CAPTURE_EDITOR_NAMESPACE,
            Pick::Folder { parent } => parent,
        };
        let parent = crate::foreign::parent_window(namespace).await;
        let request = ask(&connection, &parent, title, pick).await?;
        Ok::<_, zbus::Error>(Some((connection, request)))
    };
    let (connection, mut responses) = match tokio::time::timeout(ASK, asked).await {
        Ok(Ok(Some(asked))) => asked,
        Ok(Ok(None)) => {
            tracing::info!("no desktop portal on the bus");
            return Chosen::NoPortal;
        }
        Ok(Err(err)) => {
            tracing::info!(%err, "the desktop portal chooses no file");
            return Chosen::NoPortal;
        }
        Err(_) => {
            tracing::info!("the desktop portal did not answer in time");
            return Chosen::NoPortal;
        }
    };
    showing();
    let answer = responses.next().await;
    drop(connection);
    let Some(Ok(message)) = answer else {
        tracing::info!("the file chooser went without an answer");
        return Chosen::Cancelled;
    };
    let read: zbus::Result<(u32, HashMap<String, OwnedVariant>)> = message.body().deserialize();
    match read {
        Ok((0, results)) => {
            let uri = results
                .get("uris")
                .and_then(|v| <Vec<String>>::try_from(v.clone()).ok())
                .and_then(|uris| uris.into_iter().next());
            match uri.as_deref().and_then(path_of) {
                Some(path) => Chosen::Path(path),
                None => {
                    tracing::info!(?uri, "the file chooser chose no file");
                    Chosen::Cancelled
                }
            }
        }
        Ok((code, _)) => {
            tracing::info!(code, "the file chooser was cancelled");
            Chosen::Cancelled
        }
        Err(err) => {
            tracing::info!(%err, "the file chooser's answer does not read");
            Chosen::Cancelled
        }
    }
}

/// Whether a portal is on the bus, or would start there.
async fn there(connection: &Connection) -> zbus::Result<bool> {
    let bus = zbus::fdo::DBusProxy::new(connection).await?;
    let name = zbus::names::BusName::try_from(PORTAL)?;
    if bus.name_has_owner(name).await? {
        return Ok(true);
    }
    let started = bus.list_activatable_names().await?;
    Ok(started.iter().any(|n| n.as_str() == PORTAL))
}

/// `SaveFile` (or `OpenFile` for a folder) asked: where its answer comes,
/// listened to before it is asked.
async fn ask(
    connection: &Connection,
    parent: &str,
    title: &str,
    pick: Pick<'_>,
) -> zbus::Result<MessageStream> {
    static TOKENS: AtomicU32 = AtomicU32::new(1);
    let token = format!(
        "hocuspanda{}_{}",
        std::process::id(),
        TOKENS.fetch_add(1, Ordering::Relaxed)
    );
    let sender = connection
        .unique_name()
        .map(|n| n.trim_start_matches(':').replace('.', "_"))
        .unwrap_or_default();
    let path = format!("{PATH}/request/{sender}/{token}");
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(REQUEST)
        .member("Response")
        .path(path.as_str())
        .build()?;
    let responses = MessageStream::for_match_rule(rule, connection, None).await?;
    let mut options: HashMap<&str, Variant<'_>> = HashMap::new();
    options.insert("handle_token", Variant::from(token.as_str()));
    options.insert("modal", Variant::from(true));
    let method = match pick {
        Pick::Save { name, folder } => {
            options.insert("current_name", Variant::from(name));
            let mut bytes = folder.as_os_str().as_encoded_bytes().to_vec();
            bytes.push(0);
            if !folder.as_os_str().is_empty() {
                options.insert("current_folder", Variant::from(bytes));
            }
            "SaveFile"
        }
        Pick::Folder { .. } => {
            options.insert("directory", Variant::from(true));
            "OpenFile"
        }
    };
    let reply = connection
        .call_method(
            Some(PORTAL),
            PATH,
            Some(CHOOSER),
            method,
            &(parent, title, options),
        )
        .await?;
    let handle: OwnedObjectPath = reply.body().deserialize()?;
    if handle.as_str() != path {
        // An older portal names its request otherwise: listen there.
        tracing::debug!(handle = handle.as_str(), "the file chooser's request moved");
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface(REQUEST)
            .member("Response")
            .path(handle.as_str())
            .build()?;
        return MessageStream::for_match_rule(rule, connection, None).await;
    }
    Ok(responses)
}

/// The path of a `file://` address, its escapes read.
fn path_of(uri: &str) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let mut bytes = Vec::with_capacity(rest.len());
    let mut chars = rest.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hex = [chars.next()?, chars.next()?];
            let hex = std::str::from_utf8(&hex).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
        } else {
            bytes.push(b);
        }
    }
    let path = PathBuf::from(std::ffi::OsString::from_vec(bytes));
    path.is_absolute().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chosen_address_is_its_path() {
        assert_eq!(
            path_of("file:///home/alex/Images/Captures%20d%27%C3%A9cran/a%20b.png"),
            Some(PathBuf::from("/home/alex/Images/Captures d'écran/a b.png"))
        );
        assert_eq!(
            path_of("file://localhost/tmp/x.png"),
            Some(PathBuf::from("/tmp/x.png"))
        );
        assert_eq!(path_of("https://example.org/x.png"), None);
        assert_eq!(path_of("file://relative"), None);
        assert_eq!(path_of("file:///bad%2"), None);
    }

    #[test]
    fn a_bus_that_is_not_there_has_no_portal() {
        let dir = tempfile::tempdir().expect("a directory");
        let bus = format!("unix:path={}", dir.path().join("none").display());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let pick = Pick::Save {
            name: "a.png",
            folder: dir.path(),
        };
        let chosen = runtime.block_on(choose(&bus, "t", pick, || panic!("no chooser shows")));
        assert_eq!(chosen, Chosen::NoPortal);
    }
}
