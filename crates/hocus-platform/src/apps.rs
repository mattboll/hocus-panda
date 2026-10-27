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
    /// The MIME types it opens (`MimeType`), for default applications.
    pub mime_types: Vec<String>,
    /// Its icon (`Icon`): a name in the icon themes, or a file's path.
    pub icon: Option<String>,
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
    let keys = entry_keys(text);
    let get = |k: &str| keys.get(k).map(|v| unescape(v));
    let yes = |k: &str| get(k).is_some_and(|v| v == "true");
    if (yes("NoDisplay") && !autostart)
        || yes("Hidden")
        || (autostart && get("X-GNOME-Autostart-enabled").as_deref() == Some("false"))
    {
        return None;
    }
    parse_keys(id, &keys, lang)
}

/// The keys of the `[Desktop Entry]` group, the first of each.
fn entry_keys(text: &str) -> BTreeMap<String, String> {
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
    keys
}

/// An entry from its keys, whether hidden or not: `None` for what does not
/// run here (another desktop's, a program missing).
fn parse_keys(id: &str, keys: &BTreeMap<String, String>, lang: Option<&str>) -> Option<Entry> {
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
    if get("Type").as_deref() != Some("Application") {
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
        mime_types: list(get("MimeType")),
        icon: get("Icon").filter(|i| !i.is_empty()),
    })
}

/// An autostart entry, started with the session or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Autostart {
    /// `tray.desktop`.
    pub id: String,
    pub name: String,
    pub enabled: bool,
}

/// Every autostart entry that would run here, on or off: an entry of an
/// earlier directory says whether it starts (`Hidden`,
/// `X-GNOME-Autostart-enabled`), the first that is whole says what it is.
/// Reads files: for a worker thread.
pub fn autostart_entries(dirs: &[PathBuf]) -> Vec<Autostart> {
    let mut out = Vec::new();
    for (id, files) in autostart_files(dirs) {
        let texts: Vec<String> = files
            .iter()
            .filter_map(|f| std::fs::read_to_string(f).ok())
            .collect();
        let Some(first) = texts.first() else {
            continue;
        };
        let keys = entry_keys(first);
        let get = |k: &str| keys.get(k).map(|v| unescape(v));
        let enabled = get("Hidden").as_deref() != Some("true")
            && get("X-GNOME-Autostart-enabled").as_deref() != Some("false");
        let Some(entry) = texts
            .iter()
            .find_map(|t| parse_keys(&id, &entry_keys(t), None))
        else {
            continue;
        };
        out.push(Autostart {
            id,
            name: entry.name,
            enabled,
        });
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// Starts `id` with the session or not: the user's own copy of it, in the
/// first of `dirs`, says so (the XDG way, which every desktop follows).
/// Writes a file: for a worker thread.
pub fn set_autostart(dirs: &[PathBuf], id: &str, enabled: bool) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    let user = dirs
        .first()
        .ok_or_else(|| Error::new(ErrorKind::NotFound, "no configuration directory"))?
        .join("autostart");
    let files = autostart_files(dirs)
        .remove(id)
        .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("no autostart entry {id}")))?;
    // A copy that only hides the entry says nothing else: the first whole
    // one is copied.
    let text = files
        .iter()
        .filter_map(|f| std::fs::read_to_string(f).ok())
        .find(|t| parse_keys(id, &entry_keys(t), None).is_some())
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, format!("{id} does not read")))?;
    let mut keys = vec![("Hidden", if enabled { "false" } else { "true" })];
    if entry_keys(&text).contains_key("X-GNOME-Autostart-enabled") {
        keys.push((
            "X-GNOME-Autostart-enabled",
            if enabled { "true" } else { "false" },
        ));
    }
    let text = set_keys(&text, "Desktop Entry", &keys);
    std::fs::create_dir_all(&user)?;
    write_atomically(&user.join(id), &text)
}

/// Each autostart entry's files, from the first directory to the last.
fn autostart_files(dirs: &[PathBuf]) -> BTreeMap<String, Vec<PathBuf>> {
    let mut by_id: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
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
            if let Some(id) = file.file_name().map(|n| n.to_string_lossy().into_owned()) {
                by_id.entry(id).or_default().push(file);
            }
        }
    }
    by_id
}

