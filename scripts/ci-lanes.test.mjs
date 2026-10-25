// SPDX-License-Identifier: Apache-2.0
// node --test scripts/ci-lanes.test.mjs
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, "..");
const script = join(here, "ci-lanes.mjs");
const fixtures = join(here, "fixtures", "ci-lanes");
const fixtureTree = join(fixtures, "tree");
const fixtureCi = readFileSync(join(fixtures, "ci.yml"), "utf8");
const repoCi = readFileSync(join(repo, ".github", "workflows", "ci.yml"), "utf8");

/** Run the CLI exactly as CI does; the workflow text goes through a temp file. */
function check(root, workflowText) {
  const dir = mkdtempSync(join(tmpdir(), "ci-lanes-"));
  try {
    const workflow = join(dir, "ci.yml");
    writeFileSync(workflow, workflowText);
    const r = spawnSync(process.execPath, [script, "--check", "--root", root, "--workflow", workflow], {
      encoding: "utf8",
    });
    const lines = `${r.stdout}${r.stderr}`.split(/\r?\n/).filter(Boolean);
    const named = (prefix) => lines.filter((l) => l.startsWith(prefix)).map((l) => l.slice(prefix.length));
    return {
      status: r.status,
      lines,
      uncovered: named("uncovered lane: "),
      stale: named("stale lane: "),
      unparseable: named("unparseable lane: "),
      zero: named("zero-test risk: "),
      denominator: lines.at(-1),
    };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

function replaced(text, from, to) {
  assert.ok(text.includes(from), `fixture edit: \`${from}\` not found`);
  return text.replace(from, to);
}

/** The repo's workflow with its `python:` job cut out: the shape ci.yml had before the lane. */
function withoutPythonJob(text) {
  const lines = text.split(/\r?\n/);
  const start = lines.findIndex((l) => /^ {2}python:\s*$/.test(l));
  assert.ok(start > 0, "ci.yml has no python job");
  let end = start + 1;
  while (end < lines.length && !/^ {2}\S/.test(lines[end]) && !/^\S/.test(lines[end])) end++;
  return [...lines.slice(0, start), ...lines.slice(end)].join("\n");
}

const PYTHON_LANES = [
  "tools/avatar unittest test_depth_map test_learned_depth",
  "tools/avatar3d unittest discover",
  "tools/rig unittest discover",
  "tools/voice unittest discover",
];

test("case 1: the repo's lanes without a python job -> exit 1, exactly the four Python suites uncovered", () => {
  const r = check(repo, withoutPythonJob(repoCi));
  assert.equal(r.status, 1, r.lines.join("\n"));
  assert.deepEqual([...r.uncovered].sort(), PYTHON_LANES);
  assert.deepEqual(r.stale, []);
  assert.deepEqual(r.unparseable, []);
});

test("fixture baseline: every lane covered -> exit 0 with its denominator", () => {
  const r = check(fixtureTree, fixtureCi);
  assert.equal(r.status, 0, r.lines.join("\n"));
  assert.equal(r.denominator, "8 lanes derived, 8 covered, 0 uncovered, 0 stale, 0 unparseable, 0 zero-test risks");
});

test("case 2: the stage-kit lint step deleted -> exit 1 naming packages/stage-kit lint", () => {
  const ci = replaced(fixtureCi, "      - name: stage-kit lint\n        run: pnpm -C packages/stage-kit lint\n", "");
  const r = check(fixtureTree, ci);
  assert.equal(r.status, 1);
  assert.deepEqual(r.uncovered, ["packages/stage-kit lint"]);
  assert.equal(r.denominator, "8 lanes derived, 7 covered, 1 uncovered, 0 stale, 0 unparseable, 0 zero-test risks");
});

test("case 3: apps/stage gains a lint script, ci.yml unchanged -> exit 1 naming apps/stage lint", () => {
  const root = mkdtempSync(join(tmpdir(), "ci-lanes-tree-"));
  try {
    cpSync(fixtureTree, root, { recursive: true });
    const pkgFile = join(root, "apps", "stage", "package.json");
    const pkg = JSON.parse(readFileSync(pkgFile, "utf8"));
    pkg.scripts.lint = "eslint .";
    writeFileSync(pkgFile, JSON.stringify(pkg));
    const r = check(root, fixtureCi);
    assert.equal(r.status, 1);
    assert.deepEqual(r.uncovered, ["apps/stage lint"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("case 4: a step for a package that does not exist -> exit 1 naming it stale", () => {
  const ci = replaced(fixtureCi, "          pnpm -C apps/stage build\n", "          pnpm -C apps/stage build\n          pnpm -C packages/gone test\n");
  const r = check(fixtureTree, ci);
  assert.equal(r.status, 1);
  assert.deepEqual(r.uncovered, []);
  assert.equal(r.stale.length, 1);
  assert.match(r.stale[0], /^packages\/gone test\b/);
});

const COV = "exec vitest run --coverage --coverage.provider=v8 --coverage.reporter=json-summary";
const BUILD = "          pnpm -C apps/stage build\n";

test("case 4a: a per-package coverage step for a real package is understood, not unparseable", () => {
  const ci = replaced(fixtureCi, BUILD, BUILD + "          pnpm -C apps/stage " + COV + "\n");
  const r = check(fixtureTree, ci);
  assert.equal(r.status, 0);
  assert.deepEqual(r.unparseable, []);
  assert.deepEqual(r.stale, []);
});

test("case 4a2: a coverage step for a package that does not exist -> stale", () => {
  const ci = replaced(fixtureCi, BUILD, BUILD + "          pnpm -C packages/gone " + COV + "\n");
  const r = check(fixtureTree, ci);
  assert.equal(r.status, 1);
  assert.match(r.stale[0], /^packages\/gone coverage/);
});

test("case 4b: a python step in a folder with no suite, or naming a missing module -> stale", () => {
  const ci = replaced(fixtureCi, "run: python -m unittest test_a test_b", "run: python -m unittest test_a test_b test_c");
  const r = check(fixtureTree, ci);
  assert.equal(r.status, 1);
  assert.deepEqual(r.uncovered, ["tools/flat unittest test_a test_b"]);
  assert.match(r.stale[0], /^tools\/flat unittest test_a test_b test_c\b/);
});

test("case 5: pnpm or python in a form the planner cannot parse -> exit 1 'unparseable lane', never a pass", () => {
  for (const line of ["cd packages/stage-kit && pnpm test", "pnpm -r run test", "python -m pytest", "python3 -m unittest -v"]) {
    const ci = replaced(fixtureCi, "        run: node scripts/ci-lanes.mjs --check\n", `        run: node scripts/ci-lanes.mjs --check\n      - name: odd\n        run: ${line}\n`);
    const r = check(fixtureTree, ci);
    assert.equal(r.status, 1, line);
    assert.deepEqual(r.unparseable, [line]);
    assert.match(r.denominator, / 1 unparseable, 0 zero-test risks$/);
  }
});

test("case 6: the repo as built -> exit 0, 22 lanes (18 web package-script + 4 Python; Rust not counted)", () => {
  const r = check(repo, repoCi);
  assert.equal(r.status, 0, r.lines.join("\n"));
  assert.equal(r.denominator, "22 lanes derived, 22 covered, 0 uncovered, 0 stale, 0 unparseable, 0 zero-test risks");
});

/** A scratch copy of the fixture tree, with one package.json script rewritten. */
function withScript(pkgDir, name, cmd, fn) {
  const dir = mkdtempSync(join(tmpdir(), "ci-lanes-tree-"));
  try {
    cpSync(fixtureTree, dir, { recursive: true });
    const file = join(dir, pkgDir, "package.json");
    const pkg = JSON.parse(readFileSync(file, "utf8"));
    pkg.scripts[name] = cmd;
    writeFileSync(file, JSON.stringify(pkg));
    return fn(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("case 7: a test script with passWithNoTests can pass on zero -> exit 1 'zero-test risk'", () => {
  for (const cmd of ["vitest run --passWithNoTests", "vitest run --pass-with-no-tests"]) {
    const r = withScript("apps/stage", "test", cmd, (root) => check(root, fixtureCi));
    assert.equal(r.status, 1, cmd);
    assert.deepEqual(r.zero, ["apps/stage test: passes with no tests"]);
  }
});

test("case 8: a node --test glob or a missing file in a package script -> exit 1 'zero-test risk'", () => {
  const glob = withScript("apps/stage", "lint", "node --test scripts/*.test.mjs", (root) => check(root, fixtureCi));
  assert.equal(glob.status, 1);
  assert.match(glob.zero[0], /node --test glob `scripts\/\*\.test\.mjs` passes on zero matches/);
  const missing = withScript("apps/stage", "lint", "eslint src && node --test scripts/gone.test.mjs", (root) => check(root, fixtureCi));
  assert.equal(missing.status, 1);
  assert.match(missing.zero[0], /target `scripts\/gone\.test\.mjs` does not exist/);
});

test("case 9: a workflow node --test step naming a missing file or a glob -> exit 1", () => {
  for (const line of ["node --test scripts/nope.test.mjs", "node --test scripts/*.test.mjs"]) {
    const anchor = "        run: node scripts/ci-lanes.mjs --check";
    const ci = replaced(fixtureCi, anchor, [anchor, "      - name: odd", `        run: ${line}`].join("\n"));
    const r = check(fixtureTree, ci);
    assert.equal(r.status, 1, line);
    assert.equal(r.zero.length, 1, line);
  }
});

test("case 10: the repo's own node --test steps and scripts name files that exist (no zero-test risk)", () => {
  assert.deepEqual(check(repo, repoCi).zero, []);
});
