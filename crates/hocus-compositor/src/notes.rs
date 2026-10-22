//! Notes and to-dos (#79): one Markdown file, read and written on worker
//! threads. Super+N shows them in the launcher's card; the column shows
//! the to-dos not done; the command field finds them.

use std::path::PathBuf;

use hocus_core::notes::{Line, Notes};
use hocus_shell::launcher::{Mode, NoteItem};
use smithay::reexports::calloop::{LoopHandle, channel};

use crate::{App, State};

#[derive(Debug, Default)]
pub struct NotesState {
    pub notes: Notes,
    /// Writes the whole file, in order, off the loop.
    writer: Option<std::sync::mpsc::Sender<String>>,
    /// The file, and where reading it ends, to read it again when the
    /// notes open: it may have been edited.
    read: Option<(PathBuf, channel::Sender<String>)>,
}

/// Reads `path` on a worker thread; its text goes to `loaded`.
fn read(path: PathBuf, loaded: channel::Sender<String>) {
    let spawned = std::thread::Builder::new()
        .name("notes read".into())
        .spawn(move || match std::fs::read_to_string(&path) {
            Ok(text) => {
                let _ = loaded.send(text);
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => tracing::warn!(%err, "the notes do not read"),
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "the notes were not read");
    }
}

/// The notes' file: `$XDG_DATA_HOME/hocuspanda/notes.md` in a session, else
/// in the state directory given (tests).
pub fn file(is_session: bool, state_dir: Option<&std::path::Path>) -> Option<PathBuf> {
    if is_session {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .map(|d| d.join("hocuspanda").join("notes.md"))
    } else {
        state_dir.map(|d| d.join("notes.md"))
    }
}

pub fn start(handle: &LoopHandle<'static, App>, state: &mut State, path: Option<PathBuf>) {
    let Some(path) = path else {
        return;
    };
    let (loaded, received) = channel::channel::<String>();
    let inserted = handle.insert_source(received, |event, _, app| {
        if let channel::Event::Msg(text) = event {
            app.timed("notes", |app| app.state.notes_loaded(&text));
        }
    });
    if let Err(err) = inserted {
        tracing::warn!(%err.error, "no notes");
        return;
    }
    read(path.clone(), loaded.clone());
    state.notes.read = Some((path.clone(), loaded));
    let (writer, wanted) = std::sync::mpsc::channel::<String>();
    let spawned = std::thread::Builder::new()
        .name("notes write".into())
        .spawn(move || {
            while let Ok(text) = wanted.recv() {
                let tmp = path.with_extension("md.new");
                let written = path
                    .parent()
                    .map_or(Ok(()), std::fs::create_dir_all)
                    .and_then(|()| std::fs::write(&tmp, &text))
                    .and_then(|()| std::fs::rename(&tmp, &path));
                if let Err(err) = written {
                    tracing::warn!(%err, "the notes were not saved");
                }
            }
        });
    match spawned {
        Ok(_) => state.notes.writer = Some(writer),
        Err(err) => tracing::warn!(%err, "the notes will not be saved"),
    }
}

impl State {
    fn notes_loaded(&mut self, text: &str) {
        self.notes.notes = Notes::parse(text);
        self.show_notes();
    }

    fn save_notes(&mut self) {
        if let Some(writer) = &self.notes.writer {
            let _ = writer.send(self.notes.notes.to_markdown());
        }
        self.show_notes();
    }

    /// The card and the column show the notes as they are.
    fn show_notes(&mut self) {
        let items: Vec<NoteItem> = self
            .notes
            .notes
            .lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| match line {
                Line::Todo { done, text } => Some(NoteItem {
                    index,
                    text: text.clone(),
                    todo: Some(*done),
                }),
                Line::Note(text) => Some(NoteItem {
                    index,
                    text: text.clone(),
                    todo: None,
                }),
                Line::Other(_) => None,
            })
            .collect();
        if let Some(panel) = self.launcher.as_mut() {
            panel.launcher.set_notes(items);
        }
        self.needs_redraw = true;
    }

    /// The column's to-dos.
    pub fn open_todos(&self) -> Vec<(usize, String)> {
        self.notes
            .notes
            .open_todos()
            .into_iter()
            .map(|(i, t)| (i, t.to_owned()))
            .collect()
    }

    /// Super+N: the notes in the launcher's card, read again first.
    pub fn toggle_notes(&mut self) {
        if let Some((path, loaded)) = &self.notes.read {
            read(path.clone(), loaded.clone());
        }
        self.show_notes();
        self.toggle_launcher_mode(Mode::Notes);
    }

    pub fn add_note(&mut self, typed: &str) {
        if self.notes.notes.add(typed) {
            self.save_notes();
        }
    }

    pub fn tick_note(&mut self, index: usize) {
        if self.notes.notes.toggle(index) {
            self.save_notes();
        }
    }

    pub fn remove_note(&mut self, index: usize) {
        if self.notes.notes.remove(index) {
            self.save_notes();
        }
    }
}
