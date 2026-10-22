#version 100

// The Matrix rain (#108), a texture shader over the rain's glyph atlas:
// smithay renders the atlas over the whole screen and this program draws
// the night and the falling glyphs instead, looking glyphs up in it. The
// numbers match hocus_shell::matrix and hocus_shell::wallpaper, which draw
// the same rain still.

//_DEFINES_

#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

// The hashes need every bit of a float: without highp, the shader does
// not compile and the wallpaper stays still.
precision highp float;

#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

uniform float alpha;
varying vec2 v_coords;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

// The screen, logical pixels; the time, seconds; the sun, fractions of
// the screen.
uniform vec2 screen;
uniform float time;
uniform vec2 sun;
// The atlas: its cells across and down, a cell and a glyph in pixels,
// the whole of it in pixels.
uniform vec2 cells;
uniform vec2 cell_px;
uniform vec2 glyph_px;
uniform vec2 atlas_px;

const vec3 BASE = vec3(0.008, 0.016, 0.039);   // #02040A
const vec3 NIGHT = vec3(0.020, 0.043, 0.110);  // #050B1C
const vec3 LIGHT = vec3(0.118, 0.235, 0.549);  // #1E3C8C
const vec3 GREEN = vec3(0.0, 0.941, 0.376);    // #00F060
const vec3 HEAD = vec3(0.8, 1.0, 0.8);         // #CCFFCC
const float STEP = 48.0;
const float LINE = 24.0;
const float COLUMN = 28.0;
const vec2 GLYPH = vec2(12.0, 17.0);
const float GLYPHS = 100.0;

// "Hash without Sine" (Dave Hoskins, MIT): the same on every GPU.
float hash(vec2 p) {
    vec3 p3 = fract(vec3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

void main() {
    vec2 p = v_coords * screen;
    vec2 d = (v_coords - sun) / vec2(0.52, 0.83);
    float light = 0.18 * clamp(1.0 - length(d) / 0.7, 0.0, 1.0);
    vec3 color = mix(mix(BASE, NIGHT, v_coords.y), LIGHT, light);

    float col = floor((p.x - 10.0) / STEP);
    float x = p.x - (10.0 + col * STEP) - (COLUMN - GLYPH.x) / 2.0;
    float row = floor(p.y / LINE);
    float y = p.y - row * LINE - (LINE - GLYPH.y) / 2.0;
    if (col >= 0.0 && x >= 0.0 && x < GLYPH.x && y >= 0.0 && y < GLYPH.y) {
        // One drop a column, 8 to 30 glyphs long, 4 to 12 lines a second,
        // from above the screen to below it.
        float len = 8.0 + floor(23.0 * hash(vec2(col, 1.0)));
        float speed = 4.0 + 8.0 * hash(vec2(col, 2.0));
        float span = screen.y / LINE + len;
        float head = mod(time * speed + hash(vec2(col, 3.0)) * span, span);
        float k = floor(head) - row;
        if (k >= 0.0 && k < len) {
            float strength = 0.10 + 0.40 * hash(vec2(col, 4.0));
            vec3 ink = GREEN;
            float a = strength * (0.35 + 0.65 * (1.0 - k / len));
            if (k < 1.0) {
                ink = mix(GREEN, HEAD, 0.5);
                a = min(strength * 1.6, 0.9);
            }
            // Each glyph changes now and then, at a pace of its own.
            float tick = floor(time * (0.3 + 1.5 * hash(vec2(row, col))));
            float g = floor(hash(vec2(col * 7.0 + tick, row * 3.0 - tick)) * GLYPHS);
            vec2 at = vec2(mod(g, cells.x), floor(g / cells.x));
            vec2 uv = (at * cell_px + 1.0 + vec2(x, y) / GLYPH * glyph_px) / atlas_px;
            float mask = texture2D(tex, uv).a;
            color = mix(color, ink, mask * a);
        }
    }

    gl_FragColor = vec4(color, 1.0) * alpha;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        gl_FragColor = vec4(0.0, 0.2, 0.0, 0.2) + gl_FragColor * 0.8;
#endif
}
