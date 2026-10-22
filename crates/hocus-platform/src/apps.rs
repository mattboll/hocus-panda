//! Desktop entries: the applications the launcher offers, read from the XDG
//! data directories on a thread of their own (the Desktop Entry
//! Specification 1.5).

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// What the desktop's own name is, for `OnlyShowIn` and `NotShowIn`.
pub const DESKTOP: &str = "Hocuspanda";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// `firefox.desktop`, `org.gnome.Nautilus.desktop`.
    pub id: String,
    pub name: String,
    pub generic: Option<String>,
    pub keywords: Vec<String>,
    /// The program and its arguments, field codes removed.
    pub exec: Vec<String>,
    /// Runs in a terminal.
    pub terminal: bool,
}

/// Where entries are looked for, first wins: the user's data directory,
/// then the system's.
pub fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    let system = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    home.into_iter()
        .chain(
            system
                .split(':')
                .map(PathBuf::from)
                .filter(|p| p.is_absolute()),
        )
        .collect()
}

/// Every application entry under `dirs`, by name; `lang` (`fr_FR`) picks
/// translated names.
pub fn scan(dirs: &[PathBuf], lang: Option<&str>) -> Vec<Entry> {
    let mut by_id: BTreeMap<String, Option<Entry>> = BTreeMap::new();
    for dir in dirs {
        let apps = dir.join("applications");
        let mut files = Vec::new();
        collect(&apps, &mut files);
        for file in files {
            let Some(id) = file
                .strip_prefix(&apps)
                .ok()
                .map(|rel| rel.to_string_lossy().replace('/', "-"))
            else {
                continue;
            };
            // An earlier directory hides the same id, even to hide it.
            if by_id.contains_key(&id) {
                continue;
            }
            let entry = std::fs::read_to_string(&file)
                .ok()
                .and_then(|text| parse(&id, &text, lang));
            by_id.insert(id, entry);
        }
    }
    let mut entries: Vec<Entry> = by_id.into_values().flatten().collect();
    entries.sort_by_key(|e| e.name.to_lowercase());
    entries
}

/// Where autostart entries are: the user's configuration directory, then
/// the system's (`$XDG_CONFIG_HOME`, `$XDG_CONFIG_DIRS`).
pub fn config_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    let system = std::env::var("XDG_CONFIG_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/etc/xdg".into());
    home.into_iter()
        .chain(
            system
                .split(':')
                .map(PathBuf::from)
                .filter(|p| p.is_absolute()),
        )
        .collect()
}

/// The applications to start with the session, from `autostart/` in each
/// of `dirs`; an entry of an earlier directory overrides, or hides, the same
/// one in a later directory. Reads files: for a worker thread.
pub fn autostart(dirs: &[PathBuf]) -> Vec<Entry> {
    let mut by_id: BTreeMap<String, Option<Entry>> = BTreeMap::new();
    for dir in dirs {
        let Ok(read) = std::fs::read_dir(dir.join("autostart")) else {
            continue;
        };
        let mut files: Vec<PathBuf> = read
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "desktop"))
            .collect();
        files.sort();
        for file in files {
            let Some(id) = file.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            if by_id.contains_key(&id) {
                continue;
            }
            let entry = std::fs::read_to_string(&file)
                .ok()
                .and_then(|text| parse_for(&id, &text, None, true));
            by_id.insert(id, entry);
        }
    }
    by_id.into_values().flatten().collect()
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "desktop") {
            out.push(path);
        }
    }
}

/// An entry from a file's text; `None` for what the launcher must not show.
pub fn parse(id: &str, text: &str, lang: Option<&str>) -> Option<Entry> {
    parse_for(id, text, lang, false)
}

/// The same for autostart: `NoDisplay` does not matter there, and GNOME's
/// `X-GNOME-Autostart-enabled=false` turns an entry off.
fn parse_for(id: &str, text: &str, lang: Option<&str>, autostart: bool) -> Option<Entry> {
    let mut keys: BTreeMap<String, String> = BTreeMap::new();
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            keys.entry(k.trim().to_owned())
                .or_insert_with(|| v.trim().to_owned());
        }
    }
    let get = |k: &str| keys.get(k).map(|v| unescape(v));
    let localized = |k: &str| {
        lang.and_then(|l| {
            let short = l.split(['_', '.', '@']).next().unwrap_or(l);
            let full = l.split(['.', '@']).next().unwrap_or(l);
            get(&format!("{k}[{full}]")).or_else(|| get(&format!("{k}[{short}]")))
        })
        .or_else(|| get(k))
    };
    let yes = |k: &str| get(k).is_some_and(|v| v == "true");
    let list = |v: Option<String>| -> Vec<String> {
        v.map(|v| {
            v.split(';')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
    };
    if get("Type").as_deref() != Some("Application")
        || (yes("NoDisplay") && !autostart)
        || yes("Hidden")
        || (autostart && get("X-GNOME-Autostart-enabled").as_deref() == Some("false"))
    {
        return None;
    }
    let only = list(get("OnlyShowIn"));
    if !only.is_empty() && !only.iter().any(|d| d == DESKTOP) {
        return None;
    }
    if list(get("NotShowIn")).iter().any(|d| d == DESKTOP) {
        return None;
    }
    if let Some(program) = get("TryExec")
        && !found(&program)
    {
        return None;
    }
    let exec = exec_args(&get("Exec")?)?;
    Some(Entry {
        id: id.to_owned(),
        name: localized("Name")?,
        generic: localized("GenericName"),
        keywords: list(localized("Keywords")),
        exec,
        terminal: yes("Terminal"),
    })
}

/// The string escapes of desktop entries: `\s`, `\n`, `\t`, `\r`, `\\`.
fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
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

/// `Exec` as arguments: quoted ones joined, field codes (`%u`, `%F`…)
/// dropped since the launcher passes no files, `%%` a percent sign.
pub fn exec_args(exec: &str) -> Option<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                started = true;
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => current.push(chars.next()?),
                        c => current.push(c),
                    }
                }
            }
            ' ' | '\t' => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            // `%%` is a percent sign; a field code fills nothing here, and
            // alone it is no argument.
            '%' => {
                if chars.next()? == '%' {
                    current.push('%');
                    started = true;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        args.push(current);
    }
    (!args.is_empty()).then_some(args)
}

