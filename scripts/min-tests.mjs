#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// min-tests: run a test command and fail when it ran fewer tests than a floor. `cargo test` exits 0
// on zero tests (an empty workspace, a filter that matches nothing), so a lane that silently lost
// its tests would stay green. This wrapper counts the "test result: ok. N passed" summaries cargo
// prints (or "Ran N tests" from python unittest) and exits 1 below --min, whatever the command's
// own exit code was.
//
// Usage: node scripts/min-tests.mjs --min <N> -- <command> [args...]
//        node scripts/min-tests.mjs --min <N> --count-only < output.txt

import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

/** Passed-test count in a run's output: cargo summaries plus python unittest "Ran N tests". */
export function countTests(text) {
  let n = 0;
  for (const m of text.matchAll(/^test result: \w+\. (\d+) passed;/gm)) n += Number(m[1]);
  for (const m of text.matchAll(/^Ran (\d+) tests? in /gm)) n += Number(m[1]);
  return n;
}

/** Exit code for a run: the command's own failure wins; otherwise below the floor is 1. */
export function verdict(exitCode, count, min) {
  if (exitCode !== 0) return exitCode;
  return count < min ? 1 : 0;
}

function main(argv) {
  const mi = argv.indexOf("--min");
  const min = mi >= 0 ? Number(argv[mi + 1]) : NaN;
  if (!Number.isInteger(min) || min < 1) {
    console.error("min-tests: --min <N> with N >= 1 is required");
    return Promise.resolve(2);
  }
  const finish = (code, text) => {
    const count = countTests(text);
    const v = verdict(code, count, min);
    if (code === 0 && v !== 0) console.error(`min-tests: ran ${count} tests, expected at least ${min}`);
    else console.log(`min-tests: ${count} tests (floor ${min})`);
    return v;
  };
  if (argv.includes("--count-only")) return Promise.resolve(finish(0, readFileSync(0, "utf8")));
  const sep = argv.indexOf("--");
  if (sep < 0 || sep === argv.length - 1) {
    console.error("min-tests: a command is required after --");
    return Promise.resolve(2);
  }
  return new Promise((done) => {
    const child = spawn(argv[sep + 1], argv.slice(sep + 2), { stdio: ["inherit", "pipe", "pipe"], shell: false });
    let text = "";
    for (const [stream, sink] of [[child.stdout, process.stdout], [child.stderr, process.stderr]]) {
      stream.on("data", (d) => {
        text += d;
        sink.write(d);
      });
    }
    child.on("error", (e) => {
      console.error(`min-tests: ${e.message}`);
      done(2);
    });
    child.on("close", (code) => done(finish(code ?? 1, text)));
  });
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = await main(process.argv.slice(2));
}
