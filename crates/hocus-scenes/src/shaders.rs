//! The living wallpapers' shaders, as the compositor draws them on the
//! screen (docs/design/effects.md) and the Hocuspanda camera behind the
//! person (docs/design/camera.md, lot 3): GLSL ES 1.00, one source for
//! both. The compositor compiles them with GLES; the camera adapts them
//! to its GPU's (`hocus-camera`'s `glsl`).

/// A picture's quad: its place through `matrix`, its pixels from its
/// corner (`v_px`).
pub const VERTEX: &str = "#version 100
uniform mat3 matrix;
attribute vec2 position;
attribute vec2 local;
varying vec2 v_px;
void main() {
    gl_Position = vec4(matrix * vec3(position, 1.0), 1.0);
    v_px = local;
}
";

/// Where an effect may draw (the clip): `p` in `keep`'s and the holes'
/// space.
pub const CLIP: &str = "
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
/// colour and their shape, as the particles' vertices lays them.
pub const SPARKS_VERTEX: &str = "#version 100
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

/// The shapes of a mesh: a fill as its vertices say; a flake, a
/// soft disc; a clump, an ellipse lit from its upper left (the board's
/// two-point gradient, white from a focus at (-0.3, -0.3) to blue at its
/// edge); a box blurred by a Gaussian (or sharp, its edges smoothed); a
/// box with round corners, an oval and a glow (the garland's, #799; a
/// firefly's, #804, its own colours; a firework's spark, #810, and one
/// in the bar, #800); a drop of the rain's map (#802); a lily pad or its
/// shadow (#803). Nothing where the clip says not, nor out of its round
/// corners when it has some.
pub const SHAPES: &str = "#version 100
precision highp float;
varying vec2 v_local;
varying vec4 v_colour;
varying vec4 v_shape;
varying vec2 v_at;
uniform float corner;

