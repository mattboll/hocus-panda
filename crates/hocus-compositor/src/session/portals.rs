//! The desktop portal left by a previous session (#538). The user's
//! service manager outlives a graphical session (lingering, a terminal
//! multiplexer, another login): `xdg-desktop-portal` may still be the
//! process a previous session started, with its `XDG_CURRENT_DESKTOP` and
//! `WAYLAND_DISPLAY`. It never reads the variables the session gives
//! systemd again, so it keeps reading that session's `*-portals.conf` and
//! sends the screen cast to a backend that is not this desktop's. Once the
//! variables are given, the portal and its backends that run as units of
//! the user's manager are restarted when they serve another session; those
//! not running are left to D-Bus activation, which starts them with the
//! variables given.
//!
//! **The rule**, for each unit that runs (its main process known):
//! - its process's environment (`/proc/PID/environ`, what it was started
//!   with and reads its configuration by) names another desktop or another
//!   display than this session's: restarted;
//! - the same desktop and display: left, it serves this session;
//! - its environment unreadable (another system, a process that hides it):
//!   restarted if it became active before the variables reached systemd
//!   (`ActiveEnterTimestampMonotonic`), as it could not have had them;
//!   left otherwise.
//!
//! The environment comes first because it is what the portal acts on: a
//! portal activated by an application started with the session, between
//! the session's start and the variables' reaching systemd, is newer than
//! the session yet has the previous one's variables; and a portal from
//! this same desktop on the same display (the session started again after
//! a crash) serves it as it is.

use std::process::Command;

use crate::programs::Env;

/// The portal's frontend, the user's unit.
pub const FRONTEND: &str = "xdg-desktop-portal.service";

/// Its backends that run as units (`SystemdService=` in their D-Bus
/// service files), whatever desktop they are for. Not the frontend (no
/// `-` after `portal`), nor the document portal and the permission store,
/// which serve every desktop alike.
pub const BACKENDS: &str = "*xdg-desktop-portal-*.service";

/// The variables a portal is checked by: the desktop decides which
/// backends it uses, the display which screen a backend's windows go to.
const CHECKED: [&str; 2] = ["XDG_CURRENT_DESKTOP", "WAYLAND_DISPLAY"];

/// What this session gave the user's manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub desktop: String,
    pub wayland_display: String,
    /// When systemd had the variables: `CLOCK_MONOTONIC`, microseconds,
    /// the clock of systemd's `…TimestampMonotonic`.
    pub given_at: u64,
}

/// A unit as `systemctl show` describes it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Unit {
    pub id: String,
    pub active: bool,
    /// Its main process; 0 when none runs.
    pub main_pid: u32,
    /// When it became active (`CLOCK_MONOTONIC`, microseconds); 0 when it
    /// never did.
    pub active_since: u64,
}

