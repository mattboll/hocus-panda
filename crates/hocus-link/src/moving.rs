//! The wallpapers that move (#433), "rain" and "stars", and the glyphs the
//! windows' rain falls in (#432): what the compositor's shaders draw on the
//! GPU, and the still picture drawn on the CPU where they do not run (no
//! GPU, reduced motion, the lock, the Settings' tiles).
//!
//! Both are a function of an animation's time and nothing else: the
//! shaders keep no state from frame to frame, and the still picture is the
//! same recipe at a fixed moment ([`STILL_TICK`]). The glyphs are the
//! desktop's own drawings, half-width katakana, digits and a few signs on
//! a grid of 5 × 7 dots, drawn mirrored as in the film; no font is read.
//!
//! The rain's colours are tokens ([`RainColours`]) that a theme can set;
//! every theme has the phosphor green for now.

use crate::theme::AccentColours;

/// A wallpaper that moves: `[wallpaper] picture = "rain"` or `"stars"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Moving {
    /// Columns of glyphs falling one cell per frame, a bright head and a
    /// green tail.
    Rain,
    /// Three planes of stars drifting towards the viewer over a night-blue
    /// haze.
    Stars,
}

impl Moving {
    pub const ALL: [Self; 2] = [Self::Rain, Self::Stars];

    /// Its word in the file.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Rain => "rain",
            Self::Stars => "stars",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.word() == word)
    }

    /// Its name, in French then English.
    pub const fn names(self) -> (&'static str, &'static str) {
        match self {
            Self::Rain => ("Pluie", "Rain"),
            Self::Stars => ("Étoiles", "Stars"),
        }
    }

    /// Frames a second while it runs on mains power; half on battery.
    pub const fn fps(self) -> u32 {
        match self {
            Self::Rain => RAIN_FPS,
            Self::Stars => STARS_FPS,
        }
    }

    /// The accent it gives `[appearance] accent = "wallpaper"`.
    pub fn accent(self) -> AccentColours {
        match self {
            Self::Rain => AccentColours::lifted(PHOSPHOR.tail),
            Self::Stars => AccentColours::lifted([0x7F, 0x9C, 0xE8]),
        }
    }
}

/// The rain's pace: its heads move one cell per frame, as in the film.
pub const RAIN_FPS: u32 = 15;
/// The stars drift slowly: 20 frames a second are enough.
pub const STARS_FPS: u32 = 20;
/// A cell of the wallpaper's rain, logical pixels.
pub const RAIN_CELL: (f64, f64) = (14.0, 22.0);
/// A cell of a window's rain, logical pixels: its columns are 10 px wide.
pub const WINDOW_CELL: (f64, f64) = (10.0, 15.0);
/// The tail fades out in this many steps.
pub const TAIL_STEPS: f64 = 6.0;
/// The moment the still picture shows: the rain's frame at this tick (its
/// columns well under way), the stars at this many seconds.
pub const STILL_TICK: u32 = 90;

/// The rain's colours, tokens a theme sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RainColours {
    /// A column's head.
    pub head: [u8; 3],
    /// Its tail, and a window's veil as it opens.
    pub tail: [u8; 3],
    /// The wallpaper's background, from its top to its bottom.
    pub top: [u8; 3],
    pub bottom: [u8; 3],
}

/// Phosphor green on black to night blue.
pub const PHOSPHOR: RainColours = RainColours {
    head: [0xCC, 0xFF, 0xCC],
    tail: [0x00, 0xF0, 0x60],
    top: [0x02, 0x04, 0x0A],
    bottom: [0x05, 0x0B, 0x1C],
};

/// The rain's colours in `scheme`: phosphor green in every theme until a
/// theme gives its own.
pub const fn rain_colours(_scheme: crate::theme::Scheme) -> RainColours {
    PHOSPHOR
}

/// The stars' colours: the haze's two blues, the stars' white.
pub const STARS_TOP: [u8; 3] = [0x03, 0x06, 0x12];
pub const STARS_BOTTOM: [u8; 3] = [0x07, 0x0E, 0x26];
pub const STARS_HAZE: [u8; 3] = [0x1A, 0x2C, 0x5C];
pub const STAR: [u8; 3] = [0xDC, 0xE6, 0xFF];

/// The glyphs: how many, and their grid of dots.
pub const GLYPHS: usize = 64;
pub const GLYPH_DOTS: (usize, usize) = (5, 7);

