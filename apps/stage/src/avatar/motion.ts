// The face's frame clock and the reduced-motion question: the stage's small equivalent of the
// frame clock the face was ported with. Everything that moves per frame for the face (the face
// loop, the voice's signal producer) subscribes here instead of calling requestAnimationFrame
// itself, so two subscribers are one frame callback. The loop runs while at least one subscriber
// is live and the document is visible, and costs nothing otherwise. A subscriber returns `false`
// when it has settled and is dropped on the spot.

/** True when the OS asks for less motion. `false` where there is no media query to ask. */
export function systemPrefersReduced(): boolean {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return false;
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** A subscriber: called once per frame with the frame time and the time since its last call (ms). */
export type FrameFn = (time: number, delta: number) => boolean | void;

export interface Clock {
  /** Run `fn` every frame until it returns `false` or the returned function is called. */
  subscribe: (fn: FrameFn) => () => void;
  /** Live subscribers. A settled page reads 0. */
  readonly size: number;
  /** True while a frame callback is pending with the browser. */
  readonly running: boolean;
}

export interface ClockEnv {
  raf: (cb: (time: number) => void) => number;
  caf: (id: number) => void;
  hidden: () => boolean;
  /** Called when the page becomes visible again. */
  onVisible: (cb: () => void) => void;
}

/** The loop, with its environment injected so a test can drive it one frame at a time. */
export function createClock(env: ClockEnv): Clock {
  const subs = new Map<FrameFn, number>();
  let handle: number | null = null;

  const frame = (time: number) => {
    handle = null;
    if (env.hidden()) return; // resumes through onVisible
    for (const [fn, last] of [...subs]) {
      if (!subs.has(fn)) continue; // dropped by an earlier subscriber this frame
      let keep: boolean | void;
      try {
        keep = fn(time, last < 0 ? 0 : time - last);
      } catch (error) {
        // One broken subscriber is dropped, said once, and does not stop the others.
        console.error(`[stage] frame subscriber dropped: ${error instanceof Error ? error.message : String(error)}`);
        keep = false;
      }
      if (keep === false) subs.delete(fn);
      else subs.set(fn, time);
    }
    if (subs.size > 0) schedule();
  };

  const schedule = () => {
    if (handle === null && !env.hidden()) handle = env.raf(frame);
  };

  env.onVisible(() => {
    // A long hidden gap must not read as one enormous delta: every subscriber restarts its clock.
    for (const fn of subs.keys()) subs.set(fn, -1);
    schedule();
  });

  return {
    subscribe(fn) {
      subs.set(fn, -1);
      schedule();
      return () => {
        subs.delete(fn);
        if (subs.size === 0 && handle !== null) {
          env.caf(handle);
          handle = null;
        }
      };
    },
    get size() {
      return subs.size;
    },
    get running() {
      return handle !== null;
    },
  };
}

let shared: Clock | null = null;

/** The page's clock, made on first use so importing this module does not touch the document. */
export function frames(): Clock {
  shared ??= createClock({
    raf: (cb) => window.requestAnimationFrame(cb),
    caf: (id) => window.cancelAnimationFrame(id),
    hidden: () => document.hidden,
    onVisible: (cb) => document.addEventListener("visibilitychange", () => !document.hidden && cb()),
  });
  return shared;
}
