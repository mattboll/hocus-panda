//! The capture's editor (5.9): a panel in the middle of its screen, over
//! the desktop darkened and blurred, the image fitted in it at most the
//! mockup's 672 × 420 × the launcher's zoom. Its tools draw marks on the
//! image: an arrow, a box, a text, a blur; five colours; `Ctrl+Z` undoes
//! and `Ctrl+Shift+Z` redoes. "Copier et fermer" writes the annotated
//! image over the capture's file and the clipboard; "Enregistrer sous…"
//! saves it where the desktop portal's file chooser says, or beside the
//! capture without one; "Supprimer" deletes the capture.
//!
//! The marks are kept in the image's own pixels, the original image with
//! them until the editor closes: undoing back to nothing gives it back,
//! and closing with no marks changes nothing. The image is drawn with its
//! marks at its real resolution, encoded and written on a thread
//! ([`marks::render`]).
//!
//! A layer surface of its own ([`hocus_link::CAPTURE_EDITOR_NAMESPACE`]),
//! all of its screen on the overlay layer, holding the keyboard; the
//! compositor blurs the desktop behind it and fades it in.

pub mod draw;
pub mod view;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

use hocus_link::CaptureAsk;
use iced::{
    Point, Rectangle,
    advanced::image::{Allocation, Handle},
    window,
};

use super::{Shot, marks, marks::Mark, words};
use crate::{
    field::{self, Entry},
    popover::Effect,
};

/// The editor's tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Arrow,
    Box,
    Text,
    Blur,
}

impl Tool {
    pub const ALL: [Self; 4] = [Self::Arrow, Self::Box, Self::Text, Self::Blur];
}

/// The colours: red (the first), amber, green, blue, white.
pub const COLOURS: [marks::Rgb; 5] = [
    [0xF2, 0x54, 0x5B],
    [0xF5, 0xB4, 0x4A],
    [0x6C, 0xC2, 0x87],
    [0x5B, 0x8D, 0xEF],
    [0xFF, 0xFF, 0xFF],
];

/// The marks' sizes, the mockup's, in its units (the editor's image at
/// its zoom of 1): an arrow 4 wide with a 16 head; a box 3 wide, its
/// corners 4; a text 18 high, outlined 1.5; a blur 7 far; a drag shorter
/// than 8 draws nothing.
pub const ARROW: (f32, f32) = (4.0, 16.0);
pub const BOX: (f32, f32) = (3.0, 4.0);
pub const TEXT: (f32, f32) = (18.0, 1.5);
pub const BLUR: f32 = 7.0;
pub const LEAST: f32 = 8.0;
/// The text's field: its text 7 in from where it was clicked.
pub const TEXT_IN: f32 = 7.0;

/// The editor's controls (the kit's, [`view`]): what tests and screen
/// readers know them by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Tool(Tool),
    Colour(usize),
    Undo,
    /// The text being typed.
    Text,
    Delete,
    SaveAs,
    Copy,
}

/// A drag on the image, from where to where (the surface's frame).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drag {
    pub from: Point,
    pub to: Point,
}

/// A text being typed, where it was clicked (the surface's frame): the
/// shell's text field (#771), on one line.
#[derive(Debug, Clone)]
pub struct Typing {
    pub at: Point,
    pub text: Entry,
}

/// The characters a text takes at most.
const TEXT_MOST: usize = 200;

impl Typing {
    /// An empty text at `at`.
    pub fn at(at: Point) -> Self {
        Self {
            at,
            text: Entry::new(field::Kind::Line, TEXT_MOST),
        }
    }
}

/// Where the editor is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Shown, its surface `id`.
    Shown(window::Id),
    /// The file chooser shows: the editor steps aside, and comes back.
    Choosing,
}

