//! The desktop's background until wallpapers come: a dark night-blue
//! gradient with soft glows, as the bar's mockup shows it (bar 3.1), so
//! that the bar's glass has something to show. Two of the glows sit where
//! the bar is on a screen, the way a wallpaper's light would.
//!
//! It is computed once per screen size on the CPU at a quarter of the
//! screen's pixels (the gradients are smooth: drawn larger, they stay so),
//! and drawn stretched under everything.

use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    utils::Transform,
};

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

/// An elliptic glow fading out at 70 % of its radii, as CSS's
/// `radial-gradient(RXpx RYpx at X Y, colour, transparent 70%)`.
struct Glow {
    /// Its centre, on the board.
    at: (f64, f64),
    radii: (f64, f64),
    colour: Rgba,
}

/// Back to front, as CSS paints them.
const GLOWS: [Glow; 5] = [
    Glow {
        at: (0.20 * BOARD.0, 0.70 * BOARD.1),
        radii: (900.0, 700.0),
        colour: hex(0x8FB3FF, 0.18),
    },
    Glow {
        at: (0.92 * BOARD.0, 0.82 * BOARD.1),
        radii: (1100.0, 800.0),
        colour: hex(0x5B8DEF, 0.38),
    },
    Glow {
        at: (0.78 * BOARD.0, 0.18 * BOARD.1),
        radii: (1200.0, 900.0),
        colour: hex(0x6CC287, 0.30),
    },
    // The lights behind the bar: circles from their box's 40 % and 50 %,
    // as far as its farthest corner.
    Glow {
        at: (2240.0 + 0.4 * 380.0, 260.0 + 0.4 * 380.0),
        radii: (
            0.6 * 380.0 * std::f64::consts::SQRT_2,
            0.6 * 380.0 * std::f64::consts::SQRT_2,
        ),
        colour: hex(0xF5B44A, 0.55),
    },
    Glow {
        at: (2300.0 + 150.0, 1080.0 + 150.0),
        radii: (
            150.0 * std::f64::consts::SQRT_2,
            150.0 * std::f64::consts::SQRT_2,
        ),
        colour: hex(0x7EE0A0, 0.45),
    },
];

/// `linear-gradient(160deg, #10172a 0%, #0b1020 55%, #090c16 100%)`.
const STOPS: [(f64, Rgba); 3] = [
    (0.0, hex(0x10172A, 1.0)),
    (0.55, hex(0x0B1020, 1.0)),
    (1.0, hex(0x090C16, 1.0)),
];

fn mix(a: Rgba, b: Rgba, t: f64) -> Rgba {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
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
fn colour_at(x: f64, y: f64) -> Rgba {
    // CSS's gradient line for 160°: through the centre, long enough that
    // the corners get the first and last colours.
    let angle = 160f64.to_radians();
    let (dx, dy) = (angle.sin(), -angle.cos());
    let length = (BOARD.0 * dx).abs() + (BOARD.1 * dy).abs();
    let t = (((x - BOARD.0 / 2.0) * dx + (y - BOARD.1 / 2.0) * dy) / length + 0.5).clamp(0.0, 1.0);
    let mut colour = STOPS[0].1;
    for pair in STOPS.windows(2) {
        let ((a, ca), (b, cb)) = (pair[0], pair[1]);
        if t >= a {
            colour = mix(ca, cb, ((t - a) / (b - a)).clamp(0.0, 1.0));
        }
    }
    for glow in &GLOWS {
        let (rx, ry) = glow.radii;
        let d = (((x - glow.at.0) / rx).powi(2) + ((y - glow.at.1) / ry).powi(2)).sqrt();
        let fade = (d / 0.7).min(1.0);
        let mut top = glow.colour;
        top[3] *= 1.0 - fade;
        colour = over(colour, top);
    }
    colour
}

/// The background of a screen `size` physical pixels large: a picture to
/// draw stretched over it.
pub fn picture(size: (i32, i32)) -> MemoryRenderBuffer {
    let (w, h) = ((size.0 / SHRINK).max(1), (size.1 / SHRINK).max(1));
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    // Dithered by a pixel's worth of noise: dark gradients band otherwise.
    let mut noise: u32 = 0x9E37_79B9;
    for row in 0..h {
        for col in 0..w {
            let x = (f64::from(col) + 0.5) / f64::from(w) * BOARD.0;
            let y = (f64::from(row) + 0.5) / f64::from(h) * BOARD.1;
            let c = colour_at(x, y);
            noise ^= noise << 13;
            noise ^= noise >> 17;
            noise ^= noise << 5;
            let dither = f64::from(noise & 0xff) / 255.0 - 0.5;
            let byte = |v: f64| (v * 255.0 + dither).round().clamp(0.0, 255.0) as u8;
            // ARGB8888, little-endian: B, G, R, A.
            pixels.extend_from_slice(&[byte(c[2]), byte(c[1]), byte(c[0]), 255]);
        }
    }
    MemoryRenderBuffer::from_slice(
        &pixels,
        Fourcc::Argb8888,
        (w, h),
        1,
        Transform::Normal,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn night_blue_with_glows_behind_the_bar() {
        // Top left: the gradient's first colour, about.
        let corner = colour_at(0.0, 0.0);
        assert!(corner[2] > corner[0] && corner[2] < 0.25, "{corner:?}");
        // The amber light behind the bar is brighter and warmer.
        let amber = colour_at(2392.0, 412.0);
        assert!(amber[0] > corner[0] + 0.2, "{amber:?}");
        assert!(amber[0] > amber[2], "{amber:?}");
    }
}
