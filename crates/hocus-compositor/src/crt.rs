//! The Matrix theme's refinements on the GPU (#169), what plain elements
//! cannot draw (docs/design/matrix-theme.md):
//!
//! - A closing window's glitch separates its colours: within each band
//!   shifted sideways, red is taken from one side and blue from the other
//!   (`shaders/glitch.frag`).
//! - Bright lines (the CRT's line, the circuit traces, the grid guides)
//!   bloom: a 2 px gaussian glow around them, drawn in nine parts from a
//!   profile painted once (`shaders/crt.frag`).
//! - Fullscreen overlays (Alt+Tab, the overview, the lock, the boot
//!   screen) have scanlines and a vignette, which swells once, slowly, as
//!   the lock comes.
//!
//! As for the blur and the moving wallpapers, a GLES backend paints them
//! before it draws an output (`paint_crt`), and the scene, whatever its
//! renderer, shows the textures. The bloom's profile, the vignette and a
//! screen's scanlines are painted once and drawn with ids kept from frame
//! to frame: at rest they repaint nothing. The glitch is painted again on
//! each frame of the 120 ms it plays, frames drawn anyway, and damages
//! only the window. Nothing is queued per frame.
//!
//! The pixman renderer has no shaders: there, and with reduced motion (or
//! in tests that ask for instant motion), the plain effects stay, the
//! same pixels as before.

use std::{any::Any, collections::BTreeMap};

use hocus_core::WindowId;
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, Color32F, Frame, Offscreen, Renderer, Texture,
            element::{Id, Kind, texture::TextureRenderElement},
            gles::{
                GlesError, GlesPixelProgram, GlesRenderer, GlesTexProgram, GlesTexture, Uniform,
                UniformName, UniformType,
            },
            sync::SyncPoint,
            utils::draw_render_elements,
        },
    },
    output::Output,
    utils::{Buffer, Logical, Physical, Point, Rectangle, Size, Transform},
};

use crate::{
    State,
    matrix::{millis, phase},
    scene::SceneElement,
    snapshot::Snapshot,
};

/// How far a bloom reaches beyond its line, logical pixels: its gaussian
/// is one pixel wide (sigma), so that it shows over two.
pub const BLOOM_REACH: f64 = 3.0;
/// The profile's texels per logical pixel.
const TEXEL: f64 = 4.0;
/// Texels of the profile from its edge to the line in its middle, which is
/// two texels wide.
const BLOOM_EDGE: f64 = BLOOM_REACH * TEXEL;
const BLOOM_SIZE: i32 = 2 * (BLOOM_REACH * TEXEL) as i32 + 2;
/// The vignette, stretched over the screen: it is smooth.
const VIGNETTE_SIZE: i32 = 256;
/// The scanlines, stretched across the screen.
const SCANLINES_WIDTH: i32 = 4;
/// Every other row darkened by 15 %; behind Alt+Tab, by 8 %.
const SCAN: f32 = 0.15;
const SCAN_SWITCHER: f32 = 0.08;
/// The vignette's darkest, in the corners.
const VIGNETTE: f32 = 0.45;
/// How much the vignette swells as the lock comes.
const SWELL: f32 = 0.4;

/// The shaders, once compiled, or why there are none.
#[derive(Debug, Default)]
enum Shaders {
    /// No GLES backend has painted in the Matrix theme yet.
    #[default]
    Unknown,
    Ready(Box<Programs>),
    /// They did not compile: the plain effects only.
    Broken,
}

#[derive(Debug)]
struct Programs {
    glitch: GlesTexProgram,
    paint: GlesPixelProgram,
    /// The bloom's profile and the vignette, painted once.
    bloom: GlesTexture,
    vignette: GlesTexture,
}

