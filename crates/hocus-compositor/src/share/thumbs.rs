//! The chooser's thumbnails made small, off the loop: a screen or a
//! window drawn at its own resolution comes here, is averaged down to fit
//! its place in the chooser (keeping its proportions), its corners
//! rounded off, and goes back to the loop to be drawn over the chooser.

use std::sync::mpsc;

use hocus_link::ShareSource;
use smithay::reexports::calloop::channel::Sender;

/// A thumbnail to make: `pixels` (premultiplied B, G, R, A rows, `size`
/// pixels), to fit in `room` pixels, its corners `radius` pixels round.
#[derive(Debug)]
pub struct Job {
    pub request: u64,
    pub source: ShareSource,
    pub pixels: Vec<u8>,
    pub size: (u32, u32),
    pub room: (u32, u32),
    pub radius: f64,
}

/// A thumbnail made: its pixels and their size.
#[derive(Debug)]
pub struct Made {
    pub request: u64,
    pub source: ShareSource,
    pub pixels: Vec<u8>,
    pub size: (u32, u32),
}

/// The thread, as the loop holds it.
#[derive(Debug)]
pub struct Maker {
    to: mpsc::Sender<Job>,
}

impl Maker {
    pub fn start(back: Sender<Made>) -> Option<Self> {
        let (to, jobs) = mpsc::channel::<Job>();
        let started = std::thread::Builder::new()
            .name("screen cast thumbnails".to_owned())
            .spawn(move || {
                while let Ok(job) = jobs.recv() {
                    let (pixels, size) = shrink(&job.pixels, job.size, job.room, job.radius);
                    let made = Made {
                        request: job.request,
                        source: job.source,
                        pixels,
                        size,
                    };
                    if back.send(made).is_err() {
                        return;
                    }
                }
            });
        match started {
            Ok(_) => Some(Self { to }),
            Err(err) => {
                tracing::warn!(%err, "no thread for the chooser's thumbnails");
                None
            }
        }
    }

    pub fn make(&self, job: Job) {
        let _ = self.to.send(job);
    }
}

/// The size `size` takes fitted in `room`, its proportions kept.
pub fn fitted(size: (u32, u32), room: (u32, u32)) -> (u32, u32) {
    let (w, h) = (f64::from(size.0.max(1)), f64::from(size.1.max(1)));
    let k = (f64::from(room.0) / w).min(f64::from(room.1) / h).min(1.0);
    (
        ((w * k).round() as u32).clamp(1, room.0.max(1)),
        ((h * k).round() as u32).clamp(1, room.1.max(1)),
    )
}

/// `pixels` (`size`) averaged down to fit `room`, corners `radius` round.
pub fn shrink(
    pixels: &[u8],
    size: (u32, u32),
    room: (u32, u32),
    radius: f64,
) -> (Vec<u8>, (u32, u32)) {
    let (sw, sh) = (size.0 as usize, size.1 as usize);
    let (tw, th) = fitted(size, room);
    let (tw, th) = (tw as usize, th as usize);
    let mut out = vec![0u8; tw * th * 4];
    if sw == 0 || sh == 0 || pixels.len() < sw * sh * 4 {
        return (out, (tw as u32, th as u32));
    }
    for ty in 0..th {
        let y0 = ty * sh / th;
        let y1 = ((ty + 1) * sh / th).max(y0 + 1).min(sh);
        for tx in 0..tw {
            let x0 = tx * sw / tw;
            let x1 = ((tx + 1) * sw / tw).max(x0 + 1).min(sw);
            let mut sum = [0u32; 4];
            for y in y0..y1 {
                let row = pixels
                    .get((y * sw + x0) * 4..(y * sw + x1) * 4)
                    .unwrap_or_default();
                for px in row.as_chunks::<4>().0 {
                    for (sum, value) in sum.iter_mut().zip(px) {
                        *sum += u32::from(*value);
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            let cover = corner_cover(tx, ty, tw, th, radius);
            let at = (ty * tw + tx) * 4;
            let to = out.get_mut(at..at + 4).unwrap_or_default();
            for (to, sum) in to.iter_mut().zip(sum) {
                *to = ((f64::from(sum / n.max(1))) * cover).round() as u8;
            }
        }
    }
    (out, (tw as u32, th as u32))
}

/// How much of pixel (`x`, `y`) of a `w` × `h` image its round corners
/// keep: 1 inside, 0 outside, a pixel's antialiasing between.
fn corner_cover(x: usize, y: usize, w: usize, h: usize, radius: f64) -> f64 {
    let r = radius.min(w as f64 / 2.0).min(h as f64 / 2.0);
    if r <= 0.0 {
        return 1.0;
    }
    let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
    let cx = if px < r {
        r
    } else if px > w as f64 - r {
        w as f64 - r
    } else {
        return 1.0;
    };
    let cy = if py < r {
        r
    } else if py > h as f64 - r {
        h as f64 - r
    } else {
        return 1.0;
    };
    let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
    (r - d + 0.5).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thumbnail_keeps_its_proportions_and_its_colour() {
        assert_eq!(fitted((2560, 1600), (236, 129)), (206, 129));
        assert_eq!(fitted((800, 1200), (236, 129)), (86, 129));
        assert_eq!(fitted((100, 50), (236, 129)), (100, 50));
        let pixels: Vec<u8> = [10u8, 20, 30, 255].repeat(400 * 200);
        let (small, size) = shrink(&pixels, (400, 200), (40, 40), 4.0);
        assert_eq!(size, (40, 20));
        let at = |x: usize, y: usize| &small[(y * 40 + x) * 4..(y * 40 + x) * 4 + 4];
        assert_eq!(at(20, 10), [10, 20, 30, 255]);
        // Its corner cut off, round.
        assert_eq!(at(0, 0), [0, 0, 0, 0]);
    }
}
