//! The Matrix theme's rain (docs/design/matrix-theme.md): its glyphs, the
//! still rain of the wallpaper, a sheet of glyphs the compositor's
//! effects cut their rows and columns from, and the shapes they draw in
//! glyphs (the lock's padlock, the boot's large letters).
//!
//! The glyphs are drawn here, stroke by stroke, after half-width katakana
//! and digits, some of them mirrored as in the films: no bundled font has
//! katakana, and a few strokes on a 5×7 grid are all a falling glyph needs.
//! Plain computation, no smithay: the wallpaper's worker draws the rain,
//! the compositor uploads the sheet once per scale.

use hocus_ui::Color;
use image::RgbaImage;

/// The rain's green (the theme's accent, as in the boards).
pub const GREEN: Color = Color::rgb(0x00, 0xF0, 0x60);
/// The bright glyph at the head of a drop.
pub const HEAD: Color = Color::rgb(0xCC, 0xFF, 0xCC);
/// Phosphor glow: lines, borders and sparks.
pub const GLOW: Color = Color::rgb(0x00, 0xFF, 0x88);
/// Behind the glyphs, where nothing else is.
pub const BLACK: Color = Color::rgb(0x02, 0x04, 0x0A);
/// The bottom of the wallpaper's gradient: night blue.
pub const NIGHT: Color = Color::rgb(0x05, 0x0B, 0x1C);

/// A glyph cell of the window effects, logical pixels.
pub const CELL: (u32, u32) = (8, 14);

type Stroke = &'static [(f32, f32)];

