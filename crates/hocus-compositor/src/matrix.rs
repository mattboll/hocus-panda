//! The compositor's glyph shaders: a window's rain as it opens and closes
//! (#432, [`crate::rain`]) and the wallpapers that move (#433,
//! [`crate::moving`]). Both draw glyphs from one atlas
//! ([`hocus_link::moving::atlas`], 16 KB whatever the scale), each lit dot
//! at the size of its cell, and compute everything else from their time:
//! no state is kept on the CPU from frame to frame.
//!
//! GLES only, through programs of their own (a window's picture and the
//! atlas are two textures, which smithay's pixel shaders do not take).
//! Pixman draws neither: a window fades instead, and a moving wallpaper is
//! its still picture.

use std::{any::Any, rc::Rc};

use hocus_link::moving::{FrostColours, Moving, RainColours};
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

use crate::wobble::Picture;

/// A window's rain as drawn this frame: its picture through the rain.
#[derive(Debug)]
pub struct RainElement {
    id: Id,
    commit: CommitCounter,
    /// Physical, output-local: the picture's place.
    geometry: Rectangle<i32, Physical>,
    texture: Rc<dyn Any>,
    look: RainLook,
}

/// What a rain's shader is told.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RainLook {
    /// Seconds into the motion: an opening's time, or a closing's.
    pub time: f32,
    pub closing: bool,
    pub seed: f32,
    /// A cell, physical pixels.
    pub cell: (f32, f32),
    pub colours: RainColours,
    pub alpha: f32,
    /// The window in its picture, physical pixels: x, y, width, height.
    pub window: [f32; 4],
    /// The rain, or Givre's thawing pane (#740), and the ice's colours.
    pub style: crate::rain::Style,
    pub frost: FrostColours,
    /// Physical pixels a logical one.
    pub scale: f32,
}

/// A moving wallpaper drawn on a screen.
#[derive(Debug, Clone)]
pub struct WallElement {
    id: Id,
    commit: CommitCounter,
    /// Physical, output-local: the whole screen.
    geometry: Rectangle<i32, Physical>,
    look: WallLook,
}

/// What a moving wallpaper's shader is told.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WallLook {
    pub moving: Moving,
    /// Its frames so far: the rain's heads move one cell each.
    pub tick: f32,
    /// A cell of the rain, physical pixels; the scale for the stars.
    pub cell: (f32, f32),
    pub scale: f32,
    pub colours: RainColours,
}

impl WallElement {
    /// The element of `look` over a screen `size` physical pixels large;
    /// `commit` moves on with each frame drawn.
    pub fn new(id: Id, commit: CommitCounter, size: (i32, i32), look: WallLook) -> Self {
        Self {
            id,
            commit,
            geometry: Rectangle::from_size(size.into()),
            look,
        }
    }
}

/// Renderers that draw the glyph shaders.
pub trait Matrix: Renderer + Sized {
    /// Draws `parts` (output-local, as if the window's picture's corner
    /// were at theirs) into `rain`'s picture, unless `redraw` is false and
    /// it is there, and gives the element drawing it through the
    /// rain. `None`: drawn as the fallback fade.
    fn rain<E: RenderElement<Self>>(
        &mut self,
        rain: &crate::rain::Rain,
        redraw: bool,
        picture: &Picture,
        parts: &[E],
        look: RainLook,
    ) -> Option<RainElement>;

    fn draw_rain(
        frame: &mut Self::Frame<'_, '_>,
        rain: &RainElement,
        dst: Rectangle<i32, Physical>,
    ) -> Result<(), Self::Error>;

    /// Whether this renderer draws moving wallpapers.
    fn draws_walls(&mut self) -> bool;

    fn draw_wall(
        frame: &mut Self::Frame<'_, '_>,
        wall: &WallElement,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
    ) -> Result<(), Self::Error>;
}

impl Matrix for PixmanRenderer {
    fn rain<E: RenderElement<Self>>(
        &mut self,
        _rain: &crate::rain::Rain,
        _redraw: bool,
        _picture: &Picture,
        _parts: &[E],
        _look: RainLook,
    ) -> Option<RainElement> {
        None
    }

