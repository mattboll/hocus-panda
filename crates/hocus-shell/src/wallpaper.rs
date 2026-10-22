//! Wallpapers as pixels: the theme's soft glow placed by the time of day
//! (with the HUD's grid, or the Matrix theme's rain), an image filling,
//! fitting, stretched or centred, a folder of images through the day, or a
//! plain colour. Everything here is plain computation: the compositor runs
//! it on a worker thread, never the loop.

use std::path::{Path, PathBuf};

use hocus_ui::Color;
use image::{DynamicImage, RgbaImage, imageops::FilterType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The theme's light, placed by the time of day, and its pattern.
    Glow(Pattern),
    Image(PathBuf),
    /// Its images in name order, through the day.
    Folder(PathBuf),
    Colour,
}

/// What a theme draws over its glow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    None,
    /// The HUD theme's grid.
    Grid,
    /// The Matrix theme's rain, still, on night blue (the compositor makes
    /// it fall where it can).
    Rain,
    /// Stars on night blue, still (the compositor makes them drift).
    Stars,
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
    /// Physical pixels per logical one: patterns keep their size.
    pub scale: f32,
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
        Source::Glow(Pattern::Rain) => rain(w, h, req.colour, req.hour, req.scale),
        Source::Glow(Pattern::Stars) => stars(w, h, req.colour, req.hour, req.scale),
        Source::Glow(pattern) => glow(w, h, req.colour, req.hour, *pattern == Pattern::Grid),
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

/// Where the sun is at `hour`, as fractions of the screen, and its light
/// (low and warm in the morning and the evening, high at noon, night blue
/// at night).
pub fn sun(hour: f32) -> (f32, f32, Color) {
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
    (cx, cy, light)
}

