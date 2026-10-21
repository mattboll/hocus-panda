//! RGBA images: PNG in and out, and comparison with a tolerance.

use std::{fs, path::Path};

use anyhow::Context;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Rows of RGBA bytes, top to bottom.
    pub rgba: Vec<u8>,
}

/// How far two screenshots may differ and still match: antialiasing and
/// rounding move a few values by a step or two.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    /// Largest difference on a channel for a pixel to count as equal.
    pub channel: u8,
    /// Share of pixels, 0 to 1, that may differ beyond `channel`.
    pub pixels: f64,
}

impl Default for Tolerance {
    fn default() -> Self {
        Self {
            channel: 2,
            pixels: 0.0,
        }
    }
}

/// How two images differ.
#[derive(Debug, Clone, PartialEq)]
pub enum Difference {
    Size {
        expected: (u32, u32),
        actual: (u32, u32),
    },
    Pixels {
        differing: usize,
        total: usize,
        largest: u8,
        /// The actual image, darkened, with differing pixels in red.
        diff: Image,
    },
}

impl std::fmt::Display for Difference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Size { expected, actual } => write!(
                f,
                "size: expected {}x{}, got {}x{}",
                expected.0, expected.1, actual.0, actual.1
            ),
            Self::Pixels {
                differing,
                total,
                largest,
                ..
            } => write!(
                f,
                "{differing} of {total} pixels differ ({:.3} %), by up to {largest} on a channel",
                *differing as f64 * 100.0 / *total as f64
            ),
        }
    }
}

impl Image {
    pub fn from_png(png: &[u8]) -> anyhow::Result<Self> {
        let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info()?;
        let mut rgba = vec![0; reader.output_buffer_size().context("image too large")?];
        let info = reader.next_frame(&mut rgba)?;
        anyhow::ensure!(
            info.color_type == png::ColorType::Rgba && info.bit_depth == png::BitDepth::Eight,
            "not 8-bit RGBA: {:?} {:?}",
            info.color_type,
            info.bit_depth
        );
        rgba.truncate(info.buffer_size());
        Ok(Self {
            width: info.width,
            height: info.height,
            rgba,
        })
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let png = fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
        Self::from_png(&png).with_context(|| format!("{} is not a usable PNG", path.display()))
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let file = fs::File::create(path)
            .with_context(|| format!("could not create {}", path.display()))?;
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&self.rgba)?;
        writer.finish()?;
        Ok(())
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = ((y * self.width + x) * 4) as usize;
        self.rgba.get(i..i + 4)?.try_into().ok()
    }

    /// `None` when `self` matches `expected` within `tolerance`.
    pub fn compare(&self, expected: &Image, tolerance: Tolerance) -> Option<Difference> {
        if (self.width, self.height) != (expected.width, expected.height) {
            return Some(Difference::Size {
                expected: (expected.width, expected.height),
                actual: (self.width, self.height),
            });
        }
        let mut diff = Vec::with_capacity(self.rgba.len());
        let mut differing = 0;
        let mut largest = 0;
        let pixels = self.rgba.as_chunks::<4>().0;
        for (actual, wanted) in pixels.iter().zip(expected.rgba.as_chunks::<4>().0) {
            let delta = actual
                .iter()
                .zip(wanted)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap_or(0);
            largest = largest.max(delta);
            if delta > tolerance.channel {
                differing += 1;
                diff.extend_from_slice(&[255, 0, 0, 255]);
            } else {
                diff.extend_from_slice(&[actual[0] / 4, actual[1] / 4, actual[2] / 4, 255]);
            }
        }
        let total = pixels.len();
        if differing as f64 <= tolerance.pixels * total as f64 {
            return None;
        }
        Some(Difference::Pixels {
            differing,
            total,
            largest,
            diff: Image {
                width: self.width,
                height: self.height,
                rgba: diff,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Image {
        Image {
            width,
            height,
            rgba: rgba.repeat((width * height) as usize),
        }
    }

    #[test]
    fn small_differences_are_tolerated() {
        let a = solid(4, 4, [10, 20, 30, 255]);
        let b = solid(4, 4, [12, 19, 30, 255]);
        assert_eq!(a.compare(&b, Tolerance::default()), None);
    }

    #[test]
    fn differing_pixels_are_counted_and_shown_in_red() {
        let expected = solid(4, 4, [10, 20, 30, 255]);
        let mut actual = expected.clone();
        actual.rgba[0..4].copy_from_slice(&[200, 20, 30, 255]);

        let Some(Difference::Pixels {
            differing,
            largest,
            diff,
            ..
        }) = actual.compare(&expected, Tolerance::default())
        else {
            panic!("the images should differ");
        };
        assert_eq!((differing, largest), (1, 190));
        assert_eq!(diff.pixel(0, 0), Some([255, 0, 0, 255]));
        assert_eq!(
            actual.compare(
                &expected,
                Tolerance {
                    channel: 2,
                    pixels: 0.1
                }
            ),
            None
        );
    }

    #[test]
    fn sizes_must_match() {
        let difference = solid(4, 4, [0; 4]).compare(&solid(4, 5, [0; 4]), Tolerance::default());
        assert!(matches!(difference, Some(Difference::Size { .. })));
    }

    #[test]
    fn png_round_trip() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("a.png");
        let image = solid(3, 2, [1, 2, 3, 4]);
        image.save(&path)?;
        assert_eq!(Image::load(&path)?, image);
        Ok(())
    }
}
