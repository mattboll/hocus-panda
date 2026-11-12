//! How the effects are drawn (#793): through GLES programs of their own,
//! made once a context, as the glyphs' ([`crate::matrix`], whose program
//! helpers they share). A window's effect draws its last picture, drawn
//! once into a texture, through a shader over the window and a margin
//! around it ([`Fire::burn`]); the particles are one batch of quads a
//! blend (smoke over, embers added); a light is a radial gradient screened
//! over what is under it. Pixman draws none of them: the effects do not
//! start there.
//!
//! The flames' shader is the board's (`FLAME_FS` of "Fermer en flammes"),
//! from WebGL into GLSL ES 1.00 as it was: its lengths in logical pixels.

use std::{any::Any, rc::Rc};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Offscreen, Renderer,
            element::{Element, Id, Kind, RenderElement},
            gles::{Capability, GlesError, GlesFrame, GlesRenderer, GlesTexture, ffi},
            pixman::PixmanRenderer,
            utils::{CommitCounter, DamageSet, OpaqueRegions},
        },
    },
    utils::{Buffer as BufferCoords, Physical, Point, Rectangle, Scale, Transform},
};

use super::{flames::Burn, particles::FLOATS};
use crate::{
    matrix::{Program, bind, compile_with, quads},
    wobble::Picture,
};

/// An effect's part drawn this frame.
#[derive(Debug)]
pub struct FireElement {
    id: Id,
    commit: CommitCounter,
    /// Physical, output-local.
    geometry: Rectangle<i32, Physical>,
    what: What,
}

#[derive(Debug)]
enum What {
    /// A window's picture through the flames.
    Burn {
        texture: Rc<dyn Any>,
        look: BurnLook,
    },
    /// A warm light, screened over what is under it.
    Light(LightLook),
    /// Particles: the smoke's quads and the embers', as
    /// [`super::particles::vertices`] lays them.
    Sparks { smoke: Vec<f32>, embers: Vec<f32> },
}

/// What the flames' shader is told.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BurnLook {
    /// The window's size, logical.
    pub size: [f32; 2],
    /// The element's corner from the window's, logical (negative).
    pub corner: [f32; 2],
    /// Physical pixels a logical one.
    pub scale: f32,
    /// The window's corners' radius, logical.
    pub radius: f32,
    /// The burn's front, from 0 to 1 ([`Burn::front`]).
    pub front: f32,
    /// Its clock, seconds.
    pub time: f32,
}

/// A warm light: its centre and radii, element-local and physical, and
/// its strength (the fire's, flickering).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightLook {
    pub centre: [f32; 2],
    pub inner: f32,
    pub outer: f32,
    pub strength: f32,
}

impl FireElement {
    /// A light over `geometry` (output-local, physical).
    pub fn light(
        id: Id,
        commit: CommitCounter,
        geometry: Rectangle<i32, Physical>,
        look: LightLook,
    ) -> Self {
        Self {
            id,
            commit,
            geometry,
            what: What::Light(look),
        }
    }

    /// Particles within `geometry` (output-local, physical), their quads
    /// laid out output-local.
    pub fn sparks(
        id: Id,
        commit: CommitCounter,
        geometry: Rectangle<i32, Physical>,
        smoke: Vec<f32>,
        embers: Vec<f32>,
    ) -> Self {
        Self {
            id,
            commit,
            geometry,
            what: What::Sparks { smoke, embers },
        }
    }
}

/// Renderers that draw the effects.
pub trait Fire: Renderer + Sized {
    /// Draws `parts` (output-local, as if the picture's corner were at
    /// theirs) into `burn`'s picture the first time, and gives the element
    /// drawing it through the flames over `geometry` (output-local,
    /// physical). `None`: not drawn so.
    fn burn<E: RenderElement<Self>>(
        &mut self,
        burn: &Burn,
        picture: &Picture,
        parts: &[E],
        geometry: Rectangle<i32, Physical>,
        look: BurnLook,
    ) -> Option<FireElement>;

    fn draw_fire(
        frame: &mut Self::Frame<'_, '_>,
        element: &FireElement,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
    ) -> Result<(), Self::Error>;
}

impl Fire for PixmanRenderer {
    fn burn<E: RenderElement<Self>>(
        &mut self,
        _burn: &Burn,
        _picture: &Picture,
        _parts: &[E],
        _geometry: Rectangle<i32, Physical>,
        _look: BurnLook,
    ) -> Option<FireElement> {
        None
    }