/// The editor open.
#[derive(Debug, Clone)]
pub struct Editor {
    pub phase: Phase,
    pub shot: Shot,
    /// The capture as it was when the editor opened.
    pub image: Arc<marks::Image>,
    /// The image as it shows, at the screen's pixels, with its marks drawn
    /// as they are saved ([`marks::render`]); without them, and blurred
    /// (where a blur is being drawn).
    pub shown: Handle,
    pub plain: Arc<marks::Image>,
    pub blurred: Handle,
    /// `shown` and `blurred` in the renderer's memory, for as long as they
    /// show ([`draw::hold`]).
    pub held: Vec<Allocation>,
    /// The screen's scale.
    pub scale: f32,
    pub tool: Tool,
    pub colour: usize,
    pub marks: Vec<Mark>,
    /// What was undone, the last undone last: redone first.
    pub undone: Vec<Mark>,
    pub drag: Option<Drag>,
    pub typing: Option<Typing>,
    /// Where the keyboard's ring is.
    pub focus: Option<Control>,
    /// Where its pieces are, as its log last said it (for tests).
    pub laid: String,
    /// Where "Enregistrer sous…" last wrote it.
    pub saved_as: Option<String>,
}

static EDITOR: Mutex<Option<Editor>> = Mutex::new(None);

pub fn with<T>(with: impl FnOnce(&mut Option<Editor>) -> T) -> T {
    with(&mut EDITOR.lock().unwrap_or_else(PoisonError::into_inner))
}

/// Whether surface `id` is the editor's.
pub fn is_surface(id: window::Id) -> bool {
    with(|e| e.as_ref().is_some_and(|e| e.phase == Phase::Shown(id)))
}

/// What the editor opens with, made on a thread: the image as it shows,
/// and blurred.
struct Made {
    plain: Arc<marks::Image>,
    blurred: Handle,
}

/// The image shown in the editor at its largest: the mockup's 672 × 420,
/// zoomed.
pub const IMAGE_ROOM: (f32, f32) = (672.0 * draw::Z, 420.0 * draw::Z);

impl Made {
    fn of(image: &marks::Image, scale: f32) -> Self {
        let (w, h) = super::thumb::fit(
            image.dimensions(),
            (IMAGE_ROOM.0 * scale, IMAGE_ROOM.1 * scale),
        );
        let small = super::thumb::resize(image, w, h);
        let mut blurred = small.clone();
        let whole = marks::Rect {
            x: 0.0,
            y: 0.0,
            w: w as f32,
            h: h as f32,
        };
        marks::blur(&mut blurred, whole, BLUR * draw::Z * scale);
        Self {
            plain: Arc::new(small),
            blurred: Handle::from_rgba(w, h, blurred.into_raw()),
        }
    }
}

/// The editor opens on capture `shot`, its image read first on a thread
/// unless the thumbnail read it.
pub fn open(shot: Shot) {
    if with(|e| e.is_some()) {
        return;
    }
    let scale = crate::popover::kit::scale(&shot.output);
    let cached = super::decoded(shot.id);
    let started = std::thread::Builder::new()
        .name("capture's editor".to_owned())
        .spawn(move || {
            let image = match cached {
                Some(image) => image,
                None => match super::read_image(&shot.file) {
                    Ok(image) => Arc::new(image),
                    Err(err) => {
                        tracing::warn!(err, "the capture cannot be read: no editor");
                        return;
                    }
                },
            };
            let made = Made::of(&image, scale);
            shown(shot, image, made, scale);
        });
    if let Err(err) = started {
        tracing::warn!(%err, "no thread for the capture's editor");
    }
}

/// The editor shows, made.
fn shown(shot: Shot, image: Arc<marks::Image>, made: Made, scale: f32) {
    let id = window::Id::unique();
    let output = shot.output.clone();
    let opened = with(|editor| {
        if editor.is_some() {
            return false;
        }
        tracing::info!(capture = shot.id, "the capture's editor opens");
        *editor = Some(Editor {
            phase: Phase::Shown(id),
            shot,
            image,
            shown: Handle::from_rgba(made.plain.width(), made.plain.height(), made.plain.to_vec()),
            plain: made.plain,
            blurred: made.blurred,
            held: Vec::new(),
            scale,
            tool: Tool::Arrow,
            colour: 0,
            marks: Vec::new(),
            undone: Vec::new(),
            drag: None,
            typing: None,
            focus: None,
            laid: String::new(),
            saved_as: None,
        });
        true
    });
    if opened {
        // A popover open (the history's) closes under it.
        crate::popover::event(crate::popover::Event::Outside);
        crate::popover::send(vec![Effect::CaptureEditor { id, output }]);
    }
}