/// Each glyph as lines through points on a grid 4 wide and 6 tall.
const GLYPHS: &[&[Stroke]] = &[
    // Katakana-like: a, i, u, e, o…
    &[
        &[(0.0, 0.5), (4.0, 0.5), (3.0, 2.5)],
        &[(2.2, 1.8), (2.0, 4.0), (0.8, 6.0)],
    ],
    &[&[(4.0, 0.0), (0.5, 3.5)], &[(2.5, 2.0), (2.5, 6.0)]],
    &[
        &[(2.0, 0.0), (2.0, 1.2)],
        &[(0.5, 1.5), (0.5, 3.0)],
        &[(0.5, 1.5), (4.0, 1.5), (3.5, 4.0), (1.5, 6.0)],
    ],
    &[
        &[(0.5, 1.0), (3.5, 1.0)],
        &[(2.0, 1.0), (2.0, 5.5)],
        &[(0.0, 5.5), (4.0, 5.5)],
    ],
    &[
        &[(0.0, 2.0), (4.0, 2.0)],
        &[(2.5, 0.0), (2.5, 6.0), (1.8, 5.6)],
        &[(2.3, 2.5), (0.0, 5.5)],
    ],
    &[
        &[(0.5, 1.5), (4.0, 1.5), (3.6, 6.0), (2.8, 5.6)],
        &[(2.0, 0.0), (1.6, 3.5), (0.3, 6.0)],
    ],
    &[
        &[(0.5, 1.5), (4.0, 1.0)],
        &[(0.0, 3.5), (4.0, 3.0)],
        &[(1.5, 0.0), (2.5, 6.0)],
    ],
    &[
        &[(1.5, 0.0), (0.0, 3.0)],
        &[(1.0, 1.2), (4.0, 1.2), (3.0, 4.0), (1.0, 6.0)],
    ],
    &[
        &[(1.2, 0.0), (0.0, 3.0)],
        &[(0.8, 1.8), (4.0, 1.8)],
        &[(2.8, 1.8), (2.6, 4.5), (1.5, 6.0)],
    ],
    &[&[(0.5, 1.0), (4.0, 1.0), (4.0, 5.5), (0.5, 5.5)]],
    &[
        &[(0.0, 2.0), (4.0, 2.0)],
        &[(1.2, 0.5), (1.2, 3.5)],
        &[(3.0, 0.5), (3.0, 3.5), (1.5, 6.0)],
    ],
    &[
        &[(0.5, 1.0), (1.5, 1.8)],
        &[(0.0, 3.0), (1.0, 3.8)],
        &[(0.5, 6.0), (4.0, 1.5)],
    ],
    &[
        &[(0.5, 1.0), (3.5, 1.0), (0.5, 6.0)],
        &[(2.0, 3.5), (4.0, 6.0)],
    ],
    &[
        &[(0.0, 2.5), (4.0, 1.5), (3.5, 3.5)],
        &[(1.2, 0.0), (1.2, 5.5), (4.0, 5.5)],
    ],
    &[
        &[(0.5, 1.0), (1.5, 3.0)],
        &[(4.0, 0.8), (3.5, 3.5), (1.0, 6.0)],
    ],
    &[
        &[(1.6, 0.0), (0.0, 3.0)],
        &[(1.0, 1.2), (4.0, 1.2), (3.0, 4.0), (1.0, 6.0)],
        &[(1.0, 3.0), (3.0, 4.0)],
    ],
    &[
        &[(3.5, 0.0), (0.5, 1.0)],
        &[(0.0, 2.5), (4.0, 2.5)],
        &[(2.2, 1.0), (2.2, 4.0), (1.0, 6.0)],
    ],
    &[
        &[(0.0, 1.0), (0.6, 2.2)],
        &[(1.6, 0.8), (2.0, 2.0)],
        &[(4.0, 0.8), (3.5, 3.5), (1.5, 6.0)],
    ],
    &[
        &[(0.8, 0.5), (3.2, 0.5)],
        &[(0.0, 2.2), (4.0, 2.2)],
        &[(2.0, 2.2), (2.0, 4.5), (1.0, 6.0)],
    ],
    &[&[(1.5, 0.0), (1.5, 6.0)], &[(1.5, 2.5), (3.8, 3.8)]],
    &[
        &[(0.0, 2.0), (4.0, 2.0)],
        &[(2.2, 0.0), (2.2, 3.5), (1.0, 6.0)],
    ],
    &[&[(0.8, 1.0), (3.2, 1.0)], &[(0.0, 5.0), (4.0, 5.0)]],
    &[
        &[(0.5, 1.0), (4.0, 1.0), (3.0, 4.0), (0.5, 6.0)],
        &[(1.5, 3.0), (4.0, 5.5)],
    ],
    &[
        &[(2.0, 0.0), (2.0, 1.0)],
        &[(0.5, 1.5), (3.5, 1.5), (0.0, 5.0)],
        &[(2.0, 3.0), (2.0, 6.0)],
        &[(2.8, 3.2), (4.0, 4.5)],
    ],
    &[&[(3.8, 0.5), (3.4, 3.0), (0.5, 6.0)]],
    &[&[(1.5, 1.0), (0.0, 5.5)], &[(2.5, 1.0), (4.0, 5.5)]],
    &[
        &[(0.8, 0.0), (0.8, 5.5), (4.0, 5.5)],
        &[(0.8, 2.5), (3.5, 1.5)],
    ],
    &[&[(0.3, 1.0), (4.0, 1.0), (3.2, 4.0), (1.0, 6.0)]],
    &[&[(0.0, 3.5), (1.5, 1.8), (4.0, 5.0)]],
    &[
        &[(0.0, 1.8), (4.0, 1.8)],
        &[(2.0, 0.0), (2.0, 6.0)],
        &[(1.0, 3.2), (0.0, 5.0)],
        &[(3.0, 3.2), (4.0, 5.0)],
    ],
    &[
        &[(0.0, 1.0), (4.0, 1.0), (2.5, 3.5)],
        &[(1.2, 2.8), (2.5, 5.5)],
    ],
    &[
        &[(0.8, 0.6), (3.5, 1.4)],
        &[(0.8, 2.6), (3.2, 3.4)],
        &[(0.5, 4.6), (3.8, 5.8)],
    ],
    &[
        &[(2.0, 0.0), (0.3, 5.2), (4.0, 4.6)],
        &[(3.0, 3.0), (4.0, 6.0)],
    ],
    &[&[(3.8, 0.0), (0.5, 6.0)], &[(0.8, 2.0), (3.8, 5.0)]],
    &[
        &[(0.5, 1.0), (3.5, 1.0)],
        &[(0.0, 3.0), (4.0, 3.0)],
        &[(1.8, 1.0), (1.8, 5.5), (4.0, 5.5)],
    ],
    &[
        &[(0.0, 2.5), (4.0, 1.5), (3.2, 3.5)],
        &[(1.0, 0.0), (2.5, 6.0)],
    ],
    &[
        &[(0.8, 1.5), (3.0, 1.5), (3.0, 5.0)],
        &[(0.0, 5.0), (4.0, 5.0)],
    ],
    &[
        &[(0.5, 0.8), (3.8, 0.8), (3.8, 5.5), (0.5, 5.5)],
        &[(0.8, 3.1), (3.8, 3.1)],
    ],
    &[
        &[(0.8, 0.5), (3.2, 0.5)],
        &[(0.3, 2.2), (3.8, 2.2), (3.0, 4.5), (1.2, 6.0)],
    ],
    &[
        &[(0.8, 0.5), (0.8, 3.5)],
        &[(3.2, 0.0), (3.2, 3.5), (1.5, 6.0)],
    ],
    &[
        &[(1.2, 0.5), (1.2, 3.5), (0.0, 6.0)],
        &[(2.8, 0.0), (2.8, 5.5), (4.0, 4.0)],
    ],
    &[&[(0.8, 0.0), (0.8, 5.8), (4.0, 2.5)]],
    &[&[(0.5, 1.0), (3.8, 1.0), (3.8, 5.5), (0.5, 5.5), (0.5, 1.0)]],
    &[&[(0.5, 2.5), (0.5, 1.0), (3.8, 1.0), (3.3, 4.0), (1.2, 6.0)]],
    &[&[(0.3, 0.8), (1.5, 1.8)], &[(0.3, 6.0), (4.0, 2.0)]],
    // Digits.
    &[
        &[(0.5, 0.5), (3.5, 0.5), (3.5, 5.5), (0.5, 5.5), (0.5, 0.5)],
        &[(0.5, 5.5), (3.5, 0.5)],
    ],
    &[
        &[(1.0, 1.2), (2.2, 0.3), (2.2, 6.0)],
        &[(1.0, 6.0), (3.4, 6.0)],
    ],
    &[&[
        (0.4, 1.2),
        (1.5, 0.3),
        (3.0, 0.3),
        (3.6, 1.5),
        (0.4, 6.0),
        (3.8, 6.0),
    ]],
    &[&[
        (0.4, 0.4),
        (3.6, 0.4),
        (1.8, 2.6),
        (3.6, 3.6),
        (3.4, 5.2),
        (2.2, 6.0),
        (0.4, 5.6),
    ]],
    &[&[(3.0, 6.0), (3.0, 0.3), (0.3, 4.3), (4.0, 4.3)]],
    &[&[
        (3.6, 0.3),
        (0.6, 0.3),
        (0.4, 2.8),
        (2.6, 2.6),
        (3.6, 3.8),
        (3.2, 5.6),
        (0.4, 5.9),
    ]],
    &[&[
        (3.2, 0.3),
        (1.0, 2.0),
        (0.5, 4.5),
        (1.5, 6.0),
        (3.0, 5.8),
        (3.6, 4.4),
        (2.6, 3.2),
        (0.7, 3.6),
    ]],
    &[&[(0.4, 0.4), (3.8, 0.4), (1.6, 6.0)]],
    &[&[
        (2.0, 3.0),
        (0.8, 2.2),
        (0.9, 0.8),
        (2.0, 0.3),
        (3.1, 0.8),
        (3.2, 2.2),
        (2.0, 3.0),
        (0.6, 4.0),
        (0.7, 5.5),
        (2.0, 6.0),
        (3.3, 5.5),
        (3.4, 4.0),
        (2.0, 3.0),
    ]],
    &[&[
        (3.3, 2.6),
        (1.4, 3.0),
        (0.4, 1.8),
        (1.0, 0.4),
        (2.6, 0.3),
        (3.4, 1.4),
        (3.0, 4.0),
        (1.0, 6.0),
    ]],
];

