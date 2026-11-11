//! The wallpaper (6.3, issue #403): what `[wallpaper]` and `[outputs.NAME]
//! wallpaper` say, and how a screen's background is drawn from it. The
//! compositor draws it under the windows, the shell under the lock, the
//! Settings in their thumbnails and preview: all three with [`render`], on
//! a thread of their own, never on the compositor's loop nor on a window's
//! thread.
//!
//! A built-in ([`Backdrop`]) is computed at a quarter of the screen's
//! pixels and drawn stretched, and so is the plain colour; a picture of one's own (JPEG, PNG or WebP, its EXIF
//! orientation applied) is scaled and cropped to the screen's pixels for
//! its fit, the decoded source dropped once scaled. A picture that does not
//! read draws Nuit, and says why.

use std::{
    fmt,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    BadLine,
    backdrop::{self, Backdrop},
    moving::{self, Moving},
    theme::AccentColours,
};

/// `[wallpaper] picture`, or a screen's `wallpaper`: a built-in by its
/// word, `"plain"`, or a picture's path (it holds a `/`; `~/` is the home).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Picture {
    Builtin(Backdrop),
    /// A built-in that moves (#433): the compositor's shader draws it,
    /// its still picture where it does not run.
    Moving(Moving),
    /// The `color`, the whole screen.
    Plain,
    /// A picture's path, as the file writes it.
    File(String),
}

impl Default for Picture {
    fn default() -> Self {
        Self::Builtin(Backdrop::Night)
    }
}

impl Picture {
    /// What the file's text says; an unknown word is refused.
    pub fn of(text: &str) -> Result<Self, BadLine> {
        if text == "plain" {
            return Ok(Self::Plain);
        }
        if let Some(builtin) = Backdrop::of(text) {
            return Ok(Self::Builtin(builtin));
        }
        if let Some(moving) = Moving::of(text) {
            return Ok(Self::Moving(moving));
        }
        if text.contains('/') && !text.contains(['\t', '\n', '\r']) {
            return Ok(Self::File(text.to_owned()));
        }
        Err(BadLine(format!(
            "no wallpaper {text:?}: night, mist, dawn, forest, rain, stars, aurora, plain or a picture's path"
        )))
    }

    /// As the file writes it.
    pub fn word(&self) -> &str {
        match self {
            Self::Builtin(builtin) => builtin.word(),
            Self::Moving(moving) => moving.word(),
            Self::Plain => "plain",
            Self::File(path) => path,
        }
    }

    /// A picture's file, its `~/` the home.
    pub fn path(&self) -> Option<PathBuf> {
        match self {
            Self::File(text) => Some(expand(text)),
            _ => None,
        }
    }
}

impl fmt::Display for Picture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

/// `~/…` from the home (`HOME`); anything else as it is.
pub fn expand(text: &str) -> PathBuf {
    match (text.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest),
        _ => PathBuf::from(text),
    }
}

/// Where the pictures chosen in the Settings are copied:
/// `$XDG_DATA_HOME/hocuspanda/wallpapers` (`~/.local/share/…`).
pub fn folder() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|d| d.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/share")))?;
    Some(data.join("hocuspanda/wallpapers"))
}

/// `[wallpaper] fit`: how a picture of one's own fills its screen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fit {
    /// Scaled to cover the screen, centred; what overflows is cut.
    #[default]
    Cover,
    /// The whole picture, the colour on its sides.
    Contain,
    /// At its size, in the middle, the colour around it.
    Center,
}

impl Fit {
    pub const ALL: [Self; 3] = [Self::Cover, Self::Contain, Self::Center];

    pub const fn word(self) -> &'static str {
        match self {
            Self::Cover => "cover",
            Self::Contain => "contain",
            Self::Center => "center",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.word() == word)
    }
}

/// `[wallpaper] lock`: what the lock screen shows under its veil.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LockLook {
    /// Each screen's wallpaper, dimmed by the veil (as before 6.3).
    #[default]
    Dimmed,
    /// The same, blurred (the background alone, never the windows).
    Blurred,
    /// `lock-picture`, dimmed.
    Picture,
}

