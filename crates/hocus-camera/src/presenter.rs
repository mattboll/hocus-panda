//! Présentateur (lot 3, the board « Présentateur »): the person cut out
//! of the camera's picture, for the compositor to lay over a shared
//! screen's stream like a weather presenter, never on the screen itself.
//!
//! The cut-out ([`cut`]): the person's box in the frame (where they are
//! a quarter opaque or more), the person premultiplied by their alpha,
//! and the board's `drop-shadow(0 6px 18px rgba(0,0,0,0.35))` under them,
//! its lengths the board's for its 262 px tall silhouette, scaled to the
//! person's height. Sent as B, G, R, A premultiplied (DRM's ARGB8888),
//! the frame's own pixels, unscaled: the compositor sizes it.

use crate::{
    backdrop::Alpha,
    look::{H, W},
};

/// The board's silhouette's height, which its shadow's lengths are of.
const BOARD: f32 = 262.0;
const SHADOW_DY: f32 = 6.0;
/// `drop-shadow`'s blur radius is twice its Gaussian's deviation.
const SHADOW_SIGMA: f32 = 9.0;
const SHADOW_ALPHA: f32 = 0.35;
/// A pixel this opaque or more is the person's, for their box.
const SEEN: u8 = 64;

/// The person cut out: B, G, R, A premultiplied, `width` × `height`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cutout {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// The person of `frame` (B, G, R, x), whose alpha is `alpha`, cut out
/// with their shadow; `None` with nobody in the picture.
pub fn cut(frame: &[u8], alpha: &Alpha) -> Option<Cutout> {
    let (mut x0, mut y0, mut x1, mut y1) = (W, H, 0, 0);
    for (y, row) in alpha.values.chunks(W).enumerate() {
        let mut any = false;
        for (x, &a) in row.iter().enumerate() {
            if a >= SEEN {
                x0 = x0.min(x);
                x1 = x1.max(x + 1);
                any = true;
            }
        }
        if any {
            y0 = y0.min(y);
            y1 = y1.max(y + 1);
        }
    }
    if x1 <= x0 || y1 <= y0 || (y1 - y0) < H / 8 {
        return None;
    }
    let k = (y1 - y0) as f32 / BOARD;
    let (dy, sigma) = (SHADOW_DY * k, SHADOW_SIGMA * k);
    let m = (3.0 * sigma).ceil() as usize;
    // The canvas: the box and room for the shadow around it, but under
    // the frame's foot (where the person goes on, out of the picture).
    let bottom = if y1 >= H { 0 } else { m + dy.ceil() as usize };
    let (cx0, cy0) = (x0 as isize - m as isize, y0 as isize - m as isize);
    let (cw, ch) = (x1 - x0 + 2 * m, y1 - y0 + m + bottom);
    // The person's alpha and colours on the canvas.
    let mut a = vec![0f32; cw * ch];
    let mut out = vec![0u8; cw * ch * 4];
    for cy in 0..ch {
        let fy = cy0 + cy as isize;
        if !(0..H as isize).contains(&fy) {
            continue;
        }
        for cx in 0..cw {
            let fx = cx0 + cx as isize;
            if !(0..W as isize).contains(&fx) {
                continue;
            }
            let i = fy as usize * W + fx as usize;
            let al = alpha.values.get(i).copied().unwrap_or(0);
            if let Some(v) = a.get_mut(cy * cw + cx) {
                *v = f32::from(al) / 255.0;
            }
            if let (Some(&[b, g, r, _]), Some(to)) = (
                frame.get(i * 4..i * 4 + 4),
                out.get_mut((cy * cw + cx) * 4..(cy * cw + cx) * 4 + 4),
            ) {
                let p = |c: u8| ((u32::from(c) * u32::from(al) + 127) / 255) as u8;
                to.copy_from_slice(&[p(b), p(g), p(r), al]);
            }
        }
    }
    // The shadow: the alpha blurred (at a quarter of the canvas, three box
    // blurs each way), moved down, under the person.
    let (qw, qh) = (cw.div_ceil(4), ch.div_ceil(4));
    let mut q = vec![0f32; qw * qh];
    for (i, v) in a.iter().enumerate() {
        let (x, y) = (i % cw / 4, i / cw / 4);
        if let Some(s) = q.get_mut(y * qw + x) {
            *s += v / 16.0;
        }
    }
    let r = ((sigma / 4.0) * (12.0f32 / 9.0).sqrt() / 2.0)
        .round()
        .max(1.0) as usize;
    blur_plane(&mut q, qw, qh, r);
    for cy in 0..ch {
        let sy = (cy as f32 - dy).max(0.0) / 4.0;
        for cx in 0..cw {
            let at = cy * cw + cx;
            let shadow = sample(&q, qw, qh, cx as f32 / 4.0, sy) * SHADOW_ALPHA;
            let Some(px) = out.get_mut(at * 4..at * 4 + 4) else {
                continue;
            };
            let Some(a) = px.get_mut(3) else {
                continue;
            };
            let person = f32::from(*a) / 255.0;
            let under = shadow * (1.0 - person);
            *a = ((person + under) * 255.0).round().min(255.0) as u8;
        }
    }
    Some(Cutout {
        width: cw as u32,
        height: ch as u32,
        bgra: out,
    })
}

