//! The session's start-up applications (6.15): with `--session` only, once
//! per session, once the bar shows (the first frame of the shell's bar),
//! the compositor starts the XDG autostart entries that start here
//! ([`hocus_xdg::autostart`]), each as the launcher's applications are
//! (`crate::programs::launch`: off the loop, without a shell, in its
//! `Path`, in the terminal when `Terminal=true`), with
//! `XDG_CURRENT_DESKTOP=Hocuspanda`. One that fails is logged, the others
//! go on. No systemd unit: the same on FreeBSD.
//!
//! Nested, headless and in tests, nothing starts unless asked
//! (`Headless::autostart`). A shell that starts again draws its bar again:
//! nothing starts twice.

use std::path::PathBuf;

use hocus_xdg::{autostart, desktop::Locale};
use smithay::{backend::renderer::utils::with_renderer_surface_state, desktop::LayerSurface};

use crate::programs::{self, Env, on_a_thread};

/// Where the session is with its start-up applications.
#[derive(Debug, Default)]
pub enum Autostart {
    /// Not a session's: nothing starts.
    #[default]
    Off,
    /// The bar has not shown yet; then the entries of these autostart
    /// directories (the user's first) start.
    Waiting(Vec<PathBuf>),
    /// Started, once.
    Started,
}

impl crate::State {
    /// A commit of the shell's bar `layer`: its first frame (or another
    /// bar's, later) starts the start-up applications, the first time.
    /// The last session's applications start then too, before the
    /// start-up ones, which they tell apart (windows.md §14).
    pub(crate) fn bar_committed(&mut self, layer: &LayerSurface) {
        if !matches!(self.autostart, Autostart::Waiting(_)) && !self.session_waits() {
            return;
        }
        let drawn = with_renderer_surface_state(layer.wl_surface(), |s| s.buffer().is_some())
            .unwrap_or(false);
        if !drawn {
            return;
        }
        let dirs = match &self.autostart {
            Autostart::Waiting(dirs) => dirs.clone(),
            Autostart::Off | Autostart::Started => Vec::new(),
        };
        self.session_restore_start(dirs);
        let Autostart::Waiting(dirs) = std::mem::replace(&mut self.autostart, Autostart::Started)
        else {
            return;
        };
        let env = Env::of(self);
        let terminal = self.config.programs.terminal.clone();
        start(dirs, env, terminal);
    }
}

/// Reads the entries of `dirs` and starts those that start here, on a
/// thread: the directories are files.
fn start(dirs: Vec<PathBuf>, env: Env, terminal: String) {
    on_a_thread("autostart".to_owned(), move || {
        let desktops = [super::DESKTOP.to_owned()];
        let entries = autostart::scan(&dirs, &Locale::from_env(), &desktops);
        let mut started = 0;
        for entry in entries {
            if !entry.starts() {
                tracing::debug!(
                    id = entry.id,
                    hidden = entry.hidden,
                    here = ?entry.here,
                    "an autostart entry not started"
                );
                continue;
            }
            started += 1;
            let launch = hocus_link::Launch {
                id: entry.id,
                terminal: entry.terminal,
                dir: entry.dir.unwrap_or_default(),
                argv: entry.argv,
            };
            let terminal = launch.terminal.then(|| terminal.clone());
            programs::launch(launch, terminal, env.clone());
        }
        tracing::info!(started, ?dirs, "the session's start-up applications");
    });
}
