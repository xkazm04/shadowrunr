// The cut-out renderer: the rig drawn as a streamer's cut-out in WebGL2, on a transparent canvas.
//
// Each frame draws the layers into a texture the size of the canvas (the glow's layer passes,
// adapted: shaders.ts), makes its mips, then draws one triangle over the canvas that adds the rim
// lights and lays the shadow behind and beneath him. Inside the figure the canvas is opaque,
// outside it is transparent but for the shadow. The backplate survives only inside the figure's
// rest matte, which is made once from the solids' coverage when the renderer is built.
//
// No allocation per frame. A lost context is waited out and rebuilt from the decoded pictures, the
// way the glow renderer does it.

import type { Pose } from "../driver";
import { CONTEXT_ATTRIBUTES, gapCentre, gapHalf, gridMesh, rigColors } from "../glow-geom";
import type { GlowRenderer } from "../glow";
import type { LoadedForm } from "../layers";
import { breathe, figureTarget, MATTE, matteTexture, program, restMatte } from "./gpu";
import { wholeMatte } from "./matte";
import { EYE_PX } from "./look";
import { artBox, cutLayout, lightRig, newLightRig, planCut, type PlannedLayer } from "./plan";
import { CFS, CVS, FS, KIND, VS } from "./shaders";

export interface CutRenderer extends GlowRenderer {
  /** Wait for the GPU to finish the frame (the harness's timing only). */
  finish(): void;
}

