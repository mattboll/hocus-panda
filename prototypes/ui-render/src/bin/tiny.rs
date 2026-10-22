//! tiny-skia for shapes, cosmic-text (swash) for text, on the CPU.

use std::collections::HashMap;

use cosmic_text::{
    Attrs, Buffer, Family as CtFamily, FontSystem, Metrics, Shaping, SwashCache, SwashContent,
    Weight, fontdb,
};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, PixmapMut, Rect, Stroke, Transform};
use ui_render_prototype::{
    BACKGROUND, Family, Fonts, Rgba, Shape, Widget, agent, column, height, measure, peak_rss_kib,
    report, write_png,
};

struct Painter {
    fonts: FontSystem,
    glyphs: SwashCache,
    /// Shaped text by content and style: only changed text is shaped again.
    layouts: HashMap<(String, u32, u16, Family, u32), Buffer>,
    scale: f32,
}

impl Painter {
    fn new(scale: f32) -> Self {
        let data = Fonts::load();
        let mut db = fontdb::Database::new();
        db.load_font_data(data.interface);
        db.load_font_data(data.display);
        Self {
            fonts: FontSystem::new_with_locale_and_db("fr-FR".into(), db),
            glyphs: SwashCache::new(),
            layouts: HashMap::new(),
            scale,
        }
    }

    fn widget(&mut self, pixmap: &mut PixmapMut, widget: &Widget, x0: f32, y0: f32) {
        let s = self.scale;
        let transform = Transform::from_scale(s, s).pre_translate(x0, y0);
        for shape in &widget.shapes {
            match shape {
                Shape::RoundRect {
                    x,
                    y,
                    w,
                    h,
                    radius,
                    fill,
                    border,
                } => {
                    if let Some(path) = round_rect(*x, *y, *w, *h, *radius) {
                        pixmap.fill_path(&path, &paint(*fill), FillRule::Winding, transform, None);
                        if let Some(border) = border
                            && let Some(inner) =
                                round_rect(x + 0.5, y + 0.5, w - 1.0, h - 1.0, radius - 0.5)
                        {
                            let stroke = Stroke {
                                width: 1.0,
                                ..Stroke::default()
                            };
                            pixmap.stroke_path(&inner, &paint(*border), &stroke, transform, None);
                        }
                    }
                }
                Shape::Arc {
                    cx,
                    cy,
                    r,
                    start,
                    sweep,
                    width,
                    color,
                } => {
                    let mut pb = PathBuilder::new();
                    let steps = 48;
                    for i in 0..=steps {
                        let a = start + sweep * i as f32 / steps as f32;
                        let (px, py) = (cx + r * a.cos(), cy + r * a.sin());
                        if i == 0 {
                            pb.move_to(px, py);
                        } else {
                            pb.line_to(px, py);
                        }
                    }
                    if let Some(path) = pb.finish() {
                        let stroke = Stroke {
                            width: *width,
                            line_cap: tiny_skia::LineCap::Round,
                            ..Stroke::default()
                        };
                        pixmap.stroke_path(&path, &paint(*color), &stroke, transform, None);
                    }
                }
                Shape::Text {
                    x,
                    y,
                    width,
                    size,
                    weight,
                    family,
                    color,
                    text,
                } if !ui_render_prototype::skip_text() => self.text(
                    pixmap,
                    (x0 + x) * s,
                    (y0 + y) * s,
                    *width,
                    *size,
                    *weight,
                    *family,
                    *color,
                    text,
                ),
                Shape::Text { .. } => {}
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        pixmap: &mut PixmapMut,
        px: f32,
        py: f32,
        width: f32,
        size: f32,
        weight: u16,
        family: Family,
        color: Rgba,
        text: &str,
    ) {
        let s = self.scale;
        let key = (
            text.to_owned(),
            size.to_bits(),
            weight,
            family,
            width.to_bits(),
        );
        let fonts = &mut self.fonts;
        let buffer = self.layouts.entry(key).or_insert_with(|| {
            // Shaped at the physical size, so hinting and positions are exact.
            let mut buffer = Buffer::new(fonts, Metrics::new(size * s, size * s * 1.35));
            let mut b = buffer.borrow_with(fonts);
            b.set_size(Some(width * s), None);
            let name = match family {
                Family::Interface => "Manrope",
                Family::Display => "Fraunces",
            };
            let attrs = Attrs::new()
                .family(CtFamily::Name(name))
                .weight(Weight(weight));
            b.set_text(text, &attrs, Shaping::Advanced, None);
            b.shape_until_scroll(true);
            buffer
        });
        let (w, h) = (pixmap.width() as i32, pixmap.height() as i32);
        let data = pixmap.data_mut();
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                let physical = glyph.physical((px, py + run.line_y), 1.0);
                let Some(image) = self.glyphs.get_image(&mut self.fonts, physical.cache_key) else {
                    continue;
                };
                let left = physical.x + image.placement.left;
                let top = physical.y - image.placement.top;
                let (iw, ih) = (image.placement.width as i32, image.placement.height as i32);
                for gy in 0..ih {
                    let ty = top + gy;
                    if ty < 0 || ty >= h {
                        continue;
                    }
                    for gx in 0..iw {
                        let tx = left + gx;
                        if tx < 0 || tx >= w {
                            continue;
                        }
                        let coverage = match image.content {
                            SwashContent::Mask => image.data[(gy * iw + gx) as usize] as u32,
                            SwashContent::Color => {
                                image.data[((gy * iw + gx) * 4 + 3) as usize] as u32
                            }
                            SwashContent::SubpixelMask => {
                                image.data[((gy * iw + gx) * 4 + 1) as usize] as u32
                            }
                        };
                        if coverage == 0 {
                            continue;
                        }
                        let a = coverage * color.3 as u32 / 255;
                        let i = ((ty * w + tx) * 4) as usize;
                        let dst = &mut data[i..i + 4];
                        // Source over, premultiplied.
                        for (c, src) in [color.0, color.1, color.2, 255].into_iter().enumerate() {
                            let src = src as u32 * a / 255;
                            dst[c] = (src + dst[c] as u32 * (255 - a) / 255) as u8;
                        }
                    }
                }
            }
        }
    }
}

