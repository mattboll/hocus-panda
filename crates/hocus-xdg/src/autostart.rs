//! The session's start-up applications (freedesktop's Desktop Application
//! Autostart Specification): the desktop entries of the user's
//! `$XDG_CONFIG_HOME/autostart` (or `~/.config/autostart`), then of each
//! `$XDG_CONFIG_DIRS`' `autostart` (or `/etc/xdg/autostart`), by their
//! file names, the first directory's file of a name winning.
//!
//! An entry starts when it is not `Hidden`, its `OnlyShowIn` names the
//! session's desktop (or it has none), its `NotShowIn` does not, and its
//! `TryExec` program is there. `NoDisplay` does not count here: it keeps
//! an entry out of the launcher's list, not out of the session. The keys
//! other desktops add (delays, phases, their own switches) are not read.
//!
//! The compositor starts the entries that start (6.15); the Settings list
//! them all, a hidden one as switched off, those of other desktops apart.

use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use crate::desktop::{Group, Locale, command, list, runs, unescape};

/// Whether an entry is for this session, as installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Here {
    /// It starts in this session, unless it is hidden.
    Starts,
    /// Its `OnlyShowIn` or `NotShowIn` leaves this session's desktop out.
    OtherDesktops,
    /// Its `TryExec` program is not there.
    NotInstalled,
}

/// An autostart entry, as the winning file of its name says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its file's name without `.desktop` (`org.keepassxc.KeePassXC`).
    pub id: String,
    /// The file read: the first directory's of its name.
    pub file: PathBuf,
    /// That file is in the first directory, the user's.
    pub user: bool,
    /// A later directory (the system's) has a file of its name too: the
    /// user's file, if there is one, is a copy of an installed entry.
    pub installed: bool,
    /// Its `Name` in the locale's language, else its id.
    pub name: String,
    pub comment: Option<String>,
    /// An icon's name, or a file's absolute path.
    pub icon: Option<String>,
    /// Its `Exec` as written, unescaped.
    pub exec: String,
    /// Its command, one argument each, its field codes taken out; empty
    /// when `Exec` does not read.
    pub argv: Vec<String>,
    pub terminal: bool,
    /// Its working directory (`Path`), if it names one.
    pub dir: Option<String>,
    /// `Hidden=true`: switched off.
    pub hidden: bool,
    pub here: Here,
}

impl Entry {
    /// Whether the session starts it.
    pub fn starts(&self) -> bool {
        !self.hidden && self.here == Here::Starts && !self.argv.is_empty()
    }
}