/// What is done with a unit, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Not running: D-Bus activation starts it, with the variables given.
    NotRunning,
    /// Started for this session: the same desktop and display.
    Serves,
    /// Started for another session: what it has, `(variable, value)`, of
    /// those that differ (`None`: unset).
    Other(Vec<(&'static str, Option<String>)>),
    /// Its environment unreadable, and active since before the variables
    /// reached systemd.
    Older,
    /// Its environment unreadable, and active since they did.
    Newer,
}

impl Decision {
    pub fn restart(&self) -> bool {
        matches!(self, Self::Other(_) | Self::Older)
    }
}

/// What to do with `unit`, whose process was started with `environ`
/// (`/proc/PID/environ`: `NAME=value`, each ended by a NUL), `None` when it
/// could not be read.
pub fn decide(unit: &Unit, environ: Option<&[u8]>, session: &Session) -> Decision {
    if !unit.active || unit.main_pid == 0 {
        return Decision::NotRunning;
    }
    match environ {
        Some(environ) => {
            let ours = [session.desktop.as_str(), session.wayland_display.as_str()];
            let differ: Vec<_> = CHECKED
                .into_iter()
                .zip(ours)
                .filter_map(|(name, ours)| {
                    let theirs = variable(environ, name);
                    (theirs.as_deref() != Some(ours)).then_some((name, theirs))
                })
                .collect();
            if differ.is_empty() {
                Decision::Serves
            } else {
                Decision::Other(differ)
            }
        }
        None if unit.active_since < session.given_at => Decision::Older,
        None => Decision::Newer,
    }
}

/// `name`'s value in a process's environment, as `/proc/PID/environ`
/// holds it.
fn variable(environ: &[u8], name: &str) -> Option<String> {
    environ.split(|b| *b == 0).find_map(|entry| {
        let value = entry.strip_prefix(name.as_bytes())?.strip_prefix(b"=")?;
        Some(String::from_utf8_lossy(value).into_owned())
    })
}

/// The units of `systemctl show -p Id,ActiveState,MainPID,
/// ActiveEnterTimestampMonotonic`: one block of `Key=value` lines each,
/// blocks apart by an empty line.
pub fn parse_show(text: &str) -> Vec<Unit> {
    text.split("\n\n")
        .filter_map(|block| {
            let mut unit = Unit::default();
            for line in block.lines() {
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                match key {
                    "Id" => unit.id = value.to_owned(),
                    "ActiveState" => unit.active = value == "active",
                    "MainPID" => unit.main_pid = value.parse().unwrap_or(0),
                    "ActiveEnterTimestampMonotonic" => {
                        unit.active_since = value.parse().unwrap_or(0);
                    }
                    _ => {}
                }
            }
            (!unit.id.is_empty()).then_some(unit)
        })
        .collect()
}

/// The units' names of `systemctl list-units --plain --no-legend`: the
/// first word of each line.
pub fn parse_list(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| name.ends_with(".service"))
        .map(str::to_owned)
        .collect()
}

/// `CLOCK_MONOTONIC` now, microseconds.
pub fn now() -> u64 {
    let clock = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    let secs = u64::try_from(clock.tv_sec).unwrap_or(0);
    let micros = u64::try_from(clock.tv_nsec).unwrap_or(0) / 1000;
    secs * 1_000_000 + micros
}

/// Restarts the portal and its backends that serve another session. On
/// the announcing thread, after systemd has the variables: it waits for
/// `systemctl`, which is never done on the loop.
pub fn refresh(env: &Env, session: &Session) {
    let backends = match systemctl(
        env,
        &[
            "list-units",
            "--type=service",
            "--state=running",
            "--plain",
            "--no-legend",
            "--full",
            BACKENDS,
        ],
    ) {
        Ok(text) => parse_list(&text),
        Err(err) => {
            tracing::warn!(%err, "could not list the portal's backends");
            Vec::new()
        }
    };
    let mut args = vec![
        "show",
        "-p",
        "Id,ActiveState,MainPID,ActiveEnterTimestampMonotonic",
        FRONTEND,
    ];
    args.extend(backends.iter().map(String::as_str));
    let units = match systemctl(env, &args) {
        Ok(text) => parse_show(&text),
        Err(err) => {
            tracing::warn!(%err, "could not ask systemd about the desktop portal");
            return;
        }
    };
    let mut stale_backends = Vec::new();
    let mut stale_frontend = None;
    for unit in &units {
        let environ = (unit.main_pid != 0)
            .then(|| std::fs::read(format!("/proc/{}/environ", unit.main_pid)).ok())
            .flatten();
        let decision = decide(unit, environ.as_deref(), session);
        tracing::debug!(
            unit = unit.id,
            pid = unit.main_pid,
            ?decision,
            "a portal's unit"
        );
        if decision.restart() {
            if unit.id == FRONTEND {
                stale_frontend = Some((unit, decision));
            } else {
                stale_backends.push((unit, decision));
            }
        }
    }
    // The backends first: the frontend started again finds them serving
    // this session.
    for (unit, decision) in stale_backends.into_iter().chain(stale_frontend) {
        restart(env, unit, &decision);
    }
}

