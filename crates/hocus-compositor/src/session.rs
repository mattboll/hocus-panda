//! The compositor as the user's session (`--session`): once it is ready,
//! the user's service manager and D-Bus learn where its displays are, so
//! that what they start (portals, notification daemons, applications
//! activated by D-Bus) shows in it. The session's launcher
//! (`packaging/hocuspanda-session`) takes them back when it ends. A
//! desktop portal a previous session left running is restarted then
//! ([`portals`]). Once the bar shows, the session's start-up applications
//! start ([`autostart`]).

pub mod autostart;
pub mod portals;

use std::process::Output;

use crate::programs::{Env, on_a_thread};

/// The variables the session gives the user's services.
pub const VARIABLES: [&str; 6] = [
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
    "XCURSOR_THEME",
    "XCURSOR_SIZE",
];

/// The session's `XDG_CURRENT_DESKTOP`, as the programs started have it.
pub const DESKTOP: &str = "Hocuspanda";

/// Tells the session bus's activation environment and systemd's user
/// manager about the session, on a thread: both are calls to other
/// processes. Each separately: the session bus may be one of the display
/// manager's own (`dbus-run-session`) that systemd is not on. Once systemd
/// has them, the portals it runs for another session are restarted.
pub fn announce(env: Env) {
    on_a_thread("announce the session".to_owned(), move || {
        let names: Vec<&str> = VARIABLES
            .into_iter()
            .filter(|v| *v != "DISPLAY" || env.xdisplay.is_some())
            .collect();
        let dbus = env
            .command("dbus-update-activation-environment")
            .args(&names)
            .output();
        report("D-Bus", dbus, &names);
        let systemd = env
            .command("systemctl")
            .args(["--user", "import-environment"])
            .args(&names)
            .output();
        let given = matches!(&systemd, Ok(out) if out.status.success());
        report("systemd", systemd, &names);
        if given {
            let session = portals::Session {
                desktop: DESKTOP.to_owned(),
                wayland_display: env.wayland_display().to_string_lossy().into_owned(),
                given_at: portals::now(),
            };
            portals::refresh(&env, &session);
        }
    });
}

fn report(to: &str, result: std::io::Result<Output>, names: &[&str]) {
    match result {
        Ok(out) if out.status.success() => {
            tracing::info!(to, ?names, "the session's variables given");
        }
        Ok(out) => tracing::warn!(
            to,
            status = %out.status,
            stderr = %String::from_utf8_lossy(&out.stderr).trim(),
            "could not give the session's variables"
        ),
        Err(err) => tracing::warn!(to, %err, "could not give the session's variables"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the compositor gives, the launcher takes back when the session
    /// ends.
    #[test]
    fn the_launcher_takes_back_every_variable() {
        let launcher = include_str!("../../../packaging/hocuspanda-session");
        let unset = launcher
            .lines()
            .find(|l| l.contains("unset-environment"))
            .unwrap_or_default();
        for variable in VARIABLES {
            assert!(unset.contains(variable), "{variable} in {unset:?}");
        }
    }
}
