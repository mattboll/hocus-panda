//! The editor's marks (5.9): arrows, boxes, texts and blurs, in the
//! image's own pixels, and the image drawn with them at its real
//! resolution, off the shell's thread, for saving. The editor draws them
//! the same way on its screen (`editor::draw`), from the same geometry.
//!
//! The pixels are drawn here, without a 2D library: each mark is a shape
//! whose distance to a pixel says how much of the pixel it covers (its
//! edge smoothed over a pixel), the text Manrope bold rasterised by
//! fontdue with a dark outline, the blur three box blurs of the region
//! (about a Gaussian), strong enough that text under it cannot be read:
//! the saved image is blurred, not covered.

use std::sync::OnceLock;

/// Straight RGBA, as PNG has it.
pub type Image = image::RgbaImage;

/// Manrope bold, the shell's own (SIL Open Font License 1.1, its notice
/// in `crates/hocus-shell/fonts`).
static MANROPE_BOLD: &[u8] = include_bytes!("../../fonts/Manrope-Bold.ttf");

/// A colour of the editor's.
pub type Rgb = [u8; 3];

/// The blur's faint tint over what it blurs, the mockup's
/// `rgba(120, 130, 150, 0.12)`: the region shows where it is.
pub const BLUR_TINT: (Rgb, f32) = ([120, 130, 150], 0.12);
/// The text's outline: `rgba(0, 0, 0, 0.9)`, around the glyphs.
pub const OUTLINE: (Rgb, f32) = ([0, 0, 0], 0.9);
/// The arrow's dark halo, the mockup's `0 0 2px rgba(0, 0, 0, 0.6)`.
pub const HALO: (Rgb, f32) = ([0, 0, 0], 0.35);

/// A rectangle, image pixels: its left, top, width and height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    /// The rectangle with these two corners, in any order.
    pub fn between(a: (f32, f32), b: (f32, f32)) -> Self {
        Self {
            x: a.0.min(b.0),
            y: a.1.min(b.1),
            w: (a.0 - b.0).abs(),
            h: (a.1 - b.1).abs(),
        }
    }
}

/// A mark, image pixels.
#[derive(Debug, Clone, PartialEq)]
pub enum Mark {
    /// A line from its tail to its head, `width` wide, its head a
    /// triangle `head` long and as wide.
    Arrow {
        from: (f32, f32),
        to: (f32, f32),
        width: f32,
        head: f32,
        colour: Rgb,
    },
    /// A frame inside `rect`, `width` wide, its outer corners `radius`.
    Box {
        rect: Rect,
        width: f32,
        radius: f32,
        colour: Rgb,
    },
    /// `text`, `size` high, its line's left and middle at `at`, outlined
    /// `outline` wide.
    Text {
        at: (f32, f32),
        text: String,
        size: f32,
        outline: f32,
        colour: Rgb,
    },
    /// What is under `rect` blurred, `radius` far.
    Blur { rect: Rect, radius: f32 },
}

impl Mark {
    /// The mark `k` times as large, from the image's origin: the same mark
    /// on the image shown smaller.
    pub fn scaled(&self, k: f32) -> Self {
        let p = |(x, y): (f32, f32)| (x * k, y * k);
        let r = |r: Rect| Rect {
            x: r.x * k,
            y: r.y * k,
            w: r.w * k,
            h: r.h * k,
        };
        match self.clone() {
            Self::Arrow {
                from,
                to,
                width,
                head,
                colour,
            } => Self::Arrow {
                from: p(from),
                to: p(to),
                width: width * k,
                head: head * k,
                colour,
            },
            Self::Box {
                rect,
                width,
                radius,
                colour,
            } => Self::Box {
                rect: r(rect),
                width: width * k,
                radius: radius * k,
                colour,
            },
            Self::Text {
                at,
                text,
                size,
                outline,
                colour,
            } => Self::Text {
                at: p(at),
                text,
                size: size * k,
                outline: outline * k,
                colour,
            },
            Self::Blur { rect, radius } => Self::Blur {
                rect: r(rect),
                radius: radius * k,
            },
        }
    }
}

/// A point, image pixels.
pub type At = (f32, f32);

