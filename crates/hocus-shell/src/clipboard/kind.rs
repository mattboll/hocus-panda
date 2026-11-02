//! What an item of the clipboard's history is, for its row (5.6): a
//! command or code (`>_`), a link (`↗`), a path (`/`), a colour (its
//! swatch), an image (its thumbnail), else text (`Aa`). Told from the text
//! alone, by its look: a guess, which only picks a tile and a font.

use std::sync::Arc;

use iced::advanced::image;

/// An item's kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Code,
    Link,
    Path,
    /// Red, green, blue, alpha.
    Colour([u8; 4]),
    Text,
    Image,
}

impl Kind {
    /// What a text is.
    pub fn of(text: &str) -> Self {
        let line = text.trim();
        if line.is_empty() {
            return Self::Text;
        }
        let one_line = !line.contains('\n');
        if one_line && let Some(colour) = colour(line) {
            return Self::Colour(colour);
        }
        if one_line && !line.contains(char::is_whitespace) && link(line) {
            return Self::Link;
        }
        if one_line && path(line) {
            return Self::Path;
        }
        if code(line) {
            return Self::Code;
        }
        Self::Text
    }

    /// Its tile's glyph, for those drawn with one.
    pub fn glyph(self) -> Option<&'static str> {
        match self {
            Self::Code => Some(">_"),
            Self::Link => Some("↗"),
            Self::Path => Some("/"),
            Self::Text => Some("Aa"),
            Self::Colour(_) | Self::Image => None,
        }
    }

    /// Whether its text is set in the monospaced font.
    pub fn mono(self) -> bool {
        matches!(self, Self::Code | Self::Path | Self::Colour(_))
    }

    /// Its name, for the log.
    pub fn word(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Link => "link",
            Self::Path => "path",
            Self::Colour(_) => "colour",
            Self::Text => "text",
            Self::Image => "image",
        }
    }
}

