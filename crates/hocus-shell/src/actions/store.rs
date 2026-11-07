//! The palette's recent actions (#736): the last ones run from it, the
//! latest first, one id a line in `$XDG_STATE_HOME/hocuspanda/actions`.
//! Read on a thread as the shell starts, written on one of its own after
//! each action run.

use std::{
    path::Path,
    sync::{Mutex, PoisonError, mpsc},
};

static RECENT: Mutex<Option<Vec<String>>> = Mutex::new(None);
static WRITE: Mutex<Option<mpsc::Sender<Vec<String>>>> = Mutex::new(None);

/// The recent actions' ids, the latest first.
pub fn recent() -> Vec<String> {
    RECENT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .unwrap_or_default()
}

/// Action `id` ran: first among the recent ones, written off the thread
/// that draws.
pub fn ran(id: &str) {
    let recent = {
        let mut recent = RECENT.lock().unwrap_or_else(PoisonError::into_inner);
        let recent = recent.get_or_insert_with(Vec::new);
        super::list::ran(recent, id);
        recent.clone()
    };
    if let Some(write) = WRITE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        let _ = write.send(recent);
    }
}

/// What a file holds: the ids of actions still in the catalogue.
fn read(text: &str) -> Vec<String> {
    let mut recent: Vec<String> = Vec::new();
    for id in text.lines().map(str::trim) {
        if hocus_link::actions::find(id).is_some() && !recent.iter().any(|r| r == id) {
            recent.push(id.to_owned());
        }
    }
    recent
}

/// Reads them, and writes what changes, on a thread of its own.
pub fn start() {
    let Some(file) = crate::launcher::apps::state_file("actions") else {
        return;
    };
    let (write, writes) = mpsc::channel::<Vec<String>>();
    *WRITE.lock().unwrap_or_else(PoisonError::into_inner) = Some(write);
    let started = std::thread::Builder::new()
        .name("actions".to_owned())
        .spawn(move || {
            match std::fs::read_to_string(&file) {
                Ok(text) => {
                    let mut read = read(&text);
                    let mut kept = RECENT.lock().unwrap_or_else(PoisonError::into_inner);
                    // Run while it was read: those first.
                    if let Some(now) = kept.take() {
                        for id in now.iter().rev() {
                            super::list::ran(&mut read, id);
                        }
                    }
                    *kept = Some(read);
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => tracing::warn!(file = %file.display(), %err, "recent actions not read"),
            }
            while let Ok(mut recent) = writes.recv() {
                recent = writes.try_iter().last().unwrap_or(recent);
                if let Err(err) = save(&file, &recent) {
                    tracing::warn!(file = %file.display(), %err, "recent actions not written");
                }
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot keep the recent actions");
    }
}

fn save(file: &Path, recent: &[String]) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut text = String::new();
    for id in recent {
        text.push_str(id);
        text.push('\n');
    }
    let new = file.with_extension("new");
    std::fs::write(&new, text)?;
    std::fs::rename(&new, file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_actions_still_there_once_each() {
        assert_eq!(
            read("maximize\nno-such-action\n\nsnap-right\nmaximize\n"),
            ["maximize", "snap-right"]
        );
    }
}
