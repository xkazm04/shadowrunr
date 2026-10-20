// The glow rig's shaders: one program, one draw per layer. The vertex shader warps each layer's
// mesh (head parallax as an orthographic sphere, eye lids, the hinged aperture, floating parts,
// body sway and breath); the fragment shader emits premultiplied light over a transparent ground
// (see glow.ts). Ported unchanged from the rig the face came with, except that the light in the
// open mouth is a uniform (`u_mouthCol`) instead of a fixed colour.

import type { LayerKind } from "./layers";

/** Each layer kind's `u_kind` in the shaders. */
export const KIND: Record<LayerKind, number> = {
  static: 0,
  body: 1,
  head: 2,
  float: 3,
  eye: 4,
  seam: 5,
  aperture_upper: 6,
  aperture_lower: 7,
  aperture_glow: 8,
  halo: 9,
  pulse: 10,
  ground: 11,
};

export const VS = `#version 300 es
precision highp float;
precision highp int;
in vec2 a_src;
uniform vec4 u_view;
uniform int u_kind;
uniform float u_depth, u_follow;
uniform vec2 u_headC, u_headR;
uniform float u_pivotY, u_imgH;
uniform vec3 u_ang;
uniform float u_breath;
uniform float u_sway;
uniform vec4 u_float;
uniform vec2 u_floatPivot;
uniform vec4 u_eye;
uniform vec2 u_gaze;
uniform vec4 u_aper;
out vec2 v_src;
out float v_shade;

vec2 rot(vec2 p, vec2 c, float a) { vec2 r = p - c; float s = sin(a), k = cos(a); return c + vec2(r.x * k - r.y * s, r.x * s + r.y * k); }

void main() {
  vec2 p = a_src;
  v_src = a_src;
  v_shade = 1.0;
  if (u_kind == 0 || u_kind == 11) { gl_Position = vec4(p * u_view.xy + u_view.zw, 0.0, 1.0); return; }
  if (u_kind == 4) { p.y = u_eye.y + (p.y - u_eye.y) * (1.0 - u_eye.z); p += u_gaze; }
  if (u_kind == 6) p.y -= u_aper.w * clamp((p.y - u_aper.x) / max(1.0, u_aper.y - u_aper.x), 0.0, 1.0);
  if (u_kind == 7) p.y += u_aper.w * clamp((u_aper.z - p.y) / max(1.0, u_aper.z - u_aper.y), 0.0, 1.0);
  if (u_kind == 3) { p = rot(p, u_floatPivot, u_float.z) + u_float.xy; }

  float w = u_follow;
  if (u_kind == 1) w = 1.0 - smoothstep(u_pivotY - 0.2 * u_headR.y, u_pivotY + 0.9 * u_headR.y, a_src.y);
  vec2 u = (p - u_headC) / u_headR;
  float zs = sqrt(max(0.0, 1.0 - dot(u, u)));
  float z = (u_kind == 2 ? zs * 0.6 : 0.0) + u_depth;
  float yaw = u_ang.x * w, pitch = u_ang.y * w, roll = u_ang.z * w;
  float x1 = u.x * cos(yaw) + z * sin(yaw);
  float z1 = -u.x * sin(yaw) + z * cos(yaw);
  float y1 = u.y * cos(pitch) + z1 * sin(pitch);
  vec2 q = mix(p, u_headC + vec2(x1, y1) * u_headR, step(0.001, w));
  q = rot(q, vec2(u_headC.x, u_pivotY), roll);
  v_shade = 1.0 + 0.6 * (x1 - u.x) * step(0.001, w) * step(1.5, float(u_kind)) * (u_kind == 2 ? 1.0 : 0.4);

  float body = clamp((u_imgH - a_src.y) / u_imgH, 0.0, 1.0);
  q = rot(q, vec2(u_headC.x, u_imgH * 1.1), u_sway * (u_kind == 1 ? 1.0 : 0.8));
  q.y -= u_breath * body;
  gl_Position = vec4(q * u_view.xy + u_view.zw, 0.0, 1.0);
}`;

