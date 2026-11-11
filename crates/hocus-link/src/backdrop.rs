//! The desktop's built-in backgrounds (6.3): a dark gradient with soft
//! glows, as the bar's mockup shows it (bar 3.1), so that the bar's glass
//! has something to show. Two of the glows sit where the bar is on a
//! screen, the way a wallpaper's light would. Nuit (Night) is the default;
//! Brume, Aube and Forêt (Mist, Dawn, Forest) are the same recipe, the
//! stops and glows at the same places, in other colours. The compositor
//! draws the chosen one under the windows; the shell draws it again on the
//! lock screen (7.1), which shows the background alone; the Settings draw
//! it in their thumbnails ([`crate::wallpaper`]).
//!
//! It is computed once per screen size on the CPU at a quarter of the
//! screen's pixels (the gradients are smooth: drawn larger, they stay so),
//! and drawn stretched.

/// The picture is this many times smaller than the screen's pixels.
pub const SHRINK: i32 = 4;

/// The mockup's screen, which the glows' places and sizes are given on.
const BOARD: (f64, f64) = (2560.0, 1600.0);

/// A colour with its alpha, components from 0 to 1.
type Rgba = [f64; 4];

const fn hex(rgb: u32, a: f64) -> Rgba {
    [
        ((rgb >> 16) & 0xff) as f64 / 255.0,
        ((rgb >> 8) & 0xff) as f64 / 255.0,
        (rgb & 0xff) as f64 / 255.0,
        a,
    ]
}

/// The glows' centres and radii on the board, back to front as CSS paints
/// them; each palette gives their colours. Each fades out at 70 % of its
/// radii, as CSS's `radial-gradient(RXpx RYpx at X Y, colour, transparent
/// 70%)`.
const GLOWS: [((f64, f64), (f64, f64)); 5] = [
    ((0.20 * BOARD.0, 0.70 * BOARD.1), (900.0, 700.0)),
    ((0.92 * BOARD.0, 0.82 * BOARD.1), (1100.0, 800.0)),
    ((0.78 * BOARD.0, 0.18 * BOARD.1), (1200.0, 900.0)),
    // The lights behind the bar: circles from their box's 40 % and 50 %,
    // as far as its farthest corner.
    (
        (2240.0 + 0.4 * 380.0, 260.0 + 0.4 * 380.0),
        (
            0.6 * 380.0 * std::f64::consts::SQRT_2,
            0.6 * 380.0 * std::f64::consts::SQRT_2,
        ),
    ),
    (
        (2300.0 + 150.0, 1080.0 + 150.0),
        (
            150.0 * std::f64::consts::SQRT_2,
            150.0 * std::f64::consts::SQRT_2,
        ),
    ),
];

/// A built-in background: `linear-gradient(160deg, …)` at 0 %, 55 % and
/// 100 %, under the five glows in their colours.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Backdrop {
    /// `#10172a 0%, #0b1020 55%, #090c16 100%`, blue, green and the bar's
    /// amber and green lights: the desktop's background since the bar.
    #[default]
    Night,
    /// Slate greys with pale blue and white glows.
    Mist,
    /// Plum with violet, orange and amber.
    Dawn,
    /// Deep greens with one amber light behind the bar.
    Forest,
}

struct Palette {
    stops: [Rgba; 3],
    glows: [Rgba; 5],
}

const NIGHT: Palette = Palette {
    stops: [hex(0x10172A, 1.0), hex(0x0B1020, 1.0), hex(0x090C16, 1.0)],
    glows: [
        hex(0x8FB3FF, 0.18),
        hex(0x5B8DEF, 0.38),
        hex(0x6CC287, 0.30),
        hex(0xF5B44A, 0.55),
        hex(0x7EE0A0, 0.45),
    ],
};

const MIST: Palette = Palette {
    stops: [hex(0x2B3445, 1.0), hex(0x1F2634, 1.0), hex(0x171C27, 1.0)],
    glows: [
        hex(0xD5DBE5, 0.16),
        hex(0x8FB3FF, 0.30),
        hex(0xA9B8D0, 0.30),
        hex(0xEEF1F6, 0.30),
        hex(0x8FB3FF, 0.30),
    ],
};

const DAWN: Palette = Palette {
    stops: [hex(0x2A1830, 1.0), hex(0x1C1226, 1.0), hex(0x120C1A, 1.0)],
    glows: [
        hex(0x8F7BFF, 0.22),
        hex(0xF2945B, 0.40),
        hex(0xF5B44A, 0.30),
        hex(0xFFD68C, 0.55),
        hex(0xF2945B, 0.45),
    ],
};

const FOREST: Palette = Palette {
    stops: [hex(0x0F2019, 1.0), hex(0x0B1712, 1.0), hex(0x070F0B, 1.0)],
    glows: [
        hex(0x7EE0A0, 0.16),
        hex(0x3FA66B, 0.40),
        hex(0x6CC287, 0.32),
        hex(0xF5B44A, 0.40),
        hex(0x7EE0A0, 0.45),
    ],
};

impl Backdrop {
    pub const ALL: [Self; 4] = [Self::Night, Self::Mist, Self::Dawn, Self::Forest];

