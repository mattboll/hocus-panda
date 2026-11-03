//! The ring around what is shared (5.11): a 2 px blue ring outside its
//! edge and a soft glow beyond it, breathing over 2.4 s, the mockup's
//! `box-shadow: 0 0 0 2px rgba(91,141,239,a), 0 0 Bpx rgba(91,141,239,g)`
//! from `0.9, 18px, 0.45` to `0.6, 8px, 0.2` and back. It is drawn on the
//! screens only, never in a stream nor a capture.
//!
//! On GLES, a pixel shader of the closed-form CSS, as the halo's; the CPU
//! renderer has no shaders and draws the ring alone, steady. The breath
//! goes in steps ([`STEPS`] a second): an element per step, the same one
//! between, so that the ring costs a few frames a second of its own
//! region, and none at all with reduced motion.

use std::time::Duration;

use smithay::{
    backend::renderer::{
        element::Kind,
        gles::{
            GlesPixelProgram, GlesRenderer, Uniform, UniformName, UniformType,
            element::PixelShaderElement,
        },
    },
    utils::{Logical, Rectangle},
};

use crate::going::ShaderElement;

/// `#5B8DEF`.
pub const BLUE: [u8; 3] = [0x5B, 0x8D, 0xEF];
pub const WIDTH: f64 = 2.0;
/// One breath.
pub const BREATH: Duration = Duration::from_millis(2400);
/// Its steps a second.
pub const STEPS: u32 = 20;

/// The ring at a moment: its alpha, the glow's blur (CSS's) and alpha.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Breath {
    pub ring: f32,
    pub blur: f32,
    pub glow: f32,
}

impl Breath {
    /// The mockup's keyframes' ends.
    pub const FULL: Self = Self {
        ring: 0.9,
        blur: 18.0,
        glow: 0.45,
    };
    pub const LOW: Self = Self {
        ring: 0.6,
        blur: 8.0,
        glow: 0.2,
    };

    /// At `t` since the share started, in whole steps; still: full.
    pub fn at(t: Duration, still: bool) -> (Self, u64) {
        if still {
            return (Self::FULL, 0);
        }
        let step = t.as_millis() as u64 * u64::from(STEPS) / 1000;
        let per = BREATH.as_millis() as u64 * u64::from(STEPS) / 1000;
        let phase = (step % per.max(1)) as f64 / per.max(1) as f64;
        // 0 → 1 → 0, `ease-in-out` each half, as CSS's keyframes at 0, 50
        // and 100 %.
        let half = if phase < 0.5 {
            phase * 2.0
        } else {
            (1.0 - phase) * 2.0
        };
        let k = crate::motion::Bezier(0.42, 0.0, 0.58, 1.0).at(half) as f32;
        let lerp = |a: f32, b: f32| a + (b - a) * k;
        (
            Self {
                ring: lerp(Self::FULL.ring, Self::LOW.ring),
                blur: lerp(Self::FULL.blur, Self::LOW.blur),
                glow: lerp(Self::FULL.glow, Self::LOW.glow),
            },
            step,
        )
    }
}

/// What the shader draws around, CSS's border box, from the element's
/// corner: left, top, right, bottom, and its corners' radius.
const SHADER: &str = r"
precision highp float;
varying vec2 v_coords;
uniform vec2 size;
uniform float alpha;
#ifdef DEBUG_FLAGS
uniform float tint;
#endif
uniform vec4 box;
uniform float radius;
uniform vec3 colour;
uniform vec2 ring;
uniform vec2 glow;
uniform float pixel;

float erf_(float x) {
    float s = sign(x);
    float a = abs(x);
    float t = 1.0 / (1.0 + 0.3275911 * a);
    float y = 1.0 - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t
        - 0.284496736) * t + 0.254829592) * t * exp(-a * a);
    return s * y;
}

float across(float x, float lo, float hi, float sigma) {
    float k = 1.0 / (sigma * 1.41421356);
    return 0.5 * (erf_((x - lo) * k) - erf_((x - hi) * k));
}

