// The bench's pure arithmetic: percentiles, a run's summary, the spread over repeated runs, and the
// CPU share of a process tree from 1 Hz samples of its processes' CPU times. No I/O (stats.test.mjs).

/** The `p` (0..1) percentile of `values` by nearest rank; undefined for no values. */
export function percentile(values, p) {
  const xs = values.filter((x) => Number.isFinite(x)).sort((a, b) => a - b);
  if (xs.length === 0) return undefined;
  const rank = Math.ceil(p * xs.length);
  return xs[Math.min(xs.length - 1, Math.max(0, rank - 1))];
}

const round = (x, d = 3) => (x === undefined ? undefined : Math.round(x * 10 ** d) / 10 ** d);

/** n, p50, p95, p99, max and mean of `values` (finite ones only); `{ n: 0 }` for none. */
export function summarize(values, digits = 3) {
  const xs = values.filter((x) => Number.isFinite(x));
  if (xs.length === 0) return { n: 0 };
  const mean = xs.reduce((s, x) => s + x, 0) / xs.length;
  return {
    n: xs.length,
    p50: round(percentile(xs, 0.5), digits),
    p95: round(percentile(xs, 0.95), digits),
    p99: round(percentile(xs, 0.99), digits),
    max: round(Math.max(...xs), digits),
    mean: round(mean, digits),
  };
}

/**
 * The spread of one statistic over repeated runs: its median run, min and max. `runs` are
 * summaries (summarize); `key` names the statistic. Runs without it are left out, never zeroed.
 */
export function spread(runs, key) {
  const xs = runs.map((r) => r?.[key]).filter((x) => Number.isFinite(x));
  if (xs.length === 0) return undefined;
  return { median: percentile(xs, 0.5), min: Math.min(...xs), max: Math.max(...xs), n: xs.length };
}

/** A Chromium process's role from its command line: browser, renderer, gpu, utility or other. */
export function processRole(commandLine) {
  const m = /--type=([\w-]+)/.exec(commandLine ?? "");
  if (!m) return "browser";
  if (m[1] === "renderer") return "renderer";
  if (m[1] === "gpu-process") return "gpu";
  if (m[1] === "utility") return "utility";
  return m[1];
}

/** The pids in the tree under `root` (itself included), from `{pid, ppid}` rows. */
export function pidTree(procs, root) {
  const kids = new Map();
  for (const p of procs) {
    if (!kids.has(p.ppid)) kids.set(p.ppid, []);
    kids.get(p.ppid).push(p.pid);
  }
  const out = new Set();
  const stack = [root];
  while (stack.length) {
    const pid = stack.pop();
    if (out.has(pid) || !procs.some((p) => p.pid === pid)) continue;
    out.add(pid);
    for (const k of kids.get(pid) ?? []) stack.push(k);
  }
  return out;
}

/**
 * CPU share by role over consecutive samples of one process tree. Each sample is
 * `{ t: ms, procs: [{pid, ppid, cmd, cpuMs}] }` (cpuMs = user + kernel time so far). Between two
 * samples a process's share is its CPU time delta over the wall time, in percent of ONE core
 * (100 = a core busy); a process seen in only one of the two samples is skipped for that
 * interval. Returns per-role summaries over the intervals, and the tree total.
 */
export function treeCpu(samples, root) {
  const intervals = [];
  for (let k = 1; k < samples.length; k++) {
    const a = samples[k - 1];
    const b = samples[k];
    const wall = b.t - a.t;
    if (!(wall > 0)) continue;
    const tree = pidTree(b.procs, root);
    const before = new Map(a.procs.map((p) => [p.pid, p]));
    const byRole = {};
    let total = 0;
    for (const p of b.procs) {
      if (!tree.has(p.pid)) continue;
      const prev = before.get(p.pid);
      if (!prev) continue;
      const pct = (100 * Math.max(0, p.cpuMs - prev.cpuMs)) / wall;
      const role = processRole(p.cmd);
      byRole[role] = (byRole[role] ?? 0) + pct;
      total += pct;
    }
    intervals.push({ ...byRole, total });
  }
  const roles = new Set(intervals.flatMap((i) => Object.keys(i)));
  const out = { intervals: intervals.length };
  for (const r of roles) out[r] = summarize(intervals.map((i) => i[r] ?? 0), 1);
  return out;
}

/** Machine busy share, percent of all cores, between two os.cpus() snapshots. */
export function machineBusy(before, after) {
  let busy = 0;
  let all = 0;
  after.forEach((c, i) => {
    const p = before[i];
    if (!p) return;
    const d = (k) => c.times[k] - p.times[k];
    const idle = d("idle");
    const total = d("user") + d("nice") + d("sys") + d("irq") + idle;
    busy += total - idle;
    all += total;
  });
  return all > 0 ? (100 * busy) / all : undefined;
}

/** Fisher-Yates over `items` with `rand` (0..1): a fresh array. */
export function shuffle(items, rand = Math.random) {
  const a = [...items];
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(rand() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}

/** `first part after N ms` from `shadowrunr voice say`'s summary line, or undefined. */
export function firstAudioMs(output) {
  const m = /first part after (\d+) ms/.exec(output ?? "");
  return m ? Number(m[1]) : undefined;
}
