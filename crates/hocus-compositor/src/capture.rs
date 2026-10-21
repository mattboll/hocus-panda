//! Screenshots: an output or a window drawn again into a fresh offscreen
//! buffer and read back. The PNG is encoded on a worker thread.

use anyhow::Context;
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, ExportMem, ImportAll, ImportMem, Offscreen, Renderer, Texture,
            damage::OutputDamageTracker,
            element::{AsRenderElements, surface::WaylandSurfaceRenderElement},
        },
    },
    desktop::Window,
    output::Output,
    utils::{Physical, Point, Rectangle, Scale, Size, Transform},
};

use crate::{BACKGROUND, State};

/// Pixels read back from a renderer: rows of RGBA bytes, top to bottom.
#[derive(Debug)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Pixels {
    pub fn encode_png(&self) -> anyhow::Result<Vec<u8>> {
        let mut png = Vec::new();
        let mut encoder = png::Encoder::new(&mut png, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&self.rgba)?;
        writer.finish()?;
        Ok(png)
    }
}

/// Draws everything `output` shows, at its pixel size.
pub fn output<R, T>(
    renderer: &mut R,
    output: &Output,
    state: &State,
    cursor: bool,
) -> anyhow::Result<Pixels>
where
    R: Renderer + ImportAll + ImportMem + Offscreen<T> + Bind<T> + ExportMem,
    R::TextureId: Send + Clone + Texture + 'static,
    R::Error: Send + Sync + 'static,
{
    let size = output
        .current_mode()
        .context("the output has no mode")?
        .size;
    let scale = output.current_scale().fractional_scale();
    let elements = state.scene_elements(renderer, output, cursor);
    draw(renderer, size, scale, &elements, BACKGROUND)
}

/// Draws `window` alone at `scale`, on a transparent background.
pub fn window<R, T>(renderer: &mut R, window: &Window, scale: f64) -> anyhow::Result<Pixels>
where
    R: Renderer + ImportAll + Offscreen<T> + Bind<T> + ExportMem,
    R::TextureId: Clone + Texture + 'static,
    R::Error: Send + Sync + 'static,
{
    let geometry = window.geometry();
    let size = geometry.size.to_f64().to_physical(scale).to_i32_round();
    // The window's geometry starts at the buffer's origin.
    let origin = (Point::default() - geometry.loc.to_f64().to_physical(scale)).to_i32_round();
    let elements: Vec<WaylandSurfaceRenderElement<R>> =
        window.render_elements(renderer, origin, Scale::from(scale), 1.0);
    draw(renderer, size, scale, &elements, [0.0; 4])
}

fn draw<R, T, E>(
    renderer: &mut R,
    size: Size<i32, Physical>,
    scale: f64,
    elements: &[E],
    background: [f32; 4],
) -> anyhow::Result<Pixels>
where
    R: Renderer + Offscreen<T> + Bind<T> + ExportMem,
    R::Error: Send + Sync + 'static,
    E: smithay::backend::renderer::element::RenderElement<R>,
{
    anyhow::ensure!(size.w > 0 && size.h > 0, "nothing to capture: {size:?}");
    let buffer_size = size.to_logical(1).to_buffer(1, Transform::Normal);
    let mut target = renderer
        .create_buffer(Fourcc::Abgr8888, buffer_size)
        .context("could not create the capture buffer")?;
    let mut framebuffer = renderer
        .bind(&mut target)
        .context("could not bind the capture buffer")?;
    let mut damage_tracker = OutputDamageTracker::new(size, scale, Transform::Normal);
    damage_tracker
        .render_output(renderer, &mut framebuffer, 0, elements, background)
        .map_err(|err| anyhow::anyhow!("{err:?}"))
        .context("could not draw the capture")?;
    let mapping = renderer
        .copy_framebuffer(
            &framebuffer,
            Rectangle::from_size(buffer_size),
            Fourcc::Abgr8888,
        )
        .context("could not read the capture back")?;
    let rgba = renderer
        .map_texture(&mapping)
        .context("could not map the capture")?
        .to_vec();
    Ok(Pixels {
        width: size.w as u32,
        height: size.h as u32,
        rgba,
    })
}
