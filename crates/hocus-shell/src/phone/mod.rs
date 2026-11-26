//! The phone, through KDE Connect (bar.md "The phone", #957): a row per
//! paired phone or tablet in the connections, under Bluetooth's, its
//! popover, the files sent to it and from it.
//!
//! KDE Connect's daemon is read on a thread of its own (`hocus-phone`),
//! on the session bus the compositor names, never on the loop that draws:
//! it says what changes ([`hocus_phone::News`]) and takes what is asked
//! ([`Ask`]) through a channel. What it says is kept here for the rows
//! ([`rows`]) and the popover, each device known by a number of its own
//! for this program's life ([`Row::key`]), and the rows are woken through
//! the compositor only when what they show changes. A file the phone sent
//! makes a bubble of the desktop's own ([`received`]); one dropped on a
//! row goes to that phone ([`drag`]).

pub mod drag;
pub mod draw;

pub use drag::dropping;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::ToCompositor;
pub use hocus_phone::{Ask, Battery, Device, Kind, Phones, Transfer};
use hocus_words::shell::phone as txt;

use crate::{
    clock::time::Lang,
    notifications::{self, Input, note},
};

/// What changed, for whoever shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    /// What the rows say.
    pub row: bool,
    /// Which rows there are.
    pub shown: bool,
}

type Waker = Box<dyn FnMut(Change) -> bool + Send>;

/// A device's row: its number here, and the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: u32,
    pub device: Device,
}

struct Service {
    phones: Mutex<Phones>,
    /// Each device's number, by KDE Connect's id, kept for the program's
    /// life.
    keys: Mutex<HashMap<String, u32>>,
    waker: Mutex<Option<Waker>>,
    asks: hocus_phone::Asks,
}

static SERVICE: OnceLock<Service> = OnceLock::new();

/// The files received in a row, by device: one bubble for them.
static RECEIVED: Mutex<Option<Received>> = Mutex::new(None);

/// Files that come within this of the one before are of the same share.
const IN_A_ROW: Duration = Duration::from_secs(10);

/// Starts KDE Connect's thread, once, on the bus the compositor names.
pub fn start() {
    let (asks, incoming) = hocus_phone::channel();
    let service = Service {
        phones: Mutex::new(Phones::default()),
        keys: Mutex::new(HashMap::new()),
        waker: Mutex::new(None),
        asks,
    };
    if SERVICE.set(service).is_ok() {
        hocus_phone::start(crate::day::session_bus(), incoming, news);
    }
}

/// KDE Connect as last read.
pub fn snapshot() -> Phones {
    SERVICE.get().map_or_else(Phones::default, |s| {
        s.phones
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    })
}

/// Device `id`'s number.
fn key_of(service: &Service, id: &str) -> u32 {
    let mut keys = service.keys.lock().unwrap_or_else(PoisonError::into_inner);
    let next = u32::try_from(keys.len())
        .unwrap_or(u32::MAX)
        .saturating_add(1);
    *keys.entry(id.to_owned()).or_insert(next)
}

/// The rows, in their order.
pub fn rows() -> Vec<Row> {
    let Some(service) = SERVICE.get() else {
        return Vec::new();
    };
    snapshot()
        .devices
        .into_iter()
        .map(|device| Row {
            key: key_of(service, &device.id),
            device,
        })
        .collect()
}

/// The row of number `key`.
pub fn row(key: u32) -> Option<Row> {
    rows().into_iter().find(|r| r.key == key)
}

/// Whether a desktop portal chooses files: "Envoyer un fichier…" shows.
pub fn portal() -> bool {
    snapshot().portal
}

pub fn ask(ask: Ask) {
    let sent = SERVICE
        .get()
        .is_some_and(|service| service.asks.send(ask).is_ok());
    if !sent {
        tracing::warn!("KDE Connect's thread has stopped: dropped");
    }
}

/// From now on, a change calls `wake` (until it returns `false`); it is
/// called once now, for what changed before.
pub fn on_change(wake: impl FnMut(Change) -> bool + Send + 'static) {
    if let Some(service) = SERVICE.get() {
        let mut waker = service.waker.lock().unwrap_or_else(PoisonError::into_inner);
        let mut wake: Waker = Box::new(wake);
        let alive = wake(Change {
            row: true,
            shown: true,
        });
        *waker = alive.then_some(wake);
    }
}

/// The keys of the rows shown, for the popovers to follow.
pub fn keys() -> Vec<u32> {
    rows().into_iter().map(|r| r.key).collect()
}

/// KDE Connect's thread says `news`.
fn news(news: hocus_phone::News) {
    match news {
        hocus_phone::News::Phones(phones) => kept(phones),
        hocus_phone::News::Received { device, kind, url } => received(&device, kind, &url),
    }
}

