//! Default applications, as the XDG MIME Applications Associations
//! specification keeps them: the `mimeapps.list` files say which desktop
//! entry opens a type, the user's own first; choosing one writes the
//! user's. Every program that opens files by type (`xdg-open`, GTK, Qt)
//! reads the same files.
//!
//! Reads and writes files: for a worker thread.

use std::path::{Path, PathBuf};

use crate::apps::{self, Entry};

/// What the desktop calls itself in `$desktop-mimeapps.list`.
const DESKTOP: &str = "hocuspanda";
const DEFAULTS: &str = "Default Applications";

/// The kinds of files the settings offer a default for, by name, with the
/// types each covers; the first is the one asked about.
pub const KINDS: [(&str, &[&str]); 8] = [
    (
        "web",
        &[
            "x-scheme-handler/https",
            "x-scheme-handler/http",
            "text/html",
        ],
    ),
    ("mail", &["x-scheme-handler/mailto"]),
    ("files", &["inode/directory"]),
    ("text", &["text/plain"]),
    (
        "images",
        &["image/png", "image/jpeg", "image/gif", "image/webp"],
    ),
    ("video", &["video/mp4", "video/webm", "video/x-matroska"]),
    ("music", &["audio/mpeg", "audio/flac", "audio/ogg"]),
    ("pdf", &["application/pdf"]),
];

/// Where the files are: the configuration directories, the user's first
/// (written), and the data directories, as [`apps::config_dirs`] and
/// [`apps::data_dirs`] give them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dirs {
    pub config: Vec<PathBuf>,
    pub data: Vec<PathBuf>,
}

impl Dirs {
    /// The machine's, from the environment.
    pub fn machine() -> Self {
        Self {
            config: apps::config_dirs(),
            data: apps::data_dirs(),
        }
    }

    /// The `mimeapps.list` files in the order they are read.
    fn lists(&self) -> Vec<PathBuf> {
        let names = [format!("{DESKTOP}-mimeapps.list"), "mimeapps.list".into()];
        let config = self.config.iter().cloned();
        let data = self.data.iter().map(|d| d.join("applications"));
        config
            .chain(data)
            .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
            .collect()
    }
}

/// A kind of file, the application that opens it and those that could.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultApp {
    /// As [`KINDS`] names it.
    pub kind: &'static str,
    /// The entry's id, when one is set and installed.
    pub current: Option<String>,
    /// The installed applications that open it: id and name, by name.
    pub choices: Vec<(String, String)>,
}

/// Each kind of [`KINDS`] with its default and choices; `entries` are the
/// installed applications ([`apps::scan`]).
pub fn defaults(dirs: &Dirs, entries: &[Entry]) -> Vec<DefaultApp> {
    let lists: Vec<String> = dirs
        .lists()
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect();
    KINDS
        .iter()
        .map(|(kind, types)| {
            let asked = types[0];
            let choices: Vec<(String, String)> = entries
                .iter()
                .filter(|e| e.mime_types.iter().any(|t| t == asked))
                .map(|e| (e.id.clone(), e.name.clone()))
                .collect();
            let installed = |id: &str| entries.iter().any(|e| e.id == id);
            // The first file that names an installed one for the type wins.
            let current = lists.iter().find_map(|text| {
                listed(text, DEFAULTS, asked)
                    .into_iter()
                    .find(|id| installed(id))
            });
            DefaultApp {
                kind,
                current,
                choices,
            }
        })
        .collect()
}

/// The ids `text` lists for `mime` in `[group]`.
fn listed(text: &str, group: &str, mime: &str) -> Vec<String> {
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == format!("[{group}]");
            continue;
        }
        if !inside {
            continue;
        }
        if let Some((key, value)) = line.split_once('=')
            && key.trim() == mime
        {
            return value
                .split(';')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
                .collect();
        }
    }
    Vec::new()
}

/// `id` opens every type of `kind` from now on: the user's
/// `mimeapps.list` says so, and the user's `hocuspanda-mimeapps.list` too
/// when it has a word on them, since it is read first.
pub fn set_default(dirs: &Dirs, kind: &str, id: &str) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    let (_, types) = KINDS
        .iter()
        .find(|(k, _)| *k == kind)
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, format!("no kind {kind}")))?;
    let user = dirs
        .config
        .first()
        .ok_or_else(|| Error::new(ErrorKind::NotFound, "no configuration directory"))?;
    let value = format!("{id};");
    let keys: Vec<(&str, &str)> = types.iter().map(|t| (*t, value.as_str())).collect();
    let ours = user.join(format!("{DESKTOP}-mimeapps.list"));
    if let Ok(text) = std::fs::read_to_string(&ours)
        && types.iter().any(|t| !listed(&text, DEFAULTS, t).is_empty())
    {
        write(&ours, &apps::set_keys(&text, DEFAULTS, &keys))?;
    }
    let shared = user.join("mimeapps.list");
    let text = std::fs::read_to_string(&shared).unwrap_or_default();
    write(&shared, &apps::set_keys(&text, DEFAULTS, &keys))
}

fn write(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    apps::write_atomically(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, name: &str, types: &[&str]) -> Entry {
        Entry {
            id: id.into(),
            name: name.into(),
            generic: None,
            keywords: Vec::new(),
            exec: vec![name.to_lowercase()],
            terminal: false,
            mime_types: types.iter().map(|t| (*t).to_owned()).collect(),
        }
    }

    #[test]
    fn the_user_s_list_wins_and_is_the_one_written() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let (user, system, data) = (
            dir.path().join("user"),
            dir.path().join("system"),
            dir.path().join("data"),
        );
        std::fs::create_dir_all(&system)?;
        std::fs::create_dir_all(data.join("applications"))?;
        std::fs::write(
            data.join("applications/mimeapps.list"),
            "[Default Applications]\nx-scheme-handler/https=web.desktop;\napplication/pdf=gone.desktop;reader.desktop;\n",
        )?;
        std::fs::write(
            system.join("mimeapps.list"),
            "[Default Applications]\nx-scheme-handler/https=firefox.desktop\n",
        )?;
        let dirs = Dirs {
            config: vec![user.clone(), system],
            data: vec![data],
        };
        let entries = [
            entry("firefox.desktop", "Firefox", &["x-scheme-handler/https"]),
            entry("web.desktop", "Web", &["x-scheme-handler/https"]),
            entry("reader.desktop", "Reader", &["application/pdf"]),
        ];
        let current = |kind: &str| {
            defaults(&dirs, &entries)
                .into_iter()
                .find(|d| d.kind == kind)
                .and_then(|d| d.current)
        };
        assert_eq!(current("web").as_deref(), Some("firefox.desktop"));
        // One not installed is passed over.
        assert_eq!(current("pdf").as_deref(), Some("reader.desktop"));
        assert_eq!(current("mail"), None);
        let web = defaults(&dirs, &entries).remove(0);
        assert_eq!(web.choices.len(), 2);
        set_default(&dirs, "web", "web.desktop")?;
        assert_eq!(current("web").as_deref(), Some("web.desktop"));
        let written = std::fs::read_to_string(user.join("mimeapps.list"))?;
        assert!(
            written.contains("x-scheme-handler/http=web.desktop;\n"),
            "{written}"
        );
        assert!(written.contains("text/html=web.desktop;\n"), "{written}");
        assert!(set_default(&dirs, "nothing", "web.desktop").is_err());
        Ok(())
    }
}