impl Editor {
    /// The image's pixels per logical pixel of its place on the screen.
    pub fn pixels_per(&self, image: Rectangle) -> f32 {
        self.image.width() as f32 / image.width.max(1.0)
    }

    /// Point `p` of the surface in the image's pixels.
    pub fn to_image(&self, image: Rectangle, p: Point) -> (f32, f32) {
        let k = self.pixels_per(image);
        ((p.x - image.x) * k, (p.y - image.y) * k)
    }

    /// The mark a drag from `from` to `to` draws with the tool, in the
    /// image's pixels; none for a text, or one too short. `z`: the
    /// editor's zoom.
    pub fn mark_of(&self, image: Rectangle, drag: Drag, z: f32) -> Option<Mark> {
        let (dx, dy) = (drag.to.x - drag.from.x, drag.to.y - drag.from.y);
        if dx.hypot(dy) < LEAST * z {
            return None;
        }
        let k = self.pixels_per(image) * z;
        let (from, to) = (
            self.to_image(image, drag.from),
            self.to_image(image, drag.to),
        );
        let colour = COLOURS[self.colour];
        match self.tool {
            Tool::Arrow => Some(Mark::Arrow {
                from,
                to,
                width: ARROW.0 * k,
                head: ARROW.1 * k,
                colour,
            }),
            Tool::Box => Some(Mark::Box {
                rect: marks::Rect::between(from, to),
                width: BOX.0 * k,
                radius: BOX.1 * k,
                colour,
            }),
            Tool::Blur => Some(Mark::Blur {
                rect: marks::Rect::between(from, to),
                radius: BLUR * k,
            }),
            Tool::Text => None,
        }
    }

    /// The text typed as a mark, if it says something.
    pub fn text_mark(&self, image: Rectangle, typing: &Typing, z: f32) -> Option<Mark> {
        let text = typing.text.text().trim();
        if text.is_empty() {
            return None;
        }
        let k = self.pixels_per(image) * z;
        let at = Point::new(typing.at.x + TEXT_IN * z, typing.at.y);
        Some(Mark::Text {
            at: self.to_image(image, at),
            text: text.to_owned(),
            size: TEXT.0 * k,
            outline: TEXT.1 * k,
            colour: COLOURS[self.colour],
        })
    }

    /// A new mark: what was undone cannot be redone any more.
    pub fn add(&mut self, mark: Mark) {
        tracing::debug!(?mark, "a mark on the capture");
        self.marks.push(mark);
        self.undone.clear();
        self.recompose();
    }

    /// The image shown drawn again with its marks, as the saved one is,
    /// at the screen's pixels.
    fn recompose(&mut self) {
        let k = self.plain.width() as f32 / self.image.width().max(1) as f32;
        let scaled: Vec<Mark> = self.marks.iter().map(|m| m.scaled(k)).collect();
        let drawn = marks::render(&self.plain, &scaled);
        self.shown = Handle::from_rgba(drawn.width(), drawn.height(), drawn.into_raw());
    }

    /// The text typed becomes a mark, if it says something.
    pub fn commit_text(&mut self, image: Rectangle, z: f32) {
        if let Some(typing) = self.typing.take()
            && let Some(mark) = self.text_mark(image, &typing, z)
        {
            self.add(mark);
        }
    }

    pub fn undo(&mut self) -> bool {
        let Some(mark) = self.marks.pop() else {
            return false;
        };
        tracing::debug!(left = self.marks.len(), "a mark undone");
        self.undone.push(mark);
        self.recompose();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(mark) = self.undone.pop() else {
            return false;
        };
        tracing::debug!(left = self.marks.len() + 1, "a mark redone");
        self.marks.push(mark);
        self.recompose();
        true
    }
}

