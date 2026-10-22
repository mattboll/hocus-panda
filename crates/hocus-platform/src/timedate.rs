//! The system's time zone, through systemd-timedated
//! (`org.freedesktop.timedate1` on the system bus): read, followed as it
//! changes, and set, polkit asking whoever is at the screen. Where there is
//! no timedated (FreeBSD, a system without systemd), `/etc/localtime` says
//! the zone and changes made to it are followed; setting it is refused.
//!
//! Everything here blocks or runs on threads of its own: never on a loop.

use std::{future::poll_fn, path::Path, pin::Pin, time::Duration};

use futures_core::Stream;
use zbus::{Connection, Proxy};

const SERVICE: &str = "org.freedesktop.timedate1";
const PATH: &str = "/org/freedesktop/timedate1";
const LOCALTIME: &str = "/etc/localtime";
/// How often `/etc/localtime` is looked at.
const LOOK: Duration = Duration::from_secs(2);

async fn connect(bus: &Option<String>) -> zbus::Result<Connection> {
    match bus {
        Some(address) => {
            zbus::connection::Builder::address(address.as_str())
                .build()
                .await
        }
        None => Connection::system().await,
    }
}

async fn timedated(conn: &Connection) -> zbus::Result<Proxy<'static>> {
    zbus::proxy::Builder::new(conn)
        .destination(SERVICE)
        .path(PATH)
        .interface(SERVICE)
        // Read again at each change: a cache could keep the old one.
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
}

/// The zone timedated has, on `system_bus` (the machine's when `None`).
pub fn read(system_bus: &Option<String>) -> Result<String, String> {
    zbus::block_on(async {
        let conn = connect(system_bus).await?;
        timedated(&conn)
            .await?
            .get_property::<String>("Timezone")
            .await
    })
    .map_err(|err| err.to_string())
}

/// Asks timedated to set the system's zone; polkit asks for the right to.
pub fn set(system_bus: &Option<String>, zone: &str) -> Result<(), String> {
    zbus::block_on(async {
        let conn = connect(system_bus).await?;
        timedated(&conn)
            .await?
            .call::<_, _, ()>("SetTimezone", &(zone, true))
            .await
    })
    .map_err(|err| match err {
        // What polkit or timedated said, without the error's name.
        zbus::Error::MethodError(name, message, _) => {
            message.unwrap_or_else(|| name.as_str().to_owned())
        }
        zbus::Error::FDO(err) => {
            use zbus::DBusError;
            err.description()
                .map_or_else(|| err.to_string(), str::to_owned)
        }
        other => other.to_string(),
    })
}

/// The zone `/etc/localtime` points to: `Europe/Paris` for a link to
/// `…/zoneinfo/Europe/Paris`.
pub fn localtime_name(path: &Path) -> Option<String> {
    let target = std::fs::read_link(path).ok()?;
    let target = target.to_string_lossy();
    let (_, name) = target.split_once("zoneinfo/")?;
    let name = name
        .trim_start_matches("posix/")
        .trim_start_matches("right/");
    (!name.is_empty()).then(|| name.to_owned())
}

/// Follows the system's zone: timedated's `Timezone`, read at once and
/// again at each change, and, when `watch_file`, `/etc/localtime` changed
/// by hand. Each name heard goes to `sink`, on these threads.
pub fn follow(
    system_bus: Option<String>,
    watch_file: bool,
    sink: impl Fn(String) + Send + Sync + 'static,
) -> std::io::Result<()> {
    let sink = std::sync::Arc::new(sink);
    {
        let sink = sink.clone();
        std::thread::Builder::new()
            .name("timedated".into())
            .spawn(move || {
                if let Err(err) = zbus::block_on(changes(system_bus, &*sink)) {
                    tracing::debug!(%err, "timedated is not there");
                }
            })?;
    }
    if watch_file {
        std::thread::Builder::new()
            .name("localtime".into())
            .spawn(move || {
                let mut last = localtime_name(Path::new(LOCALTIME));
                loop {
                    std::thread::sleep(LOOK);
                    let now = localtime_name(Path::new(LOCALTIME));
                    if now != last {
                        if let Some(name) = &now {
                            sink(name.clone());
                        }
                        last = now;
                    }
                }
            })?;
    }
    Ok(())
}

async fn changes(bus: Option<String>, sink: &(dyn Fn(String) + Send + Sync)) -> zbus::Result<()> {
    let conn = connect(&bus).await?;
    let proxy = timedated(&conn).await?;
    let properties = zbus::fdo::PropertiesProxy::builder(&conn)
        .destination(SERVICE)
        .path(PATH)
        .build()
        .await?;
    let mut changed = properties.receive_properties_changed().await?;
    loop {
        let zone: String = proxy.get_property("Timezone").await?;
        sink(zone);
        if poll_fn(|cx| Pin::new(&mut changed).poll_next(cx))
            .await
            .is_none()
        {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_names_its_zone() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let link = dir.path().join("localtime");
        std::os::unix::fs::symlink("/usr/share/zoneinfo/Asia/Tokyo", &link)?;
        assert_eq!(localtime_name(&link).as_deref(), Some("Asia/Tokyo"));
        let plain = dir.path().join("plain");
        std::fs::write(&plain, b"TZif")?;
        assert_eq!(localtime_name(&plain), None);
        Ok(())
    }
}
