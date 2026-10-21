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
// The per-layer rules every face shares: ../layer-frame.ts (the depth face's gaze adds its look
// follower's eye slide; see the table there); the GL plumbing: ../gl-kit.ts.

import type { Pose } from "../driver";
import { compileProgram, contextLossGuard, uploadLayerTextures } from "../gl-kit";
import { rigColors } from "../glow-geom";
import { KIND } from "../glow-shaders";
import { apertureLift, DEPTH_MAPPING, describeLayer, layerFrame, newLayerFrame, type FaceRig, type LayerDraw } from "../layer-frame";
import type { LoadedForm } from "../layers";
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

interface Draw extends LayerDraw {
  solid: boolean;
  tex: WebGLTexture | null;
  bbox: [number, number, number, number];
  mesh: number;
}

const clamp = (x: number, a: number, b: number) => (x < a ? a : x > b ? b : x);

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

  let disposed = false;
  let prog: WebGLProgram | null = null;
  let U: Record<string, WebGLUniformLocation | null> = {};
  let draws: Draw[] = [];
  let vaos: { vao: WebGLVertexArrayObject; count: number; cell: number }[] = [];
  let shadowVao: WebGLVertexArrayObject | null = null;
  let owned: { textures: WebGLTexture[]; buffers: WebGLBuffer[]; vaos: WebGLVertexArrayObject[]; shaders: WebGLShader[] } = { textures: [], buffers: [], vaos: [], shaders: [] };
  let view = fitView(canvas.width || 1, canvas.height || 1, W, H);
  const frame = newLayerFrame();
  const eyeSlide = { eyeX: 0, eyeY: 0 };
  const face: FaceRig = { aperture: R.aperture, upperLip, lift: 0, look: eyeSlide };

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
    const p = await compileProgram(gl, VS, FS, owned.shaders);
    prog = p;
    gl.useProgram(p);
    U = {};

    const textures = await uploadLayerTextures(gl, images, owned.textures);

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
      const bb: [number, number, number, number] = l.bbox ?? [0, 0, W, H];
      draws.push({
        ...describeLayer(l, kind, bb, fi),
        solid: !light && Boolean(l.file),
        tex: l.file ? (textures.get(l.file) ?? null) : null,
        bbox: bb,
        mesh,
      });
      if (l.kind === "float") fi++;
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

  const guard = contextLossGuard(
    canvas,
    () => {
      // the old handles died with the context; drop them, do not delete them
      owned = { textures: [], buffers: [], vaos: [], shaders: [] };
      prog = null;
      return build();
    },
    onRestored,
    "the depth face",
  );

  await build();

  function render(pose: Pose, look: LookPose) {
    if (guard.lost || disposed || gl.isContextLost() || !shadowVao) return;
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
    face.lift = apertureLift(ap, pose);
    eyeSlide.eyeX = look.eyeX;
    eyeSlide.eyeY = look.eyeY;
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
      const f = layerFrame(d, pose, face, DEPTH_MAPPING, frame);
      if (f.skip) continue;
      const mesh = vaos[d.mesh];
      if (!mesh) continue;
      if (f.hasAper) gl.uniform4f(loc("u_aper"), f.aper[0], f.aper[1], f.aper[2], f.aper[3]);
      gl.uniform1i(loc("u_kind"), kind);
      gl.uniform1i(loc("u_solid"), d.solid ? 1 : 0);
      gl.uniform1f(loc("u_follow"), f.follow);
      gl.uniform1f(loc("u_cell"), mesh.cell);
      // the shards' cut edges are hard; a full rim would outline every one of them
      gl.uniform1f(loc("u_rimK"), kind === 3 ? 0.35 : 1);
      if (f.hasEye) {
        gl.uniform4f(loc("u_eye"), f.eye[0], f.eye[1], f.eye[2], f.eye[3]);
        gl.uniform4f(loc("u_eyeP"), f.eyeP[0], f.eyeP[1], f.eyeP[2], f.eyeP[3]);
        gl.uniform2f(loc("u_gaze"), f.gaze[0], f.gaze[1]);
      }
      gl.uniform1f(loc("u_gain"), f.gain);
      gl.uniform1f(loc("u_bloom"), l.bloom ?? 0);
      if (f.hasFloat && d.float) {
        gl.uniform4f(loc("u_float"), f.float[0], f.float[1], f.float[2], f.float[3]);
        gl.uniform2f(loc("u_floatPivot"), d.float.pivot[0], d.float.pivot[1]);
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
      return guard.lost || gl.isContextLost();
    },
    dispose() {
      disposed = true;
      guard.dispose();
      if (!gl.isContextLost()) free();
    },
  };
}
