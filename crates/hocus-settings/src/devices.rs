//! The mice and touchpads plugged in, as the compositor says them (6.12):
//! a thread of its own follows them on the desktop's control socket
//! (`msg input-devices follow`, `$HOCUS_SOCKET`), never the window's.
//!
//! The window hears the whole list each time it changes. Without a
//! desktop to ask (started outside Hocuspanda), it hears nothing and
//! shows every row.

use std::{
    io::{BufRead as _, BufReader, Write as _},
    os::unix::net::UnixStream,
    path::PathBuf,
};

use hocus_link::input::{DeviceEvent, InputDevice};

/// Where the compositor listens: `$HOCUS_SOCKET`, which it sets for the
/// programs it starts, else beside `$WAYLAND_DISPLAY`'s socket.
pub fn socket() -> Option<PathBuf> {
    if let Some(socket) = std::env::var_os(hocus_agents::SOCKET_ENV).filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(socket));
    }
    let display = PathBuf::from(std::env::var_os("WAYLAND_DISPLAY")?);
    let wayland = if display.is_absolute() {
        display
    } else {
        PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?).join(display)
    };
    Some(hocus_agents::socket_path(&wayland))
}

/// Starts following the devices on `socket`; `heard` takes the list each
/// time it changes, and returns `false` once the window is gone.
pub fn follow(socket: PathBuf, mut heard: impl FnMut(Vec<InputDevice>) -> bool + Send + 'static) {
    let started = std::thread::Builder::new()
        .name("settings' devices".to_owned())
        .spawn(move || {
            let mut stream = match UnixStream::connect(&socket) {
                Ok(stream) => stream,
                Err(err) => {
                    tracing::info!(%err, socket = %socket.display(), "the devices are not known");
                    return;
                }
            };
            let line = format!(
                "{}\n",
                hocus_agents::Question::InputDevices { follow: true }.line()
            );
            if let Err(err) = stream.write_all(line.as_bytes()) {
                tracing::info!(%err, "the devices are not known");
                return;
            }
            let mut devices: Vec<InputDevice> = Vec::new();
            let mut listed = false;
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else {
                    break;
                };
                match line.parse::<DeviceEvent>() {
                    Ok(DeviceEvent::Added(device) | DeviceEvent::Changed(device)) => {
                        match devices.iter_mut().find(|d| d.id == device.id) {
                            Some(known) => *known = device,
                            None => devices.push(device),
                        }
                    }
                    Ok(DeviceEvent::Removed(id)) => devices.retain(|d| d.id != id),
                    Ok(DeviceEvent::Listed) => listed = true,
                    Err(err) => {
                        tracing::debug!(%err, "a device's line that does not read");
                        continue;
                    }
                }
                if listed {
                    tracing::debug!(devices = devices.len(), "the devices");
                    if !heard(devices.clone()) {
                        return;
                    }
                }
            }
            tracing::info!("the desktop stopped telling the devices");
        });
    if let Err(err) = started {
        tracing::warn!(%err, "the devices are not followed");
    }
}
