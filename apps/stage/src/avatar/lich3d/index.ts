// The lich3d face (spark avatar-3d prototype B): Malakor's head as a true 3D skull in three.js.
// The skull is the rig's `lich.glb` (WP-B1) or, until there is one, a procedural placeholder in the
// same faceted style; the jaw opens on its hinge, eye fire burns in the sockets and lights the bone,
// the monocle glows, a crown of procedural crystals floats about the head, the painted torso stands
// behind it. Everything three.js is in this folder, so it lands in this variant's lazy chunk.
//
// Same seam as the other faces (`startFace`, variants.ts): `data-face` loading | ready |
// unavailable, `data-face-loop` running | still, `data-form` rig (the GLB) | placeholder. It reads
// the driver's pose, the look (eyes lead, head follows: `../depth/look`) and the performance cue,
// and the loop sleeps once all three have settled. With no WebGL2, or when three.js cannot start,
// it hands the canvas to the glow face, as the depth face does.
//
// Dev toggles in the page URL: `?lichtex=0` draws the flat bone instead of the GLB's texture,
// `?lichbloom=0` draws without bloom.

import type { FrameProbe } from "../cut";
import { createLookFollower, type LookFollower, type LookPose } from "../depth/look";
import { createDriver, type Driver, type Pose } from "../driver";
import { frames } from "../motion";
import { createPerformanceDriver, PERFORMANCE_REST, type PerformanceDriver, type PerformancePose, type PerformanceSource } from "../performance";
import { calmNow, createFaceLoop, onLookChange, startFace as startGlow, type FaceHandle, type FaceLoop } from "../runtime";
import type { Look, StartFaceOptions } from "../variants";
import { loadBackdrop, loadLichAsset } from "./asset";
import { emptyFrame, lichFrame } from "./channels";
import { chooseRenderer } from "./contract";
import { createLichRenderer, type LichRenderer, type LichRendererOptions } from "./renderer";

export type { FrameProbe };
export type LichFaceOptions = StartFaceOptions & {
  probe?: FrameProbe;
  /** Overrides the `?lichbloom=` / `?lichtex=` toggles (the bench). */
  bloom?: boolean;
  texture?: boolean;
};

const AHEAD: Look = Object.freeze({ x: 0, y: 0 });

/**
 * The face driver with the look and the performance on top: it steps all three together and is
 * settled only when all three are, so the loop runs while the head turns or a gesture plays.
 */
export function lichDriver(
  inner: Driver,
  follower: LookFollower,
  look: () => Look,
  perf: PerformanceDriver,
  performance?: PerformanceSource,
): Driver & { readonly look: LookPose; readonly perf: PerformancePose } {
  let perfPose: PerformancePose = PERFORMANCE_REST;
  return {
    setSignal: (s) => inner.setSignal(s),
    step(dt) {
      follower.step(dt, look());
      if (performance) perf.setCue(performance.get());
      perfPose = perf.step(dt);
      return inner.step(dt);
    },
    get pose() {
      return inner.pose;
    },
    get settled() {
      return inner.settled && follower.settledAt(look()) && perf.settled;
    },
    rest() {
      follower.snap(look());
      perfPose = PERFORMANCE_REST;
      return inner.rest();
    },
    get look() {
      return follower.pose;
    },
    get perf() {
      return perfPose;
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

/** A `?name=0` toggle in the page URL (on unless it says 0 or false). */
function toggle(name: string): boolean {
  if (typeof location === "undefined") return true;
  const v = new URLSearchParams(location.search).get(name);
  return v === null || (v !== "0" && v !== "false");
}

export function startFace(opts: LichFaceOptions): FaceHandle {
  const { root, canvas, source } = opts;
  const pick = chooseRenderer({ webgl2: typeof WebGL2RenderingContext !== "undefined" });
  if (pick.use === "glow") {
    say(`glow renderer (${pick.why})`);
    return startGlow(opts);
  }

  let disposed = false;
  let size = opts.size;
  let delegate: FaceHandle | null = null;
  let renderer: LichRenderer | null = null;
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
  const perf = createPerformanceDriver();
  const driver = lichDriver(createDriver({ seed: 7 }), createLookFollower(), lookNow, perf, opts.performance);
  let jawOpen = 0.3;
  const frame = emptyFrame();
  let last: Pose = driver.rest();
  const paint = () => renderer?.render(lichFrame(last, driver.perf, driver.look, jawOpen, frame));
  const probe = opts.probe;
  const draw = (pose: Pose) => {
    last = pose;
    if (!renderer) return;
    if (!probe) {
      paint();
      return;
    }
    probe.start?.();
    const t0 = performance.now();
    paint();
    renderer.finish();
    probe.end(performance.now() - t0);
  };

  const rendererOptions: LichRendererOptions = { bloom: opts.bloom ?? toggle("lichbloom"), texture: opts.texture ?? toggle("lichtex") };

  Promise.all([loadLichAsset(opts.rigBase), loadBackdrop(opts.rigBase)])
    .then(([asset, backdrop]) => {
      if (disposed) {
        backdrop?.texture.dispose();
        return;
      }
      let made: LichRenderer;
      try {
        made = createLichRenderer(canvas, asset, { ...rendererOptions, backdrop });
      } catch (error) {
        backdrop?.texture.dispose();
        const fallback = chooseRenderer({ webgl2: true, rendererError: error instanceof Error ? error.message : String(error) });
        glow(fallback.use === "glow" ? fallback.why : "the 3D head did not start");
        return;
      }
      renderer = made;
      jawOpen = asset.meta.jaw_open_rad;
      say(asset.kind === "glb" ? `lich3d (${asset.meta.source_model}, ${asset.meta.tris} tris)` : `lich3d (placeholder skull: ${asset.why ?? "no GLB"})`);
      const form = asset.kind === "glb" ? "rig" : "placeholder";
      root.setAttribute("data-form", form);
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
      opts.onDrawn?.(form);

      cleanups.push(source.subscribe((signal) => driver.setSignal(signal) && l.wake()));
      // a still face runs no loop: a change of the look or the cue wakes it, read per frame
      cleanups.push(onLookChange(opts.look, () => l.wake()));
      cleanups.push(opts.performance?.subscribe?.(() => l.wake()) ?? (() => {}));
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
    })
    .catch((error: unknown) => glow(`the head did not load: ${error instanceof Error ? error.message : String(error)}`));

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
        paint();
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