/// Every glyph, and the katakana again mirrored: as many as there are.
pub fn glyph_count() -> usize {
    GLYPHS.len() + KATAKANA
}

/// The katakana, the first glyphs; mirrored, they are glyphs too.
const KATAKANA: usize = 45;

/// A small, quick, reproducible random sequence (splitmix64): the same
/// seed gives the same rain, so that pictures can be compared.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// 0 to 1, 1 excluded.
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// `lo` to `hi`, `hi` excluded.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    /// 0 to `n`, `n` excluded.
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
}

/// A number that looks random, from a few others.
pub fn hash(values: &[u64]) -> u64 {
    let mut rng = Rng::new(0x5EED);
    for v in values {
        rng.0 ^= v.wrapping_mul(0x2545_F491_4F6C_DD1D);
        rng.next_u64();
    }
    rng.next_u64()
}

/// Distance from `p` to the segment `a`–`b`.
fn to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx * dx + dy * dy;
    let t = if len > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (x, y) = (a.0 + t * dx - p.0, a.1 + t * dy - p.1);
    (x * x + y * y).sqrt()
}

/// How much of each pixel glyph `index` covers, 0 to 1, drawn into a
/// `w`×`h` box of pixels with strokes `stroke` pixels wide, row by row.
pub fn glyph_mask(index: usize, w: u32, h: u32, stroke: f32) -> Vec<f32> {
    let i = index % glyph_count();
    let (strokes, mirrored) = if i < GLYPHS.len() {
        (GLYPHS[i], false)
    } else {
        (GLYPHS[i - GLYPHS.len()], true)
    };
    // The grid's 4×6 inside the box, the stroke's half kept in.
    let half = stroke / 2.0;
    let (sx, sy) = (
        (w as f32 - stroke).max(1.0) / 4.0,
        (h as f32 - stroke).max(1.0) / 6.0,
    );
    let place = |(x, y): (f32, f32)| {
        let x = if mirrored { 4.0 - x } else { x };
        (half + x * sx, half + y * sy)
    };
    let segments: Vec<((f32, f32), (f32, f32))> = strokes
        .iter()
        .flat_map(|s| s.windows(2).map(|p| (place(p[0]), place(p[1]))))
        .collect();
    (0..w * h)
        .map(|n| {
            let p = ((n % w) as f32 + 0.5, (n / w) as f32 + 0.5);
            let d = segments
                .iter()
                .map(|&(a, b)| to_segment(p, a, b))
                .fold(f32::MAX, f32::min);
            (half + 0.5 - d).clamp(0.0, 1.0)
        })
        .collect()
}

