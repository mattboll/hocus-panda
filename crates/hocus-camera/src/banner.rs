//! Bandeau: the banner across the bottom of the picture, the user's name
//! and, under it, their project, as the mockup's (« CAMÉRA ET MICRO »'s
//! preview, 312 × 176 at scale 1) scaled to the frame: 32 px tall, a
//! gradient of `rgba(10,14,24,0.85)` to `0.35` from left to right, 10 px
//! in; the name in Manrope bold 11.5 px `#EEF1F6`, the project in Manrope
//! 10 px `#A9B2C2`, centred together in its height.
//!
//! Drawn once for what it says ([`overlay`]), then laid over each frame.

use std::sync::OnceLock;

use hocus_link::camera::Banner;

use crate::look::{H, Overlay, W};

/// Manrope, the shell's own (SIL Open Font License 1.1, its notice in
/// `crates/hocus-shell/fonts`).
static MANROPE_BOLD: &[u8] = include_bytes!("../../hocus-shell/fonts/Manrope-Bold.ttf");
static MANROPE: &[u8] = include_bytes!("../../hocus-shell/fonts/Manrope-Regular.ttf");

/// The mockup's preview's height: the frame's measures are its, scaled.
const PREVIEW: f32 = 176.0;
const BAND: f32 = 32.0;
const PAD: f32 = 10.0;
const NAME: f32 = 11.5;
const PROJECT: f32 = 10.0;
const SHADE: [u8; 3] = [10, 14, 24];
const SHADE_LEFT: f32 = 0.85;
const SHADE_RIGHT: f32 = 0.35;
const NAME_INK: [u8; 3] = [0xEE, 0xF1, 0xF6];
const PROJECT_INK: [u8; 3] = [0xA9, 0xB2, 0xC2];

fn font(bold: bool) -> Option<&'static fontdue::Font> {
    static FONTS: OnceLock<[Option<fontdue::Font>; 2]> = OnceLock::new();
    let fonts = FONTS.get_or_init(|| {
        [MANROPE, MANROPE_BOLD].map(|bytes| {
            fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                .map_err(|err| tracing::warn!(err, "Manrope does not read"))
                .ok()
        })
    });
    fonts.get(usize::from(bold))?.as_ref()
}

/// A line of text's coverage: its width, its height (the font's line), and
/// each pixel's from 0 to 1.
struct Coverage {
    width: usize,
    height: usize,
    cover: Vec<f32>,
}

/// `text` at `size`, cut with "…" to fit in `room` pixels.
fn line(text: &str, size: f32, bold: bool, room: f32) -> Option<Coverage> {
    let font = font(bold)?;
    let advance = |s: &str| -> f32 {
        let mut last = None;
        let mut pen = 0.0;
        for c in s.chars() {
            if let Some(before) = last {
                pen += font.horizontal_kern(before, c, size).unwrap_or(0.0);
            }
            pen += font.metrics(c, size).advance_width;
            last = Some(c);
        }
        pen
    };
    let mut text = text.to_owned();
    if advance(&text) > room {
        while !text.is_empty() && advance(&format!("{text}…")) > room {
            text.pop();
        }
        text = format!("{}…", text.trim_end());
    }
    let metrics = font.horizontal_line_metrics(size)?;
    let width = advance(&text).ceil().max(1.0) as usize;
    let height = (metrics.ascent - metrics.descent + metrics.line_gap)
        .ceil()
        .max(1.0) as usize;
    let baseline = metrics.ascent + metrics.line_gap / 2.0;
    let mut cover = vec![0.0f32; width * height];
    let mut pen = 0.0f32;
    let mut last = None;
    for c in text.chars() {
        if let Some(before) = last {
            pen += font.horizontal_kern(before, c, size).unwrap_or(0.0);
        }
        let (glyph, bitmap) = font.rasterize(c, size);
        let gx = (pen + glyph.xmin as f32).round() as i64;
        let gy = (baseline - (glyph.height as f32 + glyph.ymin as f32)).round() as i64;
        for (row, run) in bitmap.chunks(glyph.width.max(1)).enumerate() {
            for (col, &a) in run.iter().enumerate() {
                let (x, y) = (gx + col as i64, gy + row as i64);
                if let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y))
                    && x < width
                    && let Some(at) = cover.get_mut(y * width + x)
                {
                    *at = at.max(f32::from(a) / 255.0);
                }
            }
        }
        pen += glyph.advance_width;
        last = Some(c);
    }
    Some(Coverage {
        width,
        height,
        cover,
    })
}

