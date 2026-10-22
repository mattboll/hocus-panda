//! Wallpapers as pixels: the theme's soft glow placed by the time of day,
//! an image filling, fitting, stretched or centred, a folder of images
//! through the day, or a plain colour. Everything here is plain
//! computation: the compositor runs it on a worker thread, never the loop.

use std::path::{Path, PathBuf};

use hocus_ui::Color;
use image::{DynamicImage, RgbaImage, imageops::FilterType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The theme's light, placed by the time of day; `grid` for the HUD
    /// theme.
    Glow {
        grid: bool,
    },
    Image(PathBuf),
    /// Its images in name order, through the day.
    Folder(PathBuf),
    Colour,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Covers the screen, cropped.
    Fill,
    /// Whole, with the colour around.
    Fit,
    Stretch,
    /// At its size, in the middle.
    Center,
}

impl Mode {
    pub fn parse(s: &str) -> Self {
        match s {
            "fit" => Self::Fit,
            "stretch" => Self::Stretch,
            "center" => Self::Center,
            _ => Self::Fill,
        }
    }
}

/// What to draw, for a screen of `width`×`height` physical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub source: Source,
    pub mode: Mode,
    /// Around a fitted or centred image, and the base of the glow.
    pub colour: Color,
    pub width: u32,
    pub height: u32,
    /// The hour, 0 to 24, as a fraction.
    pub hour: f32,
}

/// Opaque pixels, row by row, RGBA.
#[derive(Debug, Clone)]
pub struct Picture {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// The colour that stands out, for an accent taken from it.
    pub dominant: Option<Color>,
}

pub fn render(req: &Request) -> Result<Picture, String> {
    let (w, h) = (req.width.max(1), req.height.max(1));
    let image = match &req.source {
        Source::Colour => solid(w, h, req.colour),
        Source::Glow { grid } => glow(w, h, req.colour, req.hour, *grid),
        Source::Image(path) => place(&open(path)?, w, h, req.mode, req.colour),
        Source::Folder(dir) => {
            let path = of_the_hour(dir, req.hour)?;
            place(&open(&path)?, w, h, req.mode, req.colour)
        }
    };
    let dominant = dominant(image.as_raw());
    Ok(Picture {
        pixels: image.into_raw(),
        width: w,
        height: h,
        dominant,
    })
}

fn open(path: &Path) -> Result<DynamicImage, String> {
    image::open(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The folder's image for `hour`: the day shared between them in name
/// order.
fn of_the_hour(dir: &Path, hour: f32) -> Result<PathBuf, String> {
    let mut images: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                matches!(e.to_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp")
            })
        })
        .collect();
    images.sort();
    if images.is_empty() {
        return Err(format!("{}: no image", dir.display()));
    }
    let i = ((hour.rem_euclid(24.0) / 24.0) * images.len() as f32) as usize;
    Ok(images[i.min(images.len() - 1)].clone())
}

fn solid(w: u32, h: u32, c: Color) -> RgbaImage {
    RgbaImage::from_pixel(w, h, image::Rgba([c.r, c.g, c.b, 255]))
}

/// `src` placed on a `w`×`h` screen.
fn place(src: &DynamicImage, w: u32, h: u32, mode: Mode, around: Color) -> RgbaImage {
    let (sw, sh) = (src.width().max(1), src.height().max(1));
    let mut out = solid(w, h, around);
    let scaled = match mode {
        Mode::Stretch => src.resize_exact(w, h, FilterType::Triangle),
        Mode::Fill => {
            let k = (w as f32 / sw as f32).max(h as f32 / sh as f32);
            let (nw, nh) = ((sw as f32 * k).ceil() as u32, (sh as f32 * k).ceil() as u32);
            let big = src.resize_exact(nw.max(w), nh.max(h), FilterType::Triangle);
            big.crop_imm((big.width() - w) / 2, (big.height() - h) / 2, w, h)
        }
        Mode::Fit => {
            let k = (w as f32 / sw as f32).min(h as f32 / sh as f32);
            let (nw, nh) = (
                (sw as f32 * k).round() as u32,
                (sh as f32 * k).round() as u32,
            );
            src.resize_exact(nw.max(1), nh.max(1), FilterType::Triangle)
        }
        Mode::Center => src.clone(),
    };
    let scaled = scaled.to_rgba8();
    let x = (i64::from(w) - i64::from(scaled.width())) / 2;
    let y = (i64::from(h) - i64::from(scaled.height())) / 2;
    image::imageops::overlay(&mut out, &scaled, x, y);
    // Opaque: what shows through a transparent image is the colour.
    for px in out.pixels_mut() {
        px.0[3] = 255;
    }
    out
}