/// Cells of random glyphs to cut rows and columns from: `cols`×`rows`
/// cells of `cell` pixels, premultiplied, as B, G, R, A bytes
/// (ARGB8888). Every cell is a glyph in the rain's green, of a brightness
/// of its own; the last row holds bright heads.
#[derive(Debug, Clone)]
pub struct Sheet {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub cell: (u32, u32),
    pub cols: u32,
    pub rows: u32,
}

impl Sheet {
    /// A sheet whose cells are [`CELL`] at `scale`.
    pub fn new(scale: f64, cols: u32, rows: u32) -> Self {
        let cell = (
            ((f64::from(CELL.0) * scale).round() as u32).max(2),
            ((f64::from(CELL.1) * scale).round() as u32).max(2),
        );
        // The glyph inside its cell, a little air around it.
        let (gw, gh) = (
            (cell.0 as f32 * 0.75).round() as u32,
            (cell.1 as f32 * 0.72).round() as u32,
        );
        let (ox, oy) = ((cell.0 - gw) / 2, (cell.1 - gh) / 2);
        let stroke = (scale as f32 * 1.1).max(1.0);
        let masks: Vec<Vec<f32>> = (0..glyph_count())
            .map(|i| glyph_mask(i, gw, gh, stroke))
            .collect();
        let (width, height) = (cols * cell.0, rows * cell.1);
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let mut rng = Rng::new(0x3A7_21C5);
        for row in 0..rows {
            for col in 0..cols {
                let glyph = rng.below(masks.len());
                let (color, strength) = if row + 1 == rows {
                    (HEAD, 1.0)
                } else {
                    (GREEN, rng.range(0.4, 1.0))
                };
                let mask = &masks[glyph];
                for y in 0..gh {
                    for x in 0..gw {
                        let a = mask[(y * gw + x) as usize] * strength;
                        if a <= 0.0 {
                            continue;
                        }
                        let (px, py) = (col * cell.0 + ox + x, row * cell.1 + oy + y);
                        let at = ((py * width + px) * 4) as usize;
                        let c = |v: u8| (f32::from(v) * a).round() as u8;
                        pixels[at..at + 4].copy_from_slice(&[
                            c(color.b),
                            c(color.g),
                            c(color.r),
                            (a * 255.0).round() as u8,
                        ]);
                    }
                }
            }
        }
        Self {
            pixels,
            width,
            height,
            cell,
            cols,
            rows,
        }
    }
}

