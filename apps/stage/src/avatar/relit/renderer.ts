// The relit renderer: the rig's layers as lit cards in 3D, on a TRANSPARENT canvas with no
// backplate. Its geometry is the depth face's (one 128-cell grid over the head that every head
// layer shares, a coarser grid over the torso, a flat card per crown shard at its plane depth,
// all displaced by the depth map and turned under a perspective camera); what it adds is light
// (shaders.ts): normals from the depth field and each picture, a key light that changes with the
// turn, the eye fire and the monocle's ring as point lights on the bone, a hinged jaw, a brow band
// and the performance channels (eye light, flare, brow, gestures, lean).
//
// No allocation per frame. A lost context is waited out and rebuilt from the decoded pictures.

import type { Pose } from "../driver";
import { compileProgram, contextLossGuard, uploadLayerTextures } from "../gl-kit";
import { rigColors } from "../glow-geom";
import { KIND } from "../glow-shaders";
import { apertureLift, DEPTH_MAPPING, describeLayer, layerFrame, newLayerFrame, type FaceRig, type LayerDraw } from "../layer-frame";
import type { LoadedForm } from "../layers";
import { cameraFor, displacedGrid, fitView, rigMeshes, STRIDE, type DepthField, type DepthMeta } from "../depth/geom";
import type { LookPose } from "../depth/look";
import type { PerformancePose } from "../performance";
import { browBand, eyeFire, headTurn, jawHinge, rigLights, sobelSlopes, YAW_LIMIT, type Turn } from "./geom";
import { FS, LIGHTS, SHADOW_KIND, VS } from "./shaders";

export interface RelitRenderer {
  render(pose: Pose, look: LookPose, perf: PerformancePose): void;
  resize(): void;
  /** Wait for the GPU to finish the frame (the harness's timing only): a one-pixel read-back. */
  finish(): void;
  readonly lost: boolean;
  dispose(): void;
}

/** The context the relit face asks for: as the depth face's, transparent with a depth buffer. */
export const RELIT_CONTEXT: WebGLContextAttributes = Object.freeze({
  alpha: true,
  premultipliedAlpha: true,
  antialias: true,
  depth: true,
  preserveDrawingBuffer: false,
});

const DEG = Math.PI / 180;
/** The eye fire's and the ring's light on the bone at eyeLight 1, and the ring's at its rest pulse. */
const EYE_LIGHT = 0.9;
const RING_LIGHT = 0.55;

/** Each layer's surface: bump px, bevel px, share of the point lights, specular. */
const SURF_BONE: readonly [number, number, number, number] = [5, 6, 1, 0.3];
const SURF_CLOAK: readonly [number, number, number, number] = [3, 4, 0.45, 0.12];
const SURF_SHARD: readonly [number, number, number, number] = [4, 10, 0.8, 0.55];

interface Draw extends LayerDraw {
  solid: boolean;
  tex: WebGLTexture | null;
  bbox: [number, number, number, number];
  mesh: number;
  surf: readonly [number, number, number, number];
}