/// Each glyph's seven rows of five dots, `#` lit: forty-six katakana, the
/// ten digits and eight signs.
const DRAWN: [&str; GLYPHS] = [
    // ア イ ウ エ オ カ キ ク
    "#####....#.#.#..##...#....#...#....",
    "....#...#...#...##..#.#....#....#..",
    "..#..######...#....#...#...#...#...",
    ".....#####..#....#....#....#..#####",
    "...#.#####...#...##..#.#.#..#....#.",
    ".#...#####.#..#.#..#.#..##...##..#.",
    ".#...#####..#..#####..#....#....#..",
    ".####.#..##...#....#...#...#...#...",
    // ケ コ サ シ ス セ ソ タ
    ".#....#####..#....#....#...#...#...",
    ".....#####....#....#....#....######",
    ".#.#.#####.#.#..#.#....#...#...#...",
    "##.......###..#....#...#...#..##...",
    ".....#####....#...#...#...#.#.#...#",
    ".#...#####.#..#.#.#..#....#.....###",
    "#...##...#.#..#....#...#...#...#...",
    ".####.#..##.#.#...#...#...#...#....",
    // チ ツ テ ト ナ ニ ヌ ネ
    "...#.###....#..#####..#....#...#...",
    "#.#.##.#.#....#....#...#...#...#...",
    ".###......#####..#....#....#...#...",
    ".#....#....##...#.#..#....#....#...",
    "..#..#####..#....#....#...#...#....",
    "......###................#####.....",
    "#####....#.#.#...#...#.#.#.........",
    "..#..#####...#...#...###.#.#.#..#..",
    // ノ ハ ヒ フ ヘ ホ マ ミ
    "....#....#...#...#...#...#.........",
    ".......#..#..#.#...##...##...#.....",
    "#....#....####.#....#....#.....####",
    "#####....#....#...#...#...#...#....",
    "......#...#.#.....#.....#..........",
    "..#..#####..#..#.#.##.#.#..#....#..",
    "#####....#...#.#.#...#.....#.......",
    ".###.......###......###.....##.....",
    // ム メ モ ヤ ユ ヨ ラ リ
    "..#....#...#....#...#..#.#####....#",
    "....#...#.#.#...#....#.#.#.........",
    ".###...#..#####..#....#....#.....##",
    ".#...#####.#..#.#.#..#....#....#...",
    "......###....#....#....#.#####.....",
    "#####....#.####....#....######.....",
    ".###......#####....#...#...#...#...",
    "#..#.#..#.#..#.#..#....#...#...#...",
    // ル レ ロ ワ ヲ ン : =
    "..#..#.#..#.#..#.#.##.#.##.##......",
    "#....#....#....#...##..#.###.......",
    ".....######...##...##...######.....",
    "######...##...#....#...#...#...#...",
    "#####....######....#...#...#...#...",
    "##.......#....#....#...#...#..##...",
    "......##...##........##...##.......",
    "..........#####.....#####..........",
    // * + > Z < | 0 1
    ".......#..#.#.#.###.#.#.#..#.......",
    ".......#....#..#####..#....#.......",
    ".#.....#.....#.....#...#...#...#...",
    "#####....#...#...#...#...#....#####",
    "...#...#...#...#.....#.....#.....#.",
    "..#....#....#....#....#....#....#..",
    ".###.#...##..###.#.###..##...#.###.",
    "..#...##....#....#....#....#...###.",
    // 2 3 4 5 6 7 8 9
    ".###.#...#....#...#...#...#...#####",
    "#####...#...#.....#.....##...#.###.",
    "...#...##..#.#.#..#.#####...#....#.",
    "######....####.....#....##...#.###.",
    "..##..#...#....####.#...##...#.###.",
    "#####....#...#...#...#....#....#...",
    ".###.#...##...#.###.#...##...#.###.",
    ".###.#...##...#.####....#...#..##..",
];

/// Glyph `index`'s dot at column `x` (from its left, unmirrored) and row
/// `y`: whether it is lit. A drawing shorter than its 35 dots is unlit
/// past its end.
pub fn lit(index: usize, x: usize, y: usize) -> bool {
    let (w, h) = GLYPH_DOTS;
    if x >= w || y >= h {
        return false;
    }
    DRAWN[index % GLYPHS].as_bytes().get(y * w + x) == Some(&b'#')
}

