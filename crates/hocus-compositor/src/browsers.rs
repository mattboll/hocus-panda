//! Browsers' tabs (docs/design/projects.md §8.3, lot C of #789): read from
//! the browsers' own session files, which they keep for their crash
//! recovery, on a worker thread (never on the loop); no extension, nothing
//! in the browser changed.
//!
//! - **Which windows**: those whose app id (or desktop entry) is Firefox's
//!   or a Chromium's ([`of_app`]).
//! - **Firefox**: its profiles (`profiles.ini`) in its four places; the one
//!   running is the one whose `lock` names a process alive on this
//!   machine, else the one whose session was written last. Its file is
//!   `sessionstore-backups/recovery.jsonlz4` (`sessionstore.jsonlz4` once
//!   it quit): mozLz4 ([`mozlz4`]), JSON inside ([`firefox_session`]).
//!   Private windows are not written; one that would be is left out.
//! - **Chromium**: the profile last used (`Local State`), its newest
//!   `Sessions/Session_*`: SNSS ([`snss`]), its commands read as
//!   Chromium's `components/sessions` (BSD) writes them; incognito windows
//!   are not written.
//! - **Matching** a window to the file's ([`matched`]): its title without
//!   the browser's suffix against each saved window's selected tab's
//!   title; two alike are told apart by their size, then their order. A
//!   private window (its title says so) is never matched.
//!
//! Addresses and titles are never logged ([`Tab`]'s `Debug` says neither).

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A browser's family, as the project's file says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    #[default]
    Firefox,
    Chromium,
}

/// A browser a window is of: its family, and for a Chromium its folder
/// under the configuration's home.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Browser {
    pub kind: Kind,
    pub folder: &'static str,
}

/// Firefox's app ids and entries' ids (§8.3).
const FIREFOXES: [&str; 4] = [
    "firefox",
    "org.mozilla.firefox",
    "firefox-esr",
    "firefox_firefox",
];

/// The Chromiums' app ids and entries' ids, and their folders.
const CHROMIUMS: [(&str, &str); 11] = [
    ("chromium", "chromium"),
    ("chromium-browser", "chromium"),
    ("google-chrome", "google-chrome"),
    ("google-chrome-stable", "google-chrome"),
    ("brave-browser", "BraveSoftware/Brave-Browser"),
    ("brave-browser-stable", "BraveSoftware/Brave-Browser"),
    ("com.brave.browser", "BraveSoftware/Brave-Browser"),
    ("vivaldi", "vivaldi"),
    ("vivaldi-stable", "vivaldi"),
    ("microsoft-edge", "microsoft-edge"),
    ("microsoft-edge-stable", "microsoft-edge"),
];

/// The browser of an app id or a desktop entry's id, if it is one.
pub fn of_app(id: &str) -> Option<Browser> {
    let id = id.to_lowercase();
    if FIREFOXES.contains(&id.as_str()) {
        return Some(Browser {
            kind: Kind::Firefox,
            folder: "",
        });
    }
    CHROMIUMS
        .iter()
        .find(|(app, _)| *app == id)
        .map(|(_, folder)| Browser {
            kind: Kind::Chromium,
            folder,
        })
}

/// Where the browsers keep their files: the home, and the configuration's
/// home (`$XDG_CONFIG_HOME`, else `~/.config`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Places {
    pub home: PathBuf,
    pub config: PathBuf,
}

/// A tab: its address, its title, whether it is pinned. Never logged.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Tab {
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
}

impl std::fmt::Debug for Tab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Tab")
    }
}

/// A browser window as a project keeps it (§6, `[window.browser]`): its
/// tabs in order and the one shown (from 1); none, and `selected` 0, when
/// they could not be read (the window was not found in the file).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SavedBrowser {
    pub kind: Kind,
    #[serde(default)]
    pub selected: usize,
    #[serde(default, rename = "tab", skip_serializing_if = "Vec::is_empty")]
    pub tabs: Vec<Tab>,
}

impl SavedBrowser {
    /// Whether its tabs were read.
    pub fn read(&self) -> bool {
        !self.tabs.is_empty()
    }

    /// The title of the tab it shows.
    pub fn shown_title(&self) -> Option<&str> {
        self.tabs
            .get(self.selected.saturating_sub(1))
            .map(|t| t.title.as_str())
    }

    /// The addresses it opens again, in order: those a browser takes from
    /// its command line (a scheme, never an option), 200 at most.
    pub fn addresses(&self) -> Vec<String> {
        self.tabs
            .iter()
            .map(|t| t.url.trim())
            .filter(|u| openable(u))
            .take(200)
            .map(str::to_owned)
            .collect()
    }
}

/// Whether `url` may be given on a browser's command line: a scheme, and
/// nothing an option could be read in.
fn openable(url: &str) -> bool {
    let Some((scheme, _)) = url.split_once(':') else {
        return false;
    };
    !url.starts_with('-')
        && !url.chars().any(char::is_whitespace)
        && scheme
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}

