//! Captures et presse-papiers / Screenshots and clipboard (6.17, issue
//! #451, variant A): where screenshots and screen recordings go and in
//! which format, the shutter, and what the clipboard's history keeps.
//!
//! The compositor follows `[capture]`, `[recording]` and `[clipboard]` as
//! the file changes (docs/design/screenshots.md, bar.md): the next capture
//! or recording takes the folder, the format and the shutter; the shell's
//! history takes its rules at once, erasing what they leave out once the
//! toast's "Annuler" is gone (6 s). The folders' rows, the applications
//! left out and "Effacer l'historique" are drawn by the page itself; the
//! history's count comes from the compositor's line (`msg screens`),
//! which also clears it (`clipboard-clear`).

use hocus_words::settings::captures as words;
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use crate::{
    lang::{Lang, Words, pair, w},
    model::{Choice, Codec, Kind, Outside, Places, RowGroup, SectionId, Setting, Value, one},
};

pub const CAPTURES: RowGroup = RowGroup {
    label: w(words::SCREENSHOTS),
    per_source: false,
};
pub const RECORDING: RowGroup = RowGroup {
    label: w(words::SCREEN_RECORDING),
    per_source: false,
};
pub const CLIPBOARD: RowGroup = RowGroup {
    label: w(words::CLIPBOARD_HISTORY),
    per_source: false,
};

pub const CAPTURE_FOLDER: &str = "capture.folder";
pub const CAPTURE_FORMAT: &str = "capture.format";
pub const SOUND: &str = "capture.sound";
pub const RECORD_FOLDER: &str = "recording.folder";
pub const RECORD_FORMAT: &str = "recording.format";
pub const HISTORY: &str = "clipboard.history";
pub const KEEP: &str = "clipboard.keep";
pub const SECRETS: &str = "clipboard.skip-secrets";
pub const IGNORE: &str = "clipboard.ignore";
/// "Effacer l'historique": not in the file.
pub const CLEAR: &str = "clipboard.clear";
/// Keyboard's "Raccourcis du bureau", which the page's link lights.
pub const SHORTCUTS: &str = "keyboard.shortcuts";

/// The password managers left out always, by a part of their ids (the
/// compositor's), as the chips show them.
pub const BUILT_IN: [(&str, &str); 3] = [
    ("org.keepassxc.KeePassXC", "KeePassXC"),
    ("com.bitwarden.desktop", "Bitwarden"),
    ("1password", "1Password"),
];

/// Which folder a row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Folder {
    Captures,
    Recordings,
}

impl Folder {
    pub fn setting(self) -> &'static str {
        match self {
            Self::Captures => CAPTURE_FOLDER,
            Self::Recordings => RECORD_FOLDER,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Self::Captures => "capture",
            Self::Recordings => "recording",
        }
    }

    /// The default folder's own name, in the pictures' or videos' folder
    /// (the compositor's).
    pub fn default_name(self, lang: Lang) -> &'static str {
        match self {
            Self::Captures => words::SCREENSHOTS_2.get(lang),
            Self::Recordings => words::SCREEN_RECORDINGS.get(lang),
        }
    }

    /// The default folder: the pictures' (videos') folder's own.
    pub fn default_path(self, lang: Lang) -> PathBuf {
        let (key, fallback) = match self {
            Self::Captures => ("XDG_PICTURES_DIR", "Pictures"),
            Self::Recordings => ("XDG_VIDEOS_DIR", "Videos"),
        };
        user_dir(key, fallback).join(self.default_name(lang))
    }

    /// "« Capture du 2026-11-05 à 14-32-05.png »": the next file's name,
    /// as the compositor names it, `stamp` (`2026-11-05`, `14-32-05`) and
    /// `extension` its.
    pub fn sample(self, (day, time): (&str, &str), extension: &str, lang: Lang) -> String {
        match self {
            Self::Captures => words::screenshot_from(
                lang,
                words::ScreenshotFrom {
                    day,
                    extension,
                    time,
                },
            ),
            Self::Recordings => words::screen_recording_from(
                lang,
                words::ScreenRecordingFrom {
                    day,
                    extension,
                    time,
                },
            ),
        }
    }

    /// "Dossier des captures : Bureau › Captures".
    pub fn toast(self, shown: &str, lang: Lang) -> String {
        match self {
            Self::Captures => words::screenshots_folder(lang, shown),
            Self::Recordings => words::recordings_folder(lang, shown),
        }
    }

    /// The portal's title.
    pub fn title(self, lang: Lang) -> &'static str {
        match self {
            Self::Captures => words::CHOOSE_THE_SCREENSHOTS_FOLDER.get(lang),
            Self::Recordings => words::CHOOSE_THE_RECORDINGS_FOLDER.get(lang),
        }
    }

    /// The row's warning when its folder cannot be written, `default` the
    /// one used meanwhile.
    pub fn unwritable(self, gone: bool, default: &str, lang: Lang) -> String {
        let what = match self {
            Self::Captures => words::SCREENSHOTS_3.get(lang),
            Self::Recordings => words::RECORDINGS.get(lang),
        };
        match gone {
            true => words::this_folder_no_longer_exists_usb(
                lang,
                words::ThisFolderNoLongerExistsUsb { default, what },
            ),
            false => words::this_folder_cannot_be_written_go(
                lang,
                words::ThisFolderCannotBeWrittenGo { default, what },
            ),
        }
    }
}

