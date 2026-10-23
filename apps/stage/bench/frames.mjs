// Frame cost per face: the whole 24 s bench script, frame-stepped at 60 Hz and paced by
// requestAnimationFrame, every frame timed on the GPU by EXT_disjoint_timer_query_webgl2 and on the
// main thread by performance.now() (harness.js `frameCost`). Each config runs `--repeat` times,
// interleaved (every config once per round), because the GPU clocks down at low load.
//
//   node bench/frames.mjs [--faces cut,depth,relit,lich3d] [--sizes 440,1024] [--repeat 3] [--build]
//
// Writes <out>/frames.json (summaries) and <out>/raw/frames-<face>-<size>[-nobloom]-<k>.json
// (every frame). `--build` measures a `vite build` of the page instead of the dev server.

import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { benchPage, commit, devServer, gpuBrowser, gpuNow, machineLoad, outDir, previewServer, rigServer, settings, topProcesses } from "./lib/env.mjs";
import { summarize } from "./lib/stats.mjs";

const s = settings();
const raw = outDir(s, "raw");
const configs = [];
for (const face of s.faces) {
  for (const size of s.sizes) {
    configs.push({ face, size });
    if (face === "lich3d") configs.push({ face, size, bloom: false });
  }
}
const label = (c) => `${c.face}-${c.size}${c.bloom === false ? "-nobloom" : ""}`;

const rig = await rigServer(s.rigDir, s.rigPort);
const web = s.args.build ? await previewServer(s.port, join(s.out, "dist")) : await devServer(s.port);
const runs = [];
let gpu = null;
try {
  gpu = await gpuBrowser();
  console.log(`frames: ${configs.length} configs x ${s.repeat} on ${web.url} (${s.args.build ? "build" : "dev"})`);
  for (let k = 0; k < s.repeat; k++) {
    for (const c of configs) {
      const load = await machineLoad(2000);
      const top = topProcesses(5);
      const gpuBefore = gpuNow();
      const { context, page } = await benchPage(gpu.browser, (m) => console.log(`  ${m}`));
      await page.goto(web.url);
      await page.waitForFunction(() => !!window.__bench);
      const started = await page.evaluate((o) => window.__bench.start(o), { ...c, rig: rig.url, manual: true });
      if (started.state !== "ready" || started.fellBack) throw new Error(`${label(c)} did not start as itself: ${JSON.stringify(started)}`);
      const r = await page.evaluate(() => window.__bench.frameCost({ ticks: 1440, warm: 60 }));
      const gpuName = await page.evaluate(() => window.__bench.gpuName());
      await context.close();
      if (r.unavailable) throw new Error(`${label(c)}: ${r.unavailable}`);
      writeFileSync(join(raw, `frames-${label(c)}-${k + 1}.json`), JSON.stringify({ ...c, run: k + 1, ...r }));
      const drawn = r.rows.filter((x) => x.draws > 0);
      const run = {
        ...c,
        run: k + 1,
        form: started.form,
        isolated: started.isolated,
        px: started.px,
        gpu: gpuName,
        frames: r.rows.length,
        drawnFrames: drawn.length,
        lostQueries: r.lost,
        disjoint: r.disjoint,
        wallMs: Math.round(r.wallMs),
        gpuMs: summarize(drawn.map((x) => x.gpu)),
        cpuMs: summarize(drawn.map((x) => x.cpu)),
        drawsPerFrame: summarize(drawn.map((x) => x.draws), 0),
        heapUsedMB: r.heapUsedMB,
        heapGrowthMB: r.heapGrowthMB,
        machineBusyPct: load === undefined ? undefined : Math.round(load * 10) / 10,
        foreignTop: top,
        gpuBefore,
        gpuAfter: gpuNow(),
      };
      runs.push(run);
      console.log(`${label(c)} #${k + 1}: gpu p50 ${run.gpuMs.p50} p95 ${run.gpuMs.p95} p99 ${run.gpuMs.p99} ms, cpu p50 ${run.cpuMs.p50} ms, drawn ${drawn.length}/${r.rows.length}, busy ${run.machineBusyPct}%`);
    }
  }
} finally {
  await gpu?.close();
  await web.close();
  await rig.close();
}
writeFileSync(join(s.out, "frames.json"), JSON.stringify({ at: new Date().toISOString(), commit: commit(), mode: s.args.build ? "build" : "dev", runs }, null, 1));
console.log(`wrote ${join(s.out, "frames.json")}`);
