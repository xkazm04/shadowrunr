// SPDX-License-Identifier: Apache-2.0
// node --test scripts/coverage-ratchet-js.test.mjs
// Fixtures under scripts/fixtures/coverage-js/ are generated and labelled simulated.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const script = join(here, "coverage-ratchet-js.mjs");
const fx = join(here, "fixtures", "coverage-js");

function run(args) {
  const r = spawnSync(process.execPath, [script, ...args], { encoding: "utf8" });
  return { code: r.status, out: r.stdout, err: r.stderr };
}
const check = (root, baseline = "baseline.json") =>
  run(["--check", "--root", join(fx, root), "--baseline", join(fx, baseline)]);

test("passes at baseline and says how many packages it checked", () => {
  const r = check("ok");
  assert.equal(r.code, 0, r.err);
  assert.match(r.out, /2 packages checked, 0 problems/);
});

test("fails on a package below its baseline", () => {
  const r = check("low");
  assert.equal(r.code, 1);
  assert.match(r.err, /below baseline: alpha 70% < 80%/);
});

test("fails on a workspace package with no baseline entry", () => {
  const r = check("ok", "baseline-alpha-only.json");
  assert.equal(r.code, 1);
  assert.match(r.err, /no baseline entry: beta/);
});

test("fails on a baseline package missing from the reports", () => {
  const r = check("ok", "baseline-with-gamma.json");
  assert.equal(r.code, 1);
  assert.match(r.err, /missing from report: gamma/);
  const n = check("no-report");
  assert.equal(n.code, 1);
  assert.match(n.err, /missing from report: beta/);
});

test("fails on a report holding zero lines", () => {
  const r = check("empty");
  assert.equal(r.code, 1);
  assert.match(r.err, /zero lines: alpha/);
});

test("fails on zero packages", () => {
  const r = check("none");
  assert.equal(r.code, 1);
  assert.match(r.err, /zero packages/);
  assert.match(r.out, /0 packages checked/);
});

test("an empty baseline fails loudly", () => {
  const r = check("ok", "baseline-empty.json");
  assert.equal(r.code, 1);
  assert.match(r.err, /no baseline entry: alpha/);
});

test("fails when the baseline file is absent", () => {
  assert.equal(check("ok", "nope.json").code, 1);
});

test("--write round-trips: the written baseline equals the fixture and passes --check", () => {
  const dir = mkdtempSync(join(tmpdir(), "cov-ratchet-js-"));
  try {
    const base = join(dir, "baseline-js.json");
    const w = run(["--write", "--root", join(fx, "ok"), "--baseline", base]);
    assert.equal(w.code, 0, w.err);
    const written = JSON.parse(readFileSync(base, "utf8"));
    assert.deepEqual(written.packages, { alpha: 80, beta: 50 });
    assert.equal(written.tolerance, 0.1);
    assert.equal(run(["--check", "--root", join(fx, "ok"), "--baseline", base]).code, 0);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("--write refuses a report with zero lines", () => {
  const dir = mkdtempSync(join(tmpdir(), "cov-ratchet-js-"));
  try {
    const w = run(["--write", "--root", join(fx, "empty"), "--baseline", join(dir, "b.json")]);
    assert.equal(w.code, 1);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("the real workspace is read from pnpm-workspace.yaml", () => {
  const r = run(["--check", "--baseline", join(fx, "baseline-empty.json")]);
  for (const p of ["console", "stage", "contract", "stage-kit", "tokens"]) {
    assert.ok(r.err.split("\n").includes(`no baseline entry: ${p}`), p);
  }
});