    fn draw_fire(
        _frame: &mut Self::Frame<'_, '_>,
        _element: &FireElement,
        _dst: Rectangle<i32, Physical>,
        _damage: &[Rectangle<i32, Physical>],
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// The board's `FLAME_FS`: the burn field (fbm noise rising from the
/// bottom against the front), the paper scorching ahead of it with a
/// heat wobble and a glowing, flickering rim, charred flakes with sparks
/// behind it, flames rising over the paper still there and up to 120 px
/// above the window, and a soft shadow fading out early in the burn.
const BURN: &str = "#version 100
precision highp float;
uniform sampler2D tex;
uniform vec2 size;
uniform vec2 corner;
uniform float px;
uniform float radius;
uniform float t;
uniform float time;
varying vec2 v_px;

float hash(vec2 p) { p = fract(p * vec2(123.34, 456.21)); p += dot(p, p + 45.32); return fract(p.x * p.y); }
float noise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    vec2 u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x), u.y);
}
float fbm(vec2 p) {
    float v = 0.0;
    float a = 0.5;
    for (int i = 0; i < 5; i++) { v += a * noise(p); p = p * 2.03 + 17.1; a *= 0.5; }
    return v;
}
float sdBox(vec2 p, vec2 b, float r) { vec2 q = abs(p) - b + r; return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r; }
float burnField(vec2 uv) {
    float n = fbm(uv * vec2(3.2, 2.6) + vec2(0.0, time * 0.06));
    float fine = fbm(uv * 14.0 + 3.7) * 0.10;
    float front = t * 1.55 - 0.25;
    return (1.0 - uv.y) + (n - 0.5) * 0.6 + fine - front;
}
vec3 fireRamp(float x) {
    x = clamp(x, 0.0, 1.0);
    vec3 c = mix(vec3(0.0), vec3(0.75, 0.12, 0.02), smoothstep(0.0, 0.35, x));
    c = mix(c, vec3(1.0, 0.48, 0.08), smoothstep(0.3, 0.65, x));
    c = mix(c, vec3(1.0, 0.86, 0.45), smoothstep(0.6, 0.9, x));
    return mix(c, vec3(1.0, 0.98, 0.9), smoothstep(0.9, 1.0, x));
}
void main() {
    vec2 p = v_px / px + corner;
    vec2 uv = p / size;
    float sd = sdBox(p - size * 0.5, size * 0.5, radius);
    float inside = 1.0 - smoothstep(-0.75, 0.75, sd);
    vec4 col = vec4(0.0);
    float burning = step(0.001, t);
    float b = burnField(vec2(uv.x, max(uv.y, 0.0)));
    float above = max(-uv.y, 0.0);
    if (inside > 0.0 && b > 0.0) {
        float heat = smoothstep(0.22, 0.0, b) * burning;
        vec2 wob = vec2(fbm(uv * 9.0 + vec2(0.0, -time * 2.4)) - 0.5, fbm(uv * 9.0 + vec2(5.2, -time * 2.1)) - 0.5);
        vec4 tx = texture2D(tex, clamp(uv + wob * 0.012 * heat, 0.0, 1.0));
        vec3 paper = tx.rgb;
        float scorch = smoothstep(0.09, 0.0, b) * burning;
        paper = mix(paper, vec3(0.10, 0.05, 0.02), scorch * 0.92);
        paper *= 1.0 - smoothstep(0.16, 0.02, b) * 0.25 * burning;
        float rim = exp(-b * 70.0) * burning;
        float flick = 0.75 + 0.25 * noise(uv * 40.0 + time * 6.0);
        vec3 glow = fireRamp(rim * flick) * rim * 2.2;
        float a = inside * tx.a;
        col = vec4(paper * a + glow * inside, a);
    } else if (inside > 0.0 && b > -0.07 && burning > 0.0) {
        float charA = smoothstep(-0.07, 0.0, b);
        float spark = step(0.83, noise(uv * 90.0 + time * 3.0)) * charA;
        vec3 c = vec3(0.05, 0.03, 0.02) + vec3(1.0, 0.45, 0.1) * spark * 1.6;
        col = vec4(c * charA * inside, charA * inside * 0.9);
    }
    float dUp = b + above;
    if (burning > 0.0 && t < 0.995 && dUp > -0.02) {
        vec2 q = vec2(uv.x * 6.0, uv.y * 2.8 + time * 2.2);
        float n = fbm(q) * 0.8 + fbm(q * 2.4 + 9.0) * 0.4;
        float h = 0.16 + 0.12 * noise(vec2(uv.x * 5.0, time * 0.9));
        float k = dUp / h;
        float f = smoothstep(1.0, 0.0, k + (0.52 - n) * 1.5) * smoothstep(-0.02, 0.01, dUp);
        f = pow(f, 1.7);
        float sdUp = sdBox(p - vec2(size.x * 0.5, size.y * 0.5 - 60.0), vec2(size.x * 0.5, size.y * 0.5 + 60.0), radius);
        float side = 1.0 - smoothstep(-8.0, 14.0, sdUp);
        float live = smoothstep(1.0, 0.82, t) * smoothstep(0.0, 0.04, t);
        float plume = clamp(f * side * live, 0.0, 1.0);
        vec3 fc = fireRamp(plume * 0.92 + 0.08 * exp(-dUp * 40.0)) * 1.05;
        col.rgb = col.rgb * (1.0 - plume * 0.6) + fc * plume;
        col.a = max(col.a, plume * 0.8);
    }
    if (sd > 0.0) {
        float sh = (1.0 - smoothstep(0.0, 46.0, sd)) * 0.42 * (1.0 - smoothstep(0.0, 0.22, t));
        sh *= smoothstep(-20.0, 30.0, p.y);
        col.a = max(col.a, sh * (1.0 - col.a) + col.a);
    }
    gl_FragColor = col;
}
";

