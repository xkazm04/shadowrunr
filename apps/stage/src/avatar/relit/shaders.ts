// The relit face's shaders: the depth face's projection (each vertex lifted off the picture by its
// depth, turned about the neck under a perspective camera, so the unturned frame is the art) with
// the rig's layers drawn as lit cards. What is new against depth/shaders.ts:
//
// - Normals. Each vertex carries the depth field's Sobel slope (geom.ts `sobelSlopes`); a fragment
//   adds a bump from its own texture's luminance and a bevel from its coverage, so a card has
//   volume at its edges and the painted cracks and facets catch the light.
// - The key light is applied as a CHANGE: the art already holds its own light, so a fragment is
//   lit by how much more or less its turned normal faces the key than its rest normal did, plus a
//   specular glint that only moves in when the head turns. At rest the solids are the art.
// - Point lights: the eye fire in each socket (teal) and the monocle's ring (orange) light the
//   bone around them with an inverse-square falloff, in the rest frame (they turn with the head,
//   so Lambert is the same in either frame).
// - The jaw swings on a hinge (`u_jaw`) instead of sliding, the brow band moves with `u_brow`,
//   and a lean scales the head and brings it toward the camera.
//
// Kinds are glow-shaders.ts's `KIND`, plus 12 for the contact shadow.

export const SHADOW_KIND = 12;
/** The number of point lights the fragment shader sums. */
export const LIGHTS = 3;

export const VS = `#version 300 es
precision highp float;
precision highp int;
in vec2 a_src;
in float a_z;
in vec2 a_g;
in vec2 a_dz;
uniform vec4 u_view;
uniform vec4 u_cam;        // centre x, y; distance; pivot depth
uniform vec2 u_pivot;      // the turn's pivot on the picture
uniform vec3 u_ang;        // yaw, pitch, roll (rad)
uniform float u_lean;      // -1 back .. 1 toward the viewer
uniform int u_kind;
uniform float u_follow;
uniform vec2 u_headR;
uniform float u_imgH;
uniform float u_breath, u_sway;
uniform vec4 u_float;
uniform vec2 u_floatPivot;
uniform vec4 u_aper;
uniform vec4 u_jaw;        // hinge y, hinge z, sin, cos of the swing
uniform float u_cell;
uniform vec4 u_browC;      // the brow patches' centres (left x, y; right x, y)
uniform vec3 u_browS;      // patch half size x, y; brow -1 .. 1
out vec2 v_src;
out vec2 v_g;
out vec3 v_rest;
out vec3 v_rx;
out vec3 v_ry;
out vec3 v_rz;
out float v_tear;

vec2 rot(vec2 p, vec2 c, float a) { vec2 r = p - c; float s = sin(a), k = cos(a); return c + vec2(r.x * k - r.y * s, r.x * s + r.y * k); }
vec3 lift(vec2 p, float z) { return vec3((p - u_cam.xy) * ((u_cam.z - z) / u_cam.z), z); }
vec3 turn(vec3 r, float yaw, float pitch, float roll) {
  float x1 = r.x * cos(yaw) + r.z * sin(yaw);
  float z1 = -r.x * sin(yaw) + r.z * cos(yaw);
  float y1 = r.y * cos(pitch) + z1 * sin(pitch);
  float z2 = -r.y * sin(pitch) + z1 * cos(pitch);
  return vec3(rot(vec2(x1, y1), vec2(0.0), roll), z2);
}

// the brow band: raised it lifts, scowling it drops and draws in toward the nose, more at the inner end
vec3 brow(vec2 p) {
  float b = u_browS.z;
  if (b == 0.0) return vec3(0.0);
  vec3 o = vec3(0.0);
  for (int i = 0; i < 2; i++) {
    vec2 c = i == 0 ? u_browC.xy : u_browC.zw;
    vec2 d = (p - c) / u_browS.xy;
    float w = exp(-dot(d, d) * 1.6);
    float inward = sign(u_pivot.x - c.x);
    float inner = clamp(0.5 + 0.5 * d.x * inward, 0.0, 1.0);
    float down = b < 0.0 ? -b * (0.55 + 0.9 * inner) : -b * (1.1 - 0.3 * inner);
    o += w * vec3(inward * max(0.0, -b) * 0.15 * u_browS.y * inner, down * 0.3 * u_browS.y, max(0.0, -b) * 0.15 * u_browS.y);
  }
  return o;
}

void main() {
  vec2 p = a_src;
  float z = a_z;
  v_src = a_src;
  v_g = a_g;
  v_rx = vec3(1.0, 0.0, 0.0);
  v_ry = vec3(0.0, 1.0, 0.0);
  v_rz = vec3(0.0, 0.0, 1.0);
  if (u_kind == ${SHADOW_KIND}) {
    v_rest = vec3(p, 0.0);
    v_tear = 1.0;
    gl_Position = vec4(p * u_view.xy + u_view.zw, 0.999, 1.0);
    return;
  }
  if (u_kind == 6) p.y -= u_aper.w * clamp((p.y - u_aper.x) / max(1.0, u_aper.y - u_aper.x), 0.0, 1.0);
  if (u_kind == 7) {
    // the jaw swings about its hinge, a level axis behind the teeth
    vec2 r = vec2(p.y - u_jaw.x, z - u_jaw.y);
    p.y = u_jaw.x + r.x * u_jaw.w + r.y * u_jaw.z;
    z = u_jaw.y - r.x * u_jaw.z + r.y * u_jaw.w;
  }
  if (u_kind == 3) p = rot(p, u_floatPivot, u_float.z) + u_float.xy;
  if (u_kind == 2 || u_kind == 4 || u_kind == 5) {
    vec3 b = brow(a_src);
    p += b.xy;
    z += b.z;
  }
  v_rest = vec3(p, z);

  float w = u_follow;
  if (u_kind == 1) w = 0.12 + 0.3 * (1.0 - smoothstep(u_pivot.y - 0.2 * u_headR.y, u_pivot.y + 0.9 * u_headR.y, a_src.y));
  float yaw = u_ang.x * w, pitch = u_ang.y * w, roll = u_ang.z * w;

  vec3 piv = lift(u_pivot, u_cam.w);
  vec3 q = piv + turn(lift(p, z) - piv, yaw, pitch, roll) * (1.0 + 0.03 * u_lean * w);
  q.z += 90.0 * u_lean * w;
  v_rx = turn(vec3(1.0, 0.0, 0.0), yaw, pitch, roll);
  v_ry = turn(vec3(0.0, 1.0, 0.0), yaw, pitch, roll);
  v_rz = turn(vec3(0.0, 0.0, 1.0), yaw, pitch, roll);

  float stretch = (abs(a_dz.x * sin(yaw)) + abs(a_dz.y * sin(pitch))) / max(0.001, u_cell);
  v_tear = 1.0 - smoothstep(1.0, 2.0, stretch);

  float body = clamp((u_imgH - a_src.y) / u_imgH, 0.0, 1.0);
  vec3 base = lift(vec2(u_pivot.x, u_imgH * 1.1), 0.0);
  q.xy = rot(q.xy, base.xy, u_sway * (u_kind == 1 ? 1.0 : 0.8));
  q.y -= u_breath * body;

  float k = u_cam.z / (u_cam.z - q.z);
  vec2 s = u_cam.xy + q.xy * k;
  float cw = 1.0 / k;
  gl_Position = vec4((s * u_view.xy + u_view.zw) * cw, clamp(-q.z / 4000.0, -1.0, 1.0) * cw, cw);
}`;

