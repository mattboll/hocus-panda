//! « Je reviens » (lot 3, the board « Je reviens »): when nobody is in
//! the picture for 10 s, or the screen is locked, the call sees a card on
//! a living wallpaper instead of an empty chair: "De retour dans un
//! instant", a sleeping panda, and how long the person has been away.
//! Their return (in the picture again, the screen unlocked) removes it.
//!
//! - **Gone** ([`Watch`]): the person's share of the picture under
//!   [`GONE`] for [`AFTER`]; back once it is over [`BACK`] (a gap between
//!   the two, so that a hand in the picture does not bring the card back
//!   and forth).
//! - **The card** ([`Card`]): the board's, scaled from its 560 × 315
//!   picture to the frame: 320 wide, 16 round, `rgba(14,18,30,0.72)`, a
//!   1 px border `rgba(255,255,255,0.12)`; the panda asleep (64 px, the
//!   board's drawing), a "z" rising from it every 2.6 s; the title in
//!   Manrope bold 17 px `#EEF1F6`, how long in Manrope 12.5 px `#A9B2C2`.
//!   Drawn once a minute (its words), laid over each frame.
//! - **Under it** the living wallpaper (`Scenes`), else the board's own
//!   night: its gradient and two glows drifting (`aur1`, `aur2`).

use std::time::{Duration, Instant};

use hocus_link::camera::AwayWhy;
use hocus_words::{Lang, camera as words};
use resvg::{tiny_skia, usvg};

use crate::{
    banner,
    look::{BYTES, H, W},
};

/// The person's share of the picture under this is nobody.
pub const GONE: f32 = 0.02;
/// Over this, somebody again.
pub const BACK: f32 = 0.05;
/// Nobody for this long, and the card shows.
pub const AFTER: Duration = Duration::from_secs(10);

/// The board's picture, which the card's measures are of.
const BOARD_H: f32 = 315.0;
const CARD_W: f32 = 320.0;
const PAD_X: f32 = 20.0;
const PAD_Y: f32 = 18.0;
const RADIUS: f32 = 16.0;
const PANDA: f32 = 64.0;
const GAP: f32 = 16.0;
const TITLE: f32 = 17.0;
const SINCE: f32 = 12.5;
const LINES_GAP: f32 = 4.0;
const Z_SIZE: f32 = 13.0;
const Z_EVERY: f32 = 2.6;
const INK: [u8; 3] = [0xEE, 0xF1, 0xF6];
const DIM: [u8; 3] = [0xA9, 0xB2, 0xC2];

/// The board's panda asleep.
const PANDA_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 42 42">
<circle cx="10" cy="11" r="6.5" fill="#1A1C22"/><circle cx="32" cy="11" r="6.5" fill="#1A1C22"/>
<ellipse cx="21" cy="23" rx="15" ry="13.5" fill="#F4F1EA"/>
<ellipse cx="15" cy="22" rx="4.3" ry="5.2" fill="#1A1C22" transform="rotate(-18 15 22)"/>
<ellipse cx="27" cy="22" rx="4.3" ry="5.2" fill="#1A1C22" transform="rotate(18 27 22)"/>
<path d="M13 22.5 q2.6 1.8 5.2 0" stroke="#F4F1EA" stroke-width="1.3" fill="none" stroke-linecap="round"/>
<path d="M23.8 22.5 q2.6 1.8 5.2 0" stroke="#F4F1EA" stroke-width="1.3" fill="none" stroke-linecap="round"/>
<ellipse cx="21" cy="29" rx="2.4" ry="1.7" fill="#1A1C22"/>
</svg>"##;

/// Whether the person is there, frame after frame.
#[derive(Debug, Default)]
pub struct Watch {
    /// Since when nobody has been in the picture.
    gone_since: Option<Instant>,
    /// Since when the screen has been locked.
    locked_since: Option<Instant>,
    /// What shows, since when the person was away.
    shown: Option<(AwayWhy, Instant)>,
}

impl Watch {
    /// The person's share of the picture now (`None`: not looked at this
    /// frame), and whether the screen is locked: whether the card shows,
    /// and since when the person has been away.
    pub fn update(
        &mut self,
        cover: Option<f32>,
        locked: bool,
        now: Instant,
    ) -> Option<(AwayWhy, Instant)> {
        match cover {
            Some(c) if c < GONE => {
                self.gone_since.get_or_insert(now);
            }
            Some(c) if c > BACK => self.gone_since = None,
            _ => {}
        }
        self.locked_since = if locked {
            Some(self.locked_since.unwrap_or(now))
        } else {
            None
        };
        let gone = self
            .gone_since
            .filter(|since| now.duration_since(*since) >= AFTER);
        self.shown = match (self.locked_since, gone) {
            (Some(lock), gone) => Some((AwayWhy::Locked, gone.map_or(lock, |g| g.min(lock)))),
            (None, Some(since)) => Some((AwayWhy::Gone, since)),
            (None, None) => None,
        };
        self.shown
    }

