//! Desktop entries (freedesktop's Desktop Entry Specification), as the
//! launcher needs them: the applications in the XDG data directories, by
//! their ids, the first directory's winning (`XDG_DATA_HOME`, then
//! `XDG_DATA_DIRS` in order); their name, generic name and keywords in the
//! locale's language; their icon; and their command, its field codes taken
//! out. `Hidden`, `NoDisplay`, `OnlyShowIn`, `NotShowIn` and `TryExec` are
//! honoured; an entry hidden so hides those of the same id further down
//! the directories too.
//!
//! Our own reading of the specification, small: only the `[Desktop Entry]`
//! group, only the keys the launcher uses.

use std::{
    collections::HashSet,
    ffi::OsStr,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

/// A desktop entry the launcher shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its desktop file id: its path under `applications/`, `/` made `-`,
    /// without `.desktop` (`org.mozilla.firefox`).
    pub id: String,
    pub name: String,
    pub generic: Option<String>,
    pub keywords: Vec<String>,
    /// An icon's name, or a file's absolute path.
    pub icon: Option<String>,
    /// Its command, one argument each, its field codes taken out.
    pub argv: Vec<String>,
    pub terminal: bool,
    /// Its working directory, if it names one.
    pub dir: Option<String>,
}

/// What a desktop file is to the launcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    Shown(Entry),
    /// Not shown, and hides the files of the same id further down.
    Hidden,
}

/// The locale the names are read in: `fr_FR.UTF-8@euro` is `fr`, `FR`,
/// `euro`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Locale {
    lang: String,
    country: Option<String>,
    modifier: Option<String>,
}

impl Locale {
    /// The messages' locale: `LC_ALL`, `LC_MESSAGES`, then `LANG`.
    pub fn from_env() -> Self {
        let name = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .filter_map(|v| std::env::var(v).ok())
            .find(|v| !v.is_empty())
            .unwrap_or_default();
        Self::of(&name)
    }

    pub fn of(name: &str) -> Self {
        let (rest, modifier) = match name.split_once('@') {
            Some((rest, modifier)) => (rest, Some(modifier.to_owned())),
            None => (name, None),
        };
        let rest = rest.split('.').next().unwrap_or_default();
        let (lang, country) = match rest.split_once('_') {
            Some((lang, country)) => (lang, Some(country.to_owned())),
            None => (rest, None),
        };
        if lang.is_empty() || lang == "C" || lang == "POSIX" {
            return Self::default();
        }
        Self {
            lang: lang.to_owned(),
            country,
            modifier,
        }
    }

    /// The locales a localised key is looked for in, the most precise
    /// first, as the specification orders them; the plain key last.
    fn keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        if self.lang.is_empty() {
            return keys;
        }
        let lang = &self.lang;
        if let (Some(country), Some(modifier)) = (&self.country, &self.modifier) {
            keys.push(format!("{lang}_{country}@{modifier}"));
        }
        if let Some(country) = &self.country {
            keys.push(format!("{lang}_{country}"));
        }
        if let Some(modifier) = &self.modifier {
            keys.push(format!("{lang}@{modifier}"));
        }
        keys.push(lang.clone());
        keys
    }
}

/// The `[Desktop Entry]` group's keys and values, as written.
pub(crate) struct Group<'a> {
    pub(crate) pairs: Vec<(&'a str, Option<&'a str>, &'a str)>,
}

impl<'a> Group<'a> {
    pub(crate) fn of(text: &'a str) -> Self {
        let mut pairs = Vec::new();
        let mut inside = false;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                inside = line == "[Desktop Entry]";
                continue;
            }
            if !inside {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let (key, locale) = match key.split_once('[') {
                Some((key, locale)) => (key, locale.strip_suffix(']')),
                None => (key, None),
            };
            pairs.push((key, locale, value.trim()));
        }
        Self { pairs }
    }

    pub(crate) fn raw(&self, key: &str) -> Option<&'a str> {
        self.pairs
            .iter()
            .find(|(k, locale, _)| *k == key && locale.is_none())
            .map(|(_, _, v)| *v)
    }

    /// `key` in the first of `locale`'s keys it has, else plain.
    pub(crate) fn localised(&self, key: &str, locale: &Locale) -> Option<&'a str> {
        locale
            .keys()
            .iter()
            .find_map(|l| {
                self.pairs
                    .iter()
                    .find(|(k, at, _)| *k == key && *at == Some(l.as_str()))
                    .map(|(_, _, v)| *v)
            })
            .or_else(|| self.raw(key))
    }

    pub(crate) fn flag(&self, key: &str) -> bool {
        self.raw(key) == Some("true")
    }
}

