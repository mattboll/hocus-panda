// The Matrix theme's CRT (#169), a pixel shader that paints, once, the
// textures its refinements are drawn with: the profile of the phosphor
// bloom around a bright line, a screen's scanlines, and the vignette of
// fullscreen overlays. The scene stretches them where they go.

precision mediump float;

uniform vec2 size;
uniform float alpha;
varying vec2 v_coords;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

// What to paint: 0 the bloom, 1 the scanlines, 2 the vignette.
uniform float kind;
// The bloom: how far it reaches from the line, and a logical pixel, in
// texels. The scanlines: a row's height, in pixels.
uniform float reach;
uniform float texel;
uniform float row;

const vec3 GLOW = vec3(0.0, 1.0, 0.533);  // #00FF88

void main() {
    vec2 p = v_coords * size;
    if (kind < 0.5) {
        // A line two texels wide in the middle, glowing around it: a
        // gaussian of the distance to it, one logical pixel wide (sigma).
        vec2 d = max(max(vec2(reach) - p, p - vec2(reach + 2.0)), 0.0);
        float l = length(d) / texel;
        float a = exp(-0.5 * l * l);
        gl_FragColor = vec4(GLOW * a, a) * alpha;
    } else if (kind < 1.5) {
        // Every other row dark; the strength is the element's.
        float dark = mod(floor(p.y / row), 2.0);
        gl_FragColor = vec4(0.0, 0.0, 0.0, dark) * alpha;
    } else {
        // Dark towards the corners, clear in the middle.
        float d = length((v_coords - 0.5) * 2.0);
        float a = smoothstep(0.5, 1.42, d);
        gl_FragColor = vec4(0.0, 0.0, 0.0, a) * alpha;
    }

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        gl_FragColor = vec4(0.0, 0.2, 0.0, 0.2) + gl_FragColor * 0.8;
#endif
}
