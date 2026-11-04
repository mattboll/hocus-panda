//! `SIGTERM`, as a session that ends sends it, or `kill`: the shell writes
//! what it keeps before it ends. Ended at once, as by default, it would
//! lose a change written on a thread a moment after it was made, such as
//! the notifications the history's panel showed as it closed, seen.

use tokio::signal::unix::{SignalKind, signal};

/// Watches for `SIGTERM` on a thread of its own: once this returns, it
/// ends the shell after what it keeps is written
/// ([`crate::notifications::flush`], [`crate::day::flush`]). Without that thread, it ends the
/// shell at once, as by default.
pub fn start() {
    let (ready, watching) = std::sync::mpsc::channel();
    let started = std::thread::Builder::new()
        .name("SIGTERM".to_owned())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .build()
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    tracing::warn!(%err, "no runtime to watch for SIGTERM");
                    return;
                }
            };
            let watched = {
                let _in = runtime.enter();
                signal(SignalKind::terminate())
            };
            let _ = ready.send(());
            let mut terminate = match watched {
                Ok(terminate) => terminate,
                Err(err) => {
                    tracing::warn!(%err, "cannot watch for SIGTERM");
                    return;
                }
            };
            runtime.block_on(terminate.recv());
            tracing::info!("SIGTERM: the shell ends once what it keeps is written");
            crate::notifications::flush();
            crate::day::flush();
            std::process::exit(0);
        });
    match started {
        // Its handler set (or not settable) before the shell goes on.
        Ok(_) => {
            let _ = watching.recv();
        }
        Err(err) => tracing::warn!(%err, "no thread to watch for SIGTERM"),
    }
}
