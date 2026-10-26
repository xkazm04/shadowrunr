// SPDX-License-Identifier: Apache-2.0
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { countRedLines, claimsIn } from "./ledger-bar.mjs";

const script = fileURLToPath(new URL("./ledger-bar.mjs", import.meta.url));
const repo = fileURLToPath(new URL("..", import.meta.url));
const fixture = (name) => join(repo, "scripts/fixtures/ledger-bar", name);

function run(root, ...extra) {
  const r = spawnSync(process.execPath, [script, "--check", "--root", root, ...extra], { encoding: "utf8" });
  return { code: r.status, out: r.stdout, err: r.stderr };
}

test("a clean ledger passes and ignores other fractions", () => {
  const r = run(fixture("clean"));
  assert.equal(r.code, 0, r.err);
  assert.match(r.out, /N=3 bar rules, anchor line 6, 1 kept rows checked/);
});

test("a post-anchor kept row saying 9/9 fails, naming line and fraction", () => {
  const r = run(fixture("wrong-denominator"));
  assert.equal(r.code, 1);
  assert.match(r.err, /line 7: red lines 9\/9 has denominator 9/);
});

test("a post-anchor kept row with no claim fails", () => {
  const r = run(fixture("no-claim"));
  assert.equal(r.code, 1);
  assert.match(r.err, /line 7: kept row states no bar count/);
});

test("a missing anchor fails loudly", () => {
  const r = run(fixture("no-anchor"));
  assert.equal(r.code, 1);
  assert.match(r.err, /no anchor row/);
});

test("a rubric with zero [[red_lines]] tables fails", () => {
  const r = run(fixture("zero-tables"));
  assert.equal(r.code, 1);
  assert.match(r.err, /no \[\[red_lines\]\] tables/);
});

test("a missing rubric fails", () => {
  assert.equal(run(join(fixture("clean"), "docs")).code, 1);
});

test("a reverted row with no claim passes", () => {
  const r = run(fixture("reverted-no-claim"));
  assert.equal(r.code, 0, r.err);
});

test("claims: only bar-count forms count", () => {
  const got = claimsIn("**19/20** and red lines 9/9, bar rules 1/2, 368/368 lib tests, 0 of 51 rules");
  assert.deepEqual(got.map((c) => `${c.a}/${c.b}`), ["19/20", "9/9", "1/2"]);
  assert.equal(countRedLines("[[red_lines]]\n[[red_lines]]\n[[other]]\n"), 2);
});

test("real ledger: without the anchor the 'red lines 9/9' rows are flagged (base-red proof)", () => {
  const r = run(repo, "--no-anchor");
  assert.equal(r.code, 1);
  const rows = new Set(
    [...r.err.matchAll(/FAIL line (\d+): red lines 9\/9 has denominator 9/gi)].map((m) => m[1]),
  );
  assert.ok(rows.size >= 5, `flagged ${rows.size} rows: ${[...rows]}`);
});

test("real ledger passes as it is now", () => {
  const r = run(repo);
  assert.equal(r.code, 0, r.err);
});