/// An address's host, as the card shows it (§4, §10: never the whole
/// address): `github.com`, `localhost:4321`; a scheme without one, its
/// scheme (`about`, `file`).
pub fn host_of(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once(':') else {
        return String::new();
    };
    let Some(rest) = rest.strip_prefix("//") else {
        return scheme.to_owned();
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or_default();
    if host.is_empty() {
        scheme.to_owned()
    } else {
        host.to_owned()
    }
}

/// What a command line opening `addresses` in one new window adds (§8.3):
/// Chromium's `--new-window URL…`; Firefox's `--new-window URL` for one,
/// and the addresses alone for several, which Firefox opens together in
/// one new window (`nsDefaultCommandLineHandler`: more than one address
/// for the default window never goes to an existing one).
pub fn new_window_args(kind: Kind, addresses: &[String]) -> Vec<String> {
    match (kind, addresses) {
        (_, []) => Vec::new(),
        (Kind::Firefox, [one]) => vec!["--new-window".to_owned(), one.clone()],
        (Kind::Firefox, many) => many.to_vec(),
        (Kind::Chromium, many) => std::iter::once("--new-window".to_owned())
            .chain(many.iter().cloned())
            .collect(),
    }
}

// --- What a file says ----------------------------------------------------------------------

/// A window as a session file says it: its tabs, the one shown (from 0),
/// its size when the file has it, and whether it was closed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileWindow {
    pub tabs: Vec<Tab>,
    pub selected: usize,
    pub size: Option<(i32, i32)>,
    pub closed: bool,
}

impl FileWindow {
    fn shown_title(&self) -> &str {
        self.tabs
            .get(self.selected)
            .or_else(|| self.tabs.first())
            .map_or("", |t| t.title.as_str())
    }

    /// As a project keeps it.
    pub fn saved(&self, kind: Kind) -> SavedBrowser {
        SavedBrowser {
            kind,
            selected: self.selected.min(self.tabs.len().saturating_sub(1)) + 1,
            tabs: self.tabs.clone(),
        }
    }
}

/// mozLz4's content: the 8 bytes `mozLz40\0`, the size as 4 little-endian
/// bytes, then one LZ4 block (lz4's block format).
pub fn mozlz4(bytes: &[u8]) -> Option<Vec<u8>> {
    let rest = bytes.strip_prefix(b"mozLz40\0")?;
    let size = u32::from_le_bytes(rest.get(..4)?.try_into().ok()?);
    // A session larger than this is not one.
    if size > 256 << 20 {
        return None;
    }
    lz4_block(rest.get(4..)?, size as usize)
}

/// An LZ4 block of `size` bytes once decompressed: sequences of a token,
/// literals, then a match (an offset back and a length) but for the last.
pub fn lz4_block(input: &[u8], size: usize) -> Option<Vec<u8>> {
    let mut out: Vec<u8> = Vec::with_capacity(size);
    let mut at = 0;
    let byte = |at: &mut usize| -> Option<u8> {
        let b = *input.get(*at)?;
        *at += 1;
        Some(b)
    };
    let length = |at: &mut usize, first: usize| -> Option<usize> {
        let mut n = first;
        if first == 15 {
            loop {
                let more = byte(at)?;
                n += usize::from(more);
                if more != 255 {
                    break;
                }
            }
        }
        Some(n)
    };
    while at < input.len() {
        let token = byte(&mut at)?;
        let literals = length(&mut at, usize::from(token >> 4))?;
        out.extend_from_slice(input.get(at..at.checked_add(literals)?)?);
        at += literals;
        if at >= input.len() {
            break;
        }
        let offset = usize::from(u16::from_le_bytes([byte(&mut at)?, byte(&mut at)?]));
        if offset == 0 || offset > out.len() {
            return None;
        }
        let count = length(&mut at, usize::from(token & 15))? + 4;
        if out.len() + count > size {
            return None;
        }
        let from = out.len() - offset;
        // Byte by byte: a match may overlap what it writes.
        for i in 0..count {
            let b = *out.get(from + i)?;
            out.push(b);
        }
    }
    (out.len() == size).then_some(out)
}

/// Firefox's session (its JSON): its windows, then those closed
/// (`_closedWindows`). Private windows are left out.
pub fn firefox_session(json: &[u8]) -> Option<Vec<FileWindow>> {
    let value: Value = serde_json::from_slice(json).ok()?;
    let windows = |key: &str, closed: bool| -> Vec<FileWindow> {
        value
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|w| !w.get("isPrivate").and_then(Value::as_bool).unwrap_or(false))
            .filter_map(|w| firefox_window(w, closed))
            .collect()
    };
    let mut out = windows("windows", false);
    out.extend(windows("_closedWindows", true));
    Some(out)
}

