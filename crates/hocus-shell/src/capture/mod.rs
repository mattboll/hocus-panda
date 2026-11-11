//! The screen capture's toolbar (5.8, docs/design/screenshots.md): the
//! compositor owns the capture and draws the rest of it (the darkening,
//! the area and its handles, the countdown, the flash); the shell draws
//! the toolbar at the bottom of the screen under the pointer, as the
//! compositor says what it shows (`capture`), and tells it what is
//! clicked (`capture mode|delay|pointer|shoot`).
//!
//! Once taken (`captured`, 5.9), its thumbnail shows at the bottom right
//! of its screen for 6 s ([`thumb`]), then it goes to the notifications'
//! history; a click opens its editor ([`editor`]), which draws arrows,
//! boxes, texts and blurs on it ([`marks`]) and writes it back, or saves
//! it elsewhere ([`portal`]). A capture that was not saved has the
//! launcher's toast instead, as in 5.8.
//!
//! As for the launcher, nothing goes through iced's messages: the
//! toolbar's, the thumbnail's and the editor's states are here, the
//! pointer and the keys change them and draw their surfaces alone.

pub mod draw;
pub mod editor;
pub mod marks;
pub mod portal;
pub mod thumb;
pub mod toolbar;

use hocus_words::shell::capture as txt;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::Instant,
};

use std::time::Duration;

use hocus_link::{
    CAPTURE_TOOLBAR_NAMESPACE, CaptureAsk, CaptureBar, CaptureMode, Captured, RecordAsk,
    RecordFormat, Recorded,
};
use iced::window;

use crate::{clock::time::Lang, popover::Effect};