/// An arrow's pieces: where its line ends (under its head), and its
/// head's three corners (its tip first). None when it has no length.
pub fn arrow(from: At, to: At, head: f32) -> Option<(At, [At; 3])> {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let len = dx.hypot(dy);
    if len < 0.5 {
        return None;
    }
    let (ux, uy) = (dx / len, dy / len);
    let head = head.min(len);
    let back = (to.0 - ux * head, to.1 - uy * head);
    let half = head / 2.0;
    let corners = [
        to,
        (back.0 - uy * half, back.1 + ux * half),
        (back.0 + uy * half, back.1 - ux * half),
    ];
    // The line goes three quarters of the way into the head, under it.
    let shaft = head * 0.75;
    let end = (to.0 - ux * shaft, to.1 - uy * shaft);
    Some((end, corners))
}

/// `image` with `marks` drawn on it, in order: a blur blurs what is under
/// it, the marks before it too.
pub fn render(image: &Image, marks: &[Mark]) -> Image {
    let mut out = image.clone();
    for mark in marks {
        draw(&mut out, mark);
    }
    out
}

fn draw(image: &mut Image, mark: &Mark) {
    match mark {
        Mark::Arrow {
            from,
            to,
            width,
            head,
            colour,
        } => {
            let Some((end, corners)) = arrow(*from, *to, *head) else {
                return;
            };
            let halo = (width * 0.5).max(1.0);
            let line = |p: (f32, f32), w: f32| width_cover(segment(p, *from, end), w);
            let tip =
                |p: (f32, f32), grow: f32| (0.5 - (triangle(p, &corners) - grow)).clamp(0.0, 1.0);
            let reach = width + halo + head;
            let bounds = bounds_of(&[*from, *to], reach);
            paint(image, bounds, HALO.0, HALO.1, |p| {
                line(p, width + 2.0 * halo).max(tip(p, halo))
            });
            paint(image, bounds, *colour, 1.0, |p| {
                line(p, *width).max(tip(p, 0.0))
            });
        }
        Mark::Box {
            rect,
            width,
            radius,
            colour,
        } => {
            let bounds = bounds_of(&[(rect.x, rect.y), (rect.x + rect.w, rect.y + rect.h)], 1.0);
            paint(image, bounds, *colour, 1.0, |p| {
                let outer = rounded(p, *rect, *radius);
                // Between the outer edge and `width` inside it.
                let ring = (outer + width / 2.0).abs() - width / 2.0;
                (0.5 - ring).clamp(0.0, 1.0)
            });
        }
        Mark::Text {
            at,
            text,
            size,
            outline,
            colour,
        } => {
            let Some(glyphs) = coverage(text, *size) else {
                return;
            };
            let left = at.0.round() as i64;
            let top = (at.1 - glyphs.height as f32 / 2.0).round() as i64;
            let reach = outline.ceil().max(1.0) as i64;
            let dilated = glyphs.dilated(*outline);
            for gy in -reach..glyphs.height as i64 + reach {
                for gx in -reach..glyphs.width as i64 + reach {
                    let (x, y) = (left + gx, top + gy);
                    let Some(pixel) = pixel_at(image, x, y) else {
                        continue;
                    };
                    let dark = dilated.at(gx + reach, gy + reach);
                    blend(pixel, OUTLINE.0, OUTLINE.1 * dark);
                    blend(pixel, *colour, glyphs.at(gx, gy));
                }
            }
        }
        Mark::Blur { rect, radius } => blur(image, *rect, *radius),
    }
}

/// How much of a pixel at distance `d` from a line's middle a line `w`
/// wide covers.
fn width_cover(d: f32, w: f32) -> f32 {
    (w / 2.0 - d + 0.5).clamp(0.0, 1.0)
}

/// The distance from `p` to the segment from `a` to `b`.
fn segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (ax, ay) = (p.0 - a.0, p.1 - a.1);
    let (bx, by) = (b.0 - a.0, b.1 - a.1);
    let len = bx * bx + by * by;
    let t = if len > 0.0 {
        ((ax * bx + ay * by) / len).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (ax - bx * t).hypot(ay - by * t)
}

