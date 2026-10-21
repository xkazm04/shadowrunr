// The cut-out's shaders. Two programs:
//
// - **Layers** (VS, FS): the glow rig's layer program (../glow-shaders.ts) adapted to a cut-out.
//   Every head-bound layer warps through one shared field: the orthographic sphere of the skull
//   plus the layer's depth, turned by the pose. The body is a shallow cylinder that turns a quarter
//   as far as the head and blends into it at the neck. The backplate is kept only inside the
//   figure's rest matte (`u_matte`), as the dark behind a part a turn uncovers. The figure fades out
//   at its bottom and, low down, toward the sides (`cutMask`; plan.ts has it in JS). Solids cover
//   the page fully: outside the figure the canvas is transparent, inside it is opaque.
// - **Composite** (CVS, CFS): one full-canvas triangle over the figure, drawn first into a texture
//   with its mips. It adds the rim lights (the figure's edge, sampled against the light's side) and
//   lays the soft shadow behind and beneath him onto the page.

import { KIND } from "../glow-shaders";
import { SKULL_DEPTH } from "./plan";

export { KIND };

export const VS = `#version 300 es
precision highp float;
precision highp int;
in vec2 a_src;
uniform vec4 u_view;
uniform int u_kind;
uniform float u_depth, u_follow;
uniform vec2 u_headC, u_headR;
uniform float u_pivotY, u_imgH, u_imgW;
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

const float SKULL = ${SKULL_DEPTH.toFixed(3)};
const float BODY_Z = 0.35;
const float BODY_TURN = 0.25;

vec2 rot(vec2 p, vec2 c, float a) { vec2 r = p - c; float s = sin(a), k = cos(a); return c + vec2(r.x * k - r.y * s, r.x * s + r.y * k); }

void main() {
  vec2 p = a_src;
  v_src = a_src;
  v_shade = 1.0;
  if (u_kind == 4) { p.y = u_eye.y + (p.y - u_eye.y) * (1.0 - u_eye.z); p += u_gaze; }
  if (u_kind == 6) p.y -= u_aper.w * clamp((p.y - u_aper.x) / max(1.0, u_aper.y - u_aper.x), 0.0, 1.0);
  if (u_kind == 7) p.y += u_aper.w * clamp((u_aper.z - p.y) / max(1.0, u_aper.z - u_aper.y), 0.0, 1.0);
  if (u_kind == 3) { p = rot(p, u_floatPivot, u_float.z) + u_float.xy; }

  // the one field: where this point of the art is on the head (or the body), and how far it turns
  vec2 u = (p - u_headC) / u_headR;
  float neck = 1.0 - smoothstep(u_pivotY - 0.2 * u_headR.y, u_pivotY + 0.9 * u_headR.y, a_src.y);
  float w = u_follow;
  float z = u_depth;
  bool bodyish = u_kind == 1 || u_kind == 0;
  if (bodyish) {
    w = max(BODY_TURN, neck);
    float bx = u.x * u_headR.x / (0.5 * u_imgW);
    z = mix(BODY_Z * sqrt(max(0.0, 1.0 - bx * bx)), 0.0, neck);
  }
  if (u_kind == 2) z += sqrt(max(0.0, 1.0 - dot(u, u))) * SKULL;
  float yaw = u_ang.x * w, pitch = u_ang.y * w, roll = u_ang.z * w;
  float x1 = u.x * cos(yaw) + z * sin(yaw);
  float z1 = -u.x * sin(yaw) + z * cos(yaw);
  float y1 = u.y * cos(pitch) + z1 * sin(pitch);
  // a flat point (z 0) turned about the head's axis only narrows; one with depth slides across
  vec2 q = u_headC + vec2(x1, y1) * u_headR;
  q = rot(q, vec2(u_headC.x, u_pivotY), roll);
  v_shade = 1.0 + 0.6 * (x1 - u.x) * (u_kind == 2 ? 1.0 : (bodyish ? 0.25 : 0.4));

  float body = clamp((u_imgH - a_src.y) / u_imgH, 0.0, 1.0);
  q = rot(q, vec2(u_headC.x, u_imgH * 1.1), u_sway * (bodyish ? 1.0 : 0.8));
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
uniform sampler2D u_matte;
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
uniform vec4 u_aper;
uniform vec2 u_aperX;
uniform float u_voice;
uniform vec4 u_eyeP;
uniform vec2 u_size;         // the art, source px
uniform vec2 u_fade;         // the bottom fade: starts, gone (source y)
uniform vec3 u_mouthCol;
uniform int u_pass;          // 1: a solid's erase pass (coverage only)
uniform float u_erode;       // 1: take in a ragged matte (the shards' dark fringe)
out vec4 o;

float lmaxOf(vec3 c) { return max(c.r, max(c.g, c.b)); }
float s01(float a, float b, float v) { return smoothstep(a, b, v); }

// the figure's own frame: gone below the body's bottom fade, narrowing low down toward the sides,
// a touch of softness at the top so the tallest shard never meets an edge (plan.ts: cutMask)
float cutMask(vec2 p) {
  float bottom = 1.0 - s01(u_fade.x, u_fade.y, p.y);
  float low = s01(u_fade.x - 0.45 * u_size.x, u_fade.x, p.y);
  float side = s01(0.0, 0.14 * u_size.x, min(p.x, u_size.x - p.x));
  float top = s01(-24.0, 8.0, p.y);
  return bottom * (1.0 - low * (1.0 - side)) * top;
}

void emitLight(vec3 light, vec2 p) {
  light = max(light, vec3(0.0)) * cutMask(p);
  o = vec4(light, min(1.0, lmaxOf(light)));
}

float envAt(float delay) {
  float f = clamp(delay * 40.0, 0.0, 46.99);
  int i = int(floor(f));
  return mix(u_env[i], u_env[i + 1], fract(f));
}

void main() {
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
  vec2 uv = (v_src - u_bbox.xy) / u_bbox.zw;
  if (uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0) discard;
  vec4 c = texture(u_tex, uv);
  // a shard's matte carries a ragged dark fringe the glow hid (its solids barely cover): a cut-out
  // covers fully, so the fringe is taken in by a blurred, raised threshold, which also smooths it
  if (u_erode > 0.0) c.a *= mix(1.0, smoothstep(0.55, 0.92, textureLod(u_tex, uv, 1.6).a), u_erode);
  if (u_kind == 0) {
    // the backplate, only inside the figure's rest matte: the dark a turn uncovers behind a part
    float m = texture(u_matte, v_src / u_size).r * cutMask(v_src);
    o = vec4(c.rgb * m, m);
    return;
  }
  if (u_pass == 1) {
    o = vec4(0.0, 0.0, 0.0, c.a * u_occl * cutMask(v_src));
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
  float lmax = lmaxOf(light);
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
  if (u_occl > 0.0) {
    // a solid is a cut-out: it covers the page by its full coverage
    float m = cutMask(v_src);
    light = max(light, vec3(0.0));
    o = vec4(light * m, min(1.0, max(c.a * u_occl, lmaxOf(light))) * m);
  } else {
    emitLight(light, v_src);
  }
}`;