float within_corners(vec2 p) {
    if (corner < 0.5) {
        return 1.0;
    }
    vec2 middle = (keep.xy + keep.zw) * 0.5;
    vec2 half_size = (keep.zw - keep.xy) * 0.5;
    vec2 q = abs(p - middle) - (half_size - vec2(corner));
    float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - corner;
    return clamp(0.5 - d, 0.0, 1.0);
}

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
    } else if (kind < 3.5) {
        float cx = blurred(v_shape.y, v_local.x, v_shape.w);
        float cy = blurred(v_shape.z, v_local.y, v_shape.w);
        gl_FragColor = v_colour * cx * cy;
    } else if (kind < 4.5) {
        vec2 q = abs(v_local) - (v_shape.yz - vec2(v_shape.w));
        float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - v_shape.w;
        gl_FragColor = v_colour * clamp(0.5 - d, 0.0, 1.0);
    } else if (kind < 5.5) {
        float cover = clamp((1.0 - length(v_local)) * v_shape.y + 0.5, 0.0, 1.0);
        gl_FragColor = v_colour * cover;
    } else if (kind > 7.5) {
        vec2 p = v_local;
        float d = length(p);
        float a = atan(p.y, p.x);
        float shadow = v_shape.z;
        float cover = clamp((1.0 - d) * v_shape.y + 0.5, 0.0, 1.0);
        float notch = shadow > 0.5 ? 0.2 : 0.22;
        cover *= clamp((abs(a) - notch) * d * v_shape.y + 0.5, 0.0, 1.0);
        if (shadow > 0.5) {
            gl_FragColor = v_colour * cover;
        } else {
            float r0 = 2.0 * v_shape.w / v_shape.y;
            vec2 e = vec2(0.3, 0.3);
            float dr = 1.0 - r0;
            vec2 q = p + e;
            float qa = dot(e, e) - dr * dr;
            float qb = dot(q, e) + r0 * dr;
            float qc = dot(q, q) - r0 * r0;
            float t = clamp((qb - sqrt(max(qb * qb - qa * qc, 0.0))) / qa, 0.0, 1.0);
            vec3 c = mix(vec3(111.0, 174.0, 92.0), vec3(47.0, 107.0, 58.0), t) / 255.0;
            float a2 = a < 0.0 ? a + 6.2831853 : a;
            float k = clamp(floor((a2 - 0.4) / 0.62 + 0.5), 0.0, 8.0);
            float off = a2 - (0.4 + k * 0.62);
            float far = d * v_shape.y;
            float dist = cos(off) > 0.0 ? far * abs(sin(off)) : far;
            float vein = clamp(1.0 - dist / v_shape.w, 0.0, 1.0)
                * clamp((0.92 - d) * v_shape.y + 0.5, 0.0, 1.0) * 0.25;
            c = mix(c, vec3(190.0, 230.0, 170.0) / 255.0, vein);
            gl_FragColor = vec4(c, 1.0) * cover * v_colour.a;
        }
    } else if (kind > 6.5) {
        float r = length(v_local);
        float a = clamp((1.0 - r) * 6.0, 0.0, 1.0);
        gl_FragColor = vec4(v_local * 0.5 + 0.5, sqrt(max(1.0 - r * r, 0.0)), 1.0) * a;
    } else if (v_shape.y > 4.5) {
        float d = length(v_local);
        vec4 c = v_colour;
        gl_FragColor = d < 0.25 ? mix(vec4(c.a), c * 0.9, d / 0.25)
            : mix(c * 0.9, vec4(0.0), clamp((d - 0.25) / 0.75, 0.0, 1.0));
    } else if (v_shape.y > 3.5) {
        float d = length(v_local);
        vec4 c = v_colour;
        vec4 w = vec4(c.a);
        gl_FragColor = d < 0.15 ? mix(w, c * 0.9, d / 0.15)
            : d < 0.45 ? mix(c * 0.9, c * 0.22, (d - 0.15) / 0.3)
            : mix(c * 0.22, vec4(0.0), clamp((d - 0.45) / 0.55, 0.0, 1.0));
    } else if (v_shape.y > 2.5) {
        float d = length(v_local);
        vec4 s0 = vec4(1.0, 1.0, 220.0 / 255.0, 1.0);
        vec4 s1 = vec4(230.0 / 255.0, 1.0, 140.0 / 255.0, 1.0) * 0.95;
        vec4 s2 = vec4(190.0 / 255.0, 240.0 / 255.0, 90.0 / 255.0, 1.0) * 0.35;
        vec4 s3 = vec4(150.0 / 255.0, 220.0 / 255.0, 60.0 / 255.0, 1.0) * 0.08;
        vec4 c = d < 0.08 ? mix(s0, s1, d / 0.08)
            : d < 0.25 ? mix(s1, s2, (d - 0.08) / 0.17)
            : d < 0.6 ? mix(s2, s3, (d - 0.25) / 0.35)
            : mix(s3, vec4(0.0), clamp((d - 0.6) / 0.4, 0.0, 1.0));
        gl_FragColor = c * v_colour.a;
    } else {
        float d = length(v_local);
        float a;
        if (v_shape.y < 0.5) {
            a = d < 0.12 ? mix(0.55, 0.32, d / 0.12)
                : d < 0.35 ? mix(0.32, 0.10, (d - 0.12) / 0.23)
                : d < 0.7 ? mix(0.10, 0.025, (d - 0.35) / 0.35)
                : mix(0.025, 0.0, clamp((d - 0.7) / 0.3, 0.0, 1.0));
        } else if (v_shape.y < 1.5) {
            a = d < 0.3 ? mix(1.0, 0.85, d / 0.3)
                : d < 0.6 ? mix(0.85, 0.25, (d - 0.3) / 0.3)
                : mix(0.25, 0.0, clamp((d - 0.6) / 0.4, 0.0, 1.0));
        } else {
            a = clamp(1.0 - d, 0.0, 1.0);
        }
        gl_FragColor = v_colour * a;
    }
    gl_FragColor *= within_corners(v_at);
}
";

