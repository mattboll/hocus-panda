//! Fond (lot 3): what is behind the person, in the made frame (B, G, R,
//! x, [`W`] × [`H`]), from the person's mask.
//!
//! - **The person's alpha** ([`Alpha::from_mask`]): the model's mask,
//!   its edge tightened (`smoothstep` from 0.3 to 0.7), scaled to the
//!   frame between its pixels: an edge some five pixels soft, as a lens
//!   would draw it.
//! - **Flou** ([`blur`]): the room blurred at a quarter of its size (a
//!   Gaussian of some 26 pixels of the frame, the mockup's `blur(7px)` on
//!   its 312 px preview), the person left out of what is blurred (each
//!   pixel the mean of the room around it, not of the person), so that
//!   no halo of them is left around them; scaled back between its
//!   pixels.
//! - **A living wallpaper** ([`scene`]): its picture behind the person,
//!   scaled to the frame between its pixels.

use crate::{
    look::{BYTES, H, W},
    scenes::Picture,
    segment::{MASK_H, MASK_W, Mask},
};

/// The blur's picture: a quarter of the frame each way.
const SW: usize = W / 4;
const SH: usize = H / 4;
/// Its box blurs' radius (quarter pixels), three passes each way: a
/// Gaussian's of some 6.5 quarter pixels.
const RADIUS: usize = 4;

/// The person's opacity at each pixel of the frame, 0 to 255.
#[derive(Debug, Clone)]
pub struct Alpha {
    pub values: Vec<u8>,
}

impl Default for Alpha {
    fn default() -> Self {
        Self {
            values: vec![0; W * H],
        }
    }
}

/// `x` from `a` to `b`, smoothed, 0 to 1.
fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// For each of `to` places, the two of `from` it lies between and how far
/// along: pixel centres on pixel centres.
fn spans(from: usize, to: usize) -> Vec<(usize, usize, f32)> {
    (0..to)
        .map(|i| {
            let f =
                ((i as f32 + 0.5) * from as f32 / to as f32 - 0.5).clamp(0.0, (from - 1) as f32);
            let a = f as usize;
            (a, (a + 1).min(from - 1), f - a as f32)
        })
        .collect()
}

