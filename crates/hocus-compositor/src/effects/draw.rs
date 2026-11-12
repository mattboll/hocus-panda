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

use std::{any::Any, cell::RefCell, rc::Rc};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            ImportMem, Offscreen, Renderer,
            element::{Element, Id, Kind, RenderElement},
            gles::{Capability, GlesError, GlesFrame, GlesRenderer, GlesTexture, ffi},
            pixman::PixmanRenderer,
            utils::{CommitCounter, DamageSet, OpaqueRegions},
        },
    },
    utils::{Buffer as BufferCoords, Physical, Point, Rectangle, Scale, Transform},
};

use super::{flames::Burn, mesh::Mesh, particles::FLOATS, shatter::SHARD_FLOATS};
use crate::{
    matrix::{Program, bind, compile_with, quads},
    wobble::{Kept, Picture},
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
    /// [`super::particles::vertices`] lays them, and where they may show.
    Sparks {
        smoke: Vec<f32>,
        embers: Vec<f32>,
        clip: Clip,
    },
    /// Shapes of the snow (#795): those drawn over what is under them,
    /// then those added to it, where they may show.
    Shapes { over: Mesh, added: Mesh, clip: Clip },
    /// Pieces of a picture each drawn on a quad of its own (the creatures'
    /// parts, #796): [`SPRITE_FLOATS`] floats a vertex, six a quad.
    Sprites {
        texture: Rc<dyn Any>,
        vertices: Vec<f32>,
    },
    /// A window's picture whole, or in shards (#798):
    /// [`super::shatter::SHARD_FLOATS`] floats a vertex, three a triangle;
    /// their edges `width` wide (physical), where they may show.
    Shards {
        texture: Rc<dyn Any>,
        vertices: Vec<f32>,
        width: f32,
        clip: Clip,
    },
    /// Nothing: what stands for an effect where it must not be seen (behind
    /// the glass, blurred once and kept).
    Blank,
}

/// Other windows and the bar an effect stays off, at most this many.
pub const HOLES: usize = 8;

/// A sprite's vertex: where it is (output-local, physical), where in its
/// sheet (0 to 1), how opaque.
pub const SPRITE_FLOATS: usize = 5;

/// Where an effect may draw, output-local and physical (`x0, y0, x1, y1`):
/// within `keep`, out of each of `holes` (the other windows and the bar:
/// what the user reads). An effect never brightens them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clip {
    pub keep: [f32; 4],
    pub holes: [[f32; 4]; HOLES],
    pub count: usize,
    /// How far in from `keep`'s sides the sparks fade out, pixels.
    pub fade: f32,
}

impl Clip {
    /// Within `keep`, out of the first [`HOLES`] of `holes`.
    pub fn new(keep: [f32; 4], holes: impl IntoIterator<Item = [f32; 4]>, fade: f32) -> Self {
        let mut clip = Self {
            keep,
            holes: [[0.0; 4]; HOLES],
            count: 0,
            fade,
        };
        for (slot, hole) in clip.holes.iter_mut().zip(holes) {
            *slot = hole;
            clip.count += 1;
        }
        clip
    }

    /// The same, from an element's corner `at`.
    fn from(&self, at: Point<i32, Physical>) -> Self {
        let (x, y) = (at.x as f32, at.y as f32);
        let shift = |r: [f32; 4]| [r[0] - x, r[1] - y, r[2] - x, r[3] - y];
        Self {
            keep: shift(self.keep),
            holes: self.holes.map(shift),
            ..*self
        }
    }
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
    /// Where its shadow and its flames may show around it; within the
    /// window, all of it.
    pub clip: Clip,
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
        clip: Clip,
    ) -> Self {
        Self {
            id,
            commit,
            geometry,
            what: What::Sparks {
                smoke,
                embers,
                clip,
            },
        }
    }
}

impl FireElement {
    /// Shapes within `geometry` (output-local, physical), their vertices
    /// output-local.
    pub fn shapes(
        id: Id,
        commit: CommitCounter,
        geometry: Rectangle<i32, Physical>,
        over: Mesh,
        added: Mesh,
        clip: Clip,
    ) -> Self {
        Self {
            id,
            commit,
            geometry,
            what: What::Shapes { over, added, clip },
        }
    }