fn firefox_window(w: &Value, closed: bool) -> Option<FileWindow> {
    let number = |v: &Value, key: &str| v.get(key).and_then(Value::as_i64);
    let tabs: Vec<Tab> = w
        .get("tabs")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|tab| {
            let entries = tab.get("entries").and_then(Value::as_array)?;
            let index = usize::try_from(number(tab, "index").unwrap_or(1)).unwrap_or(1);
            let entry = entries.get(index.saturating_sub(1)).or(entries.last())?;
            let text = |key: &str| {
                entry
                    .get(key)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            Some(Tab {
                url: text("url"),
                title: text("title"),
                pinned: tab.get("pinned").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .filter(|t| !t.url.is_empty())
        .collect();
    if tabs.is_empty() {
        return None;
    }
    let selected = usize::try_from(number(w, "selected").unwrap_or(1).max(1) - 1).unwrap_or(0);
    let size = number(w, "width")
        .zip(number(w, "height"))
        .and_then(|(w, h)| Some((i32::try_from(w).ok()?, i32::try_from(h).ok()?)));
    Some(FileWindow {
        tabs,
        selected,
        size,
        closed,
    })
}

// --- Chromium's SNSS ------------------------------------------------------------------------

/// SNSS's commands read (`components/sessions/core/session_service_commands.cc`).
mod command {
    pub const SET_TAB_WINDOW: u8 = 0;
    pub const SET_TAB_INDEX_IN_WINDOW: u8 = 2;
    pub const PRUNED_FROM_BACK: u8 = 5;
    pub const UPDATE_TAB_NAVIGATION: u8 = 6;
    pub const SELECTED_NAVIGATION_INDEX: u8 = 7;
    pub const SELECTED_TAB_IN_INDEX: u8 = 8;
    pub const SET_WINDOW_TYPE: u8 = 9;
    pub const PINNED_STATE: u8 = 12;
    pub const WINDOW_BOUNDS: u8 = 14;
    pub const TAB_CLOSED: u8 = 16;
    pub const WINDOW_CLOSED: u8 = 17;
    pub const PRUNED: u8 = 24;
}

#[derive(Debug, Default)]
struct SnssTab {
    window: i32,
    index: i32,
    navigations: std::collections::BTreeMap<i32, (String, String)>,
    shown: Option<i32>,
    pinned: bool,
    closed: bool,
}

#[derive(Debug, Default)]
struct SnssWindow {
    selected: i32,
    normal: bool,
    size: Option<(i32, i32)>,
    closed: bool,
}

/// A command's payload, read as its structures are laid out (4-byte
/// integers, little-endian).
fn int(payload: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(
        payload.get(at..at + 4)?.try_into().ok()?,
    ))
}

/// A pickle (`base::Pickle`): its 4-byte header, then its fields, each
/// aligned to 4 bytes.
struct Pickle<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Pickle<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 4 }
    }

    fn int(&mut self) -> Option<i32> {
        let v = int(self.data, self.at)?;
        self.at += 4;
        Some(v)
    }

    fn bytes(&mut self, len: usize) -> Option<&'a [u8]> {
        let out = self.data.get(self.at..self.at.checked_add(len)?)?;
        self.at += len.div_ceil(4) * 4;
        Some(out)
    }

    fn string(&mut self) -> Option<String> {
        let len = usize::try_from(self.int()?).ok()?;
        Some(String::from_utf8_lossy(self.bytes(len)?).into_owned())
    }

    fn string16(&mut self) -> Option<String> {
        let len = usize::try_from(self.int()?).ok()?;
        let raw = self.bytes(len.checked_mul(2)?)?;
        let units: Vec<u16> = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        Some(String::from_utf16_lossy(&units))
    }
}