/// `text`, an INI file of desktop entries' kind, with these keys set in
/// `[group]` (added when missing), everything else as it was.
pub(crate) fn set_keys(text: &str, group: &str, keys: &[(&str, &str)]) -> String {
    let header = format!("[{group}]");
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let start = match lines.iter().position(|l| l.trim() == header) {
        Some(i) => i,
        None => {
            if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(header);
            lines.len() - 1
        }
    };
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map_or(lines.len(), |i| start + 1 + i);
    let mut insert_at = end;
    // Before the blank lines that close the group.
    while insert_at > start + 1 && lines[insert_at - 1].trim().is_empty() {
        insert_at -= 1;
    }
    for (key, value) in keys {
        let found = lines[start + 1..insert_at]
            .iter()
            .position(|l| l.split_once('=').is_some_and(|(k, _)| k.trim() == *key));
        match found {
            Some(i) => lines[start + 1 + i] = format!("{key}={value}"),
            None => {
                lines.insert(insert_at, format!("{key}={value}"));
                insert_at += 1;
            }
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Writes `text` to `path` whole or not at all: a file beside it, renamed.
pub(crate) fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, path)
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

/// The icon themes looked in, in this order: hicolor, where applications
/// install theirs, then the two most common full themes.
const ICON_THEMES: [&str; 3] = ["hicolor", "Adwaita", "breeze"];

/// An application's icon, decoded: width, height, premultiplied RGBA.
pub type IconPixels = (u16, u16, std::sync::Arc<[u8]>);

/// The icons named by `names` (desktop entries' `Icon`), at `size` pixels
/// square: from the icon themes under each of `data`
/// (`<dir>/icons/<theme>/<size>/apps`, the nearest size, an SVG drawn at
/// `size`), else `<dir>/pixmaps`; a name that is a path is that file. A
/// name found nowhere is left out. Reads and decodes files: for a worker
/// thread.
pub fn icons(names: &[String], data: &[PathBuf], size: u32) -> Vec<(String, IconPixels)> {
    let size = size.clamp(1, 1024);
    // The themes' folders are read once for all the names: an index of
    // what is wanted, the best file of each.
    let wanted: std::collections::BTreeSet<&str> = names
        .iter()
        .filter(|n| !n.starts_with('/'))
        .map(String::as_str)
        .collect();
    let mut best: BTreeMap<&str, ((usize, u32), PathBuf)> = BTreeMap::new();
    for dir in data {
        for (rank, theme) in ICON_THEMES.iter().enumerate() {
            let Ok(sizes) = std::fs::read_dir(dir.join("icons").join(theme)) else {
                continue;
            };
            for sized in sizes.flatten() {
                let name = sized.file_name().to_string_lossy().into_owned();
                let Some(px) = folder_size(&name) else {
                    continue;
                };
                let Ok(files) = std::fs::read_dir(sized.path().join("apps")) else {
                    continue;
                };
                for file in files.flatten() {
                    let path = file.path();
                    let (Some(stem), Some(ext)) = (
                        path.file_stem().and_then(|s| s.to_str()),
                        path.extension().and_then(|e| e.to_str()),
                    ) else {
                        continue;
                    };
                    let Some(&key) = wanted.get(stem) else {
                        continue;
                    };
                    let score = match (ext, px) {
                        ("svg", _) => 0,
                        // Larger is drawn smaller, sharp; smaller blurs.
                        ("png", Some(px)) if px >= size => px - size,
                        ("png", Some(px)) => 2 * (size - px),
                        _ => continue,
                    };
                    let rank = (rank, score);
                    if best.get(key).is_none_or(|(r, _)| rank < *r) {
                        best.insert(key, (rank, path));
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    for name in names {
        let path = if name.starts_with('/') {
            Some(PathBuf::from(name))
        } else {
            best.get(name.as_str())
                .map(|(_, p)| p.clone())
                .or_else(|| pixmap(name, data))
        };
        let Some(icon) = path.and_then(|p| crate::tray::load(&p, size)) else {
            continue;
        };
        if let Some(pixels) = premultiplied(&icon, size) {
            out.push((name.clone(), pixels));
        }
    }
    out
}

/// The size of an icon theme's folder (`48x48`, `48x48@2`), `None` inside
/// for `scalable`; `None` outside for what is not one.
fn folder_size(name: &str) -> Option<Option<u32>> {
    if name == "scalable" || name.starts_with("scalable@") {
        return Some(None);
    }
    let (dims, times) = name.split_once('@').unwrap_or((name, "1"));
    let px: u32 = dims.split('x').next()?.parse().ok()?;
    let times: u32 = times.trim_end_matches('x').parse().unwrap_or(1);
    Some(Some(px * times.max(1)))
}

/// `<dir>/pixmaps/<name>.png` or `.svg`, the first found.
fn pixmap(name: &str, data: &[PathBuf]) -> Option<PathBuf> {
    data.iter()
        .flat_map(|d| ["png", "svg"].map(|e| d.join("pixmaps").join(format!("{name}.{e}"))))
        .find(|p| p.is_file())
}

/// An icon at most `size` pixels square, premultiplied: a larger picture is
/// made smaller by averaging, its proportions kept.
fn premultiplied(icon: &crate::tray::Icon, size: u32) -> Option<IconPixels> {
    let (w, h) = (icon.width, icon.height);
    if w == 0 || h == 0 || icon.rgba.len() != (w * h * 4) as usize {
        return None;
    }
    let mut pre: Vec<u8> = icon
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let a = u32::from(p[3]);
            let m = |c: u8| ((u32::from(c) * a + 127) / 255) as u8;
            [m(p[0]), m(p[1]), m(p[2]), p[3]]
        })
        .collect();
    let (mut w, mut h) = (w, h);
    let longest = w.max(h);
    if longest > size {
        let (nw, nh) = ((w * size / longest).max(1), (h * size / longest).max(1));
        let mut small = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            let (y0, y1) = (y * h / nh, ((y + 1) * h / nh).max(y * h / nh + 1));
            for x in 0..nw {
                let (x0, x1) = (x * w / nw, ((x + 1) * w / nw).max(x * w / nw + 1));
                let mut sum = [0u32; 4];
                for sy in y0..y1 {
                    for sx in x0..x1 {
                        let i = ((sy * w + sx) * 4) as usize;
                        for (c, s) in sum.iter_mut().enumerate() {
                            *s += u32::from(pre[i + c]);
                        }
                    }
                }
                let n = (y1 - y0) * (x1 - x0);
                let o = ((y * nw + x) * 4) as usize;
                for (c, s) in sum.iter().enumerate() {
                    small[o + c] = (s / n) as u8;
                }
            }
        }
        (pre, w, h) = (small, nw, nh);
    }
    Some((u16::try_from(w).ok()?, u16::try_from(h).ok()?, pre.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIREFOX: &str = "[Desktop Entry]\nType=Application\nName=Firefox\nName[fr]=Firefox (fr)\nGenericName=Web Browser\nKeywords=Internet;WWW;\nMimeType=text/html;x-scheme-handler/https;\nExec=firefox %u\n\n[Desktop Action new-window]\nName=New Window\nExec=firefox --new-window %u\n";

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
                mime_types: vec!["text/html".into(), "x-scheme-handler/https".into()],
                icon: None,
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

    #[test]
    fn autostart_entries_are_turned_off_and_on_in_the_user_s_copy() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let (user, system) = (dir.path().join("user"), dir.path().join("system"));
        std::fs::create_dir_all(system.join("autostart"))?;
        let entry = |name: &str, extra: &str| {
            format!(
                "[Desktop Entry]\nType=Application\nName={name}\nExec={name}\n{extra}\n[Desktop Action x]\nName=X\n"
            )
        };
        std::fs::write(system.join("autostart/tray.desktop"), entry("Tray", ""))?;
        std::fs::write(
            system.join("autostart/gnome.desktop"),
            entry("Gnome", "X-GNOME-Autostart-enabled=false"),
        )?;
        let dirs = [user.clone(), system];
        let names = |dirs: &[PathBuf]| -> Vec<(String, bool)> {
            autostart_entries(dirs)
                .into_iter()
                .map(|a| (a.name, a.enabled))
                .collect()
        };
        assert_eq!(
            names(&dirs),
            [("Gnome".to_owned(), false), ("Tray".to_owned(), true)]
        );
        set_autostart(&dirs, "tray.desktop", false)?;
        let copy = std::fs::read_to_string(user.join("autostart/tray.desktop"))?;
        assert!(copy.contains("Hidden=true\n\n[Desktop Action x]"), "{copy}");
        assert!(autostart(&dirs).iter().all(|e| e.id != "tray.desktop"));
        set_autostart(&dirs, "gnome.desktop", true)?;
        assert_eq!(
            names(&dirs),
            [("Gnome".to_owned(), true), ("Tray".to_owned(), false)]
        );
        assert!(autostart(&dirs).iter().any(|e| e.id == "gnome.desktop"));
        // A copy that only hides it: turned on again, the whole entry.
        std::fs::write(
            user.join("autostart/tray.desktop"),
            "[Desktop Entry]\nHidden=true\n",
        )?;
        assert_eq!(names(&dirs)[1], ("Tray".to_owned(), false));
        set_autostart(&dirs, "tray.desktop", true)?;
        assert!(autostart(&dirs).iter().any(|e| e.id == "tray.desktop"));
        Ok(())
    }

    fn write_png(path: &Path, size: u32, rgba: [u8; 4]) -> std::io::Result<()> {
        std::fs::create_dir_all(path.parent().expect("a folder"))?;
        let file = std::fs::File::create(path)?;
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), size, size);
        encoder.set_color(png::ColorType::Rgba);
        let mut writer = encoder.write_header().map_err(std::io::Error::other)?;
        writer
            .write_image_data(&rgba.repeat((size * size) as usize))
            .map_err(std::io::Error::other)
    }

    #[test]
    fn icons_come_from_the_themes_at_the_nearest_size_then_pixmaps() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let data = dir.path().to_path_buf();
        let hicolor = data.join("icons/hicolor");
        write_png(&hicolor.join("48x48/apps/mail.png"), 48, [0, 0, 255, 255])?;
        write_png(
            &hicolor.join("256x256/apps/mail.png"),
            256,
            [255, 0, 0, 255],
        )?;
        std::fs::create_dir_all(hicolor.join("scalable/apps"))?;
        std::fs::write(
            hicolor.join("scalable/apps/notes.svg"),
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#00ff00"/></svg>"##,
        )?;
        // Half see-through, larger than asked: made smaller, premultiplied.
        write_png(&data.join("pixmaps/old.png"), 96, [200, 100, 0, 128])?;
        let names: Vec<String> = ["mail", "notes", "old", "missing"].map(String::from).into();
        let found = icons(&names, &[data], 48);
        let got: Vec<(&str, u16, u16, &[u8])> = found
            .iter()
            .map(|(n, (w, h, px))| (n.as_str(), *w, *h, &px[..4]))
            .collect();
        assert_eq!(
            got,
            [
                ("mail", 48, 48, &[0, 0, 255, 255][..]),
                ("notes", 48, 48, &[0, 255, 0, 255][..]),
                ("old", 48, 48, &[100, 50, 0, 128][..]),
            ]
        );
        assert_eq!(folder_size("48x48@2"), Some(Some(96)));
        assert_eq!(folder_size("scalable"), Some(None));
        assert_eq!(folder_size("symbolic"), None);
        Ok(())
    }

    #[test]
    fn keys_are_set_in_their_group_and_the_rest_kept() {
        let text = "# mine\n[Default Applications]\ntext/plain=gedit.desktop;\n\n[Added Associations]\nimage/png=eog.desktop;\n";
        assert_eq!(
            set_keys(
                text,
                "Default Applications",
                &[
                    ("text/plain", "kate.desktop;"),
                    ("image/png", "eog.desktop;")
                ]
            ),
            "# mine\n[Default Applications]\ntext/plain=kate.desktop;\nimage/png=eog.desktop;\n\n[Added Associations]\nimage/png=eog.desktop;\n"
        );
        assert_eq!(
            set_keys("", "Default Applications", &[("a/b", "c;")]),
            "[Default Applications]\na/b=c;\n"
        );
    }
}
