//! A click on a tray icon brings its application forward (5.14b, bar.md
//! §4 group 11).
//!
//! The click is the user's input, but it lands on the shell's surface: the
//! application the shell then activates over D-Bus never had the keyboard,
//! and the token it makes for itself is not one the user's input made. So
//! the shell asks for a token before it calls `Activate` and hands it to
//! the item (`ProvideXdgActivationToken`); and, for the applications that
//! ignore it (Electron's), it names the item's process, which may then
//! focus one window within [`CLICK_FOCUSES`]: by asking, with any token,
//! or by mapping it.
//!
//! Which Wayland client is that process's application: its own process,
//! one it started (Chromium's helpers, a launcher script's program), one
//! that started it (a tray helper of the application's), or, sandboxed, a
//! process of the same application unit (`app-….scope`, `snap.….scope`):
//! Flatpak's D-Bus goes through a proxy beside the application. Never by
//! process group, nor through the desktop: what the compositor starts
//! shares its group and its unit, and everything descends from it.

use std::time::Duration;

use hocus_agents::process;
use smithay::reexports::wayland_server::{Resource, protocol::wl_surface::WlSurface};

use crate::State;

/// How long after a tray click its application may focus a window.
pub const CLICK_FOCUSES: Duration = Duration::from_secs(2);

/// The last tray click, until its application used it or its time ran out.
#[derive(Debug, Clone)]
pub struct TrayClick {
    /// The item's D-Bus process.
    pub pid: u32,
    /// The token made for it.
    pub token: String,
    /// When, on the compositor's clock.
    pub at: Duration,
}

impl State {
    /// The user clicked a tray icon: a token for it, to the shell, and one
    /// focus for the item's application.
    pub fn tray_clicked(&mut self, ask: u64, pid: u32) {
        let token = self.activation_token();
        tracing::debug!(ask, pid, "a tray click");
        self.tray_click = Some(TrayClick {
            pid,
            token: token.clone(),
            at: self.now(),
        });
        self.tell_shell(hocus_link::ToShell::TrayToken { ask, token });
    }

    /// Whether the last tray click lets a window of process `pid` take
    /// the focus now; it does once.
    pub fn tray_click_focuses(&mut self, pid: Option<u32>) -> bool {
        let now = self.now();
        let Some(click) = &self.tray_click else {
            return false;
        };
        if now.saturating_sub(click.at) >= CLICK_FOCUSES {
            self.tray_click = None;
            return false;
        }
        let ours = std::process::id();
        let mine = pid.is_some_and(|pid| {
            same_application(pid, click.pid, ours, &mut process::ancestry, &mut cgroup_of)
        });
        if mine {
            tracing::debug!(?pid, "a tray click focuses its window");
            self.tray_click = None;
        }
        mine
    }

    /// A token was used: the click it was made for is spent.
    pub fn tray_token_used(&mut self, token: &str) {
        if self.tray_click.as_ref().is_some_and(|c| c.token == token) {
            self.tray_click = None;
        }
    }

    /// The process of the client of `surface`.
    pub fn client_pid(&self, surface: &WlSurface) -> Option<u32> {
        let client = self.display_handle.get_client(surface.id()).ok()?;
        let credentials = client.get_credentials(&self.display_handle).ok()?;
        u32::try_from(credentials.pid).ok()
    }
}

/// Process `pid`'s control group, as the unified hierarchy names it.
/// Read on the loop: procfs answers from memory.
fn cgroup_of(pid: u32) -> Option<String> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix("0::"))
        .map(str::to_owned)
}

/// Whether a control group is an application's own unit, as desktops and
/// sandboxes start them (systemd's `app-…` units, Flatpak's
/// `app-flatpak-….scope`, Snap's `snap.….scope`).
fn is_app_unit(cgroup: &str) -> bool {
    let unit = cgroup.rsplit('/').next().unwrap_or_default();
    (unit.starts_with("app-") && (unit.ends_with(".scope") || unit.ends_with(".service")))
        || (unit.starts_with("snap.") && unit.ends_with(".scope"))
}