/// The signed distance from `p` to a triangle (negative inside), as the
/// largest of its edges' signed distances: exact enough over a pixel.
fn triangle(p: (f32, f32), corners: &[(f32, f32); 3]) -> f32 {
    // Its corners' order, for which side of each edge is inside.
    let (a, b, c) = (corners[0], corners[1], corners[2]);
    let turn = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
    let sign = if turn < 0.0 { -1.0 } else { 1.0 };
    let mut most = f32::NEG_INFINITY;
    for i in 0..3 {
        let (u, v) = (corners[i], corners[(i + 1) % 3]);
        let (ex, ey) = (v.0 - u.0, v.1 - u.1);
        let len = ex.hypot(ey).max(1e-6);
        // Outward normal's side: negative inside.
        let d = -sign * (ex * (p.1 - u.1) - ey * (p.0 - u.0)) / len;
        most = most.max(d);
    }
    most
}

/// The signed distance from `p` to the rounded rectangle `rect` (negative
/// inside).
fn rounded(p: (f32, f32), rect: Rect, radius: f32) -> f32 {
    let half = (rect.w / 2.0, rect.h / 2.0);
    let r = radius.min(half.0).min(half.1).max(0.0);
    let centre = (rect.x + half.0, rect.y + half.1);
    let q = (
        (p.0 - centre.0).abs() - half.0 + r,
        (p.1 - centre.1).abs() - half.1 + r,
    );
    let outside = q.0.max(0.0).hypot(q.1.max(0.0));
    outside + q.0.max(q.1).min(0.0) - r
}

/// The pixels around `points`, `reach` past them.
fn bounds_of(points: &[(f32, f32)], reach: f32) -> (i64, i64, i64, i64) {
    let left = points.iter().map(|p| p.0).fold(f32::INFINITY, f32::min) - reach;
    let top = points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min) - reach;
    let right = points.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max) + reach;
    let bottom = points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max) + reach;
    (
        left.floor() as i64,
        top.floor() as i64,
        right.ceil() as i64,
        bottom.ceil() as i64,
    )
}

/// `colour` at `alpha` over the pixels of `bounds`, each as much as
/// `cover` says of its centre.
fn paint(
    image: &mut Image,
    (left, top, right, bottom): (i64, i64, i64, i64),
    colour: Rgb,
    alpha: f32,
    cover: impl Fn((f32, f32)) -> f32,
) {
    let (w, h) = (i64::from(image.width()), i64::from(image.height()));
    for y in top.max(0)..bottom.min(h) {
        for x in left.max(0)..right.min(w) {
            let c = cover((x as f32 + 0.5, y as f32 + 0.5));
            if c > 0.0
                && let Some(pixel) = pixel_at(image, x, y)
            {
                blend(pixel, colour, alpha * c);
            }
        }
    }
}

fn pixel_at(image: &mut Image, x: i64, y: i64) -> Option<&mut image::Rgba<u8>> {
    let (x, y) = (u32::try_from(x).ok()?, u32::try_from(y).ok()?);
    (x < image.width() && y < image.height()).then(|| image.get_pixel_mut(x, y))
}

/// `colour` at `alpha` over `pixel` (straight alpha both).
fn blend(pixel: &mut image::Rgba<u8>, colour: Rgb, alpha: f32) {
    let a = alpha.clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    let [r, g, b, da] = pixel.0;
    let da = f32::from(da) / 255.0;
    let out = a + da * (1.0 - a);
    if out <= 0.0 {
        return;
    }
    let mix = |src: u8, dst: u8| {
        let v = (f32::from(src) * a + f32::from(dst) * da * (1.0 - a)) / out;
        v.round().clamp(0.0, 255.0) as u8
    };
    pixel.0 = [
        mix(colour[0], r),
        mix(colour[1], g),
        mix(colour[2], b),
        (out * 255.0).round() as u8,
    ];
}

/// A text's glyphs' coverage, from 0 to 1, its line's box: as wide as its
/// advances, as tall as the font's ascent and descent.
struct Coverage {
    width: usize,
    height: usize,
    cover: Vec<f32>,
}

