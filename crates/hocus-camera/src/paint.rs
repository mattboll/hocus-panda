//! Drawing over the made frame (B, G, R, x, [`W`] × [`H`]): a picture
//! drawn apart (tiny-skia's premultiplied RGBA) laid over it where it
//! goes. What lot 4 draws in the picture (Visage's accessories, the
//! reactions, the confetti) goes through here.

use resvg::{tiny_skia, usvg};

use crate::look::{H, W};

/// `pixmap` over `frame` (B, G, R, x), its top left at (`x`, `y`) (it
/// may lie partly out of the frame), at `opacity`.
pub fn over(frame: &mut [u8], pixmap: &tiny_skia::Pixmap, x: i32, y: i32, opacity: f32) {
    let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
    let (x0, y0) = (x.max(0), y.max(0));
    let (x1, y1) = ((x + pw).min(W as i32), (y + ph).min(H as i32));
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let k = (opacity.clamp(0.0, 1.0) * 256.0) as u32;
    let data = pixmap.data();
    let run = (x1 - x0) as usize;
    for fy in y0..y1 {
        let from = (((fy - y) * pw + (x0 - x)) * 4) as usize;
        let to = ((fy as usize) * W + x0 as usize) * 4;
        let (Some(src), Some(dst)) = (
            data.get(from..from + run * 4),
            frame.get_mut(to..to + run * 4),
        ) else {
            continue;
        };
        for (s, d) in src
            .as_chunks::<4>()
            .0
            .iter()
            .zip(dst.as_chunks_mut::<4>().0)
        {
            let &[r, g, b, a] = s;
            if a == 0 {
                continue;
            }
            let scaled = |v: u8| (u32::from(v) * k) >> 8;
            let (r, g, b, a) = (scaled(r), scaled(g), scaled(b), scaled(a));
            let keep = 255 - a.min(255);
            let mix =
                |o: u32, under: u8| (o + (u32::from(under) * keep + 127) / 255).min(255) as u8;
            d[0] = mix(b, d[0]);
            d[1] = mix(g, d[1]);
            d[2] = mix(r, d[2]);
        }
    }
}

/// An SVG read once: what is drawn is its tree.
pub fn tree(svg: &str) -> Option<usvg::Tree> {
    usvg::Tree::from_str(svg, &usvg::Options::default())
        .map_err(|err| tracing::warn!(%err, "a drawing does not read"))
        .ok()
}

/// `tree` drawn by `transform` (its units to the frame's pixels) over
/// `frame`, at `opacity`: drawn apart in its box, then laid over.
pub fn svg_over(
    frame: &mut [u8],
    tree: &usvg::Tree,
    transform: tiny_skia::Transform,
    opacity: f32,
) {
    let size = tree.size();
    let Some(rect) = tiny_skia::Rect::from_xywh(0.0, 0.0, size.width(), size.height())
        .and_then(|r| r.transform(transform))
    else {
        return;
    };
    // Its box in the frame, a pixel round it for the edges.
    let x0 = (rect.left().floor() as i32 - 1).max(-(W as i32));
    let y0 = (rect.top().floor() as i32 - 1).max(-(H as i32));
    let x1 = (rect.right().ceil() as i32 + 1).min(2 * W as i32);
    let y1 = (rect.bottom().ceil() as i32 + 1).min(2 * H as i32);
    if x1 <= x0 || y1 <= y0 || x1 <= 0 || y1 <= 0 || x0 >= W as i32 || y0 >= H as i32 {
        return;
    }
    let Some(mut pixmap) = tiny_skia::Pixmap::new((x1 - x0) as u32, (y1 - y0) as u32) else {
        return;
    };
    resvg::render(
        tree,
        transform.post_translate(-x0 as f32, -y0 as f32),
        &mut pixmap.as_mut(),
    );
    over(frame, &pixmap, x0, y0, opacity);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_lies_over_its_place_only() {
        let mut frame = vec![10u8; W * H * 4];
        let mut pixmap = tiny_skia::Pixmap::new(4, 4).expect("a pixmap");
        pixmap.fill(tiny_skia::Color::from_rgba8(255, 0, 0, 255));
        over(&mut frame, &pixmap, -2, 0, 1.0);
        // Red is the frame's third byte.
        assert_eq!(&frame[0..4], &[0, 0, 255, 10]);
        assert_eq!(&frame[8..12], &[10, 10, 10, 10]);
        assert_eq!(&frame[(4 * W) * 4..(4 * W) * 4 + 4], &[10, 10, 10, 10]);
    }
}