impl LockLook {
    pub const ALL: [Self; 3] = [Self::Dimmed, Self::Blurred, Self::Picture];

    pub const fn word(self) -> &'static str {
        match self {
            Self::Dimmed => "dimmed",
            Self::Blurred => "blurred",
            Self::Picture => "picture",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.word() == word)
    }
}

/// The plain colour's six swatches: its value, its names in French and
/// English. Ardoise is the default.
pub const SWATCHES: [([u8; 3], &str, &str); 6] = [
    ([0x0B, 0x10, 0x20], "Nuit", "Night"),
    ([0x1B, 0x22, 0x33], "Ardoise", "Slate"),
    ([0x22, 0x32, 0x4D], "Bleu profond", "Deep blue"),
    ([0x1D, 0x3A, 0x2E], "Sapin", "Fir"),
    ([0x3A, 0x2A, 0x26], "Terre", "Earth"),
    ([0x2A, 0x2A, 0x30], "Graphite", "Graphite"),
];

/// `[wallpaper] color` by default: Ardoise.
pub const DEFAULT_COLOR: [u8; 3] = [0x1B, 0x22, 0x33];

/// `#RRGGBB`.
pub fn colour_of(text: &str) -> Option<[u8; 3]> {
    let hex = text.strip_prefix('#').filter(|h| h.len() == 6)?;
    let n = u32::from_str_radix(hex, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

/// `#rrggbb`, as the file writes it.
pub fn colour_word([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// What one screen shows: its picture, and for a picture of one's own its
/// fit and the colour around it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Look {
    pub picture: Picture,
    pub fit: Fit,
    pub color: [u8; 3],
}

/// The wallpapers as the compositor tells the shell (the lock draws them):
/// written `wallpaper LOCK\tLOCK-PICTURE\tPICTURE\tFIT\tRRGGBB`, then
/// for each screen with its own a tab, its name, a tab, its picture.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wallpapers {
    pub lock: LockLook,
    pub lock_picture: Picture,
    /// Every screen's, but those with their own.
    pub picture: Picture,
    pub fit: Fit,
    pub color: [u8; 3],
    /// The screens with their own (`per-screen`), by their names.
    pub outputs: Vec<(String, Picture)>,
}

impl Wallpapers {
    /// What `output` shows.
    pub fn look(&self, output: &str) -> Look {
        let picture = self
            .outputs
            .iter()
            .find(|(name, _)| name == output)
            .map_or_else(|| self.picture.clone(), |(_, p)| p.clone());
        Look {
            picture,
            fit: self.fit,
            color: self.color,
        }
    }

    /// What `output`'s lock shows: its wallpaper, or the lock's own
    /// picture.
    pub fn lock_look(&self, output: &str) -> Look {
        let picture = match self.lock {
            LockLook::Picture => self.lock_picture.clone(),
            LockLook::Dimmed | LockLook::Blurred => self.look(output).picture,
        };
        Look {
            picture,
            fit: self.fit,
            color: self.color,
        }
    }
}

/// For the log, not the line.
impl fmt::Display for Wallpapers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b] = self.color;
        write!(
            f,
            "{}\t{}\t{}\t{}\t{r:02x}{g:02x}{b:02x}",
            self.lock.word(),
            self.lock_picture,
            self.picture,
            self.fit.word()
        )?;
        for (name, picture) in &self.outputs {
            write!(f, "\t{}\t{picture}", crate::screen_name(name))?;
        }
        Ok(())
    }
}

/// A background drawn: its pixels (R, G, B, A in memory: `Abgr8888`) to
/// draw stretched over its screen, the accent it gives, and why it is Nuit
/// when the picture asked did not read.
#[derive(Clone, PartialEq, Eq)]
pub struct Rendered {
    pub width: i32,
    pub height: i32,
    pub pixels: Vec<u8>,
    pub accent: AccentColours,
    pub failed: Option<String>,
}

impl fmt::Debug for Rendered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rendered")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("accent", &self.accent)
            .field("failed", &self.failed)
            .finish_non_exhaustive()
    }
}