impl Coverage {
    fn at(&self, x: i64, y: i64) -> f32 {
        match (usize::try_from(x), usize::try_from(y)) {
            (Ok(x), Ok(y)) if x < self.width && y < self.height => self.cover[y * self.width + x],
            _ => 0.0,
        }
    }

    /// The coverage grown `by` pixels all around (the outline), its box
    /// as much larger on each side.
    fn dilated(&self, by: f32) -> Self {
        let reach = by.ceil().max(1.0) as i64;
        let (w, h) = (
            self.width + 2 * reach as usize,
            self.height + 2 * reach as usize,
        );
        let mut cover = vec![0.0f32; w * h];
        let offsets: Vec<(i64, i64, f32)> = (-reach..=reach)
            .flat_map(|dy| (-reach..=reach).map(move |dx| (dx, dy)))
            .filter_map(|(dx, dy)| {
                let d = (dx as f32).hypot(dy as f32);
                // Smoothed at its edge, as the shapes are.
                let k = (by + 0.5 - d).clamp(0.0, 1.0);
                (k > 0.0).then_some((dx, dy, k))
            })
            .collect();
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                let most = offsets
                    .iter()
                    .map(|&(dx, dy, k)| k * self.at(x - reach + dx, y - reach + dy))
                    .fold(0.0, f32::max);
                cover[y as usize * w + x as usize] = most;
            }
        }
        Self {
            width: w,
            height: h,
            cover,
        }
    }
}

fn font() -> Option<&'static fontdue::Font> {
    static FONT: OnceLock<Option<fontdue::Font>> = OnceLock::new();
    FONT.get_or_init(|| {
        fontdue::Font::from_bytes(MANROPE_BOLD, fontdue::FontSettings::default())
            .map_err(|err| tracing::warn!(err, "Manrope bold does not read"))
            .ok()
    })
    .as_ref()
}

/// `text` in Manrope bold at `size`: what its glyphs cover.
fn coverage(text: &str, size: f32) -> Option<Coverage> {
    let font = font()?;
    let line = font.horizontal_line_metrics(size)?;
    let mut pen = 0.0f32;
    let mut placed = Vec::new();
    let mut last = None;
    for c in text.chars() {
        if let Some(before) = last {
            pen += font.horizontal_kern(before, c, size).unwrap_or(0.0);
        }
        let (metrics, bitmap) = font.rasterize(c, size);
        placed.push((pen, metrics, bitmap));
        pen += metrics.advance_width;
        last = Some(c);
    }
    let width = pen.ceil().max(1.0) as usize;
    let height = (line.ascent - line.descent).ceil().max(1.0) as usize;
    let mut cover = vec![0.0f32; width * height];
    for (x0, metrics, bitmap) in placed {
        let gx = (x0 + metrics.xmin as f32).round() as i64;
        let gy = (line.ascent - (metrics.height as f32 + metrics.ymin as f32)).round() as i64;
        for (row, run) in bitmap.chunks(metrics.width.max(1)).enumerate() {
            for (col, &a) in run.iter().enumerate() {
                let (x, y) = (gx + col as i64, gy + row as i64);
                if let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y))
                    && x < width
                    && y < height
                {
                    let at = y * width + x;
                    cover[at] = cover[at].max(f32::from(a) / 255.0);
                }
            }
        }
    }
    Some(Coverage {
        width,
        height,
        cover,
    })
}