/// `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`, `rgb(…)` or `rgba(…)`.
fn colour(text: &str) -> Option<[u8; 4]> {
    if let Some(hex) = text.strip_prefix('#') {
        if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let digit = |i: usize| u8::from_str_radix(hex.get(i..=i)?, 16).ok();
        let pair = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        // `#123` is an issue's number more likely than a grey.
        let letters = hex.chars().any(|c| c.is_ascii_alphabetic());
        return match hex.len() {
            3 | 4 if letters => {
                let alpha = if hex.len() == 4 { digit(3)? * 17 } else { 255 };
                Some([digit(0)? * 17, digit(1)? * 17, digit(2)? * 17, alpha])
            }
            6 | 8 => {
                let alpha = if hex.len() == 8 { pair(6)? } else { 255 };
                Some([pair(0)?, pair(2)?, pair(4)?, alpha])
            }
            _ => None,
        };
    }
    let lower = text.to_ascii_lowercase();
    let inside = lower
        .strip_prefix("rgba(")
        .or_else(|| lower.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts: Vec<&str> = inside
        .split([',', '/', ' '])
        .filter(|p| !p.is_empty())
        .collect();
    if !(3..=4).contains(&parts.len()) {
        return None;
    }
    let channel = |p: &str| p.trim().parse::<u8>().ok();
    let alpha = match parts.get(3) {
        Some(a) => {
            let a: f32 = a.trim().parse().ok()?;
            if !(0.0..=1.0).contains(&a) {
                return None;
            }
            (a * 255.0).round() as u8
        }
        None => 255,
    };
    Some([
        channel(parts[0])?,
        channel(parts[1])?,
        channel(parts[2])?,
        alpha,
    ])
}

fn link(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    ["http://", "https://", "ftp://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme) && lower.len() > scheme.len())
}

fn path(text: &str) -> bool {
    let rooted = text.starts_with('/')
        || text.starts_with("~/")
        || text.starts_with("./")
        || text.starts_with("../")
        || text.starts_with("file://");
    // A command's flags make it one.
    rooted && text.len() > 1 && !text.contains(" -") && !text.contains("  ")
}

/// The commands a copied line most often starts with.
const COMMANDS: [&str; 40] = [
    "git",
    "cargo",
    "rustup",
    "ssh",
    "scp",
    "rsync",
    "sudo",
    "cd",
    "ls",
    "cat",
    "grep",
    "rg",
    "find",
    "echo",
    "export",
    "make",
    "cmake",
    "npm",
    "npx",
    "yarn",
    "pnpm",
    "pip",
    "python",
    "python3",
    "docker",
    "podman",
    "kubectl",
    "systemctl",
    "journalctl",
    "apt",
    "dnf",
    "curl",
    "wget",
    "tar",
    "chmod",
    "mkdir",
    "rm",
    "mv",
    "cp",
    "gh",
];

fn code(text: &str) -> bool {
    let first = text.lines().next().unwrap_or_default().trim();
    let word = first
        .trim_start_matches("$ ")
        .split_whitespace()
        .next()
        .unwrap_or_default();
    if first.starts_with("$ ") || first.starts_with("./") || COMMANDS.contains(&word) {
        return true;
    }
    let shell = [" && ", " || ", " | ", "$(", " --", "2>&1"]
        .iter()
        .any(|s| text.contains(s));
    let source = [
        "fn ", "=> ", "::", "();", "{\n", "};", "#include", "def ", "import ",
    ]
    .iter()
    .any(|s| text.contains(s));
    shell || source
}

/// An image's size and its thumbnail, made once as it is copied (or read
/// back): square, its middle, its corners rounded, for the tile.
#[derive(Debug, Clone)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    /// Its thumbnail's side and its pixels (RGBA), as kept on disk.
    pub side: u32,
    pub rgba: Arc<[u8]>,
    pub handle: image::Handle,
}

/// The thumbnail's side, pixels: the tile's 24 px inside at ×3.
pub const THUMB: u32 = 72;
/// Its corners' radius at that size (the mockup's 6 px).
const THUMB_RADIUS: f32 = 18.0;

impl Picture {
    /// A PNG's size and thumbnail; `None` when it does not read.
    pub fn of_png(png: &[u8]) -> Option<Self> {
        let mut reader =
            ::image::ImageReader::with_format(std::io::Cursor::new(png), ::image::ImageFormat::Png);
        let mut limits = ::image::Limits::default();
        limits.max_image_width = Some(16384);
        limits.max_image_height = Some(16384);
        limits.max_alloc = Some(512 << 20);
        reader.limits(limits);
        let decoded = reader
            .decode()
            .inspect_err(|err| tracing::debug!(%err, "a copied image that does not read"))
            .ok()?;
        let (width, height) = (decoded.width(), decoded.height());
        if width == 0 || height == 0 {
            return None;
        }
        // Its middle square, at the thumbnail's size.
        let side = width.min(height);
        let square = decoded.crop_imm((width - side) / 2, (height - side) / 2, side, side);
        let small = square
            .resize_exact(THUMB, THUMB, ::image::imageops::FilterType::Triangle)
            .into_rgba8();
        let mut rgba = small.into_raw();
        round_corners(&mut rgba, THUMB, THUMB_RADIUS);
        Some(Self::of_thumb(width, height, THUMB, rgba.into()))
    }

    /// A picture from what is kept of it.
    pub fn of_thumb(width: u32, height: u32, side: u32, rgba: Arc<[u8]>) -> Self {
        let handle = image::Handle::from_rgba(side, side, rgba.to_vec());
        Self {
            width,
            height,
            side,
            rgba,
            handle,
        }
    }
}

/// `rgba`, `side` pixels square, made transparent past its corners'
/// arcs of `radius`, their edge smoothed.
fn round_corners(rgba: &mut [u8], side: u32, radius: f32) {
    let s = side as f32;
    for y in 0..side {
        for x in 0..side {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let cx = px.clamp(radius, s - radius);
            let cy = py.clamp(radius, s - radius);
            let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
            let cover = (radius + 0.5 - d).clamp(0.0, 1.0);
            if cover < 1.0 {
                let at = ((y * side + x) * 4 + 3) as usize;
                if let Some(alpha) = rgba.get_mut(at) {
                    *alpha = (f32::from(*alpha) * cover).round() as u8;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_copy_has_its_kind() {
        let cases = [
            (
                "cargo nextest run -p hocus-test --test scenarios",
                Kind::Code,
            ),
            (
                "git worktree add -b feat/394-history ../w394 origin/v2",
                Kind::Code,
            ),
            ("ssh alex@192.168.1.24", Kind::Code),
            ("ls -la | wc -l", Kind::Code),
            ("fn main() {\n    println!(\"x\");\n}", Kind::Code),
            (
                "https://github.com/mattboll/hocus-panda/pull/513",
                Kind::Link,
            ),
            ("~/Documents/factures/2026-10-facture.pdf", Kind::Path),
            ("/usr/share/applications", Kind::Path),
            ("#6CC287", Kind::Colour([0x6C, 0xC2, 0x87, 255])),
            ("#fff", Kind::Colour([255, 255, 255, 255])),
            (
                "rgba(108, 194, 135, 0.5)",
                Kind::Colour([108, 194, 135, 128]),
            ),
            ("06 12 34 56 78", Kind::Text),
            ("mattboll@exemple.fr", Kind::Text),
            ("12 rue des Lilas, 35000 Rennes", Kind::Text),
            ("Bonjour Camille, la maquette est validée.", Kind::Text),
            ("#hashtag", Kind::Text),
            ("#513", Kind::Text),
            ("https://", Kind::Text),
        ];
        for (text, kind) in cases {
            assert_eq!(Kind::of(text), kind, "{text}");
        }
        assert_eq!(Kind::Code.glyph(), Some(">_"));
        assert!(Kind::Path.mono() && !Kind::Text.mono() && !Kind::Link.mono());
    }

    #[test]
    fn an_image_has_its_size_and_a_rounded_thumbnail() {
        let mut png = Vec::new();
        let red = ::image::RgbaImage::from_pixel(40, 20, ::image::Rgba([229, 57, 53, 255]));
        red.write_to(
            &mut std::io::Cursor::new(&mut png),
            ::image::ImageFormat::Png,
        )
        .expect("encoded");
        let picture = Picture::of_png(&png).expect("read");
        assert_eq!((picture.width, picture.height), (40, 20));
        assert_eq!(picture.side, THUMB);
        assert_eq!(picture.rgba.len(), (THUMB * THUMB * 4) as usize);
        // Its middle red, its corner gone.
        let at = |x: u32, y: u32| ((y * THUMB + x) * 4) as usize;
        let middle = at(THUMB / 2, THUMB / 2);
        assert_eq!(&picture.rgba[middle..middle + 4], &[229, 57, 53, 255]);
        assert_eq!(picture.rgba[at(0, 0) + 3], 0);
        assert!(Picture::of_png(b"not a png").is_none());
    }
}