/// `look` for a screen `size` physical pixels large. Slow for a picture
/// (it is read and decoded): on a thread of its own.
pub fn render(look: &Look, size: (i32, i32)) -> Rendered {
    render_at(look, size, 1.0)
}

/// `look` for a screen `size` physical pixels large at `scale`: a moving
/// wallpaper's still picture is drawn at the screen's pixels, its cells at
/// their logical size (the others do not depend on the scale).
pub fn render_at(look: &Look, size: (i32, i32), scale: f64) -> Rendered {
    let size = (size.0.max(1), size.1.max(1));
    match &look.picture {
        Picture::Builtin(builtin) => builtin_rendered(*builtin, size, None),
        Picture::Moving(m) => Rendered {
            width: size.0,
            height: size.1,
            pixels: moving::still(*m, size, scale),
            accent: m.accent(),
            failed: None,
        },
        // As large as a built-in: drawn stretched, a picture of one pixel
        // would be blended with its outside at the screen's edges.
        Picture::Plain => {
            let [r, g, b] = look.color;
            let (width, height) = (
                (size.0 / backdrop::SHRINK).max(1),
                (size.1 / backdrop::SHRINK).max(1),
            );
            Rendered {
                width,
                height,
                pixels: [r, g, b, 255].repeat((width * height) as usize),
                accent: AccentColours::lifted(look.color),
                failed: None,
            }
        }
        Picture::File(text) => match picture(&expand(text), look.fit, look.color, size) {
            Ok(rendered) => rendered,
            Err(why) => builtin_rendered(Backdrop::Night, size, Some(why.to_string())),
        },
    }
}

fn builtin_rendered(builtin: Backdrop, size: (i32, i32), failed: Option<String>) -> Rendered {
    let (width, height, pixels) = backdrop::pixels(size, builtin);
    Rendered {
        width,
        height,
        pixels,
        accent: builtin.accent(),
        failed,
    }
}

/// Why a picture's file did not open; `name` is the file's, without its
/// folders (it is shown).
#[derive(Debug, thiserror::Error)]
pub enum PictureError {
    /// The file could not be read.
    #[error("{name}: {source}")]
    Read {
        name: String,
        source: std::io::Error,
    },
    /// It was read, and is not a picture the desktop decodes.
    #[error("{name} does not open as a picture: {source}")]
    Decode {
        name: String,
        source: image::ImageError,
    },
}

/// The picture's file opened, its format found from what it holds.
fn opened(
    path: &Path,
) -> Result<
    (
        image::ImageReader<std::io::BufReader<std::fs::File>>,
        String,
    ),
    PictureError,
> {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let read = |source| PictureError::Read {
        name: name.clone(),
        source,
    };
    let reader = image::ImageReader::open(path)
        .map_err(read)?
        .with_guessed_format()
        .map_err(read)?;
    Ok((reader, name))
}

/// A picture's file read and decoded, its EXIF orientation applied.
pub fn decode(path: &Path) -> Result<image::DynamicImage, PictureError> {
    use image::ImageDecoder;
    let (reader, name) = opened(path)?;
    let not_one = |source| PictureError::Decode {
        name: name.clone(),
        source,
    };
    let mut decoder = reader.into_decoder().map_err(not_one)?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut decoded = image::DynamicImage::from_decoder(decoder).map_err(not_one)?;
    decoded.apply_orientation(orientation);
    Ok(decoded)
}

/// A picture's size in pixels as its header says it, without decoding it:
/// whether it opens as a picture (the Settings check before copying).
pub fn dimensions(path: &Path) -> Result<(u32, u32), PictureError> {
    let (reader, name) = opened(path)?;
    reader
        .into_dimensions()
        .map_err(|source| PictureError::Decode { name, source })
}

