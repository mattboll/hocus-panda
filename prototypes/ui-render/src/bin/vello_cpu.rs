//! vello_cpu (sparse strips, on the CPU), parley for text.

#[path = "../parley_text.rs"]
mod parley_text;

use std::time::Instant;

use ui_render_prototype::{
    BACKGROUND, Rgba, Shape, WIDTH, Widget, agent, column, height, measure, peak_rss_kib, report,
    write_png,
};
use vello_cpu::{
    Pixmap, RenderContext, Resources,
    color::{AlphaColor, Srgb},
    kurbo::{Affine, Arc as KArc, Cap, Rect, RoundedRect, Shape as _, Stroke},
};

fn color(c: Rgba) -> AlphaColor<Srgb> {
    AlphaColor::from_rgba8(c.0, c.1, c.2, c.3)
}

fn encode(
    ctx: &mut RenderContext,
    resources: &mut Resources,
    texts: &mut parley_text::Texts,
    widget: &Widget,
    y0: f32,
    s: f32,
) {
    let t = Affine::scale(f64::from(s)).then_translate((0.0, f64::from(y0 * s)).into());
    ctx.set_transform(t);
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
                let rect = RoundedRect::new(
                    f64::from(*x),
                    f64::from(*y),
                    f64::from(x + w),
                    f64::from(y + h),
                    f64::from(*radius),
                );
                ctx.set_paint(color(*fill));
                ctx.fill_path(&rect.to_path(0.1));
                if let Some(border) = border {
                    let inner = RoundedRect::new(
                        f64::from(x + 0.5),
                        f64::from(y + 0.5),
                        f64::from(x + w - 0.5),
                        f64::from(y + h - 0.5),
                        f64::from(radius - 0.5),
                    );
                    ctx.set_paint(color(*border));
                    ctx.set_stroke(Stroke::new(1.0));
                    ctx.stroke_path(&inner.to_path(0.1));
                }
            }
            Shape::Arc {
                cx,
                cy,
                r,
                start,
                sweep,
                width,
                color: c,
            } => {
                let arc = KArc::new(
                    (f64::from(*cx), f64::from(*cy)),
                    (f64::from(*r), f64::from(*r)),
                    f64::from(*start),
                    f64::from(*sweep),
                    0.0,
                );
                let stroke = Stroke::new(f64::from(*width)).with_caps(Cap::Round);
                ctx.set_paint(color(*c));
                ctx.set_stroke(stroke);
                ctx.stroke_path(&arc.into_path(0.1));
            }
            Shape::Text {
                x,
                y,
                width,
                size,
                weight,
                family,
                color: c,
                text,
            } if !ui_render_prototype::skip_text() => texts.runs(
                text,
                *size,
                *weight,
                *family,
                *width,
                x * s,
                (y0 + y) * s,
                |run| {
                    // Glyph positions are physical: no scale on top.
                    ctx.set_transform(Affine::IDENTITY);
                    ctx.set_paint(color(*c));
                    let _ = ctx
                        .glyph_run(resources, run.font)
                        .atlas_cache(atlas())
                        .font_size(run.size)
                        .normalized_coords(&run.coords)
                        .fill_glyphs(run.glyphs.into_iter().map(|(id, x, y)| vello_cpu::Glyph {
                            id,
                            x,
                            y,
                        }));
                    ctx.set_transform(t);
                },
            ),
            Shape::Text { .. } => {}
        }
    }
}

/// Whether glyphs are cached in an atlas: `ATLAS=1`.
fn atlas() -> bool {
    std::env::var_os("ATLAS").is_some_and(|v| v == "1")
}

fn clear(ctx: &mut RenderContext, w: f64, h: f64, c: Rgba) {
    ctx.set_transform(Affine::IDENTITY);
    ctx.set_paint(color(c));
    ctx.fill_rect(&Rect::new(0.0, 0.0, w, h));
}

fn main() {
    let scale: f32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(2.0);
    let started = Instant::now();
    let mut texts = parley_text::Texts::new(scale);
    let mut resources = Resources::new();
    let init = started.elapsed();

    let widgets = column(0);
    let (pw, ph) = (
        (WIDTH * scale).ceil() as u16,
        (height(&widgets) * scale).ceil() as u16,
    );
    let mut ctx = RenderContext::new(pw, ph);
    let mut target = Pixmap::new(pw, ph);
    let full = measure(20, 200, |frame| {
        ctx.reset();
        clear(&mut ctx, f64::from(pw), f64::from(ph), BACKGROUND);
        for w in column(frame) {
            encode(&mut ctx, &mut resources, &mut texts, &w, w.y, scale);
        }
        ctx.flush();
        ctx.render(&mut target, &mut resources);
    });
    write_png(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("out/vello-cpu@{scale}x.png")),
        u32::from(pw),
        u32::from(ph),
        target.data_as_u8_slice(),
    );

    let probe = agent(0);
    let (lw, lh) = (
        (probe.w() * scale).ceil() as u16,
        (probe.h * scale).ceil() as u16,
    );
    let mut layer_ctx = RenderContext::new(lw, lh);
    let mut layer = Pixmap::new(lw, lh);
    let one = measure(20, 1000, |frame| {
        layer_ctx.reset();
        encode(
            &mut layer_ctx,
            &mut resources,
            &mut texts,
            &agent(frame),
            0.0,
            scale,
        );
        layer_ctx.flush();
        layer_ctx.render(&mut layer, &mut resources);
    });
    ui_render_prototype::SKIP_TEXT.store(true, std::sync::atomic::Ordering::Relaxed);
    let shapes = measure(20, 200, |frame| {
        ctx.reset();
        clear(&mut ctx, f64::from(pw), f64::from(ph), BACKGROUND);
        for w in column(frame) {
            encode(&mut ctx, &mut resources, &mut texts, &w, w.y, scale);
        }
        ctx.flush();
        ctx.render(&mut target, &mut resources);
    });
    report(
        "vello_cpu + parley",
        scale,
        full,
        one,
        &[
            ("shapes only", shapes.to_string()),
            ("init", format!("{:.1} ms", init.as_secs_f64() * 1000.0)),
            (
                "peak memory",
                format!("{} KiB", peak_rss_kib().unwrap_or_default()),
            ),
        ],
    );
}