/** One triangle over the whole canvas, no attributes. */
export const CVS = `#version 300 es
void main() {
  vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
  gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}`;

export const CFS = `#version 300 es
precision highp float;
uniform sampler2D u_fig;     // the figure, premultiplied, with mips
uniform vec2 u_px;           // canvas size, device px
uniform float u_rimW;        // how far the rim looks past the edge, device px
uniform vec3 u_rimR;         // the right-hand rim light, colour x gain
uniform vec3 u_rimL;
uniform vec4 u_shadow;       // offset x, y (uv), behind strength, beneath strength
uniform vec3 u_pool;         // beneath: centre x (uv), half width (uv), height (uv)
uniform vec4 u_box;          // the art's box in uv: x0 y0 x1 y1 (y up)
out vec4 o;

float lmaxOf(vec3 c) { return max(c.r, max(c.g, c.b)); }

void main() {
  vec2 uv = gl_FragCoord.xy / u_px;
  vec4 f = texture(u_fig, uv);
  float A = f.a;

  // rim: where the figure is and, a little toward the light (and up), it is not. The edge is found
  // on a blurred level, so it follows the big silhouette and stays faint on the small shards, and
  // is bounded by the sharp coverage so it never spills off the figure
  vec2 dx = vec2(u_rimW / u_px.x, 0.0);
  vec2 up = vec2(0.0, 0.4 * u_rimW / u_px.y);
  float Ac = textureLod(u_fig, uv, 2.0).a;
  float nearR = A * clamp(1.6 * (Ac - textureLod(u_fig, uv + 2.0 * dx + up, 2.0).a), 0.0, 1.0);
  float nearL = A * clamp(1.6 * (Ac - textureLod(u_fig, uv - 2.0 * dx + up, 2.0).a), 0.0, 1.0);
  float wideR = A * clamp(Ac - textureLod(u_fig, uv + 4.0 * dx + up, 3.0).a, 0.0, 1.0);
  float wideL = A * clamp(Ac - textureLod(u_fig, uv - 4.0 * dx + up, 3.0).a, 0.0, 1.0);
  // a small part (a shard) has little figure around it at a coarse level: its rim is a glint, not a coat
  float mass = 0.1 + 0.9 * smoothstep(0.22, 0.55, textureLod(u_fig, uv, 4.5).a);
  vec3 rim = mass * (u_rimR * (nearR + 0.35 * wideR) + u_rimL * (nearL + 0.35 * wideL));

  // shadow: the figure's blurred matte cast behind him, and a pool beneath where he sits
  vec2 off = u_shadow.xy;
  float behind = 0.6 * textureLod(u_fig, uv - off, 3.5).a + 0.4 * textureLod(u_fig, uv - 1.7 * off, 5.0).a;
  float px = (uv.x - u_pool.x) / u_pool.y;
  float beneath = exp(-2.2 * px * px) * exp(-(uv.y - u_box.y) / u_pool.z);
  float s = 1.0 - (1.0 - u_shadow.z * behind) * (1.0 - u_shadow.w * beneath);
  // nothing the shadow does ends on the box's edge
  float edge = smoothstep(0.0, 0.08, min(uv.x, 1.0 - uv.x)) * smoothstep(0.0, 0.08, 1.0 - uv.y);
  s *= edge;

  vec3 light = f.rgb + rim;
  float a = A + s * (1.0 - A);
  o = vec4(light, clamp(max(a, lmaxOf(light)), 0.0, 1.0));
}`;
