//! The two editors the window can hold: the screenshot's annotation (#128)
//! and the recording's trim (#132). Each acts on what its view asks (copy,
//! save, export) itself; the window only draws it and hands it input.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    time::Duration,
};

use hocus_api::{Client as Api, Request};
use hocus_shell::Lang;
use hocus_ui::{AccessNode, Key, Point, Size, Theme};

use crate::{
    scene::Picture,
    trim::{TrimOutcome, TrimView},
    video::{self, Frames, Info},
    view::{AnnotateView, Outcome},
};

/// What the window holds.
pub trait Editor {
    fn app_id(&self) -> &'static str;
    fn title(&self) -> &'static str;
    fn resize(&mut self, size: Size, scale: f32);
    fn draw(&mut self, now: Duration);
    fn pixels(&self) -> &[u8];
    fn pixel_size(&self) -> (u32, u32);
    fn access(&self) -> AccessNode;
    /// Where the pointer was, and whether a drag is going on.
    fn pointer(&self) -> (Option<Point>, bool);
    fn pointer_move(&mut self, at: Point);
    /// Whether the window should close.
    fn pointer_button(&mut self, pressed: bool) -> bool;
    fn key(&mut self, key: Key) -> bool;
    fn text(&mut self, text: &str);
    /// Ctrl and a letter.
    fn shortcut(&mut self, letter: char) -> bool;
    /// Work done off the window's thread came back: whether to draw again.
    fn poll(&mut self) -> bool;
}

/// The screenshot's annotation: copied through the compositor, which keeps
/// the clipboard after the window is gone; saved next to the capture.
#[derive(Debug)]
pub struct Annotate {
    view: AnnotateView,
    file: PathBuf,
    api: Option<PathBuf>,
    lang: Lang,
}

impl Annotate {
    pub fn new(
        theme: Theme,
        lang: Lang,
        size: Size,
        file: &Path,
        api: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        use anyhow::Context as _;
        let png =
            std::fs::read(file).with_context(|| format!("could not read {}", file.display()))?;
        let picture = Picture::from_png(&png)?;
        Ok(Self {
            view: AnnotateView::new(theme, lang, size, 1.0, picture),
            file: file.to_path_buf(),
            api,
            lang,
        })
    }

    /// Whether the window should close.
    fn act(&mut self, outcomes: Vec<Outcome>) -> bool {
        let lang = self.lang;
        let mut close = false;
        for outcome in outcomes {
            match outcome {
                Outcome::Close => close = true,
                Outcome::Save(png) => match crate::client::save_beside(&self.file, &png) {
                    Ok(path) => {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        self.view.set_note(Some(lang.annotate_saved(&name)));
                    }
                    Err(err) => {
                        tracing::warn!(%err, "could not save the annotated capture");
                        self.view.set_note(Some(err.to_string()));
                    }
                },
                Outcome::Copy(png) => {
                    let Some(api) = self.api.clone() else {
                        continue;
                    };
                    let copied = Api::connect(&api).and_then(|mut c| {
                        use base64::Engine;
                        c.request(Request::CopyPng {
                            png_base64: base64::engine::general_purpose::STANDARD.encode(&png),
                        })
                        .map(drop)
                    });
                    match copied {
                        Ok(()) => self.view.set_note(Some(lang.annotate_copied().into())),
                        Err(err) => tracing::warn!(%err, "could not copy the annotated capture"),
                    }
                }
            }
        }
        close
    }
}

impl Editor for Annotate {
    fn app_id(&self) -> &'static str {
        "hocus-annotate"
    }

    fn title(&self) -> &'static str {
        self.lang.annotate()
    }

    fn resize(&mut self, size: Size, scale: f32) {
        self.view.resize(size, scale);
    }

    fn draw(&mut self, now: Duration) {
        let _ = self.view.draw(now);
    }

    fn pixels(&self) -> &[u8] {
        self.view.pixels()
    }

    fn pixel_size(&self) -> (u32, u32) {
        self.view.pixel_size()
    }

    fn access(&self) -> AccessNode {
        self.view.access()
    }

    fn pointer(&self) -> (Option<Point>, bool) {
        self.view.pointer()
    }

    fn pointer_move(&mut self, at: Point) {
        self.view.pointer_move(at);
    }

    fn pointer_button(&mut self, pressed: bool) -> bool {
        let out = self.view.pointer_button(pressed);
        self.act(out)
    }

    fn key(&mut self, key: Key) -> bool {
        let out = self.view.key(key);
        self.act(out)
    }

    fn text(&mut self, text: &str) {
        self.view.text(text);
    }

    fn shortcut(&mut self, letter: char) -> bool {
        let out = self.view.shortcut(letter);
        self.act(out)
    }

    fn poll(&mut self) -> bool {
        false
    }
}

