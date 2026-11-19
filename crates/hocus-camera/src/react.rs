//! Réactions (lot 4): what the hands say and the agents' moments, drawn
//! in the picture, as the mockup's preview draws them (its 312 × 176
//! scaled to the frame; over the crop of Cadrage, under the banner).
//!
//! - **Gestes** ([`Gestures`]): a thumb up held a moment sends a rising
//!   reaction once (another needs the thumb down a second first): an
//!   amber disc `rgba(245,180,74,0.95)`, 42 px, its thumb `#1B1408`,
//!   28 px from the right and 54 from the top, popping in, rising 26 px
//!   and fading over 1.8 s (the mockup's `pop`). A hand raised a moment
//!   shows « main levée » (11 px bold `#1B1408` on the same amber, 8 px
//!   round, 3 px by 8 in, 8 px from the top right corner) while it stays
//!   up, and goes once it has been down a little.
//! - **Fêtes** ([`Confetti`]): on each of the agents' moments, a shower of
//!   the mockup's pieces (6 × 9 px, `#F5B44A`, `#7EE0A0`, `#8FB3FF`,
//!   `#F2945B`) falling through the picture and turning, 2.2 to 3.1 s
//!   each, started over a second and a half.

use std::time::{Duration, Instant};

use resvg::{tiny_skia, usvg};

use crate::{
    banner,
    hands::Gesture,
    look::{H, W},
    paint,
};

/// The mockup's preview's height: its measures are scaled to the frame's.
const PREVIEW: f32 = 176.0;
const AMBER: [u8; 4] = [245, 180, 74, 242];
const INK: [u8; 3] = [0x1B, 0x14, 0x08];

/// A thumb up seen this long sends its reaction.
pub const THUMB_HOLD: Duration = Duration::from_millis(200);
/// Another needs it down this long first.
pub const THUMB_AGAIN: Duration = Duration::from_secs(1);
/// A hand up this long shows « main levée »; down this long, it goes.
pub const RAISED_HOLD: Duration = Duration::from_millis(300);
pub const RAISED_DOWN: Duration = Duration::from_millis(600);
/// The reaction's length.
pub const POP: Duration = Duration::from_millis(1800);

fn k() -> f32 {
    H as f32 / PREVIEW
}

/// What the hands said lately.
#[derive(Debug, Default)]
pub struct Gestures {
    thumb_since: Option<Instant>,
    thumb_gone: Option<Instant>,
    /// A thumb's reaction was sent and the thumb is not down yet.
    thumb_sent: bool,
    raised_since: Option<Instant>,
    lowered_since: Option<Instant>,
    /// « main levée » shows.
    pub raised: bool,
}

impl Gestures {
    /// The hands looked at, at `now`, making `seen`: whether a thumb's
    /// reaction goes now.
    pub fn seen(&mut self, seen: &[Gesture], now: Instant) -> bool {
        let thumb = seen.contains(&Gesture::ThumbUp);
        let mut send = false;
        if thumb {
            self.thumb_gone = None;
            let since = *self.thumb_since.get_or_insert(now);
            if !self.thumb_sent && now.duration_since(since) >= THUMB_HOLD {
                self.thumb_sent = true;
                send = true;
            }
        } else {
            self.thumb_since = None;
            let gone = *self.thumb_gone.get_or_insert(now);
            if now.duration_since(gone) >= THUMB_AGAIN {
                self.thumb_sent = false;
            }
        }
        if seen.contains(&Gesture::Raised) {
            self.lowered_since = None;
            let since = *self.raised_since.get_or_insert(now);
            if now.duration_since(since) >= RAISED_HOLD {
                self.raised = true;
            }
        } else {
            self.raised_since = None;
            let down = *self.lowered_since.get_or_insert(now);
            if now.duration_since(down) >= RAISED_DOWN {
                self.raised = false;
            }
        }
        send
    }

