//! The settings app ([decision 0010](../../../docs/decisions/0010-settings-as-a-client.md)):
//! a client of its own. Opening it brings its window forward when it runs,
//! telling it the page through the event stream, and starts it otherwise.

use std::time::Duration;

use hocus_api::Event;
use smithay::utils::SERIAL_COUNTER;

use crate::{State, desktops::title_and_app_id};

/// How long a start may take before asking again starts it again.
const START_PATIENCE: Duration = Duration::from_secs(5);

/// At start, before the loop runs: the settings app beside the
/// compositor, when they were built together, quoted for the shell. The
/// one look at the file system for it; opening the settings asks none.
pub fn start(state: &mut State) {
    let command = hocus_core::config::Settings::default().command;
    state.settings_beside = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(&command)))
        .filter(|p| p.is_file())
        .map(|p| format!("'{}'", p.to_string_lossy().replace('\'', "'\\''")));
}

impl State {
    /// Shows the settings, on `page` when given (`appearance`,
    /// `keyboard`…).
    pub fn open_settings(&mut self, page: Option<String>) {
        let app_id = self.config.settings.app_id.clone();
        let window = self.windows.iter().find_map(|(id, m)| {
            let (_, a) = title_and_app_id(&m.window);
            (a.as_deref() == Some(app_id.as_str())).then_some(*id)
        });
        let now = self.clock.now();
        match window {
            Some(id) => {
                // Back from minimized, focused, on top.
                self.restore(id, SERIAL_COUNTER.next_serial());
                self.emit(|_| Event::OpenSettings { page: page.clone() });
            }
            None if self
                .settings_started
                .is_some_and(|t| now.saturating_sub(t) < START_PATIENCE) => {}
            None => {
                let mut command = self.config.settings.command.clone();
                // Next to the compositor when built together (a nested
                // session from the source tree), else from PATH.
                if command == hocus_core::config::Settings::default().command
                    && let Some(beside) = &self.settings_beside
                {
                    command.clone_from(beside);
                }
                if let Some(page) =
                    page.filter(|p| p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
                {
                    command.push_str(" --page ");
                    command.push_str(&page);
                }
                tracing::info!(command, "starting the settings");
                self.launch("/bin/sh", &["-c".to_owned(), command]);
                self.settings_started = Some(now);
            }
        }
    }

    /// A window just managed: the settings' own stops the wait.
    pub fn settings_arrived(&mut self, app_id: Option<&str>) {
        if app_id == Some(self.config.settings.app_id.as_str()) {
            self.settings_started = None;
        }
    }
}
