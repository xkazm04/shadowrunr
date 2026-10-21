#!/usr/bin/env node
// Bundle budget per route: a deterministic byte count of the built chunks (never a timing).
//   node scripts/check-bundle.mjs     after `vite build`; exit 1 over budget, 2 when nothing was walked
//
// From dist/.vite/manifest.json:
//   entry          the index.html chunk plus everything it imports statically (first load)
//   <page>.l1/.l2  each page layer's lazy chunk plus what it imports beyond the entry
//   door           @shadowrunr/contract's door with its eager validators (ServerFrame + ErrorEnvelope),
//                  loaded in parallel with the first paint; other lazy chunks are named in OTHER_ROUTES
//   door.result    each method result validator chunk (generated/results/<Def>.js), loaded on that
//                  method's first reply; its line is a PER-CHUNK max, so the largest one is gated
// Budgets live in budgets/bundle.json. A route without a budget, or a budget without a route,
// fails too, so a new page has to declare its budget.
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const APP = join(dirname(fileURLToPath(import.meta.url)), "..");
const DIST = join(APP, "dist");
const MANIFEST = join(DIST, ".vite", "manifest.json");
const BUDGETS = JSON.parse(readFileSync(join(APP, "budgets", "bundle.json"), "utf8"));

if (!existsSync(MANIFEST)) {
  console.error("check-bundle: no dist/.vite/manifest.json (run vite build with build.manifest)");
  process.exit(2);
}
const manifest = JSON.parse(readFileSync(MANIFEST, "utf8"));

/** Lazy chunks that are not page layers. */
const OTHER_ROUTES = [
  [/^src\/data\/contractDoor\.ts$/, "door"],
  [/(^|\/)contract\/src\/generated\/results\/\w+\.js$/, "door.result"],
];
/** Routes made of many chunks, each held to the line on its own: the largest one is reported. */
const PER_CHUNK = new Set(["door.result"]);

const size = (file) => readFileSync(join(DIST, file)).length;
const gz = (file) => gzipSync(readFileSync(join(DIST, file)), { level: 9 }).length;

/** The chunk keys reachable from `key` through static imports. */
function closure(key, seen = new Set()) {
  if (seen.has(key)) return seen;
  seen.add(key);
  for (const dep of manifest[key]?.imports ?? []) closure(dep, seen);
  return seen;
}

function measure(keys, exclude = new Set()) {
  let js = 0;
  let css = 0;
  let gzip = 0;
  const cssFiles = new Set();
  for (const k of keys) {
    if (exclude.has(k)) continue;
    const c = manifest[k];
    js += size(c.file);
    gzip += gz(c.file);
    for (const f of c.css ?? []) cssFiles.add(f);
  }
  for (const f of cssFiles) css += size(f);
  return { js, css, gzip };
}

const entryKey = Object.keys(manifest).find((k) => manifest[k].isEntry);
if (!entryKey) {
  console.error("check-bundle: the manifest has no entry chunk");
  process.exit(2);
}
const entryKeys = closure(entryKey);
const measured = { entry: measure(entryKeys) };

for (const [key, chunk] of Object.entries(manifest)) {
  if (!chunk.isDynamicEntry) continue;
  const page = /^src\/pages\/([a-z]+)\/Layer([12])\.tsx$/.exec(key);
  const name = page ? `${page[1]}.l${page[2]}` : (OTHER_ROUTES.find(([re]) => re.test(key))?.[1] ?? null);
  if (name === null) {
    console.error(`check-bundle: dynamic chunk ${key} is not a known route; name it in check-bundle.mjs`);
    process.exit(1);
  }
  const m = measure(closure(key), entryKeys);
  if (!PER_CHUNK.has(name)) {
    measured[name] = m;
  } else {
    const prev = measured[name];
    const count = (prev?.count ?? 0) + 1;
    measured[name] = !prev || m.js > prev.js ? { ...m, count, largest: key } : { ...prev, count };
  }
}

const failures = [];
const kb = (n) => `${(n / 1024).toFixed(1)} KiB`;
console.log("route".padEnd(16), "js".padStart(11), "budget".padStart(11), "css".padStart(10), "gzip js".padStart(10));
for (const [name, m] of Object.entries(measured)) {
  const budget = BUDGETS.routes[name];
  if (!budget) {
    failures.push(`${name}: no budget in budgets/bundle.json`);
    continue;
  }
  const of = m.largest ? `  largest of ${m.count}: ${m.largest.split("/").pop()}` : "";
  console.log(name.padEnd(16), kb(m.js).padStart(11), kb(budget.js).padStart(11), kb(m.css).padStart(10), kb(m.gzip).padStart(10) + of);
  if (m.js > budget.js) failures.push(`${name}: js ${m.js} B > budget ${budget.js} B${m.largest ? ` (${m.largest})` : ""}`);
  if (budget.css !== undefined && m.css > budget.css) failures.push(`${name}: css ${m.css} B > budget ${budget.css} B`);
}
for (const name of Object.keys(BUDGETS.routes)) {
  if (!(name in measured)) failures.push(`${name}: budgeted but no such route was built`);
}

console.log(`check-bundle: ${Object.keys(measured).length} routes measured, ${failures.length} over or unbudgeted`);
for (const f of failures) console.error(`  ${f}`);
process.exit(failures.length > 0 ? 1 : 0);
