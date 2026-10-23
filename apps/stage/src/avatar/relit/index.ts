// The relit face (spark avatar-3d, prototype `relit`): the rig's layers as lit 2.5D cards. The
// depth face's geometry (`../depth`: meshes displaced by `depth.png`, turned under a perspective
// camera) with real light on it: normals from the depth field and each picture, a key light that
// changes as the head turns, the eye fire and the monocle's ring as point lights on the bone, a
// hinged jaw, a brow band, a wider pitch (22 x 14 degrees; geom.ts says why the yaw stops at 22) and the performance channels
// (`../performance`: eye light, flare, brow, gestures, lean). See renderer.ts and shaders.ts.
//
// Same seam as the glow face (`startFace`, variants.ts) and the same fallbacks as the depth face:
// with no WebGL2, no rig or no usable depth map it hands the canvas to the glow face, saying once
// in the console which one is drawn. A `FrameProbe` (as the cut face's) times each frame.

import type { FrameProbe } from "../cut";
import { createDriver, type Driver, type Pose } from "../driver";
import { fetchDepth } from "../depth";
import { createLookFollower, type LookFollower, type LookPose } from "../depth/look";
import { loadForm, type LoadedForm } from "../layers";
import { frames } from "../motion";
import { createPerformanceDriver, type PerformanceDriver, type PerformancePose } from "../performance";
import { buildPlaceholder } from "../placeholder";
import { calmNow, createFaceLoop, onLookChange, startFace as startGlow, type FaceHandle, type FaceLoop } from "../runtime";
import type { Look, StartFaceOptions } from "../variants";
import { createRelitRenderer, type RelitRenderer } from "./renderer";

const AHEAD: Look = Object.freeze({ x: 0, y: 0 });

export type RelitFaceOptions = StartFaceOptions & { probe?: FrameProbe };

/**
 * The face driver with the look and the performance on top: it steps all three together and is
 * settled only when all three are, so the loop runs while the head turns or a gesture plays.
 */
export function relitDriver(inner: Driver, follower: LookFollower, look: () => Look, perf: PerformanceDriver): Driver & { readonly look: LookPose; readonly perf: PerformancePose } {
  let current = perf.pose;
  return {
    setSignal: (s) => inner.setSignal(s),
    step(dt) {
      follower.step(dt, look());
      current = perf.step(dt);
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
      current = perf.pose;
      return inner.rest();
    },
    get look() {
      return follower.pose;
    },
    get perf() {
      return current;
    },
  };
}

/** What the dev hook may hold the face at: any part of the pose, the look and the performance. */
export interface RelitHold {
  pose?: Partial<Pose>;
  look?: Partial<LookPose>;
  perf?: Partial<PerformancePose>;
}

/** Dev builds only (`window.__relit`): hold the face at a pose for a screenshot, and size its buffer. */
export interface RelitDev {
  hold(over: RelitHold): void;
  release(): void;
  /** Set the drawing buffer to `px` square (the CSS size stays). */
  size(px: number): void;
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

export function startFace(opts: RelitFaceOptions): FaceHandle {
  const { root, canvas, source } = opts;
  if (typeof WebGL2RenderingContext === "undefined") {
    say("glow renderer (no WebGL2 for the relit face)");
    return startGlow(opts);
  }

  let disposed = false;
  let size = opts.size;
  let delegate: FaceHandle | null = null;
  let renderer: RelitRenderer | null = null;
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
  if (opts.performance) perf.setCue(opts.performance.get());
  const driver = relitDriver(createDriver({ seed: 7 }), createLookFollower(), lookNow, perf);
  const devProbe = import.meta.env.DEV && typeof window !== "undefined" ? (window as { __faceProbe?: FrameProbe }).__faceProbe : undefined;
  const probe = opts.probe ?? devProbe;
  let held: { pose: Pose; look: LookPose; perf: PerformancePose } | null = null;
  let last: Pose = driver.rest();
  const paint = (pose: Pose) => {
    if (!renderer) return;
    if (held) renderer.render(held.pose, held.look, held.perf);
    else renderer.render(pose, driver.look, driver.perf);
  };
  const draw = (pose: Pose) => {
    last = pose;
    if (!renderer) return;
    if (!probe) {
      paint(pose);
      return;
    }
    probe.start?.();
    const t0 = performance.now();
    paint(pose);
    renderer.finish();
    probe.end(performance.now() - t0);
  };

  const start = async (form: LoadedForm, rigBase: string) => {
    const { meta, field } = await fetchDepth(rigBase, form.spec.width, form.spec.height).catch((error: unknown) => {
      throw new Error(`no depth map: ${error instanceof Error ? error.message : String(error)}`);
    });
    if (disposed) return;
    const made = await createRelitRenderer(canvas, form, field, meta, () => paint(last)).catch((error: unknown) => {
      throw new Error(`the relit face did not start: ${error instanceof Error ? error.message : String(error)}`);
    });
    if (disposed) {
      made.dispose();
      return;
    }
    renderer = made;
    say(`relit cards (${meta.method} depth)`);
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
    cleanups.push(onLookChange(opts.look, () => l.wake()));
    const performanceSource = opts.performance;
    if (performanceSource?.subscribe) {
      cleanups.push(
        performanceSource.subscribe((cue) => {
          perf.setCue(cue);
          l.wake();
        }),
      );
    }
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
    if (import.meta.env.DEV) installDev();
    l.wake();
  };

  // dev builds only: a screenshot script holds the face at an explicit pose (the performance
  // channels have no producer on this branch yet) and sizes the buffer for the frame-cost runs
  const installDev = () => {
    const rest = driver.rest();
    const dev: RelitDev = {
      hold(over) {
        held = {
          pose: { ...rest, ...over.pose },
          look: { yaw: 0, pitch: 0, eyeX: 0, eyeY: 0, ...over.look },
          perf: { ...driver.perf, ...over.perf },
        };
        draw(held.pose);
      },
      release() {
        held = null;
        loop?.wake();
      },
      size(px) {
        canvas.width = px;
        canvas.height = px;
        renderer?.resize();
        draw(last);
      },
    };
    (window as { __relit?: RelitDev }).__relit = dev;
    cleanups.push(() => delete (window as { __relit?: RelitDev }).__relit);
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
        paint(last);
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