/// The clipboard's text, on one line, into the text being typed at its
/// caret, read off the loop.
pub fn paste() {
    crate::paste::request(crate::paste::Clean::Spaced, move |pasted| {
        let Some(pasted) = pasted.filter(|t| !t.is_empty()) else {
            return;
        };
        let pasted = with(|editor| {
            editor
                .as_mut()
                .and_then(|e| e.typing.as_mut())
                .is_some_and(|typing| typing.text.paste(pasted))
        });
        if pasted {
            crate::ui::touch();
            crate::popover::send(vec![Effect::Wake(hocus_link::CAPTURE_EDITOR_NAMESPACE)]);
        }
    });
}

/// How the editor closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closed {
    /// `Escape`, or "Copier et fermer" with no marks: nothing changes;
    /// the capture goes to the history.
    Kept,
    /// "Copier et fermer" with marks: the annotated image over the file
    /// and the clipboard, then to the history.
    Annotated,
    /// "Supprimer": the capture is deleted.
    Deleted,
}

/// The editor closes.
pub fn close(how: Closed) {
    let Some(editor) = with(Option::take) else {
        return;
    };
    tracing::info!(
        capture = editor.shot.id,
        ?how,
        marks = editor.marks.len(),
        "the capture's editor closes"
    );
    if let Phase::Shown(id) = editor.phase {
        crate::popover::send(vec![Effect::Remove(id)]);
    }
    match how {
        Closed::Deleted => super::delete(editor.shot.id),
        Closed::Kept => crate::notifications::keep_capture(&editor.shot, None),
        Closed::Annotated if editor.marks.is_empty() => {
            crate::notifications::keep_capture(&editor.shot, None);
        }
        Closed::Annotated => {
            let shot = editor.shot.clone();
            let (image, marks) = (editor.image.clone(), editor.marks.clone());
            spawn("write the annotated capture", move || {
                let file = shot.file.clone();
                match encode(&image, &marks, &file).and_then(|bytes| replace(&file, &bytes)) {
                    Ok(()) => {
                        tracing::info!(capture = shot.id, file = %file.display(), "the annotated capture is written");
                        super::tell(CaptureAsk::Annotated(shot.id));
                        crate::notifications::keep_capture(&shot, None);
                    }
                    Err(err) => tracing::warn!(err, "the annotated capture is not written"),
                }
            });
        }
    }
}

/// "Copier et fermer", "Supprimer", "Enregistrer sous…".
pub fn press(control: Control, image: Rectangle, z: f32) {
    match control {
        Control::Copy => {
            with(|e| {
                if let Some(e) = e.as_mut() {
                    e.commit_text(image, z);
                }
            });
            close(Closed::Annotated);
        }
        Control::Delete => close(Closed::Deleted),
        Control::SaveAs => save_as(image, z),
        _ => {}
    }
}

/// "Enregistrer sous…": the image with its marks where the file chooser
/// says, else beside the capture under a new name; the editor steps aside
/// while the chooser shows.
fn save_as(image: Rectangle, z: f32) {
    let job = with(|editor| {
        let editor = editor.as_mut()?;
        editor.commit_text(image, z);
        Some((
            editor.shot.clone(),
            editor.image.clone(),
            editor.marks.clone(),
        ))
    });
    let Some((shot, original, marks)) = job else {
        return;
    };
    spawn("save the capture as", move || {
        let words = words();
        let stem = shot
            .file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let extension = shot
            .file
            .extension()
            .map_or_else(|| "png".to_owned(), |e| e.to_string_lossy().into_owned());
        let name = words.annotated_name(&stem, &extension);
        let folder = shot.dir().map(Path::to_path_buf).unwrap_or_default();
        let chosen = super::portal::save_file(words.save_title(), &name, &folder, || {
            // The chooser shows: the editor steps aside.
            let gone = with(|editor| {
                let editor = editor.as_mut()?;
                let Phase::Shown(id) = editor.phase else {
                    return None;
                };
                editor.phase = Phase::Choosing;
                Some(id)
            });
            if let Some(id) = gone {
                crate::popover::send(vec![Effect::Remove(id)]);
            }
        });
        let path = match chosen {
            super::portal::Chosen::Path(path) => Some(path),
            super::portal::Chosen::Cancelled => None,
            super::portal::Chosen::NoPortal => {
                tracing::info!("no file chooser: saved beside the capture");
                Some(unused(&folder, &name))
            }
        };
        let saved = path.and_then(|path| {
            match encode(&original, &marks, &path).and_then(|bytes| write_new(&path, &bytes)) {
                Ok(()) => {
                    tracing::info!(file = %path.display(), "the capture is saved as");
                    Some(path)
                }
                Err(err) => {
                    tracing::warn!(err, "the capture is not saved");
                    None
                }
            }
        });
        back_from_choosing(saved.map(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        }));
    });
}

