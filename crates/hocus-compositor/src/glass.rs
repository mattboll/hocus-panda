//! The glass behind the bar (docs/design/bar.md §13: blur 26 px, saturation
//! 1.35): what is behind the bar, blurred by GLES shaders and drawn under
//! the shell's tint, cut to the bar's rounded corners. Ported from the
//! first build's frosted glass (`blur.rs` and its shaders at
//! `v0-first-build`, our own MIT code), for the bar alone, with the
//! saturation step added.
//!
//! When an output is drawn, what is behind the bar (everything under the
//! layer surfaces above the windows) goes into a texture of its own through
//! a damage tracker, over the full bar's place and the blur's reach around
//! it, whatever the form. When nothing there changed (the usual case: the
//! work area keeps windows away from the bar), nothing is drawn and the
//! last blur stays. Only when something did, the dual Kawase blur (Marius
//! Bjørge, "Bandwidth-efficient rendering", SIGGRAPH 2015) halves that
//! picture a few times and doubles it back up. Nothing is queued per frame;
//! the form switch only moves where the blur is cut.
//!
//! Pixman has no shaders: the bar is opaque there, as with reduced
//! transparency or a shell drawing on the CPU.

use std::{any::Any, collections::HashMap, fmt};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, Color32F, Frame, Offscreen, Renderer, Texture,
            damage::OutputDamageTracker,
            element::{
                Id, RenderElement,
                utils::{Relocate, RelocateRenderElement},
            },
            gles::{GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformName, UniformType},
            pixman::PixmanRenderer,
        },
    },
    utils::{Buffer, Physical, Point, Rectangle, Size, Transform},
};

/// How far the blur reaches, logical pixels: twice CSS's `blur(26px)`
/// standard deviation, where a Gaussian has faded out.
pub const REACH: f64 = 52.0;
/// CSS's `saturate(1.35)`.
const SATURATION: f32 = 1.35;

/// How many times the picture is halved, and how far apart the samples
/// are, for a blur that reaches `reach` pixels: each level doubles the
/// reach; the offset makes up the rest, between 1 and 2.
pub fn levels(reach: f64) -> (usize, f32) {
    let reach = reach.max(4.0);
    let n = ((reach / 2.0).log2().floor() as usize).clamp(1, 6);
    let offset = reach / 2f64.powi(n as i32 + 1);
    (n, offset.clamp(1.0, 2.0) as f32)
}

/// A blur done: its texture (the renderer's), its id (new each time it is
/// done again), the region of the output it covers (physical,
/// output-local) and the texture's size.
#[derive(Debug)]
pub struct Blurred<T> {
    pub texture: T,
    pub id: Id,
    pub region: Rectangle<i32, Physical>,
    pub size: Size<i32, Buffer>,
}

/// Each output's glass as last blurred, kept between frames.
#[derive(Default)]
pub struct Panes(HashMap<String, Box<dyn Any>>);

impl fmt::Debug for Panes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.0.keys()).finish()
    }
}

impl Panes {
    /// An output went, or the glass did: its textures go.
    pub fn forget(&mut self, output: &str) {
        self.0.remove(output);
    }
}

/// Renderers that can blur what is behind the bar.
pub trait Glass: Renderer + Sized {
    /// `behind` (output-local, as drawn on the output) drawn into output
    /// `name`'s pane over `region` where it changed, and blurred again if
    /// it did. `None`: no glass (the bar is opaque).
    fn frost<E: RenderElement<Self>>(
        &mut self,
        panes: &mut Panes,
        name: &str,
        region: Rectangle<i32, Physical>,
        scale: f64,
        behind: &[E],
    ) -> Option<Blurred<Self::TextureId>>;
}

impl Glass for PixmanRenderer {
    fn frost<E: RenderElement<Self>>(
        &mut self,
        _panes: &mut Panes,
        _name: &str,
        _region: Rectangle<i32, Physical>,
        _scale: f64,
        _behind: &[E],
    ) -> Option<Blurred<Self::TextureId>> {
        None
    }
}

/// The blur's programs, compiled once per GLES context; `None` when they
/// do not compile, and the bar is then opaque.
#[derive(Debug, Clone)]
struct Programs(Option<(GlesTexProgram, GlesTexProgram)>);

fn compile(renderer: &mut GlesRenderer) -> Result<(GlesTexProgram, GlesTexProgram), String> {
    let uniforms = [
        UniformName::new("half_pixel", UniformType::_2f),
        UniformName::new("half_source", UniformType::_2f),
        UniformName::new("offset", UniformType::_1f),
        UniformName::new("saturation", UniformType::_1f),
    ];
    let down = renderer
        .compile_custom_texture_shader(include_str!("shaders/blur_down.frag"), &uniforms)
        .map_err(|e| format!("down: {e}"))?;
    let up = renderer
        .compile_custom_texture_shader(include_str!("shaders/blur_up.frag"), &uniforms)
        .map_err(|e| format!("up: {e}"))?;
    Ok((down, up))
}

/// One output's pane: what is behind the bar, and its blur.
struct Frosted {
    region: Rectangle<i32, Physical>,
    scale: f64,
    tracker: OutputDamageTracker,
    behind: GlesTexture,
    /// Halves of `behind`; the first holds the blur once done.
    levels: Vec<GlesTexture>,
    offset: f32,
    id: Id,
    blurred: bool,
}