/// The identity's wallpaper: the base colour, and a soft light where the
/// sun is.
fn glow(w: u32, h: u32, base: Color, hour: f32, grid: bool) -> RgbaImage {
    let (cx, cy, light) = sun(hour);
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

/// The night the Matrix rain and the stars fall on (as in the boards):
/// from the base colour at the top to night blue at the bottom, with a
/// night-blue light where the sun is, whatever the hour.
fn night(w: u32, h: u32, base: Color, hour: f32) -> RgbaImage {
    let (cx, cy, _) = sun(hour);
    let (fw, fh) = (w as f32, h as f32);
    RgbaImage::from_fn(w, h, |x, y| {
        let (u, v) = (x as f32 / fw, y as f32 / fh.max(1.0));
        let c = base
            .mix(crate::matrix::NIGHT, v)
            .mix(NIGHT_LIGHT, night_light(u, v, cx, cy));
        image::Rgba([c.r, c.g, c.b, 255])
    })
}

/// The night's light.
pub const NIGHT_LIGHT: Color = Color::rgb(0x1e, 0x3c, 0x8c);

/// How much of the night's light reaches `(u, v)` (fractions of the
/// screen) from the sun at `(cx, cy)`: an ellipse about half the screen
/// wide, gone at 70 % of it. The compositor's shaders do the same.
pub fn night_light(u: f32, v: f32, cx: f32, cy: f32) -> f32 {
    let (dx, dy) = ((u - cx) / 0.52, (v - cy) / 0.83);
    0.18 * (1.0 - (dx * dx + dy * dy).sqrt() / 0.7).clamp(0.0, 1.0)
}

/// The Matrix theme's wallpaper: the rain, still, on the night.
fn rain(w: u32, h: u32, base: Color, hour: f32, scale: f32) -> RgbaImage {
    let mut image = night(w, h, base, hour);
    // The same rain for a screen of the same size.
    crate::matrix::draw_rain(
        &mut image,
        scale.max(0.5),
        (u64::from(w) << 32) | u64::from(h),
    );
    image
}

/// Layers of stars, far to near: the side of the square each star has to
/// itself, how fast the layer drifts left (logical pixels a second), and
/// how bright and large its stars are, 0 to 1. The compositor's shader
/// draws the same.
pub const STAR_LAYERS: [(f32, f32, f32); 3] =
    [(90.0, 3.0, 0.45), (150.0, 7.0, 0.7), (240.0, 14.0, 1.0)];
/// One square in so many has a star.
const STAR_CHANCE: f32 = 0.55;
pub const STAR: Color = Color::rgb(0xdd, 0xe6, 0xff);

/// Stars on the night, still: where they are when they start drifting.
fn stars(w: u32, h: u32, base: Color, hour: f32, scale: f32) -> RgbaImage {
    let mut image = night(w, h, base, hour);
    let scale = scale.max(0.5);
    let (lw, lh) = (w as f32 / scale, h as f32 / scale);
    for (layer, &(side, _, strength)) in STAR_LAYERS.iter().enumerate() {
        let (cols, rows) = ((lw / side).ceil() as u64, (lh / side).ceil() as u64);
        for row in 0..rows {
            for col in 0..cols {
                let seed = crate::matrix::hash(&[layer as u64, col, row]);
                let mut rng = crate::matrix::Rng::new(seed);
                if rng.unit() > STAR_CHANCE {
                    continue;
                }
                let radius = 0.5 + 1.1 * strength * rng.unit();
                let margin = radius + 1.0;
                let x = col as f32 * side + rng.range(margin, side - margin);
                let y = row as f32 * side + rng.range(margin, side - margin);
                let bright = strength * rng.range(0.5, 1.0);
                dot(&mut image, x * scale, y * scale, radius * scale, bright);
            }
        }
    }
    image
}

/// A soft round star.
fn dot(image: &mut RgbaImage, x: f32, y: f32, radius: f32, alpha: f32) {
    let reach = radius + 1.0;
    let (x0, x1) = (
        (x - reach).floor().max(0.0) as u32,
        (x + reach).ceil() as u32,
    );
    let (y0, y1) = (
        (y - reach).floor().max(0.0) as u32,
        (y + reach).ceil() as u32,
    );
    for py in y0..y1.min(image.height()) {
        for px in x0..x1.min(image.width()) {
            let d = (px as f32 + 0.5 - x).hypot(py as f32 + 0.5 - y);
            let a = (radius + 0.5 - d).clamp(0.0, 1.0) * alpha;
            if a > 0.0 {
                let pixel = image.get_pixel_mut(px, py);
                let c = Color::rgb(pixel.0[0], pixel.0[1], pixel.0[2]).mix(STAR, a);
                pixel.0 = [c.r, c.g, c.b, 255];
            }
        }
    }
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

    #[test]
    fn the_matrix_rain_falls_on_night_blue() {
        let base = Color::rgb(0x02, 0x04, 0x0a);
        let request = |scale| Request {
            source: Source::Glow(Pattern::Rain),
            mode: Mode::Fill,
            colour: base,
            width: 640,
            height: 400,
            scale,
            hour: 23.0,
        };
        let picture = render(&request(1.0)).map_err(|e| e.to_string());
        let Ok(picture) = picture else {
            panic!("{picture:?}");
        };
        let at = |x: u32, y: u32| {
            let i = ((y * picture.width + x) * 4) as usize;
            Color::rgb(
                picture.pixels[i],
                picture.pixels[i + 1],
                picture.pixels[i + 2],
            )
        };
        // Darker and blacker at the top than at the bottom, away from the
        // drops (the gap between two columns).
        let (top, bottom) = (at(40, 0), at(40, 399));
        assert!(bottom.b > top.b, "{top:?} {bottom:?}");
        assert!(top.r < 0x10 && top.g < 0x10, "never brownish: {top:?}");
        // Green glyphs somewhere, and the same picture again.
        let green = picture
            .pixels
            .chunks(4)
            .filter(|p| p[1] > 60 && p[1] > 2 * p[0])
            .count();
        assert!(green > 200, "{green}");
        let again = render(&request(1.0)).map(|p| p.pixels);
        assert!(again.is_ok_and(|p| p == picture.pixels));
    }

    #[test]
    fn stars_shine_on_the_night() {
        let request = Request {
            source: Source::Glow(Pattern::Stars),
            mode: Mode::Fill,
            colour: Color::rgb(0x02, 0x04, 0x0a),
            width: 640,
            height: 400,
            scale: 1.0,
            hour: 23.0,
        };
        let Ok(picture) = render(&request) else {
            panic!("no stars");
        };
        let bright = picture
            .pixels
            .chunks(4)
            .filter(|p| p[0] > 80 && p[2] > 90)
            .count();
        assert!(bright > 15, "{bright} bright pixels");
    }
}