/// The board's `KOI_FS` (#803): what is under the water seen through it,
/// bent by its heights' normal, tinted, lit where the water's surface
/// curves up, with a highlight from the upper left and the sky's sheen
/// toward the top. Its heights, in the board's floats (or, without them,
/// its bytes about a half); `still`, flat water.
pub const KOI: &str = "#version 100
precision highp float;
uniform sampler2D under;
uniform sampler2D heights;
uniform vec2 screen;
uniform vec2 grid;
uniform vec2 texel;
uniform vec2 bend;
uniform float zero;
uniform float still;
varying vec2 v_px;

float h(vec2 p) { return still > 0.5 ? 0.0 : texture2D(heights, p).r - zero; }
void main() {
    vec2 uv = v_px / screen;
    vec2 q = uv * grid;
    vec2 tx = vec2(texel.x, 0.0);
    vec2 ty = vec2(0.0, texel.y);
    float c = h(q);
    float r = h(q + tx);
    float l = h(q - tx);
    float d = h(q + ty);
    float u = h(q - ty);
    float dx = r - l;
    float dy = d - u;
    float lap = r + l + d + u - 4.0 * c;
    vec3 n = normalize(vec3(-dx * 4.0, -dy * 4.0, 1.0));
    vec3 seen = texture2D(under, clamp(uv + n.xy * 0.035 * bend, 0.0, 1.0)).rgb;
    vec3 water = mix(seen, seen * vec3(0.55, 0.85, 0.85) + vec3(0.0, 0.03, 0.04), 0.35);
    water += vec3(0.75, 0.95, 1.0) * smoothstep(0.004, 0.06, -lap) * 0.28;
    vec3 lt = normalize(vec3(-0.35, -0.55, 0.75));
    float spec = pow(max(0.0, dot(reflect(-lt, n), vec3(0.0, 0.0, 1.0))), 60.0);
    water += vec3(1.0, 0.98, 0.92) * spec * 0.9;
    float sky = smoothstep(0.0, 1.0, 1.0 - uv.y) * 0.06;
    water += vec3(0.55, 0.75, 0.85) * sky;
    gl_FragColor = vec4(water, 1.0);
}
";

/// The board "La lumière du jour"'s sky (#805), its canvas drawn as one
/// shader, in its logical pixels: the gradient (top, bottom at 75 %, the
/// bottom half darkened at the foot), its stars (1.4 px squares at its
/// places, `(i × 7919) mod 1200` and `(i × 4513) mod 432` over its 1200 ×
/// 720, stretched over the screen: on another width the board's own
/// formula lines them up), by day the sun's
/// halo (380 px: 55 %, 22 % at 15 %, nothing), its disc (26 px, the light
/// whitened 40 %) and its seven rays added (from the sun toward the
/// screen's bottom middle, 0.09 apart, 0.06 wide, fading out over
/// 1100 px), by night the moon's halo and disc; the two ridges; the hour's
/// light graded over it all (soft light), as the board's grading canvas.
pub const SKY: &str = "#version 100
precision highp float;
uniform vec2 size;
uniform float px;
uniform vec3 top;
uniform vec3 bottom;
uniform vec3 light;
uniform float grade;
uniform vec2 sun;
uniform float up;
uniform float elev;
uniform float night;
uniform float stars;
varying vec2 v_px;