/// KDE Connect as it is now: kept, and the rows woken if they change.
fn kept(phones: Phones) {
    let Some(service) = SERVICE.get() else {
        return;
    };
    let change = {
        let mut kept = service
            .phones
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let ids = |p: &Phones| p.devices.iter().map(|d| d.id.clone()).collect::<Vec<_>>();
        let change = Change {
            row: kept.devices != phones.devices,
            shown: ids(&kept) != ids(&phones),
        };
        if *kept == phones {
            return;
        }
        *kept = phones;
        change
    };
    tracing::debug!(
        shown = change.shown,
        rows = %rows()
            .iter()
            .map(|r| logged_row(&r.device, Lang::of_messages()))
            .collect::<Vec<_>>()
            .join(" | "),
        "the phones' rows"
    );
    let mut waker = service.waker.lock().unwrap_or_else(PoisonError::into_inner);
    if waker.as_mut().is_some_and(|wake| !wake(change)) {
        *waker = None;
    }
}

/// A row as the log says it: as a screen reader does, its transfer after.
fn logged_row(device: &Device, lang: Lang) -> String {
    let mut said = said_row(device, lang);
    if let Some(t) = &device.transfer {
        if t.failed {
            said.push_str(&format!(" [{} stopped]", transfer_name(t, lang)));
        } else {
            said.push_str(&format!(
                " [{} {:.0} %]",
                transfer_name(t, lang),
                t.fraction() * 100.0
            ));
        }
    }
    said
}

/// What a screen reader says of `device`'s row: "Pixel 8, téléphone,
/// batterie 82 %, en charge", or "…, hors de portée".
pub fn said_row(device: &Device, lang: Lang) -> String {
    let kind = match device.kind {
        Kind::Phone => txt::PHONE_SAID,
        Kind::Tablet => txt::TABLET_SAID,
    }
    .get(lang);
    let mut said = format!("{}, {kind}", device.name);
    if !device.reachable {
        said.push_str(", ");
        said.push_str(txt::OUT_OF_REACH.get(lang));
        return said;
    }
    if let Some(battery) = device.battery {
        said.push_str(", ");
        said.push_str(&txt::battery_of(lang, battery.charge));
        if battery.charging {
            said.push_str(", ");
            said.push_str(txt::CHARGING_SAID.get(lang));
        }
    }
    said
}

/// What a device is, said as a title: "Téléphone", "Tablette".
pub fn kind_title(kind: Kind, lang: Lang) -> &'static str {
    match kind {
        Kind::Phone => txt::PHONE,
        Kind::Tablet => txt::TABLET,
    }
    .get(lang)
}

/// "4,2 Mo sur 10": a transfer's bytes, the unit said once when both
/// have it.
pub fn done_of(lang: Lang, done: u64, total: u64) -> String {
    let (done, total) = crate::removable::amounts_of(lang, done, total);
    txt::done_of(lang, txt::DoneOf { done, total })
}

/// What a transfer names: its file, or "3 fichiers".
pub fn transfer_name(transfer: &Transfer, lang: Lang) -> String {
    if transfer.files > 1 {
        txt::files(lang, u64::from(transfer.files))
    } else {
        transfer.file.clone()
    }
}

/// `paths` sent to device `id`, as `file://` URLs.
pub fn send(id: &str, paths: &[PathBuf]) {
    let urls: Vec<String> = paths.iter().map(|p| file_url(p)).collect();
    if urls.is_empty() {
        return;
    }
    tracing::debug!(files = urls.len(), "files sent to a phone");
    ask(Ask::Share {
        device: id.to_owned(),
        urls,
    });
}

/// A path as a `file://` URL, what URLs may not hold escaped.
pub fn file_url(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut url = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            url.push(char::from(b));
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    url
}

/// The path of a `file://` URL, its escapes read.
pub fn path_of(url: &str) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let mut bytes = Vec::with_capacity(rest.len());
    let mut chars = rest.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hex = [chars.next()?, chars.next()?];
            let hex = std::str::from_utf8(&hex).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
        } else {
            bytes.push(b);
        }
    }
    let path = PathBuf::from(std::ffi::OsString::from_vec(bytes));
    path.is_absolute().then_some(path)
}

// --- A file from the phone ------------------------------------------------------

/// The files received in a row from one device, and its bubble.
#[derive(Debug, Clone)]
struct Received {
    device: String,
    files: Vec<PathBuf>,
    at: Instant,
}

/// The ids of the phone's own bubbles (a file received), among the
/// desktop's own.
pub fn received_id() -> u32 {
    notifications::phone_id(0)
}

