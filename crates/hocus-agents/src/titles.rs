//! A Claude Code session's own name, given with `/rename` (#886): its
//! transcript holds lines `{"type":"custom-title","customTitle":…}`, the
//! last one winning. An agent is named by it first (bar.md, "Its name").
//!
//! Read by the control socket's thread, off the compositor's loop, as the
//! hooks give the transcript's path: a transcript only grows, so each file
//! is read on from where it was read last, its complete lines only (a
//! line being written is read whole at the next hook). The title is never
//! logged: [`CustomTitle`]'s `Debug` says its length only.

use std::{
    collections::VecDeque,
    fmt,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
};

/// How many transcripts are remembered, the most recently read kept.
const MOST: usize = 64;

/// A session's own name, as the user gave it (never logged).
#[derive(Clone, PartialEq, Eq)]
pub struct CustomTitle(pub String);

impl fmt::Debug for CustomTitle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CustomTitle({} chars)", self.0.chars().count())
    }
}

/// What was read of one transcript.
#[derive(Debug)]
struct Read {
    path: PathBuf,
    /// Where its complete lines end, as far as read.
    offset: u64,
    title: Option<CustomTitle>,
}

/// The transcripts read so far, each from where it was left.
#[derive(Debug, Default)]
pub struct Titles {
    /// The most recently read last.
    read: VecDeque<Read>,
}

impl Titles {
    /// Transcript `path`'s custom title as it is now: its lines written
    /// since the last read are read. A file shorter than what was read
    /// (written anew) is read from its start; one that does not open
    /// keeps what was known.
    pub fn of(&mut self, path: &Path) -> Option<CustomTitle> {
        let mut read = match self.read.iter().position(|r| r.path == path) {
            Some(at) => self.read.remove(at)?,
            None => Read {
                path: path.to_path_buf(),
                offset: 0,
                title: None,
            },
        };
        if let Err(err) = read_on(&mut read) {
            tracing::debug!(%err, "a transcript could not be read");
        }
        let title = read.title.clone();
        self.read.push_back(read);
        while self.read.len() > MOST {
            self.read.pop_front();
        }
        title
    }
}

fn read_on(read: &mut Read) -> std::io::Result<()> {
    let mut file = std::fs::File::open(&read.path)?;
    let length = file.metadata()?.len();
    if length < read.offset {
        read.offset = 0;
        read.title = None;
    }
    if length == read.offset {
        return Ok(());
    }
    file.seek(SeekFrom::Start(read.offset))?;
    let mut lines = BufReader::new(file);
    let mut line = Vec::new();
    loop {
        line.clear();
        let n = lines.read_until(b'\n', &mut line)?;
        // The end, or a line still being written: read whole next time.
        if n == 0 || line.last() != Some(&b'\n') {
            return Ok(());
        }
        read.offset += n as u64;
        if let Some(title) = custom_title(&line) {
            read.title = title;
        }
    }
}

/// A transcript line's custom title: `Some(None)` for one set empty (the
/// session's name taken back), `None` for any other line.
fn custom_title(line: &[u8]) -> Option<Option<CustomTitle>> {
    const MARK: &[u8] = b"custom-title";
    // Most lines are not one: they are not parsed.
    if !line.windows(MARK.len()).any(|w| w == MARK) {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(line).ok()?;
    if value.get("type")?.as_str()? != "custom-title" {
        return None;
    }
    let title = crate::name_text(value.get("customTitle")?.as_str()?);
    Some((!title.is_empty()).then_some(CustomTitle(title)))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn line(title: &str) -> String {
        format!("{{\"type\":\"custom-title\",\"customTitle\":\"{title}\",\"sessionId\":\"abc\"}}\n")
    }

    #[test]
    fn the_last_custom_title_wins_read_as_the_transcript_grows() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("abc.jsonl");
        let mut file = std::fs::File::create(&path)?;
        file.write_all(b"{\"type\":\"user\",\"message\":\"custom-title of a prompt\"}\n")?;
        let mut titles = Titles::default();
        assert_eq!(titles.of(&path), None);
        file.write_all(line("refonte").as_bytes())?;
        assert_eq!(titles.of(&path), Some(CustomTitle("refonte".into())));
        // Nothing new: the same, nothing read again.
        assert_eq!(titles.of(&path), Some(CustomTitle("refonte".into())));
        // A line being written is read once whole.
        let next = line("revue du site");
        let (start, end) = next.split_at(20);
        file.write_all(start.as_bytes())?;
        assert_eq!(titles.of(&path), Some(CustomTitle("refonte".into())));
        file.write_all(end.as_bytes())?;
        file.write_all(b"{\"type\":\"assistant\"}\n")?;
        assert_eq!(titles.of(&path), Some(CustomTitle("revue du site".into())));
        // Taken back: none.
        file.write_all(line("").as_bytes())?;
        assert_eq!(titles.of(&path), None);
        // Written anew, shorter: read from its start.
        std::fs::write(&path, line("x"))?;
        assert_eq!(titles.of(&path), Some(CustomTitle("x".into())));
        // Gone: what was known.
        std::fs::remove_file(&path)?;
        assert_eq!(titles.of(&path), Some(CustomTitle("x".into())));
        assert_eq!(
            format!("{:?}", CustomTitle("secret".into())),
            "CustomTitle(6 chars)"
        );
        Ok(())
    }

    #[test]
    fn only_so_many_transcripts_are_remembered() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let mut titles = Titles::default();
        for i in 0..MOST + 3 {
            let path = dir.path().join(format!("{i}.jsonl"));
            std::fs::write(&path, line("t"))?;
            titles.of(&path);
        }
        assert_eq!(titles.read.len(), MOST);
        Ok(())
    }
}
