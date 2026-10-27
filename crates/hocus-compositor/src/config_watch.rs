//! Watches the configuration file from a thread of its own, which reads and
//! checks it, and hands the loop a configuration ready to apply or the
//! reason it is not: the loop never touches the disk.

use std::{
    path::PathBuf,
    sync::mpsc,
    thread::JoinHandle,
    time::{Duration, SystemTime},
};

use anyhow::Context;
use hocus_core::Config;
use smithay::reexports::calloop::{LoopHandle, channel};

use crate::{App, State};

/// What the watcher found.
#[derive(Debug)]
pub enum ConfigUpdate {
    /// The configuration, and the file's text (empty when there is none).
    Read(Box<Config>, String),
    Invalid(String),
}

/// The running watcher; it stops when this is dropped.
#[derive(Debug)]
pub struct ConfigWatch {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for ConfigWatch {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Starts watching `path`, looking every `interval`: polling works the same
/// on every system and costs one `stat` per interval.
pub fn start(
    handle: &LoopHandle<'static, App>,
    path: PathBuf,
    interval: Duration,
) -> anyhow::Result<ConfigWatch> {
    let (updates, received) = channel::channel::<ConfigUpdate>();
    handle
        .insert_source(received, |event, _, app| {
            if let channel::Event::Msg(update) = event {
                app.timed("configuration", |app| app.state.apply_config_update(update));
            }
        })
        .map_err(|err| err.error)
        .context("could not watch the configuration's updates")?;
    let (stop, stopped) = mpsc::channel::<()>();
    let thread = std::thread::Builder::new()
        .name("config".into())
        .spawn(move || {
            // Unknown at first, so that the first look always reports.
            let mut seen: Option<Option<(SystemTime, u64)>> = None;
            loop {
                let now = std::fs::metadata(&path)
                    .ok()
                    .map(|m| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()));
                if seen != Some(now) {
                    seen = Some(now);
                    let update = match now {
                        // No file: the defaults.
                        None => ConfigUpdate::Read(Box::default(), String::new()),
                        Some(_) => match std::fs::read_to_string(&path) {
                            Ok(text) => match Config::parse(&text) {
                                Ok(config) => ConfigUpdate::Read(Box::new(config), text),
                                Err(err) => {
                                    ConfigUpdate::Invalid(format!("{}: {err}", path.display()))
                                }
                            },
                            Err(err) => ConfigUpdate::Invalid(format!("{}: {err}", path.display())),
                        },
                    };
                    if updates.send(update).is_err() {
                        return;
                    }
                }
                if !matches!(
                    stopped.recv_timeout(interval),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    return;
                }
            }
        })
        .context("could not start the configuration watcher")?;
    Ok(ConfigWatch {
        stop: Some(stop),
        thread: Some(thread),
    })
}

impl State {
    fn apply_config_update(&mut self, update: ConfigUpdate) {
        self.config_generation += 1;
        match update {
            ConfigUpdate::Read(config, text) => {
                self.config_error = None;
                self.config_read = true;
                self.config_text = Some(text);
                self.apply_config(*config);
                tracing::info!("configuration applied");
                self.emit(|_| hocus_api::Event::ConfigApplied);
            }
            ConfigUpdate::Invalid(message) => {
                // The previous configuration stays.
                tracing::warn!("configuration not applied: {message}");
                self.config_error = Some(message.clone());
                self.emit(|_| hocus_api::Event::ConfigError { message });
            }
        }
    }

    /// Applies a configuration already checked.
    pub fn apply_config(&mut self, config: Config) {
        let style = &mut self.style;
        let d = &config.decorations;
        style.radius = d.radius;
        style.ring_width = d.ring_width;
        // Windows' shadows follow the theme and the hour: given with the
        // theme (`apply_theme`).
        let theme = hocus_ui::Theme::builtin(
            config
                .appearance
                .theme
                .parse()
                .unwrap_or(hocus_ui::ThemeName::Dark),
        );
        let b = theme.colors.window_border;
        style.border = [b.r, b.g, b.b, b.a];
        // A ring that follows the theme takes its accent with the theme.
        if let Some(accent) = config.accent() {
            style.accent = accent;
        }
        // Drawn again with the new style.
        self.textures.clear();
        let shortcuts = config.shortcuts();
        self.config_warnings = shortcuts.warnings.clone();
        for warning in &self.config_warnings {
            tracing::warn!("configuration: {warning}");
        }
        self.shortcuts = shortcuts;
        let first = self.config_generation <= 1;
        let keyboard_changed = first || self.config.keyboard != config.keyboard;
        let outputs_changed =
            self.config.outputs != config.outputs || self.config.layouts != config.layouts;
        let notifications_changed = self.config.notifications != config.notifications;
        self.config = config;
        if notifications_changed {
            self.apply_notifications_config();
        }
        if outputs_changed {
            self.outputs_config_changed();
        }
        if keyboard_changed {
            self.apply_keyboard_config(first);
        }
        // The layouts' names follow the language too.
        self.name_layouts();
        self.relayout();
        self.needs_redraw = true;
    }
}
