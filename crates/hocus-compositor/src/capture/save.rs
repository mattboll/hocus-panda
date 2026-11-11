//! What becomes of a capture, on a thread of its own: its pieces (the
//! screens' pixels as drawn for it) cut or put together into one image,
//! encoded as PNG for the clipboard and written, as PNG or JPEG
//! (`[capture] format`, 6.17), to the folder chosen (`[capture] folder`)
//! or the pictures' folder's own; and where that folder is, what the
//! file is named, and the area kept for next time.

use hocus_words::compositor::capture_save as txt;
use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::Context;

use super::geometry::Area;
use crate::config::CaptureFormat;

/// A JPEG's quality, of 100.
const JPEG_QUALITY: u8 = 90;

/// The pixels of a screen (or a window) as drawn for the capture: rows of
/// premultiplied ARGB8888 (B, G, R, A in memory), `size` pixels, its top
/// left corner at `at` in the desktop's logical space, `scale` pixels to
/// a logical one.
#[derive(Clone)]
pub struct Piece {
    pub pixels: Vec<u8>,
    pub size: (u32, u32),
    pub at: (f64, f64),
    pub scale: f64,
}

impl std::fmt::Debug for Piece {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Piece")
            .field("size", &self.size)
            .field("at", &self.at)
            .field("scale", &self.scale)
            .finish_non_exhaustive()
    }
}

/// What the thread is handed.
#[derive(Debug)]
pub struct Job {
    pub pieces: Vec<Piece>,
    /// The part of the only piece kept, in its pixels: an area.
    pub cut: Option<(u32, u32, u32, u32)>,
    /// When it was taken: its file's name says it, in local time.
    pub when: SystemTime,
    /// The pictures' folder (`$XDG_PICTURES_DIR`); none: not saved,
    /// unless a folder is chosen.
    pub pictures: Option<PathBuf>,
    /// The folder `[capture] folder` names, if it does: used when it can
    /// be written, else the default one is.
    pub chosen: Option<PathBuf>,
    /// The file's format.
    pub format: CaptureFormat,
    pub lang: hocus_words::Lang,
}

/// What the thread made of it.
#[derive(Debug)]
pub struct Saved {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// The file written, if it was.
    pub file: Option<PathBuf>,
    /// The folder as the user reads it: `Images/Captures d'écran`.
    pub folder: String,
}

/// The pieces made one PNG and, with a folder, written there in the
/// format asked. The image is made even when the file cannot be written:
/// the clipboard still gets it.
pub fn save(job: Job) -> anyhow::Result<Saved> {
    let image = compose(&job.pieces, job.cut).context("nothing to capture")?;
    let (width, height) = image.dimensions();
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .context("could not encode the capture")?;
    let mut file = None;
    let mut folder = String::new();
    let place = match &job.chosen {
        Some(chosen) if writable(chosen) => Some((chosen.clone(), shown_chosen(chosen))),
        chosen => {
            if let Some(chosen) = chosen {
                tracing::info!(
                    folder = %chosen.display(),
                    "the captures' folder cannot be written: the default one is used"
                );
            }
            job.pictures.as_ref().map(|pictures| {
                (
                    pictures.join(folder_name(job.lang)),
                    shown_folder(pictures, job.lang),
                )
            })
        }
    };
    if let Some((dir, shown)) = place {
        let written = std::fs::create_dir_all(&dir)
            .with_context(|| format!("could not make {}", dir.display()))
            .and_then(|()| {
                let bytes = match job.format {
                    CaptureFormat::Png => png.clone(),
                    CaptureFormat::Jpeg => jpeg(&image)?,
                };
                let name = file_name(job.when, job.lang, job.format.extension());
                let path = unused(&dir, &name);
                write_new(&path, &bytes)
                    .with_context(|| format!("could not write {}", path.display()))?;
                Ok(path)
            });
        match written {
            Ok(path) => {
                folder = shown;
                file = Some(path);
            }
            Err(err) => tracing::warn!("{err:#}: the capture is only copied"),
        }
    }
    Ok(Saved {
        png,
        width,
        height,
        file,
        folder,
    })
}

/// `image` as a JPEG, at [`JPEG_QUALITY`], what no screen covers black.
fn jpeg(image: &image::RgbaImage) -> anyhow::Result<Vec<u8>> {
    let rgb = image::DynamicImage::ImageRgba8(image.clone()).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
        .encode_image(&rgb)
        .context("could not encode the capture as JPEG")?;
    Ok(out)
}