/// What the encoder's workers send back.
#[derive(Debug)]
enum Back {
    Info(Info),
    Thumbnails(Frames),
    /// Opening is done: what it is, its first frame and its thumbnails
    /// came, or could not be read.
    Opened,
    /// A frame, its seek's number, and its time while playing.
    Frame(u64, Frames, Option<Duration>),
    /// Playing ended, for the play of this number.
    Played(u64),
    Exported(Result<PathBuf, String>),
    /// The recording could not be read.
    Failed(String),
}

/// How wide the preview's frames are.
const PREVIEW: u32 = 960;

/// The recording's trim: the encoder read and run on worker threads.
#[derive(Debug)]
pub struct Trim {
    view: TrimView,
    file: PathBuf,
    lang: Lang,
    encoder: String,
    probe: String,
    info: Option<Info>,
    back: Receiver<Back>,
    sender: Sender<Back>,
    /// Seeks asked, and the latest whose frame shows: an older one's
    /// frame never replaces a newer one's.
    seeks: Arc<AtomicU64>,
    shown: u64,
    /// Stops what plays.
    playing: Option<Arc<AtomicBool>>,
    /// Its first frame and thumbnails are still being read.
    opening: bool,
}

impl Trim {
    pub fn new(theme: Theme, lang: Lang, size: Size, file: &Path) -> Self {
        let (sender, back) = channel();
        let trim = Self {
            view: TrimView::new(theme, lang, size, 1.0),
            file: file.to_path_buf(),
            lang,
            encoder: "ffmpeg".into(),
            probe: "ffprobe".into(),
            info: None,
            back,
            sender,
            seeks: Arc::new(AtomicU64::new(0)),
            shown: 0,
            playing: None,
            opening: true,
        };
        // What it is, its thumbnails, its first frame.
        let (probe, encoder, file, send) = (
            trim.probe.clone(),
            trim.encoder.clone(),
            trim.file.clone(),
            trim.sender.clone(),
        );
        spawn("trim open", move || {
            match video::info(&probe, &file) {
                Ok(info) => {
                    let _ = send.send(Back::Info(info));
                    if let Ok(frame) =
                        video::frame_at(&encoder, &file, &info, Duration::ZERO, PREVIEW)
                    {
                        let _ = send.send(Back::Frame(0, frame, None));
                    }
                    match video::thumbnails(&encoder, &file, &info, 12, 64) {
                        Ok(thumbs) => {
                            let _ = send.send(Back::Thumbnails(thumbs));
                        }
                        Err(err) => tracing::warn!(err, "no thumbnails"),
                    }
                }
                Err(err) => {
                    let _ = send.send(Back::Failed(err));
                }
            }
            let _ = send.send(Back::Opened);
        });
        trim
    }

    fn act(&mut self, outcomes: Vec<TrimOutcome>) -> bool {
        let mut close = false;
        for outcome in outcomes {
            let Some(info) = self.info else {
                close |= outcome == TrimOutcome::Close;
                continue;
            };
            let (encoder, file, send) =
                (self.encoder.clone(), self.file.clone(), self.sender.clone());
            match outcome {
                TrimOutcome::Close => close = true,
                TrimOutcome::Seek(at) => {
                    let n = self.seeks.fetch_add(1, Ordering::Relaxed) + 1;
                    spawn("trim seek", move || {
                        if let Ok(frame) = video::frame_at(&encoder, &file, &info, at, PREVIEW) {
                            let _ = send.send(Back::Frame(n, frame, None));
                        }
                    });
                }
                TrimOutcome::Play(from, to) => {
                    self.stop();
                    let stop = Arc::new(AtomicBool::new(false));
                    self.playing = Some(stop.clone());
                    let n = self.seeks.fetch_add(1, Ordering::Relaxed) + 1;
                    spawn("trim play", move || {
                        let mut shown = 0u32;
                        let played = video::play(
                            &encoder,
                            &file,
                            &info,
                            (from, to),
                            PREVIEW,
                            |rgba, w, h| {
                                // Frames come twenty a second.
                                let at = from + Duration::from_millis(u64::from(shown) * 50);
                                shown += 1;
                                let frame = Frames {
                                    width: w,
                                    height: h,
                                    rgba: vec![rgba],
                                };
                                !stop.load(Ordering::Relaxed)
                                    && send.send(Back::Frame(n, frame, Some(at))).is_ok()
                            },
                        );
                        if let Err(err) = played {
                            tracing::warn!(err, "could not play");
                        }
                        let _ = send.send(Back::Played(n));
                    });
                }
                TrimOutcome::Stop => self.stop(),
                TrimOutcome::Export(what) => {
                    self.view.set_exporting(true);
                    self.view.set_note(None);
                    let out = video::beside(&file, what.format);
                    spawn("trim export", move || {
                        let done = video::export(&encoder, &file, &info, &what, &out);
                        let _ = send.send(Back::Exported(done));
                    });
                }
            }
        }
        close
    }

