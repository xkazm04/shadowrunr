// Process cost under the paced script: each face on a `vite build` of the bench page, started at
// load on the page's own frame clock and looping the bench script in real time, as the stage runs
// it. For `--seconds` (60) it samples at 1 Hz: the CPU time of every process in the browser's tree
// by role (renderer, gpu, browser, utility; percent of ONE core), nvidia-smi's utilisation, memory
// and graphics clock, and the machine's busy share. Two baselines bracket the faces: the machine
// with no bench browser, and the browser on the bench page with no face.
//
//   node bench/process.mjs [--faces ...] [--sizes 440,1024] [--seconds 60]
//
// VRAM delta is the device's memory used during the run minus the 5 s just before the browser
// started (device-wide: anything else that allocates in that window lands in it; nvidia-smi under
// WDDM cannot attribute memory to a process). Writes <out>/process.json.

import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { benchPage, commit, configLabel, faceConfigs, gpuBrowser, gpuSampler, liveFace, loadSampler, outDir, previewServer, procSampler, rigServer, settings, sleep, topProcesses } from "./lib/env.mjs";
import { summarize, treeCpu } from "./lib/stats.mjs";

const s = settings();
outDir(s);
const seconds = Number(s.args.seconds ?? 60);
const configs = [{ face: "none" }, { face: "blank" }, ...faceConfigs(s.faces, s.sizes)];

const median = (xs) => summarize(xs).p50;

/** Sample everything for `seconds` while `tree` (a browser pid, or none) runs. */
async function measure(treePid) {
  const procs = treePid ? procSampler(["chrome.exe"]) : null;
  const gpuS = gpuSampler();
  const load = loadSampler();
  await sleep(seconds * 1000);
  const busy = load.stop();
  const gpu = gpuS ? await gpuS.stop() : undefined;
  const samples = procs ? await procs.stop() : undefined;
  return { busy, gpu, samples };
}

const rig = await rigServer(s.rigDir, s.rigPort);
const web = await previewServer(s.port, join(s.out, "dist"));
const rows = [];
try {
  for (const c of configs) {
    const top = topProcesses(6);
    // the device before this browser exists
    const pre = gpuSampler();
    await sleep(5000);
    const before = pre ? await pre.stop() : undefined;
    const vramBase = before ? median(before.map((r) => r.memMiB)) : undefined;

    let gpu = null;
    let ctx = null;
    let ready;
    let heap;
    try {
      if (c.face !== "none") {
        gpu = await gpuBrowser();
        if (c.face === "blank") {
          ctx = await benchPage(gpu.browser);
          await ctx.page.goto(web.url);
          ready = { state: "blank" };
        } else {
          ctx = await liveFace(gpu.browser, web.url, { ...c, rig: rig.url }, (m) => console.log(`  ${m}`));
          ready = ctx.ready;
          if (ready.state !== "ready" || ready.fellBack) throw new Error(`${configLabel(c)} did not start as itself: ${JSON.stringify(ready)}`);
        }
        // let the first loads and compiles pass
        await sleep(3000);
      }
      const m = await measure(gpu?.pid);
      if (ctx) heap = await ctx.page.evaluate(() => (performance.memory ? { usedMB: performance.memory.usedJSHeapSize / 1e6, totalMB: performance.memory.totalJSHeapSize / 1e6 } : undefined));
      const cpu = m.samples ? treeCpu(m.samples, gpu.pid) : undefined;
      const row = {
        ...c,
        label: configLabel(c),
        seconds,
        form: ready?.form,
        cpuPctOfOneCore: cpu,
        gpuUtilPct: m.gpu ? summarize(m.gpu.map((r) => r.util), 1) : undefined,
        gpuClockMHz: m.gpu ? summarize(m.gpu.map((r) => r.clockMHz), 0) : undefined,
        vramMiB: m.gpu ? summarize(m.gpu.map((r) => r.memMiB), 0) : undefined,
        vramBaseMiB: vramBase,
        vramDeltaMiB: m.gpu && vramBase !== undefined ? median(m.gpu.map((r) => r.memMiB)) - vramBase : undefined,
        machineBusyPct: summarize(m.busy.map((r) => r.busy), 1),
        jsHeap: heap,
        foreignTop: top,
      };
      rows.push(row);
      console.log(
        `${row.label}: tree cpu p50 ${cpu?.total?.p50 ?? "-"}% (renderer ${cpu?.renderer?.p50 ?? "-"}, gpu ${cpu?.gpu?.p50 ?? "-"}), gpu util p50 ${row.gpuUtilPct?.p50}%, vram +${row.vramDeltaMiB} MiB, machine ${row.machineBusyPct.p50}%`,
      );
    } finally {
      await ctx?.context.close().catch(() => {});
      await gpu?.close();
    }
  }
} finally {
  await web.close();
  await rig.close();
}
writeFileSync(join(s.out, "process.json"), JSON.stringify({ at: new Date().toISOString(), commit: commit(), rows }, null, 1));
console.log(`wrote ${join(s.out, "process.json")}`);
