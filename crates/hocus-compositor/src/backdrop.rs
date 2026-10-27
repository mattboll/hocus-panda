//! Wallpapers that move (#108): the Matrix rain and a starfield, drawn by
//! GLES shaders on the frame clock of their output.
//!
//! A GLES backend paints each moving wallpaper into a texture of its
//! output's size before drawing the output (`paint_backdrop`); the scene,
//! whatever its renderer, shows that texture in place of the still
//! picture. The still picture, made on the wallpapers' worker like any
//! other, stays for every case where nothing should move: reduced motion,
//! the power saver, a fullscreen window or the lock over it, and the
//! pixman renderer, which has no shaders (and keeps scenarios
//! deterministic). Only while a wallpaper moves does the loop ask for
//! frames; nothing is queued per frame.

use std::{any::Any, collections::BTreeMap};

use hocus_shell::{matrix::Atlas, wallpaper};
use hocus_ui::ThemeName;
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, Frame, ImportMem, Offscreen, Renderer, Texture,
            element::{Id, Kind, texture::TextureRenderElement},
            gles::{
                GlesPixelProgram, GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformName,
                UniformType,
            },
        },
    },
    output::Output,
    utils::{Buffer, Logical, Physical, Point, Rectangle, Size, Transform},
};

use crate::State;

/// What a moving wallpaper shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moving {
    Rain,
    Stars,
}

impl Moving {
    fn of(kind: &str) -> Option<Self> {
        match kind {
            "rain" => Some(Self::Rain),
            "stars" => Some(Self::Stars),
            _ => None,
        }
    }
}

/// The shaders, once compiled, or why there are none.
#[derive(Debug, Default)]
enum Shaders {
    /// No GLES backend has painted yet.
    #[default]
    Unknown,
    Ready(Box<Programs>),
    /// They did not compile: still pictures only.
    Broken,
}

#[derive(Debug)]
struct Programs {
    rain: GlesTexProgram,
    stars: GlesPixelProgram,
    /// The rain's glyphs, per scale.
    atlases: BTreeMap<u32, (GlesTexture, Atlas)>,
}

impl Programs {
    fn compile(renderer: &mut GlesRenderer) -> Result<Self, String> {
        let common = [
            UniformName::new("screen", UniformType::_2f),
            UniformName::new("time", UniformType::_1f),
            UniformName::new("sun", UniformType::_2f),
        ];
        let mut rain_uniforms = common.to_vec();
        rain_uniforms.extend([
            UniformName::new("cells", UniformType::_2f),
            UniformName::new("cell_px", UniformType::_2f),
            UniformName::new("glyph_px", UniformType::_2f),
            UniformName::new("atlas_px", UniformType::_2f),
        ]);
        let rain = renderer
            .compile_custom_texture_shader(include_str!("shaders/rain.frag"), &rain_uniforms)
            .map_err(|e| format!("rain: {e}"))?;
        let stars = renderer
            .compile_custom_pixel_shader(include_str!("shaders/stars.frag"), &common)
            .map_err(|e| format!("stars: {e}"))?;
        Ok(Self {
            rain,
            stars,
            atlases: BTreeMap::new(),
        })
    }

    fn atlas(
        &mut self,
        renderer: &mut GlesRenderer,
        scale: f64,
    ) -> Result<(GlesTexture, Atlas), String> {
        let key = (scale * 1000.0).round() as u32;
        if let Some(found) = self.atlases.get(&key) {
            return Ok(found.clone());
        }
        let atlas = Atlas::new(scale as f32);
        let texture = renderer
            .import_memory(
                &atlas.pixels,
                Fourcc::Abgr8888,
                (atlas.width as i32, atlas.height as i32).into(),
                false,
            )
            .map_err(|e| e.to_string())?;
        self.atlases.insert(key, (texture.clone(), atlas.clone()));
        Ok((texture, atlas))
    }
}

/// An output's last painted frame.
#[derive(Debug)]
struct Painted {
    texture: GlesTexture,
    size: Size<i32, Physical>,
}