    /// The pieces of `texture` (a sheet, [`Fire::sheet`]) laid by
    /// `vertices` (output-local, physical) within `geometry`.
    pub fn sprites(
        id: Id,
        commit: CommitCounter,
        geometry: Rectangle<i32, Physical>,
        texture: Rc<dyn Any>,
        vertices: Vec<f32>,
    ) -> Self {
        Self {
            id,
            commit,
            geometry,
            what: What::Sprites { texture, vertices },
        }
    }

    /// Nothing over `geometry`, never changing.
    pub fn blank(id: Id, geometry: Rectangle<i32, Physical>) -> Self {
        Self {
            id,
            commit: CommitCounter::default(),
            geometry,
            what: What::Blank,
        }
    }
}

/// Renderers that draw the effects.
pub trait Fire: Renderer + Sized {
    /// Makes the effects' programs, once: whether they are there to draw
    /// with.
    fn effects_ready(&mut self) -> bool;

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

    /// Draws `parts` (output-local, as if the picture's corner were at
    /// theirs) into `kept` the first time, and gives the element drawing
    /// it whole or in shards by `vertices` (output-local, physical) within
    /// `geometry` (#798). `None`: not drawn so.
    #[expect(
        clippy::too_many_arguments,
        reason = "what a shatter's frame is drawn from"
    )]
    fn shards<E: RenderElement<Self>>(
        &mut self,
        id: Id,
        commit: CommitCounter,
        kept: &RefCell<Option<Kept>>,
        picture: &Picture,
        parts: &[E],
        geometry: Rectangle<i32, Physical>,
        vertices: Vec<f32>,
        width: f32,
        clip: Clip,
    ) -> Option<FireElement>;

    /// A sheet of pictures for [`FireElement::sprites`]: `rgba`
    /// (premultiplied, `size` pixels) made a texture. `None`: not drawn so.
    fn sheet(&mut self, rgba: &[u8], size: (i32, i32)) -> Option<Rc<dyn Any>>;

    fn draw_fire(
        frame: &mut Self::Frame<'_, '_>,
        element: &FireElement,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
    ) -> Result<(), Self::Error>;
}

impl Fire for PixmanRenderer {
    fn effects_ready(&mut self) -> bool {
        false
    }

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

    fn shards<E: RenderElement<Self>>(
        &mut self,
        _id: Id,
        _commit: CommitCounter,
        _kept: &RefCell<Option<Kept>>,
        _picture: &Picture,
        _parts: &[E],
        _geometry: Rectangle<i32, Physical>,
        _vertices: Vec<f32>,
        _width: f32,
        _clip: Clip,
    ) -> Option<FireElement> {
        None
    }