    /// Its word in the file: `[wallpaper] picture = "mist"`.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Night => "night",
            Self::Mist => "mist",
            Self::Dawn => "dawn",
            Self::Forest => "forest",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.word() == word)
    }

    /// Its name, in French then English.
    pub const fn names(self) -> (&'static str, &'static str) {
        match self {
            Self::Night => ("Nuit", "Night"),
            Self::Mist => ("Brume", "Mist"),
            Self::Dawn => ("Aube", "Dawn"),
            Self::Forest => ("Forêt", "Forest"),
        }
    }

    const fn palette(self) -> &'static Palette {
        match self {
            Self::Night => &NIGHT,
            Self::Mist => &MIST,
            Self::Dawn => &DAWN,
            Self::Forest => &FOREST,
        }
    }

    /// The accent it gives `[appearance] accent = "wallpaper"`: Nuit's
    /// own, the others' main glow's hue lifted to the accents' lightness.
    pub fn accent(self) -> crate::theme::AccentColours {
        match self {
            Self::Night => crate::theme::BACKDROP_ACCENT,
            Self::Mist => crate::theme::AccentColours::lifted([0xA9, 0xB8, 0xD0]),
            Self::Dawn => crate::theme::AccentColours::lifted([0xF2, 0x94, 0x5B]),
            Self::Forest => crate::theme::AccentColours::lifted([0x3F, 0xA6, 0x6B]),
        }
    }
}

/// Where the gradient's three colours are.
const STOPS: [f64; 3] = [0.0, 0.55, 1.0];

fn mix(a: Rgba, b: Rgba, t: f64) -> Rgba {
    let mut out = a;
    for (out, b) in out.iter_mut().zip(b) {
        *out += (b - *out) * t;
    }
    out
}

/// `top` painted over `under` (straight alpha, `under` opaque).
fn over(under: Rgba, top: Rgba) -> Rgba {
    let a = top[3];
    [
        under[0] * (1.0 - a) + top[0] * a,
        under[1] * (1.0 - a) + top[1] * a,
        under[2] * (1.0 - a) + top[2] * a,
        1.0,
    ]
}

/// The colour at (`x`, `y`) of the board.
fn colour_at(palette: &Palette, x: f64, y: f64) -> Rgba {
    // CSS's gradient line for 160°: through the centre, long enough that
    // the corners get the first and last colours.
    let angle = 160f64.to_radians();
    let (dx, dy) = (angle.sin(), -angle.cos());
    let length = (BOARD.0 * dx).abs() + (BOARD.1 * dy).abs();
    let t = (((x - BOARD.0 / 2.0) * dx + (y - BOARD.1 / 2.0) * dy) / length + 0.5).clamp(0.0, 1.0);
    let [top, middle, bottom] = palette.stops;
    let [at_top, at_middle, at_bottom] = STOPS;
    let mut colour = top;
    for ((from, to), (a, b)) in [
        ((top, middle), (at_top, at_middle)),
        ((middle, bottom), (at_middle, at_bottom)),
    ] {
        if t >= a {
            colour = mix(from, to, ((t - a) / (b - a)).clamp(0.0, 1.0));
        }
    }
    for (&(at, (rx, ry)), &glow) in GLOWS.iter().zip(&palette.glows) {
        let d = (((x - at.0) / rx).powi(2) + ((y - at.1) / ry).powi(2)).sqrt();
        let fade = (d / 0.7).min(1.0);
        let mut top = glow;
        top[3] *= 1.0 - fade;
        colour = over(colour, top);
    }
    colour
}

/// `backdrop` for a screen `size` physical pixels large: a picture
/// [`SHRINK`] times smaller to draw stretched over it, its size and its
/// pixels (R, G, B, A in memory: `Abgr8888`).
pub fn pixels(size: (i32, i32), backdrop: Backdrop) -> (i32, i32, Vec<u8>) {
    let palette = backdrop.palette();
    let (w, h) = ((size.0 / SHRINK).max(1), (size.1 / SHRINK).max(1));
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    // Dithered by a pixel's worth of noise: dark gradients band otherwise.
    let mut noise: u32 = 0x9E37_79B9;
    for row in 0..h {
        for col in 0..w {
            let x = (f64::from(col) + 0.5) / f64::from(w) * BOARD.0;
            let y = (f64::from(row) + 0.5) / f64::from(h) * BOARD.1;
            let c = colour_at(palette, x, y);
            noise ^= noise << 13;
            noise ^= noise >> 17;
            noise ^= noise << 5;
            let dither = f64::from(noise & 0xff) / 255.0 - 0.5;
            let byte = |v: f64| (v * 255.0 + dither).round().clamp(0.0, 255.0) as u8;
            pixels.extend_from_slice(&[byte(c[0]), byte(c[1]), byte(c[2]), 255]);
        }
    }
    (w, h, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn night_blue_with_glows_behind_the_bar() {
        // Top left: the gradient's first colour, about.
        let corner = colour_at(&NIGHT, 0.0, 0.0);
        assert!(corner[2] > corner[0] && corner[2] < 0.25, "{corner:?}");
        // The amber light behind the bar is brighter and warmer.
        let amber = colour_at(&NIGHT, 2392.0, 412.0);
        assert!(amber[0] > corner[0] + 0.2, "{amber:?}");
        assert!(amber[0] > amber[2], "{amber:?}");
    }

    #[test]
    fn the_others_are_the_same_recipe_in_their_colours() {
        // Forest is green where Night is blue; Dawn warm; Mist greyer.
        let at = |palette: &Palette| colour_at(palette, 400.0, 1200.0);
        let (night, forest, dawn, mist) = (at(&NIGHT), at(&FOREST), at(&DAWN), at(&MIST));
        assert!(
            forest[1] > forest[2] && night[2] > night[1],
            "{forest:?} {night:?}"
        );
        assert!(dawn[0] > dawn[1], "{dawn:?}");
        assert!(mist[0] > night[0], "{mist:?}");
        for backdrop in Backdrop::ALL {
            assert_eq!(Backdrop::of(backdrop.word()), Some(backdrop));
            let (w, h, pixels) = pixels((64, 40), backdrop);
            assert_eq!((w, h, pixels.len()), (16, 10, 16 * 10 * 4));
        }
    }
}
