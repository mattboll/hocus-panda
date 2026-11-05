//! The configuration file followed live: a thread waits on inotify for
//! the file's folder (editors and the Settings rename a new file over the
//! old) and its link's target's, reads and parses the file there, off the
//! loop, and sends the loop what it now says. The loop takes the parts
//! that follow the file live; the others wait for the next session.
//!
//! A file that does not read is logged and changes nothing: the desktop
//! keeps the last configuration that read.
//!
//! Followed live: `[pointer]` and `[touchpad]` (6.12), `[shortcuts]` and
//! `[keyboard]` (6.8), `[appearance]` and `[locale]` (6.2). A keyboard whose layouts or options changed has its
//! keymap compiled on the thread too (XKB reads its files), the loop only
//! taking it in; its repeat is given to clients again.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use smithay::reexports::calloop::{
    LoopHandle,
    channel::{self, Event},
};

use smithay::input::keyboard::xkb;

use crate::{App, Config, State};

/// What the file says now, and the keymap it makes when its keyboard's
/// layouts or options changed (or why XKB refused them).
#[derive(Debug)]
pub struct Reread {
    pub config: Config,
    pub keymap: Option<Result<String, String>>,
}

/// Starts following `file`, its keyboard compiled as `current` (the
/// configuration the session started with); nothing when it cannot be
/// watched (said in the log: its changes then wait for the next session).
pub fn start(handle: &LoopHandle<'static, App>, file: &Path, current: &Config) {
    let (sender, heard) = channel::channel::<Reread>();
    if let Err(err) = handle.insert_source(heard, |event, (), app| {
        if let Event::Msg(reread) = event {
            app.state.config_changed(reread);
        }
    }) {
        tracing::warn!(error = %err.error, "the configuration is not followed");
        return;
    }
    let file = file.to_path_buf();
    let system = current.system_keyboard.clone();
    let mut names = keymap_names(current);
    let started = std::thread::Builder::new()
        .name("configuration".to_owned())
        .spawn(move || {
            watch(&file, |mut config| {
                config.system_keyboard = system.clone();
                let now = keymap_names(&config);
                let keymap = (now != names).then(|| compile(&now));
                names = now;
                sender.send(Reread { config, keymap }).is_ok()
            });
        });
    if let Err(err) = started {
        tracing::warn!(%err, "the configuration is not followed");
    }
}

/// What XKB is given for `config`'s keyboard.
fn keymap_names(config: &Config) -> (String, String, String, String) {
    let names = config.xkb_names();
    (names.model, names.layout, names.variant, names.options)
}

/// The keymap of these names, as text for the loop to take in.
fn compile(
    (model, layout, variant, options): &(String, String, String, String),
) -> Result<String, String> {
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let keymap = xkb::Keymap::new_from_names(
        &context,
        "",
        model,
        layout,
        variant,
        (!options.is_empty()).then(|| options.clone()),
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    )
    .ok_or_else(|| format!("XKB refused layouts {layout:?} ({variant:?}), options {options:?}"))?;
    Ok(keymap.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1))
}

/// The thread: blocked in inotify's read until the file may have changed;
/// `send` takes each new configuration, and says `false` once the loop is
/// gone.
fn watch(file: &Path, mut send: impl FnMut(Config) -> bool) {
    use rustix::fs::inotify;
    let Some(name) = file.file_name().map(ToOwned::to_owned) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = file.parent().map(Path::to_path_buf).into_iter().collect();
    let mut names: Vec<OsString> = vec![name];
    if let Ok(real) = std::fs::canonicalize(file)
        && real != file
    {
        dirs.extend(real.parent().map(Path::to_path_buf));
        names.extend(real.file_name().map(ToOwned::to_owned));
    }
    let fd = match inotify::init(inotify::CreateFlags::CLOEXEC) {
        Ok(fd) => fd,
        Err(err) => {
            tracing::warn!(%err, "the configuration is not followed");
            return;
        }
    };
    let flags = inotify::WatchFlags::CLOSE_WRITE
        | inotify::WatchFlags::MOVED_TO
        | inotify::WatchFlags::CREATE
        | inotify::WatchFlags::DELETE;
    let mut watched = false;
    for dir in &dirs {
        // A folder not there yet is made: the Settings' first change makes
        // it all the same.
        let _ = std::fs::create_dir_all(dir);
        match inotify::add_watch(&fd, dir, flags) {
            Ok(_) => watched = true,
            Err(err) => tracing::info!(%err, dir = %dir.display(), "a folder is not watched"),
        }
    }
    if !watched {
        tracing::warn!(file = %file.display(), "the configuration's folder is not there: not followed");
        return;
    }
    // What was last read, so that the loop hears only of changes. The
    // file is read once as soon as it is watched, without waiting: a
    // change made before the watch began is not missed (the loop takes
    // only what differs from what it has).
    let mut last: Option<String> = None;
    let mut first = true;
    let mut buffer = [std::mem::MaybeUninit::<u8>::uninit(); 4096];
    let mut reader = inotify::Reader::new(&fd, &mut buffer);
    loop {
        if !std::mem::take(&mut first) {
            match reader.next() {
                Ok(event) => {
                    let ours = event.file_name().is_some_and(|n| {
                        names
                            .iter()
                            .any(|name| name.as_encoded_bytes() == n.to_bytes())
                    });
                    if !ours {
                        continue;
                    }
                }
                Err(rustix::io::Errno::INTR) => continue,
                Err(err) => {
                    tracing::warn!(%err, "the configuration is no longer followed");
                    return;
                }
            }
        }
        let text = match std::fs::read_to_string(file) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(err) => {
                tracing::warn!(file = %file.display(), %err, "the configuration is not read again");
                continue;
            }
        };
        if last.as_ref() == Some(&text) {
            continue;
        }
        last = Some(text.clone());
        match toml::from_str::<Config>(&text) {
            Ok(config) => {
                if !send(config) {
                    return;
                }
            }
            Err(err) => tracing::warn!(
                file = %file.display(),
                "the configuration changed but does not read; the last one that read is kept: {err}"
            ),
        }
    }
}

