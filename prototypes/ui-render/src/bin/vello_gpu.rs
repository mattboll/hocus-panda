//! vello on the GPU (wgpu compute), parley for text.

#[path = "../parley_text.rs"]
mod parley_text;

use std::time::Instant;

use ui_render_prototype::{
    BACKGROUND, Rgba, Shape, WIDTH, Widget, agent, column, height, measure, peak_rss_kib, report,
    write_png,
};
use vello::{
    AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene,
    kurbo::{Affine, Arc as KArc, Cap, RoundedRect, Stroke},
    peniko::{Color, Fill},
    util::RenderContext,
    wgpu,
};

fn color(c: Rgba) -> Color {
    Color::from_rgba8(c.0, c.1, c.2, c.3)
}

fn encode(scene: &mut Scene, texts: &mut parley_text::Texts, widget: &Widget, y0: f32, s: f32) {
    let t = Affine::scale(f64::from(s)).then_translate((0.0, f64::from(y0 * s)).into());
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
                scene.fill(Fill::NonZero, t, color(*fill), None, &rect);
                if let Some(border) = border {
                    let inner = RoundedRect::new(
                        f64::from(x + 0.5),
                        f64::from(y + 0.5),
                        f64::from(x + w - 0.5),
                        f64::from(y + h - 0.5),
                        f64::from(radius - 0.5),
                    );
                    scene.stroke(&Stroke::new(1.0), t, color(*border), None, &inner);
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
                scene.stroke(&stroke, t, color(*c), None, &arc);
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
                    scene
                        .draw_glyphs(run.font)
                        .font_size(run.size)
                        .normalized_coords(&run.coords)
                        .brush(color(*c))
                        .draw(
                            Fill::NonZero,
                            run.glyphs
                                .into_iter()
                                .map(|(id, x, y)| vello::Glyph { id, x, y }),
                        );
                },
            ),
            Shape::Text { .. } => {}
        }
    }
}

fn texture(device: &wgpu::Device, w: u32, h: u32) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

/// Copies the texture back to memory: what showing it through smithay's GLES
/// renderer costs without sharing buffers between the two APIs.
fn read_back(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
    let (w, h) = (texture.width(), texture.height());
    let row = (w * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("map"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let mapped = slice.get_mapped_range().expect("mapped");
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let start = (y * row) as usize;
        pixels.extend_from_slice(&mapped[start..start + (w * 4) as usize]);
    }
    drop(mapped);
    buffer.unmap();
    pixels
}

fn main() {
    let scale: f32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(2.0);
    let started = Instant::now();
    let mut context = RenderContext::new();
    let id = pollster::block_on(context.device(None)).expect("no GPU device");
    let (device, queue) = (&context.devices[id].device, &context.devices[id].queue);
    let mut renderer = Renderer::new(
        device,
        RendererOptions {
            antialiasing_support: AaSupport::area_only(),
            ..RendererOptions::default()
        },
    )
    .expect("renderer");
    let mut texts = parley_text::Texts::new(scale);
    let init = started.elapsed();

    let widgets = column(0);
    let (pw, ph) = (
        (WIDTH * scale).ceil() as u32,
        (height(&widgets) * scale).ceil() as u32,
    );
    let (target, target_view) = texture(device, pw, ph);
    let mut scene = Scene::new();
    let params = |w, h| RenderParams {
        base_color: color(BACKGROUND),
        width: w,
        height: h,
        antialiasing_method: AaConfig::Area,
    };

    let mut render = |scene: &Scene, view: &wgpu::TextureView, w, h| {
        renderer
            .render_to_texture(device, queue, scene, view, &params(w, h))
            .expect("render");
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
    };

    // The whole column, until the GPU is done with it.
    let full = measure(20, 200, |frame| {
        scene.reset();
        for w in column(frame) {
            encode(&mut scene, &mut texts, &w, w.y, scale);
        }
        render(&scene, &target_view, pw, ph);
    });
    let pixels = read_back(device, queue, &target);
    write_png(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("out/vello-gpu@{scale}x.png")),
        pw,
        ph,
        &pixels,
    );

    // One widget into its own texture.
    let probe = agent(0);
    let (lw, lh) = (
        (probe.w() * scale).ceil() as u32,
        (probe.h * scale).ceil() as u32,
    );
    let (layer, layer_view) = texture(device, lw, lh);
    let one = measure(20, 1000, |frame| {
        scene.reset();
        encode(&mut scene, &mut texts, &agent(frame), 0.0, scale);
        render(&scene, &layer_view, lw, lh);
    });
    let encode_only = measure(20, 1000, |frame| {
        scene.reset();
        encode(&mut scene, &mut texts, &agent(frame), 0.0, scale);
    });
    let readback = measure(20, 200, |_| {
        std::hint::black_box(read_back(device, queue, &layer));
    });
    ui_render_prototype::SKIP_TEXT.store(true, std::sync::atomic::Ordering::Relaxed);
    let shapes = measure(20, 200, |frame| {
        scene.reset();
        for w in column(frame) {
            encode(&mut scene, &mut texts, &w, w.y, scale);
        }
        renderer
            .render_to_texture(device, queue, &scene, &target_view, &params(pw, ph))
            .expect("render");
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
    });
    report(
        "vello (GPU) + parley",
        scale,
        full,
        one,
        &[
            ("shapes only", shapes.to_string()),
            ("encode only", encode_only.to_string()),
            ("layer read back", readback.to_string()),
            ("init", format!("{:.1} ms", init.as_secs_f64() * 1000.0)),
            (
                "peak memory",
                format!("{} KiB", peak_rss_kib().unwrap_or_default()),
            ),
        ],
    );
}