/// The board's warm light: a radial gradient from its centre (10 px) to
/// its reach (620 px), orange to red to nothing, its colours mixed
/// premultiplied as the browser's canvas does; screened over what is under
/// it (`ONE, ONE_MINUS_SRC_COLOR`). In high precision: Mesa's software
/// device takes medium as 16 bits, whose squares overflow past 256 px.
const LIGHT: &str = "#version 100
precision highp float;
uniform vec2 centre;
uniform float inner;
uniform float outer;
uniform float strength;
varying vec2 v_px;

void main() {
    float s = clamp((length(v_px - centre) - inner) / (outer - inner), 0.0, 1.0);
    vec4 a = vec4(1.0, 140.0 / 255.0, 50.0 / 255.0, 1.0) * 0.55;
    vec4 b = vec4(200.0 / 255.0, 70.0 / 255.0, 20.0 / 255.0, 1.0) * 0.20;
    vec4 c = s < 0.35 ? mix(a, b, s / 0.35) : mix(b, vec4(0.0), (s - 0.35) / 0.65);
    gl_FragColor = c * strength;
}
";

/// The particles' quads: their place, their place across the quad, their
/// colour and their shape, as [`super::particles::vertices`] lays them.
const SPARKS_VERTEX: &str = "#version 100
uniform mat3 matrix;
attribute vec2 position;
attribute vec2 local;
attribute vec4 colour;
attribute vec4 shape;
varying vec2 v_local;
varying vec4 v_colour;
varying vec4 v_shape;
void main() {
    gl_Position = vec4(matrix * vec3(position, 1.0), 1.0);
    v_local = local;
    v_colour = colour;
    v_shape = shape;
}
";

/// A puff of smoke: its colour (premultiplied) fading to nothing at its
/// edge. An ember: its halo (the board's 64 px gradient, warm orange to
/// red to nothing) and its dot of `heat`, both as opaque as it is young,
/// added to what is under it.
const SPARKS: &str = "#version 100
precision highp float;
varying vec2 v_local;
varying vec4 v_colour;
varying vec4 v_shape;

void main() {
    float d = length(v_local);
    if (d >= 1.0) {
        gl_FragColor = vec4(0.0);
        return;
    }
    if (v_shape.x < 0.5) {
        gl_FragColor = v_colour * (1.0 - d);
        return;
    }
    vec4 h0 = vec4(1.0, 150.0 / 255.0, 60.0 / 255.0, 1.0) * 0.55;
    vec4 h1 = vec4(1.0, 90.0 / 255.0, 30.0 / 255.0, 1.0) * 0.18;
    vec4 halo = d < 0.35 ? mix(h0, h1, d / 0.35) : mix(h1, vec4(0.0), (d - 0.35) / 0.65);
    float a = v_colour.a;
    float cover = clamp((v_shape.y - d) * v_shape.z + 0.5, 0.0, 1.0);
    gl_FragColor = halo * a + vec4(v_colour.rgb, 1.0) * a * cover;
}
";

/// The effects' programs of a renderer's context, made once; `None` where
/// they do not compile, and the effects do not play.
#[derive(Debug)]
struct Made(Option<Programs>);

#[derive(Debug, Clone)]
struct Programs {
    burn: Program,
    light: Program,
    sparks: Program,
    /// The sparks' other attributes.
    colour: ffi::types::GLint,
    shape: ffi::types::GLint,
}

