//! The configuration file, `$XDG_CONFIG_HOME/hocuspanda/config.toml`, as
//! the programs that write into it share it: the bar's "Choisir…" (Day.1)
//! and the Settings (6.1).
//!
//! It is edited in place with `toml_edit`, so that its other sections, its
//! comments, its blank lines and its order stay as the user wrote them,
//! and keys nobody knows are kept. It is written whole or not at all
//! (beside it, then renamed over it), through a symbolic link to where it
//! really is, with the mode it had. Blocking: from a thread of its own,
//! never a program's loop.

use std::{
    io::Write as _,
    path::{Path, PathBuf},
};

pub mod defaults;

pub use toml_edit;
use toml_edit::DocumentMut;

/// The variable naming the file, which the compositor sets for the
/// programs it starts (`hocus_link::CONFIG_ENV`).
pub const ENV: &str = "HOCUS_CONFIG";

/// The file the compositor names (`None` when it names an empty one: no
/// file); a program started without it: [`default_file`].
pub fn file() -> Option<PathBuf> {
    match std::env::var_os(ENV) {
        Some(file) if file.is_empty() => None,
        Some(file) => Some(PathBuf::from(file)),
        None => default_file(),
    }
}

/// `$XDG_CONFIG_HOME/hocuspanda/config.toml`, or under `~/.config`.
pub fn default_file() -> Option<PathBuf> {
    xdg("XDG_CONFIG_HOME", ".config").map(|dir| dir.join("hocuspanda/config.toml"))
}

/// `$XDG_STATE_HOME/hocuspanda`, or under `~/.local/state`: what the
/// desktop keeps between sessions.
pub fn state_dir() -> Option<PathBuf> {
    xdg("XDG_STATE_HOME", ".local/state").map(|dir| dir.join("hocuspanda"))
}

/// An XDG base directory: `var`, else `$HOME/under`; only an absolute one.
fn xdg(var: &str, under: &str) -> Option<PathBuf> {
    let get = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    get(var)
        .map(PathBuf::from)
        .or_else(|| get("HOME").map(|h| PathBuf::from(h).join(under)))
        .filter(|dir| dir.is_absolute())
}

/// What the file holds now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read {
    /// Where it really is, through its links.
    pub real: PathBuf,
    /// Its text; empty when there is no file yet.
    pub text: String,
    /// Its mode (`None`: no file yet).
    pub mode: Option<u32>,
}

/// Why the file was not read, changed or written.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The file is there and could not be read, or could not be written.
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Its text is not TOML: it is left as it is.
    #[error("the configuration does not read: {0}")]
    Syntax(#[from] toml_edit::TomlError),
    /// The change could not be made in it, or the text to write was
    /// refused: why, as the change or the check said it.
    #[error("{0}")]
    Refused(String),
}

/// `path`'s text, read through its links; no file yet is an empty text.
pub fn read(path: &Path) -> Result<Read, Error> {
    let at = |source: std::io::Error| Error::Io {
        path: path.to_path_buf(),
        source,
    };
    let real = real_path(path).map_err(at)?;
    match std::fs::read_to_string(&real) {
        Ok(text) => {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&real).map_err(at)?.permissions().mode();
            Ok(Read {
                real,
                text,
                mode: Some(mode),
            })
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Read {
            real,
            text: String::new(),
            mode: None,
        }),
        Err(err) => Err(at(err)),
    }
}

/// Where `path` really is: the file its links name, or `path` when there
/// is no file yet.
pub fn real_path(path: &Path) -> std::io::Result<PathBuf> {
    match std::fs::canonicalize(path) {
        Ok(real) => Ok(real),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(err) => Err(err),
    }
}

/// `text` parsed for editing in place, or why it does not read.
pub fn parse(text: &str) -> Result<DocumentMut, toml_edit::TomlError> {
    text.parse()
}

/// The file at `path` changed by `change` and written back, whole or not
/// at all; `check` sees the text to write first and may refuse it. A file
/// that does not read is left alone. No file yet: one made with the change
/// alone. The text written.
pub fn edit(
    path: &Path,
    change: impl FnOnce(&mut DocumentMut) -> Result<(), String>,
    check: impl FnOnce(&str) -> Result<(), String>,
) -> Result<String, Error> {
    let read = read(path)?;
    let mut doc = parse(&read.text)?;
    change(&mut doc).map_err(Error::Refused)?;
    let written = doc.to_string();
    check(&written).map_err(Error::Refused)?;
    replace(&read.real, written.as_bytes(), read.mode).map_err(|source| Error::Io {
        path: read.real.clone(),
        source,
    })?;
    Ok(written)
}