/// Device `device` sent `url`: a file saved makes its bubble (several in
/// a row, one bubble); a link KDE Connect opened itself, nothing.
fn received(device: &str, kind: Kind, url: &str) {
    let Some(path) = path_of(url) else {
        tracing::debug!("a link from a phone: KDE Connect opens it");
        return;
    };
    let lang = Lang::of_messages();
    let files = {
        let mut last = RECEIVED.lock().unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        match last.as_mut() {
            Some(r) if r.device == device && now.duration_since(r.at) < IN_A_ROW => {
                r.files.push(path);
                r.at = now;
                r.files.clone()
            }
            _ => {
                *last = Some(Received {
                    device: device.to_owned(),
                    files: vec![path],
                    at: now,
                });
                last.as_ref().map(|r| r.files.clone()).unwrap_or_default()
            }
        }
    };
    let Some(first) = files.last() else {
        return;
    };
    let folder = first.parent().map(Path::to_path_buf).unwrap_or_default();
    let summary = if files.len() > 1 {
        txt::files(lang, files.len() as u64)
    } else {
        first
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let body = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.display().to_string());
    tracing::info!(files = files.len(), "a file received from a phone");
    let icon = match kind {
        Kind::Phone => "phone",
        Kind::Tablet => "tablet",
    };
    let note = note::Note {
        id: received_id(),
        app: device.to_owned(),
        entry: None,
        icon: notifications::themed_icon(icon),
        summary,
        body,
        actions: vec![
            ("open".to_owned(), txt::OPEN.get(lang).to_owned()),
            ("folder".to_owned(), txt::SHOW_FOLDER.get(lang).to_owned()),
        ],
        default: true,
        urgency: note::Urgency::Normal,
        timeout: note::Timeout::After(note::LIFE),
        progress: None,
        sound: None,
        resident: false,
        pid: 0,
        agent: false,
        conversation: None,
        icon_from: note::IconFrom {
            app_icon: icon.to_owned(),
            image_path: None,
        },
        at: notifications::wall_ms(),
        number: 0,
        transient: false,
        ancestry: Vec::new(),
        phone: None,
    };
    notifications::input(Input::Notify(Box::new(note)));
}

/// The received bubble's action `key`: the file opened (the folder for
/// several), or its folder.
pub fn from_bubble(key: &str) {
    // Acted on: what comes next is another share's.
    let files = RECEIVED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .map(|r| r.files)
        .unwrap_or_default();
    let Some(last) = files.last() else {
        return;
    };
    let folder = last.parent().map(Path::to_path_buf).unwrap_or_default();
    match key {
        "default" | "open" if files.len() == 1 => open(last),
        "default" | "open" | "folder" => open(&folder),
        _ => {}
    }
}

/// `at` opened with `xdg-open`, started by the compositor as the
/// applications are.
fn open(at: &Path) {
    let Some(opener) = crate::launcher::program("xdg-open") else {
        tracing::warn!("no xdg-open in PATH: the file does not open");
        return;
    };
    let launch = hocus_link::Launch {
        id: "xdg-open".to_owned(),
        terminal: false,
        dir: String::new(),
        argv: vec![opener, at.display().to_string()],
    };
    if launch.fits() {
        tracing::debug!("a file from a phone opens");
        crate::popover::tell(ToCompositor::Launch(launch));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(reachable: bool, battery: Option<(u8, bool)>) -> Device {
        Device {
            id: "a1".to_owned(),
            name: "Pixel 8".to_owned(),
            kind: Kind::Phone,
            reachable,
            battery: battery.map(|(charge, charging)| Battery { charge, charging }),
            ring: true,
            share: true,
            came: 1,
            transfer: None,
        }
    }

    #[test]
    fn a_screen_reader_hears_its_name_kind_and_battery() {
        let fr = Lang::French;
        assert_eq!(
            said_row(&device(true, Some((82, false))), fr),
            "Pixel 8, téléphone, batterie 82 %"
        );
        assert_eq!(
            said_row(&device(true, Some((82, true))), Lang::English),
            "Pixel 8, phone, battery 82 %, charging"
        );
        assert_eq!(
            said_row(&device(false, None), fr),
            "Pixel 8, téléphone, hors de portée"
        );
    }

    #[test]
    fn a_path_and_its_url() {
        let path = PathBuf::from("/tmp/a b/rapport été.pdf");
        let url = file_url(&path);
        assert_eq!(url, "file:///tmp/a%20b/rapport%20%C3%A9t%C3%A9.pdf");
        assert_eq!(path_of(&url), Some(path));
        assert_eq!(path_of("https://example.org/a"), None);
    }

    #[test]
    fn a_transfers_words() {
        let fr = Lang::French;
        assert_eq!(done_of(fr, 4_200_000, 10_000_000), "4,2 Mo sur 10");
        let mut t = Transfer {
            view: 1,
            file: "rapport.pdf".to_owned(),
            files: 1,
            processed: 0,
            total: 0,
            failed: false,
        };
        assert_eq!(transfer_name(&t, fr), "rapport.pdf");
        t.files = 3;
        assert_eq!(transfer_name(&t, fr), "3 fichiers");
    }
}
