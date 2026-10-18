#!/usr/bin/env node
// File-size gate: no source file in apps/console/src over 400 lines.
//   node scripts/check-size.mjs            exit 1 on a violation, 2 when it walked nothing
// No file is exempt.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const LIMIT = 400;
const APP = join(dirname(fileURLToPath(import.meta.url)), "..");
const SRC = join(APP, "src");
const EXT = /\.(ts|tsx|css)$/;
function walk(dir) {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    return statSync(p).isDirectory() ? walk(p) : [p];
  });
}

let walked = 0;
const over = [];
for (const file of walk(SRC)) {
  if (!EXT.test(file)) continue;
  const rel = relative(SRC, file).split(sep).join("/");
  walked++;
  const lines = readFileSync(file, "utf8").split("\n").length;
  if (lines > LIMIT) over.push([rel, lines]);
}

console.log(`check-size: walked ${walked}, limit ${LIMIT} lines`);
if (walked === 0) {
  console.error("check-size: walked nothing");
  process.exit(2);
}
for (const [rel, lines] of over) console.error(`  ${rel}: ${lines} lines (> ${LIMIT})`);
process.exit(over.length > 0 ? 1 : 0);