/// The identity's wallpaper: the base colour, and a soft light where the
/// sun is (low and warm in the morning and the evening, high at noon,
/// night blue at night).
fn glow(w: u32, h: u32, base: Color, hour: f32, grid: bool) -> RgbaImage {
    let hour = hour.rem_euclid(24.0);
    let day = (6.0..20.0).contains(&hour);
    let (cx, cy, light) = if day {
        let t = (hour - 6.0) / 14.0;
        let height = (t * std::f32::consts::PI).sin();
        let light = match hour {
            h if h < 10.0 => Color::rgb(0xf2, 0xc4, 0x6b),
            h if h < 16.0 => Color::rgb(0xc9, 0xe8, 0xcb),
            _ => Color::rgb(0xf2, 0x8c, 0x4a),
        };
        (t, 0.95 - 0.8 * height, light)
    } else {
        (0.5, 0.05, Color::rgb(0x2f, 0x5f, 0xa8))
    };
    let (fw, fh) = (w as f32, h as f32);
    let radius = fw.max(fh) * 0.9;
    // Light themes get a gentler light.
    let strength = if base.luminance() > 0.5 { 0.10 } else { 0.18 };
    RgbaImage::from_fn(w, h, |x, y| {
        let dx = x as f32 - cx * fw;
        let dy = y as f32 - cy * fh;
        let d = (dx * dx + dy * dy).sqrt() / radius;
        let a = strength * (1.0 - d).clamp(0.0, 1.0).powi(2);
        let mut c = base.mix(light, a);
        if grid && (x % 48 == 0 || y % 48 == 0) {
            c = c.mix(Color::rgb(0x38, 0xd9, 0xf5), 0.08);
        }
        image::Rgba([c.r, c.g, c.b, 255])
    })
}

/// The colour that stands out: the most common saturated hue, averaged.
pub fn dominant(rgba: &[u8]) -> Option<Color> {
    const BINS: usize = 24;
    let mut sums = [[0f64; 4]; BINS];
    // Every 7th pixel is plenty, and keeps it quick on large screens.
    for px in rgba.as_chunks::<4>().0.iter().step_by(7) {
        let c = Color::rgb(px[0], px[1], px[2]);
        let (h, s, l) = c.hsl();
        if s < 0.3 || !(0.15..0.9).contains(&l) {
            continue;
        }
        let bin = ((h / 360.0) * BINS as f32) as usize % BINS;
        let weight = f64::from(s);
        sums[bin][0] += f64::from(px[0]) * weight;
        sums[bin][1] += f64::from(px[1]) * weight;
        sums[bin][2] += f64::from(px[2]) * weight;
        sums[bin][3] += weight;
    }
    let best = sums.iter().max_by(|a, b| a[3].total_cmp(&b[3]))?;
    (best[3] > 0.0).then(|| {
        Color::rgb(
            (best[0] / best[3]) as u8,
            (best[1] / best[3]) as u8,
            (best[2] / best[3]) as u8,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_fn(w, h, |x, y| image::Rgba(f(x, y))))
    }

    #[test]
    fn fill_covers_and_fit_shows_the_colour_around() {
        // A wide red picture with a blue stripe at each end.
        let src = picture(400, 100, |x, _| {
            if !(20..380).contains(&x) {
                [0, 0, 255, 255]
            } else {
                [255, 0, 0, 255]
            }
        });
        let fill = place(&src, 200, 200, Mode::Fill, Color::BLACK);
        // Cropped: the middle of a 800×200 picture, all red.
        assert_eq!(fill.get_pixel(0, 100).0, [255, 0, 0, 255]);
        let fit = place(&src, 200, 200, Mode::Fit, Color::rgb(9, 9, 9));
        assert_eq!(
            fit.get_pixel(100, 10).0,
            [9, 9, 9, 255],
            "above the picture"
        );
        assert_eq!(fit.get_pixel(100, 100).0, [255, 0, 0, 255]);
    }

    #[test]
    fn a_mostly_orange_picture_is_orange() {
        let src = picture(64, 64, |x, y| {
            if (x + y) % 5 == 0 {
                [20, 40, 160, 255]
            } else {
                [235, 130, 40, 255]
            }
        });
        let c = dominant(src.to_rgba8().as_raw()).map(|c| c.hsl().0);
        assert!(c.is_some_and(|h| (20.0..40.0).contains(&h)), "{c:?}");
    }

    #[test]
    fn the_glow_follows_the_sun() {
        let base = Color::rgb(0x0e, 0x0f, 0x12);
        let morning = glow(100, 50, base, 7.0, false);
        // Low on the left in the morning: brighter there than on the right.
        let left = morning.get_pixel(5, 45).0[0];
        let right = morning.get_pixel(95, 45).0[0];
        assert!(left > right, "{left} {right}");
    }
}