/// The toolbar's words, in the shell's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    lang: Lang,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    fn pick(self, text: hocus_words::Text) -> &'static str {
        text.get(self.lang)
    }

    pub fn mode(self, mode: CaptureMode) -> &'static str {
        match mode {
            CaptureMode::Area => self.pick(txt::AREA),
            CaptureMode::Window => self.pick(txt::WINDOW),
            CaptureMode::Screen => self.pick(txt::SCREEN),
            CaptureMode::All => self.pick(txt::ALL_SCREENS),
        }
    }

    pub fn delay(self, seconds: u8) -> String {
        if seconds == 0 {
            self.pick(txt::NOW).to_owned()
        } else {
            format!("{seconds}\u{a0}s")
        }
    }

    pub fn shoot(self) -> &'static str {
        self.pick(txt::CAPTURE)
    }

    /// The switch's tabs (5.10).
    pub fn kind(self, video: bool) -> &'static str {
        if video {
            self.pick(txt::VIDEO)
        } else {
            "Photo"
        }
    }

    /// The computer's sound, and the microphone (5.10).
    pub fn sound(self) -> &'static str {
        self.pick(txt::SOUND)
    }

    pub fn micro(self) -> &'static str {
        self.pick(txt::MIC)
    }

    /// Vidéo's red button.
    pub fn start(self) -> &'static str {
        self.pick(txt::START)
    }

    /// Under the thumbnail (5.9).
    pub fn kept(self) -> &'static str {
        self.pick(txt::COPIED_SAVED)
    }

    pub fn annotate(self) -> &'static str {
        self.pick(txt::ANNOTATE)
    }

    pub fn open_folder(self) -> &'static str {
        self.pick(txt::OPEN_FOLDER)
    }

    pub fn delete(self) -> &'static str {
        self.pick(txt::DELETE)
    }

    /// The editor's tools.
    pub fn tool(self, tool: editor::Tool) -> &'static str {
        match tool {
            editor::Tool::Arrow => self.pick(txt::ARROW),
            editor::Tool::Box => self.pick(txt::BOX),
            editor::Tool::Text => self.pick(txt::TEXT),
            editor::Tool::Blur => self.pick(txt::BLUR),
        }
    }

    /// What the text's field shows while empty.
    pub fn text_hint(self) -> &'static str {
        self.pick(txt::TEXT)
    }

    /// The pointer's button, for screen readers.
    pub fn pointer(self) -> &'static str {
        self.pick(txt::SHOW_THE_POINTER)
    }

    /// The toolbar, for screen readers.
    pub fn toolbar(self) -> &'static str {
        self.pick(txt::SCREENSHOT)
    }

    /// The editor's colours, for screen readers.
    pub fn colour(self, i: usize) -> &'static str {
        match i {
            0 => self.pick(txt::RED),
            1 => self.pick(txt::AMBER),
            2 => self.pick(txt::GREEN),
            3 => self.pick(txt::BLUE),
            _ => self.pick(txt::WHITE),
        }
    }

    pub fn undo(self) -> &'static str {
        self.pick(txt::UNDO)
    }

    /// The editor, for screen readers.
    pub fn editor(self) -> &'static str {
        self.pick(txt::ANNOTATE_THE_CAPTURE)
    }

    pub fn save_as(self) -> &'static str {
        self.pick(txt::SAVE_AS)
    }

    pub fn copy_and_close(self) -> &'static str {
        self.pick(txt::COPY_AND_CLOSE)
    }

    /// The file chooser's title.
    pub fn save_title(self) -> &'static str {
        self.pick(txt::SAVE_THE_CAPTURE)
    }

    /// Where "Save as…" wrote it, under the image.
    pub fn saved_as(self, name: &str) -> String {
        txt::saved_2(self.lang, name)
    }

    /// The name a copy saved without a file chooser takes, beside the
    /// capture, in its format: "Capture du … (annotée).png".
    pub fn annotated_name(self, stem: &str, extension: &str) -> String {
        txt::annotated(self.lang, extension, stem)
    }

    /// The capture in the notifications' history: its application's name.
    pub fn screenshot(self) -> &'static str {
        self.pick(txt::SCREENSHOT)
    }

    /// A recording (5.10): under its thumbnail, "Enregistrée · MP4".
    pub fn recorded(self, format: RecordFormat) -> String {
        format!("{} · {}", self.pick(txt::SAVED), format.label())
    }

    /// Its button: the default player.
    pub fn open(self) -> &'static str {
        self.pick(txt::OPEN)
    }

    /// The recording in the notifications' history: its application's
    /// name.
    pub fn screen_recording(self) -> &'static str {
        self.pick(txt::SCREEN_RECORDING)
    }

    /// A recording that could not be saved: its toast.
    pub fn not_recorded(self) -> &'static str {
        self.pick(txt::THE_RECORDING_COULD_NOT_BE_SAVED)
    }

    /// The toast once taken: what, copied, and where it was saved.
    pub fn captured(self, captured: &Captured) -> String {
        let saved = !captured.folder.is_empty();
        let folder = &captured.folder;
        if self.lang == Lang::French {
            // Each its agreement: une zone, une fenêtre, un écran.
            let (what, copied, kept) = match captured.mode {
                CaptureMode::Area => (
                    format!("Zone de {} × {}", captured.width, captured.height),
                    "copiée",
                    "enregistrée",
                ),
                CaptureMode::Window => ("Fenêtre".to_owned(), "copiée", "enregistrée"),
                CaptureMode::Screen => ("Écran".to_owned(), "copié", "enregistré"),
                CaptureMode::All => ("Tous les écrans".to_owned(), "copiés", "enregistrés"),
            };
            if saved {
                format!("{what} {copied} · {kept} dans {folder}")
            } else {
                format!("{what} {copied}")
            }
        } else {
            let what = match captured.mode {
                CaptureMode::Area => format!("Area of {} × {}", captured.width, captured.height),
                CaptureMode::Window => "Window".to_owned(),
                CaptureMode::Screen => "Screen".to_owned(),
                CaptureMode::All => "All screens".to_owned(),
            };
            if saved {
                format!("{what} copied · saved to {folder}")
            } else {
                format!("{what} copied")
            }
        }
    }
}

pub fn words() -> Words {
    Words::new(Lang::of_messages())
}

/// A capture taken and saved, as the compositor said it (5.9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shot {
    /// The compositor's number for it.
    pub id: u64,
    /// The screen its thumbnail shows on, and its editor.
    pub output: String,
    pub file: PathBuf,
    /// The folder as the user reads it.
    pub folder: String,
    pub width: u32,
    pub height: u32,
    /// A recording (5.10): how long it lasts, its format, and its first
    /// frame's picture.
    pub video: Option<Video>,
}

/// What a recording's thumbnail and history say of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Video {
    pub duration: Duration,
    pub format: RecordFormat,
    pub poster: PathBuf,
}

/// The shell's ids of recordings, apart from the captures' (both share
/// the thumbnail and the history).
const RECORDING: u64 = 0x2000_0000;

