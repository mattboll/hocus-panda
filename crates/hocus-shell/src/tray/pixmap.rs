//! The items' icons, decoded on the tray's thread: an `IconPixmap` (ARGB32
//! in network byte order, several sizes), the best one scaled to the
//! icon's pixels; an `IconName`, looked for in the item's
//! `IconThemePath` first, then as the launcher looks for its icons (the
//! user's theme, those it inherits, `hicolor`, `pixmaps`); a dbusmenu
//! row's `icon-data` (PNG).

use std::{
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
};

use iced::advanced::image;

use crate::launcher::{
    entries,
    icons::{self, Themes},
};

/// A pixmap as an item gives it: its width, its height, its pixels.
pub type Pixmap = (i32, i32, Vec<u8>);

/// Pixmaps larger than this are not read.
const LARGEST: i32 = 1024;

/// Whether `pixmap` holds what it says.
fn sound((w, h, bytes): &Pixmap) -> bool {
    (1..=LARGEST).contains(w)
        && (1..=LARGEST).contains(h)
        && bytes.len() >= (*w as usize) * (*h as usize) * 4
}

/// The pixmap best for `px` pixels: the smallest at least that large,
/// else the largest.
pub fn best(pixmaps: &[Pixmap], px: u32) -> Option<&Pixmap> {
    let side = |p: &Pixmap| p.0.max(p.1).unsigned_abs();
    let sound: Vec<&Pixmap> = pixmaps.iter().filter(|p| sound(p)).collect();
    sound
        .iter()
        .copied()
        .filter(|p| side(p) >= px)
        .min_by_key(|p| side(p))
        .or_else(|| sound.iter().copied().max_by_key(|p| side(p)))
}

/// `pixmap`'s pixels in RGBA.
fn rgba((w, h, bytes): &Pixmap) -> Option<::image::RgbaImage> {
    let (w, h) = (w.unsigned_abs(), h.unsigned_abs());
    let n = (w * h) as usize;
    let mut out = Vec::with_capacity(n * 4);
    for argb in bytes.chunks_exact(4).take(n) {
        out.extend_from_slice(&[argb[1], argb[2], argb[3], argb[0]]);
    }
    ::image::RgbaImage::from_raw(w, h, out)
}

/// `image` in a `px` × `px` square, scaled to fit it, centred.
fn fitted(image: ::image::RgbaImage, px: u32) -> ::image::RgbaImage {
    let (w, h) = image.dimensions();
    if w == px && h == px {
        return image;
    }
    let k = px as f32 / w.max(h).max(1) as f32;
    let (nw, nh) = (
        ((w as f32 * k).round() as u32).clamp(1, px),
        ((h as f32 * k).round() as u32).clamp(1, px),
    );
    let scaled = ::image::imageops::resize(
        &image,
        nw,
        nh,
        ::image::imageops::FilterType::CatmullRom,
    );
    let mut square = ::image::RgbaImage::new(px, px);
    ::image::imageops::overlay(
        &mut square,
        &scaled,
        i64::from((px - nw) / 2),
        i64::from((px - nh) / 2),
    );
    square
}

fn handle(image: ::image::RgbaImage) -> image::Handle {
    let (w, h) = image.dimensions();
    image::Handle::from_rgba(w, h, image.into_raw())
}

/// The icon of `pixmaps` at `px` pixels.
pub fn of_pixmaps(pixmaps: &[Pixmap], px: u32) -> Option<image::Handle> {
    let image = rgba(best(pixmaps, px)?)?;
    Some(handle(fitted(image, px.clamp(8, 512))))
}

/// A PNG's icon at `px` pixels (a dbusmenu row's `icon-data`).
pub fn of_png(bytes: &[u8], px: u32) -> Option<image::Handle> {
    if bytes.is_empty() || bytes.len() > 4 << 20 {
        return None;
    }
    let image = ::image::load_from_memory_with_format(bytes, ::image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    Some(handle(fitted(image, px.clamp(8, 512))))
}

/// The icon themes, as the launcher reads them, once.
static THEMES: Mutex<Option<Themes>> = Mutex::new(None);

/// Icon `name` at `size` logical pixels on `scale`: in `theme_path` first
/// (its files, then its `hicolor`), then in the user's themes.
pub fn of_name(name: &str, theme_path: &str, size: u32, scale: f32) -> Option<image::Handle> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let px = icons::pixels(size, scale);
    let file = in_theme_path(Path::new(theme_path.trim()), name, px).or_else(|| {
        let mut themes = THEMES.lock().unwrap_or_else(PoisonError::into_inner);
        let themes = themes.get_or_insert_with(|| Themes::load(&entries::data_dirs()));
        themes.find(name, size, scale.ceil().max(1.0) as u32)
    })?;
    icons::load(&file, px)
}

