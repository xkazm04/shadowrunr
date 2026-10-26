#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// coverage-ratchet: per-crate line coverage of the root cargo workspace may only go up.
//
// Input is the report of `cargo llvm-cov --workspace --json --summary-only` (a file path, or
// stdin when none is given). Each file in it is attributed to the workspace member whose
// directory holds it, and a crate's percent is covered lines over all lines of its files (not an
// average of per-file percents). coverage/baseline.json maps crate name to that percent and
// records the command and the toolchain that produced it.
//
// --check  exits 1 when a crate is below its baseline by more than TOLERANCE percentage points,
//          when a workspace member has no baseline entry, when a baseline crate is missing from
//          the report, or when the report holds zero crates or zero lines. It always prints how
//          many crates it checked.
// A shell outside the workspace (apps/console/src-tauri has its own Cargo workspace) is measured
// by its own `cargo llvm-cov --manifest-path` run and passed as --shell-report <file>. Its percent
// sits under `shells` in the baseline. A baseline with shells fails --check without that report,
// and a shell report that is missing, unreadable or holds zero lines fails too.
//
// --write  regenerates the baseline from the report. Lowering it is a reviewed diff, by hand.
//
// Usage: node scripts/coverage-ratchet.mjs (--check | --write) [report.json] [--root <dir>]
//        [--baseline <file>] [--shell-report <file>]

import { existsSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/** Percentage points a crate may sit below its baseline before --check fails (llvm noise). */
export const TOLERANCE = 0.1;
export const COMMAND = "cargo llvm-cov --workspace --json --summary-only";

/** Shell name -> repo-relative dir of a Rust crate that lives outside the root workspace. */
export const SHELLS = { "console-src-tauri": "apps/console/src-tauri" };
export const SHELL_COMMAND = "cargo llvm-cov --manifest-path apps/console/src-tauri/Cargo.toml --json --summary-only";

const posix = (p) => p.split("\\").join("/");
const round2 = (n) => Math.round(n * 100) / 100;

/** The `members = [...]` of the root Cargo.toml, as repo-relative posix dirs. */
export function workspaceMembers(text) {
  const m = text.match(/^members\s*=\s*\[([\s\S]*?)\]/m);
  if (!m) return [];
  return [...m[1].matchAll(/"([^"]+)"/g)].map((x) => posix(x[1]).replace(/\/$/, ""));
}

/** Crate name -> { count, covered } line totals, for files under a workspace member. */
export function crateTotals(report, members) {
  const totals = new Map();
  for (const entry of report.data ?? []) {
    for (const file of entry.files ?? []) {
      // Paths are absolute (and differ per checkout), so match the member dir anywhere in them.
      const name = `/${posix(file.filename ?? "")}`;
      const dir = members.find((d) => name.includes(`/${d}/`));
      if (!dir) continue;
      const crate = dir.split("/").pop();
      const t = totals.get(crate) ?? { count: 0, covered: 0 };
      t.count += file.summary?.lines?.count ?? 0;
      t.covered += file.summary?.lines?.covered ?? 0;
      totals.set(crate, t);
    }
  }
  return totals;
}

export const percentOf = (t) => (t.count === 0 ? 100 : round2((t.covered / t.count) * 100));

/** Compare totals with a baseline; returns the problems and how many crates were checked. */
export function compare(totals, baseline, members) {
  const problems = [];
  const lines = [...totals.values()].reduce((n, t) => n + t.count, 0);
  if (totals.size === 0) problems.push("report holds zero crates");
  else if (lines === 0) problems.push("report holds zero lines");
  const base = baseline.crates ?? {};
  for (const dir of members) {
    const crate = dir.split("/").pop();
    if (!(crate in base)) problems.push(`no baseline entry: ${crate}`);
  }
  for (const crate of Object.keys(base)) {
    if (!totals.has(crate)) problems.push(`missing from report: ${crate}`);
  }
  for (const [crate, t] of totals) {
    if (!(crate in base)) continue;
    const now = percentOf(t);
    if (now < base[crate] - TOLERANCE) {
      problems.push(`below baseline: ${crate} ${now}% < ${base[crate]}% (tolerance ${TOLERANCE})`);
    }
  }
  return { problems, checked: totals.size };
}

/** Shell name -> { count, covered } line totals for files under each shell's src/. */
export function shellTotals(report) {
  const totals = new Map();
  for (const [name, dir] of Object.entries(SHELLS)) {
    const t = { count: 0, covered: 0 };
    for (const entry of report.data ?? []) {
      for (const file of entry.files ?? []) {
        if (!`/${posix(file.filename ?? "")}`.includes(`/${dir}/src/`)) continue;
        t.count += file.summary?.lines?.count ?? 0;
        t.covered += file.summary?.lines?.covered ?? 0;
      }
    }
    totals.set(name, t);
  }
  return totals;
}

/** Compare a shell report with the baseline's `shells`; a shell with zero lines is a failure. */
export function compareShells(totals, baseline) {
  const problems = [];
  const base = baseline.shells ?? {};
  for (const [name, floor] of Object.entries(base)) {
    const t = totals?.get(name);
    if (!t || t.count === 0) {
      problems.push(`shell report holds zero lines: ${name}`);
      continue;
    }
    const now = percentOf(t);
    if (now < floor - TOLERANCE) {
      problems.push(`below baseline: ${name} ${now}% < ${floor}% (tolerance ${TOLERANCE})`);
    }
  }
  return { problems, checked: Object.keys(base).length };
}

export function makeBaseline(totals, toolchain, shells) {
  const crates = {};
  for (const crate of [...totals.keys()].sort()) crates[crate] = percentOf(totals.get(crate));
  const out = { command: COMMAND, toolchain, tolerance: TOLERANCE, crates };
  if (shells && Object.keys(shells).length > 0) {
    out.shellCommand = SHELL_COMMAND;
    out.shells = shells;
  }
  return out;
}

function main(argv) {
  const arg = (name) => {
    const i = argv.indexOf(name);
    return i >= 0 ? argv[i + 1] : undefined;
  };
  const here = dirname(fileURLToPath(import.meta.url));
  const root = resolve(arg("--root") ?? join(here, ".."));
  const baselineFile = resolve(arg("--baseline") ?? join(root, "coverage", "baseline.json"));
  const skip = new Set(["--root", "--baseline", "--shell-report"]);
  const shellReport = arg("--shell-report");
  const positional = argv.filter((a, i) => !a.startsWith("--") && !skip.has(argv[i - 1]));
  const raw = readFileSync(positional[0] ?? 0, "utf8");
  const report = JSON.parse(raw);
  const members = workspaceMembers(readFileSync(join(root, "Cargo.toml"), "utf8"));
  const totals = crateTotals(report, members);

  if (argv.includes("--write")) {
    const toolchain = process.env.RUST_TOOLCHAIN ?? arg("--toolchain") ?? "unknown";
    let shells = existsSync(baselineFile) ? JSON.parse(readFileSync(baselineFile, "utf8")).shells : undefined;
    if (shellReport) {
      shells = {};
      for (const [name, t] of shellTotals(JSON.parse(readFileSync(shellReport, "utf8")))) {
        if (t.count === 0) {
          console.error(`shell report holds zero lines: ${name}`);
          return 1;
        }
        shells[name] = percentOf(t);
      }
    }
    mkdirSync(dirname(baselineFile), { recursive: true });
    writeFileSync(baselineFile, `${JSON.stringify(makeBaseline(totals, toolchain), null, 2)}\n`);
    console.log(`wrote ${totals.size} crates to ${baselineFile}`);
    return totals.size > 0 ? 0 : 1;
  }
  if (!argv.includes("--check")) {
    console.error("usage: coverage-ratchet.mjs (--check | --write) [report.json]");
    return 2;
  }
  if (!existsSync(baselineFile)) {
    console.error(`no baseline: ${baselineFile}`);
    return 1;
  }
  const baseline = JSON.parse(readFileSync(baselineFile, "utf8"));
  const { problems, checked } = compare(totals, baseline, members);
  let shellsChecked = 0;
  if (baseline.shells && Object.keys(baseline.shells).length > 0) {
    if (!shellReport) problems.push("no --shell-report given but the baseline has shells");
    else if (!existsSync(shellReport)) problems.push(`shell report missing: ${shellReport}`);
    else {
      const r = compareShells(shellTotals(JSON.parse(readFileSync(shellReport, "utf8"))), baseline);
      problems.push(...r.problems);
      shellsChecked = r.checked;
    }
  }
  for (const p of problems) console.error(p);
  const shellNote = shellsChecked > 0 ? `, ${shellsChecked} shells checked` : "";
  console.log(`${checked} crates checked${shellNote}, ${problems.length} problems`);
  return problems.length > 0 ? 1 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
