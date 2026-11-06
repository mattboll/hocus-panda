//! The applications installed, as the launcher lists them: the desktop
//! entries of the XDG data directories (`applications/*.desktop`), those
//! shown, by their name and the id their windows go by (`StartupWMClass`,
//! else the entry's id). Read on a thread of their own as the program
//! starts; the rules per application's "+ Ajouter une règle" lists them
//! (6.14).

use std::{
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock, PoisonError},
};

/// An application installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// The id its windows go by.
    pub id: String,
    pub name: String,
}

static FINDING: Mutex<Option<std::thread::JoinHandle<Vec<App>>>> = Mutex::new(None);
static FOUND: OnceLock<Vec<App>> = OnceLock::new();

/// Starts reading the desktop entries, before the window, in `French` or
/// not (their `Name[fr]`).
pub fn find(french: bool) {
    let started = std::thread::Builder::new()
        .name("applications".to_owned())
        .spawn(move || read_all(&dirs(), french));
    if let Ok(handle) = started {
        *FINDING.lock().unwrap_or_else(PoisonError::into_inner) = Some(handle);
    }
}

/// The applications, sorted by name (waiting for the thread the first
/// time, which has long finished by the time a menu opens).
pub fn list() -> &'static [App] {
    FOUND.get_or_init(|| {
        let handle = FINDING
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        handle.and_then(|h| h.join().ok()).unwrap_or_default()
    })
}

/// The application that goes by `id`, if installed.
pub fn named(id: &str) -> Option<&'static App> {
    list().iter().find(|a| a.id.eq_ignore_ascii_case(id))
}

/// The XDG data directories' `applications`, the user's first.
fn dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".local/share")));
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    data_home
        .into_iter()
        .chain(data_dirs.split(':').map(PathBuf::from))
        .map(|d| d.join("applications"))
        .collect()
}

/// Every entry shown in `dirs`, the first of an id winning, by name.
pub fn read_all(dirs: &[PathBuf], french: bool) -> Vec<App> {
    let mut seen: Vec<String> = Vec::new();
    let mut apps: Vec<App> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        files.sort();
        for path in files {
            if path.extension().is_none_or(|e| e != "desktop") {
                continue;
            }
            let Some(stem) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(ToOwned::to_owned)
            else {
                continue;
            };
            // A user's entry hides the system's of the same id.
            if seen.contains(&stem) {
                continue;
            }
            seen.push(stem.clone());
            if let Some(app) = read(&path, &stem, french) {
                apps.push(app);
            }
        }
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps.dedup_by(|a, b| a.id == b.id);
    apps
}

/// One entry: an application shown, its name and its windows' id.
fn read(path: &Path, stem: &str, french: bool) -> Option<App> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    let (mut name, mut name_fr, mut class) = (None, None, None);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "Type" if value != "Application" => return None,
            "NoDisplay" | "Hidden" if value == "true" => return None,
            "Name" => name = Some(value.to_owned()),
            "Name[fr]" => name_fr = Some(value.to_owned()),
            "StartupWMClass" => class = Some(value.to_owned()),
            _ => {}
        }
    }
    let name = if french { name_fr.or(name) } else { name }?;
    Some(App {
        id: class.unwrap_or_else(|| stem.to_owned()),
        name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entries_shown_by_name_with_their_windows_id() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let (user, system) = (dir.path().join("user"), dir.path().join("system"));
        std::fs::create_dir_all(&user)?;
        std::fs::create_dir_all(&system)?;
        std::fs::write(
            system.join("org.example.Mail.desktop"),
            "[Desktop Entry]\nType=Application\nName=Mail\nName[fr]=Courrier\n",
        )?;
        std::fs::write(
            system.join("firefox-esr.desktop"),
            "[Desktop Entry]\nType=Application\nName=Firefox\nStartupWMClass=firefox-esr\n\n\
             [Desktop Action new]\nName=New window\n",
        )?;
        std::fs::write(
            system.join("hidden.desktop"),
            "[Desktop Entry]\nType=Application\nName=Hidden\nNoDisplay=true\n",
        )?;
        std::fs::write(
            system.join("foot.desktop"),
            "[Desktop Entry]\nType=Application\nName=Foot\n",
        )?;
        // The user's foot hides the system's.
        std::fs::write(
            user.join("foot.desktop"),
            "[Desktop Entry]\nType=Application\nName=Foot\nHidden=true\n",
        )?;
        let apps = read_all(&[user, system], true);
        let said: Vec<(&str, &str)> = apps
            .iter()
            .map(|a| (a.id.as_str(), a.name.as_str()))
            .collect();
        assert_eq!(
            said,
            [("org.example.Mail", "Courrier"), ("firefox-esr", "Firefox")]
        );
        Ok(())
    }
}