    fn stop(&mut self) {
        if let Some(stop) = self.playing.take() {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

impl Drop for Trim {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Editor for Trim {
    fn app_id(&self) -> &'static str {
        "hocus-trim"
    }

    fn title(&self) -> &'static str {
        self.lang.trim()
    }

    fn resize(&mut self, size: Size, scale: f32) {
        self.view.resize(size, scale);
    }

    fn draw(&mut self, now: Duration) {
        let _ = self.view.draw(now);
    }

    fn pixels(&self) -> &[u8] {
        self.view.pixels()
    }

    fn pixel_size(&self) -> (u32, u32) {
        self.view.pixel_size()
    }

    fn access(&self) -> AccessNode {
        self.view.access()
    }

    /// Busy too while the latest seek's frame has not come, and while
    /// the thumbnails are read: a test's capture waits for them (#292).
    fn pointer(&self) -> (Option<Point>, bool) {
        let (at, held) = self.view.pointer();
        let waiting = self.playing.is_none() && self.shown < self.seeks.load(Ordering::Relaxed);
        (at, held || waiting || self.opening)
    }

    fn pointer_move(&mut self, at: Point) {
        let out = self.view.pointer_move(at);
        self.act(out);
    }

    fn pointer_button(&mut self, pressed: bool) -> bool {
        let out = self.view.pointer_button(pressed);
        self.act(out)
    }

    fn key(&mut self, key: Key) -> bool {
        let out = self.view.key(key);
        self.act(out)
    }

    fn text(&mut self, text: &str) {
        let out = self.view.text(text);
        self.act(out);
    }

    fn shortcut(&mut self, _: char) -> bool {
        false
    }

    fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(back) = self.back.try_recv() {
            changed = true;
            match back {
                Back::Info(info) => {
                    self.info = Some(info);
                    self.view.set_length(info.length);
                }
                Back::Thumbnails(thumbs) => self.view.set_thumbnails(thumbs),
                Back::Opened => self.opening = false,
                // Older than what shows: a newer one is there.
                Back::Frame(n, mut frame, at) if n >= self.shown => {
                    self.shown = n;
                    if let Some(rgba) = frame.rgba.pop() {
                        self.view.set_preview((frame.width, frame.height, rgba), at);
                    }
                }
                Back::Frame(..) => {}
                Back::Played(n) if n == self.seeks.load(Ordering::Relaxed) => {
                    self.playing = None;
                    self.view.played();
                }
                Back::Played(_) => {}
                Back::Failed(err) => self.view.set_note(Some(err)),
                Back::Exported(done) => {
                    self.view.set_exporting(false);
                    let note = match done {
                        Ok(path) => self.lang.exported(
                            &path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                        ),
                        Err(err) => self.lang.export_failed(&err),
                    };
                    self.view.set_note(Some(note));
                }
            }
        }
        changed
    }
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(err) = std::thread::Builder::new().name(name.into()).spawn(work) {
        tracing::warn!(%err, "could not start a thread");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trim_editor_shows_frames_as_its_handles_move() {
        if std::process::Command::new("ffprobe")
            .arg("-version")
            .output()
            .is_err()
        {
            eprintln!("skipped: no ffmpeg");
            return;
        }
        let dir = tempfile::tempdir().expect("a directory");
        let file = dir.path().join("Recording.mp4");
        let made = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .args(["color=c=0x3399ff:size=320x240:rate=30:duration=10"])
            .args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
            .arg(&file)
            .status()
            .expect("ffmpeg");
        assert!(made.success());
        let mut trim = Trim::new(Theme::default(), Lang::En, Size::new(900.0, 700.0), &file);
        let shows = |t: &mut Trim| {
            t.poll();
            t.draw(Duration::ZERO);
            t.access().dump().contains("image \"Trim the recording\"")
        };
        let wait = |t: &mut Trim| {
            let until = std::time::Instant::now() + Duration::from_secs(10);
            while !shows(t) {
                assert!(
                    std::time::Instant::now() < until,
                    "no frame:\n{}",
                    t.access().dump()
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        wait(&mut trim);
        for _ in 0..3 {
            trim.key(Key::Up);
        }
        let until = std::time::Instant::now() + Duration::from_secs(10);
        while trim.seeks.load(Ordering::Relaxed) != 3 || !shows(&mut trim) {
            assert!(std::time::Instant::now() < until);
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(500));
        assert!(shows(&mut trim), "{}", trim.access().dump());
    }
}