/// A string value's escapes: `\s`, `\n`, `\t`, `\r` and `\\`.
pub(crate) fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// A list's values, split at the `;` not escaped (`\;`), each unescaped,
/// the empty ones left out.
pub(crate) fn list(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut item = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(';') => item.push(';'),
                Some(other) => {
                    item.push('\\');
                    item.push(other);
                }
                None => {}
            },
            ';' => items.push(std::mem::take(&mut item)),
            c => item.push(c),
        }
    }
    items.push(item);
    items
        .into_iter()
        .map(|i| unescape(&i).trim().to_owned())
        .filter(|i| !i.is_empty())
        .collect()
}

/// `Exec`'s arguments, its quoting undone: a quoted argument keeps its
/// spaces, and `\"`, `` \` ``, `\$` and `\\` in it are those characters.
/// `None` when a quote is left open.
fn arguments(exec: &str) -> Option<Vec<String>> {
    let mut args = Vec::new();
    let mut arg: Option<String> = None;
    let mut quoted = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' => quoted = false,
                '\\' => {
                    let out = arg.get_or_insert_with(String::new);
                    match chars.next() {
                        Some(e @ ('"' | '`' | '$' | '\\')) => out.push(e),
                        Some(other) => {
                            out.push('\\');
                            out.push(other);
                        }
                        None => return None,
                    }
                }
                c => arg.get_or_insert_with(String::new).push(c),
            }
        } else if c == ' ' || c == '\t' || c == '\n' {
            if let Some(done) = arg.take() {
                args.push(done);
            }
        } else if c == '"' {
            quoted = true;
            arg.get_or_insert_with(String::new);
        } else {
            arg.get_or_insert_with(String::new).push(c);
        }
    }
    if quoted {
        return None;
    }
    args.extend(arg);
    Some(args)
}

/// The command of `exec`, its field codes taken out: the files and URLs
/// (`%f`, `%F`, `%u`, `%U`) and the deprecated ones go; `%i` is `--icon`
/// and the icon, `%c` the name, `%k` the desktop file, `%%` a `%`.
pub(crate) fn command(
    exec: &str,
    name: &str,
    icon: Option<&str>,
    file: &Path,
) -> Option<Vec<String>> {
    let mut argv = Vec::new();
    for arg in arguments(&unescape(exec))? {
        match arg.as_str() {
            "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => continue,
            "%i" => {
                if let Some(icon) = icon {
                    argv.push("--icon".to_owned());
                    argv.push(icon.to_owned());
                }
                continue;
            }
            _ => {}
        }
        let mut out = String::with_capacity(arg.len());
        let mut chars = arg.chars();
        while let Some(c) = chars.next() {
            if c != '%' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('%') => out.push('%'),
                Some('c') => out.push_str(name),
                Some('k') => out.push_str(&file.to_string_lossy()),
                // Another code, or a stray `%` at the end: nothing.
                _ => {}
            }
        }
        argv.push(out);
    }
    argv.first().filter(|p| !p.is_empty())?;
    Some(argv)
}

