// The pure half of the bundle gate: measure each route from a vite manifest, attribute its
// bytes to chunks under hash-free names, diff against a committed measured baseline, and
// format the report. check-bundle.mjs is the CLI around it (file reads, exit codes).
//
// The verdict is budget-only (budgets/bundle.json). The baseline (budgets/bundle.measured.json)
// only explains a figure - which chunk grew since it was written - and never blocks, so a stale
// baseline cannot turn the build red.

/** Lazy chunks that are not page layers. */
export const OTHER_ROUTES = [
  [/^src\/data\/contractDoor\.ts$/, "door"],
  [/^src\/shell\/GoToPalette\.tsx$/, "goto"],
  [/^src\/pages\/characters\/moderation\/ModerationTab\.tsx$/, "characters.moderation"],
  [/(^|\/)contract\/src\/generated\/results\/\w+\.js$/, "door.result"],
];
/** Routes made of many chunks, each held to the line on its own: the largest one is reported. */
export const PER_CHUNK = new Set(["door.result"]);

export const BASELINE_HINT = "no baseline: run pnpm -C apps/console budget:bundle --write-baseline";

/**
 * A chunk's name without its content hash: vite names shared chunks `_<Name>-<8-char hash>.<ext>`,
 * so `_FilterTable-C7LN3YTt.js` and `_FilterTable-Zz9Qq1Aa.js` are the same chunk across builds.
 * Source keys (src/..., ../../packages/...) carry no hash and are returned unchanged.
 */
export function chunkKey(key) {
  const m = /^(_.*)-[\w-]{8}(\.\w+)$/.exec(key);
  return m ? `${m[1]}${m[2]}` : key;
}

/**
 * Hash-free names for every manifest key. Two distinct chunks whose stems collide
 * (`_index-AAAA.js`, `_index-BBBB.js`) stay two entries: each gets `~<n>` ordered by who imports
 * it (hash-free), then by key, so the name is stable while the import graph is.
 */
export function chunkNames(manifest) {
  const importers = new Map();
  for (const [k, c] of Object.entries(manifest)) {
    for (const dep of c.imports ?? []) {
      if (!importers.has(dep)) importers.set(dep, []);
      importers.get(dep).push(chunkKey(k));
    }
  }
  const byStem = new Map();
  for (const k of Object.keys(manifest)) {
    const stem = chunkKey(k);
    if (!byStem.has(stem)) byStem.set(stem, []);
    byStem.get(stem).push(k);
  }
  const names = new Map();
  for (const [stem, keys] of byStem) {
    if (keys.length === 1) {
      names.set(keys[0], stem);
      continue;
    }
    const sortKey = (k) => `${(importers.get(k) ?? []).sort().join(",")}\u0000${k}`;
    const ordered = [...keys].sort((a, b) => (sortKey(a) < sortKey(b) ? -1 : sortKey(a) > sortKey(b) ? 1 : 0));
    const dot = stem.lastIndexOf(".");
    ordered.forEach((k, i) => names.set(k, `${stem.slice(0, dot)}~${i + 1}${stem.slice(dot)}`));
  }
  return names;
}

/** The chunk keys reachable from `key` through static imports. */
export function closure(manifest, key, seen = new Set()) {
  if (seen.has(key)) return seen;
  seen.add(key);
  for (const dep of manifest[key]?.imports ?? []) closure(manifest, dep, seen);
  return seen;
}

/**
 * Every route's js/css/gzip plus its per-chunk js, keyed by hash-free chunk name.
 * `sizeOf(file)` and `gzipOf(file)` read the built files; gzip is reported, never gated.
 * Returns { routes } or { error, code } (code 2: nothing to walk, 1: an unnamed dynamic chunk).
 */
export function measureRoutes(manifest, sizeOf, gzipOf = () => 0) {
  const names = chunkNames(manifest);
  const measure = (keys, exclude = new Set()) => {
    let js = 0;
    let css = 0;
    let gzip = 0;
    const chunks = {};
    const cssFiles = new Set();
    for (const k of keys) {
      if (exclude.has(k)) continue;
      const c = manifest[k];
      const n = sizeOf(c.file);
      js += n;
      gzip += gzipOf(c.file);
      chunks[names.get(k)] = n;
      for (const f of c.css ?? []) cssFiles.add(f);
    }
    for (const f of cssFiles) css += sizeOf(f);
    return { js, css, gzip, chunks };
  };

  const entryKey = Object.keys(manifest).find((k) => manifest[k].isEntry);
  if (!entryKey) return { error: "check-bundle: the manifest has no entry chunk", code: 2 };
  const entryKeys = closure(manifest, entryKey);
  const routes = { entry: measure(entryKeys) };

  for (const [key, chunk] of Object.entries(manifest)) {
    if (!chunk.isDynamicEntry) continue;
    const page = /^src\/pages\/([a-z]+)\/Layer([12])\.tsx$/.exec(key);
    const name = page ? `${page[1]}.l${page[2]}` : (OTHER_ROUTES.find(([re]) => re.test(key))?.[1] ?? null);
    if (name === null) {
      return { error: `check-bundle: dynamic chunk ${key} is not a known route; name it in check-bundle.mjs`, code: 1 };
    }
    const m = measure(closure(manifest, key), entryKeys);
    if (!PER_CHUNK.has(name)) {
      routes[name] = m;
    } else {
      const prev = routes[name];
      const count = (prev?.count ?? 0) + 1;
      routes[name] = !prev || m.js > prev.js ? { ...m, count, largest: key } : { ...prev, count };
    }
  }
  return { routes };
}

