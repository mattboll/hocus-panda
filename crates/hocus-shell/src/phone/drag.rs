//! A file dropped on a phone's row (bar.md "The phone", #957): sent to
//! that phone.
//!
//! The compositor says where a drag of files is over a bar's connections
//! ([`over`]); the row under it is found where the connections last drew
//! their phones' rows ([`placed`]). Over a row in reach that takes files,
//! the drag's files are read once from its offer, on a thread (the data
//! device's, `paste.rs`): only `file://` addresses of files, no directory,
//! else the drag is refused there and the row not lit. Lit, the shell takes
//! the offer; let go there ([`dropped`]), the files go to that phone and
//! the offer is finished on a thread.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Mutex, PoisonError},
};

use hocus_link::CONNECTIONS_NAMESPACE;
use iced::Rectangle;

use crate::{paste, popover};

/// What the drag carries, as far as read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum Files {
    #[default]
    Unknown,
    Reading,
    /// Files only: taken over a row.
    Read(Vec<PathBuf>),
    /// Something but files (a directory, another kind of address): never
    /// taken.
    Refused,
}

/// A drag of files over a bar's connections.
#[derive(Debug, Default)]
struct Drag {
    bar: String,
    /// The phone's row under it.
    row: Option<u32>,
    files: Files,
    /// Whether its offer is taken, as last said.
    taken: bool,
}

/// The drag over the connections, if one is.
static DRAG: Mutex<Option<Drag>> = Mutex::new(None);
/// The row lit for it.
static LIT: Mutex<Option<u32>> = Mutex::new(None);
/// A bar's phones' rows as last drawn, by number, in its connections'
/// surface.
type Rows = Vec<(u32, Rectangle)>;

/// Each bar's phones' rows.
static PLACED: Mutex<Option<HashMap<String, Rows>>> = Mutex::new(None);

/// The row a file is dragged over, lit for it (its number).
pub fn dropping() -> Option<u32> {
    *LIT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Bar `bar`'s connections drew its phones' rows there.
pub fn placed(bar: &str, rows: Rows) {
    let mut placed = PLACED.lock().unwrap_or_else(PoisonError::into_inner);
    let placed = placed.get_or_insert_with(HashMap::new);
    if placed.get(bar) != Some(&rows) {
        placed.insert(bar.to_owned(), rows);
    }
}

/// The row of bar `bar` at `at`, in its connections' surface.
fn row_at(bar: &str, (x, y): (i32, i32)) -> Option<u32> {
    let point = iced::Point::new(x as f32 + 0.5, y as f32 + 0.5);
    PLACED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()?
        .get(bar)?
        .iter()
        .find(|(_, rect)| rect.contains(point))
        .map(|(key, _)| *key)
}

/// The compositor says a drag of files is over bar `bar`'s connections at
/// `at`, or has left them.
pub fn over(bar: String, at: Option<(i32, i32)>) {
    let read = {
        let mut drag = DRAG.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(at) = at else {
            if drag.as_ref().is_some_and(|d| d.bar == bar) {
                *drag = None;
            }
            drop(drag);
            relight();
            return;
        };
        let drag = drag.get_or_insert_with(|| Drag {
            bar: bar.clone(),
            ..Drag::default()
        });
        if drag.bar != bar {
            *drag = Drag {
                bar: bar.clone(),
                ..Drag::default()
            };
        }
        drag.row = row_at(&bar, at);
        let read = drag.row.is_some() && drag.files == Files::Unknown;
        if read {
            drag.files = Files::Reading;
        }
        read
    };
    if read {
        let reader = std::thread::Builder::new()
            .name("dragged files".to_owned())
            .spawn(move || read_files(&bar));
        if let Err(err) = reader {
            tracing::warn!(%err, "could not read a drag's files");
        }
    }
    relight();
}

/// Reads the files the drag over bar `bar` carries, on a thread.
fn read_files(bar: &str) {
    let files = match paste::drag_files() {
        // None: an offer gone before it was asked; asked again at the
        // next move.
        Ok(urls) if urls.is_empty() => Files::Unknown,
        Ok(urls) => files_of(&urls).map_or(Files::Refused, Files::Read),
        Err(why) => {
            tracing::debug!(why, "a drag's files not read");
            Files::Refused
        }
    };
    tracing::debug!(files = ?files, "a drag's files over the phones");
    if let Some(drag) = DRAG
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
        .filter(|d| d.bar == bar && d.files == Files::Reading)
    {
        drag.files = files;
    }
    relight();
}

/// The paths of `urls`, files all of them: no other address, no directory.
fn files_of(urls: &[String]) -> Option<Vec<PathBuf>> {
    urls.iter()
        .map(|url| super::path_of(url).filter(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file())))
        .collect()
}

