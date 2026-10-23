#version 100

// The Matrix theme's closing glitch (#169), a texture shader over the
// closing window's picture: bands of it shifted sideways, as the plain
// effect draws them, and within each band its red taken from one side
// and its blue from the other, so that the colours come apart at every
// edge. Beyond the picture is nothing: what is behind shows in the
// fringes.

//_DEFINES_

#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

precision mediump float;

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

// Where the bands meet, from the top, as fractions of the height (in
// order); how far each band is shifted, and how far apart red and blue
// are taken, as fractions of the width.
uniform vec3 cuts;
uniform vec4 shifts;
uniform float split;

vec4 taken(vec2 at) {
    if (at.x < 0.0 || at.x > 1.0) {
        return vec4(0.0);
    }
    return texture2D(tex, at);
}

void main() {
    float y = v_coords.y;
    float dx = shifts.w;
    if (y < cuts.x) {
        dx = shifts.x;
    } else if (y < cuts.y) {
        dx = shifts.y;
    } else if (y < cuts.z) {
        dx = shifts.z;
    }
    vec2 at = v_coords - vec2(dx, 0.0);
    vec4 r = taken(at - vec2(split, 0.0));
    vec4 g = taken(at);
    vec4 b = taken(at + vec2(split, 0.0));
    // Premultiplied: each channel from its own sample, as opaque as the
    // most opaque of them.
    gl_FragColor = vec4(r.r, g.g, b.b, max(max(r.a, g.a), b.a)) * alpha;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        gl_FragColor = vec4(0.0, 0.2, 0.0, 0.2) + gl_FragColor * 0.8;
#endif
}
