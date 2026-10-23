// The whole bench in order, one script at a time (they share the ports and must not overlap):
// frames, process, ready, voice (when --shadowrunr and --voice-config are given), sheet, clips,
// then the report. Flags are passed through to every script.
//
//   node bench/run-all.mjs [--repeat 3] [--shadowrunr <exe> --voice-config <toml>] [--skip voice,clips]

import { spawnSync } from "node:child_process";
import { join } from "node:path";

import { args, BENCH } from "./lib/env.mjs";

const a = args();
const skip = new Set(String(a.skip ?? "").split(",").filter(Boolean));
if (!a.shadowrunr || !a["voice-config"]) skip.add("voice");
const pass = process.argv.slice(2).filter((_, i, all) => all[i] !== "--skip" && all[i - 1] !== "--skip");

for (const step of ["frames", "process", "ready", "voice", "sheet", "clips", "report"]) {
  if (skip.has(step)) {
    console.log(`== ${step}: skipped`);
    continue;
  }
  console.log(`== ${step}`);
  const r = spawnSync(process.execPath, [join(BENCH, `${step}.mjs`), ...pass], { stdio: "inherit" });
  if (r.status !== 0) {
    console.error(`${step} failed (exit ${r.status})`);
    process.exit(1);
  }
}
