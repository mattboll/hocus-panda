//! What the emoji picker remembers (5.7): the last 16 emoji used, the
//! latest first, and the skin tone chosen, in
//! `$XDG_STATE_HOME/hocuspanda/emoji`: a `tone N` line (0 the yellow one,
//! then light to dark), then one emoji a line, without its tone. Read on a
//! thread as the shell starts, written on one of its own after each
//! change.

use std::{
    path::Path,
    sync::{Mutex, PoisonError, mpsc},
};

/// The recent emoji kept at most.
pub const RECENT: usize = 16;

/// What is kept.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Kept {
    pub tone: usize,
    pub recent: Vec<String>,
}

impl Kept {
    /// `emoji` used: first among the recent ones, once.
    pub fn used(&mut self, emoji: &str) {
        self.recent.retain(|e| e != emoji);
        self.recent.insert(0, emoji.to_owned());
        self.recent.truncate(RECENT);
    }

    pub fn read(text: &str) -> Self {
        let mut kept = Self::default();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            if let Some(tone) = line.strip_prefix("tone ") {
                kept.tone = tone
                    .trim()
                    .parse()
                    .ok()
                    .filter(|t| *t < super::data::TONES.len())
                    .unwrap_or(0);
            } else if hocus_link::emoji_text(line)
                && kept.recent.len() < RECENT
                && !kept.recent.iter().any(|e| e == line)
            {
                kept.recent.push(line.to_owned());
            }
        }
        kept
    }

    pub fn write(&self) -> String {
        let mut text = format!("tone {}\n", self.tone);
        for emoji in &self.recent {
            text.push_str(emoji);
            text.push('\n');
        }
        text
    }
}

static KEPT: Mutex<Option<Kept>> = Mutex::new(None);
static WRITE: Mutex<Option<mpsc::Sender<Kept>>> = Mutex::new(None);

/// What is kept now.
pub fn kept() -> Kept {
    KEPT.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .unwrap_or_default()
}

/// Changes what is kept, and writes it off the thread that draws.
pub fn change(with: impl FnOnce(&mut Kept)) {
    let kept = {
        let mut kept = KEPT.lock().unwrap_or_else(PoisonError::into_inner);
        let kept = kept.get_or_insert_with(Kept::default);
        with(kept);
        kept.clone()
    };
    if let Some(write) = WRITE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        let _ = write.send(kept);
    }
}

/// Reads what was kept, and writes what changes, on threads of their own;
/// the table of emoji is read meanwhile.
pub fn start() {
    let Some(file) = crate::launcher::apps::state_file("emoji") else {
        return;
    };
    let (write, writes) = mpsc::channel::<Kept>();
    *WRITE.lock().unwrap_or_else(PoisonError::into_inner) = Some(write);
    let started = std::thread::Builder::new()
        .name("emoji".to_owned())
        .spawn(move || {
            match std::fs::read_to_string(&file) {
                Ok(text) => {
                    let read = Kept::read(&text);
                    let mut kept = KEPT.lock().unwrap_or_else(PoisonError::into_inner);
                    // Used while it was read: those first.
                    let mut merged = read;
                    if let Some(now) = kept.take() {
                        for emoji in now.recent.iter().rev() {
                            merged.used(emoji);
                        }
                        if now.tone != 0 {
                            merged.tone = now.tone;
                        }
                    }
                    *kept = Some(merged);
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => tracing::warn!(file = %file.display(), %err, "recent emoji not read"),
            }
            // Read now, not as the picker first opens.
            let _ = super::data::table();
            while let Ok(mut kept) = writes.recv() {
                kept = writes.try_iter().last().unwrap_or(kept);
                if let Err(err) = save(&file, &kept) {
                    tracing::warn!(file = %file.display(), %err, "recent emoji not written");
                }
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot keep the recent emoji");
    }
}

fn save(file: &Path, kept: &Kept) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let new = file.with_extension("new");
    std::fs::write(&new, kept.write())?;
    std::fs::rename(&new, file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_sixteen_used_first_once_each_and_the_tone() {
        let mut kept = Kept::default();
        for i in 0..20u32 {
            kept.used(&char::from_u32(0x1F600 + i).expect("an emoji").to_string());
        }
        kept.used("😃");
        assert_eq!(kept.recent.len(), RECENT);
        assert_eq!(kept.recent[0], "😃");
        assert_eq!(kept.recent.iter().filter(|e| *e == "😃").count(), 1);
        kept.tone = 4;
        let read = Kept::read(&kept.write());
        assert_eq!(read, kept);
        // What does not read is left out.
        let read = Kept::read("tone 9\n👍\n\nnot an emoji\n👍\n🐱\n");
        assert_eq!(read.tone, 0);
        assert_eq!(read.recent, ["👍", "🐱"]);
    }
}