/// After "Enregistrer sous…": where it wrote, under the image; the editor
/// back if it stepped aside.
fn back_from_choosing(saved: Option<String>) {
    let reopen = with(|editor| {
        let editor = editor.as_mut()?;
        if saved.is_some() {
            editor.saved_as.clone_from(&saved);
        }
        match editor.phase {
            Phase::Choosing => {
                let id = window::Id::unique();
                editor.phase = Phase::Shown(id);
                editor.focus = None;
                editor.laid.clear();
                Some(Effect::CaptureEditor {
                    id,
                    output: editor.shot.output.clone(),
                })
            }
            Phase::Shown(_) => {
                crate::ui::touch();
                Some(Effect::Wake(hocus_link::CAPTURE_EDITOR_NAMESPACE))
            }
        }
    });
    if let Some(effect) = reopen {
        crate::popover::send(vec![effect]);
    }
}

fn spawn(name: &str, job: impl FnOnce() + Send + 'static) {
    if let Err(err) = std::thread::Builder::new().name(name.to_owned()).spawn(job) {
        tracing::warn!(%err, name, "no thread");
    }
}

/// `image` with `marks`, as `file`'s extension says: JPEG (at quality
/// 90, as the compositor writes captures, 6.17), else PNG.
pub fn encode(image: &marks::Image, marks: &[Mark], file: &Path) -> Result<Vec<u8>, String> {
    let drawn = marks::render(image, marks);
    let jpeg = file
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"));
    let mut bytes = Vec::new();
    let written = if jpeg {
        let rgb = image::DynamicImage::ImageRgba8(drawn).to_rgb8();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90).encode_image(&rgb)
    } else {
        drawn.write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
    };
    written.map_err(|err| format!("could not encode the capture: {err}"))?;
    Ok(bytes)
}

/// `bytes` over `file`, whole or not at all (a new file renamed over it).
fn replace(file: &Path, bytes: &[u8]) -> Result<(), String> {
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let part = file.with_file_name(format!(".{name}.part"));
    std::fs::write(&part, bytes)
        .and_then(|()| std::fs::rename(&part, file))
        .map_err(|err| {
            let _ = std::fs::remove_file(&part);
            format!("{}: {err}", file.display())
        })
}

/// `bytes` in `path`, made for them alone.
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut f| f.write_all(bytes))
        .map_err(|err| format!("{}: {err}", path.display()))
}