float rounded(vec2 p, vec4 b, float r) {
    vec2 centre = 0.5 * (b.xy + b.zw);
    vec2 half_ = 0.5 * (b.zw - b.xy);
    r = min(r, min(half_.x, half_.y));
    vec2 q = abs(p - centre) - (half_ - r);
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

float inset(float y, vec4 b, float r) {
    float dy = max(max(b.y + r - y, y - (b.w - r)), 0.0);
    return r - sqrt(max(r * r - dy * dy, 0.0));
}

float blurred(vec2 p, vec4 b, float r, float sigma) {
    float mass = across(p.y, b.y, b.w, sigma);
    float lo = max(b.y, p.y - 3.0 * sigma);
    float hi = min(b.w, p.y + 3.0 * sigma);
    if (hi <= lo)
        return 0.0;
    if (lo >= b.y + r && hi <= b.w - r)
        return mass * across(p.x, b.x, b.z, sigma);
    float step_ = (hi - lo) / 16.0;
    float sum = 0.0;
    float weights = 0.0;
    for (int i = 0; i < 16; i++) {
        float t = lo + (float(i) + 0.5) * step_;
        float d = (p.y - t) / sigma;
        float w = exp(-0.5 * d * d);
        float k = inset(t, b, r);
        sum += w * across(p.x, b.x + k, b.z - k, sigma);
        weights += w;
    }
    return mass * sum / max(weights, 1e-6);
}

void main() {
    vec2 p = v_coords * size;
    float d = rounded(p, box, radius);
    float inside = clamp(0.5 - d / pixel, 0.0, 1.0);
    float outer = clamp(0.5 - (d - ring.x) / pixel, 0.0, 1.0);
    float r = ring.y * max(outer - inside, 0.0);
    float sigma = max(glow.x, 0.01);
    float g = 0.0;
    if (glow.y > 0.0 && inside < 1.0 && d < 3.0 * sigma)
        g = glow.y * blurred(p, box, radius, sigma) * (1.0 - inside);
    float a = r + g * (1.0 - r);
    gl_FragColor = vec4(colour, 1.0) * a * alpha;
#ifdef DEBUG_FLAGS
    if (tint == 1.0)
        gl_FragColor = vec4(0.0, 0.2, 0.0, 0.2) + gl_FragColor * 0.8;
#endif
}
";

#[derive(Debug, Clone)]
struct Program(Option<GlesPixelProgram>);

/// The ring's program for `renderer`'s context, compiled the first time;
/// `None` when it does not compile (the ring is then drawn plain).
pub fn program(renderer: &mut GlesRenderer) -> Option<GlesPixelProgram> {
    if renderer
        .egl_context()
        .user_data()
        .get::<Program>()
        .is_none()
    {
        let uniforms = [
            UniformName::new("box", UniformType::_4f),
            UniformName::new("radius", UniformType::_1f),
            UniformName::new("colour", UniformType::_3f),
            UniformName::new("ring", UniformType::_2f),
            UniformName::new("glow", UniformType::_2f),
            UniformName::new("pixel", UniformType::_1f),
        ];
        let program = renderer
            .compile_custom_pixel_shader(SHADER, &uniforms)
            .map_err(|err| tracing::warn!(%err, "the share's ring does not compile"))
            .ok();
        renderer
            .egl_context()
            .user_data()
            .insert_if_missing(|| Program(program));
    }
    renderer
        .egl_context()
        .user_data()
        .get::<Program>()
        .and_then(|p| p.0.clone())
}

/// A ring drawn: around what (output-local, logical), its radius, how,
/// at what scale; the same key, the same element.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Key {
    pub around: Rectangle<f64, Logical>,
    pub radius: f64,
    pub breath: Breath,
    pub scale: f64,
}

/// The ring's element for `key`.
pub fn element(program: &GlesPixelProgram, key: &Key) -> ShaderElement {
    let reach = (1.5 * f64::from(Breath::FULL.blur)).ceil() + WIDTH + 2.0;
    let r = key.around;
    let left = (r.loc.x - reach).floor();
    let top = (r.loc.y - reach).floor();
    let right = (r.loc.x + r.size.w + reach).ceil();
    let bottom = (r.loc.y + r.size.h + reach).ceil();
    let area: Rectangle<i32, Logical> = Rectangle::new(
        (left as i32, top as i32).into(),
        ((right - left) as i32, (bottom - top) as i32).into(),
    );
    let [cr, cg, cb] = BLUE.map(|c| f32::from(c) / 255.0);
    let uniforms = vec![
        Uniform::new(
            "box",
            (
                (r.loc.x - left) as f32,
                (r.loc.y - top) as f32,
                (r.loc.x + r.size.w - left) as f32,
                (r.loc.y + r.size.h - top) as f32,
            ),
        ),
        Uniform::new("radius", key.radius as f32),
        Uniform::new("colour", (cr, cg, cb)),
        Uniform::new("ring", (WIDTH as f32, key.breath.ring)),
        Uniform::new("glow", (key.breath.blur / 2.0, key.breath.glow)),
        Uniform::new("pixel", (1.0 / key.scale) as f32),
    ];
    ShaderElement(PixelShaderElement::new(
        program.clone(),
        area,
        None,
        1.0,
        uniforms,
        Kind::Unspecified,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_breathes_from_full_to_low_and_back_in_steps() {
        let ms = Duration::from_millis;
        assert_eq!(Breath::at(ms(0), false).0, Breath::FULL);
        let (low, _) = Breath::at(ms(1200), false);
        assert!((low.ring - Breath::LOW.ring).abs() < 1e-3, "{low:?}");
        assert!((Breath::at(ms(2400), false).0.ring - Breath::FULL.ring).abs() < 1e-3);
        // Within a step, the same.
        assert_eq!(Breath::at(ms(610), false), Breath::at(ms(640), false));
        assert_ne!(Breath::at(ms(610), false).1, Breath::at(ms(660), false).1);
        // Still: full, and no step.
        assert_eq!(Breath::at(ms(1200), true), (Breath::FULL, 0));
    }
}