/// Whether `dir` is a folder this user can write in: not gone (a USB key
/// unplugged), not read-only. A folder gone is not made again: the key
/// stays, and applies again once it is back.
pub fn writable(dir: &Path) -> bool {
    dir.is_dir() && rustix::fs::access(dir, rustix::fs::Access::WRITE_OK).is_ok()
}

/// A chosen folder as the toast says it: its parent's name, then its own.
pub fn shown_chosen(dir: &Path) -> String {
    let name = |p: Option<&Path>| {
        p.and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let (parent, own) = (name(dir.parent()), name(Some(dir)));
    if parent.is_empty() {
        own
    } else {
        format!("{parent}/{own}")
    }
}

/// The file, made for it alone (never over another).
fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// The pieces as one image: the only piece (cut to `cut`), or all of
/// them where the screens are, at the largest of their scales (a screen
/// of a smaller one stretched to it), transparent where no screen is.
fn compose(pieces: &[Piece], cut: Option<(u32, u32, u32, u32)>) -> Option<image::RgbaImage> {
    if let [piece] = pieces {
        let image = rgba(piece)?;
        return Some(match cut {
            Some((x, y, w, h)) => {
                let (iw, ih) = image.dimensions();
                let x = x.min(iw.saturating_sub(1));
                let y = y.min(ih.saturating_sub(1));
                let w = w.clamp(1, iw - x);
                let h = h.clamp(1, ih - y);
                image::imageops::crop_imm(&image, x, y, w, h).to_image()
            }
            None => image,
        });
    }
    let scale = pieces.iter().map(|p| p.scale).fold(0.0, f64::max);
    if scale <= 0.0 {
        return None;
    }
    let place = |p: &Piece| {
        let w = f64::from(p.size.0) / p.scale * scale;
        let h = f64::from(p.size.1) / p.scale * scale;
        (p.at.0 * scale, p.at.1 * scale, w, h)
    };
    let left = pieces
        .iter()
        .map(|p| place(p).0)
        .fold(f64::INFINITY, f64::min);
    let top = pieces
        .iter()
        .map(|p| place(p).1)
        .fold(f64::INFINITY, f64::min);
    let right = pieces
        .iter()
        .map(|p| place(p).0 + place(p).2)
        .fold(f64::NEG_INFINITY, f64::max);
    let bottom = pieces
        .iter()
        .map(|p| place(p).1 + place(p).3)
        .fold(f64::NEG_INFINITY, f64::max);
    let (w, h) = ((right - left).round() as u32, (bottom - top).round() as u32);
    if w == 0 || h == 0 {
        return None;
    }
    let mut all = image::RgbaImage::new(w, h);
    for piece in pieces {
        let (x, y, pw, ph) = place(piece);
        let mut image = rgba(piece)?;
        let (pw, ph) = (pw.round().max(1.0) as u32, ph.round().max(1.0) as u32);
        if image.dimensions() != (pw, ph) {
            image = image::imageops::resize(&image, pw, ph, image::imageops::FilterType::Triangle);
        }
        image::imageops::replace(
            &mut all,
            &image,
            (x - left).round() as i64,
            (y - top).round() as i64,
        );
    }
    Some(all)
}

/// A piece's pixels as straight RGBA, as PNG takes them.
fn rgba(piece: &Piece) -> Option<image::RgbaImage> {
    let (w, h) = piece.size;
    let len = (w as usize) * (h as usize) * 4;
    let pixels = piece.pixels.get(..len)?;
    let mut out = Vec::with_capacity(len);
    for &[b, g, r, a] in pixels.as_chunks::<4>().0 {
        let straight = |v: u8| -> u8 {
            match a {
                0 => 0,
                255 => v,
                a => ((u32::from(v) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8,
            }
        };
        out.extend_from_slice(&[straight(r), straight(g), straight(b), a]);
    }
    image::RgbaImage::from_raw(w, h, out)
}

/// The folder captures go in, in the pictures' folder.
pub fn folder_name(lang: hocus_words::Lang) -> &'static str {
    txt::SCREENSHOTS.get(lang)
}

/// The folder as the toast says it: the pictures' folder's own name, then
/// the captures'.
fn shown_folder(pictures: &Path, lang: hocus_words::Lang) -> String {
    let base = pictures
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if base.is_empty() {
        folder_name(lang).to_owned()
    } else {
        format!("{base}/{}", folder_name(lang))
    }
}

/// "Capture du 2026-11-03 à 14-32-05.png", "Screenshot from 2026-11-03
/// 14-32-05.png", at `when` in the local time zone, its extension
/// `extension`.
pub fn file_name(when: SystemTime, lang: hocus_words::Lang, extension: &str) -> String {
    let zoned = jiff::Timestamp::try_from(when)
        .map(|t| t.to_zoned(jiff::tz::TimeZone::system()))
        .unwrap_or_else(|_| jiff::Timestamp::UNIX_EPOCH.to_zoned(jiff::tz::TimeZone::UTC));
    let day = zoned.strftime("%Y-%m-%d");
    let time = zoned.strftime("%H-%M-%S");
    txt::screenshot_from(
        lang,
        txt::ScreenshotFrom {
            day: &day,
            extension: &extension,
            time: &time,
        },
    )
}

/// `name` in `dir`, or with " (2)", " (3)"… before its extension when a
/// file has it (two captures in the same second).
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

/// The pictures' folder: `$XDG_PICTURES_DIR`, else what
/// `$XDG_CONFIG_HOME/user-dirs.dirs` says (xdg-user-dirs), else
/// `~/Pictures`.
pub fn pictures_dir() -> Option<PathBuf> {
    xdg_dir("XDG_PICTURES_DIR", "Pictures")
}

/// The user's folder `key` (`XDG_PICTURES_DIR`, `XDG_VIDEOS_DIR`): the
/// variable, else what `$XDG_CONFIG_HOME/user-dirs.dirs` says, else
/// `fallback` in the home.
pub(crate) fn xdg_dir(key: &str, fallback: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    if let Some(dir) = std::env::var_os(key)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return Some(dir);
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    let said = std::fs::read_to_string(config.join("user-dirs.dirs"))
        .ok()
        .and_then(|text| user_dir(&text, key, &home));
    Some(said.unwrap_or_else(|| home.join(fallback)))
}

/// The folder `key` names in a `user-dirs.dirs` file: `"$HOME/Images"` or
/// an absolute path; the home itself means none (the specification's
/// "disabled").
pub(crate) fn user_dir(text: &str, key: &str, home: &Path) -> Option<PathBuf> {
    let value = text.lines().find_map(|line| {
        let (name, value) = line.trim().split_once('=')?;
        (name.trim() == key).then(|| value.trim().trim_matches('"').to_owned())
    })?;
    let path = match value.strip_prefix("$HOME") {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None => PathBuf::from(&value),
    };
    (path.is_absolute() && path != home).then_some(path)
}

/// The area kept for next time, on its screen: `OUTPUT X Y W H` in
/// `capture-area` of the state directory.
pub fn area_file(state_dir: &Path) -> PathBuf {
    state_dir.join("capture-area")
}

pub fn read_area(text: &str) -> Option<(String, Area)> {
    let mut words = text.split_whitespace();
    let output = words.next()?.to_owned();
    let mut number = || words.next()?.parse::<f64>().ok().filter(|v| v.is_finite());
    let area = Area {
        x: number()?,
        y: number()?,
        w: number()?,
        h: number()?,
    };
    area.drawn().then_some((output, area))
}

pub fn write_area(output: &str, area: Area) -> String {
    format!("{output} {} {} {} {}\n", area.x, area.y, area.w, area.h)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A piece `w` × `h` of one colour (B, G, R, A).
    fn piece(w: u32, h: u32, bgra: [u8; 4], at: (f64, f64), scale: f64) -> Piece {
        Piece {
            pixels: bgra.repeat((w * h) as usize),
            size: (w, h),
            at,
            scale,
        }
    }

    #[test]
    fn an_area_is_cut_from_its_screen() -> anyhow::Result<()> {
        let screen = piece(100, 80, [10, 20, 30, 255], (0.0, 0.0), 1.0);
        let image = compose(&[screen], Some((10, 20, 30, 40))).context("an image")?;
        assert_eq!(image.dimensions(), (30, 40));
        // Red, green, blue: the bytes turned around.
        assert_eq!(image.get_pixel(0, 0).0, [30, 20, 10, 255]);
        Ok(())
    }

    #[test]
    fn all_screens_are_laid_out_at_the_largest_scale() -> anyhow::Result<()> {
        // A 1000 × 500 screen at 2 (500 × 250 logical), and on its right
        // a 400 × 300 one at 1: the second stretched to twice its size.
        let hidpi = piece(1000, 500, [0, 0, 255, 255], (0.0, 0.0), 2.0);
        let plain = piece(400, 300, [255, 0, 0, 255], (500.0, 0.0), 1.0);
        let image = compose(&[hidpi, plain], None).context("an image")?;
        assert_eq!(image.dimensions(), (1800, 600));
        assert_eq!(image.get_pixel(10, 10).0, [255, 0, 0, 255]);
        assert_eq!(image.get_pixel(1500, 590).0, [0, 0, 255, 255]);
        // Under the first screen, nothing: transparent.
        assert_eq!(image.get_pixel(10, 590).0[3], 0);
        Ok(())
    }

    #[test]
    fn premultiplied_pixels_come_out_straight() -> anyhow::Result<()> {
        let window = piece(2, 2, [0, 64, 128, 128], (0.0, 0.0), 1.0);
        let image = compose(&[window], None).context("an image")?;
        assert_eq!(image.get_pixel(0, 0).0, [255, 128, 0, 128]);
        Ok(())
    }

    #[test]
    fn the_files_name_says_when_in_the_users_language() {
        let when = SystemTime::UNIX_EPOCH;
        let fr = file_name(when, hocus_words::Lang::French, "png");
        assert!(fr.starts_with("Capture du ") && fr.contains(" à ") && fr.ends_with(".png"));
        let en = file_name(when, hocus_words::Lang::English, "png");
        assert!(en.starts_with("Screenshot from ") && en.ends_with(".png"));
        // "2026-11-03 14-32-05": the date, then the time with dashes.
        let stamp = en
            .trim_start_matches("Screenshot from ")
            .trim_end_matches(".png");
        assert_eq!(stamp.len(), 19);
        assert!(!stamp.contains(':'));
    }

    #[test]
    fn the_pictures_folder_is_read_from_user_dirs() {
        let home = Path::new("/home/alex");
        let text =
            "# comment\nXDG_DESKTOP_DIR=\"$HOME/Bureau\"\nXDG_PICTURES_DIR=\"$HOME/Images\"\n";
        assert_eq!(
            user_dir(text, "XDG_PICTURES_DIR", home),
            Some(PathBuf::from("/home/alex/Images"))
        );
        let absolute = "XDG_PICTURES_DIR=\"/data/photos\"";
        assert_eq!(
            user_dir(absolute, "XDG_PICTURES_DIR", home),
            Some(PathBuf::from("/data/photos"))
        );
        // The home itself: none; and none said.
        assert_eq!(
            user_dir("XDG_PICTURES_DIR=\"$HOME/\"", "XDG_PICTURES_DIR", home),
            None
        );
        assert_eq!(
            user_dir("XDG_MUSIC_DIR=\"$HOME/Musique\"", "XDG_PICTURES_DIR", home),
            None
        );
        assert_eq!(
            shown_folder(Path::new("/home/alex/Images"), hocus_words::Lang::French),
            "Images/Captures d'écran"
        );
    }

    #[test]
    fn it_is_saved_once_and_a_second_in_the_same_second_beside_it() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let job = || Job {
            pieces: vec![piece(4, 3, [1, 2, 3, 255], (0.0, 0.0), 1.0)],
            cut: None,
            when: SystemTime::UNIX_EPOCH,
            pictures: Some(dir.path().to_path_buf()),
            chosen: None,
            format: CaptureFormat::Png,
            lang: hocus_words::Lang::English,
        };
        let first = save(job())?;
        let second = save(job())?;
        let (Some(a), Some(b)) = (first.file, second.file) else {
            anyhow::bail!("not saved");
        };
        assert_ne!(a, b);
        assert!(b.to_string_lossy().ends_with(" (2).png"));
        assert_eq!(a.parent(), Some(dir.path().join("Screenshots").as_path()));
        let read = image::load_from_memory(&std::fs::read(&a)?)?;
        assert_eq!((read.width(), read.height()), (4, 3));
        assert_eq!(
            first.folder,
            format!(
                "{}/Screenshots",
                dir.path()
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            )
        );
        Ok(())
    }

    #[test]
    fn the_area_kept_reads_back() {
        let area = Area {
            x: 12.5,
            y: 30.0,
            w: 600.0,
            h: 380.0,
        };
        assert_eq!(
            read_area(&write_area("eDP-1", area)),
            Some(("eDP-1".to_owned(), area))
        );
        assert_eq!(read_area("eDP-1 1 2 3"), None);
        assert_eq!(read_area("eDP-1 0 0 2 2"), None);
    }
}
