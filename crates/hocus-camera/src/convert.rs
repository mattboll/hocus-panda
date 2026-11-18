//! A camera's pixels into the frames' RGBA, 1280 × 720, and the frames'
//! B, G, R, x into what a reader asked for.
//!
//! A camera of another size is scaled to cover the frame, its middle kept
//! (a 4:3 camera loses a little of its top and bottom), each pixel the
//! nearest of the camera's. YUV is BT.601's, limited range, as webcams
//! send it.

use crate::look::{BYTES, H, W};

/// The pixels a camera sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pixels {
    Yuy2,
    Nv12,
    I420,
    Bgrx,
    Rgbx,
    Rgb,
    Bgr,
    /// A JPEG each frame.
    Mjpg,
}

/// A camera's frames: their pixels, their size, their rows' bytes (0: as
/// packed as can be).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Input {
    pub pixels: Pixels,
    pub size: (u32, u32),
    pub stride: u32,
}

/// Where each of the frame's columns and rows comes from in a picture
/// `size` large.
#[derive(Debug, Clone, Default)]
pub struct Map {
    size: (u32, u32),
    xs: Vec<usize>,
    ys: Vec<usize>,
}

impl Map {
    fn for_size(&mut self, size: (u32, u32)) -> &Self {
        if self.size != size || self.xs.len() != W {
            let (sw, sh) = (size.0.max(1) as f32, size.1.max(1) as f32);
            let scale = (W as f32 / sw).max(H as f32 / sh);
            let (x0, y0) = ((sw - W as f32 / scale) / 2.0, (sh - H as f32 / scale) / 2.0);
            let at = |o: usize, start: f32, last: u32| {
                ((start + (o as f32 + 0.5) / scale) as usize).min(last.saturating_sub(1) as usize)
            };
            self.xs = (0..W).map(|x| at(x, x0, size.0)).collect();
            self.ys = (0..H).map(|y| at(y, y0, size.1)).collect();
            self.size = size;
        }
        self
    }
}

fn clamp(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// BT.601, limited range, in integers (of 256ths).
fn yuv(y: u8, u: u8, v: u8) -> [u8; 3] {
    let c = (i32::from(y) - 16).max(0) * 298;
    let d = i32::from(u) - 128;
    let e = i32::from(v) - 128;
    [
        clamp((c + 409 * e + 128) >> 8),
        clamp((c - 100 * d - 208 * e + 128) >> 8),
        clamp((c + 516 * d + 128) >> 8),
    ]
}

/// The converter: what it keeps between frames.
#[derive(Debug, Default)]
pub struct Converter {
    map: Map,
    /// A JPEG's decoded pixels.
    decoded: Vec<u8>,
}

impl Converter {
    /// `data`, a frame of `input`, into RGBA `out` (`W` × `H`): whether it
    /// was read.
    pub fn to_rgba(&mut self, input: &Input, data: &[u8], out: &mut [u8]) -> bool {
        if out.len() < BYTES || input.size.0 == 0 || input.size.1 == 0 {
            return false;
        }
        let (w, h) = (input.size.0 as usize, input.size.1 as usize);
        let packed = match input.pixels {
            Pixels::Yuy2 => w * 2,
            Pixels::Nv12 | Pixels::I420 => w,
            Pixels::Bgrx | Pixels::Rgbx => w * 4,
            Pixels::Rgb | Pixels::Bgr => w * 3,
            Pixels::Mjpg => 0,
        };
        let stride = if input.stride == 0 {
            packed
        } else {
            input.stride as usize
        };
        if input.pixels == Pixels::Mjpg {
            let Ok(picture) = image::load_from_memory_with_format(data, image::ImageFormat::Jpeg)
            else {
                return false;
            };
            let picture = picture.to_rgb8();
            let size = (picture.width(), picture.height());
            self.decoded = picture.into_raw();
            let decoded = std::mem::take(&mut self.decoded);
            let read = self.to_rgba(
                &Input {
                    pixels: Pixels::Rgb,
                    size,
                    stride: size.0 * 3,
                },
                &decoded,
                out,
            );
            self.decoded = decoded;
            return read;
        }
        let need = match input.pixels {
            Pixels::Nv12 | Pixels::I420 => stride * h + stride * h.div_ceil(2),
            _ => stride * h,
        };
        if data.len() < need.min(stride * (h - 1) + packed) {
            return false;
        }
        let map = self.map.for_size(input.size);
        let get = |at: usize| data.get(at).copied().unwrap_or(0);
        for (row, &sy) in out.as_chunks_mut::<{ W * 4 }>().0.iter_mut().zip(&map.ys) {
            for (px, &sx) in row.as_chunks_mut::<4>().0.iter_mut().zip(&map.xs) {
                let rgb = match input.pixels {
                    Pixels::Yuy2 => {
                        let at = sy * stride + (sx & !1) * 2;
                        yuv(
                            get(at + if sx & 1 == 0 { 0 } else { 2 }),
                            get(at + 1),
                            get(at + 3),
                        )
                    }
                    Pixels::Nv12 => {
                        let uv = stride * h + (sy / 2) * stride + (sx & !1);
                        yuv(get(sy * stride + sx), get(uv), get(uv + 1))
                    }
                    Pixels::I420 => {
                        let half = stride / 2;
                        let u = stride * h + (sy / 2) * half + sx / 2;
                        let v = stride * h + half * h.div_ceil(2) + (sy / 2) * half + sx / 2;
                        yuv(get(sy * stride + sx), get(u), get(v))
                    }
                    Pixels::Bgrx => {
                        let at = sy * stride + sx * 4;
                        [get(at + 2), get(at + 1), get(at)]
                    }
                    Pixels::Rgbx => {
                        let at = sy * stride + sx * 4;
                        [get(at), get(at + 1), get(at + 2)]
                    }
                    Pixels::Rgb => {
                        let at = sy * stride + sx * 3;
                        [get(at), get(at + 1), get(at + 2)]
                    }
                    Pixels::Bgr => {
                        let at = sy * stride + sx * 3;
                        [get(at + 2), get(at + 1), get(at)]
                    }
                    Pixels::Mjpg => [0, 0, 0],
                };
                let [r, g, b] = rgb;
                px.copy_from_slice(&[r, g, b, 255]);
            }
        }
        true
    }
}

/// What a reader of the source takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    Bgrx,
    I420,
    Yuy2,
}