/** Make the renderer. Rejects when WebGL2 is not there or the program does not link. */
export async function createRelitRenderer(
  canvas: HTMLCanvasElement,
  form: LoadedForm,
  field: DepthField,
  meta: DepthMeta,
  onRestored: () => void = () => {},
): Promise<RelitRenderer> {
  const { spec, images } = form;
  const R = spec.rig;
  const W = spec.width;
  const H = spec.height;
  const colors = rigColors(R);
  const upperLip = spec.layers.some((l) => l.kind === "aperture_upper");
  const first = canvas.getContext("webgl2", RELIT_CONTEXT);
  if (!first) throw new Error("WebGL2 unavailable");
  const gl: WebGL2RenderingContext = first;
  const { meshes, which } = rigMeshes(form, field, meta);
  // the head and the torso take the field's Sobel normals; a shard is a flat card, its volume is its bevel
  for (const m of meshes.slice(0, 2)) sobelSlopes(m, field, Math.max(2, m.cell));
  const cam = cameraFor(W, H, R.head, meta.near - 0.8 * R.head.radius[0]);
  const lights = rigLights(form, field);
  // the monocle is a ring of glass in front of the eye: a flat card at its light's depth, not a
  // decal on the skull's surface (which tears where the ring overhangs the skull's edge)
  const ringLayer = lights.ring ? spec.layers.find((l) => l.kind === "pulse" && l.meta?.center && l.bbox) : undefined;
  if (ringLayer && lights.ring) {
    const bb = ringLayer.bbox!;
    const z = lights.ring.z;
    which.set(ringLayer.name, meshes.length);
    meshes.push(displacedGrid([bb[0] - 2, bb[1] - 2, bb[2] + 4, bb[3] + 4], 4, 4, () => z));
  }
  const hinge = jawHinge(R, field);
  const band = browBand(form);

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
  const turn: Turn = { yaw: 0, pitch: 0, roll: 0, lean: 0 };
  const sync = new Uint8Array(4);
  const lp = new Float32Array(4 * LIGHTS);
  const lc = new Float32Array(4 * LIGHTS);
  const eyeGain = [1, 1];
  const point = [lights.eyes[0] ?? null, lights.eyes[1] ?? null, lights.ring];
  point.forEach((p, i) => {
    if (!p) return;
    lp.set([p.x, p.y, p.z, p.r0], 4 * i);
    lc[4 * i + 3] = p.r1;
  });

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
        surf: kind === 1 ? SURF_CLOAK : kind === 3 ? SURF_SHARD : SURF_BONE,
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
      owned = { textures: [], buffers: [], vaos: [], shaders: [] };
      prog = null;
      return build();
    },
    onRestored,
    "the relit face",
  );

  await build();

  function render(pose: Pose, look: LookPose, perf: PerformancePose) {
    if (guard.lost || disposed || gl.isContextLost() || !shadowVao) return;
    const t = pose.t;
    const act = pose.activity;
    const voice = pose.voice;
    const head = R.head;
    const ap = R.aperture;
    headTurn(pose, look, perf, turn);
    const fire = eyeFire(perf);

    face.lift = apertureLift(ap, pose);
    eyeSlide.eyeX = look.eyeX;
    eyeSlide.eyeY = look.eyeY;

    // the eye fire's light follows its layers' blink and warmth, the ring's its pulse
    let ei = 0;
    let ring = 1;
    for (const d of draws) {
      if (d.kind === 4 && ei < 2) eyeGain[ei++] = layerFrame(d, pose, face, DEPTH_MAPPING, frame).gain;
      else if (d.kind === 10) ring = layerFrame(d, pose, face, DEPTH_MAPPING, frame).gain;
    }
    for (let i = 0; i < LIGHTS; i++) {
      const p = point[i];
      const k = !p ? 0 : i < 2 ? EYE_LIGHT * fire.light * (eyeGain[i] ?? 1) : RING_LIGHT * ring;
      const col = i < 2 ? colors.mouth : colors.lineCore;
      lc[4 * i] = col[0] * k;
      lc[4 * i + 1] = col[1] * k;
      lc[4 * i + 2] = col[2] * k;
    }

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
    gl.uniform3f(loc("u_ang"), turn.yaw * DEG, turn.pitch * DEG, turn.roll * DEG);
    gl.uniform1f(loc("u_lean"), turn.lean);
    gl.uniform1f(loc("u_breath"), act * (0.5 - 0.5 * Math.cos((2 * Math.PI * t) / 5.2)) * H * 0.006);
    gl.uniform1f(loc("u_sway"), act * (0.35 * Math.sin(t * 0.37) + 0.2 * Math.sin(t * 0.61 + 1.3)) * DEG);
    gl.uniform3f(loc("u_key"), 0.45, -0.5, 0.74);
    gl.uniform1f(loc("u_voice"), voice);
    gl.uniform3fv(loc("u_mouthCol"), colors.mouth);
    gl.uniform1f(loc("u_fade"), H * 0.08);
    gl.uniform4fv(loc("u_lp"), lp);
    gl.uniform4fv(loc("u_lc"), lc);
    if (hinge) {
      const a = hinge.angle(face.lift);
      gl.uniform4f(loc("u_jaw"), hinge.y, hinge.z, Math.sin(a), Math.cos(a));
    } else {
      gl.uniform4f(loc("u_jaw"), 0, 0, 0, 1);
    }
    if (band) {
      gl.uniform4f(loc("u_browC"), band.centres[0], band.centres[1], band.centres[2], band.centres[3]);
      gl.uniform3f(loc("u_browS"), band.size[0], band.size[1], Math.max(-1, Math.min(1, perf.brow)));
    } else {
      gl.uniform3f(loc("u_browS"), 1, 1, 0);
    }
    const sideTurn = turn.yaw / YAW_LIMIT;
    const side = sideTurn >= 0 ? 1 : -1;
    gl.uniform4f(loc("u_rimA"), 0.82 * side, -0.57, 0.28 + 0.25 * Math.abs(sideTurn), 7);
    gl.uniform3fv(loc("u_rimACol"), colors.mouth);
    gl.uniform4f(loc("u_rimB"), -0.88 * side, -0.47, 0.16, 6);
    gl.uniform3fv(loc("u_rimBCol"), colors.lineCore);
    if (ap) gl.uniform2f(loc("u_aperX"), ap.x0, ap.x1);

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
      // the shards' cut edges are hard, and the jaw's top edge is the bite: a full rim would outline them
      gl.uniform1f(loc("u_rimK"), kind === 3 ? 0.35 : kind === 7 ? 0.25 : 1);
      if (d.solid) gl.uniform4f(loc("u_surf"), d.surf[0], d.surf[1], d.surf[2], d.surf[3]);
      if (f.hasEye) {
        gl.uniform4f(loc("u_eye"), f.eye[0], f.eye[1], f.eye[2], f.eye[3]);
        gl.uniform4f(loc("u_eyeP"), f.eyeP[0], f.eyeP[1], f.eyeP[2], f.eyeP[3]);
        gl.uniform2f(loc("u_gaze"), f.gaze[0], f.gaze[1]);
      }
      // the eye fire burns with the performance's eye light, and blooms in a flare
      const eye = kind === 4;
      gl.uniform1f(loc("u_gain"), eye ? f.gain * fire.gain : f.gain);
      gl.uniform1f(loc("u_bloom"), (l.bloom ?? 0) + (eye ? fire.bloom : 0));
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
    // gl.finish() returns at once in Chrome; reading a pixel back waits for the frame for real
    finish: () => gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, sync),
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
