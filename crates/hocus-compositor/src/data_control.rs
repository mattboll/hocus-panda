//! The clipboard for KDE Connect (#957, lot 2; docs/design/bar.md, "The
//! phone"): `ext-data-control-v1` and the older
//! `zwlr_data_control_manager_v1`, which let a client read and set the
//! clipboard whatever holds the keyboard. KDE Connect's daemon needs them
//! to bring what is copied on the phone to the desktop, and the desktop's
//! copies to the phone, as its own settings say (a copy marked secret
//! included: its `sendPassword` setting decides, not the desktop).
//!
//! No other client sees either global, so that a clipboard manager or a
//! script does not read every copy: a client is let through by its
//! executable, read once as it connects (its socket's credentials' pid,
//! `/proc/PID/exe`: a `readlink`, nothing waited for), which must be one
//! of [`crate::Config::data_control`]'s, by its file's name and the
//! directory it is under (the package's: `kdeconnectd` under `/usr`, so a
//! program renamed so in the home gets nothing).
//!
//! What such a client copies is a copy as any (5.6's `copied` line and its
//! secrets' rules), its application the allowed program's, not the window
//! focused as it came.

use std::{
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
};

use smithay::{
    reexports::{
        wayland_protocols::ext::data_control::v1::server::ext_data_control_device_v1::{
            self, ExtDataControlDeviceV1,
        },
        wayland_protocols_wlr::data_control::v1::server::zwlr_data_control_device_v1::{
            self, ZwlrDataControlDeviceV1,
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, delegate_dispatch, delegate_global_dispatch,
        },
    },
    wayland::selection::{
        ext_data_control::{
            self as ext, DataControlHandler as ExtHandler, ExtDataControlDeviceUserData,
        },
        primary_selection::PrimarySelectionState,
        wlr_data_control::{self as wlr, DataControlDeviceUserData, DataControlHandler},
    },
};

use crate::{State, state::ClientState};

/// A program that may read and set the clipboard whatever holds the
/// keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allowed {
    /// Its executable's file name.
    pub name: String,
    /// A directory its executable is under, at any depth.
    pub under: PathBuf,
    /// The application its copies are said to come from (5.6), by its id.
    pub app: String,
}

impl Allowed {
    /// KDE Connect's daemon, as distributions install it
    /// (`/usr/bin/kdeconnectd`); its copies KDE Connect's window's
    /// (`org.kde.kdeconnect.app`, "KDE Connect"): the daemon's own entry is
    /// hidden (`NoDisplay`), and the history names applications by the
    /// entries the launcher shows.
    pub fn kde_connect() -> Self {
        Self {
            name: "kdeconnectd".to_owned(),
            under: PathBuf::from("/usr"),
            app: "org.kde.kdeconnect.app".to_owned(),
        }
    }

    /// Whether the executable `exe` (as `/proc/PID/exe` names it) is this
    /// program. One deleted since it started reads `… (deleted)`, and is
    /// not.
    fn is(&self, exe: &Path) -> bool {
        exe.file_name()
            .is_some_and(|name| name == self.name.as_str())
            && exe.starts_with(&self.under)
    }
}

/// The application of the client at the other end of `stream`, if it is
/// one of `allowed`.
pub fn allowed_client(stream: &UnixStream, allowed: &[Allowed]) -> Option<String> {
    if allowed.is_empty() {
        return None;
    }
    let pid = match rustix::net::sockopt::socket_peercred(stream) {
        Ok(cred) => cred.pid.as_raw_nonzero().get(),
        Err(err) => {
            tracing::debug!(%err, "a client's credentials could not be read");
            return None;
        }
    };
    let exe = match std::fs::read_link(format!("/proc/{pid}/exe")) {
        Ok(exe) => exe,
        Err(err) => {
            tracing::debug!(%err, pid, "a client's program could not be read");
            return None;
        }
    };
    let found = allowed.iter().find(|a| a.is(&exe))?;
    tracing::info!(
        pid,
        exe = %exe.display(),
        "a client may read and set the clipboard (data control)"
    );
    Some(found.app.clone())
}

/// The two globals.
#[derive(Debug)]
pub struct DataControl {
    ext: ext::DataControlState,
    wlr: wlr::DataControlState,
}

impl DataControl {
    /// Both globals, seen only by the clients [`allowed_client`] let
    /// through; the primary selection with them.
    pub fn new(dh: &DisplayHandle, primary: &PrimarySelectionState) -> Self {
        Self {
            ext: ext::DataControlState::new::<State, _>(dh, Some(primary), allowed),
            wlr: wlr::DataControlState::new::<State, _>(dh, Some(primary), allowed),
        }
    }
}