    fn draw_rain(
        _frame: &mut Self::Frame<'_, '_>,
        _rain: &RainElement,
        _dst: Rectangle<i32, Physical>,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn draws_walls(&mut self) -> bool {
        false
    }

    fn draw_wall(
        _frame: &mut Self::Frame<'_, '_>,
        _wall: &WallElement,
        _dst: Rectangle<i32, Physical>,
        _damage: &[Rectangle<i32, Physical>],
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

const VERTEX: &str = "#version 100
uniform mat3 matrix;
attribute vec2 position;
attribute vec2 local;
varying vec2 v_px;
void main() {
    gl_Position = vec4(matrix * vec3(position, 1.0), 1.0);
    v_px = local;
}
";

/// What every glyph shader starts with: the hash (the same words as
/// `hocus_link::moving::hash`), and a glyph's lit dots read from the
/// atlas, mirrored, at a point of its cell.
const COMMON: &str = "#version 100
precision highp float;
uniform sampler2D glyphs;
varying vec2 v_px;

float h(vec2 p) {
    return fract(sin(p.x * 127.1 + p.y * 311.7) * 43758.5453);
}

// Glyph g's coverage at `uv` (0 to 1 across its cell), the cell `px`
// pixels large: 5 x 7 dots in the cell's middle, each a little smaller
// than its pitch, its edge soft over a pixel.
float glyph(float g, vec2 uv, vec2 px) {
    vec2 lo = vec2(0.12, 0.14);
    vec2 hi = vec2(0.88, 0.86);
    vec2 b = (uv - lo) / (hi - lo);
    if (b.x < 0.0 || b.y < 0.0 || b.x >= 1.0 || b.y >= 1.0) {
        return 0.0;
    }
    vec2 grid = b * vec2(5.0, 7.0);
    vec2 d = floor(grid);
    vec2 f = grid - d;
    vec2 at = vec2(mod(g, 8.0) * 8.0 + 4.0 - d.x, floor(g / 8.0) * 8.0 + d.y);
    float on = texture2D(glyphs, (at + 0.5) / 64.0).a;
    vec2 pitch = (hi - lo) * px / vec2(5.0, 7.0);
    vec2 e = min(f, 1.0 - f) * pitch;
    float edge = clamp(min(e.x, e.y) - 0.07 * min(pitch.x, pitch.y) + 0.5, 0.0, 1.0);
    return on * edge;
}
";

/// A window's rain. Opening: each column's head falls through the
/// window, starting within its first 110 ms and falling in 170 to 250 ms
/// (ease-out); behind it the window shows, under its glyphs' tail; a green
/// veil fades out from 240 to 360 ms. Closing: each column drains
/// downward (ease-in), glyphs replacing the window from the top, all gone
/// by 300 ms. The picture's margin (the shadow) fades along.
const RAIN: &str = "
uniform sampler2D tex;
uniform vec2 size;
uniform vec4 win;
uniform vec2 cell;
uniform float t;
uniform float closing;
uniform float seed;
uniform vec3 head;
uniform vec3 tail;
uniform float alpha;

void main() {
    vec4 c = texture2D(tex, v_px / size);
    vec2 l = v_px - win.xy;
    bool inside = l.x >= 0.0 && l.y >= 0.0 && l.x < win.z && l.y < win.w;
    if (!inside) {
        float shown = closing > 0.5 ? 1.0 - clamp(t / 0.30, 0.0, 1.0) : clamp(t / 0.36, 0.0, 1.0);
        gl_FragColor = c * shown * alpha;
        return;
    }
    float col = floor(l.x / cell.x);
    float row = floor(l.y / cell.y);
    float trail = 6.0 * cell.y;
    vec4 base;
    float k;
    if (closing > 0.5) {
        float delay = h(vec2(col, seed)) * 0.09;
        float fall = 0.15 + h(vec2(col + 57.0, seed)) * 0.06;
        float u = clamp((t - delay) / fall, 0.0, 1.0);
        float y = u * u * (win.w + trail);
        k = (y - l.y) / cell.y;
        base = k >= 6.0 ? vec4(0.0) : (k < 0.0 ? c : c * (1.0 - k / 6.0));
    } else {
        float delay = h(vec2(col, seed)) * 0.11;
        float fall = 0.17 + h(vec2(col + 57.0, seed)) * 0.08;
        float u = clamp((t - delay) / fall, 0.0, 1.0);
        float y = (1.0 - (1.0 - u) * (1.0 - u)) * (win.w + trail);
        k = (y - l.y) / cell.y;
        base = k >= 1.0 ? c : vec4(0.0);
        float veil = 0.3 * (1.0 - clamp((t - 0.24) / 0.12, 0.0, 1.0));
        base.rgb = mix(base.rgb, tail * base.a, veil);
    }
    if (k >= 0.0 && k < 6.0) {
        float g = floor(h(vec2(col + mod(floor(t * 20.0), 97.0) * 0.37, row + seed)) * 64.0);
        float m = glyph(g, fract(l / cell), cell);
        vec3 colour = k < 1.0 ? head : tail;
        float bright = k < 1.0 ? 1.0 : 1.0 - floor(k) / 6.0;
        float a = m * bright;
        base = base * (1.0 - a) + vec4(colour, 1.0) * a;
    }
    gl_FragColor = base * alpha;
}
";

/// Givre's thawing pane (#740, "la vitre qui dégèle"). Opening, 360 ms:
/// the window shows through a sheet of ice: refracted (the gradient of a
/// 3-octave noise shifts where it is read, 5 px at most, never within a few
/// px of its edges, gone by 200 ms so that its text is steady), under a
/// pale veil with a web of fine cracks; the ice thaws to clear glass while
/// one reflection crosses it diagonally. Closing, 300 ms: the reverse, the
/// pane freezing over, then fading. No pixel is brighter than both the
/// window's there and the ice: the cracks and the reflection are the ice's
/// own, never a pattern laid on the window. Lengths in logical pixels
/// (`px` physical ones each).
const FROST: &str = "
uniform sampler2D tex;
uniform vec2 size;
uniform vec4 win;
uniform float t;
uniform float closing;
uniform float seed;
uniform vec3 ice;
uniform vec3 crack;
uniform float alpha;
uniform float px;

float noise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    return mix(mix(h(i), h(i + vec2(1.0, 0.0)), f.x), mix(h(i + vec2(0.0, 1.0)), h(i + vec2(1.0, 1.0)), f.x), f.y);
}

float fbm(vec2 p) {
    return 0.57 * noise(p) + 0.29 * noise(p * 2.07 + 7.3) + 0.14 * noise(p * 4.13 + 1.7);
}

float ease_out(float x) {
    x = clamp(x, 0.0, 1.0);
    return 1.0 - pow(1.0 - x, 3.0);
}

float ease_in_out(float x) {
    x = clamp(x, 0.0, 1.0);
    return x < 0.5 ? 4.0 * x * x * x : 1.0 - pow(-2.0 * x + 2.0, 3.0) / 2.0;
}

void main() {
    float thawed = closing > 0.5 ? 1.0 - ease_in_out(t / 0.30) : ease_out((t - 0.02) / 0.34);
    float frozen = 1.0 - thawed;
    float shown = smoothstep(0.0, 0.4, thawed);
    vec4 here = texture2D(tex, v_px / size);
    vec2 l = (v_px - win.xy) / px;
    vec2 w = win.zw / px;
    if (l.x < 0.0 || l.y < 0.0 || l.x >= w.x || l.y >= w.y) {
        gl_FragColor = here * shown * alpha;
        return;
    }
    // The refraction: small, none near the edges, and gone by 200 ms of
    // an opening.
    float edge = min(min(l.x, l.y), min(w.x - l.x, w.y - l.y));
    float settle = closing > 0.5 ? 1.0 : 1.0 - smoothstep(0.06, 0.20, t);
    float amp = pow(frozen, 1.6) * 5.0 * settle * smoothstep(3.0, 12.0, edge);
    vec2 q = l;
    if (amp > 0.01) {
        vec2 np = l / 70.0 + seed * 0.13;
        float n0 = fbm(np);
        vec2 grad = vec2(fbm(np + vec2(0.03, 0.0)) - n0, fbm(np + vec2(0.0, 0.03)) - n0) / 0.03;
        q = clamp(l + grad * amp, vec2(0.0), w - 0.5);
    }
    vec4 c = texture2D(tex, (win.xy + q * px) / size);
    // The ice: a pale veil and fine cracks, thawing.
    float ridge = 1.0 - abs(fbm(l / 46.0 + 3.7 + seed * 0.07) * 2.0 - 1.0);
    float cracks = pow(ridge, 22.0);
    vec3 rgb = mix(c.rgb, ice * c.a, frozen * 0.38) + crack * cracks * frozen * 0.9 * c.a;
    // One reflection crossing it diagonally.
    float k = l.x / w.x * 0.6 + l.y / w.y * 0.4;
    float sweep = closing > 0.5 ? 1.25 - t / 0.26 * 1.5 : (t - 0.06) / 0.30 * 1.5 - 0.25;
    rgb += vec3(exp(-pow((k - sweep) / 0.05, 2.0)) * 0.28 * c.a * (0.3 + 0.7 * frozen));
    rgb = min(rgb, max(max(c.rgb, here.rgb), ice * c.a));
    gl_FragColor = vec4(rgb, c.a) * shown * alpha;
}
";

/// Givre's aurora (#740): `hocus_link::moving::aurora` and its stars, in
/// the same words.
const WALL_AURORA: &str = "
uniform vec2 screen;
uniform float time;
uniform float scale;
uniform vec3 top;
uniform vec3 bottom;
uniform vec3 veil0;
uniform vec3 veil1;
uniform vec3 veil2;
uniform vec3 star;

// `hocus_link::moving::noise`: smooth value noise along `x`.
float noise(float x, float seed) {
    float i = floor(x);
    float f = x - i;
    return mix(h(vec2(i, seed)), h(vec2(i + 1.0, seed)), f * f * (3.0 - 2.0 * f));
}

void main() {
    vec2 uv = v_px / screen;
    vec3 colour = mix(top, bottom, uv.y);
    float x = uv.x * screen.x / screen.y;
    for (int i = 0; i < 3; i++) {
        float f = float(i);
        float xs = x + time * 0.012 * (1.0 + 0.5 * f);
        float centre = 0.28 + 0.13 * f
            + 0.05 * sin(xs * 1.3 + time * 0.11 + f * 2.1)
            + 0.07 * (noise(xs * 2.2 + time * 0.03, 11.0 + f) - 0.5);
        float d = uv.y - centre;
        float folds = 0.25 + 0.75 * (0.6 * noise(xs * 9.0 + time * 0.08, 21.0 + f)
            + 0.4 * noise(xs * 23.0 - time * 0.05, 31.0 + f));
        float rise = 0.08 + 0.16 * noise(xs * 4.0 + time * 0.04, 41.0 + f);
        float above = exp(-d * d / (rise * rise)) * folds;
        float below = exp(-d * d / 0.0045) * (0.6 + 0.4 * folds);
        float curtain = mix(above, below, smoothstep(-0.03, 0.03, d));
        float shimmer = 0.8 + 0.2 * sin(time * 0.5 + xs * 1.5 + f);
        vec3 veil = i == 0 ? veil0 : (i == 1 ? veil1 : veil2);
        float weight = i == 0 ? 0.42 : (i == 1 ? 0.72 : 0.36);
        colour += veil * curtain * shimmer * weight;
    }
    colour = min(colour, vec3(1.0));
    float size = 24.0 * scale;
    vec2 c = floor(v_px / size);
    if (h(c) >= 0.92) {
        vec2 s = (c + vec2(0.2 + 0.6 * h(vec2(c.x + 1.7, c.y)), 0.2 + 0.6 * h(vec2(c.x, c.y + 2.3)))) * size;
        float core = clamp(0.9 * scale - length(v_px - s) + 0.5, 0.0, 1.0);
        float twinkle = 0.7 + 0.3 * sin(time * 1.3 + 6.2831853 * h(vec2(c.x + 4.1, c.y)));
        colour = mix(colour, star, core * (0.4 + 0.6 * h(vec2(c.x, c.y + 7.7))) * twinkle);
    }
    gl_FragColor = vec4(colour, 1.0);
}
";

/// The wallpaper's rain: the same recipe as `hocus_link::moving`'s
/// `rain_cell` and `rain_glyph`, each pixel its cell's.
const WALL_RAIN: &str = "
uniform vec2 screen;
uniform vec2 cell;
uniform float tick;
uniform vec3 head;
uniform vec3 tail;
uniform vec3 top;
uniform vec3 bottom;

void main() {
    vec3 bg = mix(top, bottom, v_px.y / screen.y);
    float col = floor(v_px.x / cell.x);
    float row = floor(v_px.y / cell.y);
    float rows = ceil(screen.y / cell.y);
    float bright = 0.0;
    float lead = 0.0;
    for (int i = 0; i < 2; i++) {
        float w = float(i);
        float len = 8.0 + floor(h(vec2(col, 11.0 + w)) * 18.0);
        float period = rows + len + floor(h(vec2(col, 23.0 + w)) * rows * 1.5);
        float offset = floor(h(vec2(col, 37.0 + w)) * period);
        float behind = mod(tick + offset, period) - row;
        if (behind >= 0.0 && behind < len) {
            if (behind < 1.0) {
                lead = 1.0;
                bright = 1.0;
            } else if (lead < 0.5) {
                bright = max(bright, 1.0 - floor(behind * 6.0 / len) / 6.0);
            }
        }
    }
    if (bright <= 0.0) {
        gl_FragColor = vec4(bg, 1.0);
        return;
    }
    float epoch = lead > 0.5 ? tick : floor((tick + h(vec2(col, row)) * 40.0) / 40.0);
    epoch = mod(epoch, 97.0);
    float g = min(floor(h(vec2(col + epoch * 0.37, row + epoch * 0.71)) * 64.0), 63.0);
    float m = glyph(g, fract(v_px / cell), cell);
    vec3 colour = lead > 0.5 ? head : tail;
    gl_FragColor = vec4(mix(bg, colour, m * bright), 1.0);
}
";

/// The stars: `hocus_link::moving`'s `star_at` over its `haze`, each
/// pixel its cell's star in each of the three planes.
const WALL_STARS: &str = "
uniform vec2 screen;
uniform float time;
uniform float scale;
uniform vec3 star;
uniform vec3 top;
uniform vec3 bottom;
uniform vec3 haze;

float cloud(vec2 uv, vec2 at, float r) {
    float d = length(uv - at) / r;
    float k = max(1.0 - d, 0.0);
    return k * k;
}

void main() {
    vec2 uv = v_px / screen;
    vec3 colour = mix(top, bottom, uv.y);
    float a = min(0.35 * cloud(uv, vec2(0.3, 0.35), 0.45) + 0.25 * cloud(uv, vec2(0.75, 0.7), 0.4), 1.0);
    colour = mix(colour, haze, a);
    float half_ = min(screen.x, screen.y) / 2.0;
    vec2 q = (v_px - screen / 2.0) / half_;
    for (int i = 0; i < 3; i++) {
        float plane = float(i);
        float phase = time / 180.0 + plane / 3.0;
        float depth = fract(phase);
        float epoch = floor(phase);
        float zoom = 0.5 * pow(4.0, depth);
        vec2 s = q / zoom * 9.0;
        vec2 c = floor(s);
        float seed = epoch * 7.13 + plane * 3.7;
        if (h(vec2(c.x + seed, c.y)) > 0.55) {
            continue;
        }
        vec2 o = vec2(0.2 + 0.6 * h(vec2(c.x, c.y + seed)), 0.2 + 0.6 * h(vec2(c.x + seed, c.y + 1.3)));
        vec2 p = (c + o) / 9.0 * zoom;
        float fade = smoothstep(0.0, 0.15, depth) * (1.0 - smoothstep(0.8, 1.0, depth));
        float twinkle = 0.65 + 0.35 * sin(time * (1.5 + 3.0 * h(vec2(c.x, c.y + 5.1))) + 6.2831853 * h(vec2(c.x + 2.7, c.y)));
        float r = (0.6 + 1.2 * h(vec2(c.x + 9.1, c.y + seed))) * (0.5 + depth) * scale;
        float d = length(v_px - (screen / 2.0 + p * half_));
        float core = clamp(r - d + 0.5, 0.0, 1.0);
        float glow = 0.25 * exp(-d * d / (r * r * 6.0 + 1.0));
        colour = mix(colour, star, clamp((core + glow) * fade * twinkle, 0.0, 1.0));
    }
    gl_FragColor = vec4(colour, 1.0);
}
";

/// A program's place and its uniforms' places, by name.
#[derive(Debug, Clone)]
struct Program {
    program: ffi::types::GLuint,
    position: ffi::types::GLint,
    local: ffi::types::GLint,
    uniforms: Vec<(&'static str, ffi::types::GLint)>,
}

impl Program {
    fn at(&self, name: &str) -> ffi::types::GLint {
        self.uniforms
            .iter()
            .find(|(n, _)| *n == name)
            .map_or(-1, |(_, at)| *at)
    }
}

/// The programs and the atlas of a renderer's context, made once; `None`
/// where they do not compile, and nothing is drawn by them.
#[derive(Debug)]
struct Made(Option<Glyphs>);

#[derive(Debug, Clone)]
struct Glyphs {
    rain: Program,
    wall_rain: Program,
    stars: Program,
    frost: Program,
    aurora: Program,
    atlas: GlesTexture,
}

/// Compiles and links `fragment` with the vertex shader, and finds
/// `uniforms`.
///
/// # Safety
///
/// The renderer's context must be current, as `with_context` makes it.
#[allow(unsafe_code)]
unsafe fn compile(
    gl: &ffi::Gles2,
    fragment: &str,
    uniforms: &[&'static str],
) -> Result<Program, String> {
    // SAFETY: called with the context current; every pointer handed to GL
    // points to a live local for the duration of the call.
    unsafe {
        let shader = |kind: ffi::types::GLenum, src: &str| {
            let id = gl.CreateShader(kind);
            let ptr = src.as_ptr() as *const ffi::types::GLchar;
            let len = src.len() as ffi::types::GLint;
            gl.ShaderSource(id, 1, &ptr, &len);
            gl.CompileShader(id);
            let mut ok = 0;
            gl.GetShaderiv(id, ffi::COMPILE_STATUS, &mut ok);
            if ok == 0 {
                let mut log = vec![0u8; 2048];
                let mut written = 0;
                gl.GetShaderInfoLog(id, 2048, &mut written, log.as_mut_ptr() as *mut _);
                gl.DeleteShader(id);
                log.truncate(written.max(0) as usize);
                Err(String::from_utf8_lossy(&log).into_owned())
            } else {
                Ok(id)
            }
        };
        let vs = shader(ffi::VERTEX_SHADER, VERTEX)?;
        let fs = match shader(ffi::FRAGMENT_SHADER, fragment) {
            Ok(fs) => fs,
            Err(err) => {
                gl.DeleteShader(vs);
                return Err(err);
            }
        };
        let program = gl.CreateProgram();
        gl.AttachShader(program, vs);
        gl.AttachShader(program, fs);
        gl.LinkProgram(program);
        gl.DetachShader(program, vs);
        gl.DetachShader(program, fs);
        gl.DeleteShader(vs);
        gl.DeleteShader(fs);
        let mut ok = 0;
        gl.GetProgramiv(program, ffi::LINK_STATUS, &mut ok);
        if ok == 0 {
            gl.DeleteProgram(program);
            return Err("the program does not link".to_owned());
        }
        let name = |n: &str| std::ffi::CString::new(n).unwrap_or_default();
        let attrib = |n: &str| gl.GetAttribLocation(program, name(n).as_ptr());
        let found = uniforms
            .iter()
            .map(|n| (*n, gl.GetUniformLocation(program, name(n).as_ptr())))
            .collect();
        Ok(Program {
            program,
            position: attrib("position"),
            local: attrib("local"),
            uniforms: found,
        })
    }
}

/// The glyphs' programs and atlas of `renderer`'s context, made on first
/// use.
fn glyphs(renderer: &mut GlesRenderer) -> Option<Glyphs> {
    if renderer.egl_context().user_data().get::<Made>().is_none() {
        let made = make(renderer)
            .map_err(|err| {
                tracing::warn!(%err, "the glyph shaders do not compile: no rain, wallpapers still");
            })
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

fn make(renderer: &mut GlesRenderer) -> Result<Glyphs, String> {
    let (w, h, pixels) = hocus_link::moving::atlas();
    let atlas = renderer
        .import_memory(&pixels, Fourcc::Abgr8888, (w, h).into(), false)
        .map_err(|e| e.to_string())?;
    let source = |body: &str| format!("{COMMON}{body}");
    let (rain, wall, stars) = (source(RAIN), source(WALL_RAIN), source(WALL_STARS));
    let (frost, aurora) = (source(FROST), source(WALL_AURORA));
    #[allow(unsafe_code)]
    // SAFETY: `with_context` makes the renderer's context current.
    let programs = renderer
        .with_context(|gl| unsafe {
            Ok::<_, String>((
                compile(
                    gl,
                    &rain,
                    &[
                        "matrix", "glyphs", "tex", "size", "win", "cell", "t", "closing", "seed",
                        "head", "tail", "alpha",
                    ],
                )?,
                compile(
                    gl,
                    &wall,
                    &[
                        "matrix", "glyphs", "screen", "cell", "tick", "head", "tail", "top",
                        "bottom",
                    ],
                )?,
                compile(
                    gl,
                    &stars,
                    &[
                        "matrix", "glyphs", "screen", "time", "scale", "star", "top", "bottom",
                        "haze",
                    ],
                )?,
                compile(
                    gl,
                    &frost,
                    &[
                        "matrix", "glyphs", "tex", "size", "win", "t", "closing", "seed", "ice",
                        "crack", "alpha", "px",
                    ],
                )?,
                compile(
                    gl,
                    &aurora,
                    &[
                        "matrix", "glyphs", "screen", "time", "scale", "top", "bottom", "veil0",
                        "veil1", "veil2", "star",
                    ],
                )?,
            ))
        })
        .map_err(|e| e.to_string())??;
    Ok(Glyphs {
        rain: programs.0,
        wall_rain: programs.1,
        stars: programs.2,
        frost: programs.3,
        aurora: programs.4,
        atlas,
    })
}

fn rgb(c: [u8; 3]) -> [f32; 3] {
    c.map(|v| f32::from(v) / 255.0)
}

/// Draws `program` over `rects` (in `geometry`'s space, physical), the
/// frame's projection placing `geometry` at `dst`; each vertex's `local`
/// is its place in `geometry`'s space less `origin`. The textures are
/// bound and the uniforms set by `set` first.
///
/// # Safety
///
/// The frame's context must be current, as `with_context` makes it.
#[allow(unsafe_code)]
unsafe fn quads(
    gl: &ffi::Gles2,
    program: &Program,
    matrix: &[f32; 9],
    instancing: bool,
    rects: &[Rectangle<i32, Physical>],
    origin: Point<i32, Physical>,
    set: impl FnOnce(&ffi::Gles2),
) {
    // At most this many rectangles drawn apart; more, their bounds.
    const MOST: usize = 8;
    let mut vertices = [0f32; MOST * 6 * 4];
    let bounds;
    let rects = if rects.len() > MOST {
        bounds = rects
            .iter()
            .copied()
            .reduce(|a, b| a.merge(b))
            .unwrap_or_default();
        std::slice::from_ref(&bounds)
    } else {
        rects
    };
    let mut n = 0;
    for r in rects {
        let (x0, y0) = (r.loc.x as f32, r.loc.y as f32);
        let (x1, y1) = (x0 + r.size.w as f32, y0 + r.size.h as f32);
        for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x1, y0), (x1, y1), (x0, y1)] {
            let (lx, ly) = (x - origin.x as f32, y - origin.y as f32);
            vertices[n * 4..n * 4 + 4].copy_from_slice(&[x, y, lx, ly]);
            n += 1;
        }
    }
    // SAFETY: the context is current; the vertices outlive the draw call,
    // and every attribute enabled here is disabled again, as smithay's own
    // draws expect.
    unsafe {
        gl.UseProgram(program.program);
        gl.UniformMatrix3fv(program.at("matrix"), 1, ffi::FALSE, matrix.as_ptr());
        set(gl);
        gl.BindBuffer(ffi::ARRAY_BUFFER, 0);
        let stride = (4 * std::mem::size_of::<f32>()) as ffi::types::GLsizei;
        let (pos, local) = (program.position as u32, program.local as u32);
        gl.EnableVertexAttribArray(pos);
        gl.EnableVertexAttribArray(local);
        if instancing {
            gl.VertexAttribDivisor(pos, 0);
            gl.VertexAttribDivisor(local, 0);
        }
        gl.VertexAttribPointer(
            pos,
            2,
            ffi::FLOAT,
            ffi::FALSE,
            stride,
            vertices.as_ptr() as *const _,
        );
        gl.VertexAttribPointer(
            local,
            2,
            ffi::FLOAT,
            ffi::FALSE,
            stride,
            vertices.as_ptr().add(2) as *const _,
        );
        gl.DrawArrays(ffi::TRIANGLES, 0, n as ffi::types::GLsizei);
        gl.DisableVertexAttribArray(pos);
        gl.DisableVertexAttribArray(local);
        gl.ActiveTexture(ffi::TEXTURE1);
        gl.BindTexture(ffi::TEXTURE_2D, 0);
        gl.ActiveTexture(ffi::TEXTURE0);
        gl.BindTexture(ffi::TEXTURE_2D, 0);
    }
}

/// Binds `texture` to unit `unit` with nearest (`sharp`) or linear
/// sampling.
///
/// # Safety
///
/// The context must be current.
#[allow(unsafe_code)]
unsafe fn bind(gl: &ffi::Gles2, unit: u32, texture: ffi::types::GLuint, sharp: bool) {
    let filter = if sharp { ffi::NEAREST } else { ffi::LINEAR } as i32;
    // SAFETY: the context is current.
    unsafe {
        gl.ActiveTexture(ffi::TEXTURE0 + unit);
        gl.BindTexture(ffi::TEXTURE_2D, texture);
        gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_MIN_FILTER, filter);
        gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_MAG_FILTER, filter);
        gl.TexParameteri(
            ffi::TEXTURE_2D,
            ffi::TEXTURE_WRAP_S,
            ffi::CLAMP_TO_EDGE as i32,
        );
        gl.TexParameteri(
            ffi::TEXTURE_2D,
            ffi::TEXTURE_WRAP_T,
            ffi::CLAMP_TO_EDGE as i32,
        );
    }
}

impl Matrix for GlesRenderer {
    fn rain<E: RenderElement<Self>>(
        &mut self,
        rain: &crate::rain::Rain,
        redraw: bool,
        picture: &Picture,
        parts: &[E],
        look: RainLook,
    ) -> Option<RainElement> {
        glyphs(self)?;
        let kept = &rain.picture;
        let size = picture.buffer_size();
        let (texture, kept_rc, fresh) = {
            let mut kept = kept.borrow_mut();
            let reuse = kept
                .as_ref()
                .filter(|(s, _)| *s == size)
                .and_then(|(_, t)| t.downcast_ref::<GlesTexture>().cloned());
            match reuse {
                Some(texture) => (texture, kept.as_ref().map(|(_, t)| t.clone())?, false),
                None => {
                    let texture: GlesTexture =
                        Offscreen::<GlesTexture>::create_buffer(self, Fourcc::Abgr8888, size)
                            .map_err(|err| tracing::warn!(%err, "no picture for a window's rain"))
                            .ok()?;
                    let rc: Rc<dyn Any> = Rc::new(texture.clone());
                    *kept = Some((size, rc.clone()));
                    (texture, rc, true)
                }
            }
        };
        if redraw || fresh {
            let mut target = texture.clone();
            if let Err(err) = crate::wobble::draw_picture(self, &mut target, picture, parts) {
                tracing::warn!(%err, "could not draw a window's rain's picture");
                return None;
            }
        }
        let scale = picture.scale;
        let corner = picture.window.loc + picture.extent.loc;
        let loc = Point::<f64, Physical>::from((corner.x * scale, corner.y * scale));
        let geometry = Rectangle::new(
            (loc.x.round() as i32, loc.y.round() as i32).into(),
            (size.w, size.h).into(),
        );
        Some(RainElement {
            id: rain.id.clone(),
            commit: rain.next_commit(),
            geometry,
            texture: kept_rc,
            look,
        })
    }

    fn draw_rain(
        frame: &mut GlesFrame<'_, '_>,
        rain: &RainElement,
        dst: Rectangle<i32, Physical>,
    ) -> Result<(), GlesError> {
        let Some(texture) = rain.texture.downcast_ref::<GlesTexture>() else {
            return Ok(());
        };
        let Some(glyphs) = frame
            .egl_context()
            .user_data()
            .get::<Made>()
            .and_then(|m| m.0.clone())
        else {
            return Ok(());
        };
        let matrix = crate::wobble::placed(frame.projection(), rain.geometry, dst);
        let instancing = frame.capabilities().contains(&Capability::Instancing);
        let (tex, atlas) = (texture.tex_id(), glyphs.atlas.tex_id());
        let look = rain.look;
        let frost = look.style == crate::rain::Style::Frost;
        let p = if frost { &glyphs.frost } else { &glyphs.rain };
        let size = rain.geometry.size;
        let whole = [rain.geometry];
        frame.with_context(|gl| {
            #[allow(unsafe_code)]
            // SAFETY: the frame's context is current.
            unsafe {
                quads(
                    gl,
                    p,
                    &matrix,
                    instancing,
                    &whole,
                    rain.geometry.loc,
                    |gl| {
                        bind(gl, 1, atlas, true);
                        bind(gl, 0, tex, false);
                        gl.Uniform1i(p.at("tex"), 0);
                        gl.Uniform1i(p.at("glyphs"), 1);
                        gl.Uniform2f(p.at("size"), size.w as f32, size.h as f32);
                        let [x, y, w, h] = look.window;
                        gl.Uniform4f(p.at("win"), x, y, w, h);
                        gl.Uniform2f(p.at("cell"), look.cell.0, look.cell.1);
                        gl.Uniform1f(p.at("t"), look.time);
                        gl.Uniform1f(p.at("closing"), if look.closing { 1.0 } else { 0.0 });
                        gl.Uniform1f(p.at("seed"), look.seed);
                        gl.Uniform1f(p.at("alpha"), look.alpha);
                        gl.Uniform1f(p.at("px"), look.scale);
                        for (name, c) in [
                            ("head", look.colours.head),
                            ("tail", look.colours.tail),
                            ("ice", look.frost.ice),
                            ("crack", look.frost.crack),
                        ] {
                            let [r, g, b] = rgb(c);
                            gl.Uniform3f(p.at(name), r, g, b);
                        }
                    },
                );
            }
        })
    }

    fn draws_walls(&mut self) -> bool {
        glyphs(self).is_some()
    }

    fn draw_wall(
        frame: &mut GlesFrame<'_, '_>,
        wall: &WallElement,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        let Some(glyphs) = frame
            .egl_context()
            .user_data()
            .get::<Made>()
            .and_then(|m| m.0.clone())
        else {
            return Ok(());
        };
        let matrix = crate::wobble::placed(frame.projection(), wall.geometry, dst);
        let instancing = frame.capabilities().contains(&Capability::Instancing);
        let atlas = glyphs.atlas.tex_id();
        let look = wall.look;
        let screen = wall.geometry.size;
        // The damage is the element's own, from its corner; at most eight
        // apart, else their bounds.
        let mut moved = [Rectangle::<i32, Physical>::default(); 8];
        let rects: &[Rectangle<i32, Physical>] = if damage.len() <= moved.len() {
            for (to, d) in moved.iter_mut().zip(damage) {
                *to = Rectangle::new(d.loc + wall.geometry.loc, d.size);
            }
            &moved[..damage.len()]
        } else {
            let bounds = damage
                .iter()
                .copied()
                .reduce(|a, b| a.merge(b))
                .unwrap_or_default();
            moved[0] = Rectangle::new(bounds.loc + wall.geometry.loc, bounds.size);
            &moved[..1]
        };
        let colours = look.colours;
        frame.with_context(|gl| {
            #[allow(unsafe_code)]
            // SAFETY: the frame's context is current.
            unsafe {
                match look.moving {
                    Moving::Rain => {
                        let p = &glyphs.wall_rain;
                        quads(gl, p, &matrix, instancing, rects, wall.geometry.loc, |gl| {
                            bind(gl, 1, atlas, true);
                            gl.Uniform1i(p.at("glyphs"), 1);
                            gl.Uniform2f(p.at("screen"), screen.w as f32, screen.h as f32);
                            gl.Uniform2f(p.at("cell"), look.cell.0, look.cell.1);
                            gl.Uniform1f(p.at("tick"), look.tick);
                            for (name, c) in [
                                ("head", colours.head),
                                ("tail", colours.tail),
                                ("top", colours.top),
                                ("bottom", colours.bottom),
                            ] {
                                let [r, g, b] = rgb(c);
                                gl.Uniform3f(p.at(name), r, g, b);
                            }
                        });
                    }
                    Moving::Stars => {
                        use hocus_link::moving::{STAR, STARS_BOTTOM, STARS_HAZE, STARS_TOP};
                        let p = &glyphs.stars;
                        quads(gl, p, &matrix, instancing, rects, wall.geometry.loc, |gl| {
                            gl.Uniform2f(p.at("screen"), screen.w as f32, screen.h as f32);
                            gl.Uniform1f(
                                p.at("time"),
                                look.tick / hocus_link::moving::STARS_FPS as f32,
                            );
                            gl.Uniform1f(p.at("scale"), look.scale);
                            for (name, c) in [
                                ("star", STAR),
                                ("top", STARS_TOP),
                                ("bottom", STARS_BOTTOM),
                                ("haze", STARS_HAZE),
                            ] {
                                let [r, g, b] = rgb(c);
                                gl.Uniform3f(p.at(name), r, g, b);
                            }
                        });
                    }
                    Moving::Aurora => {
                        use hocus_link::moving::{
                            AURORA_BOTTOM, AURORA_FPS, AURORA_TOP, AURORA_VEILS, STAR,
                        };
                        let p = &glyphs.aurora;
                        quads(gl, p, &matrix, instancing, rects, wall.geometry.loc, |gl| {
                            gl.Uniform2f(p.at("screen"), screen.w as f32, screen.h as f32);
                            gl.Uniform1f(p.at("time"), look.tick / AURORA_FPS as f32);
                            gl.Uniform1f(p.at("scale"), look.scale);
                            for (name, c) in [
                                ("top", AURORA_TOP),
                                ("bottom", AURORA_BOTTOM),
                                ("veil0", AURORA_VEILS[0]),
                                ("veil1", AURORA_VEILS[1]),
                                ("veil2", AURORA_VEILS[2]),
                                ("star", STAR),
                            ] {
                                let [r, g, b] = rgb(c);
                                gl.Uniform3f(p.at(name), r, g, b);
                            }
                        });
                    }
                }
            }
        })
    }
}

impl Element for RainElement {
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
        self.look.alpha
    }

    fn kind(&self) -> Kind {
        Kind::Unspecified
    }
}

impl<R: Matrix> RenderElement<R> for RainElement {
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        _src: Rectangle<f64, BufferCoords>,
        dst: Rectangle<i32, Physical>,
        _damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), R::Error> {
        R::draw_rain(frame, self, dst)
    }
}

impl Element for WallElement {
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
        OpaqueRegions::from_slice(&[Rectangle::from_size(self.geometry.size)])
    }

    fn alpha(&self) -> f32 {
        1.0
    }

    fn kind(&self) -> Kind {
        Kind::Unspecified
    }
}

impl<R: Matrix> RenderElement<R> for WallElement {
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        _src: Rectangle<f64, BufferCoords>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), R::Error> {
        R::draw_wall(frame, self, dst, damage)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_shaders_hash_as_the_still_pictures_do() {
        // The same words: what the CPU's still picture draws is what the
        // shader would at that moment (up to the GPU's rounding).
        assert!(super::COMMON.contains("p.x * 127.1 + p.y * 311.7"));
        assert!(super::COMMON.contains("43758.5453"));
        let v = hocus_link::moving::hash(3.0, 4.0);
        assert!((0.0..1.0).contains(&v));
    }
}