/// A drop of the still rain: a column of glyphs from `top`, logical.
#[derive(Debug, Clone, PartialEq)]
pub struct Drop {
    pub x: f32,
    pub top: f32,
    pub glyphs: Vec<usize>,
    /// How strong its green is, 0 to 1.
    pub alpha: f32,
}

/// Between drops, and from one glyph to the next below, logical pixels
/// (the boards' rain: 20 px glyphs on 24 px lines, a column every 48 px).
pub const RAIN_STEP: f32 = 48.0;
pub const RAIN_LINE: f32 = 24.0;
/// A rain glyph's box, and the column it is centred in.
pub const RAIN_GLYPH: (f32, f32) = (12.0, 17.0);
pub const RAIN_COLUMN: f32 = 28.0;
/// The rain's strokes, logical pixels.
const RAIN_STROKE: f32 = 1.6;

/// The still rain over a `width`×`height` logical screen: one drop per
/// column, 8 to 30 glyphs long, anywhere from above the screen down.
pub fn rain(width: f32, height: f32, seed: u64) -> Vec<Drop> {
    let mut rng = Rng::new(seed);
    let columns = (width / RAIN_STEP).ceil().max(1.0) as usize;
    (0..columns)
        .map(|i| {
            let len = 8 + rng.below(23);
            let glyphs = (0..len).map(|_| rng.below(glyph_count())).collect();
            Drop {
                x: i as f32 * RAIN_STEP + 10.0,
                top: rng.unit() * height - 300.0,
                glyphs,
                alpha: rng.range(0.10, 0.50),
            }
        })
        .collect()
}

/// Draws the still rain into `image` at `scale` pixels per logical pixel:
/// each drop fades in from its tail, its last glyph a brighter head.
pub fn draw_rain(image: &mut RgbaImage, scale: f32, seed: u64) {
    let (w, h) = (image.width(), image.height());
    let (gw, gh) = (
        (RAIN_GLYPH.0 * scale).round().max(2.0) as u32,
        (RAIN_GLYPH.1 * scale).round().max(2.0) as u32,
    );
    let stroke = (RAIN_STROKE * scale).max(1.0);
    let mut masks: Vec<Option<Vec<f32>>> = vec![None; glyph_count()];
    for drop in rain(w as f32 / scale, h as f32 / scale, seed) {
        let n = drop.glyphs.len();
        for (j, &glyph) in drop.glyphs.iter().enumerate() {
            let top = drop.top + j as f32 * RAIN_LINE;
            let (x0, y0) = (
                ((drop.x + (RAIN_COLUMN - RAIN_GLYPH.0) / 2.0) * scale).round() as i64,
                ((top + (RAIN_LINE - RAIN_GLYPH.1) / 2.0) * scale).round() as i64,
            );
            if y0 + i64::from(gh) < 0 || y0 >= i64::from(h) || x0 >= i64::from(w) {
                continue;
            }
            // From faint at the tail to the drop's own strength.
            let along = (j + 1) as f32 / n as f32;
            let head = j + 1 == n;
            let (color, alpha) = if head {
                (GREEN.mix(HEAD, 0.5), (drop.alpha * 1.6).min(0.9))
            } else {
                (GREEN, drop.alpha * (0.35 + 0.65 * along))
            };
            let mask = masks[glyph].get_or_insert_with(|| glyph_mask(glyph, gw, gh, stroke));
            for y in 0..gh {
                let py = y0 + i64::from(y);
                if !(0..i64::from(h)).contains(&py) {
                    continue;
                }
                for x in 0..gw {
                    let px = x0 + i64::from(x);
                    if !(0..i64::from(w)).contains(&px) {
                        continue;
                    }
                    let a = mask[(y * gw + x) as usize] * alpha;
                    if a <= 0.0 {
                        continue;
                    }
                    let pixel = image.get_pixel_mut(px as u32, py as u32);
                    let below = Color::rgb(pixel.0[0], pixel.0[1], pixel.0[2]);
                    let c = below.mix(color, a);
                    pixel.0 = [c.r, c.g, c.b, 255];
                }
            }
        }
    }
}