/// Whether `client` sees the globals.
fn allowed(client: &Client) -> bool {
    client
        .get_data::<ClientState>()
        .is_some_and(|c| c.data_control.is_some())
}

/// The application of the client setting a selection.
fn app_of(client: &Client) -> Option<String> {
    client
        .get_data::<ClientState>()
        .and_then(|c| c.data_control.clone())
}

impl ExtHandler for State {
    fn data_control_state(&self) -> &ext::DataControlState {
        &self.protocols.data_control.ext
    }
}

impl DataControlHandler for State {
    fn data_control_state(&self) -> &wlr::DataControlState {
        &self.protocols.data_control.wlr
    }
}

// smithay's, but for the devices: a selection they set is the clipboard's
// history's as the allowed program's (`new_selection`, which smithay calls
// within the request).
delegate_global_dispatch!(State: [
    smithay::reexports::wayland_protocols::ext::data_control::v1::server::ext_data_control_manager_v1::ExtDataControlManagerV1: ext::ExtDataControlManagerGlobalData
] => ext::DataControlState);
delegate_dispatch!(State: [
    smithay::reexports::wayland_protocols::ext::data_control::v1::server::ext_data_control_manager_v1::ExtDataControlManagerV1: ext::ExtDataControlManagerUserData
] => ext::DataControlState);
delegate_dispatch!(State: [
    smithay::reexports::wayland_protocols::ext::data_control::v1::server::ext_data_control_source_v1::ExtDataControlSourceV1: ext::ExtDataControlSourceUserData
] => ext::DataControlState);
delegate_global_dispatch!(State: [
    smithay::reexports::wayland_protocols_wlr::data_control::v1::server::zwlr_data_control_manager_v1::ZwlrDataControlManagerV1: wlr::DataControlManagerGlobalData
] => wlr::DataControlState);
delegate_dispatch!(State: [
    smithay::reexports::wayland_protocols_wlr::data_control::v1::server::zwlr_data_control_manager_v1::ZwlrDataControlManagerV1: wlr::DataControlManagerUserData
] => wlr::DataControlState);
delegate_dispatch!(State: [
    smithay::reexports::wayland_protocols_wlr::data_control::v1::server::zwlr_data_control_source_v1::ZwlrDataControlSourceV1: wlr::DataControlSourceUserData
] => wlr::DataControlState);

impl Dispatch<ExtDataControlDeviceV1, ExtDataControlDeviceUserData> for State {
    fn request(
        state: &mut Self,
        client: &Client,
        resource: &ExtDataControlDeviceV1,
        request: ext_data_control_device_v1::Request,
        data: &ExtDataControlDeviceUserData,
        dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let setting = matches!(
            request,
            ext_data_control_device_v1::Request::SetSelection { .. }
        );
        if setting {
            state.clipboard.setter = app_of(client);
            tracing::debug!(app = ?state.clipboard.setter, "a data control client sets the clipboard");
        }
        <ext::DataControlState as Dispatch<_, _, Self>>::request(
            state, client, resource, request, data, dh, data_init,
        );
        state.clipboard.setter = None;
    }
}

impl Dispatch<ZwlrDataControlDeviceV1, DataControlDeviceUserData> for State {
    fn request(
        state: &mut Self,
        client: &Client,
        resource: &ZwlrDataControlDeviceV1,
        request: zwlr_data_control_device_v1::Request,
        data: &DataControlDeviceUserData,
        dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let setting = matches!(
            request,
            zwlr_data_control_device_v1::Request::SetSelection { .. }
        );
        if setting {
            state.clipboard.setter = app_of(client);
            tracing::debug!(app = ?state.clipboard.setter, "a data control client sets the clipboard");
        }
        <wlr::DataControlState as Dispatch<_, _, Self>>::request(
            state, client, resource, request, data, dh, data_init,
        );
        state.clipboard.setter = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kde_connect_is_its_daemon_under_usr_only() {
        let kde = Allowed::kde_connect();
        assert!(kde.is(Path::new("/usr/bin/kdeconnectd")));
        assert!(kde.is(Path::new("/usr/lib/x86_64-linux-gnu/libexec/kdeconnectd")));
        // Renamed in the home, another program, gone since it started, or
        // a directory merely named alike: not it.
        assert!(!kde.is(Path::new("/home/alex/.local/bin/kdeconnectd")));
        assert!(!kde.is(Path::new("/usr/bin/wl-paste")));
        assert!(!kde.is(Path::new("/usr/bin/kdeconnectd (deleted)")));
        assert!(!kde.is(Path::new("/usrx/bin/kdeconnectd")));
    }
}
