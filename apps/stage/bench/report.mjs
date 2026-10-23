// The bench's one page: frames.json, process.json, ready.json and voice.json folded into one
// markdown table, a row per face and size. A figure that was not measured is a dash, never 0.
//
//   node bench/report.mjs        -> <out>/bench.md

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { configLabel, settings } from "./lib/env.mjs";
import { spread } from "./lib/stats.mjs";

const s = settings();
const read = (f) => (existsSync(join(s.out, f)) ? JSON.parse(readFileSync(join(s.out, f), "utf8")) : undefined);
const frames = read("frames.json");
const proc = read("process.json");
const ready = read("ready.json");
const voice = read("voice.json");

const dash = "-";
const n1 = (x, d = 2) => (Number.isFinite(x) ? x.toFixed(d) : dash);
/** "median (min-max)" of one statistic over the repeated runs. */
const sp = (runs, pick, key, d = 2) => {
  const v = spread(runs.map(pick), key);
  if (!v) return dash;
  return v.n > 1 ? `${n1(v.median, d)} (${n1(v.min, d)}-${n1(v.max, d)})` : n1(v.median, d);
};

const labels = [];
for (const r of frames?.runs ?? []) if (!labels.includes(configLabel(r))) labels.push(configLabel(r));
for (const r of proc?.rows ?? []) if (r.size && !labels.includes(r.label)) labels.push(r.label);

const lines = [];
lines.push("# Face bench");
lines.push("");
const gpuName = frames?.runs?.[0]?.gpu;
lines.push(`Commit ${frames?.commit ?? proc?.commit ?? dash}; ${gpuName ?? "GPU unknown"}. Frames: ${frames?.at ?? dash} (${frames?.mode ?? dash} server). Process: ${proc?.at ?? dash}.`);
lines.push("");
lines.push("## Frame cost and process cost");
lines.push("");
lines.push("GPU and CPU ms: per drawn frame over the 24 s bench script at 60 Hz (EXT_disjoint_timer_query_webgl2; CPU = main-thread ms of the frame's step: drivers + WebGL submit). Each cell is the median run of the repeats with the range over the runs. Process: the browser's tree under the paced script for 60 s (percent of ONE core), the device's GPU utilisation and VRAM above the 5 s before the browser started.");
lines.push("");
lines.push("| Face | px | GPU p50 ms | GPU p95 ms | GPU p99 ms | CPU p50 ms | CPU p95 ms | draws/frame | frames drawn | renderer CPU % | GPU-process CPU % | tree CPU % | GPU util % | VRAM +MiB | JS heap MB | machine busy % (frames / process) |");
lines.push("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |");
for (const label of labels) {
  const runs = (frames?.runs ?? []).filter((r) => configLabel(r) === label);
  const p = (proc?.rows ?? []).find((r) => r.label === label);
  const first = runs[0] ?? p;
  const face = `${first.face}${first.bloom === false ? " (no bloom)" : first.face.startsWith("lich3d") ? " (bloom)" : ""}`;
  const cpu = p?.cpuPctOfOneCore;
  lines.push(
    [
      face,
      first.size,
      sp(runs, (r) => r.gpuMs, "p50", 3),
      sp(runs, (r) => r.gpuMs, "p95", 3),
      sp(runs, (r) => r.gpuMs, "p99", 3),
      sp(runs, (r) => r.cpuMs, "p50", 2),
      sp(runs, (r) => r.cpuMs, "p95", 2),
      runs.length ? n1(runs[0].drawsPerFrame.p50, 0) : dash,
      runs.length ? `${runs.map((r) => r.drawnFrames).join(", ")} of ${runs[0].frames}` : dash,
      n1(cpu?.renderer?.p50, 1),
      n1(cpu?.gpu?.p50, 1),
      n1(cpu?.total?.p50, 1),
      n1(p?.gpuUtilPct?.p50, 0),
      n1(p?.vramDeltaMiB, 0),
      n1(p?.jsHeap?.usedMB, 1),
      `${runs.length ? runs.map((r) => n1(r.machineBusyPct, 0)).join("/") : dash} / ${n1(p?.machineBusyPct?.p50, 0)}`,
    ].join(" | ").replace(/^/, "| ") + " |",
  );
}
for (const b of (proc?.rows ?? []).filter((r) => !r.size)) {
  const cpu = b.cpuPctOfOneCore;
  lines.push(`| baseline: ${b.face === "none" ? "no bench browser" : "browser, bench page, no face"} | - | - | - | - | - | - | - | - | ${n1(cpu?.renderer?.p50, 1)} | ${n1(cpu?.gpu?.p50, 1)} | ${n1(cpu?.total?.p50, 1)} | ${n1(b.gpuUtilPct?.p50, 0)} | ${n1(b.vramDeltaMiB, 0)} | ${n1(b.jsHeap?.usedMB, 1)} | - / ${n1(b.machineBusyPct?.p50, 0)} |`);
}
lines.push("");
if (ready) {
  lines.push(`## Ready time (${ready.size} px, cold browser per run, \`vite build\`)`);
  lines.push("");
  lines.push("| Face | navigation to ready p50 ms (min-max) | startFace to ready p50 ms (min-max) | n | page KB | rig KB |");
  lines.push("| --- | --- | --- | --- | --- | --- |");
  for (const [face, r] of Object.entries(ready.faces)) {
    const mine = ready.runs.filter((x) => x.face === face);
    const range = (k) => `${Math.min(...mine.map((x) => x[k]))}-${Math.max(...mine.map((x) => x[k]))}`;
    lines.push(`| ${face} | ${r.fromNavMs.p50} (${range("readyFromNavMs")}) | ${r.fromStartMs.p50} (${range("readyFromStartMs")}) | ${r.fromNavMs.n} | ${r.pageKB ?? dash} | ${r.rigKB ?? dash} |`);
  }
  lines.push("");
}
if (voice) {
  lines.push(`## Voice contention (\`voice say\`, Kokoro, engine: ${voice.engine ?? dash})`);
  lines.push("");
  lines.push("| Condition | first audio p50 ms | p95 ms | max ms | n |");
  lines.push("| --- | --- | --- | --- | --- |");
  for (const [k, v] of Object.entries(voice.summary)) lines.push(`| ${k} | ${v.p50 ?? dash} | ${v.p95 ?? dash} | ${v.max ?? dash} | ${v.n} |`);
  lines.push("");
}
writeFileSync(join(s.out, "bench.md"), lines.join("\n"));
console.log(lines.join("\n"));