impl State {
    /// The file says `new` now: the parts that follow it live take it.
    pub fn config_changed(&mut self, reread: Reread) {
        let Reread {
            config: new,
            keymap,
        } = reread;
        let mut followed = Vec::new();
        if new.shortcuts != self.config.shortcuts {
            self.config.shortcuts = new.shortcuts.clone();
            followed.push("[shortcuts]");
        }
        if new.keyboard != self.config.keyboard || keymap.is_some() {
            self.keyboard_changed(new.keyboard.clone(), keymap);
            followed.push("[keyboard]");
        }
        // The screens and the primary screen (6.9, displays.rs).
        if self.displays_config_changed(&new) {
            followed.push("[outputs] [panel] output");
        }
        if new.pointer != self.config.pointer || new.touchpad != self.config.touchpad {
            self.config.pointer = new.pointer;
            self.config.touchpad = new.touchpad;
            self.input_settings_changed();
            followed.push("[pointer] [touchpad]");
        }
        // The theme, the accent, motion, transparency, contrast and the
        // cursor (#402, appearance.rs).
        if self.appearance_changed(new.appearance.clone()) {
            followed.push("[appearance]");
        }
        if self.locale_changed(new.locale) {
            followed.push("[locale]");
        }
        // The windows in the bar (#604): the shell shows or hides them.
        if new.panel.windows != self.config.panel.windows {
            self.config.panel.windows = new.panel.windows;
            self.tell_shell(hocus_link::ToShell::BarWindows(new.panel.windows));
            followed.push("[panel] windows");
        }
        if !followed.is_empty() {
            tracing::info!(?followed, "the configuration changed on disk");
        }
    }

    /// `[keyboard]` changed: the keymap taken in (compiled off the loop),
    /// the repeat given again; Num Lock is the session's start's.
    fn keyboard_changed(
        &mut self,
        keyboard: crate::config::Keyboard,
        keymap: Option<Result<String, String>>,
    ) {
        let Some(handle) = self.seat.get_keyboard() else {
            self.config.keyboard = keyboard;
            return;
        };
        if (keyboard.repeat_rate, keyboard.repeat_delay)
            != (
                self.config.keyboard.repeat_rate,
                self.config.keyboard.repeat_delay,
            )
        {
            handle.change_repeat_info(keyboard.repeat_rate, keyboard.repeat_delay);
        }
        self.config.keyboard = keyboard;
        match keymap {
            None => {}
            Some(Err(err)) => tracing::warn!(err, "the new layouts are refused: the keymap stays"),
            Some(Ok(text)) => match handle.set_keymap_from_string(self, text) {
                Ok(()) => {
                    let names = self.config.xkb_names();
                    tracing::info!(
                        layout = names.layout,
                        variant = names.variant,
                        options = names.options,
                        "the keyboard's new keymap"
                    );
                    self.layout_codes = crate::layouts::codes(&names);
                    if self.shell_linked() {
                        self.tell_layouts();
                    }
                }
                Err(err) => tracing::warn!(?err, "the new keymap is refused: the old one stays"),
            },
        }
    }
}