/// The user's folder `key` (`XDG_PICTURES_DIR`): the variable, else
/// xdg-user-dirs' file, else `fallback` in the home.
fn user_dir(key: &str, fallback: &str) -> PathBuf {
    let home = home();
    if let Some(dir) = std::env::var_os(key)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return dir;
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    let said = std::fs::read_to_string(config.join("user-dirs.dirs"))
        .ok()
        .and_then(|text| {
            let value = text.lines().find_map(|line| {
                let (name, value) = line.trim().split_once('=')?;
                (name.trim() == key).then(|| value.trim().trim_matches('"').to_owned())
            })?;
            let path = match value.strip_prefix("$HOME") {
                Some(rest) => home.join(rest.trim_start_matches('/')),
                None => PathBuf::from(&value),
            };
            (path.is_absolute() && path != home).then_some(path)
        });
    said.unwrap_or_else(|| home.join(fallback))
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(PathBuf::new, PathBuf::from)
}

/// The folder the file says (`~/` the home), none for the default.
pub fn chosen(said: &str) -> Option<PathBuf> {
    let said = said.trim();
    if said.is_empty() {
        return None;
    }
    let path = match said.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => PathBuf::from(said),
    };
    path.is_absolute().then_some(path)
}

/// `path` as the file says it: from the home as `~/`.
pub fn written(path: &Path) -> String {
    let home = home();
    match path.strip_prefix(&home) {
        Ok(rest) if !home.as_os_str().is_empty() && !rest.as_os_str().is_empty() => {
            format!("~/{}", rest.display())
        }
        _ => path.display().to_string(),
    }
}

/// `path` as the row shows it: from the home, " › " between its last two
/// parts ("Images › Captures d'écran"); a folder elsewhere, its own name
/// after its parent's.
pub fn shown(path: &Path) -> String {
    let name = |p: Option<&Path>| {
        p.and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let (parent, own) = (name(path.parent()), name(Some(path)));
    let home = home();
    if path.parent() == Some(home.as_path()) || parent.is_empty() {
        own
    } else {
        format!("{parent} › {own}")
    }
}

/// What a folder's state is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Writable {
    Yes,
    Gone,
    ReadOnly,
}

/// Whether `dir` can be written, as the compositor checks it.
pub fn writable(dir: &Path) -> Writable {
    if !dir.is_dir() {
        return Writable::Gone;
    }
    match rustix::fs::access(dir, rustix::fs::Access::WRITE_OK) {
        Ok(()) => Writable::Yes,
        Err(_) => Writable::ReadOnly,
    }
}

