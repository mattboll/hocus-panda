//! The default applications (freedesktop's MIME Applications Associations
//! Specification): which application opens a type, as `xdg-open`, GLib,
//! Qt and browsers read it, and the user's `mimeapps.list` edited in place.
//!
//! Read: the `[Default Applications]` of `$XDG_CONFIG_HOME/mimeapps.list`,
//! then of each `$XDG_CONFIG_DIRS`', then of each data directory's
//! `applications/mimeapps.list`, the first application installed winning;
//! else their `[Added Associations]`; else each data directory's
//! `applications/mimeinfo.cache`. Desktop-specific lists
//! (`$desktop-mimeapps.list`) are not read: the Settings write the plain
//! one, and it is what they show.
//!
//! Written: only the lines of the types changed, in the `[Default
//! Applications]` group (added at its end, or the group at the file's end),
//! everything else as it was: other groups, comments, other types.

use std::path::{Path, PathBuf};

/// The default applications' group.
pub const DEFAULTS: &str = "Default Applications";
const ADDED: &str = "Added Associations";

/// Where the lists are, the first winning.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lists {
    /// The user's: the one written.
    pub user: PathBuf,
    /// The other `mimeapps.list` files, in order.
    pub others: Vec<PathBuf>,
    /// The `mimeinfo.cache` files, in order.
    pub caches: Vec<PathBuf>,
}

impl Lists {
    /// From `XDG_CONFIG_HOME` (or `~/.config`), `XDG_CONFIG_DIRS` (or
    /// `/etc/xdg`) and the data directories (the user's first).
    pub fn of(data_dirs: &[PathBuf]) -> Self {
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
        let config = var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_default();
        let config_dirs = var("XDG_CONFIG_DIRS").unwrap_or_else(|| "/etc/xdg".into());
        let mut others: Vec<PathBuf> = std::env::split_paths(&config_dirs)
            .filter(|d| d.is_absolute())
            .map(|d| d.join("mimeapps.list"))
            .collect();
        others.extend(
            data_dirs
                .iter()
                .map(|d| d.join("applications/mimeapps.list")),
        );
        Self {
            user: config.join("mimeapps.list"),
            others,
            caches: data_dirs
                .iter()
                .map(|d| d.join("applications/mimeinfo.cache"))
                .collect(),
        }
    }
}

/// A list's groups: each its name and its `(key, value)`s, as written.
fn groups(text: &str) -> Vec<(&str, Vec<(&str, &str)>)> {
    let mut out: Vec<(&str, Vec<(&str, &str)>)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            out.push((name, Vec::new()));
            continue;
        }
        if let (Some((_, pairs)), Some((key, value))) = (out.last_mut(), line.split_once('=')) {
            pairs.push((key.trim(), value.trim()));
        }
    }
    out
}

/// The desktop ids a value names, `.desktop` taken off: `a.desktop;b;`.
pub fn ids(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.strip_suffix(".desktop").unwrap_or(v).to_owned())
        .collect()
}

/// The ids `group` of `text` gives `mime`.
fn in_group(text: &str, group: &str, mime: &str) -> Vec<String> {
    groups(text)
        .into_iter()
        .filter(|(name, _)| *name == group)
        .flat_map(|(_, pairs)| pairs)
        .filter(|(key, _)| *key == mime)
        .flat_map(|(_, value)| ids(value))
        .collect()
}