vec3 over(vec3 c, vec3 s, float a) { return c * (1.0 - a) + s * a; }
float disc(vec2 p, vec2 at, float r) { return clamp((r - length(p - at)) * px + 0.5, 0.0, 1.0); }
float ridge(float x, float y0, float amp, float f, float s) {
    return y0 + sin(x * f + s) * amp + sin(x * f * 2.3 + s) * amp * 0.4;
}
float soft(float b, float s) {
    if (s <= 0.5) {
        return b - (1.0 - 2.0 * s) * b * (1.0 - b);
    }
    float d = b <= 0.25 ? ((16.0 * b - 12.0) * b + 4.0) * b : sqrt(b);
    return b + (2.0 * s - 1.0) * (d - b);
}
void main() {
    vec2 p = v_px / px;
    float w = size.x;
    float h = size.y;
    float t = p.y / h;
    vec3 foot = mix(bottom, vec3(10.0, 10.0, 20.0) / 255.0, 0.5);
    vec3 c = t < 0.75 ? mix(top, bottom, t / 0.75) : mix(bottom, foot, clamp((t - 0.75) / 0.25, 0.0, 1.0));
    if (night > 0.0 && p.y < h * 0.6 + 2.0) {
        float half_px = 0.5 / px;
        for (int i = 0; i < 1024; i++) {
            float fi = float(i);
            if (fi >= stars) {
                break;
            }
            vec2 s = vec2(mod(fi * 7919.0, 1200.0) / 1200.0 * w, mod(fi * 4513.0, 432.0) / 432.0 * h * 0.6);
            vec2 lo = max(p - half_px, s);
            vec2 hi = min(p + half_px, s + 1.4);
            float cover = max(hi.x - lo.x, 0.0) * max(hi.y - lo.y, 0.0) * px * px;
            if (cover > 0.0) {
                float a = 0.5 * night * (0.4 + 0.6 * mod(fi * 37.0, 10.0) / 10.0);
                c = over(c, vec3(230.0, 235.0, 255.0) / 255.0, a * min(cover, 1.0));
            }
        }
    }
    if (up > 0.5) {
        float d = length(p - sun);
        float a = d < 57.0 ? mix(0.55, 0.22, d / 57.0) : mix(0.22, 0.0, clamp((d - 57.0) / 323.0, 0.0, 1.0));
        c = over(c, light, a);
        c = over(c, mix(light, vec3(1.0), 0.4), disc(p, sun, 26.0));
        vec2 v = p - sun;
        float dist = length(v);
        float base = atan(h - sun.y, w / 2.0 - sun.x);
        float ang = atan(v.y, v.x);
        float strength = 0.07 * (1.0 - elev * 0.6);
        for (int r = 0; r < 7; r++) {
            float off = ang - (base + (float(r) - 3.0) * 0.09);
            off = mod(off + 3.14159265, 6.2831853) - 3.14159265;
            float inside = clamp((0.03 - abs(off)) * dist * px + 0.5, 0.0, 1.0);
            float along = dist * cos(off);
            float fall = clamp(1.0 - along / 1100.0, 0.0, 1.0);
            c += light * strength * fall * inside * step(0.0, along);
        }
    } else {
        vec2 moon = vec2(w * 0.72, h * 0.18);
        float a = 0.25 * clamp(1.0 - length(p - moon) / 160.0, 0.0, 1.0);
        c = over(c, vec3(200.0, 210.0, 255.0) / 255.0, a);
        c = over(c, vec3(232.0, 236.0, 248.0) / 255.0, disc(p, moon, 16.0));
    }
    float y1 = ridge(p.x, h * 0.72, 26.0, 0.005, 1.0);
    c = over(c, mix(bottom, vec3(22.0, 20.0, 40.0) / 255.0, 0.74), clamp((p.y - y1) * px + 0.5, 0.0, 1.0));
    float y2 = ridge(p.x, h * 0.82, 20.0, 0.008, 3.0);
    c = over(c, mix(bottom, vec3(10.0, 10.0, 22.0) / 255.0, 0.88), clamp((p.y - y2) * px + 0.5, 0.0, 1.0));
    c = clamp(c, 0.0, 1.0);
    vec3 g = vec3(soft(c.r, light.r), soft(c.g, light.g), soft(c.b, light.b));
    gl_FragColor = vec4(mix(c, g, grade), 1.0);
}
";