    fn sheet(&mut self, _rgba: &[u8], _size: (i32, i32)) -> Option<Rc<dyn Any>> {
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
/// Around the window, and where its paper is gone, nothing is drawn over
/// the other windows nor the bar ([`Clip`]).
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
    bool hole = clipped(v_px);
    if (sd > 0.0 && hole) {
        gl_FragColor = vec4(0.0);
        return;
    }
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
    // No flames over another window seen where the paper is gone.
    if (burning > 0.0 && t < 0.995 && dUp > -0.02 && !(hole && b <= 0.0)) {
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

/// Where an effect may draw ([`Clip`]): `p` in `keep`'s and the holes'
/// space.
const CLIP: &str = "
uniform vec4 keep;
uniform vec4 holes[8];
uniform float count;
bool clipped(vec2 p) {
    if (p.x < keep.x || p.y < keep.y || p.x >= keep.z || p.y >= keep.w) {
        return true;
    }
    for (int i = 0; i < 8; i++) {
        if (float(i) >= count) {
            break;
        }
        vec4 h = holes[i];
        if (p.x >= h.x && p.y >= h.y && p.x < h.z && p.y < h.w) {
            return true;
        }
    }
    return false;
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
varying vec2 v_at;
void main() {
    gl_Position = vec4(matrix * vec3(position, 1.0), 1.0);
    v_at = position;
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
varying vec2 v_at;
uniform float fade;

void main() {
    float d = length(v_local);
    if (d >= 1.0 || clipped(v_at)) {
        gl_FragColor = vec4(0.0);
        return;
    }
    float sides = clamp(min(v_at.x - keep.x, keep.z - v_at.x) / fade, 0.0, 1.0);
    if (v_shape.x < 0.5) {
        gl_FragColor = v_colour * (1.0 - d) * sides;
        return;
    }
    vec4 h0 = vec4(1.0, 150.0 / 255.0, 60.0 / 255.0, 1.0) * 0.55;
    vec4 h1 = vec4(1.0, 90.0 / 255.0, 30.0 / 255.0, 1.0) * 0.18;
    vec4 halo = d < 0.35 ? mix(h0, h1, d / 0.35) : mix(h1, vec4(0.0), (d - 0.35) / 0.65);
    float a = v_colour.a;
    float cover = clamp((v_shape.y - d) * v_shape.z + 0.5, 0.0, 1.0);
    gl_FragColor = (halo * a + vec4(v_colour.rgb, 1.0) * a * cover) * sides;
}
";

/// The shapes of [`super::mesh`]: a fill as its vertices say; a flake, a
/// soft disc; a clump, an ellipse lit from its upper left (the board's
/// two-point gradient, white from a focus at (-0.3, -0.3) to blue at its
/// edge); a box blurred by a Gaussian (or sharp, its edges smoothed).
/// Nothing where the clip says not.
const SHAPES: &str = "#version 100
precision highp float;
varying vec2 v_local;
varying vec4 v_colour;
varying vec4 v_shape;
varying vec2 v_at;

float erfish(float x) {
    float x2 = x * x;
    float k = 0.147 * x2;
    return sign(x) * sqrt(1.0 - exp(-x2 * (1.2732395 + k) / (1.0 + k)));
}
float blurred(float reach, float p, float sigma) {
    if (sigma < 0.5) {
        return clamp(reach - abs(p) + 0.5, 0.0, 1.0);
    }
    float k = 1.0 / (sigma * 1.4142136);
    return 0.5 * (erfish((reach - p) * k) + erfish((reach + p) * k));
}

void main() {
    if (clipped(v_at)) {
        gl_FragColor = vec4(0.0);
        return;
    }
    float kind = v_shape.x;
    if (kind < 0.5) {
        gl_FragColor = v_colour;
    } else if (kind < 1.5) {
        float d = length(v_local);
        float stop = v_shape.y;
        float a = d < stop ? mix(1.0, 0.75, d / stop) : mix(0.75, 0.0, clamp((d - stop) / (1.0 - stop), 0.0, 1.0));
        gl_FragColor = v_colour * a;
    } else if (kind < 2.5) {
        vec2 p = v_local;
        float e = length(vec2(p.x, p.y / 0.8));
        float cover = clamp((1.0 - e) * v_shape.y + 0.5, 0.0, 1.0);
        vec2 c0 = vec2(-0.3, -0.3);
        vec2 q = p - c0;
        float a = 1.0 - dot(c0, c0);
        float b = dot(q, c0);
        float t = clamp((b + sqrt(b * b + a * dot(q, q))) / a, 0.0, 1.0);
        vec3 c = mix(vec3(1.0), vec3(201.0, 213.0, 232.0) / 255.0, t);
        gl_FragColor = vec4(c, 1.0) * cover * v_colour.a;
    } else {
        float cx = blurred(v_shape.y, v_local.x, v_shape.w);
        float cy = blurred(v_shape.z, v_local.y, v_shape.w);
        gl_FragColor = v_colour * cx * cy;
    }
}
";

/// A piece of a sheet, as opaque as its vertices say (premultiplied).
const SPRITE_VERTEX: &str = "#version 100
uniform mat3 matrix;
attribute vec2 position;
attribute vec2 local;
attribute float shade;
varying vec2 v_uv;
varying float v_shade;
void main() {
    gl_Position = vec4(matrix * vec3(position, 1.0), 1.0);
    v_uv = local;
    v_shade = shade;
}
";

const SPRITE: &str = "#version 100
precision highp float;
uniform sampler2D tex;
varying vec2 v_uv;
varying float v_shade;
void main() {
    gl_FragColor = texture2D(tex, v_uv) * v_shade;
}
";

const SHARD_VERTEX: &str = "#version 100
uniform mat3 matrix;
attribute vec2 position;
attribute vec2 local;
attribute vec4 look;
varying vec2 v_uv;
varying vec4 v_look;
varying vec2 v_at;
void main() {
    gl_Position = vec4(matrix * vec3(position, 1.0), 1.0);
    v_at = position;
    v_uv = local;
    v_look = look;
}
";

/// The board's shard: its picture, the white of the light it catches and
/// the dark of its back over it (where the picture is), its outline
/// stroked across its edge, smoothed over a pixel.
const SHARD: &str = "#version 100
precision highp float;
uniform sampler2D tex;
uniform float width;
varying vec2 v_uv;
varying vec4 v_look;
varying vec2 v_at;

void main() {
    if (clipped(v_at)) {
        gl_FragColor = vec4(0.0);
        return;
    }
    float e = v_look.w;
    vec4 c = texture2D(tex, clamp(v_uv, 0.0, 1.0)) * clamp(e + 0.5, 0.0, 1.0);
    c = c * (1.0 - v_look.x) + vec4(1.0) * v_look.x * c.a;
    c = c * (1.0 - v_look.y) + vec4(10.0 / 255.0, 14.0 / 255.0, 24.0 / 255.0, 1.0) * v_look.y * c.a;
    float line = clamp(width * 0.5 + 0.5 - abs(e), 0.0, 1.0) * v_look.z;
    gl_FragColor = c * (1.0 - line) + vec4(220.0 / 255.0, 235.0 / 255.0, 1.0, 1.0) * line;
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
    shapes: Program,
    sprites: Program,
    shards: Program,
    /// The shards' look attribute.
    shard_look: ffi::types::GLint,
    /// The sprites' opacity attribute.
    shade: ffi::types::GLint,
    /// The sparks' other attributes.
    colour: ffi::types::GLint,
    shape: ffi::types::GLint,
    /// The shapes' attributes: place, place across, colour, shape.
    shapes_attribs: [ffi::types::GLint; 4],
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
            let with_clip =
                |source: &str, after: &str| source.replacen(after, &format!("{after}{CLIP}"), 1);
            let burn = compile_with(
                gl,
                crate::matrix::VERTEX,
                &with_clip(BURN, "varying vec2 v_px;\n"),
                &[
                    "matrix", "tex", "size", "corner", "px", "radius", "t", "time", "keep",
                    "holes", "count",
                ],
            )?;
            let light = compile_with(
                gl,
                crate::matrix::VERTEX,
                LIGHT,
                &["matrix", "centre", "inner", "outer", "strength"],
            )?;
            let sparks = compile_with(
                gl,
                SPARKS_VERTEX,
                &with_clip(SPARKS, "varying vec2 v_at;\n"),
                &["matrix", "keep", "holes", "count", "fade"],
            )?;
            let shapes = compile_with(
                gl,
                SPARKS_VERTEX,
                &with_clip(SHAPES, "varying vec2 v_at;\n"),
                &["matrix", "keep", "holes", "count"],
            )?;
            let sprites = compile_with(gl, SPRITE_VERTEX, SPRITE, &["matrix", "tex"])?;
            let shards = compile_with(
                gl,
                SHARD_VERTEX,
                &with_clip(SHARD, "varying vec2 v_at;\n"),
                &["matrix", "tex", "width", "keep", "holes", "count"],
            )?;
            let attrib =
                |p: &Program, n: &std::ffi::CStr| gl.GetAttribLocation(p.program, n.as_ptr());
            let shard_look = attrib(&shards, c"look");
            if shard_look < 0 || shards.position < 0 || shards.local < 0 {
                return Err("the shards' attributes are missing".to_owned());
            }
            let shade = attrib(&sprites, c"shade");
            if shade < 0 || sprites.position < 0 || sprites.local < 0 {
                return Err("the sprites' attributes are missing".to_owned());
            }
            let (colour, shape) = (attrib(&sparks, c"colour"), attrib(&sparks, c"shape"));
            if colour < 0 || shape < 0 || sparks.position < 0 || sparks.local < 0 {
                return Err("the particles' attributes are missing".to_owned());
            }
            let shapes_attribs = [
                shapes.position,
                shapes.local,
                attrib(&shapes, c"colour"),
                attrib(&shapes, c"shape"),
            ];
            if shapes_attribs.iter().any(|a| *a < 0) {
                return Err("the shapes' attributes are missing".to_owned());
            }
            Ok(Programs {
                burn,
                light,
                sparks,
                shapes,
                sprites,
                shards,
                shard_look,
                shade,
                colour,
                shape,
                shapes_attribs,
            })
        })
        .map_err(|e| e.to_string())?
}

impl Fire for GlesRenderer {
    fn effects_ready(&mut self) -> bool {
        programs(self).is_some()
    }

    fn burn<E: RenderElement<Self>>(
        &mut self,
        burn: &Burn,
        picture: &Picture,
        parts: &[E],
        geometry: Rectangle<i32, Physical>,
        look: BurnLook,
    ) -> Option<FireElement> {
        programs(self)?;
        let kept = keep_picture(self, &burn.picture, picture, parts, "a burning window")?;
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

    fn shards<E: RenderElement<Self>>(
        &mut self,
        id: Id,
        commit: CommitCounter,
        kept: &RefCell<Option<Kept>>,
        picture: &Picture,
        parts: &[E],
        geometry: Rectangle<i32, Physical>,
        vertices: Vec<f32>,
        width: f32,
        clip: Clip,
    ) -> Option<FireElement> {
        programs(self)?;
        let texture = keep_picture(self, kept, picture, parts, "a shattering window")?;
        Some(FireElement {
            id,
            commit,
            geometry,
            what: What::Shards {
                texture,
                vertices,
                width,
                clip,
            },
        })
    }

    fn sheet(&mut self, rgba: &[u8], size: (i32, i32)) -> Option<Rc<dyn Any>> {
        programs(self)?;
        let texture: GlesTexture = self
            .import_memory(rgba, Fourcc::Abgr8888, size.into(), false)
            .map_err(|err| tracing::warn!(%err, "no sheet for the creatures"))
            .ok()?;
        Some(Rc::new(texture))
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
                            set_clip(gl, p, &look.clip.from(corner));
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
            What::Sparks {
                smoke,
                embers,
                clip,
            } => frame.with_context(|gl| {
                #[allow(unsafe_code)]
                // SAFETY: the frame's context is current.
                unsafe {
                    sparks(gl, &programs, &matrix, instancing, smoke, embers, clip);
                }
            }),
            What::Shapes { over, added, clip } => {
                let projection = *frame.projection();
                frame.with_context(|gl| {
                    #[allow(unsafe_code)]
                    // SAFETY: the frame's context is current.
                    unsafe {
                        let boxes = scissors(gl, &projection, dst, damage);
                        shapes(
                            gl, &programs, &matrix, instancing, over, added, clip, &boxes,
                        );
                    }
                })
            }
            What::Sprites { texture, vertices } => {
                let Some(texture) = texture.downcast_ref::<GlesTexture>() else {
                    return Ok(());
                };
                let tex = texture.tex_id();
                let projection = *frame.projection();
                frame.with_context(|gl| {
                    #[allow(unsafe_code)]
                    // SAFETY: the frame's context is current.
                    unsafe {
                        let boxes = scissors(gl, &projection, dst, damage);
                        sprites(gl, &programs, &matrix, instancing, tex, vertices, &boxes);
                    }
                })
            }
            What::Shards {
                texture,
                vertices,
                width,
                clip,
            } => {
                let Some(texture) = texture.downcast_ref::<GlesTexture>() else {
                    return Ok(());
                };
                let tex = texture.tex_id();
                let projection = *frame.projection();
                frame.with_context(|gl| {
                    #[allow(unsafe_code)]
                    unsafe {
                        let boxes = scissors(gl, &projection, dst, damage);
                        shards(
                            gl, &programs, &matrix, instancing, tex, vertices, *width, clip, &boxes,
                        );
                    }
                })
            }
            What::Blank => Ok(()),
        }
    }
}

/// The picture `kept` of a window leaving: drawn from `parts` the first
/// time (or when its size changes), its texture after.
fn keep_picture<E: RenderElement<GlesRenderer>>(
    renderer: &mut GlesRenderer,
    kept: &RefCell<Option<Kept>>,
    picture: &Picture,
    parts: &[E],
    what: &str,
) -> Option<Rc<dyn Any>> {
    let size = picture.buffer_size();
    let mut kept = kept.borrow_mut();
    if let Some((_, texture)) = kept.as_ref().filter(|(s, _)| *s == size) {
        return Some(texture.clone());
    }
    let mut texture: GlesTexture =
        Offscreen::<GlesTexture>::create_buffer(renderer, Fourcc::Abgr8888, size)
            .map_err(|err| tracing::warn!(%err, "no picture for {what}"))
            .ok()?;
    if let Err(err) = crate::wobble::draw_picture(renderer, &mut texture, picture, parts) {
        tracing::warn!(%err, "could not draw the picture of {what}");
        return None;
    }
    let rc: Rc<dyn Any> = Rc::new(texture);
    *kept = Some((size, rc.clone()));
    Some(rc)
}

/// Draws a window whole or in shards (#798), cut to the frame's damage.
#[allow(unsafe_code, clippy::too_many_arguments)]
unsafe fn shards(
    gl: &ffi::Gles2,
    programs: &Programs,
    matrix: &[f32; 9],
    instancing: bool,
    tex: ffi::types::GLuint,
    vertices: &[f32],
    width: f32,
    clip: &Clip,
    boxes: &[[i32; 4]],
) {
    let p = &programs.shards;
    let n = vertices.len() / SHARD_FLOATS;
    if n == 0 {
        return;
    }
    let attribs = [
        (p.position as u32, 2, 0),
        (p.local as u32, 2, 2),
        (programs.shard_look as u32, 4, 4),
    ];
    let stride = (SHARD_FLOATS * std::mem::size_of::<f32>()) as ffi::types::GLsizei;
    let mut viewport = [0i32; 4];
    unsafe {
        gl.GetIntegerv(ffi::VIEWPORT, viewport.as_mut_ptr());
        gl.UseProgram(p.program);
        gl.UniformMatrix3fv(p.at("matrix"), 1, ffi::FALSE, matrix.as_ptr());
        bind(gl, 0, tex, false);
        gl.Uniform1i(p.at("tex"), 0);
        gl.Uniform1f(p.at("width"), width);
        set_clip(gl, p, clip);
        gl.BlendFunc(ffi::ONE, ffi::ONE_MINUS_SRC_ALPHA);
        gl.BindBuffer(ffi::ARRAY_BUFFER, 0);
        for (at, size, offset) in attribs {
            gl.EnableVertexAttribArray(at);
            if instancing {
                gl.VertexAttribDivisor(at, 0);
            }
            gl.VertexAttribPointer(
                at,
                size,
                ffi::FLOAT,
                ffi::FALSE,
                stride,
                vertices.as_ptr().add(offset) as *const _,
            );
        }
        for [x, y, w, h] in boxes {
            gl.Scissor(*x, *y, *w, *h);
            gl.DrawArrays(ffi::TRIANGLES, 0, n as ffi::types::GLsizei);
        }
        let [x, y, w, h] = viewport;
        gl.Scissor(x, y, w, h);
        for (at, _, _) in attribs {
            gl.DisableVertexAttribArray(at);
        }
        gl.BindTexture(ffi::TEXTURE_2D, 0);
    }
}

/// Draws the sprites `vertices` of the sheet `tex` over what is under
/// them, cut to each of `boxes` in turn.
///
/// # Safety
///
/// The context must be current, its scissor test on (as smithay's frame
/// has it).
#[allow(unsafe_code)]
unsafe fn sprites(
    gl: &ffi::Gles2,
    programs: &Programs,
    matrix: &[f32; 9],
    instancing: bool,
    tex: ffi::types::GLuint,
    vertices: &[f32],
    boxes: &[[i32; 4]],
) {
    let p = &programs.sprites;
    let n = vertices.len() / SPRITE_FLOATS;
    if n == 0 {
        return;
    }
    let attribs = [
        (p.position as u32, 2, 0),
        (p.local as u32, 2, 2),
        (programs.shade as u32, 1, 4),
    ];
    let stride = (SPRITE_FLOATS * std::mem::size_of::<f32>()) as ffi::types::GLsizei;
    let mut viewport = [0i32; 4];
    // SAFETY: the context is current; the vertices outlive each draw call,
    // every attribute enabled here is disabled again and the scissor given
    // back, as smithay's own draws expect.
    unsafe {
        gl.GetIntegerv(ffi::VIEWPORT, viewport.as_mut_ptr());
        gl.UseProgram(p.program);
        gl.UniformMatrix3fv(p.at("matrix"), 1, ffi::FALSE, matrix.as_ptr());
        bind(gl, 0, tex, false);
        gl.Uniform1i(p.at("tex"), 0);
        gl.BlendFunc(ffi::ONE, ffi::ONE_MINUS_SRC_ALPHA);
        gl.BindBuffer(ffi::ARRAY_BUFFER, 0);
        for (at, size, offset) in attribs {
            gl.EnableVertexAttribArray(at);
            if instancing {
                gl.VertexAttribDivisor(at, 0);
            }
            gl.VertexAttribPointer(
                at,
                size,
                ffi::FLOAT,
                ffi::FALSE,
                stride,
                vertices.as_ptr().add(offset) as *const _,
            );
        }
        for [x, y, w, h] in boxes {
            gl.Scissor(*x, *y, *w, *h);
            gl.DrawArrays(ffi::TRIANGLES, 0, n as ffi::types::GLsizei);
        }
        let [x, y, w, h] = viewport;
        gl.Scissor(x, y, w, h);
        for (at, _, _) in attribs {
            gl.DisableVertexAttribArray(at);
        }
        gl.BindTexture(ffi::TEXTURE_2D, 0);
    }
}

/// Tells program `p` (in use) where it may draw.
///
/// # Safety
///
/// The context must be current, `p` in use.
#[allow(unsafe_code)]
unsafe fn set_clip(gl: &ffi::Gles2, p: &Program, clip: &Clip) {
    let [x0, y0, x1, y1] = clip.keep;
    // SAFETY: the context is current; the holes outlive the call.
    unsafe {
        gl.Uniform4f(p.at("keep"), x0, y0, x1, y1);
        gl.Uniform4fv(
            p.at("holes"),
            HOLES as ffi::types::GLsizei,
            clip.holes.as_ptr() as *const f32,
        );
        gl.Uniform1f(p.at("count"), clip.count as f32);
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
    clip: &Clip,
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
        set_clip(gl, p, clip);
        gl.Uniform1f(p.at("fade"), clip.fade.max(1.0));
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

/// Where `damage` (from `dst`'s corner) is in the frame's pixels, as
/// `glScissor` takes it: a mesh is drawn whole, so it is cut there, never
/// blended again over what the frame kept of it.
///
/// # Safety
///
/// The frame's context must be current.
#[allow(unsafe_code)]
unsafe fn scissors(
    gl: &ffi::Gles2,
    projection: &[f32; 9],
    dst: Rectangle<i32, Physical>,
    damage: &[Rectangle<i32, Physical>],
) -> Vec<[i32; 4]> {
    let mut viewport = [0i32; 4];
    // SAFETY: the context is current; four integers are written.
    unsafe {
        gl.GetIntegerv(ffi::VIEWPORT, viewport.as_mut_ptr());
    }
    let [vx, vy, vw, vh] = viewport.map(|v| v as f32);
    let to_frame = |x: f32, y: f32| {
        let m = projection;
        let nx = m[0] * x + m[3] * y + m[6];
        let ny = m[1] * x + m[4] * y + m[7];
        (vx + (nx + 1.0) / 2.0 * vw, vy + (ny + 1.0) / 2.0 * vh)
    };
    damage
        .iter()
        .filter_map(|d| {
            let (x0, y0) = ((dst.loc.x + d.loc.x) as f32, (dst.loc.y + d.loc.y) as f32);
            let (x1, y1) = (x0 + d.size.w as f32, y0 + d.size.h as f32);
            let corners = [
                to_frame(x0, y0),
                to_frame(x1, y0),
                to_frame(x0, y1),
                to_frame(x1, y1),
            ];
            let lo = corners
                .iter()
                .fold((f32::MAX, f32::MAX), |a, c| (a.0.min(c.0), a.1.min(c.1)));
            let hi = corners
                .iter()
                .fold((f32::MIN, f32::MIN), |a, c| (a.0.max(c.0), a.1.max(c.1)));
            let (x, y) = (lo.0.round() as i32, lo.1.round() as i32);
            let (w, h) = (hi.0.round() as i32 - x, hi.1.round() as i32 - y);
            (w > 0 && h > 0).then_some([x, y, w, h])
        })
        .collect()
}

/// Draws the shapes `over` over, then `added` added, cut to each of
/// `boxes` in turn.
///
/// # Safety
///
/// The context must be current, its scissor test on (as smithay's frame
/// has it).
#[allow(unsafe_code, clippy::too_many_arguments)]
unsafe fn shapes(
    gl: &ffi::Gles2,
    programs: &Programs,
    matrix: &[f32; 9],
    instancing: bool,
    over: &Mesh,
    added: &Mesh,
    clip: &Clip,
    boxes: &[[i32; 4]],
) {
    let p = &programs.shapes;
    let [position, local, colour, shape] = programs.shapes_attribs.map(|a| a as u32);
    let attribs = [
        (position, 2, 0),
        (local, 2, 2),
        (colour, 4, 4),
        (shape, 4, 8),
    ];
    let stride = (FLOATS * std::mem::size_of::<f32>()) as ffi::types::GLsizei;
    let mut viewport = [0i32; 4];
    // SAFETY: the context is current; the vertices and indices outlive
    // each draw call, every attribute enabled here is disabled again, the
    // blending and the scissor given back, as smithay's own draws expect.
    unsafe {
        gl.GetIntegerv(ffi::VIEWPORT, viewport.as_mut_ptr());
        gl.UseProgram(p.program);
        gl.UniformMatrix3fv(p.at("matrix"), 1, ffi::FALSE, matrix.as_ptr());
        set_clip(gl, p, clip);
        gl.BindBuffer(ffi::ARRAY_BUFFER, 0);
        gl.BindBuffer(ffi::ELEMENT_ARRAY_BUFFER, 0);
        for (at, _, _) in attribs {
            gl.EnableVertexAttribArray(at);
            if instancing {
                gl.VertexAttribDivisor(at, 0);
            }
        }
        for (mesh, add) in [(over, false), (added, true)] {
            if mesh.is_empty() {
                continue;
            }
            gl.BlendFunc(
                ffi::ONE,
                if add {
                    ffi::ONE
                } else {
                    ffi::ONE_MINUS_SRC_ALPHA
                },
            );
            for (at, size, offset) in attribs {
                gl.VertexAttribPointer(
                    at,
                    size,
                    ffi::FLOAT,
                    ffi::FALSE,
                    stride,
                    mesh.vertices.as_ptr().add(offset) as *const _,
                );
            }
            for [x, y, w, h] in boxes {
                gl.Scissor(*x, *y, *w, *h);
                gl.DrawElements(
                    ffi::TRIANGLES,
                    mesh.indices.len() as ffi::types::GLsizei,
                    ffi::UNSIGNED_SHORT,
                    mesh.indices.as_ptr() as *const _,
                );
            }
        }
        gl.BlendFunc(ffi::ONE, ffi::ONE_MINUS_SRC_ALPHA);
        let [x, y, w, h] = viewport;
        gl.Scissor(x, y, w, h);
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