    /// Nothing seen any more (Gestes off, the camera let go).
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// The thumb's drawing, in its 42 px disc.
const THUMB: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="42" height="42">
<circle cx="21" cy="21" r="21" fill="#F5B44A" fill-opacity="0.95"/>
<g transform="translate(10 10) scale(0.9167)"><path d="M7 11v9H4v-9h3zm2 9V11l4-7c1.5 0 2.5 1 2 3l-1 3h5c1.2 0 2 1 1.8 2.2l-1.4 6.6c-.2 1-1 1.2-1.9 1.2H9z" fill="#1B1408"/></g>
</svg>"##;

/// CSS's `ease-out`, `cubic-bezier(0, 0, 0.58, 1)`.
fn ease_out(x: f32) -> f32 {
    // The curve's x for a parameter, solved for `x` by bisection.
    let (x1, x2) = (0.0f32, 0.58f32);
    let bez = |t: f32, a: f32, b: f32| {
        3.0 * (1.0 - t).powi(2) * t * a + 3.0 * (1.0 - t) * t * t * b + t * t * t
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..20 {
        let mid = (lo + hi) / 2.0;
        if bez(mid, x1, x2) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez((lo + hi) / 2.0, 0.0, 1.0)
}

/// The mockup's `pop` at `p` (0 to 1): opacity, rise (preview px, down
/// positive) and scale, between its keyframes.
fn pop(p: f32) -> (f32, f32, f32) {
    const KEYS: [(f32, f32, f32, f32); 4] = [
        (0.0, 0.0, 10.0, 0.6),
        (0.2, 1.0, 0.0, 1.1),
        (0.8, 1.0, -14.0, 1.0),
        (1.0, 0.0, -26.0, 0.9),
    ];
    let p = p.clamp(0.0, 1.0);
    let mut out = (0.0, -26.0, 0.9);
    for pair in KEYS.windows(2) {
        let (Some(&(a, oa, ya, sa)), Some(&(b, ob, yb, sb))) = (pair.first(), pair.get(1)) else {
            continue;
        };
        if p >= a && p <= b {
            let t = ease_out((p - a) / (b - a));
            out = (oa + (ob - oa) * t, ya + (yb - ya) * t, sa + (sb - sa) * t);
            break;
        }
    }
    out
}

/// The thumb's reaction `since` long after it was sent, over `frame`.
pub fn thumb(frame: &mut [u8], since: Duration) {
    static TREE: std::sync::OnceLock<Option<usvg::Tree>> = std::sync::OnceLock::new();
    let Some(tree) = TREE.get_or_init(|| paint::tree(THUMB)) else {
        return;
    };
    let (opacity, rise, scale) = pop(since.as_secs_f32() / POP.as_secs_f32());
    if opacity <= 0.0 {
        return;
    }
    let k = k();
    // Its centre: 28 px from the right, 54 from the top, half its 42.
    let (cx, cy) = (W as f32 - (28.0 + 21.0) * k, (54.0 + 21.0 + rise) * k);
    let s = k * scale;
    let transform = tiny_skia::Transform::from_translate(-21.0, -21.0)
        .post_scale(s, s)
        .post_translate(cx, cy);
    paint::svg_over(frame, tree, transform, opacity);
}

/// « main levée »'s label, drawn once for its words.
#[derive(Debug, Clone)]
pub struct Label {
    pub said: String,
    pixmap: Option<tiny_skia::Pixmap>,
}

impl Label {
    pub fn new(said: &str) -> Self {
        let k = k();
        let text = banner::line(said, 11.0 * k, true, W as f32 / 2.0);
        let pixmap = text.and_then(|text| {
            let (pad_x, pad_y) = (8.0 * k, 3.0 * k);
            let (w, h) = (
                text.width as f32 + 2.0 * pad_x,
                text.height as f32 + 2.0 * pad_y,
            );
            let mut pixmap = tiny_skia::Pixmap::new(w.ceil() as u32, h.ceil() as u32)?;
            let rect = tiny_skia::Rect::from_xywh(0.0, 0.0, w, h)?;
            let path = rounded(rect, 8.0 * k)?;
            let mut paint = tiny_skia::Paint::default();
            let [r, g, b, a] = AMBER;
            paint.set_color_rgba8(r, g, b, a);
            paint.anti_alias = true;
            pixmap.fill_path(
                &path,
                &paint,
                tiny_skia::FillRule::Winding,
                tiny_skia::Transform::identity(),
                None,
            );
            // The words, ink over the amber.
            let (x0, y0) = (pad_x.round() as usize, pad_y.round() as usize);
            let pw = pixmap.width() as usize;
            let data = pixmap.data_mut();
            for (i, &c) in text.cover.iter().enumerate() {
                if c <= 0.0 {
                    continue;
                }
                let (x, y) = (x0 + i % text.width, y0 + i / text.width);
                let at = (y * pw + x) * 4;
                if let Some(px) = data.get_mut(at..at + 4) {
                    for (ch, ink) in px.iter_mut().zip(INK) {
                        *ch = (f32::from(*ch) * (1.0 - c) + f32::from(ink) * c).round() as u8;
                    }
                }
            }
            Some(pixmap)
        });
        Self {
            said: said.to_owned(),
            pixmap,
        }
    }

    /// Over `frame`, 8 px from its top right corner.
    pub fn over(&self, frame: &mut [u8]) {
        let Some(pixmap) = &self.pixmap else {
            return;
        };
        let k = k();
        let x = W as f32 - 8.0 * k - pixmap.width() as f32;
        paint::over(
            frame,
            pixmap,
            x.round() as i32,
            (8.0 * k).round() as i32,
            1.0,
        );
    }
}

fn rounded(r: tiny_skia::Rect, radius: f32) -> Option<tiny_skia::Path> {
    let radius = radius.min(r.width() / 2.0).min(r.height() / 2.0);
    let (x0, y0, x1, y1) = (r.left(), r.top(), r.right(), r.bottom());
    let mut pb = tiny_skia::PathBuilder::new();
    pb.move_to(x0 + radius, y0);
    pb.line_to(x1 - radius, y0);
    pb.quad_to(x1, y0, x1, y0 + radius);
    pb.line_to(x1, y1 - radius);
    pb.quad_to(x1, y1, x1 - radius, y1);
    pb.line_to(x0 + radius, y1);
    pb.quad_to(x0, y1, x0, y1 - radius);
    pb.line_to(x0, y0 + radius);
    pb.quad_to(x0, y0, x0 + radius, y0);
    pb.close();
    pb.finish()
}

/// The confetti's colours, the mockup's.
const COLOURS: [[u8; 3]; 4] = [
    [0xF5, 0xB4, 0x4A],
    [0x7E, 0xE0, 0xA0],
    [0x8F, 0xB3, 0xFF],
    [0xF2, 0x94, 0x5B],
];
/// Pieces a moment.
pub const PIECES: usize = 48;
/// They start over this.
const SPREAD: f32 = 1.5;

/// A piece falling.
#[derive(Debug, Clone, Copy)]
struct Piece {
    /// Across the picture, from 0 to 1.
    x: f32,
    /// When it starts, after its moment, and how long it falls (s).
    delay: f32,
    fall: f32,
    /// Its turn's direction, and its colour.
    turn: f32,
    colour: [u8; 3],
    started: Instant,
}

/// The confetti in the picture.
#[derive(Debug, Default)]
pub struct Confetti {
    pieces: Vec<Piece>,
}

impl Confetti {
    /// A moment: a shower, seeded by `seed` (the same moment, the same
    /// shower).
    pub fn burst(&mut self, seed: u64, now: Instant) {
        let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let mut next = || {
            // xorshift64*
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32 / (1u64 << 24) as f32
        };
        for i in 0..PIECES {
            self.pieces.push(Piece {
                x: next(),
                delay: next() * SPREAD,
                fall: 2.2 + next() * 0.9,
                turn: if next() < 0.5 { -1.0 } else { 1.0 },
                colour: COLOURS.get(i % COLOURS.len()).copied().unwrap_or([255; 3]),
                started: now,
            });
        }
    }

    /// Whether some fall still.
    pub fn falling(&self) -> bool {
        !self.pieces.is_empty()
    }

    /// The pieces at `now` over `frame` (B, G, R, x); those fallen go.
    pub fn over(&mut self, frame: &mut [u8], now: Instant) {
        self.pieces
            .retain(|p| now.duration_since(p.started).as_secs_f32() < p.delay + p.fall);
        if self.pieces.is_empty() {
            return;
        }
        let Some(mut pixmap) = tiny_skia::PixmapMut::from_bytes(frame, W as u32, H as u32) else {
            return;
        };
        let k = k();
        let (pw, ph) = (6.0 * k, 9.0 * k);
        let Some(rect) = tiny_skia::Rect::from_xywh(-pw / 2.0, -ph / 2.0, pw, ph) else {
            return;
        };
        let Some(path) = rounded(rect, k) else {
            return;
        };
        for p in &self.pieces {
            let t = now.duration_since(p.started).as_secs_f32() - p.delay;
            if t < 0.0 {
                continue;
            }
            let q = (t / p.fall).clamp(0.0, 1.0);
            // The mockup's `fall`: from 20 px above the picture to 190 px
            // down its 176, turning 260°, at an even pace.
            let y = (-20.0 + 210.0 * q) * k;
            let x = p.x * W as f32;
            let transform =
                tiny_skia::Transform::from_rotate(260.0 * q * p.turn).post_translate(x, y);
            let mut paint = tiny_skia::Paint::default();
            // The frame is B, G, R, x: the colour's red and blue swapped.
            let [r, g, b] = p.colour;
            paint.set_color_rgba8(b, g, r, 255);
            paint.anti_alias = true;
            pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, transform, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A thumb held a moment sends one reaction; another, once it was
    /// down a second. A hand raised a moment shows « main levée », down a
    /// moment it goes.
    #[test]
    fn the_gestures_wait_a_moment_and_send_once() {
        let mut g = Gestures::default();
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        assert!(!g.seen(&[Gesture::ThumbUp], at(0)));
        assert!(g.seen(&[Gesture::ThumbUp], at(250)));
        assert!(!g.seen(&[Gesture::ThumbUp], at(500)));
        assert!(!g.seen(&[], at(600)));
        assert!(
            !g.seen(&[Gesture::ThumbUp], at(900)),
            "not down long enough"
        );
        assert!(!g.seen(&[], at(1000)));
        assert!(!g.seen(&[], at(2100)));
        assert!(!g.seen(&[Gesture::ThumbUp], at(2200)));
        assert!(g.seen(&[Gesture::ThumbUp], at(2450)));

        g.seen(&[Gesture::Raised], at(3000));
        assert!(!g.raised);
        g.seen(&[Gesture::Raised], at(3350));
        assert!(g.raised);
        g.seen(&[], at(3500));
        assert!(g.raised, "down a moment only");
        g.seen(&[], at(4200));
        assert!(!g.raised);
    }

    #[test]
    fn the_pop_rises_and_fades() {
        let (o0, y0, s0) = pop(0.0);
        assert!(o0.abs() < 1e-3 && (y0 - 10.0).abs() < 1e-2 && (s0 - 0.6).abs() < 1e-3);
        let (o, y, s) = pop(0.2);
        assert!((o - 1.0).abs() < 1e-3 && y.abs() < 1e-3 && (s - 1.1).abs() < 1e-3);
        let (o, y, _) = pop(1.0);
        assert!(o.abs() < 1e-3 && (y + 26.0).abs() < 1e-3);
    }

    /// The pieces fall through the picture and are gone after.
    #[test]
    fn the_confetti_fall_and_go() {
        let mut c = Confetti::default();
        let t0 = Instant::now();
        c.burst(1, t0);
        let mut frame = vec![0u8; W * H * 4];
        c.over(&mut frame, t0 + Duration::from_millis(1600));
        assert!(frame.iter().any(|&b| b != 0));
        c.over(&mut frame, t0 + Duration::from_secs(5));
        assert!(!c.falling());
    }
}