/// The banner saying `banner`, over a frame: premultiplied RGBA from its
/// band's top.
pub fn overlay(banner: &Banner) -> Overlay {
    let k = H as f32 / PREVIEW;
    let band = (BAND * k).round() as usize;
    let top = H - band;
    let mut pixels = vec![0u8; W * band * 4];
    // The band: its shade, left to right.
    for row in pixels.as_chunks_mut::<{ W * 4 }>().0.iter_mut() {
        for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let t = x as f32 / (W - 1) as f32;
            let a = SHADE_LEFT + (SHADE_RIGHT - SHADE_LEFT) * t;
            let [r, g, b] = SHADE.map(|c| (f32::from(c) * a).round() as u8);
            px.copy_from_slice(&[r, g, b, (a * 255.0).round() as u8]);
        }
    }
    let pad = (PAD * k).round();
    let room = W as f32 - 2.0 * pad;
    let name = line(&banner.name, NAME * k, true, room);
    let project = banner
        .project
        .as_deref()
        .and_then(|p| line(p, PROJECT * k, false, room));
    let lines: Vec<(Coverage, [u8; 3])> = name
        .map(|c| (c, NAME_INK))
        .into_iter()
        .chain(project.map(|c| (c, PROJECT_INK)))
        .collect();
    let total: usize = lines.iter().map(|(c, _)| c.height).sum();
    let mut y0 = band.saturating_sub(total) / 2;
    for (coverage, ink) in &lines {
        draw(&mut pixels, band, coverage, pad as usize, y0, *ink);
        y0 += coverage.height;
    }
    Overlay { top, pixels }
}

/// `coverage` in `ink` over the band's premultiplied `pixels`, its top left
/// at (`x0`, `y0`).
fn draw(pixels: &mut [u8], band: usize, coverage: &Coverage, x0: usize, y0: usize, ink: [u8; 3]) {
    for (y, row) in coverage.cover.chunks(coverage.width.max(1)).enumerate() {
        if y0 + y >= band {
            break;
        }
        for (x, &a) in row.iter().enumerate() {
            if a <= 0.0 || x0 + x >= W {
                continue;
            }
            let at = ((y0 + y) * W + x0 + x) * 4;
            let Some(px) = pixels.get_mut(at..at + 4) else {
                continue;
            };
            let keep = 1.0 - a;
            for (c, ink) in px.iter_mut().zip(ink) {
                *c = (f32::from(ink) * a + f32::from(*c) * keep).round() as u8;
            }
            if let Some(alpha) = px.get_mut(3) {
                *alpha = (255.0 * a + f32::from(*alpha) * keep).round() as u8;
            }
        }
    }
}

/// The user's name, as their account's entry says it: the first part of
/// its GECOS field ("Alex Martin,,," gives "Alex Martin"), else their
/// login (from the system's databases, else `/etc/passwd`, else `USER`).
/// `HOCUS_USER_NAME` names them instead (tests), as for the shell.
pub fn account_name() -> String {
    if let Some(name) = std::env::var("HOCUS_USER_NAME")
        .ok()
        .filter(|n| !n.trim().is_empty())
    {
        return name.trim().to_owned();
    }
    let uid = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|l| l.strip_prefix("Uid:"))
                .and_then(|ids| ids.split_whitespace().next())
                .and_then(|id| id.parse::<u32>().ok())
        });
    let found = uid.and_then(|uid| {
        let asked = std::process::Command::new("getent")
            .args(["passwd", &uid.to_string()])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned());
        asked
            .into_iter()
            .chain(std::fs::read_to_string("/etc/passwd").ok())
            .find_map(|text| text.lines().find_map(|line| name_in(line, uid)))
    });
    found
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_default()
}

/// The name an entry (`login:x:uid:gid:gecos:home:shell`) gives, if it is
/// account `uid`'s.
fn name_in(line: &str, uid: u32) -> Option<String> {
    let fields: Vec<&str> = line.split(':').collect();
    let [login, _, id, _, gecos, ..] = fields.as_slice() else {
        return None;
    };
    if id.parse::<u32>().ok()? != uid {
        return None;
    }
    let full = gecos.split(',').next().unwrap_or_default().trim();
    let name = if full.is_empty() { login.trim() } else { full };
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_banner_is_the_bottom_with_its_words_lit() {
        let o = overlay(&Banner {
            name: "Alex Martin".to_owned(),
            project: Some("projet hocus-panda".to_owned()),
        });
        assert_eq!(o.top, H - 131);
        assert_eq!(o.pixels.len(), W * 131 * 4);
        // The left of the band, shaded at 0.85; somewhere a word is lit.
        assert_eq!(o.pixels[3], 217);
        assert!(
            o.pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[0] > 200 && p[3] > 240)
        );
    }
}
