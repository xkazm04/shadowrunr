// The depth face's shaders: one program, one draw per layer. The vertex shader lifts each mesh
// vertex off the picture by its depth (so the unturned frame is the art itself), turns it about
// the neck under a perspective camera, and projects it back (geom.ts `project` is the same sum in
// JS). The fragment shader draws the rig's layers as glow.ts does - solids by their coverage,
// light added as light - with a rim light at the solids' edges, a shade that follows the turn,
// and the tearing fade. Kinds are glow-shaders.ts's `KIND`, plus 12 for the contact shadow.

export const SHADOW_KIND = 12;

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
uniform int u_kind;
uniform float u_follow;
uniform vec2 u_headR;
uniform float u_imgH;
uniform float u_breath, u_sway;
uniform vec4 u_float;
uniform vec2 u_floatPivot;
uniform vec4 u_aper;
uniform float u_cell;
uniform vec3 u_key;        // the key light's direction, for the turn's shade
out vec2 v_src;
out float v_shade;
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

void main() {
  vec2 p = a_src;
  v_src = a_src;
  if (u_kind == ${SHADOW_KIND}) {
    v_shade = 1.0;
    v_tear = 1.0;
    gl_Position = vec4(p * u_view.xy + u_view.zw, 0.999, 1.0);
    return;
  }
  if (u_kind == 6) p.y -= u_aper.w * clamp((p.y - u_aper.x) / max(1.0, u_aper.y - u_aper.x), 0.0, 1.0);
  if (u_kind == 7) p.y += u_aper.w * clamp((u_aper.z - p.y) / max(1.0, u_aper.z - u_aper.y), 0.0, 1.0);
  if (u_kind == 3) p = rot(p, u_floatPivot, u_float.z) + u_float.xy;

  // how much of the turn this vertex takes: the head all of it, a floating part its follow, the
  // body a little, more toward the neck
  float w = u_follow;
  if (u_kind == 1) w = 0.12 + 0.3 * (1.0 - smoothstep(u_pivot.y - 0.2 * u_headR.y, u_pivot.y + 0.9 * u_headR.y, a_src.y));
  float yaw = u_ang.x * w, pitch = u_ang.y * w, roll = u_ang.z * w;

  vec3 piv = lift(u_pivot, u_cam.w);
  vec3 q = piv + turn(lift(p, a_z) - piv, yaw, pitch, roll);

  // the shade follows the turn: only its change from the art's own light is applied
  vec3 n = normalize(vec3(-a_g.x, -a_g.y, 1.0));
  vec3 nr = turn(n, yaw, pitch, roll);
  v_shade = clamp(1.0 + 0.7 * (dot(nr, u_key) - dot(n, u_key)), 0.55, 1.5);

  float stretch = (abs(a_dz.x * sin(yaw)) + abs(a_dz.y * sin(pitch))) / max(0.001, u_cell);
  v_tear = 1.0 - smoothstep(1.5, 3.0, stretch);

  // breath and sway on the picture, as the glow face does
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
in float v_shade;
in float v_tear;
uniform int u_kind;
uniform int u_solid;
uniform sampler2D u_tex;
uniform vec4 u_bbox;
uniform float u_gain;
uniform float u_bloom;
uniform vec2 u_size;
uniform vec2 u_gaze;       // the eye light's slide, source px
uniform vec4 u_eye;        // eye centre x, y; squash 0..1
uniform vec4 u_eyeP;       // iris blink: amount, centre x, y, core radius
uniform vec4 u_aper;
uniform vec2 u_aperX;
uniform float u_voice;
uniform vec3 u_mouthCol;
uniform vec4 u_rimA;       // rim light A: direction x, y (picture); strength; reach (source px)
uniform vec3 u_rimACol;
uniform vec4 u_rimB;
uniform vec3 u_rimBCol;
uniform vec4 u_shadow;     // contact shadow: centre x, y; radius x, y
uniform float u_shadowA;
uniform float u_fade;      // the figure fades out over this many source px above the bottom
uniform float u_rimK;      // this layer's share of the rim light
out vec4 o;

float lmaxOf(vec3 c) { return max(c.r, max(c.g, c.b)); }

// the figure's own frame: nothing ends at the canvas's sides, and the bottom fades out
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
    // the eye light slides in its socket and closes toward its centre; the socket stays put
    src -= u_gaze;
    src.y = u_eye.y + (src.y - u_eye.y) / max(0.05, 1.0 - u_eye.z);
  }
  vec2 uv = (src - u_bbox.xy) / u_bbox.zw;
  if (uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0) discard;
  vec4 c = texture(u_tex, uv);

  if (u_solid == 1) {
    float a = c.a * frameMask(v_src) * v_tear;
    if (a < 0.02) discard;
    vec3 light = c.rgb * v_shade * u_gain;
    // rim lights where the solid's edge faces them (its coverage runs out that way)
    vec3 edge = u_rimACol * rim(uv, u_rimA) + u_rimBCol * rim(uv, u_rimB);
    o = vec4((light + edge) * a, a);
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
