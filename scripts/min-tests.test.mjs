// SPDX-License-Identifier: Apache-2.0
// node --test scripts/min-tests.test.mjs
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { countTests, verdict } from "./min-tests.mjs";

const script = join(dirname(fileURLToPath(import.meta.url)), "min-tests.mjs");
const run = (args, input) => spawnSync(process.execPath, [script, ...args], { encoding: "utf8", input });

const CARGO_ZERO = "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.00s\n";
const CARGO_TWO = "test result: ok. 5 passed; 0 failed; 0 ignored;\ntest result: ok. 3 passed; 0 failed; 0 ignored;\n";

test("counts cargo summaries across crates and python unittest runs", () => {
  assert.equal(countTests(CARGO_ZERO), 0);
  assert.equal(countTests(CARGO_TWO), 8);
  assert.equal(countTests("Ran 12 tests in 0.5s\n\nOK\n"), 12);
  assert.equal(countTests("Ran 0 tests in 0.000s\n\nNO TESTS RAN\n"), 0);
});

test("verdict: a green command with zero tests is red; a red command stays red", () => {
  assert.equal(verdict(0, 0, 1), 1);
  assert.equal(verdict(0, 8, 8), 0);
  assert.equal(verdict(0, 7, 8), 1);
  assert.equal(verdict(101, 99, 1), 101);
});

test("deliberate empty lane: cargo output that ran zero tests -> exit 1, enough tests -> exit 0", () => {
  const empty = run(["--min", "1", "--count-only"], CARGO_ZERO);
  assert.equal(empty.status, 1);
  assert.match(empty.stderr, /ran 0 tests, expected at least 1/);
  assert.equal(run(["--min", "8", "--count-only"], CARGO_TWO).status, 0);
});

test("wrapping a command that exits 0 and prints zero tests fails; no --min is a usage error", () => {
  const cmd = [process.execPath, "-e", `process.stdout.write(${JSON.stringify(CARGO_ZERO)})`];
  assert.equal(run(["--min", "1", "--", ...cmd]).status, 1);
  assert.equal(run(["--", ...cmd]).status, 2);
});