impl Alpha {
    /// The mask's person, its edge tightened, at the frame's size.
    pub fn from_mask(&mut self, mask: &Mask) {
        let tight: Vec<f32> = mask
            .values
            .iter()
            .map(|&m| smoothstep(0.3, 0.7, m) * 255.0)
            .collect();
        let xs = spans(MASK_W, W);
        let ys = spans(MASK_H, H);
        let mut row = vec![0f32; W];
        let at = |x: usize, y: usize| tight.get(y * MASK_W + x).copied().unwrap_or(0.0);
        for (y, &(y0, y1, ty)) in ys.iter().enumerate() {
            for (v, &(x0, x1, tx)) in row.iter_mut().zip(&xs) {
                let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
                let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
                *v = top + (bottom - top) * ty;
            }
            if let Some(out) = self.values.get_mut(y * W..(y + 1) * W) {
                for (o, v) in out.iter_mut().zip(&row) {
                    *o = v.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
}

/// `under` (B, G, R, x at `uw` × `uh`, scaled to the frame between its
/// pixels) behind the person of `frame`.
#[expect(
    clippy::indexing_slicing,
    reason = "a colour's three channels, looked at from 0 to 2"
)]
fn behind(frame: &mut [u8], alpha: &Alpha, under: &[[f32; 3]], uw: usize, uh: usize) {
    let xs = spans(uw, W);
    let ys = spans(uh, H);
    let mut row = vec![[0f32; 3]; W];
    let at = |x: usize, y: usize| under.get(y * uw + x).copied().unwrap_or([0.0; 3]);
    for (y, &(y0, y1, ty)) in ys.iter().enumerate() {
        for (v, &(x0, x1, tx)) in row.iter_mut().zip(&xs) {
            let (a, b, c, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
            for ch in 0..3 {
                let top = a[ch] + (b[ch] - a[ch]) * tx;
                let bottom = c[ch] + (d[ch] - c[ch]) * tx;
                v[ch] = top + (bottom - top) * ty;
            }
        }
        let (Some(out), Some(alphas)) = (
            frame.get_mut(y * W * 4..(y + 1) * W * 4),
            alpha.values.get(y * W..(y + 1) * W),
        ) else {
            continue;
        };
        for ((px, bg), &a) in out.as_chunks_mut::<4>().0.iter_mut().zip(&row).zip(alphas) {
            if a == 255 {
                continue;
            }
            let k = f32::from(a) / 255.0;
            for (c, b) in px.iter_mut().take(3).zip(bg) {
                *c = (b + (f32::from(*c) - b) * k).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// Three box blurs of `radius` along each row of `v` (`w` × `h`), then
/// down each column: a Gaussian's.
fn gaussian(v: &mut [[f32; 4]], w: usize, h: usize, radius: usize) {
    let mut line = Vec::new();
    for _ in 0..3 {
        for y in 0..h {
            if let Some(row) = v.get_mut(y * w..(y + 1) * w) {
                box_line(row, 1, &mut line, radius);
            }
        }
        for x in 0..w {
            if let Some(column) = v.get_mut(x..) {
                box_line(column, w, &mut line, radius);
            }
        }
    }
}

/// A box blur of `radius` along every `step`th of `v`, its ends held.
#[expect(
    clippy::indexing_slicing,
    reason = "four channels, looked at from 0 to 3"
)]
fn box_line(v: &mut [[f32; 4]], step: usize, line: &mut Vec<[f32; 4]>, radius: usize) {
    line.clear();
    line.extend(v.iter().step_by(step).copied());
    let n = line.len();
    if n == 0 {
        return;
    }
    let get = |i: isize| {
        line.get(i.clamp(0, n as isize - 1) as usize)
            .copied()
            .unwrap_or([0.0; 4])
    };
    let r = radius as isize;
    let k = 1.0 / (2 * radius + 1) as f32;
    let mut sum = [0f32; 4];
    for i in -r..=r {
        let p = get(i);
        for c in 0..4 {
            sum[c] += p[c];
        }
    }
    for (i, out) in v.iter_mut().step_by(step).enumerate() {
        *out = sum.map(|s| s * k);
        let (add, drop) = (get(i as isize + r + 1), get(i as isize - r));
        for c in 0..4 {
            sum[c] += add[c] - drop[c];
        }
    }
}

/// Flou: the room of `frame` (B, G, R, x) blurred behind the person.
#[expect(
    clippy::indexing_slicing,
    reason = "a colour's channels, looked at from 0 to 3"
)]
pub fn blur(frame: &mut [u8], alpha: &Alpha) {
    if frame.len() < BYTES {
        return;
    }
    // The room at a quarter of its size, each colour weighed by how much of
    // the room it is (the person left out), the weight in the fourth; and
    // the plain picture, for where no room is near.
    let mut room = vec![[0f32; 4]; SW * SH];
    let mut plain = vec![[0f32; 4]; SW * SH];
    for (i, (r, p)) in room.iter_mut().zip(plain.iter_mut()).enumerate() {
        let (sx, sy) = (i % SW * 4, i / SW * 4);
        let (mut acc, mut all) = ([0f32; 4], [0f32; 4]);
        for y in sy..sy + 4 {
            for x in sx..sx + 4 {
                let a = alpha.values.get(y * W + x).copied().unwrap_or(0);
                let k = 1.0 - f32::from(a) / 255.0;
                let at = (y * W + x) * 4;
                if let Some(&[b, g, r, _]) = frame.get(at..at + 4) {
                    let c = [f32::from(b), f32::from(g), f32::from(r)];
                    for ch in 0..3 {
                        acc[ch] += c[ch] * k;
                        all[ch] += c[ch];
                    }
                    acc[3] += k;
                    all[3] += 1.0;
                }
            }
        }
        *r = acc;
        *p = all;
    }
    gaussian(&mut room, SW, SH, RADIUS);
    gaussian(&mut plain, SW, SH, RADIUS);
    let under: Vec<[f32; 3]> = room
        .iter()
        .zip(&plain)
        .map(|(r, p)| {
            let (v, w) = if r[3] > 0.05 { (r, r[3]) } else { (p, p[3]) };
            let w = w.max(1e-3);
            [v[0] / w, v[1] / w, v[2] / w]
        })
        .collect();
    behind(frame, alpha, &under, SW, SH);
}

/// A living wallpaper's picture (RGBA) behind the person of `frame`.
pub fn scene(frame: &mut [u8], alpha: &Alpha, picture: &Picture) {
    let under: Vec<[f32; 3]> = picture
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&[r, g, b, _]| [f32::from(b), f32::from(g), f32::from(r)])
        .collect();
    behind(frame, alpha, &under, picture.width, picture.height);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segment::{Segmenter, test_person};

    /// The test picture made B, G, R, x, its person's alpha.
    fn person() -> (Vec<u8>, Alpha) {
        let rgba = test_person();
        let frame: Vec<u8> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, _]| [b, g, r, 255])
            .collect();
        let mut mask = Mask::default();
        Segmenter::new()
            .and_then(|mut s| s.run(&rgba, W, H, [0, 1, 2], &mut mask))
            .expect("the model");
        let mut alpha = Alpha::default();
        alpha.from_mask(&mask);
        (frame, alpha)
    }

