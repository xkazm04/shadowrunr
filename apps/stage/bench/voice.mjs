// Voice contention: does a face slow the local voice down? `shadowrunr voice say` speaks a fixed set
// of lines through the real sidecar (Kokoro, local: no quota) while each face runs the paced bench
// script in a GPU browser (`vite build` of the bench page, 440 px), and with no browser at all
// before and after. It reads each line's first-audio time from the summary line (`first part after
// N ms`) and reports the median per condition.
//
//   node bench/voice.mjs --shadowrunr <shadowrunr.exe> --voice-config <shadowrunr.toml> [--faces ...] [--rounds 2]
//
// The config must give a working `[voice]` (`shadowrunr --config <it> voice check`). The provider
// is forced to kokoro, so no line ever reaches a paid voice. Writes <out>/voice.json.

import { spawnSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { commit, configLabel, gpuBrowser, liveFace, machineLoad, outDir, previewServer, rigServer, settings, sleep } from "./lib/env.mjs";
import { firstAudioMs, summarize } from "./lib/stats.mjs";

/** Made up for the bench: Malakor's register, nobody's words. */
export const LINES = [
  "I am Malakor. Kneel before the ledger of doom, mortals.",
  "Your offerings are noted, and found wanting.",
  "Chat, you have one minute to impress me. Begin.",
  "Ah, a question. How delightfully brave of you.",
  "The crown remembers every insult. So do I.",
];

const s = settings();
outDir(s);
const exe = s.args.shadowrunr;
const config = s.args["voice-config"];
if (!exe || !config) throw new Error("--shadowrunr <exe> and --voice-config <toml> are required");
const rounds = Number(s.args.rounds ?? 2);
const size = Number(s.args.size ?? 440);

const check = spawnSync(exe, ["--config", config, "voice", "check"], { encoding: "utf8" });
if (check.status !== 0) throw new Error(`voice check failed (exit ${check.status}):\n${check.stdout}${check.stderr}`);

function say(line) {
  const r = spawnSync(exe, ["--config", config, "voice", "say", "--provider", "kokoro", line], { encoding: "utf8", timeout: 120_000 });
  const out = `${r.stdout}${r.stderr}`;
  return { exit: r.status, firstMs: firstAudioMs(out), engine: /engine: ([^;)]+)/.exec(out)?.[1]?.trim() };
}

const conditions = [{ face: "none" }, ...s.faces.map((face) => ({ face, size })), { face: "none", after: true }];
const rig = await rigServer(s.rigDir, s.rigPort);
const web = await previewServer(s.port, join(s.out, "dist"));
const results = [];
try {
  for (let k = 0; k < rounds; k++) {
    for (const c of conditions) {
      const busy = await machineLoad(1500);
      let gpu = null;
      try {
        if (c.face !== "none") {
          gpu = await gpuBrowser();
          const { ready } = await liveFace(gpu.browser, web.url, { ...c, rig: rig.url });
          if (ready.state !== "ready" || ready.fellBack) throw new Error(`${c.face} did not start as itself`);
          await sleep(2000);
        }
        for (const line of LINES) {
          const r = say(line);
          results.push({ condition: c.after ? "none-after" : configLabel(c), round: k + 1, line: LINES.indexOf(line), ...r, machineBusyPct: busy === undefined ? undefined : Math.round(busy) });
          console.log(`${c.after ? "none-after" : configLabel(c)} r${k + 1} line ${LINES.indexOf(line)}: first audio ${r.firstMs ?? "-"} ms (exit ${r.exit})`);
        }
      } finally {
        await gpu?.close();
      }
    }
  }
} finally {
  await web.close();
  await rig.close();
}
const summary = {};
for (const r of results) (summary[r.condition] ??= []).push(r.firstMs);
for (const k of Object.keys(summary)) summary[k] = summarize(summary[k], 0);
writeFileSync(join(s.out, "voice.json"), JSON.stringify({ at: new Date().toISOString(), commit: commit(), engine: results.find((r) => r.engine)?.engine, lines: LINES, summary, results }, null, 1));
console.log(`wrote ${join(s.out, "voice.json")}`);