/// The history's count, as the group's label says it.
pub fn count_said(items: u32, pinned: u32, lang: Lang) -> String {
    let items_word = match (items, lang) {
        (0, Lang::French) => return "Vide".to_owned(),
        (0, Lang::English) => return "Empty".to_owned(),
        (1, Lang::French) => "1 élément".to_owned(),
        (1, Lang::English) => "1 item".to_owned(),
        (n, Lang::French) => format!("{n} éléments"),
        (n, Lang::English) => format!("{n} items"),
    };
    match pinned {
        0 => items_word,
        1 => words::n_1_pinned(lang, &items_word),
        n => words::pinned(
            lang,
            words::Pinned {
                items_word: &items_word,
                x_n: n,
            },
        ),
    }
}

/// "140 éléments", what "Effacer…" and the history off erase.
pub fn items_said(n: u32, lang: Lang) -> String {
    match n {
        1 => words::N_1_ITEM.get(lang).to_owned(),
        n => words::items(lang, n),
    }
}

fn word(fr: &'static str, en: &'static str, word: &'static str) -> Choice {
    Choice {
        label: pair(fr, en),
        value: Value::Word(word),
    }
}

#[allow(clippy::too_many_arguments)]
fn setting(
    id: &'static str,
    group: RowGroup,
    name: Words,
    line: Option<Words>,
    keywords: (&'static [&'static str], &'static [&'static str]),
    kind: Kind,
    places: Places,
    needs: Option<&'static str>,
) -> Setting {
    Setting {
        id,
        section: SectionId::Captures,
        group,
        name,
        line,
        keywords,
        kind,
        places,
        needs,
    }
}

/// The page's settings, in its order.
pub fn settings() -> &'static [Setting] {
    static SETTINGS: OnceLock<Vec<Setting>> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        let folder = w(words::FOLDER);
        let format = w(words::FORMAT);
        vec![
            setting(
                CAPTURE_FOLDER,
                CAPTURES,
                folder,
                None,
                (
                    &["capture", "impr", "image", "dossier", "emplacement"],
                    &["screenshot", "print", "image", "folder", "location"],
                ),
                Kind::Text,
                one(&["capture", "folder"], Codec::Text),
                None,
            ),
            setting(
                CAPTURE_FORMAT,
                CAPTURES,
                format,
                None,
                (
                    &["capture", "png", "jpeg", "jpg", "image"],
                    &["screenshot", "png", "jpeg", "jpg", "image"],
                ),
                Kind::Segmented(vec![
                    word("PNG", "PNG", "png"),
                    word("JPEG", "JPEG", "jpeg"),
                ]),
                one(&["capture", "format"], Codec::Word(&["png", "jpeg"])),
                None,
            ),
            setting(
                SOUND,
                CAPTURES,
                w(words::SHUTTER_SOUND),
                Some(w(words::A_DISCREET_CLICK_WHEN_THE_PICTURE)),
                (
                    &["son", "obturateur", "déclic", "capture"],
                    &["sound", "shutter", "click", "screenshot"],
                ),
                Kind::Switch,
                one(&["capture", "sound"], Codec::Bool),
                None,
            ),
            setting(
                RECORD_FOLDER,
                RECORDING,
                folder,
                None,
                (
                    &["enregistrement", "vidéo", "dossier", "emplacement"],
                    &["recording", "video", "folder", "location"],
                ),
                Kind::Text,
                one(&["recording", "folder"], Codec::Text),
                None,
            ),
            setting(
                RECORD_FORMAT,
                RECORDING,
                format,
                None,
                (
                    &["enregistrement", "vidéo", "mp4", "webm"],
                    &["recording", "video", "mp4", "webm"],
                ),
                Kind::Segmented(vec![
                    word("MP4", "MP4", "mp4"),
                    word("WebM", "WebM", "webm"),
                ]),
                one(&["recording", "format"], Codec::Word(&["mp4", "webm"])),
                None,
            ),
            setting(
                HISTORY,
                CLIPBOARD,
                w(words::KEEP_WHAT_I_COPY),
                Some(w(words::SUPER_V_SHOWS_IT_TO_PASTE)),
                (
                    &["presse-papiers", "historique", "super+v", "copier"],
                    &["clipboard", "history", "super+v", "copy"],
                ),
                Kind::Switch,
                one(&["clipboard", "history"], Codec::Bool),
                None,
            ),
            setting(
                KEEP,
                CLIPBOARD,
                w(words::KEEP_FOR),
                Some(w(words::PINNED_ITEMS_ALWAYS_STAY_200_ITEMS)),
                (
                    &["durée", "garder", "presse-papiers", "historique"],
                    &["duration", "keep", "clipboard", "history"],
                ),
                Kind::Menu(vec![
                    word(
                        "Jusqu’à la fin de la session",
                        "Until the session ends",
                        "session",
                    ),
                    word("1 jour", "1 day", "1d"),
                    word("7 jours", "7 days", "7d"),
                    word("30 jours", "30 days", "30d"),
                ]),
                one(
                    &["clipboard", "keep"],
                    Codec::Word(&["session", "1d", "7d", "30d"]),
                ),
                Some(HISTORY),
            ),
            setting(
                SECRETS,
                CLIPBOARD,
                w(words::LEAVE_PASSWORDS_OUT),
                Some(w(words::WHAT_A_PASSWORD_MANAGER_MARKS_SECRET)),
                (
                    &[
                        "mots de passe",
                        "secret",
                        "keepassxc",
                        "bitwarden",
                        "presse-papiers",
                    ],
                    &["passwords", "secret", "keepassxc", "bitwarden", "clipboard"],
                ),
                Kind::Switch,
                one(&["clipboard", "skip-secrets"], Codec::Bool),
                Some(HISTORY),
            ),
            setting(
                IGNORE,
                CLIPBOARD,
                w(words::APPLICATIONS_LEFT_OUT),
                None,
                (
                    &["ignorer", "mots de passe", "application"],
                    &["ignore", "passwords", "application"],
                ),
                Kind::Text,
                one(&["clipboard", "ignore"], Codec::Texts),
                Some(HISTORY),
            ),
        ]
    })
}

