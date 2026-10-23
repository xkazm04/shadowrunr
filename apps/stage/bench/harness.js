// The face bench's page: one face at a time through its real `startFace` (variants.ts), fed the
// bench script (`benchSources`, demo.ts: speech, every emotion and gesture, the look changes), so
// every face receives exactly the same inputs. Node drives it through `window.__bench`
// (frames.mjs, clips.mjs, sheet.mjs); `?auto=1` starts one face on the page's own frame clock at
// load (ready.mjs, process.mjs, voice.mjs, which run on a `vite build` of this page).
//
// Two clocks. `manual` is a Clock (motion.ts) the bench steps itself: the face and the script run
// on the same stepped time, so a frame-cost run and a clip are the same frames on every face. Without
// it the face and the script run on `frames()`, the page's real requestAnimationFrame clock, as the
// stage does.
//
// Plain JS on purpose: it sits outside the stage's TypeScript project and its lint, and Vite
// imports the stage's own modules from it untouched. The sheet's renderers are imported lazily
// (`./sheet.js`) so a ready-time run loads only the face it times, as the stage would.

import { benchSources } from "../src/avatar/demo.ts";
import { loadVariant } from "../src/avatar/variants.ts";

const BG = "#060807";
const DRAWS = ["drawArrays", "drawElements", "drawArraysInstanced", "drawElementsInstanced", "drawRangeElements"];

// what the faces say about themselves (`[stage] face: ...`): a fallback to the glow face shows here
const said = [];
for (const level of ["info", "warn"]) {
  const orig = console[level].bind(console);
  console[level] = (...args) => {
    said.push(args.map(String).join(" "));
    orig(...args);
  };
}

/** A Clock (motion.ts) the bench steps: `step(ms)` is one frame, `ms` after the last. */
export function manualClock() {
  const subs = new Map();
  let now = 0;
  return {
    clock: {
      subscribe(fn) {
        subs.set(fn, -1);
        return () => void subs.delete(fn);
      },
      get size() {
        return subs.size;
      },
      get running() {
        return subs.size > 0;
      },
    },
    /** One frame: every subscriber hears the time and its delta (0 on its first frame, as motion.ts). */
    step(ms) {
      now += ms;
      for (const [fn, last] of [...subs]) {
        if (!subs.has(fn)) continue;
        const keep = fn(now, last < 0 ? 0 : now - last);
        if (keep === false) subs.delete(fn);
        else subs.set(fn, now);
      }
    },
    get now() {
      return now;
    },
  };
}

const raf = () => new Promise((resolve) => requestAnimationFrame(resolve));
const idle = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** Resolve when `root`'s data-face leaves loading, with performance.now() at that moment. */
function settledFace(root, timeoutMs = 60_000) {
  return new Promise((resolve, reject) => {
    const check = () => {
      const s = root.getAttribute("data-face");
      if (s && s !== "loading") {
        mo.disconnect();
        clearTimeout(timer);
        resolve({ state: s, at: performance.now() });
      }
    };
    const mo = new MutationObserver(check);
    mo.observe(root, { attributes: true, attributeFilter: ["data-face"] });
    const timer = setTimeout(() => {
      mo.disconnect();
      reject(new Error(`the face did not leave loading in ${timeoutMs} ms`));
    }, timeoutMs);
    check();
  });
}

let cur = null;

/**
 * Start `face` at `size` CSS px (device scale 1: the backing store is `size` px) on the rig at
 * `rig`. `manual`: the bench's stepped clock, else the page's frames(). `bloom`: lich3d only.
 */
