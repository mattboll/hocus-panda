//! The Wayland socket, bound at an absolute path so that a test compositor can
//! live in its own runtime directory without touching the environment.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Context;
use smithay::reexports::{
    calloop::{Interest, LoopHandle, Mode, PostAction, generic::Generic},
    wayland_server::{Display, ListeningSocket},
};

use crate::{App, State, state::ClientState};

/// How many `hocus-N` names to try before giving up.
const MAX_SOCKETS: u32 = 32;

pub struct Socket {
    listener: ListeningSocket,
    pub path: PathBuf,
}

/// Binds the first free `hocus-N` socket in `runtime_dir`.
pub fn bind(runtime_dir: &Path) -> anyhow::Result<Socket> {
    for n in 0..MAX_SOCKETS {
        let path = runtime_dir.join(format!("hocus-{n}"));
        if let Ok(listener) = ListeningSocket::bind_absolute(path.clone()) {
            return Ok(Socket { listener, path });
        }
    }
    anyhow::bail!(
        "no free Wayland socket name in {} (tried hocus-0 to hocus-{})",
        runtime_dir.display(),
        MAX_SOCKETS - 1
    )
}

/// Accepts clients on the socket and dispatches their requests on the loop.
pub fn insert(
    handle: &LoopHandle<'static, App>,
    socket: Socket,
    display: Display<State>,
) -> anyhow::Result<()> {
    handle
        .insert_source(
            Generic::new(socket.listener, Interest::READ, Mode::Level),
            |_, listener, app| {
                while let Some(stream) = listener.accept()? {
                    if let Err(err) = app
                        .state
                        .display_handle
                        .insert_client(stream, Arc::new(ClientState::default()))
                    {
                        tracing::warn!(%err, "could not add client");
                    }
                }
                Ok(PostAction::Continue)
            },
        )
        .map_err(|err| err.error)
        .context("could not watch the Wayland socket")?;

    handle
        .insert_source(
            Generic::new(display, Interest::READ, Mode::Level),
            |_, display, app| {
                // SAFETY: the display is never dropped while the loop runs; it is
                // only borrowed for the duration of this dispatch.
                #[allow(unsafe_code)]
                let display = unsafe { display.get_mut() };
                app.timed("wayland clients", |app| {
                    display.dispatch_clients(&mut app.state)
                })?;
                Ok(PostAction::Continue)
            },
        )
        .map_err(|err| err.error)
        .context("could not watch the Wayland display")?;
    Ok(())
}
