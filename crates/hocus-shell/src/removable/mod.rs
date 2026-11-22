//! Removable drives (bar.md "Removable drives", #898): a bubble when a USB
//! stick, an external disk or an SD card is plugged in, a row in the
//! connections while one is mounted, and ejecting it.
//!
//! UDisks2 is read on a thread of its own (`hocus-removable`), never on
//! the loop that draws: it says what changes ([`News`]) and takes what is
//! asked ([`Ask`]) through a channel. What it says is handled on that
//! thread: the drives kept for the rows ([`mounted`]), the bubbles handed
//! to the notifications' stack, the file manager started through the
//! compositor (`xdg-open`, as the captures' folder is). The rows draw on
//! the connections' surface ([`draw`]), woken through the compositor only
//! when what they show changes.
//!
//! What a drive plugged in does is `[removable] on-plug`, which the
//! compositor tells ([`set_on_plug`]): a bubble offering "Ouvrir" and
//! "Éjecter" (the default), the drive mounted and opened at once, or
//! nothing. An encrypted drive still locked is opened in the file manager,
//! which asks its passphrase.

pub mod draw;

use std::{
    path::Path,
    sync::{Mutex, OnceLock, PoisonError},
};

use hocus_link::{OnPlug, ToCompositor};
pub use hocus_removable::{Ask, Doing, Drive, Kind, News};
use hocus_words::shell::removable as txt;

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

struct Service {
    drives: Mutex<Vec<Drive>>,
    waker: Mutex<Option<Waker>>,
    asks: hocus_removable::Asks,
}

static SERVICE: OnceLock<Service> = OnceLock::new();
static ON_PLUG: Mutex<OnPlug> = Mutex::new(OnPlug::Ask);

/// The bubbles' icon, from the icon theme.
const ICON: &str = "drive-removable-media";

/// Starts UDisks2's thread, once.
pub fn start() {
    let (asks, incoming) = hocus_removable::channel();
    let service = Service {
        drives: Mutex::new(Vec::new()),
        waker: Mutex::new(None),
        asks,
    };
    if SERVICE.set(service).is_ok() {
        hocus_removable::start(incoming, news);
    }
}

/// The configuration says what a drive plugged in does.
pub fn set_on_plug(on_plug: OnPlug) {
    tracing::debug!(?on_plug, "a drive plugged in");
    *ON_PLUG.lock().unwrap_or_else(PoisonError::into_inner) = on_plug;
}

/// The drives mounted, in the order they came: the bar's rows.
pub fn mounted() -> Vec<Drive> {
    SERVICE.get().map_or_else(Vec::new, |s| {
        s.drives
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|d| d.mounted())
            .cloned()
            .collect()
    })
}