fn programs(renderer: &mut GlesRenderer) -> Option<Programs> {
    if renderer.egl_context().user_data().get::<Made>().is_none() {
        let made = make(renderer)
            .map_err(|err| tracing::warn!(%err, "the effects' shaders do not compile: none plays"))
            .ok();
        renderer
            .egl_context()
            .user_data()
            .insert_if_missing(|| Made(made));
    }
    renderer
        .egl_context()
        .user_data()
        .get::<Made>()
        .and_then(|m| m.0.clone())
}

fn make(renderer: &mut GlesRenderer) -> Result<Programs, String> {
    #[allow(unsafe_code)]
    // SAFETY: `with_context` makes the renderer's context current.
    renderer
        .with_context(|gl| unsafe {
            let burn = compile_with(
                gl,
                crate::matrix::VERTEX,
                BURN,
                &[
                    "matrix", "tex", "size", "corner", "px", "radius", "t", "time",
                ],
            )?;
            let light = compile_with(
                gl,
                crate::matrix::VERTEX,
                LIGHT,
                &["matrix", "centre", "inner", "outer", "strength"],
            )?;
            let sparks = compile_with(gl, SPARKS_VERTEX, SPARKS, &["matrix"])?;
            let attrib = |n: &std::ffi::CStr| gl.GetAttribLocation(sparks.program, n.as_ptr());
            let (colour, shape) = (attrib(c"colour"), attrib(c"shape"));
            if colour < 0 || shape < 0 || sparks.position < 0 || sparks.local < 0 {
                return Err("the particles' attributes are missing".to_owned());
            }
            Ok(Programs {
                burn,
                light,
                sparks,
                colour,
                shape,
            })
        })
        .map_err(|e| e.to_string())?
}

impl Fire for GlesRenderer {
    fn burn<E: RenderElement<Self>>(
        &mut self,
        burn: &Burn,
        picture: &Picture,
        parts: &[E],
        geometry: Rectangle<i32, Physical>,
        look: BurnLook,
    ) -> Option<FireElement> {
        programs(self)?;
        let size = picture.buffer_size();
        let kept = {
            let mut kept = burn.picture.borrow_mut();
            match kept.as_ref().filter(|(s, _)| *s == size) {
                Some((_, texture)) => texture.clone(),
                None => {
                    let mut texture: GlesTexture =
                        Offscreen::<GlesTexture>::create_buffer(self, Fourcc::Abgr8888, size)
                            .map_err(|err| tracing::warn!(%err, "no picture for a burning window"))
                            .ok()?;
                    if let Err(err) =
                        crate::wobble::draw_picture(self, &mut texture, picture, parts)
                    {
                        tracing::warn!(%err, "could not draw a burning window's picture");
                        return None;
                    }
                    let rc: Rc<dyn Any> = Rc::new(texture);
                    *kept = Some((size, rc.clone()));
                    rc
                }
            }
        };
        Some(FireElement {
            id: burn.ids[0].clone(),
            commit: burn.next_commit(),
            geometry,
            what: What::Burn {
                texture: kept,
                look,
            },
        })
    }

    fn draw_fire(
        frame: &mut GlesFrame<'_, '_>,
        element: &FireElement,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        let Some(programs) = frame
            .egl_context()
            .user_data()
            .get::<Made>()
            .and_then(|m| m.0.clone())
        else {
            return Ok(());
        };
        let matrix = crate::wobble::placed(frame.projection(), element.geometry, dst);
        let instancing = frame.capabilities().contains(&Capability::Instancing);
        let corner = element.geometry.loc;
        match &element.what {
            What::Burn { texture, look } => {
                let Some(texture) = texture.downcast_ref::<GlesTexture>() else {
                    return Ok(());
                };
                let tex = texture.tex_id();
                let p = &programs.burn;
                let look = *look;
                frame.with_context(|gl| {
                    #[allow(unsafe_code)]
                    // SAFETY: the frame's context is current.
                    unsafe {
                        quads(gl, p, &matrix, instancing, damage, corner, |gl| {
                            bind(gl, 0, tex, false);
                            gl.Uniform1i(p.at("tex"), 0);
                            gl.Uniform2f(p.at("size"), look.size[0], look.size[1]);
                            gl.Uniform2f(p.at("corner"), look.corner[0], look.corner[1]);
                            gl.Uniform1f(p.at("px"), look.scale);
                            gl.Uniform1f(p.at("radius"), look.radius);
                            gl.Uniform1f(p.at("t"), look.front);
                            gl.Uniform1f(p.at("time"), look.time);
                        });
                    }
                })
            }
            What::Light(look) => {
                let p = &programs.light;
                let look = *look;
                frame.with_context(|gl| {
                    #[allow(unsafe_code)]
                    // SAFETY: the frame's context is current; the blending
                    // is given back as smithay sets it.
                    unsafe {
                        quads(gl, p, &matrix, instancing, damage, corner, |gl| {
                            gl.BlendFunc(ffi::ONE, ffi::ONE_MINUS_SRC_COLOR);
                            gl.Uniform2f(p.at("centre"), look.centre[0], look.centre[1]);
                            gl.Uniform1f(p.at("inner"), look.inner);
                            gl.Uniform1f(p.at("outer"), look.outer);
                            gl.Uniform1f(p.at("strength"), look.strength);
                        });
                        gl.BlendFunc(ffi::ONE, ffi::ONE_MINUS_SRC_ALPHA);
                    }
                })
            }
            What::Sparks { smoke, embers } => frame.with_context(|gl| {
                #[allow(unsafe_code)]
                // SAFETY: the frame's context is current.
                unsafe {
                    sparks(gl, &programs, &matrix, instancing, smoke, embers);
                }
            }),
        }
    }
}

