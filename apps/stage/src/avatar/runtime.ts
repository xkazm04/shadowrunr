// The avatar runtime: the lazy half of `<Avatar>`. `Avatar.tsx` imports this module with
// `import()`, so nothing of the renderer, the driver, the placeholder painter or the shaders lands
// in the stage's first paint. It loads the form (the daemon's rig, else the placeholder), makes the
// WebGL2 renderer, subscribes to the face source and draws.
//
// **One loop, and none when nothing moves.** Every face draws from the page's one frame clock
// (`./motion`). A face's subscription ends when its driver has settled at rest and no signal
// changed, when its canvas is hidden, and under reduced motion; a signal change, coming back into
// view or leaving reduced motion starts it again. Under reduced motion (`data-motion="calm"` on
// the root, or with no attribute the OS preference) the face is one still frame at rest, eyes
// open, and stays so. A signal never causes a React render: it goes straight to the driver.
//
// **Where he looks.** With a `look` (the stage's: at the viewer, the content, or the chat column he
// is answering) the eyes lead and the head follows (`withLook`, in the glow's units below). The
// look is an event: a change wakes the loop, which runs until the head has arrived and stops; under
// reduced motion it is one still frame at the new look. Nothing polls it.

import { withLook, type LookUnits } from "./cut/look";
import { createDriver, type Driver, type Pose } from "./driver";
import { createGlowRenderer, type GlowRenderer } from "./glow";
import { loadForm, type LoadedForm } from "./layers";
import { frames, systemPrefersReduced, type Clock } from "./motion";
import { buildPlaceholder } from "./placeholder";
import type { FaceSource } from "./types";
import type { LookSource } from "./variants";

/** Degrees the glow head turns at a full look. Its warp is a flat card turned in an orthographic
 *  view, so it stays small: past about 8 degrees, driver and look together, the edges shear. */
export const GLOW_LOOK_YAW = 6;
/** The glow's look mapping: its eye light keeps its 3.5 x 2 px travel (glow.ts, GLOW_MAPPING) and
 *  a full look slides it that far again; the head turns GLOW_LOOK_YAW, held inside 8 x 5 degrees. */
export const GLOW_LOOK: LookUnits = Object.freeze({
  eyePx: [3.5, 2] as const,
  lookPx: [3.5, 2] as const,
  lookYaw: GLOW_LOOK_YAW,
  lookPitch: 2.5,
  yawMax: 8,
  pitchMax: 5,
});

/** Wake `wake` on each change of `look`; the returned function stops listening. */
export function onLookChange(look: LookSource | undefined, wake: () => void): () => void {
  return look?.subscribe?.(() => wake()) ?? (() => {});
}

/** Whether the face must hold still: the root's `data-motion`, else the OS preference. */
export function calmNow(): boolean {
  if (typeof document === "undefined") return false;
  const mode = document.documentElement.getAttribute("data-motion");
  if (mode) return mode === "calm";
  return systemPrefersReduced();
}

export interface FaceLoop {
  /** A signal changed (or the face appeared): run until settled, or draw the still frame. */
  wake(): void;
  /** Visibility of the canvas (in view and displayed). */
  setVisible(visible: boolean): void;
  /** Reduced motion turned on or off. */
  setCalm(calm: boolean): void;
  /** Whether a frame subscription is live. */
  readonly running: boolean;
  dispose(): void;
}

/**
 * The loop for one face, with its clock and drawing injected so a test can step it frame by frame.
 * `draw` receives the pose; `onState` hears "running" and "still" as the loop starts and stops.
 */
export function createFaceLoop(opts: {
  driver: Driver;
  draw: (pose: Pose) => void;
  clock: Clock;
  calm: boolean;
  onState?: (state: "running" | "still") => void;
}): FaceLoop {
  const { driver, draw, clock } = opts;
  let calm = opts.calm;
  let visible = true;
  let unsubscribe: (() => void) | null = null;
  let disposed = false;

  const state = (s: "running" | "still") => opts.onState?.(s);

  const stop = () => {
    if (unsubscribe) {
      const u = unsubscribe;
      unsubscribe = null;
      u();
    }
    state("still");
  };

  const tick = (_time: number, delta: number): boolean => {
    if (disposed || calm || !visible) {
      unsubscribe = null;
      state("still");
      return false;
    }
    draw(driver.step(delta / 1000));
    if (driver.settled) {
      unsubscribe = null;
      state("still");
      return false;
    }
    return true;
  };

  const wake = () => {
    if (disposed) return;
    if (calm) {
      draw(driver.rest());
      state("still");
      return;
    }
    if (!visible || unsubscribe) return;
    unsubscribe = clock.subscribe(tick);
    state("running");
  };

  return {
    wake,
    setVisible(v) {
      if (v === visible) return;
      visible = v;
      if (v) wake();
      else stop();
    },
    setCalm(c) {
      if (c === calm) return;
      calm = c;
      if (c) stop();
      wake();
    },
    get running() {
      return unsubscribe !== null;
    },
    dispose() {
      disposed = true;
      stop();
    },
  };
}

export interface FaceHandle {
  /** The box changed size, in CSS pixels. */
  resize(size: number): void;
  dispose(): void;
}

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
 * Start a face on `canvas`. `rigBase` is the daemon's `/avatar/` URL (null: no daemon, draw the
 * placeholder). `root` carries the state for anyone who needs to read it: `data-face` = loading |
 * ready | unavailable, `data-face-loop` = running | still, `data-form` = rig | placeholder.
 */
export function startFace(opts: {
  root: HTMLElement;
  canvas: HTMLCanvasElement;
  rigBase: string | null;
  source: FaceSource;
  size: number;
  clock?: Clock;
  /** Hears which form is drawn once the face is ready, and `null` when it is unavailable. */
  onDrawn?: (form: LoadedForm["form"] | null) => void;
  /** Where he looks; without it, straight out. */
  look?: LookSource;
}): FaceHandle {
  const { root, canvas, source } = opts;
  let disposed = false;
  let renderer: GlowRenderer | null = null;
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

  // without a look the driver is drawn as it always was; with one, it looks on top
  const driver: Driver = opts.look ? withLook(createDriver({ seed: 7 }), opts.look, GLOW_LOOK) : createDriver({ seed: 7 });
  let drawnForm: LoadedForm["form"] = "placeholder";
  let last: Pose = driver.rest();
  const draw = (pose: Pose) => {
    last = pose;
    renderer?.render(pose);
  };

  loadForm(opts.rigBase, buildPlaceholder)
    .then(async (form) => {
      if (disposed) return null;
      root.setAttribute("data-form", form.form);
      drawnForm = form.form;
      try {
        return await createGlowRenderer(canvas, form, () => renderer?.render(last));
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
        loop = createFaceLoop({
          driver,
          draw,
          clock: opts.clock ?? frames(),
          calm: calmNow(),
          onState: (s) => root.setAttribute("data-face-loop", s),
        });
        const l = loop;

        // the first frame is the rest frame, so nothing pops in half-posed
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
        // a resize clears the canvas: draw the last pose again even when the loop is still
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