/// The board "Bureaux en profondeur"'s landscape (#806), its canvases
/// drawn as one shader, in its logical pixels. The sky: its gradient
/// (`stops` at 0, 45 %, 72 % and the foot; the board's `#1C1B44`,
/// `#5B3E73`, `#D9786A`, `#F4B27A`) and its sun's glow (260 px round,
/// `glow`'s three colours at 95 %, 60 % at 12 %, nothing; the board's at
/// 62 % of the screen's width and 200 px, 52 % of its height), drifting
/// with the camera. With the daylight on (#822) they are its sky's at its
/// hour, and with the sun down the moon stands in its place (the
/// daylight's halo and disc) and the daylight's stars show: the board's
/// places, `(i × 7919) mod 1200` across, where a column of its 1200 holds
/// at most one star, `i = (column × 479) mod 1200` (7919 × 479 ≡ 1), so
/// that a pixel looks at the two or three columns that may reach it, not
/// at every star. Over it six layers, back to front, each moved by its
/// share of the camera's travel (`f`) and pulled back about the screen's
/// middle by its share of the camera's pull: the board's silhouettes
/// (peaks, hills, ground under trees: sums of sines over its own phases),
/// its trees (triangles, 18 to 58 px tall, 1.6 times on the layer that
/// moves with the windows, 2.6 on the nearest; a cell each, where the
/// board's canvas went on by a random step: 6 to 24 px, 90 to 350 px for
/// the nearest), tinted toward `tint` and darkened to `dim`, the haze
/// over the far ones (`haze`, `0.18 (1 - f)` at the foot). Below the
/// screen the layers go on, as the board's 1.35 screens tall did. Composed
/// front to back: where the near layers cover a pixel whole, the far ones
/// and the sky are not looked at; a pixel looks at the trees of the cells
/// that may reach it alone. The hour's light graded over it all (soft
/// light, as the daylight's sky), when `grade` is not 0.
pub const DEPTH: &str = "#version 100
precision highp float;
uniform vec2 size;
uniform float px;
uniform float cam;
uniform float zoom;
uniform float drift;
uniform vec3 ph[6];
uniform vec3 stops[4];
uniform vec3 glow[3];
uniform vec2 sun;
uniform float up;
uniform vec3 haze;
uniform vec3 tint;
uniform float tint_k;
uniform float dim;
uniform vec3 light;
uniform float grade;
uniform float night;
uniform float stars;
varying vec2 v_px;

float hash(float n) { return fract(sin(n * 91.3458 + 47.21) * 43758.5453); }

vec3 over(vec3 c, vec3 s, float a) { return c * (1.0 - a) + s * a; }
float disc(vec2 p, vec2 at, float r) { return clamp((r - length(p - at)) * px + 0.5, 0.0, 1.0); }
float soft(float b, float s) {
    if (s <= 0.5) {
        return b - (1.0 - 2.0 * s) * b * (1.0 - b);
    }
    float d = b <= 0.25 ? ((16.0 * b - 12.0) * b + 4.0) * b : sqrt(b);
    return b + (2.0 * s - 1.0) * (d - b);
}

/// A silhouette's colour: the board's, tinted by the hour, darkened by
/// the night, hazy by `k`.
vec3 shade(vec3 board, float k) {
    return mix(mix(board, tint, tint_k) * dim, haze, k);
}

float ground(float x, float base, float amp, vec3 p, float kind) {
    if (kind < 0.5) {
        return base - amp * (0.62 * pow(1.0 - abs(sin(x * 0.0024 + p.x)), 1.7)
            + 0.28 * pow(1.0 - abs(sin(x * 0.0063 + p.y)), 2.0)
            + 0.06 * sin(x * 0.031 + p.z));
    }
    return base - amp * (0.6 * sin(x * 0.0026 + p.x) + 0.3 * sin(x * 0.0071 + p.y)
        + 0.1 * sin(x * 0.023 + p.z));
}