/// Whether `program` runs: an executable file at its path, or in one of
/// `path`'s directories.
pub(crate) fn runs(program: &str, path: Option<&OsStr>) -> bool {
    let executable = |file: &Path| {
        std::fs::metadata(file).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if program.contains('/') {
        return executable(Path::new(program));
    }
    path.is_some_and(|path| std::env::split_paths(path).any(|dir| executable(&dir.join(program))))
}

/// What desktop file `file` (id `id`) is to the launcher, on `desktops`
/// (`XDG_CURRENT_DESKTOP`'s names), its names in `locale`, `TryExec` looked
/// for in `path`. `None`: it does not read as an entry at all.
pub fn read(
    id: &str,
    file: &Path,
    text: &str,
    locale: &Locale,
    desktops: &[String],
    path: Option<&OsStr>,
) -> Option<Read> {
    let group = Group::of(text);
    if group.pairs.is_empty() {
        return None;
    }
    if group.flag("Hidden") || group.flag("NoDisplay") {
        return Some(Read::Hidden);
    }
    if group.raw("Type") != Some("Application") {
        return Some(Read::Hidden);
    }
    if let Some(only) = group.raw("OnlyShowIn")
        && !list(only).iter().any(|d| desktops.contains(d))
    {
        return Some(Read::Hidden);
    }
    if let Some(not) = group.raw("NotShowIn")
        && list(not).iter().any(|d| desktops.contains(d))
    {
        return Some(Read::Hidden);
    }
    if let Some(try_exec) = group.raw("TryExec").map(unescape)
        && !try_exec.is_empty()
        && !runs(&try_exec, path)
    {
        return Some(Read::Hidden);
    }
    let Some(name) = group
        .localised("Name", locale)
        .map(unescape)
        .filter(|n| !n.trim().is_empty())
    else {
        return Some(Read::Hidden);
    };
    let name = name.trim().to_owned();
    let icon = group
        .localised("Icon", locale)
        .map(unescape)
        .map(|i| i.trim().to_owned())
        .filter(|i| !i.is_empty());
    let Some(argv) = group
        .raw("Exec")
        .and_then(|exec| command(exec, &name, icon.as_deref(), file))
    else {
        tracing::debug!(id, "a desktop entry without a command that reads");
        return Some(Read::Hidden);
    };
    let generic = group
        .localised("GenericName", locale)
        .map(unescape)
        .map(|g| g.trim().to_owned())
        .filter(|g| !g.is_empty());
    let keywords = group
        .localised("Keywords", locale)
        .map(list)
        .unwrap_or_default();
    let dir = group
        .raw("Path")
        .map(unescape)
        .filter(|d| !d.trim().is_empty());
    Some(Read::Shown(Entry {
        id: id.to_owned(),
        name,
        generic,
        keywords,
        icon,
        argv,
        terminal: group.flag("Terminal"),
        dir,
    }))
}

/// `XDG_DATA_HOME` (or `~/.local/share`), then `XDG_DATA_DIRS` (or
/// `/usr/local/share:/usr/share`): where applications and icons are, the
/// first winning.
pub fn data_dirs() -> Vec<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let home = var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    let system = var("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let mut seen = HashSet::new();
    home.into_iter()
        .chain(std::env::split_paths(&system))
        .filter(|d| d.is_absolute())
        .filter(|d| seen.insert(d.clone()))
        .collect()
}

/// The desktops the session is, from `XDG_CURRENT_DESKTOP`.
pub fn current_desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|d| !d.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The desktop files under `applications` (a data directory's), with
/// their ids, in a steady order.
pub fn files(applications: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    let mut stack = vec![(applications.to_owned(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut children: Vec<_> = read.flatten().map(|e| e.path()).collect();
        children.sort();
        for child in children {
            let Some(name) = child.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if child.is_dir() {
                stack.push((child.clone(), format!("{prefix}{name}-")));
            } else if let Some(stem) = name.strip_suffix(".desktop") {
                found.push((format!("{prefix}{stem}"), child.clone()));
            }
        }
    }
    found
}

/// The applications of `dirs` (data directories, the first winning), as
/// the launcher shows them, in `locale`, on `desktops`.
pub fn scan(dirs: &[PathBuf], locale: &Locale, desktops: &[String]) -> Vec<Entry> {
    let path = std::env::var_os("PATH");
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for dir in dirs {
        for (id, file) in files(&dir.join("applications")) {
            if seen.contains(&id) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            match read(&id, &file, &text, locale, desktops, path.as_deref()) {
                Some(Read::Shown(entry)) => {
                    seen.insert(id);
                    entries.push(entry);
                }
                Some(Read::Hidden) => {
                    seen.insert(id);
                }
                None => {}
            }
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(text: &str) -> Option<Entry> {
        let locale = Locale::of("fr_FR.UTF-8");
        let desktops = vec!["Hocuspanda".to_owned()];
        match read(
            "app",
            Path::new("/apps/app.desktop"),
            text,
            &locale,
            &desktops,
            None,
        ) {
            Some(Read::Shown(entry)) => Some(entry),
            _ => None,
        }
    }

    #[test]
    fn the_locales_name_then_the_plain_one() {
        let text = "[Desktop Entry]\nType=Application\nName=Files\nName[fr]=Fichiers\n\
                    Name[fr_CA]=Dossiers\nGenericName=File Manager\n\
                    GenericName[fr]=Gestionnaire de fichiers\nKeywords=folder;\n\
                    Keywords[fr]=dossier;explorateur\\;x;\nExec=nautilus --new-window %U\n\
                    Icon=org.gnome.Nautilus\n\n[Desktop Action new]\nName=New\nExec=other\n";
        let entry = shown(text).expect("an entry");
        assert_eq!(entry.name, "Fichiers");
        assert_eq!(entry.generic.as_deref(), Some("Gestionnaire de fichiers"));
        assert_eq!(entry.keywords, ["dossier", "explorateur;x"]);
        assert_eq!(entry.argv, ["nautilus", "--new-window"]);
        assert_eq!(entry.icon.as_deref(), Some("org.gnome.Nautilus"));
        let locale = Locale::of("fr_CA.UTF-8@x");
        assert_eq!(locale.keys(), ["fr_CA@x", "fr_CA", "fr@x", "fr"]);
        assert_eq!(Locale::of("C.UTF-8").keys(), Vec::<String>::new());
    }

    #[test]
    fn the_field_codes_go_and_the_quotes_hold() {
        let file = Path::new("/apps/app.desktop");
        let argv = |exec: &str| command(exec, "Mon appli", Some("app-icon"), file);
        assert_eq!(
            argv("firefox %u").as_deref(),
            Some(&["firefox".to_owned()][..])
        );
        assert_eq!(
            argv(r#"sh -c "echo \\"hi there\\" \\$HOME" %F"#),
            Some(vec![
                "sh".into(),
                "-c".into(),
                r#"echo "hi there" $HOME"#.into()
            ])
        );
        assert_eq!(
            argv("app %i --title=%c --from %k 100%%"),
            Some(vec![
                "app".into(),
                "--icon".into(),
                "app-icon".into(),
                "--title=Mon appli".into(),
                "--from".into(),
                "/apps/app.desktop".into(),
                "100%".into()
            ])
        );
        assert_eq!(argv(r#"app "unclosed"#), None);
        assert_eq!(argv("%U"), None);
        // `\s` is a space within an argument, after the quotes are read.
        assert_eq!(
            argv(r#""/opt/my\sapp/run" --x"#),
            Some(vec!["/opt/my app/run".into(), "--x".into()])
        );
    }

    /// The Settings' entry the package installs (6.1): "Réglages" in the
    /// launcher, found by its words, started as `hocus-settings`.
    #[test]
    fn the_settings_entry_shows_as_reglages() {
        let entry = shown(include_str!("../../../packaging/hocus-settings.desktop"));
        let entry = entry.expect("the Settings' entry is shown");
        assert_eq!(entry.name, "Réglages");
        assert_eq!(entry.argv, ["hocus-settings"]);
        assert!(!entry.terminal);
        assert!(
            entry.keywords.iter().any(|k| k == "veille"),
            "{:?}",
            entry.keywords
        );
    }

    #[test]
    fn hidden_nodisplay_and_the_desktops_keep_an_entry_out() {
        let base = "[Desktop Entry]\nType=Application\nName=A\nExec=a\n";
        assert!(shown(base).is_some());
        for extra in [
            "NoDisplay=true\n",
            "Hidden=true\n",
            "OnlyShowIn=GNOME;KDE;\n",
            "NotShowIn=Hocuspanda;\n",
            "TryExec=/nonexistent/a\n",
        ] {
            assert!(shown(&format!("{base}{extra}")).is_none(), "{extra}");
        }
        assert!(shown(&format!("{base}OnlyShowIn=GNOME;Hocuspanda;\n")).is_some());
        assert!(shown("[Desktop Entry]\nType=Link\nName=A\nURL=x\n").is_none());
        assert!(shown("[Desktop Entry]\nType=Application\nName=A\n").is_none());
    }

    #[test]
    fn the_first_directory_wins_and_a_hidden_one_hides() -> std::io::Result<()> {
        let home = tempfile::tempdir()?;
        let system = tempfile::tempdir()?;
        let write = |dir: &Path, name: &str, text: &str| -> std::io::Result<()> {
            let file = dir.join("applications").join(name);
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(file, text)
        };
        let app = |name: &str| format!("[Desktop Entry]\nType=Application\nName={name}\nExec=x\n");
        write(home.path(), "a.desktop", &app("Mine"))?;
        write(system.path(), "a.desktop", &app("System's"))?;
        write(
            home.path(),
            "b.desktop",
            "[Desktop Entry]\nType=Application\nName=B\nExec=b\nHidden=true\n",
        )?;
        write(system.path(), "b.desktop", &app("B"))?;
        write(system.path(), "kde/c.desktop", &app("C"))?;
        let dirs = vec![home.path().to_owned(), system.path().to_owned()];
        let entries = scan(&dirs, &Locale::default(), &[]);
        let names: Vec<(&str, &str)> = entries
            .iter()
            .map(|e| (e.id.as_str(), e.name.as_str()))
            .collect();
        assert_eq!(names, [("a", "Mine"), ("kde-c", "C")]);
        Ok(())
    }
}
