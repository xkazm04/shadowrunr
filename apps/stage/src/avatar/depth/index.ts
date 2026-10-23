// The depth-mesh face (streamer-screen prototype B): the rig's layers on meshes displaced by a
// depth map (`depth.png` + `depth.json` beside `layers.json`, made offline by
// `tools/avatar/depth_map.py`), turned under a perspective camera toward where he looks.
//
// Same seam as the glow face (`startFace`, variants.ts). It draws the depth mesh when it can and
// otherwise hands the canvas to the glow face, saying once in the console which one is drawn:
// with no WebGL2 (the glow face then shows the silhouette, as it does), with no rig (the
// placeholder has no depth), or with no usable depth map.
//
// A `performance` cue plays on top of the look (`withPerformance`, ../performance.ts) and wakes the
// loop as a look change does; the glow face it hands over to does the same.

import { createDriver, type Driver, type Pose } from "../driver";
import { loadForm, type LoadedForm } from "../layers";
import { frames } from "../motion";
import { onPerformanceChange, withPerformance } from "../performance";
import { buildPlaceholder } from "../placeholder";
import { calmNow, createFaceLoop, onLookChange, startFace as startGlow, type FaceHandle, type FaceLoop } from "../runtime";
import type { Look, StartFaceOptions } from "../variants";
import { checkDepthMeta, depthField, type DepthField, type DepthMeta } from "./geom";
import { createLookFollower, type LookFollower, type LookPose } from "./look";
import { decodeGrayPng } from "./png16";
import { createDepthRenderer, type DepthRenderer } from "./renderer";

const AHEAD: Look = Object.freeze({ x: 0, y: 0 });

/** Fetch and decode the depth map beside the rig at `base`. Rejects with the reason. */
export async function fetchDepth(base: string, width: number, height: number, get: (url: string) => Promise<Response> = (u) => fetch(u)): Promise<{ meta: DepthMeta; field: DepthField }> {
  const [mr, pr] = await Promise.all([get(`${base}depth.json`), get(`${base}depth.png`)]);
  if (!mr.ok) throw new Error(`depth.json: HTTP ${mr.status}`);
  if (!pr.ok) throw new Error(`depth.png: HTTP ${pr.status}`);
  const meta = checkDepthMeta(await mr.json(), width, height);
  const png = await decodeGrayPng(new Uint8Array(await pr.arrayBuffer()));
  if (png.width !== width || png.height !== height) throw new Error(`depth.png is ${png.width}x${png.height}, the rig ${width}x${height}`);
  return { meta, field: depthField(png, meta) };
}

/**
 * The face driver with the look on top: it steps the glow driver's pose and the look follower
 * together, and is settled only when both are, so the face loop runs while the head is turning.
 */
export function lookDriver(inner: Driver, follower: LookFollower, look: () => Look): Driver & { readonly look: LookPose } {
  return {
    setSignal: (s) => inner.setSignal(s),
    step(dt) {
      follower.step(dt, look());
      return inner.step(dt);
    },
    get pose() {
      return inner.pose;
    },
    get settled() {
      return inner.settled && follower.settledAt(look());
    },
    rest() {
      follower.snap(look());
      return inner.rest();
    },
    get look() {
      return follower.pose;
    },
  };
}

let told = false;
function say(what: string) {
  if (told) return;
  told = true;
  console.info(`[stage] face: ${what}`);
}

/** Pixels behind each CSS pixel: the screen's, at most 2, and never past 1024 (as the glow face). */
function backing(size: number): number {
  const dpr = typeof window !== "undefined" ? Math.min(2, window.devicePixelRatio || 1) : 1;
  return Math.max(1, Math.min(1024, Math.round(size * dpr)));
}

