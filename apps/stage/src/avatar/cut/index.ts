// The 2.5D cut-out face (streamer-screen prototype A): the M8 rig drawn as a streamer's cut-out
// that sits in a corner of the screen over the content. Same seam as the glow runtime
// (`../variants.ts`): the same options, the same `data-face` states and failures, plus `look`.
//
// What differs from the glow: no square backplate (only its dark, inside the figure, behind what a
// turn uncovers), a figure that fades out at its bottom, a stronger head turn with every head layer
// at its own depth in one shared field, a look the eyes lead and the head follows, rim lights in
// the rig's own colours and a soft shadow on the page behind and beneath him (renderer.ts).
//
// The loop is the glow runtime's (`createFaceLoop`): one frame clock, no frames once settled. A
// change of the `look` wakes it (`onLookChange`); nothing reads the look between frames.

import { createDriver, type Pose } from "../driver";
import { loadForm, type LoadedForm } from "../layers";
import { frames } from "../motion";
import { buildPlaceholder } from "../placeholder";
import { calmNow, createFaceLoop, onLookChange, type FaceHandle, type FaceLoop } from "../runtime";
import type { StartFaceOptions } from "../variants";
import { withLook } from "./look";
import { createCutRenderer, type CutRenderer } from "./renderer";

let warned = false;
function warnOnce(why: string) {
  if (warned) return;
  warned = true;
  console.warn(`[stage] Malakor's face is not drawn here: ${why}. The avatar slot keeps its silhouette.`);
}

/** Pixels behind each CSS pixel: the screen's, at most 2, and never past 1024. */
function backing(size: number): number {
  const dpr = typeof window !== "undefined" ? Math.min(2, window.devicePixelRatio || 1) : 1;
  return Math.max(1, Math.min(1024, Math.round(size * dpr)));
}

/**
 * What the harness may add: `start` runs just before each frame is drawn, `end` just after with the
 * milliseconds the draw took, a GPU finish included.
 */
export interface FrameProbe {
  start?(): void;
  end(ms: number): void;
}
export type CutFaceOptions = StartFaceOptions & { probe?: FrameProbe };

/**
 * Start the cut-out face on `canvas`. As the glow's `startFace`: `data-face` loading | ready |
 * unavailable, `data-face-loop` running | still, `data-form` rig | placeholder; no WebGL2 or a form
 * that cannot be drawn leaves the face unavailable and the canvas hidden, so the slot keeps its
 * silhouette. `look` wakes the face when it changes; without it he looks straight out.
 */
export function startFace(opts: CutFaceOptions): FaceHandle {
  const { root, canvas, source } = opts;
  let disposed = false;
  let renderer: CutRenderer | null = null;
  let loop: FaceLoop | null = null;
  const cleanups: (() => void)[] = [];
  root.setAttribute("data-face", "loading");

  const fail = (why: string) => {
    root.setAttribute("data-face", "unavailable");
    canvas.style.visibility = "hidden";
    warnOnce(why);
    opts.onDrawn?.(null);
  };

  const size = (s: number) => {
    const px = backing(s);
    if (canvas.width !== px || canvas.height !== px) {
      canvas.width = px;
      canvas.height = px;
    }
  };
  size(opts.size);

  if (typeof WebGL2RenderingContext === "undefined") {
    fail("WebGL2 is unavailable");
    return { resize: () => {}, dispose: () => {} };
  }

  const clock = opts.clock ?? frames();
  const driver = withLook(createDriver({ seed: 7 }), opts.look);
  let drawnForm: LoadedForm["form"] = "placeholder";
  let last: Pose = driver.rest();
  const probe = opts.probe;
  const draw = (pose: Pose) => {
    last = pose;
    if (!renderer) return;
    if (!probe) {
      renderer.render(pose);
      return;
    }
    probe.start?.();
    const t0 = performance.now();
    renderer.render(pose);
    renderer.finish();
    probe.end(performance.now() - t0);
  };

  loadForm(opts.rigBase, buildPlaceholder)
    .then(async (form) => {
      if (disposed) return null;
      root.setAttribute("data-form", form.form);
      drawnForm = form.form;
      try {
        return await createCutRenderer(canvas, form, () => renderer?.render(last));
      } catch (error) {
        fail(error instanceof Error ? error.message : String(error));
        return null;
      }
    })
    .then(
      (made) => {
        if (!made) return;
        if (disposed) {
          made.dispose();
          return;
        }
        renderer = made;
        loop = createFaceLoop({ driver, draw, clock, calm: calmNow(), onState: (s) => root.setAttribute("data-face-loop", s) });
        const l = loop;

        draw(driver.rest());
        canvas.style.visibility = "visible";
        root.setAttribute("data-face", "ready");
        opts.onDrawn?.(drawnForm);

        cleanups.push(
          source.subscribe((signal) => {
            if (driver.setSignal(signal)) l.wake();
          }),
        );

        cleanups.push(onLookChange(opts.look, () => l.wake()));

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
      },
      (error: unknown) => {
        if (!disposed) fail(`the face did not load (${error instanceof Error ? error.message : String(error)})`);
      },
    );

  return {
    resize(s) {
      size(s);
      if (renderer) {
        renderer.resize();
        renderer.render(last);
        loop?.wake();
      }
    },
    dispose() {
      disposed = true;
      loop?.dispose();
      for (const c of cleanups.splice(0)) c();
      renderer?.dispose();
      renderer = null;
    },
  };
}
