//! The text in pictures, through tesseract or what replaces it (`PROGRAM
//! IMAGE -` prints it), read once per picture (#302). The indexer reads
//! the screenshots folder's pictures, and a screenshot's toast offers to
//! copy its text: both may ask for the same file, at the same time or one
//! soon after the other. The second gets the first one's answer rather
//! than running the program again.

use std::{
    collections::VecDeque,
    os::unix::fs::MetadataExt as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Condvar, Mutex, MutexGuard},
};

/// Answers kept for pictures read already: what asks again (the indexer
/// after a click, a click after the indexer) comes soon after.
const KEPT: usize = 16;

/// A picture as it was read, and by what: changed, it is read again.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Key {
    program: String,
    path: PathBuf,
    inode: u64,
    modified: (i64, i64),
    size: u64,
}

#[derive(Debug)]
enum Read {
    /// The program runs; who asks too waits for its answer.
    Running,
    Done(Option<String>),
}

/// The reads of pictures, shared by everything that reads their text.
#[derive(Debug, Default)]
pub struct Pictures {
    reads: Mutex<VecDeque<(Key, Read)>>,
    changed: Condvar,
}

impl Pictures {
    /// The text in the picture at `path`; `None` when there is none. On a
    /// worker: it waits for the program, or for the read of the same
    /// picture another thread started.
    pub fn read(&self, program: &str, path: &Path) -> Option<String> {
        let Ok(meta) = std::fs::metadata(path) else {
            // Nothing to know it again by; the program says what it can.
            return run(program, path).flatten();
        };
        let key = Key {
            program: program.to_owned(),
            path: path.to_owned(),
            inode: meta.ino(),
            modified: (meta.mtime(), meta.mtime_nsec()),
            size: meta.len(),
        };
        let mut reads = self.lock();
        loop {
            match reads.iter().find(|(k, _)| *k == key).map(|(_, r)| r) {
                Some(Read::Done(text)) => return text.clone(),
                Some(Read::Running) => {
                    reads = self.changed.wait(reads).unwrap_or_else(|e| e.into_inner());
                }
                None => break,
            }
        }
        reads.push_back((key.clone(), Read::Running));
        drop(reads);
        // Whatever happens to this thread, those waiting for it go on.
        let mut running = Running {
            pictures: self,
            key,
            text: None,
        };
        running.text = run(program, path);
        running.text.clone().flatten()
    }

    fn lock(&self) -> MutexGuard<'_, VecDeque<(Key, Read)>> {
        self.reads.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A read under way; dropped, it is done.
struct Running<'a> {
    pictures: &'a Pictures,
    key: Key,
    /// `None`: the program did not run; `Some(None)`: no text found.
    text: Option<Option<String>>,
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        let mut reads = self.pictures.lock();
        match self.text.take() {
            Some(text) => {
                if let Some((_, read)) = reads.iter_mut().find(|(k, _)| *k == self.key) {
                    *read = Read::Done(text);
                }
            }
            // Not kept: the program may be installed by the next time.
            None => reads.retain(|(k, _)| *k != self.key),
        }
        // The oldest answers go first; reads under way stay.
        let mut done = reads
            .iter()
            .filter(|(_, r)| matches!(r, Read::Done(_)))
            .count();
        reads.retain(|(_, r)| {
            let drop = done > KEPT && matches!(r, Read::Done(_));
            done -= usize::from(drop);
            !drop
        });
        self.pictures.changed.notify_all();
    }
}

/// The program's answer; `None` when it could not be run.
fn run(program: &str, path: &Path) -> Option<Option<String>> {
    let mut words = program.split_whitespace();
    let mut command = Command::new(words.next()?);
    command.args(words);
    let out = command
        .arg(path)
        .arg("-")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    Some((out.status.success() && !text.is_empty()).then_some(text))
}

#[cfg(test)]
mod tests {
    use std::{os::unix::fs::PermissionsExt as _, sync::Arc};

    use super::*;

    /// A reader that counts its runs in `runs` and waits for `go`, giving
    /// up by itself after ten seconds.
    fn reader(dir: &Path) -> std::io::Result<PathBuf> {
        let program = dir.join("ocr");
        std::fs::write(
            &program,
            format!(
                "#!/bin/sh\necho run >> '{runs}'\nn=0\nwhile [ ! -e '{go}' ] && [ $n -lt 1000 ]; do sleep 0.01; n=$((n + 1)); done\necho \"text of $(basename \"$1\")\"\n",
                runs = dir.join("runs").display(),
                go = dir.join("go").display(),
            ),
        )?;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))?;
        Ok(program)
    }

    fn runs(dir: &Path) -> usize {
        std::fs::read_to_string(dir.join("runs"))
            .unwrap_or_default()
            .lines()
            .count()
    }

    #[test]
    fn a_picture_asked_for_twice_at_once_is_read_once() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let program = reader(dir.path())?.display().to_string();
        let picture = dir.path().join("shot.png");
        std::fs::write(&picture, b"a picture")?;
        let pictures = Arc::new(Pictures::default());
        let first = {
            let (pictures, program, picture) = (pictures.clone(), program.clone(), picture.clone());
            std::thread::spawn(move || pictures.read(&program, &picture))
        };
        while runs(dir.path()) == 0 {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let second = {
            let (pictures, program, picture) = (pictures.clone(), program.clone(), picture.clone());
            std::thread::spawn(move || pictures.read(&program, &picture))
        };
        // The second waits for the first, which waits for the test. Come
        // later, it would find the answer kept: the same, slower or not.
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(dir.path().join("go"), "")?;
        let want = Some("text of shot.png".to_owned());
        assert_eq!(first.join().ok(), Some(want.clone()));
        assert_eq!(second.join().ok(), Some(want.clone()));
        assert_eq!(runs(dir.path()), 1);
        // Asked again later: the answer kept.
        assert_eq!(pictures.read(&program, &picture), want);
        assert_eq!(runs(dir.path()), 1);
        Ok(())
    }

    #[test]
    fn a_picture_changed_is_read_again() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::write(dir.path().join("go"), "")?;
        let program = reader(dir.path())?.display().to_string();
        let picture = dir.path().join("shot.png");
        std::fs::write(&picture, b"a picture")?;
        let pictures = Pictures::default();
        assert!(pictures.read(&program, &picture).is_some());
        std::fs::write(&picture, b"another picture")?;
        assert!(pictures.read(&program, &picture).is_some());
        assert_eq!(runs(dir.path()), 2);
        Ok(())
    }

    #[test]
    fn a_program_that_does_not_run_is_tried_again() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let picture = dir.path().join("shot.png");
        std::fs::write(&picture, b"a picture")?;
        let program = dir.path().join("ocr").display().to_string();
        let pictures = Pictures::default();
        assert_eq!(pictures.read(&program, &picture), None);
        std::fs::write(dir.path().join("go"), "")?;
        reader(dir.path())?;
        assert!(pictures.read(&program, &picture).is_some());
        Ok(())
    }
}
