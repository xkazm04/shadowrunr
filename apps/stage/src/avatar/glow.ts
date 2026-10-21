// The glow-rig renderer: a figure made of light, drawn in WebGL2 with one program and one draw per
// layer, on a TRANSPARENT canvas. Every layer is premultiplied light whose alpha is its brightest
// channel, added (colour and coverage), so it composites as light over the stage's ground. A solid
// part (the skull, the jaw, the collar) first takes out what lies behind it by its coverage, then
// adds its own light. On top of the layers: head parallax (an orthographic sphere warp with
// per-layer depth), bloom from the textures' mip chain, eye lights that close as an iris or dim,
// the seam that brightens with the voice, the hinged aperture with light in its slit, floating
// parts, breathing and sway, and an optional ground line the lower figure dissolves into.
// Everything that moves by time is scaled by the pose's `activity`, so a settled face draws
// exactly its rest frame.
//
// No allocation per frame. A lost context is waited out and rebuilt from the decoded pictures.
// Shaders: glow-shaders.ts. Pure helpers (mesh, palette, ground line): glow-geom.ts. The per-layer
// rules every face shares: layer-frame.ts; the GL plumbing: gl-kit.ts.

import type { Pose } from "./driver";
import { compileProgram, contextLossGuard, uploadLayerTextures } from "./gl-kit";
import { BODY_COVER, CONTEXT_ATTRIBUTES, gridMesh, groundLine, rigColors } from "./glow-geom";
import { FS, KIND, VS } from "./glow-shaders";
import { apertureLift, describeLayer, GLOW_MAPPING, layerFrame, newLayerFrame, type FaceRig, type LayerDraw } from "./layer-frame";
import type { LayerSpec, LoadedForm } from "./layers";

type Owned = { textures: WebGLTexture[]; buffers: WebGLBuffer[]; vaos: WebGLVertexArrayObject[]; shaders: WebGLShader[] };

export interface GlowRenderer {
  /** Draw one frame of `pose`. A lost context draws nothing until it is restored. */
  render(pose: Pose): void;
  /** Re-fit after the canvas's backing size changed. */
  resize(): void;
  /** True while the GPU context is gone. */
  readonly lost: boolean;
  dispose(): void;
}

interface DrawLayer extends LayerDraw {
  bbox: [number, number, number, number];
  tex: WebGLTexture | null;
  vao: WebGLVertexArrayObject;
  count: number;
}

const DEG = Math.PI / 180;

/**
 * Make a renderer for `form` on `canvas`. Rejects when WebGL2 is not there (the caller shows
 * nothing and warns once). Setup is cut into pieces with the main thread handed back between them
 * (the program, each texture, the meshes), because done at once it measured a 61 ms task. The
 * shaders compile in parallel where the driver offers it. `onRestored` is called after a lost
 * context came back and was rebuilt, so the caller can draw again.
 */