async function start({ face, size, rig, bloom, manual = true }) {
  if (cur) stop();
  const stand = document.getElementById("stand");
  const root = document.createElement("div");
  root.style.width = root.style.height = `${size}px`;
  const canvas = document.createElement("canvas");
  canvas.style.width = canvas.style.height = `${size}px`;
  canvas.style.display = "block";
  root.appendChild(canvas);
  stand.replaceChildren(root);

  const mc = manual ? manualClock() : null;
  const sources = benchSources(mc ? mc.clock : undefined);
  // `lich3d-real` is the lich3d face in its real look, `lich3d` / `lich3d-paint` in its painted one
  const variant = face.startsWith("lich3d") ? "lich3d" : face;
  const startFace = await loadVariant(variant);
  const opts = { root, canvas, rigBase: rig, source: sources.face, size, look: sources.look, performance: sources.performance };
  if (mc) opts.clock = mc.clock;
  if (variant === "lich3d") {
    if (bloom !== undefined) opts.bloom = bloom;
    opts.lichLook = face === "lich3d-real" ? "real" : "paint";
  }
  const saidBefore = said.length;
  const t0 = performance.now();
  const handle = startFace(opts);
  const ready = await settledFace(root);
  // the depth family hands over to the glow face: it says so, then the glow face loads
  const msgs = () => said.slice(saidBefore).filter((m) => m.includes("[stage]"));
  if (ready.state === "ready" && msgs().some((m) => m.includes("glow renderer"))) await idle(0);

  // count the draw calls per frame on the face's own context (instance properties shadow the prototype)
  const gl = canvas.getContext("webgl2");
  const counter = { draws: 0 };
  if (gl) {
    for (const name of DRAWS) {
      const f = gl[name];
      if (typeof f !== "function") continue;
      gl[name] = (...a) => {
        counter.draws++;
        return f.apply(gl, a);
      };
    }
  }
  cur = { face, size, root, canvas, handle, mc, gl, counter, cache: null };
  return {
    face,
    state: ready.state,
    form: root.getAttribute("data-form"),
    px: canvas.width,
    said: msgs(),
    fellBack: msgs().some((m) => m.includes("glow renderer")),
    readyFromNavMs: ready.at,
    readyFromStartMs: ready.at - t0,
    webgl2: !!gl,
    // false: performance.now() is coarsened to 100 us (the server did not isolate the page)
    isolated: globalThis.crossOriginIsolated === true,
  };
}

function stop() {
  if (!cur) return;
  cur.handle.dispose();
  cur.root.remove();
  cur = null;
}

/** The GPU behind WebGL here (the unmasked renderer string). */
function gpuName() {
  const gl = document.createElement("canvas").getContext("webgl2");
  const dbg = gl?.getExtension("WEBGL_debug_renderer_info");
  return dbg ? gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) : null;
}

/**
 * Frame cost on the manual clock, paced by requestAnimationFrame: `warm` frames thrown away, then
 * `ticks` frames of 1/60 s (1440 = the whole 24 s script). Each frame is one step of the clock (the
 * script, the drivers and the draw) bracketed by an EXT_disjoint_timer_query_webgl2 TIME_ELAPSED
 * query: `gpu` is the GPU time of everything the face submitted that frame, `cpu` the main-thread
 * ms of the step (drivers + WebGL submit), `draws` its draw calls (0: the face slept that frame).
 */
