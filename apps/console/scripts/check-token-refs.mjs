#!/usr/bin/env node
// Token-reference gate: every `var(--sr-…)` the console's CSS uses must be defined by
// @shadowrunr/tokens. A renamed or removed token fails here instead of silently falling back.
// (F1's `generate.mjs --check` covers the other half: the generated CSS matches its source.)
//   node scripts/check-token-refs.mjs      exit 1 on an unknown token, 2 when it walked nothing
import { readdirSync, readFileSync, statSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const APP = join(dirname(fileURLToPath(import.meta.url)), "..");
const SRC = join(APP, "src");
const require = createRequire(join(APP, "package.json"));
const TOKENS_CSS = require.resolve("@shadowrunr/tokens/tokens.css");

const DEF = /(--sr-[a-z0-9-]+)\s*:/g;
const REF = /var\(\s*(--sr-[a-z0-9-]+)/g;

const defined = new Set([...readFileSync(TOKENS_CSS, "utf8").matchAll(DEF)].map((m) => m[1]));
if (defined.size === 0) {
  console.error(`check-token-refs: no --sr-* tokens defined in ${TOKENS_CSS}`);
  process.exit(2);
}

function walk(dir) {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    return statSync(p).isDirectory() ? walk(p) : [p];
  });
}

let files = 0;
let refs = 0;
const unknown = [];
for (const file of walk(SRC)) {
  if (!/\.(css|tsx?)$/.test(file)) continue;
  const rel = relative(SRC, file).split(sep).join("/");
  const text = readFileSync(file, "utf8");
  files++;
  for (const m of text.matchAll(REF)) {
    refs++;
    if (!defined.has(m[1])) unknown.push(`${rel}: ${m[1]}`);
  }
}

console.log(`check-token-refs: ${refs} references in ${files} files against ${defined.size} tokens`);
if (refs === 0) {
  console.error("check-token-refs: walked nothing");
  process.exit(2);
}
for (const u of unknown) console.error(`  unknown token ${u}`);
process.exit(unknown.length > 0 ? 1 : 0);
