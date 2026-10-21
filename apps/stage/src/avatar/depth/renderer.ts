// The depth-mesh renderer: the rig's layers drawn on meshes displaced by the depth map, under a
// perspective camera, on a TRANSPARENT canvas with no backplate.
//
// - The head's layers (skull, jaw, mouth light, eye lights, monocle) share one 128-cell grid over
//   the head, so the light layers sit on the very surface the sockets were carved into and stay
//   locked to them through a turn. The torso has a coarser grid of its own; each crown shard is a
//   flat card at its own depth (`depth.json` planes).
// - Solids composite by their coverage (premultiplied, over), each with a depth test against
//   itself so a turned head hides its own far side; light adds as light, as glow.ts does.
// - A rim light in the rig's colours on the solids' edges, the shade that follows the turn, and a
//   soft contact shadow under the figure.
//
// No allocation per frame. A lost context is waited out and rebuilt from the decoded pictures.

import type { Pose } from "../driver";
import { gapCentre, gapHalf, rigColors } from "../glow-geom";
import { KIND } from "../glow-shaders";
import type { LayerSpec, LoadedForm } from "../layers";
import { cameraFor, displacedGrid, fitView, rigMeshes, STRIDE, type DepthField, type DepthMeta } from "./geom";
import type { LookPose } from "./look";
import { FS, SHADOW_KIND, VS } from "./shaders";

export interface DepthRenderer {
  render(pose: Pose, look: LookPose): void;
  resize(): void;
  readonly lost: boolean;
  dispose(): void;
}

/** The context the depth face asks for: transparent and premultiplied like the glow face, with a depth buffer. */
export const DEPTH_CONTEXT: WebGLContextAttributes = Object.freeze({
  alpha: true,
  premultipliedAlpha: true,
  antialias: true,
  depth: true,
  preserveDrawingBuffer: false,
});

const YAW_LIMIT = 22;
const PITCH_LIMIT = 12;
const DEG = Math.PI / 180;

interface Draw {
  spec: LayerSpec;
  kind: number;
  solid: boolean;
  tex: WebGLTexture | null;
  bbox: [number, number, number, number];
  mesh: number;
  float?: { amp: [number, number]; freq: number; rot: number; pivot: [number, number]; phase: number; phase2: number };
  eyeC?: [number, number];
  right?: boolean;
}

const smooth01 = (x: number) => {
  const y = Math.min(1, Math.max(0, x));
  return y * y * (3 - 2 * y);
};
const clamp = (x: number, a: number, b: number) => (x < a ? a : x > b ? b : x);
const breathe = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