impl Programs {
    fn compile(renderer: &mut GlesRenderer) -> Result<Self, String> {
        let glitch = renderer
            .compile_custom_texture_shader(
                include_str!("shaders/glitch.frag"),
                &[
                    UniformName::new("cuts", UniformType::_3f),
                    UniformName::new("shifts", UniformType::_4f),
                    UniformName::new("split", UniformType::_1f),
                ],
            )
            .map_err(|e| format!("glitch: {e}"))?;
        let paint = renderer
            .compile_custom_pixel_shader(
                include_str!("shaders/crt.frag"),
                &[
                    UniformName::new("kind", UniformType::_1f),
                    UniformName::new("reach", UniformType::_1f),
                    UniformName::new("texel", UniformType::_1f),
                    UniformName::new("row", UniformType::_1f),
                ],
            )
            .map_err(|e| format!("crt: {e}"))?;
        let bloom = painted(
            renderer,
            &paint,
            Painting::Bloom,
            Size::from((BLOOM_SIZE, BLOOM_SIZE)),
        )?;
        let vignette = painted(
            renderer,
            &paint,
            Painting::Vignette,
            Size::from((VIGNETTE_SIZE, VIGNETTE_SIZE)),
        )?;
        Ok(Self {
            glitch,
            paint,
            bloom,
            vignette,
        })
    }
}

/// What `crt.frag` paints.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Painting {
    Bloom,
    /// Rows this many pixels tall.
    Scanlines(f32),
    Vignette,
}

/// A texture of `size` painted by `crt.frag`.
fn painted(
    renderer: &mut GlesRenderer,
    program: &GlesPixelProgram,
    painting: Painting,
    size: Size<i32, Buffer>,
) -> Result<GlesTexture, String> {
    let mut texture: GlesTexture = renderer
        .create_buffer(Fourcc::Abgr8888, size)
        .map_err(|e| e.to_string())?;
    let (kind, row) = match painting {
        Painting::Bloom => (0.0, 1.0),
        Painting::Scanlines(row) => (1.0, row),
        Painting::Vignette => (2.0, 1.0),
    };
    let target_size = Size::<i32, Physical>::from((size.w, size.h));
    let dest = Rectangle::from_size(target_size);
    let mut target = renderer.bind(&mut texture).map_err(|e| e.to_string())?;
    let mut frame = renderer
        .render(&mut target, target_size, Transform::Normal)
        .map_err(|e| e.to_string())?;
    frame
        .clear(Color32F::TRANSPARENT, &[dest])
        .map_err(|e| e.to_string())?;
    frame
        .render_pixel_shader_to(
            program,
            Rectangle::from_size(size.to_f64()),
            dest,
            size,
            None,
            1.0,
            &[
                Uniform::new("kind", kind),
                Uniform::new("reach", BLOOM_EDGE as f32),
                Uniform::new("texel", TEXEL as f32),
                Uniform::new("row", row),
            ],
        )
        .map_err(|e| e.to_string())?;
    // The same context draws with it next, in order: no need to wait.
    let _done = frame.finish().map_err(|e| e.to_string())?;
    drop(target);
    Ok(texture)
}

/// A screen's scanlines.
#[derive(Debug)]
struct Screen {
    /// Its height and a row's, physical pixels.
    height: i32,
    row: i32,
    scanlines: GlesTexture,
    /// The overlay's, kept from frame to frame: at rest, nothing repaints.
    scan_id: Id,
    vignette_id: Id,
}

/// A closing window's glitch on one screen.
#[derive(Debug)]
struct Glitched {
    size: Size<i32, Physical>,
    /// Its picture, flattened once; the glitch of it, painted each frame.
    flat: GlesTexture,
    out: GlesTexture,
    /// A new id each time it is painted: the screen draws it again.
    id: Id,
}

#[derive(Debug, Default)]
pub struct Crt {
    shaders: Shaders,
    /// By output name.
    screens: BTreeMap<String, Screen>,
    /// By output name and window.
    glitches: BTreeMap<(String, WindowId), Glitched>,
}

impl Crt {
    fn forget(&mut self, output: &str) {
        self.screens.remove(output);
        self.glitches.retain(|(o, _), _| o != output);
    }
}