/** The committed form of a measurement: js, css and per-chunk js per route (no gzip, no paths). */
export function baselineOf(routes) {
  const out = {};
  for (const [name, m] of Object.entries(routes)) {
    const chunks = Object.fromEntries(Object.entries(m.chunks).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)));
    out[name] = { js: m.js, css: m.css, chunks };
  }
  return out;
}

/**
 * Per route: the js delta against the baseline and the chunks that moved, sorted by |delta|
 * (unchanged chunks omitted; `new` = absent from the baseline, `gone` = absent now).
 * A route missing from the baseline gets { delta: null }.
 */
export function diffRoutes(baseline, measured) {
  const out = {};
  for (const [name, m] of Object.entries(measured)) {
    const b = baseline?.[name];
    if (!b) {
      out[name] = { delta: null, rows: [] };
      continue;
    }
    const rows = [];
    const before = b.chunks ?? {};
    const now = m.chunks ?? {};
    for (const [chunk, js] of Object.entries(now)) {
      if (!(chunk in before)) rows.push({ chunk, delta: js, new: true });
      else if (js !== before[chunk]) rows.push({ chunk, delta: js - before[chunk] });
    }
    for (const [chunk, js] of Object.entries(before)) {
      if (!(chunk in now)) rows.push({ chunk, delta: -js, gone: true });
    }
    rows.sort((x, y) => Math.abs(y.delta) - Math.abs(x.delta) || (x.chunk < y.chunk ? -1 : 1));
    out[name] = { delta: m.js - b.js, rows };
  }
  return out;
}

const kb = (n) => `${(n / 1024).toFixed(1)} KiB`;
const signed = (n) => (n > 0 ? `+${n}` : `${n}`);
const row = (r) => `${r.chunk} ${signed(r.delta)} B${r.new ? " (new)" : r.gone ? " (gone)" : ""}`;

/**
 * The gate's report and verdict. `baseline` is the routes object of bundle.measured.json, or null.
 * Returns { out: string[] (stdout), err: string[] (stderr), code }. The verdict (code) depends
 * on budgets and measured figures only.
 */
export function report(routes, budgets, baseline) {
  const out = [];
  const err = [];
  const failures = [];
  const diff = baseline ? diffRoutes(baseline, routes) : null;
  out.push(
    ["route".padEnd(16), "js".padStart(11), "budget".padStart(11), "slack".padStart(10), "css".padStart(10), "gzip js".padStart(10), "delta".padStart(9)].join(" "),
  );
  for (const [name, m] of Object.entries(routes)) {
    const budget = budgets[name];
    if (!budget) {
      failures.push([`${name}: no budget in budgets/bundle.json`]);
      continue;
    }
    const d = diff?.[name];
    const delta = !diff ? "" : d.delta === null ? "new" : `${signed(d.delta)} B`;
    const of = m.largest ? `  largest of ${m.count}: ${m.largest.split("/").pop()}` : "";
    out.push(
      [name.padEnd(16), kb(m.js).padStart(11), kb(budget.js).padStart(11), kb(budget.js - m.js).padStart(10), kb(m.css).padStart(10), kb(m.gzip).padStart(10), delta.padStart(9)].join(" ") + of,
    );
    if (m.js > budget.js) {
      const lines = [`${name}: js ${m.js} B > budget ${budget.js} B${m.largest ? ` (${m.largest})` : ""}`];
      if (d && d.delta !== null) {
        const top = d.rows.slice(0, 3).map(row);
        lines.push(`  grew ${signed(d.delta)} B vs bundle.measured.json: ${top.length > 0 ? top.join(", ") : "no chunk moved"}`);
      } else if (diff) {
        lines.push(`  not in bundle.measured.json: ${Object.entries(m.chunks).sort(([, a], [, b]) => b - a).slice(0, 3).map(([c, n]) => `${c} ${n} B`).join(", ")}`);
      }
      lines.push(`  slack ${budget.js - m.js} B (budget ${budget.js} - measured ${m.js})`);
      failures.push(lines);
    }
    if (budget.css !== undefined && m.css > budget.css) failures.push([`${name}: css ${m.css} B > budget ${budget.css} B`]);
  }
  for (const name of Object.keys(budgets)) {
    if (!(name in routes)) failures.push([`${name}: budgeted but no such route was built`]);
  }
  out.push(`check-bundle: ${Object.keys(routes).length} routes measured, ${failures.length} over or unbudgeted`);
  if (!baseline) out.push(BASELINE_HINT);
  for (const f of failures) for (const line of f) err.push(`  ${line}`);
  return { out, err, code: failures.length > 0 ? 1 : 0 };
}
