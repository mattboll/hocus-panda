//! Starting XWayland, without waiting for it: the X11 window manager starts
//! when XWayland says it is ready.

use std::process::Stdio;

use smithay::{
    reexports::calloop::LoopHandle,
    xwayland::{X11Wm, XWayland, XWaylandEvent},
};

use crate::{App, State};

/// Spawns XWayland on the next free display; X11 clients can connect once
/// the window manager runs. Without the `Xwayland` binary, X11 applications
/// just do not start.
pub fn start(handle: &LoopHandle<'static, App>, state: &mut State) {
    let spawned = XWayland::spawn(
        &state.display_handle,
        None,
        std::iter::empty::<(String, String)>(),
        false,
        Stdio::null(),
        Stdio::null(),
        |_| {},
    );
    let (xwayland, client) = match spawned {
        Ok(spawned) => spawned,
        Err(err) => {
            tracing::warn!(%err, "XWayland not started: X11 applications will not run");
            return;
        }
    };
    let wm_handle = handle.clone();
    let inserted = handle.insert_source(xwayland, move |event, _, app| match event {
        XWaylandEvent::Ready {
            x11_socket,
            display_number,
        } => match X11Wm::start_wm(wm_handle.clone(), x11_socket, client.clone()) {
            Ok(wm) => {
                tracing::info!(display = display_number, "XWayland ready");
                app.state.xwm = Some(wm);
                app.state.xdisplay = Some(display_number);
            }
            Err(err) => tracing::warn!(%err, "could not start the X11 window manager"),
        },
        XWaylandEvent::Error => tracing::warn!("XWayland stopped while starting"),
    });
    if let Err(err) = inserted {
        tracing::warn!(err = %err.error, "could not watch XWayland");
    }
}