export async function createGlowRenderer(
  canvas: HTMLCanvasElement,
  form: LoadedForm,
  onRestored: () => void = () => {},
): Promise<GlowRenderer> {
  const { spec, images } = form;
  const R = spec.rig;
  const colors = rigColors(R);
  const upperLip = spec.layers.some((l) => l.kind === "aperture_upper");
  const W = spec.width;
  const H = spec.height;
  const first = canvas.getContext("webgl2", CONTEXT_ATTRIBUTES);
  if (!first) throw new Error("WebGL2 unavailable");
  const gl: WebGL2RenderingContext = first;

  let disposed = false;
  let prog: WebGLProgram | null = null;
  let U: Record<string, WebGLUniformLocation | null> = {};
  let draws: DrawLayer[] = [];
  /** Everything `build` made, so `dispose` can free it without losing the context (a StrictMode
   *  remount draws on the same canvas, and so the same context, a moment later). */
  let owned: Owned = {
    textures: [],
    buffers: [],
    vaos: [],
    shaders: [],
  };
  const view = new Float32Array(4);
  const frame = newLayerFrame();
  const face: FaceRig = { aperture: R.aperture, upperLip, lift: 0 };

  function free() {
    for (const t of owned.textures) gl.deleteTexture(t);
    for (const b of owned.buffers) gl.deleteBuffer(b);
    for (const v of owned.vaos) gl.deleteVertexArray(v);
    for (const s of owned.shaders) gl.deleteShader(s);
    if (prog) gl.deleteProgram(prog);
    owned = { textures: [], buffers: [], vaos: [], shaders: [] };
    draws = [];
  }

  async function build() {
    prog = await compileProgram(gl, VS, FS, owned.shaders);
    gl.useProgram(prog);
    U = {};
    const aSrc = gl.getAttribLocation(prog, "a_src");

    const textures = await uploadLayerTextures(gl, images, owned.textures);

    const ap = R.aperture;
    draws = [];
    let fi = 0;
    const ground = R.ground;
    const layers: LayerSpec[] = ground ? [...spec.layers, { name: "ground", kind: "ground", mode: "light" }] : spec.layers;
    for (const l of layers) {
      let bb: [number, number, number, number] = l.bbox ?? [0, 0, W, H];
      if (l.kind === "halo") {
        const m = l.meta ?? {};
        const r = (m.radius ?? 0) + 40;
        const c = m.center ?? [W / 2, H / 2];
        bb = [c[0] - r, c[1] - r, 2 * r, 2 * r];
      }
      if (l.kind === "aperture_glow") {
        if (!ap) continue;
        bb = [ap.x0 - 10, ap.top - 20, ap.x1 - ap.x0 + 20, ap.bottom - ap.top + 40];
      }
      if (l.kind === "ground" && ground) bb = groundLine(ground, W, { t: 0, activity: 0, voice: 0 }).box;
      if (l.file && !textures.has(l.file)) continue;
      const step = l.kind === "static" || l.kind === "ground" ? 1e6 : l.kind.startsWith("aperture") ? 2 : 8;
      const { pos, idx } = gridMesh(bb[0], bb[1], bb[2], bb[3], step);
      const vao = gl.createVertexArray();
      const vbo = gl.createBuffer();
      const ibo = gl.createBuffer();
      owned.vaos.push(vao);
      owned.buffers.push(vbo, ibo);
      gl.bindVertexArray(vao);
      gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
      gl.bufferData(gl.ARRAY_BUFFER, pos, gl.STATIC_DRAW);
      gl.enableVertexAttribArray(aSrc);
      gl.vertexAttribPointer(aSrc, 2, gl.FLOAT, false, 0, 0);
      gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, ibo);
      gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, idx, gl.STATIC_DRAW);
      gl.bindVertexArray(null);
      draws.push({
        ...describeLayer(l, KIND[l.kind] ?? 2, bb, fi),
        bbox: bb,
        tex: l.file ? (textures.get(l.file) ?? null) : null,
        vao,
        count: idx.length,
      });
      if (l.kind === "float") fi++;
    }
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    gl.uniform1i(loc("u_tex"), 0);
    layout();
  }

  function loc(name: string): WebGLUniformLocation | null {
    if (!(name in U)) U[name] = prog ? gl.getUniformLocation(prog, name) : null;
    return U[name] ?? null;
  }

  function layout() {
    const cw = canvas.width || 1;
    const ch = canvas.height || 1;
    // fit the square art into the box, centred
    const s = Math.min(cw / W, ch / H);
    const ox = cw / 2 - (W / 2) * s;
    const oy = ch / 2 - (H / 2) * s;
    view[0] = (2 * s) / cw;
    view[1] = (-2 * s) / ch;
    view[2] = (2 * ox) / cw - 1;
    view[3] = 1 - (2 * oy) / ch;
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
  );

  await build();

  function render(pose: Pose) {
    if (guard.lost || disposed || gl.isContextLost()) return;
    const t = pose.t;
    const act = pose.activity;
    const voice = pose.voice;
    const head = R.head;
    const ap = R.aperture;

    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.uniform4fv(loc("u_view"), view);
    gl.uniform2f(loc("u_headC"), head.center[0], head.center[1]);
    gl.uniform2f(loc("u_headR"), head.radius[0], head.radius[1]);
    gl.uniform1f(loc("u_pivotY"), head.pivot_y ?? head.chin_y);
    gl.uniform1f(loc("u_imgH"), H);
    gl.uniform3f(loc("u_ang"), pose.yaw * DEG, pose.pitch * DEG, pose.roll * DEG);
    gl.uniform1f(loc("u_breath"), act * (0.5 - 0.5 * Math.cos((2 * Math.PI * t) / 5.2)) * H * 0.006);
    gl.uniform1f(loc("u_sway"), act * (0.35 * Math.sin(t * 0.37) + 0.2 * Math.sin(t * 0.61 + 1.3)) * DEG);
    gl.uniform1f(loc("u_t"), t);
    gl.uniform1fv(loc("u_env[0]"), pose.history);
    gl.uniform1f(loc("u_voice"), voice);
    const shim = R.shimmer;
    if (shim) {
      gl.uniform3f(loc("u_shim"), shim.center[0], shim.center[1], shim.speed_px_s);
      gl.uniform2f(loc("u_shimK"), shim.wavelength_px, shim.gain);
    }
    gl.uniform1f(loc("u_tipPulse"), act * (R.tip_pulse ?? 0) * Math.sin((2 * Math.PI * t) / 5.2 - 0.6));
    const gp = R.glint;
    if (gp) {
      // the sweep drifts slowly and follows the head turn, so light seems to move over the facets
      const ang = -0.6;
      const span = W * 1.4;
      const at = (((t * 0.045 + pose.yaw * 0.02) % 1) + 1) % 1;
      gl.uniform4f(loc("u_glintP"), Math.cos(ang), Math.sin(ang), at * span - span * 0.2, gp.width_px);
      gl.uniform1f(loc("u_glintGain"), gp.gain);
    }
    face.lift = apertureLift(ap, pose);
    if (ap) gl.uniform2f(loc("u_aperX"), ap.x0, ap.x1);
    gl.uniform2f(loc("u_size"), W, H);
    gl.uniform1i(loc("u_pass"), 0);
    gl.uniform1f(loc("u_body"), BODY_COVER);
    const gr = R.ground;
    const line = gr ? groundLine(gr, W, pose) : null;
    if (gr && line) {
      gl.uniform4f(loc("u_gnd"), line.cx, gr.y, gr.half_width, gr.fade);
      gl.uniform1f(loc("u_gndCurve"), gr.curve);
    } else gl.uniform4f(loc("u_gnd"), 0, 0, 1, 0);
    gl.uniform1f(loc("u_pxArt"), W / Math.max(1, Math.min(canvas.width, canvas.height)));
    gl.uniform3fv(loc("u_lineCore"), colors.lineCore);
    gl.uniform3fv(loc("u_lineGlow"), colors.lineGlow);
    gl.uniform3fv(loc("u_mouthCol"), colors.mouth);
    gl.blendFunc(gl.ONE, gl.ONE);

    for (const d of draws) {
      const l = d.spec;
      const kind = d.kind;
      const f = layerFrame(d, pose, face, GLOW_MAPPING, frame);
      if (f.skip) continue;
      if (f.hasAper) gl.uniform4f(loc("u_aper"), f.aper[0], f.aper[1], f.aper[2], f.aper[3]);
      gl.uniform1i(loc("u_kind"), kind);
      gl.uniform1f(loc("u_depth"), l.depth ?? 0);
      gl.uniform1f(loc("u_follow"), f.follow);
      // the backplate counts as light: its black is the page, not a box
      const isLight = l.mode === "light" || kind === 0 || kind === 8 || kind === 9 || kind === 11;
      gl.uniform1f(loc("u_occl"), isLight ? 0 : (l.occl ?? 1));
      if (f.hasEye) {
        gl.uniform4f(loc("u_eye"), f.eye[0], f.eye[1], f.eye[2], f.eye[3]);
        gl.uniform4f(loc("u_eyeP"), f.eyeP[0], f.eyeP[1], f.eyeP[2], f.eyeP[3]);
        gl.uniform2f(loc("u_gaze"), f.gaze[0], f.gaze[1]);
      }
      // the ground line's gain is its own: it brightens and fades with the line
      gl.uniform1f(loc("u_gain"), kind === 11 && line ? line.gain : f.gain);
      gl.uniform1f(loc("u_bloom"), l.bloom ?? 0);
      gl.uniform1i(loc("u_shimmer"), l.shimmer && shim ? 1 : 0);
      gl.uniform1i(loc("u_glint"), l.glint && gp ? 1 : 0);
      if (f.hasFloat && d.float) {
        gl.uniform4f(loc("u_float"), f.float[0], f.float[1], f.float[2], f.float[3]);
        gl.uniform2f(loc("u_floatPivot"), d.float.pivot[0], d.float.pivot[1]);
      }
      if (kind === 9) {
        const m = l.meta ?? {};
        const c = m.center ?? [W / 2, H / 2];
        const col = m.color ?? [0.13, 0.83, 0.93];
        gl.uniform4f(loc("u_ring"), c[0], c[1], m.radius ?? 0, Math.max(1.0, l.ring_w ?? 1.7));
        gl.uniform3f(loc("u_ringCol"), Math.min(1, col[0] * 1.25), Math.min(1, col[1] * 1.25), Math.min(1, col[2] * 1.25));
      }
      if (d.tex) {
        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, d.tex);
        gl.uniform4f(loc("u_bbox"), d.bbox[0], d.bbox[1], d.bbox[2], d.bbox[3]);
      }
      gl.bindVertexArray(d.vao);
      if (!isLight) {
        // a solid first takes out what lies behind it by its coverage (the ring, the backplate),
        // then adds its own light like any light: so it hides the far parts, never the stage
        gl.blendFunc(gl.ZERO, gl.ONE_MINUS_SRC_ALPHA);
        gl.uniform1i(loc("u_pass"), 1);
        gl.drawElements(gl.TRIANGLES, d.count, gl.UNSIGNED_INT, 0);
        gl.blendFunc(gl.ONE, gl.ONE);
        gl.uniform1i(loc("u_pass"), 0);
      }
      gl.drawElements(gl.TRIANGLES, d.count, gl.UNSIGNED_INT, 0);
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