fn picture(
    path: &Path,
    fit: Fit,
    color: [u8; 3],
    size: (i32, i32),
) -> Result<Rendered, PictureError> {
    use image::{GenericImageView, imageops::FilterType};
    let decoded = decode(path)?;
    let accent = dominant(&decoded);
    let (width, height) = (size.0 as u32, size.1 as u32);
    let image = match fit {
        Fit::Cover => decoded
            .resize_to_fill(width, height, FilterType::Triangle)
            .into_rgba8(),
        Fit::Contain | Fit::Center => {
            let [r, g, b] = color;
            let mut canvas =
                image::RgbaImage::from_pixel(width, height, image::Rgba([r, g, b, 255]));
            let placed = if fit == Fit::Contain {
                decoded.resize(width, height, FilterType::Triangle)
            } else {
                decoded
            };
            let (w, h) = placed.dimensions();
            let x = (i64::from(width) - i64::from(w)) / 2;
            let y = (i64::from(height) - i64::from(h)) / 2;
            image::imageops::overlay(&mut canvas, &placed.into_rgba8(), x, y);
            canvas
        }
    };
    Ok(Rendered {
        width: image.width() as i32,
        height: image.height() as i32,
        pixels: image.into_raw(),
        accent,
        failed: None,
    })
}

/// The picture's dominant hue, lifted to the accents' lightness: the
/// strongest of twelve hues, each pixel counted by its saturation and
/// value; a picture without colour gives a grey-blue.
pub fn dominant(picture: &image::DynamicImage) -> AccentColours {
    let small = picture.thumbnail(64, 64).into_rgb8();
    let mut bins = [(0.0f64, [0.0f64; 3]); 12];
    for pixel in small.pixels() {
        let [r, g, b] = pixel.0.map(|v| f64::from(v) / 255.0);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let d = max - min;
        if d <= f64::EPSILON || max <= f64::EPSILON {
            continue;
        }
        let hue = if max == r {
            60.0 * ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        let weight = (d / max) * max;
        let Some(bin) = bins.get_mut(((hue / 30.0) as usize).min(11)) else {
            continue;
        };
        bin.0 += weight;
        for (sum, v) in bin.1.iter_mut().zip([r, g, b]) {
            *sum += v * weight;
        }
    }
    let (weight, sums) =
        bins.iter().copied().fold(
            (0.0, [0.0; 3]),
            |best, bin| if bin.0 > best.0 { bin } else { best },
        );
    let total = small.pixels().len().max(1) as f64;
    if weight / total < 0.02 {
        return AccentColours::lifted([0x80, 0x80, 0x80]);
    }
    AccentColours::lifted(sums.map(|s| (s / weight * 255.0).round().clamp(0.0, 255.0) as u8))
}

/// The lock's "blurred": the background made small and blurred, once (it is
/// drawn stretched: larger, it stays soft).
pub fn soften(rendered: &Rendered) -> Rendered {
    use image::imageops::FilterType;
    let Some(source) = image::RgbaImage::from_raw(
        rendered.width as u32,
        rendered.height as u32,
        rendered.pixels.clone(),
    ) else {
        return rendered.clone();
    };
    let width = source.width().min(160);
    let height = (u64::from(source.height()) * u64::from(width) / u64::from(source.width().max(1)))
        .max(1) as u32;
    let mut small = image::imageops::resize(&source, width, height, FilterType::Triangle);
    for _ in 0..3 {
        box_blur(&mut small, 4);
    }
    Rendered {
        width: small.width() as i32,
        height: small.height() as i32,
        pixels: small.into_raw(),
        accent: rendered.accent,
        failed: rendered.failed.clone(),
    }
}

/// One pass of a box blur `radius` pixels wide, rows then columns, the
/// edges held.
fn box_blur(image: &mut image::RgbaImage, radius: i64) {
    let (w, h) = (i64::from(image.width()), i64::from(image.height()));
    for horizontal in [true, false] {
        let source = image.clone();
        let (lines, len) = if horizontal { (h, w) } else { (w, h) };
        for line in 0..lines {
            for at in 0..len {
                let mut sum = [0u32; 4];
                for k in -radius..=radius {
                    let i = (at + k).clamp(0, len - 1);
                    let (x, y) = if horizontal { (i, line) } else { (line, i) };
                    let p = source.get_pixel(x as u32, y as u32).0;
                    for (s, v) in sum.iter_mut().zip(p) {
                        *s += u32::from(v);
                    }
                }
                let n = (2 * radius + 1) as u32;
                let (x, y) = if horizontal { (at, line) } else { (line, at) };
                image.put_pixel(x as u32, y as u32, image::Rgba(sum.map(|s| (s / n) as u8)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_read() {
        assert_eq!(Picture::of("mist"), Ok(Picture::Builtin(Backdrop::Mist)));
        assert_eq!(Picture::of("plain"), Ok(Picture::Plain));
        assert_eq!(
            Picture::of("~/Images/Montagne.jpg"),
            Ok(Picture::File("~/Images/Montagne.jpg".to_owned()))
        );
        assert!(Picture::of("sunset").is_err());
        assert_eq!(colour_of("#1b2233"), Some(DEFAULT_COLOR));
        assert_eq!(colour_of("1b2233"), None);
        assert_eq!(colour_word(DEFAULT_COLOR), "#1b2233");
    }

    #[test]
    fn a_screen_shows_its_own_wallpaper_or_everyones() {
        let wallpapers = Wallpapers {
            lock: LockLook::Blurred,
            lock_picture: Picture::Builtin(Backdrop::Dawn),
            picture: Picture::Builtin(Backdrop::Mist),
            fit: Fit::Contain,
            color: [0x1D, 0x3A, 0x2E],
            outputs: vec![
                ("eDP-1".to_owned(), Picture::Builtin(Backdrop::Forest)),
                (
                    "DP-2".to_owned(),
                    Picture::File("/home/alex/Mes images/Lac au matin.jpg".to_owned()),
                ),
            ],
        };
        assert_eq!(
            wallpapers.lock_look("DP-2").picture,
            Picture::File("/home/alex/Mes images/Lac au matin.jpg".to_owned())
        );
        assert_eq!(
            wallpapers.lock_look("HDMI-A-1").picture,
            Picture::Builtin(Backdrop::Mist)
        );
    }

    #[test]
    fn a_picture_covers_contains_or_centres() -> Result<(), String> {
        let dir = std::env::temp_dir().join(format!("hocus-wallpaper-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join("red.png");
        // 40 × 20, red: covering 20 × 20 fills it; contained, the colour
        // above and below.
        image::RgbaImage::from_pixel(40, 20, image::Rgba([220, 30, 30, 255]))
            .save(&path)
            .map_err(|e| e.to_string())?;
        let look = |fit| Look {
            picture: Picture::File(path.display().to_string()),
            fit,
            color: [0, 0, 255],
        };
        let cover = render(&look(Fit::Cover), (20, 20));
        assert_eq!(
            (cover.width, cover.height, cover.failed.as_deref()),
            (20, 20, None)
        );
        assert_eq!(&cover.pixels[..3], &[220, 30, 30]);
        let contain = render(&look(Fit::Contain), (20, 20));
        assert_eq!(&contain.pixels[..3], &[0, 0, 255]);
        let middle = (10 * 20 + 10) * 4;
        assert_eq!(&contain.pixels[middle..middle + 3], &[220, 30, 30]);
        // Red's accent is a red.
        let [r, g, b] = cover.accent.base;
        assert!(r > g && r > b, "{:?}", cover.accent);
        // A file that is no picture: Nuit, and why.
        let bad = dir.join("bad.jpg");
        std::fs::write(&bad, b"not a picture").map_err(|e| e.to_string())?;
        let failed = render(
            &Look {
                picture: Picture::File(bad.display().to_string()),
                ..Look::default()
            },
            (64, 40),
        );
        assert!(failed.failed.is_some());
        assert_eq!(failed.accent, Backdrop::Night.accent());
        assert!(dimensions(&bad).is_err());
        assert_eq!(dimensions(&path).ok(), Some((40, 20)));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&bad);
        let _ = std::fs::remove_dir(&dir);
        Ok(())
    }

    #[test]
    fn blurred_is_small_and_soft() {
        let rendered = render(&Look::default(), (2560, 1600));
        let soft = soften(&rendered);
        assert!(soft.width <= 160, "{soft:?}");
        assert_eq!(soft.pixels.len(), (soft.width * soft.height * 4) as usize);
    }
}