fn restart(env: &Env, unit: &Unit, decision: &Decision) {
    let had = match decision {
        Decision::Other(differ) => format!("{differ:?}"),
        _ => "its environment unread, active before the session's variables".to_owned(),
    };
    match systemctl(env, &["restart", &unit.id]) {
        Ok(_) => tracing::info!(
            unit = unit.id,
            old_pid = unit.main_pid,
            had,
            "restarted a portal left by a previous session"
        ),
        Err(err) => tracing::warn!(
            unit = unit.id,
            old_pid = unit.main_pid,
            had,
            %err,
            "could not restart a portal left by a previous session"
        ),
    }
}

/// `systemctl --user ARGS`, its output when it succeeds.
fn systemctl(env: &Env, args: &[&str]) -> Result<String, String> {
    let mut cmd: Command = env.command("systemctl");
    cmd.arg("--user").args(args);
    match cmd.output() {
        Ok(out) if out.status.success() => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
        Ok(out) => Err(format!(
            "{}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Err(err) => Err(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        Session {
            desktop: "Hocuspanda".to_owned(),
            wayland_display: "hocus-1".to_owned(),
            given_at: 5_000_000,
        }
    }

    fn running(since: u64) -> Unit {
        Unit {
            id: FRONTEND.to_owned(),
            active: true,
            main_pid: 812,
            active_since: since,
        }
    }

    fn environ(vars: &[(&str, &str)]) -> Vec<u8> {
        let mut out = b"HOME=/home/alex\0PATH=/usr/bin\0".to_vec();
        for (name, value) in vars {
            out.extend_from_slice(format!("{name}={value}\0").as_bytes());
        }
        out
    }

    /// A portal a previous session started keeps that session's desktop
    /// and display: restarted.
    #[test]
    fn a_portal_of_a_previous_session_is_restarted() {
        let env = environ(&[
            ("XDG_CURRENT_DESKTOP", "Previous"),
            ("WAYLAND_DISPLAY", "wayland-0"),
        ]);
        let decision = decide(&running(1_000), Some(&env), &session());
        assert_eq!(
            decision,
            Decision::Other(vec![
                ("XDG_CURRENT_DESKTOP", Some("Previous".to_owned())),
                ("WAYLAND_DISPLAY", Some("wayland-0".to_owned())),
            ])
        );
        assert!(decision.restart());
    }

    /// One already started for this session (the same variables) is left,
    /// however old: the session started again on the same display.
    #[test]
    fn a_portal_of_this_session_is_left() {
        let env = environ(&[
            ("WAYLAND_DISPLAY", "hocus-1"),
            ("XDG_CURRENT_DESKTOP", "Hocuspanda"),
        ]);
        for since in [1_000, 9_000_000] {
            let decision = decide(&running(since), Some(&env), &session());
            assert_eq!(decision, Decision::Serves);
            assert!(!decision.restart());
        }
    }

    /// Newer than the session, yet with the previous one's variables: an
    /// application started with the session activated it before systemd
    /// had them.
    #[test]
    fn a_newer_portal_with_old_variables_is_restarted() {
        let env = environ(&[
            ("XDG_CURRENT_DESKTOP", "Hocuspanda"),
            ("WAYLAND_DISPLAY", "wayland-0"),
        ]);
        let decision = decide(&running(9_000_000), Some(&env), &session());
        assert_eq!(
            decision,
            Decision::Other(vec![("WAYLAND_DISPLAY", Some("wayland-0".to_owned()))])
        );
        assert!(decision.restart());
    }

    /// Without the variables at all (started before any session gave
    /// them): restarted.
    #[test]
    fn a_portal_without_the_variables_is_restarted() {
        let decision = decide(&running(9_000_000), Some(&environ(&[])), &session());
        assert_eq!(
            decision,
            Decision::Other(vec![
                ("XDG_CURRENT_DESKTOP", None),
                ("WAYLAND_DISPLAY", None),
            ])
        );
    }

    /// A value is the whole of it: neither a prefix nor a longer name.
    #[test]
    fn a_variable_is_read_whole() {
        let env = environ(&[
            ("XDG_CURRENT_DESKTOP_EXTRA", "Hocuspanda"),
            ("WAYLAND_DISPLAY", "hocus-10"),
            ("XDG_CURRENT_DESKTOP", "Hocuspanda:Previous"),
        ]);
        let decision = decide(&running(1), Some(&env), &session());
        assert_eq!(
            decision,
            Decision::Other(vec![
                (
                    "XDG_CURRENT_DESKTOP",
                    Some("Hocuspanda:Previous".to_owned())
                ),
                ("WAYLAND_DISPLAY", Some("hocus-10".to_owned())),
            ])
        );
    }

    /// Its environment unreadable: when it became active decides.
    #[test]
    fn an_unreadable_portal_goes_by_its_start() {
        let older = decide(&running(4_999_999), None, &session());
        assert_eq!(older, Decision::Older);
        assert!(older.restart());
        let newer = decide(&running(5_000_000), None, &session());
        assert_eq!(newer, Decision::Newer);
        assert!(!newer.restart());
    }

    /// Not running: left to D-Bus activation, which starts it with the
    /// variables given.
    #[test]
    fn a_portal_not_running_is_left() {
        let stale = environ(&[("XDG_CURRENT_DESKTOP", "Previous")]);
        let inactive = Unit {
            id: FRONTEND.to_owned(),
            ..Unit::default()
        };
        assert_eq!(decide(&inactive, None, &session()), Decision::NotRunning);
        let deactivating = Unit {
            active: false,
            ..running(1)
        };
        assert_eq!(
            decide(&deactivating, Some(&stale), &session()),
            Decision::NotRunning
        );
        let no_process = Unit {
            main_pid: 0,
            ..running(1)
        };
        assert!(!decide(&no_process, None, &session()).restart());
    }

    /// `systemctl show` of the frontend and a backend, as systemd 25x
    /// prints it (its properties in its own order).
    #[test]
    fn systemctl_show_is_read() {
        let text = "MainPID=812\nId=xdg-desktop-portal.service\nActiveState=active\n\
                    ActiveEnterTimestampMonotonic=4210337\n\n\
                    MainPID=0\nId=xdg-desktop-portal-gtk.service\nActiveState=inactive\n\
                    ActiveEnterTimestampMonotonic=0\n";
        assert_eq!(
            parse_show(text),
            vec![
                Unit {
                    id: FRONTEND.to_owned(),
                    active: true,
                    main_pid: 812,
                    active_since: 4_210_337,
                },
                Unit {
                    id: "xdg-desktop-portal-gtk.service".to_owned(),
                    active: false,
                    main_pid: 0,
                    active_since: 0,
                },
            ]
        );
        assert!(parse_show("").is_empty());
    }

    #[test]
    fn systemctl_list_units_is_read() {
        let text = "xdg-desktop-portal-gtk.service loaded active running Portal service (GTK implementation)\n";
        assert_eq!(parse_list(text), vec!["xdg-desktop-portal-gtk.service"]);
        assert!(parse_list("").is_empty());
    }

    /// The units matched are the backends, never the frontend itself.
    #[test]
    fn the_backends_pattern_leaves_the_frontend_out() {
        let infix = BACKENDS
            .trim_start_matches('*')
            .trim_end_matches(".service")
            .trim_end_matches('*');
        assert_eq!(infix, "xdg-desktop-portal-");
        assert!(!FRONTEND.contains(infix));
        assert!("xdg-desktop-portal-gtk.service".contains(infix));
        assert!(!"xdg-document-portal.service".contains(infix));
    }
}