export const FS = `#version 300 es
precision highp float;
precision highp int;
in vec2 v_src;
in vec2 v_g;
in vec3 v_rest;
in vec3 v_rx;
in vec3 v_ry;
in vec3 v_rz;
in float v_tear;
uniform int u_kind;
uniform int u_solid;
uniform sampler2D u_tex;
uniform vec4 u_bbox;
uniform float u_gain;
uniform float u_bloom;
uniform vec2 u_size;
uniform vec2 u_gaze;
uniform vec4 u_eye;
uniform vec4 u_eyeP;
uniform vec4 u_aper;
uniform vec2 u_aperX;
uniform float u_voice;
uniform vec3 u_mouthCol;
uniform vec4 u_rimA;
uniform vec3 u_rimACol;
uniform vec4 u_rimB;
uniform vec3 u_rimBCol;
uniform vec4 u_shadow;
uniform float u_shadowA;
uniform float u_fade;
uniform float u_rimK;
uniform vec3 u_key;        // the key light's direction (toward the light; y down)
uniform vec4 u_surf;       // this layer's bump px, bevel px, share of the point lights, specular
uniform vec4 u_lp[${LIGHTS}];  // point light: x, y, z (rest frame, source px); r0
uniform vec4 u_lc[${LIGHTS}];  // its colour times its strength; r1
out vec4 o;

float lmaxOf(vec3 c) { return max(c.r, max(c.g, c.b)); }
float luma(vec3 c) { return dot(c, vec3(0.299, 0.587, 0.114)); }

float frameMask(vec2 p) {
  float side = smoothstep(0.0, 32.0, min(p.x, u_size.x - p.x));
  float bottom = 1.0 - smoothstep(u_size.y - u_fade, u_size.y, p.y);
  return side * bottom;
}

void emitLight(vec3 light) {
  light = max(light, vec3(0.0)) * frameMask(v_src) * v_tear;
  o = vec4(light, min(1.0, lmaxOf(light)));
}

float rim(vec2 uv, vec4 r) {
  if (r.z <= 0.0) return 0.0;
  vec2 off = r.xy * r.w / u_bbox.zw;
  float out1 = textureLod(u_tex, uv + off, 2.5).a;
  float out2 = textureLod(u_tex, uv + 2.0 * off, 3.5).a;
  return r.z * u_rimK * max(0.0, 1.0 - 0.6 * out1 - 0.4 * out2);
}

// the rest normal: the depth's slope, the texture's luminance as a bump, the coverage as a bevel
vec3 restNormal(vec2 uv) {
  vec2 du = vec2(1.5 / u_bbox.z, 0.0), dv = vec2(0.0, 1.5 / u_bbox.w);
  float lx = luma(texture(u_tex, uv + du).rgb) - luma(texture(u_tex, uv - du).rgb);
  float ly = luma(texture(u_tex, uv + dv).rgb) - luma(texture(u_tex, uv - dv).rgb);
  vec2 bev = vec2(textureLod(u_tex, uv + 2.0 * du, 2.0).a - textureLod(u_tex, uv - 2.0 * du, 2.0).a,
                  textureLod(u_tex, uv + 2.0 * dv, 2.0).a - textureLod(u_tex, uv - 2.0 * dv, 2.0).a);
  vec2 g = v_g + u_surf.x * vec2(lx, ly) / 3.0 + u_surf.y * bev / 6.0;
  return normalize(vec3(-g, 1.0));
}

void main() {
  if (u_kind == ${SHADOW_KIND}) {
    vec2 d = (v_src - u_shadow.xy) / u_shadow.zw;
    float a = u_shadowA * exp(-2.2 * dot(d, d));
    o = vec4(0.0, 0.0, 0.0, a);
    return;
  }
  if (u_kind == 8) {
    float g = max(u_aper.w, 0.001);
    float dy = abs(v_src.y - u_aper.y);
    float cx = 0.5 * (u_aperX.x + u_aperX.y), hw = 0.5 * (u_aperX.y - u_aperX.x);
    float ax = abs(v_src.x - cx) / hw;
    if (ax > 1.0) discard;
    float across = 1.0 - smoothstep(0.35, 0.9, ax);
    float taper = 1.0 - 0.55 * smoothstep(0.2, 0.9, ax);
    float core = exp(-pow(dy / (g * 0.28 + 0.7), 2.0));
    float rims = 0.45 * exp(-pow((dy - g * taper) / 1.6, 2.0));
    float spill = 0.12 * exp(-dy / (g * 1.5 + 2.0));
    float fill = 0.32 * (1.0 - smoothstep(g * 0.7 * taper, g * 1.0 * taper, dy));
    float k = (core + rims + spill + fill) * across * clamp(u_aper.w / 4.0, 0.0, 1.0) * (0.55 + 0.6 * u_voice);
    emitLight(u_mouthCol * k * u_gain);
    return;
  }
  vec2 src = v_src;
  if (u_kind == 4) {
    src -= u_gaze;
    src.y = u_eye.y + (src.y - u_eye.y) / max(0.05, 1.0 - u_eye.z);
  }
  vec2 uv = (src - u_bbox.xy) / u_bbox.zw;
  if (uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0) discard;
  vec4 c = texture(u_tex, uv);

  if (u_solid == 1) {
    float a = c.a * frameMask(v_src) * v_tear;
    if (a < 0.02) discard;
    vec3 n = restNormal(uv);
    vec3 nt = mat3(v_rx, v_ry, v_rz) * n;
    // the key as a change from the art's own light, and a glint that moves in with the turn
    float shade = clamp(1.0 + 0.8 * (dot(nt, u_key) - dot(n, u_key)), 0.45, 1.6);
    vec3 h = normalize(u_key + vec3(0.0, 0.0, 1.0));
    float glint = u_surf.w * max(0.0, pow(max(0.0, dot(nt, h)), 24.0) - pow(max(0.0, dot(n, h)), 24.0));
    vec3 lit = c.rgb * shade * u_gain + glint * vec3(0.9, 0.95, 1.0);
    // the eye fire and the ring light the surface around them
    vec3 add = vec3(0.0);
    for (int i = 0; i < ${LIGHTS}; i++) {
      vec3 l = u_lp[i].xyz - v_rest;
      float d = length(l);
      float fall = 1.0 / (1.0 + d * d / (u_lp[i].w * u_lp[i].w)) * (1.0 - smoothstep(0.5 * u_lc[i].w, u_lc[i].w, d));
      add += u_lc[i].rgb * fall * (0.15 + 0.85 * max(0.0, dot(n, l / max(d, 1e-3))));
    }
    lit += c.rgb * add * u_surf.z;
    vec3 edge = u_rimACol * rim(uv, u_rimA) + u_rimBCol * rim(uv, u_rimB);
    o = vec4((lit + edge) * a, a);
    return;
  }

  float irisFade = 1.0;
  if (u_kind == 4 && u_eyeP.x > 0.0) {
    float d = length(src - u_eyeP.yz);
    irisFade = 1.0 - smoothstep(u_eyeP.w * (2.6 - 1.8 * u_eyeP.x), u_eyeP.w * (3.2 - 1.6 * u_eyeP.x), d) * min(1.0, u_eyeP.x * 2.0);
  }
  float edgeWin = smoothstep(0.0, 0.12, min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y)));
  vec3 light = c.rgb * irisFade * u_gain;
  if (u_bloom > 0.0) {
    vec4 b1 = textureLod(u_tex, uv, 3.0), b2 = textureLod(u_tex, uv, 5.0);
    light += u_bloom * u_gain * irisFade * (b1.rgb + 0.5 * b2.rgb);
  }
  emitLight(light * edgeWin);
}`;