/// "Effacer l'historique", a row of the page's own.
pub fn outside() -> &'static [Outside] {
    const OUTSIDE: [Outside; 1] = [Outside {
        id: CLEAR,
        section: SectionId::Captures,
        group: CLIPBOARD.label,
        name: w(words::CLEAR_THE_HISTORY),
        keywords: (
            &["effacer", "vider", "presse-papiers", "historique"],
            &["clear", "empty", "clipboard", "history"],
        ),
    }];
    &OUTSIDE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_count_says_its_items_and_pins() {
        assert_eq!(
            count_said(143, 3, Lang::French),
            "143 éléments · 3 épinglés"
        );
        assert_eq!(count_said(1, 1, Lang::English), "1 item · 1 pinned");
        assert_eq!(count_said(4, 0, Lang::French), "4 éléments");
        assert_eq!(count_said(0, 0, Lang::French), "Vide");
    }

    #[test]
    fn a_sample_name_is_the_compositors() {
        let stamp = ("2026-11-05", "14-32-05");
        assert_eq!(
            Folder::Captures.sample(stamp, "png", Lang::French),
            "« Capture du 2026-11-05 à 14-32-05.png »"
        );
        assert_eq!(
            Folder::Recordings.sample(stamp, "webm", Lang::English),
            "“Screen recording from 2026-11-05 14-32-05.webm”"
        );
    }

    #[test]
    fn a_folder_reads_and_writes_from_the_home() {
        let dir = tempfile::tempdir().expect("a folder");
        assert_eq!(writable(&dir.path().join("gone")), Writable::Gone);
        assert_eq!(writable(dir.path()), Writable::Yes);
        assert_eq!(chosen(""), None);
        assert_eq!(chosen("/data/shots"), Some(PathBuf::from("/data/shots")));
        assert_eq!(chosen("relative"), None);
        assert_eq!(
            shown(Path::new("/media/alex/KEY/Captures")),
            "KEY › Captures"
        );
    }
}