impl Output {
    /// A frame's rows' bytes (its first plane's), and all its bytes.
    pub const fn layout(self) -> (usize, usize) {
        match self {
            Self::Bgrx => (W * 4, W * H * 4),
            Self::I420 => (W, W * H * 3 / 2),
            Self::Yuy2 => (W * 2, W * H * 2),
        }
    }
}

/// BT.601's limited range from R, G, B.
fn to_yuv(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let (r, g, b) = (i32::from(r), i32::from(g), i32::from(b));
    let y = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
    let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
    let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
    (clamp(y), clamp(u), clamp(v))
}

/// B, G, R, x `frame` (`W` × `H`) written as `output` into `out`: the
/// bytes written.
pub fn write(frame: &[u8], output: Output, out: &mut [u8]) -> usize {
    let (_, size) = output.layout();
    if frame.len() < BYTES || out.len() < size {
        return 0;
    }
    match output {
        Output::Bgrx => {
            if let (Some(to), Some(from)) = (out.get_mut(..BYTES), frame.get(..BYTES)) {
                to.copy_from_slice(from);
            }
        }
        Output::Yuy2 => {
            for (two, to) in frame
                .as_chunks::<8>()
                .0
                .iter()
                .zip(out.as_chunks_mut::<4>().0.iter_mut())
            {
                let &[b0, g0, r0, _, b1, g1, r1, _] = two;
                let (y0, u0, v0) = to_yuv(r0, g0, b0);
                let (y1, u1, v1) = to_yuv(r1, g1, b1);
                let u = u16::midpoint(u16::from(u0), u16::from(u1)) as u8;
                let v = u16::midpoint(u16::from(v0), u16::from(v1)) as u8;
                to.copy_from_slice(&[y0, u, y1, v]);
            }
        }
        Output::I420 => {
            let (ys, chroma) = out.split_at_mut(W * H);
            let (us, vs) = chroma.split_at_mut(W * H / 4);
            for (px, y) in frame.as_chunks::<4>().0.iter().zip(ys.iter_mut()) {
                let &[b, g, r, _] = px;
                *y = to_yuv(r, g, b).0;
            }
            let rows = frame.as_chunks::<{ W * 4 }>().0.iter().step_by(2);
            for ((row, urow), vrow) in rows
                .zip(us.as_chunks_mut::<{ W / 2 }>().0.iter_mut())
                .zip(vs.as_chunks_mut::<{ W / 2 }>().0.iter_mut())
            {
                for ((two, u), v) in row.as_chunks::<8>().0.iter().zip(urow).zip(vrow) {
                    let &[b, g, r, ..] = two;
                    let (_, cu, cv) = to_yuv(r, g, b);
                    *u = cu;
                    *v = cv;
                }
            }
        }
    }
    size
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yuv_grey_is_grey_and_back() {
        let (y, u, v) = to_yuv(128, 128, 128);
        assert_eq!((u, v), (128, 128));
        let [r, g, b] = yuv(y, u, v);
        assert!(r.abs_diff(128) <= 1 && g.abs_diff(128) <= 1 && b.abs_diff(128) <= 1);
    }

    #[test]
    fn a_4_3_camera_covers_the_frame_its_middle_kept() {
        let (w, h) = (640u32, 480u32);
        let mut data = vec![0u8; (w * h * 4) as usize];
        // A red line across the middle row.
        let row = (h / 2) as usize * w as usize * 4;
        for x in 0..w as usize {
            data[row + x * 4 + 2] = 255;
        }
        let mut out = vec![0; BYTES];
        let read = Converter::default().to_rgba(
            &Input {
                pixels: Pixels::Bgrx,
                size: (w, h),
                stride: 0,
            },
            &data,
            &mut out,
        );
        assert!(read);
        let mid = (H / 2) * W * 4;
        assert_eq!(&out[mid..mid + 4], &[255, 0, 0, 255]);
        assert_eq!(&out[..4], &[0, 0, 0, 255]);
    }
}
