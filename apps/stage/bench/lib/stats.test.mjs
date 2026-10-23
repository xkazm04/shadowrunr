// The bench's pure helpers (node --test apps/stage/bench/lib).

import assert from "node:assert/strict";
import { test } from "node:test";

import { firstAudioMs, machineBusy, percentile, pidTree, processRole, shuffle, spread, summarize, treeCpu } from "./stats.mjs";

test("percentile is nearest rank and ignores what is not a number", () => {
  const xs = [5, 1, 4, 2, 3, NaN, undefined];
  assert.equal(percentile(xs, 0.5), 3);
  assert.equal(percentile(xs, 0.95), 5);
  assert.equal(percentile(xs, 0), 1);
  assert.equal(percentile([], 0.5), undefined);
  // 100 values: p99 is the 99th, not the max
  const hundred = Array.from({ length: 100 }, (_, i) => i + 1);
  assert.equal(percentile(hundred, 0.99), 99);
  assert.equal(percentile(hundred, 0.5), 50);
});

test("summarize gives n, the percentiles, max and mean; nothing for no values", () => {
  const s = summarize([1, 2, 3, 4]);
  assert.deepEqual(s, { n: 4, p50: 2, p95: 4, p99: 4, max: 4, mean: 2.5 });
  assert.deepEqual(summarize([]), { n: 0 });
});

test("spread leaves out runs without the statistic instead of counting them as zero", () => {
  const sp = spread([{ p50: 2 }, { n: 0 }, { p50: 1 }, { p50: 3 }], "p50");
  assert.deepEqual(sp, { median: 2, min: 1, max: 3, n: 3 });
  assert.equal(spread([{ n: 0 }], "p50"), undefined);
});

test("processRole reads Chromium's --type", () => {
  assert.equal(processRole("chrome.exe --no-sandbox"), "browser");
  assert.equal(processRole("chrome.exe --type=renderer --x"), "renderer");
  assert.equal(processRole("chrome.exe --type=gpu-process"), "gpu");
  assert.equal(processRole("chrome.exe --type=utility --utility-sub-type=network.mojom.NetworkService"), "utility");
  assert.equal(processRole("chrome.exe --type=crashpad-handler"), "crashpad-handler");
  assert.equal(processRole(undefined), "browser");
});

test("pidTree walks children and ignores processes outside the tree", () => {
  const procs = [
    { pid: 1, ppid: 0 },
    { pid: 2, ppid: 1 },
    { pid: 3, ppid: 2 },
    { pid: 4, ppid: 99 },
    { pid: 5, ppid: 1 },
  ];
  assert.deepEqual([...pidTree(procs, 1)].sort(), [1, 2, 3, 5]);
  assert.deepEqual([...pidTree(procs, 42)], []);
});

test("treeCpu sums a tree's CPU deltas by role, in percent of one core", () => {
  const p = (pid, ppid, cmd, cpuMs) => ({ pid, ppid, cmd, cpuMs });
  const samples = [
    { t: 0, procs: [p(1, 0, "chrome", 0), p(2, 1, "x --type=renderer", 0), p(3, 1, "x --type=gpu-process", 0), p(9, 0, "other", 0)] },
    { t: 1000, procs: [p(1, 0, "chrome", 100), p(2, 1, "x --type=renderer", 500), p(3, 1, "x --type=gpu-process", 250), p(9, 0, "other", 900)] },
    // a process that appears mid-run counts from its second sample on
    { t: 2000, procs: [p(1, 0, "chrome", 200), p(2, 1, "x --type=renderer", 1500), p(3, 1, "x --type=gpu-process", 500), p(6, 2, "x --type=renderer", 300)] },
  ];
  const r = treeCpu(samples, 1);
  assert.equal(r.intervals, 2);
  assert.deepEqual([r.renderer.p50, r.renderer.max], [50, 100]);
  assert.equal(r.gpu.mean, 25);
  assert.equal(r.browser.mean, 10);
  assert.deepEqual([r.total.p50, r.total.max], [85, 135]);
});

test("machineBusy is the busy share of all cores between two snapshots", () => {
  const t = (user, idle) => ({ times: { user, nice: 0, sys: 0, irq: 0, idle } });
  assert.equal(machineBusy([t(0, 0), t(0, 0)], [t(50, 50), t(0, 100)]), 25);
  assert.equal(machineBusy([t(0, 0)], [t(0, 0)]), undefined);
});

test("shuffle keeps every item and follows its generator", () => {
  let k = 0;
  const seq = [0.9, 0.1, 0.5];
  const out = shuffle(["a", "b", "c", "d"], () => seq[k++ % seq.length]);
  assert.deepEqual([...out].sort(), ["a", "b", "c", "d"]);
  assert.deepEqual(out, shuffle(["a", "b", "c", "d"], (() => { let j = 0; return () => seq[j++ % seq.length]; })()));
});

test("firstAudioMs reads voice say's summary line", () => {
  assert.equal(firstAudioMs("2 part(s), 3921 ms of audio, 0 visemes, 54 chars, first part after 3838 ms, all in 4438 ms"), 3838);
  assert.equal(firstAudioMs("voice did not start"), undefined);
});