/// A closing window in its glitch, for the GPU to separate its colours.
pub(crate) struct Glitching {
    pub id: WindowId,
    /// Global.
    pub frame: Rectangle<f64, Logical>,
    pub picture: Snapshot,
    /// Its bands, as `matrix::Close::bands` gives them, and how far apart
    /// red and blue are taken, logical pixels.
    pub bands: Vec<(f64, f64, f64)>,
    pub split: f64,
}

/// The glitch shader's uniforms for a picture `width` wide: where its four
/// bands meet, how far each is shifted and red and blue apart, as
/// fractions of the picture.
fn glitch_uniforms(bands: &[(f64, f64, f64)], split: f64, width: f64) -> ([f32; 3], [f32; 4], f32) {
    let width = width.max(1.0);
    let mut cuts = [1.0f32; 3];
    let mut shifts = [0.0f32; 4];
    for (i, (_, v1, dx)) in bands.iter().take(4).enumerate() {
        if i < 3 {
            cuts[i] = *v1 as f32;
        }
        shifts[i] = (*dx / width) as f32;
    }
    (cuts, shifts, (split / width) as f32)
}

/// The nine parts of a bloom around `line` (output-local, logical): where
/// each goes and its part of the profile, in texels. Its corners and sides
/// are the profile's; along the line, the middle of the profile is
/// stretched, where it does not change.
pub(crate) fn bloom_parts(
    line: Rectangle<f64, Logical>,
) -> Vec<(Rectangle<f64, Logical>, Rectangle<f64, Logical>)> {
    let r = BLOOM_REACH;
    let e = BLOOM_EDGE;
    // Across: before the line, along it, after it; each as (where, how
    // long; from, how many texels).
    let spans = |start: f64, length: f64| {
        [
            (start - r, r, 0.0, e),
            (start, length, e + 0.5, 1.0),
            (start + length, r, e + 2.0, e),
        ]
    };
    let xs = spans(line.loc.x, line.size.w);
    let ys = spans(line.loc.y, line.size.h);
    let mut parts = Vec::with_capacity(9);
    for (y, h, sy, sh) in ys {
        for (x, w, sx, sw) in xs {
            if w <= 0.0 || h <= 0.0 {
                continue;
            }
            parts.push((
                Rectangle::new((x, y).into(), (w, h).into()),
                Rectangle::new((sx, sy).into(), (sw, sh).into()),
            ));
        }
    }
    parts
}

/// How strong a fullscreen overlay's scanlines and vignette are, each as
/// its element's opacity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Overlay {
    pub scan: f32,
    pub vignette: f32,
}

impl Overlay {
    /// `scan` the scanlines' darkness, `shows` how much of the overlay
    /// shows, `swell` how much the vignette swells.
    fn new(scan: f32, shows: f32, swell: f32) -> Self {
        let shows = shows.clamp(0.0, 1.0);
        Self {
            scan: scan * shows,
            vignette: (VIGNETTE * shows * swell).min(1.0),
        }
    }
}

/// Our lock's overlay, `t` milliseconds after it began to come: as the
/// rain covers the desktop; the vignette swells once, slowly, and rests
/// once the lock is in, so that a locked screen draws nothing.
fn lock_overlay(t: f32) -> Overlay {
    let swell = 1.0 + SWELL * (std::f32::consts::PI * phase(t, 0.0, 1000.0)).sin();
    Overlay::new(SCAN, phase(t, 0.0, 600.0), swell)
}

