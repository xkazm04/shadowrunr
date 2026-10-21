// The WebGL2 plumbing every face renderer (glow, cut, depth) shares, written once: the program
// build (compiled in parallel where the driver offers it), the upload of the rig's pictures, and
// the guard that waits out a lost context and rebuilds. No per-frame work lives here.

import type { LayerImage } from "./layers";

/** Hand the main thread back between pieces of setup, so no one piece is a long task. */
export const breathe = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

/**
 * Build a program from two sources, compiling in parallel where the driver offers it
 * (KHR_parallel_shader_compile: the main thread is handed back while it links). The shaders are
 * pushed onto `shaders` so the caller can free them. Rejects with the compile or link log.
 */
export async function compileProgram(gl: WebGL2RenderingContext, vsSrc: string, fsSrc: string, shaders: WebGLShader[]): Promise<WebGLProgram> {
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

/**
 * Upload the rig's pictures as mipmapped textures, one per file, handing the main thread back
 * after each. The pictures are straight alpha and taken as they are (no premultiply, no colour
 * conversion): the shaders do both. Each texture is also pushed onto `owned` for freeing.
 */
export async function uploadLayerTextures(gl: WebGL2RenderingContext, images: Map<string, LayerImage>, owned: WebGLTexture[]): Promise<Map<string, WebGLTexture>> {
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
      owned.push(t);
    }
    await breathe();
  }
  return textures;
}

export interface ContextLossGuard {
  /** True from the context's loss until it was restored and rebuilt. */
  readonly lost: boolean;
  /** Stop listening; a rebuild still in flight then calls nothing. */
  dispose(): void;
}

/**
 * Wait out a lost context: ask for it back, and when it returns run `rebuild` (which must drop the
 * dead handles, not delete them, and make everything again), then call `onRestored` so the caller
 * can draw. A rebuild that fails is warned about once, naming `what`, and the face stays dark.
 */
export function contextLossGuard(canvas: HTMLCanvasElement, rebuild: () => Promise<void>, onRestored: () => void, what = "the face"): ContextLossGuard {
  let lost = false;
  let disposed = false;
  const onLost = (e: Event) => {
    e.preventDefault(); // ask for it back
    lost = true;
  };
  const onRestore = () => {
    if (disposed) return;
    rebuild().then(
      () => {
        if (disposed) return;
        lost = false;
        onRestored();
      },
      (error: unknown) => console.warn(`[stage] ${what} could not be rebuilt after the GPU reset: ${error instanceof Error ? error.message : String(error)}`),
    );
  };
  canvas.addEventListener("webglcontextlost", onLost);
  canvas.addEventListener("webglcontextrestored", onRestore);
  return {
    get lost() {
      return lost;
    },
    dispose() {
      disposed = true;
      canvas.removeEventListener("webglcontextlost", onLost);
      canvas.removeEventListener("webglcontextrestored", onRestore);
    },
  };
}