/// The `[Default Applications]` lines of `text`: each type and its value
/// as written.
pub fn defaults(text: &str) -> Vec<(String, String)> {
    groups(text)
        .into_iter()
        .filter(|(name, _)| *name == DEFAULTS)
        .flat_map(|(_, pairs)| pairs)
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

/// The application that opens `mime`, as the lists of `lists` say it, of
/// those `installed` says are (by id, without `.desktop`).
pub fn default_for(lists: &Lists, mime: &str, installed: &dyn Fn(&str) -> bool) -> Option<String> {
    let read = |file: &Path| std::fs::read_to_string(file).unwrap_or_default();
    let texts: Vec<String> = std::iter::once(&lists.user)
        .chain(&lists.others)
        .map(|f| read(f))
        .collect();
    for group in [DEFAULTS, ADDED] {
        for text in &texts {
            if let Some(id) = in_group(text, group, mime)
                .into_iter()
                .find(|id| installed(id))
            {
                return Some(id);
            }
        }
    }
    lists
        .caches
        .iter()
        .flat_map(|f| in_group(&read(f), "MIME Cache", mime))
        .find(|id| installed(id))
}

/// `text` (a `mimeapps.list`, empty when there is none) with each of
/// `edits`' types set to its value in `[Default Applications]` (`None`:
/// its line taken out), all else as it was.
pub fn edited(text: &str, edits: &[(String, Option<String>)]) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let key_of = |line: &str| -> Option<String> {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with('[') {
            return None;
        }
        line.split_once('=').map(|(k, _)| k.trim().to_owned())
    };
    // Where the group starts and ends (the line after its last pair).
    let span = |lines: &[String]| -> Option<(usize, usize)> {
        let start = lines
            .iter()
            .position(|l| l.trim() == format!("[{DEFAULTS}]"))?;
        let mut end = start + 1;
        for (i, line) in lines.iter().enumerate().skip(start + 1) {
            let t = line.trim();
            if t.starts_with('[') {
                break;
            }
            if !t.is_empty() {
                end = i + 1;
            }
        }
        Some((start, end))
    };
    for (mime, value) in edits {
        let at = span(&lines).and_then(|(start, end)| {
            (start + 1..end).find(|&i| key_of(&lines[i]).as_deref() == Some(mime.as_str()))
        });
        match (at, value) {
            (Some(i), Some(value)) => lines[i] = format!("{mime}={value}"),
            (Some(i), None) => {
                lines.remove(i);
            }
            (None, None) => {}
            (None, Some(value)) => {
                let line = format!("{mime}={value}");
                match span(&lines) {
                    Some((_, end)) => lines.insert(end, line),
                    None => {
                        if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                            lines.push(String::new());
                        }
                        lines.push(format!("[{DEFAULTS}]"));
                        lines.push(line);
                    }
                }
            }
        }
    }
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// The value a type's line holds for application `id`: `id.desktop;`.
pub fn value_of(id: &str) -> String {
    format!("{id}.desktop;")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "# By hand.\n[Added Associations]\ntext/html=chromium.desktop;\n\n\
                        [Default Applications]\ntext/html=librewolf.desktop;\n\
                        application/pdf=evince.desktop\n\n[Removed Associations]\n";

    #[test]
    fn a_type_set_in_place_added_at_the_groups_end_or_taken_out() {
        let out = edited(
            FILE,
            &[
                ("text/html".into(), Some(value_of("firefox"))),
                ("x-scheme-handler/http".into(), Some(value_of("firefox"))),
                ("application/pdf".into(), None),
            ],
        );
        assert_eq!(
            out,
            "# By hand.\n[Added Associations]\ntext/html=chromium.desktop;\n\n\
             [Default Applications]\ntext/html=firefox.desktop;\n\
             x-scheme-handler/http=firefox.desktop;\n\n[Removed Associations]\n"
        );
        // No group: one at the end; no file: the group alone.
        assert_eq!(
            edited(
                "[Added Associations]\n",
                &[("a/b".into(), Some("x.desktop;".into()))]
            ),
            "[Added Associations]\n\n[Default Applications]\na/b=x.desktop;\n"
        );
        assert_eq!(
            edited("", &[("a/b".into(), Some("x.desktop;".into()))]),
            "[Default Applications]\na/b=x.desktop;\n"
        );
        assert_eq!(edited("", &[("a/b".into(), None)]), "");
        assert_eq!(
            defaults(FILE),
            [
                ("text/html".to_owned(), "librewolf.desktop;".to_owned()),
                ("application/pdf".to_owned(), "evince.desktop".to_owned())
            ]
        );
    }

    #[test]
    fn the_users_default_then_the_others_then_the_cache() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let user = dir.path().join("user.list");
        let system = dir.path().join("system.list");
        let cache = dir.path().join("mimeinfo.cache");
        std::fs::write(&user, FILE)?;
        std::fs::write(
            &system,
            "[Default Applications]\nvideo/mp4=gone.desktop;vlc.desktop;\n",
        )?;
        std::fs::write(&cache, "[MIME Cache]\nimage/png=imv.desktop;eog.desktop;\n")?;
        let lists = Lists {
            user,
            others: vec![system],
            caches: vec![cache],
        };
        let installed = |id: &str| id != "gone" && id != "imv";
        assert_eq!(
            default_for(&lists, "text/html", &installed).as_deref(),
            Some("librewolf")
        );
        assert_eq!(
            default_for(&lists, "video/mp4", &installed).as_deref(),
            Some("vlc")
        );
        assert_eq!(
            default_for(&lists, "image/png", &installed).as_deref(),
            Some("eog")
        );
        assert_eq!(
            default_for(&lists, "x-scheme-handler/mailto", &installed),
            None
        );
        Ok(())
    }
}