/// Draws `from` into the whole of `to` through the glitch.
fn glitch_pass(
    renderer: &mut GlesRenderer,
    program: &GlesTexProgram,
    from: &GlesTexture,
    to: &mut GlesTexture,
    (cuts, shifts, split): ([f32; 3], [f32; 4], f32),
) -> Result<SyncPoint, String> {
    let size = to.size();
    let target_size = Size::<i32, Physical>::from((size.w, size.h));
    let dest = Rectangle::from_size(target_size);
    let mut target = renderer.bind(to).map_err(|e| e.to_string())?;
    let mut frame = renderer
        .render(&mut target, target_size, Transform::Normal)
        .map_err(|e| e.to_string())?;
    frame
        .clear(Color32F::TRANSPARENT, &[dest])
        .map_err(|e| e.to_string())?;
    frame
        .render_texture_from_to(
            from,
            Rectangle::from_size(from.size().to_f64()),
            dest,
            &[dest],
            &[],
            Transform::Normal,
            1.0,
            Some(program),
            &[
                Uniform::new("cuts", (cuts[0], cuts[1], cuts[2])),
                Uniform::new("shifts", (shifts[0], shifts[1], shifts[2], shifts[3])),
                Uniform::new("split", split),
            ],
        )
        .map_err(|e| e.to_string())?;
    frame.finish().map_err(|e| e.to_string())
}

/// `picture` flattened into `to`, stretched over it.
fn flatten(
    renderer: &mut GlesRenderer,
    picture: &Snapshot,
    to: &mut GlesTexture,
    logical: Size<f64, Logical>,
    scale: f64,
) -> Result<(), String> {
    let size = to.size();
    let target_size = Size::<i32, Physical>::from((size.w, size.h));
    let dest = Rectangle::from_size(target_size);
    let elements = picture.stretched(&*renderer, Rectangle::from_size(logical), scale, 1.0);
    let mut target = renderer.bind(to).map_err(|e| e.to_string())?;
    let mut frame = renderer
        .render(&mut target, target_size, Transform::Normal)
        .map_err(|e| e.to_string())?;
    frame
        .clear(Color32F::TRANSPARENT, &[dest])
        .map_err(|e| e.to_string())?;
    draw_render_elements::<GlesRenderer, _, _>(&mut frame, scale, &elements, &[dest])
        .map_err(|e: GlesError| e.to_string())?;
    let _done = frame.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// All of `texture`, in its own pixels.
fn whole(texture: &GlesTexture) -> Rectangle<f64, Logical> {
    let size = texture.size();
    Rectangle::from_size((f64::from(size.w), f64::from(size.h)).into())
}

/// `texture` for a renderer whose textures are the painter's.
fn as_texture<T: Clone + 'static>(texture: &GlesTexture) -> Option<T> {
    (texture as &dyn Any).downcast_ref::<T>().cloned()
}

impl State {
    /// Whether the Matrix theme's GPU refinements would show where the
    /// renderer has shaders: in the theme, moving as in a session, without
    /// reduced motion.
    fn crt_wanted(&self) -> bool {
        self.matrix_moves()
    }

    /// Whether they show now: wanted, and shaders to draw them with.
    pub(crate) fn crt_on(&self) -> bool {
        matches!(self.crt.shaders, Shaders::Ready(_)) && self.crt_wanted()
    }

    /// Before a GLES backend draws `output`: the scanlines of its size,
    /// and the glitch of each window closing on it.
    pub fn paint_crt(&mut self, renderer: &mut GlesRenderer, output: &Output) {
        let name = output.name();
        if !self.crt_wanted() {
            self.crt.forget(&name);
            return;
        }
        if matches!(self.crt.shaders, Shaders::Unknown) {
            self.crt.shaders = match Programs::compile(renderer) {
                Ok(programs) => Shaders::Ready(Box::new(programs)),
                Err(err) => {
                    tracing::warn!(%err, "the Matrix theme's effects stay plain: no shaders");
                    Shaders::Broken
                }
            };
            // The lines bloom from now on.
            self.redraw.all();
        }
        if !matches!(self.crt.shaders, Shaders::Ready(_)) {
            return;
        }
        if let Err(err) = self.paint_scanlines(renderer, output) {
            tracing::warn!(output = name, %err, "could not paint the scanlines");
            self.crt.screens.remove(&name);
        }
        if let Err(err) = self.paint_glitches(renderer, output) {
            tracing::warn!(output = name, %err, "could not paint the glitch");
            self.crt.glitches.retain(|(o, _), _| *o != name);
        }
    }

