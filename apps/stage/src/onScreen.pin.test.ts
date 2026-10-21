// The on-screen schedule is the broadcast's own rule, pinned to what the canvas renders: the demo
// run is folded and sampled every 250 ms, and at every sample stage-kit's `onScreen` names exactly
// the timed overlays the stage draws. Written against the stage's components before they read
// the schedule, so moving them onto it cannot move the broadcast.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { buildMap, initialScene, reduce, type SceneState } from "@shadowrunr/stage-kit";
import { parseTranscript } from "@shadowrunr/stage-kit/parse";
import { onScreen } from "@shadowrunr/stage-kit/ui";
import { Stage } from "./Stage";

const root = join(__dirname, "..");
const events = parseTranscript(readFileSync(join(root, "fixtures", "demo.jsonl"), "utf8")).events;
const map = buildMap(null, []);

const SLOTS = ["card", "poll", "decree", "caption", "moment", "seal", "redaction", "intro", "wipe", "ticker"] as const;
type Slot = (typeof SLOTS)[number];

/** Which timed overlays the canvas draws, read off its markup. */
function drawn(html: string): Record<Slot, boolean> {
  return {
    card: /class="explainer[ "]/.test(html),
    poll: html.includes("vote-panel"),
    decree: /class="decree decree-/.test(html),
    caption: /class="caption"/.test(html),
    moment: html.includes("moment-chip"),
    seal: html.includes("seal-toast"),
    redaction: /class="redaction"/.test(html),
    intro: /class="intro[ "]/.test(html),
    wipe: /class="wipe"/.test(html),
    ticker: !/region region-ticker hidden/.test(html),
  };
}

describe("onScreen against the broadcast canvas", () => {
  it("names every timed overlay the stage draws, at every 250 ms of the demo run", () => {
    const end = events[events.length - 1]!.ts_ms + 20_000;
    let scene: SceneState = initialScene();
    let next = 0;
    const seen: Record<Slot, number> = Object.fromEntries(SLOTS.map((s) => [s, 0])) as Record<Slot, number>;
    const misses: string[] = [];
    for (let t = 0; t <= end; t += 250) {
      while (next < events.length && events[next]!.ts_ms <= t) scene = reduce(scene, events[next++]!);
      const html = renderToStaticMarkup(
        createElement(Stage, { scene, now: t, map, caseFile: null, sourceTag: "REPLAY", debug: false, broadcast: true }),
      );
      const want = drawn(html);
      const got = onScreen(scene, t);
      for (const s of SLOTS) {
        if (want[s]) seen[s]++;
        if (want[s] !== (got[s] !== null)) misses.push(`${s}@${t}: drawn ${want[s]}, scheduled ${got[s] !== null}`);
      }
      // the decree slot holds the last line quietly between turns
      if (got.decree) expect(html.includes("decree-held")).toBe(got.decree.value.held);
    }
    expect(misses.slice(0, 10)).toEqual([]);
    // the run exercises every slot, so agreement is not agreement on nothing
    for (const s of SLOTS) expect(seen[s], s).toBeGreaterThan(0);
  }, 120_000);

  it("leaves apps/stage no hand-copied visibility window", () => {
    const src = join(root, "src");
    const copies = readdirSync(src, { recursive: true, encoding: "utf8" })
      .filter((f) => /\.tsx?$/.test(f) && !/\.test\.tsx?$/.test(f))
      .flatMap((f) => {
        const text = readFileSync(join(src, f), "utf8");
        return ["now - c" + ".at", "REDACTION_MS" + ")"].filter((p) => text.includes(p)).map((p) => `${f}: ${p}`);
      });
    expect(copies).toEqual([]);
  });
});