/// Icon `name`'s file in an item's own `IconThemePath`, if it has one: a
/// file named so in it, else in its `hicolor`'s directories, the smallest
/// at least `px` large first (`scalable` counting as the largest).
fn in_theme_path(dir: &Path, name: &str, px: u32) -> Option<PathBuf> {
    if !dir.is_absolute() || name.contains('/') {
        return None;
    }
    let files = [format!("{name}.png"), format!("{name}.svg"), name.to_owned()];
    if let Some(file) = files.iter().map(|f| dir.join(f)).find(|f| f.is_file()) {
        return Some(file);
    }
    let theme = dir.join("hicolor");
    let mut found: Vec<(u32, PathBuf)> = std::fs::read_dir(&theme)
        .ok()?
        .flatten()
        .filter_map(|size| {
            let label = size.file_name().to_string_lossy().into_owned();
            let side = if label == "scalable" {
                u32::MAX
            } else {
                label.split('x').next()?.parse().ok()?
            };
            let apps = size.path().join("apps");
            files[..2]
                .iter()
                .map(|f| apps.join(f))
                .find(|f| f.is_file())
                .map(|f| (side, f))
        })
        .collect();
    found.sort_by_key(|(side, _)| (*side < px, if *side >= px { *side } else { u32::MAX - side }));
    found.into_iter().next().map(|(_, f)| f)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixmap(side: i32, argb: [u8; 4]) -> Pixmap {
        (side, side, argb.repeat((side * side) as usize))
    }

    #[test]
    fn the_smallest_pixmap_at_least_as_large_is_taken_else_the_largest() {
        let pixmaps = vec![
            pixmap(16, [255, 0, 0, 0]),
            pixmap(48, [255, 0, 0, 0]),
            pixmap(32, [255, 0, 0, 0]),
            // Says more than it holds: left out.
            (64, 64, vec![0; 16]),
        ];
        assert_eq!(best(&pixmaps, 22).map(|p| p.0), Some(32));
        assert_eq!(best(&pixmaps, 44).map(|p| p.0), Some(48));
        assert_eq!(best(&pixmaps, 66).map(|p| p.0), Some(48));
        assert_eq!(best(&[], 22), None);
    }

    #[test]
    fn a_pixmap_is_argb_in_network_order() {
        // Opaque red, then half-transparent blue.
        let mut bytes = vec![255, 255, 0, 0];
        bytes.extend([128, 0, 0, 255]);
        let image = rgba(&(2, 1, bytes)).expect("an image");
        assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(image.get_pixel(1, 0).0, [0, 0, 255, 128]);
    }

    #[test]
    fn an_icon_is_scaled_into_its_square() {
        let image = rgba(&(10, 5, [255, 0, 255, 0].repeat(50))).expect("an image");
        let square = fitted(image, 22);
        assert_eq!(square.dimensions(), (22, 22));
        // Centred, as wide as the square, half as tall.
        assert_eq!(square.get_pixel(11, 11).0, [0, 255, 0, 255]);
        assert_eq!(square.get_pixel(11, 1).0[3], 0);
    }

    #[test]
    fn an_items_own_theme_path_is_looked_in_first() {
        let dir = tempfile::tempdir().expect("a directory");
        std::fs::write(dir.path().join("slack-dot.png"), b"x").expect("a file");
        let apps = dir.path().join("hicolor/48x48/apps");
        std::fs::create_dir_all(&apps).expect("a theme");
        std::fs::write(apps.join("other.png"), b"x").expect("a file");
        let small = dir.path().join("hicolor/16x16/apps");
        std::fs::create_dir_all(&small).expect("a theme");
        std::fs::write(small.join("other.png"), b"x").expect("a file");
        assert_eq!(
            in_theme_path(dir.path(), "slack-dot", 22),
            Some(dir.path().join("slack-dot.png"))
        );
        assert_eq!(
            in_theme_path(dir.path(), "other", 22),
            Some(apps.join("other.png"))
        );
        assert_eq!(in_theme_path(dir.path(), "none", 22), None);
        assert_eq!(in_theme_path(Path::new("relative"), "other", 22), None);
    }
}