    /// `output`'s scanlines, painted again only when its height changes.
    fn paint_scanlines(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
    ) -> Result<(), String> {
        let Some(mode) = output.current_mode() else {
            return Ok(());
        };
        let height = output.current_transform().transform_size(mode.size).h;
        let row = output.current_scale().fractional_scale().round().max(1.0) as i32;
        let name = output.name();
        if self
            .crt
            .screens
            .get(&name)
            .is_some_and(|s| s.height == height && s.row == row)
        {
            return Ok(());
        }
        let Shaders::Ready(programs) = &self.crt.shaders else {
            return Ok(());
        };
        let scanlines = painted(
            renderer,
            &programs.paint,
            Painting::Scanlines(row as f32),
            Size::from((SCANLINES_WIDTH, height.max(1))),
        )?;
        let screen = Screen {
            height,
            row,
            scanlines,
            scan_id: Id::new(),
            vignette_id: Id::new(),
        };
        self.crt.screens.insert(name, screen);
        Ok(())
    }

    /// The glitch of each window closing on `output`, at the clock's time.
    fn paint_glitches(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
    ) -> Result<(), String> {
        let name = output.name();
        let now = self.clock.now();
        let glitching = self.glitching(now);
        self.crt
            .glitches
            .retain(|(o, id), _| *o != name || glitching.iter().any(|g| g.id == *id));
        let Some(area) = self.space.output_geometry(output) else {
            return Ok(());
        };
        let scale = output.current_scale().fractional_scale();
        for g in glitching {
            if !g.frame.overlaps(area.to_f64()) {
                continue;
            }
            let local = Rectangle::new(g.frame.loc - area.loc.to_f64(), g.frame.size);
            let size = local.to_physical_precise_round(scale).size;
            if size.w <= 0 || size.h <= 0 {
                continue;
            }
            let key = (name.clone(), g.id);
            let mut glitched = match self.crt.glitches.remove(&key) {
                Some(found) if found.size == size => found,
                _ => {
                    let buffer = Size::<i32, Buffer>::from((size.w, size.h));
                    let mut flat: GlesTexture = renderer
                        .create_buffer(Fourcc::Abgr8888, buffer)
                        .map_err(|e| e.to_string())?;
                    let out: GlesTexture = renderer
                        .create_buffer(Fourcc::Abgr8888, buffer)
                        .map_err(|e| e.to_string())?;
                    flatten(renderer, &g.picture, &mut flat, local.size, scale)?;
                    Glitched {
                        size,
                        flat,
                        out,
                        id: Id::new(),
                    }
                }
            };
            let Shaders::Ready(programs) = &self.crt.shaders else {
                return Ok(());
            };
            let uniforms = glitch_uniforms(&g.bands, g.split, local.size.w);
            // The same context draws the output next, in order: no need
            // to wait for this pass (which would hold the loop).
            let _done = glitch_pass(
                renderer,
                &programs.glitch,
                &glitched.flat,
                &mut glitched.out,
                uniforms,
            )?;
            glitched.id = Id::new();
            self.crt.glitches.insert(key, glitched);
            self.measures.glitch(&name);
        }
        Ok(())
    }