/** Make the renderer. Rejects when WebGL2 is not there or the program does not link. */
export async function createDepthRenderer(
  canvas: HTMLCanvasElement,
  form: LoadedForm,
  field: DepthField,
  meta: DepthMeta,
  onRestored: () => void = () => {},
): Promise<DepthRenderer> {
  const { spec, images } = form;
  const R = spec.rig;
  const W = spec.width;
  const H = spec.height;
  const colors = rigColors(R);
  const upperLip = spec.layers.some((l) => l.kind === "aperture_upper");
  const first = canvas.getContext("webgl2", DEPTH_CONTEXT);
  if (!first) throw new Error("WebGL2 unavailable");
  const gl: WebGL2RenderingContext = first;
  const { meshes, which } = rigMeshes(form, field, meta);
  const cam = cameraFor(W, H, R.head, meta.near - 0.8 * R.head.radius[0]);

  let lost = false;
  let disposed = false;
  let prog: WebGLProgram | null = null;
  let U: Record<string, WebGLUniformLocation | null> = {};
  let draws: Draw[] = [];
  let vaos: { vao: WebGLVertexArrayObject; count: number; cell: number }[] = [];
  let shadowVao: WebGLVertexArrayObject | null = null;
  let owned: { textures: WebGLTexture[]; buffers: WebGLBuffer[]; vaos: WebGLVertexArrayObject[]; shaders: WebGLShader[] } = { textures: [], buffers: [], vaos: [], shaders: [] };
  let view = fitView(canvas.width || 1, canvas.height || 1, W, H);

  function free() {
    for (const t of owned.textures) gl.deleteTexture(t);
    for (const b of owned.buffers) gl.deleteBuffer(b);
    for (const v of owned.vaos) gl.deleteVertexArray(v);
    for (const s of owned.shaders) gl.deleteShader(s);
    if (prog) gl.deleteProgram(prog);
    owned = { textures: [], buffers: [], vaos: [], shaders: [] };
    draws = [];
    vaos = [];
  }

  function upload(data: Float32Array, idx: Uint32Array): WebGLVertexArrayObject {
    const p = prog!;
    const vao = gl.createVertexArray();
    const vbo = gl.createBuffer();
    const ibo = gl.createBuffer();
    owned.vaos.push(vao);
    owned.buffers.push(vbo, ibo);
    gl.bindVertexArray(vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
    gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW);
    const attr = (name: string, size: number, offset: number) => {
      const a = gl.getAttribLocation(p, name);
      if (a < 0) return;
      gl.enableVertexAttribArray(a);
      gl.vertexAttribPointer(a, size, gl.FLOAT, false, STRIDE * 4, offset * 4);
    };
    attr("a_src", 2, 0);
    attr("a_z", 1, 2);
    attr("a_g", 2, 3);
    attr("a_dz", 2, 5);
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, ibo);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, idx, gl.STATIC_DRAW);
    gl.bindVertexArray(null);
    return vao;
  }

  async function build() {
    const p = gl.createProgram();
    const vs = gl.createShader(gl.VERTEX_SHADER);
    const fs = gl.createShader(gl.FRAGMENT_SHADER);
    if (!p || !vs || !fs) throw new Error("WebGL2 could not make a program");
    owned.shaders.push(vs, fs);
    gl.shaderSource(vs, VS);
    gl.shaderSource(fs, FS);
    gl.compileShader(vs);
    gl.compileShader(fs);
    gl.attachShader(p, vs);
    gl.attachShader(p, fs);
    gl.linkProgram(p);
    const parallel = gl.getExtension("KHR_parallel_shader_compile") as { COMPLETION_STATUS_KHR: number } | null;
    if (parallel) for (let i = 0; i < 400 && !gl.getProgramParameter(p, parallel.COMPLETION_STATUS_KHR); i++) await breathe();
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) {
      throw new Error(gl.getShaderInfoLog(vs) || gl.getShaderInfoLog(fs) || gl.getProgramInfoLog(p) || "program did not link");
    }
    prog = p;
    gl.useProgram(p);
    U = {};

    const textures = new Map<string, WebGLTexture>();
    for (const [file, img] of images) {
      const t = gl.createTexture();
      gl.bindTexture(gl.TEXTURE_2D, t);
      gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
      gl.pixelStorei(gl.UNPACK_COLORSPACE_CONVERSION_WEBGL, gl.NONE);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, gl.RGBA, gl.UNSIGNED_BYTE, img);
      gl.generateMipmap(gl.TEXTURE_2D);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      if (t) {
        textures.set(file, t);
        owned.textures.push(t);
      }
      await breathe();
    }

    vaos = meshes.map((m) => ({ vao: upload(m.data, m.idx), count: m.idx.length, cell: m.cell }));
    // the contact shadow: one flat quad on the picture, never turned
    const sh = displacedGrid([W * 0.02, H * 0.72, W * 0.96, H * 0.3], 1, 1, () => 0);
    shadowVao = upload(sh.data, sh.idx);

    draws = [];
    let fi = 0;
    for (const l of spec.layers) {
      const mesh = which.get(l.name);
      if (mesh === undefined) continue; // the backplate, a halo: not part of a cut-out
      if (l.kind === "aperture_glow" && !R.aperture) continue;
      if (l.file && !textures.has(l.file)) continue;
      const kind = KIND[l.kind] ?? 2;
      const light = l.mode === "light" || kind === 8 || kind === 9;
      const d: Draw = {
        spec: l,
        kind,
        solid: !light && Boolean(l.file),
        tex: l.file ? (textures.get(l.file) ?? null) : null,
        bbox: l.bbox ?? [0, 0, W, H],
        mesh,
      };
      if (l.kind === "float") {
        const f = l.float ?? {};
        const bb = d.bbox;
        d.float = {
          amp: f.amp ?? [1, 3],
          freq: f.freq ?? 0.15,
          rot: ((f.rot_deg ?? 0) * Math.PI) / 180,
          pivot: f.pivot ?? [bb[0] + bb[2] / 2, bb[1] + bb[3] / 2],
          phase: 1.7 * fi + 0.4,
          phase2: 2.3 * fi + 1.1,
        };
        fi++;
      }
      if (l.kind === "eye") {
        const bb = d.bbox;
        d.eyeC = l.meta?.center ?? [bb[0] + bb[2] / 2, bb[1] + bb[3] / 2];
        d.right = !l.name.endsWith("_l");
      }
      draws.push(d);
    }
    gl.uniform1i(loc("u_tex"), 0);
    layout();
  }

  function loc(name: string): WebGLUniformLocation | null {
    if (!(name in U)) U[name] = prog ? gl.getUniformLocation(prog, name) : null;
    return U[name] ?? null;
  }

  function layout() {
    view = fitView(canvas.width || 1, canvas.height || 1, W, H);
  }

  const onLost = (e: Event) => {
    e.preventDefault();
    lost = true;
  };
  const onRestore = () => {
    if (disposed) return;
    owned = { textures: [], buffers: [], vaos: [], shaders: [] };
    prog = null;
    build().then(
      () => {
        if (disposed) return;
        lost = false;
        onRestored();
      },
      (error: unknown) => console.warn(`[stage] the depth face could not be rebuilt after the GPU reset: ${error instanceof Error ? error.message : String(error)}`),
    );
  };
  canvas.addEventListener("webglcontextlost", onLost);
  canvas.addEventListener("webglcontextrestored", onRestore);

  await build();

  function render(pose: Pose, look: LookPose) {
    if (lost || disposed || gl.isContextLost() || !shadowVao) return;
    const t = pose.t;
    const act = pose.activity;
    const voice = pose.voice;
    const head = R.head;
    const ap = R.aperture;
    const yaw = clamp(look.yaw + pose.yaw, -YAW_LIMIT, YAW_LIMIT);
    const pitch = clamp(look.pitch + pose.pitch, -PITCH_LIMIT, PITCH_LIMIT);

    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.clearColor(0, 0, 0, 0);
    gl.clearDepth(1);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    gl.enable(gl.BLEND);
    gl.depthFunc(gl.LEQUAL);
    gl.uniform4fv(loc("u_view"), view);
    gl.uniform4f(loc("u_cam"), cam.cx, cam.cy, cam.distance, cam.pivotZ);
    gl.uniform2f(loc("u_pivot"), cam.pivotX, cam.pivotY);
    gl.uniform2f(loc("u_headR"), head.radius[0], head.radius[1]);
    gl.uniform1f(loc("u_imgH"), H);
    gl.uniform2f(loc("u_size"), W, H);
    gl.uniform3f(loc("u_ang"), yaw * DEG, pitch * DEG, pose.roll * DEG);
    gl.uniform1f(loc("u_breath"), act * (0.5 - 0.5 * Math.cos((2 * Math.PI * t) / 5.2)) * H * 0.006);
    gl.uniform1f(loc("u_sway"), act * (0.35 * Math.sin(t * 0.37) + 0.2 * Math.sin(t * 0.61 + 1.3)) * DEG);
    gl.uniform3f(loc("u_key"), 0.45, -0.5, 0.74);
    gl.uniform1f(loc("u_voice"), voice);
    gl.uniform3fv(loc("u_mouthCol"), colors.mouth);
    gl.uniform1f(loc("u_fade"), H * 0.08);
    // rim lights: the rig's teal from the side he turns toward, its ember from the other
    const turn = yaw / YAW_LIMIT;
    const side = turn >= 0 ? 1 : -1;
    gl.uniform4f(loc("u_rimA"), 0.82 * side, -0.57, 0.28 + 0.25 * Math.abs(turn), 7);
    gl.uniform3fv(loc("u_rimACol"), colors.mouth);
    gl.uniform4f(loc("u_rimB"), -0.88 * side, -0.47, 0.16, 6);
    gl.uniform3fv(loc("u_rimBCol"), colors.lineCore);
    const lift = ap ? pose.open * (ap.open_frac ?? 0.15) * (ap.bottom - ap.top) * (0.75 + 0.35 * Math.min(1, voice)) : 0;
    if (ap) gl.uniform2f(loc("u_aperX"), ap.x0, ap.x1);

    // the contact shadow first, flat on the picture under him
    gl.disable(gl.DEPTH_TEST);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    gl.uniform1i(loc("u_kind"), SHADOW_KIND);
    gl.uniform4f(loc("u_shadow"), head.center[0], H * 0.99, W * 0.46, H * 0.12);
    gl.uniform1f(loc("u_shadowA"), 0.55);
    gl.bindVertexArray(shadowVao);
    gl.drawElements(gl.TRIANGLES, 6, gl.UNSIGNED_INT, 0);

    for (const d of draws) {
      const l = d.spec;
      const kind = d.kind;
      if (kind === 8 && lift < 0.2) continue;
      const mesh = vaos[d.mesh];
      if (!mesh) continue;
      if (ap && kind >= 6 && kind <= 8) {
        const lit = kind === 8;
        gl.uniform4f(loc("u_aper"), ap.top, lit ? gapCentre(ap.seam_y, lift, upperLip) : ap.seam_y, ap.bottom, lit ? gapHalf(lift, upperLip) : lift);
      }
      gl.uniform1i(loc("u_kind"), kind);
      gl.uniform1i(loc("u_solid"), d.solid ? 1 : 0);
      gl.uniform1f(loc("u_follow"), kind === 3 ? (l.follow ?? 1) : 1);
      gl.uniform1f(loc("u_cell"), mesh.cell);
      // the shards' cut edges are hard; a full rim would outline every one of them
      gl.uniform1f(loc("u_rimK"), kind === 3 ? 0.35 : 1);
      let gain = l.gain ?? 1;
      if (kind === 5) gain *= 0.9 + (l.voice_gain ?? 2) * Math.max(voice * 0.8, pose.open) * 0.6;
      if (kind === 10) gain *= 1 + act * (l.pulse?.amp ?? 0.15) * Math.sin(2 * Math.PI * (l.pulse?.freq ?? 0.2) * t) + (l.voice_gain ?? 0) * voice;
      if (kind === 4 && d.eyeC) {
        const blink = d.right ? pose.blinkR : pose.blinkL;
        if (l.blink === "iris") {
          gl.uniform4f(loc("u_eye"), d.eyeC[0], d.eyeC[1], 0.92 * Math.pow(blink, 0.8), 0);
          gl.uniform4f(loc("u_eyeP"), blink, d.eyeC[0], d.eyeC[1], l.meta?.core_r ?? 12);
          gain *= 1 - 0.6 * blink;
        } else {
          gl.uniform4f(loc("u_eye"), d.eyeC[0], d.eyeC[1], 0.35 * blink, 0);
          gl.uniform4f(loc("u_eyeP"), 0, 0, 0, 1);
          gain *= 1 - smooth01((blink - 0.25) / 0.4);
        }
        gl.uniform2f(loc("u_gaze"), look.eyeX + pose.gazeX * 3.5, look.eyeY + pose.gazeY * 2.0);
        gain *= 1 + (l.voice_gain ?? 0.12) * voice;
      }
      gl.uniform1f(loc("u_gain"), gain);
      gl.uniform1f(loc("u_bloom"), l.bloom ?? 0);
      if (kind === 3 && d.float) {
        const f = d.float;
        const w = 2 * Math.PI * f.freq;
        gl.uniform4f(loc("u_float"), act * f.amp[0] * Math.sin(w * 0.77 * t + f.phase2), act * -f.amp[1] * Math.sin(w * t + f.phase), act * f.rot * Math.sin(w * 0.6 * t + f.phase + 0.8), 0);
        gl.uniform2f(loc("u_floatPivot"), f.pivot[0], f.pivot[1]);
      }
      if (d.tex) {
        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, d.tex);
        gl.uniform4f(loc("u_bbox"), d.bbox[0], d.bbox[1], d.bbox[2], d.bbox[3]);
      }
      if (d.solid) {
        // a solid hides its own far side through a turn, and is laid over what came before it
        gl.enable(gl.DEPTH_TEST);
        gl.depthMask(true);
        gl.clear(gl.DEPTH_BUFFER_BIT);
        gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
      } else {
        gl.disable(gl.DEPTH_TEST);
        gl.blendFunc(gl.ONE, gl.ONE);
      }
      gl.bindVertexArray(mesh.vao);
      gl.drawElements(gl.TRIANGLES, mesh.count, gl.UNSIGNED_INT, 0);
    }
    gl.bindVertexArray(null);
  }

  return {
    render,
    resize: layout,
    get lost() {
      return lost || gl.isContextLost();
    },
    dispose() {
      disposed = true;
      canvas.removeEventListener("webglcontextlost", onLost);
      canvas.removeEventListener("webglcontextrestored", onRestore);
      if (!gl.isContextLost()) free();
    },
  };
}