fn paint(c: Rgba) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(c.0, c.1, c.2, c.3));
    paint.anti_alias = true;
    paint
}

fn round_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    // Cubic approximation of a quarter circle.
    let k = r * 0.552_284_8;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    let _ = Rect::from_xywh(x, y, w, h)?;
    pb.finish()
}

fn clear(pixmap: &mut Pixmap) {
    let c = BACKGROUND;
    pixmap.fill(Color::from_rgba8(c.0, c.1, c.2, c.3));
}

fn main() {
    let scale: f32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(2.0);
    let mut painter = Painter::new(scale);
    let widgets = column(0);
    let (pw, ph) = (
        (ui_render_prototype::WIDTH * scale).ceil() as u32,
        (height(&widgets) * scale).ceil() as u32,
    );
    let mut target = Pixmap::new(pw, ph).expect("pixmap");

    // The whole column, every widget, the agent changing each frame.
    let full = measure(20, 200, |frame| {
        clear(&mut target);
        for w in column(frame) {
            painter.widget(&mut target.as_mut(), &w, 0.0, w.y);
        }
    });
    write_png(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("out/tiny@{scale}x.png")),
        pw,
        ph,
        target.data(),
    );

    // One widget into its own buffer: what a damaged widget costs.
    let probe = agent(0);
    let mut layer = Pixmap::new(
        (probe.w() * scale).ceil() as u32,
        (probe.h * scale).ceil() as u32,
    )
    .expect("layer");
    let one = measure(20, 1000, |frame| {
        layer.fill(Color::TRANSPARENT);
        painter.widget(&mut layer.as_mut(), &agent(frame), 0.0, 0.0);
    });

    // What uploading the layer to a GL texture copies, at least.
    let copy = measure(20, 1000, |_| {
        std::hint::black_box(layer.data().to_vec());
    });
    ui_render_prototype::SKIP_TEXT.store(true, std::sync::atomic::Ordering::Relaxed);
    let shapes = measure(20, 200, |frame| {
        clear(&mut target);
        for w in column(frame) {
            painter.widget(&mut target.as_mut(), &w, 0.0, w.y);
        }
    });
    report(
        "tiny-skia + cosmic-text",
        scale,
        full,
        one,
        &[
            ("shapes only", shapes.to_string()),
            ("layer copy", copy.to_string()),
            (
                "peak memory",
                format!("{} KiB", peak_rss_kib().unwrap_or_default()),
            ),
        ],
    );
}
