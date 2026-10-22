//! What is drawn over a capture, in its own pixels, and the picture it
//! makes: the same drawing on screen and in the file.

use hocus_ui::{
    Canvas, Color, Draw, Kind, Length, Look, NodeId, Paint, Point, Rect, Size, Style, Surface,
    TextStyle, Theme, Tree,
};

/// The colours offered, the first by default.
pub const COLORS: [Color; 5] = [
    Color::rgb(0xE5, 0x48, 0x4D),
    Color::rgb(0xF5, 0xC4, 0x51),
    Color::rgb(0x7B, 0xC4, 0x7F),
    Color::rgb(0x4C, 0x8D, 0xF6),
    Color::rgb(0xFF, 0xFF, 0xFF),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Arrow,
    Frame,
    Text,
    Blur,
}

/// One mark, in the capture's pixels.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Arrow {
        from: Point,
        to: Point,
        color: usize,
    },
    Frame {
        rect: Rect,
        color: usize,
    },
    Text {
        at: Point,
        text: String,
        color: usize,
    },
    Blur {
        rect: Rect,
    },
}

/// A capture: straight RGBA, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Picture {
    pub fn from_png(png: &[u8]) -> anyhow::Result<Self> {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
        let mut reader = decoder.read_info()?;
        let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
        let info = reader.next_frame(&mut buf)?;
        let rgba = match info.color_type {
            png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
            png::ColorType::GrayscaleAlpha => buf[..info.buffer_size()]
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[0], p[0], p[1]])
                .collect(),
            other => anyhow::bail!("unexpected colours: {other:?}"),
        };
        Ok(Self {
            width: info.width,
            height: info.height,
            rgba,
        })
    }

    pub fn to_png(&self) -> anyhow::Result<Vec<u8>> {
        let mut png = Vec::new();
        let mut encoder = png::Encoder::new(&mut png, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&self.rgba)?;
        writer.finish()?;
        Ok(png)
    }

    /// The capture with the blurred areas blurred, premultiplied for the
    /// toolkit.
    pub fn base(&self, shapes: &[Shape]) -> Vec<u8> {
        let mut rgba = self.rgba.clone();
        for shape in shapes {
            if let Shape::Blur { rect } = shape {
                blur(&mut rgba, self.width, self.height, *rect);
            }
        }
        for px in rgba.as_chunks_mut::<4>().0 {
            let a = u16::from(px[3]);
            for c in &mut px[..3] {
                *c = ((u16::from(*c) * a + 127) / 255) as u8;
            }
        }
        rgba
    }
}

