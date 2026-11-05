//! The configuration file followed live: a thread waits on inotify for
//! the file's folder (editors and the Settings rename a new file over the
//! old) and its link's target's, reads and parses the file there, off the
//! loop, and sends the loop what it now says. The loop takes the parts
//! that follow the file live; the others wait for the next session.
//!
//! A file that does not read is logged and changes nothing: the desktop
//! keeps the last configuration that read.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use smithay::reexports::calloop::{
    LoopHandle,
    channel::{self, Event},
};

use crate::{App, Config, State};

/// Starts following `file`; nothing when it cannot be watched (said in
/// the log: its changes then wait for the next session).
pub fn start(handle: &LoopHandle<'static, App>, file: &Path) {
    let (sender, heard) = channel::channel::<Config>();
    if let Err(err) = handle.insert_source(heard, |event, (), app| {
        if let Event::Msg(config) = event {
            app.state.config_changed(config);
        }
    }) {
        tracing::warn!(error = %err.error, "the configuration is not followed");
        return;
    }
    let file = file.to_path_buf();
    let started = std::thread::Builder::new()
        .name("configuration".to_owned())
        .spawn(move || watch(&file, &sender));
    if let Err(err) = started {
        tracing::warn!(%err, "the configuration is not followed");
    }
}

/// The thread: blocked in inotify's read until the file may have changed.
fn watch(file: &Path, sender: &channel::Sender<Config>) {
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
                if sender.send(config).is_err() {
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
    pub fn config_changed(&mut self, new: Config) {
        let mut followed = Vec::new();
        if new.pointer != self.config.pointer || new.touchpad != self.config.touchpad {
            self.config.pointer = new.pointer;
            self.config.touchpad = new.touchpad;
            self.input_settings_changed();
            followed.push("[pointer] [touchpad]");
        }
        if !followed.is_empty() {
            tracing::info!(?followed, "the configuration changed on disk");
        }
    }
}