/// Every glyph of the rain, white, for a shader to look up: `cols`×`rows`
/// cells of `cell` pixels, each glyph `glyph` pixels at the cell's corner
/// plus one, so that sampling between pixels never reaches a neighbour.
/// RGBA bytes, premultiplied.
#[derive(Debug, Clone)]
pub struct Atlas {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub cols: u32,
    pub rows: u32,
    pub cell: (u32, u32),
    pub glyph: (u32, u32),
}

impl Atlas {
    /// The rain's glyphs at `scale`.
    pub fn new(scale: f32) -> Self {
        let glyph = (
            (RAIN_GLYPH.0 * scale).round().max(2.0) as u32,
            (RAIN_GLYPH.1 * scale).round().max(2.0) as u32,
        );
        let cell = (glyph.0 + 2, glyph.1 + 2);
        let cols = 10;
        let rows = (glyph_count() as u32).div_ceil(cols);
        let (width, height) = (cols * cell.0, rows * cell.1);
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let stroke = (RAIN_STROKE * scale).max(1.0);
        for i in 0..glyph_count() {
            let mask = glyph_mask(i, glyph.0, glyph.1, stroke);
            let (cx, cy) = (
                (i as u32 % cols) * cell.0 + 1,
                (i as u32 / cols) * cell.1 + 1,
            );
            for y in 0..glyph.1 {
                for x in 0..glyph.0 {
                    let a = (mask[(y * glyph.0 + x) as usize] * 255.0).round() as u8;
                    let at = (((cy + y) * width + cx + x) * 4) as usize;
                    pixels[at..at + 4].copy_from_slice(&[a, a, a, a]);
                }
            }
        }
        Self {
            pixels,
            width,
            height,
            cols,
            rows,
            cell,
            glyph,
        }
    }
}

/// How a view of the shell is drawn: as in every theme, or leaving room
/// for what the compositor draws of the Matrix theme (its rain behind,
/// its padlock, its progress strip).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Variant {
    #[default]
    Plain,
    Matrix,
}

/// The lock screen's padlock, a glyph per set bit, 12 cells across (the
/// highest bit on the left) and 16 down (docs/design/matrix-theme.md, §12).
pub const PADLOCK: [u16; 16] = [
    0b0000_1111_0000,
    0b0001_1111_1000,
    0b0011_0000_1100,
    0b0011_0000_1100,
    0b0110_0000_0110,
    0b0110_0000_0110,
    0b0110_0000_0110,
    0b1111_1111_1111,
    0b1111_1111_1111,
    0b1111_1001_1111,
    0b1111_0000_1111,
    0b1111_1001_1111,
    0b1111_1001_1111,
    0b1111_1001_1111,
    0b1111_1111_1111,
    0b1111_1111_1111,
];
/// The padlock's cells across and down.
pub const PADLOCK_CELLS: (u32, u32) = (12, 16);