/// The atlas the shaders read the glyphs from: an 8 × 8 grid of 8 × 8
/// texels, a glyph's dots in the top left 5 × 7 of its square, lit texels
/// opaque white. 64 × 64 texels, 16 KB, whatever the screen's scale: the
/// shaders draw each dot at the size of its cell.
pub fn atlas() -> (i32, i32, Vec<u8>) {
    const SIDE: usize = 64;
    let mut pixels = vec![0u8; SIDE * SIDE * 4];
    for g in 0..GLYPHS {
        let (gx, gy) = ((g % 8) * 8, (g / 8) * 8);
        for y in 0..GLYPH_DOTS.1 {
            for x in 0..GLYPH_DOTS.0 {
                if lit(g, x, y) {
                    let at = ((gy + y) * SIDE + gx + x) * 4;
                    pixels[at..at + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
    }
    (SIDE as i32, SIDE as i32, pixels)
}

/// The shaders' hash, in the same words: a fraction from a point.
pub fn hash(x: f64, y: f64) -> f64 {
    let v = (x * 127.1 + y * 311.7).sin() * 43_758.545_3;
    v - v.floor()
}

/// One stream of a column of the wallpaper's rain: its period in cells
/// (the screen's rows, its tail and a gap), where it starts, its tail.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stream {
    pub period: f64,
    pub offset: f64,
    pub tail: f64,
}

/// Column `column`'s stream `which` (0 or 1) on a screen `rows` cells
/// high.
pub fn stream(column: f64, which: f64, rows: f64) -> Stream {
    let tail = 8.0 + (hash(column, 11.0 + which) * 18.0).floor();
    let period = rows + tail + (hash(column, 23.0 + which) * rows * 1.5).floor();
    let offset = (hash(column, 37.0 + which) * period).floor();
    Stream {
        period,
        offset,
        tail,
    }
}

/// How bright cell (`column`, `row`) of the rain is at `tick`: 1 at a
/// head, the tail's steps behind it, 0 elsewhere; and whether it is a head.
pub fn rain_cell(column: f64, row: f64, rows: f64, tick: f64) -> (f64, bool) {
    let mut best = (0.0, false);
    for which in [0.0, 1.0] {
        let s = stream(column, which, rows);
        let head = (tick + s.offset) % s.period;
        let behind = head - row;
        if behind < 0.0 || behind >= s.tail {
            continue;
        }
        if behind < 1.0 {
            return (1.0, true);
        }
        let step = (behind * TAIL_STEPS / s.tail).floor();
        let level = 1.0 - step / TAIL_STEPS;
        if level > best.0 {
            best = (level, false);
        }
    }
    best
}

/// The glyph of cell (`column`, `row`) at `tick`: a head's changes every
/// frame; the others now and then, each at its own moment.
pub fn rain_glyph(column: f64, row: f64, tick: f64, head: bool) -> usize {
    let epoch = if head {
        tick
    } else {
        ((tick + hash(column, row) * 40.0) / 40.0).floor()
    };
    ((hash(column + epoch * 0.37, row + epoch * 0.71) * GLYPHS as f64).floor() as usize)
        .min(GLYPHS - 1)
}

/// Where a glyph's dots are in its cell, as fractions of the cell: the
/// box the 5 × 7 dots fill.
pub const GLYPH_BOX: ([f64; 2], [f64; 2]) = ([0.16, 0.18], [0.84, 0.82]);

fn mix(a: [u8; 3], b: [u8; 3], t: f64) -> [f64; 3] {
    std::array::from_fn(|i| f64::from(a[i]) + (f64::from(b[i]) - f64::from(a[i])) * t)
}

/// The still picture of `moving` for a screen `size` physical pixels large
/// at `scale`: its pixels (R, G, B, A in memory), drawn on the CPU.
pub fn still(moving: Moving, size: (i32, i32), scale: f64) -> Vec<u8> {
    match moving {
        Moving::Rain => still_rain(size, scale, PHOSPHOR),
        Moving::Stars => still_stars(size, scale),
    }
}

fn still_rain((w, h): (i32, i32), scale: f64, colours: RainColours) -> Vec<u8> {
    let (w, h) = (w.max(1) as usize, h.max(1) as usize);
    let mut pixels = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let c = mix(colours.top, colours.bottom, y as f64 / h as f64);
        for _ in 0..w {
            pixels.extend_from_slice(&[c[0] as u8, c[1] as u8, c[2] as u8, 255]);
        }
    }
    let scale = scale.max(0.5);
    let cell = (RAIN_CELL.0 * scale, RAIN_CELL.1 * scale);
    let (columns, rows) = (
        (w as f64 / cell.0).ceil() as usize,
        (h as f64 / cell.1).ceil() as usize,
    );
    let tick = f64::from(STILL_TICK);
    let (box0, box1) = GLYPH_BOX;
    let pitch = (
        (box1[0] - box0[0]) * cell.0 / GLYPH_DOTS.0 as f64,
        (box1[1] - box0[1]) * cell.1 / GLYPH_DOTS.1 as f64,
    );
    for column in 0..columns {
        for row in 0..rows {
            let (bright, head) = rain_cell(column as f64, row as f64, rows as f64, tick);
            if bright <= 0.0 {
                continue;
            }
            let colour = if head { colours.head } else { colours.tail };
            let glyph = rain_glyph(column as f64, row as f64, tick, head);
            for dy in 0..GLYPH_DOTS.1 {
                for dx in 0..GLYPH_DOTS.0 {
                    // Mirrored, as in the film.
                    if !lit(glyph, GLYPH_DOTS.0 - 1 - dx, dy) {
                        continue;
                    }
                    let x0 = column as f64 * cell.0 + box0[0] * cell.0 + dx as f64 * pitch.0;
                    let y0 = row as f64 * cell.1 + box0[1] * cell.1 + dy as f64 * pitch.1;
                    fill(
                        &mut pixels,
                        (w, h),
                        (x0, y0, pitch.0 * 0.86, pitch.1 * 0.86),
                        colour,
                        bright,
                    );
                }
            }
        }
    }
    pixels
}

/// A rectangle (`x`, `y`, width, height) painted over `pixels` in
/// `colour` at `alpha`, its edges covering their share of a pixel.
fn fill(
    pixels: &mut [u8],
    (w, h): (usize, usize),
    (x, y, rw, rh): (f64, f64, f64, f64),
    colour: [u8; 3],
    alpha: f64,
) {
    let (x1, y1) = (x + rw, y + rh);
    let (left, top) = (x.floor().max(0.0) as usize, y.floor().max(0.0) as usize);
    let (right, bottom) = ((x1.ceil() as usize).min(w), (y1.ceil() as usize).min(h));
    for py in top..bottom {
        let cy = (y1.min(py as f64 + 1.0) - y.max(py as f64)).clamp(0.0, 1.0);
        for px in left..right {
            let cx = (x1.min(px as f64 + 1.0) - x.max(px as f64)).clamp(0.0, 1.0);
            let a = cx * cy * alpha;
            let at = (py * w + px) * 4;
            for i in 0..3 {
                let under = f64::from(pixels[at + i]);
                pixels[at + i] = (under + (f64::from(colour[i]) - under) * a).round() as u8;
            }
        }
    }
}

/// The stars' three planes: how many cells across the screen's shorter
/// half each has at its middle depth, a cell's chance of a star.
pub const STAR_PLANES: usize = 3;
pub const STAR_CELLS: f64 = 9.0;
pub const STAR_CHANCE: f64 = 0.55;
/// A plane goes from far to near in this many seconds, growing 4 times:
/// the nearest stars drift about 6 logical px a second.
pub const STAR_CYCLE: f64 = 180.0;

/// A star of plane `plane` near point (`qx`, `qy`) (from the screen's
/// centre, in its shorter half) at `time` seconds: how bright it is there
/// and its radius in the same units, if one is.
pub fn star_at(plane: usize, qx: f64, qy: f64, time: f64) -> Option<(f64, f64, f64, f64)> {
    let phase = time / STAR_CYCLE + plane as f64 / STAR_PLANES as f64;
    let depth = phase - phase.floor();
    let epoch = phase.floor();
    // From half to twice the size, on a curve that makes speed even.
    let zoom = 0.5 * 4f64.powf(depth);
    let (sx, sy) = (qx / zoom * STAR_CELLS, qy / zoom * STAR_CELLS);
    let (cx, cy) = (sx.floor(), sy.floor());
    let seed = epoch * 7.13 + plane as f64 * 3.7;
    if hash(cx + seed, cy) > STAR_CHANCE {
        return None;
    }
    let ox = 0.2 + 0.6 * hash(cx, cy + seed);
    let oy = 0.2 + 0.6 * hash(cx + seed, cy + 1.3);
    let (px, py) = ((cx + ox) / STAR_CELLS * zoom, (cy + oy) / STAR_CELLS * zoom);
    let fade = smooth(0.0, 0.15, depth) * (1.0 - smooth(0.8, 1.0, depth));
    let twinkle = 0.65
        + 0.35
            * (time * (1.5 + 3.0 * hash(cx, cy + 5.1))
                + std::f64::consts::TAU * hash(cx + 2.7, cy))
            .sin();
    let size = (0.6 + 1.2 * hash(cx + 9.1, cy + seed)) * (0.5 + depth);
    Some((px, py, fade * twinkle, size))
}

fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn still_stars((w, h): (i32, i32), scale: f64) -> Vec<u8> {
    let (w, h) = (w.max(1) as usize, h.max(1) as usize);
    let half = w.min(h) as f64 / 2.0;
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    let mut pixels = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let c = haze(x as f64 / w as f64, y as f64 / h as f64);
            pixels.extend_from_slice(&[c[0] as u8, c[1] as u8, c[2] as u8, 255]);
        }
    }
    let time = f64::from(STILL_TICK);
    let scale = scale.max(0.5);
    // Each plane's cells, found from the screen's corners at its depth.
    for plane in 0..STAR_PLANES {
        let phase = time / STAR_CYCLE + plane as f64 / STAR_PLANES as f64;
        let zoom = 0.5 * 4f64.powf(phase - phase.floor());
        let span = |len: f64| (len / 2.0 / half / zoom * STAR_CELLS).ceil() as i64 + 1;
        let (nx, ny) = (span(w as f64), span(h as f64));
        for j in -ny..ny {
            for i in -nx..nx {
                let q = (
                    (i as f64 + 0.5) / STAR_CELLS * zoom,
                    (j as f64 + 0.5) / STAR_CELLS * zoom,
                );
                let Some((px, py, bright, size)) = star_at(plane, q.0, q.1, time) else {
                    continue;
                };
                let (sx, sy) = (cx + px * half, cy + py * half);
                let r = size * scale;
                fill(
                    &mut pixels,
                    (w, h),
                    (sx - r, sy - r, 2.0 * r, 2.0 * r),
                    STAR,
                    bright.clamp(0.0, 1.0),
                );
            }
        }
    }
    pixels
}