/// `bytes` in `file`: written beside it, then renamed over it, with `mode`
/// (else 0644 under the umask).
pub fn replace(file: &Path, bytes: &[u8], mode: Option<u32>) -> std::io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
    let dir = file
        .parent()
        .ok_or_else(|| std::io::Error::other("no folder"))?;
    std::fs::create_dir_all(dir)?;
    let name = file
        .file_name()
        .ok_or_else(|| std::io::Error::other("no file name"))?;
    let mut beside = std::ffi::OsString::from(".");
    beside.push(name);
    beside.push(format!(".{}.new", std::process::id()));
    let beside: PathBuf = dir.join(beside);
    let written = (|| {
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(mode.unwrap_or(0o644) & 0o7777)
            .open(&beside)?;
        if let Some(mode) = mode {
            out.set_permissions(std::fs::Permissions::from_mode(mode & 0o7777))?;
        }
        out.write_all(bytes)?;
        out.sync_all()?;
        std::fs::rename(&beside, file)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&beside);
    }
    written
}

/// The line (from 1) of a parse error, as the user's editor counts.
pub fn error_line(text: &str, err: &toml_edit::TomlError) -> Option<usize> {
    let start = err.span()?.start.min(text.len());
    Some(text[..start].matches('\n').count() + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_edit_keeps_the_rest_and_goes_through_links_with_the_mode() -> anyhow::Result<()> {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir()?;
        std::fs::create_dir_all(dir.path().join("dotfiles"))?;
        let real = dir.path().join("dotfiles/config.toml");
        std::fs::write(&real, "# Mine.\n[keyboard]\nlayouts = [\"fr\"]  # yes\n")?;
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600))?;
        let link = dir.path().join("config.toml");
        std::os::unix::fs::symlink(&real, &link)?;
        let written = edit(
            &link,
            |doc| {
                let mut idle = toml_edit::Table::new();
                idle.insert("delay", toml_edit::value(300));
                doc.insert("idle", toml_edit::Item::Table(idle));
                Ok(())
            },
            |_| Ok(()),
        )
        .map_err(anyhow::Error::msg)?;
        assert!(
            written.starts_with("# Mine.\n[keyboard]\nlayouts = [\"fr\"]  # yes\n"),
            "{written:?}"
        );
        assert!(written.contains("[idle]\ndelay = 300\n"), "{written}");
        assert!(std::fs::symlink_metadata(&link)?.file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&real)?, written);
        assert_eq!(
            std::fs::metadata(&real)?.permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(std::fs::read_dir(dir.path().join("dotfiles"))?.count(), 1);
        Ok(())
    }

    #[test]
    fn a_file_that_does_not_read_or_a_refused_text_is_left_alone() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("config.toml");
        std::fs::write(&file, "[keyboard\n")?;
        assert!(matches!(
            edit(&file, |_| Ok(()), |_| Ok(())),
            Err(Error::Syntax(_))
        ));
        std::fs::write(&file, "a = 1\n")?;
        assert!(matches!(
            edit(&file, |_| Ok(()), |_| Err("no".to_owned())),
            Err(Error::Refused(why)) if why == "no"
        ));
        assert_eq!(std::fs::read_to_string(&file)?, "a = 1\n");
        // No file yet: made.
        let new = dir.path().join("new/config.toml");
        edit(
            &new,
            |doc| {
                doc["a"] = toml_edit::value(true);
                Ok(())
            },
            |_| Ok(()),
        )
        .map_err(anyhow::Error::msg)?;
        assert_eq!(std::fs::read_to_string(&new)?, "a = true\n");
        Ok(())
    }

    #[test]
    fn a_parse_error_says_its_line() {
        let text = "a = 1\n\n[b\nc = 2\n";
        let err = parse(text).err();
        assert_eq!(err.and_then(|e| error_line(text, &e)), Some(3));
    }
}