/// How much of the layer covers `p` (logical, on the screen), and how
/// hazy it is there. `kind`: 0 peaks, 1 hills, 2 ground under trees, the
/// trees of the cells `reach` (from, to) around the pixel's looked at.
float layer(vec2 p, float f, float level, float amp, vec3 phase, float kind,
        float cell, float jitter, float tall, float seed, vec2 reach, out float hz) {
    float z = 1.0 - (1.0 - zoom) * f;
    vec2 c = size * 0.5;
    vec2 q = (p - c) / z + c;
    float x = q.x + cam * f + 20.0;
    float y = q.y;
    float s = px * z;
    float base = size.y * level;
    float a = clamp((y - ground(x, base, amp, phase, kind)) * s + 0.5, 0.0, 1.0);
    if (kind > 1.5 && a < 1.0 && y > base - amp + 5.0 - 58.0 * tall) {
        float i0 = floor(x / cell);
        for (int k = -3; k <= 3; k++) {
            if (float(k) < reach.x || float(k) > reach.y) {
                continue;
            }
            float i = i0 + float(k);
            float tx = (i + 0.5 + jitter * (hash(i + seed) - 0.5)) * cell;
            float h = (18.0 + 40.0 * hash(i * 1.7 + seed + 3.1)) * tall;
            float y0 = ground(tx, base, amp, phase, kind) + 6.0;
            float top = y0 - h;
            if (y > top - 1.0 && y < y0) {
                float d = (y - top) * 0.28 - abs(x - tx);
                a = max(a, clamp(d * 0.963 * s + 0.5, 0.0, 1.0));
            }
        }
    }
    float from = base - amp * 1.2;
    hz = 0.18 * max(1.0 - f, 0.0) * clamp((y - from) / (size.y - from), 0.0, 1.0);
    return a;
}

void main() {
    vec2 p = v_px / px;
    // Front to back: what a nearer layer covers whole is not looked at.
    vec3 c = vec3(0.0);
    float covered = 0.0;
    float hz;
    float a = layer(p, 1.55, 1.02, 34.0, ph[5], 2.0, 220.0, 0.6, 2.6, 37.0, vec2(-1.0, 1.0), hz);
    c += (1.0 - covered) * a * shade(vec3(10.0, 10.0, 22.0) / 255.0, 0.0);
    covered += (1.0 - covered) * a;
    if (covered < 0.999) {
        a = layer(p, 1.0, 0.93, 16.0, ph[4], 2.0, 15.0, 0.8, 1.6, 23.0, vec2(-3.0, 2.0), hz);
        c += (1.0 - covered) * a * shade(vec3(20.0, 19.0, 39.0) / 255.0, 0.0);
        covered += (1.0 - covered) * a;
    }
    if (covered < 0.999) {
        a = layer(p, 0.72, 0.84, 26.0, ph[3], 2.0, 15.0, 0.8, 1.0, 11.0, vec2(-2.0, 1.0), hz);
        c += (1.0 - covered) * a * shade(vec3(36.0, 32.0, 61.0) / 255.0, hz);
        covered += (1.0 - covered) * a;
    }
    if (covered < 0.999) {
        a = layer(p, 0.38, 0.76, 46.0, ph[2], 1.0, 1.0, 0.0, 1.0, 0.0, vec2(0.0), hz);
        c += (1.0 - covered) * a * shade(vec3(59.0, 44.0, 85.0) / 255.0, hz);
        covered += (1.0 - covered) * a;
    }
    if (covered < 0.999) {
        a = layer(p, 0.14, 0.68, 110.0, ph[1], 0.0, 1.0, 0.0, 1.0, 0.0, vec2(0.0), hz);
        c += (1.0 - covered) * a * shade(vec3(90.0, 63.0, 110.0) / 255.0, hz);
        covered += (1.0 - covered) * a;
    }
    if (covered < 0.999) {
        a = layer(p, 0.03, 0.62, 150.0, ph[0], 0.0, 1.0, 0.0, 1.0, 0.0, vec2(0.0), hz);
        c += (1.0 - covered) * a * shade(vec3(126.0, 90.0, 134.0) / 255.0, hz);
        covered += (1.0 - covered) * a;
    }
    if (covered < 0.999) {
        float t = clamp(p.y / size.y, 0.0, 1.0);
        vec3 sky = t < 0.45 ? mix(stops[0], stops[1], t / 0.45)
            : t < 0.72 ? mix(stops[1], stops[2], (t - 0.45) / 0.27)
            : mix(stops[2], stops[3], (t - 0.72) / 0.28);
        if (night > 0.0 && p.y < size.y * 0.6 + 2.0) {
            float unit = size.x / 1200.0;
            float half_px = 0.5 / px;
            float bx = mod(p.x + drift, size.x);
            float lo = floor((bx - half_px - 1.4) / unit);
            float hi = floor((bx + half_px) / unit);
            for (int k = 0; k < 8; k++) {
                float col = lo + float(k);
                if (col > hi) {
                    break;
                }
                float fi = mod(mod(col, 1200.0) * 479.0, 1200.0);
                if (fi >= stars) {
                    continue;
                }
                vec2 s = vec2(col * unit, mod(fi * 4513.0, 432.0) / 432.0 * size.y * 0.6);
                vec2 q = vec2(bx, p.y);
                vec2 from = max(q - half_px, s);
                vec2 to = min(q + half_px, s + 1.4);
                float cover = max(to.x - from.x, 0.0) * max(to.y - from.y, 0.0) * px * px;
                if (cover > 0.0) {
                    float sa = 0.5 * night * (0.4 + 0.6 * mod(fi * 37.0, 10.0) / 10.0);
                    sky = over(sky, vec3(230.0, 235.0, 255.0) / 255.0, sa * min(cover, 1.0));
                }
            }
        }
        if (up > 0.5) {
            vec2 at = vec2(sun.x - drift, sun.y);
            float d = length(p - at) / 260.0;
            vec4 g = d < 0.12
                ? mix(vec4(glow[0], 0.95), vec4(glow[1], 0.6), d / 0.12)
                : mix(vec4(glow[1], 0.6), vec4(glow[2], 0.0), clamp((d - 0.12) / 0.88, 0.0, 1.0));
            sky = mix(sky, g.rgb, g.a);
        } else {
            vec2 moon = vec2(size.x * 0.72 - drift, size.y * 0.18);
            float ma = 0.25 * clamp(1.0 - length(p - moon) / 160.0, 0.0, 1.0);
            sky = over(sky, vec3(200.0, 210.0, 255.0) / 255.0, ma);
            sky = over(sky, vec3(232.0, 236.0, 248.0) / 255.0, disc(p, moon, 16.0));
        }
        c += (1.0 - covered) * sky;
    }
    if (grade > 0.0) {
        c = clamp(c, 0.0, 1.0);
        vec3 g = vec3(soft(c.r, light.r), soft(c.g, light.g), soft(c.b, light.b));
        c = mix(c, g, grade);
    }
    gl_FragColor = vec4(c, 1.0);
}
";