impl Shot {
    /// The compositor's number of the recording, if it is one.
    pub fn recording(&self) -> Option<u64> {
        self.video.as_ref().map(|_| self.id & !RECORDING)
    }
    pub fn name(&self) -> String {
        self.file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// The folder it is in, on disk.
    pub fn dir(&self) -> Option<&Path> {
        self.file.parent()
    }
}

/// The captures the shell was told of, the newest last.
static SHOTS: Mutex<Vec<Shot>> = Mutex::new(Vec::new());
/// The most kept: what the history can still act on.
const SHOTS_KEPT: usize = 64;
/// The newest capture's image, read for its thumbnail: its editor opens
/// on it at once.
static DECODED: Mutex<Option<(u64, Arc<marks::Image>)>> = Mutex::new(None);

/// The captures' icons in the history, made with their thumbnails.
static ICONS: Mutex<Vec<(u64, crate::levels::icons::Icon)>> = Mutex::new(Vec::new());

/// Capture `id`'s icon in the history, if its thumbnail made one.
pub fn icon(id: u64) -> Option<crate::levels::icons::Icon> {
    ICONS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find(|(i, _)| *i == id)
        .map(|(_, icon)| icon.clone())
}

fn keep_icon(id: u64, icon: crate::levels::icons::Icon) {
    let mut icons = ICONS.lock().unwrap_or_else(PoisonError::into_inner);
    icons.retain(|(i, _)| *i != id);
    icons.push((id, icon));
    let extra = icons.len().saturating_sub(SHOTS_KEPT);
    icons.drain(..extra);
}

/// An action of capture `id`'s in the history: annotate it (a click on
/// it too), open its folder, delete it.
pub fn from_history(id: u64, action: &str) {
    let Some(shot) = shot(id) else {
        tracing::debug!(id, "a capture the shell knows no more");
        return;
    };
    tracing::debug!(id, action, "a capture's action in the history");
    match action {
        "default" | "open" if shot.video.is_some() => open_file(&shot.file),
        "default" | "annotate" => editor::open(shot),
        "folder" => {
            let Some(dir) = shot.dir().and_then(Path::to_str) else {
                return;
            };
            let launch = hocus_link::Launch {
                id: "xdg-open".to_owned(),
                terminal: false,
                dir: String::new(),
                argv: vec!["xdg-open".to_owned(), dir.to_owned()],
            };
            if launch.fits() {
                crate::popover::tell(hocus_link::ToCompositor::Launch(launch));
            }
        }
        "delete" => delete(id),
        _ => {}
    }
}

/// Capture `id`, if the shell was told of it.
pub fn shot(id: u64) -> Option<Shot> {
    SHOTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find(|s| s.id == id)
        .cloned()
}

fn keep_shot(shot: Shot) {
    let mut shots = SHOTS.lock().unwrap_or_else(PoisonError::into_inner);
    shots.retain(|s| s.id != shot.id);
    shots.push(shot);
    let extra = shots.len().saturating_sub(SHOTS_KEPT);
    shots.drain(..extra);
}

/// Capture `id` is no more (deleted).
fn forget_shot(id: u64) {
    SHOTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|s| s.id != id);
    let mut decoded = DECODED.lock().unwrap_or_else(PoisonError::into_inner);
    if decoded.as_ref().is_some_and(|(d, _)| *d == id) {
        *decoded = None;
    }
}

/// Capture `id`'s image, if it was read already.
fn decoded(id: u64) -> Option<Arc<marks::Image>> {
    DECODED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .filter(|(d, _)| *d == id)
        .map(|(_, image)| image.clone())
}

/// Capture `file`'s image, read and decoded (on a thread of the caller's).
fn read_image(file: &Path) -> Result<marks::Image, String> {
    let bytes = std::fs::read(file).map_err(|err| format!("{}: {err}", file.display()))?;
    // PNG, or JPEG (`[capture] format`, 6.17).
    image::load_from_memory(&bytes)
        .map(|i| i.to_rgba8())
        .map_err(|err| format!("{}: {err}", file.display()))
}

/// A line to the compositor about a capture.
fn tell(ask: CaptureAsk) {
    crate::popover::tell(hocus_link::ToCompositor::Capture(ask));
}

/// Capture `id` deleted (✕, "Supprimer"): the compositor deletes its file
/// and lets the clipboard go if it still holds it; out of the history too.
/// A recording's (5.10): its file goes.
pub fn delete(id: u64) {
    match shot(id).and_then(|s| s.recording()) {
        Some(recording) => {
            tracing::info!(recording, "the recording is deleted");
            crate::popover::tell(hocus_link::ToCompositor::Record(RecordAsk::Forget(
                recording,
            )));
        }
        None => {
            tracing::info!(id, "the capture is deleted");
            tell(CaptureAsk::Forget(id));
        }
    }
    forget_shot(id);
    crate::notifications::forget_capture(id);
}