/// The pixels under `rect` blurred `radius` far (three box blurs, about a
/// Gaussian of that deviation), read from around it too, then tinted.
pub fn blur(image: &mut Image, rect: Rect, radius: f32) {
    let (iw, ih) = (image.width() as i64, image.height() as i64);
    let left = (rect.x.floor() as i64).clamp(0, iw);
    let top = (rect.y.floor() as i64).clamp(0, ih);
    let right = ((rect.x + rect.w).ceil() as i64).clamp(0, iw);
    let bottom = ((rect.y + rect.h).ceil() as i64).clamp(0, ih);
    if right <= left || bottom <= top {
        return;
    }
    let r = radius.round().max(1.0) as i64;
    // Around it as far as three passes read.
    let margin = 3 * r;
    let (x0, y0) = ((left - margin).max(0), (top - margin).max(0));
    let (x1, y1) = ((right + margin).min(iw), (bottom + margin).min(ih));
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    // Premultiplied, so that transparent pixels add no colour.
    let mut plane = vec![[0.0f32; 4]; w * h];
    for y in 0..h {
        for x in 0..w {
            let p = image
                .get_pixel((x0 + x as i64) as u32, (y0 + y as i64) as u32)
                .0;
            let a = f32::from(p[3]) / 255.0;
            plane[y * w + x] = [
                f32::from(p[0]) * a,
                f32::from(p[1]) * a,
                f32::from(p[2]) * a,
                a,
            ];
        }
    }
    for _ in 0..3 {
        box_pass(&mut plane, w, h, r as usize, true);
        box_pass(&mut plane, w, h, r as usize, false);
    }
    for y in top..bottom {
        for x in left..right {
            let [r, g, b, a] = plane[(y - y0) as usize * w + (x - x0) as usize];
            let straight = |v: f32| {
                if a > 0.0 {
                    (v / a).round().clamp(0.0, 255.0) as u8
                } else {
                    0
                }
            };
            let pixel = image.get_pixel_mut(x as u32, y as u32);
            pixel.0 = [
                straight(r),
                straight(g),
                straight(b),
                (a * 255.0).round().clamp(0.0, 255.0) as u8,
            ];
            blend(pixel, BLUR_TINT.0, BLUR_TINT.1);
        }
    }
}

/// One box blur of `plane` (`w` × `h`) along its rows or its columns,
/// `r` either side, its edges held.
fn box_pass(plane: &mut [[f32; 4]], w: usize, h: usize, r: usize, rows: bool) {
    let (lines, len) = if rows { (h, w) } else { (w, h) };
    let at = |line: usize, i: usize| if rows { line * w + i } else { i * w + line };
    let span = (2 * r + 1) as f32;
    let mut run = vec![[0.0f32; 4]; len];
    for line in 0..lines {
        let get = |plane: &[[f32; 4]], i: i64| plane[at(line, i.clamp(0, len as i64 - 1) as usize)];
        let mut sum = [0.0f32; 4];
        for i in -(r as i64)..=(r as i64) {
            let p = get(plane, i);
            for k in 0..4 {
                sum[k] += p[k];
            }
        }
        for (i, out) in run.iter_mut().enumerate() {
            *out = sum.map(|s| s / span);
            let (add, take) = (
                get(plane, i as i64 + r as i64 + 1),
                get(plane, i as i64 - r as i64),
            );
            for k in 0..4 {
                sum[k] += add[k] - take[k];
            }
        }
        for (i, value) in run.iter().enumerate() {
            plane[at(line, i)] = *value;
        }
    }
}

