//! Her files watched (#752): her consent book, which the Settings write,
//! and the configuration file, which holds her name and her settings.
//! A change reaches her loop at once ([`crate::brain::Event::Changed`]),
//! not at her sources' next cycle: a consent given in the Settings is
//! read now, her name follows in her drawer now.
//!
//! inotify on each file's folder (editors and `write_atomically` replace a
//! file rather than write it), on a thread of its own that sleeps until a
//! change; a folder that is not there yet is not watched (her folder is
//! made before she starts, so the book's always is).

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::mpsc::Sender,
};

/// Which file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Watched {
    Consents,
    Config,
}

/// Watches `files` for changes on a thread of its own, sending each
/// change as `wrap(which)` to `to`, until `to` is gone.
pub fn start<E: Send + 'static>(
    files: Vec<(Watched, PathBuf)>,
    to: Sender<E>,
    wrap: fn(Watched) -> E,
) {
    use rustix::{
        fs::inotify::{self, CreateFlags, WatchFlags},
        io::Errno,
    };
    let fd = match inotify::init(CreateFlags::CLOEXEC) {
        Ok(fd) => fd,
        Err(err) => {
            tracing::warn!(%err, "her files are not watched: read at her cycles only");
            return;
        }
    };
    let flags = WatchFlags::CLOSE_WRITE | WatchFlags::MOVED_TO | WatchFlags::CREATE;
    // Each watch's folder, and the files of it that matter.
    let mut watches: Vec<(i32, OsString, Watched)> = Vec::new();
    for (which, file) in files {
        let (Some(dir), Some(name)) = (file.parent(), file.file_name()) else {
            continue;
        };
        let dir = if dir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            dir
        };
        match inotify::add_watch(&fd, dir, flags) {
            Ok(wd) => watches.push((wd, name.to_os_string(), which)),
            Err(err) => {
                tracing::info!(file = %file.display(), %err, "a file of hers is not watched");
            }
        }
    }
    if watches.is_empty() {
        return;
    }
    let started = std::thread::Builder::new()
        .name("her files".to_owned())
        .spawn(move || {
            let mut buffer = [std::mem::MaybeUninit::uninit(); 4096];
            let mut events = inotify::Reader::new(&fd, &mut buffer);
            loop {
                match events.next() {
                    Ok(event) => {
                        let Some(name) = event.file_name() else {
                            continue;
                        };
                        let which = watches.iter().find(|(wd, file, _)| {
                            *wd == event.wd() && name.to_bytes() == file.as_encoded_bytes()
                        });
                        if let Some((_, _, which)) = which
                            && to.send(wrap(*which)).is_err()
                        {
                            return;
                        }
                    }
                    Err(Errno::INTR) => {}
                    Err(err) => {
                        tracing::warn!(%err, "stopped watching her files");
                        return;
                    }
                }
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "no thread to watch her files");
    }
}
