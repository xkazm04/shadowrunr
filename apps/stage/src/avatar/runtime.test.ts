// The face's loop on the shared frame clock: no frames once settled, a still frame under reduced
// motion, none while unseen; and the clock itself (one callback for every subscriber).
import { describe, expect, it, vi } from "vitest";

import { createDriver, type Pose } from "./driver";
import { createClock } from "./motion";
import { createFaceLoop, startFace } from "./runtime";
import { RESTING, type FaceSignal } from "./types";

/** A clock stepped by hand: `tick()` runs one frame 1/60 s after the last. */
function handClock() {
  let pending: ((t: number) => void) | null = null;
  let hidden = false;
  let visible: () => void = () => {};
  let now = 0;
  const clock = createClock({
    raf: (cb) => {
      pending = cb;
      return 1;
    },
    caf: () => {
      pending = null;
    },
    hidden: () => hidden,
    onVisible: (cb) => {
      visible = cb;
    },
  });
  return {
    clock,
    tick() {
      const cb = pending;
      pending = null;
      now += 1000 / 60;
      cb?.(now);
    },
    hide: () => (hidden = true),
    show() {
      hidden = false;
      visible();
    },
    get scheduled() {
      return pending !== null;
    },
  };
}

function rig(calm = false, idleLinger = 1) {
  const h = handClock();
  const driver = createDriver({ seed: 1, idleLinger });
  const drawn: Pose[] = [];
  const states: string[] = [];
  const loop = createFaceLoop({ driver, clock: h.clock, calm, draw: (p) => drawn.push({ ...p }), onState: (s) => states.push(s) });
  return { h, driver, loop, drawn, states };
}

const say = (s: Partial<FaceSignal>): FaceSignal => ({ ...RESTING, ...s });

describe("the frame clock", () => {
  it("passes deltas, drops a subscriber that returns false, and stops when empty", () => {
    const h = handClock();
    const deltas: number[] = [];
    let n = 0;
    h.clock.subscribe((_t, d) => {
      deltas.push(d);
      return ++n < 3;
    });
    h.tick();
    h.tick();
    h.tick();
    expect(deltas[0]).toBe(0);
    expect(deltas[1]).toBeCloseTo(1000 / 60, 6);
    expect(h.clock.size).toBe(0);
    expect(h.scheduled).toBe(false);
  });

  it("does no work while the page is hidden and restarts deltas when it comes back", () => {
    const h = handClock();
    const deltas: number[] = [];
    h.clock.subscribe((_t, d) => void deltas.push(d));
    h.tick();
    h.hide();
    h.tick();
    expect(h.scheduled).toBe(false);
    h.show();
    h.tick();
    expect(deltas).toEqual([0, 0]);
  });

  it("a throwing subscriber is dropped and the others keep running", () => {
    const h = handClock();
    const err = console.error;
    console.error = () => {};
    let ran = 0;
    h.clock.subscribe(() => {
      throw new Error("boom");
    });
    h.clock.subscribe(() => void ran++);
    h.tick();
    h.tick();
    console.error = err;
    expect(ran).toBe(2);
    expect(h.clock.size).toBe(1);
  });
});

describe("the face's loop", () => {
  it("runs while it moves and stops asking for frames once settled", () => {
    const { h, loop, drawn, states } = rig();
    loop.wake();
    expect(loop.running).toBe(true);
    let frames = 0;
    while (h.scheduled && frames < 60 * 10) {
      h.tick();
      frames++;
    }
    expect(loop.running).toBe(false);
    expect(h.clock.size).toBe(0);
    expect(h.scheduled).toBe(false); // an idle second costs nothing
    expect(states[states.length - 1]).toBe("still");
    expect(drawn.length).toBe(frames);
    const last = drawn[drawn.length - 1]!;
    expect([last.yaw, last.blinkL, last.open, last.activity]).toEqual([0, 0, 0, 0]);
  });

  it("a signal change wakes a settled face; speaking keeps it running", () => {
    const { h, driver, loop } = rig();
    loop.wake();
    while (h.scheduled) h.tick();
    expect(driver.setSignal(say({ phase: "speaking", level: 0.6, viseme: "aa" }))).toBe(true);
    loop.wake();
    for (let i = 0; i < 600; i++) h.tick();
    expect(loop.running).toBe(true);
  });

  it("two faces on one clock are one frame callback", () => {
    const h = handClock();
    const mk = () => createFaceLoop({ driver: createDriver({ seed: 2 }), clock: h.clock, calm: false, draw: () => {} });
    mk().wake();
    mk().wake();
    expect(h.clock.size).toBe(2);
    expect(h.clock.running).toBe(true);
  });

  it("under reduced motion it draws one still frame at rest and never subscribes", () => {
    const { h, driver, loop, drawn } = rig(true);
    loop.wake();
    expect(loop.running).toBe(false);
    expect(h.scheduled).toBe(false);
    expect(drawn).toHaveLength(1);
    expect([drawn[0]!.yaw, drawn[0]!.blinkL, drawn[0]!.open]).toEqual([0, 0, 0]);
    driver.setSignal(say({ phase: "speaking", level: 0.9, viseme: "aa" }));
    loop.wake();
    expect(h.scheduled).toBe(false);
    expect(drawn.every((p) => p.open === 0)).toBe(true);
  });

  it("turning reduced motion on mid-speech stops on a still; turning it off resumes", () => {
    const { h, driver, loop, drawn } = rig();
    driver.setSignal(say({ phase: "speaking", level: 0.9, viseme: "aa" }));
    loop.wake();
    for (let i = 0; i < 20; i++) h.tick();
    loop.setCalm(true);
    expect(loop.running).toBe(false);
    expect(h.clock.size).toBe(0);
    expect(drawn[drawn.length - 1]!.open).toBe(0);
    loop.setCalm(false);
    expect(loop.running).toBe(true);
  });

  it("an unseen face asks for no frames and resumes when seen", () => {
    const { h, driver, loop } = rig();
    driver.setSignal(say({ phase: "speaking", level: 0.5 }));
    loop.wake();
    loop.setVisible(false);
    expect(h.clock.size).toBe(0);
    loop.wake();
    expect(h.clock.size).toBe(0);
    loop.setVisible(true);
    expect(loop.running).toBe(true);
  });

  it("dispose stops it for good", () => {
    const { h, loop } = rig();
    loop.wake();
    loop.dispose();
    expect(h.clock.size).toBe(0);
    loop.wake();
    expect(h.clock.size).toBe(0);
  });
});

describe("without WebGL2", () => {
  it("the face is unavailable, its canvas hidden, and it says why once", () => {
    expect(typeof (globalThis as { WebGL2RenderingContext?: unknown }).WebGL2RenderingContext).toBe("undefined");
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const face = () => {
      const attrs = new Map<string, string>();
      const root = { setAttribute: (k: string, v: string) => void attrs.set(k, v) } as unknown as HTMLElement;
      const canvas = { width: 0, height: 0, style: { visibility: "" } } as unknown as HTMLCanvasElement;
      const handle = startFace({ root, canvas, rigBase: null, source: { subscribe: () => () => {} }, size: 300 });
      return { attrs, canvas, handle };
    };
    const a = face();
    const b = face();
    for (const f of [a, b]) {
      expect(f.attrs.get("data-face")).toBe("unavailable");
      expect(f.canvas.style.visibility).toBe("hidden");
      f.handle.resize(100);
      f.handle.dispose();
    }
    expect(warn).toHaveBeenCalledTimes(1);
    expect(String(warn.mock.calls[0]?.[0])).toMatch(/WebGL2 is unavailable/);
    warn.mockRestore();
  });
});