interface DrawLayer {
  plan: PlannedLayer;
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
/** Gaps in the solids narrower than about twice this (matte px, 4 art px each) are sealed. */
const MATTE_CLOSE = 6;


/**
 * Make a cut-out renderer for `form` on `canvas`. Rejects when WebGL2 is not there. `onRestored`
 * is called after a lost context came back and was rebuilt, so the caller can draw again.
 */
export async function createCutRenderer(canvas: HTMLCanvasElement, form: LoadedForm, onRestored: () => void = () => {}): Promise<CutRenderer> {
  const { spec, images } = form;
  const R = spec.rig;
  const colors = rigColors(R);
  const plan = planCut(spec);
  const upperLip = spec.layers.some((l) => l.kind === "aperture_upper");
  const W = spec.width;
  const H = spec.height;
  const first = canvas.getContext("webgl2", { ...CONTEXT_ATTRIBUTES, antialias: false });
  if (!first) throw new Error("WebGL2 unavailable");
  const gl: WebGL2RenderingContext = first;
  const union = restMatte(plan.matte, images, W, H);
  const matteBytes = union && wholeMatte(union, MATTE, MATTE_CLOSE);

  let lost = false;
  let disposed = false;
  let prog: WebGLProgram | null = null;
  let comp: WebGLProgram | null = null;
  let U: Record<string, WebGLUniformLocation | null> = {};
  let C: Record<string, WebGLUniformLocation | null> = {};
  let draws: DrawLayer[] = [];
  let matte: WebGLTexture | null = null;
  let fig: WebGLTexture | null = null;
  let fbo: WebGLFramebuffer | null = null;
  const view = new Float32Array(4);
  let box = artBox(cutLayout(1, 1, W, H), W, H);
  const rig = newLightRig();

  const fresh = () => ({ textures: [] as WebGLTexture[], buffers: [] as WebGLBuffer[], vaos: [] as WebGLVertexArrayObject[], shaders: [] as WebGLShader[] });
  let owned = fresh();
  function free() {
    for (const t of owned.textures) gl.deleteTexture(t);
    for (const b of owned.buffers) gl.deleteBuffer(b);
    for (const v of owned.vaos) gl.deleteVertexArray(v);
    for (const s of owned.shaders) gl.deleteShader(s);
    if (prog) gl.deleteProgram(prog);
    if (comp) gl.deleteProgram(comp);
    if (fig) gl.deleteTexture(fig);
    if (fbo) gl.deleteFramebuffer(fbo);
    owned = fresh();
    fig = null;
    fbo = null;
    draws = [];
  }

  const loc = (name: string) => {
    if (!(name in U)) U[name] = prog ? gl.getUniformLocation(prog, name) : null;
    return U[name] ?? null;
  };
  const cloc = (name: string) => {
    if (!(name in C)) C[name] = comp ? gl.getUniformLocation(comp, name) : null;
    return C[name] ?? null;
  };

  async function build() {
    prog = await program(gl, VS, FS, owned.shaders);
    comp = await program(gl, CVS, CFS, owned.shaders);
    U = {};
    C = {};
    gl.useProgram(prog);
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

    // the rest matte (empty when there was no 2D canvas to make it with: the backplate is then
    // dropped below, never drawn as a square)
    matte = matteTexture(gl, matteBytes ?? new Uint8Array(MATTE * MATTE));
    if (matte) owned.textures.push(matte);

    const ap = R.aperture;
    const all: PlannedLayer[] = plan.backplate && matteBytes ? [{ spec: plan.backplate, depth: 0, follow: 0, recorded: true }, ...plan.layers] : plan.layers;
    draws = [];
    let fi = 0;
    for (const pl of all) {
      const l = pl.spec;
      let bb: [number, number, number, number] = l.bbox ?? [0, 0, W, H];
      if (l.kind === "aperture_glow") {
        if (!ap) continue;
        bb = [ap.x0 - 10, ap.top - 20, ap.x1 - ap.x0 + 20, ap.bottom - ap.top + 40];
      }
      if (l.file && !textures.has(l.file)) continue;
      if (!l.file && l.kind !== "aperture_glow") continue;
      // the backplate and the body are warped too now (sway, the body's turn): give them a grid
      const step = l.kind === "static" ? 32 : l.kind.startsWith("aperture") ? 2 : 8;
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
      const d: DrawLayer = { plan: pl, kind: KIND[l.kind] ?? 2, bbox: bb, tex: l.file ? (textures.get(l.file) ?? null) : null, vao, count: idx.length };
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
    gl.uniform1i(loc("u_tex"), 0);
    gl.uniform1i(loc("u_matte"), 1);
    gl.useProgram(comp);
    gl.uniform1i(cloc("u_fig"), 2);
    layout();
  }

  /** The figure's texture and framebuffer, at the canvas's size. */
  function target() {
    if (fig && fbo) return;
    ({ tex: fig, fbo } = figureTarget(gl, canvas.width, canvas.height));
  }

  function layout() {
    const cw = canvas.width || 1;
    const ch = canvas.height || 1;
    const v = cutLayout(cw, ch, W, H);
    view.set(v);
    box = artBox(v, W, H);
    if (prog && canvas.width > 0) {
      // a texStorage texture cannot change size: drop it, `target` remakes it
      if (fig) gl.deleteTexture(fig);
      if (fbo) gl.deleteFramebuffer(fbo);
      fig = null;
      fbo = null;
    }
  }

  const onLost = (e: Event) => {
    e.preventDefault();
    lost = true;
  };
  const onRestore = () => {
    if (disposed) return;
    owned = fresh();
    prog = null;
    comp = null;
    fig = null;
    fbo = null;
    build().then(
      () => {
        if (disposed) return;
        lost = false;
        onRestored();
      },
      (error: unknown) => console.warn(`[stage] the face could not be rebuilt after the GPU reset: ${error instanceof Error ? error.message : String(error)}`),
    );
  };
  canvas.addEventListener("webglcontextlost", onLost);
  canvas.addEventListener("webglcontextrestored", onRestore);

  await build();

  function layers(pose: Pose) {
    const t = pose.t;
    const act = pose.activity;
    const voice = pose.voice;
    const head = R.head;
    const ap = R.aperture;
    gl.useProgram(prog);
    gl.uniform4fv(loc("u_view"), view);
    gl.uniform2f(loc("u_headC"), head.center[0], head.center[1]);
    gl.uniform2f(loc("u_headR"), head.radius[0], head.radius[1]);
    gl.uniform1f(loc("u_pivotY"), head.pivot_y ?? head.chin_y);
    gl.uniform1f(loc("u_imgH"), H);
    gl.uniform1f(loc("u_imgW"), W);
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
      const ang = -0.6;
      const span = W * 1.4;
      const at = (((t * 0.045 + pose.yaw * 0.02) % 1) + 1) % 1;
      gl.uniform4f(loc("u_glintP"), Math.cos(ang), Math.sin(ang), at * span - span * 0.2, gp.width_px);
      gl.uniform1f(loc("u_glintGain"), gp.gain);
    }
    const lift = ap ? pose.open * (ap.open_frac ?? 0.15) * (ap.bottom - ap.top) * (0.75 + 0.35 * Math.min(1, voice)) : 0;
    if (ap) gl.uniform2f(loc("u_aperX"), ap.x0, ap.x1);
    gl.uniform2f(loc("u_size"), W, H);
    gl.uniform2f(loc("u_fade"), plan.fade[0], plan.fade[1]);
    gl.uniform1i(loc("u_pass"), 0);
    gl.uniform3fv(loc("u_mouthCol"), colors.mouth);
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, matte);
    gl.activeTexture(gl.TEXTURE0);
    gl.blendFunc(gl.ONE, gl.ONE);

    for (const d of draws) {
      const l = d.plan.spec;
      const kind = d.kind;
      if (kind === 8 && lift < 0.2) continue;
      if (ap && kind >= 6 && kind <= 8) {
        const lit = kind === 8;
        gl.uniform4f(loc("u_aper"), ap.top, lit ? gapCentre(ap.seam_y, lift, upperLip) : ap.seam_y, ap.bottom, lit ? gapHalf(lift, upperLip) : lift);
      }
      gl.uniform1i(loc("u_kind"), kind);
      gl.uniform1f(loc("u_depth"), d.plan.depth);
      gl.uniform1f(loc("u_follow"), d.plan.follow);
      gl.uniform1f(loc("u_erode"), kind === 3 ? 1 : 0);
      const isLight = kind === 0 || l.mode === "light" || kind === 8 || kind === 9;
      gl.uniform1f(loc("u_occl"), isLight ? 0 : (l.occl ?? 1));
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
        gl.uniform2f(loc("u_gaze"), pose.gazeX * EYE_PX[0], pose.gazeY * EYE_PX[1]);
        gain *= 1 + (l.voice_gain ?? 0.12) * voice;
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
      if (d.tex) {
        gl.bindTexture(gl.TEXTURE_2D, d.tex);
        gl.uniform4f(loc("u_bbox"), d.bbox[0], d.bbox[1], d.bbox[2], d.bbox[3]);
      }
      gl.bindVertexArray(d.vao);
      if (!isLight) {
        // a solid takes out what lies behind it by its coverage, then adds its own light
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

  function render(pose: Pose) {
    if (lost || disposed || gl.isContextLost() || !prog || !comp) return;
    target();
    gl.enable(gl.BLEND);
    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    layers(pose);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);

    gl.activeTexture(gl.TEXTURE2);
    gl.bindTexture(gl.TEXTURE_2D, fig);
    gl.generateMipmap(gl.TEXTURE_2D);
    gl.activeTexture(gl.TEXTURE0);

    gl.disable(gl.BLEND);
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.useProgram(comp);
    const px = Math.max(1, Math.min(canvas.width, canvas.height));
    gl.uniform2f(cloc("u_px"), canvas.width, canvas.height);
    gl.uniform1f(cloc("u_rimW"), Math.max(1.5, px * 0.006));
    lightRig(rig, pose.yaw, pose.voice, colors.mouth, colors.lineCore, box);
    gl.uniform3fv(cloc("u_rimR"), rig.rimR);
    gl.uniform3fv(cloc("u_rimL"), rig.rimL);
    gl.uniform4fv(cloc("u_shadow"), rig.shadow);
    gl.uniform3fv(cloc("u_pool"), rig.pool);
    gl.uniform4fv(cloc("u_box"), box);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    gl.activeTexture(gl.TEXTURE2);
    gl.bindTexture(gl.TEXTURE_2D, null);
    gl.activeTexture(gl.TEXTURE0);
  }

  return {
    render,
    resize: layout,
    finish: () => gl.finish(),
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