/// The variance of the pixels' brightness under `rect`: what a blur takes
/// away (for tests).
#[cfg(test)]
pub fn variance(image: &Image, rect: Rect) -> f64 {
    let mut values = Vec::new();
    for y in rect.y as u32..(rect.y + rect.h) as u32 {
        for x in rect.x as u32..(rect.x + rect.w) as u32 {
            let p = image.get_pixel(x, y).0;
            values.push((f64::from(p[0]) + f64::from(p[1]) + f64::from(p[2])) / 3.0);
        }
    }
    let n = values.len().max(1) as f64;
    let mean = values.iter().sum::<f64>() / n;
    values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgb = [0xF2, 0x54, 0x5B];

    /// Black and white squares of 3 pixels: as fine as small text.
    fn checkered(w: u32, h: u32) -> Image {
        Image::from_fn(w, h, |x, y| {
            if (x / 3 + y / 3) % 2 == 0 {
                image::Rgba([255, 255, 255, 255])
            } else {
                image::Rgba([0, 0, 0, 255])
            }
        })
    }

    fn grey(w: u32, h: u32) -> Image {
        Image::from_pixel(w, h, image::Rgba([40, 40, 40, 255]))
    }

    #[test]
    fn an_arrow_runs_from_its_tail_to_its_head() {
        let out = render(
            &grey(200, 100),
            &[Mark::Arrow {
                from: (20.0, 50.0),
                to: (180.0, 50.0),
                width: 6.0,
                head: 24.0,
                colour: RED,
            }],
        );
        // Along its line and in its head, red; above them, not.
        assert_eq!(out.get_pixel(60, 50).0, [RED[0], RED[1], RED[2], 255]);
        assert_eq!(out.get_pixel(170, 50).0, [RED[0], RED[1], RED[2], 255]);
        assert_eq!(out.get_pixel(165, 54).0[0], RED[0]);
        assert_eq!(out.get_pixel(60, 30).0, [40, 40, 40, 255]);
        // Past its tip, nothing.
        assert_eq!(out.get_pixel(190, 50).0, [40, 40, 40, 255]);
    }

    #[test]
    fn a_box_is_a_frame_inside_its_rectangle() {
        let rect = Rect {
            x: 20.0,
            y: 20.0,
            w: 100.0,
            h: 60.0,
        };
        let out = render(
            &grey(200, 100),
            &[Mark::Box {
                rect,
                width: 4.0,
                radius: 6.0,
                colour: RED,
            }],
        );
        assert_eq!(out.get_pixel(70, 21).0, [RED[0], RED[1], RED[2], 255]);
        assert_eq!(out.get_pixel(21, 50).0, [RED[0], RED[1], RED[2], 255]);
        // Inside and outside it, as it was.
        assert_eq!(out.get_pixel(70, 50).0, [40, 40, 40, 255]);
        assert_eq!(out.get_pixel(70, 15).0, [40, 40, 40, 255]);
        assert_eq!(out.get_pixel(150, 50).0, [40, 40, 40, 255]);
    }

    #[test]
    fn a_text_is_drawn_in_its_colour_outlined() {
        let out = render(
            &grey(200, 60),
            &[Mark::Text {
                at: (10.0, 30.0),
                text: "Ici".to_owned(),
                size: 27.0,
                outline: 2.0,
                colour: [0xFF, 0xFF, 0xFF],
            }],
        );
        let white = out.pixels().filter(|p| p.0[0] > 250).count();
        let dark = out.pixels().filter(|p| p.0[0] < 20).count();
        assert!(white > 30, "{white}");
        assert!(dark > 30, "{dark}");
        // Nothing past its few letters.
        assert_eq!(out.get_pixel(150, 30).0, [40, 40, 40, 255]);
    }

    #[test]
    fn a_blur_makes_what_is_under_it_unreadable() {
        let image = checkered(240, 120);
        let rect = Rect {
            x: 40.0,
            y: 20.0,
            w: 120.0,
            h: 60.0,
        };
        let out = render(&image, &[Mark::Blur { rect, radius: 10.0 }]);
        let before = variance(&image, rect);
        let after = variance(&out, rect);
        assert!(after < before * 0.01, "{before} → {after}");
        // Outside it, as it was.
        assert_eq!(out.get_pixel(10, 10), image.get_pixel(10, 10));
        assert_eq!(out.get_pixel(200, 100), image.get_pixel(200, 100));
    }

    #[test]
    fn a_blur_blurs_the_marks_before_it_but_not_after() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        let line = Mark::Arrow {
            from: (10.0, 50.0),
            to: (90.0, 50.0),
            width: 4.0,
            head: 16.0,
            colour: RED,
        };
        let blur = Mark::Blur { rect, radius: 8.0 };
        let under = render(&grey(100, 100), &[line.clone(), blur.clone()]);
        let over = render(&grey(100, 100), &[blur, line]);
        assert_ne!(under.get_pixel(50, 50).0[0], RED[0]);
        assert_eq!(over.get_pixel(50, 50).0[0], RED[0]);
    }

    #[test]
    fn an_arrow_has_its_head_at_its_end() {
        let (end, corners) = arrow((0.0, 0.0), (100.0, 0.0), 16.0).expect("an arrow");
        assert_eq!(end, (88.0, 0.0));
        assert_eq!(corners[0], (100.0, 0.0));
        assert_eq!(corners[1], (84.0, 8.0));
        assert_eq!(corners[2], (84.0, -8.0));
        assert!(arrow((5.0, 5.0), (5.0, 5.0), 16.0).is_none());
    }
}