/// Blurs `rect` of `rgba` strongly enough that text in it cannot be read:
/// three box passes each way.
pub fn blur(rgba: &mut [u8], width: u32, height: u32, rect: Rect) {
    let x0 = (rect.x.max(0.0) as u32).min(width);
    let y0 = (rect.y.max(0.0) as u32).min(height);
    let x1 = (rect.right().max(0.0) as u32).min(width);
    let y1 = (rect.bottom().max(0.0) as u32).min(height);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let radius = ((x1 - x0).min(y1 - y0) / 6).clamp(3, 24) as i64;
    let (w, h) = ((x1 - x0) as i64, (y1 - y0) as i64);
    let mut area: Vec<[f32; 4]> = Vec::with_capacity((w * h) as usize);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = ((y * width + x) * 4) as usize;
            area.push([0, 1, 2, 3].map(|c| f32::from(rgba[i + c])));
        }
    }
    let pass = |area: &mut Vec<[f32; 4]>, horizontal: bool| {
        let (outer, inner) = if horizontal { (h, w) } else { (w, h) };
        let at = |o: i64, i: i64| {
            if horizontal {
                (o * w + i) as usize
            } else {
                (i * w + o) as usize
            }
        };
        let mut line = vec![[0.0f32; 4]; inner as usize];
        for o in 0..outer {
            for i in 0..inner {
                let mut sum = [0.0f32; 4];
                let (from, to) = ((i - radius).max(0), (i + radius).min(inner - 1));
                for j in from..=to {
                    let p = area[at(o, j)];
                    for c in 0..4 {
                        sum[c] += p[c];
                    }
                }
                let n = (to - from + 1) as f32;
                line[i as usize] = sum.map(|s| s / n);
            }
            for i in 0..inner {
                area[at(o, i)] = line[i as usize];
            }
        }
    };
    for _ in 0..3 {
        pass(&mut area, true);
        pass(&mut area, false);
    }
    for y in 0..h {
        for x in 0..w {
            let i = (((y0 as i64 + y) * i64::from(width) + x0 as i64 + x) * 4) as usize;
            let p = area[(y * w + x) as usize];
            for c in 0..4 {
                rgba[i + c] = p[c].round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// A line's width for a capture this wide, in its pixels.
pub fn stroke(width: u32) -> f32 {
    (width as f32 / 320.0).clamp(3.0, 10.0)
}

/// A text's size for a capture this wide, in its pixels.
pub fn text_size(width: u32) -> f32 {
    (width as f32 / 48.0).clamp(18.0, 64.0)
}

/// Arrows and frames, drawn over the capture shown in a rectangle at `k`
/// view pixels per capture pixel.
struct Marks {
    shapes: Vec<Shape>,
    k: f32,
    width: f32,
}

impl Draw for Marks {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, _: &Theme, _: std::time::Duration) {
        let at = |p: Point| Point::new(rect.x + p.x * self.k, rect.y + p.y * self.k);
        let w = self.width * self.k;
        for shape in &self.shapes {
            match shape {
                Shape::Arrow { from, to, color } => {
                    let (a, b) = (at(*from), at(*to));
                    let (dx, dy) = (b.x - a.x, b.y - a.y);
                    let len = (dx * dx + dy * dy).sqrt().max(1.0);
                    let head = (w * 4.0).min(len * 0.5);
                    let (ux, uy) = (dx / len, dy / len);
                    // The head's two sides, at 30 degrees from the shaft.
                    let side = |s: f32| {
                        let (c, sn) = (0.866_f32, 0.5 * s);
                        Point::new(
                            b.x - head * (ux * c - uy * sn),
                            b.y - head * (uy * c + ux * sn),
                        )
                    };
                    let color = COLORS[*color % COLORS.len()];
                    canvas.polyline(&[a, b], w, color);
                    canvas.polyline(&[side(1.0), b, side(-1.0)], w, color);
                }
                Shape::Frame { rect: r, color } => {
                    let o = at(Point::new(r.x, r.y));
                    canvas.stroke_rect(
                        Rect::new(o.x, o.y, r.w * self.k, r.h * self.k),
                        w,
                        w,
                        COLORS[*color % COLORS.len()],
                    );
                }
                Shape::Text { .. } | Shape::Blur { .. } => {}
            }
        }
    }

    fn size(&self) -> Size {
        Size::new(0.0, 0.0)
    }
}

/// Adds the capture and its marks to `parent`, shown at `place` (`k` view
/// pixels per capture pixel): the base picture, the marks over it, then
/// the texts.
pub fn build(
    tree: &mut Tree,
    parent: NodeId,
    place: Rect,
    picture: &Picture,
    base: &[u8],
    shapes: &[Shape],
    theme: &Theme,
) -> Option<NodeId> {
    let k = place.w / picture.width as f32;
    let fixed = |r: Rect| Style {
        at: Some(Point::new(r.x, r.y)),
        width: Length::Px(r.w),
        height: Length::Px(r.h),
        ..Style::default()
    };
    let image = Kind::image(
        u16::try_from(picture.width).ok()?,
        u16::try_from(picture.height).ok()?,
        base,
        "Capture",
    )?;
    let shown = tree.add(parent, fixed(place), image)?;
    tree.add(
        parent,
        fixed(place),
        Kind::Custom {
            draw: Box::new(Marks {
                shapes: shapes.to_vec(),
                k,
                width: stroke(picture.width),
            }),
            label: None,
        },
    )?;
    let size = text_size(picture.width) * k;
    for shape in shapes {
        if let Shape::Text { at, text, color } = shape {
            let style = TextStyle {
                size,
                ..theme.text.title.with_weight(700)
            }
            .with_color(Paint::Rgba(COLORS[*color % COLORS.len()]));
            tree.add(
                parent,
                Style {
                    at: Some(Point::new(place.x + at.x * k, place.y + at.y * k)),
                    ..Style::default()
                },
                Kind::text(text.clone(), style),
            )?;
        }
    }
    Some(shown)
}

/// The capture with its marks, at its own size.
pub fn export(picture: &Picture, shapes: &[Shape], theme: &Theme) -> Option<Picture> {
    let size = Size::new(picture.width as f32, picture.height as f32);
    let mut tree = Tree::new(theme.clone(), size, 1.0);
    let root = tree.root();
    tree.set_look(root, Look::default());
    let base = picture.base(shapes);
    build(
        &mut tree,
        root,
        Rect::new(0.0, 0.0, size.w, size.h),
        picture,
        &base,
        shapes,
        theme,
    )?;
    let mut surface = Surface::new(size, 1.0);
    let damage = tree.update(std::time::Duration::ZERO);
    surface.draw(&mut tree, &damage);
    let mut rgba = surface.pixels().to_vec();
    // Straight alpha for the file.
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = u16::from(px[3]);
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((u16::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Some(Picture {
        width: picture.width,
        height: picture.height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Black and white stripes, two pixels wide: as sharp as text.
    fn stripes(w: u32, h: u32) -> Picture {
        let mut rgba = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                let v = if (x / 2) % 2 == 0 { 0 } else { 255 };
                rgba.extend([v, v, v, 255]);
            }
        }
        Picture {
            width: w,
            height: h,
            rgba,
        }
    }

    fn spread(p: &Picture, rect: Rect) -> u8 {
        let mut values = Vec::new();
        for y in rect.y as u32..rect.bottom() as u32 {
            for x in rect.x as u32..rect.right() as u32 {
                values.push(p.rgba[((y * p.width + x) * 4) as usize]);
            }
        }
        values.iter().max().copied().unwrap_or(0) - values.iter().min().copied().unwrap_or(0)
    }

    #[test]
    fn a_blurred_area_is_blurred_in_the_file_and_only_it() {
        let picture = stripes(200, 120);
        let blurred = Rect::new(40.0, 30.0, 80.0, 50.0);
        let out = export(
            &picture,
            &[Shape::Blur { rect: blurred }],
            &Theme::default(),
        )
        .expect("exported");
        let inside = Rect::new(55.0, 45.0, 50.0, 20.0);
        assert!(
            spread(&out, inside) < 40,
            "blurred: {}",
            spread(&out, inside)
        );
        let outside = Rect::new(150.0, 30.0, 30.0, 30.0);
        assert_eq!(spread(&out, outside), 255, "the rest stays sharp");
    }

    #[test]
    fn nothing_drawn_is_the_capture_as_taken() {
        let picture = stripes(64, 32);
        let out = export(&picture, &[], &Theme::default()).expect("exported");
        assert_eq!(out, picture);
    }

    #[test]
    fn an_arrow_shows_in_its_colour() {
        let picture = Picture {
            width: 200,
            height: 100,
            rgba: [0, 0, 0, 255].repeat(200 * 100),
        };
        let arrow = Shape::Arrow {
            from: Point::new(20.0, 50.0),
            to: Point::new(180.0, 50.0),
            color: 0,
        };
        let out = export(&picture, &[arrow], &Theme::default()).expect("exported");
        let i = ((50 * 200 + 100) * 4) as usize;
        assert_eq!(&out.rgba[i..i + 3], &[0xE5, 0x48, 0x4D]);
    }
}
