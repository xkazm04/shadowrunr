// The dev bench (`?face=bench`) and the held cue (`?perf=`): the script is data a bench replays on
// every face, and it exercises every channel.
import { describe, expect, it } from "vitest";

import { BENCH_MS, BENCH_SCRIPT, benchAt, heldPerformance, parsePerf } from "./demo";
import { LOOK_CHAT, LOOK_CONTENT, LOOK_HOST } from "./look";
import { createPerformanceDriver, cueFor, GESTURES } from "./performance";

describe("the bench script", () => {
  it("fills 24 s with beats end to end", () => {
    expect(BENCH_MS).toBe(24_000);
    let at = 0;
    for (const b of BENCH_SCRIPT) {
      expect(b.at).toBe(at);
      at += b.ms;
    }
    expect(at).toBe(BENCH_MS);
  });

  it("plays every gesture, every look, speech and quiet gaps", () => {
    const gestures = new Set(BENCH_SCRIPT.flatMap((b) => (b.cue?.gesture ? [b.cue.gesture] : [])));
    // outrage's recoil brings its flare; triumph flares on its own
    expect([...gestures].sort()).toEqual(["glare", "lean_in", "nod", "recoil", "tilt", "flare"].sort());
    expect(GESTURES.every((g) => gestures.has(g))).toBe(true);
    expect(new Set(BENCH_SCRIPT.map((b) => b.look))).toEqual(new Set(["host", "content", "chat"]));
    expect(BENCH_SCRIPT.some((b) => !b.speak && !b.cue)).toBe(true);
  });

  it("each cue is what the stage would make of that delivery", () => {
    for (const b of BENCH_SCRIPT) if (b.cue) expect(b.cue).toEqual(cueFor({ ...b.cue, pace: "normal" }));
  });

  it("is a pure function of time: the same ms, the same inputs; a new loop, new cue ids", () => {
    expect(benchAt(4100)).toEqual(benchAt(4100));
    expect(benchAt(4100).signal.phase).toBe("speaking");
    expect(benchAt(4100).cue).toMatchObject({ emotion: "menace", gesture: "glare" });
    expect(benchAt(7000).signal.phase).toBe("idle");
    expect(benchAt(8000).look).toEqual(LOOK_CHAT);
    expect(benchAt(14_500).look).toEqual(LOOK_CONTENT);
    expect(benchAt(500).look).toEqual(LOOK_HOST);
    const a = benchAt(11_000).cue;
    const b = benchAt(11_000 + BENCH_MS).cue;
    expect(b.id).toBeGreaterThan(a.id);
    expect({ ...b, id: 0 }).toEqual({ ...a, id: 0 });
  });

  it("drives a performance driver the same way every replay", () => {
    const replay = () => {
      const d = createPerformanceDriver();
      const out: number[] = [];
      for (let ms = 0; ms < BENCH_MS; ms += 16) {
        d.setCue(benchAt(ms).cue);
        const p = d.step(0.016);
        out.push(p.eyeLight, p.gesturePitch, p.gestureRoll, p.lean, p.flare);
      }
      return out;
    };
    expect(replay()).toEqual(replay());
  });
});

describe("?perf=", () => {
  it("parses emotion, intensity and gesture", () => {
    expect(parsePerf("outrage:1")).toEqual({ emotion: "outrage", intensity: 1, gesture: "recoil" });
    expect(parsePerf("hushed")).toEqual({ emotion: "hushed", intensity: 1, gesture: "lean_in" });
    expect(parsePerf("menace:0.4")).toEqual({ emotion: "menace", intensity: 0.4 });
    expect(parsePerf("glee:0.3:nod")).toEqual({ emotion: "glee", intensity: 0.3, gesture: "nod" });
    expect(parsePerf("smug")).toBeNull();
    expect(parsePerf(null)).toBeNull();
  });

  it("holds one cue", () => {
    const s = heldPerformance({ emotion: "outrage", intensity: 1 });
    expect(s.get()).toEqual({ id: 1, emotion: "outrage", intensity: 1 });
    expect(s.get()).toBe(s.get());
  });
});