export const FS = `#version 300 es
precision highp float;
precision highp int;
in vec2 v_src;
in float v_shade;
uniform int u_kind;
uniform sampler2D u_tex;
uniform vec4 u_bbox;
uniform float u_occl;
uniform float u_gain;
uniform float u_bloom;
uniform float u_t;
uniform int u_shimmer;
uniform vec3 u_shim;
uniform vec2 u_shimK;
uniform float u_env[48];
uniform float u_tipPulse;
uniform int u_glint;
uniform vec4 u_glintP;
uniform float u_glintGain;
uniform vec4 u_ring;
uniform vec3 u_ringCol;
uniform vec4 u_aper;
uniform vec2 u_aperX;
uniform float u_voice;
uniform vec4 u_eyeP;
uniform vec2 u_size;         // the art, source px
uniform vec4 u_gnd;          // ground: cx y half_width fade (0 fade: no ground)
uniform float u_gndCurve;
uniform float u_pxArt;       // source px per device px
uniform vec3 u_lineCore;
uniform vec3 u_lineGlow;
uniform vec3 u_mouthCol;      // the light in an open aperture
uniform int u_pass;          // 1: a solid's erase pass (coverage only)
uniform float u_body;        // how much a solid's dark interior still covers the page, 0..1
out vec4 o;

// Everything the canvas shows is premultiplied light over a transparent ground: rgb is light, alpha
// is how much of the page behind it is covered. Light carries alpha equal to its brightest channel,
// so it reads as light over any page colour and the canvas never holds rgb above alpha.
float lmaxOf(vec3 c) { return max(c.r, max(c.g, c.b)); }

// The figure's own frame: nothing ends at the canvas border, and the lower figure dissolves into
// the ground line (earlier toward the line's ends, so the shoulders narrow into it).
float frameMask(vec2 p) {
  float m = 32.0;
  float edge = smoothstep(0.0, m, min(min(p.x, u_size.x - p.x), min(p.y, u_size.y - p.y)));
  if (u_gnd.w <= 0.0) return edge;
  float dx = (p.x - u_gnd.x) / u_gnd.z;
  float y = p.y + u_gndCurve * dx * dx;
  // it ends a touch below the line, so the last of her meets the light rather than stopping short
  float t = smoothstep(u_gnd.y - u_gnd.w, u_gnd.y + 0.08 * u_gnd.w, y);
  return edge * (1.0 - t);
}

void emitLight(vec3 light, vec2 p) {
  float m = frameMask(p);
  light = max(light, vec3(0.0)) * m;
  o = vec4(light, min(1.0, lmaxOf(light)));
}

float envAt(float delay) {
  float f = clamp(delay * 40.0, 0.0, 46.99);
  int i = int(floor(f));
  return mix(u_env[i], u_env[i + 1], fract(f));
}

void main() {
  if (u_kind == 9) {
    float d = abs(length(v_src - u_ring.xy) - u_ring.z);
    float core = exp(-pow(d / u_ring.w, 2.0));
    float halo = 0.22 * exp(-d / (u_ring.w * 5.0));
    emitLight(u_ringCol * (1.25 * core + halo) * u_gain, v_src);
    return;
  }
  if (u_kind == 8) {
    float g = max(u_aper.w, 0.001);
    float dy = abs(v_src.y - u_aper.y);
    float cx = 0.5 * (u_aperX.x + u_aperX.y), hw = 0.5 * (u_aperX.y - u_aperX.x);
    float ax = abs(v_src.x - cx) / hw;
    float across = 1.0 - smoothstep(0.35, 0.9, ax);
    float taper = 1.0 - 0.55 * smoothstep(0.2, 0.9, ax);
    float core = exp(-pow(dy / (g * 0.28 + 0.7), 2.0));
    float rims = 0.45 * exp(-pow((dy - g * taper) / 1.6, 2.0));
    float spill = 0.12 * exp(-dy / (g * 1.5 + 2.0));
    float fill = 0.32 * (1.0 - smoothstep(g * 0.7 * taper, g * 1.0 * taper, dy));
    float k = (core + rims + spill + fill) * across * clamp(u_aper.w / 4.0, 0.0, 1.0) * (0.55 + 0.6 * u_voice);
    emitLight(u_mouthCol * k * u_gain, v_src);
    return;
  }
  if (u_kind == 11) { // the ground line: thin, tapering to points, a soft bloom, a pool where she meets it
    float dx = (v_src.x - u_gnd.x) / (u_gnd.z * 1.18);
    float ax = abs(dx);
    float span = max(0.0, 1.0 - ax * ax);
    float taper = pow(span, 1.6);
    float dy = v_src.y - u_gnd.y;
    float w = max(1.8, 0.9 * u_pxArt);
    float core = exp(-pow(dy / w, 2.0)) * taper;
    float glow = 0.5 * exp(-pow(dy / (w * 4.0), 2.0)) * taper;
    float bloom = 0.16 * exp(-abs(dy) / (dy < 0.0 ? 34.0 : 16.0)) * span * span;
    float pool = 0.3 * exp(-pow(dx / 0.42, 2.0)) * (dy < 0.0 ? exp(dy / 46.0) : exp(-dy / 12.0));
    vec3 light = core * mix(u_lineCore, vec3(1.0), 0.35) + (glow + bloom + pool) * u_lineGlow;
    float edge = smoothstep(0.0, 32.0, min(min(v_src.x, u_size.x - v_src.x), min(v_src.y, u_size.y - v_src.y)));
    light = light * u_gain * edge;
    o = vec4(light, min(1.0, lmaxOf(light)));
    return;
  }
  vec2 uv = (v_src - u_bbox.xy) / u_bbox.zw;
  if (uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0) discard;
  vec4 c = texture(u_tex, uv);
  if (u_pass == 1) {
    o = vec4(0.0, 0.0, 0.0, c.a * u_occl * frameMask(v_src));
    return;
  }
  float irisFade = 1.0;
  if (u_kind == 4 && u_eyeP.x > 0.0) {
    float d = length(v_src - u_eyeP.yz);
    irisFade = 1.0 - smoothstep(u_eyeP.w * (2.6 - 1.8 * u_eyeP.x), u_eyeP.w * (3.2 - 1.6 * u_eyeP.x), d) * min(1.0, u_eyeP.x * 2.0);
    c.rgb *= irisFade;
  }
  float edgeWin = u_occl > 0.0 ? 1.0 : smoothstep(0.0, 0.12, min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y)));
  vec3 light = c.rgb;
  float lmax = max(light.r, max(light.g, light.b));
  if (u_shimmer == 1) {
    float tip = smoothstep(0.14, 0.55, lmax);
    float r = length(v_src - u_shim.xy);
    float lev = envAt(r / u_shim.z);
    float wave = 0.55 + 0.45 * sin(6.2831853 * (r / u_shimK.x - u_t * u_shim.z / u_shimK.x));
    light *= 1.0 + tip * (u_shimK.y * lev * wave + u_tipPulse);
  }
  if (u_glint == 1) {
    float edge = smoothstep(0.18, 0.6, lmax);
    float s = dot(v_src, u_glintP.xy) - u_glintP.z;
    light *= 1.0 + u_glintGain * edge * exp(-pow(s / u_glintP.w, 2.0));
  }
  float cov = u_occl > 0.0 ? c.a : 1.0;
  light *= u_gain * v_shade * cov;
  if (u_bloom > 0.0) {
    vec4 b1 = textureLod(u_tex, uv, 3.0), b2 = textureLod(u_tex, uv, 5.0);
    light += u_bloom * u_gain * irisFade * (b1.rgb * (u_occl > 0.0 ? b1.a : 1.0) + 0.5 * b2.rgb * (u_occl > 0.0 ? b2.a : 1.0));
  }
  light *= edgeWin;
  if (u_kind == 0) {
    // the backplate: its black is the page; faint strands fade out on a soft curve, never clip
    light *= smoothstep(0.012, 0.16, lmaxOf(light));
  }
  if (u_occl > 0.0) {
    // a solid occludes by its coverage; alpha is never below its own light
    float m = frameMask(v_src);
    light = max(light, vec3(0.0));
    o = vec4(light * m, min(1.0, max(c.a * u_occl * u_body, lmaxOf(light))) * m);
  } else {
    emitLight(light, v_src);
  }
}`;