/// `file` in its default application (a recording's player): `xdg-open`,
/// looked for in the shell's `PATH`, started by the compositor's launch.
pub fn open_file(file: &Path) {
    let Some(path) = file.to_str() else {
        return;
    };
    let Some(opener) = crate::launcher::program("xdg-open") else {
        tracing::warn!("no xdg-open in PATH: the recording does not open");
        return;
    };
    tracing::debug!(file = path, "opened in its application");
    let launch = hocus_link::Launch {
        id: "xdg-open".to_owned(),
        terminal: false,
        dir: String::new(),
        argv: vec![opener, path.to_owned()],
    };
    if launch.fits() {
        crate::popover::tell(hocus_link::ToCompositor::Launch(launch));
    }
}

/// `recorded` (5.10): its thumbnail, its first frame read on a thread; a
/// toast when it could not be saved.
pub fn recorded(recorded: Recorded) {
    if recorded.file.is_empty() {
        crate::popover::send(crate::launcher::toast::show(
            recorded.output.clone(),
            words().not_recorded().to_owned(),
            Instant::now(),
        ));
        return;
    }
    let shot = Shot {
        id: recorded.id | RECORDING,
        output: recorded.output.clone(),
        file: PathBuf::from(&recorded.file),
        folder: recorded.folder.clone(),
        width: recorded.width,
        height: recorded.height,
        video: Some(Video {
            duration: Duration::from_millis(recorded.duration),
            format: recorded.format,
            poster: PathBuf::from(&recorded.poster),
        }),
    };
    keep_shot(shot.clone());
    let scale = crate::popover::kit::scale(&shot.output);
    let started = std::thread::Builder::new()
        .name("recording's thumbnail".to_owned())
        .spawn(move || {
            let poster = shot.video.as_ref().map(|v| v.poster.clone());
            let image = poster
                .filter(|p| !p.as_os_str().is_empty())
                .ok_or_else(|| "no first frame".to_owned())
                .and_then(|p| read_image(&p))
                .unwrap_or_else(|err| {
                    tracing::debug!(err, "the recording's first frame: a dark one instead");
                    image::RgbaImage::from_pixel(16, 10, image::Rgba([28, 36, 54, 255]))
                });
            let length = shot.video.as_ref().map_or(4, |v| {
                crate::in_use::draw::elapsed(v.duration).chars().count()
            });
            let made = thumb::Made::of_recording(&image, scale, length);
            keep_icon(shot.id, made.icon.clone());
            thumb::show(shot, made, Instant::now());
        });
    if let Err(err) = started {
        tracing::warn!(%err, "no thread for the recording's thumbnail");
    }
}

/// A button of the toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// The switch's tabs (5.10): Photo (`false`) or Vidéo.
    Kind(bool),
    Mode(CaptureMode),
    Delay,
    Pointer,
    /// In Vidéo (5.10): the computer's sound and the microphone.
    Sound,
    Micro,
    /// "Capturer", or in Vidéo "Démarrer".
    Shoot,
}

impl Button {
    fn ask(self) -> CaptureAsk {
        match self {
            Self::Kind(video) => CaptureAsk::Video(video),
            Self::Mode(mode) => CaptureAsk::Mode(mode),
            Self::Delay => CaptureAsk::Delay,
            Self::Pointer => CaptureAsk::Pointer,
            Self::Sound => CaptureAsk::Sound,
            Self::Micro => CaptureAsk::Micro,
            Self::Shoot => CaptureAsk::Shoot,
        }
    }
}

/// The toolbar shown.
#[derive(Debug, Clone)]
pub struct Toolbar {
    pub id: window::Id,
    pub bar: CaptureBar,
}

static TOOLBAR: Mutex<Option<Toolbar>> = Mutex::new(None);

/// `with` on the toolbar, if one shows.
pub fn with<T>(with: impl FnOnce(&mut Option<Toolbar>) -> T) -> T {
    with(&mut TOOLBAR.lock().unwrap_or_else(PoisonError::into_inner))
}

/// Whether surface `id` is the toolbar's.
pub fn is_surface(id: window::Id) -> bool {
    with(|t| t.as_ref().is_some_and(|t| t.id == id))
}