    /// Nothing seen any more (the camera let go).
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// How long the person has been away, in `lang`'s words.
pub fn since_words(lang: Lang, away: Duration) -> String {
    let minutes = away.as_secs() / 60;
    if minutes == 0 {
        words::AWAY_A_MOMENT.get(lang).to_owned()
    } else if minutes < 60 {
        words::away_minutes(lang, minutes)
    } else {
        words::away_hours(
            lang,
            words::AwayHours {
                h: minutes / 60,
                mm: format!("{:02}", minutes % 60),
            },
        )
    }
}

/// The card drawn: premultiplied RGBA over the whole frame, and what it
/// says.
#[derive(Debug, Clone)]
pub struct Card {
    pub said: (String, String),
    pub pixels: Vec<u8>,
    /// Where the "z" rises from: its box's left and top at rest (frame
    /// pixels), and the scale.
    z_at: (f32, f32),
}

fn k() -> f32 {
    H as f32 / BOARD_H
}

impl Card {
    /// The card saying `title` and `since`.
    pub fn draw(title: &str, since: &str) -> Self {
        let k = k();
        let mut pixmap = tiny_skia::Pixmap::new(W as u32, H as u32);
        // The board's width is its content's, its padding around it.
        let (cw, ch) = ((CARD_W + 2.0 * PAD_X) * k, (2.0 * PAD_Y + PANDA) * k);
        let (x0, y0) = ((W as f32 - cw) / 2.0, (H as f32 - ch) / 2.0);
        if let Some(pixmap) = pixmap.as_mut() {
            let rect = tiny_skia::Rect::from_xywh(x0, y0, cw, ch);
            if let Some(path) = rect.and_then(|r| rounded(r, RADIUS * k)) {
                let mut paint = tiny_skia::Paint::default();
                paint.set_color_rgba8(14, 18, 30, (0.72f32 * 255.0).round() as u8);
                paint.anti_alias = true;
                pixmap.fill_path(
                    &path,
                    &paint,
                    tiny_skia::FillRule::Winding,
                    tiny_skia::Transform::identity(),
                    None,
                );
                paint.set_color_rgba8(255, 255, 255, (0.12f32 * 255.0).round() as u8);
                let stroke = tiny_skia::Stroke {
                    width: k,
                    ..tiny_skia::Stroke::default()
                };
                pixmap.stroke_path(
                    &path,
                    &paint,
                    &stroke,
                    tiny_skia::Transform::identity(),
                    None,
                );
            }
            // The panda asleep.
            let (px, py) = (x0 + PAD_X * k, y0 + PAD_Y * k);
            if let Ok(tree) = usvg::Tree::from_str(PANDA_SVG, &usvg::Options::default()) {
                let s = PANDA * k / 64.0;
                resvg::render(
                    &tree,
                    tiny_skia::Transform::from_row(s, 0.0, 0.0, s, px, py),
                    &mut pixmap.as_mut(),
                );
            }
        }
        let mut pixels = pixmap.map(|p| p.take()).unwrap_or_else(|| vec![0; BYTES]);
        // The words, beside it, centred together in its height.
        let tx = x0 + (PAD_X + PANDA + GAP) * k;
        let room = x0 + cw - PAD_X * k - tx;
        let title_line = banner::line(title, TITLE * k, true, room);
        let since_line = banner::line(since, SINCE * k, false, room);
        let heights = title_line.as_ref().map_or(0, |c| c.height)
            + since_line.as_ref().map_or(0, |c| c.height)
            + (LINES_GAP * k) as usize;
        let mut y = (y0 + (ch - heights as f32) / 2.0).max(0.0) as usize;
        if let Some(c) = &title_line {
            draw_text(&mut pixels, c, tx as usize, y, INK, 1.0);
            y += c.height + (LINES_GAP * k) as usize;
        }
        if let Some(c) = &since_line {
            draw_text(&mut pixels, c, tx as usize, y, DIM, 1.0);
        }
        // The "z": its box's right 4 px past the panda's, its top 6 px
        // above it.
        let z_w = Z_SIZE * k * 0.6;
        let z_at = (x0 + (PAD_X + PANDA + 4.0) * k - z_w, y0 + (PAD_Y - 6.0) * k);
        Self {
            said: (title.to_owned(), since.to_owned()),
            pixels,
            z_at,
        }
    }

