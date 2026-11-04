//! The local task list (Day.1, "On this computer"): a board of two
//! columns, "todo" and "done", kept in `$XDG_STATE_HOME/hocuspanda/
//! tasks.json`. Read as its worker starts, written by that worker (never
//! the shell's loop) after each change, whole or not at all.
//!
//! ```json
//! {"todo": [{"id": "…", "title": "…", "due": null, "position": 65536.0}],
//!  "done": [...]}
//! ```
//!
//! A file that does not read is left as it is: the list then refuses
//! changes rather than write over it.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};

use crate::{
    fixture::{move_in, top},
    model::{BoardId, Column, ColumnId, Place, Task, TaskBoard, TaskId, sort_tasks},
    source::{KanbanSource, Result, SourceError},
};

pub const TODO: &str = "todo";
pub const DONE: &str = "done";

#[derive(Debug, Default, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    todo: Vec<Task>,
    #[serde(default)]
    done: Vec<Task>,
}

/// The local list.
#[derive(Debug)]
pub struct LocalList {
    /// Its file; `None` keeps it in memory only (no state directory).
    file: Option<PathBuf>,
    columns: HashMap<ColumnId, Vec<Task>>,
    /// Why its file does not read, if it does not.
    broken: Option<String>,
}

impl LocalList {
    /// The list in `file`, read now (on the worker's thread).
    pub fn open(file: Option<PathBuf>) -> Self {
        let (stored, broken) = match &file {
            None => (Stored::default(), None),
            Some(path) => match std::fs::read_to_string(path) {
                Ok(text) => match serde_json::from_str::<Stored>(&text) {
                    Ok(stored) => (stored, None),
                    Err(err) => {
                        let why = format!("{}: {err}", path.display());
                        tracing::warn!(%why, "the local task list does not read: left as it is");
                        (Stored::default(), Some(why))
                    }
                },
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => (Stored::default(), None),
                Err(err) => {
                    let why = format!("{}: {err}", path.display());
                    tracing::warn!(%why, "the local task list is not read");
                    (Stored::default(), Some(why))
                }
            },
        };
        let columns = HashMap::from([
            (ColumnId::new(TODO), stored.todo),
            (ColumnId::new(DONE), stored.done),
        ]);
        Self {
            file,
            columns,
            broken,
        }
    }

    fn writable(&self) -> Result<()> {
        match &self.broken {
            Some(why) => Err(SourceError::Invalid(why.clone())),
            None => Ok(()),
        }
    }

    /// Writes the list to its file.
    fn write(&self) -> Result<()> {
        let Some(file) = &self.file else {
            return Ok(());
        };
        let column = |name: &str| {
            let mut tasks = self
                .columns
                .get(&ColumnId::new(name))
                .cloned()
                .unwrap_or_default();
            sort_tasks(&mut tasks);
            tasks
        };
        let stored = Stored {
            todo: column(TODO),
            done: column(DONE),
        };
        let text = serde_json::to_string_pretty(&stored)
            .map_err(|err| SourceError::Invalid(err.to_string()))?;
        crate::write_file(file, text.as_bytes())
            .map_err(|err| SourceError::Refused(format!("{}: {err}", file.display())))?;
        tracing::debug!(file = %file.display(), "the local task list written");
        Ok(())
    }
}

impl KanbanSource for LocalList {
    fn board(&mut self) -> Result<TaskBoard> {
        Ok(TaskBoard {
            id: BoardId::new("local"),
            name: "local".to_owned(),
            columns: [TODO, DONE]
                .into_iter()
                .map(|name| Column {
                    id: ColumnId::new(name),
                    name: name.to_owned(),
                })
                .collect(),
        })
    }

    fn cards(&mut self, column: &ColumnId) -> Result<Vec<Task>> {
        let mut cards = self.columns.get(column).cloned().unwrap_or_default();
        sort_tasks(&mut cards);
        Ok(cards)
    }

    fn create(&mut self, column: &ColumnId, title: &str) -> Result<Task> {
        self.writable()?;
        let cards = self.columns.entry(column.clone()).or_default();
        let task = Task {
            id: new_id(),
            title: title.to_owned(),
            due: None,
            position: top(cards),
        };
        cards.push(task.clone());
        self.write()?;
        Ok(task)
    }

    fn move_card(&mut self, card: &TaskId, place: &Place) -> Result<()> {
        self.writable()?;
        move_in(&mut self.columns, card, place)?;
        self.write()
    }
}

/// A task's id, unique on this computer: the time it was made and a
/// count.
fn new_id() -> TaskId {
    static MADE: AtomicU64 = AtomicU64::new(0);
    let nanos = jiff::Timestamp::now().as_nanosecond();
    TaskId::new(format!(
        "{:x}-{}",
        nanos,
        MADE.fetch_add(1, Ordering::Relaxed)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(list: &mut LocalList, column: &str) -> Vec<String> {
        list.cards(&ColumnId::new(column))
            .expect("cards")
            .into_iter()
            .map(|t| t.title)
            .collect()
    }

    #[test]
    fn add_tick_untick_written_and_read_back() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("hocuspanda/tasks.json");
        let mut list = LocalList::open(Some(file.clone()));
        let todo = ColumnId::new(TODO);
        let done = ColumnId::new(DONE);
        for title in ["Un", "Deux", "Trois"] {
            list.create(&todo, title)?;
        }
        // Each at the top.
        assert_eq!(titles(&mut list, TODO), ["Trois", "Deux", "Un"]);
        // "Deux" ticked: to the top of done.
        let deux = list.cards(&todo)?[1].clone();
        let was = Place {
            column: todo.clone(),
            position: Some(deux.position),
        };
        list.move_card(
            &deux.id,
            &Place {
                column: done.clone(),
                position: None,
            },
        )?;
        assert_eq!(titles(&mut list, TODO), ["Trois", "Un"]);
        assert_eq!(titles(&mut list, DONE), ["Deux"]);
        // Read back as written.
        let mut again = LocalList::open(Some(file.clone()));
        assert_eq!(titles(&mut again, TODO), ["Trois", "Un"]);
        assert_eq!(titles(&mut again, DONE), ["Deux"]);
        // Unticked: back to its place, between the two.
        list.move_card(&deux.id, &was)?;
        assert_eq!(titles(&mut list, TODO), ["Trois", "Deux", "Un"]);
        assert!(titles(&mut list, DONE).is_empty());
        let mut again = LocalList::open(Some(file.clone()));
        assert_eq!(titles(&mut again, TODO), ["Trois", "Deux", "Un"]);
        // A card that is no more.
        assert_eq!(
            list.move_card(&TaskId::new("nope"), &was),
            Err(SourceError::Gone)
        );
        // Readable by the user alone.
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file)?.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        Ok(())
    }

    #[test]
    fn a_file_that_does_not_read_is_left_as_it_is() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("tasks.json");
        std::fs::write(&file, "{ not json")?;
        let mut list = LocalList::open(Some(file.clone()));
        assert!(matches!(
            list.create(&ColumnId::new(TODO), "Un"),
            Err(SourceError::Invalid(_))
        ));
        assert_eq!(std::fs::read_to_string(&file)?, "{ not json");
        Ok(())
    }
}