/// The autostart directories: the user's (`$XDG_CONFIG_HOME`, or
/// `~/.config`) first, then `$XDG_CONFIG_DIRS`' (or `/etc/xdg`), each
/// once, each's `autostart`.
pub fn dirs() -> Vec<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let home = var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".config")));
    let system = var("XDG_CONFIG_DIRS").unwrap_or_else(|| "/etc/xdg".into());
    let mut out: Vec<PathBuf> = Vec::new();
    for dir in home.into_iter().chain(std::env::split_paths(&system)) {
        let dir = dir.join("autostart");
        if dir.is_absolute() && !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

/// What autostart file `file` (id `id`) says, on `desktops`
/// (`XDG_CURRENT_DESKTOP`'s names), its names in `locale`, `TryExec` looked
/// for in `path`; `user` and `installed` as [`Entry`]'s. `None`: not an
/// application's entry.
pub fn read(
    id: &str,
    file: &Path,
    text: &str,
    locale: &Locale,
    desktops: &[String],
    path: Option<&OsStr>,
) -> Option<Entry> {
    let group = Group::of(text);
    if group.pairs.is_empty() || group.raw("Type") != Some("Application") {
        return None;
    }
    let mut here = Here::Starts;
    let only = group.raw("OnlyShowIn").map(list);
    let not = group.raw("NotShowIn").map(list).unwrap_or_default();
    if only.is_some_and(|only| !only.iter().any(|d| desktops.contains(d)))
        || not.iter().any(|d| desktops.contains(d))
    {
        here = Here::OtherDesktops;
    } else if let Some(try_exec) = group.raw("TryExec").map(unescape)
        && !try_exec.is_empty()
        && !runs(&try_exec, path)
    {
        here = Here::NotInstalled;
    }
    let name = group
        .localised("Name", locale)
        .map(unescape)
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| id.to_owned());
    let text_of = |key: &str| {
        group
            .localised(key, locale)
            .map(unescape)
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    let icon = text_of("Icon");
    let exec = group.raw("Exec").map(unescape).unwrap_or_default();
    let argv = group
        .raw("Exec")
        .and_then(|exec| command(exec, &name, icon.as_deref(), file))
        .unwrap_or_default();
    Some(Entry {
        id: id.to_owned(),
        file: file.to_owned(),
        user: false,
        installed: false,
        comment: text_of("Comment"),
        name,
        icon,
        exec,
        argv,
        terminal: group.flag("Terminal"),
        dir: group
            .raw("Path")
            .map(unescape)
            .filter(|d| !d.trim().is_empty()),
        hidden: group.flag("Hidden"),
        here,
    })
}

/// The desktop files of one autostart directory, by their ids, in a steady
/// order.
fn files(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter_map(|p| {
            let id = p
                .file_name()?
                .to_str()?
                .strip_suffix(".desktop")?
                .to_owned();
            Some((id, p))
        })
        .collect();
    found.sort();
    found
}

/// The autostart entries of `dirs` (autostart directories, the user's
/// first, the first winning), in `locale`, on `desktops`, sorted by name.
pub fn scan(dirs: &[PathBuf], locale: &Locale, desktops: &[String]) -> Vec<Entry> {
    let path = std::env::var_os("PATH");
    scan_in(dirs, locale, desktops, path.as_deref())
}

/// [`scan`], `TryExec` looked for in `path`.
pub fn scan_in(
    dirs: &[PathBuf],
    locale: &Locale,
    desktops: &[String],
    path: Option<&OsStr>,
) -> Vec<Entry> {
    // Each id's files, the first directory's first.
    let mut by_id: HashMap<String, Vec<(usize, PathBuf)>> = HashMap::new();
    for (at, dir) in dirs.iter().enumerate() {
        for (id, file) in files(dir) {
            by_id.entry(id).or_default().push((at, file));
        }
    }
    let mut entries: Vec<Entry> = by_id
        .into_iter()
        .filter_map(|(id, found)| {
            let (at, file) = found.first()?;
            let text = match std::fs::read_to_string(file) {
                Ok(text) => text,
                Err(err) => {
                    tracing::debug!(file = %file.display(), %err, "an autostart entry unread");
                    return None;
                }
            };
            let mut entry = read(&id, file, &text, locale, desktops, path)?;
            entry.user = *at == 0;
            entry.installed = found.iter().any(|(at, _)| *at > 0);
            Some(entry)
        })
        .collect();
    entries.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desktops() -> Vec<String> {
        vec!["Hocuspanda".to_owned()]
    }

    fn entry(text: &str) -> Option<Entry> {
        read(
            "app",
            Path::new("/autostart/app.desktop"),
            text,
            &Locale::of("fr_FR.UTF-8"),
            &desktops(),
            None,
        )
    }

    #[test]
    fn hidden_other_desktops_and_a_missing_tryexec_do_not_start() {
        let base = "[Desktop Entry]\nType=Application\nName=A\nExec=a --start %U\n";
        let a = entry(base).expect("an entry");
        assert!(a.starts());
        assert_eq!(a.argv, ["a", "--start"]);
        // Kept out of the launcher, not out of the session.
        assert!(entry(&format!("{base}NoDisplay=true\n")).is_some_and(|e| e.starts()));
        let hidden = entry(&format!("{base}Hidden=true\n")).expect("an entry");
        assert!(hidden.hidden && !hidden.starts());
        assert_eq!(hidden.here, Here::Starts);
        for (extra, here) in [
            ("OnlyShowIn=GNOME;KDE;\n", Here::OtherDesktops),
            ("NotShowIn=Hocuspanda;\n", Here::OtherDesktops),
            ("TryExec=/nonexistent/a\n", Here::NotInstalled),
        ] {
            let e = entry(&format!("{base}{extra}")).expect("an entry");
            assert_eq!(e.here, here, "{extra}");
            assert!(!e.starts(), "{extra}");
        }
        assert!(
            entry(&format!("{base}OnlyShowIn=GNOME;Hocuspanda;\n")).is_some_and(|e| e.starts())
        );
        assert!(entry(&format!("{base}TryExec=/bin/sh\n")).is_some_and(|e| e.starts()));
        assert!(entry("[Desktop Entry]\nType=Link\nName=A\nURL=x\n").is_none());
        // No command that reads: listed, never started.
        let broken = entry("[Desktop Entry]\nType=Application\nName=A\nExec=a \"open\n");
        assert!(broken.is_some_and(|e| !e.starts()));
    }

    #[test]
    fn the_names_the_comment_the_path_and_the_terminal() {
        let e = entry(
            "[Desktop Entry]\nType=Application\nName=Notes\nName[fr]=Carnet\n\
             Comment=Your notes\nComment[fr]=Tes notes\nIcon=notes\nExec=notes\n\
             Path=/srv/notes\nTerminal=true\n",
        )
        .expect("an entry");
        assert_eq!(e.name, "Carnet");
        assert_eq!(e.comment.as_deref(), Some("Tes notes"));
        assert_eq!(e.icon.as_deref(), Some("notes"));
        assert_eq!(e.dir.as_deref(), Some("/srv/notes"));
        assert!(e.terminal);
        // No name: its id.
        let e = entry("[Desktop Entry]\nType=Application\nExec=x\n").expect("an entry");
        assert_eq!(e.name, "app");
    }

    #[test]
    fn the_users_file_wins_and_says_where_it_comes_from() -> std::io::Result<()> {
        let user = tempfile::tempdir()?;
        let system = tempfile::tempdir()?;
        let app = |name: &str, extra: &str| {
            format!("[Desktop Entry]\nType=Application\nName={name}\nExec=x\n{extra}")
        };
        std::fs::write(user.path().join("b.desktop"), app("B", "Hidden=true\n"))?;
        std::fs::write(system.path().join("b.desktop"), app("B", ""))?;
        std::fs::write(user.path().join("mine.desktop"), app("Mine", ""))?;
        std::fs::write(system.path().join("a.desktop"), app("a", ""))?;
        std::fs::write(system.path().join("notes.txt"), "not an entry")?;
        let dirs = vec![
            user.path().to_owned(),
            system.path().to_owned(),
            PathBuf::from("/nonexistent/autostart"),
        ];
        let entries = scan_in(&dirs, &Locale::default(), &desktops(), None);
        let seen: Vec<(&str, bool, bool, bool)> = entries
            .iter()
            .map(|e| (e.id.as_str(), e.user, e.installed, e.starts()))
            .collect();
        assert_eq!(
            seen,
            [
                ("a", false, true, true),
                ("b", true, true, false),
                ("mine", true, false, true),
            ]
        );
        assert_eq!(entries[1].file, user.path().join("b.desktop"));
        Ok(())
    }
}