    /// The card over `frame` (B, G, R, x), its "z" where it is at `t`
    /// seconds.
    pub fn over(&self, frame: &mut [u8], t: f32) {
        for (to, over) in frame
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(self.pixels.as_chunks::<4>().0)
        {
            let &[r, g, b, a] = over;
            if a == 0 {
                continue;
            }
            let keep = 255 - u32::from(a);
            let mix = |o: u8, under: u8| {
                (u32::from(o) + (u32::from(under) * keep + 127) / 255).min(255) as u8
            };
            to[0] = mix(b, to[0]);
            to[1] = mix(g, to[1]);
            to[2] = mix(r, to[2]);
        }
        // The board's `z`: 0 to 40 % it appears, then fades, rising 8 px
        // right and 14 px up over its 2.6 s, `ease-out` each way.
        let k = k();
        let p = (t / Z_EVERY).fract();
        let ease = |x: f32| 1.0 - (1.0 - x).powi(2);
        let opacity = if p < 0.4 {
            ease(p / 0.4)
        } else {
            1.0 - ease((p - 0.4) / 0.6)
        };
        let moved = ease(p);
        if let Some(z) = banner::line("z", Z_SIZE * k, false, W as f32) {
            let x = self.z_at.0 + 8.0 * k * moved;
            let y = self.z_at.1 - 14.0 * k * moved;
            draw_bgrx(
                frame,
                &z,
                x.max(0.0) as usize,
                y.max(0.0) as usize,
                DIM,
                opacity,
            );
        }
    }
}

fn rounded(r: tiny_skia::Rect, radius: f32) -> Option<tiny_skia::Path> {
    let mut pb = tiny_skia::PathBuilder::new();
    let (x, y, w, h) = (r.x(), r.y(), r.width(), r.height());
    let k = 0.552_284_8 * radius;
    pb.move_to(x + radius, y);
    pb.line_to(x + w - radius, y);
    pb.cubic_to(
        x + w - radius + k,
        y,
        x + w,
        y + radius - k,
        x + w,
        y + radius,
    );
    pb.line_to(x + w, y + h - radius);
    pb.cubic_to(
        x + w,
        y + h - radius + k,
        x + w - radius + k,
        y + h,
        x + w - radius,
        y + h,
    );
    pb.line_to(x + radius, y + h);
    pb.cubic_to(
        x + radius - k,
        y + h,
        x,
        y + h - radius + k,
        x,
        y + h - radius,
    );
    pb.line_to(x, y + radius);
    pb.cubic_to(x, y + radius - k, x + radius - k, y, x + radius, y);
    pb.close();
    pb.finish()
}

/// Text in `ink` over premultiplied RGBA `pixels` (the frame's size).
fn draw_text(pixels: &mut [u8], c: &banner::Coverage, x0: usize, y0: usize, ink: [u8; 3], o: f32) {
    for (y, row) in c.cover.chunks(c.width.max(1)).enumerate() {
        for (x, &a) in row.iter().enumerate() {
            let a = a * o;
            if a <= 0.0 || x0 + x >= W || y0 + y >= H {
                continue;
            }
            let at = ((y0 + y) * W + x0 + x) * 4;
            let Some(px) = pixels.get_mut(at..at + 4) else {
                continue;
            };
            let keep = 1.0 - a;
            for (p, i) in px.iter_mut().zip(ink) {
                *p = (f32::from(i) * a + f32::from(*p) * keep).round() as u8;
            }
            if let Some(alpha) = px.get_mut(3) {
                *alpha = (255.0 * a + f32::from(*alpha) * keep).round() as u8;
            }
        }
    }
}

/// Text in `ink` straight onto a B, G, R, x frame.
fn draw_bgrx(frame: &mut [u8], c: &banner::Coverage, x0: usize, y0: usize, ink: [u8; 3], o: f32) {
    for (y, row) in c.cover.chunks(c.width.max(1)).enumerate() {
        for (x, &a) in row.iter().enumerate() {
            let a = a * o;
            if a <= 0.0 || x0 + x >= W || y0 + y >= H {
                continue;
            }
            let at = ((y0 + y) * W + x0 + x) * 4;
            let Some(px) = frame.get_mut(at..at + 3) else {
                continue;
            };
            let [r, g, b] = ink;
            for (p, i) in px.iter_mut().zip([b, g, r]) {
                *p = (f32::from(i) * a + f32::from(*p) * (1.0 - a)).round() as u8;
            }
        }
    }
}

/// The board's night when no living wallpaper can be drawn: its gradient
/// (`#071022`, `#0B1B33` at 60 %, `#0E2238`) and its two glows drifting
/// (green at the upper left over 9 s, violet at the upper right over
/// 12 s), into `frame` (B, G, R, x) at `t` seconds.
#[expect(
    clippy::indexing_slicing,
    reason = "a colour's three channels, looked at from 0 to 2"
)]
pub fn night(frame: &mut [u8], t: f32) {
    let stops = [
        (0.0, [0x07u8, 0x10, 0x22]),
        (0.6, [0x0B, 0x1B, 0x33]),
        (1.0, [0x0E, 0x22, 0x38]),
    ];
    let wave = |period: f32| ((t / period * std::f32::consts::TAU).cos() * -0.5) + 0.5;
    let (a1, a2) = (wave(9.0), wave(12.0));
    // The glows: their middles and radii (parts of the picture), colours
    // and alphas, as the board's two `radial-gradient(closest-side)`s
    // move (`translate` and `scale` between their keyframes).
    let glows = [
        (
            [0.25 + 0.14 * a1, 0.25 - 0.06 * a1],
            [0.45 * (1.0 + 0.15 * a1), 0.55 * (1.0 + 0.15 * a1)],
            [84.0, 230.0, 170.0],
            0.5,
        ),
        (
            [0.825 - 0.16 * a2, 0.30 - 0.06 * a2],
            [0.425 * (1.1 - 0.15 * a2), 0.5 * (1.1 - 0.15 * a2)],
            [130.0, 120.0, 255.0],
            0.42,
        ),
    ];
    for y in 0..H {
        let v = y as f32 / (H - 1) as f32;
        let (i, j) = if v < 0.6 { (0, 1) } else { (1, 2) };
        let (s0, s1) = (stops[i], stops[j]);
        let k = (v - s0.0) / (s1.0 - s0.0);
        let base: [f32; 3] = std::array::from_fn(|c| {
            f32::from(s0.1[c]) + (f32::from(s1.1[c]) - f32::from(s0.1[c])) * k
        });
        let Some(row) = frame.get_mut(y * W * 4..(y + 1) * W * 4) else {
            continue;
        };
        for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let u = x as f32 / (W - 1) as f32;
            let mut c = base;
            for (at, r, colour, alpha) in &glows {
                let d = (((u - at[0]) / r[0]).powi(2) + ((v - at[1]) / r[1]).powi(2)).sqrt();
                let a = alpha * (1.0 - d).clamp(0.0, 1.0);
                for ch in 0..3 {
                    c[ch] += (colour[ch] - c[ch]) * a;
                }
            }
            *px = [c[2] as u8, c[1] as u8, c[0] as u8, 255];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nobody_for_ten_seconds_then_back() {
        let mut watch = Watch::default();
        let t0 = Instant::now();
        assert_eq!(watch.update(Some(0.4), false, t0), None);
        assert_eq!(
            watch.update(Some(0.0), false, t0 + Duration::from_secs(1)),
            None
        );
        assert_eq!(watch.update(None, false, t0 + Duration::from_secs(9)), None);
        let gone = watch.update(Some(0.01), false, t0 + Duration::from_secs(11));
        assert_eq!(gone, Some((AwayWhy::Gone, t0 + Duration::from_secs(1))));
        // A hand in the picture is not someone back.
        assert!(
            watch
                .update(Some(0.03), false, t0 + Duration::from_secs(12))
                .is_some()
        );
        assert_eq!(
            watch.update(Some(0.3), false, t0 + Duration::from_secs(13)),
            None
        );
    }

    #[test]
    fn the_lock_shows_it_at_once_and_unlocking_removes_it() {
        let mut watch = Watch::default();
        let t0 = Instant::now();
        assert_eq!(
            watch.update(Some(0.4), true, t0),
            Some((AwayWhy::Locked, t0))
        );
        assert_eq!(
            watch.update(Some(0.4), false, t0 + Duration::from_secs(5)),
            None
        );
    }

    #[test]
    fn the_time_away_is_said() {
        assert_eq!(
            since_words(Lang::French, Duration::from_secs(150)),
            "absent depuis 2 min"
        );
        assert_eq!(
            since_words(Lang::French, Duration::from_secs(3900)),
            "absent depuis 1 h 05"
        );
        assert_eq!(
            since_words(Lang::English, Duration::from_secs(20)),
            "away for less than a minute"
        );
    }
}