/// The stars' background at (`u`, `v`), fractions of the screen: a
/// night-blue gradient with two soft clouds.
pub fn haze(u: f64, v: f64) -> [f64; 3] {
    let base = mix(STARS_TOP, STARS_BOTTOM, v);
    let cloud = |x: f64, y: f64, r: f64| {
        let d = ((u - x).powi(2) + (v - y).powi(2)).sqrt() / r;
        (1.0 - d).max(0.0).powi(2)
    };
    let a = (0.35 * cloud(0.3, 0.35, 0.45) + 0.25 * cloud(0.75, 0.7, 0.4)).min(1.0);
    std::array::from_fn(|i| base[i] + (f64::from(STARS_HAZE[i]) - base[i]) * a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_drawn_and_none_twice() {
        let dots = GLYPH_DOTS.0 * GLYPH_DOTS.1;
        let mut seen = std::collections::HashSet::new();
        for (i, drawn) in DRAWN.iter().enumerate() {
            assert!(drawn.len() <= dots, "glyph {i}: {} dots", drawn.len());
            assert!(drawn.contains('#'), "glyph {i} is empty");
            let bits: Vec<bool> = (0..dots)
                .map(|d| lit(i, d % GLYPH_DOTS.0, d / GLYPH_DOTS.0))
                .collect();
            assert!(seen.insert(bits), "glyph {i} is another's");
        }
        let (w, h, pixels) = atlas();
        assert_eq!((w, h, pixels.len()), (64, 64, 64 * 64 * 4));
        // 16 KB whatever the scale: under the 128 KB of the spec.
        assert!(pixels.len() <= 128 * 1024);
    }

    #[test]
    fn a_column_falls_one_cell_per_tick_its_tail_in_six_steps() {
        let rows = 40.0;
        // Find a head and follow it.
        let (column, row, tick) = (0..200)
            .flat_map(|c| (0..40).map(move |r| (f64::from(c), f64::from(r))))
            .find_map(|(c, r)| rain_cell(c, r, rows, 100.0).1.then_some((c, r, 100.0)))
            .expect("a head somewhere");
        assert!(rain_cell(column, row + 1.0, rows, tick + 1.0).1);
        let levels: std::collections::BTreeSet<u32> = (1..30)
            .map(|k| rain_cell(column, row - f64::from(k), rows, tick).0)
            .filter(|b| *b > 0.0 && *b < 1.0)
            .map(|b| (b * 600.0).round() as u32)
            .collect();
        assert!(levels.len() <= 5 && !levels.is_empty(), "{levels:?}");
    }

    #[test]
    fn the_still_pictures_are_drawn() {
        for moving in Moving::ALL {
            assert_eq!(Moving::of(moving.word()), Some(moving));
            let pixels = still(moving, (320, 200), 1.0);
            assert_eq!(pixels.len(), 320 * 200 * 4);
            let bright = pixels.chunks(4).filter(|p| p[1] > 120).count();
            assert!(bright > 20, "{moving:?}: {bright} bright pixels");
            // Dark mostly.
            assert!(bright < 320 * 200 / 3, "{moving:?}: {bright}");
        }
    }
}
