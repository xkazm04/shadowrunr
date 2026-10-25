#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// coverage-ratchet-js: per-package line coverage of the pnpm workspace may only go up.
//
// Input is one vitest json-summary report per workspace package: <package>/coverage/coverage-summary.json,
// as written by `vitest run --coverage --coverage.provider=v8 --coverage.reporter=json-summary` run in
// that package. The package list comes from pnpm-workspace.yaml (apps/*, packages/*; a directory counts
// when it holds a package.json) and a package is named by its directory. Its percent is `total.lines`
// covered over total, rounded to 2 places. coverage/baseline-js.json maps package name to that percent.
// It is a sibling of the Rust baseline on purpose: `coverage-ratchet.mjs --write` rewrites
// coverage/baseline.json whole, so a `packages` section in it would be erased by the next Rust write.
//
// --check  exits 1 when a package is below its baseline by more than TOLERANCE percentage points,
//          when a workspace package has no baseline entry, when a baseline package has no report,
//          when a report holds zero lines, or when zero packages were checked. It always prints how
//          many packages it checked.
// --write  regenerates the baseline from the reports. Lowering it is a reviewed diff, by hand.
//
// Usage: node scripts/coverage-ratchet-js.mjs (--check | --write) [--root <dir>] [--baseline <file>]

import { existsSync, readFileSync, readdirSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/** Percentage points a package may sit below its baseline before --check fails. */
export const TOLERANCE = 0.1;
export const COMMAND = "vitest run --coverage --coverage.provider=v8 --coverage.reporter=json-summary";

const round2 = (n) => Math.round(n * 100) / 100;

/** The entries of the `packages:` list in pnpm-workspace.yaml. */
export function workspaceGlobs(text) {
  const m = text.match(/^packages:[ \t]*\r?\n((?:[ \t]*-[^\n]*\n?)+)/m);
  if (!m) return [];
  return [...m[1].matchAll(/-\s*["']?([^"'\s#]+)["']?/g)].map((x) => x[1].replace(/\/$/, ""));
}

/** Workspace package dirs (repo-relative, posix) for `dir/*` globs; a package has a package.json. */
export function workspacePackages(root, globs) {
  const dirs = [];
  for (const g of globs) {
    if (!g.endsWith("/*")) continue;
    const parent = g.slice(0, -2);
    if (!existsSync(join(root, parent))) continue;
    for (const e of readdirSync(join(root, parent), { withFileTypes: true })) {
      if (e.isDirectory() && existsSync(join(root, parent, e.name, "package.json"))) {
        dirs.push(`${parent}/${e.name}`);
      }
    }
  }
  return dirs.sort();
}

/** Package name -> { count, covered } line totals, for packages that have a report. */
export function packageTotals(root, dirs) {
  const totals = new Map();
  for (const dir of dirs) {
    const file = join(root, dir, "coverage", "coverage-summary.json");
    if (!existsSync(file)) continue;
    const lines = JSON.parse(readFileSync(file, "utf8")).total?.lines;
    totals.set(dir.split("/").pop(), { count: lines?.total ?? 0, covered: lines?.covered ?? 0 });
  }
  return totals;
}

export const percentOf = (t) => (t.count === 0 ? 0 : round2((t.covered / t.count) * 100));

/** Compare totals with a baseline; returns the problems and how many packages were checked. */
export function compare(totals, baseline, dirs) {
  const problems = [];
  const base = baseline.packages ?? {};
  const names = dirs.map((d) => d.split("/").pop());
  if (names.length === 0) problems.push("workspace holds zero packages");
  for (const name of names) {
    if (!(name in base)) problems.push(`no baseline entry: ${name}`);
  }
  for (const name of Object.keys(base)) {
    if (!totals.has(name)) problems.push(`missing from report: ${name}`);
  }
  for (const [name, t] of totals) {
    if (t.count === 0) {
      problems.push(`report holds zero lines: ${name}`);
      continue;
    }
    if (!(name in base)) continue;
    const now = percentOf(t);
    if (now < base[name] - TOLERANCE) {
      problems.push(`below baseline: ${name} ${now}% < ${base[name]}% (tolerance ${TOLERANCE})`);
    }
  }
  if (totals.size === 0) problems.push("zero packages checked");
  return { problems, checked: totals.size };
}

export function makeBaseline(totals) {
  const packages = {};
  for (const name of [...totals.keys()].sort()) packages[name] = percentOf(totals.get(name));
  return { command: COMMAND, tolerance: TOLERANCE, packages };
}

function main(argv) {
  const arg = (name) => {
    const i = argv.indexOf(name);
    return i >= 0 ? argv[i + 1] : undefined;
  };
  const here = dirname(fileURLToPath(import.meta.url));
  const root = resolve(arg("--root") ?? join(here, ".."));
  const baselineFile = resolve(arg("--baseline") ?? join(root, "coverage", "baseline-js.json"));
  const globs = workspaceGlobs(readFileSync(join(root, "pnpm-workspace.yaml"), "utf8"));
  const dirs = workspacePackages(root, globs);
  const totals = packageTotals(root, dirs);

  if (argv.includes("--write")) {
    const zero = [...totals].filter(([, t]) => t.count === 0).map(([n]) => n);
    if (totals.size === 0 || zero.length > 0) {
      console.error(`refusing to write: ${totals.size === 0 ? "no reports" : `zero lines in ${zero.join(", ")}`}`);
      return 1;
    }
    mkdirSync(dirname(baselineFile), { recursive: true });
    writeFileSync(baselineFile, `${JSON.stringify(makeBaseline(totals), null, 2)}\n`);
    console.log(`wrote ${totals.size} packages to ${baselineFile}`);
    return 0;
  }
  if (!argv.includes("--check")) {
    console.error("usage: coverage-ratchet-js.mjs (--check | --write) [--root <dir>] [--baseline <file>]");
    return 2;
  }
  if (!existsSync(baselineFile)) {
    console.error(`no baseline: ${baselineFile}`);
    return 1;
  }
  const { problems, checked } = compare(totals, JSON.parse(readFileSync(baselineFile, "utf8")), dirs);
  for (const p of problems) console.error(p);
  console.log(`${checked} packages checked, ${problems.length} problems`);
  return problems.length > 0 ? 1 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