    /// How far `a` and `b` are apart where `pick` says, on average (of
    /// 255).
    fn apart(a: &[u8], b: &[u8], alpha: &Alpha, pick: impl Fn(u8) -> bool) -> f64 {
        let mut sum = 0.0f64;
        let mut n = 0.0f64;
        for (i, &al) in alpha.values.iter().enumerate() {
            if pick(al) {
                for c in 0..3 {
                    sum += f64::from(a[i * 4 + c].abs_diff(b[i * 4 + c]));
                }
                n += 3.0;
            }
        }
        sum / n.max(1.0)
    }

    /// The edge of a sharp room is gone, the person is as they were.
    #[test]
    fn flou_blurs_the_room_and_not_the_person() {
        let (frame, alpha) = person();
        let mut blurred = frame.clone();
        blur(&mut blurred, &alpha);
        let person = apart(&frame, &blurred, &alpha, |a| a == 255);
        let room = apart(&frame, &blurred, &alpha, |a| a == 0);
        assert!(person < 0.01, "the person changed by {person}");
        assert!(room > 4.0, "the room changed by {room} only");
        // The room's detail is gone: its pixels next to each other differ
        // far less than they did.
        let detail = |f: &[u8]| {
            let mut d = 0u64;
            for y in 0..H {
                for x in 0..W - 1 {
                    if alpha.values[y * W + x] == 0 && alpha.values[y * W + x + 1] == 0 {
                        d += u64::from(f[(y * W + x) * 4].abs_diff(f[(y * W + x + 1) * 4]));
                    }
                }
            }
            d
        };
        assert!(detail(&blurred) * 3 < detail(&frame));
    }

    #[test]
    fn a_wallpaper_takes_the_room_s_place_and_not_the_person_s() {
        let (frame, alpha) = person();
        let picture = Picture {
            scene: hocus_link::camera::Scene::Aurora,
            width: W / 2,
            height: H / 2,
            rgba: [10, 200, 120, 255].repeat(W / 2 * H / 2),
        };
        let mut made = frame.clone();
        scene(&mut made, &alpha, &picture);
        assert!(apart(&frame, &made, &alpha, |a| a == 255) < 0.01);
        // The room is the wallpaper's green (B, G, R).
        let i = alpha
            .values
            .iter()
            .position(|&a| a == 0)
            .expect("some room");
        assert_eq!(&made[i * 4..i * 4 + 3], &[120, 200, 10]);
    }

    /// The pull request's pictures of lot 3, the camera's frames made from
    /// the test picture of a person: as it is, Flou, the aurora and the
    /// koi pond behind them, « Je reviens »'s card on the aurora and on the
    /// board's night. Written to `$HOCUS_SHOTS` when it names a folder, on
    /// lavapipe. Run by hand (`--run-ignored only`).
    #[test]
    #[ignore = "writes the pull request's screenshots"]
    fn screenshots_of_the_camera_s_pictures() {
        let Ok(dir) = std::env::var("HOCUS_SHOTS") else {
            return;
        };
        if !crate::scenes::tests::on_lavapipe(
            "backdrop::tests::screenshots_of_the_camera_s_pictures",
        ) {
            return;
        }
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("the folder");
        let save = |name: &str, bgrx: &[u8]| {
            let rgba: Vec<u8> = bgrx
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|&[b, g, r, _]| [r, g, b, 255])
                .collect();
            image::save_buffer(
                dir.join(name),
                &rgba,
                W as u32,
                H as u32,
                image::ColorType::Rgba8,
            )
            .expect("saved");
        };
        let (frame, alpha) = person();
        save("camera-as-it-is.png", &frame);
        let mut blurred = frame.clone();
        blur(&mut blurred, &alpha);
        save("camera-flou.png", &blurred);
        let mut painter = crate::scenes::Painter::new(true).expect("lavapipe");
        for (scene, name) in [
            (hocus_link::camera::Scene::Aurora, "camera-aurore.png"),
            (hocus_link::camera::Scene::Koi, "camera-koi.png"),
        ] {
            let picture = painter.draw(scene, 3.0).expect("a picture");
            let mut made = frame.clone();
            super::scene(&mut made, &alpha, &picture);
            save(name, &made);
        }
        let card = crate::away::Card::draw(
            hocus_words::camera::BACK_IN_A_MOMENT.get(hocus_words::Lang::French),
            &crate::away::since_words(
                hocus_words::Lang::French,
                std::time::Duration::from_secs(130),
            ),
        );
        let aurora = painter
            .draw(hocus_link::camera::Scene::Aurora, 8.0)
            .expect("a picture");
        let mut away = frame.clone();
        super::scene(&mut away, &Alpha::default(), &aurora);
        card.over(&mut away, 1.0);
        save("camera-je-reviens.png", &away);
        let mut night = frame.clone();
        crate::away::night(&mut night, 2.0);
        card.over(&mut night, 1.0);
        save("camera-je-reviens-sans-gpu.png", &night);
    }
}