export function startFace(opts: StartFaceOptions): FaceHandle {
  const { root, canvas, source } = opts;
  if (typeof WebGL2RenderingContext === "undefined") {
    say("glow renderer (no WebGL2 for the depth mesh)");
    return startGlow(opts);
  }

  let disposed = false;
  let size = opts.size;
  let delegate: FaceHandle | null = null;
  let renderer: DepthRenderer | null = null;
  let loop: FaceLoop | null = null;
  const cleanups: (() => void)[] = [];
  root.setAttribute("data-face", "loading");

  const fit = (s: number) => {
    const px = backing(s);
    if (canvas.width !== px || canvas.height !== px) {
      canvas.width = px;
      canvas.height = px;
    }
  };
  fit(size);

  const glow = (why: string) => {
    if (disposed) return;
    say(`glow renderer (${why})`);
    delegate = startGlow({ ...opts, size });
  };

  const lookNow = () => opts.look?.get() ?? AHEAD;
  const driver = withPerformance(lookDriver(createDriver({ seed: 7 }), createLookFollower(), lookNow), opts.performance, { reduced: calmNow });
  let last: Pose = driver.rest();
  const draw = (pose: Pose) => {
    last = pose;
    renderer?.render(pose, driver.look);
  };

  const start = async (form: LoadedForm, rigBase: string) => {
    const { meta, field } = await fetchDepth(rigBase, form.spec.width, form.spec.height).catch((error: unknown) => {
      throw new Error(`no depth map: ${error instanceof Error ? error.message : String(error)}`);
    });
    if (disposed) return;
    const made = await createDepthRenderer(canvas, form, field, meta, () => renderer?.render(last, driver.look)).catch((error: unknown) => {
      throw new Error(`the depth mesh did not start: ${error instanceof Error ? error.message : String(error)}`);
    });
    if (disposed) {
      made.dispose();
      return;
    }
    renderer = made;
    say(`depth mesh (${meta.method})`);
    root.setAttribute("data-form", form.form);
    const l = createFaceLoop({
      driver,
      draw,
      clock: opts.clock ?? frames(),
      calm: calmNow(),
      onState: (s) => root.setAttribute("data-face-loop", s),
    });
    loop = l;
    draw(driver.rest());
    canvas.style.visibility = "visible";
    root.setAttribute("data-face", "ready");
    opts.onDrawn?.(form.form);

    cleanups.push(source.subscribe((signal) => driver.setSignal(signal) && l.wake()));
    // a still face runs no loop: a change of the look wakes it, and it reads the look per frame
    cleanups.push(onLookChange(opts.look, () => l.wake()));
    cleanups.push(onPerformanceChange(opts.performance, () => l.wake()));
    if (typeof IntersectionObserver !== "undefined") {
      const io = new IntersectionObserver((entries) => {
        const e = entries[entries.length - 1];
        if (e) l.setVisible(e.isIntersecting);
      });
      io.observe(canvas);
      cleanups.push(() => io.disconnect());
    }
    const recheck = () => l.setCalm(calmNow());
    const mo = new MutationObserver(recheck);
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-motion"] });
    cleanups.push(() => mo.disconnect());
    const mq = typeof window.matchMedia === "function" ? window.matchMedia("(prefers-reduced-motion: reduce)") : null;
    mq?.addEventListener("change", recheck);
    cleanups.push(() => mq?.removeEventListener("change", recheck));
    l.wake();
  };

  loadForm(opts.rigBase, buildPlaceholder).then(
    (form) => {
      if (disposed) return;
      if (form.form !== "rig" || !opts.rigBase) {
        glow("no rig, so no depth map");
        return;
      }
      start(form, opts.rigBase).catch((error: unknown) => {
        renderer?.dispose();
        renderer = null;
        glow(error instanceof Error ? error.message : String(error));
      });
    },
    (error: unknown) => glow(`the form did not load: ${error instanceof Error ? error.message : String(error)}`),
  );

  return {
    resize(s) {
      size = s;
      if (delegate) {
        delegate.resize(s);
        return;
      }
      fit(s);
      if (renderer) {
        renderer.resize();
        renderer.render(last, driver.look);
        loop?.wake();
      }
    },
    dispose() {
      disposed = true;
      delegate?.dispose();
      loop?.dispose();
      for (const c of cleanups.splice(0)) c();
      renderer?.dispose();
      renderer = null;
    },
  };
}