pub fn ask(ask: Ask) {
    let sent = SERVICE
        .get()
        .is_some_and(|service| service.asks.send(ask).is_ok());
    if !sent {
        tracing::warn!("UDisks2's thread has stopped: dropped");
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

/// UDisks2's thread says `news`.
fn news(news: News) {
    let lang = Lang::of_messages();
    match news {
        News::Drives(drives) => kept(drives),
        News::Plugged(drive) => {
            let on_plug = *ON_PLUG.lock().unwrap_or_else(PoisonError::into_inner);
            tracing::info!(serial = drive.serial, ?on_plug, "a drive plugged in");
            match on_plug {
                OnPlug::Ask => say(plugged(&drive, lang)),
                OnPlug::Open => ask(Ask::Open(drive.serial)),
                OnPlug::Nothing => {}
            }
        }
        News::Opened { at, .. } => open_in_files(&at),
        // Its passphrase: the file manager asks it, from its sidebar.
        News::Locked(drive) => {
            tracing::info!(
                serial = drive.serial,
                "an encrypted drive: the file manager asks"
            );
            if let Some(home) = std::env::var_os("HOME") {
                open_in_files(Path::new(&home));
            }
        }
        News::Ejected(drive) => say(note(
            &drive,
            txt::you_can_remove(lang, said_name(&drive, lang)),
            String::new(),
            Vec::new(),
        )),
        News::Busy(drive) => {
            let summary = match drive.kind {
                Kind::Disk => txt::still_in_use_m(lang, said_name(&drive, lang)),
                Kind::Stick | Kind::Card => txt::still_in_use(lang, said_name(&drive, lang)),
            };
            say(note(
                &drive,
                summary,
                txt::CLOSE_THE_FILES.get(lang).to_owned(),
                vec![("retry".to_owned(), txt::RETRY.get(lang).to_owned())],
            ));
        }
        News::Failed { drive, doing } => {
            let name = said_name(&drive, lang);
            let summary = match doing {
                Doing::Open => txt::not_opened(lang, name),
                Doing::Eject => txt::not_ejected(lang, name),
            };
            say(note(&drive, summary, String::new(), Vec::new()));
        }
    }
}

/// The drives as they are now: kept, and the rows woken if they change.
fn kept(drives: Vec<Drive>) {
    let Some(service) = SERVICE.get() else {
        return;
    };
    let change = {
        let mut kept = service
            .drives
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let rows =
            |all: &[Drive]| -> Vec<Drive> { all.iter().filter(|d| d.mounted()).cloned().collect() };
        let (before, after) = (rows(&kept), rows(&drives));
        *kept = drives;
        let serials = |rows: &[Drive]| rows.iter().map(|d| d.serial).collect::<Vec<_>>();
        Change {
            row: before != after,
            shown: serials(&before) != serials(&after),
        }
    };
    if !change.row {
        return;
    }
    tracing::debug!(shown = change.shown, "the drives' rows change");
    let mut waker = service.waker.lock().unwrap_or_else(PoisonError::into_inner);
    if waker.as_mut().is_some_and(|wake| !wake(change)) {
        *waker = None;
    }
}

/// A bubble's action, on drive `serial`'s bubble.
pub fn from_bubble(serial: u32, key: &str) {
    tracing::debug!(serial, key, "a drive's bubble");
    match key {
        "default" | "open" => ask(Ask::Open(serial)),
        "eject" | "retry" => ask(Ask::Eject(serial)),
        _ => {}
    }
}

/// The file manager on `at`: `xdg-open`, started by the compositor as
/// the applications are.
fn open_in_files(at: &Path) {
    let Some(opener) = crate::launcher::program("xdg-open") else {
        tracing::warn!("no xdg-open in PATH: the drive does not open");
        return;
    };
    let launch = hocus_link::Launch {
        id: "xdg-open".to_owned(),
        terminal: false,
        dir: String::new(),
        argv: vec![opener, at.display().to_string()],
    };
    if launch.fits() {
        tracing::debug!(at = %at.display(), "a drive opens in the file manager");
        crate::popover::tell(ToCompositor::Launch(launch));
    }
}

// --- Its bubbles ------------------------------------------------------------------

fn say(note: note::Note) {
    notifications::input(Input::Notify(Box::new(note)));
}

/// "Clé USB « KINGSTON » branchée", "14,2 Go libres sur 32" (or its size
/// alone before it is mounted), "Ouvrir" and "Éjecter"; a click opens it.
fn plugged(drive: &Drive, lang: Lang) -> note::Note {
    let name = drive.name.as_str();
    let summary = match (drive.kind, name.is_empty()) {
        (Kind::Stick, false) => txt::stick_plugged(lang, name),
        (Kind::Disk, false) => txt::disk_plugged(lang, name),
        (Kind::Card, false) => txt::card_plugged(lang, name),
        (Kind::Stick, true) => txt::STICK_PLUGGED_UNNAMED.get(lang).to_owned(),
        (Kind::Disk, true) => txt::DISK_PLUGGED_UNNAMED.get(lang).to_owned(),
        (Kind::Card, true) => txt::CARD_PLUGGED_UNNAMED.get(lang).to_owned(),
    };
    let body = match drive.free {
        Some(free) => free_of(lang, free, drive.size),
        None => size(lang, drive.size, Rounding::Whole),
    };
    let mut note = note(
        drive,
        summary,
        body,
        vec![
            ("open".to_owned(), txt::OPEN.get(lang).to_owned()),
            ("eject".to_owned(), txt::EJECT.get(lang).to_owned()),
        ],
    );
    note.default = true;
    note
}

/// The drive's name, else what it is ("Clé USB").
pub fn said_name(drive: &Drive, lang: Lang) -> String {
    if !drive.name.is_empty() {
        return drive.name.clone();
    }
    match drive.kind {
        Kind::Stick => txt::STICK,
        Kind::Disk => txt::DISK,
        Kind::Card => txt::CARD,
    }
    .get(lang)
    .to_owned()
}

/// A bubble of drive `drive`'s: each takes the place of the one before.
fn note(
    drive: &Drive,
    summary: String,
    body: String,
    actions: Vec<(String, String)>,
) -> note::Note {
    note::Note {
        id: notifications::drive_id(drive.serial),
        app: txt::DRIVES.get(Lang::of_messages()).to_owned(),
        entry: None,
        icon: notifications::themed_icon(ICON),
        summary,
        body,
        actions,
        default: false,
        urgency: note::Urgency::Normal,
        timeout: note::Timeout::After(note::LIFE),
        progress: None,
        sound: None,
        resident: false,
        pid: 0,
        agent: false,
        conversation: None,
        icon_from: note::IconFrom {
            app_icon: ICON.to_owned(),
            image_path: None,
        },
        at: notifications::wall_ms(),
        number: 0,
        transient: false,
        ancestry: Vec::new(),
    }
}

// --- Sizes, as a person reads them ---------------------------------------------------

/// How many digits a size keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rounding {
    /// Whole under ten units and past them a decimal less: "32 Go",
    /// "14 Go", "2,5 Go" (the bar's row, a drive's size).
    Whole,
    /// A decimal under a hundred units: "14,2 Go" (the bubble's free
    /// space).
    Tenths,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Mega,
    Giga,
    Tera,
}

/// `bytes` in the unit a person reads them in (decimal, as file managers
/// and drives' boxes say them), its number as `rounding` keeps it.
fn amount(lang: Lang, bytes: u64, rounding: Rounding) -> (String, Unit) {
    let bytes = bytes as f64;
    let (value, unit) = if bytes >= 1e12 {
        (bytes / 1e12, Unit::Tera)
    } else if bytes >= 1e9 {
        (bytes / 1e9, Unit::Giga)
    } else {
        (bytes / 1e6, Unit::Mega)
    };
    let tenths = match (unit, rounding) {
        (Unit::Mega, _) => false,
        (_, Rounding::Whole) => value < 9.95,
        (_, Rounding::Tenths) => value < 99.95,
    };
    let text = if tenths {
        let text = format!("{value:.1}");
        match text.strip_suffix(".0") {
            Some(whole) => whole.to_owned(),
            None => text.replace('.', hocus_words::number::DECIMAL.get(lang)),
        }
    } else {
        format!("{value:.0}")
    };
    (text, unit)
}

fn with_unit(lang: Lang, number: &str, unit: Unit) -> String {
    match unit {
        Unit::Mega => txt::megabytes(lang, number),
        Unit::Giga => txt::gigabytes(lang, number),
        Unit::Tera => txt::terabytes(lang, number),
    }
}

/// `bytes` with its unit: "32 Go".
pub fn size(lang: Lang, bytes: u64, rounding: Rounding) -> String {
    let (number, unit) = amount(lang, bytes, rounding);
    with_unit(lang, &number, unit)
}

/// "14,2 Go libres sur 32": the size's unit said once when it is the free
/// space's.
pub fn free_of(lang: Lang, free: u64, total: u64) -> String {
    let (free_number, free_unit) = amount(lang, free, Rounding::Tenths);
    let (total_number, total_unit) = amount(lang, total, Rounding::Whole);
    let total = if total_unit == free_unit {
        total_number
    } else {
        with_unit(lang, &total_number, total_unit)
    };
    txt::free_of(
        lang,
        txt::FreeOf {
            free: &with_unit(lang, &free_number, free_unit),
            size: &total,
        },
    )
}

/// The bar's row's free space: "14 Go".
pub fn row_free(lang: Lang, free: u64) -> String {
    size(lang, free, Rounding::Whole)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_000_000_000;

    #[test]
    fn sizes_read_as_on_the_box_with_the_languages_decimal() {
        let fr = Lang::French;
        let en = Lang::English;
        // The mockup's stick: 14.2 GB free of 32.
        assert_eq!(
            free_of(fr, 14_200_000_000, 32 * GB),
            "14,2 Go libres sur 32"
        );
        assert_eq!(free_of(en, 14_200_000_000, 32 * GB), "14.2 GB free of 32");
        assert_eq!(row_free(fr, 14_200_000_000), "14 Go");
        assert_eq!(size(fr, 32 * GB, Rounding::Whole), "32 Go");
        // A stick sold as 32 GB holds a little less.
        assert_eq!(size(fr, 31_037_849_600, Rounding::Whole), "31 Go");
        // Small: a decimal under ten units, none on a whole number.
        assert_eq!(row_free(fr, 2_500_000_000), "2,5 Go");
        assert_eq!(row_free(en, 4_000_000_000), "4 GB");
        assert_eq!(free_of(fr, 3 * GB, 8 * GB), "3 Go libres sur 8");
        // Under a gigabyte, megabytes; the size's unit said when it differs.
        assert_eq!(row_free(fr, 512_300_000), "512 Mo");
        assert_eq!(free_of(fr, 512_300_000, 16 * GB), "512 Mo libres sur 16 Go");
        // Past a thousand gigabytes, terabytes.
        assert_eq!(
            free_of(fr, 1_830_000_000_000, 2_000 * GB),
            "1,8 To libres sur 2"
        );
        assert_eq!(free_of(en, 250 * GB, 2_000 * GB), "250 GB free of 2 TB");
        // Past a hundred, whole.
        assert_eq!(
            free_of(fr, 431_600_000_000, 500 * GB),
            "432 Go libres sur 500"
        );
    }

    #[test]
    fn a_drives_bubble_names_it_and_says_its_room() {
        let drive = Drive {
            serial: 3,
            name: "KINGSTON".to_owned(),
            kind: Kind::Stick,
            size: 32 * GB,
            free: None,
            mount: None,
            encrypted: false,
        };
        let note = plugged(&drive, Lang::French);
        assert_eq!(note.summary, "Clé USB « KINGSTON » branchée");
        assert_eq!(note.body, "32 Go");
        let keys: Vec<&str> = note.actions.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["open", "eject"]);
        assert_eq!(notifications::drive_of(note.id), Some(3));
        let mounted = Drive {
            free: Some(14_200_000_000),
            ..drive.clone()
        };
        assert_eq!(
            plugged(&mounted, Lang::French).body,
            "14,2 Go libres sur 32"
        );
        let unnamed = Drive {
            name: String::new(),
            kind: Kind::Disk,
            ..drive
        };
        assert_eq!(plugged(&unnamed, Lang::English).summary, "Disk plugged in");
        assert_eq!(said_name(&unnamed, Lang::French), "Disque");
    }
}