impl Frosted {
    fn new(
        renderer: &mut GlesRenderer,
        region: Rectangle<i32, Physical>,
        scale: f64,
    ) -> Result<Self, String> {
        let (n, offset) = levels(REACH * scale);
        let size = |w: i32, h: i32| Size::<i32, Buffer>::from((w.max(1), h.max(1)));
        let behind: GlesTexture = renderer
            .create_buffer(Fourcc::Abgr8888, size(region.size.w, region.size.h))
            .map_err(|e| e.to_string())?;
        let mut levels = Vec::with_capacity(n);
        let (mut w, mut h) = (region.size.w, region.size.h);
        for _ in 0..n {
            (w, h) = ((w + 1) / 2, (h + 1) / 2);
            let level: GlesTexture = renderer
                .create_buffer(Fourcc::Abgr8888, size(w, h))
                .map_err(|e| e.to_string())?;
            levels.push(level);
        }
        Ok(Self {
            region,
            scale,
            tracker: OutputDamageTracker::new(region.size, scale, Transform::Normal),
            behind,
            levels,
            offset,
            id: Id::new(),
            blurred: false,
        })
    }

    /// Halves and doubles back what is behind into the first level, the
    /// last pass saturating it.
    fn blur(
        &mut self,
        renderer: &mut GlesRenderer,
        (down, up): &(GlesTexProgram, GlesTexProgram),
    ) -> Result<(), String> {
        let mut source = self.behind.clone();
        for level in &mut self.levels {
            pass(renderer, down, &source, level, self.offset, 1.0)?;
            source = level.clone();
        }
        for i in (0..self.levels.len().saturating_sub(1)).rev() {
            let from = self.levels[i + 1].clone();
            let saturation = if i == 0 { SATURATION } else { 1.0 };
            pass(
                renderer,
                up,
                &from,
                &mut self.levels[i],
                self.offset,
                saturation,
            )?;
        }
        Ok(())
    }
}

/// Draws `from` over the whole of `to` with one of the blur's programs.
fn pass(
    renderer: &mut GlesRenderer,
    program: &GlesTexProgram,
    from: &GlesTexture,
    to: &mut GlesTexture,
    offset: f32,
    saturation: f32,
) -> Result<(), String> {
    let size = to.size();
    let source = from.size();
    let target_size = Size::<i32, Physical>::from((size.w, size.h));
    let dest = Rectangle::from_size(target_size);
    let half = |s: Size<i32, Buffer>| (0.5 / s.w.max(1) as f32, 0.5 / s.h.max(1) as f32);
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
            Rectangle::from_size(source.to_f64()),
            dest,
            &[dest],
            &[],
            Transform::Normal,
            1.0,
            Some(program),
            &[
                Uniform::new("half_pixel", half(size)),
                Uniform::new("half_source", half(source)),
                Uniform::new("offset", offset),
                Uniform::new("saturation", saturation),
            ],
        )
        .map_err(|e| e.to_string())?;
    // The same context draws the output next, in order: no need to wait
    // for this pass, which would hold the loop.
    let _done = frame.finish().map_err(|e| e.to_string())?;
    Ok(())
}

impl Glass for GlesRenderer {
    fn frost<E: RenderElement<Self>>(
        &mut self,
        panes: &mut Panes,
        name: &str,
        region: Rectangle<i32, Physical>,
        scale: f64,
        behind: &[E],
    ) -> Option<Blurred<GlesTexture>> {
        if self.egl_context().user_data().get::<Programs>().is_none() {
            let programs = compile(self)
                .map_err(|err| tracing::warn!(%err, "no blur shaders: the bar is opaque"))
                .ok();
            self.egl_context()
                .user_data()
                .insert_if_missing(|| Programs(programs));
        }
        let programs = self
            .egl_context()
            .user_data()
            .get::<Programs>()
            .and_then(|p| p.0.clone())?;
        let kept = panes
            .0
            .remove(name)
            .and_then(|b| b.downcast::<Frosted>().ok())
            .filter(|f| f.region == region && f.scale == scale);
        let mut frosted = match kept {
            Some(frosted) => *frosted,
            None => Frosted::new(self, region, scale)
                .map_err(|err| tracing::warn!(output = name, %err, "no glass"))
                .ok()?,
        };
        let shift = Point::from((-region.loc.x, -region.loc.y));
        let behind: Vec<_> = behind
            .iter()
            .map(|e| RelocateRenderElement::from_element(e, shift, Relocate::Relative))
            .collect();
        let changed = {
            let mut target = self
                .bind(&mut frosted.behind)
                .map_err(|err| tracing::warn!(output = name, %err, "no glass"))
                .ok()?;
            // The texture keeps its pixels: only what changed is drawn.
            match frosted.tracker.render_output(
                self,
                &mut target,
                1,
                &behind,
                crate::render::BACKGROUND,
            ) {
                Ok(result) => result.damage.is_some_and(|d| !d.is_empty()),
                Err(err) => {
                    tracing::warn!(output = name, ?err, "could not draw behind the bar");
                    return None;
                }
            }
        };
        if changed || !frosted.blurred {
            if let Err(err) = frosted.blur(self, &programs) {
                tracing::warn!(output = name, %err, "could not blur");
                return None;
            }
            frosted.blurred = true;
            frosted.id = Id::new();
        }
        let level = frosted.levels.first()?.clone();
        let blurred = Blurred {
            size: level.size(),
            texture: level,
            id: frosted.id.clone(),
            region,
        };
        panes.0.insert(name.to_owned(), Box::new(frosted));
        Some(blurred)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blur_reaches_as_far_as_asked() {
        for reach in [8.0, 24.0, 52.0, 64.0, 104.0] {
            let (n, offset) = levels(reach);
            let got = f64::from(offset) * 2f64.powi(n as i32 + 1);
            assert!((1.0..=2.0).contains(&offset), "{reach}: {offset}");
            assert!(
                (got - reach).abs() <= reach * 0.5,
                "{reach}: {n} levels, {offset}"
            );
        }
        assert_eq!(levels(0.0).0, 1);
        assert_eq!(levels(52.0).0, 4);
    }
}
