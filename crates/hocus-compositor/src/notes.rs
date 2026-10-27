//! Notes (#79): one Markdown file, read and written on worker threads.
//! Super+N shows them in the launcher's card; the command field finds
//! them. Its to-dos (`- [ ] …`, `- [x] …`) are tasks now (#255): those
//! found in the file, or typed so in the card, move to the task list.

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
        if self.notes_todos_to_tasks() {
            self.save_notes();
        } else {
            self.show_notes();
        }
    }

    /// The notes' to-dos go to the tasks; whether some went.
    fn notes_todos_to_tasks(&mut self) -> bool {
        let todos: Vec<(bool, String)> = self
            .notes
            .notes
            .lines
            .iter()
            .filter_map(|l| match l {
                Line::Todo { done, text } => Some((*done, text.clone())),
                _ => None,
            })
            .collect();
        if !self.tasks_from_notes(todos) {
            return false;
        }
        self.notes
            .notes
            .lines
            .retain(|l| !matches!(l, Line::Todo { .. }));
        true
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
                // Left only without a task list to move them to.
                Line::Todo { text, .. } | Line::Note(text) => Some(NoteItem {
                    index,
                    text: text.clone(),
                }),
                Line::Other(_) => None,
            })
            .collect();
        if let Some(panel) = self.launcher.as_mut() {
            panel.launcher.set_notes(items);
        }
        self.redraw.all();
    }

    /// Super+N: the notes in the launcher's card, read again first.
    pub fn toggle_notes(&mut self) {
        if let Some((path, loaded)) = &self.notes.read {
            read(path.clone(), loaded.clone());
        }
        self.show_notes();
        self.toggle_launcher_mode(Mode::Notes);
    }

    /// A line typed in the notes; one typed as a to-do is a task.
    pub fn add_note(&mut self, typed: &str) {
        if self.notes.notes.add(typed) {
            self.notes_todos_to_tasks();
            self.save_notes();
        }
    }

    pub fn remove_note(&mut self, index: usize) {
        if self.notes.notes.remove(index) {
            self.save_notes();
        }
    }
}