/// `name` in `dir`, or with " (2)", " (3)"… before its extension when a
/// file has it.
fn unused(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, extension) = name.rsplit_once('.').unwrap_or((name, "png"));
    (2..1000)
        .map(|n| dir.join(format!("{stem} ({n}).{extension}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Size;

    fn editor(width: u32, height: u32) -> Editor {
        let image = Arc::new(marks::Image::new(width, height));
        let handle = Handle::from_rgba(1, 1, vec![0; 4]);
        Editor {
            phase: Phase::Choosing,
            shot: Shot {
                id: 1,
                output: "eDP-1".into(),
                file: PathBuf::from("/nowhere/a.png"),
                folder: String::new(),
                width,
                height,
                video: None,
            },
            image,
            shown: handle.clone(),
            plain: Arc::new(marks::Image::new(1, 1)),
            blurred: handle,
            held: Vec::new(),
            scale: 1.0,
            tool: Tool::Arrow,
            colour: 0,
            marks: Vec::new(),
            undone: Vec::new(),
            drag: None,
            typing: None,
            focus: None,
            laid: String::new(),
            saved_as: None,
        }
    }

    #[test]
    fn a_drag_draws_in_the_images_pixels_its_sizes_scaled_with_it() {
        let mut e = editor(2000, 1000);
        // The image shown 1000 wide: two of its pixels a logical one.
        let image = Rectangle::new(Point::new(100.0, 50.0), Size::new(1000.0, 500.0));
        let drag = Drag {
            from: Point::new(200.0, 100.0),
            to: Point::new(300.0, 150.0),
        };
        let z = 1.5;
        assert_eq!(
            e.mark_of(image, drag, z),
            Some(Mark::Arrow {
                from: (200.0, 100.0),
                to: (400.0, 200.0),
                width: 12.0,
                head: 48.0,
                colour: COLOURS[0],
            })
        );
        e.tool = Tool::Blur;
        assert_eq!(
            e.mark_of(image, drag, z),
            Some(Mark::Blur {
                rect: marks::Rect {
                    x: 200.0,
                    y: 100.0,
                    w: 200.0,
                    h: 100.0
                },
                radius: 21.0
            })
        );
        // Too short: nothing.
        let short = Drag {
            from: Point::new(200.0, 100.0),
            to: Point::new(205.0, 103.0),
        };
        assert_eq!(e.mark_of(image, short, z), None);
        e.tool = Tool::Text;
        assert_eq!(e.mark_of(image, drag, z), None);
    }

    #[test]
    fn undo_and_redo_go_through_the_marks_and_a_new_one_forgets_what_was_undone() {
        let mut e = editor(100, 100);
        let blur = |x| Mark::Blur {
            rect: marks::Rect {
                x,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            radius: 2.0,
        };
        e.add(blur(1.0));
        e.add(blur(2.0));
        assert!(e.undo());
        assert_eq!(e.marks, vec![blur(1.0)]);
        assert!(e.redo());
        assert_eq!(e.marks, vec![blur(1.0), blur(2.0)]);
        assert!(!e.redo());
        assert!(e.undo() && e.undo() && !e.undo());
        assert!(e.marks.is_empty());
        assert!(e.redo());
        e.add(blur(3.0));
        assert!(!e.redo());
        assert_eq!(e.marks, vec![blur(1.0), blur(3.0)]);
    }

    #[test]
    fn a_text_says_something_or_is_nothing() {
        let e = editor(1000, 500);
        let image = Rectangle::new(Point::ORIGIN, Size::new(1000.0, 500.0));
        let mut typing = Typing::at(Point::new(100.0, 50.0));
        typing.text.set("  ");
        assert_eq!(e.text_mark(image, &typing, 1.5), None);
        typing.text.set(" Ici ");
        assert_eq!(
            e.text_mark(image, &typing, 1.5),
            Some(Mark::Text {
                at: (110.5, 50.0),
                text: "Ici".to_owned(),
                size: 27.0,
                outline: 2.25,
                colour: COLOURS[0],
            })
        );
    }

    #[test]
    fn a_copy_saved_beside_never_takes_another_files_name() {
        let dir = tempfile::tempdir().expect("a folder");
        let name = "Capture du 2026-11-03 à 14-32-05 (annotée).png";
        assert_eq!(unused(dir.path(), name), dir.path().join(name));
        std::fs::write(dir.path().join(name), b"x").expect("written");
        assert_eq!(
            unused(dir.path(), name),
            dir.path()
                .join("Capture du 2026-11-03 à 14-32-05 (annotée) (2).png")
        );
        let file = dir.path().join("a.png");
        std::fs::write(&file, b"old").expect("written");
        replace(&file, b"new").expect("replaced");
        assert_eq!(std::fs::read(&file).expect("read"), b"new");
        assert!(write_new(&file, b"again").is_err());
    }
}