async function frameCost({ ticks = 1440, warm = 60 } = {}) {
  const { gl, mc, counter } = cur;
  if (!gl) return { unavailable: "the face has no WebGL2 context on its canvas" };
  const ext = gl.getExtension("EXT_disjoint_timer_query_webgl2");
  if (!ext) return { unavailable: "EXT_disjoint_timer_query_webgl2 is not exposed" };
  const rows = [];
  const pending = [];
  let disjoint = 0;
  const harvest = () => {
    while (pending.length && gl.getQueryParameter(pending[0].q, gl.QUERY_RESULT_AVAILABLE)) {
      const p = pending.shift();
      const ns = gl.getQueryParameter(p.q, gl.QUERY_RESULT);
      gl.deleteQuery(p.q);
      if (gl.getParameter(ext.GPU_DISJOINT_EXT)) {
        disjoint++;
        continue;
      }
      if (p.i >= warm) rows.push({ i: p.i - warm, gpu: ns / 1e6, cpu: p.cpu, draws: p.draws });
    }
  };
  let wall0 = 0;
  const heap0 = performance.memory?.usedJSHeapSize;
  for (let i = 0; i < warm + ticks; i++) {
    await raf();
    harvest();
    if (i === warm) wall0 = performance.now();
    const q = gl.createQuery();
    gl.beginQuery(ext.TIME_ELAPSED_EXT, q);
    counter.draws = 0;
    const t0 = performance.now();
    mc.step(1000 / 60);
    const cpu = performance.now() - t0;
    gl.endQuery(ext.TIME_ELAPSED_EXT);
    pending.push({ q, i, cpu, draws: counter.draws });
  }
  const wallMs = performance.now() - wall0;
  for (let k = 0; k < 100 && pending.length; k++) {
    await idle(20);
    harvest();
  }
  const heap1 = performance.memory?.usedJSHeapSize;
  return {
    rows,
    lost: pending.length,
    disjoint,
    wallMs,
    ticks,
    form: cur.root.getAttribute("data-form"),
    heapUsedMB: heap1 === undefined ? undefined : heap1 / 1e6,
    heapGrowthMB: heap1 === undefined || heap0 === undefined ? undefined : (heap1 - heap0) / 1e6,
  };
}

/**
 * One recorded frame: step the manual clock `steps` times by `dt` ms, then the face's canvas over
 * the stage's dark, with `letter` burned in the corner, as a JPEG data URL. A frame on which the
 * face slept (no draw) repeats its last picture, which is what the screen shows.
 */
function grab({ steps = 1, dt = 1000 / 30, letter = "", quality = 0.95 } = {}) {
  const { canvas, mc, counter } = cur;
  let drew = false;
  for (let s = 0; s < steps; s++) {
    counter.draws = 0;
    mc.step(dt);
    if (counter.draws > 0) drew = true;
    // a WebGL canvas without preserveDrawingBuffer reads back only in the task that drew it
    if (counter.draws > 0 || !cur.cache) {
      cur.cache ??= Object.assign(document.createElement("canvas"), { width: canvas.width, height: canvas.height });
      const c = cur.cache.getContext("2d");
      c.clearRect(0, 0, canvas.width, canvas.height);
      c.drawImage(canvas, 0, 0);
    }
  }
  const out = (cur.out ??= Object.assign(document.createElement("canvas"), { width: canvas.width, height: canvas.height }));
  const c = out.getContext("2d");
  c.fillStyle = BG;
  c.fillRect(0, 0, out.width, out.height);
  c.drawImage(cur.cache, 0, 0);
  if (letter) {
    const h = Math.round(out.height * 0.09);
    c.font = `700 ${h}px sans-serif`;
    c.textBaseline = "top";
    c.lineWidth = Math.max(2, h * 0.08);
    c.strokeStyle = "#000";
    c.fillStyle = "#f2efe6";
    c.strokeText(letter, h * 0.4, h * 0.3);
    c.fillText(letter, h * 0.4, h * 0.3);
  }
  return { drew, url: out.toDataURL("image/jpeg", quality) };
}

/** Step the manual clock `steps` frames of `dt` ms without recording (to reach a clip's window). */
function advance({ steps, dt = 1000 / 30 }) {
  for (let s = 0; s < steps; s++) cur.mc.step(dt);
}

async function sheet(opts) {
  const m = await import("./sheet.js");
  return m.sheet(opts);
}

window.__bench = { start, stop, frameCost, grab, advance, sheet, gpuName, said };

// `?auto=1&face=<face>&size=<px>&rig=<url>[&bloom=0]`: start at load on the page's own clock
const q = new URLSearchParams(location.search);
if (q.get("auto") === "1") {
  const bloom = q.get("bloom");
  window.__benchReady = start({
    face: q.get("face") ?? "cut",
    size: Number(q.get("size") ?? 440),
    rig: q.get("rig"),
    manual: false,
    ...(bloom === null ? {} : { bloom: bloom !== "0" }),
  }).then(
    (r) => ({ ...r, gpu: gpuName() }),
    (e) => ({ error: String(e) }),
  );
}