#[derive(Debug, Default)]
pub struct Backdrops {
    shaders: Shaders,
    painted: BTreeMap<String, Painted>,
}

/// The shaders' time: seconds on the monotonic clock, wrapped every hour
/// so that it keeps its precision as a float (the rain rearranges once).
fn shader_time(now: std::time::Duration) -> f32 {
    (now.as_secs_f64() % 3600.0) as f32
}

impl State {
    /// What `output`'s wallpaper would show moving, if nothing asks it to
    /// rest: an animated kind, seen, motion allowed and the power to spare.
    fn backdrop_wanted(&self, output: &Output) -> Option<Moving> {
        // Wallpapers come with the shell.
        self.wallpapers.as_ref()?;
        let kind = &self.config.wallpaper.for_output(&output.name()).kind;
        // The Matrix theme's own wallpaper is its rain, falling.
        let matrix = kind == "glow"
            && self.config.appearance.theme.parse::<ThemeName>() == Ok(ThemeName::Matrix);
        let moving = if matrix {
            Moving::Rain
        } else {
            Moving::of(kind)?
        };
        let rest = self.config.appearance.reduced_motion
            || self.power_saver()
            || self.locked()
            || self.fullscreen_on(output);
        (!rest).then_some(moving)
    }

    /// What `output`'s wallpaper shows moving now: only where shaders
    /// draw it.
    pub fn backdrop_moving(&self, output: &Output) -> Option<Moving> {
        match self.backdrops.shaders {
            Shaders::Ready(_) => self.backdrop_wanted(output),
            Shaders::Unknown | Shaders::Broken => None,
        }
    }

    /// Whether a wallpaper moves on some output: frames keep coming.
    pub fn backdrops_moving(&self) -> bool {
        self.space
            .outputs()
            .any(|o| self.backdrop_moving(o).is_some())
    }

    /// Before a GLES backend draws `output`: its moving wallpaper, painted
    /// at the clock's time; or nothing, and the still picture shows.
    pub fn paint_backdrop(&mut self, renderer: &mut GlesRenderer, output: &Output) {
        let name = output.name();
        let Some(moving) = self.backdrop_wanted(output) else {
            self.backdrops.painted.remove(&name);
            return;
        };
        if matches!(self.backdrops.shaders, Shaders::Unknown) {
            self.backdrops.shaders = match Programs::compile(renderer) {
                Ok(programs) => Shaders::Ready(Box::new(programs)),
                Err(err) => {
                    tracing::warn!(%err, "wallpapers that move are still: no shaders");
                    Shaders::Broken
                }
            };
            // Frames start.
            self.redraw.all();
        }
        if let Err(err) = self.paint(renderer, output, moving) {
            tracing::warn!(output = name, %err, "could not paint the wallpaper");
            self.backdrops.painted.remove(&name);
        }
    }