/// Chromium's session file (SNSS): the 4 bytes `SNSS`, a version (1 or 3;
/// 2 and 4 are encrypted, not read), then commands, each a 16-bit size, an
/// 8-bit id and its payload. Its normal windows, those closed too, each
/// with its tabs in order.
pub fn snss(bytes: &[u8]) -> Option<Vec<FileWindow>> {
    let rest = bytes.strip_prefix(b"SNSS")?;
    let version = int(rest, 0)?;
    if !matches!(version, 1 | 3) {
        return None;
    }
    let mut at = 4;
    let mut tabs: HashMap<i32, SnssTab> = HashMap::new();
    let mut windows: HashMap<i32, SnssWindow> = HashMap::new();
    let mut order: Vec<i32> = Vec::new();
    let mut window = |id: i32, windows: &mut HashMap<i32, SnssWindow>| {
        if !order.contains(&id) {
            order.push(id);
        }
        windows.entry(id).or_insert_with(|| SnssWindow {
            normal: true,
            ..SnssWindow::default()
        });
    };
    while let Some(Ok(head)) = rest.get(at..at + 2).map(<[u8; 2]>::try_from) {
        let size = usize::from(u16::from_le_bytes(head));
        let Some(body) = rest.get(at + 2..at + 2 + size) else {
            // A file cut short: what was read stays.
            break;
        };
        at += 2 + size;
        let Some((&id, payload)) = body.split_first() else {
            continue;
        };
        let pair = || Some((int(payload, 0)?, int(payload, 4)?));
        match id {
            command::SET_TAB_WINDOW => {
                if let Some((w, t)) = pair() {
                    window(w, &mut windows);
                    tabs.entry(t).or_default().window = w;
                }
            }
            command::SET_TAB_INDEX_IN_WINDOW => {
                if let Some((t, index)) = pair() {
                    tabs.entry(t).or_default().index = index;
                }
            }
            command::PRUNED_FROM_BACK => {
                if let Some((t, count)) = pair() {
                    tabs.entry(t)
                        .or_default()
                        .navigations
                        .retain(|i, _| *i < count);
                }
            }
            command::PRUNED => {
                if let (Some((t, index)), Some(count)) = (pair(), int(payload, 8)) {
                    let tab = tabs.entry(t).or_default();
                    let kept = std::mem::take(&mut tab.navigations);
                    tab.navigations = kept
                        .into_iter()
                        .filter(|(i, _)| *i < index || *i >= index + count)
                        .map(|(i, n)| (if i >= index + count { i - count } else { i }, n))
                        .collect();
                }
            }
            command::UPDATE_TAB_NAVIGATION => {
                let mut pickle = Pickle::new(payload);
                let read = (|| {
                    let tab = pickle.int()?;
                    let index = pickle.int()?;
                    let url = pickle.string()?;
                    let title = pickle.string16()?;
                    Some((tab, index, url, title))
                })();
                if let Some((t, index, url, title)) = read {
                    tabs.entry(t)
                        .or_default()
                        .navigations
                        .insert(index, (url, title));
                }
            }
            command::SELECTED_NAVIGATION_INDEX => {
                if let Some((t, index)) = pair() {
                    tabs.entry(t).or_default().shown = Some(index);
                }
            }
            command::SELECTED_TAB_IN_INDEX => {
                if let Some((w, index)) = pair() {
                    window(w, &mut windows);
                    if let Some(win) = windows.get_mut(&w) {
                        win.selected = index;
                    }
                }
            }
            command::SET_WINDOW_TYPE => {
                if let Some((w, kind)) = pair() {
                    window(w, &mut windows);
                    if let Some(win) = windows.get_mut(&w) {
                        win.normal = kind == 0;
                    }
                }
            }
            command::PINNED_STATE => {
                if let (Some(t), Some(&pinned)) = (int(payload, 0), payload.get(4)) {
                    tabs.entry(t).or_default().pinned = pinned != 0;
                }
            }
            command::WINDOW_BOUNDS => {
                if let (Some(w), Some(width), Some(height)) =
                    (int(payload, 0), int(payload, 12), int(payload, 16))
                {
                    window(w, &mut windows);
                    if let Some(win) = windows.get_mut(&w) {
                        win.size = Some((width, height));
                    }
                }
            }
            command::TAB_CLOSED => {
                if let Some(t) = int(payload, 0) {
                    tabs.entry(t).or_default().closed = true;
                }
            }
            command::WINDOW_CLOSED => {
                if let Some(w) = int(payload, 0)
                    && let Some(win) = windows.get_mut(&w)
                {
                    win.closed = true;
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for id in order {
        let Some(win) = windows.get(&id).filter(|w| w.normal) else {
            continue;
        };
        let mut theirs: Vec<&SnssTab> = tabs
            .values()
            .filter(|t| t.window == id && !t.closed && !t.navigations.is_empty())
            .collect();
        theirs.sort_by_key(|t| t.index);
        let tabs: Vec<Tab> = theirs
            .iter()
            .filter_map(|t| {
                let shown = t
                    .shown
                    .and_then(|i| t.navigations.get(&i))
                    .or_else(|| t.navigations.values().last())?;
                Some(Tab {
                    url: shown.0.clone(),
                    title: shown.1.clone(),
                    pinned: t.pinned,
                })
            })
            .collect();
        if tabs.is_empty() {
            continue;
        }
        out.push(FileWindow {
            selected: usize::try_from(win.selected).unwrap_or(0),
            tabs,
            size: win.size,
            closed: win.closed,
        });
    }
    Some(out)
}

// --- Matching --------------------------------------------------------------------------------

/// What a browser window's title says: its page's title, or that it is
/// private.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shown {
    Page(String),
    Private,
}

const PRIVATE_WORDS: [&str; 5] = [
    "private browsing",
    "navigation privée",
    "incognito",
    "inprivate",
    "privé",
];
const BROWSER_WORDS: [&str; 7] = [
    "firefox", "mozilla", "chromium", "chrome", "brave", "vivaldi", "edge",
];

/// A browser window's title without its browser's suffix ("hocus-site ·
/// Accueil — Mozilla Firefox", "… - Chromium"), or that it is private.
pub fn shown(title: &str) -> Shown {
    let title = title.trim();
    let split = [" — ", " – ", " - "]
        .iter()
        .filter_map(|sep| title.rfind(sep).map(|at| (at, sep.len())))
        .max_by_key(|(at, _)| *at);
    let (page, tail) = match split {
        Some((at, len)) => (
            title.get(..at).unwrap_or_default(),
            title.get(at + len..).unwrap_or_default(),
        ),
        None => ("", title),
    };
    let tail_low = tail.to_lowercase();
    if PRIVATE_WORDS.iter().any(|w| tail_low.contains(w)) {
        return Shown::Private;
    }
    if BROWSER_WORDS.iter().any(|w| tail_low.contains(w)) {
        Shown::Page(page.trim().to_owned())
    } else {
        Shown::Page(title.to_owned())
    }
}

/// A window to match: its title as [`shown`] says it, and its size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub title: String,
    pub size: (i32, i32),
}

/// Which of `file`'s windows each of `seen` is (in order), none for one
/// not found: the same shown title, the nearest size, then the first;
/// each file window taken once. The closed ones first when `closed_first`
/// (read again once the windows closed, §5 step 3), else the open ones.
pub fn matched(seen: &[Seen], file: &[FileWindow], closed_first: bool) -> Vec<Option<usize>> {
    let mut taken = vec![false; file.len()];
    seen.iter()
        .map(|s| {
            let pick = file
                .iter()
                .enumerate()
                .filter(|(i, w)| {
                    !taken.get(*i).copied().unwrap_or(true) && w.shown_title().trim() == s.title
                })
                .min_by_key(|(i, w)| {
                    let distance = w.size.map_or(0, |(width, height)| {
                        (width - s.size.0).abs() + (height - s.size.1).abs()
                    });
                    (w.closed != closed_first, distance, *i)
                })
                .map(|(i, _)| i);
            if let Some(slot) = pick.and_then(|i| taken.get_mut(i)) {
                *slot = true;
            }
            pick
        })
        .collect()
}

// --- Profiles --------------------------------------------------------------------------------

/// The paths `profiles.ini` in `root` names.
fn profiles_in(root: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(root.join("profiles.ini")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut path: Option<String> = None;
    let mut relative = true;
    let flush = |path: &mut Option<String>, relative: bool, out: &mut Vec<PathBuf>| {
        if let Some(p) = path.take() {
            let p = if relative {
                root.join(p)
            } else {
                PathBuf::from(p)
            };
            if !out.contains(&p) {
                out.push(p);
            }
        }
    };
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            flush(&mut path, relative, &mut out);
            relative = true;
        } else if let Some(p) = line.strip_prefix("Path=") {
            path = Some(p.to_owned());
        } else if let Some(r) = line.strip_prefix("IsRelative=") {
            relative = r != "0";
        }
    }
    flush(&mut path, relative, &mut out);
    out
}

/// Whether profile `dir`'s `lock` names a process alive here
/// (`IP:+PID`).
fn locked_here(dir: &Path) -> bool {
    let Ok(target) = std::fs::read_link(dir.join("lock")) else {
        return false;
    };
    target
        .to_string_lossy()
        .rsplit_once(":+")
        .and_then(|(_, pid)| pid.parse::<u32>().ok())
        .is_some_and(|pid| Path::new("/proc").join(pid.to_string()).exists())
}

/// Firefox's session files in profile `dir`, the newest first.
fn firefox_files(dir: &Path) -> [PathBuf; 2] {
    [
        dir.join("sessionstore-backups/recovery.jsonlz4"),
        dir.join("sessionstore.jsonlz4"),
    ]
}

fn modified(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Firefox's profile running: the one locked here, else the one whose
/// session was written last.
pub fn firefox_profile(places: &Places) -> Option<PathBuf> {
    let roots = [
        places.home.join(".mozilla/firefox"),
        places.config.join("mozilla/firefox"),
        places
            .home
            .join(".var/app/org.mozilla.firefox/.mozilla/firefox"),
        places.home.join("snap/firefox/common/.mozilla/firefox"),
    ];
    let profiles: Vec<PathBuf> = roots.iter().flat_map(|r| profiles_in(r)).collect();
    if let Some(running) = profiles.iter().find(|p| locked_here(p)) {
        return Some(running.clone());
    }
    profiles
        .into_iter()
        .filter_map(|p| {
            let newest = firefox_files(&p).iter().filter_map(|f| modified(f)).max()?;
            Some((newest, p))
        })
        .max_by_key(|(at, _)| *at)
        .map(|(_, p)| p)
}

/// A Chromium's profile last used, in its `folder` of the configuration's
/// home: `Local State`'s `profile.last_used`, else `Default`.
pub fn chromium_profile(places: &Places, folder: &str) -> PathBuf {
    let root = places.config.join(folder);
    let last = std::fs::read(root.join("Local State"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|v| {
            v.pointer("/profile/last_used")
                .and_then(Value::as_str)
                .filter(|p| !p.is_empty() && !p.contains('/'))
                .map(str::to_owned)
        });
    root.join(last.unwrap_or_else(|| "Default".to_owned()))
}

/// A Chromium profile's newest `Sessions/Session_*`.
fn chromium_file(profile: &Path) -> Option<PathBuf> {
    std::fs::read_dir(profile.join("Sessions"))
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("Session_"))
        .map(|e| e.path())
        .max_by_key(|p| (modified(p), p.clone()))
}

/// Whether a browser reopens its own windows as it starts (§8.3, §13 D):
/// Firefox's `browser.startup.page` 3, Chromium's
/// `session.restore_on_startup` 1.
pub fn restores_itself(browser: Browser, places: &Places) -> bool {
    match browser.kind {
        Kind::Firefox => firefox_profile(places)
            .and_then(|p| std::fs::read_to_string(p.join("prefs.js")).ok())
            .is_some_and(|prefs| {
                prefs.lines().any(|l| {
                    let l: String = l.chars().filter(|c| !c.is_whitespace()).collect();
                    l == "user_pref(\"browser.startup.page\",3);"
                })
            }),
        Kind::Chromium => {
            std::fs::read(chromium_profile(places, browser.folder).join("Preferences"))
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                .and_then(|v| {
                    v.pointer("/session/restore_on_startup")
                        .and_then(Value::as_i64)
                })
                == Some(1)
        }
    }
}

/// Reads the browsers' files for windows, off the loop; remembers each
/// browser's profile, read again once its windows closed (Firefox's lock
/// goes as it quits).
#[derive(Debug, Default)]
pub struct Reader {
    profiles: HashMap<Browser, PathBuf>,
}

/// A browser window handed to the reader: its browser, its title, its
/// size.
#[derive(Debug, Clone)]
pub struct Asked {
    pub browser: Browser,
    pub title: String,
    pub size: (i32, i32),
}

impl Reader {
    fn windows_of(&mut self, browser: Browser, places: &Places) -> Option<Vec<FileWindow>> {
        let profile = match browser.kind {
            Kind::Firefox => firefox_profile(places),
            Kind::Chromium => Some(chromium_profile(places, browser.folder)),
        }
        .or_else(|| self.profiles.get(&browser).cloned())?;
        self.profiles.insert(browser, profile.clone());
        match browser.kind {
            Kind::Firefox => firefox_files(&profile).iter().find_map(|f| {
                let bytes = std::fs::read(f).ok()?;
                firefox_session(&mozlz4(&bytes)?)
            }),
            Kind::Chromium => {
                let bytes = std::fs::read(chromium_file(&profile)?).ok()?;
                snss(&bytes)
            }
        }
    }

    /// What each of `asked` holds: its tabs, empty when it was not found
    /// in its browser's file; none for a private window.
    pub fn read(
        &mut self,
        places: &Places,
        asked: &[Asked],
        closed_first: bool,
    ) -> Vec<Option<SavedBrowser>> {
        let mut out: Vec<Option<SavedBrowser>> = asked
            .iter()
            .map(|a| {
                Some(SavedBrowser {
                    kind: a.browser.kind,
                    ..SavedBrowser::default()
                })
            })
            .collect();
        let mut browsers: Vec<Browser> = asked.iter().map(|a| a.browser).collect();
        browsers.dedup();
        browsers.sort_by_key(|b| (b.kind == Kind::Chromium, b.folder));
        browsers.dedup();
        for browser in browsers {
            let theirs: Vec<(usize, Seen)> = asked
                .iter()
                .enumerate()
                .filter(|(_, a)| a.browser == browser)
                .filter_map(|(i, a)| match shown(&a.title) {
                    Shown::Private => {
                        if let Some(slot) = out.get_mut(i) {
                            *slot = None;
                        }
                        None
                    }
                    Shown::Page(title) => Some((
                        i,
                        Seen {
                            title,
                            size: a.size,
                        },
                    )),
                })
                .collect();
            if theirs.is_empty() {
                continue;
            }
            let Some(file) = self.windows_of(browser, places) else {
                tracing::debug!(kind = ?browser.kind, "no browser session read");
                continue;
            };
            let seen: Vec<Seen> = theirs.iter().map(|(_, s)| s.clone()).collect();
            let found = matched(&seen, &file, closed_first);
            let mut tabs = 0;
            for ((i, _), m) in theirs.iter().zip(found) {
                if let (Some(slot), Some(w)) = (out.get_mut(*i), m.and_then(|m| file.get(m))) {
                    tabs += w.tabs.len();
                    *slot = Some(w.saved(browser.kind));
                }
            }
            // How many, never what.
            tracing::debug!(kind = ?browser.kind, windows = theirs.len(), tabs, "browsers' tabs read");
        }
        out
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An LZ4 block of literals only: valid, if not small.
    pub fn lz4_literals(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let n = data.len();
        if n >= 15 {
            out.push(0xF0);
            let mut rest = n - 15;
            while rest >= 255 {
                out.push(255);
                rest -= 255;
            }
            out.push(rest as u8);
        } else {
            out.push((n as u8) << 4);
        }
        out.extend_from_slice(data);
        out
    }

    /// mozLz4 of `data`.
    pub fn mozlz4_of(data: &[u8]) -> Vec<u8> {
        let mut out = b"mozLz40\0".to_vec();
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend(lz4_literals(data));
        out
    }

    #[test]
    fn mozlz4_reads_literals_and_overlapping_matches() {
        let text = b"{\"windows\":[]} and a long enough line to need more than fifteen".to_vec();
        assert_eq!(mozlz4(&mozlz4_of(&text)), Some(text));
        // "abcabcabcabc": three literals, then a match 3 back, 9 long.
        let block = [0x35, b'a', b'b', b'c', 3, 0];
        assert_eq!(lz4_block(&block, 12).as_deref(), Some(&b"abcabcabcabc"[..]));
        assert_eq!(lz4_block(&block, 11), None, "the size it says");
        assert_eq!(lz4_block(&[0x15, b'a', 9, 0], 6), None, "an offset too far");
        assert_eq!(mozlz4(b"mozLz40\0\x01"), None);
        assert_eq!(mozlz4(b"notlz4\0\0\0\0\0\0"), None);
    }

    #[test]
    fn firefoxs_session_its_tabs_in_order_and_never_a_private_window() {
        let json = r#"{"version":["sessionrestore",1],"windows":[
            {"tabs":[
              {"entries":[{"url":"https://a.example/1","title":"Old"},{"url":"http://localhost:4321/","title":"hocus-site · Accueil"}],"index":2,"pinned":true},
              {"entries":[{"url":"https://docs.rs/iced","title":"iced"}],"index":1}],
             "selected":1,"width":1180,"height":1584},
            {"isPrivate":true,"tabs":[{"entries":[{"url":"https://secret.example/","title":"Secret"}],"index":1}],"selected":1}
          ],
          "_closedWindows":[{"tabs":[{"entries":[{"url":"https://github.com/x","title":"PR"}],"index":1}],"selected":1}]}"#;
        let windows = firefox_session(json.as_bytes()).expect("read");
        assert_eq!(windows.len(), 2);
        let first = &windows[0];
        assert_eq!(first.tabs.len(), 2);
        assert_eq!(first.tabs[0].url, "http://localhost:4321/");
        assert_eq!(first.tabs[0].title, "hocus-site · Accueil");
        assert!(first.tabs[0].pinned && !first.tabs[1].pinned);
        assert_eq!(
            (first.selected, first.size, first.closed),
            (0, Some((1180, 1584)), false)
        );
        assert!(windows[1].closed);
        assert!(
            !windows
                .iter()
                .flat_map(|w| &w.tabs)
                .any(|t| t.title == "Secret")
        );
        let saved = first.saved(Kind::Firefox);
        assert_eq!(
            (saved.selected, saved.shown_title()),
            (1, Some("hocus-site · Accueil"))
        );
        assert_eq!(format!("{:?}", first.tabs[0]), "Tab", "never said");
    }

    /// An SNSS file, as Chromium writes one: its commands' payloads laid
    /// out as their structures and pickles are.
    pub fn snss_file() -> Vec<u8> {
        fn cmd(out: &mut Vec<u8>, id: u8, payload: &[u8]) {
            out.extend_from_slice(&((payload.len() + 1) as u16).to_le_bytes());
            out.push(id);
            out.extend_from_slice(payload);
        }
        fn ints(v: &[i32]) -> Vec<u8> {
            v.iter().flat_map(|i| i.to_le_bytes()).collect()
        }
        fn pad(p: &mut Vec<u8>) {
            while !p.len().is_multiple_of(4) {
                p.push(0);
            }
        }
        fn navigation(tab: i32, index: i32, url: &str, title: &str) -> Vec<u8> {
            let mut p = ints(&[tab, index, url.len() as i32]);
            p.extend_from_slice(url.as_bytes());
            pad(&mut p);
            let units: Vec<u16> = title.encode_utf16().collect();
            p.extend_from_slice(&(units.len() as i32).to_le_bytes());
            p.extend(units.iter().flat_map(|u| u.to_le_bytes()));
            pad(&mut p);
            // The rest of an entry: its page state, transition… not read.
            p.extend_from_slice(&ints(&[0, 0, 0]));
            let mut out = (p.len() as u32).to_le_bytes().to_vec();
            out.extend(p);
            out
        }
        let mut out = b"SNSS".to_vec();
        out.extend_from_slice(&3i32.to_le_bytes());
        // Window 1, normal, 1200 × 800, two tabs, the second shown.
        cmd(&mut out, command::SET_WINDOW_TYPE, &ints(&[1, 0]));
        cmd(
            &mut out,
            command::WINDOW_BOUNDS,
            &ints(&[1, 0, 0, 1200, 800, 1]),
        );
        for (tab, index) in [(10, 0), (11, 1), (12, 2)] {
            cmd(&mut out, command::SET_TAB_WINDOW, &ints(&[1, tab]));
            cmd(
                &mut out,
                command::SET_TAB_INDEX_IN_WINDOW,
                &ints(&[tab, index]),
            );
        }
        cmd(
            &mut out,
            command::UPDATE_TAB_NAVIGATION,
            &navigation(10, 0, "https://github.com/a/b/pull/1", "Pull request #1"),
        );
        cmd(
            &mut out,
            command::UPDATE_TAB_NAVIGATION,
            &navigation(11, 0, "https://example.org/", "Example"),
        );
        cmd(
            &mut out,
            command::UPDATE_TAB_NAVIGATION,
            &navigation(11, 1, "https://docs.rs/smithay", "smithay · docs.rs"),
        );
        cmd(
            &mut out,
            command::SELECTED_NAVIGATION_INDEX,
            &ints(&[11, 1]),
        );
        cmd(
            &mut out,
            command::UPDATE_TAB_NAVIGATION,
            &navigation(12, 0, "https://closed.example/", "Closed"),
        );
        cmd(&mut out, command::TAB_CLOSED, &ints(&[12, 0, 0, 0]));
        cmd(&mut out, command::PINNED_STATE, &[10, 0, 0, 0, 1, 0, 0, 0]);
        cmd(&mut out, command::SELECTED_TAB_IN_INDEX, &ints(&[1, 1]));
        // Window 2, a popup: not read.
        cmd(&mut out, command::SET_WINDOW_TYPE, &ints(&[2, 1]));
        cmd(&mut out, command::SET_TAB_WINDOW, &ints(&[2, 20]));
        cmd(
            &mut out,
            command::UPDATE_TAB_NAVIGATION,
            &navigation(20, 0, "https://popup.example/", "Popup"),
        );
        // Window 3, closed with its tab.
        cmd(&mut out, command::SET_WINDOW_TYPE, &ints(&[3, 0]));
        cmd(&mut out, command::SET_TAB_WINDOW, &ints(&[3, 30]));
        cmd(
            &mut out,
            command::UPDATE_TAB_NAVIGATION,
            &navigation(30, 0, "https://gone.example/", "Gone"),
        );
        cmd(&mut out, command::WINDOW_CLOSED, &ints(&[3, 0, 0, 0]));
        // A command this reader does not know, and a last one cut short.
        cmd(&mut out, 21, &ints(&[11, 0, 0]));
        out.extend_from_slice(&[40, 0, command::TAB_CLOSED, 1]);
        out
    }

    /// The committed file, written again with `HOCUS_SNSS_WRITE=1`.
    #[test]
    fn the_committed_snss_file_is_the_one_built_here() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/Session_13400000000000000"
        );
        if std::env::var_os("HOCUS_SNSS_WRITE").is_some() {
            std::fs::write(path, snss_file()).expect("written");
        }
        let committed = std::fs::read(path).expect("committed");
        assert_eq!(
            &committed[..],
            &snss_file()[..],
            "regenerate it: see this test"
        );
    }

    #[test]
    fn snss_commands_make_windows_their_tabs_and_the_one_shown() {
        let windows = snss(&snss_file()).expect("read");
        assert_eq!(windows.len(), 2, "{windows:?}");
        let first = &windows[0];
        let urls: Vec<&str> = first.tabs.iter().map(|t| t.url.as_str()).collect();
        assert_eq!(
            urls,
            ["https://github.com/a/b/pull/1", "https://docs.rs/smithay"]
        );
        assert_eq!(first.tabs[1].title, "smithay · docs.rs");
        assert!(first.tabs[0].pinned);
        assert_eq!(
            (first.selected, first.size, first.closed),
            (1, Some((1200, 800)), false)
        );
        assert!(windows[1].closed);
        assert_eq!(windows[1].tabs[0].title, "Gone");
        assert_eq!(snss(b"SNSS\x02\0\0\0"), None, "encrypted");
        assert_eq!(snss(b"XXXX\x01\0\0\0"), None);
    }

    #[test]
    fn a_window_is_matched_by_its_title_then_its_size_never_a_private_one() {
        assert_eq!(
            shown("hocus-site · Accueil — Mozilla Firefox"),
            Shown::Page("hocus-site · Accueil".into())
        );
        assert_eq!(shown("a - b - Chromium"), Shown::Page("a - b".into()));
        assert_eq!(shown("Mozilla Firefox"), Shown::Page(String::new()));
        assert_eq!(
            shown("Secret — Mozilla Firefox Private Browsing"),
            Shown::Private
        );
        assert_eq!(shown("Secret - Chromium (Incognito)"), Shown::Private);
        assert_eq!(shown("Notes - plain"), Shown::Page("Notes - plain".into()));
        let window = |title: &str, size: (i32, i32), closed: bool| FileWindow {
            tabs: vec![Tab {
                url: "https://x/".into(),
                title: title.into(),
                pinned: false,
            }],
            selected: 0,
            size: Some(size),
            closed,
        };
        let file = [
            window("Docs", (800, 600), false),
            window("Site", (1200, 900), false),
            window("Docs", (1600, 1000), false),
            window("Site", (1200, 900), true),
        ];
        let seen = |title: &str, size| Seen {
            title: title.into(),
            size,
        };
        assert_eq!(
            matched(
                &[
                    seen("Docs", (1590, 1000)),
                    seen("Docs", (100, 100)),
                    seen("Other", (1, 1))
                ],
                &file,
                false
            ),
            [Some(2), Some(0), None]
        );
        assert_eq!(
            matched(&[seen("Site", (1200, 900))], &file, false),
            [Some(1)]
        );
        assert_eq!(
            matched(&[seen("Site", (1200, 900))], &file, true),
            [Some(3)]
        );
    }

    #[test]
    fn addresses_open_in_one_new_window_never_as_options() {
        let saved = SavedBrowser {
            kind: Kind::Firefox,
            selected: 1,
            tabs: [
                "https://a/",
                "-new-instance",
                "about:config",
                "javascript alert",
            ]
            .iter()
            .map(|u| Tab {
                url: (*u).into(),
                ..Tab::default()
            })
            .collect(),
        };
        let addresses = saved.addresses();
        assert_eq!(addresses, ["https://a/", "about:config"]);
        assert_eq!(new_window_args(Kind::Firefox, &addresses), addresses);
        assert_eq!(
            new_window_args(Kind::Firefox, &addresses[..1]),
            ["--new-window", "https://a/"]
        );
        assert_eq!(
            new_window_args(Kind::Chromium, &addresses),
            ["--new-window", "https://a/", "about:config"]
        );
        assert_eq!(host_of("https://user@github.com:443/a?b"), "github.com:443");
        assert_eq!(host_of("http://localhost:4321/"), "localhost:4321");
        assert_eq!(host_of("about:newtab"), "about");
        assert_eq!(host_of("file:///home/a"), "file");
        assert!(of_app("org.mozilla.firefox").is_some_and(|b| b.kind == Kind::Firefox));
        assert_eq!(
            of_app("brave-browser").map(|b| b.folder),
            Some("BraveSoftware/Brave-Browser")
        );
        assert!(of_app("foot").is_none());
    }

    #[test]
    fn the_running_profile_is_the_locked_one_else_the_last_written() -> std::io::Result<()> {
        let home = tempfile::tempdir()?;
        let root = home.path().join(".mozilla/firefox");
        for p in ["a.default", "b.work"] {
            std::fs::create_dir_all(root.join(p).join("sessionstore-backups"))?;
        }
        std::fs::write(
            root.join("profiles.ini"),
            "[Profile0]\nName=a\nIsRelative=1\nPath=a.default\n\n[Profile1]\nName=b\nIsRelative=1\nPath=b.work\n",
        )?;
        let places = Places {
            home: home.path().to_path_buf(),
            config: home.path().join(".config"),
        };
        std::fs::write(
            root.join("a.default/sessionstore-backups/recovery.jsonlz4"),
            b"x",
        )?;
        assert_eq!(firefox_profile(&places), Some(root.join("a.default")));
        std::os::unix::fs::symlink(
            format!("127.0.1.1:+{}", std::process::id()),
            root.join("b.work/lock"),
        )?;
        assert_eq!(firefox_profile(&places), Some(root.join("b.work")));
        std::fs::write(
            root.join("b.work/prefs.js"),
            "user_pref(\"browser.startup.page\", 3);\n",
        )?;
        let firefox = of_app("firefox").expect("firefox");
        assert!(restores_itself(firefox, &places));
        let chromium = of_app("chromium").expect("chromium");
        assert!(!restores_itself(chromium, &places));
        Ok(())
    }
}