/// `plane` at (`x`, `y`) between its pixels.
fn sample(plane: &[f32], w: usize, h: usize, x: f32, y: f32) -> f32 {
    let x = (x - 0.5).clamp(0.0, (w - 1) as f32);
    let y = (y - 0.5).clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x as usize, y as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (tx, ty) = (x - x0 as f32, y - y0 as f32);
    let v = |x: usize, y: usize| plane.get(y * w + x).copied().unwrap_or(0.0);
    let top = v(x0, y0) + (v(x1, y0) - v(x0, y0)) * tx;
    let bottom = v(x0, y1) + (v(x1, y1) - v(x0, y1)) * tx;
    top + (bottom - top) * ty
}

/// Three box blurs of `r` each way: a Gaussian's.
fn blur_plane(v: &mut [f32], w: usize, h: usize, r: usize) {
    let mut line = Vec::new();
    for _ in 0..3 {
        for y in 0..h {
            if let Some(row) = v.get_mut(y * w..(y + 1) * w) {
                box_line(row, 1, &mut line, r);
            }
        }
        for x in 0..w {
            if let Some(column) = v.get_mut(x..) {
                box_line(column, w, &mut line, r);
            }
        }
    }
}

fn box_line(v: &mut [f32], step: usize, line: &mut Vec<f32>, r: usize) {
    line.clear();
    line.extend(v.iter().step_by(step).copied());
    let n = line.len() as isize;
    if n == 0 {
        return;
    }
    // Nothing past the ends: the shadow fades out there.
    let get = |i: isize| {
        if (0..n).contains(&i) {
            line.get(i as usize).copied().unwrap_or(0.0)
        } else {
            0.0
        }
    };
    let r = r as isize;
    let k = 1.0 / (2 * r + 1) as f32;
    let mut sum: f32 = (-r..=r).map(get).sum();
    for (i, out) in v.iter_mut().step_by(step).enumerate() {
        *out = sum * k;
        sum += get(i as isize + r + 1) - get(i as isize - r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_person_is_cut_out_with_their_shadow() {
        let mut alpha = Alpha::default();
        // A person from a third across to two thirds, from a quarter down
        // to the frame's foot.
        for y in H / 4..H {
            for x in W / 3..2 * W / 3 {
                alpha.values[y * W + x] = 255;
            }
        }
        let frame = [10u8, 20, 30, 255].repeat(W * H);
        let cut = cut(&frame, &alpha).expect("someone");
        let (w, h) = (cut.width as usize, cut.height as usize);
        assert!(w > W / 3 && h > H * 3 / 4 && h < H, "{w} × {h}");
        // Its middle is the person, opaque; its corner above them nothing.
        let mid = (h * 2 / 3 * w + w / 2) * 4;
        assert_eq!(&cut.bgra[mid..mid + 4], &[10, 20, 30, 255]);
        assert_eq!(cut.bgra[3], 0);
        // Just beside them, the shadow: dark and a little opaque.
        let m = (3.0 * SHADOW_SIGMA * (H - H / 4) as f32 / BOARD).ceil() as usize;
        let beside = (h * 2 / 3 * w + m - 4) * 4;
        let a = cut.bgra[beside + 3];
        assert!(a > 0 && a < 120, "{a}");
        assert_eq!(&cut.bgra[beside..beside + 3], &[0, 0, 0]);
        assert!(super::cut(&frame, &Alpha::default()).is_none());
    }
}