/// The row under the drag lights if it takes its files, and the offer is
/// taken there, or refused.
fn relight() {
    let (lit, take) = {
        let mut drag = DRAG.lock().unwrap_or_else(PoisonError::into_inner);
        match drag.as_mut() {
            Some(drag) => {
                let lit = drag
                    .row
                    .filter(|_| matches!(drag.files, Files::Read(_)))
                    .filter(|key| takes_files(*key));
                let take = (drag.taken != lit.is_some()).then_some(lit.is_some());
                drag.taken = lit.is_some();
                (lit, take)
            }
            None => (None, None),
        }
    };
    if let Some(take) = take {
        paste::drag_take(take);
    }
    let before = std::mem::replace(
        &mut *LIT.lock().unwrap_or_else(PoisonError::into_inner),
        lit,
    );
    if before != lit {
        tracing::debug!(
            row = ?lit.and_then(super::row).map(|r| r.device.name),
            "a drag of files over the phones' rows"
        );
        popover::send(vec![popover::Effect::Wake(CONNECTIONS_NAMESPACE)]);
    }
}

/// Whether row `key`'s device takes files now: in reach, its share plugin
/// on.
fn takes_files(key: u32) -> bool {
    super::row(key).is_some_and(|r| r.device.reachable && r.device.share)
}

/// The drag was let go on bar `bar`'s connections: its files go to the row
/// lit, and its offer is finished.
pub fn dropped(bar: &str) {
    let drag = {
        let mut drag = DRAG.lock().unwrap_or_else(PoisonError::into_inner);
        if drag.as_ref().is_none_or(|d| d.bar != bar) {
            return;
        }
        drag.take()
    };
    let lit = dropping();
    relight();
    let (
        Some(key),
        Some(Drag {
            files: Files::Read(paths),
            ..
        }),
    ) = (lit, drag)
    else {
        return;
    };
    let Some(row) = super::row(key) else {
        return;
    };
    tracing::debug!(
        phone = row.device.name,
        files = paths.len(),
        "files dropped on a phone's row"
    );
    super::send(&row.device.id, &paths);
    let finisher = std::thread::Builder::new()
        .name("dropped files".to_owned())
        .spawn(|| {
            if !paste::drag_finish() {
                tracing::debug!("a drop on a phone's row never came to the shell");
            }
        });
    if let Err(err) = finisher {
        tracing::warn!(%err, "could not finish a drop");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_only_no_directory_nor_another_address() {
        let dir = std::env::temp_dir().join(format!("hp-drag-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a folder");
        let file = dir.join("rapport final.pdf");
        std::fs::write(&file, b"%PDF").expect("a file");
        let url = super::super::file_url(&file);
        assert_eq!(
            files_of(std::slice::from_ref(&url)),
            Some(vec![file.clone()])
        );
        let folder = super::super::file_url(&dir);
        assert_eq!(files_of(&[url.clone(), folder]), None);
        assert_eq!(
            files_of(&[url, "https://example.org/a.pdf".to_owned()]),
            None
        );
        std::fs::remove_file(&file).expect("removed");
        std::fs::remove_dir(&dir).expect("removed");
    }
}
