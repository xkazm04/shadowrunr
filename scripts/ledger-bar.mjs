#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// ledger-bar: turn the "how many bar rules" sentence in docs/tuning/LEDGER.md into a check. Every
// [[red_lines]] table in sim/rubric.toml is a 100% bar, so N is counted from the rubric, never kept as
// a constant. Rows are append-only, so the rule starts at the anchor row (the first row whose gap cell
// begins "Correction (audit)"); each later row whose last cell starts "kept" must state at least one
// bar-count claim (`**a/b**`, "red lines a/b" or "bar rules a/b") and every claim must have b == N
// and a <= b. Other fractions ("368/368 lib tests") are ignored.
//
// Usage: node scripts/ledger-bar.mjs --check [--root <dir>] [--no-anchor]
//        --no-anchor checks every kept row from the top; it exists only for the test that proves the
//        check would have caught the real "red lines 9/9" defect.

import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, resolve } from "node:path";

const ANCHOR = /^Correction \(audit\)/;
const CLAIMS = [/\*\*(\d+)\/(\d+)\*\*/g, /red lines (\d+)\/(\d+)/gi, /bar rules (\d+)\/(\d+)/gi];

/** N: the number of [[red_lines]] tables in the rubric text. */
export function countRedLines(toml) {
  return (toml.match(/^\s*\[\[red_lines\]\]/gm) ?? []).length;
}

/** Markdown table body rows as { line, cells }, header and separator rows dropped. */
export function parseRows(md) {
  const rows = [];
  md.split(/\r?\n/).forEach((text, i) => {
    const t = text.trim();
    if (!t.startsWith("|")) return;
    const cells = t.replace(/^\|/, "").replace(/\|$/, "").split(/(?<!\\)\|/).map((c) => c.trim());
    if (cells.every((c) => /^:?-+:?$/.test(c)) || cells[0] === "Date") return;
    rows.push({ line: i + 1, cells });
  });
  return rows;
}

/** Bar-count claims in a row's text: [{ text, a, b }]. */
export function claimsIn(text) {
  const out = [];
  for (const re of CLAIMS) {
    for (const m of text.matchAll(re)) out.push({ text: m[0], a: Number(m[1]), b: Number(m[2]) });
  }
  return out;
}

/** Check a ledger against N. Returns { anchorLine, checked, failures: [{ line, message }] }. */
export function checkLedger(md, n, { useAnchor = true } = {}) {
  const rows = parseRows(md);
  const failures = [];
  if (rows.length === 0) return { anchorLine: null, checked: 0, failures: [{ line: 0, message: "no ledger table rows parsed" }] };
  let start = 0;
  let anchorLine = null;
  if (useAnchor) {
    const idx = rows.findIndex((r) => ANCHOR.test(r.cells[1] ?? ""));
    if (idx < 0) return { anchorLine: null, checked: 0, failures: [{ line: 0, message: "no anchor row (gap cell starting 'Correction (audit)')" }] };
    anchorLine = rows[idx].line;
    start = idx + 1;
  }
  let checked = 0;
  for (const row of rows.slice(start)) {
    if (!/^kept/i.test(row.cells[row.cells.length - 1] ?? "")) continue;
    checked++;
    const claims = claimsIn(row.cells.join(" | "));
    if (claims.length === 0) {
      failures.push({ line: row.line, message: `kept row states no bar count (expected **a/${n}**)` });
      continue;
    }
    for (const c of claims) {
      if (c.b !== n) failures.push({ line: row.line, message: `${c.text} has denominator ${c.b}, the rubric has ${n} bar rules` });
      else if (c.a > c.b) failures.push({ line: row.line, message: `${c.text} has a numerator above its denominator` });
    }
  }
  return { anchorLine, checked, failures };
}

function main(argv) {
  if (!argv.includes("--check")) {
    console.error("ledger-bar: --check is required");
    return 2;
  }
  const ri = argv.indexOf("--root");
  const root = resolve(ri >= 0 ? argv[ri + 1] : fileURLToPath(new URL("..", import.meta.url)));
  const rubric = join(root, "sim/rubric.toml");
  const ledger = join(root, "docs/tuning/LEDGER.md");
  if (!existsSync(rubric)) return fail(`${rubric} is missing`);
  if (!existsSync(ledger)) return fail(`${ledger} is missing`);
  const n = countRedLines(readFileSync(rubric, "utf8"));
  if (n === 0) return fail("sim/rubric.toml has no [[red_lines]] tables");
  const r = checkLedger(readFileSync(ledger, "utf8"), n, { useAnchor: !argv.includes("--no-anchor") });
  console.log(`ledger-bar: N=${n} bar rules, anchor line ${r.anchorLine ?? "none"}, ${r.checked} kept rows checked`);
  for (const f of r.failures) console.error(`ledger-bar: FAIL line ${f.line}: ${f.message}`);
  return r.failures.length ? 1 : 0;
}

function fail(msg) {
  console.error(`ledger-bar: ${msg}`);
  return 1;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
