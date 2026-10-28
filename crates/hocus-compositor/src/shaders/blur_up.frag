#version 100

// The bar's glass (bar.md §13), the dual Kawase blur's way back up (see
// blur_down.frag): smithay draws a picture into one twice its size, and
// this program makes each pixel the weighted average of eight samples on
// a diamond around it, the diagonal ones counting twice. The last pass
// also saturates the colours, as CSS's saturate() does.

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

// Half a pixel of the picture drawn and of the one read, in texture
// coordinates, and how far apart the samples are, in those halves.
uniform vec2 half_pixel;
uniform vec2 half_source;
uniform float offset;
// 1 for none; CSS's saturate() matrix around the luminance otherwise.
uniform float saturation;

// The source's edge goes on beyond it (the texture itself may wrap).
vec4 taken(vec2 at) {
    return texture2D(tex, clamp(at, half_source, 1.0 - half_source));
}

void main() {
    vec2 d = half_pixel * offset;
    vec4 sum = taken(v_coords + vec2(-d.x * 2.0, 0.0));
    sum += taken(v_coords + vec2(-d.x, d.y)) * 2.0;
    sum += taken(v_coords + vec2(0.0, d.y * 2.0));
    sum += taken(v_coords + vec2(d.x, d.y)) * 2.0;
    sum += taken(v_coords + vec2(d.x * 2.0, 0.0));
    sum += taken(v_coords + vec2(d.x, -d.y)) * 2.0;
    sum += taken(v_coords + vec2(0.0, -d.y * 2.0));
    sum += taken(v_coords + vec2(-d.x, -d.y)) * 2.0;
    vec4 c = sum / 12.0;
    float luma = dot(c.rgb, vec3(0.2126, 0.7152, 0.0722));
    c.rgb = clamp(mix(vec3(luma), c.rgb, saturation), 0.0, c.a);
    gl_FragColor = c * alpha;
}