/// Draws the smoke's quads over, then the embers' added.
///
/// # Safety
///
/// The context must be current.
#[allow(unsafe_code)]
unsafe fn sparks(
    gl: &ffi::Gles2,
    programs: &Programs,
    matrix: &[f32; 9],
    instancing: bool,
    smoke: &[f32],
    embers: &[f32],
) {
    let p = &programs.sparks;
    let attribs = [
        (p.position as u32, 2, 0),
        (p.local as u32, 2, 2),
        (programs.colour as u32, 4, 4),
        (programs.shape as u32, 4, 8),
    ];
    let stride = (FLOATS * std::mem::size_of::<f32>()) as ffi::types::GLsizei;
    // SAFETY: the context is current; the vertices outlive each draw call,
    // every attribute enabled here is disabled again and the blending
    // given back, as smithay's own draws expect.
    unsafe {
        gl.UseProgram(p.program);
        gl.UniformMatrix3fv(p.at("matrix"), 1, ffi::FALSE, matrix.as_ptr());
        gl.BindBuffer(ffi::ARRAY_BUFFER, 0);
        for (at, _, _) in attribs {
            gl.EnableVertexAttribArray(at);
            if instancing {
                gl.VertexAttribDivisor(at, 0);
            }
        }
        for (vertices, added) in [(smoke, false), (embers, true)] {
            let n = vertices.len() / FLOATS;
            if n == 0 {
                continue;
            }
            if added {
                gl.BlendFunc(ffi::ONE, ffi::ONE);
            }
            for (at, size, offset) in attribs {
                gl.VertexAttribPointer(
                    at,
                    size,
                    ffi::FLOAT,
                    ffi::FALSE,
                    stride,
                    vertices.as_ptr().add(offset) as *const _,
                );
            }
            gl.DrawArrays(ffi::TRIANGLES, 0, n as ffi::types::GLsizei);
        }
        gl.BlendFunc(ffi::ONE, ffi::ONE_MINUS_SRC_ALPHA);
        for (at, _, _) in attribs {
            gl.DisableVertexAttribArray(at);
        }
    }
}

impl Element for FireElement {
    fn id(&self) -> &Id {
        &self.id
    }

    fn current_commit(&self) -> CommitCounter {
        self.commit
    }

    fn src(&self) -> Rectangle<f64, BufferCoords> {
        Rectangle::from_size((1.0, 1.0).into())
    }

    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.geometry
    }

    fn location(&self, _scale: Scale<f64>) -> Point<i32, Physical> {
        self.geometry.loc
    }

    fn transform(&self) -> Transform {
        Transform::Normal
    }

    fn damage_since(
        &self,
        _scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        if commit == Some(self.commit) {
            DamageSet::default()
        } else {
            DamageSet::from_slice(&[Rectangle::from_size(self.geometry.size)])
        }
    }

    fn opaque_regions(&self, _scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        OpaqueRegions::default()
    }

    fn alpha(&self) -> f32 {
        1.0
    }

    fn kind(&self) -> Kind {
        Kind::Unspecified
    }
}

impl<R: Fire> RenderElement<R> for FireElement {
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        _src: Rectangle<f64, BufferCoords>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), R::Error> {
        R::draw_fire(frame, self, dst, damage)
    }
}
