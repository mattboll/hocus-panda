// A starfield (#108), a pixel shader: three layers of stars drifting left,
// the nearer the faster, each twinkling at a pace of its own, on the same
// night as the Matrix rain. The numbers match hocus_shell::wallpaper,
// which draws the same stars still.

// The hashes need every bit of a float: without highp, the shader does
// not compile and the wallpaper stays still.
precision highp float;

uniform vec2 size;
uniform float alpha;
varying vec2 v_coords;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

uniform vec2 screen;
uniform float time;
uniform vec2 sun;

const vec3 BASE = vec3(0.008, 0.016, 0.039);   // #02040A
const vec3 NIGHT = vec3(0.020, 0.043, 0.110);  // #050B1C
const vec3 LIGHT = vec3(0.118, 0.235, 0.549);  // #1E3C8C
const vec3 STAR = vec3(0.867, 0.902, 1.0);     // #DDE6FF
const float CHANCE = 0.55;

// "Hash without Sine" (Dave Hoskins, MIT): the same on every GPU.
float hash(vec2 p) {
    vec3 p3 = fract(vec3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

float layer(vec2 p, float side, float speed, float strength, float n) {
    vec2 q = p + vec2(time * speed, 0.0);
    vec2 c = floor(q / side);
    vec2 f = q - c * side;
    vec2 seed = c + vec2(n * 131.0, n * 71.0);
    if (hash(seed) > CHANCE) {
        return 0.0;
    }
    float radius = 0.5 + 1.1 * strength * hash(seed + 0.5);
    float margin = radius + 1.0;
    vec2 star = vec2(
        mix(margin, side - margin, hash(seed + 1.5)),
        mix(margin, side - margin, hash(seed + 2.5)));
    float bright = strength * mix(0.5, 1.0, hash(seed + 3.5));
    float twinkle = 0.75 + 0.25 * sin(time * (0.5 + 2.0 * hash(seed + 4.5)) + 6.283 * hash(seed + 5.5));
    float scale = size.x / screen.x;
    float d = length(f - star) * scale;
    return clamp(radius * scale + 0.5 - d, 0.0, 1.0) * bright * twinkle;
}

void main() {
    vec2 p = v_coords * screen;
    vec2 d = (v_coords - sun) / vec2(0.52, 0.83);
    float light = 0.18 * clamp(1.0 - length(d) / 0.7, 0.0, 1.0);
    vec3 color = mix(mix(BASE, NIGHT, v_coords.y), LIGHT, light);
    float a = layer(p, 90.0, 3.0, 0.45, 0.0);
    a = max(a, layer(p, 150.0, 7.0, 0.7, 1.0));
    a = max(a, layer(p, 240.0, 14.0, 1.0, 2.0));
    color = mix(color, STAR, a);

    gl_FragColor = vec4(color, 1.0) * alpha;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        gl_FragColor = vec4(0.0, 0.2, 0.0, 0.2) + gl_FragColor * 0.8;
#endif
}