    fn paint(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
        moving: Moving,
    ) -> Result<(), String> {
        let Some(mode) = output.current_mode() else {
            return Ok(());
        };
        let size = mode.size;
        let scale = output.current_scale().fractional_scale();
        let logical = size.to_f64().to_logical(scale);
        let (sx, sy, _) = wallpaper::sun(self.wall_minutes() as f32 / 60.0);
        let time = shader_time(self.clock.now());
        let Shaders::Ready(programs) = &mut self.backdrops.shaders else {
            return Ok(());
        };
        let atlas = match moving {
            Moving::Rain => Some(programs.atlas(renderer, scale)?),
            Moving::Stars => None,
        };
        let name = output.name();
        let fresh = self
            .backdrops
            .painted
            .get(&name)
            .is_none_or(|p| p.size != size);
        if fresh {
            let texture: GlesTexture = renderer
                .create_buffer(
                    Fourcc::Abgr8888,
                    size.to_logical(1).to_buffer(1, Transform::Normal),
                )
                .map_err(|e| e.to_string())?;
            self.backdrops
                .painted
                .insert(name.clone(), Painted { texture, size });
        }
        let (Shaders::Ready(programs), Some(painted)) = (
            &self.backdrops.shaders,
            self.backdrops.painted.get_mut(&name),
        ) else {
            return Ok(());
        };
        let common = [
            Uniform::new("screen", (logical.w as f32, logical.h as f32)),
            Uniform::new("time", time),
            Uniform::new("sun", (sx, sy)),
        ];
        let dest = Rectangle::<i32, Physical>::from_size(size);
        let mut target = renderer
            .bind(&mut painted.texture)
            .map_err(|e| e.to_string())?;
        let mut frame = renderer
            .render(&mut target, size, Transform::Normal)
            .map_err(|e| e.to_string())?;
        let drawn = match (moving, atlas) {
            (Moving::Rain, Some((texture, atlas))) => {
                let mut uniforms = common.to_vec();
                uniforms.extend([
                    Uniform::new("cells", (atlas.cols as f32, atlas.rows as f32)),
                    Uniform::new("cell_px", (atlas.cell.0 as f32, atlas.cell.1 as f32)),
                    Uniform::new("glyph_px", (atlas.glyph.0 as f32, atlas.glyph.1 as f32)),
                    Uniform::new("atlas_px", (atlas.width as f32, atlas.height as f32)),
                ]);
                let src = Rectangle::<f64, Buffer>::from_size(texture.size().to_f64());
                frame.render_texture_from_to(
                    &texture,
                    src,
                    dest,
                    &[dest],
                    &[],
                    Transform::Normal,
                    1.0,
                    Some(&programs.rain),
                    &uniforms,
                )
            }
            _ => frame.render_pixel_shader_to(
                &programs.stars,
                Rectangle::from_size(size.to_logical(1).to_buffer(1, Transform::Normal).to_f64()),
                dest,
                size.to_logical(1).to_buffer(1, Transform::Normal),
                None,
                1.0,
                &common,
            ),
        };
        drawn.map_err(|e| e.to_string())?;
        // The same context draws the output next, in order: no need to
        // wait for this frame (which would hold the loop).
        let _done = frame.finish().map_err(|e| e.to_string())?;
        Ok(())
    }

    /// `output`'s wallpaper moving, as painted last, for any renderer whose
    /// textures are the painter's.
    pub fn backdrop_element<R>(
        &self,
        renderer: &R,
        output: &Output,
    ) -> Option<TextureRenderElement<R::TextureId>>
    where
        R: Renderer,
        R::TextureId: Clone + Texture + 'static,
    {
        self.backdrop_moving(output)?;
        let painted = self.backdrops.painted.get(&output.name())?;
        let texture = (&painted.texture as &dyn Any).downcast_ref::<R::TextureId>()?;
        let area = self.space.output_geometry(output)?;
        let size: Size<i32, Logical> = area.size;
        // A new picture every frame: all of it changes.
        Some(TextureRenderElement::from_static_texture(
            Id::new(),
            renderer.context_id(),
            Point::<f64, Physical>::from((0.0, 0.0)),
            texture.clone(),
            1,
            Transform::Normal,
            None,
            Some(Rectangle::from_size(painted.size.to_f64().to_logical(1.0))),
            Some(size),
            Some(vec![Rectangle::from_size(
                painted.size.to_logical(1).to_buffer(1, Transform::Normal),
            )]),
            Kind::Unspecified,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shaders_time_keeps_its_precision() {
        let late =
            std::time::Duration::from_secs(30 * 24 * 3600) + std::time::Duration::from_millis(16);
        let t = shader_time(late);
        assert!(t < 3600.0);
        assert!((t - 0.016).abs() < 1e-3, "{t}");
    }

    #[test]
    fn only_animated_kinds_move() {
        assert_eq!(Moving::of("rain"), Some(Moving::Rain));
        assert_eq!(Moving::of("stars"), Some(Moving::Stars));
        assert_eq!(Moving::of("glow"), None);
    }
}