/// The compositor's `capture` line: the toolbar opens on its screen, or
/// shows what changed.
pub fn set(bar: CaptureBar) {
    let effects = with(|toolbar| match toolbar {
        Some(shown) if shown.bar.output == bar.output => {
            // Photo and Vidéo have their own buttons: its size follows.
            let resized = shown.bar.video != bar.video;
            let video = bar.video;
            shown.bar = bar;
            // Its buttons are built again from it.
            crate::ui::touch();
            let mut effects = vec![Effect::Wake(CAPTURE_TOOLBAR_NAMESPACE)];
            if resized {
                let size = draw::Layout::of(words(), video).size;
                effects.push(Effect::Resize {
                    id: shown.id,
                    size: (size.width.ceil() as u32, size.height.ceil() as u32),
                    margin: None,
                });
            }
            effects
        }
        _ => {
            let mut effects = Vec::new();
            if let Some(old) = toolbar.take() {
                effects.push(Effect::Remove(old.id));
            }
            let id = window::Id::unique();
            let size = draw::Layout::of(words(), bar.video).size;
            tracing::debug!(output = bar.output, "the capture's toolbar opens");
            effects.push(Effect::CaptureBar {
                id,
                output: bar.output.clone(),
                size: (size.width.ceil() as u32, size.height.ceil() as u32),
            });
            *toolbar = Some(Toolbar { id, bar });
            effects
        }
    });
    crate::popover::send(effects);
}

/// `capture-end`: the toolbar goes.
pub fn end() {
    let effects = with(|toolbar| {
        toolbar
            .take()
            .map(|t| vec![Effect::Remove(t.id)])
            .unwrap_or_default()
    });
    if !effects.is_empty() {
        tracing::debug!("the capture's toolbar goes");
        crate::popover::send(effects);
    }
}

/// `captured`: its thumbnail once its image is read (on a thread); the
/// launcher's toast when it was not saved, or cannot be read.
pub fn captured(captured: Captured) {
    if captured.file.is_empty() {
        return toast(&captured);
    }
    let shot = Shot {
        id: captured.id,
        output: captured.output.clone(),
        file: PathBuf::from(&captured.file),
        folder: captured.folder.clone(),
        width: captured.width,
        height: captured.height,
        video: None,
    };
    keep_shot(shot.clone());
    let scale = crate::popover::kit::scale(&shot.output);
    let started = std::thread::Builder::new()
        .name("capture's thumbnail".to_owned())
        .spawn(move || match read_image(&shot.file) {
            Ok(image) => {
                let image = Arc::new(image);
                *DECODED.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some((shot.id, image.clone()));
                let made = thumb::Made::of(&image, scale);
                keep_icon(shot.id, made.icon.clone());
                thumb::show(shot, made, Instant::now());
            }
            Err(err) => {
                tracing::warn!(err, "the capture cannot be read: its toast instead");
                toast(&captured);
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "no thread for the capture's thumbnail");
    }
}

/// The launcher's toast says what was taken, on its screen (5.8).
fn toast(captured: &Captured) {
    let said = words().captured(captured);
    crate::popover::send(crate::launcher::toast::show(
        captured.output.clone(),
        said,
        Instant::now(),
    ));
}

/// `button` clicked on the toolbar on `id`: the compositor is told.
pub fn clicked(id: window::Id, button: Button) {
    if !is_surface(id) {
        return;
    }
    tracing::debug!(?button, "the capture's toolbar is clicked");
    crate::popover::send(vec![Effect::Tell(hocus_link::ToCompositor::Capture(
        button.ask(),
    ))]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured(mode: CaptureMode, folder: &str) -> Captured {
        Captured {
            id: 1,
            output: "eDP-1".into(),
            mode,
            width: 1200,
            height: 760,
            folder: folder.into(),
            file: String::new(),
        }
    }

    #[test]
    fn the_toast_says_what_was_taken_and_where() {
        let fr = Words::new(Lang::French);
        let en = Words::new(Lang::English);
        let folder = "Images/Captures d'écran";
        assert_eq!(
            fr.captured(&captured(CaptureMode::Area, folder)),
            "Zone de 1200 × 760 copiée · enregistrée dans Images/Captures d'écran"
        );
        assert_eq!(
            fr.captured(&captured(CaptureMode::Screen, folder)),
            "Écran copié · enregistré dans Images/Captures d'écran"
        );
        assert_eq!(
            en.captured(&captured(CaptureMode::All, "Pictures/Screenshots")),
            "All screens copied · saved to Pictures/Screenshots"
        );
        // Not saved: only copied.
        assert_eq!(
            en.captured(&captured(CaptureMode::Window, "")),
            "Window copied"
        );
        assert_eq!(fr.delay(0), "Tout de suite");
        assert_eq!(en.delay(10), "10\u{a0}s");
    }
}
