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
// Shaders: glow-shaders.ts. Pure helpers (mesh, palette, ground line): glow-geom.ts.

import type { Pose } from "./driver";
import { BODY_COVER, CONTEXT_ATTRIBUTES, gridMesh, groundLine, rigColors } from "./glow-geom";
import { FS, KIND, VS } from "./glow-shaders";
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

interface DrawLayer {
  spec: LayerSpec;
  kind: number;
  bbox: [number, number, number, number];
  tex: WebGLTexture | null;
  vao: WebGLVertexArrayObject;
  count: number;
  float?: { amp: [number, number]; freq: number; rot: number; pivot: [number, number]; phase: number; phase2: number };
  eyeC?: [number, number];
  right?: boolean;
}

const smooth01 = (x: number) => {
  const y = Math.min(1, Math.max(0, x));
  return y * y * (3 - 2 * y);
};
const DEG = Math.PI / 180;

/** Hand the main thread back between pieces of setup, so no one piece is a long task. */
const breathe = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

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
  const W = spec.width;
  const H = spec.height;
  const first = canvas.getContext("webgl2", CONTEXT_ATTRIBUTES);
  if (!first) throw new Error("WebGL2 unavailable");
  const gl: WebGL2RenderingContext = first;

  let lost = false;
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
    const p = gl.createProgram();
    if (!p) throw new Error("WebGL2 could not make a program");
    const parallel = gl.getExtension("KHR_parallel_shader_compile") as { COMPLETION_STATUS_KHR: number } | null;
    const vs = gl.createShader(gl.VERTEX_SHADER);
    const fs = gl.createShader(gl.FRAGMENT_SHADER);
    if (!vs || !fs) throw new Error("WebGL2 could not make a shader");
    owned.shaders.push(vs, fs);
    gl.shaderSource(vs, VS);
    gl.shaderSource(fs, FS);
    gl.compileShader(vs);
    gl.compileShader(fs);
    gl.attachShader(p, vs);
    gl.attachShader(p, fs);
    gl.linkProgram(p);
    if (parallel) {
      for (let i = 0; i < 400 && !gl.getProgramParameter(p, parallel.COMPLETION_STATUS_KHR); i++) await breathe();
    }
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) {
      const why = gl.getShaderInfoLog(vs) || gl.getShaderInfoLog(fs) || gl.getProgramInfoLog(p) || "program did not link";
      throw new Error(why);
    }
    prog = p;
    gl.useProgram(prog);
    U = {};
    const aSrc = gl.getAttribLocation(prog, "a_src");

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
      const d: DrawLayer = {
        spec: l,
        kind: KIND[l.kind] ?? 2,
        bbox: bb,
        tex: l.file ? (textures.get(l.file) ?? null) : null,
        vao,
        count: idx.length,
      };
      if (l.kind === "float") {
        const f = l.float ?? {};
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
        d.eyeC = l.meta?.center ?? [bb[0] + bb[2] / 2, bb[1] + bb[3] / 2];
        d.right = !l.name.endsWith("_l");
      }
      draws.push(d);
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

  const onLost = (e: Event) => {
    e.preventDefault(); // ask for it back
    lost = true;
  };
  const onRestore = () => {
    if (disposed) return;
    // the old handles died with the context; drop them, do not delete them
    owned = { textures: [], buffers: [], vaos: [], shaders: [] };
    prog = null;
    build().then(
      () => {
        if (disposed) return;
        lost = false;
        onRestored();
      },
      (error: unknown) =>
        console.warn(`[stage] the face could not be rebuilt after the GPU reset: ${error instanceof Error ? error.message : String(error)}`),
    );
  };
  canvas.addEventListener("webglcontextlost", onLost);
  canvas.addEventListener("webglcontextrestored", onRestore);

  await build();

  function render(pose: Pose) {
    if (lost || disposed || gl.isContextLost()) return;
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
    const lift = ap ? pose.open * (ap.open_frac ?? 0.15) * (ap.bottom - ap.top) * (0.75 + 0.35 * Math.min(1, voice)) : 0;
    if (ap) {
      gl.uniform4f(loc("u_aper"), ap.top, ap.seam_y, ap.bottom, lift);
      gl.uniform2f(loc("u_aperX"), ap.x0, ap.x1);
    }
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
      if (kind === 8 && lift < 0.2) continue;
      gl.uniform1i(loc("u_kind"), kind);
      gl.uniform1f(loc("u_depth"), l.depth ?? 0);
      gl.uniform1f(loc("u_follow"), l.follow ?? (kind === 0 || kind === 1 ? 0 : 1));
      // the backplate counts as light: its black is the page, not a box
      const isLight = l.mode === "light" || kind === 0 || kind === 8 || kind === 9 || kind === 11;
      gl.uniform1f(loc("u_occl"), isLight ? 0 : (l.occl ?? 1));
      let gain = l.gain ?? 1;
      if (kind === 11 && line) gain = line.gain;
      if (kind === 5) gain *= 0.9 + (l.voice_gain ?? 2) * Math.max(voice * 0.8, pose.open) * 0.6;
      if (kind === 10)
        gain *=
          1 +
          act * (l.pulse?.amp ?? 0.15) * Math.sin(2 * Math.PI * (l.pulse?.freq ?? 0.2) * t) +
          (l.voice_gain ?? 0) * voice;
      if (kind === 9) gain *= 0.92 + act * 0.08 * Math.sin((2 * Math.PI * t) / 5.2) + 0.12 * voice;
      if (kind === 4 && d.eyeC) {
        const blink = d.right ? pose.blinkR : pose.blinkL;
        if (l.blink === "iris") {
          gl.uniform4f(loc("u_eye"), d.eyeC[0], d.eyeC[1], 0.92 * Math.pow(blink, 0.8), 0);
          gl.uniform4f(loc("u_eyeP"), blink, d.eyeC[0], d.eyeC[1], l.meta?.core_r ?? 12);
          gain *= 1 - 0.6 * blink;
        } else {
          gl.uniform4f(loc("u_eye"), d.eyeC[0], d.eyeC[1], 0.35 * blink, 0);
          gl.uniform4f(loc("u_eyeP"), 0, 0, 0, 1);
          // a facet: bright light to dark crystal in the middle of the blink
          gain *= 1 - smooth01((blink - 0.25) / 0.4);
        }
        gl.uniform2f(loc("u_gaze"), pose.gazeX * 3.5, pose.gazeY * 2.0);
        gain *= 1 + (l.voice_gain ?? 0.12) * voice; // the eyes warm while he speaks
      }
      gl.uniform1f(loc("u_gain"), gain);
      gl.uniform1f(loc("u_bloom"), l.bloom ?? 0);
      gl.uniform1i(loc("u_shimmer"), l.shimmer && shim ? 1 : 0);
      gl.uniform1i(loc("u_glint"), l.glint && gp ? 1 : 0);
      if (kind === 3 && d.float) {
        const f = d.float;
        const w = 2 * Math.PI * f.freq;
        gl.uniform4f(
          loc("u_float"),
          act * f.amp[0] * Math.sin(w * 0.77 * t + f.phase2),
          act * -f.amp[1] * Math.sin(w * t + f.phase),
          act * f.rot * Math.sin(w * 0.6 * t + f.phase + 0.8),
          0,
        );
        gl.uniform2f(loc("u_floatPivot"), f.pivot[0], f.pivot[1]);
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