/// Whether the Wayland client of process `window` is the application
/// whose tray item's D-Bus connection is process `item`; `ours` is the
/// compositor's process, `ancestry` a process and those above it, nearest
/// first, `cgroup` a process's control group.
fn same_application(
    window: u32,
    item: u32,
    ours: u32,
    ancestry: &mut dyn FnMut(u32) -> Vec<u32>,
    cgroup: &mut dyn FnMut(u32) -> Option<String>,
) -> bool {
    if window <= 1 || item <= 1 {
        return false;
    }
    if window == item {
        return true;
    }
    // The desktop and what is above it are everyone's parents.
    let desktop = ancestry(ours);
    if desktop.contains(&window) || desktop.contains(&item) {
        return false;
    }
    if ancestry(window).contains(&item) || ancestry(item).contains(&window) {
        return true;
    }
    match (cgroup(window), cgroup(item)) {
        (Some(a), Some(b)) => a == b && is_app_unit(&a) && cgroup(ours).as_ref() != Some(&a),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// A machine: each process's parent and control group.
    struct Machine(HashMap<u32, (u32, &'static str)>);

    impl Machine {
        fn ancestry(&self, pid: u32) -> Vec<u32> {
            process::ancestry_in(pid, |p| {
                self.0.get(&p).map(|(parent, _)| process::Process {
                    name: String::new(),
                    parent: *parent,
                })
            })
        }

        fn same(&self, window: u32, item: u32) -> bool {
            same_application(window, item, 100, &mut |p| self.ancestry(p), &mut |p| {
                self.0.get(&p).map(|(_, c)| (*c).to_owned())
            })
        }
    }

    const SESSION: &str = "/user.slice/user-1000.slice/session-2.scope";
    const FLATPAK: &str = "/user.slice/user-1000.slice/user@1000.service/app.slice/\
                           app-flatpak-com.slack.Slack-4242.scope";

    const SIGNAL: &str = "/user.slice/user-1000.slice/user@1000.service/app.slice/\
                          app-flatpak-org.signal.Signal-77.scope";

    fn machine() -> Machine {
        Machine(HashMap::from([
            // The compositor, started from a login shell.
            (90, (1, SESSION)),
            (100, (90, SESSION)),
            // Slack started by the desktop: its helpers under it.
            (200, (100, SESSION)),
            (201, (200, SESSION)),
            (202, (201, SESSION)),
            // Another program the desktop started.
            (300, (100, SESSION)),
            // A program whose tray is a helper it started.
            (400, (100, SESSION)),
            (401, (400, SESSION)),
            // Flatpak's Slack: its D-Bus proxy beside it, both under bwrap.
            (500, (1, FLATPAK)),
            (501, (500, FLATPAK)),
            (502, (500, FLATPAK)),
            (503, (502, FLATPAK)),
            // Another Flatpak, of another unit.
            (600, (1, SIGNAL)),
        ]))
    }

    #[test]
    fn its_own_process_and_those_it_started_or_that_started_it() {
        let m = machine();
        assert!(m.same(200, 200));
        // Electron's Wayland client a child, or a grandchild, of its D-Bus
        // process.
        assert!(m.same(202, 200));
        // The tray a helper of the program with the window.
        assert!(m.same(400, 401));
    }

    #[test]
    fn a_sandboxed_one_by_its_unit() {
        let m = machine();
        // Flatpak: the item's connection is its proxy's, the window its
        // application's.
        assert!(m.same(503, 501));
        assert!(!m.same(600, 501));
    }

    #[test]
    fn never_another_program_nor_through_the_desktop() {
        let m = machine();
        // Siblings, the desktop's children both, in the session's group.
        assert!(!m.same(300, 200));
        assert!(!m.same(200, 300));
        // The compositor and its parents are everyone's ancestors.
        assert!(!m.same(200, 100));
        assert!(!m.same(200, 90));
        assert!(!m.same(100, 200));
        // Not known.
        assert!(!m.same(0, 200));
        assert!(!m.same(200, 0));
        assert!(!m.same(1, 200));
    }

    #[test]
    fn units_of_applications() {
        assert!(is_app_unit(FLATPAK));
        assert!(is_app_unit(
            "/user.slice/user-1000.slice/user@1000.service/app.slice/app-gnome-slack-1234.scope"
        ));
        assert!(is_app_unit(
            "/user.slice/user-1000.slice/user@1000.service/app.slice/app-slack@autostart.service"
        ));
        assert!(is_app_unit(
            "/user.slice/user-1000.slice/user@1000.service/app.slice/snap.slack.slack-0c1d.scope"
        ));
        assert!(!is_app_unit(SESSION));
        assert!(!is_app_unit(
            "/user.slice/user-1000.slice/user@1000.service/app.slice"
        ));
        assert!(!is_app_unit("/"));
    }
}
