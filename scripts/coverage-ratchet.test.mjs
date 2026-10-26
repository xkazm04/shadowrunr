// SPDX-License-Identifier: Apache-2.0
// node --test scripts/coverage-ratchet.test.mjs
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const script = join(here, "coverage-ratchet.mjs");
const fx = join(here, "fixtures", "coverage");
const root = join(fx, "root");

function run(args, input) {
  const r = spawnSync(process.execPath, [script, ...args, "--root", root], { encoding: "utf8", input });
  return { code: r.status, out: r.stdout, err: r.stderr };
}
const check = (report) => run(["--check", join(fx, report), "--baseline", join(fx, "baseline.json")]);

test("passes at baseline and says how many crates it checked", () => {
  const r = check("report-ok.json");
  assert.equal(r.code, 0, r.err);
  assert.match(r.out, /2 crates checked, 0 problems/);
});

test("reads the report from stdin", () => {
  const r = run(["--check", "--baseline", join(fx, "baseline.json")], readFileSync(join(fx, "report-ok.json")));
  assert.equal(r.code, 0, r.err);
});

test("fails on a crate below its baseline", () => {
  const r = check("report-low.json");
  assert.equal(r.code, 1);
  assert.match(r.err, /below baseline: alpha 70% < 80%/);
});

test("fails on a baseline crate missing from the report", () => {
  const r = check("report-missing.json");
  assert.equal(r.code, 1);
  assert.match(r.err, /missing from report: beta/);
});

test("fails on an empty report", () => {
  const r = check("report-empty.json");
  assert.equal(r.code, 1);
  assert.match(r.err, /zero crates/);
});

test("fails on a workspace member with no baseline entry", () => {
  const dir = mkdtempSync(join(tmpdir(), "cov-ratchet-"));
  try {
    const base = join(dir, "baseline.json");
    run(["--write", join(fx, "report-missing.json"), "--baseline", base]);
    const r = run(["--check", join(fx, "report-ok.json"), "--baseline", base]);
    assert.equal(r.code, 1);
    assert.match(r.err, /no baseline entry: beta/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("--write round-trips: the written baseline equals the fixture and passes --check", () => {
  const dir = mkdtempSync(join(tmpdir(), "cov-ratchet-"));
  try {
    const base = join(dir, "baseline.json");
    const w = run(["--write", join(fx, "report-ok.json"), "--baseline", base]);
    assert.equal(w.code, 0, w.err);
    const written = JSON.parse(readFileSync(base, "utf8"));
    const fixture = JSON.parse(readFileSync(join(fx, "baseline.json"), "utf8"));
    assert.deepEqual(written.crates, fixture.crates);
    assert.equal(written.tolerance, 0.1);
    assert.equal(run(["--check", join(fx, "report-ok.json"), "--baseline", base]).code, 0);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

const shellBase = join(fx, "baseline-shell.json");
const checkShell = (shellReport) =>
  run(["--check", join(fx, "report-ok.json"), "--baseline", shellBase, "--shell-report", shellReport]);

test("src-tauri shell: passes at its baseline and counts the shell", () => {
  const r = checkShell(join(fx, "report-shell-ok.json"));
  assert.equal(r.code, 0, r.err);
  assert.match(r.out, /2 crates checked, 1 shells checked, 0 problems/);
});

test("src-tauri shell: fails when lowered below its baseline", () => {
  const r = checkShell(join(fx, "report-shell-low.json"));
  assert.equal(r.code, 1);
  assert.match(r.err, /below baseline: console-src-tauri 70% < 80%/);
});

test("src-tauri shell: fails on a missing report file", () => {
  const r = checkShell(join(fx, "no-such-report.json"));
  assert.equal(r.code, 1);
  assert.match(r.err, /shell report missing/);
});

test("src-tauri shell: fails on a report with zero lines", () => {
  const r = checkShell(join(fx, "report-shell-empty.json"));
  assert.equal(r.code, 1);
  assert.match(r.err, /zero lines: console-src-tauri/);
});

test("src-tauri shell: fails when no shell report is given", () => {
  const r = run(["--check", join(fx, "report-ok.json"), "--baseline", shellBase]);
  assert.equal(r.code, 1);
  assert.match(r.err, /no --shell-report/);
});