/// A capital letter on a grid 5 wide and 7 tall, a row per byte (the
/// highest of its five bits on the left), for the boot's large words;
/// accents dropped, anything else blank.
pub fn letter(c: char) -> [u8; 7] {
    let base = match c {
        'À' | 'Â' | 'Ä' | 'à' | 'â' | 'ä' => 'A',
        'Ç' | 'ç' => 'C',
        'É' | 'È' | 'Ê' | 'Ë' | 'é' | 'è' | 'ê' | 'ë' => 'E',
        'Î' | 'Ï' | 'î' | 'ï' => 'I',
        'Ô' | 'Ö' | 'ô' | 'ö' => 'O',
        'Ù' | 'Û' | 'Ü' | 'ù' | 'û' | 'ü' => 'U',
        c => c.to_ascii_uppercase(),
    };
    match base {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 2, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 17, 25, 21, 19, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        _ => [0; 7],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_draws_something_inside_its_box() {
        for i in 0..glyph_count() {
            let mask = glyph_mask(i, 12, 17, 1.6);
            let ink: f32 = mask.iter().sum();
            assert!(ink > 12.0, "glyph {i} is nearly empty: {ink}");
            assert!(ink < 12.0 * 17.0 * 0.6, "glyph {i} is a blot: {ink}");
        }
    }

    #[test]
    fn mirrored_katakana_are_mirrored() {
        let (w, h) = (12, 17);
        let plain = glyph_mask(0, w, h, 1.6);
        let mirrored = glyph_mask(GLYPHS.len(), w, h, 1.6);
        for y in 0..h {
            for x in 0..w {
                let a = plain[(y * w + x) as usize];
                let b = mirrored[(y * w + (w - 1 - x)) as usize];
                assert!((a - b).abs() < 0.2, "at {x},{y}: {a} and {b}");
            }
        }
    }

    #[test]
    fn the_rain_is_the_same_for_the_same_seed() {
        assert_eq!(rain(1280.0, 800.0, 7), rain(1280.0, 800.0, 7));
        assert_ne!(rain(1280.0, 800.0, 7), rain(1280.0, 800.0, 8));
        let drops = rain(1280.0, 800.0, 7);
        assert_eq!(drops.len(), 27, "a drop every 48 px");
        assert!(drops.iter().all(|d| (8..=30).contains(&d.glyphs.len())));
        assert!(drops.iter().all(|d| (0.10..0.50).contains(&d.alpha)));
    }

    #[test]
    fn the_sheet_has_its_heads_in_its_last_row() {
        let sheet = Sheet::new(1.0, 8, 4);
        assert_eq!((sheet.width, sheet.height), (64, 56));
        let brightest = |row: u32| {
            (row * sheet.cell.1 * sheet.width..(row + 1) * sheet.cell.1 * sheet.width)
                .map(|i| sheet.pixels[(i * 4 + 2) as usize])
                .max()
                .unwrap_or(0)
        };
        // Red is all but absent from the green, and strong in the heads.
        assert!(brightest(0) < 10, "{}", brightest(0));
        assert!(brightest(3) > 150, "{}", brightest(3));
    }

    #[test]
    fn the_atlas_holds_every_glyph_apart() {
        let atlas = Atlas::new(1.0);
        assert_eq!(atlas.cols * atlas.rows, glyph_count() as u32);
        // The cells' borders stay empty.
        for y in 0..atlas.height {
            for x in (0..atlas.width).step_by(atlas.cell.0 as usize) {
                assert_eq!(atlas.pixels[((y * atlas.width + x) * 4 + 3) as usize], 0);
            }
        }
    }

    #[test]
    fn letters_fit_their_grid_and_accents_drop() {
        for c in 'A'..='Z' {
            let rows = letter(c);
            assert!(rows.iter().all(|r| *r < 32), "{c}");
            assert!(rows.iter().any(|r| *r != 0), "{c} is blank");
        }
        assert_eq!(letter('È'), letter('E'));
        assert_eq!(letter('ê'), letter('E'));
        assert_eq!(letter(' '), [0; 7]);
        assert!(PADLOCK.iter().all(|r| *r < 1 << PADLOCK_CELLS.0));
    }

    #[test]
    fn rain_lands_on_the_picture() {
        let mut image = RgbaImage::from_pixel(320, 200, image::Rgba([2, 4, 10, 255]));
        draw_rain(&mut image, 1.0, 7);
        let green = image.pixels().filter(|p| p.0[1] > 40).count();
        assert!(green > 100, "{green} green pixels");
        assert!(image.pixels().all(|p| p.0[3] == 255), "still opaque");
    }
}
