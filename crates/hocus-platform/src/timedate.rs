//! The system's time zone, through systemd-timedated
//! (`org.freedesktop.timedate1` on the system bus): read, followed as it
//! changes, and set, polkit asking whoever is at the screen. Where there is
//! no timedated (FreeBSD, a system without systemd), `/etc/localtime` says
//! the zone and changes made to it are followed; setting it is refused.
//!
//! Everything here blocks or runs on threads of its own: never on a loop.

use std::{
    future::poll_fn,
    path::{Path, PathBuf},
    pin::Pin,
    time::Duration,
};

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

/// Where the time zone database is: `TZDIR`, else `/usr/share/zoneinfo`.
pub fn zoneinfo() -> PathBuf {
    std::env::var_os("TZDIR")
        .filter(|d| !d.is_empty())
        .map_or_else(|| PathBuf::from("/usr/share/zoneinfo"), PathBuf::from)
}

/// A zone where its principal city is: degrees, north and east positive.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub zone: String,
    pub latitude: f32,
    pub longitude: f32,
}

/// The zones of the database in `zoneinfo`, with their places, from its
/// `zone1970.tab` (public domain); none when it is missing. It reads a
/// file: for a worker thread.
pub fn places(zoneinfo: &Path) -> Vec<Place> {
    let Ok(table) = std::fs::read_to_string(zoneinfo.join("zone1970.tab")) else {
        return Vec::new();
    };
    table
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|line| {
            let mut columns = line.split('\t');
            let (_, at, zone) = (columns.next()?, columns.next()?, columns.next()?);
            let (latitude, longitude) = coordinates(at)?;
            Some(Place {
                zone: zone.to_owned(),
                latitude,
                longitude,
            })
        })
        .collect()
}

/// ISO 6709 as the table writes it: `±DDMM±DDDMM` or `±DDMMSS±DDDMMSS`.
fn coordinates(at: &str) -> Option<(f32, f32)> {
    let split = at
        .char_indices()
        .skip(1)
        .find(|(_, c)| matches!(c, '+' | '-'))?
        .0;
    let (lat, lon) = at.split_at(split);
    let degrees = |part: &str, width: usize| -> Option<f32> {
        let (sign, digits) = part.split_at(1);
        let sign = if sign == "-" { -1.0 } else { 1.0 };
        let number = |range: std::ops::Range<usize>| -> Option<f32> {
            digits.get(range)?.parse::<u16>().ok().map(f32::from)
        };
        let d = number(0..width)?;
        let m = number(width..width + 2)?;
        let s = if digits.len() > width + 2 {
            number(width + 2..width + 4)?
        } else {
            0.0
        };
        Some(sign * (d + m / 60.0 + s / 3600.0))
    };
    Some((degrees(lat, 2)?, degrees(lon, 3)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_are_read_from_the_table() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::write(
            dir.path().join("zone1970.tab"),
            "# comment\nFR,MC\t+4852+00220\tEurope/Paris\nUS\t+404251-0740023\tAmerica/New_York\tEastern (most areas)\nbad line\n",
        )?;
        let places = places(dir.path());
        assert_eq!(places.len(), 2);
        assert_eq!(places[0].zone, "Europe/Paris");
        assert!((places[0].latitude - 48.8667).abs() < 0.01);
        assert!((places[0].longitude - 2.3333).abs() < 0.01);
        assert!((places[1].latitude - 40.7142).abs() < 0.01);
        assert!((places[1].longitude + 74.0064).abs() < 0.01);
        assert!(super::places(&dir.path().join("none")).is_empty());
        Ok(())
    }

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
