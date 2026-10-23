// First paint: how long each face takes from navigation to `data-face="ready"` on a `vite build`
// of the bench page, cold (a fresh browser and profile per run, so no HTTP, shader or GPU cache
// carries over; the rig is served no-store). Also the time from the face's `startFace` call to
// ready (the face's own load: its chunk, the rig, a depth map or the GLB, the shaders) and the
// bytes the page fetched, by origin.
//
//   node bench/ready.mjs [--faces ...] [--size 440] [--repeat 5]
//
// Writes <out>/ready.json.

import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { commit, gpuBrowser, liveFace, machineLoad, outDir, previewServer, rigServer, settings } from "./lib/env.mjs";
import { summarize } from "./lib/stats.mjs";

const s = settings();
outDir(s);
const size = Number(s.args.size ?? 440);
const repeat = Number(s.args.repeat ?? 5);

const rig = await rigServer(s.rigDir, s.rigPort);
const web = await previewServer(s.port, join(s.out, "dist"));
const runs = [];
try {
  for (let k = 0; k < repeat; k++) {
    for (const face of s.faces) {
      const busy = await machineLoad(1500);
      const gpu = await gpuBrowser();
      try {
        const { page, ready } = await liveFace(gpu.browser, web.url, { face, size, rig: rig.url });
        if (ready.state !== "ready" || ready.fellBack) throw new Error(`${face} did not start as itself: ${JSON.stringify(ready)}`);
        const bytes = await page.evaluate((rigUrl) => {
          const out = { page: 0, rig: 0 };
          for (const e of performance.getEntriesByType("resource")) out[e.name.startsWith(rigUrl) ? "rig" : "page"] += e.encodedBodySize || e.transferSize || 0;
          return out;
        }, rig.url);
        const run = { face, size, run: k + 1, readyFromNavMs: Math.round(ready.readyFromNavMs), readyFromStartMs: Math.round(ready.readyFromStartMs), pageKB: Math.round(bytes.page / 1024), rigKB: Math.round(bytes.rig / 1024), machineBusyPct: busy === undefined ? undefined : Math.round(busy) };
        runs.push(run);
        console.log(`${face} #${k + 1}: ready ${run.readyFromNavMs} ms from navigation (${run.readyFromStartMs} ms from startFace), page ${run.pageKB} KB, rig ${run.rigKB} KB, busy ${run.machineBusyPct}%`);
      } finally {
        await gpu.close();
      }
    }
  }
} finally {
  await web.close();
  await rig.close();
}
const faces = {};
for (const face of s.faces) {
  const mine = runs.filter((r) => r.face === face);
  faces[face] = { fromNavMs: summarize(mine.map((r) => r.readyFromNavMs), 0), fromStartMs: summarize(mine.map((r) => r.readyFromStartMs), 0), pageKB: mine[0]?.pageKB, rigKB: mine[0]?.rigKB };
}
writeFileSync(join(s.out, "ready.json"), JSON.stringify({ at: new Date().toISOString(), commit: commit(), size, faces, runs }, null, 1));
console.log(`wrote ${join(s.out, "ready.json")}`);