    /// The bloom's profile, for a renderer whose textures are the
    /// painter's, while the refinements show.
    pub(crate) fn bloom_texture<T: Clone + 'static>(&self) -> Option<T> {
        let Shaders::Ready(programs) = &self.crt.shaders else {
            return None;
        };
        if !self.crt_wanted() {
            return None;
        }
        as_texture(&programs.bloom)
    }

    /// The glitch of window `id` closing on `output`, as painted last
    /// (for this frame, or the one before when a capture takes its own),
    /// over `clip` (output-local, physical), `size` logical.
    pub(crate) fn glitch_element<R>(
        &self,
        renderer: &R,
        output: &Output,
        id: WindowId,
        clip: Rectangle<i32, Physical>,
        size: Size<i32, Logical>,
        alpha: f32,
    ) -> Option<TextureRenderElement<R::TextureId>>
    where
        R: Renderer,
        R::TextureId: Clone + Texture + 'static,
    {
        if !self.crt_on() {
            return None;
        }
        let glitched = self.crt.glitches.get(&(output.name(), id))?;
        let texture = as_texture::<R::TextureId>(&glitched.out)?;
        Some(TextureRenderElement::from_static_texture(
            glitched.id.clone(),
            renderer.context_id(),
            clip.loc.to_f64(),
            texture,
            1,
            Transform::Normal,
            (alpha < 1.0).then_some(alpha),
            Some(whole(&glitched.out)),
            Some(size),
            None,
            Kind::Unspecified,
        ))
    }

    /// The fullscreen overlay showing on `output` in the Matrix theme, if
    /// any, and how strong its scanlines and vignette are.
    pub(crate) fn crt_overlay(&self, output: &Output) -> Option<Overlay> {
        if !self.crt_on() {
            return None;
        }
        let now = self.clock.now();
        if self.lock_screen_on(output).is_some() {
            let t = self
                .lock_matrix_since()
                .map_or(f32::MAX, |s| millis(s, now));
            return Some(lock_overlay(t));
        }
        if let Some((since, _)) = self.unlock_matrix_on(output) {
            let t = millis(since, now);
            return Some(Overlay::new(SCAN, 1.0 - phase(t, 0.0, 200.0), 1.0));
        }
        if let Some((overview, shows)) = self.matrix_switcher_shows(output) {
            let scan = if overview { SCAN } else { SCAN_SWITCHER };
            return Some(Overlay::new(scan, shows, 1.0));
        }
        if let Some((_, leaving, _)) = self.boot_matrix_on(output) {
            // Gone with the flash.
            let gone = leaving.map_or(0.0, |l| phase(millis(l, now), 850.0, 1000.0));
            return Some(Overlay::new(SCAN, 1.0 - gone, 1.0));
        }
        None
    }

    /// The scanlines and the vignette over a fullscreen overlay on
    /// `output`, front first.
    pub(crate) fn crt_elements<R>(&self, renderer: &R, output: &Output) -> Vec<SceneElement<R>>
    where
        R: Renderer + smithay::backend::renderer::ImportAll + smithay::backend::renderer::ImportMem,
        R::TextureId: Clone + Texture + 'static,
    {
        let Some(overlay) = self.crt_overlay(output) else {
            return Vec::new();
        };
        let (Shaders::Ready(programs), Some(screen), Some(area)) = (
            &self.crt.shaders,
            self.crt.screens.get(&output.name()),
            self.space.output_geometry(output),
        ) else {
            return Vec::new();
        };
        let layer = |id: &Id, painted: &GlesTexture, alpha: f32| {
            let texture = as_texture::<R::TextureId>(painted)?;
            (alpha > 0.0).then(|| {
                SceneElement::Backdrop(TextureRenderElement::from_static_texture(
                    id.clone(),
                    renderer.context_id(),
                    Point::<f64, Physical>::from((0.0, 0.0)),
                    texture,
                    1,
                    Transform::Normal,
                    Some(alpha.min(1.0)),
                    // All of it, stretched: never repeated.
                    Some(whole(painted)),
                    Some(area.size),
                    None,
                    Kind::Unspecified,
                ))
            })
        };
        [
            layer(&screen.scan_id, &screen.scanlines, overlay.scan),
            layer(&screen.vignette_id, &programs.vignette, overlay.vignette),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rectangle<f64, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    #[test]
    fn a_bloom_surrounds_its_line_and_stretches_only_the_middle() {
        let line = rect(100.0, 50.0, 200.0, 1.0);
        let parts = bloom_parts(line);
        assert_eq!(parts.len(), 9);
        let around = parts
            .iter()
            .map(|(dst, _)| *dst)
            .reduce(|a, b| a.merge(b))
            .unwrap_or_default();
        assert_eq!(around, rect(97.0, 47.0, 206.0, 7.0));
        // The middle row's middle part: one texel of the line, stretched.
        let (dst, src) = parts[4];
        assert_eq!(dst, line);
        assert_eq!(src.size, (1.0, 1.0).into());
        assert!(src.loc.x > BLOOM_EDGE && src.loc.x + 1.0 < BLOOM_EDGE + 2.0 + 0.5);
        // Corners take the profile's corners, whole.
        assert_eq!(parts[0].1, rect(0.0, 0.0, BLOOM_EDGE, BLOOM_EDGE));
        assert_eq!(
            parts[8].1,
            rect(BLOOM_EDGE + 2.0, BLOOM_EDGE + 2.0, BLOOM_EDGE, BLOOM_EDGE)
        );
        assert_eq!(f64::from(BLOOM_SIZE), parts[8].1.loc.x + BLOOM_EDGE);
    }

    #[test]
    fn the_glitch_uniforms_are_fractions_of_the_picture() {
        let bands = [
            (0.0, 0.2, 4.0),
            (0.2, 0.5, -8.0),
            (0.5, 0.9, 0.0),
            (0.9, 1.0, 2.0),
        ];
        let (cuts, shifts, split) = glitch_uniforms(&bands, 3.0, 400.0);
        assert_eq!(cuts, [0.2, 0.5, 0.9]);
        assert_eq!(shifts, [0.01, -0.02, 0.0, 0.005]);
        assert_eq!(split, 0.0075);
    }

    /// A texture, its part, where it goes and how strongly.
    type Layer<'a> = (
        &'a GlesTexture,
        Rectangle<f64, Buffer>,
        Rectangle<i32, Physical>,
        f32,
    );

    /// The fastest of `runs` timings of `pass`, each waited for on the
    /// GPU, in milliseconds: what else runs on the machine only ever adds
    /// time to a pass, so the fastest is the closest to its own cost.
    fn fastest_ms(
        runs: usize,
        mut pass: impl FnMut() -> Result<SyncPoint, String>,
    ) -> Result<f64, String> {
        // Warm up: the driver compiles and allocates on first use.
        for _ in 0..3 {
            pass()?.wait().map_err(|e| format!("{e:?}"))?;
        }
        let mut times = Vec::with_capacity(runs);
        for _ in 0..runs {
            let start = std::time::Instant::now();
            pass()?.wait().map_err(|e| format!("{e:?}"))?;
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        Ok(times.into_iter().fold(f64::INFINITY, f64::min))
    }

    /// What a frame of 1920×1080 costs the GPU at most with the
    /// refinements: a window as large as the screen glitching, the
    /// scanlines and vignette over all of it, and the blooms of eight
    /// traces. Skipped without a GPU.
    ///
    /// A measure of wall-clock time: alongside the rest of the suite it
    /// measures the machine's load as much as the passes, and went over
    /// under it (#235). It runs alone, when asked:
    /// `cargo nextest run -p hocus-compositor --run-ignored only
    /// the_refinements_cost` (add `--release` for the 2 ms budget).
    #[test]
    #[ignore = "a timing: run alone, see its comment"]
    fn the_refinements_cost_less_than_2_ms_a_frame() -> Result<(), String> {
        let has_node = std::fs::read_dir("/dev/dri").is_ok_and(|entries| {
            entries
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with("renderD"))
        });
        if !has_node {
            eprintln!("skipped: no render node");
            return Ok(());
        }
        let mut renderer = match crate::headless::gles_renderer() {
            Ok(renderer) => renderer,
            Err(err) => {
                eprintln!("skipped: {err:#}");
                return Ok(());
            }
        };
        let programs = Programs::compile(&mut renderer)?;
        let (w, h) = (1920, 1080);
        let buffer = Size::<i32, Buffer>::from((w, h));
        let screen_size = Size::<i32, Physical>::from((w, h));
        let full = Rectangle::from_size(screen_size);
        let new = |renderer: &mut GlesRenderer| -> Result<GlesTexture, String> {
            renderer
                .create_buffer(Fourcc::Abgr8888, buffer)
                .map_err(|e| e.to_string())
        };
        let flat = painted(&mut renderer, &programs.paint, Painting::Vignette, buffer)?;
        let mut out = new(&mut renderer)?;
        let bands = [
            (0.0, 0.3, 5.0),
            (0.3, 0.5, -5.0),
            (0.5, 0.8, 2.0),
            (0.8, 1.0, -1.0),
        ];
        let uniforms = glitch_uniforms(&bands, 4.0, f64::from(w));
        let glitch = fastest_ms(21, || {
            glitch_pass(&mut renderer, &programs.glitch, &flat, &mut out, uniforms)
        })?;
        let scanlines = painted(
            &mut renderer,
            &programs.paint,
            Painting::Scanlines(1.0),
            Size::from((SCANLINES_WIDTH, h)),
        )?;
        let mut screen = new(&mut renderer)?;
        // Each as the scene draws it: stretched over the screen, blended.
        let draw = |renderer: &mut GlesRenderer,
                    screen: &mut GlesTexture,
                    layers: &[Layer<'_>]|
         -> Result<SyncPoint, String> {
            let mut target = renderer.bind(screen).map_err(|e| e.to_string())?;
            let mut frame = renderer
                .render(&mut target, screen_size, Transform::Normal)
                .map_err(|e| e.to_string())?;
            for (texture, src, dst, alpha) in layers {
                frame
                    .render_texture_from_to(
                        texture,
                        *src,
                        *dst,
                        &[*dst],
                        &[],
                        Transform::Normal,
                        *alpha,
                        None,
                        &[],
                    )
                    .map_err(|e| e.to_string())?;
            }
            frame.finish().map_err(|e| e.to_string())
        };
        let all_of = |t: &GlesTexture| Rectangle::from_size(t.size().to_f64());
        let overlay = fastest_ms(21, || {
            draw(
                &mut renderer,
                &mut screen,
                &[
                    (&scanlines, all_of(&scanlines), full, SCAN),
                    (
                        &programs.vignette,
                        all_of(&programs.vignette),
                        full,
                        VIGNETTE,
                    ),
                ],
            )
        })?;
        let mut parts = Vec::new();
        for i in 0..8 {
            let y = 100.0 + 100.0 * f64::from(i);
            for (dst, src) in bloom_parts(rect(100.0, y, 1500.0, 1.0)) {
                let dst = dst.to_physical_precise_round(1.0);
                let src = Rectangle::<f64, Buffer>::new(
                    (src.loc.x, src.loc.y).into(),
                    (src.size.w, src.size.h).into(),
                );
                parts.push((&programs.bloom, src, dst, 0.3));
            }
        }
        let bloom = fastest_ms(21, || draw(&mut renderer, &mut screen, &parts))?;
        let total = glitch + overlay + bloom;
        eprintln!(
            "GPU at 1920x1080: glitch {glitch:.3} ms, scanlines and vignette {overlay:.3} ms, \
             8 blooms {bloom:.3} ms, {total:.3} ms in all"
        );
        // The budget is a release build's; a debug build pays Rust's own
        // overhead on each draw, and other tests share the GPU.
        let budget = if cfg!(debug_assertions) { 4.0 } else { 2.0 };
        assert!(total < budget, "{total} ms a frame");
        Ok(())
    }

    #[test]
    fn the_lock_s_vignette_swells_once_then_rests() {
        let start = lock_overlay(0.0);
        assert_eq!(start.scan, 0.0, "nothing before the rain covers");
        let covered = lock_overlay(600.0);
        let rest = lock_overlay(1000.0);
        assert_eq!(covered.scan, SCAN);
        assert!(covered.vignette > rest.vignette, "{covered:?} {rest:?}");
        assert_eq!(rest, lock_overlay(5000.0), "still once in");
        assert!((rest.vignette - VIGNETTE).abs() < 1e-5, "{rest:?}");
    }
}
