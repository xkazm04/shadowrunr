// The cut-out renderer's WebGL plumbing that holds no state: the program builder, the rest
// matte (drawn with the 2D canvas, then its texture) and the figure's render target.

import type { LayerImage, LayerSpec } from "../layers";

/** The rest matte's size, px a side. */
export const MATTE = 256;

export const breathe = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

/**
 * The figure's rest matte: the union of the solids' coverage, `MATTE` px a side, as one byte a
 * pixel. Drawn with the 2D canvas, so it needs a document (or OffscreenCanvas); null without one.
 */
export function restMatte(layers: LayerSpec[], images: Map<string, LayerImage>, W: number, H: number): Uint8Array | null {
  const make = (): OffscreenCanvas | HTMLCanvasElement | null => {
    if (typeof OffscreenCanvas !== "undefined") return new OffscreenCanvas(MATTE, MATTE);
    if (typeof document !== "undefined") {
      const c = document.createElement("canvas");
      c.width = MATTE;
      c.height = MATTE;
      return c;
    }
    return null;
  };
  const canvas = make();
  const ctx = canvas?.getContext("2d") as OffscreenCanvasRenderingContext2D | CanvasRenderingContext2D | null | undefined;
  if (!ctx) return null;
  const sx = MATTE / W;
  const sy = MATTE / H;
  for (const l of layers) {
    const img = l.file ? images.get(l.file) : undefined;
    if (!img) continue;
    const b = l.bbox ?? [0, 0, W, H];
    ctx.drawImage(img, b[0] * sx, b[1] * sy, b[2] * sx, b[3] * sy);
  }
  const data = ctx.getImageData(0, 0, MATTE, MATTE).data;
  const out = new Uint8Array(MATTE * MATTE);
  for (let i = 0; i < out.length; i++) out[i] = data[i * 4 + 3] ?? 0;
  return out;
}

/** Build the program from two sources, compiling in parallel where the driver offers it. */
export async function program(gl: WebGL2RenderingContext, vsSrc: string, fsSrc: string, shaders: WebGLShader[]): Promise<WebGLProgram> {
  const p = gl.createProgram();
  const vs = gl.createShader(gl.VERTEX_SHADER);
  const fs = gl.createShader(gl.FRAGMENT_SHADER);
  if (!p || !vs || !fs) throw new Error("WebGL2 could not make a program");
  shaders.push(vs, fs);
  gl.shaderSource(vs, vsSrc);
  gl.shaderSource(fs, fsSrc);
  gl.compileShader(vs);
  gl.compileShader(fs);
  gl.attachShader(p, vs);
  gl.attachShader(p, fs);
  gl.linkProgram(p);
  const parallel = gl.getExtension("KHR_parallel_shader_compile") as { COMPLETION_STATUS_KHR: number } | null;
  if (parallel) {
    for (let i = 0; i < 400 && !gl.getProgramParameter(p, parallel.COMPLETION_STATUS_KHR); i++) await breathe();
  }
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) {
    throw new Error(gl.getShaderInfoLog(vs) || gl.getShaderInfoLog(fs) || gl.getProgramInfoLog(p) || "program did not link");
  }
  return p;
}

/** The figure's texture (with room for its mips) and the framebuffer that draws into it. */
export function figureTarget(gl: WebGL2RenderingContext, w: number, h: number): { tex: WebGLTexture | null; fbo: WebGLFramebuffer | null } {
  const tex = gl.createTexture();
  const fbo = gl.createFramebuffer();
  gl.activeTexture(gl.TEXTURE2);
  gl.bindTexture(gl.TEXTURE_2D, tex);
  gl.texStorage2D(gl.TEXTURE_2D, 1 + Math.floor(Math.log2(Math.max(w, h, 1))), gl.RGBA8, w, h);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  gl.bindTexture(gl.TEXTURE_2D, null);
  gl.activeTexture(gl.TEXTURE0);
  gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
  gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  return { tex, fbo };
}

/** The rest matte as a one-channel texture, `MATTE` px a side, filtered. */
export function matteTexture(gl: WebGL2RenderingContext, bytes: Uint8Array): WebGLTexture | null {
  const t = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, t);
  gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, MATTE, MATTE, 0, gl.RED, gl.UNSIGNED_BYTE, bytes);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  return t;
}