/// The board "Aurore en musique"'s night (#801), its `AURORA_FS` from
/// WebGL as it was, its lengths the board's: `size` is its `uRes`, the
/// screen's logical size, `res` the picture's pixels (0.6 of them, as its
/// canvas). Its sky's gradient and twinkling stars, three curtains (each
/// a fold of sines and noise, rippling on the beat, its rays five octaves
/// of noise, green at the foot to violet at the top), the two ridges lit by
/// them, a grain, a vignette and a soft tone curve. One change: the
/// noise's lattice wraps at 1024 cells (the board's for all it showed), so
/// that its hash keeps its precision as its time goes on.
pub const AURORA: &str = "#version 100
precision highp float;
uniform vec2 size;
uniform vec2 res;
uniform float time;
uniform float amp;
uniform float beat;
varying vec2 v_px;

float hash(vec2 p) {
    p = fract(p * vec2(234.34, 435.345));
    p += dot(p, p + 34.23);
    return fract(p.x * p.y);
}
float lattice(vec2 i) { return hash(mod(i, 1024.0)); }
float noise(vec2 p) {
    vec2 i = floor(p), f = fract(p);
    vec2 u = f * f * (3.0 - 2.0 * f);
    return mix(mix(lattice(i), lattice(i + vec2(1.0, 0.0)), u.x),
        mix(lattice(i + vec2(0.0, 1.0)), lattice(i + vec2(1.0, 1.0)), u.x), u.y);
}
float fbm(vec2 p) {
    float v = 0.0, a = 0.5;
    for (int i = 0; i < 5; i++) {
        v += a * noise(p);
        p = p * 2.02 + 11.7;
        a *= 0.5;
    }
    return v;
}
vec3 curtain(vec2 uv, float seed, float base, float t, vec3 low, vec3 high, float gain) {
    float x = uv.x * (size.x / size.y);
    float fold = 0.06 * sin(x * 3.1 + t * 0.45 + seed)
        + 0.05 * (fbm(vec2(x * 0.9 + seed, t * 0.06)) - 0.5) * 2.0;
    float ripple = 0.022 * beat * sin(x * 18.0 - t * 9.0 + seed);
    float b = base + fold + ripple;
    float d = uv.y - b;
    float below = exp(min(d, 0.0) * 55.0);
    float above = exp(-max(d, 0.0) * (6.5 - 2.5 * amp));
    float env = smoothstep(0.38, 0.68, fbm(vec2(x * 0.5 + seed * 3.0, t * 0.025)));
    float warp = fbm(vec2(x * 1.6 + seed, t * 0.09)) * 4.0;
    float rays = fbm(vec2(x * 26.0 + warp, t * 0.35 + seed));
    rays = pow(rays, 2.2) * 2.4;
    float body = below * above * (0.35 + rays);
    float edge = exp(-abs(d) * 38.0) * 0.9;
    vec3 col = mix(low, high * 0.75, smoothstep(0.0, 0.22, d));
    float shimmer = 0.85 + 0.15 * sin(t * 2.0 + x * 9.0 + seed);
    return col * (body + edge * (0.4 + rays)) * gain * shimmer * env;
}
void main() {
    vec2 uv = vec2(v_px.x / res.x, 1.0 - v_px.y / res.y);
    float t = time;
    vec3 sky = mix(vec3(0.035, 0.07, 0.11), vec3(0.008, 0.012, 0.035), smoothstep(0.1, 1.0, uv.y));
    vec2 sp = uv * size / 2.5;
    vec2 cell = floor(sp);
    float h = hash(cell);
    if (h > 0.986) {
        vec2 c = cell + 0.5 + (vec2(hash(cell + 3.1), hash(cell + 7.7)) - 0.5) * 0.6;
        float d = length(sp - c);
        float tw = 0.55 + 0.45 * sin(t * (1.0 + h * 3.0) + h * 60.0);
        sky += vec3(0.85, 0.9, 1.0) * smoothstep(1.1, 0.0, d) * tw * (h - 0.986) * 70.0
            * smoothstep(0.25, 0.6, uv.y);
    }
    float gain = 0.7 + 0.8 * amp;
    vec3 a = curtain(uv, 1.3, 0.50, t, vec3(0.15, 1.0, 0.55), vec3(0.55, 0.25, 0.95), gain * 0.9);
    a += curtain(uv, 4.7, 0.62, t * 0.8, vec3(0.1, 0.85, 0.75), vec3(0.85, 0.3, 0.7), gain * 0.55);
    a += curtain(uv, 9.1, 0.40, t * 1.1, vec3(0.2, 0.95, 0.45), vec3(0.3, 0.5, 1.0), gain * 0.45);
    vec3 col = sky + a * 0.9;
    float x = uv.x * (size.x / size.y);
    float ridgeFar = 0.15 + 0.15 * fbm(vec2(x * 1.1, 2.0)) + 0.035 * fbm(vec2(x * 6.0, 5.0));
    float ridgeNear = 0.06 + 0.12 * fbm(vec2(x * 1.8 + 7.0, 9.0)) + 0.03 * fbm(vec2(x * 11.0, 1.0));
    vec3 glowTint = a * 0.08;
    if (uv.y < ridgeFar) col = mix(col, vec3(0.025, 0.04, 0.06) + glowTint, 0.92);
    if (uv.y < ridgeNear) col = vec3(0.012, 0.018, 0.028) + glowTint * 0.5;
    col += (hash(uv * size + t) - 0.5) * 0.018;
    float vig = smoothstep(1.35, 0.35, length((uv - 0.5) * vec2(1.4, 1.0)));
    col *= mix(0.75, 1.0, vig);
    col = col / (1.0 + col * 0.35);
    gl_FragColor = vec4(pow(col, vec3(0.92)), 1.0);
}
";