/// Whether `program` exists: a path, or a name on the `PATH`.
fn found(program: &str) -> bool {
    let p = Path::new(program);
    if p.is_absolute() {
        return p.is_file();
    }
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIREFOX: &str = "[Desktop Entry]\nType=Application\nName=Firefox\nName[fr]=Firefox (fr)\nGenericName=Web Browser\nKeywords=Internet;WWW;\nExec=firefox %u\n\n[Desktop Action new-window]\nName=New Window\nExec=firefox --new-window %u\n";

    #[test]
    fn an_entry_reads_its_name_and_command() {
        let e = parse("firefox.desktop", FIREFOX, None);
        assert_eq!(
            e,
            Some(Entry {
                id: "firefox.desktop".into(),
                name: "Firefox".into(),
                generic: Some("Web Browser".into()),
                keywords: vec!["Internet".into(), "WWW".into()],
                exec: vec!["firefox".into()],
                terminal: false,
            })
        );
        assert_eq!(
            parse("firefox.desktop", FIREFOX, Some("fr_FR.UTF-8")).map(|e| e.name),
            Some("Firefox (fr)".into())
        );
    }

    #[test]
    fn hidden_and_foreign_entries_are_left_out() {
        let hidden = FIREFOX.replace("Type=Application", "Type=Application\nNoDisplay=true");
        assert_eq!(parse("x.desktop", &hidden, None), None);
        let gnome = FIREFOX.replace("Type=Application", "Type=Application\nOnlyShowIn=GNOME;");
        assert_eq!(parse("x.desktop", &gnome, None), None);
        let link = FIREFOX.replace("Type=Application", "Type=Link");
        assert_eq!(parse("x.desktop", &link, None), None);
    }

    #[test]
    fn exec_follows_the_quoting_rules() {
        assert_eq!(
            exec_args(r#"sh -c "echo \"hi\" > \$HOME/x" %F"#),
            Some(vec![
                "sh".into(),
                "-c".into(),
                r#"echo "hi" > $HOME/x"#.into()
            ])
        );
        assert_eq!(
            exec_args("prog --rate=100%%"),
            Some(vec!["prog".into(), "--rate=100%".into()])
        );
        assert_eq!(exec_args("%U"), None);
    }

    #[test]
    fn string_escapes_come_first() {
        // `\\$` in the file is `\$` once unescaped, then `$` in the argument.
        let text = "[Desktop Entry]\nType=Application\nName=Env\nExec=sh -c \"env > \\\\$XDG_RUNTIME_DIR/env\"\n";
        let e = parse("env.desktop", text, None);
        assert_eq!(
            e.map(|e| e.exec),
            Some(vec![
                "sh".into(),
                "-c".into(),
                "env > $XDG_RUNTIME_DIR/env".into()
            ])
        );
    }

    #[test]
    fn autostart_entries_are_overridden_and_hidden_by_the_user() {
        let dir = std::env::temp_dir().join(format!("hocus-autostart-{}", std::process::id()));
        let user = dir.join("user/autostart");
        let system = dir.join("system/autostart");
        std::fs::create_dir_all(&user).expect("dir");
        std::fs::create_dir_all(&system).expect("dir");
        let entry = |exec: &str, extra: &str| {
            format!(
                "[Desktop Entry]\nType=Application\nName=X\nExec={exec}\nNoDisplay=true\n{extra}"
            )
        };
        std::fs::write(system.join("agent.desktop"), entry("agent", "")).expect("write");
        std::fs::write(system.join("tray.desktop"), entry("tray", "")).expect("write");
        std::fs::write(
            system.join("gnome.desktop"),
            entry("gnome", "X-GNOME-Autostart-enabled=false"),
        )
        .expect("write");
        std::fs::write(user.join("tray.desktop"), entry("tray", "Hidden=true")).expect("write");
        std::fs::write(user.join("mine.desktop"), entry("mine --quiet", "")).expect("write");
        let found: Vec<Vec<String>> = autostart(&[dir.join("user"), dir.join("system")])
            .into_iter()
            .map(|e| e.exec)
            .collect();
        assert_eq!(
            found,
            vec![
                vec!["agent".to_owned()],
                vec!["mine".to_owned(), "--quiet".to_owned()]
            ]
        );
        std::fs::remove_dir_all(&dir).expect("cleaned");
    }
}
