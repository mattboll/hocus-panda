//! The internal panel's backlight: read from sysfs, its changes heard from
//! udev (the kernel announces each, whoever made it), on a thread of its
//! own. It is written through logind ([`super::power`]).
//!
//! `HOCUS_BACKLIGHT_DIR` names another directory than
//! `/sys/class/backlight`, for tests; the files there are also watched with
//! inotify, which sysfs does not feed.

use std::{
    os::fd::AsFd,
    path::{Path, PathBuf},
};

use rustix::{
    event::{PollFd, PollFlags, poll},
    fs::inotify,
    io::Errno,
};

use super::{Presence, publish};

/// A backlight device: its directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub path: PathBuf,
}

/// Where backlights are.
pub fn dir() -> PathBuf {
    std::env::var_os("HOCUS_BACKLIGHT_DIR")
        .map_or_else(|| PathBuf::from("/sys/class/backlight"), PathBuf::from)
}

/// The panel's backlight in `dir`: the firmware's, else the platform's,
/// else a raw one (as the kernel ranks them), the first by name.
pub fn device(dir: &Path) -> Option<Device> {
    let mut found: Vec<(u8, Device)> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let kind = std::fs::read_to_string(path.join("type")).ok()?;
            let rank = match kind.trim() {
                "firmware" => 0,
                "platform" => 1,
                "raw" => 2,
                _ => return None,
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            Some((rank, Device { name, path }))
        })
        .collect();
    found.sort_by(|a, b| (a.0, &a.1.name).cmp(&(b.0, &b.1.name)));
    found.into_iter().next().map(|(_, device)| device)
}

fn number(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Its value and its maximum.
pub fn read(device: &Device) -> Option<(u32, u32)> {
    let max = number(&device.path.join("max_brightness")).filter(|m| *m > 0)?;
    let raw = number(&device.path.join("brightness"))?;
    Some((raw.min(max), max))
}

/// Reads the backlight again and shows it; `None` when there is none.
fn refresh(presence: &mut Presence) -> Option<(Device, u32, u32)> {
    let dir = dir();
    let read = device(&dir).and_then(|d| read(&d).map(|(raw, max)| (d, raw, max)));
    presence.now(
        read.is_some(),
        "the backlight",
        format_args!("none in {}", dir.display()),
    );
    let percent = read
        .as_ref()
        .map(|(_, raw, max)| *raw as f32 * 100.0 / *max as f32);
    publish(|levels| levels.brightness = percent);
    read
}

pub fn start() {
    let started = std::thread::Builder::new()
        .name("levels: backlight".to_owned())
        .spawn(watch);
    if let Err(err) = started {
        tracing::warn!(%err, "cannot watch the backlight");
    }
}

/// Reads the backlight, and again at each change udev or inotify tells.
fn watch() {
    let mut presence = Presence::Unknown;
    let udev = udev::MonitorBuilder::new()
        .and_then(|b| b.match_subsystem("backlight"))
        .and_then(|b| b.listen());
    let udev = match udev {
        Ok(socket) => Some(socket),
        Err(err) => {
            tracing::warn!(%err, "no udev: the backlight's changes from elsewhere go unseen");
            None
        }
    };
    let notify = inotify::init(inotify::CreateFlags::CLOEXEC | inotify::CreateFlags::NONBLOCK)
        .inspect_err(|err| tracing::debug!(%err, "no inotify for the backlight"))
        .ok();
    let watch_files = |device: Option<&Device>| {
        let Some(fd) = &notify else { return };
        let flags = inotify::WatchFlags::CREATE
            | inotify::WatchFlags::DELETE
            | inotify::WatchFlags::MODIFY
            | inotify::WatchFlags::CLOSE_WRITE;
        let _ = inotify::add_watch(fd, dir(), flags);
        if let Some(device) = device {
            let _ = inotify::add_watch(fd, device.path.join("brightness"), flags);
        }
    };
    let found = refresh(&mut presence);
    watch_files(found.as_ref().map(|(d, _, _)| d));
    let mut buffer = [std::mem::MaybeUninit::<u8>::uninit(); 1024];
    loop {
        let mut fds = Vec::new();
        if let Some(socket) = &udev {
            fds.push(PollFd::new(socket, PollFlags::IN));
        }
        if let Some(fd) = &notify {
            fds.push(PollFd::new(fd, PollFlags::IN));
        }
        if fds.is_empty() {
            tracing::info!("nothing tells the backlight's changes: it is read once");
            return;
        }
        match poll(&mut fds, None) {
            Ok(_) | Err(Errno::INTR) => {}
            Err(err) => {
                tracing::warn!(%err, "cannot wait on the backlight's changes");
                return;
            }
        }
        drop(fds);
        if let Some(socket) = &udev {
            for event in socket.iter() {
                tracing::debug!(kind = ?event.event_type(), "a backlight event");
            }
        }
        if let Some(fd) = &notify {
            let mut reader = inotify::Reader::new(fd.as_fd(), &mut buffer);
            while reader.next().is_ok() {}
        }
        let found = refresh(&mut presence);
        watch_files(found.as_ref().map(|(d, _, _)| d));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backlight(dir: &Path, name: &str, kind: &str, value: u32, max: u32) {
        let path = dir.join(name);
        std::fs::create_dir_all(&path).expect("a directory");
        std::fs::write(path.join("type"), format!("{kind}\n")).expect("type");
        std::fs::write(path.join("brightness"), format!("{value}\n")).expect("brightness");
        std::fs::write(path.join("max_brightness"), format!("{max}\n")).expect("max");
    }

    #[test]
    fn the_firmwares_backlight_first_then_the_platforms_then_a_raw_one() {
        let dir = tempfile::tempdir().expect("a directory");
        assert_eq!(device(dir.path()), None);
        backlight(dir.path(), "intel_backlight", "raw", 300, 1000);
        let found = device(dir.path()).expect("a backlight");
        assert_eq!(found.name, "intel_backlight");
        assert_eq!(read(&found), Some((300, 1000)));
        backlight(dir.path(), "acpi_video0", "firmware", 7, 15);
        assert_eq!(
            device(dir.path()).map(|d| d.name).as_deref(),
            Some("acpi_video0")
        );
        // No maximum: not a backlight to show.
        backlight(dir.path(), "broken", "raw", 1, 0);
        assert_eq!(
            read(&Device {
                name: "broken".into(),
                path: dir.path().join("broken")
            }),
            None
        );
        assert_eq!(device(Path::new("/nonexistent")), None);
    }
}
