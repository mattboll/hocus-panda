//! "Choisir une image…" (6.3): the desktop portal's file chooser
//! (`org.freedesktop.portal.FileChooser.OpenFile`) on the session bus
//! (`DBUS_SESSION_BUS_ADDRESS`, which the compositor gives the programs it
//! starts; without it, none), asked from the wallpaper page's thread,
//! never the window's. Whether a portal is there, and the call, are given
//! [`ASK`]; then the user chooses for as long as they like.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

use iced::futures::StreamExt;
use zbus::{
    Connection, MatchRule, MessageStream, OwnedObjectPath, OwnedVariant, Variant,
    connection::Builder, message::Type,
};

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const CHOOSER: &str = "org.freedesktop.portal.FileChooser";
const REQUEST: &str = "org.freedesktop.portal.Request";
const ASK: Duration = Duration::from_secs(5);

/// What the file chooser said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chosen {
    Path(PathBuf),
    Cancelled,
    NoPortal,
}

/// The user chooses a picture from `folder`, in a dialog titled `title`
/// with the filter `filter` (JPEG, PNG, WebP). Blocks this thread until
/// they do.
pub fn open_file(title: &str, filter: &str, folder: &Path) -> Chosen {
    let Some(bus) = std::env::var("DBUS_SESSION_BUS_ADDRESS")
        .ok()
        .filter(|b| !b.is_empty())
    else {
        tracing::info!("no session bus: no file chooser");
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
    runtime.block_on(choose(&bus, title, filter, folder))
}

async fn choose(bus: &str, title: &str, filter: &str, folder: &Path) -> Chosen {
    let asked = async {
        let connection = Builder::address(bus).build().await?;
        let request = ask(&connection, title, filter, folder).await?;
        Ok::<_, zbus::Error>((connection, request))
    };
    let (connection, mut responses) = match tokio::time::timeout(ASK, asked).await {
        Ok(Ok(asked)) => asked,
        Ok(Err(err)) => {
            tracing::info!(%err, "the desktop portal chooses no file");
            return Chosen::NoPortal;
        }
        Err(_) => {
            tracing::info!("the desktop portal did not answer in time");
            return Chosen::NoPortal;
        }
    };
    tracing::info!("the file chooser shows");
    let answer = responses.next().await;
    drop(connection);
    let Some(Ok(message)) = answer else {
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
                None => Chosen::Cancelled,
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

/// `OpenFile` asked: where its answer comes, listened to before it is
/// asked.
async fn ask(
    connection: &Connection,
    title: &str,
    filter: &str,
    folder: &Path,
) -> zbus::Result<MessageStream> {
    static TOKENS: AtomicU32 = AtomicU32::new(1);
    let token = format!(
        "hocussettings{}_{}",
        std::process::id(),
        TOKENS.fetch_add(1, Ordering::Relaxed)
    );
    let sender = connection
        .unique_name()
        .map(|n| n.trim_start_matches(':').replace('.', "_"))
        .unwrap_or_default();
    let path = format!("{PATH}/request/{sender}/{token}");
    let listen = |path: &str| -> zbus::Result<MatchRule<'static>> {
        MatchRule::builder()
            .msg_type(Type::Signal)
            .interface(REQUEST)
            .member("Response")
            .path(path.to_owned())
            .build()
    };
    let responses = MessageStream::for_match_rule(listen(&path)?, connection, None).await?;
    // `[(name, [(0, mime type)])]`: the pictures the desktop draws.
    let types: Vec<(u32, &str)> = ["image/jpeg", "image/png", "image/webp"]
        .into_iter()
        .map(|t| (1, t))
        .collect();
    let filters = vec![(filter, types)];
    let mut options: HashMap<&str, Variant<'_>> = HashMap::new();
    options.insert("handle_token", Variant::from(token.as_str()));
    options.insert("modal", Variant::from(true));
    options.insert("multiple", Variant::from(false));
    options.insert("filters", Variant::from(filters.clone()));
    options.insert("current_filter", Variant::from(filters[0].clone()));
    let mut bytes = folder.as_os_str().as_encoded_bytes().to_vec();
    bytes.push(0);
    if !folder.as_os_str().is_empty() {
        options.insert("current_folder", Variant::from(bytes));
    }
    let reply = connection
        .call_method(
            Some(PORTAL),
            PATH,
            Some(CHOOSER),
            "OpenFile",
            &("", title, options),
        )
        .await?;
    let handle: OwnedObjectPath = reply.body().deserialize()?;
    if handle.as_str() != path {
        // An older portal names its request otherwise: listen there.
        return MessageStream::for_match_rule(listen(handle.as_str())?, connection, None).await;
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
            path_of("file:///home/alex/Images/Lac%20au%20matin.jpg"),
            Some(PathBuf::from("/home/alex/Images/Lac au matin.jpg"))
        );
        assert_eq!(path_of("https://example.org/x.png"), None);
    }
}
